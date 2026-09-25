//! Sincronia com o celular (Fase 9.3): o ISPer do PC recebe as gravações do
//! app Android, transcreve pela mesma fila da importação (9.0) e devolve a
//! ata ao celular. Decisões no ADR 0017.
//!
//! ```text
//!  celular ── iroh (QUIC; a chave do PC vai no QR) ──► isper_sync::Server
//!                                                         │ AppHost:
//!   Documentos\ISPer\Do celular\<aparelho>\<id>.opus ◄────┤ guarda o arquivo
//!   fila de importação (Origin::Phone)                ◄───┤ transcreve
//!   banco: sync_devices, sync_items                   ◄───┘ estado e ata
//! ```
//!
//! Desligada por padrão (`phone_sync` no config). Ligada, o PC abre a porta
//! UDP [`isper_sync::DEFAULT_PORT`] (na primeira vez, o Windows pergunta se o
//! ISPer pode usar a rede) e se anuncia por mDNS. Sem relay configurado,
//! nada sai da rede local.

use std::str::FromStr;

use isper_core::store::{SyncDevice, SyncItem};
use isper_sync::proto::{Minutes, RecordingOffer, RemoteState};
use isper_sync::server::{Approval, Host, Server, ServerConfig};
use isper_sync::{EndpointId, RelayUrl, SecretKey};

use crate::audio_import::{self, PhoneMeta};
use crate::prelude::*;

/// Evento para as janelas: algo mudou; a tela pede o estado de novo.
const EVENT: &str = "isper-phone";
/// A chave do PC (a identidade dele perante os celulares), no `%APPDATA%\ISPer`.
const KEY_FILE: &str = "sincronia-chave.txt";
/// `Documentos\ISPer\Do celular`: as gravações recebidas, por aparelho.
const PHONE_DIR: &str = "Do celular";

/// A sincronia, em `AppState`.
#[derive(Default)]
pub(crate) struct PhoneSync {
    server: tokio::sync::Mutex<Option<Server<AppHost>>>,
    view: Mutex<View>,
}

/// O que a tela de Configurações mostra.
#[derive(Default)]
struct View {
    running: bool,
    port: Option<u16>,
    addrs: Vec<String>,
    error: Option<String>,
    pairing: Option<Pairing>,
    pending: Option<Pending>,
}

struct Pairing {
    code: String,
    svg: String,
    expires: Instant,
}

/// Um celular esperando "Permitir?".
struct Pending {
    device: EndpointId,
    name: String,
    answer: tokio::sync::oneshot::Sender<bool>,
}

fn now_stamp() -> String {
    chrono::Local::now().format("%d/%m/%Y %H:%M").to_string()
}

fn emit(app: &AppHandle) {
    let _ = app.emit(EVENT, ());
}

fn key_path() -> anyhow::Result<PathBuf> {
    Ok(crate::paths::roaming_dir()
        .ok_or_else(|| anyhow::anyhow!("pasta de dados do usuário (AppData) indisponível"))?
        .join(KEY_FILE))
}

/// A chave do PC: criada na primeira vez que a sincronia liga e mantida
/// depois — trocá-la desfaz o pareamento de todos os celulares.
fn load_or_create_key() -> anyhow::Result<SecretKey> {
    let path = key_path()?;
    if let Ok(text) = std::fs::read_to_string(&path) {
        return SecretKey::from_str(text.trim()).map_err(|e| {
            anyhow::anyhow!(
                "a chave da sincronia está corrompida ({}): {e}",
                path.display()
            )
        });
    }
    let key = SecretKey::generate();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let hex: String = key.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(&path, hex)?;
    tracing::info!(pc = %key.public().fmt_short(), "chave da sincronia criada");
    Ok(key)
}

/// `Documentos\ISPer\Do celular` (nos e2e, a do perfil de teste).
pub(crate) fn phone_dir() -> anyhow::Result<PathBuf> {
    let dir = crate::paths::home_dir()
        .ok_or_else(|| anyhow::anyhow!("pasta do usuário indisponível"))?
        .join("Documents")
        .join("ISPer")
        .join(PHONE_DIR);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// O nome do aparelho, bom para nome de pasta no Windows.
fn folder_name(name: &str, device: &EndpointId) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(40)
        .collect();
    let clean = clean.trim().trim_end_matches('.');
    let clean = if clean.is_empty() { "Celular" } else { clean };
    format!("{clean} ({})", device.fmt_short())
}

/// `dd/mm/aaaa hh:mm` no fuso do PC, a partir do RFC 3339 do celular.
fn local_started_at(rfc3339: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(rfc3339).ok().map(|d| {
        d.with_timezone(&chrono::Local)
            .format("%d/%m/%Y %H:%M")
            .to_string()
    })
}

fn phone_meta(device: &str, device_name: &str, offer: &RecordingOffer) -> PhoneMeta {
    let file = offer
        .source_name
        .clone()
        .unwrap_or_else(|| format!("{}.{}", offer.id, offer.extension));
    PhoneMeta {
        device_id: device.to_string(),
        recording_id: offer.id.clone(),
        started_at: local_started_at(&offer.started_at),
        moments: offer.moments.iter().map(|m| *m as f32).collect(),
        source_name: format!("{device_name} · {file}"),
        original_name: offer.source_name.clone(),
    }
}

/// O manifesto da gravação, guardado ao lado do áudio: é o que permite pôr
/// na fila de novo, com a data e os momentos, se o app fechar antes.
fn sidecar(path: &Path, id: &str) -> PathBuf {
    path.with_file_name(format!("{id}.isper.json"))
}

// --------------------------------------------------------------------- host

pub(crate) struct AppHost {
    app: AppHandle,
}

impl AppHost {
    fn device_name(&self, device: &EndpointId) -> String {
        open_store()
            .ok()
            .and_then(|s| s.sync_device(&device.to_string()).ok().flatten())
            .map(|d| d.name)
            .unwrap_or_else(|| "Celular".to_string())
    }
}

impl Host for AppHost {
    fn pc_name(&self) -> String {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into())
    }

    fn is_paired(&self, device: &EndpointId) -> bool {
        open_store()
            .and_then(|s| Ok(s.sync_device(&device.to_string())?))
            .is_ok_and(|d| d.is_some())
    }

    fn seen(&self, device: &EndpointId, name: &str) {
        if let Ok(store) = open_store() {
            let _ = store.touch_sync_device(&device.to_string(), name, &now_stamp());
        }
    }

    fn approve(&self, device: EndpointId, name: String) -> Approval {
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let state = self.app.state::<AppState>();
            let mut view = state.phone.view.lock_or_recover();
            view.pairing = None; // o segredo já foi usado
            view.pending = Some(Pending {
                device,
                name: name.clone(),
                answer: tx,
            });
        }
        emit(&self.app);
        // A tela do QR costuma estar aberta; o aviso cobre quem saiu dela.
        let heading = crate::i18n::tr(&self.app, "notify.phone-pair");
        let line2 = crate::i18n::tr(&self.app, "notify.phone-pair-line2");
        let app = self.app.clone();
        let _ = notify::show(
            notify::Toast {
                title: &heading,
                line1: &name,
                line2: Some(&line2),
                silent: false,
            },
            move || {
                tauri::async_runtime::spawn(open_settings_window(app.clone()));
            },
        );
        let app = self.app.clone();
        Box::pin(async move {
            let allowed = rx.await.unwrap_or(false);
            app.state::<AppState>()
                .phone
                .view
                .lock_or_recover()
                .pending
                .take_if(|p| p.device == device);
            emit(&app);
            allowed
        })
    }

    fn add_device(&self, device: EndpointId, name: &str) -> std::io::Result<()> {
        let result = open_store()
            .and_then(|s| Ok(s.add_sync_device(&device.to_string(), name, &now_stamp())?))
            .map_err(|e| std::io::Error::other(e.to_string()));
        emit(&self.app);
        result
    }

    fn forget_device(&self, device: &EndpointId) {
        if let Ok(store) = open_store() {
            let _ = store.forget_sync_device(&device.to_string());
        }
        tracing::info!(aparelho = %device.fmt_short(), "celular esquecido");
        emit(&self.app);
    }

    fn inbox(&self, device: &EndpointId) -> PathBuf {
        let base = phone_dir().unwrap_or_else(|_| std::env::temp_dir().join("ISPer-celular"));
        base.join(folder_name(&self.device_name(device), device))
    }

    fn received(&self, device: &EndpointId, offer: &RecordingOffer, path: &Path) -> RemoteState {
        let device_id = device.to_string();
        if let Ok(json) = serde_json::to_vec_pretty(offer)
            && let Err(e) = std::fs::write(sidecar(path, &offer.id), json)
        {
            tracing::warn!("não consegui guardar o manifesto da gravação: {e}");
        }
        let item = SyncItem {
            device_id: device_id.clone(),
            recording_id: offer.id.clone(),
            sha256: offer.sha256.clone(),
            path: path.to_string_lossy().into_owned(),
            state: "queued".into(),
            meeting_id: None,
            error: None,
            received_at: now_stamp(),
        };
        if let Err(e) = open_store().and_then(|s| Ok(s.put_sync_item(&item)?)) {
            tracing::warn!("não consegui registrar a gravação recebida: {e}");
        }
        let meta = phone_meta(&device_id, &self.device_name(device), offer);
        audio_import::enqueue_phone(&self.app, path.to_path_buf(), meta);
        emit(&self.app);
        RemoteState::Queued
    }

    fn state(&self, device: &EndpointId, id: &str) -> RemoteState {
        let Ok(store) = open_store() else {
            return RemoteState::Unknown;
        };
        let Some(item) = store.sync_item(&device.to_string(), id).ok().flatten() else {
            return RemoteState::Unknown;
        };
        match item.state.as_str() {
            "done" => match item
                .meeting_id
                .and_then(|m| store.get_meeting(m).ok().flatten())
            {
                Some(detail) => RemoteState::Done {
                    title: detail.meeting.title,
                },
                None => RemoteState::Failed {
                    reason: crate::i18n::tr(&self.app, "phone.meeting-deleted"),
                },
            },
            "failed" => RemoteState::Failed {
                reason: item.error.unwrap_or_default(),
            },
            _ => match audio_import::queue_position(&self.app, Path::new(&item.path)) {
                Some(true) => RemoteState::Processing,
                _ => RemoteState::Queued,
            },
        }
    }

    fn minutes(&self, device: &EndpointId, id: &str) -> Option<Minutes> {
        let store = open_store().ok()?;
        let item = store.sync_item(&device.to_string(), id).ok().flatten()?;
        let detail = store.get_meeting(item.meeting_id?).ok().flatten()?;
        let markdown = std::fs::read_to_string(detail.meeting.md_path.as_deref()?).ok()?;
        Some(Minutes {
            title: detail.meeting.title,
            started_at: detail.meeting.started_at,
            markdown,
        })
    }
}

/// A fila de importação terminou uma gravação do celular.
pub(crate) fn import_finished(meta: &PhoneMeta, meeting_id: Option<i64>, error: Option<&str>) {
    let result = open_store().and_then(|s| {
        Ok(s.finish_sync_item(&meta.device_id, &meta.recording_id, meeting_id, error)?)
    });
    if let Err(e) = result {
        tracing::warn!("não consegui anotar o resultado da gravação do celular: {e}");
    }
}

// ----------------------------------------------------------- ligar/desligar

/// Na abertura: põe de volta na fila o que chegou e não virou reunião (o app
/// pode ter fechado no meio), e liga a sincronia se ela estiver ligada.
pub(crate) fn startup(app: &AppHandle) {
    requeue_received(app);
    apply(app);
}

fn requeue_received(app: &AppHandle) {
    let Ok(store) = open_store() else { return };
    let Ok(items) = store.queued_sync_items() else {
        return;
    };
    for item in items {
        let path = PathBuf::from(&item.path);
        let offer = std::fs::read(sidecar(&path, &item.recording_id))
            .ok()
            .and_then(|b| serde_json::from_slice::<RecordingOffer>(&b).ok());
        match offer {
            Some(offer) if path.is_file() => {
                let name = store
                    .sync_device(&item.device_id)
                    .ok()
                    .flatten()
                    .map_or_else(|| "Celular".to_string(), |d: SyncDevice| d.name);
                let meta = phone_meta(&item.device_id, &name, &offer);
                audio_import::enqueue_phone(app, path, meta);
            }
            _ => {
                let why = crate::i18n::tr(app, "phone.file-missing");
                let _ =
                    store.finish_sync_item(&item.device_id, &item.recording_id, None, Some(&why));
            }
        }
    }
}

/// Liga ou desliga o servidor conforme a configuração (e o reinicia quando
/// o relay muda).
pub(crate) fn apply(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let (enabled, relay) = {
            let cfg = state.config.lock_or_recover();
            (cfg.phone_sync, cfg.phone_relay.clone())
        };
        let mut slot = state.phone.server.lock().await;
        if let Some(old) = slot.take() {
            old.shutdown().await;
        }
        {
            let mut view = state.phone.view.lock_or_recover();
            *view = View::default();
        }
        if enabled {
            match start_server(&app, relay.as_deref()).await {
                Ok(server) => {
                    let mut view = state.phone.view.lock_or_recover();
                    view.running = true;
                    view.port = server.port();
                    view.addrs = server.addrs().iter().map(ToString::to_string).collect();
                    *slot = Some(server);
                }
                Err(e) => {
                    tracing::warn!("não consegui ligar a sincronia com o celular: {e}");
                    state.phone.view.lock_or_recover().error = Some(e.to_string());
                }
            }
        }
        drop(slot);
        emit(&app);
    });
}

async fn start_server(app: &AppHandle, relay: Option<&str>) -> anyhow::Result<Server<AppHost>> {
    let key = load_or_create_key()?;
    let relay = relay
        .map(RelayUrl::from_str)
        .transpose()
        .map_err(|e| anyhow::anyhow!("relay inválido: {e}"))?;
    Ok(Server::start(
        ServerConfig {
            secret_key: key,
            port: isper_sync::DEFAULT_PORT,
            relay,
            mdns: true,
        },
        Arc::new(AppHost { app: app.clone() }),
    )
    .await?)
}

fn qr_svg(text: &str) -> String {
    use qrcode::render::svg;
    match qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M) {
        Ok(code) => code
            .render::<svg::Color<'_>>()
            .min_dimensions(232, 232)
            .quiet_zone(true)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build(),
        Err(e) => {
            tracing::warn!("não consegui desenhar o QR: {e}");
            String::new()
        }
    }
}

// ------------------------------------------------------------------ comandos

#[derive(serde::Serialize)]
pub(crate) struct PhoneStatus {
    enabled: bool,
    running: bool,
    error: Option<String>,
    pc_name: String,
    port: Option<u16>,
    addrs: Vec<String>,
    relay: Option<String>,
    folder: String,
    devices: Vec<DeviceDto>,
    pairing: Option<PairingDto>,
    pending: Option<PendingDto>,
}

#[derive(serde::Serialize)]
struct DeviceDto {
    id: String,
    short: String,
    name: String,
    paired_at: String,
    last_seen: Option<String>,
}

#[derive(serde::Serialize)]
struct PairingDto {
    code: String,
    svg: String,
    remaining_secs: u64,
}

#[derive(serde::Serialize)]
struct PendingDto {
    name: String,
    short: String,
}

fn short_id(id: &str) -> String {
    id.chars().take(10).collect()
}

/// O estado da sincronia, para a tela de Configurações.
#[tauri::command]
pub(crate) fn phone_sync_status(app: AppHandle) -> PhoneStatus {
    let state = app.state::<AppState>();
    let (enabled, relay) = {
        let cfg = state.config.lock_or_recover();
        (cfg.phone_sync, cfg.phone_relay.clone())
    };
    let devices = open_store()
        .and_then(|s| Ok(s.sync_devices()?))
        .unwrap_or_default()
        .into_iter()
        .map(|d| DeviceDto {
            short: short_id(&d.id),
            id: d.id,
            name: d.name,
            paired_at: d.paired_at,
            last_seen: d.last_seen,
        })
        .collect();
    let mut view = state.phone.view.lock_or_recover();
    // O servidor desistiu de esperar (ou o celular foi embora).
    if view.pending.as_ref().is_some_and(|p| p.answer.is_closed()) {
        view.pending = None;
    }
    let now = Instant::now();
    if view.pairing.as_ref().is_some_and(|p| p.expires <= now) {
        view.pairing = None;
    }
    PhoneStatus {
        enabled,
        running: view.running,
        error: view.error.clone(),
        pc_name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into()),
        port: view.port,
        addrs: view.addrs.clone(),
        relay,
        folder: phone_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_default(),
        devices,
        pairing: view.pairing.as_ref().map(|p| PairingDto {
            code: p.code.clone(),
            svg: p.svg.clone(),
            remaining_secs: p.expires.saturating_duration_since(now).as_secs(),
        }),
        pending: view.pending.as_ref().map(|p| PendingDto {
            name: p.name.clone(),
            short: p.device.fmt_short().to_string(),
        }),
    }
}

fn save_config(app: &AppHandle, change: impl FnOnce(&mut config::AppConfig)) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut cfg = state.config.lock_or_recover().clone();
    change(&mut cfg);
    cfg.normalize();
    config::save(&cfg).map_err(|e| e.to_string())?;
    *state.config.lock_or_recover() = cfg;
    Ok(())
}

/// Liga ou desliga "Receber gravações do celular".
#[tauri::command]
pub(crate) fn phone_sync_set_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    save_config(&app, |c| c.phone_sync = enabled)?;
    apply(&app);
    Ok(())
}

/// Relay para quando o celular está fora da rede (vazio: só a rede local).
#[tauri::command]
pub(crate) fn phone_sync_set_relay(app: AppHandle, relay: Option<String>) -> Result<(), String> {
    let relay = relay
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());
    if let Some(r) = &relay {
        RelayUrl::from_str(r)
            .map_err(|e| crate::i18n::trv(&app, "phone.bad-relay", &[("error", e.to_string())]))?;
    }
    save_config(&app, |c| c.phone_relay = relay)?;
    apply(&app);
    Ok(())
}

/// Abre um pareamento: o QR que o celular lê (vale 2 minutos).
#[tauri::command]
pub(crate) async fn phone_sync_start_pairing(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let slot = state.phone.server.lock().await;
    let Some(server) = slot.as_ref() else {
        return Err(crate::i18n::tr(&app, "phone.not-running"));
    };
    let code = server.start_pairing().map_err(|e| e.to_string())?;
    if code.addrs.is_empty() && code.relay.is_none() {
        return Err(crate::i18n::tr(&app, "phone.no-network"));
    }
    let uri = code.to_uri();
    let svg = qr_svg(&uri);
    let mut view = state.phone.view.lock_or_recover();
    view.addrs = code.addrs.iter().map(ToString::to_string).collect();
    view.pairing = Some(Pairing {
        code: uri,
        svg,
        expires: Instant::now() + isper_sync::PAIRING_TTL,
    });
    drop(view);
    emit(&app);
    Ok(())
}

/// Fecha o pareamento aberto (a tela saiu do QR).
#[tauri::command]
pub(crate) async fn phone_sync_cancel_pairing(app: AppHandle) {
    let state = app.state::<AppState>();
    if let Some(server) = state.phone.server.lock().await.as_ref() {
        server.cancel_pairing();
    }
    state.phone.view.lock_or_recover().pairing = None;
    emit(&app);
}

/// A resposta de quem está no PC ao "Permitir?".
#[tauri::command]
pub(crate) fn phone_sync_answer(app: AppHandle, allow: bool) {
    let pending = app
        .state::<AppState>()
        .phone
        .view
        .lock_or_recover()
        .pending
        .take();
    if let Some(p) = pending {
        let _ = p.answer.send(allow);
    }
    emit(&app);
}

/// Esquece um celular: ele não manda mais nada (as reuniões que vieram dele ficam).
#[tauri::command]
pub(crate) fn phone_sync_forget(app: AppHandle, id: String) -> Result<(), String> {
    open_store()
        .and_then(|s| Ok(s.forget_sync_device(&id)?))
        .map_err(|e| e.to_string())?;
    tracing::info!(aparelho = %short_id(&id), "celular esquecido no PC");
    emit(&app);
    Ok(())
}

/// Abre `Documentos\ISPer\Do celular` no Explorador.
#[tauri::command]
pub(crate) fn open_phone_folder() -> Result<(), String> {
    let dir = phone_dir().map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasta_do_aparelho_e_segura_no_windows() {
        let id = SecretKey::from_bytes(&[3u8; 32]).public();
        let short = id.fmt_short().to_string();
        assert_eq!(
            folder_name("Galaxy Tab A9", &id),
            format!("Galaxy Tab A9 ({short})")
        );
        assert_eq!(folder_name("a/b:c*?", &id), format!("a_b_c__ ({short})"));
        assert_eq!(folder_name("  ...  ", &id), format!("Celular ({short})"));
    }

    #[test]
    fn data_do_celular_vira_a_do_pc() {
        let s = local_started_at("2026-09-24T20:12:24-03:00").unwrap();
        assert_eq!(s.len(), "24/09/2026 20:12".len());
        assert!(s.starts_with("2"), "{s}");
        assert!(local_started_at("ontem").is_none());
    }

    #[test]
    fn origem_mostra_o_aparelho_e_o_arquivo() {
        let offer = RecordingOffer {
            id: "20260924-201224".into(),
            size: 1,
            sha256: "00".repeat(32),
            extension: "opus".into(),
            started_at: "2026-09-24T20:12:24-03:00".into(),
            duration_secs: None,
            moments: vec![9.5, 30.25],
            source_name: None,
        };
        let m = phone_meta("ab", "Galaxy Tab A9", &offer);
        assert_eq!(m.source_name, "Galaxy Tab A9 · 20260924-201224.opus");
        assert_eq!(m.moments, vec![9.5, 30.25]);
        assert!(m.original_name.is_none());
        let shared = RecordingOffer {
            source_name: Some("Reunião com fornecedor.m4a".into()),
            extension: "m4a".into(),
            ..offer
        };
        let m = phone_meta("ab", "Galaxy Tab A9", &shared);
        assert_eq!(m.source_name, "Galaxy Tab A9 · Reunião com fornecedor.m4a");
        assert_eq!(
            m.original_name.as_deref(),
            Some("Reunião com fornecedor.m4a")
        );
    }
}
