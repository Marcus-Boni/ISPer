//! Primeira execução (fase 7.5): uma janela guiada — boas-vindas com idioma
//! e tema, microfone com medidor de nível, modelo Whisper, atalho com um
//! ditado de teste e IA opcional. Aparece uma vez: concluir, pular ou fechar
//! a janela marcam `onboarding_done` e abrem a tela Início. Quem já usava o
//! ISPer antes dela não a vê (migração 1 → 2 do `config.toml`); Configurações
//! → Sistema a reabre.

use std::sync::atomic::{AtomicBool, Ordering};

use isper_core::audio::MonitorEnd;

use crate::config::AppConfig;
use crate::prelude::*;

pub(crate) const LABEL: &str = "onboarding";

/// Duração máxima de um teste de microfone; a tela oferece testar de novo.
const MIC_TEST_MAX: Duration = Duration::from_secs(60);

/// Pedido de parada do teste de microfone em andamento (um por vez).
static MIC_TEST: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

/// Qual janela abre ao iniciar: a primeira configuração (uma vez), a tela
/// Início (se o usuário não a desligou) ou nenhuma — no autostart o ISPer
/// nasce quieto na bandeja.
pub(crate) fn launch_window(cfg: &AppConfig, autostarted: bool) -> Option<&'static str> {
    if autostarted {
        None
    } else if !cfg.onboarding_done {
        Some(LABEL)
    } else {
        cfg.show_home_on_launch.then_some("home")
    }
}

/// O modelo que a primeira configuração sugere: com GPU, o Large v3 Turbo;
/// sem, o Small (o Large na CPU demora mais que a fala).
pub(crate) fn recommended_model(gpu: bool) -> &'static str {
    if gpu {
        "ggml-large-v3-turbo-q5_0.bin"
    } else {
        "ggml-small.bin"
    }
}

/// Grava uma mudança na config (em memória e no disco), com a mesma
/// normalização do resto do app. Devolve a config anterior e a nova.
fn update_config(
    app: &AppHandle,
    change: impl FnOnce(&mut AppConfig),
) -> Result<(AppConfig, AppConfig), String> {
    let state = app.state::<AppState>();
    let (previous, cfg) = {
        let mut guard = state.config.lock_or_recover();
        let previous = guard.clone();
        change(&mut guard);
        guard.normalize();
        (previous, guard.clone())
    };
    config::save(&cfg).map_err(|e| e.to_string())?;
    Ok((previous, cfg))
}

#[derive(serde::Serialize)]
pub(crate) struct OnboardingState {
    gpu: bool,
    recommended: &'static str,
    devices: Vec<String>,
    input_device: Option<String>,
    shortcut: Option<String>,
    active_shortcut: String,
    llm_provider: String,
    llm_key_present: bool,
    ui_lang: String,
    theme: String,
}

/// Tudo o que a janela precisa para montar os passos, de uma vez.
#[tauri::command]
pub(crate) fn onboarding_state(app: AppHandle) -> OnboardingState {
    let state = app.state::<AppState>();
    let cfg = state.config.lock_or_recover().clone();
    let active_shortcut = state.active_shortcut.lock_or_recover().clone();
    let llm = isper_llm::load_settings();
    let llm_key_present = !llm.provider.is_empty()
        && isper_llm::get_api_key(&llm.provider)
            .map(|k| k.is_some())
            .unwrap_or(false);
    let gpu = cfg!(feature = "cuda");
    OnboardingState {
        gpu,
        recommended: recommended_model(gpu),
        devices: isper_core::audio::list_input_devices(),
        input_device: cfg.input_device,
        shortcut: cfg.shortcut,
        active_shortcut,
        llm_provider: llm.provider,
        llm_key_present,
        ui_lang: cfg.ui_lang,
        theme: cfg.theme,
    }
}

/// Começa a medir o nível de `device` (ou do padrão): a janela recebe
/// `isper-mic-level` (RMS, ~20 por segundo) e, se a medição acabar sozinha,
/// `isper-mic-end` com o motivo. Nada é gravado. Um teste novo encerra o
/// anterior.
#[tauri::command]
pub(crate) fn mic_test_start(app: AppHandle, device: Option<String>) {
    let stop = Arc::new(AtomicBool::new(false));
    if let Some(previous) = MIC_TEST.lock_or_recover().replace(stop.clone()) {
        previous.store(true, Ordering::Relaxed);
    }
    std::thread::spawn(move || {
        let device = device.filter(|d| !d.trim().is_empty());
        let result =
            isper_core::audio::monitor_input(device.as_deref(), MIC_TEST_MAX, &stop, |rms| {
                let _ = app.emit_to(LABEL, "isper-mic-level", rms);
            });
        {
            let mut slot = MIC_TEST.lock_or_recover();
            if slot.as_ref().is_some_and(|s| Arc::ptr_eq(s, &stop)) {
                *slot = None;
            }
        }
        let (reason, message) = match result {
            // Parada pedida pela tela (ou por um teste novo): ela já sabe.
            Ok(MonitorEnd::Stopped) => return,
            Ok(MonitorEnd::Timeout) => ("timeout", None),
            Ok(MonitorEnd::NoAudio) => ("no-audio", None),
            Err(e) => (
                "error",
                Some(isper_core::audio::describe_error(&e.to_string())),
            ),
        };
        if reason != "timeout" {
            tracing::warn!(
                reason,
                ?message,
                "teste de microfone da primeira configuração"
            );
        }
        let _ = app.emit_to(
            LABEL,
            "isper-mic-end",
            json!({ "reason": reason, "message": message }),
        );
    });
}

#[tauri::command]
pub(crate) fn mic_test_stop() {
    if let Some(stop) = MIC_TEST.lock_or_recover().take() {
        stop.store(true, Ordering::Relaxed);
    }
}

/// Microfone escolhido: vale na hora para o ditado e para o canal "Eu".
#[tauri::command]
pub(crate) fn onboarding_set_mic(app: AppHandle, device: Option<String>) -> Result<(), String> {
    let (_, cfg) = update_config(&app, |c| c.input_device = device)?;
    app.state::<AppState>()
        .audio
        .set_device(cfg.input_device.clone());
    Ok(())
}

/// Atalho do ditado (`None` = o primeiro livre). Devolve o rótulo do que foi
/// registrado — pode ser outro, se o pedido estiver ocupado.
#[tauri::command]
pub(crate) fn onboarding_set_shortcut(
    app: AppHandle,
    shortcut: Option<String>,
) -> Result<String, String> {
    let (_, cfg) = update_config(&app, |c| c.shortcut = shortcut)?;
    let (label, _, _, _) = register_shortcuts(&app, &cfg);
    set_hint(&app, &label);
    let recording = app.state::<AppState>().meeting.lock_or_recover().is_some();
    set_meeting_text(&app, &meeting_item_text(&app, recording));
    notify_status(&app);
    Ok(label)
}

/// Modelo preferido. Se ele já estiver instalado e não for o carregado,
/// carrega; se não, o `download_model` que vem em seguida carrega sozinho.
#[tauri::command]
pub(crate) fn onboarding_set_model(app: AppHandle, file: String) -> Result<(), String> {
    if isper_models::catalog_entry(&file).is_none() {
        return Err(format!("modelo fora do catálogo: {file}"));
    }
    update_config(&app, |c| c.model = Some(file.clone()))?;
    let status = app
        .state::<AppState>()
        .engine_status
        .lock_or_recover()
        .clone();
    let needs_load = match status {
        EngineStatus::Loading => false,
        EngineStatus::Ready { file: loaded, .. } => loaded != file,
        EngineStatus::Missing | EngineStatus::Failed { .. } => true,
    };
    if needs_load && isper_models::installed_path(&file).is_some() {
        load_engine_in_background(app);
    }
    Ok(())
}

/// Provider de IA escolhido no último passo (`none` desliga). A chave vai
/// por `set_llm_key`, como nas Configurações. Devolve se esse provider já
/// tem chave guardada (as chaves são uma por provider).
#[tauri::command]
pub(crate) fn onboarding_set_provider(app: AppHandle, provider: String) -> Result<bool, String> {
    let provider = provider.trim().to_lowercase();
    let provider = if provider == "none" {
        String::new()
    } else {
        provider
    };
    let mut settings = isper_llm::load_settings();
    if settings.provider != provider {
        // O modelo específico é de um provider ("openai/gpt-oss-120b" é do
        // Groq): trocando de provider, volta ao padrão do novo.
        settings.model = None;
    }
    settings.provider = provider;
    isper_llm::save_settings(&settings).map_err(|e| e.to_string())?;
    notify_status(&app);
    let key_present = !settings.provider.is_empty()
        && isper_llm::get_api_key(&settings.provider)
            .map(|k| k.is_some())
            .unwrap_or(false);
    Ok(key_present)
}

/// Concluir ou pular: fecha a janela, e o fechamento marca a configuração
/// como feita e abre a tela Início.
#[tauri::command]
pub(crate) fn onboarding_finish(app: AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.close();
    } else {
        closed(&app);
    }
}

/// Reabre a primeira configuração (Configurações → Sistema).
#[tauri::command]
pub(crate) async fn open_onboarding_window(app: AppHandle) -> Result<(), String> {
    open_onboarding(&app);
    Ok(())
}

/// A janela fechou — pelo "Concluir", pelo "Pular" ou pelo × da barra de
/// título: em qualquer caso a configuração não volta sozinha, e a tela
/// Início (com o checklist do que faltou) assume.
pub(crate) fn closed(app: &AppHandle) {
    mic_test_stop();
    match update_config(app, |c| c.onboarding_done = true) {
        Ok((previous, _)) if !previous.onboarding_done => {
            tracing::info!("primeira configuração concluída");
        }
        Ok(_) => {}
        Err(e) => tracing::warn!("não consegui gravar o fim da primeira configuração: {e}"),
    }
    open_home(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primeira_execucao_abre_a_configuracao_e_depois_o_inicio() {
        let mut cfg = AppConfig::default();
        assert_eq!(launch_window(&cfg, false), Some(LABEL));
        assert_eq!(launch_window(&cfg, true), None, "autostart fica na bandeja");
        cfg.onboarding_done = true;
        assert_eq!(launch_window(&cfg, false), Some("home"));
        cfg.show_home_on_launch = false;
        assert_eq!(launch_window(&cfg, false), None);
    }

    #[test]
    fn modelos_sugeridos_existem_e_o_da_cpu_nao_pede_gpu() {
        for gpu in [true, false] {
            let file = recommended_model(gpu);
            let entry = isper_models::catalog_entry(file).expect("no catálogo");
            if !gpu {
                assert!(!entry.needs_gpu, "{file} sem GPU");
            }
        }
    }
}
