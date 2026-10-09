//! Cliente MCP do ISPer ([ADR 0022]): um conector é uma URL, um token no
//! Credential Manager e o catálogo de ferramentas que o servidor publica.
//!
//! - [`client`]: abre a conexão pelo transporte HTTP "streamable" do `rmcp`
//!   (o SDK oficial), lista e chama ferramentas e lê o erro de ferramenta com
//!   código e dica;
//! - [`opttime`]: as ferramentas do OptTime que as rotinas usam, tipadas
//!   pelo `outputSchema` dele.
//!
//! É assíncrono (tokio) e não cria runtime: quem chama dá o seu (no app, o
//! do Tauri). Cada operação abre e fecha a própria conexão; o OptTime não
//! guarda sessão, e uma rotina consulta de poucos em poucos minutos.
//!
//! [ADR 0022]: ../../../docs/adr/0022-conectores-mcp-e-opttime.md

pub mod client;
pub mod opttime;

pub use client::{Connection, Endpoint, ToolError, ToolOutput, ToolSummary};
pub use opttime::{
    Agenda, AgendaEvent, Applied, ApplyItem, ApplyRequest, DaySummary, OptTime, Suggestion,
    Suggestions, Whoami, match_event,
};

/// O que pode dar errado ao falar com um servidor MCP.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    /// O conector ainda não tem token guardado.
    #[error("o conector não tem token; guarde um nas Configurações")]
    NoToken,
    /// URL que não serve (sem https fora do próprio PC, por exemplo).
    #[error("endereço do conector inválido: {0}")]
    BadUrl(String),
    /// O servidor não aceitou o token (HTTP 401).
    #[error("o servidor recusou o token; gere outro e guarde de novo")]
    Unauthorized,
    /// O token não tem a permissão que a ferramenta pede (HTTP 403).
    #[error("o token não tem permissão para isso: {0}")]
    Forbidden(String),
    /// O servidor não respondeu a tempo.
    #[error("o servidor não respondeu em {0} s")]
    Timeout(u64),
    /// Rede, TLS ou HTTP.
    #[error("não consegui falar com o servidor: {0}")]
    Transport(String),
    /// A ferramenta rodou e devolveu erro, com código e dica.
    #[error("{0}")]
    Tool(ToolError),
    /// Resposta fora do que o servidor publica (falta o `structuredContent`,
    /// por exemplo).
    #[error("resposta inesperada do servidor: {0}")]
    Protocol(String),
}

impl McpError {
    /// Código curto para a interface e para o diário (`MICROSOFT_NOT_CONNECTED`
    /// quando é erro de ferramenta; o nome do tipo nos outros casos).
    pub fn code(&self) -> String {
        match self {
            Self::NoToken => "NO_TOKEN".into(),
            Self::BadUrl(_) => "BAD_URL".into(),
            Self::Unauthorized => "UNAUTHORIZED".into(),
            Self::Forbidden(_) => "INSUFFICIENT_SCOPE".into(),
            Self::Timeout(_) => "TIMEOUT".into(),
            Self::Transport(_) => "NETWORK".into(),
            Self::Tool(e) => e.code.clone(),
            Self::Protocol(_) => "PROTOCOL".into(),
        }
    }

    /// A dica de correção, quando o servidor mandou uma.
    pub fn hint(&self) -> Option<&str> {
        match self {
            Self::Tool(e) => e.hint.as_deref(),
            _ => None,
        }
    }
}

/// Resultado do cliente MCP.
pub type Result<T> = std::result::Result<T, McpError>;
