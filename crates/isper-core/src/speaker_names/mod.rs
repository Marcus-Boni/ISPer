//! Nomes dos participantes de uma chamada do Teams, lidos da própria janela
//! da chamada (ideia do Yogurt, FrigadeHQ/yogurt).
//!
//! ```text
//!  janela da chamada ──UI Automation──► árvore ──teams──► leitura
//!   (Teams gravado)      (uia, Windows)   (tree)            (quem está,
//!                                                            quem fala)
//!                                                               │ no relógio
//!                                                               ▼ da reunião
//!  falas do passe final ("Participante N", início, fim) ──► attribute ──► nome
//!                                                                      ou nada
//! ```
//!
//! O núcleo daqui é puro: a árvore é um valor, o relógio é um número e a
//! atribuição é uma função — os testes rodam em qualquer sistema, com
//! fixtures de `fixtures/teams`. Só [`uia`] fala com o Windows. Quem roda o
//! leitor num subprocesso, amarra as leituras ao áudio e guarda os nomes é o
//! app (`teams_names.rs`).
//!
//! O que nunca acontece: nome para "Eu" (o microfone já diz quem é), nome
//! sem evidência no próprio trecho, nome quando o Teams marca duas pessoas,
//! ou quando o áudio gravado não é o do Teams que está sendo lido.

pub mod attribute;
pub mod teams;
pub mod tree;
#[cfg(windows)]
pub mod uia;

pub use attribute::{
    Attribution, AttributionReport, Observation, ObservationState, RemoteSegment, Rules, attribute,
    group_of,
};
pub use teams::{CallReading, is_call_window, read_call};
pub use tree::{UiNode, UiWindow};
