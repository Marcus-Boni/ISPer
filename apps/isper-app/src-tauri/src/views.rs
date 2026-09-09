//! Janelas (Início, Biblioteca, Configurações): criação segura fora da thread principal e foco.

use crate::prelude::*;

/// Mostra a janela `label` se já existir; senão cria com `build` — numa
/// thread própria. No Windows, construir um WebView2 na thread principal de
/// dentro de um comando ou handler de evento congela o loop de eventos
/// (issue conhecida do Tauri: "use async commands and separate threads when
/// creating windows"). Foi a causa da Biblioteca em branco com o app inteiro
/// travado — inclusive o indicador, que não redimensionava nem arrastava.
pub(crate) fn open_or_focus<F>(app: &AppHandle, label: &'static str, build: F)
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

pub(crate) fn build_settings(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
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

pub(crate) fn open_settings(app: &AppHandle) {
    open_or_focus(app, "settings", build_settings);
}

pub(crate) fn build_library(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
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

pub(crate) fn open_library(app: &AppHandle) {
    open_or_focus(app, "library", build_library);
}

/// Abre (ou foca) a Biblioteca já com a reunião selecionada.
pub(crate) fn open_library_at(app: &AppHandle, meeting_id: i64) {
    *app.state::<AppState>().pending_meeting.lock().unwrap() = Some(meeting_id);
    let already_open = app.get_webview_window("library").is_some();
    open_library(app);
    if already_open {
        let _ = app.emit_to("library", "isper-library-select", ());
    }
}

/// Janela central do app: estado do sistema, o que falta configurar,
/// ações principais, totais e reuniões recentes.
pub(crate) fn build_home(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    tauri::WebviewWindowBuilder::new(app, "home", tauri::WebviewUrl::App("home.html".into()))
        .title("ISPer")
        .inner_size(960.0, 680.0)
        .min_inner_size(780.0, 560.0)
        .center()
        .build()
}

pub(crate) fn open_home(app: &AppHandle) {
    open_or_focus(app, "home", build_home);
}

#[tauri::command]
pub(crate) async fn open_settings_window(app: AppHandle) -> Result<(), String> {
    open_settings(&app);
    Ok(())
}

/// Abre a Biblioteca; com `meeting`, já com essa reunião selecionada.
/// `async`: comandos síncronos rodam na thread principal, onde criar janela
/// é proibido no Windows (ver `open_or_focus`).
#[tauri::command]
pub(crate) async fn open_library_window(app: AppHandle, meeting: Option<i64>) -> Result<(), String> {
    match meeting {
        Some(id) => open_library_at(&app, id),
        None => open_library(&app),
    }
    Ok(())
}

/// A Biblioteca chama ao carregar e ao receber `isper-library-select`.
#[tauri::command]
pub(crate) fn take_pending_meeting(app: AppHandle) -> Option<i64> {
    app.state::<AppState>().pending_meeting.lock().unwrap().take()
}
