//! Decisões estruturadas pela API "System One" — hoje, o Jev da TypeSafe.
//!
//! Um LLM escreve; o Jev só decide. Recebe um `state` (texto ou JSON) e
//! perguntas tipadas — escolha entre opções, nota numa rubrica, verdadeiro ou
//! falso — e devolve, numa passada, a probabilidade de cada resposta. Não gera
//! texto nenhum, e por isso responde em ~0,4 s e custa US$ 0,042 por milhão de
//! tokens de entrada (a saída é de graça).
//!
//! O Copilot o usa como filtro na frente do LLM (ver [`crate::copilot`]):
//! medido em 450 parágrafos de reuniões reais, com a pergunta do
//! [`crate::copilot::filter_questions`], pega 92% dos momentos que viram card
//! e dispensa 55% dos parágrafos — `docs/estudos/gemini-transcribe-e-jev.md`,
//! §12.5. O Laya, alternativa local no mesmo protocolo, ficou de fora no mesmo
//! teste (§12.4).
//!
//! As regras da ADR 0003 valem aqui como para os LLMs: só texto sai, a chave
//! mora no Credential Manager (conta [`TYPESAFE_KEY`]) e vai em header.

use std::time::Duration;

use serde::Serialize;
use serde::ser::SerializeMap;
use serde_json::Value;

use crate::{LlmError, Result};

/// Conta da chave da TypeSafe no Credential Manager (alvo `typesafe.ISPer`).
pub const TYPESAFE_KEY: &str = "typesafe";
/// Versão fixada: a mesma pergunta não muda de resposta sozinha quando a
/// TypeSafe publicar outro modelo — trocar é uma decisão, com medição.
pub const JEV_MODEL: &str = "jev-1.13.0";
/// Preço de tabela da entrada (a saída é grátis), para o medidor de custo.
pub const JEV_USD_PER_MTOK: f64 = 0.042;
const JEV_URL: &str = "https://api.typesafe.ai/v1/systemone";
/// Teto por tentativa. O p50 medido daqui é ~0,4 s; passar disso é a cauda de
/// rede, e quem chama tem de seguir sem a resposta (o Copilot manda o trecho
/// para o LLM do mesmo jeito).
const TIMEOUT: Duration = Duration::from_secs(3);
/// Espera antes da única nova tentativa (429, 529, 5xx, rede).
const RETRY_AFTER: Duration = Duration::from_millis(300);
/// Quanto do corpo de um erro da API entra na mensagem.
const ERROR_BODY_CHARS: usize = 300;

/// Uma pergunta tipada.
#[derive(Debug, Clone, PartialEq)]
pub enum Question {
    /// Verdadeiro ou falso — devolve a probabilidade de "verdadeiro".
    Noul {
        instructions: String,
        if_true: Option<String>,
        if_false: Option<String>,
    },
    /// Uma opção entre várias (até 255), cada uma com a sua descrição.
    /// A ordem é mantida no pedido: ela pesa para o modelo.
    Choice {
        instructions: String,
        options: Vec<(String, String)>,
    },
    /// Nota numa rubrica de 2 a 10 níveis, do 0 para cima.
    Score {
        instructions: String,
        levels: Vec<String>,
    },
}

/// A resposta de uma pergunta.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Noul(f32),
    Choice {
        chosen: String,
        /// Na ordem das opções da pergunta.
        probabilities: Vec<(String, f32)>,
        confidence: f32,
    },
    Score {
        score: f32,
        probabilities: Vec<f32>,
        confidence: f32,
    },
}

impl Answer {
    /// Probabilidade de uma opção de `Choice` (`None` para os outros tipos).
    pub fn probability(&self, option: &str) -> Option<f32> {
        match self {
            Self::Choice { probabilities, .. } => probabilities
                .iter()
                .find(|(label, _)| label == option)
                .map(|(_, p)| *p),
            _ => None,
        }
    }
}

/// O que uma chamada devolve: as respostas, o modelo que respondeu e os
/// tokens de entrada cobrados.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub model: String,
    pub answers: Vec<(String, Answer)>,
    pub input_tokens: u32,
}

impl Decision {
    pub fn answer(&self, id: &str) -> Option<&Answer> {
        self.answers.iter().find(|(q, _)| q == id).map(|(_, a)| a)
    }

    /// Quanto esta chamada custou, pelo preço de tabela.
    pub fn cost_usd(&self) -> f64 {
        f64::from(self.input_tokens) * JEV_USD_PER_MTOK / 1e6
    }
}

/// Quem responde perguntas tipadas. O Copilot depende deste trait, não do
/// Jev: nos testes entra o `FakeClassifier`, sem rede.
pub trait Classifier: Send + Sync {
    fn name(&self) -> &'static str;
    fn classify(&self, state: &Value, questions: &[(&str, Question)]) -> Result<Decision>;
}

/// O Jev, pela API HTTP da TypeSafe (sem SDK: não há um em Rust, e o
/// protocolo é um POST só — como os providers de LLM do crate).
pub struct Jev {
    api_key: String,
    model: String,
}

impl Jev {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: JEV_MODEL.to_string(),
        }
    }

    /// A chave guardada no Credential Manager (ou em `ISPER_TYPESAFE_API_KEY`).
    pub fn from_keyring() -> Result<Self> {
        crate::get_api_key(TYPESAFE_KEY)?
            .map(Self::new)
            .ok_or_else(|| LlmError::NoApiKey(TYPESAFE_KEY.to_string()))
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .build(),
    )
}

impl Classifier for Jev {
    fn name(&self) -> &'static str {
        "jev"
    }

    fn classify(&self, state: &Value, questions: &[(&str, Question)]) -> Result<Decision> {
        let body = Request {
            model: &self.model,
            state,
            questions: Questions(questions),
        };
        let auth = format!("Bearer {}", self.api_key);
        let mut last = LlmError::Http("sem resposta da TypeSafe".into());
        for attempt in 0..2 {
            if attempt > 0 {
                std::thread::sleep(RETRY_AFTER);
            }
            let mut resp = match agent()
                .post(JEV_URL)
                .header("authorization", &auth)
                .send_json(&body)
            {
                Ok(r) => r,
                Err(e) => {
                    last = LlmError::Http(e.to_string());
                    continue;
                }
            };
            let status = resp.status().as_u16();
            if resp.status().is_success() {
                let v: Value = resp
                    .body_mut()
                    .read_json()
                    .map_err(|e| LlmError::BadResponse(e.to_string()))?;
                return parse_response(&v, questions);
            }
            let text: String = resp
                .body_mut()
                .read_to_string()
                .unwrap_or_default()
                .chars()
                .take(ERROR_BODY_CHARS)
                .collect();
            let (retry, err) = status_error(status, &text);
            if !retry {
                return Err(err);
            }
            last = err;
        }
        Err(last)
    }
}

/// O que um status de erro significa: `true` = vale tentar de novo.
///
/// Chave recusada ou sem crédito não melhora com outra tentativa — e o
/// Copilot precisa saber disso para desligar o filtro na reunião em vez de
/// errar a cada parágrafo.
fn status_error(status: u16, body: &str) -> (bool, LlmError) {
    match status {
        401 | 403 => (
            false,
            LlmError::Refused(format!(
                "a TypeSafe recusou a chave ({status}) — confira em Configurações → Inteligência"
            )),
        ),
        402 => (
            false,
            LlmError::Refused("sem créditos na TypeSafe (402)".into()),
        ),
        422 => (
            false,
            LlmError::BadResponse(format!("pergunta inválida para o Jev (422): {body}")),
        ),
        429 | 529 | 500..=599 => (true, LlmError::Http(format!("status {status}: {body}"))),
        _ => (false, LlmError::Http(format!("status {status}: {body}"))),
    }
}

// ---------------------------------------------------------------- Pedido

/// O corpo do `POST /v1/systemone`. Serialização própria porque o
/// `serde_json` do projeto ordena as chaves dos objetos, e a ordem das
/// opções de uma escolha é parte da pergunta.
#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    state: &'a Value,
    questions: Questions<'a>,
}

struct Questions<'a>(&'a [(&'a str, Question)]);

impl Serialize for Questions<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (id, q) in self.0 {
            m.serialize_entry(id, q)?;
        }
        m.end()
    }
}

struct Ordered<'a>(&'a [(String, String)]);

impl Serialize for Ordered<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in self.0 {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl Serialize for Question {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        match self {
            Self::Noul {
                instructions,
                if_true,
                if_false,
            } => {
                m.serialize_entry("type", "noul")?;
                m.serialize_entry("instructions", instructions)?;
                let mut criteria: Vec<(String, String)> = Vec::new();
                if let Some(t) = if_true {
                    criteria.push(("true".into(), t.clone()));
                }
                if let Some(f) = if_false {
                    criteria.push(("false".into(), f.clone()));
                }
                if !criteria.is_empty() {
                    m.serialize_entry("criteria", &Ordered(&criteria))?;
                }
            }
            Self::Choice {
                instructions,
                options,
            } => {
                m.serialize_entry("type", "choice")?;
                m.serialize_entry("instructions", instructions)?;
                m.serialize_entry("criteria", &Ordered(options))?;
            }
            Self::Score {
                instructions,
                levels,
            } => {
                m.serialize_entry("type", "score")?;
                m.serialize_entry("instructions", instructions)?;
                m.serialize_entry("criteria", levels)?;
            }
        }
        m.end()
    }
}

// ---------------------------------------------------------------- Resposta

/// Lê a resposta da API. Toda pergunta feita precisa ter voltado respondida:
/// meia resposta é resposta inválida, não um zero silencioso.
pub(crate) fn parse_response(v: &Value, questions: &[(&str, Question)]) -> Result<Decision> {
    let answers = v["answers"]
        .as_object()
        .ok_or_else(|| LlmError::BadResponse("resposta do Jev sem 'answers'".into()))?;
    let mut out = Vec::with_capacity(questions.len());
    for (id, q) in questions {
        let a = answers
            .get(*id)
            .ok_or_else(|| LlmError::BadResponse(format!("o Jev não respondeu '{id}'")))?;
        out.push(((*id).to_string(), parse_answer(id, q, a)?));
    }
    Ok(Decision {
        model: v["model"].as_str().unwrap_or_default().to_string(),
        answers: out,
        input_tokens: v["usage"]["input_tokens"]
            .as_u64()
            .map_or(0, |t| u32::try_from(t).unwrap_or(u32::MAX)),
    })
}

fn parse_answer(id: &str, q: &Question, a: &Value) -> Result<Answer> {
    let bad = |what: &str| LlmError::BadResponse(format!("resposta de '{id}' sem {what}"));
    let num = |v: &Value| v.as_f64().map(|x| x as f32);
    match q {
        Question::Noul { .. } => num(&a["noul"])
            .map(Answer::Noul)
            .ok_or_else(|| bad("'noul'")),
        Question::Choice { options, .. } => {
            let probs = a["probabilities"]
                .as_object()
                .ok_or_else(|| bad("'probabilities'"))?;
            let probabilities: Vec<(String, f32)> = options
                .iter()
                .map(|(label, _)| (label.clone(), probs.get(label).and_then(num).unwrap_or(0.0)))
                .collect();
            let chosen = a["choice"].as_str().map_or_else(
                || {
                    probabilities
                        .iter()
                        .max_by(|x, y| x.1.total_cmp(&y.1))
                        .map(|(l, _)| l.clone())
                        .unwrap_or_default()
                },
                str::to_string,
            );
            Ok(Answer::Choice {
                chosen,
                probabilities,
                confidence: num(&a["confidence"]).unwrap_or(0.0),
            })
        }
        Question::Score { levels, .. } => {
            let p = &a["probabilities"];
            let probabilities: Vec<f32> = (0..levels.len())
                .map(|i| {
                    p.get(i.to_string())
                        .or_else(|| p.get(i))
                        .and_then(num)
                        .unwrap_or(0.0)
                })
                .collect();
            Ok(Answer::Score {
                score: num(&a["score"]).ok_or_else(|| bad("'score'"))?,
                probabilities,
                confidence: num(&a["confidence"]).unwrap_or(0.0),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn kind() -> Question {
        Question::Choice {
            instructions: "Classifique".into(),
            options: vec![
                ("decision".into(), "decisão".into()),
                ("action".into(), "ação".into()),
                ("risk".into(), "risco".into()),
                ("none".into(), "nada".into()),
            ],
        }
    }

    #[test]
    fn pedido_mantem_a_ordem_das_opcoes_e_o_formato_da_api() {
        let qs = [
            ("kind", kind()),
            (
                "owner",
                Question::Noul {
                    instructions: "Tem responsável?".into(),
                    if_true: Some("sim".into()),
                    if_false: None,
                },
            ),
        ];
        let state = json!({"anterior": "Eu: oi", "atual": "Participantes: fechado"});
        let body = serde_json::to_string(&Request {
            model: JEV_MODEL,
            state: &state,
            questions: Questions(&qs),
        })
        .unwrap();
        assert_eq!(
            body,
            concat!(
                r#"{"model":"jev-1.13.0","state":{"anterior":"Eu: oi","atual":"Participantes: fechado"},"#,
                r#""questions":{"kind":{"type":"choice","instructions":"Classifique","criteria":"#,
                r#"{"decision":"decisão","action":"ação","risk":"risco","none":"nada"}},"#,
                r#""owner":{"type":"noul","instructions":"Tem responsável?","criteria":{"true":"sim"}}}}"#
            )
        );
    }

    #[test]
    fn score_manda_os_niveis_em_lista() {
        let q = Question::Score {
            instructions: "Urgência".into(),
            levels: vec!["baixa".into(), "alta".into()],
        };
        assert_eq!(
            serde_json::to_value(&q).unwrap(),
            json!({"type": "score", "instructions": "Urgência", "criteria": ["baixa", "alta"]})
        );
    }

    #[test]
    fn le_a_resposta_documentada_de_choice_e_noul() {
        let qs = [
            ("kind", kind()),
            (
                "owner",
                Question::Noul {
                    instructions: "x".into(),
                    if_true: None,
                    if_false: None,
                },
            ),
        ];
        let v = json!({
            "model": "jev-1.13.0",
            "answers": {
                "kind": {"type": "choice", "choice": "none", "confidence": 0.59,
                         "probabilities": {"risk": 0.3, "action": 0.0, "decision": 0.0, "none": 0.7}},
                "owner": {"type": "noul", "noul": 0.02}
            },
            "usage": {"input_tokens": 655, "output_tokens": 0}
        });
        let d = parse_response(&v, &qs).unwrap();
        assert_eq!(d.model, "jev-1.13.0");
        assert_eq!(d.input_tokens, 655);
        let k = d.answer("kind").unwrap();
        assert_eq!(k.probability("none"), Some(0.7));
        assert_eq!(k.probability("risk"), Some(0.3));
        match k {
            Answer::Choice {
                chosen,
                probabilities,
                ..
            } => {
                assert_eq!(chosen, "none");
                let order: Vec<&str> = probabilities.iter().map(|(l, _)| l.as_str()).collect();
                assert_eq!(
                    order,
                    ["decision", "action", "risk", "none"],
                    "ordem da pergunta"
                );
            }
            other => panic!("esperava choice, veio {other:?}"),
        }
        assert_eq!(d.answer("owner"), Some(&Answer::Noul(0.02)));
        assert!((d.cost_usd() - 655.0 * 0.042e-6).abs() < 1e-12);
    }

    #[test]
    fn resposta_sem_uma_das_perguntas_e_erro() {
        let qs = [("kind", kind())];
        let v = json!({"answers": {}, "usage": {"input_tokens": 1}});
        assert!(
            matches!(parse_response(&v, &qs), Err(LlmError::BadResponse(m)) if m.contains("kind"))
        );
        assert!(parse_response(&json!({"erro": 1}), &qs).is_err());
    }

    /// A chamada de verdade, com a chave do Credential Manager (ou
    /// `ISPER_TYPESAFE_API_KEY`): `cargo test --release -p isper-llm -- --ignored jev_de_verdade`.
    /// Custa ~US$ 0,00006.
    #[test]
    #[ignore = "rede e chave da TypeSafe"]
    fn jev_de_verdade_marca_o_compromisso_e_deixa_passar_a_conversa() {
        use crate::copilot::{filter_questions, filter_state, read_filter};
        let jev = Jev::from_keyring().expect("chave da TypeSafe guardada");
        let qs = filter_questions();
        let d = jev
            .classify(
                &filter_state(
                    "Participantes: e aí, qual prazo a gente consegue?",
                    "Eu: então fica combinado, eu mando a proposta revisada até sexta.",
                ),
                &qs,
            )
            .expect("resposta do Jev");
        assert_eq!(d.model, JEV_MODEL);
        assert!(d.input_tokens > 100, "tokens: {}", d.input_tokens);
        let v = read_filter(&d).expect("veredito");
        assert!(v.flagged, "compromisso com prazo tem de ser marcado: {v:?}");

        let calmo = jev
            .classify(
                &filter_state(
                    "Eu: bom dia, pessoal",
                    "Participantes: bom dia, tudo bem com vocês? Deixa eu só compartilhar a tela aqui rapidinho.",
                ),
                &qs,
            )
            .expect("resposta do Jev");
        let v = read_filter(&calmo).expect("veredito");
        assert!(!v.flagged, "cumprimento não é card: {v:?}");
    }

    #[test]
    fn chave_recusada_e_sem_credito_nao_se_repetem() {
        assert!(!status_error(401, "").0);
        assert!(!status_error(403, "").0);
        assert!(matches!(status_error(402, "").1, LlmError::Refused(_)));
        assert!(!status_error(422, "x").0);
        for s in [429, 529, 500, 503] {
            assert!(status_error(s, "").0, "{s} deve ser tentado de novo");
        }
    }
}
