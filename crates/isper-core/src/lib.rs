//! isper-core — o motor do ISPer: captura de áudio, transcrição, reunião e
//! banco, sem interface nenhuma.
//!
//! É usado pelo app Tauri (`apps/isper-app`) e pelo laboratório de terminal
//! (`isper-cli`). Manter o motor separado da UI é o que permite testá-lo
//! sozinho — com fontes de áudio falsas, relógio injetado e provider de IA
//! falso — e reaproveitá-lo nos dois.
//!
//! # Mapa
//!
//! | Etapa | Módulos |
//! |---|---|
//! | captura | [`audio`] (microfone, WAV, resample), [`loopback`] (o que sai na caixa de som), [`recorder`] (ditado), [`calls`] (chamada do Teams em andamento) |
//! | arquivos de áudio | [`decode`] (MP3, M4A, WAV, FLAC, OGG → 16 kHz mono), [`recording`] (data e título pelo nome do arquivo) |
//! | reunião ao vivo | [`meeting`] (os dois canais no relógio da reunião), [`chunk`] (corte em silêncio) |
//! | transcrição | [`engine`] (whisper.cpp), [`profile`] (perfis e decodificação), [`context`] (glossário e contexto), [`text`] (alucinações, comandos de voz, dicionário) |
//! | passe final | [`vad`] (Silero), [`pipeline`] (o passe inteiro), [`align`] (falante por palavra) |
//! | dados | [`store`] (SQLite), [`export`] (Markdown, SRT, DOCX), [`import`] (Markdown → banco), [`embed`] (trechos e vetores) |
//! | qualidade | [`metrics`] (WER, CER, DER e o relatório do benchmark), [`panics`] (pânico → log) |
//!
//! O porquê das escolhas grandes está nos ADRs, em `docs/adr/`; a arquitetura
//! do pipeline de transcrição, em `docs/transcription-pipeline.md`.

// Todo item público tem documentação: o `cargo doc` é a referência da API
// do motor (fase 7.6), e um item novo sem doc não compila.
#![deny(missing_docs)]

pub mod align;
pub mod audio;
pub mod calls;
pub mod chunk;
pub mod context;
pub mod decode;
pub mod embed;
pub mod engine;
pub mod export;
pub mod import;
pub mod loopback;
pub mod meeting;
pub mod metrics;
pub mod panics;
pub mod pipeline;
pub mod profile;
pub mod recorder;
pub mod recording;
pub mod store;
pub mod text;
pub mod vad;

pub use audio::RawAudio;
pub use engine::{EngineOptions, Transcript, TranscriptSegment, WhisperEngine, Word};
pub use profile::{DecodeConfig, TranscriptionProfile};

/// O Whisper só aceita áudio em 16 kHz, mono, f32. Tudo converge para cá.
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// Erros do motor. `thiserror` gera as impls de `std::error::Error` — em uma
/// biblioteca, erros tipados permitem que quem chama trate cada caso;
/// o `anyhow` (genérico) fica só no binário.
#[derive(Debug, thiserror::Error)]
pub enum IsperError {
    /// O Windows não expõe nenhum microfone.
    #[error("nenhum dispositivo de entrada de áudio encontrado")]
    NoInputDevice,
    /// Falha de captura ou de dispositivo de áudio (com a dica, quando há).
    #[error("erro de áudio: {0}")]
    Audio(String),
    /// Falha ao ler ou gravar um WAV.
    #[error("erro de WAV: {0}")]
    Wav(#[from] hound::Error),
    /// Falha na conversão de taxa de amostragem.
    #[error("erro de resample: {0}")]
    Resample(String),
    /// Falha ao carregar o modelo ou ao transcrever.
    #[error("erro do Whisper: {0}")]
    Whisper(String),
    /// Erro do SQLite.
    #[error("erro de banco de dados: {0}")]
    Db(#[from] rusqlite::Error),
    /// Banco numa versão de schema que este ISPer não entende, ou migração que falhou.
    #[error("banco de dados: {0}")]
    Schema(String),
    /// Erro de arquivo ou de sistema.
    #[error("erro de E/S: {0}")]
    Io(#[from] std::io::Error),
    /// Arquivo de áudio que não pôde ser lido: formato, codec ou dados.
    #[error("não consegui ler o áudio: {0}")]
    Decode(String),
    /// A operação foi cancelada por quem pediu.
    #[error("cancelado")]
    Cancelled,
}

/// `Result` com o erro do núcleo.
pub type Result<T> = std::result::Result<T, IsperError>;
