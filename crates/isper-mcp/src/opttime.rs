//! O conector do OptTime: as ferramentas que as rotinas usam, com os tipos
//! do `outputSchema` publicado (OptTime v1.11.0, `src/lib/mcp/output-schemas.ts`).
//!
//! Campos que o OptTime ainda pode acrescentar são ignorados, e listas que
//! ele declara abertas (a `source` de uma sugestão) ficam como texto.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::Endpoint;
use crate::{McpError, Result};

/// O MCP hospedado do OptTime em produção.
pub const DEFAULT_URL: &str = "https://opt-time.optsolv.com.br/api/mcp";
/// Nome do token no cofre: alvo `opttime.ISPer` no Credential Manager.
pub const SECRET_NAME: &str = "opttime";
/// Escopos do preset "Assistente pessoal (ISPer)".
pub const REQUIRED_SCOPES: [&str; 3] = ["time:read", "time:write", "calendar:read"];
/// Quantas sugestões o `apply` aceita de uma vez.
pub const MAX_APPLY_ITEMS: usize = 12;

const WHOAMI: &str = "opt_time_whoami";
const DAY_SUMMARY: &str = "opt_time_get_today_summary";
const SUGGEST: &str = "opt_time_suggest_daily_entries";
const APPLY: &str = "opt_time_apply_suggestions";

/// Quem é o dono do token e o que está conectado (`opt_time_whoami`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Whoami {
    /// Id do usuário.
    pub user_id: String,
    /// Nome.
    pub name: String,
    /// E-mail.
    pub email: String,
    /// `member`, `manager` ou `admin`.
    pub role: String,
    /// Escopos do token.
    pub scopes: Vec<String>,
    /// Nome dado ao token.
    #[serde(default)]
    pub token_name: Option<String>,
    /// Fuso em que o OptTime lê datas.
    pub timezone: String,
    /// Capacidade semanal em minutos.
    pub weekly_capacity_minutes: i64,
    /// Hoje no OptTime.
    pub today: WhoamiToday,
    /// A conta Microsoft (agenda e Teams).
    pub microsoft: MicrosoftStatus,
    /// O Azure DevOps.
    pub azure_dev_ops: DevOpsStatus,
    /// Se o resumo noturno do Teams (17:30) está ligado.
    pub evening_digest_enabled: bool,
}

impl Whoami {
    /// Os escopos do preset que faltam no token.
    pub fn missing_scopes(&self) -> Vec<&'static str> {
        REQUIRED_SCOPES
            .into_iter()
            .filter(|s| !self.scopes.iter().any(|have| have == s))
            .collect()
    }
}

/// O dia de hoje no `whoami`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhoamiToday {
    /// `AAAA-MM-DD`.
    pub date: String,
    /// Minutos já registrados.
    pub total_minutes: i64,
    /// Capacidade diária.
    pub daily_capacity_minutes: i64,
}

/// A conta Microsoft no OptTime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrosoftStatus {
    /// Há conta vinculada.
    pub connected: bool,
    /// A conexão expirou: novo login no OptTime.
    pub needs_reconnect: bool,
    /// Um token do Graph saiu agora. Falso com `connected` verdadeiro: a
    /// agenda e as sugestões do Outlook vão falhar.
    pub token_usable: bool,
}

/// O Azure DevOps no OptTime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevOpsStatus {
    /// A integração está ativa.
    pub configured: bool,
}

/// O resumo de um dia (`opt_time_get_today_summary`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaySummary {
    /// `AAAA-MM-DD`.
    pub date: String,
    /// Dia da semana por extenso.
    pub weekday: String,
    /// Minutos registrados.
    pub total_minutes: i64,
    /// Total legível.
    pub total_label: String,
    /// Lançamentos no dia.
    pub entry_count: i64,
    /// Capacidade diária (semanal ÷ 5).
    pub daily_capacity_minutes: i64,
    /// Falso em fim de semana, folga do expediente ou ausência agendada.
    pub is_workday: bool,
    /// A meta do dia, já ajustada ao expediente do Outlook; 0 se não é útil.
    pub target_minutes: i64,
    /// Como o dia foi avaliado.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Minutos por projeto.
    #[serde(default)]
    pub by_project: Vec<ProjectMinutes>,
    /// Timer rodando, como veio.
    #[serde(default)]
    pub active_timer: Option<serde_json::Value>,
}

impl DaySummary {
    /// Quanto falta para a meta do dia (0 se já fechou ou não é dia útil).
    pub fn missing_minutes(&self) -> i64 {
        if self.is_workday {
            (self.target_minutes - self.total_minutes).max(0)
        } else {
            0
        }
    }

    /// Dia útil com a meta batida.
    pub fn reached_target(&self) -> bool {
        self.is_workday && self.missing_minutes() == 0
    }
}

/// Minutos de um projeto no dia.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMinutes {
    /// Nome do projeto.
    pub project_name: String,
    /// Código.
    pub project_code: String,
    /// Minutos.
    pub minutes: i64,
}

/// As sugestões para preencher um dia (`opt_time_suggest_daily_entries`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestions {
    /// `AAAA-MM-DD`.
    pub date: String,
    /// As sugestões.
    pub suggestions: Vec<Suggestion>,
    /// Minutos já registrados.
    pub already_logged_minutes: i64,
    /// A meta.
    pub target_minutes: i64,
    /// Quanto falta.
    pub gap_minutes: i64,
    /// De onde as sugestões vieram.
    pub sources: SuggestionSources,
    /// Fontes que falharam (o plano segue sem elas).
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Orientações do OptTime.
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Que fontes o OptTime conseguiu ler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionSources {
    /// A agenda do Outlook.
    pub outlook: bool,
    /// As chamadas do Teams.
    pub teams_calls: bool,
    /// O Azure DevOps.
    pub azure_dev_ops: bool,
    /// O histórico da semana.
    pub history: bool,
}

/// Uma sugestão de lançamento.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// Id estável: o mesmo dia reconstruído dá o mesmo id.
    pub id: String,
    /// `calendar`, `teams_call`, `commits`, `work_item`, `pattern`,
    /// `document`… (lista aberta).
    pub source: String,
    /// O evento, a chamada ou o work item de origem.
    #[serde(default)]
    pub source_ref: Option<String>,
    /// Projeto sugerido; sem ele, é preciso escolher ao aplicar.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Nome do projeto.
    #[serde(default)]
    pub project_name: Option<String>,
    /// Descrição sugerida.
    pub description: String,
    /// `AAAA-MM-DD`.
    pub date: String,
    /// Início, quando ancorada no tempo.
    #[serde(default)]
    pub starts_at: Option<String>,
    /// Duração em minutos.
    pub duration_minutes: i64,
    /// Duração legível.
    pub duration_label: String,
    /// Faturável.
    pub billable: bool,
    /// Work item vinculado.
    #[serde(default)]
    pub azure_work_item_id: Option<i64>,
    /// `high`, `medium` ou `low`.
    pub confidence: String,
    /// Por que foi sugerida, numa linha.
    pub evidence: String,
}

/// Uma sugestão aprovada, com as edições permitidas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyItem {
    /// O id da sugestão.
    pub suggestion_id: String,
    /// Outro projeto (id, código ou nome).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Outra duração.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_minutes: Option<i64>,
    /// Outra descrição.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Outro faturável.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billable: Option<bool>,
}

/// O pedido de `opt_time_apply_suggestions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    /// O dia das sugestões.
    pub date: String,
    /// Gerada uma vez por pedido: repetir a mesma chamada (um repique de rede)
    /// devolve o mesmo resultado sem duplicar lançamento.
    pub idempotency_key: String,
    /// As aprovadas.
    pub items: Vec<ApplyItem>,
    /// As recusadas, para o OptTime aprender.
    #[serde(default)]
    pub rejected_suggestion_ids: Vec<String>,
}

impl ApplyRequest {
    /// Um pedido novo, com chave de idempotência nova.
    pub fn new(date: impl Into<String>, items: Vec<ApplyItem>, rejected: Vec<String>) -> Self {
        Self {
            date: date.into(),
            idempotency_key: uuid::Uuid::new_v4().to_string(),
            items,
            rejected_suggestion_ids: rejected,
        }
    }

    fn check(&self) -> Result<()> {
        if self.items.is_empty() || self.items.len() > MAX_APPLY_ITEMS {
            return Err(McpError::Protocol(format!(
                "aplique de 1 a {MAX_APPLY_ITEMS} sugestões por vez"
            )));
        }
        if self
            .items
            .iter()
            .any(|i| i.duration_minutes.is_some_and(|m| !(1..=1440).contains(&m)))
        {
            return Err(McpError::Protocol("duração de 1 minuto a 24 horas".into()));
        }
        Ok(())
    }
}

/// O que `opt_time_apply_suggestions` gravou.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    /// O dia.
    pub date: String,
    /// Os lançamentos criados.
    pub created_entry_ids: Vec<String>,
    /// Total do dia depois.
    pub day_total_minutes: i64,
    /// Capacidade diária.
    pub daily_capacity_minutes: i64,
    /// Quanto ainda falta.
    pub remaining_minutes: i64,
    /// A chave já tinha sido usada: nada novo foi gravado.
    pub replayed: bool,
}

/// O OptTime por MCP.
#[derive(Debug, Clone)]
pub struct OptTime {
    endpoint: Endpoint,
}

impl OptTime {
    /// O conector com URL e token.
    pub fn new(endpoint: Endpoint) -> Self {
        Self { endpoint }
    }

    /// O endpoint em uso.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Quem é, que escopos tem e o que está conectado.
    pub async fn whoami(&self) -> Result<Whoami> {
        self.endpoint.call_typed(WHOAMI, json!({})).await
    }

    /// O resumo de um dia (`None` = hoje no fuso do OptTime).
    pub async fn day_summary(&self, date: Option<&str>) -> Result<DaySummary> {
        self.endpoint.call_typed(DAY_SUMMARY, date_args(date)).await
    }

    /// As sugestões para um dia (hoje ou até 30 dias para trás).
    pub async fn suggest(&self, date: Option<&str>) -> Result<Suggestions> {
        self.endpoint.call_typed(SUGGEST, date_args(date)).await
    }

    /// Grava as sugestões aprovadas, numa transação do OptTime. Só depois do
    /// toque do usuário ([ADR 0023]).
    ///
    /// [ADR 0023]: ../../../docs/adr/0023-escada-de-confianca.md
    pub async fn apply(&self, request: &ApplyRequest) -> Result<Applied> {
        request.check()?;
        let args = serde_json::to_value(request)
            .map_err(|e| McpError::Protocol(format!("pedido ilegível: {e}")))?;
        self.endpoint.call_typed(APPLY, args).await
    }
}

fn date_args(date: Option<&str>) -> serde_json::Value {
    match date {
        Some(d) => json!({ "date": d }),
        None => json!({}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whoami_de_producao_abre() {
        // A forma do `structuredContent` do OptTime em 08/10/2026 (dados
        // trocados), com um campo novo que o ISPer ainda não conhece.
        let v = json!({
            "userId": "u1", "name": "Fulano", "email": "f@x.com", "role": "member",
            "scopes": ["time:read", "time:write"], "tokenName": "ISPer",
            "timezone": "America/Sao_Paulo", "weeklyCapacityMinutes": 2400,
            "today": { "date": "2026-10-08", "totalMinutes": 300, "dailyCapacityMinutes": 480 },
            "microsoft": { "connected": true, "needsReconnect": false, "tokenUsable": true },
            "azureDevOps": { "configured": true },
            "eveningDigestEnabled": true,
            "novidade": 1
        });
        let w: Whoami = serde_json::from_value(v).unwrap();
        assert_eq!(w.missing_scopes(), ["calendar:read"]);
        assert!(w.evening_digest_enabled);
    }

    #[test]
    fn meta_do_dia() {
        let mut d: DaySummary = serde_json::from_value(json!({
            "date": "2026-10-08", "weekday": "quinta-feira", "totalMinutes": 280,
            "totalLabel": "4h40", "entryCount": 3, "dailyCapacityMinutes": 480,
            "isWorkday": true, "targetMinutes": 480, "warnings": []
        }))
        .unwrap();
        assert_eq!(d.missing_minutes(), 200);
        assert!(!d.reached_target());
        d.total_minutes = 500;
        assert!(d.reached_target());
        d.is_workday = false;
        d.target_minutes = 0;
        d.total_minutes = 0;
        assert_eq!(d.missing_minutes(), 0);
        assert!(!d.reached_target(), "fim de semana não é meta batida");
    }

    #[test]
    fn pedido_de_aplicar_leva_a_chave_e_so_o_editado() {
        let req = ApplyRequest::new(
            "2026-10-08",
            vec![ApplyItem {
                suggestion_id: "s1".into(),
                project_id: None,
                duration_minutes: Some(90),
                description: None,
                billable: None,
            }],
            vec!["s2".into()],
        );
        assert!(req.check().is_ok());
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(
            v["items"][0],
            json!({ "suggestionId": "s1", "durationMinutes": 90 })
        );
        assert_eq!(v["rejectedSuggestionIds"][0], "s2");
        assert_eq!(v["idempotencyKey"].as_str().unwrap().len(), 36);
        assert_ne!(
            ApplyRequest::new("2026-10-08", vec![], vec![]).idempotency_key,
            req.idempotency_key
        );
        assert!(
            ApplyRequest::new("2026-10-08", vec![], vec![])
                .check()
                .is_err()
        );
    }
}
