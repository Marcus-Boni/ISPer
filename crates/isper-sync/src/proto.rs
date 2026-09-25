//! As mensagens do protocolo `isper/sync/1` e como elas viajam.
//!
//! Cada pedido abre um stream bidirecional próprio: o celular manda um
//! quadro (4 bytes com o tamanho, little-endian, e o JSON), e o PC responde
//! com outro. Num [`Request::Upload`], os bytes do áudio vêm logo depois do
//! quadro, até o celular fechar o lado dele do stream.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{Result, SyncError};

/// Maior quadro JSON aceito. A ata vai num quadro, então cabe folgada.
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

/// O que o celular pede.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Request {
    /// Pede para parear, com o segredo do QR. É o único pedido que um
    /// aparelho desconhecido pode fazer.
    Pair {
        secret: String,
        device_name: String,
        app_version: String,
    },
    /// "Estou aqui": o PC anota quando viu o aparelho e responde com os
    /// endereços atuais dele.
    Hello {
        device_name: String,
        app_version: String,
    },
    /// Anuncia uma gravação; o PC responde quanto já tem dela.
    Offer(RecordingOffer),
    /// Manda os bytes a partir de `offset` (o que o PC disse que tinha).
    Upload { id: String, offset: u64 },
    /// Pergunta como estão as gravações já mandadas.
    Status { ids: Vec<String> },
    /// Pede a ata de uma gravação processada.
    Minutes { id: String },
    /// O celular se despede: o PC esquece o aparelho.
    Forget,
}

/// Uma gravação que o celular quer mandar.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RecordingOffer {
    /// O id da gravação no celular (vira o nome do arquivo no PC).
    pub id: String,
    pub size: u64,
    /// SHA-256 do arquivo inteiro, em hexadecimal.
    pub sha256: String,
    /// Extensão do arquivo (`opus`, ou a do áudio compartilhado com o app).
    pub extension: String,
    /// Quando a gravação começou (RFC 3339, com o fuso do celular).
    pub started_at: String,
    pub duration_secs: Option<f64>,
    /// Momentos marcados, em segundos desde o início.
    #[serde(default)]
    pub moments: Vec<f64>,
    /// Nome original, quando o áudio veio de outro app pelo "Compartilhar".
    #[serde(default)]
    pub source_name: Option<String>,
}

/// O que o PC responde.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Response {
    Paired {
        pc_name: String,
    },
    Denied {
        reason: String,
    },
    Welcome {
        pc_name: String,
        /// Endereços diretos atuais do PC (`ip:porta`).
        addrs: Vec<String>,
        relay: Option<String>,
    },
    Offered {
        /// Bytes que o PC já tem desta gravação.
        have: u64,
        state: RemoteState,
    },
    Uploaded {
        state: RemoteState,
    },
    Status {
        items: Vec<ItemStatus>,
    },
    Minutes(Minutes),
    NoMinutes,
    Forgotten,
    Error {
        code: ErrorCode,
        message: String,
    },
}

/// Em que pé está uma gravação no PC.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "estado", rename_all = "snake_case")]
pub enum RemoteState {
    /// O PC não conhece esta gravação.
    Unknown,
    /// Chegando: o PC tem `have` bytes.
    Receiving { have: u64 },
    /// Chegou inteira e espera a vez na fila de importação.
    Queued,
    /// Sendo transcrita.
    Processing,
    /// Virou reunião; a ata pode ser pedida.
    Done { title: String },
    /// Não deu para processar (o motivo vai para o celular mostrar).
    Failed { reason: String },
}

impl RemoteState {
    /// Terminou (bem ou mal): o celular não precisa mais perguntar.
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Done { .. } | Self::Failed { .. })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ItemStatus {
    pub id: String,
    pub state: RemoteState,
}

/// A ata de uma gravação, como o PC a escreveu.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Minutes {
    pub title: String,
    /// `dd/mm/aaaa hh:mm`, como na Biblioteca do PC.
    pub started_at: String,
    pub markdown: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// O aparelho não está (mais) pareado.
    NotPaired,
    /// Nenhum pareamento aberto no PC, ou ele venceu.
    PairingClosed,
    BadRequest,
    /// O `offset` do upload não bate com o que o PC tem.
    BadOffset,
    TooLarge,
    /// O arquivo chegou, mas o SHA-256 não bate: o PC descarta e o celular
    /// manda de novo.
    HashMismatch,
    Internal,
}

/// Escreve um quadro: 4 bytes com o tamanho e o JSON.
pub async fn write_frame<W, T>(w: &mut W, msg: &T) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let body = serde_json::to_vec(msg).map_err(|e| SyncError::Protocol(e.to_string()))?;
    if body.len() > MAX_FRAME {
        return Err(SyncError::Protocol(format!(
            "mensagem grande demais ({} bytes)",
            body.len()
        )));
    }
    let len = u32::try_from(body.len()).map_err(|e| SyncError::Protocol(e.to_string()))?;
    w.write_all(&len.to_le_bytes()).await?;
    w.write_all(&body).await?;
    w.flush().await?;
    Ok(())
}

/// Lê um quadro; recusa um tamanho acima de [`MAX_FRAME`] antes de alocar.
pub async fn read_frame<R, T>(r: &mut R) -> Result<T>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(SyncError::Protocol(format!(
            "mensagem grande demais ({len} bytes)"
        )));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    serde_json::from_slice(&body).map_err(|e| SyncError::Protocol(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn quadros_vao_e_voltam() {
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        let req = Request::Offer(RecordingOffer {
            id: "20260924-201224".into(),
            size: 198_000,
            sha256: "ab".repeat(32),
            extension: "opus".into(),
            started_at: "2026-09-24T20:12:24-03:00".into(),
            duration_secs: Some(48.0),
            moments: vec![12.5],
            source_name: None,
        });
        write_frame(&mut a, &req).await.unwrap();
        let back: Request = read_frame(&mut b).await.unwrap();
        assert_eq!(back, req);

        let resp = Response::Uploaded {
            state: RemoteState::Done {
                title: "Visita à fábrica".into(),
            },
        };
        write_frame(&mut b, &resp).await.unwrap();
        let back: Response = read_frame(&mut a).await.unwrap();
        assert_eq!(back, resp);
        assert!(matches!(back, Response::Uploaded { state } if state.is_final()));
    }

    #[tokio::test]
    async fn quadro_gigante_e_recusado_sem_alocar() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        a.write_all(&(u32::MAX).to_le_bytes()).await.unwrap();
        let e = read_frame::<_, Request>(&mut b).await.unwrap_err();
        assert!(e.to_string().contains("grande demais"), "{e}");
    }

    #[test]
    fn o_json_e_legivel() {
        let s = serde_json::to_string(&Request::Status {
            ids: vec!["a".into()],
        })
        .unwrap();
        assert_eq!(s, r#"{"tipo":"status","ids":["a"]}"#);
        let s = serde_json::to_string(&RemoteState::Receiving { have: 3 }).unwrap();
        assert_eq!(s, r#"{"estado":"receiving","have":3}"#);
    }
}
