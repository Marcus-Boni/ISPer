//! Implementações do trait [`LlmProvider`] para as APIs suportadas.
//!
//! Todas usam HTTP cru via `ureq` (bloqueante — combina com o design em
//! threads do ISPer; Rust não tem SDK oficial da Anthropic).
//!
//! Os catálogos de modelos mudam rápido (e variam por conta/plano), por isso
//! cada provider sabe **listar seus modelos ao vivo** — o padrão hardcoded é
//! só um ponto de partida.

use std::time::Duration;

use serde_json::{json, Value};

use crate::settings::{get_api_key, LlmSettings};
use crate::{LlmError, Result};

/// Um provider de LLM: recebe system + user, devolve texto.
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn model(&self) -> &str;
    fn complete(&self, system: &str, user: &str) -> Result<String>;
    /// Modelos disponíveis para ESTA chave/conta, direto da API.
    fn list_models(&self) -> Result<Vec<String>>;
}

/// Constrói o provider a partir das configurações salvas + chave do
/// Credential Manager. `Err(NotConfigured)` quando o usuário nunca escolheu.
pub fn provider_from_settings(settings: &LlmSettings) -> Result<Box<dyn LlmProvider>> {
    let name = settings.provider.trim().to_lowercase();
    if name.is_empty() || name == "none" {
        return Err(LlmError::NotConfigured);
    }
    let key = get_api_key(&name)?.ok_or_else(|| LlmError::NoApiKey(name.clone()))?;
    let model = settings.model.clone();
    Ok(match name.as_str() {
        "claude" | "anthropic" => Box::new(Claude {
            api_key: key,
            model: model.unwrap_or_else(|| "claude-opus-5".into()),
        }),
        "groq" => Box::new(Groq {
            api_key: key,
            model: model.unwrap_or_else(|| "openai/gpt-oss-120b".into()),
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

fn agent() -> ureq::Agent {
    ureq::builder().timeout(TIMEOUT).build()
}

fn handle_response(result: std::result::Result<ureq::Response, ureq::Error>) -> Result<Value> {
    match result {
        Ok(resp) => resp
            .into_json()
            .map_err(|e| LlmError::BadResponse(e.to_string())),
        Err(ureq::Error::Status(code, resp)) => {
            let text: String = resp
                .into_string()
                .unwrap_or_default()
                .chars()
                .take(400)
                .collect();
            Err(LlmError::Http(format!("status {code}: {text}")))
        }
        Err(e) => Err(LlmError::Http(e.to_string())),
    }
}

fn post_json(url: &str, headers: &[(&str, &str)], body: Value) -> Result<Value> {
    let mut req = agent().post(url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    handle_response(req.send_json(body))
}

fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<Value> {
    let mut req = agent().get(url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    handle_response(req.call())
}

/// Traduz o "modelo não existe / sem acesso" (que cada API expressa de um
/// jeito) num erro único e acionável.
fn map_model_error(err: LlmError, model: &str) -> LlmError {
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
        let resp = post_json("https://api.anthropic.com/v1/messages", &self.headers(), body)
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

    fn list_models(&self) -> Result<Vec<String>> {
        let resp = get_json("https://api.anthropic.com/v1/models?limit=100", &self.headers())?;
        Ok(ids_from_data(&resp))
    }
}

// ------------------------------------------------------------------ Groq

/// Groq (free tier generoso) — API compatível com OpenAI chat/completions.
pub struct Groq {
    pub api_key: String,
    pub model: String,
}

impl LlmProvider for Groq {
    fn name(&self) -> &'static str {
        "groq"
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
            "https://api.groq.com/openai/v1/chat/completions",
            &[("authorization", auth.as_str())],
            body,
        )
        .map_err(|e| map_model_error(e, &self.model))?;
        resp["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| LlmError::BadResponse("resposta sem texto".into()))
    }

    fn list_models(&self) -> Result<Vec<String>> {
        let auth = format!("Bearer {}", self.api_key);
        let resp = get_json(
            "https://api.groq.com/openai/v1/models",
            &[("authorization", auth.as_str())],
        )?;
        Ok(ids_from_data(&resp))
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
}
