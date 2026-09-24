//! isper-mobile — a fachada do núcleo do ISPer para o celular (Fase 9.1).
//!
//! O app Android (Kotlin e Jetpack Compose, em `apps/isper-android`) fala com
//! o núcleo Rust por aqui; o iOS (SwiftUI) vai usar a mesma fachada. Os
//! bindings são gerados pelo UniFFI a partir das anotações `#[uniffi::export]`:
//! o contrato é tipado dos dois lados, então uma assinatura que muda quebra a
//! compilação do app, e não o app em uso. A decisão está no ADR 0014.
//!
//! Esta primeira versão serve o **spike** da 9.1 — medir o passe final no
//! próprio aparelho antes de apostar nele:
//!
//! - baixar o modelo Whisper, o VAD e os dois modelos de diarização, com a
//!   mesma conferência de SHA-256 do desktop ([`download_model`] e cia.);
//! - transcrever um arquivo com o **mesmo** pipeline do PC
//!   ([`TranscriptionJob::run`]): decodificação, VAD Silero, busca em feixe,
//!   falante por palavra;
//! - devolver tempo por etapa, fator de tempo real, pico de memória e, quando
//!   há referência, WER, CER e DER ([`SpikeReport`]).
//!
//! O gravador (9.2) e a sincronia com o PC (9.3) entram depois, em cima
//! desta mesma fachada.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use isper_core::align::SpeakerTurn;
use isper_core::context::MeetingContext;
use isper_core::metrics::{self, DiarizeStats, Normalization};
use isper_core::pipeline::{self, Diarizer, DiarizerOutput, FinalConfig};
use isper_core::{EngineOptions, WhisperEngine};

uniffi::setup_scaffolding!();

/// Erro que chega ao app como exceção. A mensagem já vem em português e é
/// para mostrar como está.
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum MobileError {
    /// Modelo ausente, download que falhou ou checksum que não bateu.
    #[error("{0}")]
    Model(String),
    /// Arquivo de áudio que não abriu ou não decodificou.
    #[error("{0}")]
    Audio(String),
    /// Falha do motor: Whisper, VAD ou diarização.
    #[error("{0}")]
    Engine(String),
    /// Quem chamou pediu para parar.
    #[error("cancelado")]
    Cancelled,
}

impl From<isper_models::ModelsError> for MobileError {
    fn from(e: isper_models::ModelsError) -> Self {
        Self::Model(e.to_string())
    }
}

impl From<isper_diarize::DiarizeError> for MobileError {
    fn from(e: isper_diarize::DiarizeError) -> Self {
        match e {
            isper_diarize::DiarizeError::Download(d) => Self::Model(d.to_string()),
            other => Self::Engine(other.to_string()),
        }
    }
}

impl From<isper_core::IsperError> for MobileError {
    fn from(e: isper_core::IsperError) -> Self {
        use isper_core::IsperError as E;
        match e {
            E::Cancelled => Self::Cancelled,
            E::Decode(_) | E::Io(_) | E::Wav(_) | E::Resample(_) => Self::Audio(e.to_string()),
            other => Self::Engine(other.to_string()),
        }
    }
}

/// A etapa que o progresso descreve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Stage {
    /// Baixando um modelo (`done`/`total` em bytes).
    Download,
    /// Lendo o arquivo de áudio (`done`/`total` em porcentagem).
    Decode,
    /// Transcrevendo (`done`/`total` em janelas do VAD).
    Transcribe,
    /// Separando os falantes (`0/1` ao começar, `1/1` ao terminar).
    Diarize,
}

/// Quem acompanha o progresso: implementado em Kotlin (ou Swift) pelo app.
///
/// É chamado da thread que faz o trabalho — o app devolve para a thread da
/// interface por conta própria.
#[uniffi::export(with_foreign)]
pub trait ProgressListener: Send + Sync {
    /// `done` de `total`, na unidade da etapa (ver [`Stage`]).
    fn on_progress(&self, stage: Stage, done: u64, total: u64);
}

/// O que o app precisa saber do motor embarcado.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct EngineInfo {
    /// Versão da fachada.
    pub version: String,
    /// Arquitetura e sistema para os quais a biblioteca foi compilada
    /// (`aarch64-android`, `x86_64-android`…).
    pub target: String,
    /// Núcleos que o sistema oferece a este processo.
    pub cpu_threads: u32,
}

/// Versão e alvo da biblioteca carregada.
#[uniffi::export]
pub fn engine_info() -> EngineInfo {
    EngineInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        cpu_threads: cpu_threads(),
    }
}

fn cpu_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1)
}

/// Um modelo Whisper que o spike pode baixar.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ModelOption {
    /// Nome do arquivo (também é o identificador).
    pub file: String,
    /// Nome para a tela.
    pub label: String,
    /// Tamanho aproximado do download, em MB.
    pub approx_mb: u32,
    /// Para que serve, em uma frase.
    pub note: String,
}

/// Os modelos candidatos para o celular, do menor ao maior.
#[uniffi::export]
pub fn mobile_models() -> Vec<ModelOption> {
    isper_models::MOBILE_CATALOG
        .iter()
        .map(|m| ModelOption {
            file: m.file.to_string(),
            label: m.label.to_string(),
            approx_mb: m.approx_mb,
            note: m.note.to_string(),
        })
        .collect()
}

/// Baixa um modelo Whisper de [`mobile_models`] para `dir`, conferindo o
/// SHA-256 publicado pelo Hugging Face. Devolve o caminho do arquivo.
#[uniffi::export]
pub fn download_model(
    file: String,
    dir: String,
    listener: Arc<dyn ProgressListener>,
) -> Result<String, MobileError> {
    let path = isper_models::download_whisper_to(&file, Path::new(&dir), &mut |done, total| {
        listener.on_progress(Stage::Download, done, total)
    })?;
    Ok(path.display().to_string())
}

/// Baixa o modelo de VAD (Silero, ~1 MB) para `dir`, se ainda não estiver lá.
#[uniffi::export]
pub fn download_vad(
    dir: String,
    listener: Arc<dyn ProgressListener>,
) -> Result<String, MobileError> {
    let path = isper_models::download_vad_to(Path::new(&dir), &mut |done, total| {
        listener.on_progress(Stage::Download, done, total)
    })?;
    Ok(path.display().to_string())
}

/// Baixa os dois modelos de diarização (~45 MB) para `dir`, pulando os que
/// já estão lá.
#[uniffi::export]
pub fn download_diarize_models(
    dir: String,
    listener: Arc<dyn ProgressListener>,
) -> Result<(), MobileError> {
    isper_diarize::download_models_to(Path::new(&dir), &mut |_, done, total| {
        listener.on_progress(Stage::Download, done, total)
    })?;
    Ok(())
}

/// Os modelos de diarização estão todos em `dir`?
#[uniffi::export]
pub fn diarize_models_installed(dir: String) -> bool {
    let (seg, emb) = isper_diarize::model_paths_in(Path::new(&dir));
    seg.exists() && emb.exists()
}

/// Como transcrever.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TranscribeOptions {
    /// Caminho do modelo Whisper (`ggml-*.bin`).
    pub model_path: String,
    /// Caminho do modelo de VAD (ver [`download_vad`]).
    pub vad_path: String,
    /// Idioma da fala: `pt`, `en`… ou `auto`.
    pub lang: String,
    /// Pasta com os modelos de diarização; `None` transcreve sem separar os
    /// falantes.
    pub diarize_models_dir: Option<String>,
    /// Número de participantes, quando se sabe.
    pub num_speakers: Option<u32>,
    /// Threads da diarização; `None` deixa o `isper-diarize` escolher.
    pub diarize_threads: Option<u32>,
    /// Transcrição de referência, para calcular WER e CER.
    pub reference_text: Option<String>,
    /// Turnos de referência (`início<TAB>fim<TAB>falante` por linha), para
    /// calcular o DER.
    pub reference_turns: Option<String>,
}

/// Uma fala da transcrição.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Utterance {
    /// Início, em segundos do áudio.
    pub start_secs: f32,
    /// Fim, em segundos do áudio.
    pub end_secs: f32,
    /// Falante (0, 1, 2…); `None` sem diarização.
    pub speaker: Option<u32>,
    /// O que foi dito.
    pub text: String,
}

/// Tudo o que uma rodada mediu — o relatório do spike.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SpikeReport {
    /// Arquivo do modelo usado.
    pub model: String,
    /// Duração do áudio.
    pub audio_secs: f32,
    /// Leitura e conversão do arquivo para 16 kHz mono.
    pub decode_secs: f32,
    /// Carga do modelo.
    pub load_secs: f32,
    /// VAD e Whisper.
    pub transcribe_secs: f32,
    /// Diarização (0 sem ela).
    pub diarize_secs: f32,
    /// Do começo ao fim, somando tudo.
    pub total_secs: f32,
    /// `total_secs / audio_secs`: abaixo de 1, mais rápido que a reunião.
    pub realtime_factor: f32,
    /// Pico de memória residente do processo, em MB (quando o sistema diz).
    pub peak_rss_mb: Option<f32>,
    /// Threads que o sistema oferece ao processo.
    pub cpu_threads: u32,
    /// Falantes encontrados (0 sem diarização).
    pub speakers: u32,
    /// Taxa de erro de palavras, de 0 a 1 (com referência).
    pub wer: Option<f32>,
    /// Taxa de erro de caracteres, de 0 a 1 (com referência).
    pub cer: Option<f32>,
    /// Taxa de erro da diarização, de 0 a 1 (com turnos de referência).
    pub der: Option<f32>,
    /// Texto corrido, depois do glossário.
    pub text: String,
    /// As falas, com falante e horário.
    pub utterances: Vec<Utterance>,
    /// O relatório completo do pipeline, em JSON — o mesmo formato do
    /// `isper-cli bench`, para comparar com o PC.
    pub pipeline_json: String,
}

/// Uma transcrição que pode ser cancelada de outra thread.
#[derive(Debug, Default, uniffi::Object)]
pub struct TranscriptionJob {
    cancel: AtomicBool,
}

#[uniffi::export]
impl TranscriptionJob {
    /// Um trabalho novo, ainda não iniciado.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Pede para parar: a rodada termina com [`MobileError::Cancelled`] antes
    /// da próxima janela.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Transcreve `audio_path` com o passe final do PC e mede cada etapa.
    ///
    /// Bloqueia até terminar — chame de uma thread de trabalho.
    pub fn run(
        &self,
        audio_path: String,
        options: TranscribeOptions,
        listener: Arc<dyn ProgressListener>,
    ) -> Result<SpikeReport, MobileError> {
        run_job(&self.cancel, Path::new(&audio_path), &options, &listener)
    }
}

fn run_job(
    cancel: &AtomicBool,
    audio: &Path,
    options: &TranscribeOptions,
    listener: &Arc<dyn ProgressListener>,
) -> Result<SpikeReport, MobileError> {
    let started = Instant::now();

    let t = Instant::now();
    let on_decode = |fraction: f32| {
        listener.on_progress(Stage::Decode, (fraction * 100.0).round() as u64, 100);
    };
    let decoded = isper_core::decode::decode_to_16k(audio, Some(&on_decode), Some(cancel))?;
    let decode_secs = t.elapsed().as_secs_f32();
    let audio_secs = decoded.duration_secs();

    let cfg = FinalConfig {
        lang: options.lang.clone(),
        ..FinalConfig::meeting_final(PathBuf::from(&options.vad_path))
    };
    let t = Instant::now();
    let engine = WhisperEngine::new_with(
        Path::new(&options.model_path),
        &EngineOptions {
            dtw: cfg.decode.token_timestamps,
        },
    )?;
    let load_secs = t.elapsed().as_secs_f32();

    let diarizer = match &options.diarize_models_dir {
        Some(dir) => {
            let (segmentation, embedding) = isper_diarize::model_paths_in(Path::new(dir));
            if !segmentation.exists() || !embedding.exists() {
                return Err(MobileError::Model(
                    "modelos de diarização ausentes — baixe antes de transcrever".into(),
                ));
            }
            Some(MobileDiarizer {
                segmentation,
                embedding,
                opts: isper_diarize::DiarizeOptions {
                    num_speakers: options.num_speakers,
                    threads: options.diarize_threads.unwrap_or(0) as usize,
                    ..isper_diarize::DiarizeOptions::default()
                },
                listener: Arc::clone(listener),
            })
        }
        None => None,
    };

    let on_window = |done: usize, total: usize| {
        listener.on_progress(Stage::Transcribe, done as u64, total as u64);
    };
    let out = pipeline::run_cancellable(
        &engine,
        &decoded.samples_16k,
        &cfg,
        &MeetingContext::default(),
        diarizer.as_ref().map(|d| d as &dyn Diarizer),
        Some(&on_window),
        Some(cancel),
    )?;

    let normalization = Normalization::default();
    let (wer, cer) = match &options.reference_text {
        Some(reference) => (
            Some(metrics::wer(reference, &out.normalized_text, &normalization).rate),
            Some(metrics::cer(reference, &out.normalized_text, &normalization).rate),
        ),
        None => (None, None),
    };
    let der = match &options.reference_turns {
        Some(tsv) => {
            let reference = metrics::parse_turns(tsv).map_err(MobileError::Engine)?;
            let hypothesis: Vec<SpeakerTurn> = out
                .utterances
                .iter()
                .filter_map(|u| {
                    u.speaker.map(|speaker| SpeakerTurn {
                        start_secs: u.start_secs,
                        end_secs: u.end_secs,
                        speaker,
                    })
                })
                .collect();
            diarizer
                .is_some()
                .then(|| metrics::der(&reference, &hypothesis).rate)
        }
        None => None,
    };

    let speakers = out
        .report
        .diarization
        .as_ref()
        .map(|d| d.speakers as u32)
        .unwrap_or(0);
    Ok(SpikeReport {
        model: engine.model_name().to_string(),
        audio_secs,
        decode_secs,
        load_secs,
        transcribe_secs: out.report.timings.vad_secs + out.report.timings.asr_secs,
        diarize_secs: out.report.timings.diarize_secs,
        total_secs: started.elapsed().as_secs_f32(),
        realtime_factor: if audio_secs > 0.0 {
            started.elapsed().as_secs_f32() / audio_secs
        } else {
            0.0
        },
        peak_rss_mb: peak_rss_mb(),
        cpu_threads: cpu_threads(),
        speakers,
        wer,
        cer,
        der,
        text: out.normalized_text.clone(),
        utterances: out
            .utterances
            .iter()
            .map(|u| Utterance {
                start_secs: u.start_secs,
                end_secs: u.end_secs,
                speaker: u.speaker,
                text: u.text.clone(),
            })
            .collect(),
        pipeline_json: out.report.to_json(),
    })
}

/// Liga o `isper-diarize` ao pipeline do núcleo, avisando o app do começo e
/// do fim da etapa (a diarização não tem progresso intermediário).
struct MobileDiarizer {
    segmentation: PathBuf,
    embedding: PathBuf,
    opts: isper_diarize::DiarizeOptions,
    listener: Arc<dyn ProgressListener>,
}

impl Diarizer for MobileDiarizer {
    fn diarize(&self, samples_16k: &[f32]) -> Result<DiarizerOutput, String> {
        self.listener.on_progress(Stage::Diarize, 0, 1);
        let out = isper_diarize::diarize_with_models(
            &self.segmentation,
            &self.embedding,
            samples_16k,
            &self.opts,
        )
        .map_err(|e| e.to_string())?;
        self.listener.on_progress(Stage::Diarize, 1, 1);
        Ok(DiarizerOutput {
            turns: out
                .turns
                .iter()
                .map(|t| SpeakerTurn {
                    start_secs: t.start,
                    end_secs: t.end,
                    speaker: t.speaker,
                })
                .collect(),
            stats: DiarizeStats {
                raw_clusters: out.metrics.raw_clusters,
                raw_turns: out.metrics.raw_turns,
                speakers: out.metrics.speakers,
                turns: out.metrics.turns,
                absorbed_clusters: out.metrics.absorbed_clusters,
                median_turn_secs: out.metrics.median_turn_secs,
                very_short_turns: out.metrics.very_short_turns,
                warnings: Vec::new(),
            },
            warnings: out.warnings,
        })
    }
}

/// Pico de memória residente (`VmHWM` de `/proc/self/status`), em MB.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn peak_rss_mb() -> Option<f32> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: f32 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024.0)
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn peak_rss_mb() -> Option<f32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_descreve_a_biblioteca() {
        let info = engine_info();
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(info.cpu_threads >= 1);
        assert!(info.target.contains(std::env::consts::OS));
    }

    #[test]
    fn catalogo_do_celular_vai_do_menor_ao_maior() {
        let models = mobile_models();
        assert!(models.len() >= 3);
        assert!(models.windows(2).all(|w| w[0].approx_mb <= w[1].approx_mb));
        assert!(models.iter().all(|m| m.file.starts_with("ggml-")));
    }

    #[test]
    fn diarizacao_sem_modelos_nao_esta_instalada() {
        let dir = std::env::temp_dir().join("isper-mobile-sem-modelos");
        assert!(!diarize_models_installed(dir.display().to_string()));
    }

    struct Silent;
    impl ProgressListener for Silent {
        fn on_progress(&self, _: Stage, _: u64, _: u64) {}
    }

    #[test]
    fn arquivo_que_nao_existe_vira_erro_de_audio() {
        let job = TranscriptionJob::new();
        let err = job
            .run(
                "nao-existe.wav".into(),
                TranscribeOptions {
                    model_path: "nao-existe.bin".into(),
                    vad_path: "nao-existe-vad.bin".into(),
                    lang: "pt".into(),
                    diarize_models_dir: None,
                    num_speakers: None,
                    diarize_threads: None,
                    reference_text: None,
                    reference_turns: None,
                },
                Arc::new(Silent),
            )
            .expect_err("arquivo inexistente");
        assert!(matches!(err, MobileError::Audio(_)), "{err:?}");
    }

    #[test]
    fn cancelar_antes_de_comecar_para_na_leitura() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/fala-16k.wav");
        let job = TranscriptionJob::new();
        job.cancel();
        let err = job
            .run(
                fixture.display().to_string(),
                TranscribeOptions {
                    model_path: "nao-existe.bin".into(),
                    vad_path: "nao-existe-vad.bin".into(),
                    lang: "pt".into(),
                    diarize_models_dir: None,
                    num_speakers: None,
                    diarize_threads: None,
                    reference_text: None,
                    reference_turns: None,
                },
                Arc::new(Silent),
            )
            .expect_err("cancelado");
        assert!(matches!(err, MobileError::Cancelled), "{err:?}");
    }
}
