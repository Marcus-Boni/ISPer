//! Configurações: DTOs, modelos Whisper, IA, diagnóstico e aplicação a quente.

use crate::config::AppConfig;
use crate::prelude::*;
use isper_core::WhisperEngine;
use tauri_plugin_autostart::ManagerExt;

#[derive(serde::Serialize)]
pub(crate) struct SettingsDto {
    shortcut: Option<String>,
    active_shortcut: String,
    lang: String,
    dictionary: String,
    model: Option<String>,
    meeting_source: String,
    has_gpu: bool,
    llm_provider: String,
    llm_model: Option<String>,
    llm_key_present: bool,
    autostart: bool,
    show_home_on_launch: bool,
    input_device: Option<String>,
    meeting_shortcut: Option<String>,
    active_meeting_shortcut: String,
    mark_shortcut: Option<String>,
    active_mark_shortcut: String,
    copilot_shortcut: Option<String>,
    active_copilot_shortcut: String,
    polish: bool,
    polish_style: String,
    after_meeting: String,
    voice_commands: bool,
    auto_update_check: bool,
    version: String,
    /// `notify` · `auto` · `off`.
    call_detect: String,
    live_insights: bool,
    insights_interval_min: u32,
    /// Busca semântica: provider (`none` = desligada), modelo, base URL e chave.
    emb_provider: String,
    emb_model: Option<String>,
    emb_base_url: Option<String>,
    emb_key_present: bool,
    /// Retenção de reuniões e ditados, em dias (0 = para sempre).
    retention_days: u32,
    /// Passe final ligado (refaz a transcrição ao encerrar a reunião).
    final_pass: bool,
    /// Transcrever o que cair na pasta Importar (Fase 9.0).
    import_watch: bool,
    /// A pasta Importar, para a tela mostrar o caminho.
    import_dir: String,
    /// Participantes conhecidos da reunião (0 = descobrir pelo agrupamento).
    meeting_speakers: u32,
    /// Limiar do agrupamento de falantes (0 = o padrão do projeto).
    diarize_threshold: f32,
    /// Tema da interface (`system` · `light` · `dark`) — muda por `set_ui_theme`.
    theme: String,
    /// Idioma da interface (`auto` · `pt-BR` · `en`) — muda por `set_ui_lang`.
    ui_lang: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct SettingsPatch {
    shortcut: Option<String>,
    lang: String,
    dictionary: String,
    model: Option<String>,
    meeting_source: String,
    llm_provider: String,
    llm_model: Option<String>,
    autostart: bool,
    #[serde(default = "default_true")]
    show_home_on_launch: bool,
    #[serde(default)]
    input_device: Option<String>,
    #[serde(default)]
    meeting_shortcut: Option<String>,
    #[serde(default)]
    mark_shortcut: Option<String>,
    #[serde(default)]
    copilot_shortcut: Option<String>,
    #[serde(default)]
    polish: bool,
    #[serde(default)]
    polish_style: Option<String>,
    #[serde(default)]
    after_meeting: Option<String>,
    #[serde(default = "default_true")]
    voice_commands: bool,
    #[serde(default = "default_true")]
    auto_update_check: bool,
    #[serde(default)]
    call_detect: Option<String>,
    #[serde(default)]
    live_insights: bool,
    #[serde(default)]
    insights_interval_min: Option<u32>,
    #[serde(default)]
    emb_provider: Option<String>,
    #[serde(default)]
    emb_model: Option<String>,
    #[serde(default)]
    emb_base_url: Option<String>,
    #[serde(default)]
    retention_days: Option<u32>,
    /// Avançado — passe final ligado (padrão) ou não.
    #[serde(default)]
    final_pass: Option<bool>,
    /// Vigiar a pasta Importar (ausente = mantém).
    #[serde(default)]
    import_watch: Option<bool>,
    /// Avançado — quantos participantes a reunião tem (0 = descobrir).
    #[serde(default)]
    meeting_speakers: Option<u32>,
    /// Avançado — limiar do agrupamento de falantes (0 = padrão do projeto).
    #[serde(default)]
    diarize_threshold: Option<f32>,
}

pub(crate) fn default_true() -> bool {
    true
}

#[derive(serde::Serialize)]
pub(crate) struct ModelDto {
    file: String,
    label: String,
    approx_mb: u32,
    note: String,
    needs_gpu: bool,
    installed: bool,
    active: bool,
}

/// Carrega (ou recarrega) o modelo Whisper em background, publicando cada
/// passo em `EngineStatus`. Sem nenhum modelo instalado, a tela Início (se
/// aberta) orienta o download; senão, abrem-se as Configurações.
pub(crate) fn load_engine_in_background(app: AppHandle) {
    std::thread::spawn(move || {
        set_engine_status(&app, EngineStatus::Loading);
        let preferred = app
            .state::<AppState>()
            .config
            .lock_or_recover()
            .model
            .clone();
        let Some(path) = isper_models::resolve_whisper_model(
            preferred.as_deref(),
            cfg!(feature = "cuda"),
            &dev_dirs(),
        ) else {
            tracing::warn!("nenhum modelo instalado");
            set_engine_status(&app, EngineStatus::Missing);
            if app.get_webview_window("home").is_none()
                && app.get_webview_window(crate::onboarding::LABEL).is_none()
            {
                open_settings(&app);
            }
            return;
        };
        tracing::info!("carregando modelo {}", path.display());
        match WhisperEngine::new(&path) {
            Ok(engine) => {
                *app.state::<AppState>().engine.lock_or_recover() = Some(Arc::new(engine));
                let file = path
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let label = isper_models::catalog_entry(&file)
                    .map(|m| m.label.to_string())
                    .unwrap_or_else(|| {
                        file.trim_start_matches("ggml-")
                            .trim_end_matches(".bin")
                            .to_string()
                    });
                set_engine_status(&app, EngineStatus::Ready { file, label });
                tracing::info!("modelo Whisper carregado");
            }
            Err(e) => {
                tracing::error!("falha ao carregar modelo: {e}");
                set_engine_status(
                    &app,
                    EngineStatus::Failed {
                        message: e.to_string(),
                    },
                );
                let _ = app.emit_to(
                    "overlay",
                    "isper-state",
                    json!({"state": "error", "message": e.to_string()}),
                );
            }
        }
    });
}

pub(crate) fn set_engine_status(app: &AppHandle, status: EngineStatus) {
    *app.state::<AppState>().engine_status.lock_or_recover() = status;
    notify_status(app);
}

#[tauri::command]
pub(crate) fn models_status(app: AppHandle) -> Vec<ModelDto> {
    let preferred = app
        .state::<AppState>()
        .config
        .lock_or_recover()
        .model
        .clone();
    let dirs = dev_dirs();
    let active_file =
        isper_models::resolve_whisper_model(preferred.as_deref(), cfg!(feature = "cuda"), &dirs)
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()));
    // Removido há instantes (janela do Desfazer aberta): já aparece como não instalado.
    let hidden_models = crate::undo::hidden_models();
    isper_models::WHISPER_CATALOG
        .iter()
        .map(|m| ModelDto {
            file: m.file.to_string(),
            label: m.label.to_string(),
            approx_mb: m.approx_mb,
            note: m.note.to_string(),
            needs_gpu: m.needs_gpu,
            installed: !hidden_models.iter().any(|f| f == m.file)
                && isper_models::installed_path(m.file).is_some()
                || dirs.iter().any(|d| d.join("models").join(m.file).exists()),
            active: active_file.as_deref() == Some(m.file),
        })
        .collect()
}

/// Baixa um modelo do catálogo emitindo `isper-model-progress` para as
/// Configurações e a primeira configuração. Se ainda não havia modelo
/// carregado, carrega.
#[tauri::command]
pub(crate) async fn download_model(app: AppHandle, file: String) -> Result<(), String> {
    let app2 = app.clone();
    let file2 = file.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut last = 0u64;
        isper_models::download_whisper(&file2, &mut |done, total| {
            // No máximo ~1 evento por MB — a UI não precisa de mais.
            if done - last >= 1_000_000 || done == total {
                last = done;
                let progress = json!({"file": file2, "done": done, "total": total});
                for label in ["settings", crate::onboarding::LABEL] {
                    let _ = app2.emit_to(label, "isper-model-progress", progress.clone());
                }
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    if app.state::<AppState>().engine.lock_or_recover().is_none() {
        load_engine_in_background(app.clone());
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn delete_model(file: String) -> crate::undo::Scheduled {
    crate::undo::schedule(crate::undo::Doomed::Model(file))
}

#[tauri::command]
pub(crate) fn diarize_status() -> bool {
    isper_diarize::models_installed()
}

/// Baixa os modelos de diarização, com progresso em `isper-model-progress`
/// (file = "diarize").
#[tauri::command]
pub(crate) async fn download_diarize_models(app: AppHandle) -> Result<(), String> {
    let progress_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut last = 0u64;
        isper_diarize::download_models(&mut |name, done, total| {
            if done - last >= 500_000 || done == total {
                last = done;
                let _ = progress_app.emit_to(
                    "settings",
                    "isper-model-progress",
                    json!({"file": "diarize", "name": name, "done": done, "total": total}),
                );
            }
        })
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    notify_status(&app);
    Ok(())
}

#[tauri::command]
pub(crate) fn get_settings(app: AppHandle) -> Result<SettingsDto, String> {
    let state = app.state::<AppState>();
    let cfg = state.config.lock_or_recover().clone();
    let active_shortcut = state.active_shortcut.lock_or_recover().clone();
    let active_meeting_shortcut = state.active_meeting_shortcut.lock_or_recover().clone();
    let active_mark_shortcut = state.active_mark_shortcut.lock_or_recover().clone();
    let active_copilot_shortcut = state.active_copilot_shortcut.lock_or_recover().clone();
    let llm = isper_llm::load_settings();
    let llm_key_present = if llm.provider.is_empty() {
        false
    } else {
        isper_llm::get_api_key(&llm.provider)
            .map(|k| k.is_some())
            .unwrap_or(false)
    };
    let emb = llm.embeddings.clone();
    let emb_provider = emb.provider.trim().to_lowercase();
    let emb_key_present = match emb_provider.as_str() {
        "gemini" | "google" => isper_llm::get_api_key("gemini").ok().flatten().is_some(),
        "openai" | "ollama" => isper_llm::get_api_key(isper_llm::embeddings::OPENAI_COMPAT_KEY)
            .ok()
            .flatten()
            .is_some(),
        _ => false,
    };
    Ok(SettingsDto {
        shortcut: cfg.shortcut,
        active_shortcut,
        lang: cfg.lang,
        dictionary: cfg.dictionary.join("\n"),
        model: cfg.model,
        meeting_source: cfg.meeting_source,
        has_gpu: cfg!(feature = "cuda"),
        llm_provider: if llm.provider.is_empty() {
            "none".into()
        } else {
            llm.provider
        },
        llm_model: llm.model,
        llm_key_present,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        show_home_on_launch: cfg.show_home_on_launch,
        input_device: cfg.input_device,
        meeting_shortcut: cfg.meeting_shortcut,
        active_meeting_shortcut,
        mark_shortcut: cfg.mark_shortcut,
        active_mark_shortcut,
        copilot_shortcut: cfg.copilot_shortcut,
        active_copilot_shortcut,
        polish: cfg.polish,
        polish_style: cfg.polish_style,
        after_meeting: cfg.after_meeting,
        voice_commands: cfg.voice_commands,
        auto_update_check: cfg.auto_update_check,
        version: env!("CARGO_PKG_VERSION").to_string(),
        call_detect: cfg.call_detect,
        live_insights: cfg.live_insights,
        insights_interval_min: cfg.insights_interval_min,
        emb_provider: if emb.is_configured() {
            emb_provider
        } else {
            "none".into()
        },
        emb_model: emb.model,
        emb_base_url: emb.base_url,
        emb_key_present,
        retention_days: cfg.retention_days,
        final_pass: cfg.final_pass,
        import_watch: cfg.import_watch,
        import_dir: crate::audio_import::import_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_default(),
        meeting_speakers: cfg.meeting_speakers,
        diarize_threshold: cfg.diarize_threshold,
        theme: cfg.theme,
        ui_lang: cfg.ui_lang,
    })
}

#[tauri::command]
pub(crate) fn apply_settings(app: AppHandle, patch: SettingsPatch) -> Result<String, String> {
    let state = app.state::<AppState>();

    let dictionary: Vec<String> = patch
        .dictionary
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let previous = state.config.lock_or_recover().clone();
    let previous_model = previous.model.clone();
    let mut cfg = AppConfig {
        shortcut: patch.shortcut,
        lang: patch.lang,
        dictionary,
        model: patch.model,
        meeting_source: patch.meeting_source,
        // Preferências do indicador não passam pela tela — preserva as atuais.
        overlay_pos: previous.overlay_pos,
        overlay_mini: previous.overlay_mini,
        overlay_captions: previous.overlay_captions,
        auto_update_check: patch.auto_update_check,
        show_home_on_launch: patch.show_home_on_launch,
        input_device: patch.input_device,
        meeting_shortcut: patch.meeting_shortcut,
        mark_shortcut: patch.mark_shortcut,
        copilot_shortcut: patch.copilot_shortcut,
        polish: patch.polish,
        polish_style: patch.polish_style.unwrap_or_default(),
        after_meeting: patch.after_meeting.unwrap_or_default(),
        voice_commands: patch.voice_commands,
        call_detect: patch.call_detect.unwrap_or_default(),
        live_insights: patch.live_insights,
        insights_interval_min: patch.insights_interval_min.unwrap_or_default(),
        overlay_pinned: previous.overlay_pinned,
        retention_days: patch.retention_days.unwrap_or_default(),
        // Avançado: o que a tela não mandar mantém o valor atual.
        final_pass: patch.final_pass.unwrap_or(previous.final_pass),
        import_watch: patch.import_watch.unwrap_or(previous.import_watch),
        meeting_speakers: patch.meeting_speakers.unwrap_or(previous.meeting_speakers),
        diarize_threshold: patch
            .diarize_threshold
            .unwrap_or(previous.diarize_threshold),
        // O tema muda na hora por `set_ui_theme`, não pelo Salvar.
        theme: previous.theme.clone(),
        ui_lang: previous.ui_lang.clone(),
        onboarding_done: previous.onboarding_done,
        config_version: config::CONFIG_VERSION,
    };
    // Caixa, espaços, vazios e valores fora das listas: a mesma regra única
    // que vale para o config.toml (`AppConfig::normalize`, com testes).
    cfg.normalize();
    config::save(&cfg).map_err(|e| e.to_string())?;
    *state.config.lock_or_recover() = cfg.clone();
    state.audio.set_device(cfg.input_device.clone());

    // Prazo de retenção novo (ou mais curto): aplica agora, em segundo plano.
    if cfg.retention_days != 0 && cfg.retention_days != previous.retention_days {
        sweep_in_background(&app);
    }

    // Troca de modelo a quente: o antigo continua servindo até o novo carregar.
    if cfg.model != previous_model {
        load_engine_in_background(app.clone());
    }

    // Reaplica os atalhos na hora — sem reiniciar o app.
    let (label, _meeting_label, _mark_label, _copilot_label) = register_shortcuts(&app, &cfg);
    set_hint(&app, &label);
    let recording = state.meeting.lock_or_recover().is_some();
    set_meeting_text(&app, &meeting_item_text(&app, recording));

    // Provider de IA (a chave é gravada separadamente, via set_llm_key).
    let provider = if patch.llm_provider == "none" {
        String::new()
    } else {
        patch.llm_provider.trim().to_lowercase()
    };
    let model = patch.llm_model.filter(|m| !m.trim().is_empty());
    // Busca semântica: provider próprio (Gemini ou endpoint compatível com OpenAI).
    let emb_provider = patch.emb_provider.unwrap_or_default().trim().to_lowercase();
    let embeddings = isper_llm::EmbeddingSettings {
        provider: if emb_provider == "none" {
            String::new()
        } else {
            emb_provider
        },
        model: patch
            .emb_model
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty()),
        base_url: patch
            .emb_base_url
            .map(|u| u.trim().trim_end_matches('/').to_string())
            .filter(|u| !u.is_empty()),
    };
    isper_llm::save_settings(&isper_llm::LlmSettings {
        provider,
        model,
        embeddings,
    })
    .map_err(|e| e.to_string())?;

    if crate::paths::manages_autostart() {
        let autolaunch = app.autolaunch();
        let _ = if patch.autostart {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
    }

    notify_status(&app);
    Ok(label)
}

#[tauri::command]
pub(crate) fn set_llm_key(app: AppHandle, provider: String, key: String) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("chave vazia".into());
    }
    isper_llm::set_api_key(&provider.trim().to_lowercase(), &key).map_err(|e| e.to_string())?;
    notify_status(&app);
    Ok(())
}

#[tauri::command]
pub(crate) async fn test_llm() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let settings = isper_llm::load_settings();
        let provider = isper_llm::provider_from_settings(&settings).map_err(|e| e.to_string())?;
        provider
            .complete(
                "Você é o teste de conexão do ISPer. Responda em português, em uma linha.",
                "Diga apenas: conexão ok!",
            )
            .map(|r| format!("{} ({}): {}", provider.name(), provider.model(), r.trim()))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Lista os modelos disponíveis para a chave guardada do provider indicado
/// (o que está selecionado na tela, mesmo antes de salvar).
#[tauri::command]
pub(crate) async fn list_llm_models(
    provider: String,
    model: Option<String>,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = isper_llm::LlmSettings {
            provider: provider.trim().to_lowercase(),
            model,
            embeddings: Default::default(),
        };
        let p = isper_llm::provider_from_settings(&settings).map_err(|e| e.to_string())?;
        p.list_models().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Microfones disponíveis (a tela de Configurações lista; vazio = só o padrão).
#[tauri::command]
pub(crate) fn list_input_devices() -> Vec<String> {
    isper_core::audio::list_input_devices()
}

/// Toggle do rodapé do Início: abrir (ou não) esta tela com o app.
#[tauri::command]
pub(crate) fn set_show_home(app: AppHandle, show: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let cfg = {
        let mut c = state.config.lock_or_recover();
        c.show_home_on_launch = show;
        c.clone()
    };
    config::save(&cfg).map_err(|e| e.to_string())
}

/// Abre a pasta de logs no Explorer (Configurações → Sistema).
#[tauri::command]
pub(crate) fn open_logs_folder() -> Result<(), String> {
    let dir = logs_dir().ok_or("pasta de logs indisponível")?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(serde::Serialize)]
pub(crate) struct Diagnostics {
    version: &'static str,
    gpu_build: bool,
    engine: EngineStatus,
    model_path: Option<String>,
    models_dir: String,
    diarize_installed: bool,
    db_path: String,
    config_path: String,
    logs_dir: String,
    meetings_dir: String,
    input_devices: Vec<String>,
    /// (nome da DLL, encontrada?) — o motivo clássico de "o exe não abre".
    cuda_dlls: Vec<(String, bool)>,
    exe_path: String,
    /// Roda da versão portátil (zip), não de uma instalação.
    portable: bool,
    /// Variáveis de ambiente ausentes no processo (pastas então vêm da API do Windows).
    missing_env: Vec<String>,
    /// Versão do schema do banco (`PRAGMA user_version`); `None` se ele não abriu.
    db_schema: Option<i64>,
    /// Tamanho do arquivo do banco, em bytes.
    db_bytes: u64,
    /// Retenção configurada, em dias (0 = para sempre).
    retention_days: u32,
    /// Métricas locais dos últimos 30 dias (ditado, blocos de reunião).
    metrics: Vec<isper_core::store::KindMetrics>,
}

/// Raio-X para suporte: caminhos, modelo, DLLs do CUDA, dispositivos.
#[tauri::command]
pub(crate) fn diagnostics(app: AppHandle) -> Diagnostics {
    let state = app.state::<AppState>();
    let engine = state.engine_status.lock_or_recover().clone();
    let (preferred, retention_days) = {
        let cfg = state.config.lock_or_recover();
        (cfg.model.clone(), cfg.retention_days)
    };
    let db_file = crate::paths::roaming_dir().map(|d| d.join("isper.db"));
    let db_bytes = db_file
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .unwrap_or(0);
    let (db_schema, metrics) = match open_store() {
        Ok(store) => (
            store.schema_version().ok(),
            store
                .metrics(local_now_ts() - METRICS_WINDOW_DAYS * 86_400)
                .unwrap_or_default(),
        ),
        Err(_) => (None, Vec::new()),
    };
    let model_path = isper_models::resolve_whisper_model(
        preferred.as_deref(),
        cfg!(feature = "cuda"),
        &dev_dirs(),
    )
    .map(|p| p.display().to_string());
    let exe = std::env::current_exe().ok();
    let exe_dir = exe.as_ref().and_then(|p| p.parent().map(Path::to_path_buf));
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let cuda_dlls = ["cudart64_13.dll", "cublas64_13.dll", "cublasLt64_13.dll"]
        .iter()
        .map(|dll| {
            let found = exe_dir
                .iter()
                .chain(path_dirs.iter())
                .any(|d| d.join(dll).exists());
            (dll.to_string(), found)
        })
        .collect();
    Diagnostics {
        version: env!("CARGO_PKG_VERSION"),
        gpu_build: cfg!(feature = "cuda"),
        engine,
        model_path,
        models_dir: isper_models::models_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        diarize_installed: isper_diarize::models_installed(),
        db_path: db_file.map(|p| p.display().to_string()).unwrap_or_default(),
        config_path: config::path()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        logs_dir: logs_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        meetings_dir: meetings_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        input_devices: isper_core::audio::list_input_devices(),
        cuda_dlls,
        exe_path: exe.map(|p| p.display().to_string()).unwrap_or_default(),
        portable: crate::paths::is_portable(),
        missing_env: crate::paths::missing_env_vars(),
        db_schema,
        db_bytes,
        retention_days,
        metrics,
    }
}

/// Dispara um toast de teste (botão nas Configurações) — prova que o Windows
/// aceita as notificações do ISPer nesta máquina.
#[tauri::command]
pub(crate) async fn notify_test(app: AppHandle) -> Result<(), String> {
    let app2 = app.clone();
    let line1 = crate::i18n::tr(&app, "notify.test-line1");
    let line2 = crate::i18n::tr(&app, "notify.test-line2");
    notify::show(
        notify::Toast {
            title: "ISPer",
            line1: &line1,
            line2: Some(&line2),
            silent: false,
        },
        move || open_home(&app2),
    )
    .map_err(|e| e.to_string())
}
