//! Janelas do app: a principal (Início, Biblioteca, Configurações e a
//! primeira configuração, com barra lateral) e o Copilot, que fica à parte
//! por ser um painel para acoplar ao lado do Teams. O indicador flutuante é
//! a terceira e mora em `overlay.rs`.
//!
//! Antes, cada tela era uma janela própria: abrir as Configurações a partir
//! do Início punha outra janela na barra de tarefas. Agora o app tem uma
//! janela só (ver ADR 0020). As telas são páginas dentro dela (`app.html`
//! carrega cada uma num iframe e as mantém vivas), e todo pedido de "abrir
//! tela X" — bandeja, notificação, atalho, um botão de outra tela — vira uma
//! navegação nessa janela ([`navigate`]).

use crate::prelude::*;

/// Rótulo da janela principal. Todas as telas moram nela, então os eventos
/// que antes iam para "home", "library", "settings" ou "onboarding" vão para cá.
pub(crate) const MAIN: &str = "main";

/// As telas da janela principal (`<tela>.html` em `ui/`).
pub(crate) const VIEWS: [&str; 4] = ["home", "library", "settings", crate::onboarding::LABEL];

/// Para onde a janela principal deve ir: uma tela e, nas Configurações, a
/// seção (`celular`, `inteligencia`…).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct Route {
    pub(crate) view: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) section: Option<String>,
}

impl Route {
    pub(crate) fn to(view: &'static str) -> Self {
        Self {
            view,
            section: None,
        }
    }
}

/// O que o Rust sabe da janela principal entre um evento e outro.
#[derive(Debug)]
struct Shell {
    /// A tela que ela mostra (ou vai mostrar assim que a página carregar).
    view: &'static str,
    /// A página da janela já chamou `shell_ready`: um `isper-nav` chega a ela.
    ready: bool,
    /// Maximizar ao aparecer (ela fechou maximizada). Fica para a hora de
    /// mostrar: maximizar é mostrar, e a janela nasce escondida.
    maximize_on_show: bool,
}

static SHELL: Mutex<Shell> = Mutex::new(Shell {
    view: "home",
    ready: false,
    maximize_on_show: false,
});

/// Mostra a janela que nasceu escondida — já maximizada, se ela fechou assim.
fn first_show(w: &tauri::WebviewWindow) {
    let maximize = std::mem::take(&mut SHELL.lock_or_recover().maximize_on_show);
    if maximize {
        placement::show_maximized(w);
    } else {
        let _ = w.show();
    }
    let _ = w.set_focus();
}

/// Tela válida por nome (o que vem da página); `None` para qualquer outra coisa.
pub(crate) fn view_named(name: &str) -> Option<&'static str> {
    VIEWS.iter().copied().find(|v| *v == name)
}

/// A tela que a janela principal mostra agora.
pub(crate) fn current_view() -> &'static str {
    SHELL.lock_or_recover().view
}

/// Traz a janela para a frente, inclusive minimizada ou escondida.
fn reveal(w: &tauri::WebviewWindow) {
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
}

/// Leva a janela principal até `route`, criando-a se preciso.
///
/// Criar é numa thread própria: no Windows, construir um WebView2 na thread
/// principal de dentro de um comando ou handler de evento congela o loop de
/// eventos (issue conhecida do Tauri: "use async commands and separate
/// threads when creating windows"). Foi a causa da Biblioteca em branco com o
/// app inteiro travado — inclusive o indicador, que não redimensionava nem
/// arrastava.
pub(crate) fn navigate(app: &AppHandle, route: Route) {
    let left_onboarding = {
        let mut shell = SHELL.lock_or_recover();
        let left = shell.view == crate::onboarding::LABEL && route.view != crate::onboarding::LABEL;
        shell.view = route.view;
        left
    };
    // Sair da primeira configuração por qualquer caminho (a bandeja, uma
    // notificação) conta como concluí-la — como fechar a janela dela contava.
    if left_onboarding {
        crate::onboarding::mark_done(app);
    }
    if let Some(w) = app.get_webview_window(MAIN) {
        reveal(&w);
        set_title(app, &w, route.view);
        let _ = app.emit_to(MAIN, "isper-nav", &route);
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        // Dois pedidos quase simultâneos: o segundo só navega na que o primeiro criou.
        if let Some(w) = app.get_webview_window(MAIN) {
            reveal(&w);
            let _ = app.emit_to(MAIN, "isper-nav", &route);
            return;
        }
        if let Err(e) = build_main(&app, &route) {
            tracing::error!("não consegui abrir a janela principal: {e}");
        }
    });
}

/// Traz a janela principal como ela estava (clique no ícone da bandeja, um
/// segundo clique no atalho do ISPer). Fechada, abre no Início — ou na
/// primeira configuração, se ela ainda não foi feita.
pub(crate) fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        reveal(&w);
        return;
    }
    let done = app
        .state::<AppState>()
        .config
        .lock_or_recover()
        .onboarding_done;
    navigate(
        app,
        Route::to(if done {
            "home"
        } else {
            crate::onboarding::LABEL
        }),
    );
}

/// O que a página da janela recebe no nascimento, junto com as preferências
/// de interface: a tela inicial e a barra lateral recolhida ou não.
#[derive(serde::Serialize)]
struct ShellBoot<'a> {
    route: &'a Route,
    sidebar_collapsed: bool,
    version: &'static str,
}

/// Cria a janela principal já em `route`. Ela nasce escondida e aparece quando
/// a página avisa que pintou (`shell_ready`) — sem o clarão branco do WebView2
/// vazio —, com um prazo de segurança caso a página nunca avise.
pub(crate) fn build_main(app: &AppHandle, route: &Route) -> tauri::Result<tauri::WebviewWindow> {
    let prefs = crate::ui::current(app);
    let (geom, collapsed) = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock_or_recover();
        (cfg.main_window, cfg.sidebar_collapsed)
    };
    let boot = ShellBoot {
        route,
        sidebar_collapsed: collapsed,
        version: env!("CARGO_PKG_VERSION"),
    };
    let boot_json = serde_json::to_string(&boot).unwrap_or_else(|_| "{}".into());
    {
        let mut shell = SHELL.lock_or_recover();
        shell.view = route.view;
        shell.ready = false;
        shell.maximize_on_show = false;
    }
    let w = tauri::WebviewWindowBuilder::new(app, MAIN, tauri::WebviewUrl::App("app.html".into()))
        .title(crate::i18n::tr(app, title_key(route.view)))
        .theme(crate::ui::native_theme(&prefs.theme))
        .initialization_script(format!(
            "{}window.__ISPER_SHELL = {boot_json};",
            crate::ui::boot_script(&prefs)
        ))
        .inner_size(1200.0, 780.0)
        .min_inner_size(880.0, 580.0)
        .center()
        .visible(false)
        .build()?;
    restore_geometry(&w, geom);
    let handle = app.clone();
    w.on_window_event(move |event| on_main_event(&handle, event));
    // Prazo de segurança: uma página que não chama `shell_ready` (erro de JS)
    // ainda mostra a janela.
    let w2 = w.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        if !SHELL.lock_or_recover().ready && !w2.is_visible().unwrap_or(true) {
            first_show(&w2);
        }
    });
    Ok(w)
}

/// Põe a janela onde ela estava da última vez — se esse lugar ainda cai num
/// monitor ligado. Um monitor removido ou uma troca de resolução deixaria a
/// janela fora da tela; aí ela fica no centro, no tamanho padrão.
fn restore_geometry(w: &tauri::WebviewWindow, geom: Option<config::WindowGeom>) {
    let Some(mut g) = geom else { return };
    let monitors: Vec<MonitorRect> = w
        .available_monitors()
        .map(|ms| ms.iter().map(MonitorRect::from).collect())
        .unwrap_or_default();
    if !monitors.iter().any(|m| g.width <= m.w && g.height <= m.h) {
        return;
    }
    let Some((x, y)) = clamp_to_monitors((g.x, g.y), (g.width, g.height), &monitors) else {
        return;
    };
    (g.x, g.y) = (x, y);
    placement::apply_hidden(w, &g);
    SHELL.lock_or_recover().maximize_on_show = g.maximized;
}

fn on_main_event(app: &AppHandle, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { .. } = event {
        let view = {
            let mut shell = SHELL.lock_or_recover();
            shell.ready = false;
            shell.view
        };
        if let Some(g) = app
            .get_webview_window(MAIN)
            .and_then(|w| placement::read(&w))
        {
            let state = app.state::<AppState>();
            let cfg = {
                let mut c = state.config.lock_or_recover();
                c.main_window = Some(g);
                c.clone()
            };
            if let Err(e) = config::save(&cfg) {
                tracing::warn!("não consegui lembrar o tamanho da janela: {e}");
            }
        }
        // Fechar a janela na primeira configuração conta como concluí-la.
        if view == crate::onboarding::LABEL {
            crate::onboarding::mark_done(app);
            SHELL.lock_or_recover().view = "home";
        }
    }
}

/// Posição e tamanho da janela como o Windows guarda (`WINDOWPLACEMENT`): o
/// retângulo "normal" vale mesmo com a janela maximizada ou minimizada. É o
/// jeito dos apps nativos de lembrar onde a janela estava. Ler os eventos de
/// redimensionar não serve: ao maximizar, o Windows avisa o tamanho novo antes
/// de dizer que a janela está maximizada, e o tamanho maximizado viraria o
/// "normal".
mod placement {
    use crate::config::WindowGeom;

    #[cfg(windows)]
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowPlacement, SW_HIDE, SW_SHOWMAXIMIZED, SetWindowPlacement, ShowWindow,
        WINDOWPLACEMENT, WPF_RESTORETOMAXIMIZED,
    };

    #[cfg(windows)]
    fn hwnd(w: &tauri::WebviewWindow) -> Option<windows_sys::Win32::Foundation::HWND> {
        w.hwnd().ok().map(|h| h.0 as _)
    }

    #[cfg(windows)]
    fn empty() -> WINDOWPLACEMENT {
        // SAFETY: WINDOWPLACEMENT é uma estrutura C só de inteiros; zerada é válida.
        let mut p: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
        p.length = size_of::<WINDOWPLACEMENT>() as u32;
        p
    }

    /// O retângulo normal (com as bordas, em pixels físicos) e se ela está
    /// maximizada — ou voltaria maximizada, se está minimizada.
    #[cfg(windows)]
    pub(super) fn read(w: &tauri::WebviewWindow) -> Option<WindowGeom> {
        let h = hwnd(w)?;
        let mut p = empty();
        // SAFETY: HWND vivo (a janela está no CloseRequested) e estrutura com o
        // `length` preenchido, como a API pede.
        if unsafe { GetWindowPlacement(h, &mut p) } == 0 {
            return None;
        }
        let r = p.rcNormalPosition;
        let maximized =
            p.showCmd == SW_SHOWMAXIMIZED as u32 || (p.flags & WPF_RESTORETOMAXIMIZED) != 0;
        Some(WindowGeom {
            x: r.left,
            y: r.top,
            width: (r.right - r.left).max(0) as u32,
            height: (r.bottom - r.top).max(0) as u32,
            maximized,
        })
    }

    /// Aplica o retângulo normal sem mostrar a janela.
    #[cfg(windows)]
    pub(super) fn apply_hidden(w: &tauri::WebviewWindow, g: &WindowGeom) {
        let Some(h) = hwnd(w) else { return };
        let mut p = empty();
        p.showCmd = SW_HIDE as u32;
        p.rcNormalPosition.left = g.x;
        p.rcNormalPosition.top = g.y;
        p.rcNormalPosition.right = g.x + g.width as i32;
        p.rcNormalPosition.bottom = g.y + g.height as i32;
        // SAFETY: HWND da janela recém-criada e estrutura completa.
        unsafe { SetWindowPlacement(h, &p) };
    }

    /// Mostra já maximizada, sem passar pelo tamanho normal na tela.
    #[cfg(windows)]
    pub(super) fn show_maximized(w: &tauri::WebviewWindow) {
        match hwnd(w) {
            // SAFETY: HWND vivo; ShowWindow pode ser chamado de qualquer thread.
            Some(h) => unsafe {
                ShowWindow(h, SW_SHOWMAXIMIZED);
            },
            None => {
                let _ = w.maximize();
            }
        }
    }

    #[cfg(not(windows))]
    pub(super) fn read(w: &tauri::WebviewWindow) -> Option<WindowGeom> {
        let pos = w.outer_position().ok()?;
        let size = w.outer_size().ok()?;
        Some(WindowGeom {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
            maximized: w.is_maximized().unwrap_or(false),
        })
    }

    #[cfg(not(windows))]
    pub(super) fn apply_hidden(w: &tauri::WebviewWindow, g: &WindowGeom) {
        let _ = w.set_position(tauri::PhysicalPosition::new(g.x, g.y));
        let _ = w.set_size(tauri::PhysicalSize::new(g.width, g.height));
    }

    #[cfg(not(windows))]
    pub(super) fn show_maximized(w: &tauri::WebviewWindow) {
        let _ = w.show();
        let _ = w.maximize();
    }
}

/// Chave do título da janela para cada tela.
pub(crate) fn title_key(view: &str) -> &'static str {
    match view {
        "library" => "window.library",
        "settings" => "window.settings",
        v if v == crate::onboarding::LABEL => "window.onboarding",
        "copilot" => "window.copilot",
        _ => "window.home",
    }
}

/// Título nativo da janela principal: o nome da tela que ela mostra (o que
/// aparece no Alt+Tab e na barra de tarefas).
pub(crate) fn set_title(app: &AppHandle, w: &tauri::WebviewWindow, view: &str) {
    let _ = w.set_title(&crate::i18n::tr(app, title_key(view)));
}

pub(crate) fn open_home(app: &AppHandle) {
    navigate(app, Route::to("home"));
}

pub(crate) fn open_library(app: &AppHandle) {
    navigate(app, Route::to("library"));
}

pub(crate) fn open_settings(app: &AppHandle) {
    navigate(app, Route::to("settings"));
}

/// As Configurações já na seção `section` (`celular`, `inteligencia`…).
pub(crate) fn open_settings_section(app: &AppHandle, section: &str) {
    navigate(
        app,
        Route {
            view: "settings",
            section: Some(section.to_string()),
        },
    );
}

pub(crate) fn open_onboarding(app: &AppHandle) {
    navigate(app, Route::to(crate::onboarding::LABEL));
}

/// Abre a Biblioteca já com a reunião selecionada. A página pede a reunião
/// por `take_pending_meeting` ao carregar e a cada `isper-library-select`.
pub(crate) fn open_library_at(app: &AppHandle, meeting_id: i64) {
    *app.state::<AppState>().pending_meeting.lock_or_recover() = Some(meeting_id);
    open_library(app);
    let _ = app.emit_to(MAIN, "isper-library-select", ());
}

/// Janela do ISPer Copilot: HUD de decisões e notetaker em tempo real durante
/// a reunião. Fica fora da janela principal de propósito: é desenhada para
/// ficar acoplada ao lado do Teams (360 px de largura, sempre no topo).
pub(crate) fn build_copilot(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    tauri::WebviewWindowBuilder::new(
        app,
        "copilot",
        tauri::WebviewUrl::App("copilot.html".into()),
    )
    .title(crate::i18n::tr(app, title_key("copilot")))
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
    if let Some(w) = app.get_webview_window("copilot") {
        reveal(&w);
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        if let Some(w) = app.get_webview_window("copilot") {
            let _ = w.set_focus();
            return;
        }
        if let Err(e) = build_copilot(&app) {
            tracing::error!("não consegui abrir o Copilot: {e}");
        }
    });
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

/// Abre as Configurações; com `section`, já nessa seção.
#[tauri::command]
pub(crate) async fn open_settings_window(
    app: AppHandle,
    section: Option<String>,
) -> Result<(), String> {
    match section {
        Some(s) => open_settings_section(&app, &s),
        None => open_settings(&app),
    }
    Ok(())
}

/// Abre a Biblioteca; com `meeting`, já com essa reunião selecionada.
/// `async`: comandos síncronos rodam na thread principal, onde criar janela
/// é proibido no Windows (ver [`navigate`]).
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

/// Vai para o Início (a paleta de comandos e os atalhos da janela usam).
#[tauri::command]
pub(crate) async fn open_home_window(app: AppHandle) -> Result<(), String> {
    open_home(&app);
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

/// A página da janela principal pintou: a janela aparece, e a página recebe a
/// rota mais recente — a que valia ao criar a janela pode ter sido trocada
/// por outro pedido enquanto ela carregava.
#[tauri::command]
pub(crate) fn shell_ready(app: AppHandle) -> Route {
    let view = {
        let mut shell = SHELL.lock_or_recover();
        shell.ready = true;
        shell.view
    };
    if let Some(w) = app.get_webview_window(MAIN) {
        set_title(&app, &w, view);
        if !w.is_visible().unwrap_or(true) {
            first_show(&w);
        }
    }
    Route::to(view)
}

/// A janela trocou de tela por conta própria (barra lateral, paleta, atalho
/// de teclado): o título nativo acompanha, e o Rust passa a saber onde ela
/// está. Os pedidos de fora passam por [`navigate`], que faz o mesmo.
#[tauri::command]
pub(crate) fn shell_view(app: AppHandle, view: String) -> Result<(), String> {
    let view = view_named(&view).ok_or_else(|| format!("tela desconhecida: {view}"))?;
    let left_onboarding = {
        let mut shell = SHELL.lock_or_recover();
        let left = shell.view == crate::onboarding::LABEL && view != crate::onboarding::LABEL;
        shell.view = view;
        left
    };
    if left_onboarding {
        crate::onboarding::mark_done(&app);
    }
    if let Some(w) = app.get_webview_window(MAIN) {
        set_title(&app, &w, view);
    }
    Ok(())
}

/// Barra lateral recolhida (só ícones) ou aberta — lembrada entre aberturas.
#[tauri::command]
pub(crate) fn set_sidebar_collapsed(app: AppHandle, collapsed: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let cfg = {
        let mut c = state.config.lock_or_recover();
        if c.sidebar_collapsed == collapsed {
            return Ok(());
        }
        c.sidebar_collapsed = collapsed;
        c.clone()
    };
    config::save(&cfg).map_err(|e| e.to_string())
}

/// O que a barra lateral mostra o tempo todo: gravação em andamento (e há
/// quanto tempo), transcrição em segundo plano, versão nova e os atalhos.
/// Bem mais leve que `home_status`, que consulta o banco.
#[derive(serde::Serialize)]
pub(crate) struct ShellStatus {
    version: &'static str,
    meeting_active: bool,
    meeting_elapsed_secs: Option<u64>,
    /// Reunião encerrada cuja separação de falantes ainda roda.
    processing: bool,
    update: Option<String>,
    /// Atalhos globais em uso (a tela de atalhos de teclado lista).
    shortcut: String,
    meeting_shortcut: String,
    mark_shortcut: String,
    copilot_shortcut: String,
}

#[tauri::command]
pub(crate) fn shell_status(app: AppHandle) -> ShellStatus {
    let state = app.state::<AppState>();
    let meeting_active = state.meeting.lock_or_recover().is_some();
    let meeting_elapsed_secs = state
        .meeting_started
        .lock_or_recover()
        .map(|t| t.elapsed().as_secs());
    let processing = state.diarizing.lock_or_recover().is_some();
    let update = state
        .update_available
        .lock_or_recover()
        .as_ref()
        .map(|u| u.version.clone());
    let key = |m: &Mutex<String>| key_label(&app, &m.lock_or_recover());
    ShellStatus {
        version: env!("CARGO_PKG_VERSION"),
        meeting_active,
        meeting_elapsed_secs: meeting_active.then_some(meeting_elapsed_secs).flatten(),
        processing,
        update,
        shortcut: key(&state.active_shortcut),
        meeting_shortcut: key(&state.active_meeting_shortcut),
        mark_shortcut: key(&state.active_mark_shortcut),
        copilot_shortcut: key(&state.active_copilot_shortcut),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_telas_conhecidas_viram_rota() {
        for v in VIEWS {
            assert_eq!(view_named(v), Some(v));
        }
        assert_eq!(view_named("copilot"), None, "o Copilot é janela própria");
        assert_eq!(view_named("../etc"), None);
        assert_eq!(view_named(""), None);
    }

    #[test]
    fn cada_tela_tem_titulo_no_dicionario() {
        let strings = crate::i18n::strings("pt-BR");
        for v in VIEWS.iter().copied().chain(["copilot"]) {
            assert!(
                strings.get(title_key(v)).is_some_and(|s| s.is_string()),
                "{v} sem título"
            );
        }
    }

    #[test]
    fn rota_serializa_sem_secao_vazia() {
        let r = serde_json::to_value(Route::to("library")).unwrap();
        assert_eq!(r, json!({"view": "library"}));
        let r = serde_json::to_value(Route {
            view: "settings",
            section: Some("celular".into()),
        })
        .unwrap();
        assert_eq!(r, json!({"view": "settings", "section": "celular"}));
    }
}
