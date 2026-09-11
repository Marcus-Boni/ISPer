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
            HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
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

/// Retângulo de um monitor, em pixels físicos (como o Tauri os informa).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MonitorRect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

impl From<&tauri::Monitor> for MonitorRect {
    fn from(m: &tauri::Monitor) -> Self {
        Self {
            x: m.position().x,
            y: m.position().y,
            w: m.size().width,
            h: m.size().height,
        }
    }
}

/// Quanto do indicador precisa continuar visível para a posição lembrada valer.
const MIN_VISIBLE_PX: i32 = 40;

/// Confere a posição lembrada do indicador contra os monitores de agora. Um
/// monitor desligado, uma troca de resolução ou de escala (DPI) muda o mapa
/// de pixels físicos e deixaria o indicador fora da tela — e ele não é
/// focável, então ninguém conseguiria trazê-lo de volta. Devolve a posição
/// (empurrada para dentro do monitor com que mais se sobrepõe, se estava
/// parcialmente fora) ou `None` quando nenhum monitor a contém — aí vale a
/// posição padrão.
pub(crate) fn clamp_to_monitors(
    pos: (i32, i32),
    size: (u32, u32),
    monitors: &[MonitorRect],
) -> Option<(i32, i32)> {
    let (x, y) = (pos.0 as i64, pos.1 as i64);
    let (w, h) = (size.0 as i64, size.1 as i64);
    // Largura e altura da janela que caem dentro de cada monitor.
    let visible = |m: &MonitorRect| -> (i64, i64) {
        let (mx, my, mw, mh) = (m.x as i64, m.y as i64, m.w as i64, m.h as i64);
        (
            (x + w).min(mx + mw) - x.max(mx),
            (y + h).min(my + mh) - y.max(my),
        )
    };
    let min = MIN_VISIBLE_PX as i64;
    let best = monitors
        .iter()
        .map(|m| (visible(m), m))
        .filter(|((vw, vh), _)| *vw >= min && *vh >= min)
        .max_by_key(|((vw, vh), _)| vw * vh)?
        .1;
    // Cabe? Encosta na borda. Não cabe (janela maior que o monitor)? Alinha
    // ao canto do monitor, onde os controles ficam visíveis.
    let clamp = |v: i64, lo: i64, hi: i64| if hi < lo { lo } else { v.clamp(lo, hi) };
    let (mx, my, mw, mh) = (best.x as i64, best.y as i64, best.w as i64, best.h as i64);
    Some((
        clamp(x, mx, mx + mw - w) as i32,
        clamp(y, my, my + mh - h) as i32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: MonitorRect = MonitorRect {
        x: 0,
        y: 0,
        w: 2560,
        h: 1440,
    };
    /// Segundo monitor à esquerda do principal (coordenadas negativas, como
    /// o Windows faz) e mais baixo.
    const LEFT: MonitorRect = MonitorRect {
        x: -1920,
        y: 200,
        w: 1920,
        h: 1080,
    };
    const SIZE: (u32, u32) = (460, 104);

    #[test]
    fn posicao_dentro_de_um_monitor_fica_como_esta() {
        assert_eq!(
            clamp_to_monitors((1050, 1240), SIZE, &[MAIN]),
            Some((1050, 1240))
        );
        assert_eq!(
            clamp_to_monitors((-1200, 900), SIZE, &[MAIN, LEFT]),
            Some((-1200, 900))
        );
    }

    #[test]
    fn parcialmente_fora_e_empurrada_para_dentro() {
        // Passou da borda direita e de baixo do principal.
        assert_eq!(
            clamp_to_monitors((2400, 1400), SIZE, &[MAIN]),
            Some((2100, 1336))
        );
        // Um pouco acima do topo do monitor da esquerda.
        assert_eq!(
            clamp_to_monitors((-1000, 150), SIZE, &[MAIN, LEFT]),
            Some((-1000, 200))
        );
    }

    #[test]
    fn fora_de_todos_os_monitores_volta_none() {
        // Monitor da esquerda foi desligado: só o principal restou.
        assert_eq!(clamp_to_monitors((-1200, 900), SIZE, &[MAIN]), None);
        // Quase toda fora (só 20 px visíveis) também não vale.
        assert_eq!(clamp_to_monitors((2540, 700), SIZE, &[MAIN]), None);
        assert_eq!(clamp_to_monitors((100, 100), SIZE, &[]), None);
    }

    #[test]
    fn janela_maior_que_o_monitor_alinha_ao_canto() {
        let tiny = MonitorRect {
            x: 0,
            y: 0,
            w: 300,
            h: 80,
        };
        assert_eq!(clamp_to_monitors((10, 10), SIZE, &[tiny]), Some((0, 0)));
    }
}
