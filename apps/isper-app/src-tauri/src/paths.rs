//! Pastas do usuário. Vêm da API de pastas conhecidas do Windows (crate `dirs`,
//! `SHGetKnownFolderPath`) e só então das variáveis de ambiente: um processo
//! pode nascer sem `LOCALAPPDATA`/`APPDATA` no ambiente — aconteceu numa
//! instância aberta pelo Explorer logo depois de uma atualização, e o app
//! "perdeu" modelos e logs — e nada aqui pode depender disso.

use std::path::PathBuf;

/// `%LOCALAPPDATA%\ISPer` — modelos, logs e o ícone do toast.
pub(crate) fn local_dir() -> Option<PathBuf> {
    dirs::data_local_dir()
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .map(|p| p.join("ISPer"))
}

/// `%APPDATA%\ISPer` — `config.toml`, `llm.toml` e o banco.
pub(crate) fn roaming_dir() -> Option<PathBuf> {
    dirs::config_dir()
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .map(|p| p.join("ISPer"))
}

/// Perfil do usuário — `Documentos\ISPer\Reunioes` parte daqui.
pub(crate) fn home_dir() -> Option<PathBuf> {
    dirs::home_dir().or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

/// Variáveis de ambiente que faltam neste processo (vai para o log e para o
/// Diagnóstico: é o rastro do caso descrito acima).
pub(crate) fn missing_env_vars() -> Vec<String> {
    ["LOCALAPPDATA", "APPDATA", "USERPROFILE"]
        .iter()
        .filter(|v| std::env::var_os(v).is_none())
        .map(|v| v.to_string())
        .collect()
}
