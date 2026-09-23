//! Ícone e menu da bandeja: textos dinâmicos, ícone de gravação e o "Sair" que salva antes.

use crate::i18n::{tr, trv};
use crate::prelude::*;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};

/// Rótulo de atalho no idioma da interface ("Espaço" → "Space").
pub(crate) fn key_label(app: &AppHandle, label: &str) -> String {
    label.replace("Espaço", &tr(app, "keys.space"))
}

pub(crate) fn hint_text(app: &AppHandle, label: &str) -> String {
    trv(app, "tray.hint", &[("label", key_label(app, label))])
}

pub(crate) fn set_hint(app: &AppHandle, label: &str) {
    let state = app.state::<AppState>();
    let guard = state.hint_item.lock_or_recover();
    if let Some(item) = guard.as_ref() {
        let _ = item.set_text(hint_text(app, label));
    }
}

/// Monta o menu da bandeja no idioma atual e guarda os itens que mudam de
/// texto (dica do atalho, reunião, indicador).
pub(crate) fn build_menu(
    app: &AppHandle,
    indicator_visible: bool,
) -> tauri::Result<Menu<tauri::Wry>> {
    let label = app
        .state::<AppState>()
        .active_shortcut
        .lock_or_recover()
        .clone();
    let recording = app.state::<AppState>().meeting.lock_or_recover().is_some();
    let hint = MenuItem::with_id(app, "hint", hint_text(app, &label), false, None::<&str>)?;
    let home_item = MenuItem::with_id(app, "home", tr(app, "tray.home"), true, None::<&str>)?;
    let copilot_item =
        MenuItem::with_id(app, "copilot", tr(app, "tray.copilot"), true, None::<&str>)?;
    let library_item =
        MenuItem::with_id(app, "library", tr(app, "tray.library"), true, None::<&str>)?;
    let settings_item = MenuItem::with_id(
        app,
        "settings",
        tr(app, "tray.settings"),
        true,
        None::<&str>,
    )?;
    let meeting_item = MenuItem::with_id(
        app,
        "meeting",
        meeting_item_text(app, recording),
        true,
        None::<&str>,
    )?;
    let indicator_item = MenuItem::with_id(
        app,
        "indicator",
        indicator_item_text(app, indicator_visible),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", tr(app, "tray.quit"), true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &hint,
            &home_item,
            &copilot_item,
            &library_item,
            &meeting_item,
            &indicator_item,
            &settings_item,
            &quit,
        ],
    )?;
    let state = app.state::<AppState>();
    *state.meeting_item.lock_or_recover() = Some(meeting_item);
    *state.hint_item.lock_or_recover() = Some(hint);
    *state.indicator_item.lock_or_recover() = Some(indicator_item);
    Ok(menu)
}

/// Remonta o menu (idioma trocado) e atualiza o tooltip.
pub(crate) fn refresh_menu(app: &AppHandle) {
    let visible = app
        .get_webview_window("overlay")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    let recording = app.state::<AppState>().meeting.lock_or_recover().is_some();
    let menu = match build_menu(app, visible) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("não consegui remontar o menu da bandeja: {e}");
            return;
        }
    };
    if let Some(tray) = app.state::<AppState>().tray.lock_or_recover().as_ref() {
        let _ = tray.set_menu(Some(menu));
        let _ = tray.set_tooltip(Some(tray_tooltip(app, recording)));
    }
}

/// Tooltip da bandeja no idioma atual.
pub(crate) fn tray_tooltip(app: &AppHandle, recording: bool) -> String {
    tr(
        app,
        if recording {
            "tray.tooltip-recording"
        } else {
            "tray.tooltip"
        },
    )
}

/// Texto do item de reunião na bandeja, com o atalho ativo.
pub(crate) fn meeting_item_text(app: &AppHandle, recording: bool) -> String {
    let state = app.state::<AppState>();
    let label = state.active_meeting_shortcut.lock_or_recover().clone();
    let base = tr(
        app,
        if recording {
            "tray.meeting-stop"
        } else {
            "tray.meeting-start"
        },
    );
    if label.is_empty() || label.starts_with('(') {
        base
    } else {
        format!("{base} ({})", key_label(app, &label))
    }
}

pub(crate) fn set_meeting_text(app: &AppHandle, text: &str) {
    let state = app.state::<AppState>();
    let guard = state.meeting_item.lock_or_recover();
    if let Some(item) = guard.as_ref() {
        let _ = item.set_text(text);
    }
}

/// Texto do item da bandeja que alterna o indicador flutuante.
pub(crate) fn indicator_item_text(app: &AppHandle, visible: bool) -> String {
    tr(
        app,
        if visible {
            "tray.indicator-hide"
        } else {
            "tray.indicator-show"
        },
    )
}

pub(crate) fn set_indicator_text(app: &AppHandle, visible: bool) {
    let state = app.state::<AppState>();
    let guard = state.indicator_item.lock_or_recover();
    if let Some(item) = guard.as_ref() {
        let _ = item.set_text(indicator_item_text(app, visible));
    }
}

/// Ícone da bandeja com um ponto vermelho no canto (estado "gravando"),
/// desenhado sobre o ícone normal — sem arquivo extra.
pub(crate) fn recording_icon(base: &Image<'static>) -> Image<'static> {
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
pub(crate) fn set_tray_recording(app: &AppHandle, recording: bool) {
    let state = app.state::<AppState>();
    let icons = state.tray_icons.lock_or_recover();
    let tray = state.tray.lock_or_recover();
    if let (Some((normal, rec)), Some(tray)) = (icons.as_ref(), tray.as_ref()) {
        let icon = if recording { rec } else { normal };
        let _ = tray.set_icon(Some(icon.clone()));
        let _ = tray.set_tooltip(Some(tray_tooltip(app, recording)));
    }
}

/// Sair pela bandeja. Com uma reunião em andamento, encerra e SALVA antes —
/// sem isso a gravação inteira se perderia com um clique.
pub(crate) fn quit_app(app: &AppHandle) {
    let handle = app.state::<AppState>().meeting.lock_or_recover().take();
    let Some(handle) = handle else {
        tracing::info!("saindo");
        app.exit(0);
        return;
    };
    tracing::info!("saindo com reunião ativa — encerrando e salvando antes");
    *app.state::<AppState>().meeting_started.lock_or_recover() = None;
    let copilot = copilot_wrap_up(app);
    set_tray_recording(app, false);
    let _ = app.emit("isper-state", json!({"state": "meeting-processing"}));
    show_overlay(app);
    let app = app.clone();
    std::thread::spawn(move || {
        match finish_meeting(&app, handle, copilot) {
            Ok(path) => tracing::info!("reunião salva antes de sair em {path}"),
            Err(e) => tracing::warn!("ao salvar antes de sair: {e}"),
        }
        app.exit(0);
    });
}

#[cfg(test)]
mod tests {
    use crate::i18n::tr_lang;

    #[test]
    fn textos_da_bandeja_nos_dois_idiomas() {
        let label = || vec![("label", "Ctrl+Alt+D".to_string())];
        assert_eq!(
            tr_lang("pt-BR", "tray.hint", &label()),
            "Segure Ctrl+Alt+D para ditar (toque rápido = mãos-livres)"
        );
        assert_eq!(
            tr_lang("en", "tray.hint", &label()),
            "Hold Ctrl+Alt+D to dictate (quick tap = hands-free)"
        );
        for lang in ["pt-BR", "en"] {
            assert_ne!(
                tr_lang(lang, "tray.indicator-hide", &[]),
                tr_lang(lang, "tray.indicator-show", &[])
            );
            assert_ne!(
                tr_lang(lang, "tray.meeting-start", &[]),
                tr_lang(lang, "tray.meeting-stop", &[])
            );
        }
        assert!(tr_lang("pt-BR", "tray.indicator-hide", &[]).starts_with("Ocultar"));
        assert!(tr_lang("en", "tray.indicator-show", &[]).starts_with("Show"));
    }
}
