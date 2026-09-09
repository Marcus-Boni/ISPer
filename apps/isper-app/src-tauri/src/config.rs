//! Configurações do app — persistidas em `%APPDATA%\ISPer\config.toml`.
//! (As configurações de IA vivem em `llm.toml`, cuidadas pelo isper-llm;
//! a chave de API vive no Credential Manager, nunca em arquivo.)

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Atalho preferido (ex.: "ctrl+alt+space"). `None` = automático
    /// (primeiro livre da lista de candidatos).
    #[serde(default)]
    pub shortcut: Option<String>,
    /// Idioma da fala: "pt", "en", ... ou "auto".
    #[serde(default = "default_lang")]
    pub lang: String,
    /// Dicionário pessoal: termos que o Whisper costuma errar
    /// (nomes próprios, siglas, jargões).
    #[serde(default)]
    pub dictionary: Vec<String>,
    /// Arquivo do modelo Whisper (nome do catálogo). `None` = melhor disponível.
    #[serde(default)]
    pub model: Option<String>,
    /// Fonte do áudio dos participantes: `system` · `teams` · `process:<exe>`.
    #[serde(default = "default_source")]
    pub meeting_source: String,
    /// Onde o usuário deixou o indicador (pixels físicos). `None` = rodapé centralizado.
    #[serde(default)]
    pub overlay_pos: Option<(i32, i32)>,
    /// Indicador em modo mini (só o ponto + cronômetro).
    #[serde(default)]
    pub overlay_mini: bool,
    /// Abrir a tela Início quando o usuário abre o ISPer (nunca no autostart).
    #[serde(default = "default_true")]
    pub show_home_on_launch: bool,
    /// Microfone do ditado e do canal "Eu" da reunião (`None` = padrão do sistema).
    #[serde(default)]
    pub input_device: Option<String>,
    /// Atalho global que inicia/encerra a gravação de reunião.
    #[serde(default = "default_meeting_shortcut")]
    pub meeting_shortcut: Option<String>,
    /// Polimento do ditado por IA antes de colar (só o texto sai da máquina;
    /// usa o provider já configurado; qualquer falha cola o original).
    #[serde(default)]
    pub polish: bool,
    /// `clean` (só limpeza) · `formal` · `casual`.
    #[serde(default = "default_polish_style")]
    pub polish_style: String,
    /// Ao salvar a reunião: `notify` (toast do Windows; clicar abre a
    /// Biblioteca) · `open` (abre o `.md` no app padrão) · `silent`.
    #[serde(default = "default_after_meeting")]
    pub after_meeting: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            shortcut: None,
            lang: default_lang(),
            dictionary: Vec::new(),
            model: None,
            meeting_source: default_source(),
            overlay_pos: None,
            overlay_mini: false,
            show_home_on_launch: true,
            input_device: None,
            meeting_shortcut: default_meeting_shortcut(),
            polish: false,
            polish_style: default_polish_style(),
            after_meeting: default_after_meeting(),
        }
    }
}

fn default_meeting_shortcut() -> Option<String> {
    Some("ctrl+alt+m".into())
}

fn default_after_meeting() -> String {
    "notify".into()
}

fn default_polish_style() -> String {
    "clean".into()
}

fn default_lang() -> String {
    "pt".into()
}

fn default_true() -> bool {
    true
}

fn default_source() -> String {
    "system".into()
}

impl AppConfig {
    /// O dicionário vira o `initial_prompt` do Whisper: o modelo tende a
    /// grafar corretamente os termos que "acabou de ver".
    pub fn initial_prompt(&self) -> Option<String> {
        if self.dictionary.is_empty() {
            None
        } else {
            Some(format!("Vocabulário: {}.", self.dictionary.join(", ")))
        }
    }
}

pub fn path() -> Option<PathBuf> {
    std::env::var("APPDATA")
        .ok()
        .map(|a| PathBuf::from(a).join("ISPer").join("config.toml"))
}

pub fn load() -> AppConfig {
    let Some(p) = path() else {
        return AppConfig::default();
    };
    match std::fs::read_to_string(&p) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("config.toml inválido ({e}) — usando padrão");
            AppConfig::default()
        }),
        Err(_) => AppConfig::default(),
    }
}

/// Grava de forma atômica: escreve num `.tmp` ao lado e renomeia por cima.
/// Um desligamento no meio da escrita nunca deixa um `config.toml` truncado
/// (que viraria "config inválido → padrão" no próximo início).
pub fn save(cfg: &AppConfig) -> anyhow::Result<()> {
    let p = path().ok_or_else(|| anyhow::anyhow!("APPDATA não definido"))?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = p.with_extension("toml.tmp");
    std::fs::write(&tmp, toml::to_string_pretty(cfg)?)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}
