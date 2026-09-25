//! O lado do celular: pareia com o PC e manda as gravações.
//!
//! ```no_run
//! # async fn exemplo() -> isper_sync::Result<()> {
//! use isper_sync::{SecretKey, client::Client, code::PairingCode};
//! let client = Client::bind(SecretKey::generate(), None, true).await?;
//! let code = PairingCode::parse("isper://parear?v=1&pc=…")?;
//! let pc = client.pair(&code, "Galaxy Tab A9", "0.3.0").await?;
//! let session = client.connect(&pc).await?;
//! # Ok(()) }
//! ```

use std::net::SocketAddr;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use iroh::endpoint::{Connection, QuicTransportConfig, presets};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayUrl, SecretKey};
use iroh_mdns_address_lookup::MdnsAddressLookup;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::code::{PairingCode, dial_addr};
use crate::proto::{
    ErrorCode, ItemStatus, Minutes, RecordingOffer, RemoteState, Request, Response, read_frame,
    write_frame,
};
use crate::server::relay_mode;
use crate::util::hex;
use crate::{ALPN, Result, SyncError};

/// Quanto o celular espera o PC atender.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);
/// Quanto espera uma resposta (o PC pode estar perguntando "Permitir?").
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);
/// Quanto espera o motivo de um envio que o PC parou de ler: ele o manda
/// antes de parar, então já está a caminho.
const REFUSAL_TIMEOUT: Duration = Duration::from_secs(10);
const CHUNK: usize = 256 * 1024;
/// Nome do serviço mDNS: só PCs do ISPer aparecem.
pub(crate) const MDNS_SERVICE: &str = "isper-sync";

/// Liga a busca (e, no PC, o anúncio) por mDNS na rede local.
pub(crate) fn add_mdns(endpoint: &Endpoint, advertise: bool) -> Result<()> {
    let mdns = MdnsAddressLookup::builder()
        .service_name(MDNS_SERVICE)
        .advertise(advertise)
        .build(endpoint.id())
        .map_err(|e| SyncError::Connect(e.to_string()))?;
    endpoint
        .address_lookup()
        .map_err(|e| SyncError::Connect(e.to_string()))?
        .add(mdns);
    Ok(())
}

/// O PC com que o celular está pareado, como o celular o guarda.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PairedPc {
    /// A chave pública do PC (hexadecimal).
    pub id: String,
    pub name: String,
    /// Os últimos endereços diretos conhecidos (`ip:porta`).
    pub addrs: Vec<String>,
    pub relay: Option<String>,
}

impl PairedPc {
    pub fn endpoint_id(&self) -> Result<EndpointId> {
        EndpointId::from_str(&self.id)
            .map_err(|_| SyncError::BadCode("chave do PC inválida".into()))
    }

    pub fn relay_url(&self) -> Option<RelayUrl> {
        self.relay
            .as_deref()
            .and_then(|r| RelayUrl::from_str(r).ok())
    }

    fn endpoint_addr(&self) -> Result<EndpointAddr> {
        let addrs: Vec<SocketAddr> = self
            .addrs
            .iter()
            .filter_map(|a| SocketAddr::from_str(a).ok())
            .collect();
        Ok(dial_addr(
            self.endpoint_id()?,
            &addrs,
            self.relay_url().as_ref(),
        ))
    }
}

/// O que o PC diz de si num [`Session::hello`].
#[derive(Debug, Clone, PartialEq)]
pub struct Welcome {
    pub pc_name: String,
    pub addrs: Vec<String>,
    pub relay: Option<String>,
}

/// O endpoint do celular. Um por processo basta; a chave é a identidade do
/// aparelho perante o PC.
pub struct Client {
    endpoint: Endpoint,
}

impl Client {
    /// Abre o endpoint. `relay`: o do PC, se ele usa um; `None` fica só na
    /// rede local. `mdns`: procura o PC na rede local se o IP dele mudou.
    pub async fn bind(secret_key: SecretKey, relay: Option<RelayUrl>, mdns: bool) -> Result<Self> {
        // Sem GSO (vários pacotes UDP num envio só): há placas e drivers que o
        // recusam com EIO no meio do envio, e o QUIC não se recuperava — visto
        // no emulador Android, com a conexão parada até cair. O celular manda
        // pouco (~30 MB por reunião), e sem GSO nenhum driver quebra o envio.
        let transport = QuicTransportConfig::builder()
            .enable_segmentation_offload(false)
            .build();
        let endpoint = Endpoint::builder(presets::Minimal)
            .secret_key(secret_key)
            .relay_mode(relay_mode(relay.as_ref()))
            .transport_config(transport)
            .bind()
            .await
            .map_err(|e| SyncError::Connect(e.to_string()))?;
        if mdns && let Err(e) = add_mdns(&endpoint, false) {
            tracing::warn!("sem mDNS: {e}");
        }
        Ok(Self { endpoint })
    }

    /// A chave pública do celular.
    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// Pareia com o PC do código. Espera alguém tocar em "Permitir" no PC.
    pub async fn pair(
        &self,
        code: &PairingCode,
        device_name: &str,
        app_version: &str,
    ) -> Result<PairedPc> {
        let conn = self.dial(code.endpoint_addr()).await?;
        let resp = request(
            &conn,
            &Request::Pair {
                secret: hex(&code.secret),
                device_name: device_name.to_string(),
                app_version: app_version.to_string(),
            },
        )
        .await;
        conn.close(0u32.into(), b"ok");
        match resp? {
            Response::Paired { pc_name } => Ok(PairedPc {
                id: code.pc.to_string(),
                name: pc_name,
                addrs: code.addrs.iter().map(ToString::to_string).collect(),
                relay: code.relay.as_ref().map(ToString::to_string),
            }),
            Response::Denied { reason } => Err(SyncError::Denied(reason)),
            other => Err(unexpected(other)),
        }
    }

    /// Conecta ao PC pareado.
    pub async fn connect(&self, pc: &PairedPc) -> Result<Session> {
        let conn = self.dial(pc.endpoint_addr()?).await?;
        Ok(Session { conn })
    }

    /// A rede mudou (Wi-Fi ↔ dados): o endpoint refaz as contas.
    pub async fn network_change(&self) {
        self.endpoint.network_change().await;
    }

    pub async fn close(self) {
        self.endpoint.close().await;
    }

    async fn dial(&self, addr: EndpointAddr) -> Result<Connection> {
        tokio::time::timeout(CONNECT_TIMEOUT, self.endpoint.connect(addr, ALPN))
            .await
            .map_err(|_| {
                SyncError::Connect(
                    "o PC não respondeu (está ligado, na mesma rede, com o ISPer aberto?)".into(),
                )
            })?
            .map_err(|e| SyncError::Connect(e.to_string()))
    }
}

/// Uma conexão aberta com o PC; os pedidos podem ir em sequência.
pub struct Session {
    conn: Connection,
}

impl Session {
    pub async fn hello(&self, device_name: &str, app_version: &str) -> Result<Welcome> {
        match request(
            &self.conn,
            &Request::Hello {
                device_name: device_name.to_string(),
                app_version: app_version.to_string(),
            },
        )
        .await?
        {
            Response::Welcome {
                pc_name,
                addrs,
                relay,
            } => Ok(Welcome {
                pc_name,
                addrs,
                relay,
            }),
            other => Err(unexpected(other)),
        }
    }

    /// Anuncia a gravação; devolve quanto o PC já tem e em que pé ela está.
    pub async fn offer(&self, offer: &RecordingOffer) -> Result<(u64, RemoteState)> {
        match request(&self.conn, &Request::Offer(offer.clone())).await? {
            Response::Offered { have, state } => Ok((have, state)),
            other => Err(unexpected(other)),
        }
    }

    /// Manda o arquivo a partir de `offset`. `progress(enviados, total)`;
    /// com `cancel`, fecha o envio no ponto em que está (o PC guarda o que
    /// chegou) e devolve [`SyncError::Cancelled`].
    pub async fn upload(
        &self,
        path: &Path,
        offer: &RecordingOffer,
        offset: u64,
        progress: &(dyn Fn(u64, u64) + Sync),
        cancel: &AtomicBool,
    ) -> Result<RemoteState> {
        let (mut send, mut recv) = self
            .conn
            .open_bi()
            .await
            .map_err(|e| SyncError::Connection(e.to_string()))?;
        write_frame(
            &mut send,
            &Request::Upload {
                id: offer.id.clone(),
                offset,
            },
        )
        .await?;
        let mut file = tokio::fs::File::open(path).await?;
        file.seek(std::io::SeekFrom::Start(offset)).await?;
        let mut sent = offset;
        let mut buf = vec![0u8; CHUNK];
        let mut cancelled = false;
        // O PC pode recusar o envio no meio (o offset não bate, o pareamento
        // foi desfeito): ele responde com o motivo e para de ler, e a escrita
        // daqui falha com "stopped by peer". O motivo chega pela outra metade
        // do stream, e é ele que o celular tem de mostrar.
        let mut refused: Option<String> = None;
        while sent < offer.size {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            let want = usize::try_from(offer.size - sent)
                .unwrap_or(CHUNK)
                .min(CHUNK);
            let n = file.read(&mut buf[..want]).await?;
            if n == 0 {
                return Err(SyncError::Protocol(format!(
                    "o arquivo encolheu: {} de {} bytes",
                    sent, offer.size
                )));
            }
            if let Err(e) = send.write_all(&buf[..n]).await {
                refused = Some(e.to_string());
                break;
            }
            sent += n as u64;
            progress(sent, offer.size);
        }
        // Num stream que o PC parou de ler, o finish também falha; quem diz
        // o que houve é a resposta, lida abaixo.
        if refused.is_none()
            && let Err(e) = send.finish()
        {
            refused = Some(e.to_string());
        }
        let wait = if refused.is_some() {
            REFUSAL_TIMEOUT
        } else {
            RESPONSE_TIMEOUT
        };
        let resp: Response = match tokio::time::timeout(wait, read_frame(&mut recv)).await {
            Ok(Ok(resp)) => resp,
            // Sem resposta, vale o erro da escrita, se ela falhou.
            Ok(Err(e)) => return Err(refused.map_or(e, SyncError::Connection)),
            Err(_) => {
                return Err(SyncError::Connection(
                    refused.unwrap_or_else(|| "o PC não respondeu ao envio".into()),
                ));
            }
        };
        if cancelled {
            return Err(SyncError::Cancelled);
        }
        match resp {
            Response::Uploaded { state } => Ok(state),
            other => Err(unexpected(other)),
        }
    }

    pub async fn status(&self, ids: Vec<String>) -> Result<Vec<ItemStatus>> {
        match request(&self.conn, &Request::Status { ids }).await? {
            Response::Status { items } => Ok(items),
            other => Err(unexpected(other)),
        }
    }

    pub async fn minutes(&self, id: &str) -> Result<Option<Minutes>> {
        match request(&self.conn, &Request::Minutes { id: id.to_string() }).await? {
            Response::Minutes(m) => Ok(Some(m)),
            Response::NoMinutes => Ok(None),
            other => Err(unexpected(other)),
        }
    }

    /// Pede ao PC que esqueça este celular.
    pub async fn forget(&self) -> Result<()> {
        match request(&self.conn, &Request::Forget).await? {
            Response::Forgotten => Ok(()),
            other => Err(unexpected(other)),
        }
    }

    pub fn close(&self) {
        self.conn.close(0u32.into(), b"ok");
    }
}

async fn request(conn: &Connection, req: &Request) -> Result<Response> {
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| SyncError::Connection(e.to_string()))?;
    write_frame(&mut send, req).await?;
    send.finish()
        .map_err(|e| SyncError::Connection(e.to_string()))?;
    tokio::time::timeout(RESPONSE_TIMEOUT, read_frame(&mut recv))
        .await
        .map_err(|_| SyncError::Connection("o PC não respondeu".into()))?
}

fn unexpected(resp: Response) -> SyncError {
    match resp {
        Response::Error {
            code: ErrorCode::NotPaired,
            ..
        } => SyncError::NotPaired,
        Response::Error { message, .. } => SyncError::Remote(message),
        other => SyncError::Protocol(format!("{other:?}")),
    }
}
