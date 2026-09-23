//! Preferências de interface que valem para todas as janelas: o tema
//! (seguir o Windows, claro ou escuro).
//!
//! Como chega à página sem piscar: cada janela nasce com um script de
//! inicialização ([`boot_script`]) que define `window.__ISPER_UI`, e o
//! `assets/boot.js` — o primeiro script do `<head>` — aplica o tema no
//! `<html data-theme>` antes da primeira pintura. Uma mudança nas
//! Configurações vale na hora: o evento `isper-ui` chega a todas as janelas
//! abertas, e a barra de título nativa acompanha via `set_theme`.
//!
//! O indicador flutuante e o Copilot continuam sempre escuros: o indicador
//! flutua sobre qualquer app e precisa de contraste próprio; o Copilot ainda
//! não passou pela migração de cores (ver ROADMAP 7.5).

use crate::prelude::*;

/// Valores aceitos em `AppConfig::theme`.
pub(crate) const THEMES: [&str; 3] = ["system", "light", "dark"];

/// Janelas que acompanham o tema escolhido. As demais ficam escuras.
pub(crate) const THEMED_WINDOWS: [&str; 3] = ["home", "library", "settings"];

/// O tema da barra de título nativa: `None` segue o Windows.
pub(crate) fn native_theme(theme: &str) -> Option<tauri::Theme> {
    match theme {
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => None,
    }
}

/// O que cada janela recebe no nascimento, antes de qualquer script da página.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct UiPrefs {
    pub(crate) theme: String,
}

impl UiPrefs {
    pub(crate) fn from_config(cfg: &config::AppConfig) -> Self {
        Self {
            theme: cfg.theme.clone(),
        }
    }
}

/// Script de inicialização da janela: publica as preferências em
/// `window.__ISPER_UI` para o `assets/boot.js` aplicar.
pub(crate) fn boot_script(prefs: &UiPrefs) -> String {
    let json = serde_json::to_string(prefs).unwrap_or_else(|_| "{}".into());
    format!("window.__ISPER_UI = {json};")
}

/// Preferências atuais, lidas da config em memória.
pub(crate) fn current(app: &AppHandle) -> UiPrefs {
    UiPrefs::from_config(&app.state::<AppState>().config.lock_or_recover())
}

/// Aplica o tema às janelas abertas: barra de título nativa e evento
/// `isper-ui` para as páginas trocarem as cores na hora.
pub(crate) fn apply(app: &AppHandle, prefs: &UiPrefs) {
    let native = native_theme(&prefs.theme);
    for label in THEMED_WINDOWS {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_theme(native);
        }
    }
    let _ = app.emit("isper-ui", prefs);
}

/// Troca o tema (Configurações → Sistema → Aparência). Vale na hora, sem Salvar.
#[tauri::command]
pub(crate) fn set_ui_theme(app: AppHandle, theme: String) -> Result<String, String> {
    let state = app.state::<AppState>();
    let cfg = {
        let mut c = state.config.lock_or_recover();
        c.theme = theme;
        c.normalize();
        c.clone()
    };
    config::save(&cfg).map_err(|e| e.to_string())?;
    let prefs = UiPrefs::from_config(&cfg);
    apply(&app, &prefs);
    tracing::info!(theme = %prefs.theme, "tema da interface trocado");
    Ok(prefs.theme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tema_nativo_segue_o_windows_quando_e_sistema() {
        assert_eq!(native_theme("system"), None);
        assert_eq!(native_theme("light"), Some(tauri::Theme::Light));
        assert_eq!(native_theme("dark"), Some(tauri::Theme::Dark));
        assert_eq!(native_theme("qualquer"), None);
    }

    #[test]
    fn script_de_inicializacao_publica_json_valido() {
        let s = boot_script(&UiPrefs {
            theme: "light".into(),
        });
        assert!(s.starts_with("window.__ISPER_UI = "));
        let json = s
            .trim_start_matches("window.__ISPER_UI = ")
            .trim_end_matches(';');
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(v["theme"], "light");
    }

    /// O tema claro aparece duas vezes no CSS (escolhido à mão e "seguir o
    /// Windows" com o sistema claro): os dois blocos têm de ser idênticos.
    #[test]
    fn os_dois_blocos_do_tema_claro_sao_identicos() {
        let css = include_str!("../../ui/assets/base.css");
        let block = |start: &str| -> String {
            let from = css.find(start).expect("bloco do tema claro") + start.len();
            let rest = &css[from..];
            let end = rest
                .find("/* fim do tema claro */")
                .expect("marcador de fim");
            rest[..end]
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && *l != "}")
                .collect::<Vec<_>>()
                .join("\n")
        };
        let manual = block(":root[data-theme=\"light\"] {");
        let system = block(":root[data-theme=\"system\"] {");
        assert!(manual.contains("--bg:"), "o bloco tem os tokens");
        assert_eq!(manual, system);
    }
}
