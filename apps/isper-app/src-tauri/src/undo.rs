//! Desfazer em vez de "tem certeza?": exclusões adiadas.
//!
//! Excluir uma reunião, um ditado ou um modelo responde na hora com um
//! token; a interface esconde o item e mostra "Desfazer" por
//! [`UNDO_WINDOW`]. Só depois disso a exclusão acontece de fato — e
//! [`undo_delete`] a cancela enquanto isso. É o padrão de quem lida com
//! dados que importam: a confirmação em dois cliques treina o usuário a
//! clicar duas vezes sem ler; o desfazer não pede atenção antes e perdoa
//! depois.
//!
//! O que fica pendente mora no processo (a bandeja vive o dia todo), não
//! na janela: fechar a Biblioteca durante a janela de desfazer não perde
//! nem ressuscita nada. Se o app sair nesse intervalo, [`flush_all`] aplica
//! as exclusões pendentes — o usuário pediu para apagar.

use std::collections::BTreeMap;

use crate::prelude::*;

/// Quanto tempo o "Desfazer" fica disponível.
pub(crate) const UNDO_WINDOW: Duration = Duration::from_secs(7);

/// O que vai ser apagado quando a janela de desfazer fechar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Doomed {
    Meeting(i64),
    Dictation(i64),
    Model(String),
}

impl Doomed {
    fn execute(&self) -> anyhow::Result<()> {
        match self {
            Doomed::Meeting(id) => open_store()?.delete_meeting(*id)?,
            Doomed::Dictation(id) => open_store()?.delete_dictation(*id)?,
            Doomed::Model(file) => isper_models::remove(file)?,
        }
        Ok(())
    }
}

/// Fila das exclusões adiadas. Pura (sem relógio nem banco) para ser testada.
#[derive(Debug, Default)]
pub(crate) struct Pending {
    next: u64,
    items: BTreeMap<u64, Doomed>,
}

impl Pending {
    pub(crate) const fn new() -> Self {
        Self {
            next: 0,
            items: BTreeMap::new(),
        }
    }

    /// Agenda `d` e devolve o token. O mesmo item pedido duas vezes (duplo
    /// clique) mantém o token da primeira.
    pub(crate) fn push(&mut self, d: Doomed) -> u64 {
        if let Some((token, _)) = self.items.iter().find(|(_, x)| **x == d) {
            return *token;
        }
        self.next += 1;
        self.items.insert(self.next, d);
        self.next
    }

    /// Tira da fila (desfazer, ou a hora de apagar chegou).
    pub(crate) fn take(&mut self, token: u64) -> Option<Doomed> {
        self.items.remove(&token)
    }

    /// Tudo o que ainda está pendente, na ordem em que foi pedido.
    pub(crate) fn drain(&mut self) -> Vec<Doomed> {
        std::mem::take(&mut self.items).into_values().collect()
    }

    pub(crate) fn meetings(&self) -> Vec<i64> {
        self.items
            .values()
            .filter_map(|d| match d {
                Doomed::Meeting(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn dictations(&self) -> Vec<i64> {
        self.items
            .values()
            .filter_map(|d| match d {
                Doomed::Dictation(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn models(&self) -> Vec<String> {
        self.items
            .values()
            .filter_map(|d| match d {
                Doomed::Model(f) => Some(f.clone()),
                _ => None,
            })
            .collect()
    }
}

static PENDING: Mutex<Pending> = Mutex::new(Pending::new());

fn pending() -> MutexGuard<'static, Pending> {
    PENDING.lock_or_recover()
}

/// Resposta de uma exclusão: o token para desfazer e por quanto tempo vale.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct Scheduled {
    pub(crate) token: u64,
    pub(crate) undo_ms: u64,
}

/// Agenda a exclusão e dispara o relógio da janela de desfazer.
pub(crate) fn schedule(d: Doomed) -> Scheduled {
    let token = pending().push(d);
    std::thread::spawn(move || {
        std::thread::sleep(UNDO_WINDOW);
        commit(token);
    });
    Scheduled {
        token,
        undo_ms: UNDO_WINDOW.as_millis() as u64,
    }
}

fn commit(token: u64) {
    let Some(d) = pending().take(token) else {
        return; // desfeito a tempo
    };
    match d.execute() {
        Ok(()) => tracing::info!(?d, "exclusão aplicada"),
        Err(e) => tracing::warn!(?d, "não consegui excluir: {e}"),
    }
}

/// Aplica o que estiver pendente — chamado na saída do app.
pub(crate) fn flush_all() {
    let items = pending().drain();
    for d in items {
        if let Err(e) = d.execute() {
            tracing::warn!(?d, "não consegui excluir na saída: {e}");
        }
    }
}

/// Itens escondidos das listas enquanto a janela de desfazer está aberta.
pub(crate) fn hidden_meetings() -> Vec<i64> {
    pending().meetings()
}
pub(crate) fn hidden_dictations() -> Vec<i64> {
    pending().dictations()
}
pub(crate) fn hidden_models() -> Vec<String> {
    pending().models()
}

/// Cancela uma exclusão. `false` = tarde demais (já apagou) ou token inválido.
#[tauri::command]
pub(crate) fn undo_delete(token: u64) -> bool {
    let undone = pending().take(token);
    if let Some(d) = &undone {
        tracing::info!(?d, "exclusão desfeita");
    }
    undone.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agenda_desfaz_e_nao_duplica() {
        let mut p = Pending::new();
        let a = p.push(Doomed::Meeting(7));
        let b = p.push(Doomed::Dictation(3));
        assert_ne!(a, b);
        assert_eq!(p.push(Doomed::Meeting(7)), a, "duplo clique mantém o token");
        assert_eq!(p.meetings(), vec![7]);
        assert_eq!(p.dictations(), vec![3]);
        assert_eq!(p.take(a), Some(Doomed::Meeting(7)));
        assert_eq!(p.take(a), None, "desfazer duas vezes não faz nada");
        assert!(p.meetings().is_empty());
    }

    #[test]
    fn saida_aplica_tudo_na_ordem_e_esvazia() {
        let mut p = Pending::new();
        p.push(Doomed::Model("ggml-small.bin".into()));
        p.push(Doomed::Meeting(1));
        assert_eq!(p.models(), vec!["ggml-small.bin".to_string()]);
        let all = p.drain();
        assert_eq!(
            all,
            vec![Doomed::Model("ggml-small.bin".into()), Doomed::Meeting(1)]
        );
        assert!(p.drain().is_empty());
    }

    #[test]
    fn tokens_nao_se_repetem_depois_de_desfeitos() {
        let mut p = Pending::new();
        let a = p.push(Doomed::Meeting(1));
        p.take(a);
        let b = p.push(Doomed::Meeting(1));
        assert!(b > a, "um token velho nunca desfaz uma exclusão nova");
    }
}
