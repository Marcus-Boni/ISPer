//! Idioma da interface (fase 7.5): dicionários JSON em `ui/locales/`, um só
//! lugar para as janelas, a bandeja, as notificações e as mensagens que o
//! Rust devolve à tela.
//!
//! Os dicionários entram no binário (`include_str!`) — o app não lê arquivo
//! de idioma do disco, então não há como uma instalação ficar com o texto
//! pela metade. As janelas recebem o dicionário no nascimento (ver
//! [`crate::ui::boot_script`]); `ui_prefs` serve quem nasce sem ele.
//!
//! `pt-BR` é a referência: toda chave existe nele, e é para ele que qualquer
//! chave ausente em outro idioma cai. Os testes garantem que `en` tem as
//! mesmas chaves, os mesmos `{marcadores}` e a mesma forma de plural, e que
//! toda chave usada pelas páginas existe.

use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::prelude::*;

/// Valores aceitos em `AppConfig::ui_lang`.
pub(crate) const UI_LANGS: [&str; 3] = ["auto", "pt-BR", "en"];

const PT_BR: &str = include_str!("../../ui/locales/pt-BR.json");
const EN: &str = include_str!("../../ui/locales/en.json");

type Dict = Map<String, Value>;

fn parse(src: &str) -> Dict {
    match serde_json::from_str::<Value>(src) {
        Ok(Value::Object(m)) => m,
        // Um JSON quebrado não passa nos testes; em produção, cai no vazio
        // (o texto em pt-BR do HTML continua lá) em vez de derrubar o app.
        _ => Map::new(),
    }
}

fn pt_br() -> &'static Dict {
    static D: OnceLock<Dict> = OnceLock::new();
    D.get_or_init(|| parse(PT_BR))
}

fn en() -> &'static Dict {
    static D: OnceLock<Dict> = OnceLock::new();
    D.get_or_init(|| parse(EN))
}

fn dict(lang: &str) -> &'static Dict {
    if lang == "en" { en() } else { pt_br() }
}

/// O idioma do Windows, pelo registro do usuário: a lista de idiomas de
/// exibição preferidos e, na falta dela, o formato regional. Português de
/// qualquer lugar vira `pt-BR`; o resto, `en`.
fn system_lang() -> &'static str {
    static L: OnceLock<&'static str> = OnceLock::new();
    L.get_or_init(|| {
        let first = windows_ui_language().unwrap_or_default();
        if first.to_ascii_lowercase().starts_with("pt") {
            "pt-BR"
        } else if first.is_empty() {
            // Sem pista nenhuma: o público do ISPer começou no Brasil.
            "pt-BR"
        } else {
            "en"
        }
    })
}

#[cfg(windows)]
fn windows_ui_language() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let multi = |path: &str, name: &str| -> Option<String> {
        let key = hkcu.open_subkey(path).ok()?;
        let v: Vec<String> = key.get_value(name).ok()?;
        v.into_iter().find(|s| !s.trim().is_empty())
    };
    multi(r"Control Panel\Desktop", "PreferredUILanguages")
        .or_else(|| {
            multi(
                r"Control Panel\Desktop\MuiCached",
                "MachinePreferredUILanguages",
            )
        })
        .or_else(|| {
            hkcu.open_subkey(r"Control Panel\International")
                .ok()?
                .get_value::<String, _>("LocaleName")
                .ok()
        })
}

#[cfg(not(windows))]
fn windows_ui_language() -> Option<String> {
    None
}

/// `auto` → o idioma do Windows; os demais passam como estão.
pub(crate) fn resolve(ui_lang: &str) -> &'static str {
    match ui_lang {
        "pt-BR" => "pt-BR",
        "en" => "en",
        _ => system_lang(),
    }
}

/// O idioma em uso agora, pela config em memória.
pub(crate) fn current_lang(app: &AppHandle) -> &'static str {
    resolve(&app.state::<AppState>().config.lock_or_recover().ui_lang)
}

/// Dicionário completo de `lang`, com `pt-BR` preenchendo o que faltar.
pub(crate) fn strings(lang: &str) -> Value {
    let mut out = pt_br().clone();
    if lang != "pt-BR" {
        for (k, v) in dict(lang) {
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Object(out)
}

/// Texto de uma chave, com `{marcadores}` preenchidos. Plural: se o valor
/// for `{ "one", "other" }`, a variável `n` escolhe (regra de pt e en: 1 é
/// singular). Chave ausente devolve a própria chave — o teste impede.
pub(crate) fn tr_lang(lang: &str, key: &str, vars: &[(&str, String)]) -> String {
    let v = dict(lang).get(key).or_else(|| pt_br().get(key));
    let text = match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(forms)) => {
            let n = vars
                .iter()
                .find(|(k, _)| *k == "n")
                .and_then(|(_, v)| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let form = if (n - 1.0).abs() < f64::EPSILON {
                "one"
            } else {
                "other"
            };
            forms
                .get(form)
                .or_else(|| forms.get("other"))
                .and_then(Value::as_str)
                .unwrap_or(key)
                .to_string()
        }
        _ => key.to_string(),
    };
    fill(&text, vars)
}

/// [`tr_lang`] no idioma atual do app.
pub(crate) fn tr(app: &AppHandle, key: &str) -> String {
    tr_lang(current_lang(app), key, &[])
}

/// [`tr_lang`] no idioma atual, com variáveis.
pub(crate) fn trv(app: &AppHandle, key: &str, vars: &[(&str, String)]) -> String {
    tr_lang(current_lang(app), key, vars)
}

fn fill(text: &str, vars: &[(&str, String)]) -> String {
    let mut out = text.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Idiomas com dicionário.
    const LOCALES: [&str; 2] = ["pt-BR", "en"];

    fn placeholders(v: &Value) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut scan = |s: &str| {
            let mut rest = s;
            while let Some(i) = rest.find('{') {
                let after = &rest[i + 1..];
                let Some(j) = after.find('}') else { break };
                let name = &after[..j];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    out.insert(name.to_string());
                }
                rest = &after[j + 1..];
            }
        };
        match v {
            Value::String(s) => scan(s),
            Value::Object(m) => m.values().filter_map(Value::as_str).for_each(&mut scan),
            _ => {}
        }
        out
    }

    #[test]
    fn dicionarios_sao_json_valido_e_nao_vazios() {
        assert!(pt_br().len() > 50, "pt-BR tem {} chaves", pt_br().len());
        assert!(en().len() > 50, "en tem {} chaves", en().len());
    }

    #[test]
    fn en_tem_as_mesmas_chaves_marcadores_e_plurais_que_pt_br() {
        let pt: BTreeSet<&String> = pt_br().keys().collect();
        let en_keys: BTreeSet<&String> = en().keys().collect();
        let faltam: Vec<_> = pt.difference(&en_keys).collect();
        let sobram: Vec<_> = en_keys.difference(&pt).collect();
        assert!(faltam.is_empty(), "faltam em en: {faltam:?}");
        assert!(sobram.is_empty(), "sobram em en: {sobram:?}");
        for (k, v) in pt_br() {
            let e = &en()[k];
            assert_eq!(
                v.is_object(),
                e.is_object(),
                "{k}: plural em um idioma e texto simples no outro"
            );
            if let (Value::Object(a), Value::Object(b)) = (v, e) {
                assert!(
                    a.contains_key("other") && b.contains_key("other"),
                    "{k}: plural sem 'other'"
                );
            }
            assert_eq!(
                placeholders(v),
                placeholders(e),
                "{k}: marcadores diferentes"
            );
            if let Value::String(s) = e {
                assert!(!s.trim().is_empty(), "{k}: vazio em en");
            }
        }
    }

    #[test]
    fn plural_e_marcadores() {
        assert_eq!(
            tr_lang("pt-BR", "__teste_inexistente__", &[]),
            "__teste_inexistente__"
        );
        let key = pt_br()
            .iter()
            .find(|(_, v)| v.is_object())
            .map(|(k, _)| k.clone())
            .expect("há ao menos um plural");
        let one = tr_lang("en", &key, &[("n", "1".into())]);
        let many = tr_lang("en", &key, &[("n", "3".into())]);
        assert_ne!(one, many, "{key}: singular e plural iguais");
    }

    #[test]
    fn resolve_aceita_os_idiomas_e_auto_vira_um_deles() {
        assert_eq!(resolve("pt-BR"), "pt-BR");
        assert_eq!(resolve("en"), "en");
        assert!(LOCALES.contains(&resolve("auto")));
        assert!(LOCALES.contains(&resolve("xx")));
    }

    /// Toda chave citada pelas páginas (data-i18n, data-i18n-attr,
    /// data-i18n-html, data-i18n-title e t('…')) existe no pt-BR.
    #[test]
    fn toda_chave_usada_pelas_paginas_existe() {
        let pages = [
            ("home.html", include_str!("../../ui/home.html")),
            ("library.html", include_str!("../../ui/library.html")),
            ("settings.html", include_str!("../../ui/settings.html")),
            ("index.html", include_str!("../../ui/index.html")),
            ("onboarding.html", include_str!("../../ui/onboarding.html")),
            ("ui.js", include_str!("../../ui/assets/ui.js")),
        ];
        let mut missing = Vec::new();
        for (name, src) in pages {
            for key in used_keys(src) {
                if !pt_br().contains_key(&key) {
                    missing.push(format!("{name}: {key}"));
                }
            }
        }
        assert!(missing.is_empty(), "chaves sem tradução: {missing:#?}");
    }

    /// Extrai as chaves citadas num HTML/JS (sem regex: só varredura de texto).
    fn used_keys(src: &str) -> Vec<String> {
        let mut out = Vec::new();
        let grab = |s: &str, from: usize, end: char| -> Option<String> {
            let rest = &s[from..];
            let j = rest.find(end)?;
            Some(rest[..j].to_string())
        };
        for pat in ["data-i18n=\"", "data-i18n-html=\"", "data-i18n-title=\""] {
            let mut i = 0;
            while let Some(p) = src[i..].find(pat) {
                let at = i + p + pat.len();
                if let Some(k) = grab(src, at, '"') {
                    out.push(k);
                }
                i = at;
            }
        }
        let mut i = 0;
        while let Some(p) = src[i..].find("data-i18n-attr=\"") {
            let at = i + p + "data-i18n-attr=\"".len();
            if let Some(list) = grab(src, at, '"') {
                for pair in list.split(',') {
                    if let Some((_, k)) = pair.split_once(':') {
                        out.push(k.trim().to_string());
                    }
                }
            }
            i = at;
        }
        for pat in ["t('", "I18N.t('"] {
            let mut i = 0;
            while let Some(p) = src[i..].find(pat) {
                let at = i + p + pat.len();
                // `t('` precisa ser a função: antes dele, nada de letra (ex.: "set('").
                let before = src[..i + p].chars().last();
                let is_call = before
                    .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
                    || pat.starts_with("I18N");
                if is_call
                    && let Some(k) = grab(src, at, '\'')
                    && k.contains('.')
                    && !k.contains(' ')
                {
                    out.push(k);
                }
                i = at;
            }
        }
        out.sort();
        out.dedup();
        out
    }
}
