//! Provider falso para os testes do crate: nenhuma rede, resposta sob controle
//! e registro de tudo que foi pedido — assim resumo, título, polimento e
//! insights são testados de ponta a ponta (prompt → chamada → pós-processamento)
//! sem chave de API nem provider real.

use std::sync::Mutex;

use crate::providers::LlmProvider;
use crate::{LlmError, Result};

type Reply = Box<dyn Fn(&str, &str) -> Result<String> + Send + Sync>;

/// Um [`LlmProvider`] de mentira: responde o que o teste mandar (texto fixo,
/// erro ou uma função do prompt) e guarda cada par `(system, user)` recebido.
pub(crate) struct FakeProvider {
    reply: Reply,
    calls: Mutex<Vec<(String, String)>>,
}

impl FakeProvider {
    /// Responde sempre o mesmo texto.
    pub(crate) fn replying(text: &str) -> Self {
        let text = text.to_string();
        Self::with(move |_, _| Ok(text.clone()))
    }

    /// Falha sempre com o erro construído por `err` (o `LlmError` não é
    /// clonável, por isso uma função).
    pub(crate) fn failing(err: impl Fn() -> LlmError + Send + Sync + 'static) -> Self {
        Self::with(move |_, _| Err(err()))
    }

    /// Resposta calculada a partir do prompt `(system, user)`.
    pub(crate) fn with(
        reply: impl Fn(&str, &str) -> Result<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            reply: Box::new(reply),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Todas as chamadas recebidas, na ordem: `(system, user)`.
    pub(crate) fn calls(&self) -> Vec<(String, String)> {
        self.calls
            .lock()
            .expect("registro do provider falso")
            .clone()
    }

    /// A única chamada recebida — o caso comum; falha se houve 0 ou 2+.
    pub(crate) fn single_call(&self) -> (String, String) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
