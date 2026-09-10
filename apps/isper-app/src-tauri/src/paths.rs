//! Pastas do usuário. Vêm da API de pastas conhecidas do Windows (crate `dirs`,
//! `SHGetKnownFolderPath`) e só então das variáveis de ambiente: um processo
//! pode nascer sem `LOCALAPPDATA`/`APPDATA` no ambiente — aconteceu numa
//! instância aberta pelo Explorer logo depois de uma atualização, e o app
//! "perdeu" modelos e logs — e nada aqui pode depender disso.
//!
//! Dados locais ficam em `%LOCALAPPDATA%\com.isper.desktop` (o identificador do
//! app, como a pasta do WebView2). Até a 0.12.1 ficavam em `%LOCALAPPDATA%\ISPer`,
//! que é a pasta padrão de INSTALAÇÃO por usuário do Tauri: um instalador rodado
//! à mão misturou programa e dados. `migrate_legacy_local` move o que houver.

use std::path::PathBuf;

fn local_base() -> Option<PathBuf> {
    dirs::data_local_dir().or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
}

/// `%LOCALAPPDATA%\com.isper.desktop` — modelos, logs e o ícone do toast.
pub(crate) fn local_dir() -> Option<PathBuf> {
    local_base().map(|p| p.join(isper_models::APP_ID))
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

/// Move `logs` e `models` de `%LOCALAPPDATA%\ISPer` para a pasta nova (se a
/// antiga existir e a nova estiver vazia) e apaga o que sobrar da antiga quando
/// ela fica vazia. Roda antes do log abrir; idempotente. Devolve o que moveu.
pub(crate) fn migrate_legacy_local() -> Vec<&'static str> {
    let Some(base) = local_base() else {
        return Vec::new();
    };
    let old = base.join("ISPer");
    let new = base.join(isper_models::APP_ID);
    let mut moved = Vec::new();
    for sub in ["logs", "models"] {
        if isper_models::migrate_dir(&old.join(sub), &new.join(sub)) {
            moved.push(sub);
        }
    }
    let _ = std::fs::remove_file(old.join("isper.png")); // o toast regenera
    let _ = std::fs::remove_dir(&old); // só se ficou vazia
    moved
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
