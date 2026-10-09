//! Implementações do trait [`LlmProvider`] para as APIs suportadas.
//!
//! Todas usam HTTP cru via `ureq` (bloqueante — combina com o design em
//! threads do ISPer; Rust não tem SDK oficial da Anthropic).
//!
//! Os catálogos de modelos mudam rápido (e variam por conta/plano), por isso
//! cada provider sabe **listar seus modelos ao vivo** — o padrão hardcoded é
//! só um ponto de partida.

use std::io::{BufRead, BufReader};
use std::time::Duration;

use serde_json::{Value, json};

use crate::chat::{ChatMessage, ChatReply, ToolSpec};
use crate::settings::{LlmSettings, get_api_key};
use crate::{LlmError, Result};

/// Um provider de LLM: recebe system + user, devolve texto.
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn model(&self) -> &str;
    fn complete(&self, system: &str, user: &str) -> Result<String>;
    /// Modelos disponíveis para ESTA chave/conta, direto da API.
    fn list_models(&self) -> Result<Vec<String>>;

    /// Como [`complete`](Self::complete), mas entrega o texto em pedaços
    /// conforme a API os produz — `on_token` é chamado a cada trecho novo e o
    /// texto inteiro volta no fim.
    ///
    /// A implementação padrão simplesmente chama `complete` e entrega tudo de
    /// uma vez. Assim um provider sem streaming continua funcionando e quem
    /// chama nunca precisa saber a diferença; só a sensação muda.
    fn complete_stream(
        &self,
        system: &str,
        user: &str,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String> {
        let text = self.complete(system, user)?;
        on_token(&text);
        Ok(text)
    }

    /// Como [`complete`](Self::complete), mas a resposta é um JSON que segue
    /// `schema` (JSON Schema simples: `object` com `additionalProperties:
    /// false`, `array`, `string`, `integer`, `boolean`, `enum`, `required`,
    /// `description` e campo anulável como `"type": ["string", "null"]`).
    ///
    /// Cada provider usa o modo nativo de saída estruturada da API (Claude
    /// `output_config.format`, Gemini `responseSchema`, Groq `json_schema`).
    /// O padrão, para quem não tem, pede o JSON no texto e o extrai — quem
    /// chama valida o formato do mesmo jeito.
    fn complete_json(&self, system: &str, user: &str, schema: &Value) -> Result<Value> {
        let system = format!(
            "{system}\n\nResponda só com um objeto JSON, sem texto em volta, que siga este JSON Schema:\n{schema}"
        );
        parse_json_text(&self.complete(&system, user)?)
    }

    /// Uma rodada de conversa com ferramentas (Fase 10.4): o modelo responde
    /// ou pede ferramentas; quem chama roda as ferramentas e chama de novo
    /// com os resultados (ver [`crate::agent`]). O padrão é não saber.
    fn chat(
        &self,
        _system: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSpec],
    ) -> Result<ChatReply> {
        Err(LlmError::Unsupported(format!(
            "o provider {} não conversa com ferramentas",
            self.name()
        )))
    }
}

/// O JSON de uma resposta em texto: sem cercas Markdown e sem conversa em
/// volta ("Claro! Aqui está: {…}").
pub(crate) fn parse_json_text(raw: &str) -> Result<Value> {
    let text = crate::copilot::sanitize_json(raw);
    serde_json::from_str(&text)
        .map_err(|e| LlmError::BadResponse(format!("JSON inválido na resposta: {e}")))
}

/// Os campos do esquema que o Gemini aceita (subconjunto OpenAPI). O resto
/// (`additionalProperties`, `$schema`, `format`, `default`, `oneOf`…) sai:
/// um campo desconhecido derruba a chamada inteira.
const GEMINI_SCHEMA_KEYS: [&str; 11] = [
    "type",
    "description",
    "nullable",
    "enum",
    "properties",
    "required",
    "items",
    "minItems",
    "maxItems",
    "minimum",
    "maximum",
];

/// O esquema no dialeto do Gemini (subconjunto OpenAPI), para o
/// `responseSchema` e os parâmetros das ferramentas: tipos em maiúsculas,
/// `["x", "null"]` vira `nullable`, uma união com texto vira texto (o
/// `["integer", "string"]` de um work item do OptTime) e só ficam os campos
/// de [`GEMINI_SCHEMA_KEYS`].
pub(crate) fn gemini_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if !GEMINI_SCHEMA_KEYS.contains(&k.as_str()) {
                    continue;
                }
                match k.as_str() {
                    "type" => match v {
                        Value::Array(types) => {
                            let names: Vec<&str> = types
                                .iter()
                                .filter_map(Value::as_str)
                                .filter(|t| *t != "null")
                                .collect();
                            let main = if names.len() > 1 && names.contains(&"string") {
                                "string"
                            } else {
                                names.first().copied().unwrap_or("string")
                            };
                            out.insert("type".into(), json!(main.to_uppercase()));
                            if types.iter().any(|t| t.as_str() == Some("null")) {
                                out.insert("nullable".into(), json!(true));
                            }
                        }
                        Value::String(t) => {
                            out.insert("type".into(), json!(t.to_uppercase()));
                        }
                        other => {
                            out.insert("type".into(), other.clone());
                        }
                    },
                    "properties" => {
                        let props = v
                            .as_object()
                            .map(|p| {
                                p.iter()
                                    .map(|(name, s)| (name.clone(), gemini_schema(s)))
                                    .collect::<serde_json::Map<_, _>>()
                            })
                            .unwrap_or_default();
                        out.insert("properties".into(), Value::Object(props));
                    }
                    "items" => {
                        out.insert("items".into(), gemini_schema(v));
                    }
                    _ => {
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Constrói o provider a partir das configurações salvas + chave do
/// Credential Manager. `Err(NotConfigured)` quando o usuário nunca escolheu.
pub fn provider_from_settings(settings: &LlmSettings) -> Result<Box<dyn LlmProvider>> {
    let name = settings.provider.trim().to_lowercase();
    if name.is_empty() || name == "none" {
        return Err(LlmError::NotConfigured);
    }
    let model = settings.model.clone();
    // Compatível com OpenAI (Ollama, LM Studio, um servidor da empresa): o
    // endereço vem das configurações e a chave é opcional (o Ollama local não
    // usa).
    if name == "openai" {
        let base = settings
            .base_url
            .as_deref()
            .map(|u| u.trim().trim_end_matches('/'))
            .filter(|u| !u.is_empty())
            .ok_or_else(|| {
                LlmError::BadResponse("falta o endereço da API compatível com OpenAI".into())
            })?;
        return Ok(Box::new(Groq {
            api_key: get_api_key("openai")?.unwrap_or_default(),
            model: model.unwrap_or_else(|| "llama3.1".into()),
            base: base.to_string(),
            label: "openai",
        }));
    }
    let key = get_api_key(&name)?.ok_or_else(|| LlmError::NoApiKey(name.clone()))?;
    Ok(match name.as_str() {
        "claude" | "anthropic" => Box::new(Claude {
            api_key: key,
            model: model.unwrap_or_else(|| "claude-opus-5".into()),
        }),
        "groq" => Box::new(Groq {
            api_key: key,
            model: model.unwrap_or_else(|| "openai/gpt-oss-120b".into()),
            base: GROQ_BASE.into(),
            label: "groq",
        }),
        "gemini" | "google" => Box::new(Gemini {
            api_key: key,
            model: model.unwrap_or_else(|| "gemini-3.5-flash-lite".into()),
        }),
        other => return Err(LlmError::UnknownProvider(other.to_string())),
    })
}

const TIMEOUT: Duration = Duration::from_secs(120);
const MAX_TOKENS: u32 = 4096;
/// Quanto do corpo de um erro da API entra na mensagem (o resto só poluiria o log).
const ERROR_BODY_CHARS: usize = 400;

/// Agente HTTP (ureq 3): timeout global por chamada e 4xx/5xx tratados como
/// resposta normal — a API explica o erro no corpo ("model_not_found"…) e é
/// isso que queremos mostrar, não só o número.
fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .build(),
    )
}

type HttpResult = std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>;

fn handle_response(result: HttpResult) -> Result<Value> {
    let mut resp = result.map_err(|e| LlmError::Http(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        let text: String = resp
            .body_mut()
            .read_to_string()
            .unwrap_or_default()
            .chars()
            .take(ERROR_BODY_CHARS)
            .collect();
        return Err(LlmError::Http(format!(
            "status {}: {text}",
            status.as_u16()
        )));
    }
    resp.body_mut()
        .read_json::<Value>()
        .map_err(|e| LlmError::BadResponse(e.to_string()))
}

pub(crate) fn post_json(url: &str, headers: &[(&str, &str)], body: Value) -> Result<Value> {
    let mut req = agent().post(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    handle_response(req.send_json(&body))
}

/// Teto do corpo de um streaming (bytes). O `as_reader` do ureq é ilimitado
/// por padrão: sem isto, um servidor que não para de falar consome a memória
/// toda da máquina.
const MAX_STREAM_BYTES: u64 = 32 * 1024 * 1024;

/// POST que lê a resposta como `text/event-stream`, entregando cada evento a
/// `on_event` como `(nome, dados)` — o nome é `""` quando a API não manda um
/// `event:` (o caso de OpenAI/Groq e Gemini).
///
/// Só o suficiente do protocolo SSE para estas APIs: linhas `event:` e
/// `data:`, `data: [DONE]` encerra, comentários (`:`) e campos que não usamos
/// (`id:`, `retry:`) são ignorados. `data:` com JSON inválido é pulado — um
/// keep-alive no meio do fluxo não pode derrubar a resposta inteira.
fn post_sse(
    url: &str,
    headers: &[(&str, &str)],
    body: Value,
    on_event: &mut dyn FnMut(&str, &Value) -> Result<()>,
) -> Result<()> {
    let mut req = agent().post(url).header("accept", "text/event-stream");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut resp = req
        .send_json(&body)
        .map_err(|e| LlmError::Http(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        // Erro vem como JSON comum, não como stream: mesma mensagem do
        // caminho não-streaming, para o usuário ver o motivo de sempre.
        let text: String = resp
            .body_mut()
            .read_to_string()
            .unwrap_or_default()
            .chars()
            .take(ERROR_BODY_CHARS)
            .collect();
        return Err(LlmError::Http(format!(
            "status {}: {text}",
            status.as_u16()
        )));
    }

    let reader = resp
        .body_mut()
        .with_config()
        .limit(MAX_STREAM_BYTES)
        .reader();
    read_sse(BufReader::new(reader), on_event)
}

/// O protocolo SSE em si, separado do HTTP para poder ser testado com bytes
/// canônicos de cada API (ver os testes no fim do arquivo).
fn read_sse(
    reader: impl BufRead,
    on_event: &mut dyn FnMut(&str, &Value) -> Result<()>,
) -> Result<()> {
    let mut event = String::new();
    for line in reader.lines() {
        let line = line.map_err(|e| LlmError::Http(format!("stream interrompido: {e}")))?;
        let line = line.trim_end();

        if line.is_empty() {
            event.clear(); // fim do evento: o próximo `data:` começa limpo
            continue;
        }
        if let Some(name) = line.strip_prefix("event:") {
            event = name.trim().to_string();
            continue;
        }
        let Some(data) = line.strip_prefix("data:") else {
            continue; // comentário (`:`), `id:`, `retry:` — nada que usemos
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            break;
        }
        let Ok(val) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        on_event(&event, &val)?;
    }
    Ok(())
}

fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<Value> {
    let mut req = agent().get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    handle_response(req.call())
}

/// Traduz o "modelo não existe / sem acesso" (que cada API expressa de um
/// jeito) num erro único e acionável.
pub(crate) fn map_model_error(err: LlmError, model: &str) -> LlmError {
    match &err {
        LlmError::Http(msg)
            if msg.contains("model_not_found")
                || msg.contains("not_found_error")
                || (msg.contains("status 404") && msg.to_lowercase().contains("model")) =>
        {
            LlmError::ModelNotFound(model.to_string())
        }
        _ => err,
    }
}

/// Extrai `data[].id` (formato OpenAI/Anthropic) em ordem alfabética.
fn ids_from_data(resp: &Value) -> Vec<String> {
    let mut ids: Vec<String> = resp["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
}

/// O que um evento do streaming da Anthropic significa para nós.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ClaudeEvent {
    /// Pedaço novo de texto da resposta.
    Text(String),
    /// Filtro de segurança recusou (categoria).
    Refusal(String),
    /// A própria API reportou erro no meio do fluxo.
    Error(String),
    /// Começo/fim de bloco, ping, uso de tokens — nada a fazer.
    Ignored,
}

/// Traduz um evento SSE da Anthropic. Blocos que não são texto (`thinking`,
/// `tool_use`) não entram na resposta.
pub(crate) fn claude_event(event: &str, val: &Value) -> ClaudeEvent {
    match event {
        "content_block_delta" if val["delta"]["type"] == "text_delta" => {
            match val["delta"]["text"].as_str() {
                Some(t) if !t.is_empty() => ClaudeEvent::Text(t.to_string()),
                _ => ClaudeEvent::Ignored,
            }
        }
        "message_delta" if val["delta"]["stop_reason"].as_str() == Some("refusal") => {
            ClaudeEvent::Refusal(
                val["delta"]["stop_details"]["category"]
                    .as_str()
                    .unwrap_or("sem categoria")
                    .to_string(),
            )
        }
        "error" => ClaudeEvent::Error(
            val["error"]["message"]
                .as_str()
                .unwrap_or("erro sem mensagem")
                .to_string(),
        ),
        _ => ClaudeEvent::Ignored,
    }
}

/// Pedaço de texto num chunk no formato OpenAI (Groq e compatíveis).
pub(crate) fn openai_delta(val: &Value) -> Option<&str> {
    val["choices"][0]["delta"]["content"]
        .as_str()
        .filter(|t| !t.is_empty())
}

/// Pedaços de texto num chunk do Gemini (um candidato pode trazer várias parts).
pub(crate) fn gemini_deltas(val: &Value) -> Vec<&str> {
    val["candidates"][0]["content"]["parts"]
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------- Claude

/// Claude API (Anthropic) — https://api.anthropic.com/v1/messages
pub struct Claude {
    pub api_key: String,
    pub model: String,
}

impl Claude {
    fn headers(&self) -> [(&str, &str); 3] {
        [
            ("x-api-key", self.api_key.as_str()),
            ("anthropic-version", "2023-06-01"),
            ("anthropic-beta", "server-side-fallback-2026-07-01"),
        ]
    }
}

impl LlmProvider for Claude {
    fn name(&self) -> &'static str {
        "claude"
    }
    fn model(&self) -> &str {
        &self.model
    }

    fn complete(&self, system: &str, user: &str) -> Result<String> {
        // `fallbacks: "default"` (beta server-side-fallback): se um filtro de
        // segurança recusar, a própria API redireciona p/ um modelo adequado.
        let body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": system,
            "messages": [{"role": "user", "content": user}],
            "fallbacks": "default",
        });
        let resp = post_json(
            "https://api.anthropic.com/v1/messages",
            &self.headers(),
            body,
        )
        .map_err(|e| map_model_error(e, &self.model))?;

        // Sempre checar stop_reason antes de ler o conteúdo.
        if resp["stop_reason"].as_str() == Some("refusal") {
            let cat = resp["stop_details"]["category"]
                .as_str()
                .unwrap_or("sem categoria");
            return Err(LlmError::Refused(cat.to_string()));
        }
        let text: String = resp["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"].as_str() == Some("text"))
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if text.is_empty() {
            return Err(LlmError::BadResponse("resposta sem texto".into()));
        }
        Ok(text)
    }

    fn complete_json(&self, system: &str, user: &str, schema: &Value) -> Result<Value> {
        // Saída estruturada nativa: a API garante um JSON que segue o esquema.
        let body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": system,
            "messages": [{"role": "user", "content": user}],
            "output_config": {"format": {"type": "json_schema", "schema": schema}},
            "fallbacks": "default",
        });
        let resp = post_json(
            "https://api.anthropic.com/v1/messages",
            &self.headers(),
            body,
        )
        .map_err(|e| map_model_error(e, &self.model))?;
        match resp["stop_reason"].as_str() {
            Some("refusal") => {
                let cat = resp["stop_details"]["category"]
                    .as_str()
                    .unwrap_or("sem categoria");
                return Err(LlmError::Refused(cat.to_string()));
            }
            Some("max_tokens") => {
                return Err(LlmError::BadResponse(
                    "a resposta estourou o limite de tokens".into(),
                ));
            }
            _ => {}
        }
        let text = resp["content"]
            .as_array()
            .and_then(|blocks| {
                blocks
                    .iter()
                    .find(|b| b["type"].as_str() == Some("text"))
                    .and_then(|b| b["text"].as_str())
            })
            .ok_or_else(|| LlmError::BadResponse("resposta sem texto".into()))?;
        parse_json_text(text)
    }

    fn list_models(&self) -> Result<Vec<String>> {
        let resp = get_json(
            "https://api.anthropic.com/v1/models?limit=100",
            &self.headers(),
        )?;
        Ok(ids_from_data(&resp))
    }

    fn chat(
        &self,
        system: &str,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
    ) -> Result<ChatReply> {
        // Sem `fallbacks`: o pensamento fica amarrado ao modelo e à conversa,
        // e trocar de modelo no meio do laço o invalidaria.
        let body = crate::chat::claude_body(&self.model, system, messages, tools);
        let resp = post_json(
            "https://api.anthropic.com/v1/messages",
            &[
                ("x-api-key", self.api_key.as_str()),
                ("anthropic-version", "2023-06-01"),
            ],
            body,
        )
        .map_err(|e| map_model_error(e, &self.model))?;
        crate::chat::claude_reply(&resp)
    }

    fn complete_stream(
        &self,
        system: &str,
        user: &str,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String> {
        let body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": system,
            "messages": [{"role": "user", "content": user}],
            "fallbacks": "default",
            "stream": true,
        });

        let mut text = String::new();
        let mut refusal: Option<String> = None;
        let mut api_error: Option<String> = None;

        post_sse(
            "https://api.anthropic.com/v1/messages",
            &self.headers(),
            body,
            &mut |event, val| {
                match claude_event(event, val) {
                    ClaudeEvent::Text(chunk) => {
                        text.push_str(&chunk);
                        on_token(&chunk);
                    }
                    // Mesma checagem do caminho não-streaming: recusa por
                    // filtro de segurança vem aqui, não como erro HTTP.
                    ClaudeEvent::Refusal(cat) => refusal = Some(cat),
                    ClaudeEvent::Error(msg) => api_error = Some(msg),
                    ClaudeEvent::Ignored => {}
                }
                Ok(())
            },
        )
        .map_err(|e| map_model_error(e, &self.model))?;

        if let Some(cat) = refusal {
            return Err(LlmError::Refused(cat));
        }
        if let Some(msg) = api_error {
            return Err(LlmError::Http(msg));
        }
        if text.is_empty() {
            return Err(LlmError::BadResponse("resposta sem texto".into()));
        }
        Ok(text)
    }
}

// ------------------------------------------------------------------ Groq

/// O endereço da API da Groq.
const GROQ_BASE: &str = "https://api.groq.com/openai/v1";

/// Groq (free tier generoso) e qualquer API compatível com OpenAI
/// chat/completions (Ollama, LM Studio): muda só o endereço e a chave.
pub struct Groq {
    pub api_key: String,
    pub model: String,
    /// A base da API, sem a barra do fim (`…/v1`).
    pub base: String,
    /// `groq` ou `openai`, o nome que aparece para a pessoa.
    pub label: &'static str,
}

impl Groq {
    /// O cabeçalho de autorização, quando há chave.
    fn auth<'a>(&self, bearer: &'a str) -> Vec<(&'static str, &'a str)> {
        if self.api_key.is_empty() {
            Vec::new()
        } else {
            vec![("authorization", bearer)]
        }
    }
}

impl LlmProvider for Groq {
    fn name(&self) -> &'static str {
        self.label
    }
    fn model(&self) -> &str {
        &self.model
    }

    fn complete(&self, system: &str, user: &str) -> Result<String> {
        let body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        });
        let auth = format!("Bearer {}", self.api_key);
        let resp = post_json(
            &format!("{}/chat/completions", self.base),
            &self.auth(&auth),
            body,
        )
        .map_err(|e| map_model_error(e, &self.model))?;
        resp["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| LlmError::BadResponse("resposta sem texto".into()))
    }

    fn complete_json(&self, system: &str, user: &str, schema: &Value) -> Result<Value> {
        let body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": {"name": "resposta", "strict": true, "schema": schema},
            },
        });
        let auth = format!("Bearer {}", self.api_key);
        match post_json(
            &format!("{}/chat/completions", self.base),
            &self.auth(&auth),
            body,
        ) {
            Ok(resp) => parse_json_text(
                resp["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or_default(),
            ),
            // Nem todo modelo da Groq aceita json_schema: cai no modo de texto.
            Err(LlmError::Http(msg)) if msg.contains("status 400") => {
                let system = format!(
                    "{system}\n\nResponda só com um objeto JSON, sem texto em volta, que siga este JSON Schema:\n{schema}"
                );
                parse_json_text(&self.complete(&system, user)?)
            }
            Err(e) => Err(map_model_error(e, &self.model)),
        }
    }

    fn list_models(&self) -> Result<Vec<String>> {
        let auth = format!("Bearer {}", self.api_key);
        let resp = get_json(&format!("{}/models", self.base), &self.auth(&auth))?;
        Ok(ids_from_data(&resp))
    }

    fn chat(
        &self,
        system: &str,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
    ) -> Result<ChatReply> {
        let auth = format!("Bearer {}", self.api_key);
        let resp = post_json(
            &format!("{}/chat/completions", self.base),
            &self.auth(&auth),
            crate::chat::openai_body(&self.model, system, messages, tools),
        )
        .map_err(|e| map_model_error(e, &self.model))?;
        crate::chat::openai_reply(&resp)
    }

    fn complete_stream(
        &self,
        system: &str,
        user: &str,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String> {
        let body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "stream": true,
        });
        let auth = format!("Bearer {}", self.api_key);

        let mut text = String::new();
        post_sse(
            &format!("{}/chat/completions", self.base),
            &self.auth(&auth),
            body,
            &mut |_event, val| {
                if let Some(chunk) = openai_delta(val) {
                    text.push_str(chunk);
                    on_token(chunk);
                }
                Ok(())
            },
        )
        .map_err(|e| map_model_error(e, &self.model))?;

        if text.is_empty() {
            return Err(LlmError::BadResponse("resposta sem texto".into()));
        }
        Ok(text)
    }
}

// ---------------------------------------------------------------- Gemini

/// Google Gemini (free tier) — generateContent. A chave vai em HEADER
/// (x-goog-api-key), nunca na URL.
pub struct Gemini {
    pub api_key: String,
    pub model: String,
}

impl LlmProvider for Gemini {
    fn name(&self) -> &'static str {
        "gemini"
    }
    fn model(&self) -> &str {
        &self.model
    }

    fn complete(&self, system: &str, user: &str) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );
        let body = json!({
            "system_instruction": {"parts": [{"text": system}]},
            "contents": [{"role": "user", "parts": [{"text": user}]}],
            "generationConfig": {"maxOutputTokens": MAX_TOKENS},
        });
        let resp = post_json(&url, &[("x-goog-api-key", self.api_key.as_str())], body)
            .map_err(|e| map_model_error(e, &self.model))?;
        resp["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| LlmError::BadResponse("resposta sem texto".into()))
    }

    fn complete_json(&self, system: &str, user: &str, schema: &Value) -> Result<Value> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );
        let mut config = json!({
            "maxOutputTokens": MAX_TOKENS,
            "responseMimeType": "application/json",
            "responseSchema": gemini_schema(schema),
        });
        // Extração curta: os Gemini 3 pensam o mínimo e respondem mais rápido.
        if self.model.starts_with("gemini-3") {
            config["thinkingConfig"] = json!({"thinkingLevel": "minimal"});
        }
        let body = |config: &Value| {
            json!({
                "system_instruction": {"parts": [{"text": system}]},
                "contents": [{"role": "user", "parts": [{"text": user}]}],
                "generationConfig": config,
            })
        };
        let key = [("x-goog-api-key", self.api_key.as_str())];
        let resp = match post_json(&url, &key, body(&config)) {
            // Um modelo que não aceite o nível de raciocínio não trava a
            // captura: tenta de novo sem ele.
            Err(LlmError::Http(msg)) if msg.contains("status 400") && msg.contains("hinking") => {
                if let Some(c) = config.as_object_mut() {
                    c.remove("thinkingConfig");
                }
                post_json(&url, &key, body(&config))
            }
            other => other,
        }
        .map_err(|e| map_model_error(e, &self.model))?;
        // Com raciocínio, pode haver mais de uma parte: vale a de texto final.
        let text = resp["candidates"][0]["content"]["parts"]
            .as_array()
            .and_then(|parts| {
                parts
                    .iter()
                    .filter(|p| p["thought"].as_bool() != Some(true))
                    .find_map(|p| p["text"].as_str())
            })
            .ok_or_else(|| LlmError::BadResponse("resposta sem texto".into()))?;
        parse_json_text(text)
    }

    fn chat(
        &self,
        system: &str,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
    ) -> Result<ChatReply> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );
        let mut body = crate::chat::gemini_body(system, messages, tools);
        // No laço do agente, os Gemini 3 pensam pouco: o padrão (alto) deixa
        // cada rodada lenta demais para uma pergunta sobre o dia.
        if self.model.starts_with("gemini-3") {
            body["generationConfig"]["thinkingConfig"] = json!({"thinkingLevel": "low"});
        }
        let key = [("x-goog-api-key", self.api_key.as_str())];
        let resp = match post_json(&url, &key, body.clone()) {
            Err(LlmError::Http(msg)) if msg.contains("status 400") && msg.contains("hinking") => {
                if let Some(c) = body["generationConfig"].as_object_mut() {
                    c.remove("thinkingConfig");
                }
                post_json(&url, &key, body)
            }
            other => other,
        }
        .map_err(|e| map_model_error(e, &self.model))?;
        crate::chat::gemini_reply(&resp)
    }

    fn list_models(&self) -> Result<Vec<String>> {
        let resp = get_json(
            "https://generativelanguage.googleapis.com/v1beta/models?pageSize=200",
            &[("x-goog-api-key", self.api_key.as_str())],
        )?;
        // Só os que aceitam generateContent; sem o prefixo "models/".
        let mut ids: Vec<String> = resp["models"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter(|m| {
                        m["supportedGenerationMethods"]
                            .as_array()
                            .map(|ms| ms.iter().any(|x| x.as_str() == Some("generateContent")))
                            .unwrap_or(false)
                    })
                    .filter_map(|m| m["name"].as_str())
                    .map(|n| n.trim_start_matches("models/").to_string())
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        Ok(ids)
    }

    fn complete_stream(
        &self,
        system: &str,
        user: &str,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String> {
        // `alt=sse` troca o JSON em fatias pelo formato de eventos; sem ele a
        // resposta vem como um array e não dá para ler incremental.
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
            self.model
        );
        let body = json!({
            "system_instruction": {"parts": [{"text": system}]},
            "contents": [{"role": "user", "parts": [{"text": user}]}],
            "generationConfig": {"maxOutputTokens": MAX_TOKENS},
        });

        let mut text = String::new();
        post_sse(
            &url,
            &[("x-goog-api-key", self.api_key.as_str())],
            body,
            &mut |_event, val| {
                for chunk in gemini_deltas(val) {
                    text.push_str(chunk);
                    on_token(chunk);
                }
                Ok(())
            },
        )
        .map_err(|e| map_model_error(e, &self.model))?;

        if text.is_empty() {
            return Err(LlmError::BadResponse("resposta sem texto".into()));
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compativel_com_openai_sem_chave_nao_manda_autorizacao() {
        let local = Groq {
            api_key: String::new(),
            model: "llama3.1".into(),
            base: "http://localhost:11434/v1".into(),
            label: "openai",
        };
        assert!(local.auth("Bearer ").is_empty());
        assert_eq!(local.name(), "openai");
        let groq = Groq {
            api_key: "k".into(),
            base: GROQ_BASE.into(),
            label: "groq",
            ..local
        };
        assert_eq!(groq.auth("Bearer k"), [("authorization", "Bearer k")]);
    }

    #[test]
    fn compativel_com_openai_precisa_do_endereco() {
        let settings = LlmSettings {
            provider: "openai".into(),
            model: None,
            base_url: Some("  ".into()),
            embeddings: Default::default(),
        };
        let Err(e) = provider_from_settings(&settings) else {
            panic!("sem endereço não há provider");
        };
        assert!(e.to_string().contains("endereço"));
    }

    /// Junta o texto de um fluxo SSE do jeito que cada provider o produz.
    fn collect(body: &str, mut pick: impl FnMut(&str, &Value) -> Vec<String>) -> Vec<String> {
        let mut out = Vec::new();
        read_sse(BufReader::new(body.as_bytes()), &mut |ev, val| {
            out.extend(pick(ev, val));
            Ok(())
        })
        .expect("fluxo bem formado");
        out
    }

    #[test]
    fn sse_le_eventos_e_para_no_done() {
        // Formato do Groq/OpenAI, com o keep-alive e o [DONE] finais.
        let body = concat!(
            ": keep-alive\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Olá\"}}]}\n",
            "\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" mundo\"}}]}\n",
            "\n",
            "data: [DONE]\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"depois do fim\"}}]}\n",
        );
        let got = collect(body, |_, v| {
            openai_delta(v).map(str::to_string).into_iter().collect()
        });
        assert_eq!(got, vec!["Olá", " mundo"], "[DONE] encerra o fluxo");
    }

    #[test]
    fn sse_ignora_linha_torta_sem_derrubar_o_fluxo() {
        // Um `data:` inválido (keep-alive de proxy, JSON cortado) não pode
        // fazer a resposta inteira falhar.
        let body = concat!(
            "data: {nao é json}\n",
            "\n",
            "id: 42\n",
            "retry: 1000\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"vale\"}}]}\n",
            "\n",
        );
        let got = collect(body, |_, v| {
            openai_delta(v).map(str::to_string).into_iter().collect()
        });
        assert_eq!(got, vec!["vale"]);
    }

    #[test]
    fn sse_associa_o_nome_do_evento_ao_data_seguinte() {
        let body = concat!(
            "event: primeiro\n",
            "data: {}\n",
            "\n",
            "data: {}\n", // sem `event:` — a linha em branco limpou o anterior
            "\n",
            "event: terceiro\n",
            "data: {}\n",
            "\n",
        );
        let mut nomes = Vec::new();
        read_sse(BufReader::new(body.as_bytes()), &mut |ev, _| {
            nomes.push(ev.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(nomes, vec!["primeiro", "", "terceiro"]);
    }

    #[test]
    fn claude_junta_so_os_deltas_de_texto() {
        let body = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\"}\n",
            "\n",
            "event: content_block_delta\n",
            "data: {\"delta\":{\"type\":\"text_delta\",\"text\":\"Bom \"}}\n",
            "\n",
            "event: content_block_delta\n",
            "data: {\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hmm\"}}\n",
            "\n",
            "event: content_block_delta\n",
            "data: {\"delta\":{\"type\":\"text_delta\",\"text\":\"dia\"}}\n",
            "\n",
            "event: message_stop\n",
            "data: {}\n",
            "\n",
        );
        let got = collect(body, |ev, v| match claude_event(ev, v) {
            ClaudeEvent::Text(t) => vec![t],
            _ => vec![],
        });
        assert_eq!(got.concat(), "Bom dia", "raciocínio não entra na resposta");
    }

    #[test]
    fn claude_reconhece_recusa_e_erro_no_meio_do_fluxo() {
        let refusal = serde_json::json!({
            "delta": {"stop_reason": "refusal", "stop_details": {"category": "violencia"}}
        });
        assert_eq!(
            claude_event("message_delta", &refusal),
            ClaudeEvent::Refusal("violencia".into())
        );

        let err = serde_json::json!({"error": {"message": "overloaded_error"}});
        assert_eq!(
            claude_event("error", &err),
            ClaudeEvent::Error("overloaded_error".into())
        );

        // Fim normal não é recusa.
        let fim = serde_json::json!({"delta": {"stop_reason": "end_turn"}});
        assert_eq!(claude_event("message_delta", &fim), ClaudeEvent::Ignored);
    }

    #[test]
    fn gemini_junta_todas_as_parts_do_candidato() {
        let body = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"um \"},{\"text\":\"dois\"}]}}]}\n",
            "\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" três\"}]}}]}\n",
            "\n",
        );
        let got = collect(body, |_, v| {
            gemini_deltas(v).into_iter().map(str::to_string).collect()
        });
        assert_eq!(got.concat(), "um dois três");
    }

    #[test]
    fn deltas_vazios_nao_viram_token() {
        // Chunk de abertura do OpenAI traz `content: ""`; emitir isso faria a
        // tela piscar sem motivo.
        assert_eq!(
            openai_delta(&serde_json::json!({"choices":[{"delta":{"content":""}}]})),
            None
        );
        assert_eq!(
            openai_delta(&serde_json::json!({"choices":[{"delta":{"role":"assistant"}}]})),
            None
        );
        assert!(gemini_deltas(&serde_json::json!({"candidates":[]})).is_empty());
    }

    #[test]
    fn provider_sem_streaming_cai_no_complete() {
        // A implementação padrão do trait: entrega tudo de uma vez, e quem
        // chama não precisa saber que não houve streaming de verdade.
        struct Simples;
        impl LlmProvider for Simples {
            fn name(&self) -> &'static str {
                "simples"
            }
            fn model(&self) -> &str {
                "m"
            }
            fn complete(&self, _: &str, _: &str) -> Result<String> {
                Ok("resposta inteira".into())
            }
            fn list_models(&self) -> Result<Vec<String>> {
                Ok(vec![])
            }
        }
        let mut pedacos = Vec::new();
        let full = Simples
            .complete_stream("s", "u", &mut |t| pedacos.push(t.to_string()))
            .unwrap();
        assert_eq!(full, "resposta inteira");
        assert_eq!(pedacos, vec!["resposta inteira"]);
    }
}
