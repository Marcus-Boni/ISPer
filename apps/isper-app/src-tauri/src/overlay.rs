//! Indicador flutuante: sempre no topo (SetWindowPos), mostrar/ocultar/fixar, modos e posição lembrada.

use crate::prelude::*;

/// HWND do indicador, capturado uma vez no setup (a janela vive até o fim do app).
pub(crate) fn overlay_hwnd(overlay: &tauri::WebviewWindow) -> isize {
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
pub(crate) fn assert_topmost(app: &AppHandle) {
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
pub(crate) fn show_overlay(app: &AppHandle) {
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.show();
    }
    assert_topmost(app);
}

/// O indicador está na tela agora?
pub(crate) fn overlay_visible(app: &AppHandle) -> bool {
    app.get_webview_window("overlay")
        .and_then(|o| o.is_visible().ok())
        .unwrap_or(false)
}

/// Indicador fixo em repouso (o usuário pediu para ele ficar na tela).
pub(crate) fn overlay_pinned(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .config
        .lock()
        .unwrap()
        .overlay_pinned
}

fn set_pinned(app: &AppHandle, pinned: bool) {
    let cfg = {
        let state = app.state::<AppState>();
        let mut c = state.config.lock().unwrap();
        if c.overlay_pinned == pinned {
            return;
        }
        c.overlay_pinned = pinned;
        c.clone()
    };
    if let Err(e) = config::save(&cfg) {
        tracing::warn!("não consegui gravar a preferência do indicador: {e}");
    }
}

/// Mostra o indicador e, fora de reunião, o deixa FIXO na tela até "ocultar".
/// Antes ele aparecia por 2,5 s e sumia — ninguém conseguia olhar.
pub(crate) fn show_indicator(app: &AppHandle) {
    let meeting_active = app.state::<AppState>().meeting.lock().unwrap().is_some();
    show_overlay(app);
    if meeting_active {
        let _ = app.emit("isper-state", json!({"state": "meeting"}));
    } else {
        set_pinned(app, true);
        let _ = app.emit("isper-state", json!({"state": "idle"}));
    }
    set_indicator_text(app, true);
    notify_status(app);
}

/// Oculta o indicador (a gravação continua) e desfixa; volta pela bandeja,
/// pelo botão do Início ou no próximo ditado/reunião.
pub(crate) fn hide_indicator(app: &AppHandle) {
    set_pinned(app, false);
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }
    set_indicator_text(app, false);
    notify_status(app);
}

/// Botão "Indicador" do Início e item da bandeja: alterna visível/oculto.
/// Devolve o estado novo (visível?).
pub(crate) fn toggle_indicator(app: &AppHandle) -> bool {
    if overlay_visible(app) {
        hide_indicator(app);
        false
    } else {
        show_indicator(app);
        true
    }
}

#[derive(serde::Serialize)]
pub(crate) struct OverlayPrefs {
    mini: bool,
    meeting_active: bool,
    pinned: bool,
    /// Rótulo do atalho de ditado (o indicador em repouso mostra "pronto · <atalho>").
    shortcut: String,
}

#[tauri::command]
pub(crate) fn overlay_prefs(app: AppHandle) -> OverlayPrefs {
    let state = app.state::<AppState>();
    let (mini, pinned) = {
        let cfg = state.config.lock().unwrap();
        (cfg.overlay_mini, cfg.overlay_pinned)
    };
    let meeting_active = state.meeting.lock().unwrap().is_some();
    let shortcut = state.active_shortcut.lock().unwrap().clone();
    OverlayPrefs {
        mini,
        meeting_active,
        pinned,
        shortcut,
    }
}

/// Tamanho lógico do indicador para o modo configurado.
pub(crate) fn overlay_size(cfg: &config::AppConfig) -> (f64, f64) {
    if cfg.overlay_captions {
        OVERLAY_CAPTIONS
    } else if cfg.overlay_mini {
        OVERLAY_MINI
    } else {
        OVERLAY_FULL
    }
}

/// Troca o modo do indicador — `normal`, `mini` ou `captions` (legendas ao
/// vivo) — redimensionando a janela e lembrando a preferência. A página
/// decide o layout pelo tamanho real, então ela e a janela nunca divergem.
#[tauri::command]
pub(crate) fn overlay_set_mode(app: AppHandle, mode: String) -> Result<(), String> {
    let mode = mode.trim().to_lowercase();
    if !["normal", "mini", "captions"].contains(&mode.as_str()) {
        return Err(format!("modo desconhecido: {mode}"));
    }
    let state = app.state::<AppState>();
    let cfg = {
        let mut c = state.config.lock().unwrap();
        c.overlay_mini = mode == "mini";
        c.overlay_captions = mode == "captions";
        c.clone()
    };
    let (w, h) = overlay_size(&cfg);
    if let Some(overlay) = app.get_webview_window("overlay") {
        overlay
            .set_size(tauri::LogicalSize::new(w, h))
            .map_err(|e| e.to_string())?;
    }
    config::save(&cfg).map_err(|e| e.to_string())?;
    notify_status(&app);
    Ok(())
}

/// Botão × do indicador: esconde (a gravação continua) e desfixa.
#[tauri::command]
pub(crate) fn overlay_hide(app: AppHandle) {
    hide_indicator(&app);
}

/// Lembra onde o usuário deixou o indicador (pixels físicos).
#[tauri::command]
pub(crate) fn overlay_moved(app: AppHandle, x: i32, y: i32) -> Result<(), String> {
    let state = app.state::<AppState>();
    let cfg = {
        let mut c = state.config.lock().unwrap();
        c.overlay_pos = Some((x, y));
        c.clone()
    };
    config::save(&cfg).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn show_indicator_cmd(app: AppHandle) {
    show_indicator(&app);
}

/// Alterna o indicador (botão do Início); devolve se ficou visível.
#[tauri::command]
pub(crate) fn overlay_toggle_pin(app: AppHandle) -> bool {
    toggle_indicator(&app)
}
