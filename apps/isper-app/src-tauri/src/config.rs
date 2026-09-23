//! Configurações do app — persistidas em `%APPDATA%\ISPer\config.toml`.
//! (As configurações de IA vivem em `llm.toml`, cuidadas pelo isper-llm;
//! a chave de API vive no Credential Manager, nunca em arquivo.)

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Modos de `after_meeting`.
pub(crate) const AFTER_MEETING_MODES: [&str; 3] = ["notify", "open", "silent"];

/// Versão do formato do `config.toml`. Um arquivo antigo chega com 0 (o campo
/// não existia) e passa por [`AppConfig::migrate`]; um arquivo de versão maior
/// (gravado por um ISPer mais novo) é lido com os campos que este entende e
/// regravado nesta versão, com aviso no log.
pub const CONFIG_VERSION: u32 = 1;

/// Opções de retenção, em dias; 0 = guardar para sempre.
pub(crate) const RETENTION_DAYS: [u32; 5] = [0, 30, 90, 180, 365];

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
    /// Atalho global que abre/fecha o Copilot (`None` = o primeiro livre).
    #[serde(default = "default_copilot_shortcut")]
    pub copilot_shortcut: Option<String>,
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
    /// Apagar reuniões e ditados com mais de N dias (LGPD: guardar só o
    /// necessário). 0 = para sempre. A varredura roda ao abrir e uma vez por dia.
    #[serde(default)]
    pub retention_days: u32,
    /// Passe final: ao encerrar a reunião, refazer a transcrição sobre o
    /// áudio inteiro (VAD, beam search, falante por palavra) e substituir a
    /// transcrição ao vivo. Custa alguns minutos por hora de reunião e é o
    /// que dá a transcrição boa o bastante para virar ata.
    #[serde(default = "default_true")]
    pub final_pass: bool,
    /// Quantos participantes a reunião tem, quando se sabe.
    ///
    /// Zero = descobrir pelo agrupamento. Informado, o agrupamento corta o
    /// dendrograma em exatamente N grupos em vez de usar limiar de distância
    /// — e é a única coisa que mantém a contagem de falantes estável numa
    /// reunião de duas horas (ver `isper-diarize`).
    #[serde(default)]
    pub meeting_speakers: u32,
    /// Limiar do agrupamento de falantes. 0 = o padrão do projeto (0,5).
    /// Configuração avançada: mexer aqui sem medir costuma piorar.
    #[serde(default)]
    pub diarize_threshold: f32,
    /// Tema da interface: `system` (segue o Windows) · `light` · `dark`.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Versão do formato deste arquivo — ver [`CONFIG_VERSION`].
    #[serde(default)]
    pub config_version: u32,
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
            copilot_shortcut: default_copilot_shortcut(),
            overlay_captions: false,
            auto_update_check: true,
            call_detect: default_call_detect(),
            live_insights: false,
            insights_interval_min: default_insights_interval(),
            overlay_pinned: false,
            retention_days: 0,
            final_pass: true,
            meeting_speakers: 0,
            diarize_threshold: 0.0,
            theme: default_theme(),
            config_version: CONFIG_VERSION,
        }
    }
}

fn default_theme() -> String {
    "system".into()
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

fn default_copilot_shortcut() -> Option<String> {
    Some("ctrl+alt+c".into())
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

    /// Leva um arquivo de versão antiga até [`CONFIG_VERSION`], um passo por
    /// versão, e devolve o que cada passo fez (vai para o log). Diferente de
    /// `normalize`, que corrige valores, aqui entram mudanças de formato:
    /// campo renomeado, unidade trocada, valor que mudou de significado.
    /// Um arquivo de versão maior mantém os campos que este ISPer entende e
    /// desce para a versão atual (o que ele não conhece já foi ignorado na
    /// leitura).
    pub fn migrate(&mut self) -> Vec<&'static str> {
        let mut notes = Vec::new();
        if self.config_version > CONFIG_VERSION {
            notes.push("arquivo de uma versão mais nova do ISPer: campos desconhecidos ignorados");
            self.config_version = CONFIG_VERSION;
        }
        while self.config_version < CONFIG_VERSION {
            match self.config_version {
                // 0 → 1: o campo de versão passou a existir. As versões até a
                // 0.14 gravavam exatamente estes campos, com estes nomes e
                // unidades — nada a converter; `normalize` cuida dos valores.
                0 => notes.push("config.toml sem versão → 1"),
                _ => break,
            }
            self.config_version += 1;
        }
        notes
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
        clean(&mut self.copilot_shortcut);
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
        if !RETENTION_DAYS.contains(&self.retention_days) {
            self.retention_days = 0;
        }
        pick(&mut self.theme, &crate::ui::THEMES, "system");
        self.config_version = CONFIG_VERSION;
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

/// Lê `p` (ausente → padrão; inválido → padrão com aviso), migra o formato
/// e normaliza: um valor fora da lista num arquivo editado à mão é corrigido,
/// não propagado.
pub fn load_from(p: &Path) -> AppConfig {
    let mut cfg = match std::fs::read_to_string(p) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("config.toml inválido ({e}) — usando padrão");
            AppConfig::default()
        }),
        Err(_) => AppConfig::default(),
    };
    for note in cfg.migrate() {
        tracing::info!("config: {note}");
    }
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
        assert_eq!(cfg.config_version, 0, "arquivo sem o campo chega como 0");
        // Um campo desconhecido (versão mais nova gravou) não derruba a leitura.
        let mut cfg: AppConfig = toml::from_str("campo_do_futuro = 1\n").unwrap();
        cfg.migrate();
        assert_eq!(cfg, AppConfig::default());
    }

    #[test]
    fn config_sem_versao_e_migrada_e_regravada_na_versao_atual() {
        let p = temp_config("migra");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        // Um config.toml da 0.14: sem config_version nem retention_days.
        std::fs::write(&p, "lang = \"en\"\npolish = true\n").unwrap();
        let cfg = load_from(&p);
        assert_eq!(cfg.config_version, CONFIG_VERSION);
        assert_eq!(cfg.lang, "en");
        assert!(cfg.polish);
        assert_eq!(cfg.retention_days, 0, "retenção nasce desligada");
        save_to(&p, &cfg).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(
            text.contains(&format!("config_version = {CONFIG_VERSION}")),
            "{text}"
        );
        // Migrar de novo não faz nada.
        let mut again = cfg.clone();
        assert!(again.migrate().is_empty());
        assert_eq!(again, cfg);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn config_de_versao_mais_nova_mantem_o_que_entende_e_desce_de_versao() {
        let mut cfg: AppConfig =
            toml::from_str("config_version = 99\nlang = \"en\"\ncampo_novo = \"x\"\n").unwrap();
        let notes = cfg.migrate();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("mais nova"));
        assert_eq!(cfg.config_version, CONFIG_VERSION);
        assert_eq!(cfg.lang, "en");
    }

    #[test]
    fn tema_normaliza_caixa_e_volta_ao_padrao_fora_da_lista() {
        let mut cfg = AppConfig {
            theme: " Light ".into(),
            ..AppConfig::default()
        };
        cfg.normalize();
        assert_eq!(cfg.theme, "light");
        cfg.theme = "sepia".into();
        cfg.normalize();
        assert_eq!(cfg.theme, "system");
        // Arquivo antigo, sem o campo: segue o Windows.
        let old: AppConfig = toml::from_str("lang = \"pt\"\n").unwrap();
        assert_eq!(old.theme, "system");
    }

    #[test]
    fn retencao_fora_da_lista_volta_a_desligada() {
        let mut cfg = AppConfig {
            retention_days: 45,
            ..AppConfig::default()
        };
        cfg.normalize();
        assert_eq!(cfg.retention_days, 0);
        for days in RETENTION_DAYS {
            let mut cfg = AppConfig {
                retention_days: days,
                ..AppConfig::default()
            };
            cfg.normalize();
            assert_eq!(cfg.retention_days, days);
        }
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
