//! O assistente na tela Hoje (Fase 10.4, [ADR 0021] e [ADR 0023]).
//!
//! - **Perguntar:** a barra da tela Hoje manda a pergunta; o agente do
//!   `isper-agent` busca nas ferramentas do ISPer (tarefas, diário, reuniões,
//!   ditados, rotinas) e do OptTime (agenda, horas, work items) e responde
//!   com as fontes citadas, que a tela mostra como selos clicáveis.
//! - **Resumo da manhã e fechamento do dia:** os dois botões mandam os
//!   pedidos prontos ([`isper_agent::MORNING`] e [`isper_agent::CLOSING`]).
//! - **Cartão de confirmação:** o que escreve (criar, mover e concluir
//!   tarefa; lançar horas) para e espera o toque. A conversa fica guardada
//!   aqui entre a pergunta e o toque.
//! - **Permissões:** cada ferramenta fica liberada, pergunta ou bloqueada,
//!   nas Configurações; o que apaga nunca fica liberado.
//!
//! O modelo é o das Configurações (Inteligência). As ferramentas rodam no
//! PC: o modelo só vê o que elas devolvem para a pergunta.
//!
//! [ADR 0021]: ../../../../docs/adr/0021-assistente-pessoal-no-isper.md
//! [ADR 0023]: ../../../../docs/adr/0023-escada-de-confianca.md

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::connectors::{ConnectorError, opttime_endpoint, token_present};
use crate::prelude::*;
use isper_agent::{IsperHost, OptTimeTools, default_or_chosen, local_catalog, system_prompt};
use isper_llm::LlmError;
use isper_llm::agent::{Conversation, Permission, Step, advance, resume};

/// Quanto o catálogo do OptTime lido vale.
const CATALOG_TTL: Duration = Duration::from_secs(10 * 60);
/// Quanto um erro de leitura do catálogo vale (OptTime fora do ar).
const CATALOG_ERROR_TTL: Duration = Duration::from_secs(60);

/// A conversa em andamento. `conv` fica vazio enquanto o agente trabalha.
struct Session {
    id: u64,
    conv: Option<Conversation>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// O catálogo do OptTime lido, ou o erro da última leitura.
static OPTTIME: Mutex<Option<(Instant, Result<OptTimeTools, ConnectorError>)>> = Mutex::new(None);

/// A resposta do agente para a tela.
#[derive(Debug, serde::Serialize)]
pub(crate) struct AssistantReply {
    /// A conversa: o cartão de confirmação devolve este id.
    id: u64,
    /// A resposta (com as fontes) ou as ações esperando o toque.
    step: Step,
    /// O OptTime está conectado, mas não respondeu: a resposta saiu sem ele.
    opttime_error: Option<ConnectorError>,
}

/// Um erro do assistente como a tela mostra.
#[derive(Debug, serde::Serialize)]
pub(crate) struct AssistantError {
    /// `no_provider` · `no_key` · `busy` · `stale` · `failed` ·
    /// `failed_after_action` (a ação rodou; a resposta depois dela falhou).
    code: &'static str,
    message: String,
}

impl AssistantError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn busy() -> Self {
        Self::new("busy", "o assistente ainda está respondendo")
    }

    fn stale() -> Self {
        Self::new("stale", "esta conversa já mudou; pergunte de novo")
    }
}

impl From<LlmError> for AssistantError {
    fn from(e: LlmError) -> Self {
        let code = match e {
            LlmError::NotConfigured => "no_provider",
            LlmError::NoApiKey(_) => "no_key",
            _ => "failed",
        };
        Self::new(code, e.to_string())
    }
}

/// Esquece o catálogo do OptTime (o conector mudou).
pub(crate) fn forget() {
    *OPTTIME.lock_or_recover() = None;
}

/// O conector do OptTime com o catálogo, do cache quando ainda vale. `None`
/// sem token.
fn opttime_tools(app: &AppHandle) -> Option<Result<OptTimeTools, ConnectorError>> {
    if !token_present() {
        return None;
    }
    if let Some((at, tools)) = OPTTIME.lock_or_recover().as_ref() {
        let ttl = if tools.is_ok() {
            CATALOG_TTL
        } else {
            CATALOG_ERROR_TTL
        };
        if at.elapsed() < ttl {
            return Some(tools.clone());
        }
    }
    let handle = tauri::async_runtime::handle().inner().clone();
    let tools = opttime_endpoint(app)
        .and_then(|endpoint| OptTimeTools::connect(endpoint, handle))
        .map_err(|e| ConnectorError::from(&e));
    if let Err(e) = &tools {
        tracing::warn!(code = %e.code, "catálogo do OptTime para o assistente: {}", e.message);
    }
    *OPTTIME.lock_or_recover() = Some((Instant::now(), tools.clone()));
    Some(tools)
}

fn parse_permission(s: &str) -> Option<Permission> {
    match s {
        "allow" => Some(Permission::Allow),
        "ask" => Some(Permission::Ask),
        "never" => Some(Permission::Never),
        _ => None,
    }
}

fn permission_name(p: Permission) -> &'static str {
    match p {
        Permission::Allow => "allow",
        Permission::Ask => "ask",
        Permission::Never => "never",
    }
}

/// As permissões escolhidas nas Configurações.
fn chosen(app: &AppHandle) -> HashMap<String, Permission> {
    app.state::<AppState>()
        .config
        .lock_or_recover()
        .agent_permissions
        .iter()
        .filter_map(|(name, p)| Some((name.clone(), parse_permission(p)?)))
        .collect()
}

/// Uma volta do agente: até a resposta ou até uma ação pedir o toque.
/// `approved` traz os ids aprovados no cartão (`None` = pergunta nova).
fn run(
    app: &AppHandle,
    conv: &mut Conversation,
    approved: Option<&[String]>,
) -> Result<(Step, Option<ConnectorError>), AssistantError> {
    let provider = isper_llm::provider_from_settings(&isper_llm::load_settings())?;
    let (opttime, opttime_error) = match opttime_tools(app) {
        None => (None, None),
        Some(Ok(tools)) => (Some(tools), None),
        Some(Err(e)) => (None, Some(e)),
    };
    // A memória que a pessoa vê e edita nas Configurações (Fase 10.5).
    let memories: Vec<String> = crate::today::open_assist()
        .ok()
        .and_then(|s| s.memories_for_prompt().ok())
        .map(|list| list.into_iter().map(|m| m.text).collect())
        .unwrap_or_default();
    let system = system_prompt(
        crate::connectors::me_first_name().as_deref(),
        chrono::Local::now(),
        opttime.is_some(),
        &memories,
    );
    let db = db_path().map_err(|e| AssistantError::new("failed", e.to_string()))?;
    let host = IsperHost::new(&db, opttime, chosen(app));
    let step = match approved {
        None => advance(provider.as_ref(), &host, &system, conv)?,
        Some(ids) => resume(provider.as_ref(), &host, &system, conv, ids)?,
    };
    Ok((step, opttime_error))
}

/// Roda o agente fora do runtime assíncrono (as ferramentas do OptTime
/// bloqueiam nele) e guarda a conversa de volta, se ela ainda é a atual.
/// Se der errado, `restore` é a conversa que fica (a de antes da pergunta);
/// `None` mantém a de depois (as ações aprovadas já rodaram).
async fn drive(
    app: AppHandle,
    id: u64,
    mut conv: Conversation,
    approved: Option<Vec<String>>,
    restore: Option<Conversation>,
) -> Result<AssistantReply, AssistantError> {
    let worker = app.clone();
    let (conv, result) = tauri::async_runtime::spawn_blocking(move || {
        let result = run(&worker, &mut conv, approved.as_deref());
        (conv, result)
    })
    .await
    .map_err(|e| AssistantError::new("failed", e.to_string()))?;
    {
        let mut slot = SESSION.lock_or_recover();
        if let Some(session) = slot.as_mut().filter(|s| s.id == id) {
            session.conv = Some(match (&result, restore) {
                (Err(_), Some(before)) => before,
                _ => conv,
            });
        }
    }
    // O que o agente criou, moveu ou concluiu aparece na lista.
    let _ = app.emit(crate::today::TASKS_EVENT, ());
    let (step, opttime_error) = result?;
    Ok(AssistantReply {
        id,
        step,
        opttime_error,
    })
}

/// Uma pergunta ao assistente. `fresh` começa uma conversa nova; sem ele, a
/// pergunta segue a conversa em andamento (e um cartão aberto conta como
/// recusado).
#[tauri::command]
pub(crate) async fn assistant_ask(
    app: AppHandle,
    question: String,
    fresh: bool,
) -> Result<AssistantReply, AssistantError> {
    let question = question.trim().to_string();
    if question.is_empty() {
        return Err(AssistantError::new("failed", "pergunta vazia"));
    }
    let (id, before) = {
        let mut slot = SESSION.lock_or_recover();
        match slot.as_mut() {
            Some(session) if !fresh => {
                let before = session.conv.take().ok_or_else(AssistantError::busy)?;
                (session.id, before)
            }
            _ => {
                let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
                *slot = Some(Session { id, conv: None });
                (id, Conversation::default())
            }
        }
    };
    let mut conv = before.clone();
    conv.ask(&question);
    drive(app, id, conv, None, Some(before)).await
}

/// O resumo da manhã (`morning`) ou o fechamento do dia (`closing`), numa
/// conversa nova.
#[tauri::command]
pub(crate) async fn assistant_brief(
    app: AppHandle,
    kind: String,
) -> Result<AssistantReply, AssistantError> {
    let question = match kind.as_str() {
        "morning" => isper_agent::MORNING,
        "closing" => isper_agent::CLOSING,
        other => {
            return Err(AssistantError::new(
                "failed",
                format!("pedido desconhecido: {other}"),
            ));
        }
    };
    assistant_ask(app, question.to_string(), true).await
}

/// O toque no cartão: roda as ações aprovadas (os ids das chamadas), recusa
/// as outras e segue a conversa.
#[tauri::command]
pub(crate) async fn assistant_confirm(
    app: AppHandle,
    id: u64,
    approved: Vec<String>,
) -> Result<AssistantReply, AssistantError> {
    let conv = {
        let mut slot = SESSION.lock_or_recover();
        let session = slot
            .as_mut()
            .filter(|s| s.id == id)
            .ok_or_else(AssistantError::stale)?;
        match &session.conv {
            None => return Err(AssistantError::busy()),
            Some(c) if c.pending.is_empty() => return Err(AssistantError::stale()),
            Some(_) => {}
        }
        session.conv.take().unwrap_or_default()
    };
    let ran = !approved.is_empty();
    drive(app, id, conv, Some(approved), None)
        .await
        .map_err(|e| {
            if ran && e.code == "failed" {
                AssistantError::new("failed_after_action", e.message)
            } else {
                e
            }
        })
}

/// Esquece a conversa ("Nova conversa").
#[tauri::command]
pub(crate) fn assistant_reset() {
    *SESSION.lock_or_recover() = None;
}

/// Uma ferramenta na tela de permissões.
#[derive(Debug, serde::Serialize)]
pub(crate) struct ToolView {
    name: String,
    /// `isper` ou `opttime`.
    group: &'static str,
    /// O título para gente (o OptTime manda; as do ISPer a tela traduz).
    title: Option<String>,
    description: String,
    /// A permissão sem escolha.
    default: Permission,
    /// A escolhida nas Configurações.
    chosen: Option<Permission>,
    /// A que vale: a escolhida, sem liberar o que apaga.
    effective: Permission,
    /// Apaga ou sobrescreve: "liberada" não vale.
    destructive: bool,
}

/// A tela de permissões.
#[derive(Debug, serde::Serialize)]
pub(crate) struct ToolsView {
    tools: Vec<ToolView>,
    /// O OptTime tem token.
    opttime: bool,
    /// Tem token, mas o catálogo não veio.
    opttime_error: Option<ConnectorError>,
}

/// As ferramentas do assistente e o que cada uma pode fazer.
#[tauri::command]
pub(crate) async fn assistant_tools(app: AppHandle) -> Result<ToolsView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let chosen = chosen(&app);
        let mut tools: Vec<ToolView> = local_catalog()
            .into_iter()
            .map(|(spec, default)| {
                let pick = chosen.get(&spec.name).copied();
                ToolView {
                    group: "isper",
                    title: None,
                    description: spec.description,
                    default,
                    chosen: pick,
                    effective: pick.unwrap_or(default),
                    destructive: false,
                    name: spec.name,
                }
            })
            .collect();
        let mut opttime_error = None;
        match opttime_tools(&app) {
            Some(Ok(ot)) => tools.extend(ot.catalog().iter().map(|t| {
                let pick = chosen.get(&t.name).copied();
                ToolView {
                    name: t.name.clone(),
                    group: "opttime",
                    title: t.title.clone(),
                    description: t.description.clone().unwrap_or_default(),
                    default: default_or_chosen(t, None),
                    chosen: pick,
                    effective: default_or_chosen(t, pick),
                    destructive: t.destructive,
                }
            })),
            Some(Err(e)) => opttime_error = Some(e),
            None => {}
        }
        ToolsView {
            tools,
            opttime: token_present(),
            opttime_error,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Muda a permissão de uma ferramenta; `None` (ou `default`) volta ao padrão.
#[tauri::command]
pub(crate) fn assistant_set_permission(
    app: AppHandle,
    name: String,
    permission: Option<String>,
) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty()
        || name.len() > 80
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(format!("nome de ferramenta inválido: {name}"));
    }
    let pick = match permission.as_deref() {
        None | Some("default") => None,
        Some(p) => Some(parse_permission(p).ok_or_else(|| format!("permissão desconhecida: {p}"))?),
    };
    let cfg = {
        let state = app.state::<AppState>();
        let mut c = state.config.lock_or_recover();
        match pick {
            Some(p) => {
                c.agent_permissions
                    .insert(name, permission_name(p).to_string());
            }
            None => {
                c.agent_permissions.remove(&name);
            }
        }
        c.normalize();
        c.clone()
    };
    config::save(&cfg).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissao_vai_e_volta_pelo_nome() {
        for p in [Permission::Allow, Permission::Ask, Permission::Never] {
            assert_eq!(parse_permission(permission_name(p)), Some(p));
        }
        assert_eq!(parse_permission("sempre"), None);
    }

    #[test]
    fn erro_sem_ia_configurada_tem_codigo_proprio() {
        assert_eq!(
            AssistantError::from(LlmError::NotConfigured).code,
            "no_provider"
        );
        assert_eq!(
            AssistantError::from(LlmError::NoApiKey("groq".into())).code,
            "no_key"
        );
        assert_eq!(
            AssistantError::from(LlmError::Http("503".into())).code,
            "failed"
        );
    }
}
