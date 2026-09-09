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
mod state;
mod shortcuts;
mod tray;
mod dictation;
mod meetings;
mod views;
mod overlay;
mod settings;
mod library;
mod home;
mod notify;
mod prelude;




use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::ShortcutState;

use isper_core::recorder::{self, RecorderEvent};

use crate::prelude::*;

/// Log no stdout (útil no terminal) E em arquivo com rotação diária: o exe é
/// `windows_subsystem`, então sem o arquivo ninguém vê um aviso sequer.
/// O guard devolvido precisa viver até o fim do `main` (descarrega o buffer).
pub(crate) fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::EnvFilter;
    // `info` por padrão; `RUST_LOG=debug` (ou `isper_core=trace`) para investigar.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let stdout = tracing_subscriber::fmt::layer().with_target(false).compact();
    let Some(dir) = logs_dir() else {
        tracing_subscriber::registry().with(filter).with(stdout).init();
        return None;
    };
    // Retenção: apaga logs com mais de 14 dias.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let cutoff = std::time::SystemTime::now() - Duration::from_secs(14 * 24 * 3600);
        for e in entries.flatten() {
            let old = e.metadata().and_then(|m| m.modified()).map(|t| t < cutoff).unwrap_or(false);
            if old && e.file_name().to_string_lossy().starts_with("isper.log") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let file = tracing_appender::rolling::daily(&dir, "isper.log");
    let (writer, guard) = tracing_appender::non_blocking(file);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_ansi(false)
        .with_writer(writer);
    tracing_subscriber::registry()
        .with(filter)
        .with(stdout)
        .with(file_layer)
        .init();
    Some(guard)
}

fn main() {
    let _log_guard = init_logging();
    tracing::info!("ISPer {} iniciando", env!("CARGO_PKG_VERSION"));
    if let Err(e) = notify::ensure_registered() {
        tracing::warn!("não consegui registrar o ISPer para notificações: {e}");
    }

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
            open_logs_folder,
            diagnostics,
            notify_test,
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
                diarizing: Mutex::new(None),
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
                    "quit" => quit_app(app),
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
