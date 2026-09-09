//! Notificações do Windows (toasts) para um app que roda "solto", sem
//! instalador. O Windows só mostra toasts de um AppUserModelID que ele
//! conheça; registramos o nosso em `HKCU\Software\Classes\AppUserModelId`
//! com nome e ícone — o mesmo que o `ToastNotificationManagerCompat` da
//! Microsoft faz para apps desktop não empacotados. Se ainda assim falhar,
//! caímos para o AUMID do PowerShell (o toast sai como "Windows PowerShell",
//! mas sai). Fora do Windows, tudo aqui é no-op.

#[cfg(windows)]
use std::path::PathBuf;

/// Igual ao `identifier` do tauri.conf.json: quando o instalador criar o
/// atalho do menu Iniciar com esse ID, as duas identidades coincidem.
pub const AUMID: &str = "com.isper.desktop";
#[cfg(windows)]
const ICON_PNG: &[u8] = include_bytes!("../icons/128x128.png");

/// Conteúdo de uma notificação.
pub struct Toast<'a> {
    pub title: &'a str,
    pub line1: &'a str,
    pub line2: Option<&'a str>,
    /// Sem som (para avisos secundários, como "falantes identificados").
    pub silent: bool,
}

#[cfg(windows)]
fn icon_path() -> anyhow::Result<PathBuf> {
    let dir = PathBuf::from(std::env::var("LOCALAPPDATA")?).join("ISPer");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("isper.png"))
}

/// Registra (ou atualiza) o AUMID do ISPer no registro do usuário — barato e
/// idempotente; roda a cada início.
#[cfg(windows)]
pub fn ensure_registered() -> anyhow::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let icon = icon_path()?;
    if !icon.exists() {
        std::fs::write(&icon, ICON_PNG)?;
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(format!("Software\\Classes\\AppUserModelId\\{AUMID}"))?;
    key.set_value("DisplayName", &"ISPer")?;
    key.set_value("IconUri", &icon.to_string_lossy().to_string())?;
    Ok(())
}

#[cfg(not(windows))]
pub fn ensure_registered() -> anyhow::Result<()> {
    Ok(())
}

/// Mostra a notificação. `on_click` roda quando o usuário clica nela —
/// enquanto o ISPer estiver aberto, o que na bandeja é sempre.
#[cfg(windows)]
pub fn show(toast: Toast<'_>, on_click: impl FnMut() + Send + 'static) -> anyhow::Result<()> {
    use std::sync::{Arc, Mutex};
    use tauri_winrt_notification::{Duration, IconCrop, Sound, Toast as WinToast};

    let on_click = Arc::new(Mutex::new(on_click));
    let build = |app_id: &str| {
        let mut t = WinToast::new(app_id)
            .title(toast.title)
            .text1(toast.line1)
            .duration(Duration::Short)
            .sound(if toast.silent { None } else { Some(Sound::Default) });
        if let Some(l2) = toast.line2 {
            t = t.text2(l2);
        }
        if let Ok(icon) = icon_path() {
            if icon.exists() {
                t = t.icon(&icon, IconCrop::Circular, "ISPer");
            }
        }
        let cb = Arc::clone(&on_click);
        t.on_activated(move |_args| {
            if let Ok(mut f) = cb.lock() {
                f();
            }
            Ok(())
        })
    };
    match build(AUMID).show() {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!("toast com AUMID próprio falhou ({e}); tentando o do PowerShell");
            build(WinToast::POWERSHELL_APP_ID)
                .show()
                .map_err(|e| anyhow::anyhow!("toast: {e}"))
        }
    }
}

#[cfg(not(windows))]
pub fn show(_toast: Toast<'_>, _on_click: impl FnMut() + Send + 'static) -> anyhow::Result<()> {
    Ok(())
}
