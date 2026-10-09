//! A conexão com um servidor MCP: `initialize`, `tools/list` e `tools/call`.
//!
//! O protocolo pedido é o 2025-06-18, o que o OptTime fala; um servidor mais
//! novo responde com a versão dele no `initialize` e o `rmcp` segue. O
//! transporte é o HTTP "streamable" sem sessão: cada mensagem é um POST, e o
//! GET de eventos do servidor pode ser recusado (405) sem problema.

use std::fmt;
use std::time::Duration;

use rmcp::ServiceExt;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation, JsonObject,
    ProtocolVersion,
};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{McpError, Result};

/// Quanto uma operação (abrir, chamar e fechar) espera por padrão.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Onde fica o servidor e com que token entrar.
#[derive(Clone)]
pub struct Endpoint {
    url: String,
    token: String,
    timeout: Duration,
}

/// O token nunca aparece em log nem em mensagem de erro.
impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("url", &self.url)
            .field("token", &"…")
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl Endpoint {
    /// Valida a URL e o token. Fora do próprio PC só vale `https`: o token
    /// não viaja em texto aberto.
    pub fn new(url: &str, token: &str) -> Result<Self> {
        let parsed = url::Url::parse(url.trim()).map_err(|e| McpError::BadUrl(e.to_string()))?;
        let loopback = matches!(
            parsed.host(),
            Some(url::Host::Domain("localhost"))
                | Some(url::Host::Ipv4(std::net::Ipv4Addr::LOCALHOST))
                | Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))
        );
        match parsed.scheme() {
            "https" => {}
            "http" if loopback => {}
            other => {
                return Err(McpError::BadUrl(format!(
                    "{other}:// não serve; use https:// (http só no próprio PC)"
                )));
            }
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(McpError::BadUrl(
                "usuário e senha não vão na URL; o token vai no cofre".into(),
            ));
        }
        let token = token.trim();
        if token.is_empty() {
            return Err(McpError::NoToken);
        }
        if token.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(McpError::BadUrl(
                "o token tem espaço ou caractere de controle; copie de novo".into(),
            ));
        }
        Ok(Self {
            url: parsed.to_string(),
            token: token.to_string(),
            timeout: DEFAULT_TIMEOUT,
        })
    }

    /// O mesmo endpoint com outro prazo por operação.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// A URL normalizada.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// O prazo de cada operação.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Abre a conexão (o `initialize` do protocolo).
    pub async fn connect(&self) -> Result<Connection> {
        // O rustls do app não traz provedor de criptografia escolhido (o
        // reqwest vem com `rustls-no-provider`): instala o ring, que é o que
        // está compilado. Se alguém já instalou, fica o dele.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let config = StreamableHttpClientTransportConfig::with_uri(self.url.as_str())
            .auth_header(self.token.clone());
        let transport = StreamableHttpClientTransport::from_config(config);
        let info = ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("isper", env!("CARGO_PKG_VERSION")),
        )
        .with_protocol_version(ProtocolVersion::V_2025_06_18);
        let service = self
            .deadline(async { info.serve(transport).await.map_err(|e| classify(&e)) })
            .await?;
        Ok(Connection {
            service,
            timeout: self.timeout,
        })
    }

    async fn deadline<T>(&self, fut: impl Future<Output = Result<T>>) -> Result<T> {
        tokio::time::timeout(self.timeout, fut)
            .await
            .unwrap_or(Err(McpError::Timeout(self.timeout.as_secs())))
    }

    /// Abre, chama uma ferramenta e fecha.
    pub async fn call(&self, name: &str, args: Value) -> Result<ToolOutput> {
        let conn = self.connect().await?;
        let out = conn.call(name, args).await;
        conn.close().await;
        out
    }

    /// Abre, chama e lê o `structuredContent` como `T`.
    pub async fn call_typed<T: DeserializeOwned>(&self, name: &str, args: Value) -> Result<T> {
        self.call(name, args).await?.parse(name)
    }
}

/// Uma conexão aberta.
pub struct Connection {
    service: RunningService<RoleClient, ClientConfig>,
    timeout: Duration,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection").finish_non_exhaustive()
    }
}

impl Connection {
    /// Nome e versão do servidor, como ele se apresentou.
    pub fn server(&self) -> Option<(String, String)> {
        let info = self.service.peer_info()?;
        let server = info.server_info.as_ref()?;
        Some((server.name.clone(), server.version.clone()))
    }

    /// O catálogo de ferramentas.
    pub async fn tools(&self) -> Result<Vec<ToolSummary>> {
        let tools = self
            .deadline(async {
                self.service
                    .list_all_tools()
                    .await
                    .map_err(|e| classify(&e))
            })
            .await?;
        Ok(tools.into_iter().map(ToolSummary::from).collect())
    }

    /// Chama uma ferramenta. Erro de ferramenta (`isError`) vira
    /// [`McpError::Tool`], com o código e a dica do `_meta` quando o servidor
    /// manda (o OptTime manda em `opt-time/error`).
    pub async fn call(&self, name: &str, args: Value) -> Result<ToolOutput> {
        let arguments: Option<JsonObject> = match args {
            Value::Object(map) => Some(map),
            Value::Null => None,
            other => {
                return Err(McpError::Protocol(format!(
                    "argumentos de {name} precisam ser um objeto, não {other}"
                )));
            }
        };
        let mut params = CallToolRequestParams::new(name.to_string());
        params.arguments = arguments;
        let result = self
            .deadline(async {
                self.service
                    .call_tool(params)
                    .await
                    .map_err(|e| classify(&e))
            })
            .await?;
        let text = result
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        if result.is_error == Some(true) {
            let meta = result.meta.as_ref().map(|m| &m.0);
            return Err(McpError::Tool(ToolError::from_result(meta, &text)));
        }
        Ok(ToolOutput {
            structured: result.structured_content,
            text,
        })
    }

    async fn deadline<T>(&self, fut: impl Future<Output = Result<T>>) -> Result<T> {
        tokio::time::timeout(self.timeout, fut)
            .await
            .unwrap_or(Err(McpError::Timeout(self.timeout.as_secs())))
    }

    /// Fecha a conexão. Sem sessão no servidor, é só parar o cliente.
    pub async fn close(self) {
        let _ = tokio::time::timeout(Duration::from_secs(2), self.service.cancel()).await;
    }
}

/// O que uma ferramenta devolveu.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolOutput {
    /// O `structuredContent`, que segue o `outputSchema` da ferramenta.
    pub structured: Option<Value>,
    /// O texto para gente (e para a LLM).
    pub text: String,
}

impl ToolOutput {
    /// Lê o `structuredContent` como `T`.
    pub fn parse<T: DeserializeOwned>(self, tool: &str) -> Result<T> {
        let value = self
            .structured
            .ok_or_else(|| McpError::Protocol(format!("{tool} sem structuredContent")))?;
        serde_json::from_value(value).map_err(|e| McpError::Protocol(format!("{tool}: {e}")))
    }
}

/// Erro de ferramenta, com o que o servidor disse para corrigir.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolError {
    /// `MICROSOFT_NOT_CONNECTED`, `INSUFFICIENT_SCOPE`… ou `TOOL_ERROR`
    /// quando o servidor não manda código.
    pub code: String,
    /// A mensagem.
    pub message: String,
    /// Como corrigir.
    pub hint: Option<String>,
    /// Detalhes (o item que falhou, por exemplo).
    pub details: Option<Value>,
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, " ({hint})")?;
        }
        Ok(())
    }
}

impl ToolError {
    /// Procura no `_meta` um objeto `<servidor>/error` com `code` e
    /// `message`; sem ele, o texto da resposta vira a mensagem.
    fn from_result(meta: Option<&JsonObject>, text: &str) -> Self {
        let found = meta.and_then(|m| {
            m.iter()
                .filter(|(k, _)| k.ends_with("/error"))
                .find_map(|(_, v)| {
                    let code = v.get("code")?.as_str()?;
                    let message = v.get("message")?.as_str()?;
                    Some(Self {
                        code: code.to_string(),
                        message: message.to_string(),
                        hint: v.get("hint").and_then(Value::as_str).map(String::from),
                        details: v.get("details").filter(|d| !d.is_null()).cloned(),
                    })
                })
        });
        found.unwrap_or_else(|| Self {
            code: "TOOL_ERROR".into(),
            message: if text.trim().is_empty() {
                "a ferramenta falhou sem dizer por quê".into()
            } else {
                text.trim().to_string()
            },
            hint: None,
            details: None,
        })
    }
}

/// Uma ferramenta do catálogo, com as marcações que decidem a permissão
/// ([ADR 0023]).
///
/// [ADR 0023]: ../../../docs/adr/0023-escada-de-confianca.md
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolSummary {
    /// Nome.
    pub name: String,
    /// Título para gente.
    pub title: Option<String>,
    /// Descrição.
    pub description: Option<String>,
    /// `readOnlyHint`: só lê.
    pub read_only: bool,
    /// `destructiveHint`: apaga ou sobrescreve. O padrão do MCP é `true`
    /// quando o servidor não diz.
    pub destructive: bool,
    /// `idempotentHint`: repetir não duplica.
    pub idempotent: bool,
    /// O esquema de entrada.
    pub input_schema: Value,
    /// O esquema de saída, quando publicado.
    pub output_schema: Option<Value>,
}

impl From<rmcp::model::Tool> for ToolSummary {
    fn from(tool: rmcp::model::Tool) -> Self {
        let notes = tool.annotations.as_ref();
        let read_only = notes.and_then(|a| a.read_only_hint).unwrap_or(false);
        Self {
            name: tool.name.to_string(),
            title: tool
                .title
                .clone()
                .or_else(|| notes.and_then(|a| a.title.clone())),
            description: tool.description.as_ref().map(|d| d.to_string()),
            read_only,
            destructive: !read_only && notes.and_then(|a| a.destructive_hint).unwrap_or(true),
            idempotent: notes.and_then(|a| a.idempotent_hint).unwrap_or(false),
            input_schema: Value::Object((*tool.input_schema).clone()),
            output_schema: tool.output_schema.map(|s| Value::Object((*s).clone())),
        }
    }
}

/// Traduz o erro do `rmcp` (que chega como texto, depois de passar pelo
/// worker do transporte) para o que a interface sabe explicar.
fn classify(error: &dyn std::error::Error) -> McpError {
    let mut chain = error.to_string();
    let mut source = error.source();
    while let Some(e) = source {
        chain.push_str(": ");
        chain.push_str(&e.to_string());
        source = e.source();
    }
    let lower = chain.to_lowercase();
    if lower.contains("auth required") || lower.contains("401") {
        McpError::Unauthorized
    } else if lower.contains("insufficient scope") || lower.contains("403") {
        McpError::Forbidden(chain)
    } else if lower.contains("timed out") || lower.contains("timeout") {
        McpError::Timeout(DEFAULT_TIMEOUT.as_secs())
    } else {
        McpError::Transport(chain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn so_https_fora_do_pc() {
        assert!(Endpoint::new("https://opt-time.optsolv.com.br/api/mcp", "t").is_ok());
        assert!(Endpoint::new("http://127.0.0.1:8123/api/mcp", "t").is_ok());
        assert!(Endpoint::new("http://localhost:8123/api/mcp", "t").is_ok());
        assert!(matches!(
            Endpoint::new("http://opt-time.optsolv.com.br/api/mcp", "t"),
            Err(McpError::BadUrl(_))
        ));
        assert!(matches!(
            Endpoint::new("https://eu:senha@x.com/mcp", "t"),
            Err(McpError::BadUrl(_))
        ));
        assert!(matches!(
            Endpoint::new("https://x.com/mcp", "  "),
            Err(McpError::NoToken)
        ));
        assert!(Endpoint::new("https://x.com/mcp", "a\u{16}b").is_err());
    }

    #[test]
    fn token_nao_aparece_no_debug() {
        let e = Endpoint::new("https://x.com/mcp", "opt_tok_segredo").unwrap();
        assert!(!format!("{e:?}").contains("segredo"));
    }

    #[test]
    fn erro_de_ferramenta_vem_do_meta() {
        let meta = json!({
            "opt-time/error": {
                "code": "MICROSOFT_NOT_CONNECTED",
                "message": "Conta Microsoft não conectada.",
                "hint": "Entre no OptTime com a conta Microsoft.",
                "details": null
            }
        });
        let e = ToolError::from_result(meta.as_object(), "❌ texto");
        assert_eq!(e.code, "MICROSOFT_NOT_CONNECTED");
        assert_eq!(
            e.hint.as_deref(),
            Some("Entre no OptTime com a conta Microsoft.")
        );
        assert_eq!(e.details, None);

        let sem_meta = ToolError::from_result(None, "  algo quebrou ");
        assert_eq!(sem_meta.code, "TOOL_ERROR");
        assert_eq!(sem_meta.message, "algo quebrou");
    }
}
