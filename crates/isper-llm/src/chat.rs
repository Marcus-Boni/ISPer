//! Conversa com ferramentas (Fase 10.4): o mesmo laço sobre Claude, Groq e
//! Gemini.
//!
//! Cada provider fala um dialeto: o Claude devolve blocos `tool_use` e
//! recebe `tool_result`; o Groq segue o formato da OpenAI (`tool_calls` e
//! mensagens `tool`); o Gemini usa partes `functionCall` e `functionResponse`
//! e só aceita um subconjunto do JSON Schema. Aqui ficam os tipos comuns e a
//! tradução de ida e volta, em funções puras que os testes exercitam sem
//! rede.
//!
//! A resposta do assistente volta **intacta** na rodada seguinte (`raw`): o
//! Claude 5.5 amarra os blocos de pensamento à conversa e o Gemini 3 exige a
//! assinatura de pensamento das partes de volta. Montar de novo a partir do
//! texto quebraria as duas coisas.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::providers::gemini_schema;
use crate::{LlmError, Result};

/// Uma ferramenta que o modelo pode chamar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    /// Nome (letras, números, `_` e `-`).
    pub name: String,
    /// Quando e para que usar: é por isto que o modelo decide.
    pub description: String,
    /// JSON Schema dos argumentos (`type: object`).
    pub input_schema: Value,
}

/// Um pedido do modelo para rodar uma ferramenta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Identificador da chamada (a resposta precisa citá-lo).
    pub id: String,
    /// A ferramenta.
    pub name: String,
    /// Os argumentos, como o modelo os mandou.
    pub arguments: Value,
}

/// O resultado de uma ferramenta, de volta ao modelo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    /// A chamada a que responde.
    pub call_id: String,
    /// A ferramenta (o Gemini pede o nome de volta).
    pub name: String,
    /// O que a ferramenta devolveu, em texto (JSON costuma ir aqui).
    pub content: String,
    /// A ferramenta falhou ou foi recusada.
    pub is_error: bool,
}

/// Uma resposta do assistente.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantTurn {
    /// O texto da resposta (pode ser vazio quando só há chamadas).
    pub text: String,
    /// As ferramentas que ele quer rodar.
    pub tool_calls: Vec<ToolCall>,
    /// A resposta como o provider mandou, para reenviar sem mexer.
    pub raw: Value,
}

/// Uma mensagem da conversa.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ChatMessage {
    /// O que a pessoa escreveu.
    User {
        /// O texto.
        text: String,
    },
    /// O que o assistente respondeu.
    Assistant {
        /// A resposta.
        turn: AssistantTurn,
    },
    /// Os resultados das ferramentas pedidas na resposta anterior.
    Tools {
        /// Um por chamada, na ordem das chamadas.
        results: Vec<ToolResult>,
    },
}

/// Por que o modelo parou.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Terminou de responder.
    EndTurn,
    /// Quer ferramentas.
    ToolUse,
    /// Bateu no limite de tokens (a resposta pode estar cortada).
    MaxTokens,
}

/// Uma rodada de conversa.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatReply {
    /// A resposta.
    pub turn: AssistantTurn,
    /// Por que parou.
    pub stop: StopReason,
}

/// O tanto de saída numa rodada com ferramentas: com raciocínio ligado (o
/// padrão nos modelos novos), o pensamento conta no limite.
pub(crate) const CHAT_MAX_TOKENS: u32 = 16_000;

// ------------------------------------------------------------------ Claude

/// O corpo de `/v1/messages` com ferramentas.
pub(crate) fn claude_body(
    model: &str,
    system: &str,
    messages: &[ChatMessage],
    tools: &[ToolSpec],
) -> Value {
    let msgs: Vec<Value> = messages
        .iter()
        .map(|m| match m {
            ChatMessage::User { text } => json!({"role": "user", "content": text}),
            ChatMessage::Assistant { turn } => {
                let content = if turn.raw.is_array() {
                    turn.raw.clone()
                } else {
                    let mut blocks = Vec::new();
                    if !turn.text.is_empty() {
                        blocks.push(json!({"type": "text", "text": turn.text}));
                    }
                    for c in &turn.tool_calls {
                        blocks.push(json!({
                            "type": "tool_use", "id": c.id, "name": c.name, "input": c.arguments,
                        }));
                    }
                    Value::Array(blocks)
                };
                json!({"role": "assistant", "content": content})
            }
            ChatMessage::Tools { results } => json!({
                "role": "user",
                "content": results
                    .iter()
                    .map(|r| json!({
                        "type": "tool_result",
                        "tool_use_id": r.call_id,
                        "content": r.content,
                        "is_error": r.is_error,
                    }))
                    .collect::<Vec<_>>(),
            }),
        })
        .collect();
    let mut body = json!({
        "model": model,
        "max_tokens": CHAT_MAX_TOKENS,
        "system": system,
        "messages": msgs,
    });
    if !tools.is_empty() {
        body["tools"] = tools
            .iter()
            .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.input_schema}))
            .collect();
    }
    body
}

/// Lê a resposta do Claude.
pub(crate) fn claude_reply(resp: &Value) -> Result<ChatReply> {
    let stop = match resp["stop_reason"].as_str() {
        Some("refusal") => {
            let cat = resp["stop_details"]["category"]
                .as_str()
                .unwrap_or("sem categoria");
            return Err(LlmError::Refused(cat.to_string()));
        }
        Some("max_tokens") => StopReason::MaxTokens,
        Some("tool_use") => StopReason::ToolUse,
        _ => StopReason::EndTurn,
    };
    let blocks = resp["content"]
        .as_array()
        .ok_or_else(|| LlmError::BadResponse("resposta sem conteúdo".into()))?;
    let text = blocks
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("");
    let tool_calls = blocks
        .iter()
        .filter(|b| b["type"] == "tool_use")
        .map(|b| ToolCall {
            id: b["id"].as_str().unwrap_or_default().to_string(),
            name: b["name"].as_str().unwrap_or_default().to_string(),
            arguments: b["input"].clone(),
        })
        .collect();
    Ok(ChatReply {
        turn: AssistantTurn {
            text,
            tool_calls,
            raw: resp["content"].clone(),
        },
        stop,
    })
}

// -------------------------------------------------------------------- Groq

/// O corpo de `chat/completions` (formato da OpenAI) com ferramentas.
pub(crate) fn openai_body(
    model: &str,
    system: &str,
    messages: &[ChatMessage],
    tools: &[ToolSpec],
) -> Value {
    let mut msgs = vec![json!({"role": "system", "content": system})];
    for m in messages {
        match m {
            ChatMessage::User { text } => msgs.push(json!({"role": "user", "content": text})),
            ChatMessage::Assistant { turn } => {
                // Só os campos que a API aceita de volta: o raciocínio que
                // alguns modelos mandam não é reenviado.
                let mut msg = json!({
                    "role": "assistant",
                    "content": if turn.text.is_empty() { Value::Null } else { json!(turn.text) },
                });
                if !turn.tool_calls.is_empty() {
                    msg["tool_calls"] = turn
                        .tool_calls
                        .iter()
                        .map(|c| {
                            json!({
                                "id": c.id,
                                "type": "function",
                                "function": {"name": c.name, "arguments": c.arguments.to_string()},
                            })
                        })
                        .collect();
                }
                msgs.push(msg);
            }
            ChatMessage::Tools { results } => {
                for r in results {
                    msgs.push(json!({
                        "role": "tool",
                        "tool_call_id": r.call_id,
                        "content": r.content,
                    }));
                }
            }
        }
    }
    let mut body = json!({
        "model": model,
        "max_tokens": CHAT_MAX_TOKENS,
        "messages": msgs,
    });
    if !tools.is_empty() {
        body["tools"] = tools
            .iter()
            .map(|t| json!({
                "type": "function",
                "function": {"name": t.name, "description": t.description, "parameters": t.input_schema},
            }))
            .collect();
    }
    body
}

/// Lê a resposta no formato da OpenAI. Argumentos que não são JSON válido
/// ficam como texto: quem roda a ferramenta recusa e o modelo tenta de novo.
pub(crate) fn openai_reply(resp: &Value) -> Result<ChatReply> {
    let choice = &resp["choices"][0];
    let message = &choice["message"];
    if message.is_null() {
        return Err(LlmError::BadResponse("resposta sem mensagem".into()));
    }
    let tool_calls: Vec<ToolCall> = message["tool_calls"]
        .as_array()
        .map(|calls| {
            calls
                .iter()
                .map(|c| {
                    let raw = c["function"]["arguments"].as_str().unwrap_or("{}");
                    ToolCall {
                        id: c["id"].as_str().unwrap_or_default().to_string(),
                        name: c["function"]["name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        arguments: serde_json::from_str(raw).unwrap_or_else(|_| json!(raw)),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let stop = match choice["finish_reason"].as_str() {
        Some("length") => StopReason::MaxTokens,
        _ if !tool_calls.is_empty() => StopReason::ToolUse,
        _ => StopReason::EndTurn,
    };
    Ok(ChatReply {
        turn: AssistantTurn {
            text: message["content"].as_str().unwrap_or_default().to_string(),
            tool_calls,
            raw: message.clone(),
        },
        stop,
    })
}

// ------------------------------------------------------------------ Gemini

/// Prefixo dos ids que o ISPer inventa quando o Gemini não manda um (os
/// modelos antigos não mandam): não voltam na resposta da função.
const LOCAL_ID: &str = "isper-call-";

/// O corpo de `generateContent` com ferramentas.
pub(crate) fn gemini_body(system: &str, messages: &[ChatMessage], tools: &[ToolSpec]) -> Value {
    let contents: Vec<Value> = messages
        .iter()
        .map(|m| match m {
            ChatMessage::User { text } => json!({"role": "user", "parts": [{"text": text}]}),
            ChatMessage::Assistant { turn } => {
                let parts = if turn.raw.is_array() {
                    turn.raw.clone()
                } else {
                    let mut parts = Vec::new();
                    if !turn.text.is_empty() {
                        parts.push(json!({"text": turn.text}));
                    }
                    for c in &turn.tool_calls {
                        let mut call = json!({"name": c.name, "args": c.arguments});
                        if !c.id.starts_with(LOCAL_ID) {
                            call["id"] = json!(c.id);
                        }
                        parts.push(json!({"functionCall": call}));
                    }
                    Value::Array(parts)
                };
                json!({"role": "model", "parts": parts})
            }
            ChatMessage::Tools { results } => json!({
                "role": "user",
                "parts": results
                    .iter()
                    .map(|r| {
                        let key = if r.is_error { "error" } else { "result" };
                        let mut response = json!({"name": r.name, "response": {key: r.content}});
                        if !r.call_id.starts_with(LOCAL_ID) {
                            response["id"] = json!(r.call_id);
                        }
                        json!({"functionResponse": response})
                    })
                    .collect::<Vec<_>>(),
            }),
        })
        .collect();
    let mut body = json!({
        "system_instruction": {"parts": [{"text": system}]},
        "contents": contents,
        "generationConfig": {"maxOutputTokens": CHAT_MAX_TOKENS},
    });
    if !tools.is_empty() {
        body["tools"] = json!([{
            "functionDeclarations": tools
                .iter()
                .map(|t| json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": gemini_schema(&t.input_schema),
                }))
                .collect::<Vec<_>>(),
        }]);
    }
    body
}

/// Lê a resposta do Gemini.
pub(crate) fn gemini_reply(resp: &Value) -> Result<ChatReply> {
    if let Some(block) = resp["promptFeedback"]["blockReason"].as_str() {
        return Err(LlmError::Refused(block.to_string()));
    }
    let candidate = &resp["candidates"][0];
    let finish = candidate["finishReason"].as_str().unwrap_or("STOP");
    if matches!(
        finish,
        "SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST" | "SPII" | "RECITATION"
    ) {
        return Err(LlmError::Refused(finish.to_lowercase()));
    }
    let parts = candidate["content"]["parts"].as_array();
    let text = parts
        .map(|ps| {
            ps.iter()
                .filter(|p| p["thought"].as_bool() != Some(true))
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    let tool_calls: Vec<ToolCall> = parts
        .map(|ps| {
            ps.iter()
                .filter_map(|p| p.get("functionCall"))
                .enumerate()
                .map(|(i, f)| ToolCall {
                    id: f["id"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map_or_else(|| format!("{LOCAL_ID}{i}"), String::from),
                    name: f["name"].as_str().unwrap_or_default().to_string(),
                    arguments: if f["args"].is_null() {
                        json!({})
                    } else {
                        f["args"].clone()
                    },
                })
                .collect()
        })
        .unwrap_or_default();
    if parts.is_none() && finish == "STOP" {
        return Err(LlmError::BadResponse("resposta sem conteúdo".into()));
    }
    let stop = match finish {
        "MAX_TOKENS" => StopReason::MaxTokens,
        _ if !tool_calls.is_empty() => StopReason::ToolUse,
        _ => StopReason::EndTurn,
    };
    Ok(ChatReply {
        turn: AssistantTurn {
            text,
            tool_calls,
            raw: candidate["content"]["parts"].clone(),
        },
        stop,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> ToolSpec {
        ToolSpec {
            name: "opt_time_log_time".into(),
            description: "Registra horas.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "durationMinutes": {"type": "integer", "minimum": 1},
                    "azureWorkItemId": {"type": ["integer", "string"], "description": "Work item"},
                    "note": {"type": ["string", "null"]}
                },
                "required": ["durationMinutes"],
                "additionalProperties": false
            }),
        }
    }

    fn conversa(raw: Value) -> Vec<ChatMessage> {
        vec![
            ChatMessage::User {
                text: "Lança 1h no 4512".into(),
            },
            ChatMessage::Assistant {
                turn: AssistantTurn {
                    text: "Vou lançar.".into(),
                    tool_calls: vec![ToolCall {
                        id: "call_1".into(),
                        name: "opt_time_log_time".into(),
                        arguments: json!({"durationMinutes": 60}),
                    }],
                    raw,
                },
            },
            ChatMessage::Tools {
                results: vec![ToolResult {
                    call_id: "call_1".into(),
                    name: "opt_time_log_time".into(),
                    content: "{\"ok\":true}".into(),
                    is_error: false,
                }],
            },
        ]
    }

    #[test]
    fn claude_reenvia_a_resposta_intacta_e_le_tool_use() {
        let raw = json!([
            {"type": "thinking", "thinking": "", "signature": "sig-1"},
            {"type": "text", "text": "Vou lançar."},
            {"type": "tool_use", "id": "call_1", "name": "opt_time_log_time", "input": {"durationMinutes": 60}}
        ]);
        let body = claude_body("claude-opus-5-5", "sys", &conversa(raw.clone()), &[tool()]);
        assert_eq!(
            body["messages"][1]["content"], raw,
            "o pensamento volta junto"
        );
        assert_eq!(body["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "call_1");
        assert_eq!(
            body["tools"][0]["input_schema"]["properties"]["azureWorkItemId"]["type"][1],
            "string"
        );

        let resp = json!({
            "stop_reason": "tool_use",
            "content": [
                {"type": "text", "text": "Conferindo."},
                {"type": "tool_use", "id": "toolu_9", "name": "hoje", "input": {"dia": "2026-10-09"}}
            ]
        });
        let r = claude_reply(&resp).unwrap();
        assert_eq!(r.stop, StopReason::ToolUse);
        assert_eq!(r.turn.text, "Conferindo.");
        assert_eq!(r.turn.tool_calls[0].arguments["dia"], "2026-10-09");
        assert!(matches!(
            claude_reply(&json!({"stop_reason": "refusal", "content": []})),
            Err(LlmError::Refused(_))
        ));
    }

    #[test]
    fn openai_monta_tool_calls_e_mensagens_tool() {
        let body = openai_body(
            "openai/gpt-oss-120b",
            "sys",
            &conversa(Value::Null),
            &[tool()],
        );
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(
            msgs[2]["tool_calls"][0]["function"]["arguments"],
            "{\"durationMinutes\":60}"
        );
        assert_eq!(msgs[3]["role"], "tool");
        assert_eq!(msgs[3]["tool_call_id"], "call_1");
        assert_eq!(body["tools"][0]["type"], "function");

        let resp = json!({"choices": [{"finish_reason": "tool_calls", "message": {
            "role": "assistant", "content": null, "reasoning": "pensando",
            "tool_calls": [{"id": "c9", "type": "function", "function": {"name": "hoje", "arguments": "{\"dia\":\"hoje\"}"}}]
        }}]});
        let r = openai_reply(&resp).unwrap();
        assert_eq!(r.stop, StopReason::ToolUse);
        assert_eq!(r.turn.tool_calls[0].arguments["dia"], "hoje");
        // O raciocínio não volta no reenvio.
        let again = openai_body("m", "s", &[ChatMessage::Assistant { turn: r.turn }], &[]);
        assert!(again["messages"][1].get("reasoning").is_none());
        assert!(again.get("tools").is_none());
    }

    #[test]
    fn gemini_limpa_o_esquema_e_devolve_a_assinatura() {
        let raw = json!([
            {"functionCall": {"id": "fc-1", "name": "opt_time_log_time", "args": {"durationMinutes": 60}}, "thoughtSignature": "assinatura"}
        ]);
        let body = gemini_body("sys", &conversa(raw.clone()), &[tool()]);
        assert_eq!(body["contents"][1]["role"], "model");
        assert_eq!(
            body["contents"][1]["parts"], raw,
            "a assinatura volta intacta"
        );
        let fr = &body["contents"][2]["parts"][0]["functionResponse"];
        assert_eq!(fr["name"], "opt_time_log_time");
        assert_eq!(fr["id"], "call_1");
        assert_eq!(fr["response"]["result"], "{\"ok\":true}");
        let params = &body["tools"][0]["functionDeclarations"][0]["parameters"];
        assert!(params.get("additionalProperties").is_none());
        assert_eq!(
            params["properties"]["azureWorkItemId"]["type"], "STRING",
            "união com texto vira texto"
        );
        assert_eq!(params["properties"]["note"]["nullable"], true);
        assert_eq!(params["properties"]["durationMinutes"]["minimum"], 1);

        let resp = json!({"candidates": [{"finishReason": "STOP", "content": {"role": "model", "parts": [
            {"text": "pensei", "thought": true},
            {"functionCall": {"name": "hoje", "args": {"dia": "hoje"}}, "thoughtSignature": "s"}
        ]}}]});
        let r = gemini_reply(&resp).unwrap();
        assert_eq!(r.stop, StopReason::ToolUse);
        assert_eq!(r.turn.text, "", "o pensamento não vira resposta");
        assert!(r.turn.tool_calls[0].id.starts_with(LOCAL_ID));
        let tools = vec![ChatMessage::Tools {
            results: vec![ToolResult {
                call_id: r.turn.tool_calls[0].id.clone(),
                name: "hoje".into(),
                content: "erro".into(),
                is_error: true,
            }],
        }];
        let b = gemini_body("s", &tools, &[]);
        let fr = &b["contents"][0]["parts"][0]["functionResponse"];
        assert!(fr.get("id").is_none(), "id inventado não volta");
        assert_eq!(fr["response"]["error"], "erro");
        assert!(matches!(
            gemini_reply(&json!({"candidates": [{"finishReason": "SAFETY"}]})),
            Err(LlmError::Refused(_))
        ));
    }

    #[test]
    fn mensagens_vao_e_voltam_do_json() {
        let msgs = conversa(json!([{"type": "text", "text": "x"}]));
        let v = serde_json::to_value(&msgs).unwrap();
        assert_eq!(v[0]["role"], "user");
        assert_eq!(v[2]["role"], "tools");
        let back: Vec<ChatMessage> = serde_json::from_value(v).unwrap();
        assert_eq!(back, msgs);
    }
}
