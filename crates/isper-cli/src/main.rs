//! isper-cli — laboratório do motor: transcrição e reunião no terminal.
//!
//! Uso:
//!   isper-cli rec 5              # grava 5 s do microfone e transcreve
//!   isper-cli file fala.wav      # transcreve um arquivo WAV
//!   isper-cli meeting 30         # grava reunião (mic + sistema) por 30 s
//!   isper-cli --lang en --model models/ggml-small.bin file x.wav

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use isper_core::{audio, meeting, store::MeetingStore, WhisperEngine};

#[derive(Parser)]
#[command(name = "isper-cli", about = "ISPer — transcrição 100% local com Whisper")]
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
    /// Grava uma reunião (mic = "Eu" + áudio do sistema = "Participantes")
    /// por N segundos, transcreve em blocos e salva Markdown + SQLite
    Meeting { seconds: u64 },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).compact().init();
    let cli = Cli::parse();

    match &cli.cmd {
        Cmd::Meeting { seconds } => run_meeting(&cli, *seconds),
        _ => run_dictation(&cli),
    }
}

fn run_dictation(cli: &Cli) -> anyhow::Result<()> {
    let raw = match &cli.cmd {
        Cmd::Rec { seconds } => {
            println!("== gravando por {seconds}s... fale! ==");
            audio::record(Duration::from_secs(*seconds)).context("falha ao gravar do microfone")?
        }
        Cmd::File { path } => audio::load_wav(path)
            .with_context(|| format!("falha ao ler {}", path.display()))?,
        Cmd::Meeting { .. } => unreachable!(),
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

fn run_meeting(cli: &Cli, seconds: u64) -> anyhow::Result<()> {
    println!("carregando modelo {} ...", cli.model.display());
    let engine = Arc::new(WhisperEngine::new(&cli.model)?);

    println!("== gravando reuniao por {seconds}s (mic = Eu, sistema = Participantes) ==");
    let handle = meeting::start(engine).context("falha ao abrir captura da reunião")?;
    std::thread::sleep(Duration::from_secs(seconds));

    println!("encerrando... transcrevendo blocos restantes");
    let result = handle.stop()?;

    println!();
    for seg in &result.segments {
        println!(
            "[{:>7.1}s] {:>13}: {}",
            seg.start_secs,
            seg.speaker.label(),
            seg.text
        );
    }
    println!();
    println!(
        "reuniao de {:.0}s com {} segmento(s)",
        result.duration_secs,
        result.segments.len()
    );

    let now = chrono::Local::now();
    let started_at = now.format("%d/%m/%Y %H:%M").to_string();
    let title = format!("Reunião — {started_at}");

    std::fs::create_dir_all("reunioes")?;
    let md_path = format!("reunioes/reuniao-{}.md", now.format("%Y%m%d-%H%M%S"));
    std::fs::write(&md_path, meeting::to_markdown(&title, &started_at, &result))?;

    let db = MeetingStore::open(std::path::Path::new("isper.db"))?;
    let id = db.save(&title, &started_at, &result)?;

    println!("salvo: {md_path} | banco: isper.db (reuniao id {id})");
    Ok(())
}
