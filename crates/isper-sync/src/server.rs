//! O lado do PC: aceita pareamentos e recebe gravações.
//!
//! Quem usa o servidor (o app desktop, o `isper-cli receber`) implementa
//! [`Host`]: onde ficam os aparelhos pareados, a quem perguntar "Permitir?",
//! o que fazer com uma gravação que chegou e em que pé ela está.
//!
//! O servidor cuida do resto:
//!
//! - um aparelho desconhecido só pode pedir [`Request::Pair`], e só com o
//!   segredo do pareamento aberto (uso único, [`crate::PAIRING_TTL`],
//!   [`crate::PAIRING_MAX_ATTEMPTS`] tentativas);
//! - a gravação chega num `.part` na pasta do aparelho: se a conexão cair, o
//!   próximo [`Request::Offer`] diz quanto já chegou, e o envio continua dali;
//! - com o último byte, o SHA-256 é conferido; só então o arquivo ganha o
//!   nome final e vai para o [`Host::received`].

use std::future::Future;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use iroh::endpoint::{RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointId, RelayMode, RelayUrl, SecretKey};
use tokio::io::AsyncWriteExt;

use crate::code::{PairingCode, preferred_addrs};
use crate::proto::{
    ErrorCode, ItemStatus, Minutes, RecordingOffer, RemoteState, Request, Response, read_frame,
    write_frame,
};
use crate::util::{check_id, ct_eq, random_16, sha256_file, unhex};
use crate::{
    ALPN, APPROVAL_TIMEOUT, MAX_RECORDING_BYTES, PAIRING_MAX_ATTEMPTS, PAIRING_TTL, Result,
    SyncError,
};

/// Quanto o PC espera o quadro de um pedido depois de o stream abrir.
const HEADER_TIMEOUT: Duration = Duration::from_secs(30);
/// Pedaço lido do stream e escrito no `.part`.
const CHUNK: usize = 256 * 1024;

/// A resposta de "Permitir?", que chega quando alguém toca num botão.
pub type Approval = Pin<Box<dyn Future<Output = bool> + Send + 'static>>;

/// O que o servidor precisa de quem o hospeda.
pub trait Host: Send + Sync + 'static {
    /// O nome do PC, para o celular mostrar.
    fn pc_name(&self) -> String;
    /// O aparelho está pareado?
    fn is_paired(&self, device: &EndpointId) -> bool;
    /// O aparelho pareado apareceu (para "visto por último").
    fn seen(&self, _device: &EndpointId, _name: &str) {}
    /// Pergunta a quem está no PC se o aparelho pode entrar. O servidor
    /// desiste depois de [`crate::APPROVAL_TIMEOUT`].
    fn approve(&self, device: EndpointId, name: String) -> Approval;
    /// Guarda o aparelho aprovado.
    fn add_device(&self, device: EndpointId, name: &str) -> std::io::Result<()>;
    /// Esquece o aparelho (o celular pediu, ou alguém revogou no PC).
    fn forget_device(&self, device: &EndpointId);
    /// A pasta das gravações deste aparelho.
    fn inbox(&self, device: &EndpointId) -> PathBuf;
    /// Uma gravação chegou inteira e conferida em `path`: o host a põe para
    /// processar e diz em que pé ela ficou.
    fn received(&self, device: &EndpointId, offer: &RecordingOffer, path: &Path) -> RemoteState;
    /// O estado de uma gravação que o aparelho mandou. [`RemoteState::Unknown`]
    /// quando ela ainda não chegou inteira.
    fn state(&self, device: &EndpointId, id: &str) -> RemoteState;
    /// A ata de uma gravação processada.
    fn minutes(&self, device: &EndpointId, id: &str) -> Option<Minutes>;
}

/// Como o servidor se apresenta na rede.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// A identidade do PC: a mesma chave em toda abertura, senão os celulares
    /// pareados deixam de reconhecê-lo.
    pub secret_key: SecretKey,
    /// Porta UDP ([`crate::DEFAULT_PORT`]); `0` escolhe qualquer uma.
    pub port: u16,
    /// Relay para quando o celular está fora da rede local. `None`: só a
    /// rede local, sem falar com nenhum servidor de fora.
    pub relay: Option<RelayUrl>,
    /// Anuncia o PC por mDNS, para o celular achá-lo se o IP mudar.
    pub mdns: bool,
}

struct Pairing {
    secret: [u8; 16],
    expires: Instant,
    attempts: u32,
}

struct Ctx<H> {
    host: Arc<H>,
    endpoint: Endpoint,
    relay: Option<RelayUrl>,
    pairing: Mutex<Option<Pairing>>,
}

/// O servidor em funcionamento. Soltá-lo não o para: use [`Server::shutdown`].
pub struct Server<H: Host> {
    ctx: Arc<Ctx<H>>,
    accept: tokio::task::JoinHandle<()>,
}

impl<H: Host> Server<H> {
    /// Abre o endpoint e começa a aceitar conexões. Com a porta ocupada, usa
    /// outra (o QR leva a que valeu).
    pub async fn start(config: ServerConfig, host: Arc<H>) -> Result<Self> {
        let endpoint = match bind(&config, config.port).await {
            Ok(ep) => ep,
            Err(e) if config.port != 0 => {
                tracing::warn!(porta = config.port, "porta ocupada, usando outra: {e}");
                bind(&config, 0).await?
            }
            Err(e) => return Err(e),
        };
        if config.mdns
            && let Err(e) = crate::client::add_mdns(&endpoint, true)
        {
            tracing::warn!("sem mDNS (o celular precisa do endereço do QR): {e}");
        }
        let ctx = Arc::new(Ctx {
            host,
            endpoint,
            relay: config.relay,
            pairing: Mutex::new(None),
        });
        let accept = tokio::spawn(accept_loop(ctx.clone()));
        tracing::info!(pc = %ctx.endpoint.id().fmt_short(), socks = ?ctx.endpoint.bound_sockets(), "sincronia com o celular ligada");
        Ok(Self { ctx, accept })
    }

    /// A chave pública do PC.
    pub fn id(&self) -> EndpointId {
        self.ctx.endpoint.id()
    }

    /// Os endereços diretos que valem a pena anunciar.
    pub fn addrs(&self) -> Vec<SocketAddr> {
        current_addrs(&self.ctx.endpoint)
    }

    /// A porta em que o PC ouve (IPv4).
    pub fn port(&self) -> Option<u16> {
        self.ctx
            .endpoint
            .bound_sockets()
            .iter()
            .find(|s| s.is_ipv4())
            .map(SocketAddr::port)
    }

    /// Abre um pareamento: um segredo novo, que vale por [`crate::PAIRING_TTL`]
    /// e substitui o anterior.
    pub fn start_pairing(&self) -> Result<PairingCode> {
        self.start_pairing_for(PAIRING_TTL)
    }

    /// [`Self::start_pairing`] com outra validade (os testes usam uma curta).
    pub fn start_pairing_for(&self, ttl: Duration) -> Result<PairingCode> {
        let secret = random_16()?;
        *lock(&self.ctx.pairing) = Some(Pairing {
            secret,
            expires: Instant::now() + ttl,
            attempts: 0,
        });
        Ok(PairingCode {
            pc: self.id(),
            pc_name: self.ctx.host.pc_name(),
            addrs: self.addrs(),
            relay: self.ctx.relay.clone(),
            secret,
        })
    }

    /// Fecha o pareamento aberto (a janela do QR fechou).
    pub fn cancel_pairing(&self) {
        *lock(&self.ctx.pairing) = None;
    }

    /// Quanto falta para o pareamento aberto vencer.
    pub fn pairing_remaining(&self) -> Option<Duration> {
        lock(&self.ctx.pairing)
            .as_ref()
            .and_then(|p| p.expires.checked_duration_since(Instant::now()))
    }

    /// Para de aceitar e fecha as conexões.
    pub async fn shutdown(self) {
        self.accept.abort();
        self.ctx.endpoint.close().await;
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn relay_mode(relay: Option<&RelayUrl>) -> RelayMode {
    match relay {
        Some(url) => RelayMode::custom([url.clone()]),
        None => RelayMode::Disabled,
    }
}

async fn bind(config: &ServerConfig, port: u16) -> Result<Endpoint> {
    let builder = |v6: bool| -> Result<iroh::endpoint::Builder> {
        let b = Endpoint::builder(presets::Minimal)
            .secret_key(config.secret_key.clone())
            .alpns(vec![ALPN.to_vec()])
            .relay_mode(relay_mode(config.relay.as_ref()))
            .clear_ip_transports()
            .bind_addr(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))
            .map_err(|e| SyncError::Connect(e.to_string()))?;
        if v6 {
            b.bind_addr(SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)))
                .map_err(|e| SyncError::Connect(e.to_string()))
        } else {
            Ok(b)
        }
    };
    match builder(true)?.bind().await {
        Ok(ep) => Ok(ep),
        // Máquina sem IPv6: fica só no IPv4.
        Err(e) => {
            tracing::debug!("sem IPv6 na porta {port}: {e}");
            builder(false)?
                .bind()
                .await
                .map_err(|e| SyncError::Connect(e.to_string()))
        }
    }
}

fn current_addrs(endpoint: &Endpoint) -> Vec<SocketAddr> {
    preferred_addrs(endpoint.addr().ip_addrs().copied())
}

async fn accept_loop<H: Host>(ctx: Arc<Ctx<H>>) {
    while let Some(incoming) = ctx.endpoint.accept().await {
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let conn = match incoming.await {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!("conexão recusada: {e}");
                    return;
                }
            };
            let device = conn.remote_id();
            // Cada pedido num stream próprio, e eles podem se sobrepor.
            while let Ok((send, recv)) = conn.accept_bi().await {
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve(&ctx, device, send, recv).await {
                        tracing::debug!(aparelho = %device.fmt_short(), "pedido interrompido: {e}");
                    }
                });
            }
        });
    }
}

fn error(code: ErrorCode, message: impl Into<String>) -> Response {
    Response::Error {
        code,
        message: message.into(),
    }
}

async fn serve<H: Host>(
    ctx: &Ctx<H>,
    device: EndpointId,
    mut send: SendStream,
    mut recv: RecvStream,
) -> Result<()> {
    let req: Request = tokio::time::timeout(HEADER_TIMEOUT, read_frame(&mut recv))
        .await
        .map_err(|_| SyncError::Protocol("o pedido não chegou a tempo".into()))??;
    let paired = ctx.host.is_paired(&device);
    let resp = match req {
        Request::Pair {
            secret,
            device_name,
            ..
        } => pair(ctx, device, &secret, &device_name).await,
        _ if !paired => error(
            ErrorCode::NotPaired,
            "este aparelho não está pareado com o PC",
        ),
        Request::Hello { device_name, .. } => {
            ctx.host.seen(&device, &device_name);
            Response::Welcome {
                pc_name: ctx.host.pc_name(),
                addrs: current_addrs(&ctx.endpoint)
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                relay: ctx.relay.as_ref().map(ToString::to_string),
            }
        }
        Request::Offer(offer) => self::offer(ctx, device, offer).await,
        Request::Upload { id, offset } => upload(ctx, device, &id, offset, &mut recv).await,
        Request::Status { ids } => {
            let mut items = Vec::new();
            for id in ids.into_iter().filter(|id| check_id(id)).take(200) {
                let state = match ctx.host.state(&device, &id) {
                    RemoteState::Unknown => part_state(ctx, &device, &id).await,
                    s => s,
                };
                items.push(ItemStatus { id, state });
            }
            Response::Status { items }
        }
        Request::Minutes { id } => match ctx.host.minutes(&device, &id) {
            Some(m) => Response::Minutes(m),
            None => Response::NoMinutes,
        },
        Request::Forget => {
            ctx.host.forget_device(&device);
            Response::Forgotten
        }
    };
    write_frame(&mut send, &resp).await?;
    send.finish()
        .map_err(|e| SyncError::Connection(e.to_string()))?;
    Ok(())
}

async fn pair<H: Host>(ctx: &Ctx<H>, device: EndpointId, secret: &str, name: &str) -> Response {
    let given = unhex(secret).unwrap_or_default();
    let ok = {
        let mut slot = lock(&ctx.pairing);
        match slot.as_mut() {
            None => {
                return error(
                    ErrorCode::PairingClosed,
                    "nenhum pareamento aberto no PC: abra \"Parear um celular\" de novo",
                );
            }
            Some(p) if p.expires <= Instant::now() => {
                *slot = None;
                return error(
                    ErrorCode::PairingClosed,
                    "o código venceu: gere outro no PC",
                );
            }
            Some(p) => {
                if ct_eq(&given, &p.secret) {
                    *slot = None; // uso único
                    true
                } else {
                    p.attempts += 1;
                    if p.attempts >= PAIRING_MAX_ATTEMPTS {
                        *slot = None;
                    }
                    false
                }
            }
        }
    };
    if !ok {
        tracing::warn!(aparelho = %device.fmt_short(), "pareamento com segredo errado");
        return Response::Denied {
            reason: "código errado".into(),
        };
    }
    let name = clean_name(name);
    let approved = tokio::time::timeout(APPROVAL_TIMEOUT, ctx.host.approve(device, name.clone()))
        .await
        .unwrap_or(false);
    if !approved {
        return Response::Denied {
            reason: "recusado no PC".into(),
        };
    }
    if let Err(e) = ctx.host.add_device(device, &name) {
        return error(
            ErrorCode::Internal,
            format!("não consegui guardar o aparelho: {e}"),
        );
    }
    tracing::info!(aparelho = %device.fmt_short(), nome = %name, "celular pareado");
    Response::Paired {
        pc_name: ctx.host.pc_name(),
    }
}

/// O nome que o celular mandou, apresentável: sem controles, até 60 letras.
fn clean_name(name: &str) -> String {
    let s: String = name.chars().filter(|c| !c.is_control()).take(60).collect();
    let s = s.trim();
    if s.is_empty() {
        "Celular".to_string()
    } else {
        s.to_string()
    }
}

fn valid_extension(ext: &str) -> bool {
    (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric())
}

fn paths(dir: &Path, id: &str) -> (PathBuf, PathBuf) {
    (
        dir.join(format!("{id}.part")),
        dir.join(format!("{id}.offer.json")),
    )
}

async fn file_len(path: &Path) -> u64 {
    tokio::fs::metadata(path)
        .await
        .map(|m| m.len())
        .unwrap_or(0)
}

async fn part_state<H: Host>(ctx: &Ctx<H>, device: &EndpointId, id: &str) -> RemoteState {
    let (part, _) = paths(&ctx.host.inbox(device), id);
    match tokio::fs::metadata(&part).await {
        Ok(m) => RemoteState::Receiving { have: m.len() },
        Err(_) => RemoteState::Unknown,
    }
}

async fn offer<H: Host>(ctx: &Ctx<H>, device: EndpointId, offer: RecordingOffer) -> Response {
    if !check_id(&offer.id)
        || offer.sha256.len() != 64
        || unhex(&offer.sha256).is_none()
        || !valid_extension(&offer.extension)
    {
        return error(
            ErrorCode::BadRequest,
            "gravação com id, hash ou extensão inválidos",
        );
    }
    if offer.size == 0 || offer.size > MAX_RECORDING_BYTES {
        return error(
            ErrorCode::TooLarge,
            format!("tamanho recusado: {} bytes", offer.size),
        );
    }
    match ctx.host.state(&device, &offer.id) {
        RemoteState::Unknown | RemoteState::Receiving { .. } => {}
        // Já chegou: o celular não manda de novo.
        state => {
            return Response::Offered {
                have: offer.size,
                state,
            };
        }
    }
    let dir = ctx.host.inbox(&device);
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return error(ErrorCode::Internal, format!("pasta de recebidos: {e}"));
    }
    let (part, meta) = paths(&dir, &offer.id);
    // Outro conteúdo com o mesmo id (o celular regravou? improvável, mas
    // barato): a parcial antiga não serve.
    if let Ok(old) = tokio::fs::read(&meta).await {
        let same = serde_json::from_slice::<RecordingOffer>(&old)
            .is_ok_and(|o| o.sha256 == offer.sha256 && o.size == offer.size);
        if !same {
            let _ = tokio::fs::remove_file(&part).await;
        }
    }
    let json = match serde_json::to_vec(&offer) {
        Ok(j) => j,
        Err(e) => return error(ErrorCode::Internal, e.to_string()),
    };
    if let Err(e) = tokio::fs::write(&meta, json).await {
        return error(
            ErrorCode::Internal,
            format!("não consegui anotar a gravação: {e}"),
        );
    }
    let mut have = file_len(&part).await;
    if have > offer.size {
        let _ = tokio::fs::remove_file(&part).await;
        have = 0;
    }
    Response::Offered {
        have,
        state: RemoteState::Receiving { have },
    }
}

async fn upload<H: Host>(
    ctx: &Ctx<H>,
    device: EndpointId,
    id: &str,
    offset: u64,
    recv: &mut RecvStream,
) -> Response {
    if !check_id(id) {
        return error(ErrorCode::BadRequest, "id inválido");
    }
    let dir = ctx.host.inbox(&device);
    let (part, meta) = paths(&dir, id);
    let Some(offer) = tokio::fs::read(&meta)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice::<RecordingOffer>(&b).ok())
    else {
        return error(
            ErrorCode::BadRequest,
            "anuncie a gravação (Offer) antes de mandar",
        );
    };
    let have = file_len(&part).await;
    if offset != have {
        return error(ErrorCode::BadOffset, format!("o PC tem {have} bytes"));
    }

    let copied = match receive_into(&part, recv, offer.size - have).await {
        Ok(n) => n,
        Err(e) => {
            // O que chegou fica no .part, para o próximo envio continuar.
            return error(ErrorCode::Internal, format!("a transferência parou: {e}"));
        }
    };
    let have = have + copied;
    if have < offer.size {
        return Response::Uploaded {
            state: RemoteState::Receiving { have },
        };
    }

    let part_for_hash = part.clone();
    let sha = tokio::task::spawn_blocking(move || sha256_file(&part_for_hash)).await;
    let sha = match sha {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return error(ErrorCode::Internal, format!("não consegui conferir: {e}")),
        Err(e) => return error(ErrorCode::Internal, e.to_string()),
    };
    if sha != offer.sha256 {
        let _ = tokio::fs::remove_file(&part).await;
        let _ = tokio::fs::remove_file(&meta).await;
        tracing::warn!(aparelho = %device.fmt_short(), id, "gravação chegou corrompida (SHA-256 não bate)");
        return error(
            ErrorCode::HashMismatch,
            "o arquivo chegou diferente do enviado; mande de novo",
        );
    }
    let final_path = dir.join(format!("{id}.{}", offer.extension.to_ascii_lowercase()));
    if let Err(e) = tokio::fs::rename(&part, &final_path).await {
        return error(
            ErrorCode::Internal,
            format!("não consegui guardar a gravação: {e}"),
        );
    }
    let _ = tokio::fs::remove_file(&meta).await;
    tracing::info!(aparelho = %device.fmt_short(), id, bytes = offer.size, "gravação recebida");
    Response::Uploaded {
        state: ctx.host.received(&device, &offer, &final_path),
    }
}

/// Copia até `limit` bytes do stream para o fim de `part` e devolve quantos
/// chegaram. O que chegou vai para o disco mesmo se o stream quebrar.
async fn receive_into(part: &Path, recv: &mut RecvStream, limit: u64) -> Result<u64> {
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(part)
        .await?;
    let mut out = tokio::io::BufWriter::with_capacity(CHUNK, file);
    let mut buf = vec![0u8; CHUNK];
    let mut copied = 0u64;
    let result = async {
        while copied < limit {
            let want = usize::try_from(limit - copied).unwrap_or(CHUNK).min(CHUNK);
            let n = match recv.read(&mut buf[..want]).await {
                Ok(Some(n)) => n,
                Ok(None) => break, // o celular fechou o envio (acabou ou cancelou)
                Err(e) => return Err(std::io::Error::other(e.to_string())),
            };
            out.write_all(&buf[..n]).await?;
            copied += n as u64;
        }
        Ok::<(), std::io::Error>(())
    }
    .await;
    out.flush().await?;
    out.get_ref().sync_data().await?;
    result?;
    Ok(copied)
}
