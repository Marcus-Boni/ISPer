//! A sincronia com o PC, do lado do celular (Fase 9.3): parear pelo QR,
//! mandar as gravações, perguntar pelo resultado e buscar a ata
//! ([`isper_sync`], ADR 0017).
//!
//! O estado mora em arquivos, como o resto do gravador:
//!
//! - `<state_dir>/chave-sincronia.txt`: a chave do celular, que é a
//!   identidade dele perante o PC (a pasta é a interna do app);
//! - `<state_dir>/pc.json`: o PC pareado;
//! - `<gravações>/<id>.sync.json`: em que pé cada gravação está no PC;
//! - `<gravações>/<id>.ata.md`: a ata que voltou.
//!
//! Os métodos bloqueiam (o app chama de uma thread de fundo, o WorkManager):
//! cada um roda no runtime do tokio que o [`PcLink`] carrega.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use isper_core::capture::{self, CaptureManifest, CaptureState};
use isper_sync::client::{Client, PairedPc, Session};
use isper_sync::code::PairingCode;
use isper_sync::proto::{RecordingOffer, RemoteState};
use isper_sync::{SecretKey, SyncError};
use serde::{Deserialize, Serialize};

use crate::MobileError;

const KEY_FILE: &str = "chave-sincronia.txt";
const PC_FILE: &str = "pc.json";

impl From<SyncError> for MobileError {
    fn from(e: SyncError) -> Self {
        match e {
            SyncError::NotPaired => Self::NotPaired,
            SyncError::Cancelled => Self::Cancelled,
            other => Self::Sync(other.to_string()),
        }
    }
}

/// Em que pé uma gravação está no PC, como a Biblioteca mostra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "snake_case")]
pub enum RemoteStage {
    /// Ainda não foi (ou o PC perdeu e ela vai de novo).
    #[default]
    NotSent,
    /// Foi em parte; o próximo envio continua de onde parou.
    Sending,
    /// Chegou inteira e espera a vez no PC.
    Queued,
    /// O PC está transcrevendo.
    Processing,
    /// A ata voltou.
    Ready,
    /// O PC não conseguiu (o motivo fica em `remote_error`).
    Failed,
}

/// `<id>.sync.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct SyncRecord {
    #[serde(default)]
    stage: RemoteStage,
    /// SHA-256 e tamanho do áudio quando foi calculado (não recalcula à toa).
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    /// O título que o PC deu à reunião.
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

fn record_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.sync.json"))
}

/// Onde fica a ata de uma gravação.
pub(crate) fn minutes_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.ata.md"))
}

fn load_record(dir: &Path, id: &str) -> SyncRecord {
    std::fs::read(record_path(dir, id))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Escreve de forma atômica: um arquivo pela metade nunca fica no lugar.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn save_record(dir: &Path, id: &str, rec: &SyncRecord) {
    let saved = serde_json::to_vec_pretty(rec)
        .map_err(std::io::Error::other)
        .and_then(|json| write_atomic(&record_path(dir, id), &json));
    if let Err(e) = saved {
        tracing::warn!(id, "não consegui anotar a sincronia da gravação: {e}");
    }
}

/// O que a Biblioteca mostra da sincronia de uma gravação.
pub(crate) struct RemoteView {
    pub(crate) stage: RemoteStage,
    pub(crate) title: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) minutes: Option<String>,
}

pub(crate) fn remote_view(dir: &Path, id: &str) -> RemoteView {
    let rec = load_record(dir, id);
    let minutes = minutes_path(dir, id);
    RemoteView {
        stage: rec.stage,
        title: rec.title,
        error: rec.error,
        minutes: minutes.is_file().then(|| minutes.display().to_string()),
    }
}

/// Apaga o que a sincronia guardou de uma gravação (ao apagar a gravação).
pub(crate) fn forget_recording(dir: &Path, id: &str) {
    let _ = std::fs::remove_file(record_path(dir, id));
    let _ = std::fs::remove_file(minutes_path(dir, id));
}

/// O PC pareado, como o app mostra.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PcInfo {
    /// A chave pública do PC (hexadecimal).
    pub id: String,
    /// O nome do PC ("MARCUS-PC").
    pub name: String,
    /// Os últimos endereços diretos conhecidos.
    pub addrs: Vec<String>,
    /// O relay, se o PC usa um.
    pub relay: Option<String>,
}

impl From<&PairedPc> for PcInfo {
    fn from(pc: &PairedPc) -> Self {
        Self {
            id: pc.id.clone(),
            name: pc.name.clone(),
            addrs: pc.addrs.clone(),
            relay: pc.relay.clone(),
        }
    }
}

/// Uma ata que chegou.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReadyMinutes {
    /// A gravação.
    pub recording_id: String,
    /// O título da reunião no PC.
    pub title: String,
}

/// Como terminou uma rodada de sincronia.
#[derive(Debug, Clone, PartialEq, Default, uniffi::Record)]
pub struct SyncSummary {
    /// Gravações que chegaram inteiras ao PC nesta rodada.
    pub sent: u32,
    /// Atas que voltaram nesta rodada.
    pub ready: Vec<ReadyMinutes>,
    /// Gravações que esperam o PC (na fila ou transcrevendo).
    pub waiting: u32,
    /// Gravações que ainda não foram.
    pub unsent: u32,
    /// Por que a rodada parou antes do fim (o PC fora de alcance), se parou.
    pub error: Option<String>,
}

/// Quem acompanha o envio (a notificação do app).
#[uniffi::export(with_foreign)]
pub trait SyncListener: Send + Sync {
    /// Mandando `recording_id`: `sent` de `total` bytes.
    fn sending(&self, recording_id: String, sent: u64, total: u64);
}

/// A ligação do celular com o PC.
#[derive(uniffi::Object)]
pub struct PcLink {
    rt: tokio::runtime::Runtime,
    state_dir: PathBuf,
    key: SecretKey,
    cancel: AtomicBool,
}

fn load_or_create_key(path: &Path) -> Result<SecretKey, MobileError> {
    if let Ok(text) = std::fs::read_to_string(path) {
        return SecretKey::from_str(text.trim())
            .map_err(|e| MobileError::Sync(format!("a chave da sincronia está corrompida: {e}")));
    }
    let key = SecretKey::generate();
    let hex: String = key.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
    write_atomic(path, hex.as_bytes()).map_err(|e| MobileError::Sync(e.to_string()))?;
    Ok(key)
}

impl PcLink {
    fn pc_file(&self) -> PathBuf {
        self.state_dir.join(PC_FILE)
    }

    fn load_pc(&self) -> Option<PairedPc> {
        std::fs::read(self.pc_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
    }

    fn save_pc(&self, pc: &PairedPc) -> Result<(), MobileError> {
        let json = serde_json::to_vec_pretty(pc).map_err(|e| MobileError::Sync(e.to_string()))?;
        write_atomic(&self.pc_file(), &json).map_err(|e| MobileError::Sync(e.to_string()))
    }

    fn drop_pc(&self) {
        let _ = std::fs::remove_file(self.pc_file());
    }

    async fn round(
        &self,
        dir: &Path,
        device_name: &str,
        app_version: &str,
        active: Option<&str>,
        listener: &dyn SyncListener,
    ) -> Result<SyncSummary, MobileError> {
        let pc = self.load_pc().ok_or(MobileError::NotPaired)?;
        let mut summary = SyncSummary::default();

        // O que há para fazer: mandar, perguntar, buscar a ata.
        let mut to_send: Vec<(CaptureManifest, SyncRecord)> = Vec::new();
        let mut to_ask: Vec<String> = Vec::new();
        let mut to_fetch: Vec<String> = Vec::new();
        for m in capture::list(dir)? {
            if m.state == CaptureState::Recording || Some(m.id.as_str()) == active {
                continue;
            }
            let rec = load_record(dir, &m.id);
            match rec.stage {
                RemoteStage::NotSent | RemoteStage::Sending => to_send.push((m, rec)),
                RemoteStage::Queued | RemoteStage::Processing => to_ask.push(m.id),
                RemoteStage::Ready if !minutes_path(dir, &m.id).is_file() => to_fetch.push(m.id),
                _ => {}
            }
        }
        if to_send.is_empty() && to_ask.is_empty() && to_fetch.is_empty() {
            return Ok(summary);
        }

        let client = Client::bind(self.key.clone(), pc.relay_url(), true).await?;
        let session = match client.connect(&pc).await {
            Ok(s) => s,
            Err(e) => {
                summary.error = Some(e.to_string());
                summary.unsent = to_send.len() as u32;
                summary.waiting = to_ask.len() as u32;
                client.close().await;
                return Ok(summary);
            }
        };
        let result = self
            .with_session(
                &session,
                dir,
                &pc,
                device_name,
                app_version,
                listener,
                to_send,
                to_ask,
                to_fetch,
                &mut summary,
            )
            .await;
        session.close();
        client.close().await;
        match result {
            Err(MobileError::NotPaired) => {
                // O PC esqueceu este celular: o pareamento acabou dos dois lados.
                self.drop_pc();
                Err(MobileError::NotPaired)
            }
            Err(e) => Err(e),
            Ok(()) => Ok(summary),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn with_session(
        &self,
        session: &Session,
        dir: &Path,
        pc: &PairedPc,
        device_name: &str,
        app_version: &str,
        listener: &dyn SyncListener,
        to_send: Vec<(CaptureManifest, SyncRecord)>,
        mut to_ask: Vec<String>,
        mut to_fetch: Vec<String>,
        summary: &mut SyncSummary,
    ) -> Result<(), MobileError> {
        // O PC diz como está agora (os endereços mudam com o DHCP).
        let welcome = session.hello(device_name, app_version).await?;
        let updated = PairedPc {
            name: welcome.pc_name,
            addrs: if welcome.addrs.is_empty() {
                pc.addrs.clone()
            } else {
                welcome.addrs
            },
            relay: welcome.relay.or_else(|| pc.relay.clone()),
            ..pc.clone()
        };
        if &updated != pc {
            self.save_pc(&updated)?;
        }

        for (m, mut rec) in to_send {
            if self.cancel.load(Ordering::Relaxed) {
                summary.unsent += 1;
                continue;
            }
            let path = m.audio_path(dir);
            let offer = match offer_for(&m, &path, &mut rec) {
                Ok(o) => o,
                Err(e) => {
                    rec.stage = RemoteStage::Failed;
                    rec.error = Some(e.to_string());
                    save_record(dir, &m.id, &rec);
                    continue;
                }
            };
            let sent = async {
                let (have, state) = session.offer(&offer).await?;
                if have >= offer.size {
                    return Ok(state);
                }
                rec.stage = RemoteStage::Sending;
                save_record(dir, &m.id, &rec);
                let progress = |sent: u64, total: u64| listener.sending(m.id.clone(), sent, total);
                session
                    .upload(&path, &offer, have, &progress, &self.cancel)
                    .await
            }
            .await;
            match sent {
                Ok(state) => {
                    if apply(&mut rec, &state) {
                        to_fetch.push(m.id.clone());
                    }
                    if matches!(rec.stage, RemoteStage::Queued | RemoteStage::Processing) {
                        summary.sent += 1;
                    }
                }
                Err(SyncError::NotPaired) => return Err(MobileError::NotPaired),
                Err(SyncError::Cancelled) => {
                    rec.stage = RemoteStage::Sending;
                    summary.unsent += 1;
                }
                Err(e) => {
                    // Fica para a próxima rodada: o PC guardou o que chegou.
                    tracing::warn!(id = %m.id, "envio interrompido: {e}");
                    rec.stage = RemoteStage::Sending;
                    rec.error = Some(e.to_string());
                    summary.unsent += 1;
                    summary.error = Some(e.to_string());
                }
            }
            save_record(dir, &m.id, &rec);
        }

        if !to_ask.is_empty() {
            to_ask.retain(|id| !to_fetch.contains(id));
            for item in session.status(to_ask).await? {
                let mut rec = load_record(dir, &item.id);
                if apply(&mut rec, &item.state) {
                    to_fetch.push(item.id.clone());
                }
                save_record(dir, &item.id, &rec);
            }
        }

        for id in to_fetch {
            let mut rec = load_record(dir, &id);
            match session.minutes(&id).await? {
                Some(minutes) => {
                    write_atomic(&minutes_path(dir, &id), minutes.markdown.as_bytes())?;
                    rec.stage = RemoteStage::Ready;
                    rec.title = Some(minutes.title.clone());
                    rec.error = None;
                    summary.ready.push(ReadyMinutes {
                        recording_id: id.clone(),
                        title: minutes.title,
                    });
                }
                None => {
                    rec.stage = RemoteStage::Failed;
                    rec.error = Some("o PC não tem mais a ata desta gravação".into());
                }
            }
            save_record(dir, &id, &rec);
        }

        summary.waiting = capture::list(dir)?
            .iter()
            .filter(|m| {
                matches!(
                    load_record(dir, &m.id).stage,
                    RemoteStage::Queued | RemoteStage::Processing
                )
            })
            .count() as u32;
        Ok(())
    }
}

/// Aplica o que o PC disse; `true` quando a ata está pronta para buscar.
fn apply(rec: &mut SyncRecord, state: &RemoteState) -> bool {
    rec.error = None;
    match state {
        RemoteState::Unknown => rec.stage = RemoteStage::NotSent,
        RemoteState::Receiving { .. } => rec.stage = RemoteStage::Sending,
        RemoteState::Queued => rec.stage = RemoteStage::Queued,
        RemoteState::Processing => rec.stage = RemoteStage::Processing,
        RemoteState::Done { title } => {
            rec.stage = RemoteStage::Processing;
            rec.title = Some(title.clone());
            return true;
        }
        RemoteState::Failed { reason } => {
            rec.stage = RemoteStage::Failed;
            rec.error = Some(reason.clone());
        }
    }
    false
}

/// O anúncio da gravação: o que o manifesto diz, mais o tamanho e o hash.
fn offer_for(
    m: &CaptureManifest,
    path: &Path,
    rec: &mut SyncRecord,
) -> std::io::Result<RecordingOffer> {
    let size = std::fs::metadata(path)?.len();
    let sha256 = match (&rec.sha256, rec.size) {
        (Some(sha), Some(s)) if s == size => sha.clone(),
        _ => {
            let sha = isper_sync::sha256_file(path)?;
            rec.sha256 = Some(sha.clone());
            rec.size = Some(size);
            sha
        }
    };
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "opus".into());
    Ok(RecordingOffer {
        id: m.id.clone(),
        size,
        sha256,
        extension,
        started_at: m.started_at.clone(),
        duration_secs: m.duration_secs,
        moments: m.moments.clone(),
        source_name: m.source_name.clone(),
    })
}

#[uniffi::export]
impl PcLink {
    /// Abre (ou cria) a identidade do celular em `state_dir` — a pasta
    /// interna do app, que nenhum outro app lê.
    #[uniffi::constructor]
    pub fn new(state_dir: String) -> Result<Arc<Self>, MobileError> {
        let state_dir = PathBuf::from(state_dir);
        std::fs::create_dir_all(&state_dir).map_err(|e| MobileError::Sync(e.to_string()))?;
        let key = load_or_create_key(&state_dir.join(KEY_FILE))?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| MobileError::Sync(e.to_string()))?;
        Ok(Arc::new(Self {
            rt,
            state_dir,
            key,
            cancel: AtomicBool::new(false),
        }))
    }

    /// A chave pública deste celular (o PC a mostra na lista de aparelhos).
    pub fn device_id(&self) -> String {
        self.key.public().to_string()
    }

    /// O PC pareado, se houver.
    pub fn paired_pc(&self) -> Option<PcInfo> {
        self.load_pc().as_ref().map(PcInfo::from)
    }

    /// Lê o código (do QR ou colado) sem conectar e devolve o nome do PC,
    /// para o app perguntar "Parear com …?".
    pub fn read_code(&self, code: String) -> Result<String, MobileError> {
        Ok(PairingCode::parse(&code)?.pc_name)
    }

    /// Pareia com o PC do código. Bloqueia até alguém tocar em "Permitir"
    /// no PC (ou ele desistir).
    pub fn pair(
        &self,
        code: String,
        device_name: String,
        app_version: String,
    ) -> Result<PcInfo, MobileError> {
        let code = PairingCode::parse(&code)?;
        self.rt.block_on(async {
            let client = Client::bind(self.key.clone(), code.relay.clone(), true).await?;
            let result = client.pair(&code, &device_name, &app_version).await;
            client.close().await;
            let pc = result?;
            self.save_pc(&pc)?;
            Ok(PcInfo::from(&pc))
        })
    }

    /// Desfaz o pareamento: avisa o PC, se ele estiver ao alcance, e esquece
    /// o PC aqui de qualquer jeito.
    pub fn unpair(&self, device_name: String, app_version: String) {
        if let Some(pc) = self.load_pc() {
            self.rt.block_on(async {
                if let Ok(client) = Client::bind(self.key.clone(), pc.relay_url(), true).await {
                    if let Ok(session) = client.connect(&pc).await {
                        let _ = session.hello(&device_name, &app_version).await;
                        let _ = session.forget().await;
                        session.close();
                    }
                    client.close().await;
                }
            });
        }
        self.drop_pc();
    }

    /// Uma rodada: manda o que falta, pergunta pelo que está no PC e busca as
    /// atas prontas. `active_id` é a gravação em andamento (fica de fora).
    /// Sem PC ao alcance, devolve o resumo com `error` (o app tenta depois).
    pub fn sync(
        &self,
        recordings_dir: String,
        device_name: String,
        app_version: String,
        active_id: Option<String>,
        listener: Arc<dyn SyncListener>,
    ) -> Result<SyncSummary, MobileError> {
        self.cancel.store(false, Ordering::Relaxed);
        let dir = PathBuf::from(recordings_dir);
        self.rt.block_on(self.round(
            &dir,
            &device_name,
            &app_version,
            active_id.as_deref(),
            listener.as_ref(),
        ))
    }

    /// Para a rodada em andamento no ponto em que está (o que chegou ao PC fica).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// "Tentar de novo": a gravação que falhou volta a ser mandada.
#[uniffi::export]
pub fn retry_recording(recordings_dir: String, id: String) {
    let _ = std::fs::remove_file(record_path(Path::new(&recordings_dir), &id));
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use isper_sync::EndpointId;
    use isper_sync::proto::Minutes;
    use isper_sync::server::{Approval, Host, Server, ServerConfig};

    use super::*;
    use crate::recording::{Recorder, list_recordings};

    #[derive(Default)]
    struct TestHost {
        inbox: PathBuf,
        devices: Mutex<HashMap<EndpointId, String>>,
        done: Mutex<HashMap<String, String>>,
        received: Mutex<Vec<RecordingOffer>>,
    }

    impl Host for TestHost {
        fn pc_name(&self) -> String {
            "PC-DE-TESTE".into()
        }
        fn is_paired(&self, device: &EndpointId) -> bool {
            self.devices.lock().unwrap().contains_key(device)
        }
        fn approve(&self, _: EndpointId, _: String) -> Approval {
            Box::pin(async { true })
        }
        fn add_device(&self, device: EndpointId, name: &str) -> std::io::Result<()> {
            self.devices.lock().unwrap().insert(device, name.into());
            Ok(())
        }
        fn forget_device(&self, device: &EndpointId) {
            self.devices.lock().unwrap().remove(device);
        }
        fn inbox(&self, _: &EndpointId) -> PathBuf {
            self.inbox.clone()
        }
        fn received(&self, _: &EndpointId, offer: &RecordingOffer, _: &Path) -> RemoteState {
            self.received.lock().unwrap().push(offer.clone());
            RemoteState::Queued
        }
        fn state(&self, _: &EndpointId, id: &str) -> RemoteState {
            if let Some(t) = self.done.lock().unwrap().get(id) {
                return RemoteState::Done { title: t.clone() };
            }
            if self.received.lock().unwrap().iter().any(|o| o.id == id) {
                RemoteState::Queued
            } else {
                RemoteState::Unknown
            }
        }
        fn minutes(&self, _: &EndpointId, id: &str) -> Option<Minutes> {
            let title = self.done.lock().unwrap().get(id)?.clone();
            Some(Minutes {
                markdown: format!("# {title}\n"),
                title,
                started_at: String::new(),
            })
        }
    }

    struct Count(Mutex<u64>);
    impl SyncListener for Count {
        fn sending(&self, _: String, sent: u64, _: u64) {
            *self.0.lock().unwrap() = sent;
        }
    }

    fn record(dir: &str, id: &str) {
        let r = Recorder::start(
            dir.into(),
            id.into(),
            "2026-09-25T10:00:00-03:00".into(),
            16_000,
        )
        .unwrap();
        r.write(vec![1u8; 64_000]).unwrap();
        r.mark_moment().unwrap();
        r.finish().unwrap();
    }

    #[test]
    fn pareia_manda_e_traz_a_ata() {
        let tmp = std::env::temp_dir().join(format!("isper-mobile-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let rec_dir = tmp.join("Gravacoes");
        std::fs::create_dir_all(&rec_dir).unwrap();
        let rec_dir_s = rec_dir.display().to_string();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let host = Arc::new(TestHost {
            inbox: tmp.join("pc"),
            ..TestHost::default()
        });
        let server = rt
            .block_on(Server::start(
                ServerConfig {
                    secret_key: SecretKey::generate(),
                    port: 0,
                    relay: None,
                    mdns: false,
                },
                host.clone(),
            ))
            .unwrap();
        let mut code = server.start_pairing().unwrap();
        code.addrs = vec![([127, 0, 0, 1], server.port().unwrap()).into()];

        let link = PcLink::new(tmp.join("estado").display().to_string()).unwrap();
        assert!(link.paired_pc().is_none());
        assert_eq!(link.read_code(code.to_uri()).unwrap(), "PC-DE-TESTE");
        assert!(link.read_code("qualquer coisa".into()).is_err());
        let pc = link
            .pair(code.to_uri(), "Galaxy Tab A9".into(), "t".into())
            .unwrap();
        assert_eq!(pc.name, "PC-DE-TESTE");
        assert_eq!(link.paired_pc(), Some(pc));

        // Nada gravado: a rodada nem conecta.
        let quiet = link
            .sync(
                rec_dir_s.clone(),
                "Galaxy Tab A9".into(),
                "t".into(),
                None,
                Arc::new(Count(Mutex::new(0))),
            )
            .unwrap();
        assert_eq!(quiet, SyncSummary::default());

        record(&rec_dir_s, "20260925-100000");
        let progress = Count(Mutex::new(0));
        let s1 = link
            .sync(
                rec_dir_s.clone(),
                "Galaxy Tab A9".into(),
                "t".into(),
                None,
                Arc::new(progress),
            )
            .unwrap();
        assert_eq!((s1.sent, s1.waiting, s1.error.as_deref()), (1, 1, None));
        let offer = host.received.lock().unwrap()[0].clone();
        assert_eq!(offer.moments.len(), 1, "o momento marcado vai junto");
        let info = &list_recordings(rec_dir_s.clone(), None).unwrap().recordings[0];
        assert_eq!(info.remote, RemoteStage::Queued);

        // O PC terminou: a próxima rodada traz a ata.
        host.done
            .lock()
            .unwrap()
            .insert("20260925-100000".into(), "Visita à fábrica".into());
        let s2 = link
            .sync(
                rec_dir_s.clone(),
                "Galaxy Tab A9".into(),
                "t".into(),
                None,
                Arc::new(Count(Mutex::new(0))),
            )
            .unwrap();
        assert_eq!(s2.ready.len(), 1);
        assert_eq!(s2.ready[0].title, "Visita à fábrica");
        let info = &list_recordings(rec_dir_s.clone(), None).unwrap().recordings[0];
        assert_eq!(info.remote, RemoteStage::Ready);
        assert_eq!(info.remote_title.as_deref(), Some("Visita à fábrica"));
        let ata = std::fs::read_to_string(info.minutes_path.as_deref().unwrap()).unwrap();
        assert_eq!(ata, "# Visita à fábrica\n");

        // O PC esquece o celular: a próxima conversa desfaz o pareamento aqui.
        let device: EndpointId = link.device_id().parse().unwrap();
        host.forget_device(&device);
        record(&rec_dir_s, "20260925-110000");
        let e = link
            .sync(
                rec_dir_s.clone(),
                "Galaxy Tab A9".into(),
                "t".into(),
                None,
                Arc::new(Count(Mutex::new(0))),
            )
            .unwrap_err();
        assert!(matches!(e, MobileError::NotPaired), "{e}");
        assert!(link.paired_pc().is_none());

        // Apagar a gravação leva a sincronia e a ata junto.
        crate::recording::delete_recording(rec_dir_s.clone(), "20260925-100000".into()).unwrap();
        assert!(!minutes_path(&rec_dir, "20260925-100000").exists());
        assert!(!record_path(&rec_dir, "20260925-100000").exists());

        rt.block_on(server.shutdown());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
