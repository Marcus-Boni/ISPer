//! O conector do OptTime: as ferramentas que as rotinas usam, com os tipos
//! do `outputSchema` publicado (OptTime v1.11.0, `src/lib/mcp/output-schemas.ts`).
//!
//! Campos que o OptTime ainda pode acrescentar são ignorados, e listas que
//! ele declara abertas (a `source` de uma sugestão) ficam como texto.

use chrono::{DateTime, FixedOffset};
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
const AGENDA: &str = "opt_time_get_my_agenda";

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

/// A agenda do Outlook de um ou mais dias (`opt_time_get_my_agenda`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agenda {
    /// Fuso das datas.
    pub timezone: String,
    /// Avisos sobre a leitura.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Os eventos, em ordem de início (cancelados de fora).
    pub events: Vec<AgendaEvent>,
}

/// Uma pessoa num evento.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPerson {
    /// Nome.
    #[serde(default)]
    pub name: Option<String>,
    /// E-mail.
    #[serde(default)]
    pub email: Option<String>,
}

/// A presença medida pelo Teams.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attendance {
    /// Entrou na reunião.
    pub joined: bool,
    /// Minutos medidos.
    pub minutes: i64,
}

/// Um evento da agenda.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgendaEvent {
    /// Id no Graph.
    pub id: String,
    /// Estável entre série e ocorrências.
    #[serde(rename = "iCalUId")]
    pub ical_uid: String,
    /// A série, se recorrente.
    #[serde(default)]
    pub series_master_id: Option<String>,
    /// Assunto.
    pub subject: String,
    /// Início, ISO 8601 com offset.
    pub start: String,
    /// Fim, ISO 8601 com offset.
    pub end: String,
    /// Duração em minutos.
    pub duration_minutes: i64,
    /// Dia inteiro (férias, feriado).
    pub is_all_day: bool,
    /// Reunião online.
    pub is_online: bool,
    /// Link para entrar.
    #[serde(default)]
    pub join_url: Option<String>,
    /// Quem organiza.
    pub organizer: EventPerson,
    /// O usuário organiza.
    pub is_organizer: bool,
    /// Resposta ao convite (`accepted`, `declined`, `organizer`…).
    pub response_status: String,
    /// Total de convidados.
    pub attendee_count: i64,
    /// Até 20 convidados.
    #[serde(default)]
    pub attendees: Vec<EventPerson>,
    /// Local.
    #[serde(default)]
    pub location: Option<String>,
    /// Como aparece na disponibilidade (`busy`, `free`, `oof`…).
    pub show_as: String,
    /// Link para abrir no Outlook.
    #[serde(default)]
    pub web_link: Option<String>,
    /// Minutos já lançados no OptTime para o evento.
    #[serde(default)]
    pub logged_minutes: i64,
    /// Presença no Teams, quando já medida.
    #[serde(default)]
    pub attendance: Option<Attendance>,
}

impl AgendaEvent {
    /// Início como instante.
    pub fn starts(&self) -> Option<DateTime<FixedOffset>> {
        DateTime::parse_from_rfc3339(&self.start).ok()
    }

    /// Fim como instante.
    pub fn ends(&self) -> Option<DateTime<FixedOffset>> {
        DateTime::parse_from_rfc3339(&self.end).ok()
    }

    /// Conta como reunião de que a pessoa participa: não é de dia inteiro,
    /// não foi recusado e não está marcado como livre.
    pub fn is_meeting(&self) -> bool {
        !self.is_all_day && self.response_status != "declined" && self.show_as != "free"
    }

    /// A identidade que casa ocorrências da mesma série.
    pub fn series(&self) -> Option<&str> {
        self.series_master_id.as_deref().filter(|s| !s.is_empty())
    }
}

/// Folga antes e depois do evento ao casar uma gravação: quem começa a
/// gravar cinco minutos antes, ou passa do horário, ainda está nele.
pub const MATCH_SLACK_SECS: i64 = 5 * 60;
/// Menos que isso de sobreposição não casa.
const MIN_OVERLAP_SECS: i64 = 60;

/// O evento da agenda em que uma gravação aconteceu: o que mais se sobrepõe
/// ao intervalo gravado (com [`MATCH_SLACK_SECS`] de folga), contando só as
/// reuniões de que a pessoa participa. Empate fica com o online, depois com o
/// que começou mais perto do começo da gravação.
pub fn match_event(
    events: &[AgendaEvent],
    started: DateTime<FixedOffset>,
    ended: DateTime<FixedOffset>,
) -> Option<&AgendaEvent> {
    let slack = chrono::Duration::seconds(MATCH_SLACK_SECS);
    events
        .iter()
        .filter(|e| e.is_meeting())
        .filter_map(|e| {
            let (s, f) = (e.starts()?, e.ends()?);
            let from = started.max(s - slack);
            let to = ended.min(f + slack);
            let overlap = (to - from).num_seconds();
            (overlap >= MIN_OVERLAP_SECS).then(|| {
                let distance = (s - started).num_seconds().abs();
                (e, overlap, e.is_online, distance)
            })
        })
        .max_by(|a, b| a.1.cmp(&b.1).then(a.2.cmp(&b.2)).then(b.3.cmp(&a.3)))
        .map(|(e, ..)| e)
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

    /// A agenda do Outlook a partir de `date` (`None` = hoje), por `days`
    /// dias (1 a 7), sem os eventos recusados.
    pub async fn agenda(&self, date: Option<&str>, days: u8) -> Result<Agenda> {
        let mut args = date_args(date);
        args["days"] = json!(days.clamp(1, 7));
        self.endpoint.call_typed(AGENDA, args).await
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

    fn evento(uid: &str, start: &str, end: &str, online: bool) -> AgendaEvent {
        serde_json::from_value(json!({
            "id": format!("id-{uid}"), "iCalUId": uid, "seriesMasterId": null, "type": "singleInstance",
            "subject": format!("Reunião {uid}"), "start": start, "end": end, "durationMinutes": 30,
            "isAllDay": false, "isOnline": online, "joinUrl": null,
            "organizer": { "name": "Ana", "email": "ana@example.com" }, "isOrganizer": false,
            "responseStatus": "accepted", "attendeeCount": 3, "attendees": [], "showAs": "busy",
            "sensitivity": "normal", "loggedMinutes": 0, "attendance": null
        }))
        .unwrap()
    }

    fn at(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    #[test]
    fn a_gravacao_casa_com_o_evento_que_mais_se_sobrepoe() {
        let eventos = vec![
            evento(
                "daily",
                "2026-10-08T09:00:00-03:00",
                "2026-10-08T09:30:00-03:00",
                true,
            ),
            evento(
                "refino",
                "2026-10-08T09:30:00-03:00",
                "2026-10-08T10:30:00-03:00",
                true,
            ),
            evento(
                "almoco",
                "2026-10-08T12:00:00-03:00",
                "2026-10-08T13:00:00-03:00",
                false,
            ),
        ];
        // Começou a gravar 3 min antes da daily e passou 5 min do horário.
        let m = match_event(
            &eventos,
            at("2026-10-08T08:57:00-03:00"),
            at("2026-10-08T09:35:00-03:00"),
        );
        assert_eq!(m.unwrap().ical_uid, "daily");
        // Gravou o refinamento inteiro.
        let m = match_event(
            &eventos,
            at("2026-10-08T09:31:00-03:00"),
            at("2026-10-08T10:25:00-03:00"),
        );
        assert_eq!(m.unwrap().ical_uid, "refino");
        // Fora de qualquer evento.
        assert!(
            match_event(
                &eventos,
                at("2026-10-08T15:00:00-03:00"),
                at("2026-10-08T15:40:00-03:00")
            )
            .is_none()
        );
    }

    #[test]
    fn recusado_livre_e_dia_inteiro_nao_casam() {
        let mut recusado = evento(
            "x",
            "2026-10-08T14:00:00-03:00",
            "2026-10-08T15:00:00-03:00",
            true,
        );
        recusado.response_status = "declined".into();
        let mut livre = recusado.clone();
        livre.response_status = "accepted".into();
        livre.show_as = "free".into();
        let mut dia = livre.clone();
        dia.show_as = "busy".into();
        dia.is_all_day = true;
        let gravou = (
            at("2026-10-08T14:05:00-03:00"),
            at("2026-10-08T14:50:00-03:00"),
        );
        for e in [recusado, livre, dia] {
            assert!(match_event(std::slice::from_ref(&e), gravou.0, gravou.1).is_none());
        }
    }

    #[test]
    fn empate_fica_com_o_online() {
        let presencial = evento(
            "sala",
            "2026-10-08T16:00:00-03:00",
            "2026-10-08T17:00:00-03:00",
            false,
        );
        let online = evento(
            "teams",
            "2026-10-08T16:00:00-03:00",
            "2026-10-08T17:00:00-03:00",
            true,
        );
        let ambos = [presencial, online];
        let m = match_event(
            &ambos,
            at("2026-10-08T16:00:00-03:00"),
            at("2026-10-08T16:40:00-03:00"),
        );
        assert_eq!(m.unwrap().ical_uid, "teams");
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
