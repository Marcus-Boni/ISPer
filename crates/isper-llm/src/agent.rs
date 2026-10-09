//! O laço do agente (Fase 10.4, [ADR 0023]): o modelo pergunta às
//! ferramentas até poder responder, citando de onde tirou cada coisa.
//!
//! - **Ler é livre:** ferramenta com permissão [`Permission::Allow`] roda na
//!   hora e o resultado volta ao modelo.
//! - **Escrever pede um toque:** com [`Permission::Ask`], o laço para e
//!   devolve [`Step::Confirm`] com a descrição de cada ação; quem chama
//!   mostra o cartão e retoma com [`resume`], dizendo o que foi aprovado. O
//!   que não foi aprovado volta ao modelo como recusa.
//! - **Nunca:** ferramenta bloqueada nem chega a rodar; o modelo é avisado.
//!
//! As ferramentas devolvem fontes (`tarefa:…`, `reuniao:42@754`,
//! `opttime:dia:…`) e o modelo é orientado a citá-las como `[[ref]]`; a
//! resposta final traz as fontes citadas, para a tela virar cada marca num
//! selo que abre a origem. A conversa inteira é serializável: o app guarda
//! entre um toque e outro.
//!
//! [ADR 0023]: ../../../docs/adr/0023-escada-de-confianca.md

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::{ChatMessage, StopReason, ToolCall, ToolResult, ToolSpec};
use crate::providers::LlmProvider;
use crate::{LlmError, Result};

/// Rodadas de ferramenta antes de pedir a resposta sem ferramentas.
pub const MAX_STEPS: usize = 8;
/// Resultado de ferramenta mais longo que vai ao modelo (caracteres).
const MAX_RESULT_CHARS: usize = 24_000;
/// Novas tentativas num erro passageiro (limite de taxa, servidor ocupado).
const RETRIES: usize = 3;
/// A espera mais longa que se aceita entre tentativas.
const MAX_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Quanto a API mandou esperar ("try again in 6.765s", "in 630ms"); sem
/// dica, uma espera crescente.
fn retry_wait(err: &LlmError, attempt: usize) -> std::time::Duration {
    let fallback = std::time::Duration::from_secs([2, 6, 15][attempt.min(2)]);
    let LlmError::Http(msg) = err else {
        return fallback;
    };
    // Groq: "try again in 6.765s"; Gemini: "Please retry in 35.1s" e
    // "retryDelay": "35s".
    let Some((at, len)) = ["try again in ", "retry in ", "\"retryDelay\": \""]
        .iter()
        .find_map(|k| msg.find(k).map(|at| (at, k.len())))
    else {
        return fallback;
    };
    let rest = &msg[at + len..];
    let number: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let unit = &rest[number.len()..];
    let Ok(value) = number.parse::<f64>() else {
        return fallback;
    };
    let secs = if unit.starts_with("ms") {
        value / 1000.0
    } else {
        value
    };
    std::time::Duration::from_secs_f64((secs + 0.25).max(0.5)).min(MAX_WAIT)
}

/// Uma rodada, tentando de novo nos erros passageiros.
fn chat_with_retry(
    provider: &dyn LlmProvider,
    system: &str,
    messages: &[ChatMessage],
    tools: &[ToolSpec],
) -> Result<crate::chat::ChatReply> {
    let mut attempt = 0;
    loop {
        match provider.chat(system, messages, tools) {
            // Passageiro: limite de taxa, servidor ocupado, ou o modelo pediu
            // uma ferramenta que não existe (o gpt-oss inventa as do treino
            // dele, e o Groq devolve 400 `tool_use_failed`): de novo costuma ir.
            Err(e)
                if (crate::tasks::is_transient(&e)
                    || matches!(&e, LlmError::Http(m) if m.contains("tool_use_failed")))
                    && attempt < RETRIES =>
            {
                let wait = retry_wait(&e, attempt);
                tracing::info!(attempt, ?wait, "agente: erro passageiro, tentando de novo");
                std::thread::sleep(wait);
                attempt += 1;
            }
            other => return other,
        }
    }
}

/// O que uma ferramenta pode fazer sem perguntar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Roda sem perguntar (leitura).
    Allow,
    /// Pede um toque antes (escrita).
    Ask,
    /// Não roda.
    Never,
}

/// De onde veio uma informação: o que a tela mostra como selo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// A referência que o modelo cita entre `[[ ]]` (`reuniao:42@754`).
    #[serde(rename = "ref")]
    pub reference: String,
    /// `tarefa`, `reuniao`, `ditado`, `diario`, `opttime`…
    pub kind: String,
    /// O nome para gente ("Daily do Portal · 12:34").
    pub label: String,
}

/// O que uma ferramenta devolveu.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutcome {
    /// O conteúdo para o modelo (JSON em texto, de preferência).
    pub content: String,
    /// Falhou.
    pub is_error: bool,
    /// As fontes que o resultado traz.
    pub sources: Vec<Source>,
}

impl ToolOutcome {
    /// Um erro para o modelo ler e contornar.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: message.into(),
            is_error: true,
            sources: Vec::new(),
        }
    }
}

/// Quem tem as ferramentas: o app (ou a CLI) implementa.
pub trait ToolHost {
    /// As ferramentas oferecidas ao modelo.
    fn tools(&self) -> Vec<ToolSpec>;
    /// O que a ferramenta pode fazer sem perguntar.
    fn permission(&self, name: &str) -> Permission;
    /// Roda a ferramenta.
    fn call(&self, name: &str, arguments: &Value) -> ToolOutcome;
    /// A frase do cartão de confirmação ("Lançar 1h30 no Portal do Cliente").
    fn describe(&self, name: &str, arguments: &Value) -> String;
}

/// Uma ação esperando o toque.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingAction {
    /// A chamada.
    pub call: ToolCall,
    /// O que vai acontecer, em português.
    pub description: String,
}

/// Onde o laço parou.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    /// A resposta, com as fontes citadas nela.
    Answer {
        /// O texto (com as marcas `[[ref]]`).
        text: String,
        /// As fontes citadas.
        sources: Vec<Source>,
    },
    /// Ações esperando um toque.
    Confirm {
        /// O que confirmar.
        actions: Vec<PendingAction>,
    },
}

/// Uma conversa com o agente, guardável entre um toque e outro.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    /// As mensagens até aqui.
    pub messages: Vec<ChatMessage>,
    /// Todas as fontes que as ferramentas trouxeram.
    pub sources: Vec<Source>,
    /// Rodadas com ferramentas desde a última pergunta.
    pub steps: usize,
    /// Chamadas da última resposta esperando o toque.
    pub pending: Vec<ToolCall>,
    /// Resultados já prontos da mesma resposta (as que não precisavam do toque).
    pub ready: Vec<ToolResult>,
}

impl Conversation {
    /// Uma conversa que começa com a pergunta.
    pub fn new(question: &str) -> Self {
        let mut c = Self::default();
        c.ask(question);
        c
    }

    /// Mais uma pergunta na mesma conversa. Se havia ações esperando o toque,
    /// a pessoa seguiu sem confirmar: voltam ao modelo como recusadas, sem
    /// rodar.
    pub fn ask(&mut self, question: &str) {
        if !self.pending.is_empty() {
            let pending = std::mem::take(&mut self.pending);
            let mut results = std::mem::take(&mut self.ready);
            results.extend(pending.iter().map(|call| {
                refused(
                    call,
                    "o usuário seguiu com outra pergunta sem confirmar; não rode isto sem perguntar de novo",
                )
            }));
            self.close_round(results);
        }
        self.messages.push(ChatMessage::User {
            text: question.trim().to_string(),
        });
        self.steps = 0;
    }

    /// Fecha a rodada de ferramentas com os resultados na ordem das chamadas
    /// da última resposta: o Gemini confere a contagem e o Claude, os ids.
    fn close_round(&mut self, mut by_id: Vec<ToolResult>) {
        let order: Vec<String> = match self.messages.last() {
            Some(ChatMessage::Assistant { turn }) => {
                turn.tool_calls.iter().map(|c| c.id.clone()).collect()
            }
            _ => Vec::new(),
        };
        let mut results = Vec::with_capacity(by_id.len());
        for id in &order {
            if let Some(pos) = by_id.iter().position(|r| &r.call_id == id) {
                results.push(by_id.remove(pos));
            }
        }
        results.extend(by_id);
        self.messages.push(ChatMessage::Tools { results });
    }

    fn add_sources(&mut self, sources: Vec<Source>) {
        for s in sources {
            if !self.sources.iter().any(|k| k.reference == s.reference) {
                self.sources.push(s);
            }
        }
    }
}

/// As referências `[[ref]]` citadas num texto, na ordem, sem repetir.
pub fn cited_refs(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else {
            break;
        };
        let reference = after[..end].trim().to_string();
        if !reference.is_empty() && !out.contains(&reference) {
            out.push(reference);
        }
        rest = &after[end + 2..];
    }
    out
}

/// Citações em outros formatos viram `[[ref]]`: o gpt-oss cita do jeito do
/// treino dele (`【ditado:1】`). Só o que é uma fonte conhecida muda.
fn normalize_citations(text: &str, sources: &[Source]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('【') {
        out.push_str(&rest[..start]);
        let after = &rest[start + '【'.len_utf8()..];
        match after.find('】') {
            Some(end) => {
                let inner = after[..end].trim();
                // "【ditado:1†L3】": o que vem depois da cruz é a linha.
                let reference = inner.split('†').next().unwrap_or(inner).trim();
                if sources.iter().any(|s| s.reference == reference) {
                    out.push_str(&format!("[[{reference}]]"));
                } else {
                    out.push_str(&rest[start..start + '【'.len_utf8() + end + '】'.len_utf8()]);
                }
                rest = &after[end + '】'.len_utf8()..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn clip(text: String) -> String {
    if text.chars().count() <= MAX_RESULT_CHARS {
        return text;
    }
    let head: String = text.chars().take(MAX_RESULT_CHARS).collect();
    format!("{head}\n…(cortado: resultado longo demais)")
}

fn run_tool(host: &dyn ToolHost, conv: &mut Conversation, call: &ToolCall) -> ToolResult {
    let outcome = if call.arguments.is_object() {
        host.call(&call.name, &call.arguments)
    } else {
        ToolOutcome::error("argumentos inválidos: mande um objeto JSON que siga o esquema")
    };
    conv.add_sources(outcome.sources);
    ToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: clip(outcome.content),
        is_error: outcome.is_error,
    }
}

fn refused(call: &ToolCall, why: &str) -> ToolResult {
    ToolResult {
        call_id: call.id.clone(),
        name: call.name.clone(),
        content: why.to_string(),
        is_error: true,
    }
}

fn answer(conv: &Conversation, text: String) -> Step {
    let text = normalize_citations(&text, &conv.sources);
    let sources = cited_refs(&text)
        .into_iter()
        .filter_map(|r| conv.sources.iter().find(|s| s.reference == r).cloned())
        .collect();
    Step::Answer { text, sources }
}

/// Anda a conversa até a resposta ou até uma ação pedir o toque.
pub fn advance(
    provider: &dyn LlmProvider,
    host: &dyn ToolHost,
    system: &str,
    conv: &mut Conversation,
) -> Result<Step> {
    if !conv.pending.is_empty() {
        return Err(LlmError::BadResponse(
            "há ações esperando confirmação; retome com `resume`".into(),
        ));
    }
    loop {
        // Passou do limite: a resposta sai com o que já se sabe.
        let tools = if conv.steps >= MAX_STEPS {
            Vec::new()
        } else {
            host.tools()
        };
        let mut reply = chat_with_retry(provider, system, &conv.messages, &tools)?;
        // Resposta vazia (só raciocínio, sem texto nem ferramenta): pede de novo
        // uma vez antes de devolver o vazio.
        if reply.turn.text.trim().is_empty() && reply.turn.tool_calls.is_empty() {
            reply = chat_with_retry(provider, system, &conv.messages, &tools)?;
        }
        if reply.stop == StopReason::MaxTokens && !reply.turn.tool_calls.is_empty() {
            return Err(LlmError::BadResponse(
                "a resposta foi cortada no meio de uma chamada de ferramenta".into(),
            ));
        }
        let calls = reply.turn.tool_calls.clone();
        let text = reply.turn.text.clone();
        conv.messages
            .push(ChatMessage::Assistant { turn: reply.turn });
        if calls.is_empty() {
            return Ok(answer(conv, text));
        }
        conv.steps += 1;
        let mut ready = Vec::new();
        let mut pending = Vec::new();
        for call in &calls {
            match host.permission(&call.name) {
                Permission::Allow => ready.push(run_tool(host, conv, call)),
                Permission::Never => ready.push(refused(
                    call,
                    "ferramenta bloqueada nas permissões do ISPer; responda sem ela",
                )),
                Permission::Ask => pending.push(call.clone()),
            }
        }
        if !pending.is_empty() {
            let actions = pending
                .iter()
                .map(|call| PendingAction {
                    description: host.describe(&call.name, &call.arguments),
                    call: call.clone(),
                })
                .collect();
            conv.pending = pending;
            conv.ready = ready;
            return Ok(Step::Confirm { actions });
        }
        conv.messages.push(ChatMessage::Tools { results: ready });
    }
}

/// Retoma depois do toque: roda as aprovadas (`approved` traz os ids das
/// chamadas), recusa as outras e segue a conversa.
pub fn resume(
    provider: &dyn LlmProvider,
    host: &dyn ToolHost,
    system: &str,
    conv: &mut Conversation,
    approved: &[String],
) -> Result<Step> {
    let pending = std::mem::take(&mut conv.pending);
    let mut by_id: Vec<ToolResult> = std::mem::take(&mut conv.ready);
    for call in &pending {
        let result = if approved.contains(&call.id) {
            run_tool(host, conv, call)
        } else {
            refused(
                call,
                "o usuário não autorizou esta ação; não tente de novo sem perguntar",
            )
        };
        by_id.push(result);
    }
    conv.close_round(by_id);
    advance(provider, host, system, conv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{AssistantTurn, ChatReply};
    use crate::testing::FakeChat;
    use serde_json::json;
    use std::cell::RefCell;

    struct Host {
        ran: RefCell<Vec<String>>,
    }

    impl Host {
        fn new() -> Self {
            Self {
                ran: RefCell::new(Vec::new()),
            }
        }
    }

    impl ToolHost for Host {
        fn tools(&self) -> Vec<ToolSpec> {
            ["hoje", "lancar_horas", "apagar_tudo"]
                .into_iter()
                .map(|n| ToolSpec {
                    name: n.into(),
                    description: n.into(),
                    input_schema: json!({"type": "object", "properties": {}}),
                })
                .collect()
        }
        fn permission(&self, name: &str) -> Permission {
            match name {
                "hoje" => Permission::Allow,
                "lancar_horas" => Permission::Ask,
                _ => Permission::Never,
            }
        }
        fn call(&self, name: &str, _arguments: &Value) -> ToolOutcome {
            self.ran.borrow_mut().push(name.to_string());
            ToolOutcome {
                content: format!("{{\"ok\":\"{name}\"}}"),
                is_error: false,
                sources: vec![Source {
                    reference: format!("{name}:1"),
                    kind: name.into(),
                    label: format!("Fonte {name}"),
                }],
            }
        }
        fn describe(&self, name: &str, _arguments: &Value) -> String {
            format!("Rodar {name}")
        }
    }

    fn calls(names: &[(&str, &str)]) -> ChatReply {
        ChatReply {
            turn: AssistantTurn {
                text: String::new(),
                tool_calls: names
                    .iter()
                    .map(|(id, n)| ToolCall {
                        id: (*id).into(),
                        name: (*n).into(),
                        arguments: json!({}),
                    })
                    .collect(),
                raw: Value::Null,
            },
            stop: StopReason::ToolUse,
        }
    }

    fn says(text: &str) -> ChatReply {
        ChatReply {
            turn: AssistantTurn {
                text: text.into(),
                tool_calls: Vec::new(),
                raw: Value::Null,
            },
            stop: StopReason::EndTurn,
        }
    }

    #[test]
    fn leitura_roda_sozinha_e_a_resposta_traz_as_fontes_citadas() {
        let fake = FakeChat::new(vec![
            calls(&[("c1", "hoje")]),
            says("Você tem 3 tarefas hoje [[hoje:1]]. [[inventada:9]]"),
        ]);
        let host = Host::new();
        let mut conv = Conversation::new("o que tenho hoje?");
        let step = advance(&fake, &host, "sys", &mut conv).unwrap();
        let Step::Answer { text, sources } = step else {
            panic!("esperava resposta");
        };
        assert!(text.contains("3 tarefas"));
        assert_eq!(sources.len(), 1, "fonte inventada fica de fora");
        assert_eq!(sources[0].label, "Fonte hoje");
        assert_eq!(*host.ran.borrow(), ["hoje"]);
        assert_eq!(
            conv.messages.len(),
            4,
            "pergunta, chamada, resultado, resposta"
        );
        let sent = fake.messages(1);
        assert!(
            matches!(&sent[2], ChatMessage::Tools { results } if results[0].content.contains("ok"))
        );
    }

    #[test]
    fn escrita_para_no_toque_e_retoma_com_a_decisao() {
        let fake = FakeChat::new(vec![
            calls(&[("c1", "hoje"), ("c2", "lancar_horas")]),
            says("Lancei."),
            calls(&[("c3", "lancar_horas")]),
            says("Tudo bem, não lancei."),
        ]);
        let host = Host::new();
        let mut conv = Conversation::new("lança 1h");
        let Step::Confirm { actions } = advance(&fake, &host, "sys", &mut conv).unwrap() else {
            panic!("esperava confirmação");
        };
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].description, "Rodar lancar_horas");
        assert_eq!(*host.ran.borrow(), ["hoje"], "a escrita ainda não rodou");
        // A conversa vai e volta do JSON entre um toque e outro.
        let mut conv: Conversation =
            serde_json::from_value(serde_json::to_value(&conv).unwrap()).unwrap();
        let step = resume(&fake, &host, "sys", &mut conv, &["c2".into()]).unwrap();
        assert!(matches!(step, Step::Answer { .. }));
        assert_eq!(*host.ran.borrow(), ["hoje", "lancar_horas"]);
        let sent = fake.messages(1);
        let ChatMessage::Tools { results } = &sent[2] else {
            panic!("esperava os resultados");
        };
        let ids: Vec<&str> = results.iter().map(|r| r.call_id.as_str()).collect();
        assert_eq!(ids, ["c1", "c2"], "na ordem das chamadas");

        // Recusar volta ao modelo como recusa, sem rodar.
        conv.ask("lança mais 1h");
        let Step::Confirm { .. } = advance(&fake, &host, "sys", &mut conv).unwrap() else {
            panic!("esperava confirmação");
        };
        resume(&fake, &host, "sys", &mut conv, &[]).unwrap();
        assert_eq!(host.ran.borrow().len(), 2);
        let sent = fake.messages(3);
        let ChatMessage::Tools { results } = sent.last().unwrap() else {
            panic!("esperava a recusa");
        };
        assert!(results[0].is_error && results[0].content.contains("não autorizou"));
    }

    #[test]
    fn outra_pergunta_com_o_cartao_aberto_recusa_sem_rodar() {
        let fake = FakeChat::new(vec![
            calls(&[("c1", "hoje"), ("c2", "lancar_horas")]),
            says("Certo, não lancei."),
        ]);
        let host = Host::new();
        let mut conv = Conversation::new("lança 1h");
        let Step::Confirm { .. } = advance(&fake, &host, "sys", &mut conv).unwrap() else {
            panic!("esperava confirmação");
        };
        conv.ask("deixa, o que tenho amanhã?");
        assert!(conv.pending.is_empty() && conv.ready.is_empty());
        advance(&fake, &host, "sys", &mut conv).unwrap();
        assert_eq!(*host.ran.borrow(), ["hoje"], "a escrita não rodou");
        let sent = fake.messages(1);
        let ChatMessage::Tools { results } = &sent[2] else {
            panic!("esperava os resultados da rodada aberta");
        };
        let ids: Vec<&str> = results.iter().map(|r| r.call_id.as_str()).collect();
        assert_eq!(ids, ["c1", "c2"]);
        assert!(!results[0].is_error && results[1].is_error);
        assert!(matches!(&sent[3], ChatMessage::User { text } if text.contains("amanhã")));
    }

    #[test]
    fn bloqueada_nao_roda_nem_pergunta() {
        let fake = FakeChat::new(vec![calls(&[("c1", "apagar_tudo")]), says("Não posso.")]);
        let host = Host::new();
        let mut conv = Conversation::new("apaga tudo");
        let step = advance(&fake, &host, "sys", &mut conv).unwrap();
        assert!(matches!(step, Step::Answer { .. }));
        assert!(host.ran.borrow().is_empty());
    }

    #[test]
    fn passou_do_limite_responde_sem_ferramentas() {
        let mut script: Vec<ChatReply> = (0..MAX_STEPS)
            .map(|i| calls(&[(&format!("c{i}"), "hoje")]))
            .collect();
        script.push(says("Resumo com o que achei."));
        let fake = FakeChat::new(script);
        let host = Host::new();
        let mut conv = Conversation::new("tudo");
        assert!(matches!(
            advance(&fake, &host, "sys", &mut conv).unwrap(),
            Step::Answer { .. }
        ));
        assert!(
            fake.tools(MAX_STEPS).is_empty(),
            "a última rodada vai sem ferramentas"
        );
        assert!(!fake.tools(0).is_empty());
    }

    #[test]
    fn chamada_cortada_nao_roda() {
        let mut cortada = calls(&[("c1", "hoje")]);
        cortada.stop = StopReason::MaxTokens;
        let fake = FakeChat::new(vec![cortada]);
        let host = Host::new();
        let mut conv = Conversation::new("x");
        assert!(advance(&fake, &host, "sys", &mut conv).is_err());
        assert!(host.ran.borrow().is_empty());
    }

    #[test]
    fn espera_o_que_a_api_mandou() {
        let e = |m: &str| LlmError::Http(m.into());
        let w = retry_wait(
            &e("status 429: ... Please try again in 6.765s. Need more"),
            0,
        );
        assert!((w.as_secs_f64() - 7.015).abs() < 0.01);
        assert!(
            retry_wait(&e("status 429: try again in 630ms."), 0)
                < std::time::Duration::from_secs(1)
        );
        assert_eq!(retry_wait(&e("status 429: try again in 90s"), 0), MAX_WAIT);
        assert_eq!(
            retry_wait(&e("status 503"), 1),
            std::time::Duration::from_secs(6)
        );
        let g = retry_wait(
            &e("status 429: Please retry in 12.5s. \"retryDelay\": \"12s\""),
            0,
        );
        assert!((g.as_secs_f64() - 12.75).abs() < 0.01);
    }

    #[test]
    fn erro_passageiro_tenta_de_novo() {
        let fake = FakeChat::new(vec![says("ok")]);
        struct Flaky<'a> {
            inner: &'a FakeChat,
            fails: std::sync::atomic::AtomicUsize,
        }
        impl LlmProvider for Flaky<'_> {
            fn name(&self) -> &'static str {
                "flaky"
            }
            fn model(&self) -> &str {
                "flaky"
            }
            fn complete(&self, _: &str, _: &str) -> Result<String> {
                Err(LlmError::Unsupported("x".into()))
            }
            fn list_models(&self) -> Result<Vec<String>> {
                Ok(Vec::new())
            }
            fn chat(
                &self,
                system: &str,
                messages: &[ChatMessage],
                tools: &[ToolSpec],
            ) -> Result<ChatReply> {
                use std::sync::atomic::Ordering;
                if self.fails.load(Ordering::SeqCst) > 0 {
                    self.fails.fetch_sub(1, Ordering::SeqCst);
                    return Err(LlmError::Http("status 429: try again in 10ms.".into()));
                }
                self.inner.chat(system, messages, tools)
            }
        }
        let flaky = Flaky {
            inner: &fake,
            fails: std::sync::atomic::AtomicUsize::new(2),
        };
        let host = Host::new();
        let mut conv = Conversation::new("x");
        assert!(matches!(
            advance(&flaky, &host, "s", &mut conv).unwrap(),
            Step::Answer { .. }
        ));
    }

    #[test]
    fn citacao_do_gpt_oss_vira_a_nossa() {
        let fontes = vec![Source {
            reference: "ditado:1".into(),
            kind: "ditado".into(),
            label: "Ditado".into(),
        }];
        assert_eq!(
            normalize_citations("Você ditou isso【ditado:1†L1】 e aquilo【outra】.", &fontes),
            "Você ditou isso[[ditado:1]] e aquilo【outra】."
        );
    }

    #[test]
    fn resposta_vazia_pede_de_novo() {
        let fake = FakeChat::new(vec![says(""), says("Agora sim.")]);
        let host = Host::new();
        let mut conv = Conversation::new("x");
        let Step::Answer { text, .. } = advance(&fake, &host, "s", &mut conv).unwrap() else {
            panic!("esperava resposta");
        };
        assert_eq!(text, "Agora sim.");
        assert_eq!(fake.rounds(), 2);
    }

    #[test]
    fn referencias_citadas() {
        assert_eq!(
            cited_refs("a [[reuniao:42@754]] b [[tarefa:x]] c [[reuniao:42@754]] [[ ]] [[sem fim"),
            ["reuniao:42@754", "tarefa:x"]
        );
    }
}
