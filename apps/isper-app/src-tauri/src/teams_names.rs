//! Nomes da chamada do Teams ([ADR 0027]): enquanto a reunião grava, um
//! subprocesso lê a janela da chamada do Teams pelo UI Automation; quando o
//! passe final termina, cada fala dos participantes que a leitura sustenta
//! ganha o nome que o Teams mostrava. Desligado por padrão.
//!
//! ```text
//!  meeting::start ──► CaptureBinding (PID do Teams gravado, geração)
//!        │                      │
//!        ▼                      ▼
//!  TeamsNamesSession ── supervisor ──► isper-app.exe --isper-teams-reader
//!   (junto da gravação)    │  (Job Object: morre com o app)   │ stdout: uma
//!        │                 │                                  │ linha JSON
//!        │                 └──◄── leitor de linhas ◄──────────┘ por leitura
//!        ▼ seal() ao parar        (QPC → relógio da reunião)
//!  SealedEvidence ──► final_pass ──► speaker_names::attribute ──► banco
//! ```
//!
//! O que esta parte garante (contrato de implementação, com testes):
//!
//! - **O leitor só lê.** O subprocesso recebe o PID raiz por argumento e um
//!   ambiente vazio (nada de perfil, chave de API ou caminho do banco);
//!   não abre o banco, a configuração, o log nem a rede, e só escreve
//!   leituras já interpretadas — quem fala, quem está, estado — no próprio
//!   stdout, um pipe anônimo que só o app lê. Nunca clica nem foca nada.
//! - **Não segura a gravação.** Ele roda noutro processo, num Job Object
//!   com limite de memória que o mata quando o app fecha (ou cai). Travou?
//!   Depois de [`QUIET`] sem leitura, é morto e substituído; a gravação e a
//!   interface nem percebem. Parar a sessão não espera ninguém.
//! - **Mesmo relógio.** O leitor carimba cada leitura com o
//!   QueryPerformanceCounter; o app converte para segundos da reunião com o
//!   mesmo contador, que é o do `Instant` em que a gravação começou.
//! - **Mesmo Teams.** Só se lê a árvore de processos cujo áudio está sendo
//!   gravado. Se o loopback cai para o sistema, reabre noutro PID ou o
//!   leitor recomeça, as leituras dos segundos em volta são descartadas e
//!   uma lacuna entra no lugar.
//! - **Nada no log.** Só contagens, latência e motivos de descarte. Os
//!   nomes ficam em memória até a publicação (ou o descarte).
//! - **Desligar vale na hora.** Desligar a opção mata o leitor e invalida a
//!   evidência das gravações ativas e das que esperam o passe final — mesmo
//!   que a opção seja ligada de novo antes do passe acabar.
//!
//! [ADR 0027]: ../../../../docs/adr/0027-nomes-da-chamada-do-teams.md

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use isper_core::loopback::CaptureBinding;
use isper_core::speaker_names::{
    self, CallReading, Observation, ObservationState, RemoteSegment, Rules,
};
use serde::{Deserialize, Serialize};

use crate::prelude::*;

/// Primeiro argumento do subprocesso leitor (ver `main`).
pub(crate) const READER_FLAG: &str = "--isper-teams-reader";
/// Teste ponta a ponta: um roteiro de leituras falsas (JSONL) no lugar do
/// UI Automation. Só vale com o perfil de teste (`ISPER_PROFILE_DIR`).
pub(crate) const SCRIPT_ENV: &str = "ISPER_TEAMS_READER_SCRIPT";

/// Intervalo entre leituras (um app lento é lido menos vezes).
const EVERY_MS: u64 = 500;
/// Sem nenhuma linha por isto: o leitor travou e é trocado.
const QUIET: Duration = Duration::from_secs(5);
/// Quantas vezes o leitor pode ser trocado numa gravação.
const MAX_RESTARTS: u32 = 10;
/// Uma linha maior que isto é descartada (uma leitura normal tem < 2 KB).
const MAX_LINE: usize = 64 * 1024;
/// Leituras guardadas por gravação, no máximo (6 h a 2 por segundo).
const MAX_OBSERVATIONS: usize = 43_200;
/// Uma leitura que chega mais atrasada que isto não vale.
const MAX_DELAY_SECS: f64 = 5.0;
/// Quando o vínculo com o áudio muda, as leituras destes últimos segundos
/// também saem: o loopback só percebe a troca depois que ela aconteceu.
const UNBIND_MARGIN_SECS: f64 = 2.0;
/// O leitor sai sozinho depois disto, mesmo que ninguém o feche.
const READER_MAX: Duration = Duration::from_secs(12 * 3600);
/// Memória máxima do leitor (o Job Object recusa além disto).
const READER_MEMORY: usize = 512 * 1024 * 1024;

/// Sobe a cada vez que a opção é desligada: evidência de uma época anterior
/// não vira nome, mesmo que a opção volte a ser ligada.
static CONSENT_EPOCH: AtomicU64 = AtomicU64::new(0);

/// A opção foi desligada: para a coleta e invalida o que foi coletado.
pub(crate) fn revoke() {
    CONSENT_EPOCH.fetch_add(1, Ordering::SeqCst);
    tracing::info!("nomes do Teams desligados: coleta parada e evidência pendente descartada");
}

fn epoch() -> u64 {
    CONSENT_EPOCH.load(Ordering::SeqCst)
}

// ------------------------------------------------------------ relógio

#[cfg(windows)]
fn qpc_now() -> i64 {
    let mut v = 0i64;
    // SAFETY: escreve um i64 no ponteiro dado; nunca falha desde o XP.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut v) };
    v
}

#[cfg(windows)]
fn qpc_freq() -> i64 {
    let mut v = 0i64;
    // SAFETY: idem; a frequência é fixa desde o boot.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut v) };
    v.max(1)
}

#[cfg(not(windows))]
fn qpc_now() -> i64 {
    // Fora do Windows não há leitor; o relógio só existe para os testes.
    static ZERO: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    ZERO.get_or_init(Instant::now).elapsed().as_nanos() as i64
}

#[cfg(not(windows))]
fn qpc_freq() -> i64 {
    1_000_000_000
}

/// Segundos da reunião de uma leitura carimbada com `qpc`, sabendo que
/// agora são `now_qpc` e `since_start` desde o início da gravação. `None`
/// se a leitura veio do futuro ou atrasada demais.
fn meeting_secs(qpc: i64, now_qpc: i64, freq: i64, since_start: f64) -> Option<f64> {
    let age = (now_qpc - qpc) as f64 / freq as f64;
    if !(0.0..=MAX_DELAY_SECS).contains(&age) {
        return None;
    }
    let at = since_start - age;
    (at >= 0.0).then_some(at)
}

// ------------------------------------------------------------ protocolo

/// Uma linha do leitor para o app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ReaderLine {
    /// QueryPerformanceCounter no meio da leitura.
    qpc: i64,
    state: ObservationState,
    #[serde(default)]
    reading: CallReading,
}

/// Uma linha do roteiro de teste: o que mostrar `at_ms` depois de o leitor
/// começar.
#[derive(Debug, Clone, Deserialize)]
struct ScriptLine {
    at_ms: u64,
    #[serde(default = "script_ok")]
    state: ObservationState,
    #[serde(default)]
    reading: CallReading,
}

fn script_ok() -> ObservationState {
    ObservationState::Ok
}

// ------------------------------------------------------------ subprocesso

/// Se este processo é o leitor, roda e devolve o código de saída. Chamado
/// no começo do `main`, antes do log, da migração de pastas e do Tauri: o
/// leitor não toca em nada disso.
pub(crate) fn run_reader_if_requested() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some(READER_FLAG) {
        return None;
    }
    Some(reader_main(&args[1..]))
}

fn arg_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn reader_main(args: &[String]) -> i32 {
    // stdin fechado = o app parou a sessão ou saiu: o leitor sai junto.
    std::thread::spawn(|| {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        std::process::exit(0);
    });
    let started = Instant::now();
    let every = arg_value(args, "--every-ms")
        .and_then(|v| v.parse().ok())
        .unwrap_or(EVERY_MS)
        .max(100);
    let mut out = std::io::stdout().lock();
    let mut emit = |line: &ReaderLine| -> bool {
        serde_json::to_writer(&mut out, line).is_ok()
            && out.write_all(b"\n").is_ok()
            && out.flush().is_ok()
    };
    if let Some(script) = arg_value(args, "--script") {
        return replay(Path::new(script), every, started, &mut emit);
    }
    let Some(root) = arg_value(args, "--root-pid").and_then(|v| v.parse::<u32>().ok()) else {
        return 2;
    };
    read_loop(root, every, started, &mut emit)
}

#[cfg(windows)]
fn read_loop(
    root: u32,
    every: u64,
    started: Instant,
    emit: &mut dyn FnMut(&ReaderLine) -> bool,
) -> i32 {
    use isper_core::speaker_names::uia::UiaReader;
    let mut reader: Option<UiaReader> = None;
    while started.elapsed() < READER_MAX {
        let t0 = Instant::now();
        let q0 = qpc_now();
        if reader.is_none() {
            reader = UiaReader::new().ok();
        }
        let line = match &reader {
            Some(r) => {
                let snap = r.read(root);
                let reading = if snap.state == ObservationState::Ok {
                    speaker_names::read_call(&snap.windows)
                } else {
                    CallReading::default()
                };
                ReaderLine {
                    qpc: q0 + (qpc_now() - q0) / 2,
                    state: snap.state,
                    reading,
                }
            }
            None => ReaderLine {
                qpc: q0,
                state: ObservationState::Error,
                reading: CallReading::default(),
            },
        };
        if !emit(&line) {
            return 0;
        }
        // Um Teams lento (página grande) é lido menos vezes: a leitura nunca
        // ocupa mais que metade do tempo.
        let took = t0.elapsed().as_millis() as u64;
        std::thread::sleep(Duration::from_millis(
            every.max(took * 2).saturating_sub(took),
        ));
    }
    0
}

#[cfg(not(windows))]
fn read_loop(_: u32, _: u64, _: Instant, _: &mut dyn FnMut(&ReaderLine) -> bool) -> i32 {
    2
}

/// O roteiro de teste: cada linha no seu instante; depois da última, ela
/// se repete a cada leitura (como um Teams parado na mesma tela).
fn replay(
    path: &Path,
    every: u64,
    started: Instant,
    emit: &mut dyn FnMut(&ReaderLine) -> bool,
) -> i32 {
    let Ok(text) = std::fs::read_to_string(path) else {
        return 2;
    };
    let mut lines: Vec<ScriptLine> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    lines.sort_by_key(|l| l.at_ms);
    let mut last: Option<&ScriptLine> = None;
    let mut i = 0;
    while started.elapsed() < READER_MAX {
        let now_ms = started.elapsed().as_millis() as u64;
        while i < lines.len() && lines[i].at_ms <= now_ms {
            last = Some(&lines[i]);
            i += 1;
        }
        if let Some(l) = last {
            let line = ReaderLine {
                qpc: qpc_now(),
                state: l.state,
                reading: l.reading.clone(),
            };
            if !emit(&line) {
                return 0;
            }
        }
        std::thread::sleep(Duration::from_millis(every));
    }
    0
}

// ------------------------------------------------------------ Job Object

/// O Job Object do leitor: fechar o handle (o app saiu, caiu, ou a sessão
/// acabou) mata o subprocesso.
#[cfg(windows)]
struct Job(windows_sys::Win32::Foundation::HANDLE);

// SAFETY: um HANDLE de kernel pode ser usado e fechado de qualquer thread.
#[cfg(windows)]
unsafe impl Send for Job {}

#[cfg(windows)]
impl Job {
    fn for_child(child: &Child) -> anyhow::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW,
            JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
        };
        // SAFETY: Job anônimo, sem atributos; o handle é fechado no Drop.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            anyhow::bail!("CreateJobObject: {}", std::io::Error::last_os_error());
        }
        let job = Self(handle);
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        info.ProcessMemoryLimit = READER_MEMORY;
        // SAFETY: estrutura do tamanho declarado, viva durante a chamada; o
        // processo é o filho recém-criado, de quem temos o handle.
        unsafe {
            if SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                anyhow::bail!(
                    "SetInformationJobObject: {}",
                    std::io::Error::last_os_error()
                );
            }
            if AssignProcessToJobObject(job.0, child.as_raw_handle()) == 0 {
                anyhow::bail!(
                    "AssignProcessToJobObject: {}",
                    std::io::Error::last_os_error()
                );
            }
        }
        Ok(job)
    }

    fn terminate(&self) {
        // SAFETY: handle válido até o Drop.
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 0);
        }
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: o handle é nosso e é fechado uma vez só.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(not(windows))]
struct Job;

#[cfg(not(windows))]
impl Job {
    fn for_child(_: &Child) -> anyhow::Result<Self> {
        Ok(Self)
    }
    fn terminate(&self) {}
}

/// O leitor em execução.
struct ReaderChild {
    child: Child,
    job: Job,
    /// Fechar o stdin pede para ele sair.
    stdin: Option<ChildStdin>,
    /// A geração do vínculo com o áudio quando ele foi aberto.
    generation: u64,
}

impl ReaderChild {
    fn kill(mut self) {
        drop(self.stdin.take());
        self.job.terminate();
        let _ = self.child.kill();
        // Não espera: o Job já garantiu a morte, e quem para a sessão não
        // pode ficar preso a um processo travado.
        std::thread::spawn(move || {
            let _ = self.child.wait();
            drop(self.job);
        });
    }
}

/// O ambiente do leitor: só o que o Windows precisa para abrir o COM e o
/// carregador achar as DLLs do próprio exe (o `PATH`: num build de
/// desenvolvimento, as do CUDA vêm dele). Nada do perfil, das pastas do
/// usuário, das chaves de API ou de `ISPER_*`.
const READER_ENV: &[&str] = &[
    "PATH",
    "SystemRoot",
    "SYSTEMROOT",
    "windir",
    "SystemDrive",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
    "OS",
];

fn reader_command(root_pid: u32) -> anyhow::Result<Command> {
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg(READER_FLAG)
        .arg("--root-pid")
        .arg(root_pid.to_string())
        .arg("--every-ms")
        .arg(EVERY_MS.to_string());
    if let Some(script) = test_script() {
        cmd.arg("--script").arg(script);
    }
    cmd.env_clear();
    for k in READER_ENV {
        if let Some(v) = std::env::var_os(k) {
            cmd.env(k, v);
        }
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    Ok(cmd)
}

/// O roteiro de leituras falsas, só dentro de um perfil de teste.
fn test_script() -> Option<PathBuf> {
    crate::paths::profile_dir()?;
    std::env::var_os(SCRIPT_ENV)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

// ------------------------------------------------------------ sessão

/// Contagens da coleta, para o log.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct ReaderStats {
    /// Linhas recebidas do leitor.
    pub(crate) lines: usize,
    /// Leituras guardadas.
    pub(crate) accepted: usize,
    /// Atrasadas ou do futuro.
    pub(crate) late: usize,
    /// Linhas que não eram uma leitura.
    pub(crate) malformed: usize,
    /// Além do limite de leituras da gravação.
    pub(crate) overflow: usize,
    /// Leitores trocados (travados, mortos ou o vínculo mudou).
    pub(crate) restarts: u32,
    /// Lacunas marcadas (vínculo com o áudio perdido).
    pub(crate) gaps: usize,
    /// Leitores que nem abriram.
    pub(crate) spawn_failures: usize,
}

/// O que a sessão juntou até agora.
struct Collected {
    observations: Vec<Observation>,
    stats: ReaderStats,
    /// Quando chegou a última linha (para achar um leitor travado).
    last_line: Instant,
    /// Qual leitor está valendo: linhas de um leitor anterior são ignoradas.
    current: u64,
}

impl Collected {
    /// Uma lacuna: as leituras dos últimos segundos saem, e uma leitura
    /// inválida entra no lugar (ela corta a validade da anterior).
    fn gap(&mut self, at: f64, state: ObservationState) {
        let from = (at - UNBIND_MARGIN_SECS).max(0.0);
        self.observations.retain(|o| o.at_secs < from);
        self.observations.push(Observation::gap(from, state));
        self.stats.gaps += 1;
    }
}

struct Shared {
    stop: AtomicBool,
    collected: Mutex<Collected>,
}

/// A coleta de uma gravação. Nasce depois que o áudio abriu e morre em
/// [`TeamsNamesSession::seal`], com a gravação.
pub(crate) struct TeamsNamesSession {
    shared: Arc<Shared>,
    epoch: u64,
}

/// O que a coleta deixou para o passe final daquela reunião.
pub(crate) struct SealedEvidence {
    observations: Vec<Observation>,
    stats: ReaderStats,
    epoch: u64,
}

/// O que o passe final precisa da sessão: a evidência e a revisão das
/// edições de falante no momento em que a reunião foi salva.
pub(crate) struct FinalNames {
    pub(crate) evidence: SealedEvidence,
    pub(crate) expected_rev: i64,
}

/// Por que a coleta não começou (vai para o log).
///
/// A fonte precisa gravar um processo: "Só o Microsoft Teams" na tela, ou
/// `process:<nome.exe>` no `config.toml` (o mesmo caminho do loopback; é o
/// que o e2e usa para não depender do Teams da máquina). O leitor só olha a
/// árvore de processos que está sendo gravada, então outro programa sem
/// janela de chamada do Teams simplesmente não dá leitura nenhuma.
fn skip_reason(cfg: &config::AppConfig, binding: &CaptureBinding) -> Option<&'static str> {
    use isper_core::loopback::LoopbackSource;
    if !cfg!(windows) {
        Some("só no Windows")
    } else if !matches!(
        LoopbackSource::parse(&cfg.meeting_source),
        LoopbackSource::Process { .. }
    ) {
        Some("a fonte do áudio não é \"Só o Microsoft Teams\"")
    } else if !cfg.final_pass {
        Some("o passe final está desligado")
    } else if !isper_diarize::models_installed() {
        Some("os modelos de diarização não estão instalados")
    } else if binding.current().pid.is_none() {
        Some("o Teams não estava aberto: o áudio é o do sistema inteiro")
    } else {
        None
    }
}

impl TeamsNamesSession {
    /// Começa a coleta da gravação que acabou de abrir, se a opção estiver
    /// ligada e os pré-requisitos valerem.
    pub(crate) fn start_if_enabled(
        app: &AppHandle,
        started: Instant,
        binding: Arc<CaptureBinding>,
    ) -> Option<Self> {
        let cfg = app.state::<AppState>().config.lock_or_recover().clone();
        if !cfg.teams_speaker_names {
            return None;
        }
        if let Some(why) = skip_reason(&cfg, &binding) {
            tracing::info!("nomes do Teams: não vou ler a chamada ({why})");
            return None;
        }
        Some(Self::start(started, binding))
    }

    fn start(started: Instant, binding: Arc<CaptureBinding>) -> Self {
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            collected: Mutex::new(Collected {
                observations: Vec::new(),
                stats: ReaderStats::default(),
                last_line: Instant::now(),
                current: 0,
            }),
        });
        let epoch = epoch();
        let sup = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("teams-names".into())
            .spawn(move || supervise(&sup, started, &binding, epoch));
        if let Err(e) = spawned {
            tracing::warn!("nomes do Teams: não consegui abrir o supervisor ({e})");
            shared.stop.store(true, Ordering::SeqCst);
        } else {
            tracing::info!("nomes do Teams: lendo a chamada desta gravação");
        }
        Self { shared, epoch }
    }

    /// Encerra a coleta e entrega o que ela juntou. Não espera o leitor
    /// morrer (o supervisor e o Job cuidam disso).
    pub(crate) fn seal(self) -> SealedEvidence {
        self.shared.stop.store(true, Ordering::SeqCst);
        let mut c = self.shared.collected.lock_or_recover();
        // Linhas que ainda chegarem são de uma sessão encerrada.
        c.current = u64::MAX;
        let observations = std::mem::take(&mut c.observations);
        let stats = c.stats.clone();
        tracing::info!(
            leituras = stats.accepted,
            atrasadas = stats.late,
            invalidas = stats.malformed,
            excesso = stats.overflow,
            trocas = stats.restarts,
            lacunas = stats.gaps,
            falhas = stats.spawn_failures,
            "nomes do Teams: coleta encerrada"
        );
        SealedEvidence {
            observations,
            stats,
            epoch: self.epoch,
        }
    }
}

/// O laço do supervisor: abre o leitor, troca quando o vínculo com o áudio
/// muda ou ele trava, e marca as lacunas.
fn supervise(shared: &Arc<Shared>, started: Instant, binding: &CaptureBinding, my_epoch: u64) {
    let mut reader: Option<ReaderChild> = None;
    let mut spawns: u64 = 0;
    let mut restarts: u32 = 0;
    let now_secs = || started.elapsed().as_secs_f64();
    loop {
        if shared.stop.load(Ordering::SeqCst) || epoch() != my_epoch {
            break;
        }
        let bound = binding.current();
        let quiet = shared.collected.lock_or_recover().last_line.elapsed() > QUIET;
        let exited = reader
            .as_mut()
            .is_some_and(|r| !matches!(r.child.try_wait(), Ok(None)));
        let stale = reader
            .as_ref()
            .is_some_and(|r| r.generation != bound.generation);
        if reader.is_some() && (quiet || exited || stale || bound.pid.is_none()) {
            let why = if stale || bound.pid.is_none() {
                "o áudio gravado mudou de origem"
            } else if exited {
                "o leitor saiu"
            } else {
                "o leitor parou de responder"
            };
            tracing::info!("nomes do Teams: trocando o leitor ({why})");
            if let Some(r) = reader.take() {
                r.kill();
            }
            {
                let mut c = shared.collected.lock_or_recover();
                c.gap(now_secs(), ObservationState::Unbound);
                c.stats.restarts += 1;
            }
            restarts += 1;
            if restarts > MAX_RESTARTS {
                tracing::warn!(
                    "nomes do Teams: o leitor foi trocado vezes demais — desisto nesta gravação"
                );
                break;
            }
        }
        if reader.is_none()
            && let Some(pid) = bound.pid
        {
            spawns += 1;
            match spawn_reader(shared, started, pid, bound.generation, spawns) {
                Ok(r) => reader = Some(r),
                Err(e) => {
                    tracing::warn!("nomes do Teams: o leitor não abriu ({e})");
                    shared.collected.lock_or_recover().stats.spawn_failures += 1;
                    restarts += 1;
                    if restarts > MAX_RESTARTS {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    if let Some(r) = reader.take() {
        r.kill();
    }
}

fn spawn_reader(
    shared: &Arc<Shared>,
    started: Instant,
    pid: u32,
    generation: u64,
    id: u64,
) -> anyhow::Result<ReaderChild> {
    let mut child = reader_command(pid)?.spawn()?;
    let job = match Job::for_child(&child) {
        Ok(job) => job,
        Err(e) => {
            // Sem o Job, um leitor órfão poderia sobreviver ao app: não roda.
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("leitor sem stdout"))?;
    {
        let mut c = shared.collected.lock_or_recover();
        c.current = id;
        c.last_line = Instant::now();
    }
    let sh = Arc::clone(shared);
    std::thread::Builder::new()
        .name("teams-names-lines".into())
        .spawn(move || read_lines(stdout, &sh, started, id))?;
    Ok(ReaderChild {
        stdin: child.stdin.take(),
        child,
        job,
        generation,
    })
}

/// Lê as linhas de um leitor até ele fechar o stdout.
fn read_lines(stdout: impl Read, shared: &Shared, started: Instant, id: u64) {
    let freq = qpc_freq();
    let mut reader = BufReader::new(stdout);
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    loop {
        buf.clear();
        let n = match (&mut reader)
            .take(MAX_LINE as u64 + 1)
            .read_until(b'\n', &mut buf)
        {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        let too_long = n > MAX_LINE && !buf.ends_with(b"\n");
        if too_long {
            // O resto da linha vai embora sem ser guardado.
            let mut sink = Vec::new();
            if reader.read_until(b'\n', &mut sink).unwrap_or(0) == 0 {
                return;
            }
        }
        let (now_qpc, since) = (qpc_now(), started.elapsed().as_secs_f64());
        let parsed: Option<ReaderLine> = if too_long {
            None
        } else {
            serde_json::from_slice(&buf).ok()
        };
        let mut c = shared.collected.lock_or_recover();
        if c.current != id {
            return; // leitor substituído ou sessão encerrada
        }
        c.stats.lines += 1;
        c.last_line = Instant::now();
        let Some(line) = parsed else {
            c.stats.malformed += 1;
            continue;
        };
        let Some(at_secs) = meeting_secs(line.qpc, now_qpc, freq, since) else {
            c.stats.late += 1;
            continue;
        };
        if c.observations.len() >= MAX_OBSERVATIONS {
            c.stats.overflow += 1;
            continue;
        }
        c.stats.accepted += 1;
        c.observations.push(Observation {
            at_secs,
            state: line.state,
            reading: line.reading,
        });
    }
}

// ------------------------------------------------------------ passe final

/// Os nomes das falas do passe final (`rows[i]` → `names[i]`), ou uma lista
/// vazia quando não há nome nenhum — ou quando a evidência não vale mais
/// (a opção foi desligada depois que a gravação começou).
pub(crate) fn names_for_rows(
    app: &AppHandle,
    meeting_id: i64,
    rows: &[final_pass::Row],
    evidence: &SealedEvidence,
) -> Vec<Option<String>> {
    let enabled = app
        .state::<AppState>()
        .config
        .lock_or_recover()
        .teams_speaker_names;
    if !enabled || evidence.epoch != epoch() {
        tracing::info!(
            meeting_id,
            "nomes do Teams: evidência descartada (a opção foi desligada)"
        );
        return Vec::new();
    }
    let a = attribute_rows(rows, &evidence.observations);
    let r = &a.report;
    tracing::info!(
        meeting_id,
        leituras = r.observations,
        decisivas = r.decisive_observations,
        falas = r.segments,
        com_evidencia = r.segments_with_evidence,
        nomeadas = r.named_segments,
        grupos = r.groups,
        grupos_nomeados = r.named_groups,
        divididos = r.split_groups,
        pouca_evidencia = r.thin_groups,
        disputados = r.disputed_names,
        cobertura = if r.remote_secs > 0.0 {
            r.named_secs / r.remote_secs
        } else {
            0.0
        },
        lacunas = evidence.stats.gaps,
        "nomes do Teams: atribuição"
    );
    if r.named_segments == 0 {
        Vec::new()
    } else {
        a.names
    }
}

/// A atribuição sobre as falas do passe final: "Eu" e "Participantes" nunca
/// recebem nome (não têm grupo de diarização).
fn attribute_rows(
    rows: &[final_pass::Row],
    observations: &[Observation],
) -> speaker_names::Attribution {
    let segments: Vec<RemoteSegment> = rows
        .iter()
        .map(|(speaker, start, end, _)| RemoteSegment {
            group: speaker_names::group_of(speaker),
            start_secs: f64::from(*start),
            end_secs: f64::from(*end),
        })
        .collect();
    speaker_names::attribute(observations, &segments, &Rules::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relogio_da_reuniao_a_partir_do_qpc() {
        let freq = 10_000_000; // 100 ns por tique, como no Windows
        // Lida há 0,25 s; a gravação começou há 10 s: aconteceu aos 9,75 s.
        let at = meeting_secs(1_000_000, 3_500_000, freq, 10.0).unwrap();
        assert!((at - 9.75).abs() < 1e-9, "{at}");
        // Atrasada demais, do futuro, ou antes de a gravação começar: não vale.
        assert_eq!(meeting_secs(0, 6 * freq, freq, 10.0), None);
        assert_eq!(meeting_secs(2 * freq, freq, freq, 10.0), None);
        assert_eq!(meeting_secs(0, 2 * freq, freq, 1.0), None);
    }

    #[test]
    fn linha_do_leitor_vai_e_volta_e_nao_leva_arvore() {
        let line = ReaderLine {
            qpc: 42,
            state: ObservationState::Ok,
            reading: CallReading {
                speaking: vec!["Ana".into()],
                participants: vec!["Ana".into(), "Bruno".into()],
                me: Some("Bruno".into()),
            },
        };
        let json = serde_json::to_string(&line).unwrap();
        assert_eq!(
            json,
            r#"{"qpc":42,"state":"ok","reading":{"speaking":["Ana"],"participants":["Ana","Bruno"],"me":"Bruno"}}"#
        );
        assert_eq!(serde_json::from_str::<ReaderLine>(&json).unwrap(), line);
        // Estado fora da chamada, sem leitura.
        let gap: ReaderLine =
            serde_json::from_str(r#"{"qpc":1,"state":"no-call-window"}"#).unwrap();
        assert_eq!(gap.state, ObservationState::NoCallWindow);
    }

    fn shared() -> Arc<Shared> {
        Arc::new(Shared {
            stop: AtomicBool::new(false),
            collected: Mutex::new(Collected {
                observations: Vec::new(),
                stats: ReaderStats::default(),
                last_line: Instant::now(),
                current: 7,
            }),
        })
    }

    #[test]
    fn linhas_ruins_atrasadas_e_longas_nao_entram() {
        let sh = shared();
        let started = Instant::now() - Duration::from_secs(30);
        let now = qpc_now();
        let freq = qpc_freq();
        let ok = serde_json::to_string(&ReaderLine {
            qpc: now,
            state: ObservationState::Ok,
            reading: CallReading::default(),
        })
        .unwrap();
        let late = serde_json::to_string(&ReaderLine {
            qpc: now - 60 * freq,
            state: ObservationState::Ok,
            reading: CallReading::default(),
        })
        .unwrap();
        let long = format!("{{\"qpc\":{now},\"x\":\"{}\"}}", "a".repeat(MAX_LINE + 10));
        let input = format!("{ok}\nnão é json\n{late}\n{long}\n{ok}\n");
        read_lines(input.as_bytes(), &sh, started, 7);
        let c = sh.collected.lock_or_recover();
        assert_eq!(c.stats.lines, 5);
        assert_eq!(c.stats.accepted, 2);
        assert_eq!(c.stats.malformed, 2, "lixo e a linha longa");
        assert_eq!(c.stats.late, 1);
        assert_eq!(c.observations.len(), 2);
        assert!(
            c.observations
                .iter()
                .all(|o| (29.0..=30.5).contains(&o.at_secs))
        );
    }

    #[test]
    fn linhas_de_um_leitor_substituido_sao_ignoradas() {
        let sh = shared();
        let line = serde_json::to_string(&ReaderLine {
            qpc: qpc_now(),
            state: ObservationState::Ok,
            reading: CallReading::default(),
        })
        .unwrap();
        // O leitor 6 já foi trocado pelo 7.
        read_lines(format!("{line}\n").as_bytes(), &sh, Instant::now(), 6);
        assert!(sh.collected.lock_or_recover().observations.is_empty());
    }

    #[test]
    fn lacuna_apaga_os_ultimos_segundos_e_corta_a_validade() {
        let mut c = Collected {
            observations: (0..20)
                .map(|i| Observation::gap(f64::from(i) * 0.5, ObservationState::Ok))
                .collect(),
            stats: ReaderStats::default(),
            last_line: Instant::now(),
            current: 1,
        };
        c.gap(9.0, ObservationState::Unbound);
        let last = c.observations.last().unwrap();
        assert_eq!(last.state, ObservationState::Unbound);
        assert!((last.at_secs - 7.0).abs() < 1e-9);
        assert!(
            c.observations[..c.observations.len() - 1]
                .iter()
                .all(|o| o.at_secs < 7.0)
        );
        assert_eq!(c.stats.gaps, 1);
    }

    #[test]
    fn falas_do_passe_final_eu_e_participantes_nunca_ganham_nome() {
        let reading = CallReading {
            speaking: vec!["Ana Souza".into()],
            participants: vec!["Bruno".into(), "Ana Souza".into(), "Carla".into()],
            me: Some("Bruno".into()),
        };
        let obs: Vec<Observation> = (0..40)
            .map(|i| Observation {
                at_secs: f64::from(i) * 0.5 + 0.75,
                state: ObservationState::Ok,
                reading: reading.clone(),
            })
            .collect();
        let rows: Vec<final_pass::Row> = vec![
            ("Eu".into(), 0.0, 3.0, "a".into()),
            ("Participante 1".into(), 1.0, 4.0, "b".into()),
            ("Participantes".into(), 5.0, 8.0, "c".into()),
            ("Participante 1".into(), 9.0, 12.0, "d".into()),
        ];
        let a = attribute_rows(&rows, &obs);
        assert_eq!(
            a.names,
            [
                None,
                Some("Ana Souza".into()),
                None,
                Some("Ana Souza".into())
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn so_uma_fonte_por_processo_que_abriu_liga_a_leitura() {
        const SOURCE: &str = "a fonte do áudio não é \"Só o Microsoft Teams\"";
        let binding = CaptureBinding::default();
        let mut cfg = config::AppConfig {
            final_pass: true,
            ..config::AppConfig::default()
        };
        cfg.meeting_source = "system".into();
        assert_eq!(skip_reason(&cfg, &binding), Some(SOURCE));
        for src in ["teams", "process:isper-e2e-teams.exe"] {
            cfg.meeting_source = src.into();
            assert_ne!(skip_reason(&cfg, &binding), Some(SOURCE), "{src}");
        }
        cfg.final_pass = false;
        assert_eq!(
            skip_reason(&cfg, &binding),
            Some("o passe final está desligado")
        );
        // Com tudo certo mas o loopback no sistema inteiro (o Teams não estava
        // aberto), não há o que ler.
        if isper_diarize::models_installed() {
            cfg.final_pass = true;
            assert!(skip_reason(&cfg, &binding).is_some_and(|w| w.contains("sistema inteiro")));
            binding.set(Some(4242));
            assert_eq!(skip_reason(&cfg, &binding), None);
        }
    }

    #[test]
    fn ambiente_do_leitor_nao_leva_perfil_nem_chaves() {
        // Com `env_clear`, só as variáveis da lista chegam ao leitor: nada
        // do perfil do usuário, de `ISPER_*` ou de chaves de API.
        let cmd = reader_command(1234).unwrap();
        let envs: Vec<String> = cmd
            .get_envs()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert!(
            envs.iter().all(|k| READER_ENV.contains(&k.as_str())),
            "{envs:?}"
        );
        for k in [
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "ISPER_PROFILE_DIR",
        ] {
            assert!(!envs.iter().any(|e| e.eq_ignore_ascii_case(k)), "{k} vazou");
        }
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[..3], [READER_FLAG, "--root-pid", "1234"]);
        assert!(
            !args.iter().any(|a| a == "--script"),
            "roteiro só no perfil de teste"
        );
    }
}
