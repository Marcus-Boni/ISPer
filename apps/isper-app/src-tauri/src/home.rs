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
    mark_shortcut: String,
    /// Versão nova encontrada pela checagem automática (banner).
    update: Option<UpdateInfo>,
    /// Indicador em modo legendas ao vivo (a tela Início mostra o interruptor ligado).
    overlay_captions: bool,
    polish: bool,
    polish_style: String,
    input_device: Option<String>,
    /// Reunião com diarização em andamento (depois de salva).
    diarizing_meeting: Option<i64>,
    voice_commands: bool,
    /// Chamada do Teams em andamento (banner "Gravar transcrição?").
    call: Option<CallInfo>,
    /// A chamada terminou com a gravação ainda ligada (banner "encerrar?").
    call_ended: bool,
    /// O usuário dispensou o aviso desta chamada ("agora não" / "continuar gravando").
    call_dismissed: bool,
    /// `notify` · `auto` · `off`.
    call_detect: String,
    /// Indicador flutuante: na tela agora / fixo em repouso.
    overlay_visible: bool,
    overlay_pinned: bool,
    /// Insights ao vivo (painel do card de reunião).
    insights: InsightsDto,
    /// Busca semântica configurada (a Biblioteca mostra o modo "Semântica").
    semantic_search: bool,
}

/// O que o Início mostra sobre a IA: (provider, modelo, chave presente). Sem
/// provider escolhido não há nada a mostrar — e nem se consulta a chave. Sem
/// modelo escolhido vale o padrão do provider, resolvido por `default_model`
/// (que precisa da chave, por isso é preguiçoso).
pub(crate) fn llm_summary(
    llm: &isper_llm::LlmSettings,
    key_present: impl FnOnce(&str) -> bool,
    default_model: impl FnOnce() -> Option<String>,
) -> (Option<String>, Option<String>, bool) {
    let provider = llm.provider.trim();
    if provider.is_empty() {
        return (None, None, false);
    }
    let has_key = key_present(provider);
    let model = llm
        .model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .or_else(default_model);
    (Some(provider.to_string()), model, has_key)
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
    let mark_shortcut = state.active_mark_shortcut.lock().unwrap().clone();
    let update = state.update_available.lock().unwrap().clone();
    let diarizing_meeting = *state.diarizing.lock().unwrap();
    let meeting_active = state.meeting.lock().unwrap().is_some();
    let meeting_elapsed_secs = state
        .meeting_started
        .lock()
        .unwrap()
        .map(|t| t.elapsed().as_secs());
    let (call, call_ended, call_dismissed) = call_info(&app);

    let llm = isper_llm::load_settings();
    let (llm_provider, llm_model, llm_key_present) = llm_summary(
        &llm,
        |provider| isper_llm::get_api_key(provider).ok().flatten().is_some(),
        || {
            isper_llm::provider_from_settings(&llm)
                .ok()
                .map(|p| p.model().to_string())
        },
    );

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
        mark_shortcut,
        update,
        overlay_captions: cfg.overlay_captions,
        polish: cfg.polish,
        polish_style: cfg.polish_style,
        input_device: cfg.input_device,
        diarizing_meeting,
        voice_commands: cfg.voice_commands,
        call,
        call_ended,
        call_dismissed,
        call_detect: cfg.call_detect,
        overlay_visible: overlay_visible(&app),
        overlay_pinned: cfg.overlay_pinned,
        insights: insights_dto(&app),
        semantic_search: llm.embeddings.is_configured(),
    }
}

#[cfg(test)]
mod tests {
    use super::llm_summary;
    use std::cell::Cell;

    fn settings(provider: &str, model: Option<&str>) -> isper_llm::LlmSettings {
        isper_llm::LlmSettings {
            provider: provider.into(),
            model: model.map(str::to_string),
            embeddings: isper_llm::EmbeddingSettings::default(),
        }
    }

    #[test]
    fn sem_provider_nao_consulta_chave_nem_modelo() {
        let asked = Cell::new(false);
        let out = llm_summary(
            &settings("  ", None),
            |_| {
                asked.set(true);
                true
            },
            || {
                asked.set(true);
                Some("x".into())
            },
        );
        assert_eq!(out, (None, None, false));
        assert!(!asked.get());
    }

    #[test]
    fn modelo_escolhido_dispensa_o_padrao_e_vazio_usa_o_padrao() {
        let out = llm_summary(
            &settings("groq", Some("openai/gpt-oss-120b")),
            |p| p == "groq",
            || panic!("não deveria resolver o padrão"),
        );
        assert_eq!(
            out,
            (
                Some("groq".into()),
                Some("openai/gpt-oss-120b".into()),
                true
            )
        );
        let out = llm_summary(
            &settings("gemini", Some("  ")),
            |_| false,
            || Some("gemini-3.5-flash-lite".into()),
        );
        assert_eq!(
            out,
            (
                Some("gemini".into()),
                Some("gemini-3.5-flash-lite".into()),
                false
            )
        );
        // Sem chave, o padrão pode não resolver: o Início mostra só o provider.
        let out = llm_summary(&settings("claude", None), |_| false, || None);
        assert_eq!(out, (Some("claude".into()), None, false));
    }
}
