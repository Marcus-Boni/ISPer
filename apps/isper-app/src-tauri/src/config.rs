//! Configurações do app — persistidas em `%APPDATA%\ISPer\config.toml`.
//! (As configurações de IA vivem em `llm.toml`, cuidadas pelo isper-llm;
//! a chave de API vive no Credential Manager, nunca em arquivo.)

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Modos de `after_meeting`.
pub(crate) const AFTER_MEETING_MODES: [&str; 3] = ["notify", "open", "silent"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Comandos de voz no ditado ("nova linha", "ponto final", "apagar isso"…).
    #[serde(default = "default_true")]
    pub voice_commands: bool,
    /// Atalho global que marca um momento durante a reunião.
    #[serde(default = "default_mark_shortcut")]
    pub mark_shortcut: Option<String>,
    /// Indicador no modo "legendas ao vivo" (barra larga com as últimas falas).
    #[serde(default)]
    pub overlay_captions: bool,
    /// Consultar se há versão nova ao abrir e uma vez por dia (só lê um JSON;
    /// instalar é sempre um clique do usuário).
    #[serde(default = "default_true")]
    pub auto_update_check: bool,
    /// Chamada do Teams detectada: `notify` (avisa e pergunta se grava) ·
    /// `auto` (começa a gravar sozinho) · `off`.
    #[serde(default = "default_call_detect")]
    pub call_detect: String,
    /// Insights ao vivo durante a reunião (pendências, compromissos, decisões)
    /// — opt-in: custa chamadas de API a cada rodada.
    #[serde(default)]
    pub live_insights: bool,
    /// Intervalo entre rodadas de insights, em minutos.
    #[serde(default = "default_insights_interval")]
    pub insights_interval_min: u32,
    /// Indicador flutuante fixo na tela mesmo em repouso (botão "Indicador"
    /// do Início / bandeja); "ocultar" desfixa.
    #[serde(default)]
    pub overlay_pinned: bool,
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
            voice_commands: true,
            mark_shortcut: default_mark_shortcut(),
            overlay_captions: false,
            auto_update_check: true,
            call_detect: default_call_detect(),
            live_insights: false,
            insights_interval_min: default_insights_interval(),
            overlay_pinned: false,
        }
    }
}

fn default_call_detect() -> String {
    "notify".into()
}

fn default_insights_interval() -> u32 {
    5
}

fn default_meeting_shortcut() -> Option<String> {
    Some("ctrl+alt+m".into())
}

fn default_mark_shortcut() -> Option<String> {
    Some("ctrl+alt+k".into())
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

    /// Corrige o que veio de fora — a tela de Configurações ou um
    /// `config.toml` editado à mão: caixa e espaços, vazios que significam
    /// "padrão", termos repetidos no dicionário e valores fora das listas
    /// aceitas (que voltam ao padrão em vez de derrubar a leitura). É a única
    /// regra de validação, e é idempotente.
    pub fn normalize(&mut self) {
        fn clean(value: &mut Option<String>) {
            *value = value
                .take()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
        }
        fn lowered_or(value: &mut String, fallback: String) {
            let v = value.trim().to_lowercase();
            *value = if v.is_empty() { fallback } else { v };
        }
        fn pick(value: &mut String, allowed: &[&str], fallback: &str) {
            let v = value.trim().to_lowercase();
            *value = if allowed.contains(&v.as_str()) {
                v
            } else {
                fallback.to_string()
            };
        }
        clean(&mut self.shortcut);
        clean(&mut self.meeting_shortcut);
        clean(&mut self.mark_shortcut);
        clean(&mut self.model);
        clean(&mut self.input_device);
        lowered_or(&mut self.lang, default_lang());
        lowered_or(&mut self.meeting_source, default_source());
        let mut dictionary: Vec<String> = Vec::with_capacity(self.dictionary.len());
        for term in self.dictionary.drain(..) {
            let term = term.trim().to_string();
            if !term.is_empty() && !dictionary.contains(&term) {
                dictionary.push(term);
            }
        }
        self.dictionary = dictionary;
        pick(&mut self.polish_style, &isper_llm::POLISH_STYLES, "clean");
        pick(&mut self.after_meeting, &AFTER_MEETING_MODES, "notify");
        pick(
            &mut self.call_detect,
            &crate::calls::CALL_DETECT_MODES,
            "notify",
        );
        if !crate::insights::INSIGHTS_INTERVALS.contains(&self.insights_interval_min) {
            self.insights_interval_min = default_insights_interval();
        }
    }
}

pub fn path() -> Option<PathBuf> {
    crate::paths::roaming_dir().map(|d| d.join("config.toml"))
}

pub fn load() -> AppConfig {
    match path() {
        Some(p) => load_from(&p),
        None => AppConfig::default(),
    }
}

/// Lê `p` (ausente → padrão; inválido → padrão com aviso) e normaliza: um
/// valor fora da lista num arquivo editado à mão é corrigido, não propagado.
pub fn load_from(p: &Path) -> AppConfig {
    let mut cfg = match std::fs::read_to_string(p) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("config.toml inválido ({e}) — usando padrão");
            AppConfig::default()
        }),
        Err(_) => AppConfig::default(),
    };
    cfg.normalize();
    cfg
}

pub fn save(cfg: &AppConfig) -> anyhow::Result<()> {
    let p = path().ok_or_else(|| anyhow::anyhow!("APPDATA não definido"))?;
    save_to(&p, cfg)
}

/// Grava de forma atômica: escreve num `.tmp` ao lado e renomeia por cima.
/// Um desligamento no meio da escrita nunca deixa um `config.toml` truncado
/// (que viraria "config inválido → padrão" no próximo início).
pub fn save_to(p: &Path, cfg: &AppConfig) -> anyhow::Result<()> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = p.with_extension("toml.tmp");
    std::fs::write(&tmp, toml::to_string_pretty(cfg)?)?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("isper-config-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("config.toml")
    }

    #[test]
    fn padrao_faz_ida_e_volta_em_toml() {
        let cfg = AppConfig::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn arquivo_antigo_ou_de_versao_nova_le_com_os_padroes() {
        // Só o que a versão antiga conhecia: o resto vem do padrão.
        let cfg: AppConfig = toml::from_str("lang = \"en\"\n").unwrap();
        assert_eq!(cfg.lang, "en");
        assert_eq!(cfg.call_detect, "notify");
        assert_eq!(cfg.insights_interval_min, 5);
        assert_eq!(cfg.meeting_shortcut.as_deref(), Some("ctrl+alt+m"));
        assert!(cfg.show_home_on_launch && cfg.voice_commands && cfg.auto_update_check);
        // Um campo desconhecido (versão mais nova gravou) não derruba a leitura.
        let cfg: AppConfig = toml::from_str("campo_do_futuro = 1\n").unwrap();
        assert_eq!(cfg, AppConfig::default());
    }

    #[test]
    fn normalize_corrige_caixa_vazios_repetidos_e_valores_fora_da_lista() {
        let mut cfg = AppConfig {
            shortcut: Some("   ".into()),
            lang: " PT-BR ".into(),
            dictionary: vec![" MRP ".into(), "".into(), "MRP".into(), "Kanban".into()],
            model: Some(" ".into()),
            meeting_source: "".into(),
            input_device: Some(" Fone USB ".into()),
            polish_style: "Formal".into(),
            after_meeting: "banana".into(),
            call_detect: "AUTO".into(),
            insights_interval_min: 42,
            ..AppConfig::default()
        };
        cfg.normalize();
        assert_eq!(cfg.shortcut, None);
        assert_eq!(cfg.lang, "pt-br");
        assert_eq!(
            cfg.dictionary,
            vec!["MRP".to_string(), "Kanban".to_string()]
        );
        assert_eq!(cfg.model, None);
        assert_eq!(cfg.meeting_source, "system");
        assert_eq!(cfg.input_device.as_deref(), Some("Fone USB"));
        assert_eq!(cfg.polish_style, "formal");
        assert_eq!(cfg.after_meeting, "notify");
        assert_eq!(cfg.call_detect, "auto");
        assert_eq!(cfg.insights_interval_min, 5);
        // Idempotente: normalizar de novo não muda nada.
        let once = cfg.clone();
        cfg.normalize();
        assert_eq!(cfg, once);
        // O padrão já é normal.
        let mut default = AppConfig::default();
        default.normalize();
        assert_eq!(default, AppConfig::default());
    }

    #[test]
    fn salva_atomicamente_e_recarrega_igual() {
        let p = temp_config("save");
        let mut cfg = AppConfig {
            lang: "en".into(),
            dictionary: vec!["Kanban".into()],
            overlay_pos: Some((120, -40)),
            polish: true,
            ..AppConfig::default()
        };
        save_to(&p, &cfg).unwrap();
        assert!(p.is_file());
        assert!(
            !p.with_extension("toml.tmp").exists(),
            "o .tmp é renomeado por cima, nunca fica"
        );
        assert_eq!(load_from(&p), cfg);
        // Regrava por cima (o rename substitui o arquivo existente).
        cfg.lang = "pt".into();
        save_to(&p, &cfg).unwrap();
        assert_eq!(load_from(&p), cfg);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn arquivo_ausente_invalido_ou_editado_a_mao_vira_padrao_ou_e_corrigido() {
        let p = temp_config("invalid");
        assert_eq!(load_from(&p), AppConfig::default());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "isto nao e toml = = =").unwrap();
        assert_eq!(load_from(&p), AppConfig::default());
        std::fs::write(&p, "call_detect = \"banana\"\ninsights_interval_min = 99\n").unwrap();
        let cfg = load_from(&p);
        assert_eq!(cfg.call_detect, "notify");
        assert_eq!(cfg.insights_interval_min, 5);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
