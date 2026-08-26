//! ISPer — app de ditado (Fase 2).
//!
//! Fluxo: segurar o atalho global → overlay aparece + gravação começa →
//! soltar → transcreve com Whisper → cola (Ctrl+V) no app que está focado.
//! O overlay é *não-focável*: o foco nunca sai do app do usuário.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::ShortcutState;

use isper_core::{recorder, WhisperEngine};

/// Candidatos a atalho push-to-talk, em ordem de preferência. Outro programa
/// pode já ter registrado o primeiro (neste PC, Ctrl+Alt+Espaço estava
/// ocupado!), então o ISPer registra o primeiro que estiver livre em vez de
/// quebrar no startup.
const SHORTCUT_CANDIDATES: [(&str, &str); 4] = [
    ("ctrl+alt+space", "Ctrl+Alt+Espaço"),
    ("ctrl+shift+space", "Ctrl+Shift+Espaço"),
    ("ctrl+alt+d", "Ctrl+Alt+D"),
    ("ctrl+alt+i", "Ctrl+Alt+I"),
];
const LANG: &str = "pt";

struct AppState {
    /// Carregado em background no startup; `None` enquanto carrega.
    engine: Mutex<Option<Arc<WhisperEngine>>>,
    audio: recorder::AudioHandle,
    recording: AtomicBool,
    busy: AtomicBool,
}

fn main() {
    tracing_subscriber::fmt().with_target(false).compact().init();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| match event.state {
                    ShortcutState::Pressed => on_pressed(app),
                    ShortcutState::Released => on_released(app),
                })
                .build(),
        )
        .setup(|app| {
            app.manage(AppState {
                engine: Mutex::new(None),
                audio: recorder::spawn(),
                recording: AtomicBool::new(false),
                busy: AtomicBool::new(false),
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

            // Registra o primeiro atalho livre da lista de candidatos.
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            let mut shortcut_label = None;
            for (combo, label) in SHORTCUT_CANDIDATES {
                match app.global_shortcut().register(combo) {
                    Ok(()) => {
                        tracing::info!("atalho registrado: {label}");
                        shortcut_label = Some(label);
                        break;
                    }
                    Err(e) => tracing::warn!("atalho {label} indisponível: {e}"),
                }
            }
            let shortcut_label = shortcut_label.unwrap_or("(nenhum atalho livre!)");

            // Ícone na bandeja com menu.
            let hint = MenuItem::with_id(
                app,
                "hint",
                format!("Segure {shortcut_label} para ditar"),
                false,
                None::<&str>,
            )?;
            let quit = MenuItem::with_id(app, "quit", "Sair do ISPer", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&hint, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("ícone do app").clone())
                .menu(&menu)
                .tooltip("ISPer — ditado 100% local")
                .on_menu_event(|app, event| {
                    if event.id() == "quit" {
                        app.exit(0);
                    }
                })
                .build(app)?;

            // O modelo (~0,5 GB) carrega em background p/ não travar o startup.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let result = find_model()
                    .ok_or_else(|| "models/ggml-small.bin não encontrado".to_string())
                    .and_then(|p| WhisperEngine::new(&p).map_err(|e| e.to_string()));
                match result {
                    Ok(engine) => {
                        *handle.state::<AppState>().engine.lock().unwrap() = Some(Arc::new(engine));
                        tracing::info!("modelo Whisper carregado");
                    }
                    Err(e) => {
                        tracing::error!("falha ao carregar modelo: {e}");
                        let _ = handle
                            .emit_to("overlay", "isper-state", json!({"state": "error", "message": e}));
                    }
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

fn on_pressed(app: &AppHandle) {
    let state = app.state::<AppState>();
    // `swap` garante que só UMA gravação começa mesmo com repeat de tecla.
    if state.busy.load(Ordering::SeqCst) || state.recording.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = app.emit_to("overlay", "isper-state", json!({"state": "recording"}));
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.show();
    }
    state.audio.start();
}

fn on_released(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.recording.swap(false, Ordering::SeqCst) {
        return;
    }
    state.busy.store(true, Ordering::SeqCst);

    // Transcrição é pesada — sai da thread do atalho global.
    let app = app.clone();
    std::thread::spawn(move || {
        match dictate(&app) {
            Ok(text) => {
                let _ = app.emit_to("overlay", "isper-state", json!({"state": "done", "text": text}));
            }
            Err(e) => {
                tracing::warn!("ditado falhou: {e}");
                let _ = app.emit_to(
                    "overlay",
                    "isper-state",
                    json!({"state": "error", "message": e.to_string()}),
                );
            }
        }
        // Deixa o resultado visível um instante antes de esconder o overlay.
        std::thread::sleep(Duration::from_millis(1600));
        let state = app.state::<AppState>();
        if !state.recording.load(Ordering::SeqCst) {
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.hide();
            }
        }
        state.busy.store(false, Ordering::SeqCst);
    });
}

fn dictate(app: &AppHandle) -> anyhow::Result<String> {
    let state = app.state::<AppState>();
    let raw = state.audio.stop()?;
    if raw.duration_secs() < 0.4 {
        anyhow::bail!("segure o atalho enquanto fala");
    }
    let _ = app.emit_to("overlay", "isper-state", json!({"state": "transcribing"}));

    let engine = {
        let guard = state.engine.lock().unwrap();
        guard
            .clone()
            .ok_or_else(|| anyhow::anyhow!("o modelo ainda está carregando — tente em instantes"))?
    };

    let audio_secs = raw.duration_secs();
    let samples = raw.into_whisper_input()?;
    let t = engine.transcribe(&samples, LANG)?;
    let text = t.text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("não entendi — tente de novo");
    }
    tracing::info!(audio_secs, infer_secs = t.infer_secs, "transcrito: {text}");
    paste_text(&text)?;
    Ok(text)
}

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

/// Procura `models/ggml-small.bin`: env ISPER_MODEL, depois cwd e os
/// ancestrais do executável (funciona em `cargo run` e no app instalado).
fn find_model() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ISPER_MODEL") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.extend(cwd.ancestors().take(5).map(|d| d.join("models/ggml-small.bin")));
    }
    if let Ok(exe) = std::env::current_exe() {
        candidates.extend(exe.ancestors().take(7).map(|d| d.join("models/ggml-small.bin")));
    }
    candidates.into_iter().find(|p| p.exists())
}
