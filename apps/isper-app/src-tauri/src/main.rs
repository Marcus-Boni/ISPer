//! ISPer — app de ditado (Fase 2).
//!
//! Dois modos, no mesmo atalho:
//! - **Push-to-talk**: segure, fale, solte → transcreve e cola.
//! - **Mãos-livres**: toque rápido (<350 ms), fale à vontade → ~1,2 s de
//!   silêncio (ou um segundo toque) encerra, transcreve e cola.
//!
//! O overlay é *não-focável*: o foco nunca sai do app do usuário, então o
//! Ctrl+V cai exatamente onde o cursor está.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::ShortcutState;

use isper_core::recorder::{self, RecorderEvent};
use isper_core::{RawAudio, WhisperEngine};

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
/// Soltar antes disso = toque rápido → vira modo mãos-livres.
const TAP_THRESHOLD: Duration = Duration::from_millis(350);

/// Máquina de estados do ditado — um único lugar decide o que cada evento
/// de tecla significa (inclusive o auto-repeat do teclado, que dispara
/// `Pressed` repetido enquanto a tecla está segurada).
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
                phase: Mutex::new(Phase::Idle),
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
                format!("Segure {shortcut_label} para ditar (toque rápido = mãos-livres)"),
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

            // Pipeline: toda gravação concluída (manual ou por VAD) chega aqui.
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

                    // Deixa o resultado visível um instante antes de esconder.
                    std::thread::sleep(Duration::from_millis(1200));
                    let state = handle.state::<AppState>();
                    let mut phase = state.phase.lock().unwrap();
                    if matches!(*phase, Phase::Processing) {
                        *phase = Phase::Idle;
                        if let Some(overlay) = handle.get_webview_window("overlay") {
                            let _ = overlay.hide();
                        }
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
            // virar mãos-livres: o auto-repeat do teclado dispara `Pressed`
            // repetido enquanto a tecla está segurada no push-to-talk.
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
            // Toque rápido → mãos-livres: o VAD encerra quando você parar
            // de falar (ou um segundo toque encerra na hora).
            *phase = Phase::Recording {
                started,
                handsfree: true,
            };
            drop(phase);
            state.audio.set_vad(true);
            let _ = app.emit_to("overlay", "isper-state", json!({"state": "recording-handsfree"}));
        } else {
            // Push-to-talk: soltou = terminou.
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
