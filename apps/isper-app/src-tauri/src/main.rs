//! ISPer — ditado por voz e notetaker de reuniões, 100% local.
//!
//! Ditado (Fase 2): segurar o atalho = push-to-talk; toque rápido =
//! mãos-livres (VAD encerra no silêncio). O texto é colado no app focado.
//! Reuniões (Fase 4): mic + loopback → transcript com "Eu"/"Participantes".
//! IA (Fase 5): resumo pós-reunião via provider de nuvem configurável.
//! Configurações (Fase 3): janela própria — atalho, idioma, dicionário,
//! IA e autostart — persistidas em %APPDATA%\ISPer\config.toml.
//! Início: janela central com o estado do sistema, atalhos para tudo e as
//! reuniões recentes — abre com o app (nunca no autostart), no clique
//! esquerdo do ícone da bandeja e no segundo clique do atalho.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{Shortcut, ShortcutState};

use config::AppConfig;
use isper_core::loopback::LoopbackSource;
use isper_core::meeting::{self, MeetingHandle, MeetingOptions, MeetingSegment, SegmentRef};
use isper_core::recorder::{self, RecorderEvent};
use isper_core::store::{MeetingDetail, MeetingStore};
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
/// Candidatos ao atalho de reunião (iniciar/encerrar a gravação).
const MEETING_SHORTCUT_CANDIDATES: [&str; 3] = ["ctrl+alt+m", "ctrl+shift+m", "ctrl+alt+r"];
/// Toques do atalho de reunião mais próximos que isso são ignorados (auto-repeat
/// da tecla e duplo aperto nervoso não podem iniciar e encerrar em sequência).
const MEETING_HOTKEY_DEBOUNCE: Duration = Duration::from_millis(1200);
/// Últimas falas guardadas para a transcrição ao vivo (o Início pede ao abrir).
const LIVE_KEEP: usize = 400;
/// Soltar antes disso = toque rápido → vira modo mãos-livres.
const TAP_THRESHOLD: Duration = Duration::from_millis(350);
/// Tamanhos lógicos do indicador flutuante: normal e mini (ponto + cronômetro).
const OVERLAY_FULL: (f64, f64) = (460.0, 104.0);
const OVERLAY_MINI: (f64, f64) = (150.0, 56.0);
/// Argumento que o autostart passa ao ISPer: nesse caso ele nasce quieto na
/// bandeja, sem abrir a tela Início.
const AUTOSTART_FLAG: &str = "--autostart";
/// Reuniões listadas na tela Início.
const HOME_RECENT: i64 = 5;

/// Máquina de estados do ditado — um único lugar decide o que cada evento
/// de tecla significa (inclusive o auto-repeat, que dispara `Pressed`
/// repetido enquanto a tecla está segurada).
enum Phase {
    Idle,
    Recording { started: Instant, handsfree: bool },
    Processing,
}

/// Estado do motor Whisper — fonte única para a tela Início responder
/// "por que não transcreve?" sem adivinhar.
#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum EngineStatus {
    Loading,
    Ready { file: String, label: String },
    Missing,
    Failed { message: String },
}

struct AppState {
    /// Carregado em background no startup; `None` enquanto carrega.
    engine: Mutex<Option<Arc<WhisperEngine>>>,
    engine_status: Mutex<EngineStatus>,
    audio: recorder::AudioHandle,
    phase: Mutex<Phase>,
    config: Mutex<AppConfig>,
    /// Rótulo do atalho registrado no momento (p/ a bandeja e as configurações).
    active_shortcut: Mutex<String>,
    /// Gravação de reunião em andamento (Fase 4).
    meeting: Mutex<Option<MeetingHandle>>,
    /// Quando a reunião atual começou (cronômetro da tela Início).
    meeting_started: Mutex<Option<Instant>>,
    /// Reunião que a Biblioteca deve abrir já selecionada.
    pending_meeting: Mutex<Option<i64>>,
    /// Itens do menu da bandeja cujo texto muda em tempo de execução.
    meeting_item: Mutex<Option<MenuItem<tauri::Wry>>>,
    hint_item: Mutex<Option<MenuItem<tauri::Wry>>>,
    /// HWND do indicador (0 fora do Windows) — p/ reafirmar o topo sem passar pelo tao.
    overlay_hwnd: isize,
    /// Atalhos registrados no momento — o handler precisa saber qual disparou.
    dictation_shortcut: Mutex<Option<Shortcut>>,
    meeting_shortcut: Mutex<Option<Shortcut>>,
    active_meeting_shortcut: Mutex<String>,
    last_meeting_toggle: Mutex<Option<Instant>>,
    /// Falas da reunião em andamento, na ordem em que foram transcritas.
    live: Mutex<Vec<LiveSegment>>,
    /// Ícone da bandeja e suas duas versões (normal / gravando).
    tray: Mutex<Option<TrayIcon>>,
    tray_icons: Mutex<Option<(Image<'static>, Image<'static>)>>,
}

/// Uma fala transcrita durante a reunião (evento `isper-live`).
#[derive(Clone, serde::Serialize)]
struct LiveSegment {
    speaker: String,
    start_secs: f32,
    end_secs: f32,
    text: String,
}

fn main() {
    tracing_subscriber::fmt().with_target(false).compact().init();

    tauri::Builder::default()
        // Instância única: um segundo clique no atalho não abre outro ISPer —
        // o pedido é encaminhado ao já aberto, que responde com a tela Início.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|a| a == AUTOSTART_FLAG) {
                open_home(app);
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_FLAG]),
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    let is_meeting = app
                        .state::<AppState>()
                        .meeting_shortcut
                        .lock()
                        .unwrap()
                        .as_ref()
                        == Some(shortcut);
                    if is_meeting {
                        if event.state == ShortcutState::Pressed {
                            on_meeting_hotkey(app);
                        }
                        return;
                    }
                    match event.state {
                        ShortcutState::Pressed => on_pressed(app),
                        ShortcutState::Released => on_released(app),
                    }
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
            download_diarize_models,
            list_meetings,
            get_meeting,
            rename_meeting,
            delete_meeting,
            open_meeting_file,
            open_meetings_folder,
            list_dictations,
            delete_dictation,
            overlay_prefs,
            overlay_set_mini,
            overlay_hide,
            overlay_moved,
            list_input_devices,
            live_transcript,
            rename_speaker,
            export_meeting,
            home_status,
            toggle_meeting_cmd,
            open_library_window,
            take_pending_meeting,
            open_settings_window,
            show_indicator_cmd,
            set_show_home
        ])
        .setup(|app| {
            let cfg = config::load();
            let overlay = app.get_webview_window("overlay").expect("janela overlay");
            app.manage(AppState {
                engine: Mutex::new(None),
                engine_status: Mutex::new(EngineStatus::Loading),
                audio: recorder::spawn(),
                phase: Mutex::new(Phase::Idle),
                config: Mutex::new(cfg.clone()),
                active_shortcut: Mutex::new(String::new()),
                meeting: Mutex::new(None),
                meeting_started: Mutex::new(None),
                pending_meeting: Mutex::new(None),
                meeting_item: Mutex::new(None),
                hint_item: Mutex::new(None),
                overlay_hwnd: overlay_hwnd(&overlay),
                dictation_shortcut: Mutex::new(None),
                meeting_shortcut: Mutex::new(None),
                active_meeting_shortcut: Mutex::new(String::new()),
                last_meeting_toggle: Mutex::new(None),
                live: Mutex::new(Vec::new()),
                tray: Mutex::new(None),
                tray_icons: Mutex::new(None),
            });
            app.state::<AppState>().audio.set_device(cfg.input_device.clone());

            // Overlay: nunca focável; tamanho (mini/normal) e posição lembrados.
            overlay.set_focusable(false)?;
            if cfg.overlay_mini {
                let _ = overlay.set_size(tauri::LogicalSize::new(OVERLAY_MINI.0, OVERLAY_MINI.1));
            }
            match cfg.overlay_pos {
                Some((x, y)) => overlay.set_position(tauri::PhysicalPosition::new(x, y))?,
                None => {
                    if let Some(monitor) = overlay.primary_monitor()? {
                        let mon_size = monitor.size();
                        let mon_pos = monitor.position();
                        let win = overlay.outer_size()?;
                        let x = mon_pos.x + (mon_size.width as i32 - win.width as i32) / 2;
                        let y = mon_pos.y + mon_size.height as i32 - win.height as i32 - 96;
                        overlay.set_position(tauri::PhysicalPosition::new(x, y))?;
                    }
                }
            }

            // Atalhos globais: os preferidos das configurações, senão os primeiros livres.
            let (label, _meeting_label) = register_shortcuts(app.handle(), &cfg);

            // Ícone na bandeja: clique esquerdo abre o Início; direito, o menu.
            let hint = MenuItem::with_id(app, "hint", hint_text(&label), false, None::<&str>)?;
            let home_item =
                MenuItem::with_id(app, "home", "Abrir o ISPer (Início)", true, None::<&str>)?;
            let library_item =
                MenuItem::with_id(app, "library", "Biblioteca de reuniões…", true, None::<&str>)?;
            let settings_item =
                MenuItem::with_id(app, "settings", "Configurações…", true, None::<&str>)?;
            let meeting_item = MenuItem::with_id(
                app,
                "meeting",
                meeting_item_text(app.handle(), false),
                true,
                None::<&str>,
            )?;
            let indicator_item = MenuItem::with_id(
                app,
                "indicator",
                "Mostrar indicador flutuante",
                true,
                None::<&str>,
            )?;
            let quit = MenuItem::with_id(app, "quit", "Sair do ISPer", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &hint,
                    &home_item,
                    &library_item,
                    &meeting_item,
                    &indicator_item,
                    &settings_item,
                    &quit,
                ],
            )?;
            {
                let state = app.state::<AppState>();
                *state.meeting_item.lock().unwrap() = Some(meeting_item);
                *state.hint_item.lock().unwrap() = Some(hint);
            }
            // Duas versões do ícone: a normal e a com o ponto vermelho de gravação.
            let base_icon = app.default_window_icon().expect("ícone do app").clone().to_owned();
            let rec_icon = recording_icon(&base_icon);
            let tray = TrayIconBuilder::new()
                .icon(base_icon.clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("ISPer — ditado e reuniões, 100% local")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "quit" => app.exit(0),
                    "home" => open_home(app),
                    "meeting" => {
                        let _ = toggle_meeting(app);
                    }
                    "indicator" => show_indicator(app),
                    "library" => open_library(app),
                    "settings" => open_settings(app),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        open_home(tray.app_handle());
                    }
                })
                .build(app)?;
            {
                let state = app.state::<AppState>();
                *state.tray.lock().unwrap() = Some(tray);
                *state.tray_icons.lock().unwrap() = Some((base_icon, rec_icon));
            }

            // Autostart ligado numa versão anterior não passava a flag — reaplica
            // o registro para que o próximo login também nasça quieto na bandeja.
            let autostarted = std::env::args().skip(1).any(|a| a == AUTOSTART_FLAG);
            if app.autolaunch().is_enabled().unwrap_or(false) {
                let _ = app.autolaunch().enable();
            }

            // Tela Início: só no lançamento manual (e se o usuário não desligou).
            // Vem ANTES do carregamento do modelo: sem modelo, é ela quem orienta
            // o download — as Configurações só abrem sozinhas se ela não existir.
            // (No setup a criação direta é segura; fora dele, ver `open_or_focus`.)
            if cfg.show_home_on_launch && !autostarted {
                if let Err(e) = build_home(app.handle()) {
                    tracing::error!("não consegui abrir a tela Início: {e}");
                }
            }

            // O modelo (~0,5 GB) carrega em background p/ não travar o startup.
            load_engine_in_background(app.handle().clone());

            // Enquanto o indicador estiver visível, reafirma o topo a cada 1,5 s:
            // um SetWindowPos barato que devolve a prioridade sobre qualquer
            // janela "sempre no topo" ativada depois dele (Teams, players…).
            {
                let handle = app.handle().clone();
                std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_millis(1500));
                    let visible = handle
                        .get_webview_window("overlay")
                        .and_then(|o| o.is_visible().ok())
                        .unwrap_or(false);
                    if visible {
                        assert_topmost(&handle);
                    }
                });
            }

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
                                .emit("isper-state", json!({"state": "done", "text": text}));
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

/// Rótulo legível de um combo ("ctrl+alt+space" → "Ctrl+Alt+Espaço").
fn pretty_label(combo: &str) -> String {
    if let Some((_, l)) = SHORTCUT_CANDIDATES.iter().find(|(c, _)| *c == combo) {
        return l.to_string();
    }
    combo
        .split('+')
        .map(|part| match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => "Ctrl".to_string(),
            "alt" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "super" | "win" | "meta" => "Win".to_string(),
            "space" => "Espaço".to_string(),
            other => {
                let mut c = other.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Tenta registrar, na ordem, o primeiro combo livre de `candidates` que não
/// esteja em `taken`. Devolve o `Shortcut` e o combo registrado.
fn register_first_free(
    shortcuts: &tauri_plugin_global_shortcut::GlobalShortcut<tauri::Wry>,
    candidates: &[String],
    taken: Option<&Shortcut>,
) -> Option<(Shortcut, String)> {
    for combo in candidates {
        let parsed = match Shortcut::from_str(combo) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("atalho '{combo}' inválido: {e}");
                continue;
            }
        };
        if taken == Some(&parsed) {
            continue; // já é o atalho de ditado
        }
        match shortcuts.register(parsed) {
            Ok(()) => {
                tracing::info!("atalho registrado: {}", pretty_label(combo));
                return Some((parsed, combo.clone()));
            }
            Err(e) => tracing::warn!("atalho {} indisponível: {e}", pretty_label(combo)),
        }
    }
    None
}

/// (Re)registra os dois atalhos globais — ditado e reunião — a partir da
/// configuração: o preferido de cada um tem prioridade; os candidatos padrão
/// são o fallback. Guarda os `Shortcut`s no estado (o handler compara com
/// eles) e devolve os rótulos ativos (ditado, reunião).
fn register_shortcuts(app: &AppHandle, cfg: &AppConfig) -> (String, String) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    let state = app.state::<AppState>();

    // Preferido primeiro, depois os padrões — sem repetir (o preferido costuma
    // ser um deles, e cada tentativa repetida vira um aviso no log).
    let candidates = |preferred: Option<&str>, defaults: &[&str]| -> Vec<String> {
        let mut list: Vec<String> = Vec::new();
        for c in preferred.into_iter().chain(defaults.iter().copied()) {
            let c = c.trim().to_lowercase();
            if !c.is_empty() && !list.contains(&c) {
                list.push(c);
            }
        }
        list
    };
    let dictation_defaults: Vec<&str> = SHORTCUT_CANDIDATES.iter().map(|(c, _)| *c).collect();
    let dictation = candidates(cfg.shortcut.as_deref(), &dictation_defaults);
    let (dict_sc, dict_label) = match register_first_free(&shortcuts, &dictation, None) {
        Some((sc, combo)) => (Some(sc), pretty_label(&combo)),
        None => (None, "(nenhum atalho livre!)".to_string()),
    };

    let meeting = candidates(cfg.meeting_shortcut.as_deref(), &MEETING_SHORTCUT_CANDIDATES);
    let (meet_sc, meet_label) = match register_first_free(&shortcuts, &meeting, dict_sc.as_ref()) {
        Some((sc, combo)) => (Some(sc), pretty_label(&combo)),
        None => (None, "(nenhum)".to_string()),
    };

    *state.dictation_shortcut.lock().unwrap() = dict_sc;
    *state.meeting_shortcut.lock().unwrap() = meet_sc;
    *state.active_shortcut.lock().unwrap() = dict_label.clone();
    *state.active_meeting_shortcut.lock().unwrap() = meet_label.clone();
    (dict_label, meet_label)
}

fn hint_text(label: &str) -> String {
    format!("Segure {label} para ditar (toque rápido = mãos-livres)")
}

/// Texto do item de reunião na bandeja, com o atalho ativo.
fn meeting_item_text(app: &AppHandle, recording: bool) -> String {
    let state = app.state::<AppState>();
    let label = state.active_meeting_shortcut.lock().unwrap().clone();
    let base = if recording {
        "Encerrar e transcrever a reunião"
    } else {
        "Iniciar gravação de reunião"
    };
    if label.is_empty() || label.starts_with('(') {
        base.to_string()
    } else {
        format!("{base} ({label})")
    }
}

/// Atalho de reunião: alterna a gravação, com debounce contra auto-repeat.
/// Roda fora da thread principal — abrir os dispositivos leva um instante.
fn on_meeting_hotkey(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut last = state.last_meeting_toggle.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < MEETING_HOTKEY_DEBOUNCE) {
            return;
        }
        *last = Some(Instant::now());
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = toggle_meeting(&app);
    });
}

/// Ícone da bandeja com um ponto vermelho no canto (estado "gravando"),
/// desenhado sobre o ícone normal — sem arquivo extra.
fn recording_icon(base: &Image<'static>) -> Image<'static> {
    let (w, h) = (base.width() as i32, base.height() as i32);
    let mut rgba = base.rgba().to_vec();
    let r = (w.min(h) as f32 * 0.30).max(2.0);
    let (cx, cy) = (w as f32 - r - 1.0, h as f32 - r - 1.0);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let i = ((y * w + x) * 4) as usize;
            if i + 3 >= rgba.len() {
                continue;
            }
            if d <= r + 1.0 {
                // borda escura fina para contraste em qualquer tema
                rgba[i..i + 4].copy_from_slice(&[27, 15, 13, 255]);
            }
            if d <= r - 0.6 {
                rgba[i..i + 4].copy_from_slice(&[240, 88, 72, 255]);
            }
        }
    }
    Image::new_owned(rgba, w as u32, h as u32)
}

/// Troca o ícone e o tooltip da bandeja conforme a gravação de reunião.
fn set_tray_recording(app: &AppHandle, recording: bool) {
    let state = app.state::<AppState>();
    let icons = state.tray_icons.lock().unwrap();
    let tray = state.tray.lock().unwrap();
    if let (Some((normal, rec)), Some(tray)) = (icons.as_ref(), tray.as_ref()) {
        let icon = if recording { rec } else { normal };
        let _ = tray.set_icon(Some(icon.clone()));
        let _ = tray.set_tooltip(Some(if recording {
            "ISPer — gravando reunião"
        } else {
            "ISPer — ditado e reuniões, 100% local"
        }));
    }
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
            let _ = app.emit("isper-state", json!({"state": "recording"}));
            show_overlay(app);
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
            let _ = app.emit("isper-state", json!({"state": "recording-handsfree"}));
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
    let _ = app.emit("isper-state", json!({"state": "transcribing"}));

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
    let raw_text = t.text.trim().to_string();
    if raw_text.is_empty() {
        anyhow::bail!("não entendi — tente de novo");
    }
    tracing::info!(audio_secs, infer_secs = t.infer_secs, "transcrito: {raw_text}");

    // Polimento opcional por IA (só o texto viaja). Qualquer falha cola o original.
    let text = polish_if_enabled(app, &raw_text);
    paste_text(&text)?;

    // Histórico de ditados (Fase 3) — falha aqui não pode travar o fluxo.
    if let Ok(store) = open_store() {
        let at = chrono::Local::now().format("%d/%m/%Y %H:%M:%S").to_string();
        let raw = (text != raw_text).then_some(raw_text.as_str());
        let _ = store.save_dictation(&at, &text, raw, audio_secs, t.infer_secs);
    }
    Ok(text)
}

/// Passa o ditado pelo provider de IA quando o polimento está ligado.
fn polish_if_enabled(app: &AppHandle, raw_text: &str) -> String {
    let (enabled, style) = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().unwrap();
        (cfg.polish, cfg.polish_style.clone())
    };
    if !enabled {
        return raw_text.to_string();
    }
    let settings = isper_llm::load_settings();
    match isper_llm::provider_from_settings(&settings) {
        Ok(provider) => {
            let _ = app.emit("isper-state", json!({"state": "polishing"}));
            let started = Instant::now();
            match isper_llm::polish_dictation(provider.as_ref(), raw_text, &style) {
                Ok(polished) => {
                    tracing::info!(secs = started.elapsed().as_secs_f32(), "ditado polido via {}", provider.name());
                    polished
                }
                Err(e) => {
                    tracing::warn!("polimento falhou (colando o original): {e}");
                    raw_text.to_string()
                }
            }
        }
        Err(isper_llm::LlmError::NotConfigured) => {
            tracing::info!("polimento ligado sem provider de IA — colando o original");
            raw_text.to_string()
        }
        Err(e) => {
            tracing::warn!("polimento indisponível ({e}) — colando o original");
            raw_text.to_string()
        }
    }
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
        let _ = app.emit("isper-state", json!({"state": "meeting"}));
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

/// Alterna a gravação de reunião (bandeja e tela Início). Encerrar devolve
/// `Ok` na hora — a transcrição segue em background e avisa pelo indicador;
/// falha ao INICIAR volta como erro para quem chamou mostrar.
fn toggle_meeting(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let mut slot = state.meeting.lock().unwrap();

    if let Some(handle) = slot.take() {
        drop(slot);
        *state.meeting_started.lock().unwrap() = None;
        set_meeting_text(app, &meeting_item_text(app, false));
        set_tray_recording(app, false);
        notify_status(app);
        let _ = app.emit("isper-state", json!({"state": "meeting-processing"}));
        show_overlay(app);
        let app = app.clone();
        std::thread::spawn(move || {
            match finish_meeting(&app, handle) {
                Ok(path) => {
                    tracing::info!("reunião salva em {path}");
                    let _ = app.emit("isper-state", json!({"state": "meeting-done"}));
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
            // A Biblioteca e o Início mostram a reunião nova / os totais.
            notify_status(&app);
            std::thread::sleep(Duration::from_millis(2500));
            maybe_restore_overlay(&app);
        });
        return Ok(());
    }
    drop(slot);

    let engine = { state.engine.lock().unwrap().clone() };
    state.live.lock().unwrap().clear();
    // Cada fala transcrita durante a reunião vira um evento `isper-live` (Início
    // e indicador) e fica guardada para quem abrir a janela no meio.
    let live_app = app.clone();
    let on_segment: meeting::SegmentSink = Arc::new(move |seg: &MeetingSegment| {
        let item = LiveSegment {
            speaker: seg.speaker.label(),
            start_secs: seg.start_secs,
            end_secs: seg.end_secs,
            text: seg.text.clone(),
        };
        {
            let state = live_app.state::<AppState>();
            let mut live = state.live.lock().unwrap();
            live.push(item.clone());
            if live.len() > LIVE_KEEP {
                let excess = live.len() - LIVE_KEEP;
                live.drain(..excess);
            }
        }
        let _ = live_app.emit("isper-live", &item);
    });
    let opts = {
        let cfg = state.config.lock().unwrap();
        MeetingOptions {
            lang: cfg.lang.clone(),
            initial_prompt: cfg.initial_prompt(),
            source: LoopbackSource::parse(&cfg.meeting_source),
            input_device: cfg.input_device.clone(),
            on_segment: Some(on_segment),
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
            *state.meeting_started.lock().unwrap() = Some(Instant::now());
            set_meeting_text(app, &meeting_item_text(app, true));
            set_tray_recording(app, true);
            notify_status(app);
            let _ = app.emit("isper-state", payload);
            show_overlay(app);
            Ok(())
        }
        Err(e) => {
            tracing::error!("não consegui iniciar a reunião: {e}");
            let _ = app.emit_to(
                "overlay",
                "isper-state",
                json!({"state": "error", "message": e.to_string()}),
            );
            show_overlay(app);
            let app2 = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(2500));
                maybe_restore_overlay(&app2);
            });
            Err(e)
        }
    }
}

fn open_store() -> anyhow::Result<MeetingStore> {
    let dir = PathBuf::from(std::env::var("APPDATA")?).join("ISPer");
    std::fs::create_dir_all(&dir)?;
    Ok(MeetingStore::open(&dir.join("isper.db"))?)
}

/// `Documentos\ISPer\Reunioes` — onde os Markdowns das reuniões moram.
fn meetings_dir() -> anyhow::Result<PathBuf> {
    let dir = PathBuf::from(std::env::var("USERPROFILE")?)
        .join("Documents")
        .join("ISPer")
        .join("Reunioes");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
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
        let _ = app.emit("isper-state", json!({"state": "meeting-diarize"}));
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
    let mut title = format!("Reunião — {started_at}");
    let md = meeting::to_markdown(&title, &started_at, &result);

    // O transcript é salvo ANTES do resumo: se a API falhar, nada se perde.
    let docs = meetings_dir()?;
    let md_path = docs.join(format!("reuniao-{}.md", now.format("%Y%m%d-%H%M%S")));
    std::fs::write(&md_path, &md)?;

    let store = open_store()?;
    let meeting_id = store.save(
        &title,
        &started_at,
        &result,
        Some(&md_path.to_string_lossy()),
    )?;

    // Fase 5: título + resumo por IA de nuvem, numa chamada — só o TEXTO do
    // transcript sai da máquina. Com resposta, o Markdown é regravado inteiro
    // (título novo + resumo) a partir da mesma fonte que a Biblioteca usa.
    let settings = isper_llm::load_settings();
    match isper_llm::provider_from_settings(&settings) {
        Ok(provider) => {
            let _ = app.emit("isper-state", json!({"state": "meeting-summary"}));
            match isper_llm::summarize_meeting_titled(provider.as_ref(), &md) {
                Ok(summary) => {
                    if let Some(t) = summary.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                        title = t.to_string();
                        let _ = store.rename_meeting(meeting_id, &title);
                    }
                    let _ = store.set_summary(meeting_id, summary.body.trim());
                    let labels: Vec<String> = result.segments.iter().map(|s| s.speaker.label()).collect();
                    let refs: Vec<SegmentRef<'_>> = result
                        .segments
                        .iter()
                        .zip(&labels)
                        .map(|(s, l)| SegmentRef { speaker: l, start_secs: s.start_secs, end_secs: s.end_secs, text: &s.text })
                        .collect();
                    let full = meeting::render_markdown(
                        &title,
                        &started_at,
                        result.duration_secs,
                        &refs,
                        Some(&format!(
                            "{}\n\n_Resumo gerado via {} ({})._",
                            summary.body.trim(),
                            provider.name(),
                            provider.model()
                        )),
                    );
                    if let Err(e) = std::fs::write(&md_path, full) {
                        tracing::warn!("não consegui regravar o Markdown com o resumo: {e}");
                    }
                    tracing::info!("resumo e título gerados via {}", provider.name());
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

/// Mostra a janela `label` se já existir; senão cria com `build` — numa
/// thread própria. No Windows, construir um WebView2 na thread principal de
/// dentro de um comando ou handler de evento congela o loop de eventos
/// (issue conhecida do Tauri: "use async commands and separate threads when
/// creating windows"). Foi a causa da Biblioteca em branco com o app inteiro
/// travado — inclusive o indicador, que não redimensionava nem arrastava.
fn open_or_focus<F>(app: &AppHandle, label: &'static str, build: F)
where
    F: FnOnce(&AppHandle) -> tauri::Result<tauri::WebviewWindow> + Send + 'static,
{
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        // Dois pedidos quase simultâneos: o segundo só foca o que o primeiro criou.
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_focus();
            return;
        }
        if let Err(e) = build(&app) {
            tracing::error!("não consegui abrir a janela {label}: {e}");
        }
    });
}

fn build_settings(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    tauri::WebviewWindowBuilder::new(
        app,
        "settings",
        tauri::WebviewUrl::App("settings.html".into()),
    )
    .title("ISPer — Configurações")
    .inner_size(560.0, 700.0)
    .resizable(false)
    .build()
}

fn open_settings(app: &AppHandle) {
    open_or_focus(app, "settings", build_settings);
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
    show_home_on_launch: bool,
    input_device: Option<String>,
    meeting_shortcut: Option<String>,
    active_meeting_shortcut: String,
    polish: bool,
    polish_style: String,
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
    #[serde(default = "default_true")]
    show_home_on_launch: bool,
    #[serde(default)]
    input_device: Option<String>,
    #[serde(default)]
    meeting_shortcut: Option<String>,
    #[serde(default)]
    polish: bool,
    #[serde(default)]
    polish_style: Option<String>,
}

fn default_true() -> bool {
    true
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

/// Carrega (ou recarrega) o modelo Whisper em background, publicando cada
/// passo em `EngineStatus`. Sem nenhum modelo instalado, a tela Início (se
/// aberta) orienta o download; senão, abrem-se as Configurações.
fn load_engine_in_background(app: AppHandle) {
    std::thread::spawn(move || {
        set_engine_status(&app, EngineStatus::Loading);
        let preferred = app.state::<AppState>().config.lock().unwrap().model.clone();
        let Some(path) =
            isper_models::resolve_whisper_model(preferred.as_deref(), cfg!(feature = "cuda"), &dev_dirs())
        else {
            tracing::warn!("nenhum modelo instalado");
            set_engine_status(&app, EngineStatus::Missing);
            if app.get_webview_window("home").is_none() {
                open_settings(&app);
            }
            return;
        };
        tracing::info!("carregando modelo {}", path.display());
        match WhisperEngine::new(&path) {
            Ok(engine) => {
                *app.state::<AppState>().engine.lock().unwrap() = Some(Arc::new(engine));
                let file = path
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let label = isper_models::catalog_entry(&file)
                    .map(|m| m.label.to_string())
                    .unwrap_or_else(|| {
                        file.trim_start_matches("ggml-").trim_end_matches(".bin").to_string()
                    });
                set_engine_status(&app, EngineStatus::Ready { file, label });
                tracing::info!("modelo Whisper carregado");
            }
            Err(e) => {
                tracing::error!("falha ao carregar modelo: {e}");
                set_engine_status(&app, EngineStatus::Failed { message: e.to_string() });
                let _ = app.emit_to(
                    "overlay",
                    "isper-state",
                    json!({"state": "error", "message": e.to_string()}),
                );
            }
        }
    });
}

fn set_engine_status(app: &AppHandle, status: EngineStatus) {
    *app.state::<AppState>().engine_status.lock().unwrap() = status;
    notify_status(app);
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
fn get_settings(app: AppHandle) -> Result<SettingsDto, String> {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().unwrap().clone();
    let active_shortcut = state.active_shortcut.lock().unwrap().clone();
    let active_meeting_shortcut = state.active_meeting_shortcut.lock().unwrap().clone();
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
        show_home_on_launch: cfg.show_home_on_launch,
        input_device: cfg.input_device,
        meeting_shortcut: cfg.meeting_shortcut,
        active_meeting_shortcut,
        polish: cfg.polish,
        polish_style: cfg.polish_style,
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
    let previous = state.config.lock().unwrap().clone();
    let previous_model = previous.model.clone();
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
        // Preferências do indicador não passam pela tela — preserva as atuais.
        overlay_pos: previous.overlay_pos,
        overlay_mini: previous.overlay_mini,
        show_home_on_launch: patch.show_home_on_launch,
        input_device: patch.input_device.filter(|d| !d.trim().is_empty()),
        meeting_shortcut: patch.meeting_shortcut.filter(|s| !s.trim().is_empty()),
        polish: patch.polish,
        polish_style: {
            let s = patch.polish_style.unwrap_or_default().trim().to_lowercase();
            if isper_llm::POLISH_STYLES.contains(&s.as_str()) { s } else { "clean".into() }
        },
    };
    config::save(&cfg).map_err(|e| e.to_string())?;
    *state.config.lock().unwrap() = cfg.clone();
    state.audio.set_device(cfg.input_device.clone());

    // Troca de modelo a quente: o antigo continua servindo até o novo carregar.
    if cfg.model != previous_model {
        load_engine_in_background(app.clone());
    }

    // Reaplica os atalhos na hora — sem reiniciar o app.
    let (label, _meeting_label) = register_shortcuts(&app, &cfg);
    set_hint(&app, &label);
    let recording = state.meeting.lock().unwrap().is_some();
    set_meeting_text(&app, &meeting_item_text(&app, recording));

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

    notify_status(&app);
    Ok(label)
}

#[tauri::command]
fn set_llm_key(app: AppHandle, provider: String, key: String) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("chave vazia".into());
    }
    isper_llm::set_api_key(&provider.trim().to_lowercase(), &key).map_err(|e| e.to_string())?;
    notify_status(&app);
    Ok(())
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

// ----------------------------------------------------------- biblioteca

fn build_library(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    tauri::WebviewWindowBuilder::new(
        app,
        "library",
        tauri::WebviewUrl::App("library.html".into()),
    )
    .title("ISPer — Biblioteca")
    .inner_size(980.0, 680.0)
    .min_inner_size(720.0, 480.0)
    .build()
}

fn open_library(app: &AppHandle) {
    open_or_focus(app, "library", build_library);
}

/// Mostra o indicador flutuante (útil depois de "ocultar" durante a reunião).
fn show_indicator(app: &AppHandle) {
    let meeting_active = app.state::<AppState>().meeting.lock().unwrap().is_some();
    show_overlay(app);
    if meeting_active {
        let _ = app.emit("isper-state", json!({"state": "meeting"}));
    } else {
        let _ = app.emit("isper-state", json!({"state": "idle"}));
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(2500));
            maybe_restore_overlay(&app);
        });
    }
}

#[tauri::command]
fn list_meetings(query: Option<String>) -> Result<Vec<isper_core::store::MeetingRow>, String> {
    let store = open_store().map_err(|e| e.to_string())?;
    match query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => store.search_meetings(q),
        None => store.list_meetings(),
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_meeting(id: i64) -> Result<Option<isper_core::store::MeetingDetail>, String> {
    open_store()
        .map_err(|e| e.to_string())?
        .get_meeting(id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_meeting(app: AppHandle, id: i64, title: String) -> Result<(), String> {
    if title.trim().is_empty() {
        return Err("título vazio".into());
    }
    let store = open_store().map_err(|e| e.to_string())?;
    store.rename_meeting(id, &title).map_err(|e| e.to_string())?;
    rewrite_markdown(&store, id);
    notify_status(&app);
    Ok(())
}

/// Renomeia um falante nesta reunião ("Participante 1" → "Tatiana") em todos
/// os segmentos, e regrava o `.md` para acompanhar.
#[tauri::command]
fn rename_speaker(app: AppHandle, id: i64, from: String, to: String) -> Result<usize, String> {
    let to = to.trim();
    if to.is_empty() {
        return Err("nome vazio".into());
    }
    if to.chars().count() > 40 {
        return Err("nome longo demais (máx. 40 caracteres)".into());
    }
    let store = open_store().map_err(|e| e.to_string())?;
    let n = store.rename_speaker(id, &from, to).map_err(|e| e.to_string())?;
    rewrite_markdown(&store, id);
    notify_status(&app);
    Ok(n)
}

/// Segmentos do banco como referências para os renderizadores do core.
fn segment_refs(detail: &MeetingDetail) -> Vec<SegmentRef<'_>> {
    detail
        .segments
        .iter()
        .map(|s| SegmentRef {
            speaker: &s.speaker,
            start_secs: s.start_secs,
            end_secs: s.end_secs,
            text: &s.text,
        })
        .collect()
}

/// Regrava o Markdown da reunião a partir do banco (fonte única): título,
/// falantes e resumo sempre iguais aos da Biblioteca. Falha só vai ao log —
/// o banco já está certo.
fn rewrite_markdown(store: &MeetingStore, id: i64) {
    let Ok(Some(detail)) = store.get_meeting(id) else {
        return;
    };
    let Some(path) = detail.meeting.md_path.as_deref() else {
        return;
    };
    let md = meeting::render_markdown(
        &detail.meeting.title,
        &detail.meeting.started_at,
        detail.meeting.duration_secs,
        &segment_refs(&detail),
        detail.summary.as_deref(),
    );
    if let Err(e) = std::fs::write(path, md) {
        tracing::warn!("não consegui regravar {path}: {e}");
    }
}

/// Exporta a reunião ao lado do `.md` (SRT, DOCX ou MD) e abre o arquivo.
/// Devolve o caminho gravado.
#[tauri::command]
fn export_meeting(id: i64, format: String) -> Result<String, String> {
    let store = open_store().map_err(|e| e.to_string())?;
    let detail = store
        .get_meeting(id)
        .map_err(|e| e.to_string())?
        .ok_or("reunião não encontrada")?;
    let refs = segment_refs(&detail);
    let m = &detail.meeting;
    let (ext, bytes): (&str, Vec<u8>) = match format.to_lowercase().as_str() {
        "srt" => ("srt", isper_core::export::to_srt(&refs).into_bytes()),
        "docx" => (
            "docx",
            isper_core::export::to_docx(&m.title, &m.started_at, m.duration_secs, &refs, detail.summary.as_deref()),
        ),
        "md" => (
            "md",
            meeting::render_markdown(&m.title, &m.started_at, m.duration_secs, &refs, detail.summary.as_deref())
                .into_bytes(),
        ),
        other => return Err(format!("formato desconhecido: {other}")),
    };
    let base = match m.md_path.as_deref().map(Path::new) {
        Some(p) if p.parent().is_some() => p.with_extension(""),
        _ => meetings_dir().map_err(|e| e.to_string())?.join(format!("reuniao-{id}")),
    };
    let out = base.with_extension(ext);
    std::fs::write(&out, bytes).map_err(|e| e.to_string())?;
    // Revela o arquivo no Explorer em vez de abri-lo: SRT/DOCX podem não ter
    // programa associado, e o diálogo "com qual app?" no meio do fluxo irrita.
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,{}", out.to_string_lossy()))
        .spawn();
    Ok(out.display().to_string())
}

/// Microfones disponíveis (a tela de Configurações lista; vazio = só o padrão).
#[tauri::command]
fn list_input_devices() -> Vec<String> {
    isper_core::audio::list_input_devices()
}

/// Falas já transcritas da reunião em andamento (para quem abre o Início no meio).
#[tauri::command]
fn live_transcript(app: AppHandle) -> Vec<LiveSegment> {
    let state = app.state::<AppState>();
    let live = state.live.lock().unwrap().clone();
    live
}

/// Remove do histórico; o arquivo .md continua na pasta (decisão do usuário).
#[tauri::command]
fn delete_meeting(id: i64) -> Result<(), String> {
    open_store()
        .map_err(|e| e.to_string())?
        .delete_meeting(id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_meeting_file(id: i64) -> Result<(), String> {
    let store = open_store().map_err(|e| e.to_string())?;
    let detail = store
        .get_meeting(id)
        .map_err(|e| e.to_string())?
        .ok_or("reunião não encontrada")?;
    let path = detail
        .meeting
        .md_path
        .ok_or("esta reunião não tem arquivo .md registrado")?;
    if !std::path::Path::new(&path).exists() {
        return Err(format!("arquivo não encontrado: {path}"));
    }
    std::process::Command::new("cmd")
        .args(["/C", "start", "", &path])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn open_meetings_folder() -> Result<(), String> {
    let dir = meetings_dir().map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn list_dictations(query: Option<String>) -> Result<Vec<isper_core::store::DictationRow>, String> {
    open_store()
        .map_err(|e| e.to_string())?
        .list_dictations(query.as_deref(), 300)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_dictation(id: i64) -> Result<(), String> {
    open_store()
        .map_err(|e| e.to_string())?
        .delete_dictation(id)
        .map_err(|e| e.to_string())
}

// ------------------------------------------------------------ indicador

/// HWND do indicador, capturado uma vez no setup (a janela vive até o fim do app).
fn overlay_hwnd(overlay: &tauri::WebviewWindow) -> isize {
    #[cfg(windows)]
    {
        overlay.hwnd().map(|h| h.0 as isize).unwrap_or(0)
    }
    #[cfg(not(windows))]
    {
        let _ = overlay;
        0
    }
}

/// Reafirma o indicador no topo da faixa "sempre no topo" do Windows. O
/// `alwaysOnTop` da config só liga a flag: qualquer outra janela topmost
/// ativada depois (Teams em chamada, players, outros overlays) passa na frente,
/// e a nossa — que nunca é ativada — não voltaria sozinha. O tao ignora
/// `set_always_on_top(true)` com a flag já ligada, daí o SetWindowPos direto:
/// sem ativar, sem mover, sem redimensionar.
fn assert_topmost(app: &AppHandle) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        };
        let hwnd = app.state::<AppState>().overlay_hwnd;
        if hwnd != 0 {
            // SAFETY: o HWND pertence a uma janela que só é destruída ao sair do
            // app; SetWindowPos pode ser chamado de qualquer thread.
            unsafe {
                SetWindowPos(
                    hwnd as _,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(overlay) = app.get_webview_window("overlay") {
            let _ = overlay.set_always_on_top(true);
        }
    }
}

/// Mostra o indicador e garante que ele fica por cima de tudo.
fn show_overlay(app: &AppHandle) {
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.show();
    }
    assert_topmost(app);
}

#[derive(serde::Serialize)]
struct OverlayPrefs {
    mini: bool,
    meeting_active: bool,
}

#[tauri::command]
fn overlay_prefs(app: AppHandle) -> OverlayPrefs {
    let state = app.state::<AppState>();
    let mini = state.config.lock().unwrap().overlay_mini;
    let meeting_active = state.meeting.lock().unwrap().is_some();
    OverlayPrefs {
        mini,
        meeting_active,
    }
}

/// Alterna o indicador entre normal e mini, redimensionando a janela e
/// lembrando a preferência.
#[tauri::command]
fn overlay_set_mini(app: AppHandle, mini: bool) -> Result<(), String> {
    let (w, h) = if mini { OVERLAY_MINI } else { OVERLAY_FULL };
    if let Some(overlay) = app.get_webview_window("overlay") {
        overlay
            .set_size(tauri::LogicalSize::new(w, h))
            .map_err(|e| e.to_string())?;
    }
    let state = app.state::<AppState>();
    let cfg = {
        let mut c = state.config.lock().unwrap();
        c.overlay_mini = mini;
        c.clone()
    };
    config::save(&cfg).map_err(|e| e.to_string())
}

/// Esconde o indicador (a gravação continua; volta pela bandeja).
#[tauri::command]
fn overlay_hide(app: AppHandle) {
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }
}

/// Lembra onde o usuário deixou o indicador (pixels físicos).
#[tauri::command]
fn overlay_moved(app: AppHandle, x: i32, y: i32) -> Result<(), String> {
    let state = app.state::<AppState>();
    let cfg = {
        let mut c = state.config.lock().unwrap();
        c.overlay_pos = Some((x, y));
        c.clone()
    };
    config::save(&cfg).map_err(|e| e.to_string())
}

// --------------------------------------------------------------- início

/// Janela central do app: estado do sistema, o que falta configurar,
/// ações principais, totais e reuniões recentes.
fn build_home(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    tauri::WebviewWindowBuilder::new(app, "home", tauri::WebviewUrl::App("home.html".into()))
        .title("ISPer")
        .inner_size(960.0, 680.0)
        .min_inner_size(780.0, 560.0)
        .center()
        .build()
}

fn open_home(app: &AppHandle) {
    open_or_focus(app, "home", build_home);
}

/// Avisa todas as janelas que o estado mudou (modelo, reunião, configurações,
/// downloads) — a tela Início relê `home_status` e a Biblioteca, sua lista.
fn notify_status(app: &AppHandle) {
    let _ = app.emit("isper-status", ());
}

#[derive(serde::Serialize)]
struct HomeStatus {
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
}

/// Fotografia de tudo que a tela Início mostra — uma chamada, sem estado no
/// front (que só renderiza e reage ao evento `isper-status`).
#[tauri::command]
fn home_status(app: AppHandle) -> HomeStatus {
    let state = app.state::<AppState>();
    let cfg = state.config.lock().unwrap().clone();
    let engine = state.engine_status.lock().unwrap().clone();
    let shortcut = state.active_shortcut.lock().unwrap().clone();
    let meeting_shortcut = state.active_meeting_shortcut.lock().unwrap().clone();
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
    }
}

/// Inicia/encerra a reunião a partir do Início. Roda fora da thread principal:
/// abrir os dispositivos de áudio leva um instante e a UI não pode congelar.
#[tauri::command]
async fn toggle_meeting_cmd(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        toggle_meeting(&app).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Abre a Biblioteca; com `meeting`, já com essa reunião selecionada.
/// `async`: comandos síncronos rodam na thread principal, onde criar janela
/// é proibido no Windows (ver `open_or_focus`).
#[tauri::command]
async fn open_library_window(app: AppHandle, meeting: Option<i64>) -> Result<(), String> {
    *app.state::<AppState>().pending_meeting.lock().unwrap() = meeting;
    let already_open = app.get_webview_window("library").is_some();
    open_library(&app);
    if already_open && meeting.is_some() {
        let _ = app.emit_to("library", "isper-library-select", ());
    }
    Ok(())
}

/// A Biblioteca chama ao carregar e ao receber `isper-library-select`.
#[tauri::command]
fn take_pending_meeting(app: AppHandle) -> Option<i64> {
    app.state::<AppState>().pending_meeting.lock().unwrap().take()
}

#[tauri::command]
async fn open_settings_window(app: AppHandle) -> Result<(), String> {
    open_settings(&app);
    Ok(())
}

#[tauri::command]
fn show_indicator_cmd(app: AppHandle) {
    show_indicator(&app);
}

/// Toggle do rodapé do Início: abrir (ou não) esta tela com o app.
#[tauri::command]
fn set_show_home(app: AppHandle, show: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let cfg = {
        let mut c = state.config.lock().unwrap();
        c.show_home_on_launch = show;
        c.clone()
    };
    config::save(&cfg).map_err(|e| e.to_string())
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

