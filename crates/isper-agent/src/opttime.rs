//! As ferramentas do OptTime para o agente, pelo catálogo do MCP ([ADR 0022]).
//!
//! O catálogo vem do servidor (`tools/list`): nome, descrição, esquema e as
//! marcações. A permissão padrão sai delas ([ADR 0023]): só leitura roda
//! livre, o resto pede um toque. A pessoa muda por ferramenta nas
//! Configurações, mas o que apaga (`destructiveHint`) nunca fica liberado.
//!
//! [ADR 0022]: ../../../docs/adr/0022-conectores-mcp-e-opttime.md
//! [ADR 0023]: ../../../docs/adr/0023-escada-de-confianca.md

use std::collections::HashMap;

use isper_llm::ToolSpec;
use isper_llm::agent::{Permission, Source, ToolOutcome};
use isper_mcp::{Endpoint, McpError, ToolSummary};
use serde_json::Value;
use tokio::runtime::Handle;

/// O conector do OptTime com o catálogo já lido.
#[derive(Clone)]
pub struct OptTimeTools {
    endpoint: Endpoint,
    handle: Handle,
    catalog: Vec<ToolSummary>,
}

impl std::fmt::Debug for OptTimeTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OptTimeTools")
            .field("tools", &self.catalog.len())
            .finish_non_exhaustive()
    }
}

impl OptTimeTools {
    /// Lê o catálogo do servidor. `handle` é o runtime em que as chamadas
    /// assíncronas rodam; quem chama fica numa thread comum, fora dele.
    pub fn connect(endpoint: Endpoint, handle: Handle) -> Result<Self, McpError> {
        let catalog = handle.block_on(async {
            let conn = endpoint.connect().await?;
            let tools = conn.tools().await;
            conn.close().await;
            tools
        })?;
        Ok(Self {
            endpoint,
            handle,
            catalog,
        })
    }

    /// O catálogo, para a tela de permissões.
    pub fn catalog(&self) -> &[ToolSummary] {
        &self.catalog
    }

    pub(crate) fn specs(&self) -> Vec<ToolSpec> {
        self.catalog
            .iter()
            .map(|t| ToolSpec {
                name: t.name.clone(),
                description: t.description.clone().unwrap_or_else(|| t.name.clone()),
                input_schema: t.input_schema.clone(),
            })
            .collect()
    }

    fn find(&self, name: &str) -> Option<&ToolSummary> {
        self.catalog.iter().find(|t| t.name == name)
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// A permissão: a escolhida pela pessoa, ou a das marcações.
    pub(crate) fn permission(
        &self,
        name: &str,
        chosen: &HashMap<String, Permission>,
    ) -> Permission {
        let Some(tool) = self.find(name) else {
            return Permission::Never;
        };
        default_or_chosen(tool, chosen.get(name).copied())
    }

    pub(crate) fn call(&self, name: &str, args: &Value) -> ToolOutcome {
        let title = self
            .find(name)
            .and_then(|t| t.title.clone())
            .unwrap_or_else(|| name.to_string());
        match self.handle.block_on(self.endpoint.call(name, args.clone())) {
            Ok(out) => {
                let content = out.structured.map(|v| v.to_string()).unwrap_or(out.text);
                let day = args
                    .get("date")
                    .and_then(Value::as_str)
                    .unwrap_or("hoje")
                    .to_string();
                let reference = format!("opttime:{name}:{day}");
                let resultado =
                    serde_json::from_str::<Value>(&content).unwrap_or(Value::String(content));
                ToolOutcome {
                    content: serde_json::json!({"ref": reference, "resultado": resultado})
                        .to_string(),
                    is_error: false,
                    sources: vec![Source {
                        reference,
                        kind: "opttime".into(),
                        label: format!("OptTime · {title}"),
                    }],
                }
            }
            Err(e) => {
                let hint = e
                    .hint()
                    .map(|h| format!(" Como resolver: {h}"))
                    .unwrap_or_default();
                ToolOutcome::error(format!("{} ({}).{hint}", e, e.code()))
            }
        }
    }

    /// A frase do cartão: o título da ferramenta e os argumentos.
    pub(crate) fn describe(&self, name: &str, args: &Value) -> String {
        let title = self
            .find(name)
            .and_then(|t| t.title.clone())
            .unwrap_or_else(|| name.to_string());
        let mut parts = Vec::new();
        if let Some(map) = args.as_object() {
            for (k, v) in map {
                if k == "idempotencyKey" {
                    continue;
                }
                let v = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let v: String = v.chars().take(80).collect();
                parts.push(format!("{k}: {v}"));
            }
        }
        if parts.is_empty() {
            format!("OptTime · {title}")
        } else {
            format!("OptTime · {title} — {}", parts.join(", "))
        }
    }
}

/// A permissão de uma ferramenta: a escolhida, sem nunca liberar o que
/// apaga; sem escolha, ler é livre e o resto pede o toque.
pub fn default_or_chosen(tool: &ToolSummary, chosen: Option<Permission>) -> Permission {
    match chosen {
        Some(Permission::Allow) if tool.destructive => Permission::Ask,
        Some(p) => p,
        None if tool.read_only => Permission::Allow,
        None => Permission::Ask,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn summary(read_only: bool, destructive: bool) -> ToolSummary {
        ToolSummary {
            name: "x".into(),
            title: None,
            description: None,
            read_only,
            destructive,
            idempotent: false,
            input_schema: json!({"type": "object"}),
            output_schema: None,
        }
    }

    #[test]
    fn ler_e_livre_escrever_pergunta_e_apagar_nunca_fica_liberado() {
        assert_eq!(
            default_or_chosen(&summary(true, false), None),
            Permission::Allow
        );
        assert_eq!(
            default_or_chosen(&summary(false, false), None),
            Permission::Ask
        );
        assert_eq!(
            default_or_chosen(&summary(false, false), Some(Permission::Allow)),
            Permission::Allow
        );
        assert_eq!(
            default_or_chosen(&summary(false, true), Some(Permission::Allow)),
            Permission::Ask,
            "o que apaga sempre pergunta"
        );
        assert_eq!(
            default_or_chosen(&summary(true, false), Some(Permission::Never)),
            Permission::Never
        );
    }
}
