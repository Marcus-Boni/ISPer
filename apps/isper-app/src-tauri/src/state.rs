//! Estado compartilhado do app: constantes, `AppState`, `EngineStatus`, caminhos e o aviso `isper-status`.

use crate::config::AppConfig;
use crate::prelude::*;
use isper_core::meeting::MeetingHandle;
use isper_core::recorder;
use isper_core::store::MeetingStore;
use isper_core::WhisperEngine;
use tauri::image::Image;
use tauri::menu::MenuItem;
use tauri::tray::TrayIcon;
use tauri_plugin_global_shortcut::Shortcut;

/// Candidatos a atalho, em ordem de preferência, usados quando o usuário não
/// fixou um nas configurações (ou quando o preferido está ocupado — neste PC,
/// Ctrl+Alt+Espaço vive ocupado por outro programa).
pub(crate) const SHORTCUT_CANDIDATES: [(&str, &str); 4] = [
    ("ctrl+alt+space", "Ctrl+Alt+Espaço"),
    ("ctrl+shift+space", "Ctrl+Shift+Espaço"),
    ("ctrl+alt+d", "Ctrl+Alt+D"),
    ("ctrl+alt+i", "Ctrl+Alt+I"),
];

/// Candidatos ao atalho de reunião (iniciar/encerrar a gravação).
pub(crate) const MEETING_SHORTCUT_CANDIDATES: [&str; 3] =
    ["ctrl+alt+m", "ctrl+shift+m", "ctrl+alt+r"];

/// Toques do atalho de reunião mais próximos que isso são ignorados (auto-repeat
/// da tecla e duplo aperto nervoso não podem iniciar e encerrar em sequência).
pub(crate) const MEETING_HOTKEY_DEBOUNCE: Duration = Duration::from_millis(1200);

/// Últimas falas guardadas para a transcrição ao vivo (o Início pede ao abrir).
pub(crate) const LIVE_KEEP: usize = 400;

/// Soltar antes disso = toque rápido → vira modo mãos-livres.
pub(crate) const TAP_THRESHOLD: Duration = Duration::from_millis(350);

/// Tamanhos lógicos do indicador flutuante: normal e mini (ponto + cronômetro).
pub(crate) const OVERLAY_FULL: (f64, f64) = (460.0, 104.0);

pub(crate) const OVERLAY_MINI: (f64, f64) = (150.0, 56.0);

/// Argumento que o autostart passa ao ISPer: nesse caso ele nasce quieto na
/// bandeja, sem abrir a tela Início.
pub(crate) const AUTOSTART_FLAG: &str = "--autostart";

/// Reuniões listadas na tela Início.
pub(crate) const HOME_RECENT: i64 = 5;

/// Máquina de estados do ditado — um único lugar decide o que cada evento
/// de tecla significa (inclusive o auto-repeat, que dispara `Pressed`
/// repetido enquanto a tecla está segurada).
pub(crate) enum Phase {
    Idle,
    Recording { started: Instant, handsfree: bool },
    Processing,
}

/// Estado do motor Whisper — fonte única para a tela Início responder
/// "por que não transcreve?" sem adivinhar.
#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum EngineStatus {
    Loading,
    Ready { file: String, label: String },
    Missing,
    Failed { message: String },
}

pub(crate) struct AppState {
    /// Carregado em background no startup; `None` enquanto carrega.
    pub(crate) engine: Mutex<Option<Arc<WhisperEngine>>>,
    pub(crate) engine_status: Mutex<EngineStatus>,
    pub(crate) audio: recorder::AudioHandle,
    pub(crate) phase: Mutex<Phase>,
    pub(crate) config: Mutex<AppConfig>,
    /// Rótulo do atalho registrado no momento (p/ a bandeja e as configurações).
    pub(crate) active_shortcut: Mutex<String>,
    /// Gravação de reunião em andamento (Fase 4).
    pub(crate) meeting: Mutex<Option<MeetingHandle>>,
    /// Quando a reunião atual começou (cronômetro da tela Início).
    pub(crate) meeting_started: Mutex<Option<Instant>>,
    /// Reunião que a Biblioteca deve abrir já selecionada.
    pub(crate) pending_meeting: Mutex<Option<i64>>,
    /// Itens do menu da bandeja cujo texto muda em tempo de execução.
    pub(crate) meeting_item: Mutex<Option<MenuItem<tauri::Wry>>>,
    pub(crate) hint_item: Mutex<Option<MenuItem<tauri::Wry>>>,
    /// HWND do indicador (0 fora do Windows) — p/ reafirmar o topo sem passar pelo tao.
    pub(crate) overlay_hwnd: isize,
    /// Atalhos registrados no momento — o handler precisa saber qual disparou.
    pub(crate) dictation_shortcut: Mutex<Option<Shortcut>>,
    pub(crate) meeting_shortcut: Mutex<Option<Shortcut>>,
    pub(crate) active_meeting_shortcut: Mutex<String>,
    pub(crate) last_meeting_toggle: Mutex<Option<Instant>>,
    /// Falas da reunião em andamento, na ordem em que foram transcritas.
    pub(crate) live: Mutex<Vec<LiveSegment>>,
    /// Reunião cuja diarização está rodando em segundo plano (chip no Início).
    pub(crate) diarizing: Mutex<Option<i64>>,
    /// Ícone da bandeja e suas duas versões (normal / gravando).
    pub(crate) tray: Mutex<Option<TrayIcon>>,
    pub(crate) tray_icons: Mutex<Option<(Image<'static>, Image<'static>)>>,
}

/// Uma fala transcrita durante a reunião (evento `isper-live`).
#[derive(Clone, serde::Serialize)]
pub(crate) struct LiveSegment {
    pub(crate) speaker: String,
    pub(crate) start_secs: f32,
    pub(crate) end_secs: f32,
    pub(crate) text: String,
}

pub(crate) fn open_store() -> anyhow::Result<MeetingStore> {
    let dir = PathBuf::from(std::env::var("APPDATA")?).join("ISPer");
    std::fs::create_dir_all(&dir)?;
    Ok(MeetingStore::open(&dir.join("isper.db"))?)
}

/// `Documentos\ISPer\Reunioes` — onde os Markdowns das reuniões moram.
pub(crate) fn meetings_dir() -> anyhow::Result<PathBuf> {
    let dir = PathBuf::from(std::env::var("USERPROFILE")?)
        .join("Documents")
        .join("ISPer")
        .join("Reunioes");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Em desenvolvimento, o `models/` do repositório também vale como fonte.
pub(crate) fn dev_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        dirs.extend(cwd.ancestors().take(5).map(|d| d.to_path_buf()));
    }
    if let Ok(exe) = std::env::current_exe() {
        dirs.extend(exe.ancestors().take(7).map(|d| d.to_path_buf()));
    }
    dirs
}

/// Avisa todas as janelas que o estado mudou (modelo, reunião, configurações,
/// downloads) — a tela Início relê `home_status` e a Biblioteca, sua lista.
pub(crate) fn notify_status(app: &AppHandle) {
    let _ = app.emit("isper-status", ());
}

/// `%LOCALAPPDATA%\ISPer\logs` — um arquivo por dia, 14 dias guardados.
pub(crate) fn logs_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var("LOCALAPPDATA").ok()?)
        .join("ISPer")
        .join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}
