//! isper-cli — laboratório do motor: transcrição, reunião, IA e modelos.
//!
//! Uso:
//!   isper-cli rec 5                     # grava 5 s do microfone e transcreve
//!   isper-cli file fala.wav             # transcreve um arquivo WAV
//!   isper-cli meeting 30 --source teams # reunião: mic + só o áudio do Teams
//!   isper-cli models list|download|remove
//!   isper-cli llm use|set-key|status|test|models

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use isper_core::loopback::LoopbackSource;
use isper_core::meeting::{self, MeetingOptions};
use isper_core::{WhisperEngine, audio, store::MeetingStore};

#[derive(Parser)]
#[command(
    name = "isper-cli",
    about = "ISPer — transcrição 100% local com Whisper"
)]
struct Cli {
    /// Caminho de um modelo ggml (padrão: o melhor instalado — ver `models list`)
    #[arg(long)]
    model: Option<PathBuf>,

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
    Meeting {
        seconds: u64,
        /// Fonte dos participantes: system · teams · process:<nome.exe>
        #[arg(long, default_value = "system")]
        source: String,
    },
    /// Identifica os falantes de um WAV (calibração da diarização)
    Diarize { path: PathBuf },
    /// Gerencia os modelos Whisper (Fase 3)
    #[command(subcommand)]
    Models(ModelsCmd),
    /// Configura a inteligência de nuvem (Fase 5)
    #[command(subcommand)]
    Llm(LlmCmd),
}

#[derive(Subcommand)]
enum ModelsCmd {
    /// Lista o catálogo e o que está instalado
    List,
    /// Baixa um modelo do catálogo (com verificação SHA-256)
    Download { file: String },
    /// Remove um modelo instalado
    Remove { file: String },
    /// Baixa os modelos de diarização (quem falou o quê), ~45 MB
    DownloadDiarize,
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
    /// Lista os modelos disponíveis para a sua chave no provider configurado
    Models,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();
    let cli = Cli::parse();

    match &cli.cmd {
        Cmd::Meeting { seconds, source } => run_meeting(&cli, *seconds, source),
        Cmd::Diarize { path } => run_diarize(path),
        Cmd::Models(cmd) => run_models(cmd),
        Cmd::Llm(cmd) => run_llm(cmd),
        _ => run_dictation(&cli),
    }
}

/// Roda só a diarização num WAV — útil para calibrar `ISPER_DIARIZE_THRESHOLD`.
fn run_diarize(path: &PathBuf) -> anyhow::Result<()> {
    let raw = audio::load_wav(path).with_context(|| format!("falha ao ler {}", path.display()))?;
    let secs = raw.duration_secs();
    let samples = raw.into_whisper_input()?;
    println!(
        "diarizando {:.1}s (threshold {})...",
        secs,
        std::env::var("ISPER_DIARIZE_THRESHOLD")
            .unwrap_or_else(|_| isper_diarize::DEFAULT_THRESHOLD.to_string())
    );
    let started = std::time::Instant::now();
    let turns = isper_diarize::diarize(&samples)?;
    let mut speakers: Vec<usize> = turns.iter().map(|t| t.speaker).collect();
    speakers.sort_unstable();
    speakers.dedup();
    for t in &turns {
        println!(
            "[{:>6.1}s -> {:>6.1}s] Participante {}",
            t.start,
            t.end,
            t.speaker + 1
        );
    }
    println!(
        "{} turno(s), {} falante(s) em {:.1}s",
        turns.len(),
        speakers.len(),
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// Em desenvolvimento, o `models/` do repositório também vale como fonte.
fn dev_dirs() -> Vec<PathBuf> {
    std::env::current_dir()
        .map(|cwd| cwd.ancestors().take(4).map(|d| d.to_path_buf()).collect())
        .unwrap_or_default()
}

fn resolve_model(cli: &Cli) -> anyhow::Result<PathBuf> {
    if let Some(p) = &cli.model {
        anyhow::ensure!(p.exists(), "modelo não encontrado: {}", p.display());
        return Ok(p.clone());
    }
    isper_models::resolve_whisper_model(None, cfg!(feature = "cuda"), &dev_dirs()).ok_or_else(
        || {
            anyhow::anyhow!(
                "nenhum modelo instalado — baixe um com `isper-cli models download ggml-small.bin`"
            )
        },
    )
}

fn run_dictation(cli: &Cli) -> anyhow::Result<()> {
    let raw = match &cli.cmd {
        Cmd::Rec { seconds } => {
            println!("== gravando por {seconds}s... fale! ==");
            audio::record(Duration::from_secs(*seconds)).context("falha ao gravar do microfone")?
        }
        Cmd::File { path } => {
            audio::load_wav(path).with_context(|| format!("falha ao ler {}", path.display()))?
        }
        Cmd::Meeting { .. } | Cmd::Diarize { .. } | Cmd::Models(_) | Cmd::Llm(_) => unreachable!(),
    };

    println!(
        "audio: {:.1}s @ {} Hz, {} canal(is) -> convertendo para 16 kHz mono",
        raw.duration_secs(),
        raw.sample_rate,
        raw.channels
    );
    let samples = raw.into_whisper_input()?;

    let model = resolve_model(cli)?;
    println!("carregando modelo {} ...", model.display());
    let engine = WhisperEngine::new(&model)?;

    let t = engine.transcribe(&samples, &cli.lang, None)?;
    println!();
    for seg in &t.segments {
        println!(
            "[{:>6.2}s -> {:>6.2}s] {}",
            seg.start_secs, seg.end_secs, seg.text
        );
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

fn run_meeting(cli: &Cli, seconds: u64, source: &str) -> anyhow::Result<()> {
    let model = resolve_model(cli)?;
    println!("carregando modelo {} ...", model.display());
    let engine = Arc::new(WhisperEngine::new(&model)?);

    let source = LoopbackSource::parse(source);
    println!("== gravando reuniao por {seconds}s (mic = Eu | {source:?} = Participantes) ==");
    let handle = meeting::start(
        engine,
        MeetingOptions {
            lang: cli.lang.clone(),
            initial_prompt: None,
            source,
            input_device: None,
            on_segment: None,
            dictionary: Vec::new(),
        },
    )
    .context("falha ao abrir captura da reunião")?;
    for w in &handle.warnings {
        println!("aviso: {w}");
    }
    std::thread::sleep(Duration::from_secs(seconds));

    println!("encerrando... transcrevendo blocos restantes");
    let mut result = handle.stop()?;

    // Fase 4: quem falou o quê (se os modelos de diarização estiverem instalados).
    if isper_diarize::models_installed() && !result.others_audio_16k.is_empty() {
        println!("identificando falantes...");
        match isper_diarize::diarize(&result.others_audio_f32()) {
            Ok(turns) => {
                let t: Vec<(f32, f32, usize)> =
                    turns.iter().map(|t| (t.start, t.end, t.speaker)).collect();
                result.apply_speaker_turns(&t);
                println!(
                    "  {} participante(s) identificado(s)",
                    result.distinct_participants()
                );
            }
            Err(e) => println!("diarização falhou (rótulos genéricos mantidos): {e}"),
        }
    } else if !isper_diarize::models_installed() {
        println!("(sem diarização — `isper-cli models download-diarize` p/ identificar falantes)");
    }

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
    std::fs::write(
        &md_path,
        meeting::to_markdown(&title, &started_at, &result, &[]),
    )?;

    let db = MeetingStore::open(std::path::Path::new("isper.db"))?;
    let id = db.save(&title, &started_at, &result, Some(&md_path))?;

    println!("salvo: {md_path} | banco: isper.db (reuniao id {id})");

    // Fase 5: resumo por IA, se houver provider configurado.
    let settings = isper_llm::load_settings();
    match isper_llm::provider_from_settings(&settings) {
        Ok(provider) => {
            println!(
                "gerando resumo via {} ({})...",
                provider.name(),
                provider.model()
            );
            let transcript = meeting::to_markdown(&title, &started_at, &result, &[]);
            match isper_llm::summarize_meeting(provider.as_ref(), &transcript) {
                Ok(summary) => {
                    let block = format!(
                        "\n\n---\n\n{}\n\n> Resumo gerado via {} ({}) — revise antes de usar.\n",
                        summary.trim(),
                        provider.name(),
                        provider.model()
                    );
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

fn run_models(cmd: &ModelsCmd) -> anyhow::Result<()> {
    match cmd {
        ModelsCmd::List => {
            println!("pasta: {}", isper_models::models_dir()?.display());
            for m in isper_models::WHISPER_CATALOG {
                let status = match isper_models::installed_path(m.file) {
                    Some(_) => "instalado",
                    None => "-",
                };
                println!(
                    "  {:<32} {:>5} MB  {:<10} {} — {}",
                    m.file, m.approx_mb, status, m.label, m.note
                );
            }
            Ok(())
        }
        ModelsCmd::Download { file } => {
            println!("baixando {file} (com verificação SHA-256 do Hugging Face)...");
            let mut last_pct: i64 = -1;
            let path = isper_models::download_whisper(file, &mut |done, total| {
                if total > 0 {
                    let pct = (done * 100 / total) as i64;
                    if pct != last_pct && pct % 2 == 0 {
                        last_pct = pct;
                        print!(
                            "\r  {pct:>3}%  {:>5} / {:>5} MB",
                            done / 1_000_000,
                            total / 1_000_000
                        );
                        let _ = std::io::stdout().flush();
                    }
                }
            })?;
            println!("\nok: {}", path.display());
            Ok(())
        }
        ModelsCmd::Remove { file } => {
            isper_models::remove(file)?;
            println!("removido (se existia): {file}");
            Ok(())
        }
        ModelsCmd::DownloadDiarize => {
            println!(
                "baixando modelos de diarização para {}...",
                isper_diarize::models_dir()?.display()
            );
            let mut last_pct: i64 = -1;
            isper_diarize::download_models(&mut |name, done, total| {
                if total > 0 {
                    let pct = (done * 100 / total) as i64;
                    if pct != last_pct && pct % 5 == 0 {
                        last_pct = pct;
                        print!("\r  {name}: {pct:>3}%");
                        let _ = std::io::stdout().flush();
                    }
                }
            })?;
            println!("\nok — reuniões passam a identificar 'Participante 1, 2, 3…'");
            Ok(())
        }
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
                println!(
                    "falta a chave: rode `isper-cli llm set-key {}`",
                    settings.provider
                );
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
                if has_key {
                    "guardada"
                } else {
                    "FALTANDO (llm set-key)"
                }
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
        LlmCmd::Models => {
            let settings = isper_llm::load_settings();
            let provider = isper_llm::provider_from_settings(&settings)?;
            let models = provider.list_models()?;
            println!(
                "modelos disponíveis em {} ({}):",
                provider.name(),
                models.len()
            );
            for m in &models {
                let mark = if m == provider.model() {
                    "  <- atual"
                } else {
                    ""
                };
                println!("  {m}{mark}");
            }
            println!("\nuse: isper-cli llm use {} --model <id>", provider.name());
            Ok(())
        }
    }
}
