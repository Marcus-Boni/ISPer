//! isper-llm — a camada de inteligência do ISPer (Fase 5).
//!
//! Por decisão de projeto, o LLM é de **nuvem via API** (rodar um local
//! pesaria na máquina, que já dedica a GPU ao Whisper). Princípios:
//!
//! - **Privacidade**: só o TEXTO do transcript sai da máquina — áudio nunca.
//!   A chave de API mora no Credential Manager do Windows (crate `keyring`),
//!   não em arquivo de configuração nem em variável hardcoded.
//! - **Provider trocável**: o trait [`LlmProvider`] abstrai a API — Claude,
//!   Groq e Gemini implementados; trocar é editar uma linha de configuração.
//! - **Decidir não é escrever**: o que só precisa de uma decisão (este
//!   parágrafo merece card?) vai ao trait [`Classifier`] — hoje o Jev, da
//!   TypeSafe, que responde em ~0,4 s e custa uma fração de centavo por hora
//!   de reunião ([`systemone`]).

pub mod copilot;
pub mod embeddings;
mod insights;
pub mod meeting_actions;
mod polish;
mod providers;
mod settings;
mod summary;
pub mod systemone;
pub mod tasks;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use copilot::{
    CardKind, CardStatus, CardUrgency, CopilotAnalysis, CopilotCard, CopilotInput,
    FILTER_MIN_WORDS, FILTER_THRESHOLD, FilterVerdict, FocusHint, TriggerKind, analyze_meeting,
    card_id, detect_trigger, enrich_notes, enrich_notes_stream, filter_questions, filter_state,
    is_same_card, query_meeting, query_meeting_stream, read_filter, render_decisions_markdown,
};
pub use embeddings::{Embedder, EmbeddingSettings, embedder_from_settings};
pub use insights::{InsightsInput, live_insights};
pub use meeting_actions::{MyAction, TimedLine, extract_my_actions};
pub use polish::{POLISH_STYLES, polish_dictation};
pub use providers::{LlmProvider, provider_from_settings};
pub use settings::{
    LlmSettings, delete_api_key, get_api_key, load_settings, save_settings, set_api_key,
};
pub use summary::{MeetingSummary, summarize_meeting, summarize_meeting_titled};
pub use systemone::{Answer, Classifier, Decision, JEV_MODEL, Jev, Question, TYPESAFE_KEY};
pub use tasks::{ExtractedTask, extract_tasks};

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("nenhum provider de IA configurado — rode `isper-cli llm use <claude|groq|gemini>`")]
    NotConfigured,
    #[error("chave de API não encontrada para '{0}' — rode `isper-cli llm set-key {0}`")]
    NoApiKey(String),
    #[error("provider desconhecido: '{0}' (opções: claude, groq, gemini)")]
    UnknownProvider(String),
    #[error(
        "o modelo '{0}' não existe ou sua conta não tem acesso a ele — liste os disponíveis (`isper-cli llm models` ou o botão 'Listar modelos' nas Configurações) e escolha outro"
    )]
    ModelNotFound(String),
    #[error("erro HTTP da API: {0}")]
    Http(String),
    #[error("resposta inesperada da API: {0}")]
    BadResponse(String),
    #[error("a API recusou a solicitação ({0})")]
    Refused(String),
    #[error("erro de credencial: {0}")]
    Keyring(String),
    #[error("erro de E/S: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, LlmError>;
