//! Conectores MCP (Fase 10.2, [ADR 0022]): por enquanto, o OptTime.
//!
//! A tela do conector mora nas Configurações. Ela guarda o token no
//! Credential Manager (`opttime.ISPer`; no perfil de teste, o cofre dele),
//! nunca em arquivo, e confere a conexão pelo `opt_time_whoami`: quem é,
//! que escopos o token tem, se a Microsoft e o Azure DevOps estão
//! conectados e se o aviso das 17:30 do Teams está ligado (com a rotina das
//! 8 horas ligada, ele duplicaria o lembrete — [ADR 0023]).
//!
//! [ADR 0022]: ../../../../docs/adr/0022-conectores-mcp-e-opttime.md
//! [ADR 0023]: ../../../../docs/adr/0023-escada-de-confianca.md

use crate::prelude::*;
use isper_mcp::opttime::{DEFAULT_URL, SECRET_NAME};
use isper_mcp::{Endpoint, McpError, OptTime, Whoami};

/// Evento que avisa as telas de que o conector mudou (token, URL ou a
/// última conferência).
pub(crate) const CONNECTOR_EVENT: &str = "isper-connector";

/// A última conferência, para a tela mostrar sem consultar de novo.
static LAST: Mutex<Option<OptTimeCheck>> = Mutex::new(None);

/// Um erro do conector como a interface mostra: código, mensagem e dica.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ConnectorError {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) hint: Option<String>,
}

impl From<&McpError> for ConnectorError {
    fn from(e: &McpError) -> Self {
        Self {
            code: e.code(),
            message: match e {
                McpError::Tool(t) => t.message.clone(),
                other => other.to_string(),
            },
            hint: e.hint().map(String::from),
        }
    }
}

/// O resultado de "Testar conexão".
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct OptTimeCheck {
    at_ms: i64,
    ok: bool,
    whoami: Option<Whoami>,
    missing_scopes: Vec<&'static str>,
    error: Option<ConnectorError>,
}

/// O que a tela do conector mostra.
#[derive(Debug, serde::Serialize)]
pub(crate) struct OptTimeStatus {
    url: String,
    default_url: &'static str,
    token_present: bool,
    last: Option<OptTimeCheck>,
}

/// O endereço em uso: o das Configurações ou o de produção.
pub(crate) fn opttime_url(app: &AppHandle) -> String {
    app.state::<AppState>()
        .config
        .lock_or_recover()
        .opttime_url
        .clone()
        .unwrap_or_else(|| DEFAULT_URL.to_string())
}

/// Se há token guardado (sem ler o token para a tela).
pub(crate) fn token_present() -> bool {
    isper_llm::get_api_key(SECRET_NAME).ok().flatten().is_some()
}

/// O conector pronto para chamar, com o token do cofre.
pub(crate) fn opttime(app: &AppHandle) -> Result<OptTime, McpError> {
    let token = isper_llm::get_api_key(SECRET_NAME)
        .map_err(|e| McpError::Transport(format!("cofre de senhas: {e}")))?
        .ok_or(McpError::NoToken)?;
    Ok(OptTime::new(Endpoint::new(&opttime_url(app), &token)?))
}

/// O primeiro nome de quem é dono do token, da última conferência (é por ele
/// que os outros pedem coisas numa reunião).
pub(crate) fn me_first_name() -> Option<String> {
    let last = LAST.lock_or_recover();
    let name = last.as_ref()?.whoami.as_ref()?.name.clone();
    name.split(|c: char| c.is_whitespace() || c == '|')
        .find(|w| !w.is_empty())
        .map(String::from)
}

fn changed(app: &AppHandle) {
    crate::agenda::forget();
    let _ = app.emit(CONNECTOR_EVENT, ());
    // A tela Hoje mostra "conecte o OptTime" nas rotinas que dependem dele.
    let _ = app.emit(crate::today::TASKS_EVENT, ());
}

#[tauri::command]
pub(crate) fn opttime_status(app: AppHandle) -> OptTimeStatus {
    OptTimeStatus {
        url: opttime_url(&app),
        default_url: DEFAULT_URL,
        token_present: token_present(),
        last: LAST.lock_or_recover().clone(),
    }
}

/// Guarda o token. Vem colado da tela do OptTime: espaço ou quebra de linha
/// no meio é sinal de cópia errada, e é recusado em vez de salvo.
#[tauri::command]
pub(crate) fn opttime_set_token(app: AppHandle, token: String) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return Err(crate::i18n::tr(&app, "connectors.err.empty-token"));
    }
    if token.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(crate::i18n::tr(&app, "connectors.err.bad-token"));
    }
    isper_llm::set_api_key(SECRET_NAME, token).map_err(|e| e.to_string())?;
    *LAST.lock_or_recover() = None;
    changed(&app);
    Ok(())
}

/// Tira o token do cofre.
#[tauri::command]
pub(crate) fn opttime_clear_token(app: AppHandle) -> Result<(), String> {
    isper_llm::delete_api_key(SECRET_NAME).map_err(|e| e.to_string())?;
    *LAST.lock_or_recover() = None;
    changed(&app);
    Ok(())
}

/// Muda o endereço (vazio volta ao de produção). Só `https`, ou `http` no
/// próprio PC: o token não viaja em texto aberto.
#[tauri::command]
pub(crate) fn opttime_set_url(app: AppHandle, url: Option<String>) -> Result<String, String> {
    let url = url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
    let normalized = match &url {
        Some(u) => Some(
            Endpoint::new(u, "conferencia")
                .map_err(|e| e.to_string())?
                .url()
                .to_string(),
        ),
        None => None,
    };
    let cfg = {
        let state = app.state::<AppState>();
        let mut c = state.config.lock_or_recover();
        c.opttime_url = normalized.filter(|u| u != DEFAULT_URL);
        c.normalize();
        c.clone()
    };
    config::save(&cfg).map_err(|e| e.to_string())?;
    *LAST.lock_or_recover() = None;
    changed(&app);
    Ok(opttime_url(&app))
}

/// "Testar conexão": o `whoami` do OptTime.
#[tauri::command]
pub(crate) async fn opttime_check(app: AppHandle) -> OptTimeCheck {
    let result = match opttime(&app) {
        Ok(ot) => ot.whoami().await,
        Err(e) => Err(e),
    };
    let check = match result {
        Ok(me) => OptTimeCheck {
            at_ms: chrono::Utc::now().timestamp_millis(),
            ok: true,
            missing_scopes: me.missing_scopes(),
            whoami: Some(me),
            error: None,
        },
        Err(e) => {
            tracing::warn!(code = %e.code(), "conector do OptTime: {e}");
            OptTimeCheck {
                at_ms: chrono::Utc::now().timestamp_millis(),
                ok: false,
                whoami: None,
                missing_scopes: Vec::new(),
                error: Some(ConnectorError::from(&e)),
            }
        }
    };
    *LAST.lock_or_recover() = Some(check.clone());
    let _ = app.emit(CONNECTOR_EVENT, ());
    check
}
