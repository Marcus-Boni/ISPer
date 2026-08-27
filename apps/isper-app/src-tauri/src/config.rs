//! Configurações do app — persistidas em `%APPDATA%\ISPer\config.toml`.
//! (As configurações de IA vivem em `llm.toml`, cuidadas pelo isper-llm;
//! a chave de API vive no Credential Manager, nunca em arquivo.)

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Atalho preferido (ex.: "ctrl+alt+space"). `None` = automático
    /// (primeiro livre da lista de candidatos).
    #[serde(default)]
    pub shortcut: Option<String>,
    /// Idioma da fala: "pt", "en", ... ou "auto".
    #[serde(default = "default_lang")]
    pub lang: String,
    /// Dicionário pessoal: termos que o Whisper costuma errar
    /// (nomes próprios, siglas, jargões).
    #[serde(default)]
    pub dictionary: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            shortcut: None,
            lang: default_lang(),
            dictionary: Vec::new(),
        }
    }
}

fn default_lang() -> String {
    "pt".into()
}

impl AppConfig {
    /// O dicionário vira o `initial_prompt` do Whisper: o modelo tende a
    /// grafar corretamente os termos que "acabou de ver".
    pub fn initial_prompt(&self) -> Option<String> {
        if self.dictionary.is_empty() {
            None
        } else {
            Some(format!("Vocabulário: {}.", self.dictionary.join(", ")))
        }
    }
}

fn path() -> Option<PathBuf> {
    std::env::var("APPDATA")
        .ok()
        .map(|a| PathBuf::from(a).join("ISPer").join("config.toml"))
}

pub fn load() -> AppConfig {
    let Some(p) = path() else {
        return AppConfig::default();
    };
    match std::fs::read_to_string(&p) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("config.toml inválido ({e}) — usando padrão");
            AppConfig::default()
        }),
        Err(_) => AppConfig::default(),
    }
}

pub fn save(cfg: &AppConfig) -> anyhow::Result<()> {
    let p = path().ok_or_else(|| anyhow::anyhow!("APPDATA não definido"))?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(p, toml::to_string_pretty(cfg)?)?;
    Ok(())
}
