//! Atalhos globais (ditado e reunião): registro com fallback, rótulos e o disparo do atalho de reunião.

use crate::config::AppConfig;
use crate::prelude::*;
use std::str::FromStr;
use tauri_plugin_global_shortcut::Shortcut;

/// Rótulo legível de um combo ("ctrl+alt+space" → "Ctrl+Alt+Espaço").
pub(crate) fn pretty_label(combo: &str) -> String {
    if let Some((_, l)) = SHORTCUT_CANDIDATES.iter().find(|(c, _)| *c == combo) {
        return l.to_string();
    }
    combo
        .split('+')
        .map(|part| match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => "Ctrl".to_string(),
            "alt" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "super" | "win" | "meta" => "Win".to_string(),
            "space" => "Espaço".to_string(),
            other => {
                let mut c = other.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Tenta registrar, na ordem, o primeiro combo livre de `candidates` que não
/// esteja em `taken` (os atalhos já nossos). Devolve o `Shortcut` e o combo.
pub(crate) fn register_first_free(
    shortcuts: &tauri_plugin_global_shortcut::GlobalShortcut<tauri::Wry>,
    candidates: &[String],
    taken: &[Shortcut],
) -> Option<(Shortcut, String)> {
    for combo in candidates {
        let parsed = match Shortcut::from_str(combo) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("atalho '{combo}' inválido: {e}");
                continue;
            }
        };
        if taken.contains(&parsed) {
            continue; // já é outro atalho do ISPer
        }
        match shortcuts.register(parsed) {
            Ok(()) => {
                tracing::info!("atalho registrado: {}", pretty_label(combo));
                return Some((parsed, combo.clone()));
            }
            Err(e) => tracing::warn!("atalho {} indisponível: {e}", pretty_label(combo)),
        }
    }
    None
}

/// (Re)registra os três atalhos globais — ditado, reunião e marcar momento —
/// a partir da configuração: o preferido de cada um tem prioridade; os
/// candidatos padrão são o fallback. Guarda os `Shortcut`s no estado (o
/// handler compara com eles) e devolve os rótulos ativos, nessa ordem.
pub(crate) fn register_shortcuts(app: &AppHandle, cfg: &AppConfig) -> (String, String, String) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    let state = app.state::<AppState>();

    // Preferido primeiro, depois os padrões — sem repetir (o preferido costuma
    // ser um deles, e cada tentativa repetida vira um aviso no log).
    let candidates = |preferred: Option<&str>, defaults: &[&str]| -> Vec<String> {
        let mut list: Vec<String> = Vec::new();
        for c in preferred.into_iter().chain(defaults.iter().copied()) {
            let c = c.trim().to_lowercase();
            if !c.is_empty() && !list.contains(&c) {
                list.push(c);
            }
        }
        list
    };
    let dictation_defaults: Vec<&str> = SHORTCUT_CANDIDATES.iter().map(|(c, _)| *c).collect();
    let dictation = candidates(cfg.shortcut.as_deref(), &dictation_defaults);
    let (dict_sc, dict_label) = match register_first_free(&shortcuts, &dictation, &[]) {
        Some((sc, combo)) => (Some(sc), pretty_label(&combo)),
        None => (None, "(nenhum atalho livre!)".to_string()),
    };
    let mut taken: Vec<Shortcut> = dict_sc.into_iter().collect();

    let meeting = candidates(
        cfg.meeting_shortcut.as_deref(),
        &MEETING_SHORTCUT_CANDIDATES,
    );
    let (meet_sc, meet_label) = match register_first_free(&shortcuts, &meeting, &taken) {
        Some((sc, combo)) => (Some(sc), pretty_label(&combo)),
        None => (None, "(nenhum)".to_string()),
    };
    taken.extend(meet_sc);

    let mark = candidates(cfg.mark_shortcut.as_deref(), &MARK_SHORTCUT_CANDIDATES);
    let (mark_sc, mark_label) = match register_first_free(&shortcuts, &mark, &taken) {
        Some((sc, combo)) => (Some(sc), pretty_label(&combo)),
        None => (None, "(nenhum)".to_string()),
    };

    *state.dictation_shortcut.lock().unwrap() = dict_sc;
    *state.meeting_shortcut.lock().unwrap() = meet_sc;
    *state.mark_shortcut.lock().unwrap() = mark_sc;
    *state.active_shortcut.lock().unwrap() = dict_label.clone();
    *state.active_meeting_shortcut.lock().unwrap() = meet_label.clone();
    *state.active_mark_shortcut.lock().unwrap() = mark_label.clone();
    (dict_label, meet_label, mark_label)
}

/// Atalho de reunião: alterna a gravação, com debounce contra auto-repeat.
/// Roda fora da thread principal — abrir os dispositivos leva um instante.
pub(crate) fn on_meeting_hotkey(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut last = state.last_meeting_toggle.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < MEETING_HOTKEY_DEBOUNCE) {
            return;
        }
        *last = Some(Instant::now());
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = toggle_meeting(&app);
    });
}
