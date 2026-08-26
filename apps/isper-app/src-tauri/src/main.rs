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

use isper_core::meeting::{self, MeetingHandle};
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
    /// Gravação de reunião em andamento (Fase 4).
    meeting: Mutex<Option<MeetingHandle>>,
    /// Item do menu da bandeja que alterna a gravação (p/ trocar o texto).
    meeting_item: Mutex<Option<MenuItem<tauri::Wry>>>,
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
                meeting: Mutex::new(None),
                meeting_item: Mutex::new(None),
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
            let meeting_item = MenuItem::with_id(
                app,
                "meeting",
                "Iniciar gravação de reunião",
                true,
                None::<&str>,
            )?;
            let quit = MenuItem::with_id(app, "quit", "Sair do ISPer", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&hint, &meeting_item, &quit])?;
            *app.state::<AppState>().meeting_item.lock().unwrap() = Some(meeting_item);
            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("ícone do app").clone())
                .menu(&menu)
                .tooltip("ISPer — ditado e reuniões, 100% local")
                .on_menu_event(|app, event| {
                    if event.id() == "quit" {
                        app.exit(0);
                    } else if event.id() == "meeting" {
                        toggle_meeting(app);
                    }
                })
                .build(app)?;

            // O modelo (~0,5 GB) carrega em background p/ não travar o startup.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let result = find_model()
                    .ok_or_else(|| "nenhum modelo encontrado na pasta models/".to_string())
                    .and_then(|p| {
                        tracing::info!("carregando modelo {}", p.display());
                        WhisperEngine::new(&p).map_err(|e| e.to_string())
                    });
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

/// Depois de um ditado ou reunião: se houver reunião ativa, o overlay volta
/// a mostrar o estado dela; senão, esconde.
fn maybe_restore_overlay(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !matches!(*state.phase.lock().unwrap(), Phase::Idle) {
        return; // um novo ditado já assumiu o overlay
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
        // Encerrar: transcreve o resto, salva e abre o Markdown.
        drop(slot);
        set_meeting_text(app, "Iniciar gravação de reunião");
        let _ = app.emit_to("overlay", "isper-state", json!({"state": "meeting-processing"}));
        if let Some(overlay) = app.get_webview_window("overlay") {
            let _ = overlay.show();
        }
        let app = app.clone();
        std::thread::spawn(move || {
            match finish_meeting(handle) {
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
        // Iniciar.
        drop(slot);
        let engine = { state.engine.lock().unwrap().clone() };
        let started = engine
            .ok_or_else(|| anyhow::anyhow!("o modelo ainda está carregando — tente em instantes"))
            .and_then(|engine| meeting::start(engine).map_err(anyhow::Error::from));
        match started {
            Ok(handle) => {
                *state.meeting.lock().unwrap() = Some(handle);
                set_meeting_text(app, "Encerrar e transcrever a reunião");
                let _ = app.emit_to("overlay", "isper-state", json!({"state": "meeting"}));
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

/// Encerra a gravação, salva o Markdown em Documentos\ISPer\Reunioes e no
/// banco SQLite em %APPDATA%\ISPer, e abre o arquivo no app padrão.
fn finish_meeting(handle: MeetingHandle) -> anyhow::Result<String> {
    let result = handle.stop()?;
    if result.segments.is_empty() {
        anyhow::bail!("nenhuma fala detectada na reunião");
    }
    let now = chrono::Local::now();
    let started_at = now.format("%d/%m/%Y %H:%M").to_string();
    let title = format!("Reunião — {started_at}");
    let md = meeting::to_markdown(&title, &started_at, &result);

    let docs = PathBuf::from(std::env::var("USERPROFILE")?)
        .join("Documents")
        .join("ISPer")
        .join("Reunioes");
    std::fs::create_dir_all(&docs)?;
    let md_path = docs.join(format!("reuniao-{}.md", now.format("%Y%m%d-%H%M%S")));
    std::fs::write(&md_path, md)?;

    let db_dir = PathBuf::from(std::env::var("APPDATA")?).join("ISPer");
    std::fs::create_dir_all(&db_dir)?;
    let store = isper_core::store::MeetingStore::open(&db_dir.join("isper.db"))?;
    store.save(&title, &started_at, &result)?;

    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", &md_path.to_string_lossy()])
        .spawn();

    Ok(md_path.display().to_string())
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

/// Modelos aceitos, em ordem de preferência. Com CUDA, o `large-v3-turbo`
/// quantizado dá qualidade de large em tempo real na GPU; sem GPU, o `small`
/// é o equilíbrio certo em CPU.
const MODEL_CANDIDATES: &[&str] = if cfg!(feature = "cuda") {
    &[
        "models/ggml-large-v3-turbo-q5_0.bin",
        "models/ggml-small.bin",
    ]
} else {
    &[
        "models/ggml-small.bin",
        "models/ggml-large-v3-turbo-q5_0.bin",
    ]
};

/// Procura um modelo: env ISPER_MODEL, depois cwd e os ancestrais do
/// executável (funciona em `cargo run` e no app instalado).
fn find_model() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ISPER_MODEL") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        dirs.extend(cwd.ancestors().take(5).map(|d| d.to_path_buf()));
    }
    if let Ok(exe) = std::env::current_exe() {
        dirs.extend(exe.ancestors().take(7).map(|d| d.to_path_buf()));
    }
    for name in MODEL_CANDIDATES {
        if let Some(p) = dirs.iter().map(|d| d.join(name)).find(|p| p.exists()) {
            return Some(p);
        }
    }
    None
}
