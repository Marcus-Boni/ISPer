//! Importar gravações de fora (Fase 9.0): um arquivo escolhido ou arrastado
//! na Biblioteca, ou deixado na pasta vigiada `Documentos\ISPer\Importar`,
//! vira uma reunião — o mesmo passe final de uma reunião gravada, com
//! diarização, resumo por IA, busca semântica e notificação.
//!
//! ```text
//!  Biblioteca (escolher/arrastar) ─┐
//!                                  ├─► fila ─► espera a reunião e o motor
//!  pasta Importar (vigiada) ───────┘            │
//!                          SHA-256 já importado? ├─► aponta a reunião que existe
//!                                               ▼
//!                  ler (16 kHz) → transcrever → falantes → salvar → resumo
//!                                               │
//!                  arquivo da pasta → Importados  (ou "Não importados" + motivo)
//! ```
//!
//! Um arquivo por vez: transcrever disputa a GPU com o ditado. Enquanto uma
//! reunião grava, ou o passe final dela roda, a fila espera. Nada é apagado:
//! o arquivo escolhido fica onde está, e o da pasta vigiada muda de pasta.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Condvar;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use crate::prelude::*;
use isper_core::meeting::{self, SegmentRef};
use isper_core::store::NewImportedMeeting;
use isper_core::{decode, recording};

/// Para onde vai o arquivo da pasta vigiada depois de virar reunião.
const DONE_DIR: &str = "Importados";
/// E para onde vai quando não deu — com um `.txt` dizendo por quê.
const FAILED_DIR: &str = "Não importados";
/// Intervalo entre as varreduras da pasta vigiada.
const WATCH_EVERY: Duration = Duration::from_secs(4);

/// De onde veio o arquivo.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Origin {
    /// Escolhido ou arrastado na Biblioteca: fica onde está.
    Picked,
    /// Deixado na pasta vigiada: muda de pasta ao terminar.
    Watched,
}

#[derive(Clone, Debug)]
struct Job {
    path: PathBuf,
    origin: Origin,
}

/// A etapa do arquivo em processamento, para a Biblioteca.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Stage {
    /// Na fila, esperando a reunião em andamento ou o modelo carregar.
    Waiting,
    /// Lendo e convertendo o áudio.
    Reading,
    /// Transcrevendo.
    Transcribing,
    /// Identificando quem falou.
    Speakers,
    /// Gravando a reunião.
    Saving,
    /// Pedindo o resumo à IA.
    Summary,
}

/// Como terminou a importação de um arquivo.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct ImportOutcome {
    pub(crate) file: String,
    pub(crate) ok: bool,
    /// A reunião criada — ou a que já existia, quando o áudio já tinha entrado.
    pub(crate) meeting_id: Option<i64>,
    pub(crate) duplicate: bool,
    pub(crate) cancelled: bool,
    pub(crate) message: Option<String>,
}

/// O que a Biblioteca mostra da fila (evento `isper-import`).
#[derive(Clone, Debug, Default, serde::Serialize)]
pub(crate) struct ImportStatus {
    /// O arquivo em processamento.
    pub(crate) current: Option<String>,
    pub(crate) stage: Option<Stage>,
    /// De 0 a 1, dentro da etapa (leitura e transcrição).
    pub(crate) progress: f32,
    /// Os que esperam, na ordem.
    pub(crate) queued: Vec<String>,
    /// O último que terminou.
    pub(crate) last: Option<ImportOutcome>,
}

/// A fila de importação (em `AppState`).
#[derive(Default)]
pub(crate) struct ImportQueue {
    jobs: Mutex<VecDeque<Job>>,
    ready: Condvar,
    status: Mutex<ImportStatus>,
    /// Cancela o arquivo em processamento.
    cancel: AtomicBool,
    /// Caminhos na fila ou em processamento (a pasta vigiada não repete).
    busy: Mutex<HashSet<PathBuf>>,
}

impl ImportQueue {
    /// Põe na fila; `false` se o caminho já estava nela ou em processamento.
    fn push(&self, job: Job) -> bool {
        if !self.busy.lock_or_recover().insert(job.path.clone()) {
            return false;
        }
        self.jobs.lock_or_recover().push_back(job);
        self.ready.notify_one();
        true
    }

    fn next(&self) -> Job {
        let mut jobs = self.jobs.lock_or_recover();
        loop {
            if let Some(j) = jobs.pop_front() {
                return j;
            }
            jobs = self
                .ready
                .wait(jobs)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn queued_names(&self) -> Vec<String> {
        self.jobs
            .lock_or_recover()
            .iter()
            .map(|j| file_name(&j.path))
            .collect()
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ------------------------------------------------------------- estado/UI

fn publish(app: &AppHandle) {
    let q = &app.state::<AppState>().imports;
    let mut status = q.status.lock_or_recover().clone();
    status.queued = q.queued_names();
    let _ = app.emit("isper-import", status);
}

fn set_stage(app: &AppHandle, stage: Stage, progress: f32) {
    {
        let q = &app.state::<AppState>().imports;
        let mut s = q.status.lock_or_recover();
        // Progresso só é avisado a cada 1%: um arquivo longo tem milhares de
        // janelas, e cada evento repinta a Biblioteca.
        if s.stage == Some(stage) && (progress - s.progress).abs() < 0.01 && progress < 1.0 {
            return;
        }
        s.stage = Some(stage);
        s.progress = progress.clamp(0.0, 1.0);
    }
    publish(app);
}

// ------------------------------------------------------------ o trabalho

/// Por que um arquivo não virou reunião.
#[derive(Debug)]
struct Failure {
    message: String,
    cancelled: bool,
}

/// Qualquer erro vira [`Failure`]; o cancelamento é reconhecido pelo tipo.
fn fail(e: impl Into<anyhow::Error>) -> Failure {
    let e: anyhow::Error = e.into();
    let cancelled = e.chain().any(|c| {
        matches!(
            c.downcast_ref::<isper_core::IsperError>(),
            Some(isper_core::IsperError::Cancelled)
        )
    });
    Failure {
        message: e.to_string(),
        cancelled,
    }
}

enum Done {
    Imported {
        id: i64,
        title: String,
        secs: f32,
        md_path: PathBuf,
        has_summary: bool,
    },
    Duplicate {
        id: i64,
    },
}

/// Espera a vez: reunião gravando ou passe final rodando seguram a fila, e o
/// modelo Whisper precisa estar carregado.
fn wait_turn(app: &AppHandle) -> Result<(), Failure> {
    let state = app.state::<AppState>();
    loop {
        if state.imports.cancel.load(Ordering::Relaxed) {
            return Err(Failure {
                message: crate::i18n::tr(app, "import.cancelled"),
                cancelled: true,
            });
        }
        let busy = state.meeting.lock_or_recover().is_some()
            || state.diarizing.lock_or_recover().is_some();
        let engine_ready = state.engine.lock_or_recover().is_some();
        if !busy && engine_ready {
            return Ok(());
        }
        if !busy
            && matches!(
                *state.engine_status.lock_or_recover(),
                EngineStatus::Missing | EngineStatus::Failed { .. }
            )
        {
            return Err(Failure {
                message: crate::i18n::tr(app, "import.no-model"),
                cancelled: false,
            });
        }
        std::thread::sleep(Duration::from_millis(800));
    }
}

fn process(app: &AppHandle, job: &Job) -> Result<Done, Failure> {
    let state = app.state::<AppState>();
    let q = &state.imports;
    if !job.path.is_file() {
        return Err(Failure {
            message: crate::i18n::tr(app, "import.missing"),
            cancelled: false,
        });
    }
    set_stage(app, Stage::Waiting, 0.0);

    // O mesmo áudio não vira duas reuniões.
    let sha = isper_models::sha256_file(&job.path).map_err(fail)?;
    let store = open_store().map_err(fail)?;
    if let Some(id) = store.meeting_with_source(&sha).map_err(fail)? {
        return Ok(Done::Duplicate { id });
    }

    wait_turn(app)?;

    set_stage(app, Stage::Reading, 0.0);
    let on_read = |f: f32| set_stage(app, Stage::Reading, f);
    let decoded =
        decode::decode_to_16k(&job.path, Some(&on_read), Some(&q.cancel)).map_err(fail)?;

    let engine = final_pass::loaded_engine(app).map_err(fail)?;
    let vad = final_pass::vad_model().map_err(fail)?;
    set_stage(app, Stage::Transcribing, 0.0);
    let on_asr = |done: usize, total: usize| {
        if total > 0 && done >= total {
            set_stage(app, Stage::Speakers, 0.0);
        } else if total > 0 {
            set_stage(app, Stage::Transcribing, done as f32 / total as f32);
        }
    };
    let rows = final_pass::transcribe_mixed(
        app,
        &engine,
        &vad,
        &decoded.samples_16k,
        0,
        Some(&on_asr),
        Some(&q.cancel),
    )
    .map_err(fail)?;
    if rows.is_empty() {
        return Err(Failure {
            message: crate::i18n::tr(app, "import.no-speech"),
            cancelled: false,
        });
    }

    set_stage(app, Stage::Saving, 0.0);
    let source_name = file_name(&job.path);
    let stem = job
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let started_at =
        recording::started_at_from_name(&stem).unwrap_or_else(|| modified_stamp(&job.path));
    let named = recording::title_from_name(&stem);
    let mut title = named.clone().unwrap_or_else(|| {
        crate::i18n::trv(
            app,
            "meeting.default-title",
            &[("date", started_at.clone())],
        )
    });
    let md_path = unique_md_path(&meetings_dir().map_err(fail)?, &started_at);

    // O `.md` antes do banco, como numa reunião gravada: se o resto falhar,
    // a transcrição está no arquivo e o `import` a recupera.
    let refs: Vec<SegmentRef<'_>> = rows
        .iter()
        .map(|(speaker, start, end, text)| SegmentRef {
            speaker,
            start_secs: *start,
            end_secs: *end,
            text,
        })
        .collect();
    let secs = decoded.duration_secs();
    let mut md = meeting::render_markdown(&title, &started_at, secs, &refs, None, &[]);
    meeting::insert_source_line(&mut md, &source_name);
    std::fs::write(&md_path, &md).map_err(fail)?;
    let id = store
        .save_imported(&NewImportedMeeting {
            title: &title,
            started_at: &started_at,
            duration_secs: secs,
            md_path: &md_path.to_string_lossy(),
            segments: &rows,
            source_name: &source_name,
            source_sha256: &sha,
        })
        .map_err(fail)?;
    tracing::info!(
        meeting_id = id,
        arquivo = %source_name,
        secs,
        falas = rows.len(),
        "gravação importada"
    );

    // Título da IA só quando o nome do arquivo não dizia nada.
    if let Some(t) = summarize_saved(&store, id, &md, named.is_none(), || {
        set_stage(app, Stage::Summary, 0.0)
    }) {
        title = t;
    }
    let has_summary = store
        .get_meeting(id)
        .ok()
        .flatten()
        .is_some_and(|d| d.summary.is_some());
    index_meeting_background(app, id);
    Ok(Done::Imported {
        id,
        title,
        secs,
        md_path,
        has_summary,
    })
}

/// `dd/mm/aaaa hh:mm` da última modificação do arquivo, no fuso local — a
/// data quando o nome não traz uma.
fn modified_stamp(path: &Path) -> String {
    let when = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| SystemTime::now());
    chrono::DateTime::<chrono::Local>::from(when)
        .format("%d/%m/%Y %H:%M")
        .to_string()
}

/// `reuniao-AAAAMMDD-HHMM00.md` a partir de `dd/mm/aaaa hh:mm`, sem pisar em
/// um arquivo que já existe (duas gravações no mesmo minuto ganham `-2`).
fn unique_md_path(dir: &Path, started_at: &str) -> PathBuf {
    let stamp = chrono::NaiveDateTime::parse_from_str(started_at, "%d/%m/%Y %H:%M")
        .map(|d| d.format("%Y%m%d-%H%M00").to_string())
        .unwrap_or_else(|_| chrono::Local::now().format("%Y%m%d-%H%M%S").to_string());
    let mut path = dir.join(format!("reuniao-{stamp}.md"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("reuniao-{stamp}-{n}.md"));
        n += 1;
    }
    path
}

/// Move o arquivo para `dir_name` ao lado dele, sem sobrescrever nada.
fn move_aside(path: &Path, dir_name: &str) -> std::io::Result<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let dir = parent.join(dir_name);
    std::fs::create_dir_all(&dir)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut dest = dir.join(format!("{stem}{ext}"));
    let mut n = 2;
    while dest.exists() {
        dest = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    std::fs::rename(path, &dest)?;
    Ok(dest)
}

/// Fecha um arquivo: estado, notificação e, se veio da pasta vigiada, a
/// mudança de pasta.
fn finish(app: &AppHandle, job: &Job, result: Result<Done, Failure>) {
    let state = app.state::<AppState>();
    let file = file_name(&job.path);
    // O cancelamento que vem do núcleo diz só "cancelado": a mensagem vai no
    // idioma da interface, igual à de quem cancelou antes de começar.
    let result = result.map_err(|f| {
        if f.cancelled {
            Failure {
                message: crate::i18n::tr(app, "import.cancelled"),
                cancelled: true,
            }
        } else {
            f
        }
    });
    let outcome = match &result {
        Ok(Done::Imported { id, .. }) => ImportOutcome {
            file: file.clone(),
            ok: true,
            meeting_id: Some(*id),
            duplicate: false,
            cancelled: false,
            message: None,
        },
        Ok(Done::Duplicate { id }) => ImportOutcome {
            file: file.clone(),
            ok: true,
            meeting_id: Some(*id),
            duplicate: true,
            cancelled: false,
            message: None,
        },
        Err(f) => ImportOutcome {
            file: file.clone(),
            ok: false,
            meeting_id: None,
            duplicate: false,
            cancelled: f.cancelled,
            message: Some(f.message.clone()),
        },
    };

    if job.origin == Origin::Watched {
        let moved = match &result {
            Ok(_) => move_aside(&job.path, DONE_DIR),
            Err(f) => move_aside(&job.path, FAILED_DIR).inspect(|dest| {
                let _ = std::fs::write(
                    dest.with_extension("motivo.txt"),
                    format!("{}\r\n", f.message),
                );
            }),
        };
        if let Err(e) = moved {
            tracing::warn!(arquivo = %file, "não consegui mover o arquivo da pasta Importar: {e}");
        }
    }

    match (&result, job.origin) {
        (
            Ok(Done::Imported {
                id,
                title,
                secs,
                md_path,
                has_summary,
            }),
            _,
        ) => notify_done(app, *id, title, *secs, *has_summary, md_path),
        // Da pasta vigiada ninguém está olhando: o erro vira aviso. Da
        // Biblioteca, ele aparece ali mesmo.
        (Err(f), Origin::Watched) if !f.cancelled => {
            let heading = crate::i18n::tr(app, "notify.import-failed");
            let _ = notify::show(
                notify::Toast {
                    title: &heading,
                    line1: &file,
                    line2: Some(&f.message),
                    silent: true,
                },
                || {},
            );
        }
        _ => {}
    }
    if let Err(f) = &result {
        tracing::warn!(arquivo = %file, cancelado = f.cancelled, "importação não concluída: {}", f.message);
    }

    {
        let mut s = state.imports.status.lock_or_recover();
        s.current = None;
        s.stage = None;
        s.progress = 0.0;
        s.last = Some(outcome);
    }
    state.imports.busy.lock_or_recover().remove(&job.path);
    publish(app);
    notify_status(app);
}

fn notify_done(app: &AppHandle, id: i64, title: &str, secs: f32, has_summary: bool, md: &Path) {
    let after = app
        .state::<AppState>()
        .config
        .lock_or_recover()
        .after_meeting
        .clone();
    match after.as_str() {
        "open" => open_file(md),
        "silent" => {}
        _ => {
            let summary = if has_summary {
                crate::i18n::tr(app, "notify.summary-ready")
            } else {
                String::new()
            };
            let line2 = crate::i18n::trv(
                app,
                "notify.meeting-saved-line2",
                &[("duration", meeting::fmt_ts(secs)), ("summary", summary)],
            );
            let heading = crate::i18n::tr(app, "notify.import-done");
            let app2 = app.clone();
            let _ = notify::show(
                notify::Toast {
                    title: &heading,
                    line1: title,
                    line2: Some(&line2),
                    silent: false,
                },
                move || open_library_at(&app2, id),
            );
        }
    }
}

/// A thread que processa a fila, um arquivo por vez.
pub(crate) fn spawn_worker(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("isper-import".into())
        .spawn(move || {
            loop {
                let job = app.state::<AppState>().imports.next();
                {
                    let q = &app.state::<AppState>().imports;
                    q.cancel.store(false, Ordering::Relaxed);
                    let mut s = q.status.lock_or_recover();
                    s.current = Some(file_name(&job.path));
                    s.stage = Some(Stage::Waiting);
                    s.progress = 0.0;
                }
                publish(&app);
                let result = process(&app, &job);
                finish(&app, &job, result);
            }
        });
    if let Err(e) = spawned {
        tracing::error!("não consegui iniciar a fila de importação: {e}");
    }
}

// --------------------------------------------------------- pasta vigiada

/// `Documentos\ISPer\Importar` (nos e2e, a do perfil de teste).
pub(crate) fn import_dir() -> anyhow::Result<PathBuf> {
    let dir = crate::paths::home_dir()
        .ok_or_else(|| anyhow::anyhow!("pasta do usuário indisponível"))?
        .join("Documents")
        .join("ISPer")
        .join("Importar");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// O arquivo terminou de ser copiado? Só quando dá para abri-lo sem ninguém
/// mais com ele aberto (quem copia ainda segura o arquivo).
fn can_open_exclusively(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(path)
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        std::fs::File::open(path).is_ok()
    }
}

/// Tamanho e data de modificação: iguais em duas varreduras seguidas = o
/// arquivo parou de crescer.
type Snapshot = (u64, SystemTime);

/// Os arquivos da pasta que estão prontos para entrar na fila: de áudio,
/// parados desde a varredura anterior e livres. `seen` guarda o que foi visto.
fn scan_ready(dir: &Path, seen: &mut HashMap<PathBuf, Snapshot>) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut now: HashMap<PathBuf, Snapshot> = HashMap::new();
    let mut ready = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        let hidden = file_name(&path).starts_with(['.', '~']);
        if !meta.is_file() || hidden || !decode::is_supported(&path) {
            continue;
        }
        let snap = (
            meta.len(),
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        );
        if seen.get(&path) == Some(&snap) && snap.0 > 0 && can_open_exclusively(&path) {
            ready.push(path.clone());
        }
        now.insert(path, snap);
    }
    *seen = now;
    ready.sort();
    ready
}

/// A thread que vigia a pasta Importar (com a opção ligada nas Configurações).
pub(crate) fn spawn_watcher(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("isper-import-watch".into())
        .spawn(move || {
            let mut seen: HashMap<PathBuf, Snapshot> = HashMap::new();
            loop {
                std::thread::sleep(WATCH_EVERY);
                let on = app
                    .state::<AppState>()
                    .config
                    .lock_or_recover()
                    .import_watch;
                if !on {
                    seen.clear();
                    continue;
                }
                let Ok(dir) = import_dir() else { continue };
                let mut added = false;
                for path in scan_ready(&dir, &mut seen) {
                    let q = &app.state::<AppState>().imports;
                    if q.push(Job {
                        path: path.clone(),
                        origin: Origin::Watched,
                    }) {
                        tracing::info!(arquivo = %file_name(&path), "gravação na pasta Importar");
                        added = true;
                    }
                }
                if added {
                    publish(&app);
                }
            }
        });
    if let Err(e) = spawned {
        tracing::error!("não consegui vigiar a pasta Importar: {e}");
    }
}

// --------------------------------------------------------------- comandos

/// Quantos arquivos entraram na fila e quais foram recusados (não são áudio).
#[derive(serde::Serialize)]
pub(crate) struct Queued {
    queued: usize,
    rejected: Vec<String>,
}

fn enqueue_picked(app: &AppHandle, paths: Vec<PathBuf>) -> Queued {
    let q = &app.state::<AppState>().imports;
    let mut out = Queued {
        queued: 0,
        rejected: Vec::new(),
    };
    for path in paths {
        if path.is_file() && decode::is_supported(&path) {
            if q.push(Job {
                path,
                origin: Origin::Picked,
            }) {
                out.queued += 1;
            }
        } else {
            out.rejected.push(file_name(&path));
        }
    }
    publish(app);
    out
}

/// Abre o seletor de arquivos e põe os escolhidos na fila.
#[tauri::command]
pub(crate) async fn import_pick_files(app: AppHandle) -> Result<Queued, String> {
    use tauri_plugin_dialog::DialogExt;
    let title = crate::i18n::tr(&app, "import.dialog-title");
    let filter = crate::i18n::tr(&app, "import.dialog-filter");
    let mut builder = app
        .dialog()
        .file()
        .set_title(title)
        .add_filter(filter, &decode::AUDIO_EXTENSIONS);
    if let Some(parent) = app.get_webview_window("library") {
        builder = builder.set_parent(&parent);
    }
    let picked = tauri::async_runtime::spawn_blocking(move || builder.blocking_pick_files())
        .await
        .map_err(|e| e.to_string())?;
    let paths: Vec<PathBuf> = picked
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .collect();
    Ok(enqueue_picked(&app, paths))
}

/// Põe na fila os arquivos dados (arrastados para a Biblioteca).
#[tauri::command]
pub(crate) fn import_audio_files(app: AppHandle, paths: Vec<String>) -> Queued {
    enqueue_picked(&app, paths.into_iter().map(PathBuf::from).collect())
}

/// O estado da fila, para a Biblioteca abrir já sabendo.
#[tauri::command]
pub(crate) fn import_status(app: AppHandle) -> ImportStatus {
    let q = &app.state::<AppState>().imports;
    let mut status = q.status.lock_or_recover().clone();
    status.queued = q.queued_names();
    status
}

/// Cancela o arquivo em processamento e, com `all`, os que esperam. Um
/// arquivo da pasta vigiada que sai da fila vai para "Não importados" —
/// senão a próxima varredura o poria de volta.
#[tauri::command]
pub(crate) fn import_cancel(app: AppHandle, all: bool) {
    let q = &app.state::<AppState>().imports;
    q.cancel.store(true, Ordering::Relaxed);
    if all {
        let dropped: Vec<Job> = q.jobs.lock_or_recover().drain(..).collect();
        let motivo = crate::i18n::tr(&app, "import.cancelled");
        for job in dropped {
            q.busy.lock_or_recover().remove(&job.path);
            if job.origin == Origin::Watched
                && let Ok(dest) = move_aside(&job.path, FAILED_DIR)
            {
                let _ = std::fs::write(dest.with_extension("motivo.txt"), format!("{motivo}\r\n"));
            }
        }
    }
    publish(&app);
}

/// Abre a pasta vigiada no Explorador.
#[tauri::command]
pub(crate) fn open_import_folder() -> Result<(), String> {
    let dir = import_dir().map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("isper-import-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_ata_de_duas_gravacoes_no_mesmo_minuto_nao_se_pisa() {
        let dir = temp_dir("md");
        let a = unique_md_path(&dir, "22/09/2026 15:30");
        assert_eq!(file_name(&a), "reuniao-20260922-153000.md");
        std::fs::write(&a, "x").unwrap();
        let b = unique_md_path(&dir, "22/09/2026 15:30");
        assert_eq!(file_name(&b), "reuniao-20260922-153000-2.md");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mover_para_o_lado_nunca_sobrescreve() {
        let dir = temp_dir("mover");
        let f = dir.join("gravação.mp3");
        std::fs::write(&f, "1").unwrap();
        let d1 = move_aside(&f, DONE_DIR).unwrap();
        assert_eq!(d1, dir.join(DONE_DIR).join("gravação.mp3"));
        std::fs::write(&f, "2").unwrap();
        let d2 = move_aside(&f, DONE_DIR).unwrap();
        assert_eq!(d2, dir.join(DONE_DIR).join("gravação (2).mp3"));
        assert_eq!(
            std::fs::read_to_string(d1).unwrap(),
            "1",
            "o primeiro continua lá"
        );
        assert!(!f.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pasta_vigiada_so_entrega_o_que_parou_de_crescer() {
        let dir = temp_dir("vigia");
        let mut seen = HashMap::new();
        std::fs::write(dir.join("reuniao.mp3"), vec![1u8; 100]).unwrap();
        std::fs::write(dir.join("notas.txt"), "não é áudio").unwrap();
        std::fs::write(dir.join("~temporario.mp3"), vec![1u8; 10]).unwrap();
        std::fs::write(dir.join("vazio.m4a"), b"").unwrap();
        std::fs::create_dir_all(dir.join(DONE_DIR)).unwrap();
        std::fs::write(dir.join(DONE_DIR).join("antigo.mp3"), vec![1u8; 10]).unwrap();

        assert!(
            scan_ready(&dir, &mut seen).is_empty(),
            "na primeira vez só anota"
        );
        let prontos = scan_ready(&dir, &mut seen);
        assert_eq!(prontos, vec![dir.join("reuniao.mp3")], "{prontos:?}");

        // Ainda crescendo: espera a próxima varredura.
        std::fs::write(dir.join("reuniao.mp3"), vec![1u8; 200]).unwrap();
        assert!(scan_ready(&dir, &mut seen).is_empty());
        assert_eq!(scan_ready(&dir, &mut seen).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fila_nao_repete_o_mesmo_arquivo() {
        let q = ImportQueue::default();
        let job = || Job {
            path: PathBuf::from("C:/x/a.mp3"),
            origin: Origin::Watched,
        };
        assert!(q.push(job()));
        assert!(!q.push(job()), "já está na fila");
        assert_eq!(q.queued_names(), vec!["a.mp3".to_string()]);
        assert_eq!(q.next().path, PathBuf::from("C:/x/a.mp3"));
        assert!(!q.push(job()), "em processamento também não entra de novo");
        q.busy
            .lock_or_recover()
            .remove(&PathBuf::from("C:/x/a.mp3"));
        assert!(q.push(job()));
    }

    #[test]
    fn erro_de_cancelamento_e_reconhecido() {
        assert!(fail(isper_core::IsperError::Cancelled).cancelled);
        // Mesmo embrulhado num contexto, como o passe final devolve.
        let embrulhado =
            anyhow::Error::from(isper_core::IsperError::Cancelled).context("passe final");
        assert!(fail(embrulhado).cancelled);
        assert!(!fail(anyhow::anyhow!("outra coisa")).cancelled);
    }
}
