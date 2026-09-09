//! Tela Início: a fotografia do estado do sistema que a janela renderiza.

use crate::prelude::*;
use tauri_plugin_autostart::ManagerExt;

#[derive(serde::Serialize)]
pub(crate) struct HomeStatus {
    version: &'static str,
    gpu: bool,
    engine: EngineStatus,
    shortcut: String,
    lang: String,
    dictionary_terms: usize,
    meeting_active: bool,
    meeting_elapsed_secs: Option<u64>,
    meeting_source: String,
    diarize_installed: bool,
    llm_provider: Option<String>,
    llm_model: Option<String>,
    llm_key_present: bool,
    autostart: bool,
    show_home_on_launch: bool,
    meetings_dir: String,
    stats: isper_core::store::Stats,
    recent: Vec<isper_core::store::MeetingRow>,
    meeting_shortcut: String,
    polish: bool,
    polish_style: String,
    input_device: Option<String>,
    /// Reunião com diarização em andamento (depois de salva).
    diarizing_meeting: Option<i64>,
}

/// Fotografia de tudo que a tela Início mostra — uma chamada, sem estado no
/// front (que só renderiza e reage ao evento `isper-status`).
#[tauri::command]
pub(crate) fn home_status(app: AppHandle) -> HomeStatus {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().unwrap().clone();
    let engine = state.engine_status.lock().unwrap().clone();
    let shortcut = state.active_shortcut.lock().unwrap().clone();
    let meeting_shortcut = state.active_meeting_shortcut.lock().unwrap().clone();
    let diarizing_meeting = *state.diarizing.lock().unwrap();
    let meeting_active = state.meeting.lock().unwrap().is_some();
    let meeting_elapsed_secs = state
        .meeting_started
        .lock()
        .unwrap()
        .map(|t| t.elapsed().as_secs());

    let llm = isper_llm::load_settings();
    let (llm_provider, llm_model, llm_key_present) = if llm.provider.is_empty() {
        (None, None, false)
    } else {
        let key_present = isper_llm::get_api_key(&llm.provider).ok().flatten().is_some();
        // Sem modelo escolhido, mostra o padrão do provider (só resolve com chave).
        let model = llm.model.clone().or_else(|| {
            isper_llm::provider_from_settings(&llm)
                .ok()
                .map(|p| p.model().to_string())
        });
        (Some(llm.provider.clone()), model, key_present)
    };

    let (stats, recent) = match open_store() {
        Ok(store) => (
            store.stats().unwrap_or_default(),
            store.recent_meetings(HOME_RECENT).unwrap_or_default(),
        ),
        Err(e) => {
            tracing::warn!("banco indisponível para a tela Início: {e}");
            Default::default()
        }
    };

    HomeStatus {
        version: env!("CARGO_PKG_VERSION"),
        gpu: cfg!(feature = "cuda"),
        engine,
        shortcut,
        lang: cfg.lang,
        dictionary_terms: cfg.dictionary.len(),
        meeting_active,
        meeting_elapsed_secs,
        meeting_source: cfg.meeting_source,
        diarize_installed: isper_diarize::models_installed(),
        llm_provider,
        llm_model,
        llm_key_present,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        show_home_on_launch: cfg.show_home_on_launch,
        meetings_dir: meetings_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        stats,
        recent,
        meeting_shortcut,
        polish: cfg.polish,
        polish_style: cfg.polish_style,
        input_device: cfg.input_device,
        diarizing_meeting,
    }
}
