//! Atualização automática: consulta o `latest.json` das releases do ISPer no
//! GitHub (plugin updater do Tauri), avisa no Início/Configurações e instala
//! só quando o usuário pede. O pacote baixado precisa bater com a assinatura
//! minisign feita pela chave privada de quem publica — a pública fica no
//! `tauri.conf.json` — senão é descartado. A checagem de fundo só lê um JSON;
//! sem rede ou sem release publicada, ela falha em silêncio (no log).

use crate::prelude::*;
use tauri_plugin_updater::{Update, Updater, UpdaterExt};

/// Primeira checagem depois de o app estabilizar (modelo carregando etc.).
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(45);
/// Depois, uma vez por dia enquanto o app viver na bandeja.
const RECHECK_EVERY: Duration = Duration::from_secs(24 * 3600);
/// Progresso do download: um evento a cada ~1 % ou 250 ms, o que vier antes.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Endpoint alternativo, para testes ponta a ponta com um servidor local
/// (só funciona num build com `dangerousInsecureTransportProtocol`).
const ENDPOINT_ENV: &str = "ISPER_UPDATE_ENDPOINT";
/// Baixa e verifica a assinatura, mas não instala (testes ponta a ponta).
const DRY_RUN_ENV: &str = "ISPER_UPDATE_DRY_RUN";

/// Versões já anunciadas por toast nesta execução (uma vez por versão).
static ANNOUNCED: Mutex<Option<String>> = Mutex::new(None);

/// O que o Início e as Configurações mostram sobre a versão disponível.
#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct UpdateInfo {
    pub(crate) version: String,
    pub(crate) current: String,
    pub(crate) notes: Option<String>,
    /// Data de publicação, `dd/mm/aaaa`.
    pub(crate) date: Option<String>,
}

fn build_updater(app: &AppHandle) -> anyhow::Result<Updater> {
    let mut builder = app.updater_builder();
    if let Ok(endpoint) = std::env::var(ENDPOINT_ENV) {
        let url: tauri::Url = endpoint.parse()?;
        builder = builder.endpoints(vec![url])?;
        tracing::warn!("endpoint de atualização sobrescrito por {ENDPOINT_ENV}: {endpoint}");
    }
    Ok(builder.build()?)
}

fn describe(update: &Update) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        current: update.current_version.clone(),
        notes: update
            .body
            .as_deref()
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .map(str::to_string),
        date: update
            .date
            .map(|d| format!("{:02}/{:02}/{}", d.day(), d.month() as u8, d.year())),
    }
}

/// Erros do plugin em português, pelo que importa para quem lê: sem release,
/// sem rede ou assinatura inválida. O resto passa como veio.
fn friendly(e: impl std::fmt::Display) -> anyhow::Error {
    let raw = e.to_string();
    let lower = raw.to_lowercase();
    let msg = if lower.contains("404") || lower.contains("could not fetch a valid release") {
        "nenhuma versão publicada para esta plataforma ainda (o servidor não devolveu um latest.json válido)".to_string()
    } else if lower.contains("dns")
        || lower.contains("connect")
        || lower.contains("timed out")
        || lower.contains("error sending request")
    {
        format!("sem conexão com o servidor de atualizações ({raw})")
    } else if lower.contains("signature") || lower.contains("minisign") {
        "a assinatura do pacote não confere — download descartado por segurança".to_string()
    } else {
        raw
    };
    anyhow::anyhow!(msg)
}

/// Consulta o servidor. `Ok(None)` = já está na última versão. O resultado
/// fica no estado para o Início mostrar o banner.
pub(crate) async fn check(app: &AppHandle) -> anyhow::Result<Option<UpdateInfo>> {
    let updater = build_updater(app)?;
    let found = updater.check().await.map_err(friendly)?;
    let info = found.as_ref().map(describe);
    *app.state::<AppState>().update_available.lock().unwrap() = info.clone();
    Ok(info)
}

/// Baixa, verifica a assinatura e instala. No Windows o plugin encerra o
/// ISPer e o instalador (modo passivo) reabre o app na versão nova.
pub(crate) async fn install(app: &AppHandle) -> anyhow::Result<String> {
    if app.state::<AppState>().meeting.lock().unwrap().is_some() {
        anyhow::bail!("encerre a reunião antes de atualizar");
    }
    let updater = build_updater(app)?;
    let Some(update) = updater.check().await.map_err(friendly)? else {
        anyhow::bail!("você já está na última versão");
    };
    let version = update.version.clone();

    let progress_app = app.clone();
    let mut downloaded: u64 = 0;
    let mut last_emit: Option<Instant> = None;
    let on_chunk = move |chunk: usize, total: Option<u64>| {
        downloaded += chunk as u64;
        let due = last_emit.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY);
        if due {
            last_emit = Some(Instant::now());
            let _ = progress_app.emit(
                "isper-update-progress",
                json!({ "downloaded": downloaded, "total": total }),
            );
        }
    };
    let finish_app = app.clone();
    let on_finish = move || {
        let _ = finish_app.emit("isper-update-progress", json!({ "done": true }));
    };

    if std::env::var(DRY_RUN_ENV).is_ok() {
        let bytes = update
            .download(on_chunk, on_finish)
            .await
            .map_err(friendly)?;
        tracing::info!(
            bytes = bytes.len(),
            %version,
            "atualização baixada e assinatura verificada (dry run: nada instalado)"
        );
        return Ok(format!(
            "dry-run: {} bytes baixados e assinatura verificada",
            bytes.len()
        ));
    }
    tracing::info!(%version, "instalando atualização");
    update
        .download_and_install(on_chunk, on_finish)
        .await
        .map_err(friendly)?;
    Ok(version)
}

/// Avisa que há versão nova: o Início relê o estado (banner) e, uma vez por
/// versão, um toast silencioso — o app vive na bandeja, o Início nem sempre
/// está aberto.
fn announce(app: &AppHandle, info: &UpdateInfo) {
    notify_status(app);
    let _ = app.emit("isper-update", info.clone());
    {
        let mut announced = ANNOUNCED.lock().unwrap();
        if announced.as_deref() == Some(info.version.as_str()) {
            return;
        }
        *announced = Some(info.version.clone());
    }
    let line1 = format!("ISPer {} está pronto para instalar", info.version);
    let app2 = app.clone();
    let _ = notify::show(
        notify::Toast {
            title: "Atualização disponível",
            line1: &line1,
            line2: Some("Abra o Início para atualizar quando quiser."),
            silent: true,
        },
        move || open_home(&app2),
    );
}

/// Checagem de fundo: a primeira depois de o app assentar, depois uma vez por
/// dia — respeitando o interruptor das Configurações a cada volta. Só lê o
/// JSON; nunca baixa nem instala sozinha.
pub(crate) fn schedule_background_checks(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("isper-updater".into())
        .spawn(move || {
            let mut delay = FIRST_CHECK_DELAY;
            loop {
                std::thread::sleep(delay);
                delay = RECHECK_EVERY;
                let enabled = app
                    .state::<AppState>()
                    .config
                    .lock()
                    .unwrap()
                    .auto_update_check;
                if !enabled {
                    continue;
                }
                match tauri::async_runtime::block_on(check(&app)) {
                    Ok(Some(info)) => {
                        tracing::info!(version = %info.version, "atualização disponível");
                        announce(&app, &info);
                    }
                    Ok(None) => tracing::info!("ISPer está na última versão"),
                    Err(e) => tracing::info!("checagem de atualização não concluída: {e}"),
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("não consegui iniciar a checagem de atualizações: {e}");
    }
}

#[tauri::command]
pub(crate) async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let result = check(&app).await.map_err(|e| e.to_string())?;
    if let Some(info) = &result {
        announce(&app, info);
    }
    Ok(result)
}

#[tauri::command]
pub(crate) async fn install_update(app: AppHandle) -> Result<String, String> {
    install(&app).await.map_err(|e| e.to_string())
}
