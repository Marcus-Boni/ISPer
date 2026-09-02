//! ISPer — ditado por voz e notetaker de reuniões, 100% local.
//!
//! Ditado (Fase 2): segurar o atalho = push-to-talk; toque rápido =
//! mãos-livres (VAD encerra no silêncio). O texto é colado no app focado.
//! Reuniões (Fase 4): mic + loopback → transcript com "Eu"/"Participantes".
//! IA (Fase 5): resumo pós-reunião via provider de nuvem configurável.
//! Configurações (Fase 3): janela própria — atalho, idioma, dicionário,
//! IA e autostart — persistidas em %APPDATA%\ISPer\config.toml.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::ShortcutState;

use config::AppConfig;
use isper_core::loopback::LoopbackSource;
use isper_core::meeting::{self, MeetingHandle, MeetingOptions};
use isper_core::recorder::{self, RecorderEvent};
use isper_core::store::MeetingStore;
use isper_core::{RawAudio, WhisperEngine};

/// Candidatos a atalho, em ordem de preferência, usados quando o usuário não
/// fixou um nas configurações (ou quando o preferido está ocupado — neste PC,
/// Ctrl+Alt+Espaço vive ocupado por outro programa).
const SHORTCUT_CANDIDATES: [(&str, &str); 4] = [
    ("ctrl+alt+space", "Ctrl+Alt+Espaço"),
    ("ctrl+shift+space", "Ctrl+Shift+Espaço"),
    ("ctrl+alt+d", "Ctrl+Alt+D"),
    ("ctrl+alt+i", "Ctrl+Alt+I"),
];
/// Soltar antes disso = toque rápido → vira modo mãos-livres.
const TAP_THRESHOLD: Duration = Duration::from_millis(350);

/// Máquina de estados do ditado — um único lugar decide o que cada evento
/// de tecla significa (inclusive o auto-repeat, que dispara `Pressed`
/// repetido enquanto a tecla está segurada).
enum Phase {
    Idle,
    Recording { started: Instant, handsfree: bool },
    Processing,
}

struct AppState {
    /// Carregado em background no startup; `None` enquanto carrega.
    engine: Mutex<Option<Arc<WhisperEngine>>>,
    audio: recorder::AudioHandle,
    phase: Mutex<Phase>,
    config: Mutex<AppConfig>,
    /// Rótulo do atalho registrado no momento (p/ a bandeja e as configurações).
    active_shortcut: Mutex<String>,
    /// Gravação de reunião em andamento (Fase 4).
    meeting: Mutex<Option<MeetingHandle>>,
    /// Itens do menu da bandeja cujo texto muda em tempo de execução.
    meeting_item: Mutex<Option<MenuItem<tauri::Wry>>>,
    hint_item: Mutex<Option<MenuItem<tauri::Wry>>>,
}

fn main() {
    tracing_subscriber::fmt().with_target(false).compact().init();

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| match event.state {
                    ShortcutState::Pressed => on_pressed(app),
                    ShortcutState::Released => on_released(app),
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_settings,
            apply_settings,
            set_llm_key,
            test_llm,
            list_llm_models,
            models_status,
            download_model,
            delete_model,
            diarize_status,
            download_diarize_models
        ])
        .setup(|app| {
            let cfg = config::load();
            app.manage(AppState {
                engine: Mutex::new(None),
                audio: recorder::spawn(),
                phase: Mutex::new(Phase::Idle),
                config: Mutex::new(cfg.clone()),
                active_shortcut: Mutex::new(String::new()),
                meeting: Mutex::new(None),
                meeting_item: Mutex::new(None),
                hint_item: Mutex::new(None),
            });

            // Overlay: rodapé do monitor primário, nunca focável.
            let overlay = app.get_webview_window("overlay").expect("janela overlay");
            overlay.set_focusable(false)?;
            if let Some(monitor) = overlay.primary_monitor()? {
                let mon_size = monitor.size();
                let mon_pos = monitor.position();
                let win = overlay.outer_size()?;
                let x = mon_pos.x + (mon_size.width as i32 - win.width as i32) / 2;
                let y = mon_pos.y + mon_size.height as i32 - win.height as i32 - 96;
                overlay.set_position(tauri::PhysicalPosition::new(x, y))?;
            }

            // Atalho global: o preferido das configurações, senão o primeiro livre.
            let label = register_best_shortcut(app.handle(), cfg.shortcut.as_deref());
            *app.state::<AppState>().active_shortcut.lock().unwrap() = label.clone();

            // Ícone na bandeja com menu.
            let hint = MenuItem::with_id(app, "hint", hint_text(&label), false, None::<&str>)?;
            let settings_item =
                MenuItem::with_id(app, "settings", "Configurações…", true, None::<&str>)?;
            let meeting_item = MenuItem::with_id(
                app,
                "meeting",
                "Iniciar gravação de reunião",
                true,
                None::<&str>,
            )?;
            let quit = MenuItem::with_id(app, "quit", "Sair do ISPer", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&hint, &settings_item, &meeting_item, &quit])?;
            {
                let state = app.state::<AppState>();
                *state.meeting_item.lock().unwrap() = Some(meeting_item);
                *state.hint_item.lock().unwrap() = Some(hint);
            }
            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("ícone do app").clone())
                .menu(&menu)
                .tooltip("ISPer — ditado e reuniões, 100% local")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "quit" => app.exit(0),
                    "meeting" => toggle_meeting(app),
                    "settings" => open_settings(app),
                    _ => {}
                })
                .build(app)?;

            // O modelo (~0,5 GB) carrega em background p/ não travar o startup.
            load_engine_in_background(app.handle().clone());

            // Pipeline: toda gravação de ditado concluída chega aqui.
            let events = app.state::<AppState>().audio.events();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                for RecorderEvent::Finished(result) in events.iter() {
                    *handle.state::<AppState>().phase.lock().unwrap() = Phase::Processing;

                    let outcome = result
                        .map_err(anyhow::Error::from)
                        .and_then(|raw| dictate(&handle, raw));
                    match outcome {
                        Ok(text) => {
                            let _ = handle
                                .emit_to("overlay", "isper-state", json!({"state": "done", "text": text}));
                        }
                        Err(e) => {
                            tracing::warn!("ditado falhou: {e}");
                            let _ = handle.emit_to(
                                "overlay",
                                "isper-state",
                                json!({"state": "error", "message": e.to_string()}),
                            );
                        }
                    }

                    std::thread::sleep(Duration::from_millis(1200));
                    {
                        let state = handle.state::<AppState>();
                        let mut phase = state.phase.lock().unwrap();
                        if matches!(*phase, Phase::Processing) {
                            *phase = Phase::Idle;
                        }
                    }
                    maybe_restore_overlay(&handle);
                }
            });

            // Encaminha o nível do microfone para o waveform da UI.
            let levels = app.state::<AppState>().audio.levels();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                for level in levels.iter() {
                    let _ = handle.emit_to("overlay", "isper-level", level);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o ISPer");
}

// ------------------------------------------------------------- atalho

/// Registra o primeiro atalho livre: o preferido (das configurações) tem
/// prioridade; os candidatos padrão são o fallback. Devolve o rótulo ativo.
fn register_best_shortcut(app: &AppHandle, preferred: Option<&str>) -> String {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();

    let mut tried: Vec<(String, String)> = Vec::new();
    if let Some(p) = preferred {
        let p = p.trim().to_lowercase();
        if !p.is_empty() {
            let label = SHORTCUT_CANDIDATES
                .iter()
                .find(|(combo, _)| *combo == p)
                .map(|(_, l)| l.to_string())
                .unwrap_or_else(|| p.clone());
            tried.push((p, label));
        }
    }
    for (combo, label) in SHORTCUT_CANDIDATES {
        tried.push((combo.to_string(), label.to_string()));
    }

    for (combo, label) in tried {
        match shortcuts.register(combo.as_str()) {
            Ok(()) => {
                tracing::info!("atalho registrado: {label}");
                return label;
            }
            Err(e) => tracing::warn!("atalho {label} indisponível: {e}"),
        }
    }
    "(nenhum atalho livre!)".into()
}

fn hint_text(label: &str) -> String {
    format!("Segure {label} para ditar (toque rápido = mãos-livres)")
}

fn set_hint(app: &AppHandle, label: &str) {
    let state = app.state::<AppState>();
    let guard = state.hint_item.lock().unwrap();
    if let Some(item) = guard.as_ref() {
        let _ = item.set_text(hint_text(label));
    }
}

// ------------------------------------------------------------- ditado

fn on_pressed(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut phase = state.phase.lock().unwrap();
    match *phase {
        Phase::Idle => {
            *phase = Phase::Recording {
                started: Instant::now(),
                handsfree: false,
            };
            drop(phase);
            let _ = app.emit_to("overlay", "isper-state", json!({"state": "recording"}));
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.show();
            }
            state.audio.start();
        }
        Phase::Recording { started, handsfree } => {
            // Segundo toque encerra o mãos-livres na hora. Só conta DEPOIS de
            // virar mãos-livres: o auto-repeat dispara Pressed repetido
            // enquanto a tecla está segurada no push-to-talk.
            if handsfree && started.elapsed() > Duration::from_millis(500) {
                *phase = Phase::Processing;
                drop(phase);
                state.audio.stop();
            }
        }
        Phase::Processing => {}
    }
}

fn on_released(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut phase = state.phase.lock().unwrap();
    if let Phase::Recording {
        started,
        handsfree: false,
    } = *phase
    {
        if started.elapsed() < TAP_THRESHOLD {
            *phase = Phase::Recording {
                started,
                handsfree: true,
            };
            drop(phase);
            state.audio.set_vad(true);
            let _ = app.emit_to("overlay", "isper-state", json!({"state": "recording-handsfree"}));
        } else {
            *phase = Phase::Processing;
            drop(phase);
            state.audio.stop();
        }
    }
}

fn dictate(app: &AppHandle, raw: RawAudio) -> anyhow::Result<String> {
    if raw.duration_secs() < 0.4 {
        anyhow::bail!("segure o atalho enquanto fala");
    }
    let _ = app.emit_to("overlay", "isper-state", json!({"state": "transcribing"}));

    let state = app.state::<AppState>();
    let engine = {
        let guard = state.engine.lock().unwrap();
        guard
            .clone()
            .ok_or_else(|| anyhow::anyhow!("o modelo ainda está carregando — tente em instantes"))?
    };
    let (lang, prompt) = {
        let cfg = state.config.lock().unwrap();
        (cfg.lang.clone(), cfg.initial_prompt())
    };

    let audio_secs = raw.duration_secs();
    let samples = raw.into_whisper_input()?;
    let t = engine.transcribe(&samples, &lang, prompt.as_deref())?;
    let text = t.text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("não entendi — tente de novo");
    }
    tracing::info!(audio_secs, infer_secs = t.infer_secs, "transcrito: {text}");
    paste_text(&text)?;

    // Histórico de ditados (Fase 3) — falha aqui não pode travar o fluxo.
    if let Ok(store) = open_store() {
        let at = chrono::Local::now().format("%d/%m/%Y %H:%M:%S").to_string();
        let _ = store.save_dictation(&at, &text, audio_secs, t.infer_secs);
    }
    Ok(text)
}

// ------------------------------------------------------------ reuniões

/// Depois de um ditado ou reunião: se houver reunião ativa, o overlay volta
/// a mostrar o estado dela; senão, esconde.
fn maybe_restore_overlay(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !matches!(*state.phase.lock().unwrap(), Phase::Idle) {
        return;
    }
    if state.meeting.lock().unwrap().is_some() {
        let _ = app.emit_to("overlay", "isper-state", json!({"state": "meeting"}));
    } else if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }
}

fn set_meeting_text(app: &AppHandle, text: &str) {
    let state = app.state::<AppState>();
    let guard = state.meeting_item.lock().unwrap();
    if let Some(item) = guard.as_ref() {
        let _ = item.set_text(text);
    }
}

/// Alterna a gravação de reunião pelo menu da bandeja.
fn toggle_meeting(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut slot = state.meeting.lock().unwrap();

    if let Some(handle) = slot.take() {
        drop(slot);
        set_meeting_text(app, "Iniciar gravação de reunião");
        let _ = app.emit_to("overlay", "isper-state", json!({"state": "meeting-processing"}));
        if let Some(overlay) = app.get_webview_window("overlay") {
            let _ = overlay.show();
        }
        let app = app.clone();
        std::thread::spawn(move || {
            match finish_meeting(&app, handle) {
                Ok(path) => {
                    tracing::info!("reunião salva em {path}");
                    let _ = app.emit_to("overlay", "isper-state", json!({"state": "meeting-done"}));
                }
                Err(e) => {
                    tracing::warn!("reunião falhou: {e}");
                    let _ = app.emit_to(
                        "overlay",
                        "isper-state",
                        json!({"state": "error", "message": e.to_string()}),
                    );
                }
            }
            std::thread::sleep(Duration::from_millis(2500));
            maybe_restore_overlay(&app);
        });
    } else {
        drop(slot);
        let engine = { state.engine.lock().unwrap().clone() };
        let opts = {
            let cfg = state.config.lock().unwrap();
            MeetingOptions {
                lang: cfg.lang.clone(),
                initial_prompt: cfg.initial_prompt(),
                source: LoopbackSource::parse(&cfg.meeting_source),
            }
        };
        let started = engine
            .ok_or_else(|| anyhow::anyhow!("o modelo ainda está carregando — tente em instantes"))
            .and_then(|engine| meeting::start(engine, opts).map_err(anyhow::Error::from));
        match started {
            Ok(handle) => {
                let mut payload = json!({"state": "meeting"});
                if !handle.warnings.is_empty() {
                    payload["message"] = json!(handle.warnings.join(" · "));
                }
                *state.meeting.lock().unwrap() = Some(handle);
                set_meeting_text(app, "Encerrar e transcrever a reunião");
                let _ = app.emit_to("overlay", "isper-state", payload);
                if let Some(overlay) = app.get_webview_window("overlay") {
                    let _ = overlay.show();
                }
            }
            Err(e) => {
                tracing::error!("não consegui iniciar a reunião: {e}");
                let _ = app.emit_to(
                    "overlay",
                    "isper-state",
                    json!({"state": "error", "message": e.to_string()}),
                );
                if let Some(overlay) = app.get_webview_window("overlay") {
                    let _ = overlay.show();
                }
                let app = app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(2500));
                    maybe_restore_overlay(&app);
                });
            }
        }
    }
}

fn open_store() -> anyhow::Result<MeetingStore> {
    let dir = PathBuf::from(std::env::var("APPDATA")?).join("ISPer");
    std::fs::create_dir_all(&dir)?;
    Ok(MeetingStore::open(&dir.join("isper.db"))?)
}

/// Encerra a gravação, salva o Markdown em Documentos\ISPer\Reunioes e no
/// banco SQLite, gera o resumo por IA (Fase 5, se configurado) e abre o
/// arquivo no app padrão.
fn finish_meeting(app: &AppHandle, handle: MeetingHandle) -> anyhow::Result<String> {
    let mut result = handle.stop()?;
    if result.segments.is_empty() {
        anyhow::bail!("nenhuma fala detectada na reunião");
    }

    // Fase 4: quem falou o quê — só se os modelos de diarização existirem.
    if isper_diarize::models_installed() && !result.others_audio_16k.is_empty() {
        let _ = app.emit_to("overlay", "isper-state", json!({"state": "meeting-diarize"}));
        match isper_diarize::diarize(&result.others_audio_16k) {
            Ok(turns) => {
                let t: Vec<(f32, f32, usize)> =
                    turns.iter().map(|t| (t.start, t.end, t.speaker)).collect();
                result.apply_speaker_turns(&t);
                tracing::info!("{} participante(s) identificado(s)", result.distinct_participants());
            }
            Err(e) => tracing::warn!("diarização falhou (rótulos genéricos mantidos): {e}"),
        }
    }
    let now = chrono::Local::now();
    let started_at = now.format("%d/%m/%Y %H:%M").to_string();
    let title = format!("Reunião — {started_at}");
    let md = meeting::to_markdown(&title, &started_at, &result);

    // O transcript é salvo ANTES do resumo: se a API falhar, nada se perde.
    let docs = PathBuf::from(std::env::var("USERPROFILE")?)
        .join("Documents")
        .join("ISPer")
        .join("Reunioes");
    std::fs::create_dir_all(&docs)?;
    let md_path = docs.join(format!("reuniao-{}.md", now.format("%Y%m%d-%H%M%S")));
    std::fs::write(&md_path, &md)?;

    let store = open_store()?;
    let meeting_id = store.save(&title, &started_at, &result)?;

    // Fase 5: resumo por IA de nuvem — só o TEXTO do transcript sai da máquina.
    let settings = isper_llm::load_settings();
    match isper_llm::provider_from_settings(&settings) {
        Ok(provider) => {
            let _ = app.emit_to("overlay", "isper-state", json!({"state": "meeting-summary"}));
            match isper_llm::summarize_meeting(provider.as_ref(), &md) {
                Ok(summary) => {
                    let block = format!(
                        "\n\n---\n\n{}\n\n> Resumo gerado via {} ({}) — revise antes de usar.\n",
                        summary.trim(),
                        provider.name(),
                        provider.model()
                    );
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&md_path) {
                        let _ = f.write_all(block.as_bytes());
                    }
                    let _ = store.set_summary(meeting_id, summary.trim());
                    tracing::info!("resumo gerado via {}", provider.name());
                }
                Err(e) => tracing::warn!("resumo falhou (transcript preservado): {e}"),
            }
        }
        Err(isper_llm::LlmError::NotConfigured) => {
            tracing::info!("sem provider de IA configurado — reunião salva sem resumo");
        }
        Err(e) => tracing::warn!("resumo indisponível: {e}"),
    }

    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", &md_path.to_string_lossy()])
        .spawn();

    Ok(md_path.display().to_string())
}

// ------------------------------------------------------- configurações

fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let result = tauri::WebviewWindowBuilder::new(
        app,
        "settings",
        tauri::WebviewUrl::App("settings.html".into()),
    )
    .title("ISPer — Configurações")
    .inner_size(560.0, 700.0)
    .resizable(false)
    .build();
    if let Err(e) = result {
        tracing::error!("não consegui abrir as configurações: {e}");
    }
}

#[derive(serde::Serialize)]
struct SettingsDto {
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
}

#[derive(serde::Deserialize)]
struct SettingsPatch {
    shortcut: Option<String>,
    lang: String,
    dictionary: String,
    model: Option<String>,
    meeting_source: String,
    llm_provider: String,
    llm_model: Option<String>,
    autostart: bool,
}

#[derive(serde::Serialize)]
struct ModelDto {
    file: String,
    label: String,
    approx_mb: u32,
    note: String,
    needs_gpu: bool,
    installed: bool,
    active: bool,
}

/// Em desenvolvimento, o `models/` do repositório também vale como fonte.
fn dev_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        dirs.extend(cwd.ancestors().take(5).map(|d| d.to_path_buf()));
    }
    if let Ok(exe) = std::env::current_exe() {
        dirs.extend(exe.ancestors().take(7).map(|d| d.to_path_buf()));
    }
    dirs
}

/// Carrega (ou recarrega) o modelo Whisper em background. Sem nenhum modelo
/// instalado, abre as Configurações para o usuário baixar um.
fn load_engine_in_background(app: AppHandle) {
    std::thread::spawn(move || {
        let preferred = app.state::<AppState>().config.lock().unwrap().model.clone();
        let Some(path) =
            isper_models::resolve_whisper_model(preferred.as_deref(), cfg!(feature = "cuda"), &dev_dirs())
        else {
            tracing::warn!("nenhum modelo instalado — abrindo Configurações");
            let app2 = app.clone();
            let _ = app.run_on_main_thread(move || open_settings(&app2));
            return;
        };
        tracing::info!("carregando modelo {}", path.display());
        match WhisperEngine::new(&path) {
            Ok(engine) => {
                *app.state::<AppState>().engine.lock().unwrap() = Some(Arc::new(engine));
                tracing::info!("modelo Whisper carregado");
            }
            Err(e) => {
                tracing::error!("falha ao carregar modelo: {e}");
                let _ = app.emit_to(
                    "overlay",
                    "isper-state",
                    json!({"state": "error", "message": e.to_string()}),
                );
            }
        }
    });
}

#[tauri::command]
fn models_status(app: AppHandle) -> Vec<ModelDto> {
    let preferred = app.state::<AppState>().config.lock().unwrap().model.clone();
    let dirs = dev_dirs();
    let active_file = isper_models::resolve_whisper_model(preferred.as_deref(), cfg!(feature = "cuda"), &dirs)
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()));
    isper_models::WHISPER_CATALOG
        .iter()
        .map(|m| ModelDto {
            file: m.file.to_string(),
            label: m.label.to_string(),
            approx_mb: m.approx_mb,
            note: m.note.to_string(),
            needs_gpu: m.needs_gpu,
            installed: isper_models::installed_path(m.file).is_some()
                || dirs.iter().any(|d| d.join("models").join(m.file).exists()),
            active: active_file.as_deref() == Some(m.file),
        })
        .collect()
}

/// Baixa um modelo do catálogo emitindo `isper-model-progress` para a
/// janela de Configurações. Se ainda não havia modelo carregado, carrega.
#[tauri::command]
async fn download_model(app: AppHandle, file: String) -> Result<(), String> {
    let app2 = app.clone();
    let file2 = file.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut last = 0u64;
        isper_models::download_whisper(&file2, &mut |done, total| {
            // No máximo ~1 evento por MB — a UI não precisa de mais.
            if done - last >= 1_000_000 || done == total {
                last = done;
                let _ = app2.emit_to(
                    "settings",
                    "isper-model-progress",
                    json!({"file": file2, "done": done, "total": total}),
                );
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    if app.state::<AppState>().engine.lock().unwrap().is_none() {
        load_engine_in_background(app.clone());
    }
    Ok(())
}

#[tauri::command]
fn delete_model(file: String) -> Result<(), String> {
    isper_models::remove(&file).map_err(|e| e.to_string())
}

#[tauri::command]
fn diarize_status() -> bool {
    isper_diarize::models_installed()
}

/// Baixa os modelos de diarização, com progresso em `isper-model-progress`
/// (file = "diarize").
#[tauri::command]
async fn download_diarize_models(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut last = 0u64;
        isper_diarize::download_models(&mut |name, done, total| {
            if done - last >= 500_000 || done == total {
                last = done;
                let _ = app.emit_to(
                    "settings",
                    "isper-model-progress",
                    json!({"file": "diarize", "name": name, "done": done, "total": total}),
                );
            }
        })
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Result<SettingsDto, String> {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().unwrap().clone();
    let active_shortcut = state.active_shortcut.lock().unwrap().clone();
    let llm = isper_llm::load_settings();
    let llm_key_present = if llm.provider.is_empty() {
        false
    } else {
        isper_llm::get_api_key(&llm.provider)
            .map(|k| k.is_some())
            .unwrap_or(false)
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
    })
}

#[tauri::command]
fn apply_settings(app: AppHandle, patch: SettingsPatch) -> Result<String, String> {
    let state = app.state::<AppState>();

    let dictionary: Vec<String> = patch
        .dictionary
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let previous_model = state.config.lock().unwrap().model.clone();
    let cfg = AppConfig {
        shortcut: patch.shortcut.filter(|s| !s.trim().is_empty()),
        lang: {
            let l = patch.lang.trim().to_lowercase();
            if l.is_empty() { "pt".into() } else { l }
        },
        dictionary,
        model: patch.model.filter(|m| !m.trim().is_empty()),
        meeting_source: {
            let s = patch.meeting_source.trim().to_lowercase();
            if s.is_empty() { "system".into() } else { s }
        },
    };
    config::save(&cfg).map_err(|e| e.to_string())?;
    *state.config.lock().unwrap() = cfg.clone();

    // Troca de modelo a quente: o antigo continua servindo até o novo carregar.
    if cfg.model != previous_model {
        load_engine_in_background(app.clone());
    }

    // Reaplica o atalho na hora — sem reiniciar o app.
    let label = register_best_shortcut(&app, cfg.shortcut.as_deref());
    *state.active_shortcut.lock().unwrap() = label.clone();
    set_hint(&app, &label);

    // Provider de IA (a chave é gravada separadamente, via set_llm_key).
    let provider = if patch.llm_provider == "none" {
        String::new()
    } else {
        patch.llm_provider.trim().to_lowercase()
    };
    let model = patch.llm_model.filter(|m| !m.trim().is_empty());
    isper_llm::save_settings(&isper_llm::LlmSettings { provider, model })
        .map_err(|e| e.to_string())?;

    let autolaunch = app.autolaunch();
    let _ = if patch.autostart {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };

    Ok(label)
}

#[tauri::command]
fn set_llm_key(provider: String, key: String) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("chave vazia".into());
    }
    isper_llm::set_api_key(&provider.trim().to_lowercase(), &key).map_err(|e| e.to_string())
}

#[tauri::command]
async fn test_llm() -> Result<String, String> {
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
async fn list_llm_models(provider: String, model: Option<String>) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = isper_llm::LlmSettings {
            provider: provider.trim().to_lowercase(),
            model,
        };
        let p = isper_llm::provider_from_settings(&settings).map_err(|e| e.to_string())?;
        p.list_models().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// -------------------------------------------------------------- comuns

/// Cola `text` no app focado: salva o clipboard, injeta o texto, simula
/// Ctrl+V e restaura o clipboard anterior. Como o overlay não é focável,
/// o foco continua no app do usuário e o paste cai no lugar certo.
fn paste_text(text: &str) -> anyhow::Result<()> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    let mut clipboard = arboard::Clipboard::new()?;
    let previous = clipboard.get_text().ok();
    clipboard.set_text(text.to_string())?;
    std::thread::sleep(Duration::from_millis(60));

    let mut enigo = Enigo::new(&Settings::default())?;
    enigo.key(Key::Control, Direction::Press)?;
    enigo.key(Key::Unicode('v'), Direction::Click)?;
    enigo.key(Key::Control, Direction::Release)?;

    // Dá tempo do app alvo ler o clipboard antes de restaurá-lo.
    std::thread::sleep(Duration::from_millis(300));
    if let Some(old) = previous {
        let _ = clipboard.set_text(old);
    }
    Ok(())
}

