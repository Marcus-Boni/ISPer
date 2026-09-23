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
    .title(crate::ui::window_title(app, "settings"))
    .theme(crate::ui::native_theme(&crate::ui::current(app).theme))
    .initialization_script(crate::ui::boot_script(&crate::ui::current(app)))
    .inner_size(600.0, 760.0)
    .min_inner_size(480.0, 520.0)
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
    .title(crate::ui::window_title(app, "library"))
    .theme(crate::ui::native_theme(&crate::ui::current(app).theme))
    .initialization_script(crate::ui::boot_script(&crate::ui::current(app)))
    .inner_size(980.0, 680.0)
    .min_inner_size(720.0, 480.0)
    .build()
}

pub(crate) fn open_library(app: &AppHandle) {
    open_or_focus(app, "library", build_library);
}

/// Abre (ou foca) a Biblioteca já com a reunião selecionada.
pub(crate) fn open_library_at(app: &AppHandle, meeting_id: i64) {
    *app.state::<AppState>().pending_meeting.lock_or_recover() = Some(meeting_id);
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
        .title(crate::ui::window_title(app, "home"))
        .theme(crate::ui::native_theme(&crate::ui::current(app).theme))
        .initialization_script(crate::ui::boot_script(&crate::ui::current(app)))
        .inner_size(960.0, 680.0)
        .min_inner_size(780.0, 560.0)
        .center()
        .build()
}

pub(crate) fn open_home(app: &AppHandle) {
    open_or_focus(app, "home", build_home);
}

/// Primeira configuração (ver `onboarding`). Fechar por qualquer caminho —
/// Concluir, Pular ou o × — conta como feita e abre o Início.
pub(crate) fn build_onboarding(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let w = tauri::WebviewWindowBuilder::new(
        app,
        crate::onboarding::LABEL,
        tauri::WebviewUrl::App("onboarding.html".into()),
    )
    .title(crate::ui::window_title(app, crate::onboarding::LABEL))
    .theme(crate::ui::native_theme(&crate::ui::current(app).theme))
    .initialization_script(crate::ui::boot_script(&crate::ui::current(app)))
    .inner_size(760.0, 640.0)
    .min_inner_size(620.0, 560.0)
    .maximizable(false)
    .center()
    .build()?;
    let handle = app.clone();
    w.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { .. } = event {
            crate::onboarding::closed(&handle);
        }
    });
    Ok(w)
}

pub(crate) fn open_onboarding(app: &AppHandle) {
    open_or_focus(app, crate::onboarding::LABEL, build_onboarding);
}

/// Janela do ISPer Copilot: HUD de decisões e notetaker em tempo real durante a reunião.
pub(crate) fn build_copilot(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    tauri::WebviewWindowBuilder::new(
        app,
        "copilot",
        tauri::WebviewUrl::App("copilot.html".into()),
    )
    .title(crate::ui::window_title(app, "copilot"))
    // A página do Copilot é sempre escura (ver `ui.rs`): a barra de título acompanha.
    .theme(Some(tauri::Theme::Dark))
    // Só o dicionário (a página não carrega o boot.js do tema): nasce no idioma certo.
    .initialization_script(crate::ui::boot_script(&crate::ui::current(app)))
    .inner_size(980.0, 720.0)
    // 360 px de mínimo porque o HUD foi desenhado para caber acoplado ao lado
    // do Teams; com o mínimo em 520 o modo sidecar não era alcançável.
    .min_inner_size(360.0, 480.0)
    .build()
}

pub(crate) fn open_copilot(app: &AppHandle) {
    open_or_focus(app, "copilot", build_copilot);
}

/// Atalho global do Copilot: traz o HUD para a frente ou o esconde.
///
/// Esconder só quando ele já está na frente — aberto atrás do Teams, o que se
/// espera do atalho é ver o Copilot, não fazê-lo sumir. Esconder (e não
/// fechar) mantém a conversa do chat e o que estiver digitado.
pub(crate) fn toggle_copilot(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("copilot")
        && w.is_visible().unwrap_or(false)
        && w.is_focused().unwrap_or(false)
    {
        let _ = w.hide();
        return;
    }
    open_copilot(app);
}

#[tauri::command]
pub(crate) async fn open_copilot_window(app: AppHandle) -> Result<(), String> {
    open_copilot(&app);
    Ok(())
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
pub(crate) async fn open_library_window(
    app: AppHandle,
    meeting: Option<i64>,
) -> Result<(), String> {
    match meeting {
        Some(id) => open_library_at(&app, id),
        None => open_library(&app),
    }
    Ok(())
}

/// A Biblioteca chama ao carregar e ao receber `isper-library-select`.
#[tauri::command]
pub(crate) fn take_pending_meeting(app: AppHandle) -> Option<i64> {
    app.state::<AppState>()
        .pending_meeting
        .lock_or_recover()
        .take()
}
