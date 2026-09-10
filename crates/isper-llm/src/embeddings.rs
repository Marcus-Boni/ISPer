//! Embeddings para a busca semântica (Fase 5).
//!
//! **Decisão de provider (10/09/2026):** os providers de chat do ISPer não
//! servem — a Anthropic não oferece embeddings e o Groq tampouco. Ficam duas
//! portas, escolhidas pela mesma régua do resto do projeto (custo zero e o
//! mínimo de dado saindo da máquina):
//!
//! - **Gemini** (`gemini-embedding-001`, free tier, multilíngue): reutiliza a
//!   chave já guardada para o provider Gemini;
//! - **Compatível com OpenAI** (`/v1/embeddings`): cobre o **Ollama local**
//!   (`nomic-embed-text`, `bge-m3` — 100% na máquina, sem chave), LM Studio,
//!   e também OpenAI/Mistral para quem preferir.
//!
//! Só o TEXTO dos trechos e da pergunta viaja; os vetores voltam normalizados
//! (norma 1), então similaridade = produto escalar.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::providers::{map_model_error, post_json};
use crate::settings::get_api_key;
use crate::{LlmError, Result};

/// Configuração da busca semântica (seção `[embeddings]` do `llm.toml`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmbeddingSettings {
    /// `gemini` · `openai` (compatível: Ollama, LM Studio, OpenAI…) · `` (desligado).
    #[serde(default)]
    pub provider: String,
    /// Modelo; `None` = padrão do provider.
    #[serde(default)]
    pub model: Option<String>,
    /// Só para `openai`: base da API (`http://localhost:11434/v1` = Ollama).
    #[serde(default)]
    pub base_url: Option<String>,
}

/// Nome da credencial (Credential Manager) da chave do endpoint compatível
/// com OpenAI. O Gemini usa a chave do provider `gemini`.
pub const OPENAI_COMPAT_KEY: &str = "embeddings";
pub const DEFAULT_OPENAI_BASE_URL: &str = "http://localhost:11434/v1";
pub const DEFAULT_GEMINI_MODEL: &str = "gemini-embedding-001";
pub const DEFAULT_OPENAI_MODEL: &str = "nomic-embed-text";
/// Dimensão pedida ao Gemini (o padrão são 3072 — pesado à toa para este uso).
const GEMINI_DIM: u32 = 768;
const GEMINI_BATCH: usize = 100;
const OPENAI_BATCH: usize = 64;

/// Quem transforma texto em vetor.
pub trait Embedder: Send + Sync {
    fn name(&self) -> &'static str;
    fn model(&self) -> &str;
    /// Identificador estável do índice: vetores de modelos diferentes não se
    /// comparam, e o banco guarda esta string ao lado de cada vetor.
    fn id(&self) -> String {
        format!("{}/{}", self.name(), self.model())
    }
    /// Vetores (normalizados) dos documentos, na mesma ordem.
    fn embed_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    /// Vetor (normalizado) de uma pergunta de busca.
    fn embed_query(&self, text: &str) -> Result<Vec<f32>>;
}

impl EmbeddingSettings {
    pub fn is_configured(&self) -> bool {
        !self.provider.trim().is_empty() && self.provider.trim() != "none"
    }

    /// Modelo efetivo (o configurado ou o padrão do provider).
    pub fn effective_model(&self) -> Option<String> {
        let provider = self.provider.trim().to_lowercase();
        let default = match provider.as_str() {
            "gemini" | "google" => DEFAULT_GEMINI_MODEL,
            "openai" | "ollama" => DEFAULT_OPENAI_MODEL,
            _ => return None,
        };
        Some(
            self.model
                .as_deref()
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .unwrap_or(default)
                .to_string(),
        )
    }
}

/// Constrói o embedder configurado. `Err(NotConfigured)` quando desligado.
pub fn embedder_from_settings(settings: &EmbeddingSettings) -> Result<Box<dyn Embedder>> {
    let provider = settings.provider.trim().to_lowercase();
    if !settings.is_configured() {
        return Err(LlmError::NotConfigured);
    }
    let model = settings
        .effective_model()
        .ok_or_else(|| LlmError::UnknownProvider(provider.clone()))?;
    Ok(match provider.as_str() {
        "gemini" | "google" => {
            let key = get_api_key("gemini")?.ok_or_else(|| LlmError::NoApiKey("gemini".into()))?;
            Box::new(GeminiEmbedder {
                api_key: key,
                model,
                dim: GEMINI_DIM,
            })
        }
        "openai" | "ollama" => Box::new(OpenAiCompatEmbedder {
            api_key: get_api_key(OPENAI_COMPAT_KEY)?,
            model,
            base_url: settings
                .base_url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .unwrap_or(DEFAULT_OPENAI_BASE_URL)
                .trim_end_matches('/')
                .to_string(),
        }),
        other => return Err(LlmError::UnknownProvider(other.to_string())),
    })
}

fn normalized(values: &[Value]) -> Vec<f32> {
    let mut v: Vec<f32> = values
        .iter()
        .filter_map(|x| x.as_f64())
        .map(|x| x as f32)
        .collect();
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    v
}

// ---------------------------------------------------------------- Gemini

pub struct GeminiEmbedder {
    pub api_key: String,
    pub model: String,
    pub dim: u32,
}

impl GeminiEmbedder {
    fn request(&self, text: &str, task: &str) -> Value {
        json!({
            "model": format!("models/{}", self.model),
            "content": {"parts": [{"text": text}]},
            "taskType": task,
            "outputDimensionality": self.dim,
        })
    }
}

/// `embeddings[].values` de um `batchEmbedContents` — exige um vetor por pedido.
pub(crate) fn parse_gemini_batch(resp: &Value, expected: usize) -> Result<Vec<Vec<f32>>> {
    let list = resp["embeddings"]
        .as_array()
        .ok_or_else(|| LlmError::BadResponse("resposta sem 'embeddings'".into()))?;
    if list.len() != expected {
        return Err(LlmError::BadResponse(format!(
            "esperava {expected} vetores, vieram {}",
            list.len()
        )));
    }
    list.iter()
        .map(|e| {
            e["values"]
                .as_array()
                .map(|v| normalized(v))
                .filter(|v| !v.is_empty())
                .ok_or_else(|| LlmError::BadResponse("vetor vazio".into()))
        })
        .collect()
}

impl Embedder for GeminiEmbedder {
    fn name(&self) -> &'static str {
        "gemini"
    }
    fn model(&self) -> &str {
        &self.model
    }

    fn embed_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:batchEmbedContents",
            self.model
        );
        for batch in texts.chunks(GEMINI_BATCH) {
            let requests: Vec<Value> = batch
                .iter()
                .map(|t| self.request(t, "RETRIEVAL_DOCUMENT"))
                .collect();
            let resp = post_json(
                &url,
                &[("x-goog-api-key", self.api_key.as_str())],
                json!({"requests": requests}),
            )
            .map_err(|e| map_model_error(e, &self.model))?;
            out.extend(parse_gemini_batch(&resp, batch.len())?);
        }
        Ok(out)
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:embedContent",
            self.model
        );
        let resp = post_json(
            &url,
            &[("x-goog-api-key", self.api_key.as_str())],
            self.request(text, "RETRIEVAL_QUERY"),
        )
        .map_err(|e| map_model_error(e, &self.model))?;
        resp["embedding"]["values"]
            .as_array()
            .map(|v| normalized(v))
            .filter(|v| !v.is_empty())
            .ok_or_else(|| LlmError::BadResponse("resposta sem 'embedding.values'".into()))
    }
}

// ------------------------------------------------- compatível com OpenAI

/// `POST {base_url}/embeddings` no formato da OpenAI — o que Ollama, LM Studio,
/// OpenAI e Mistral falam. Sem chave, vai sem `Authorization` (Ollama local).
pub struct OpenAiCompatEmbedder {
    pub api_key: Option<String>,
    pub model: String,
    pub base_url: String,
}

/// `data[].embedding`, reordenado por `data[].index`.
pub(crate) fn parse_openai_embeddings(resp: &Value, expected: usize) -> Result<Vec<Vec<f32>>> {
    let data = resp["data"]
        .as_array()
        .ok_or_else(|| LlmError::BadResponse("resposta sem 'data'".into()))?;
    if data.len() != expected {
        return Err(LlmError::BadResponse(format!(
            "esperava {expected} vetores, vieram {}",
            data.len()
        )));
    }
    let mut indexed: Vec<(usize, Vec<f32>)> = data
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let idx = d["index"].as_u64().map(|x| x as usize).unwrap_or(i);
            let v = d["embedding"]
                .as_array()
                .map(|v| normalized(v))
                .filter(|v| !v.is_empty())
                .ok_or_else(|| LlmError::BadResponse("vetor vazio".into()))?;
            Ok((idx, v))
        })
        .collect::<Result<Vec<_>>>()?;
    indexed.sort_by_key(|(i, _)| *i);
    Ok(indexed.into_iter().map(|(_, v)| v).collect())
}

impl OpenAiCompatEmbedder {
    fn post(&self, input: Value) -> Result<Value> {
        let url = format!("{}/embeddings", self.base_url);
        let auth = self.api_key.as_ref().map(|k| format!("Bearer {k}"));
        let mut headers: Vec<(&str, &str)> = Vec::new();
        if let Some(a) = auth.as_deref() {
            headers.push(("authorization", a));
        }
        post_json(&url, &headers, json!({"model": self.model, "input": input}))
            .map_err(|e| map_model_error(e, &self.model))
    }
}

impl Embedder for OpenAiCompatEmbedder {
    fn name(&self) -> &'static str {
        "openai"
    }
    fn model(&self) -> &str {
        &self.model
    }

    fn embed_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(OPENAI_BATCH) {
            let resp = self.post(json!(batch))?;
            out.extend(parse_openai_embeddings(&resp, batch.len())?);
        }
        Ok(out)
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let resp = self.post(json!([text]))?;
        parse_openai_embeddings(&resp, 1)?
            .pop()
            .ok_or_else(|| LlmError::BadResponse("resposta vazia".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_batch_normaliza_e_confere_a_contagem() {
        let resp = json!({"embeddings": [{"values": [3.0, 4.0]}, {"values": [0.0, 2.0]}]});
        let v = parse_gemini_batch(&resp, 2).unwrap();
        assert!((v[0][0] - 0.6).abs() < 1e-6 && (v[0][1] - 0.8).abs() < 1e-6);
        assert_eq!(v[1], vec![0.0, 1.0]);
        assert!(parse_gemini_batch(&resp, 3).is_err());
        assert!(parse_gemini_batch(&json!({"embeddings": [{"values": []}]}), 1).is_err());
    }

    #[test]
    fn openai_reordena_pelo_index() {
        let resp = json!({"data": [
            {"index": 1, "embedding": [0.0, 1.0]},
            {"index": 0, "embedding": [1.0, 0.0]}
        ]});
        let v = parse_openai_embeddings(&resp, 2).unwrap();
        assert_eq!(v[0], vec![1.0, 0.0]);
        assert_eq!(v[1], vec![0.0, 1.0]);
        assert!(parse_openai_embeddings(&json!({"nada": 1}), 1).is_err());
    }

    #[test]
    fn settings_resolvem_modelo_padrao_e_desligado() {
        let off = EmbeddingSettings::default();
        assert!(!off.is_configured());
        assert!(matches!(
            embedder_from_settings(&off),
            Err(LlmError::NotConfigured)
        ));
        let g = EmbeddingSettings {
            provider: "gemini".into(),
            ..Default::default()
        };
        assert_eq!(g.effective_model().as_deref(), Some(DEFAULT_GEMINI_MODEL));
        let o = EmbeddingSettings {
            provider: "openai".into(),
            model: Some("  bge-m3 ".into()),
            base_url: None,
        };
        assert_eq!(o.effective_model().as_deref(), Some("bge-m3"));
        assert!(matches!(
            embedder_from_settings(&EmbeddingSettings {
                provider: "cohere".into(),
                ..Default::default()
            }),
            Err(LlmError::UnknownProvider(_))
        ));
    }

    #[test]
    fn id_do_indice_e_provider_barra_modelo() {
        let e = OpenAiCompatEmbedder {
            api_key: None,
            model: "nomic-embed-text".into(),
            base_url: DEFAULT_OPENAI_BASE_URL.into(),
        };
        assert_eq!(e.id(), "openai/nomic-embed-text");
    }
}
