//! Provider falso para os testes: nenhuma rede, resposta sob controle e
//! registro de tudo que foi pedido — assim resumo, título, polimento e
//! insights são testados de ponta a ponta (prompt → chamada → pós-processamento)
//! sem chave de API nem provider real.
//!
//! Os testes do crate o usam direto; quem depende do `isper-llm` liga a
//! feature `testing` nas `dev-dependencies` (o app testa assim o resumo que
//! refaz depois do passe final).

use std::sync::Mutex;

use crate::providers::LlmProvider;
use crate::systemone::{Answer, Classifier, Decision, Question};
use crate::{LlmError, Result};

type Reply = Box<dyn Fn(&str, &str) -> Result<String> + Send + Sync>;

/// Um [`LlmProvider`] de mentira: responde o que o teste mandar (texto fixo,
/// erro ou uma função do prompt) e guarda cada par `(system, user)` recebido.
pub struct FakeProvider {
    reply: Reply,
    calls: Mutex<Vec<(String, String)>>,
}

impl FakeProvider {
    /// Responde sempre o mesmo texto.
    pub fn replying(text: &str) -> Self {
        let text = text.to_string();
        Self::with(move |_, _| Ok(text.clone()))
    }

    /// Falha sempre com o erro construído por `err` (o `LlmError` não é
    /// clonável, por isso uma função).
    pub fn failing(err: impl Fn() -> LlmError + Send + Sync + 'static) -> Self {
        Self::with(move |_, _| Err(err()))
    }

    /// Resposta calculada a partir do prompt `(system, user)`.
    pub fn with(reply: impl Fn(&str, &str) -> Result<String> + Send + Sync + 'static) -> Self {
        Self {
            reply: Box::new(reply),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Todas as chamadas recebidas, na ordem: `(system, user)`.
    pub fn calls(&self) -> Vec<(String, String)> {
        self.calls
            .lock()
            .expect("registro do provider falso")
            .clone()
    }

    /// A única chamada recebida — o caso comum; falha se houve 0 ou 2+.
    pub fn single_call(&self) -> (String, String) {
        let calls = self.calls();
        assert_eq!(
            calls.len(),
            1,
            "esperava exatamente uma chamada ao provider"
        );
        calls.into_iter().next().expect("uma chamada")
    }
}

impl LlmProvider for FakeProvider {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn model(&self) -> &str {
        "fake-1"
    }

    fn complete(&self, system: &str, user: &str) -> Result<String> {
        self.calls
            .lock()
            .expect("registro do provider falso")
            .push((system.to_string(), user.to_string()));
        (self.reply)(system, user)
    }

    fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![self.model().to_string()])
    }
}

type Verdict = Box<dyn Fn(&serde_json::Value) -> Result<Decision> + Send + Sync>;

/// Um [`Classifier`] de mentira para o filtro do Copilot: responde o que o
/// teste mandar a partir do `state` e guarda cada `state` recebido.
pub struct FakeClassifier {
    reply: Verdict,
    calls: Mutex<Vec<serde_json::Value>>,
}

impl FakeClassifier {
    /// Resposta calculada a partir do `state` (ex.: marcar só o que tiver
    /// "combinado" no parágrafo atual).
    pub fn with(
        reply: impl Fn(&serde_json::Value) -> Result<Decision> + Send + Sync + 'static,
    ) -> Self {
        Self {
            reply: Box::new(reply),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Sempre a mesma chance de card, com o tipo `kind` como o mais provável.
    pub fn card_probability(p_card: f32, kind: &'static str) -> Self {
        Self::with(move |_| Ok(filter_decision(p_card, kind)))
    }

    /// Falha sempre com o erro construído por `err`.
    pub fn failing(err: impl Fn() -> LlmError + Send + Sync + 'static) -> Self {
        Self::with(move |_| Err(err()))
    }

    pub fn calls(&self) -> Vec<serde_json::Value> {
        self.calls
            .lock()
            .expect("registro do classificador falso")
            .clone()
    }
}

impl Classifier for FakeClassifier {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn classify(&self, state: &serde_json::Value, _: &[(&str, Question)]) -> Result<Decision> {
        self.calls
            .lock()
            .expect("registro do classificador falso")
            .push(state.clone());
        (self.reply)(state)
    }
}

/// A resposta do filtro com `p_card` = 1 − p(nada), toda a massa restante no
/// tipo `kind` (`"decision"`, `"action"` ou `"risk"`).
pub fn filter_decision(p_card: f32, kind: &str) -> Decision {
    let probabilities = ["decision", "action", "risk", "none"]
        .iter()
        .map(|l| {
            let p = match *l {
                "none" => 1.0 - p_card,
                l if l == kind => p_card,
                _ => 0.0,
            };
            ((*l).to_string(), p)
        })
        .collect();
    Decision {
        model: "fake-jev".into(),
        answers: vec![(
            "kind".into(),
            Answer::Choice {
                chosen: String::new(),
                probabilities,
                confidence: 0.5,
            },
        )],
        input_tokens: 600,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classificador_falso_responde_e_registra() {
        let fake = FakeClassifier::card_probability(0.8, "risk");
        let d = fake
            .classify(
                &serde_json::json!({"atual": "x"}),
                &crate::filter_questions(),
            )
            .unwrap();
        let v = crate::read_filter(&d).unwrap();
        assert!(v.flagged);
        assert_eq!(v.kind, crate::CardKind::Risk);
        assert_eq!(fake.calls().len(), 1);
    }

    #[test]
    fn registra_chamadas_e_responde_o_combinado() {
        let fake = FakeProvider::replying("ok");
        assert_eq!(fake.complete("s", "u").unwrap(), "ok");
        assert_eq!(fake.single_call(), ("s".to_string(), "u".to_string()));
        assert_eq!(fake.list_models().unwrap(), vec!["fake-1".to_string()]);
    }

    #[test]
    fn falha_quando_mandado() {
        let fake = FakeProvider::failing(|| LlmError::Http("status 500: boom".into()));
        assert!(matches!(fake.complete("s", "u"), Err(LlmError::Http(m)) if m.contains("boom")));
        assert_eq!(fake.calls().len(), 1);
    }
}
