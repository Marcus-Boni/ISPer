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

mod polish;
mod providers;
mod settings;
mod summary;

pub use polish::{POLISH_STYLES, polish_dictation};
pub use providers::{LlmProvider, provider_from_settings};
pub use settings::{
    LlmSettings, delete_api_key, get_api_key, load_settings, save_settings, set_api_key,
};
pub use summary::{MeetingSummary, summarize_meeting, summarize_meeting_titled};

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
