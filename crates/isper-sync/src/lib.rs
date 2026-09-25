//! Sincronia do celular com o PC (Fase 9.3): o celular grava, o PC transcreve
//! e devolve a ata ([ADR 0017](../../../docs/adr/0017-sincronia-celular-pc.md)).
//!
//! ```text
//!  celular (cliente)                           PC (servidor)
//!  ─────────────────                           ─────────────
//!  lê o QR ─► Pair { segredo, nome } ────────► confere o segredo (uso único, 2 min)
//!                                              e pergunta "Permitir?" no PC
//!           ◄──────────────────── Paired ───── guarda a chave do celular
//!  Offer { id, tamanho, SHA-256 } ───────────► "já tenho N bytes"
//!  Upload { id, offset N } + bytes ──────────► .part → confere o SHA-256 → fila
//!  Status { ids } ───────────────────────────► na fila, processando, pronta
//!  Minutes { id } ───────────────────────────► a ata em Markdown
//! ```
//!
//! O transporte é o [iroh](https://www.iroh.computer): QUIC em que cada lado
//! é uma chave Ed25519, e a conexão só se estabelece com a chave esperada.
//! Depois do pareamento, a autorização é a própria chave do celular: não há
//! senha nem token para vazar. Sem relay configurado, nada sai da rede local.
//!
//! - [`code`]: o código de pareamento (o texto dentro do QR);
//! - [`proto`]: as mensagens e o enquadramento;
//! - [`server`]: o lado do PC, que recebe (o app desktop e o `isper-cli`);
//! - [`client`]: o lado do celular, que envia (o `isper-mobile` e o `isper-cli`).

use std::time::Duration;

pub mod client;
pub mod code;
pub mod proto;
pub mod server;
mod util;

pub use iroh::{EndpointId, RelayUrl, SecretKey};
pub use util::{check_id, sha256_file};

/// O protocolo, anunciado na negociação da conexão (ALPN). Uma versão nova
/// do protocolo ganha outro ALPN, e os dois convivem.
pub const ALPN: &[u8] = b"isper/sync/1";
/// Porta UDP fixa do PC, para a regra do firewall valer sempre. Se estiver
/// ocupada, o PC usa outra, e o QR leva a que valeu.
pub const DEFAULT_PORT: u16 = 47823;
/// Quanto tempo o código de pareamento vale.
pub const PAIRING_TTL: Duration = Duration::from_secs(120);
/// Quantos segredos errados encerram um pareamento aberto.
pub const PAIRING_MAX_ATTEMPTS: u32 = 5;
/// Quanto o PC espera alguém tocar em "Permitir".
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(90);
/// Maior gravação aceita (12 h a 32 kbit/s são ~180 MB).
pub const MAX_RECORDING_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// O que pode dar errado na sincronia.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("código de pareamento inválido: {0}")]
    BadCode(String),
    #[error("não consegui falar com o PC: {0}")]
    Connect(String),
    #[error("a conexão com o PC caiu: {0}")]
    Connection(String),
    #[error("este celular não está pareado com o PC")]
    NotPaired,
    #[error("o PC recusou o pareamento: {0}")]
    Denied(String),
    #[error("o PC respondeu com erro: {0}")]
    Remote(String),
    #[error("resposta inesperada do PC: {0}")]
    Protocol(String),
    #[error("cancelado")]
    Cancelled,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, SyncError>;
