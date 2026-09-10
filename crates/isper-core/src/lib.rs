//! isper-core — o motor do ISPer: captura de áudio, resample e transcrição.
//!
//! Esta biblioteca não conhece interface nenhuma: é usada pelo `isper-cli`
//! (Fase 1) e, futuramente, pelo app Tauri (Fase 2). Manter o motor separado
//! da UI é o que permite testá-lo sozinho e evoluir o projeto sem reescrever.

pub mod audio;
pub mod engine;
pub mod export;
pub mod loopback;
pub mod meeting;
pub mod panics;
pub mod recorder;
pub mod store;
pub mod text;

pub use audio::RawAudio;
pub use engine::{Transcript, TranscriptSegment, WhisperEngine};

/// O Whisper só aceita áudio em 16 kHz, mono, f32. Tudo converge para cá.
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// Erros do motor. `thiserror` gera as impls de `std::error::Error` — em uma
/// biblioteca, erros tipados permitem que quem chama trate cada caso;
/// o `anyhow` (genérico) fica só no binário.
#[derive(Debug, thiserror::Error)]
pub enum IsperError {
    #[error("nenhum dispositivo de entrada de áudio encontrado")]
    NoInputDevice,
    #[error("erro de áudio: {0}")]
    Audio(String),
    #[error("erro de WAV: {0}")]
    Wav(#[from] hound::Error),
    #[error("erro de resample: {0}")]
    Resample(String),
    #[error("erro do Whisper: {0}")]
    Whisper(String),
    #[error("erro de banco de dados: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("erro de E/S: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, IsperError>;
