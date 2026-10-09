//! Fala solta vira tarefas (Fase 10.1, [ADR 0021]).
//!
//! A pessoa dita do jeito que pensa ("amanhã cedo revisar o contrato e às 3
//! ligar pro João, ah, e pagar o boleto até sexta") e a LLM separa as tarefas,
//! num JSON de esquema estrito. Ela **não calcula datas**: devolve o trecho
//! falado ("amanhã às 3", "até sexta"), e quem vira data é o parser
//! determinístico do `isper-assist` (`when::resolve`). Conta de dia da semana
//! é onde os modelos erram.
//!
//! [ADR 0021]: ../../../docs/adr/0021-assistente-pessoal-no-isper.md

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::providers::LlmProvider;
use crate::{LlmError, Result};

/// Tarefas demais numa fala só é sinal de algo errado (um texto colado, não
/// um ditado): o resto fica de fora.
pub const MAX_TASKS: usize = 20;
/// Título mais longo aceito (caracteres); o resto vira nota.
const MAX_TITLE_CHARS: usize = 200;

/// Uma tarefa tirada da fala, ainda sem data resolvida.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedTask {
    /// O que fazer, começando pelo verbo.
    pub title: String,
    /// O trecho falado de quando fazer ("amanhã às 3"), como foi dito.
    pub when: Option<String>,
    /// O trecho falado do prazo ("até sexta").
    pub due: Option<String>,
    /// Detalhe que não cabe no título.
    pub notes: Option<String>,
}

const SYSTEM: &str = "Você transforma uma fala ditada numa lista de tarefas. A fala é a \
transcrição automática de alguém falando sozinho, e pode ter repetições, hesitações e correções.

Regras:
1. Uma tarefa por ação que a pessoa precisa fazer. \"Revisar o PR e mandar pro João\" são duas tarefas.
2. Título curto, começando por um verbo no infinitivo (\"Ligar pro contador sobre o IR\"), com os nomes, números e o objeto da ação. Sem \"eu preciso\", \"tenho que\", \"lembrar de\", \"não esquecer de\".
3. when: o trecho exato da fala que diz QUANDO fazer (\"amanhã às 3\", \"sexta que vem\", \"dia 12 de manhã\"), copiado como foi dito, sem calcular a data. Se o dia vem de outra parte da fala (\"amanhã cedo revisar o contrato e às 3 ligar pro João\"), junte o dia e a hora no trecho (\"amanhã às 3\"). null se não disse. Esse trecho não entra no título.
4. due: o trecho exato do prazo (\"até sexta\", \"prazo dia 15\"). null se não disse.
5. notes: um detalhe útil que não cabe no título. null se não houver.
6. Correções valem: se a pessoa se corrige (\"não, na verdade é na quinta\", \"esquece a segunda\"), aplique a correção e não crie a tarefa errada.
7. Não invente. Comentário, desabafo ou pensamento solto não vira tarefa. Fala sem nenhuma tarefa devolve a lista vazia.
8. Escreva no idioma da fala.";

/// O esquema da resposta (JSON Schema estrito: todo campo obrigatório, os
/// opcionais anuláveis, sem propriedades extras).
pub fn tasks_schema() -> Value {
    let nullable =
        |description: &str| json!({"type": ["string", "null"], "description": description});
    json!({
        "type": "object",
        "properties": {
            "tasks": {
                "type": "array",
                "description": "As tarefas, na ordem em que foram ditas.",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": {"type": "string", "description": "O que fazer, começando pelo verbo no infinitivo."},
                        "when": nullable("Trecho exato da fala que diz quando fazer, ou null."),
                        "due": nullable("Trecho exato da fala que diz o prazo, ou null."),
                        "notes": nullable("Detalhe útil que não cabe no título, ou null."),
                    },
                    "required": ["title", "when", "due", "notes"],
                    "additionalProperties": false,
                },
            },
        },
        "required": ["tasks"],
        "additionalProperties": false,
    })
}

/// Separa as tarefas de uma fala. Fala vazia nem chama a API.
pub fn extract_tasks(provider: &dyn LlmProvider, speech: &str) -> Result<Vec<ExtractedTask>> {
    let speech = speech.trim();
    if speech.is_empty() {
        return Ok(Vec::new());
    }
    let user = format!("Fala:\n<<<\n{speech}\n>>>");
    let schema = tasks_schema();
    let value = with_retries(&RETRY_WAITS, || {
        provider.complete_json(SYSTEM, &user, &schema)
    })?;
    parse_tasks(&value)
}

/// Esperas antes de cada nova tentativa num erro passageiro da API.
pub(crate) const RETRY_WAITS: [std::time::Duration; 2] = [
    std::time::Duration::from_millis(400),
    std::time::Duration::from_millis(1200),
];

/// Erro que costuma passar sozinho: limite de taxa (429) e sobrecarga ou
/// falha do servidor (5xx). O Gemini responde 503 em horário de pico.
pub(crate) fn is_transient(err: &LlmError) -> bool {
    matches!(err, LlmError::Http(msg)
        if ["status 429", "status 500", "status 502", "status 503", "status 504"]
            .iter()
            .any(|code| msg.contains(code)))
}

/// Tenta de novo, depois de cada espera, enquanto o erro for passageiro.
pub(crate) fn with_retries<T>(
    waits: &[std::time::Duration],
    mut call: impl FnMut() -> Result<T>,
) -> Result<T> {
    let mut waits = waits.iter();
    loop {
        match call() {
            Err(e) if is_transient(&e) => match waits.next() {
                Some(wait) => std::thread::sleep(*wait),
                None => return Err(e),
            },
            other => return other,
        }
    }
}

/// Lê e arruma a resposta: títulos aparados (e encurtados, com o excesso nas
/// notas), trechos vazios viram `None`, repetidas e sem título saem.
pub fn parse_tasks(value: &Value) -> Result<Vec<ExtractedTask>> {
    let items = value["tasks"]
        .as_array()
        .ok_or_else(|| LlmError::BadResponse("resposta sem a lista de tarefas".into()))?;
    let text = |v: &Value| {
        v.as_str()
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty() && s != "null")
    };
    let mut out: Vec<ExtractedTask> = Vec::new();
    for item in items {
        let Some(mut title) = text(&item["title"]) else {
            continue;
        };
        let mut notes = text(&item["notes"]);
        if title.chars().count() > MAX_TITLE_CHARS {
            let rest: String = title.chars().skip(MAX_TITLE_CHARS).collect();
            title = title.chars().take(MAX_TITLE_CHARS).collect();
            notes = Some(match notes {
                Some(n) => format!("…{rest}\n{n}"),
                None => format!("…{rest}"),
            });
        }
        if out.iter().any(|t| t.title.eq_ignore_ascii_case(&title)) {
            continue;
        }
        out.push(ExtractedTask {
            title,
            when: text(&item["when"]),
            due: text(&item["due"]),
            notes,
        });
        if out.len() == MAX_TASKS {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeProvider;

    #[test]
    fn a_fala_vai_delimitada_e_o_json_volta_em_tarefas() {
        let fake = FakeProvider::replying(
            r#"{"tasks":[
                {"title":"Revisar o contrato","when":"amanhã cedo","due":null,"notes":null},
                {"title":"Ligar pro João","when":"às 3","due":null,"notes":"sobre o orçamento"},
                {"title":"Pagar o boleto","when":null,"due":"até sexta","notes":null}
            ]}"#,
        );
        let tasks = extract_tasks(
            &fake,
            "amanhã cedo revisar o contrato e às 3 ligar pro João sobre o orçamento, e pagar o boleto até sexta",
        )
        .unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[1].when.as_deref(), Some("às 3"));
        assert_eq!(tasks[1].notes.as_deref(), Some("sobre o orçamento"));
        assert_eq!(tasks[2].due.as_deref(), Some("até sexta"));
        let (system, user) = fake.single_call();
        assert!(system.contains("sem calcular a data"));
        assert!(
            system.contains("JSON Schema"),
            "o padrão pede o esquema no texto"
        );
        assert!(user.starts_with("Fala:\n<<<\n") && user.ends_with("\n>>>"));
    }

    #[test]
    fn fala_vazia_nao_chama_a_api() {
        let fake = FakeProvider::replying("{}");
        assert!(extract_tasks(&fake, "   ").unwrap().is_empty());
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn json_com_cerca_e_conversa_tambem_vale() {
        let fake = FakeProvider::replying(
            "Claro! Aqui está:\n```json\n{\"tasks\":[{\"title\":\"Ligar\",\"when\":\"\",\"due\":\"null\",\"notes\":null}]}\n```",
        );
        let tasks = extract_tasks(&fake, "ligar").unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].when, None, "trecho vazio vira None");
        assert_eq!(tasks[0].due, None, "o texto 'null' também");
    }

    #[test]
    fn sem_titulo_e_repetida_saem_e_titulo_longo_vira_nota() {
        let longo = "a".repeat(250);
        let value = json!({"tasks": [
            {"title": "  ", "when": null, "due": null, "notes": null},
            {"title": "Revisar o PR", "when": null, "due": null, "notes": null},
            {"title": "revisar o pr", "when": "hoje", "due": null, "notes": null},
            {"title": longo, "when": null, "due": null, "notes": "x"},
        ]});
        let tasks = parse_tasks(&value).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[1].title.chars().count(), MAX_TITLE_CHARS);
        assert!(tasks[1].notes.as_deref().unwrap().starts_with('…'));
    }

    #[test]
    fn resposta_sem_lista_e_erro() {
        assert!(parse_tasks(&json!({"tarefas": []})).is_err());
        let fake = FakeProvider::replying("não sei");
        assert!(extract_tasks(&fake, "ligar").is_err());
    }

    #[test]
    fn erro_passageiro_tenta_de_novo_e_o_resto_nao() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = AtomicUsize::new(0);
        let zero = [std::time::Duration::ZERO; 2];
        let ok = with_retries(&zero, || {
            if calls.fetch_add(1, Ordering::SeqCst) < 2 {
                Err(LlmError::Http("status 503: high demand".into()))
            } else {
                Ok(7)
            }
        });
        assert_eq!(ok.unwrap(), 7);
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        calls.store(0, Ordering::SeqCst);
        let gave_up: Result<i32> = with_retries(&zero, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(LlmError::Http("status 503".into()))
        });
        assert!(gave_up.is_err());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "uma tentativa e duas repetições"
        );

        calls.store(0, Ordering::SeqCst);
        let fatal: Result<i32> = with_retries(&zero, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(LlmError::Http("status 400: bad request".into()))
        });
        assert!(fatal.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "400 não se repete");
    }

    #[test]
    fn o_esquema_e_estrito() {
        let s = tasks_schema();
        assert_eq!(s["additionalProperties"], json!(false));
        let item = &s["properties"]["tasks"]["items"];
        assert_eq!(item["additionalProperties"], json!(false));
        assert_eq!(item["required"], json!(["title", "when", "due", "notes"]));
    }

    #[test]
    fn o_esquema_no_dialeto_do_gemini() {
        let g = crate::providers::gemini_schema(&tasks_schema());
        assert_eq!(g["type"], json!("OBJECT"));
        assert!(g.get("additionalProperties").is_none());
        let item = &g["properties"]["tasks"]["items"];
        assert_eq!(item["properties"]["when"]["type"], json!("STRING"));
        assert_eq!(item["properties"]["when"]["nullable"], json!(true));
        assert_eq!(item["properties"]["title"]["type"], json!("STRING"));
        assert!(item["properties"]["title"].get("nullable").is_none());
    }
}
