//! isper-cli — laboratório da Fase 1: gravar/ler áudio e transcrever no terminal.
//!
//! Uso:
//!   isper-cli rec 5              # grava 5 s do microfone e transcreve
//!   isper-cli file fala.wav      # transcreve um arquivo WAV
//!   isper-cli --lang en file x.wav --model models/ggml-small.bin

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use isper_core::{audio, WhisperEngine};

#[derive(Parser)]
#[command(name = "isper-cli", about = "ISPer — transcrição 100% local com Whisper (Fase 1)")]
struct Cli {
    /// Caminho do modelo ggml
    #[arg(long, default_value = "models/ggml-small.bin")]
    model: PathBuf,

    /// Idioma da fala (pt, en, ... ou "auto")
    #[arg(long, default_value = "pt")]
    lang: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Grava N segundos do microfone e transcreve
    Rec { seconds: u64 },
    /// Transcreve um arquivo .wav
    File { path: PathBuf },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).compact().init();
    let cli = Cli::parse();

    let raw = match &cli.cmd {
        Cmd::Rec { seconds } => {
            println!("== gravando por {seconds}s... fale! ==");
            audio::record(Duration::from_secs(*seconds)).context("falha ao gravar do microfone")?
        }
        Cmd::File { path } => audio::load_wav(path)
            .with_context(|| format!("falha ao ler {}", path.display()))?,
    };

    println!(
        "audio: {:.1}s @ {} Hz, {} canal(is) -> convertendo para 16 kHz mono",
        raw.duration_secs(),
        raw.sample_rate,
        raw.channels
    );
    let samples = raw.into_whisper_input()?;

    println!("carregando modelo {} ...", cli.model.display());
    let engine = WhisperEngine::new(&cli.model)?;

    let t = engine.transcribe(&samples, &cli.lang)?;
    println!();
    for seg in &t.segments {
        println!("[{:>6.2}s -> {:>6.2}s] {}", seg.start_secs, seg.end_secs, seg.text);
    }
    println!();
    println!(">> {}", t.text);
    let rtf = t.audio_secs / t.infer_secs.max(0.001);
    println!(
        "tempo: audio {:.1}s | inferencia {:.1}s | {:.1}x tempo real",
        t.audio_secs, t.infer_secs, rtf
    );
    Ok(())
}
