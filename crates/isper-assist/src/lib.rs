//! O assistente do ISPer (Fase 10): o dia num lugar só
//! ([ADR 0021](../../../docs/adr/0021-assistente-pessoal-no-isper.md)).
//!
//! Este crate guarda o domínio local do assistente sobre o `isper.db`:
//!
//! - [`model`]: tarefa, origem, estado e o pedido de mudança;
//! - [`store`]: o [`AssistStore`], que cria, muda, conclui e desfaz tarefas,
//!   monta o dia ([`Today`]) e escreve tudo no diário;
//! - [`clock`]: o relógio, injetável nos testes, que diz que dia é "hoje".
//!
//! O schema é a v7 do banco, migrada pelo `isper-core` (uma cadeia de
//! migrações só). Nada aqui apaga: descartar é um estado, e cada mudança fica
//! no diário com o "antes", que é o que o desfazer usa
//! ([ADR 0009](../../../docs/adr/0009-nada-some-sem-o-usuario-pedir.md)).

pub mod capture;
pub mod clock;
pub mod model;
pub mod store;
pub mod when;

pub use clock::{Clock, FixedClock, SystemClock};
pub use model::{Actor, JournalEntry, NewTask, SourceKind, Task, TaskPatch, TaskStatus, Today};
pub use store::{AssistStore, ImportOutcome, SCHEMA_VERSION_REQUIRED};

/// Erros do assistente.
#[derive(Debug, thiserror::Error)]
pub enum AssistError {
    /// O banco ainda não passou pela v7 (o `isper-core` migra ao abrir).
    #[error(
        "o banco está na versão {0} do schema; o assistente precisa da {SCHEMA_VERSION_REQUIRED}"
    )]
    NotMigrated(i64),
    /// Tarefa (ou entrada do diário) que não existe.
    #[error("não encontrei {0}")]
    NotFound(String),
    /// Entrada que não passa na validação (título vazio, hora inválida…).
    #[error("{0}")]
    Invalid(String),
    /// O desfazer não vale mais: a tarefa mudou de novo depois.
    #[error("a tarefa mudou depois disso; não dá para desfazer esta mudança")]
    UndoStale,
    /// Erro do SQLite.
    #[error("banco: {0}")]
    Sql(#[from] rusqlite::Error),
    /// JSON guardado no banco que não abre.
    #[error("dado guardado ilegível: {0}")]
    Json(#[from] serde_json::Error),
}

/// Resultado do assistente.
pub type Result<T> = std::result::Result<T, AssistError>;
