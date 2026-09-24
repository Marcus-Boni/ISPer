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
//!
//! Com [`PROFILE_ENV`] definida, os dados do usuário (banco, configurações,
//! atas e logs) saem todos de uma pasta só: é o perfil dos testes ponta a ponta.

use std::path::{Path, PathBuf};

/// Variáveis que um processo Windows normal sempre tem — e que já faltaram.
const REQUIRED_ENV: [&str; 3] = ["LOCALAPPDATA", "APPDATA", "USERPROFILE"];

/// Troca as pastas de dados do usuário por uma pasta só: a dos testes ponta a
/// ponta (`tools/e2e`), que assim nunca leem nem gravam o banco, as
/// configurações, as atas, os backups e os logs de quem os roda. Dentro dela,
/// o mesmo desenho do Windows: `AppData\Roaming\ISPer`,
/// `AppData\Local\com.isper.desktop\logs` e `Documents\ISPer`. Os modelos
/// continuam na pasta de verdade, porque são grandes e só lidos. Com o perfil,
/// o app também não mexe no registro de iniciar com o Windows
/// ([`manages_autostart`]), e o `isper-llm` guarda chaves num cofre à parte.
pub(crate) const PROFILE_ENV: &str = "ISPER_PROFILE_DIR";

/// A pasta de [`PROFILE_ENV`], quando definida (vazia não conta).
pub(crate) fn profile_dir() -> Option<PathBuf> {
    profile_from(std::env::var_os(PROFILE_ENV))
}

fn profile_from(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    value.filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// O registro de iniciar com o Windows é de quem usa o ISPer: um app de teste
/// (com perfil próprio) não o grava nem o apaga. Sem isso, um build de
/// desenvolvimento aberto pelos e2e regravava a entrada com o caminho DELE, e
/// o próximo login abria a cópia de teste no lugar da instalada.
pub(crate) fn manages_autostart() -> bool {
    profile_dir().is_none()
}

/// Arquivo que marca a versão portátil (o zip da release): fica ao lado do exe.
/// Explícito de propósito — "não tem desinstalador ao lado" também valeria
/// para um build de desenvolvimento, e o atualizador deixaria de funcionar nele.
pub(crate) const PORTABLE_MARKER: &str = "portable.txt";

/// A pasta `dir` é de uma cópia portátil (tem o marcador).
pub(crate) fn is_portable_dir(dir: &Path) -> bool {
    dir.join(PORTABLE_MARKER).is_file()
}

/// Este ISPer roda da versão portátil, e não de uma instalação: o atualizador
/// avisa da versão nova, mas não instala por cima (instalaria uma segunda
/// cópia em `Programs`); aponta a página de download. Os dados ficam nas
/// mesmas pastas da versão instalada (ADR 0012).
pub(crate) fn is_portable() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(is_portable_dir))
        .unwrap_or(false)
}

fn local_base() -> Option<PathBuf> {
    dirs::data_local_dir().or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
}

/// `%LOCALAPPDATA%\com.isper.desktop` — modelos, logs e o ícone do toast.
pub(crate) fn local_dir() -> Option<PathBuf> {
    local_base().map(|p| p.join(isper_models::APP_ID))
}

/// `%APPDATA%\ISPer` — `config.toml`, `llm.toml` e o banco.
pub(crate) fn roaming_dir() -> Option<PathBuf> {
    if let Some(profile) = profile_dir() {
        return Some(profile_roaming(&profile));
    }
    dirs::config_dir()
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .map(|p| p.join("ISPer"))
}

/// Perfil do usuário — `Documentos\ISPer\Reunioes` parte daqui.
pub(crate) fn home_dir() -> Option<PathBuf> {
    profile_dir()
        .or_else(dirs::home_dir)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

/// `%LOCALAPPDATA%\com.isper.desktop\logs` (no perfil de teste, a dele).
pub(crate) fn logs_dir() -> Option<PathBuf> {
    match profile_dir() {
        Some(profile) => Some(profile_logs(&profile)),
        None => local_dir().map(|d| d.join("logs")),
    }
}

fn profile_roaming(profile: &Path) -> PathBuf {
    profile.join("AppData").join("Roaming").join("ISPer")
}

fn profile_logs(profile: &Path) -> PathBuf {
    profile
        .join("AppData")
        .join("Local")
        .join(isper_models::APP_ID)
        .join("logs")
}

/// Move `logs` e `models` de `%LOCALAPPDATA%\ISPer` para a pasta nova (se a
/// antiga existir e a nova estiver vazia) e apaga o que sobrar da antiga quando
/// ela fica vazia. Roda antes do log abrir; idempotente. Devolve o que moveu.
pub(crate) fn migrate_legacy_local() -> Vec<&'static str> {
    match local_base() {
        Some(base) => migrate_legacy_local_in(&base),
        None => Vec::new(),
    }
}

/// A migração em si, com o `%LOCALAPPDATA%` por parâmetro (testável).
pub(crate) fn migrate_legacy_local_in(base: &Path) -> Vec<&'static str> {
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
    missing_vars(|name| std::env::var_os(name).is_some())
}

/// As obrigatórias que `present` diz não existirem, na ordem de [`REQUIRED_ENV`].
pub(crate) fn missing_vars(present: impl Fn(&str) -> bool) -> Vec<String> {
    REQUIRED_ENV
        .iter()
        .filter(|name| !present(name))
        .map(|name| name.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copia_portatil_e_reconhecida_pelo_marcador_ao_lado_do_exe() {
        let dir = std::env::temp_dir().join(format!("isper-portable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(
            !is_portable_dir(&dir),
            "sem marcador: instalação normal (ou build de dev)"
        );
        std::fs::write(dir.join(PORTABLE_MARKER), "ISPer portátil").unwrap();
        assert!(is_portable_dir(&dir));
        // Uma PASTA com o nome do marcador não conta.
        let other = dir.join("sub");
        std::fs::create_dir_all(other.join(PORTABLE_MARKER)).unwrap();
        assert!(!is_portable_dir(&other));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn temp_base(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("isper-paths-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn move_logs_e_modelos_da_pasta_antiga_e_apaga_o_que_sobrou() {
        let base = temp_base("move");
        let old = base.join("ISPer");
        std::fs::create_dir_all(old.join("logs")).unwrap();
        std::fs::write(old.join("logs").join("isper.log.2026-09-01"), "linha").unwrap();
        std::fs::create_dir_all(old.join("models")).unwrap();
        std::fs::write(old.join("models").join("ggml-tiny.bin"), "modelo").unwrap();
        std::fs::write(old.join("isper.png"), "png").unwrap();

        assert_eq!(migrate_legacy_local_in(&base), vec!["logs", "models"]);
        let new = base.join(isper_models::APP_ID);
        assert_eq!(
            std::fs::read_to_string(new.join("models").join("ggml-tiny.bin")).unwrap(),
            "modelo"
        );
        assert!(new.join("logs").join("isper.log.2026-09-01").is_file());
        assert!(!old.exists(), "a pasta antiga some quando fica vazia");
        assert!(migrate_legacy_local_in(&base).is_empty(), "idempotente");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn nao_sobrescreve_o_que_ja_existe_na_pasta_nova() {
        let base = temp_base("keep");
        let old = base.join("ISPer");
        let new = base.join(isper_models::APP_ID);
        std::fs::create_dir_all(old.join("models")).unwrap();
        std::fs::write(old.join("models").join("a.bin"), "antigo").unwrap();
        std::fs::create_dir_all(new.join("models")).unwrap();
        std::fs::write(new.join("models").join("b.bin"), "novo").unwrap();

        assert!(migrate_legacy_local_in(&base).is_empty());
        assert!(
            old.join("models").join("a.bin").is_file(),
            "o antigo fica onde está"
        );
        assert!(new.join("models").join("b.bin").is_file());
        assert!(!new.join("models").join("a.bin").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn sem_pasta_antiga_nada_acontece() {
        let base = temp_base("none");
        assert!(migrate_legacy_local_in(&base).is_empty());
        assert!(!base.join(isper_models::APP_ID).exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn perfil_de_teste_vale_so_com_uma_pasta_de_verdade() {
        assert_eq!(profile_from(None), None);
        assert_eq!(profile_from(Some("".into())), None, "vazia não conta");
        assert_eq!(
            profile_from(Some(r"C:	\perfil".into())),
            Some(PathBuf::from(r"C:	\perfil"))
        );
    }

    #[test]
    fn perfil_de_teste_imita_as_pastas_do_windows() {
        let p = Path::new(r"C:	\perfil");
        assert_eq!(
            profile_roaming(p),
            PathBuf::from(r"C:	\perfil\AppData\Roaming\ISPer")
        );
        assert_eq!(
            profile_logs(p),
            PathBuf::from(r"C:	\perfil\AppData\Local")
                .join(isper_models::APP_ID)
                .join("logs")
        );
    }

    #[test]
    fn lista_as_variaveis_que_faltam_na_ordem() {
        assert!(missing_vars(|_| true).is_empty());
        assert_eq!(
            missing_vars(|v| v != "APPDATA"),
            vec!["APPDATA".to_string()]
        );
        assert_eq!(
            missing_vars(|_| false),
            vec!["LOCALAPPDATA", "APPDATA", "USERPROFILE"]
        );
    }
}
