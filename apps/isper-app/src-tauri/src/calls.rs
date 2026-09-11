//! Detecção de chamada do Teams → "Gravar transcrição?".
//!
//! Uma thread sonda as sessões de áudio do Windows a cada [`POLL`] (ver
//! `isper_core::calls`). Quando o Teams entra em chamada e nada está sendo
//! gravado, o ISPer avisa por três canais — toast do Windows (clicar grava),
//! banner na tela Início e aviso no indicador flutuante — ou, no modo
//! automático, começa a gravar sozinho. Quando a chamada termina com uma
//! gravação em andamento, pergunta se deve encerrar (ou encerra sozinho, se
//! foi ele quem começou).

use crate::prelude::*;
use isper_core::calls::{CallEvent, CallSignal, CallTracker, TEAMS_PROCESSES};

const POLL: Duration = Duration::from_secs(4);
/// Leituras positivas seguidas para considerar "em chamada" (~8 s com microfone).
const START_POLLS: u32 = 2;
/// Leituras negativas seguidas para considerar a chamada encerrada (~24 s).
const END_POLLS: u32 = 6;
/// Quanto tempo o indicador fica visível com o aviso.
const OVERLAY_HINT: Duration = Duration::from_secs(7);

/// Modos de `AppConfig::call_detect`.
pub(crate) const CALL_DETECT_MODES: [&str; 3] = ["notify", "auto", "off"];

/// Estado da chamada em curso (compartilhado com a tela Início).
#[derive(Default)]
pub(crate) struct CallState {
    /// `Some` enquanto há chamada em andamento.
    pub(crate) since: Option<Instant>,
    /// O usuário respondeu "agora não" a esta chamada.
    pub(crate) dismissed: bool,
    /// A gravação em andamento foi iniciada pela detecção (modo automático) —
    /// só essa é encerrada sozinha quando a chamada acaba.
    pub(crate) auto_started: bool,
    /// A chamada terminou enquanto a gravação continuava (banner "encerrar?").
    pub(crate) ended_while_recording: bool,
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct CallInfo {
    pub(crate) since_secs: u64,
    pub(crate) dismissed: bool,
}

/// (chamada em andamento, a chamada acabou com a gravação ligada, o usuário
/// dispensou o aviso) — para o `home_status`.
pub(crate) fn call_info(app: &AppHandle) -> (Option<CallInfo>, bool, bool) {
    let state = app.state::<AppState>();
    let call = state.call.lock_or_recover();
    (
        call.since.map(|s| CallInfo {
            since_secs: s.elapsed().as_secs(),
            dismissed: call.dismissed,
        }),
        call.ended_while_recording,
        call.dismissed,
    )
}

/// Sonda o Teams para sempre; o modo é relido a cada volta, então mudar nas
/// Configurações vale na hora.
pub(crate) fn start_call_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut tracker = CallTracker::new(START_POLLS, END_POLLS);
        let mut warned = false;
        loop {
            std::thread::sleep(POLL);
            let mode = app
                .state::<AppState>()
                .config
                .lock_or_recover()
                .call_detect
                .clone();
            if mode == "off" {
                if tracker.in_call() {
                    tracker.reset();
                    clear_call(&app);
                }
                continue;
            }
            let signal = match isper_core::calls::probe(&TEAMS_PROCESSES) {
                Ok(s) => s,
                Err(e) => {
                    if !warned {
                        tracing::warn!("detecção de chamada indisponível: {e}");
                        warned = true;
                    }
                    CallSignal::default()
                }
            };
            match tracker.update(signal) {
                Some(CallEvent::Started) => on_call_started(&app, &mode),
                Some(CallEvent::Ended) => on_call_ended(&app, &mode),
                None => {}
            }
        }
    });
}

fn clear_call(app: &AppHandle) {
    *app.state::<AppState>().call.lock_or_recover() = CallState::default();
    notify_status(app);
}

fn meeting_active(app: &AppHandle) -> bool {
    app.state::<AppState>().meeting.lock_or_recover().is_some()
}

fn on_call_started(app: &AppHandle, mode: &str) {
    {
        let state = app.state::<AppState>();
        *state.call.lock_or_recover() = CallState {
            since: Some(Instant::now()),
            ..CallState::default()
        };
    }
    let recording = meeting_active(app);
    tracing::info!(mode, recording, "chamada do Teams detectada");
    if recording {
        notify_status(app);
        return;
    }

    let engine_ready = matches!(
        *app.state::<AppState>().engine_status.lock_or_recover(),
        EngineStatus::Ready { .. }
    );
    if mode == "auto" && engine_ready {
        match toggle_meeting(app) {
            Ok(()) => {
                app.state::<AppState>().call.lock_or_recover().auto_started = true;
                let app2 = app.clone();
                let _ = notify::show(
                    notify::Toast {
                        title: "Gravando a chamada do Teams",
                        line1: "A transcrição começou automaticamente.",
                        line2: Some("Clique para abrir o ISPer · o áudio nunca sai da sua máquina"),
                        silent: true,
                    },
                    move || open_home(&app2),
                );
                notify_status(app);
                return;
            }
            Err(e) => tracing::warn!("gravação automática falhou ({e}) — perguntando"),
        }
    }

    let shortcut = {
        let label = app
            .state::<AppState>()
            .active_meeting_shortcut
            .lock_or_recover()
            .clone();
        (!label.is_empty() && !label.starts_with('(')).then_some(label)
    };
    let line2 = match &shortcut {
        Some(label) => {
            format!("Clique para gravar (ou {label}) · o áudio nunca sai da sua máquina")
        }
        None => "Clique para gravar · o áudio nunca sai da sua máquina".to_string(),
    };
    let app2 = app.clone();
    if let Err(e) = notify::show(
        notify::Toast {
            title: "Chamada do Teams em andamento",
            line1: "Gravar a transcrição desta reunião?",
            line2: Some(&line2),
            silent: false,
        },
        move || record_from_prompt(&app2),
    ) {
        tracing::warn!("notificação de chamada indisponível: {e}");
    }
    let hint = match &shortcut {
        Some(label) => format!("Teams em chamada — gravar? ({label})"),
        None => "Teams em chamada — gravar? (tela Início)".to_string(),
    };
    show_call_hint(app, &hint);
    notify_status(app);
}

fn on_call_ended(app: &AppHandle, mode: &str) {
    let recording = meeting_active(app);
    let auto_started = {
        let state = app.state::<AppState>();
        let mut call = state.call.lock_or_recover();
        let auto = call.auto_started;
        call.since = None;
        call.dismissed = false;
        call.ended_while_recording = recording;
        auto
    };
    tracing::info!(mode, recording, auto_started, "chamada do Teams encerrada");
    if recording {
        if mode == "auto" && auto_started {
            tracing::info!("encerrando a gravação iniciada automaticamente");
            let _ = toggle_meeting(app);
        } else {
            let app2 = app.clone();
            let _ = notify::show(
                notify::Toast {
                    title: "A chamada do Teams terminou",
                    line1: "Encerrar a gravação e transcrever?",
                    line2: Some("Clique para encerrar agora · ou continue gravando"),
                    silent: true,
                },
                move || {
                    if meeting_active(&app2) {
                        let _ = toggle_meeting(&app2);
                    }
                },
            );
            show_call_hint(app, "chamada encerrada — encerrar a gravação?");
        }
    }
    notify_status(app);
}

/// Mostra o aviso no indicador por alguns segundos e devolve o estado normal.
fn show_call_hint(app: &AppHandle, text: &str) {
    if meeting_active(app) {
        // O indicador já mostra a reunião; um aviso passageiro no lugar do status.
        let _ = app.emit_to(
            "overlay",
            "isper-state",
            json!({"state": "meeting", "message": text}),
        );
        return;
    }
    let _ = app.emit_to(
        "overlay",
        "isper-state",
        json!({"state": "call", "message": text}),
    );
    show_overlay(app);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(OVERLAY_HINT);
        maybe_restore_overlay(&app);
    });
}

/// "Gravar" — pela notificação ou pelo banner do Início.
fn record_from_prompt(app: &AppHandle) {
    if meeting_active(app) {
        open_home(app);
        return;
    }
    app.state::<AppState>().call.lock_or_recover().dismissed = true;
    match toggle_meeting(app) {
        Ok(()) => tracing::info!("gravação iniciada a partir do aviso de chamada"),
        Err(e) => tracing::warn!("não consegui gravar a chamada: {e}"),
    }
    notify_status(app);
}

/// A gravação terminou (por qualquer caminho): os avisos ligados a ela somem.
pub(crate) fn on_meeting_stopped(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut call = state.call.lock_or_recover();
    call.auto_started = false;
    call.ended_while_recording = false;
}

/// "Agora não" no banner do Início: some até a próxima chamada.
#[tauri::command]
pub(crate) fn dismiss_call_prompt(app: AppHandle) {
    app.state::<AppState>().call.lock_or_recover().dismissed = true;
    notify_status(&app);
}

/// "Gravar" no banner do Início (fora da thread principal: abre áudio).
#[tauri::command]
pub(crate) async fn record_call_cmd(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || record_from_prompt(&app))
        .await
        .map_err(|e| e.to_string())
}
