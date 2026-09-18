//! Detecção de fala (VAD) e o planejamento das janelas que vão ao Whisper.
//!
//! O problema que este módulo resolve: até aqui a reunião era fatiada em
//! blocos de ~20 s cortados no ponto de MENOR ENERGIA do último 1,5 s. Menor
//! energia não é silêncio — numa fala contínua o mínimo cai no meio de uma
//! palavra, e "manual" vira "ma" + "nual", que o modelo lê como "anual".
//!
//! Aqui o corte só acontece onde o Silero (o VAD que o whisper.cpp já embarca,
//! ~0,9 MB) diz que ninguém está falando. Duas partes:
//!
//! - [`Vad`] — o modelo, que devolve as regiões de fala do áudio;
//! - [`plan_windows`] — função pura que agrupa regiões em janelas de ASR.
//!   Sem modelo, sem E/S: dá para testar todo o comportamento de fronteira.
//!
//! Atenção a uma armadilha: `FullParams::enable_vad` NÃO funciona por este
//! caminho. O whisper.cpp só aplica o VAD dentro de `whisper_full`, e o
//! whisper-rs chama `whisper_full_with_state` — o parâmetro é aceito e
//! silenciosamente ignorado. Por isso rodamos o VAD nós mesmos, o que também
//! nos dá as regiões como dado (métricas, diarização, alinhamento).

use std::path::Path;

use serde::{Deserialize, Serialize};
use whisper_rs::{WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

use crate::{IsperError, Result};

/// Um trecho em que o VAD ouviu fala, em segundos do áudio analisado.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeechRegion {
    pub start_secs: f32,
    pub end_secs: f32,
}

impl SpeechRegion {
    pub fn secs(&self) -> f32 {
        (self.end_secs - self.start_secs).max(0.0)
    }
}

/// Parâmetros do Silero. Os defaults são os do whisper.cpp
/// (`whisper_vad_default_params`), com uma exceção anotada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VadOptions {
    /// Acima disto o quadro é fala. Default do whisper.cpp: 0,5.
    pub threshold: f32,
    /// Fala mais curta que isto é descartada (ruído). Default: 250 ms.
    pub min_speech_ms: i32,
    /// Silêncio precisa durar isto para encerrar uma fala. Default do
    /// whisper.cpp: 100 ms — curto demais para reunião, onde a pausa entre
    /// duas palavras da mesma frase passa fácil de 100 ms e viraria uma
    /// fronteira. Usamos 300 ms: separa turnos sem picotar frases.
    pub min_silence_ms: i32,
    /// Sobra de áudio antes e depois de cada região, para não comer o começo
    /// nem o fim da palavra. Default do whisper.cpp: 30 ms.
    pub speech_pad_ms: i32,
}

impl Default for VadOptions {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            min_speech_ms: 250,
            min_silence_ms: 300,
            speech_pad_ms: 30,
        }
    }
}

/// O Silero carregado. Reusável: carregue uma vez por passe final.
pub struct Vad {
    ctx: WhisperVadContext,
}

impl Vad {
    /// Carrega o modelo (`ggml-silero-v5.1.2.bin`, ~0,9 MB).
    pub fn new(model_path: &Path) -> Result<Self> {
        crate::engine::install_whisper_logging();
        if !model_path.exists() {
            return Err(IsperError::Whisper(format!(
                "modelo de VAD não encontrado em {}",
                model_path.display()
            )));
        }
        let path = model_path
            .to_str()
            .ok_or_else(|| IsperError::Whisper("caminho de VAD inválido".into()))?;
        // Na CPU de propósito: o Silero é minúsculo e a GPU está ocupada com
        // o Whisper — disputar contexto CUDA por 0,9 MB não paga.
        let mut params = WhisperVadContextParams::new();
        params.set_use_gpu(false);
        params.set_n_threads(
            std::thread::available_parallelism()
                .map(|n| n.get() as i32)
                .unwrap_or(4)
                .clamp(1, 4),
        );
        let ctx = WhisperVadContext::new(path, params)
            .map_err(|e| IsperError::Whisper(format!("VAD: {e}")))?;
        Ok(Self { ctx })
    }

    /// Regiões de fala de um áudio 16 kHz mono f32.
    pub fn regions(&mut self, samples_16k: &[f32], opts: &VadOptions) -> Result<Vec<SpeechRegion>> {
        if samples_16k.is_empty() {
            return Ok(Vec::new());
        }
        let mut p = WhisperVadParams::new();
        p.set_threshold(opts.threshold);
        p.set_min_speech_duration(opts.min_speech_ms);
        p.set_min_silence_duration(opts.min_silence_ms);
        p.set_speech_pad(opts.speech_pad_ms);
        // Não deixamos o VAD partir fala longa: quem decide o tamanho da
        // janela é [`plan_windows`], que conhece as regras do Whisper.
        p.set_max_speech_duration(f32::MAX);
        p.set_samples_overlap(0.0);

        let segments = self
            .ctx
            .segments_from_samples(p, samples_16k)
            .map_err(|e| IsperError::Whisper(format!("VAD: {e}")))?;

        let total = samples_16k.len() as f32 / crate::WHISPER_SAMPLE_RATE as f32;
        let mut out = Vec::with_capacity(segments.num_segments().max(0) as usize);
        for i in 0..segments.num_segments() {
            let Some(seg) = segments.get_segment(i) else {
                break;
            };
            // O whisper.cpp devolve os tempos do VAD em centissegundos.
            let start = (seg.start / 100.0).clamp(0.0, total);
            let end = (seg.end / 100.0).clamp(0.0, total);
            if end > start {
                out.push(SpeechRegion {
                    start_secs: start,
                    end_secs: end,
                });
            }
        }
        Ok(out)
    }
}

/// Regras de montagem das janelas que vão ao Whisper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowOptions {
    /// Teto de uma janela.
    ///
    /// 30 s não é arbitrário: o Whisper monta um espectrograma de 30 s para
    /// CADA chamada, independentemente de quanto áudio foi passado. Uma
    /// janela de 3 s custa o mesmo encoder que uma de 30 s — encher a janela
    /// é o que faz o passe final caber no orçamento de tempo.
    ///
    /// Medido no corpus de regressão (190 s, 3 falantes): 11 janelas de
    /// ~18 s → 8,8 s de ASR e WER 5,99%; 37 janelas de ~2,7 s → 20,9 s de ASR
    /// e WER 5,28%. Dois terços do tempo por dois erros a menos em 284
    /// palavras — diferença dentro do ruído de uma fixture só, custo não.
    pub max_window_secs: f32,
    /// Silêncio a partir do qual vale a pena fechar a janela.
    ///
    /// Pausas menores ficam DENTRO da janela — e é isso que se quer: o
    /// Whisper processa 30 s por vez e carrega o próprio contexto entre eles,
    /// então poucas janelas grandes transcrevem melhor que muitas pequenas.
    /// O VAD não está aqui para picotar; está para garantir que, quando um
    /// corte acontecer, ele caia onde ninguém está falando.
    ///
    /// 2 s é também a folga de silêncio que o próprio whisper.cpp mantém
    /// entre trechos quando roda o VAD internamente.
    pub split_gap_secs: f32,
    /// Silêncio a partir do qual o contexto da janela anterior deixa de
    /// valer: depois de uma pausa longa, o assunto mudou.
    pub context_gap_secs: f32,
    /// Sobra de áudio nas pontas da janela (além do padding do VAD).
    pub pad_secs: f32,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            max_window_secs: 30.0,
            split_gap_secs: 2.0,
            context_gap_secs: 8.0,
            pad_secs: 0.2,
        }
    }
}

/// Uma janela de áudio para uma chamada do Whisper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AsrWindow {
    /// Início no relógio do áudio (já com padding).
    pub start_secs: f32,
    pub end_secs: f32,
    /// Fala útil dentro da janela (sem o padding) — para as métricas.
    pub speech_secs: f32,
    /// A janela continua a anterior sem pausa longa: o texto anterior serve
    /// de contexto. Falso depois de um silêncio, quando herdar contexto só
    /// arrastaria o assunto velho.
    pub continues_previous: bool,
}

impl AsrWindow {
    pub fn secs(&self) -> f32 {
        (self.end_secs - self.start_secs).max(0.0)
    }
}

/// Agrupa regiões de fala em janelas de ASR. Função pura — é onde mora a
/// regra "só corte onde ninguém está falando".
///
/// `total_secs` é a duração do áudio, para não pedir padding além do fim.
pub fn plan_windows(
    regions: &[SpeechRegion],
    total_secs: f32,
    opts: &WindowOptions,
) -> Vec<AsrWindow> {
    // Estado de uma janela em construção, antes do padding.
    struct Open {
        start: f32,
        end: f32,
        speech: f32,
        continues: bool,
    }
    let mut open: Option<Open> = None;
    let mut out: Vec<AsrWindow> = Vec::new();

    let close = |o: Open, out: &mut Vec<AsrWindow>| {
        let start = (o.start - opts.pad_secs).max(0.0);
        let end = (o.end + opts.pad_secs).min(total_secs.max(o.end));
        out.push(AsrWindow {
            start_secs: start,
            end_secs: end,
            speech_secs: o.speech,
            continues_previous: o.continues,
        });
    };

    for r in regions.iter().filter(|r| r.secs() > 0.0) {
        match open.take() {
            None => {
                open = Some(Open {
                    start: r.start_secs,
                    end: r.end_secs,
                    speech: r.secs(),
                    continues: false,
                });
            }
            Some(mut o) => {
                let gap = (r.start_secs - o.end).max(0.0);
                let would_be = r.end_secs - o.start;
                if gap >= opts.split_gap_secs || would_be > opts.max_window_secs {
                    let continues = gap < opts.context_gap_secs;
                    close(o, &mut out);
                    open = Some(Open {
                        start: r.start_secs,
                        end: r.end_secs,
                        speech: r.secs(),
                        continues,
                    });
                } else {
                    o.end = r.end_secs;
                    o.speech += r.secs();
                    open = Some(o);
                }
            }
        }
    }
    if let Some(o) = open {
        close(o, &mut out);
    }

    // Uma única região de fala pode, sozinha, passar do teto (alguém falou
    // dez minutos sem uma pausa de 0,6 s). Aí não há silêncio onde cortar:
    // partimos no teto e marcamos que a próxima janela continua a anterior,
    // para o contexto atravessar o corte.
    split_oversized(out, opts)
}

/// Quebra janelas acima do teto em pedaços iguais, com sobreposição de meio
/// segundo — a palavra partida aparece inteira em um dos dois lados.
fn split_oversized(windows: Vec<AsrWindow>, opts: &WindowOptions) -> Vec<AsrWindow> {
    const OVERLAP: f32 = 0.5;
    let mut out = Vec::with_capacity(windows.len());
    for w in windows {
        if w.secs() <= opts.max_window_secs + 0.001 {
            out.push(w);
            continue;
        }
        let parts = (w.secs() / opts.max_window_secs).ceil().max(1.0);
        let step = w.secs() / parts;
        let n = parts as usize;
        for i in 0..n {
            let start = w.start_secs + step * i as f32;
            let end = (start + step + OVERLAP).min(w.end_secs);
            out.push(AsrWindow {
                start_secs: start,
                end_secs: end,
                // A fala é distribuída proporcionalmente: dentro de uma
                // região contínua praticamente tudo é fala mesmo.
                speech_secs: w.speech_secs / parts,
                continues_previous: if i == 0 { w.continues_previous } else { true },
            });
        }
    }
    out
}

/// Recorta as amostras de uma janela. Devolve fatia vazia se a janela cair
/// fora do áudio.
pub fn slice<'a>(samples_16k: &'a [f32], window: &AsrWindow) -> &'a [f32] {
    let rate = crate::WHISPER_SAMPLE_RATE as f32;
    let a = ((window.start_secs * rate) as usize).min(samples_16k.len());
    let b = ((window.end_secs * rate).ceil() as usize).min(samples_16k.len());
    if b > a { &samples_16k[a..b] } else { &[] }
}

/// Soma da fala detectada, em segundos.
pub fn speech_secs(regions: &[SpeechRegion]) -> f32 {
    regions.iter().map(SpeechRegion::secs).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(a: f32, b: f32) -> SpeechRegion {
        SpeechRegion {
            start_secs: a,
            end_secs: b,
        }
    }

    #[test]
    fn pausas_curtas_ficam_dentro_da_janela() {
        // Respirar (0,2 s) e até pensar por um segundo não fecham janela:
        // menos chamadas ao modelo, e ele carrega o próprio contexto dentro
        // de uma chamada.
        let regions = [r(0.0, 5.0), r(5.2, 9.0), r(10.0, 12.0)];
        let w = plan_windows(&regions, 12.0, &WindowOptions::default());
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].start_secs <= 0.0);
        assert!((w[0].end_secs - 12.0).abs() < 0.3);
        assert!(!w[0].continues_previous);
    }

    #[test]
    fn silencio_longo_fecha_a_janela_e_depois_o_contexto() {
        // 3 s fecha a janela mas ainda é o mesmo assunto; 15 s não é.
        let regions = [r(0.0, 5.0), r(8.0, 11.0), r(26.0, 29.0)];
        let w = plan_windows(&regions, 29.0, &WindowOptions::default());
        assert_eq!(w.len(), 3, "{w:?}");
        assert!(w[1].continues_previous, "3 s de pausa mantém o contexto");
        assert!(!w[2].continues_previous, "15 s de silêncio zera o contexto");
    }

    #[test]
    fn janela_nunca_carrega_silencio_demais_por_dentro() {
        // O silêncio interno é limitado por `split_gap_secs`: mandar minutos
        // de nada ao Whisper é onde ele inventa legenda.
        let regions = [r(0.0, 5.0), r(6.5, 9.0), r(30.0, 33.0)];
        let opts = WindowOptions::default();
        let w = plan_windows(&regions, 33.0, &opts);
        for x in &w {
            let silencio = x.secs() - x.speech_secs;
            assert!(
                silencio <= opts.split_gap_secs + 2.0 * opts.pad_secs + 0.01,
                "janela com {silencio:.1}s de silêncio: {x:?}"
            );
        }
    }

    #[test]
    fn janela_nunca_passa_do_teto() {
        let opts = WindowOptions {
            max_window_secs: 30.0,
            ..Default::default()
        };
        // Uma fala contínua de 100 s: não há silêncio onde cortar.
        let w = plan_windows(&[r(0.0, 100.0)], 100.0, &opts);
        assert!(w.len() >= 4, "{w:?}");
        for x in &w {
            assert!(
                x.secs() <= opts.max_window_secs + 0.6,
                "janela de {:.1}s passou do teto",
                x.secs()
            );
        }
        // Todas menos a primeira continuam a anterior — o contexto atravessa.
        assert!(!w[0].continues_previous);
        assert!(w[1..].iter().all(|x| x.continues_previous));
        // E cobrem o áudio inteiro.
        assert!(w.last().expect("janelas").end_secs >= 99.9);
    }

    #[test]
    fn regioes_partidas_cobrem_toda_a_fala() {
        let regions = [r(1.0, 3.0), r(10.0, 14.0), r(30.0, 31.0)];
        let w = plan_windows(&regions, 40.0, &WindowOptions::default());
        let coberto: f32 = w.iter().map(|x| x.speech_secs).sum();
        assert!(
            (coberto - speech_secs(&regions)).abs() < 0.01,
            "a fala contabilizada tem que bater: {coberto}"
        );
    }

    #[test]
    fn sem_fala_nao_ha_janela() {
        assert!(plan_windows(&[], 60.0, &WindowOptions::default()).is_empty());
        // Região degenerada (fim antes do início) é ignorada.
        assert!(plan_windows(&[r(5.0, 5.0)], 60.0, &WindowOptions::default()).is_empty());
    }

    #[test]
    fn padding_nao_sai_do_audio() {
        let w = plan_windows(&[r(0.05, 9.95)], 10.0, &WindowOptions::default());
        assert_eq!(w.len(), 1);
        assert!(w[0].start_secs >= 0.0);
        assert!(w[0].end_secs <= 10.0 + f32::EPSILON, "{:?}", w[0]);
    }

    #[test]
    fn slice_respeita_os_limites_do_buffer() {
        let samples = vec![0.0f32; 16_000 * 3]; // 3 s
        let w = AsrWindow {
            start_secs: 2.0,
            end_secs: 10.0,
            speech_secs: 1.0,
            continues_previous: false,
        };
        assert_eq!(slice(&samples, &w).len(), 16_000);
        let fora = AsrWindow {
            start_secs: 30.0,
            end_secs: 40.0,
            speech_secs: 0.0,
            continues_previous: false,
        };
        assert!(slice(&samples, &fora).is_empty());
    }
}
