//! O passe FINAL: do áudio inteiro da reunião à transcrição oficial.
//!
//! A transcrição ao vivo ([`crate::meeting`]) existe para dar retorno durante
//! a reunião e paga o preço disso: blocos pequenos, decodificação rápida, sem
//! contexto e sem diarização. O passe final roda DEPOIS, com o áudio inteiro
//! na mão, e por isso pode fazer o que o ao vivo não pode:
//!
//! ```text
//! áudio original
//!   → VAD (só fala vai ao modelo, e o corte cai no silêncio)
//!   → janelas grandes
//!   → ASR (beam search, contexto da janela anterior, timestamps por palavra)
//!   → diarização sobre o MESMO áudio contínuo
//!   → falante por palavra
//!   → reconstrução das falas
//!   → normalização pelo glossário (preservando o bruto)
//! ```
//!
//! A configuração de cada etapa está em [`FinalConfig`], e o que aconteceu em
//! [`crate::metrics::PipelineReport`] — a mesma estrutura serve ao benchmark,
//! que é como se compara uma mudança com a anterior.
//!
//! Uma decisão de arquitetura: a diarização NÃO é chamada daqui. O
//! `isper-core` não depende do `sherpa-rs` (30 MB de C++), então quem chama
//! passa um [`Diarizer`]. O app e a CLI ligam os dois em cinco linhas.

use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::align::{self, AlignOptions, SpeakerTurn, TaggedWord, Utterance};
use crate::context::{MeetingContext, window_prompt};
use crate::engine::{TranscribeRequest, TranscriptSegment, WhisperEngine, Word};
use crate::metrics::{
    AsrStats, DiarizeStats, PipelineReport, RunParams, StageTimings, VadStats, device,
};
use crate::profile::{DecodeConfig, TranscriptionProfile};
use crate::vad::{self, AsrWindow, SpeechRegion, Vad, VadOptions, WindowOptions};
use crate::{Result, WHISPER_SAMPLE_RATE};

/// Como o áudio é fatiado antes de ir ao modelo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Segmentation {
    /// O comportamento até a v0.15, mantido para o benchmark ter um "antes"
    /// reproduzível: blocos de tamanho fixo, cortados no ponto de MENOR
    /// energia do fim do bloco — silêncio ou não.
    LegacyChunks {
        chunk_secs: f32,
        /// Blocos abaixo deste RMS nem iam ao Whisper.
        silence_rms: f32,
    },
    /// Corte guiado pelo Silero: só onde ninguém está falando.
    Vad {
        model: PathBuf,
        opts: VadOptions,
        windows: WindowOptions,
    },
}

/// Tudo o que define uma rodada do passe final.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FinalConfig {
    /// Nome livre da rodada, para o relatório ("baseline", "final"…).
    pub name: String,
    pub profile: TranscriptionProfile,
    pub lang: String,
    pub decode: DecodeConfig,
    pub segmentation: Segmentation,
    pub align: AlignOptions,
    /// Quantos caracteres do texto anterior entram como contexto da próxima
    /// janela. 0 desliga o contexto.
    pub carry_chars: usize,
    /// Log-probabilidade média mínima para um texto servir de contexto.
    /// É a trava contra "um erro no começo contamina a reunião inteira":
    /// texto em que o próprio modelo não confiou não é passado adiante.
    pub min_carry_logprob: f32,
    /// Aplica a correção por semelhança do glossário sobre o texto final
    /// (o bruto continua disponível em [`FinalTranscript::raw_text`]).
    pub apply_glossary: bool,
}

impl FinalConfig {
    /// O "antes": reproduz offline o que o ISPer fazia durante a reunião.
    pub fn baseline() -> Self {
        Self {
            name: "baseline".into(),
            profile: TranscriptionProfile::Live,
            lang: "pt".into(),
            // Greedy com best_of 1 era literalmente o que estava no motor.
            decode: DecodeConfig {
                best_of: 1,
                ..DecodeConfig::live()
            },
            segmentation: Segmentation::LegacyChunks {
                chunk_secs: 20.0,
                silence_rms: 0.0035,
            },
            align: AlignOptions::default(),
            carry_chars: 0,
            min_carry_logprob: f32::MIN,
            apply_glossary: true,
        }
    }

    /// O "depois": VAD, beam search, contexto e palavras com timestamp.
    pub fn meeting_final(vad_model: PathBuf) -> Self {
        Self {
            name: "final".into(),
            profile: TranscriptionProfile::MeetingFinal,
            lang: "pt".into(),
            decode: DecodeConfig::meeting_final(),
            segmentation: Segmentation::Vad {
                model: vad_model,
                opts: VadOptions::default(),
                windows: WindowOptions::default(),
            },
            align: AlignOptions::default(),
            carry_chars: 240,
            // -1,0 é o mesmo limiar que o whisper.cpp usa para decidir que
            // uma decodificação foi ruim e precisa de outra tentativa.
            min_carry_logprob: -1.0,
            apply_glossary: true,
        }
    }
}

/// Quem sabe separar falantes. Implementado fora do core (ver `isper-diarize`).
pub trait Diarizer {
    /// Turnos de fala do áudio 16 kHz mono, no relógio do próprio áudio.
    fn diarize(&self, samples_16k: &[f32]) -> std::result::Result<DiarizerOutput, String>;
}

/// O que um [`Diarizer`] devolve.
#[derive(Debug, Clone, Default)]
pub struct DiarizerOutput {
    pub turns: Vec<SpeakerTurn>,
    pub stats: DiarizeStats,
    /// Vazio = resultado confiável. Com avisos, o pipeline mantém os turnos
    /// mas registra tudo no relatório, e quem chama decide se publica.
    pub warnings: Vec<String>,
}

/// O resultado do passe final.
#[derive(Debug, Clone)]
pub struct FinalTranscript {
    /// Falas reconstruídas, com falante e horário.
    pub utterances: Vec<Utterance>,
    /// Palavras com falante — a camada de auditoria mais fina.
    pub words: Vec<TaggedWord>,
    /// Segmentos do ASR, em tempo absoluto do áudio, ANTES de qualquer
    /// normalização. É o que garante que nada do pós-processamento seja
    /// irreversível.
    pub raw_segments: Vec<TranscriptSegment>,
    /// Texto corrido do ASR bruto.
    pub raw_text: String,
    /// Texto corrido depois do glossário (igual ao bruto quando desligado).
    pub normalized_text: String,
    pub report: PipelineReport,
}

/// Chamado a cada janela processada: `(feita, total)`.
pub type Progress<'a> = &'a (dyn Fn(usize, usize) + Send + Sync);

/// Roda o passe final sobre um áudio 16 kHz mono f32.
pub fn run(
    engine: &WhisperEngine,
    samples_16k: &[f32],
    cfg: &FinalConfig,
    ctx: &MeetingContext,
    diarizer: Option<&dyn Diarizer>,
    progress: Option<Progress<'_>>,
) -> Result<FinalTranscript> {
    let started = Instant::now();
    let audio_secs = samples_16k.len() as f32 / WHISPER_SAMPLE_RATE as f32;
    let mut timings = StageTimings::default();

    // ---------------------------------------------------------- 1. fronteiras
    let t = Instant::now();
    let (windows, regions) = plan(samples_16k, &cfg.segmentation)?;
    timings.vad_secs = t.elapsed().as_secs_f32();
    // Sem VAD não há "regiões de fala": o que foi ao modelo é o que as
    // janelas cobrem. Reportar o áudio inteiro como silêncio descartado seria
    // mentira nos dois sentidos.
    let speech = if regions.is_empty() {
        windows.iter().map(AsrWindow::secs).sum()
    } else {
        vad::speech_secs(&regions)
    };
    let vad_stats = VadStats {
        regions: regions.len(),
        speech_secs: speech,
        discarded_silence_secs: (audio_secs - speech).max(0.0),
        windows: windows.len(),
        median_window_secs: median(&windows.iter().map(AsrWindow::secs).collect::<Vec<_>>()),
    };
    tracing::info!(
        audio_secs,
        regions = regions.len(),
        speech_secs = speech,
        windows = windows.len(),
        secs = timings.vad_secs,
        "fronteiras definidas"
    );

    // -------------------------------------------------------------- 2. ASR
    let t = Instant::now();
    let base_prompt = ctx.initial_prompt();
    let mut segments: Vec<TranscriptSegment> = Vec::new();
    let mut asr = AsrStats::default();
    let mut carried: Option<String> = None;
    let mut last_word_end = f32::MIN;
    let mut previous_window_end = f32::MIN;

    for (i, w) in windows.iter().enumerate() {
        if let Some(p) = progress {
            p(i, windows.len());
        }
        let slice = vad::slice(samples_16k, w);
        if slice.is_empty() {
            asr.empty_windows += 1;
            continue;
        }
        let herda = cfg.carry_chars > 0 && w.continues_previous;
        let prompt = if herda {
            asr.windows_with_context += 1;
            window_prompt(
                base_prompt.as_deref(),
                carried.as_deref().unwrap_or_default(),
                cfg.carry_chars,
            )
        } else {
            base_prompt.clone()
        };
        let out = engine.transcribe_with(
            slice,
            TranscribeRequest {
                lang: &cfg.lang,
                decode: &cfg.decode,
                prompt: prompt.as_deref(),
            },
        );
        let out = match out {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!(
                    janela = i,
                    inicio = w.start_secs,
                    "janela falhou ({e}) — a reunião segue sem ela"
                );
                asr.failed_windows += 1;
                carried = None;
                continue;
            }
        };
        asr.dropped_segments += out.dropped_segments;
        if out.segments.is_empty() {
            asr.empty_windows += 1;
            carried = None;
            continue;
        }

        // O texto só atravessa para a próxima janela se o modelo confiou nele.
        let confianca = mean(out.segments.iter().map(|s| s.avg_logprob));
        carried = (confianca >= cfg.min_carry_logprob).then(|| out.text.clone());
        if carried.is_none() {
            tracing::debug!(
                janela = i,
                avg_logprob = confianca,
                "texto pouco confiável não vira contexto da próxima janela"
            );
        }

        // Timestamps da janela → relógio da reunião. As janelas podem se
        // sobrepor de propósito (corte forçado numa fala longa): o que já foi
        // emitido não é emitido de novo.
        let sobrepoe = w.start_secs < previous_window_end;
        for mut s in out.segments {
            s.start_secs += w.start_secs;
            s.end_secs += w.start_secs;
            for word in &mut s.words {
                word.start_secs += w.start_secs;
                word.end_secs += w.start_secs;
            }
            if sobrepoe {
                s.words
                    .retain(|word| word.start_secs >= last_word_end - 0.05);
                if s.words.is_empty() && !s.text.is_empty() && s.end_secs <= previous_window_end {
                    continue; // segmento inteiro já saiu na janela anterior
                }
            }
            last_word_end = s
                .words
                .last()
                .map(|w| w.end_secs)
                .unwrap_or(s.end_secs)
                .max(last_word_end);
            segments.push(s);
        }
        previous_window_end = w.end_secs;
    }
    if let Some(p) = progress {
        p(windows.len(), windows.len());
    }
    timings.asr_secs = t.elapsed().as_secs_f32();

    segments.sort_by(|a, b| a.start_secs.total_cmp(&b.start_secs));
    asr.segments = segments.len();
    asr.words = segments.iter().map(|s| s.words.len()).sum();
    asr.avg_logprob = mean(segments.iter().map(|s| s.avg_logprob));
    asr.segments_under_1s = segments.iter().filter(|s| s.secs() < 1.0).count();
    asr.median_segment_secs = median(&segments.iter().map(|s| s.secs()).collect::<Vec<_>>());
    let raw_text = join_segments(&segments);
    tracing::info!(
        segments = asr.segments,
        words = asr.words,
        avg_logprob = asr.avg_logprob,
        dropped = asr.dropped_segments,
        failed_windows = asr.failed_windows,
        secs = timings.asr_secs,
        "ASR concluído"
    );

    // ------------------------------------------------------- 3. diarização
    let t = Instant::now();
    let diar = match diarizer {
        Some(d) => match d.diarize(samples_16k) {
            Ok(out) => Some(out),
            Err(e) => {
                tracing::warn!("diarização falhou ({e}) — falas ficam sem falante");
                None
            }
        },
        None => None,
    };
    timings.diarize_secs = t.elapsed().as_secs_f32();

    // ------------------------------------------- 4. falante por palavra
    let t = Instant::now();
    let turns: Vec<SpeakerTurn> = diar.as_ref().map(|d| d.turns.clone()).unwrap_or_default();
    let words = flatten_words(&segments);
    let tagged = align::tag_words(&words, &turns, &cfg.align);
    let utterances = align::build_utterances(&tagged, &cfg.align);
    let speakers = align::speaker_metrics(&utterances);
    timings.align_secs = t.elapsed().as_secs_f32();

    // ----------------------------------------------------- 5. normalização
    let terms = ctx.terms();
    let normalized_text = if cfg.apply_glossary && !terms.is_empty() {
        crate::text::apply_dictionary(&raw_text, &terms)
    } else {
        raw_text.clone()
    };
    let utterances = if cfg.apply_glossary && !terms.is_empty() {
        utterances
            .into_iter()
            .map(|mut u| {
                u.text = crate::text::apply_dictionary(&u.text, &terms);
                u
            })
            .collect()
    } else {
        utterances
    };

    timings.total_secs = started.elapsed().as_secs_f32();
    let report = PipelineReport {
        config: cfg.name.clone(),
        model: engine.model_name().to_string(),
        device: device().into(),
        dtw: engine.has_dtw(),
        audio_secs,
        realtime_factor: if audio_secs > 0.0 {
            timings.total_secs / audio_secs
        } else {
            0.0
        },
        timings,
        params: RunParams {
            profile: cfg.profile,
            lang: cfg.lang.clone(),
            decode: cfg.decode.clone(),
            vad: match &cfg.segmentation {
                Segmentation::Vad { opts, .. } => Some(opts.clone()),
                Segmentation::LegacyChunks { .. } => None,
            },
            windows: match &cfg.segmentation {
                Segmentation::Vad { windows, .. } => windows.clone(),
                Segmentation::LegacyChunks { chunk_secs, .. } => WindowOptions {
                    max_window_secs: *chunk_secs,
                    ..Default::default()
                },
            },
            align: cfg.align.clone(),
            context_chars: cfg.carry_chars,
        },
        vad: vad_stats,
        asr,
        diarization: diar.as_ref().map(|d| {
            let mut s = d.stats.clone();
            s.warnings = d.warnings.clone();
            s
        }),
        speakers,
    };
    tracing::info!(
        rtf = report.realtime_factor,
        total_secs = report.timings.total_secs,
        speakers = report.speakers.speakers,
        switches = report.speakers.switches,
        "passe final concluído"
    );

    Ok(FinalTranscript {
        utterances,
        words: tagged,
        raw_segments: segments,
        raw_text,
        normalized_text,
        report,
    })
}

/// Define as janelas segundo a estratégia escolhida.
fn plan(samples_16k: &[f32], seg: &Segmentation) -> Result<(Vec<AsrWindow>, Vec<SpeechRegion>)> {
    let total = samples_16k.len() as f32 / WHISPER_SAMPLE_RATE as f32;
    match seg {
        Segmentation::Vad {
            model,
            opts,
            windows,
        } => {
            let mut v = Vad::new(model)?;
            let regions = v.regions(samples_16k, opts)?;
            Ok((vad::plan_windows(&regions, total, windows), regions))
        }
        Segmentation::LegacyChunks {
            chunk_secs,
            silence_rms,
        } => Ok((
            legacy_windows(samples_16k, *chunk_secs, *silence_rms),
            Vec::new(),
        )),
    }
}

/// Reproduz o fatiamento antigo sobre um arquivo: blocos de `chunk_secs`
/// cortados no ponto de menor energia do último 1,5 s, pulando os blocos
/// silenciosos. Existe só para o benchmark ter um "antes" fiel.
fn legacy_windows(samples_16k: &[f32], chunk_secs: f32, silence_rms: f32) -> Vec<AsrWindow> {
    let rate = WHISPER_SAMPLE_RATE as usize;
    let alvo = (chunk_secs * rate as f32) as usize;
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < samples_16k.len() {
        let fim_bruto = (pos + alvo).min(samples_16k.len());
        let bloco = &samples_16k[pos..fim_bruto];
        let corte = if fim_bruto == samples_16k.len() {
            bloco.len()
        } else {
            legacy_quiet_cut(bloco, rate)
        };
        let fim = pos + corte.max(1);
        let pedaco = &samples_16k[pos..fim];
        let rms = (pedaco.iter().map(|s| s * s).sum::<f32>() / pedaco.len().max(1) as f32).sqrt();
        if rms >= silence_rms {
            out.push(AsrWindow {
                start_secs: pos as f32 / rate as f32,
                end_secs: fim as f32 / rate as f32,
                speech_secs: (fim - pos) as f32 / rate as f32,
                continues_previous: false,
            });
        }
        pos = fim;
    }
    out
}

/// A regra de corte antiga, palavra por palavra como estava em `meeting.rs`:
/// janela de 100 ms de menor energia dentro do último 1,5 s. Mantida aqui,
/// isolada e com nome que não engana, só para o benchmark.
fn legacy_quiet_cut(buf: &[f32], rate: usize) -> usize {
    let frames = buf.len();
    let win = (rate / 10).max(1);
    let search = (rate * 3 / 2).min(frames);
    if search < win * 2 {
        return buf.len();
    }
    let start = frames - search;
    let mut best_frame = frames;
    let mut best_energy = f32::MAX;
    let mut f = start;
    while f + win <= frames {
        let e: f32 = buf[f..f + win].iter().map(|s| s * s).sum();
        if e < best_energy {
            best_energy = e;
            best_frame = f + win / 2;
        }
        f += win / 2;
    }
    best_frame
}

/// Palavras de todos os segmentos. Quando o perfil não pediu timestamps por
/// token, cada SEGMENTO vira uma "palavra" com o texto inteiro — que é
/// exatamente a granularidade da atribuição antiga, sem inventar horário
/// nenhum.
fn flatten_words(segments: &[TranscriptSegment]) -> Vec<Word> {
    let mut out = Vec::new();
    for s in segments {
        if s.words.is_empty() {
            if !s.text.trim().is_empty() {
                out.push(Word {
                    text: s.text.clone(),
                    start_secs: s.start_secs,
                    end_secs: s.end_secs,
                    prob: 0.0,
                });
            }
            continue;
        }
        out.extend(s.words.iter().cloned());
    }
    out
}

fn join_segments(segments: &[TranscriptSegment]) -> String {
    segments
        .iter()
        .map(|s| s.text.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn mean(it: impl Iterator<Item = f32>) -> f32 {
    let (soma, n) = it.fold((0.0f64, 0usize), |(s, n), v| (s + v as f64, n + 1));
    if n == 0 {
        0.0
    } else {
        (soma / n as f64) as f32
    }
}

fn median(v: &[f32]) -> f32 {
    let mut d = v.to_vec();
    d.sort_by(f32::total_cmp);
    match d.len() {
        0 => 0.0,
        n if n % 2 == 1 => d[n / 2],
        n => (d[n / 2 - 1] + d[n / 2]) / 2.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f32, end: f32, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            start_secs: start,
            end_secs: end,
            text: text.into(),
            no_speech_prob: 0.0,
            avg_logprob: -0.3,
            words: Vec::new(),
        }
    }

    #[test]
    fn segmento_sem_palavras_vira_uma_unidade_so() {
        let segs = [seg(0.0, 5.0, "bom dia a todos"), seg(5.0, 6.0, "  ")];
        let w = flatten_words(&segs);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].text, "bom dia a todos");
        assert_eq!(w[0].start_secs, 0.0);
        assert_eq!(w[0].end_secs, 5.0);
    }

    #[test]
    fn fatiamento_antigo_cobre_o_audio_e_pula_silencio() {
        // 40 s: 20 s de tom, 20 s de silêncio absoluto.
        let rate = WHISPER_SAMPLE_RATE as usize;
        let mut buf: Vec<f32> = (0..rate * 20)
            .map(|i| (i as f32 * 200.0 * std::f32::consts::TAU / rate as f32).sin() * 0.3)
            .collect();
        buf.extend(std::iter::repeat_n(0.0f32, rate * 20));
        let w = legacy_windows(&buf, 20.0, 0.0035);
        assert!(!w.is_empty());
        // Nenhuma janela deve estar inteiramente dentro do silêncio.
        assert!(
            w.iter().all(|x| x.start_secs < 21.0),
            "janela de silêncio não foi pulada: {w:?}"
        );
        // As janelas são contíguas e não retrocedem.
        for par in w.windows(2) {
            assert!(par[1].start_secs >= par[0].end_secs - 0.001, "{par:?}");
        }
    }

    #[test]
    fn baseline_e_final_sao_configuracoes_distintas_e_declaradas() {
        let b = FinalConfig::baseline();
        let f = FinalConfig::meeting_final(PathBuf::from("silero.bin"));
        assert!(matches!(b.segmentation, Segmentation::LegacyChunks { .. }));
        assert!(matches!(f.segmentation, Segmentation::Vad { .. }));
        assert_eq!(b.decode.beam_size, 0);
        assert_eq!(f.decode.beam_size, 5);
        assert_eq!(b.carry_chars, 0);
        assert!(f.carry_chars > 0);
        assert!(!b.decode.token_timestamps);
        assert!(f.decode.token_timestamps);
        // As duas têm que ir e voltar do JSON: é assim que o benchmark grava
        // o que rodou.
        let json = serde_json::to_string(&f).expect("serializa");
        let volta: FinalConfig = serde_json::from_str(&json).expect("desserializa");
        assert_eq!(volta, f);
    }

    #[test]
    fn media_e_mediana_de_lista_vazia_nao_explodem() {
        assert_eq!(mean(std::iter::empty()), 0.0);
        assert_eq!(median(&[]), 0.0);
        assert_eq!(median(&[1.0, 3.0]), 2.0);
        assert_eq!(median(&[1.0, 3.0, 10.0]), 3.0);
    }
}
