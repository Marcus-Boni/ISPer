//! O motor Whisper: carrega o modelo uma vez, transcreve muitas vezes.
//!
//! `WhisperContext` (o modelo na memória) é caro de criar — no app final ele
//! vive o processo inteiro. Cada transcrição usa um `WhisperState` novo, que
//! é barato.
//!
//! Os parâmetros da decodificação NÃO moram aqui: vêm de um
//! [`crate::profile::DecodeConfig`], para que ditado, transcrição ao vivo e
//! passe final possam divergir sem `if` espalhado pelo motor.

use std::path::Path;
use std::time::Instant;

use whisper_rs::{
    DtwMode, DtwModelPreset, DtwParameters, WhisperContext, WhisperContextParameters,
};

use crate::profile::{DecodeConfig, normalize_lang};
use crate::{IsperError, Result};

/// Manda os logs do whisper.cpp e do ggml para o `tracing` em vez do stderr.
///
/// Sem isto, uma reunião de duas horas despeja milhares de linhas soltas no
/// console (o VAD escreve uma por região de fala). Com o hook, elas viram
/// eventos com alvo `whisper_rs::*`, que o app filtra como qualquer outro.
/// É idempotente, mas o `Once` deixa a intenção explícita.
pub(crate) fn install_whisper_logging() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(whisper_rs::install_logging_hooks);
}

pub struct WhisperEngine {
    ctx: WhisperContext,
    /// Serializa inferências: ditado e blocos de reunião compartilham o
    /// mesmo modelo na GPU sem disputa.
    infer_lock: std::sync::Mutex<()>,
    /// O modelo foi carregado com alinhamento DTW (timestamps por token bem
    /// mais precisos, ao custo de memória — ver [`EngineOptions`]).
    dtw: bool,
    /// Nome do arquivo do modelo, para os relatórios.
    model_name: String,
}

/// Uma palavra reconhecida, com o intervalo em que foi falada.
///
/// É o insumo da atribuição de falante: um segmento do Whisper pode conter
/// duas pessoas, uma palavra não.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub start_secs: f32,
    pub end_secs: f32,
    /// Probabilidade média dos tokens que formam a palavra.
    pub prob: f32,
}

#[derive(Debug, Clone)]
pub struct TranscriptSegment {
    pub start_secs: f32,
    pub end_secs: f32,
    pub text: String,
    /// Probabilidade, dada pelo próprio modelo, de o trecho NÃO ser fala
    /// (0 = fala certa, 1 = silêncio/ruído). Insumo do filtro de alucinações.
    pub no_speech_prob: f32,
    /// Log-probabilidade média dos tokens do segmento — quanto o modelo
    /// "confiou" no que escreveu. Vai para as métricas e decide se o texto
    /// serve como contexto do bloco seguinte.
    pub avg_logprob: f32,
    /// Palavras com timestamp. Vazio quando o perfil não pede
    /// `token_timestamps` (ao vivo e ditado não pedem).
    pub words: Vec<Word>,
}

impl TranscriptSegment {
    /// Duração, nunca negativa.
    pub fn secs(&self) -> f32 {
        (self.end_secs - self.start_secs).max(0.0)
    }
}

#[derive(Debug, Clone)]
pub struct Transcript {
    /// Texto completo, segmentos emendados.
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
    /// Quanto tempo a inferência levou (para medir o fator de tempo real).
    pub infer_secs: f32,
    /// Duração do áudio transcrito.
    pub audio_secs: f32,
    /// Quantos segmentos o filtro de alucinações descartou.
    pub dropped_segments: usize,
}

/// O que uma transcrição precisa saber além do áudio.
#[derive(Debug, Clone, Copy)]
pub struct TranscribeRequest<'a> {
    /// "pt", "en"… ou "auto" (normalizado por [`normalize_lang`]).
    pub lang: &'a str,
    pub decode: &'a DecodeConfig,
    /// `initial_prompt`: glossário e/ou o fim do bloco anterior.
    pub prompt: Option<&'a str>,
}

/// Como carregar o modelo.
#[derive(Debug, Clone, Default)]
pub struct EngineOptions {
    /// Liga o alinhamento DTW dos timestamps por token. Melhora bastante a
    /// precisão das fronteiras de palavra (que é o que decide o falante),
    /// ao custo de guardar a atenção cruzada — só vale a pena quando o
    /// passe final vai usar `token_timestamps`.
    pub dtw: bool,
}

impl WhisperEngine {
    /// Carrega o modelo ggml. Operação cara (~1 s) — faça uma vez e reuse.
    pub fn new(model_path: &Path) -> Result<Self> {
        Self::new_with(model_path, &EngineOptions::default())
    }

    /// Como [`Self::new`], escolhendo o que o contexto precisa suportar.
    pub fn new_with(model_path: &Path, opts: &EngineOptions) -> Result<Self> {
        install_whisper_logging();
        if !model_path.exists() {
            return Err(IsperError::Whisper(format!(
                "modelo não encontrado em {} — baixe de https://huggingface.co/ggerganov/whisper.cpp",
                model_path.display()
            )));
        }
        let path_str = model_path
            .to_str()
            .ok_or_else(|| IsperError::Whisper("caminho de modelo inválido".into()))?;
        let model_name = model_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let mut params = WhisperContextParameters::default();
        // O preset de DTW precisa casar com o modelo carregado; um preset
        // errado faz o whisper.cpp recusar o contexto. Sem preset conhecido,
        // seguimos sem DTW (os timestamps por token continuam existindo, só
        // menos precisos).
        let preset = opts.dtw.then(|| dtw_preset(&model_name)).flatten();
        let dtw = preset.is_some();
        if let Some(model_preset) = preset {
            params.dtw_parameters(DtwParameters {
                mode: DtwMode::ModelPreset { model_preset },
                ..Default::default()
            });
        } else if opts.dtw {
            tracing::warn!(
                model = %model_name,
                "sem preset de DTW para este modelo — timestamps por token seguem na heurística padrão"
            );
        }

        let ctx = WhisperContext::new_with_params(path_str, params)
            .map_err(|e| IsperError::Whisper(e.to_string()))?;
        Ok(Self {
            ctx,
            infer_lock: std::sync::Mutex::new(()),
            dtw,
            model_name,
        })
    }

    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// O contexto foi carregado com DTW (timestamps por token mais precisos).
    pub fn has_dtw(&self) -> bool {
        self.dtw
    }

    /// Transcreve áudio já em 16 kHz mono f32 (use `RawAudio::into_whisper_input`).
    /// `lang` é o código do idioma ("pt", "en") ou "auto" para detecção.
    /// `initial_prompt` alimenta o dicionário pessoal: o Whisper tende a
    /// grafar corretamente termos que "acabou de ver" (nomes próprios, siglas).
    ///
    /// Usa o perfil ao vivo — é o caminho do ditado e dos blocos da reunião.
    pub fn transcribe(
        &self,
        samples_16k: &[f32],
        lang: &str,
        initial_prompt: Option<&str>,
    ) -> Result<Transcript> {
        let decode = DecodeConfig::live();
        self.transcribe_with(
            samples_16k,
            TranscribeRequest {
                lang,
                decode: &decode,
                prompt: initial_prompt,
            },
        )
    }

    /// Transcreve com um perfil de decodificação escolhido pelo chamador.
    pub fn transcribe_with(
        &self,
        samples_16k: &[f32],
        req: TranscribeRequest<'_>,
    ) -> Result<Transcript> {
        if samples_16k.is_empty() {
            return Err(IsperError::Whisper("áudio vazio".into()));
        }
        // Uma inferência por vez — ditado e reunião dividem a GPU em paz.
        // Envenenado (pânico numa inferência anterior)? O motor continua servindo.
        let _guard = self
            .infer_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| IsperError::Whisper(e.to_string()))?;

        let lang = normalize_lang(req.lang);
        let params = req.decode.to_full_params(&lang, req.prompt, &[]);

        let started = Instant::now();
        state
            .full(params, samples_16k)
            .map_err(|e| IsperError::Whisper(e.to_string()))?;
        let infer_secs = started.elapsed().as_secs_f32();

        let want_words = req.decode.token_timestamps;
        let mut segments = Vec::new();
        let n = state.full_n_segments();
        for i in 0..n {
            let Some(seg) = state.get_segment(i) else {
                break;
            };
            let text = seg
                .to_str_lossy()
                .map_err(|e| IsperError::Whisper(e.to_string()))?
                .into_owned();
            let (words, avg_logprob) = self.segment_tokens(&seg, want_words);
            segments.push(TranscriptSegment {
                // Timestamps do whisper.cpp vêm em centissegundos.
                start_secs: seg.start_timestamp() as f32 / 100.0,
                end_secs: seg.end_timestamp() as f32 / 100.0,
                text: text.trim().to_string(),
                no_speech_prob: seg.no_speech_probability(),
                avg_logprob,
                words,
            });
        }
        // Filtro de alucinações (texto de legenda inventado, loops, símbolos,
        // trechos que o modelo mesmo diz não serem fala).
        let before = segments.len();
        let segments = crate::text::filter_hallucinations(segments);
        let dropped_segments = before - segments.len();
        if dropped_segments > 0 {
            tracing::debug!(
                removed = dropped_segments,
                "segmentos descartados pelo filtro de alucinações"
            );
        }

        let text = segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();
        let audio_secs = samples_16k.len() as f32 / crate::WHISPER_SAMPLE_RATE as f32;

        Ok(Transcript {
            text,
            segments,
            infer_secs,
            audio_secs,
            dropped_segments,
        })
    }

    /// Lê os tokens de um segmento: agrupa em palavras (quando pedido) e
    /// devolve a log-probabilidade média. Tokens especiais (timestamps, `<|…|>`)
    /// não entram em nenhuma das duas contas.
    fn segment_tokens(
        &self,
        seg: &whisper_rs::WhisperSegment<'_>,
        want_words: bool,
    ) -> (Vec<Word>, f32) {
        let eot = self.ctx.token_eot();
        let mut words: Vec<Word> = Vec::new();
        let mut logprob_sum = 0.0f64;
        let mut counted = 0usize;
        // Acumulador da palavra em construção: soma das probabilidades e
        // quantos tokens entraram (a média sai no fecho).
        let mut prob_sum = 0.0f32;
        let mut prob_n = 0usize;

        for t in 0..seg.n_tokens() {
            let Some(tok) = seg.get_token(t) else { break };
            if tok.token_id() >= eot {
                continue; // token especial: não é texto
            }
            let data = tok.token_data();
            logprob_sum += data.plog as f64;
            counted += 1;
            if !want_words {
                continue;
            }
            let Ok(piece) = tok.to_str_lossy() else {
                continue;
            };
            if piece.is_empty() {
                continue;
            }
            // O tokenizador do Whisper marca o início de palavra com o espaço
            // à esquerda; o que não começa com espaço é continuação.
            let starts_word = piece.starts_with(' ') || words.is_empty();
            let t0 = data.t0 as f32 / 100.0;
            let t1 = data.t1 as f32 / 100.0;
            match words.last_mut() {
                Some(w) if !starts_word => {
                    w.text.push_str(&piece);
                    w.end_secs = w.end_secs.max(t1);
                    prob_sum += data.p;
                    prob_n += 1;
                    w.prob = prob_sum / prob_n as f32;
                }
                _ => {
                    prob_sum = data.p;
                    prob_n = 1;
                    words.push(Word {
                        text: piece.trim_start().to_string(),
                        start_secs: t0,
                        end_secs: t1.max(t0),
                        prob: data.p,
                    });
                }
            }
        }
        words.retain(|w| !w.text.trim().is_empty());
        let avg_logprob = if counted == 0 {
            0.0
        } else {
            (logprob_sum / counted as f64) as f32
        };
        (words, avg_logprob)
    }
}

/// Preset de DTW do whisper.cpp para um arquivo de modelo do catálogo.
/// A ordem importa: "large-v3-turbo" tem que ser testado antes de "large-v3".
fn dtw_preset(file: &str) -> Option<DtwModelPreset> {
    let f = file.to_ascii_lowercase();
    let has = |needle: &str| f.contains(needle);
    if has("large-v3-turbo") {
        Some(DtwModelPreset::LargeV3Turbo)
    } else if has("large-v3") {
        Some(DtwModelPreset::LargeV3)
    } else if has("large-v2") {
        Some(DtwModelPreset::LargeV2)
    } else if has("large-v1") {
        Some(DtwModelPreset::LargeV1)
    } else if has("medium.en") {
        Some(DtwModelPreset::MediumEn)
    } else if has("medium") {
        Some(DtwModelPreset::Medium)
    } else if has("small.en") {
        Some(DtwModelPreset::SmallEn)
    } else if has("small") {
        Some(DtwModelPreset::Small)
    } else if has("base.en") {
        Some(DtwModelPreset::BaseEn)
    } else if has("base") {
        Some(DtwModelPreset::Base)
    } else if has("tiny.en") {
        Some(DtwModelPreset::TinyEn)
    } else if has("tiny") {
        Some(DtwModelPreset::Tiny)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_de_dtw_casa_o_arquivo_do_catalogo() {
        // O turbo precisa ganhar do large-v3 genérico.
        assert!(matches!(
            dtw_preset("ggml-large-v3-turbo-q5_0.bin"),
            Some(DtwModelPreset::LargeV3Turbo)
        ));
        assert!(matches!(
            dtw_preset("ggml-large-v3-q5_0.bin"),
            Some(DtwModelPreset::LargeV3)
        ));
        assert!(matches!(
            dtw_preset("ggml-small.bin"),
            Some(DtwModelPreset::Small)
        ));
        assert!(matches!(
            dtw_preset("ggml-medium-q5_0.bin"),
            Some(DtwModelPreset::Medium)
        ));
        assert!(dtw_preset("modelo-caseiro.bin").is_none());
    }
}
