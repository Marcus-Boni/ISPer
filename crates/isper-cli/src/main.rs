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
    /// Configura a inteligência de nuvem (Fase 5)
    #[command(subcommand)]
    Llm(LlmCmd),
}

#[derive(Subcommand)]
enum LlmCmd {
    /// Escolhe o provider (claude, groq ou gemini) e opcionalmente o modelo
    Use {
        provider: String,
        #[arg(long)]
        model: Option<String>,
    },
    /// Guarda a chave de API no Credential Manager do Windows (pede no prompt)
    SetKey { provider: String },
    /// Remove a chave guardada
    DeleteKey { provider: String },
    /// Mostra o provider configurado e se há chave guardada
    Status,
    /// Faz uma chamada de teste ao provider configurado
    Test,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).compact().init();
    let cli = Cli::parse();

    match &cli.cmd {
        Cmd::Meeting { seconds } => run_meeting(&cli, *seconds),
        Cmd::Llm(llm_cmd) => run_llm(llm_cmd),
        _ => run_dictation(&cli),
    }
}

fn run_llm(cmd: &LlmCmd) -> anyhow::Result<()> {
    match cmd {
        LlmCmd::Use { provider, model } => {
            let settings = isper_llm::LlmSettings {
                provider: provider.to_lowercase(),
                model: model.clone(),
            };
            isper_llm::save_settings(&settings)?;
            println!(
                "provider: {} | modelo: {}",
                settings.provider,
                settings.model.as_deref().unwrap_or("(padrão do provider)")
            );
            if isper_llm::get_api_key(&settings.provider)?.is_none() {
                println!("falta a chave: rode `isper-cli llm set-key {}`", settings.provider);
            }
            Ok(())
        }
        LlmCmd::SetKey { provider } => {
            let provider = provider.to_lowercase();
            let key = rpassword::prompt_password(format!(
                "Cole a chave de API de '{provider}' (não aparece ao digitar): "
            ))?;
            if key.trim().is_empty() {
                anyhow::bail!("chave vazia — nada salvo");
            }
            isper_llm::set_api_key(&provider, &key)?;
            println!("chave de '{provider}' salva no Credential Manager do Windows");
            Ok(())
        }
        LlmCmd::DeleteKey { provider } => {
            isper_llm::delete_api_key(&provider.to_lowercase())?;
            println!("chave removida (se existia)");
            Ok(())
        }
        LlmCmd::Status => {
            let settings = isper_llm::load_settings();
            if settings.provider.is_empty() {
                println!("nenhum provider configurado — `isper-cli llm use <claude|groq|gemini>`");
                return Ok(());
            }
            let has_key = isper_llm::get_api_key(&settings.provider)?.is_some();
            println!(
                "provider: {} | modelo: {} | chave: {}",
                settings.provider,
                settings.model.as_deref().unwrap_or("(padrão do provider)"),
                if has_key { "guardada" } else { "FALTANDO (llm set-key)" }
            );
            Ok(())
        }
        LlmCmd::Test => {
            let settings = isper_llm::load_settings();
            let provider = isper_llm::provider_from_settings(&settings)?;
            println!("testando {} ({})...", provider.name(), provider.model());
            let reply = provider.complete(
                "Você é o teste de conexão do ISPer. Responda em português, em uma linha.",
                "Diga apenas: conexão ok!",
            )?;
            println!("resposta: {}", reply.trim());
            Ok(())
        }
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
        Cmd::Meeting { .. } | Cmd::Llm(_) => unreachable!(),
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

    // Fase 5: resumo por IA, se houver provider configurado.
    let settings = isper_llm::load_settings();
    match isper_llm::provider_from_settings(&settings) {
        Ok(provider) => {
            println!("gerando resumo via {} ({})...", provider.name(), provider.model());
            let transcript = meeting::to_markdown(&title, &started_at, &result);
            match isper_llm::summarize_meeting(provider.as_ref(), &transcript) {
                Ok(summary) => {
                    let block = format!(
                        "\n\n---\n\n{}\n\n> Resumo gerado via {} ({}) — revise antes de usar.\n",
                        summary.trim(),
                        provider.name(),
                        provider.model()
                    );
                    use std::io::Write;
                    std::fs::OpenOptions::new()
                        .append(true)
                        .open(&md_path)?
                        .write_all(block.as_bytes())?;
                    let _ = db.set_summary(id, summary.trim());
                    println!("\n{}", summary.trim());
                }
                Err(e) => println!("resumo falhou (transcript preservado): {e}"),
            }
        }
        Err(isper_llm::LlmError::NotConfigured) => {
            println!("(sem resumo por IA — configure com `isper-cli llm use groq`)");
        }
        Err(e) => println!("resumo indisponível: {e}"),
    }
    Ok(())
}
