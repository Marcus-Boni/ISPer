//! Configuração do provider (arquivo TOML) e chaves de API (Credential
//! Manager do Windows — a chave nunca toca disco em texto plano).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{LlmError, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LlmSettings {
    /// "claude", "groq", "gemini" ou "" (não configurado).
    #[serde(default)]
    pub provider: String,
    /// Modelo específico; `None` usa o padrão do provider.
    #[serde(default)]
    pub model: Option<String>,
}

/// `%APPDATA%\ISPer\llm.toml` — pela API de pastas conhecidas do Windows, com
/// a variável de ambiente como reserva (um processo pode nascer sem ela).
fn config_path() -> Result<PathBuf> {
    let appdata = dirs::config_dir()
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .ok_or_else(|| LlmError::Keyring("APPDATA não definido".into()))?;
    Ok(appdata.join("ISPer").join("llm.toml"))
}

/// Carrega as configurações; ausência de arquivo = padrão (não configurado).
pub fn load_settings() -> LlmSettings {
    let Ok(path) = config_path() else {
        return LlmSettings::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("llm.toml inválido ({e}) — usando padrão");
            LlmSettings::default()
        }),
        Err(_) => LlmSettings::default(),
    }
}

pub fn save_settings(settings: &LlmSettings) -> Result<()> {
    let path = config_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string_pretty(settings)
        .map_err(|e| LlmError::BadResponse(format!("serializar settings: {e}")))?;
    std::fs::write(path, text)?;
    Ok(())
}

const KEYRING_SERVICE: &str = "ISPer";

fn entry(provider: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, provider).map_err(|e| LlmError::Keyring(e.to_string()))
}

/// Guarda a chave no Credential Manager do Windows.
pub fn set_api_key(provider: &str, key: &str) -> Result<()> {
    entry(provider)?
        .set_password(key.trim())
        .map_err(|e| LlmError::Keyring(e.to_string()))
}

/// Busca a chave: Credential Manager primeiro, env `ISPER_<PROVIDER>_API_KEY`
/// como fallback (útil p/ CI e testes).
pub fn get_api_key(provider: &str) -> Result<Option<String>> {
    match entry(provider)?.get_password() {
        Ok(key) => return Ok(Some(key)),
        Err(keyring::Error::NoEntry) => {}
        Err(e) => return Err(LlmError::Keyring(e.to_string())),
    }
    let env_name = format!("ISPER_{}_API_KEY", provider.to_uppercase());
    Ok(std::env::var(env_name).ok().filter(|k| !k.is_empty()))
}

pub fn delete_api_key(provider: &str) -> Result<()> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(LlmError::Keyring(e.to_string())),
    }
}
