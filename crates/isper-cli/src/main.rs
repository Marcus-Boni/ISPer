//! isper-cli — laboratório do motor: transcrição, reunião, IA e modelos.
//!
//! Uso:
//!   isper-cli rec 5                     # grava 5 s do microfone e transcreve
//!   isper-cli bench reuniao.wav --config baseline   # mede o pipeline (antes)
//!   isper-cli bench reuniao.wav --config final      # …e depois
//!   isper-cli compare a.report.json b.report.json   # tabela antes/depois
//!   isper-cli file fala.wav             # transcreve um arquivo WAV
//!   isper-cli meeting 30 --source teams # reunião: mic + só o áudio do Teams
//!   isper-cli models list|download|remove
//!   isper-cli llm use|set-key|status|test|models
//!   isper-cli receber pasta --aprovar  # faz de PC para o celular (sincronia)
//!   isper-cli enviar gravacao.opus --codigo 'isper://parear?…'  # faz de celular

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};

mod agente;
mod aprender;
mod bench;
mod opttime;
mod reunioes;
mod sync;
mod tarefas;
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
    /// Transcreve um arquivo de áudio (MP3, M4A, WAV, FLAC, OGG…)
    File { path: PathBuf },
    /// Grava uma reunião (mic = "Eu" + áudio do sistema = "Participantes")
    /// por N segundos, transcreve em blocos e salva Markdown + SQLite
    Meeting {
        seconds: u64,
        /// Fonte dos participantes: system · teams · process:<nome.exe>
        #[arg(long, default_value = "system")]
        source: String,
    },
    /// Converte um áudio para Ogg/Opus mono, como o gravador do celular grava
    Encode {
        /// Áudio de entrada (qualquer formato que o ISPer lê)
        audio: PathBuf,
        /// Arquivo .opus de saída (não é sobrescrito)
        saida: PathBuf,
        /// Taxa de bits, em kbit/s
        #[arg(long, default_value_t = 32)]
        kbps: u32,
    },
    /// Faz de PC para o celular: mostra o QR de pareamento e guarda as
    /// gravações recebidas numa pasta (uma fica "pronta" quando aparece um
    /// `<id>.ata.md` ao lado dela)
    Receber {
        /// Pasta das gravações, dos aparelhos pareados e da chave deste "PC"
        pasta: PathBuf,
        /// Porta UDP
        #[arg(long, default_value_t = isper_sync::DEFAULT_PORT)]
        porta: u16,
        /// Endereço para pôr no código no lugar dos da rede (o emulador
        /// Android chega ao PC por 10.0.2.2); pode repetir
        #[arg(long)]
        anunciar: Vec<std::net::SocketAddr>,
        /// Aprova o pareamento sem perguntar
        #[arg(long)]
        aprovar: bool,
        /// Relay (URL) para quando o celular estiver fora da rede
        #[arg(long)]
        relay: Option<String>,
        /// Quanto o código de pareamento vale, em segundos
        #[arg(long, default_value_t = 120)]
        validade: u64,
    },
    /// Faz de celular: pareia com um PC (pelo código do QR) e manda um áudio
    Enviar {
        /// O áudio a mandar
        arquivo: PathBuf,
        /// O código de pareamento (o texto do QR); sem ele, usa o PC já pareado
        #[arg(long)]
        codigo: Option<String>,
        /// Pasta com a chave e o PC pareado deste "celular"
        #[arg(long)]
        estado: Option<PathBuf>,
        /// Espera a ata por até N segundos e a salva ao lado do áudio
        #[arg(long, default_value_t = 0)]
        esperar: u64,
        /// Nome do aparelho, como o PC mostra
        #[arg(long, default_value = "isper-cli")]
        nome: String,
        /// Id da gravação (padrão: o nome do arquivo)
        #[arg(long)]
        id: Option<String>,
        /// Momentos marcados, em segundos (ex.: 1.5,3)
        #[arg(long, value_delimiter = ',')]
        momentos: Vec<f64>,
    },
    /// Identifica os falantes de um arquivo de áudio (calibração da diarização)
    Diarize {
        path: PathBuf,
        /// Número de participantes, se conhecido (muda o agrupamento de
        /// "descubra quantos" para "corte em exatamente N")
        #[arg(long)]
        speakers: Option<u32>,
        /// Limiar do agrupamento (padrão: 0.5, o do sherpa-onnx)
        #[arg(long)]
        threshold: Option<f32>,
        /// Turnos de referência (início<TAB>fim<TAB>falante), para calcular DER
        #[arg(long)]
        reference_turns: Option<PathBuf>,
        /// Semelhança a partir da qual dois falantes são a mesma voz, na
        /// conferência com a voz inteira (padrão: 0.6; 0 só mede, sem juntar)
        #[arg(long)]
        same_voice: Option<f32>,
        /// Não absorve os grupos fracos — para medir a semelhança entre todos
        /// os grupos que o agrupamento criou
        #[arg(long)]
        no_absorb: bool,
    },
    /// Roda o pipeline inteiro sobre um WAV e grava relatório + transcrições
    Bench {
        /// Áudio da reunião (MP3, M4A, WAV, FLAC, OGG; qualquer taxa e canais)
        audio: PathBuf,
        /// `baseline`, `final` ou o caminho de um JSON de configuração
        #[arg(long, default_value = "final")]
        config: String,
        /// Nome dos arquivos de saída (padrão: o da configuração)
        #[arg(long)]
        name: Option<String>,
        /// Pasta de saída
        #[arg(long, default_value = "bench")]
        out: PathBuf,
        /// Glossário: `.json` de MeetingContext ou `.txt` (um termo por linha)
        #[arg(long)]
        glossary: Option<PathBuf>,
        /// Não roda a diarização (mais rápido quando só o texto importa)
        #[arg(long)]
        no_diarize: bool,
        /// Número de participantes, se conhecido
        #[arg(long)]
        speakers: Option<u32>,
        /// Limiar do agrupamento da diarização
        #[arg(long)]
        diarize_threshold: Option<f32>,
        /// Transcrição corrigida à mão, para calcular WER e CER
        #[arg(long)]
        reference: Option<PathBuf>,
        /// Turnos de referência (início<TAB>fim<TAB>falante), para calcular DER
        #[arg(long)]
        reference_turns: Option<PathBuf>,
    },
    /// Compara dois relatórios de `bench` lado a lado
    Compare { before: PathBuf, after: PathBuf },
    /// Reconstrói a Biblioteca a partir dos Markdowns das reuniões
    ///
    /// O `.md` é o artefato durável: ele é gravado ANTES do banco e sobrevive
    /// a qualquer acidente com o índice. Este comando lê a pasta de volta e
    /// reinsere o que faltar. É seguro repetir — o que já está no banco é
    /// pulado.
    Import {
        /// Pasta com os `.md` (padrão: Documentos\ISPer\Reunioes)
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Banco a corrigir (padrão: o do app, %APPDATA%\ISPer\isper.db)
        #[arg(long)]
        db: Option<PathBuf>,
        /// Grava de verdade. Sem isto, só mostra o que faria.
        #[arg(long)]
        apply: bool,
    },
    /// WER/CER entre dois arquivos de texto
    Score {
        /// Transcrição de referência (corrigida à mão)
        #[arg(long)]
        reference: PathBuf,
        /// Transcrição a avaliar
        #[arg(long)]
        hypothesis: PathBuf,
        /// Ignorar acentos na comparação
        #[arg(long)]
        strip_accents: bool,
    },
    /// Gerencia os modelos Whisper (Fase 3)
    #[command(subcommand)]
    Models(ModelsCmd),
    /// Configura a inteligência de nuvem (Fase 5)
    #[command(subcommand)]
    Llm(LlmCmd),
    /// Tarefas da fala: extrair, montar e medir o corpus (Fase 10.1)
    #[command(subcommand)]
    Tarefas(TarefasCmd),
    /// O conector do OptTime, só leitura (Fase 10.2); o token vem do
    /// Credential Manager (`isper-cli llm set-key opttime`)
    #[command(subcommand)]
    Opttime(OpttimeCmd),
    /// Reuniões gravadas: as ações de "Eu", sem gravar nada (Fase 10.3)
    #[command(subcommand)]
    Reunioes(ReunioesCmd),
    /// O agente: perguntas livres sobre o dia, resumo da manhã e fechamento
    /// (Fase 10.4)
    #[command(subcommand)]
    Agente(AgenteCmd),
    /// A memória do assistente: os fatos que ele lê antes de responder
    /// (Fase 10.5)
    #[command(subcommand)]
    Memoria(MemoriaCmd),
}

#[derive(Subcommand)]
enum MemoriaCmd {
    /// As memórias ativas (ou as arquivadas)
    Listar {
        /// As arquivadas em vez das ativas
        #[arg(long)]
        arquivadas: bool,
        /// Banco (padrão: o do app, %APPDATA%\\ISPer\\isper.db)
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Guarda um fato curto
    Guardar {
        /// O fato, entre aspas
        texto: String,
        /// É uma preferência, não um fato
        #[arg(long)]
        preferencia: bool,
        /// Banco (padrão: o do app)
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Arquiva (sai do assistente, mas não some)
    Arquivar {
        /// O id (de `memoria listar`)
        id: String,
        /// Banco (padrão: o do app)
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Restaura uma arquivada
    Restaurar {
        /// O id (de `memoria listar --arquivadas`)
        id: String,
        /// Banco (padrão: o do app)
        #[arg(long)]
        db: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum AgenteCmd {
    /// Monta um banco sintético (tarefas, rotina, reuniões e ditado
    /// inventados) para testar o agente sem dado real
    Semear {
        /// O banco novo (não pode existir)
        #[arg(long)]
        db: PathBuf,
    },
    /// Uma pergunta ao agente ("manha" e "fechamento" pedem o resumo da
    /// manhã e o fechamento do dia)
    Perguntar {
        /// A pergunta, entre aspas
        pergunta: String,
        /// Banco (padrão: o do app, %APPDATA%\ISPer\isper.db)
        #[arg(long)]
        db: Option<PathBuf>,
        /// Provider (claude, groq, gemini); padrão: o do llm.toml
        #[arg(long)]
        provider: Option<String>,
        /// Modelo; padrão: o do provider
        #[arg(long)]
        model: Option<String>,
        /// Aprova sozinho as ações que pedem o toque (só num banco de teste)
        #[arg(long)]
        aprovar: bool,
        /// Sem as ferramentas do OptTime, mesmo com token
        #[arg(long)]
        sem_opttime: bool,
        /// O nome de quem usa (o agente chama a pessoa por ele)
        #[arg(long)]
        eu: Option<String>,
    },
    /// Mede o agente num conjunto de perguntas (.jsonl com pergunta,
    /// deve_conter e deve_citar)
    Avaliar {
        /// O arquivo de perguntas
        arquivo: PathBuf,
        /// Banco (padrão: o do app)
        #[arg(long)]
        db: Option<PathBuf>,
        /// Provider; padrão: o do llm.toml
        #[arg(long)]
        provider: Option<String>,
        /// Modelo; padrão: o do provider
        #[arg(long)]
        model: Option<String>,
        /// Sem as ferramentas do OptTime
        #[arg(long)]
        sem_opttime: bool,
        /// O nome de quem usa
        #[arg(long)]
        eu: Option<String>,
        /// Relatório JSON pergunta a pergunta
        #[arg(long)]
        saida: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ReunioesCmd {
    /// As ações de "Eu" numa reunião do banco, como a caixa de entrada as
    /// receberia (só leitura; o texto vai ao provider configurado)
    Acoes {
        /// Id da reunião (a Biblioteca mostra; `isper-cli` lista no banco)
        id: i64,
        /// Provider (claude, groq, gemini); padrão: o do llm.toml
        #[arg(long)]
        provider: Option<String>,
        /// Modelo; padrão: o do provider
        #[arg(long)]
        model: Option<String>,
        /// O nome de quem gravou, como os outros o chamam
        #[arg(long)]
        eu: Option<String>,
        /// Banco (padrão: o do app, %APPDATA%\ISPer\isper.db)
        #[arg(long)]
        db: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum OpttimeCmd {
    /// Quem é o dono do token, os escopos e o que está conectado
    QuemSou {
        /// Outro servidor (padrão: o OptTime de produção)
        #[arg(long)]
        url: Option<String>,
    },
    /// O resumo de horas de um dia e quanto falta para a meta
    Dia {
        /// Dia (AAAA-MM-DD; padrão: hoje)
        #[arg(long)]
        data: Option<String>,
        /// Outro servidor (padrão: o OptTime de produção)
        #[arg(long)]
        url: Option<String>,
    },
    /// As sugestões do "Preencher meu dia", sem aplicar nada
    Sugestoes {
        /// Dia (AAAA-MM-DD; padrão: hoje)
        #[arg(long)]
        data: Option<String>,
        /// Outro servidor (padrão: o OptTime de produção)
        #[arg(long)]
        url: Option<String>,
    },
    /// A agenda do Outlook, pelo OptTime
    Agenda {
        /// Primeiro dia (AAAA-MM-DD; padrão: hoje)
        #[arg(long)]
        data: Option<String>,
        /// Quantos dias (1 a 7)
        #[arg(long, default_value_t = 1)]
        dias: u8,
        /// Outro servidor (padrão: o OptTime de produção)
        #[arg(long)]
        url: Option<String>,
    },
    /// O catálogo de ferramentas, marcando o que lê, escreve ou apaga
    Ferramentas {
        /// Outro servidor (padrão: o OptTime de produção)
        #[arg(long)]
        url: Option<String>,
    },
}

#[derive(Subcommand)]
enum TarefasCmd {
    /// Separa as tarefas de uma fala e mostra as datas resolvidas
    Extrair {
        /// A fala, entre aspas
        fala: String,
        /// Provider (claude, groq, gemini); padrão: o do llm.toml
        #[arg(long)]
        provider: Option<String>,
        /// Modelo; padrão: o do provider
        #[arg(long)]
        model: Option<String>,
        /// Dia de referência para "amanhã", "sexta"… (AAAA-MM-DD; padrão: hoje)
        #[arg(long)]
        hoje: Option<String>,
    },
    /// Tira do banco os ditados desde um dia, sem rótulo, para o corpus
    CorpusDitados {
        /// Primeiro dia (AAAA-MM-DD)
        #[arg(long)]
        desde: String,
        /// Arquivo .jsonl novo (fora do repositório: tem fala de verdade)
        #[arg(long)]
        saida: PathBuf,
        /// Banco (padrão: o do app, %APPDATA%\ISPer\isper.db)
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// As rotinas sugeridas: tarefas que se repetem no mesmo dia da semana
    /// (3 das últimas 4 semanas), com a evidência; só lê (Fase 10.5)
    Sugestoes {
        /// Banco (padrão: o do app, %APPDATA%\\ISPer\\isper.db)
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Mede a extração num corpus rotulado (precisão, recall, datas, latência)
    Avaliar {
        /// O corpus .jsonl
        corpus: PathBuf,
        /// Provider (claude, groq, gemini); padrão: o do llm.toml
        #[arg(long)]
        provider: Option<String>,
        /// Modelo; padrão: o do provider
        #[arg(long)]
        model: Option<String>,
        /// Relatório JSON fala a fala
        #[arg(long)]
        saida: Option<PathBuf>,
        /// Só as N primeiras falas
        #[arg(long)]
        limite: Option<usize>,
    },
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
    // O whisper.cpp e o ggml agora falam pelo `tracing` (ver
    // `isper_core::engine`): úteis ao depurar, ruído no uso normal. Ficam em
    // WARN, e `RUST_LOG=whisper_rs=info` traz tudo de volta.
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                // O rmcp descreve o servidor inteiro a cada conexão, em INFO.
                .unwrap_or_else(|_| {
                    tracing_subscriber::EnvFilter::new("info,whisper_rs=warn,rmcp=warn")
                }),
        )
        .init();
    let cli = Cli::parse();

    match &cli.cmd {
        Cmd::Meeting { seconds, source } => run_meeting(&cli, *seconds, source),
        Cmd::Encode { audio, saida, kbps } => run_encode(audio, saida, *kbps),
        Cmd::Receber {
            pasta,
            porta,
            anunciar,
            aprovar,
            relay,
            validade,
        } => sync::receive(
            pasta,
            *porta,
            anunciar,
            *aprovar,
            relay.as_deref(),
            *validade,
        ),
        Cmd::Enviar {
            arquivo,
            codigo,
            estado,
            esperar,
            nome,
            id,
            momentos,
        } => {
            let default_state = std::env::temp_dir().join("isper-cli-celular");
            sync::send(sync::SendArgs {
                file: arquivo,
                code: codigo.as_deref(),
                state_dir: estado.as_deref().unwrap_or(&default_state),
                wait_secs: *esperar,
                name: nome,
                id: id.as_deref(),
                moments: momentos.clone(),
            })
        }
        Cmd::Diarize {
            path,
            speakers,
            threshold,
            reference_turns,
            same_voice,
            no_absorb,
        } => {
            let mut opts = isper_diarize::DiarizeOptions::from_env();
            if let Some(t) = threshold {
                opts.threshold = *t;
            }
            if speakers.is_some() {
                opts.num_speakers = *speakers;
            }
            if let Some(s) = same_voice {
                opts.same_voice_similarity = *s;
            }
            if *no_absorb {
                opts.min_speaker_secs = 0.0;
                opts.min_speaker_share = 0.0;
                opts.min_speaker_turns = 0;
            }
            run_diarize(path, &opts, reference_turns.as_deref())
        }
        Cmd::Bench { .. } => run_bench(&cli),
        Cmd::Compare { before, after } => bench::compare(before, after),
        Cmd::Import { dir, db, apply } => run_import(dir.as_deref(), db.as_deref(), *apply),
        Cmd::Score {
            reference,
            hypothesis,
            strip_accents,
        } => bench::score(reference, hypothesis, *strip_accents),
        Cmd::Models(cmd) => run_models(cmd),
        Cmd::Llm(cmd) => run_llm(cmd),
        Cmd::Tarefas(cmd) => run_tarefas(cmd),
        Cmd::Opttime(cmd) => run_opttime(cmd),
        Cmd::Agente(cmd) => run_agente(cmd),
        Cmd::Memoria(cmd) => run_memoria(cmd),
        Cmd::Reunioes(ReunioesCmd::Acoes {
            id,
            provider,
            model,
            eu,
            db,
        }) => {
            let db = match db {
                Some(p) => p.clone(),
                None => default_db().context("pasta de dados do usuário indisponível")?,
            };
            reunioes::acoes(
                &db,
                *id,
                provider.as_deref(),
                model.as_deref(),
                eu.as_deref(),
            )
        }
        _ => run_dictation(&cli),
    }
}

/// O perfil de dados de teste (`ISPER_PROFILE_DIR`), como no app: com ele, as
/// pastas padrão abaixo são as do perfil, e não as do usuário.
fn profile_dir() -> Option<PathBuf> {
    std::env::var_os("ISPER_PROFILE_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Pasta padrão dos Markdowns: `Documentos\ISPer\Reunioes`.
fn default_meetings_dir() -> Option<PathBuf> {
    let home = profile_dir().or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))?;
    Some(home.join("Documents").join("ISPer").join("Reunioes"))
}

/// Banco padrão do app: `%APPDATA%\ISPer\isper.db`.
fn default_db() -> Option<PathBuf> {
    let appdata = match profile_dir() {
        Some(profile) => profile.join("AppData").join("Roaming"),
        None => std::env::var_os("APPDATA").map(PathBuf::from)?,
    };
    Some(appdata.join("ISPer").join("isper.db"))
}

fn run_import(dir: Option<&Path>, db: Option<&Path>, apply: bool) -> anyhow::Result<()> {
    let dir = dir
        .map(Path::to_path_buf)
        .or_else(default_meetings_dir)
        .context("não achei a pasta de reuniões — passe --dir")?;
    let db = db
        .map(Path::to_path_buf)
        .or_else(default_db)
        .context("não achei o banco do app — passe --db")?;
    anyhow::ensure!(dir.is_dir(), "pasta não encontrada: {}", dir.display());
    anyhow::ensure!(db.is_file(), "banco não encontrado: {}", db.display());
    println!("pasta: {}", dir.display());
    println!("banco: {}", db.display());
    if !apply {
        println!(
            "(simulação — nada é gravado; use --apply para valer)
"
        );
    }

    let mut arquivos: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    arquivos.sort();

    let store = MeetingStore::open(&db)?;
    let (mut novas, mut existentes, mut falhas) = (0usize, 0usize, 0usize);
    for path in &arquivos {
        let nome = path.file_name().unwrap_or_default().to_string_lossy();
        let parsed = match isper_core::import::parse_file(path) {
            Ok(Ok(m)) => m,
            Ok(Err(e)) => {
                println!("  ! {nome}: {e}");
                falhas += 1;
                continue;
            }
            Err(e) => {
                println!("  ! {nome}: não consegui ler ({e})");
                falhas += 1;
                continue;
            }
        };
        let caminho = path.to_string_lossy().to_string();
        if store.has_meeting_from(&caminho, &parsed.started_at, &parsed.title)? {
            existentes += 1;
            continue;
        }
        println!(
            "  + {} · {} · {} falas{}",
            parsed.started_at,
            parsed.title,
            parsed.segments.len(),
            if parsed.summary.is_some() {
                " · com resumo"
            } else {
                ""
            }
        );
        if apply {
            store.import_meeting(&parsed, &caminho)?;
        }
        novas += 1;
    }

    println!();
    println!(
        "{} arquivo(s) · {} {} · {} já no banco · {} com problema",
        arquivos.len(),
        novas,
        if apply {
            "reimportada(s)"
        } else {
            "a reimportar"
        },
        existentes,
        falhas
    );
    if !apply && novas > 0 {
        println!(
            "
Para gravar: isper-cli import --apply"
        );
    }
    Ok(())
}

fn run_bench(cli: &Cli) -> anyhow::Result<()> {
    let Cmd::Bench {
        audio,
        config,
        name,
        out,
        glossary,
        no_diarize,
        speakers,
        diarize_threshold,
        reference,
        reference_turns,
    } = &cli.cmd
    else {
        unreachable!("run_bench só é chamado para Cmd::Bench")
    };
    bench::run(&bench::BenchArgs {
        audio: audio.clone(),
        config: config.clone(),
        name: name.clone(),
        out_dir: out.clone(),
        model: resolve_model(cli)?,
        lang: cli.lang.clone(),
        glossary: glossary.clone(),
        diarize: !no_diarize,
        speakers: *speakers,
        diarize_threshold: *diarize_threshold,
        reference: reference.clone(),
        reference_turns: reference_turns.clone(),
    })?;
    Ok(())
}

/// Roda só a diarização num arquivo de áudio — útil para calibrar o limiar
/// do agrupamento.
fn run_encode(audio: &Path, saida: &Path, kbps: u32) -> anyhow::Result<()> {
    let decoded = isper_core::decode::decode_to_16k(audio, None, None)
        .with_context(|| format!("falha ao ler {}", audio.display()))?;
    let pcm: Vec<i16> = decoded
        .samples_16k
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect();
    let mut w = isper_core::ogg_opus::OggOpusWriter::create(saida, 16_000, kbps as i32 * 1000)
        .with_context(|| format!("falha ao criar {}", saida.display()))?;
    w.write(&pcm)?;
    let secs = w.finish()?;
    let bytes = std::fs::metadata(saida)?.len();
    println!(
        "{} → {}: {:.1} s, {:.1} KB ({:.1} kbit/s efetivos, {:.1} MB por hora)",
        audio.display(),
        saida.display(),
        secs,
        bytes as f64 / 1024.0,
        bytes as f64 * 8.0 / secs / 1000.0,
        bytes as f64 / secs * 3600.0 / 1_000_000.0
    );
    Ok(())
}

fn run_diarize(
    path: &Path,
    opts: &isper_diarize::DiarizeOptions,
    reference_turns: Option<&Path>,
) -> anyhow::Result<()> {
    let decoded = isper_core::decode::decode_to_16k(path, None, None)
        .with_context(|| format!("falha ao ler {}", path.display()))?;
    let secs = decoded.duration_secs();
    let samples = decoded.samples_16k;
    match opts.num_speakers {
        Some(n) => println!("diarizando {secs:.1}s com {n} falante(s) conhecido(s)..."),
        None => println!("diarizando {:.1}s (limiar {})...", secs, opts.threshold),
    }
    let out = isper_diarize::diarize_with(&samples, opts)?;
    for t in &out.turns {
        println!(
            "[{:>6.1}s -> {:>6.1}s] Participante {}",
            t.start,
            t.end,
            t.speaker + 1
        );
    }
    let m = &out.metrics;
    println!(
        "{} grupo(s) bruto(s) → {} falante(s) ({} absorvido(s), {} pela voz) · {} turno(s) · mediana {:.1}s · curtos {} · {:.1}s",
        m.raw_clusters,
        m.speakers,
        m.absorbed_clusters,
        m.merged_same_voice,
        m.turns,
        m.median_turn_secs,
        m.very_short_turns,
        m.elapsed_secs
    );
    if !m.voice_similarities.is_empty() {
        // Os números são os de antes da conferência juntar alguém.
        let pares: Vec<String> = m
            .voice_similarities
            .iter()
            .map(|(a, b, s)| format!("{}↔{} {:.2}", a + 1, b + 1, s))
            .collect();
        println!(
            "conferência das vozes (limiar {}): {} → {} juntado(s)",
            opts.same_voice_similarity,
            pares.join(" · "),
            m.merged_same_voice
        );
    }
    for w in &out.warnings {
        println!("AVISO: {w}");
    }
    if let Some(reference) = reference_turns {
        let turnos = bench::read_turns(reference)?;
        let hipotese: Vec<isper_core::align::SpeakerTurn> = out
            .turns
            .iter()
            .map(|t| isper_core::align::SpeakerTurn {
                start_secs: t.start,
                end_secs: t.end,
                speaker: t.speaker,
            })
            .collect();
        let d = isper_core::metrics::der(&turnos, &hipotese);
        println!(
            "DER {:.2}% (omissão {:.1}s · falso alarme {:.1}s · confusão {:.1}s de {:.1}s)",
            d.rate * 100.0,
            d.missed_secs,
            d.false_alarm_secs,
            d.confusion_secs,
            d.reference_secs
        );
    }
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
            // Qualquer formato: o arquivo já sai em 16 kHz mono.
            let decoded = isper_core::decode::decode_to_16k(path, None, None)
                .with_context(|| format!("falha ao ler {}", path.display()))?;
            println!(
                "arquivo: {} Hz, {} canal(is)",
                decoded.source_rate, decoded.source_channels
            );
            audio::RawAudio {
                samples: decoded.samples_16k,
                sample_rate: isper_core::WHISPER_SAMPLE_RATE,
                channels: 1,
            }
        }
        outro => unreachable!(
            "run_dictation não trata {}",
            std::any::type_name_of_val(outro)
        ),
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
            on_partial: None,
            on_block: None,
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
    if isper_diarize::models_installed() && !result.others_audio.is_empty() {
        println!("identificando falantes...");
        let outcome = result
            .others_audio_f32()
            .map_err(|e| e.to_string())
            .and_then(|audio| {
                isper_diarize::diarize_with(&audio, &isper_diarize::DiarizeOptions::from_env())
                    .map_err(|e| e.to_string())
            });
        match outcome {
            Ok(out) => {
                for w in &out.warnings {
                    println!("  AVISO: {w}");
                }
                let t: Vec<isper_core::align::SpeakerTurn> = out
                    .turns
                    .iter()
                    .map(|t| isper_core::align::SpeakerTurn {
                        start_secs: t.start,
                        end_secs: t.end,
                        speaker: t.speaker,
                    })
                    .collect();
                result.apply_speaker_turns(&t);
                println!(
                    "  {} participante(s) identificado(s) de {} grupo(s) bruto(s)",
                    result.distinct_participants(),
                    out.metrics.raw_clusters
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

    let db = MeetingStore::open(Path::new("isper.db"))?;
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
                let Some(pct) = (done * 100).checked_div(total) else {
                    return;
                };
                let pct = pct as i64;
                if pct != last_pct && pct % 2 == 0 {
                    last_pct = pct;
                    print!(
                        "\r  {pct:>3}%  {:>5} / {:>5} MB",
                        done / 1_000_000,
                        total / 1_000_000
                    );
                    let _ = std::io::stdout().flush();
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
                let Some(pct) = (done * 100).checked_div(total) else {
                    return;
                };
                let pct = pct as i64;
                if pct != last_pct && pct % 5 == 0 {
                    last_pct = pct;
                    print!("\r  {name}: {pct:>3}%");
                    let _ = std::io::stdout().flush();
                }
            })?;
            println!("\nok — reuniões passam a identificar 'Participante 1, 2, 3…'");
            Ok(())
        }
    }
}

fn run_tarefas(cmd: &TarefasCmd) -> anyhow::Result<()> {
    match cmd {
        TarefasCmd::Extrair {
            fala,
            provider,
            model,
            hoje,
        } => tarefas::extrair(fala, provider.as_deref(), model.as_deref(), hoje.as_deref()),
        TarefasCmd::CorpusDitados { desde, saida, db } => {
            let db = match db {
                Some(p) => p.clone(),
                None => default_db().context("pasta de dados do usuário indisponível")?,
            };
            tarefas::corpus_ditados(&db, desde, saida)
        }
        TarefasCmd::Sugestoes { db } => aprender::sugestoes(&db_or_default(db)?),
        TarefasCmd::Avaliar {
            corpus,
            provider,
            model,
            saida,
            limite,
        } => tarefas::avaliar(
            corpus,
            provider.as_deref(),
            model.as_deref(),
            saida.as_deref(),
            *limite,
        ),
    }
}

/// O banco informado ou o do app.
fn db_or_default(db: &Option<PathBuf>) -> anyhow::Result<PathBuf> {
    match db {
        Some(p) => Ok(p.clone()),
        None => default_db().context("pasta de dados do usuário indisponível"),
    }
}

fn run_memoria(cmd: &MemoriaCmd) -> anyhow::Result<()> {
    match cmd {
        MemoriaCmd::Listar { arquivadas, db } => aprender::listar(&db_or_default(db)?, *arquivadas),
        MemoriaCmd::Guardar {
            texto,
            preferencia,
            db,
        } => aprender::guardar(&db_or_default(db)?, texto, *preferencia),
        MemoriaCmd::Arquivar { id, db } => aprender::arquivar(&db_or_default(db)?, id, true),
        MemoriaCmd::Restaurar { id, db } => aprender::arquivar(&db_or_default(db)?, id, false),
    }
}

fn run_agente(cmd: &AgenteCmd) -> anyhow::Result<()> {
    match cmd {
        AgenteCmd::Semear { db } => agente::semear(db),
        AgenteCmd::Perguntar {
            pergunta,
            db,
            provider,
            model,
            aprovar,
            sem_opttime,
            eu,
        } => agente::perguntar(
            &db_or_default(db)?,
            pergunta,
            provider.as_deref(),
            model.as_deref(),
            *aprovar,
            !*sem_opttime,
            eu.as_deref(),
        ),
        AgenteCmd::Avaliar {
            arquivo,
            db,
            provider,
            model,
            sem_opttime,
            eu,
            saida,
        } => agente::avaliar(
            &db_or_default(db)?,
            arquivo,
            provider.as_deref(),
            model.as_deref(),
            !*sem_opttime,
            eu.as_deref(),
            saida.as_deref(),
        ),
    }
}

fn run_opttime(cmd: &OpttimeCmd) -> anyhow::Result<()> {
    match cmd {
        OpttimeCmd::QuemSou { url } => opttime::quem_sou(url.as_deref()),
        OpttimeCmd::Dia { data, url } => opttime::dia(data.as_deref(), url.as_deref()),
        OpttimeCmd::Sugestoes { data, url } => opttime::sugestoes(data.as_deref(), url.as_deref()),
        OpttimeCmd::Agenda { data, dias, url } => {
            opttime::agenda(data.as_deref(), *dias, url.as_deref())
        }
        OpttimeCmd::Ferramentas { url } => opttime::ferramentas(url.as_deref()),
    }
}

fn run_llm(cmd: &LlmCmd) -> anyhow::Result<()> {
    match cmd {
        LlmCmd::Use { provider, model } => {
            let settings = isper_llm::LlmSettings {
                provider: provider.to_lowercase(),
                model: model.clone(),
                // Preserva o endereço e a busca semântica configurados pelo app.
                base_url: isper_llm::load_settings().base_url,
                embeddings: isper_llm::load_settings().embeddings,
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// A checagem que o próprio clap recomenda: nomes, conflitos e valores
    /// padrão consistentes — falha em tempo de teste, não na mão do usuário.
    #[test]
    fn definicao_da_cli_e_consistente() {
        Cli::command().debug_assert();
    }

    #[test]
    fn subcomandos_e_opcoes_sao_reconhecidos() {
        let cli = Cli::try_parse_from([
            "isper-cli",
            "--lang",
            "en",
            "meeting",
            "30",
            "--source",
            "teams",
        ])
        .unwrap();
        assert_eq!(cli.lang, "en");
        assert!(cli.model.is_none());
        match &cli.cmd {
            Cmd::Meeting { seconds, source } => {
                assert_eq!(*seconds, 30);
                assert_eq!(source, "teams");
                assert_eq!(LoopbackSource::parse(source), LoopbackSource::teams());
            }
            _ => panic!("esperava o subcomando meeting"),
        }

        let cli = Cli::try_parse_from(["isper-cli", "rec", "5"]).unwrap();
        assert_eq!(cli.lang, "pt", "idioma padrão");
        assert!(matches!(cli.cmd, Cmd::Rec { seconds: 5 }));

        let cli = Cli::try_parse_from([
            "isper-cli",
            "llm",
            "use",
            "groq",
            "--model",
            "openai/gpt-oss-120b",
        ])
        .unwrap();
        assert!(matches!(
            cli.cmd,
            Cmd::Llm(LlmCmd::Use { ref provider, ref model })
                if provider == "groq" && model.as_deref() == Some("openai/gpt-oss-120b")
        ));

        let cli =
            Cli::try_parse_from(["isper-cli", "models", "download", "ggml-small.bin"]).unwrap();
        assert!(
            matches!(cli.cmd, Cmd::Models(ModelsCmd::Download { ref file }) if file == "ggml-small.bin")
        );

        let cli = Cli::try_parse_from([
            "isper-cli",
            "--model",
            "C:/modelos/x.bin",
            "file",
            "fala.wav",
        ])
        .unwrap();
        assert_eq!(cli.model.as_deref(), Some(Path::new("C:/modelos/x.bin")));
        assert!(matches!(cli.cmd, Cmd::File { ref path } if path == Path::new("fala.wav")));
    }

    #[test]
    fn argumentos_obrigatorios_faltando_dao_erro_de_uso() {
        assert!(
            Cli::try_parse_from(["isper-cli"]).is_err(),
            "sem subcomando"
        );
        assert!(
            Cli::try_parse_from(["isper-cli", "meeting"]).is_err(),
            "sem segundos"
        );
        assert!(
            Cli::try_parse_from(["isper-cli", "models", "download"]).is_err(),
            "sem arquivo"
        );
        assert!(
            Cli::try_parse_from(["isper-cli", "llm", "set-key"]).is_err(),
            "sem provider"
        );
        assert!(
            Cli::try_parse_from(["isper-cli", "rec", "cinco"]).is_err(),
            "segundos não numéricos"
        );
    }

    #[test]
    fn dev_dirs_comeca_no_diretorio_atual() {
        let dirs = dev_dirs();
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(dirs.first(), Some(&cwd));
        assert!(dirs.len() <= 4);
    }
}
