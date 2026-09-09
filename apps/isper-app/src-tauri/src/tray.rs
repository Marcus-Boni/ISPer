//! Ícone e menu da bandeja: textos dinâmicos, ícone de gravação e o "Sair" que salva antes.

use crate::prelude::*;
use tauri::image::Image;

pub(crate) fn hint_text(label: &str) -> String {
    format!("Segure {label} para ditar (toque rápido = mãos-livres)")
}

pub(crate) fn set_hint(app: &AppHandle, label: &str) {
    let state = app.state::<AppState>();
    let guard = state.hint_item.lock().unwrap();
    if let Some(item) = guard.as_ref() {
        let _ = item.set_text(hint_text(label));
    }
}

/// Texto do item de reunião na bandeja, com o atalho ativo.
pub(crate) fn meeting_item_text(app: &AppHandle, recording: bool) -> String {
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

pub(crate) fn set_meeting_text(app: &AppHandle, text: &str) {
    let state = app.state::<AppState>();
    let guard = state.meeting_item.lock().unwrap();
    if let Some(item) = guard.as_ref() {
        let _ = item.set_text(text);
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

/// Sair pela bandeja. Com uma reunião em andamento, encerra e SALVA antes —
/// sem isso a gravação inteira se perderia com um clique.
pub(crate) fn quit_app(app: &AppHandle) {
    let handle = app.state::<AppState>().meeting.lock().unwrap().take();
    let Some(handle) = handle else {
        tracing::info!("saindo");
        app.exit(0);
        return;
    };
    tracing::info!("saindo com reunião ativa — encerrando e salvando antes");
    *app.state::<AppState>().meeting_started.lock().unwrap() = None;
    set_tray_recording(app, false);
    let _ = app.emit("isper-state", json!({"state": "meeting-processing"}));
    show_overlay(app);
    let app = app.clone();
    std::thread::spawn(move || {
        match finish_meeting(&app, handle) {
            Ok(path) => tracing::info!("reunião salva antes de sair em {path}"),
            Err(e) => tracing::warn!("ao salvar antes de sair: {e}"),
        }
        app.exit(0);
    });
}
