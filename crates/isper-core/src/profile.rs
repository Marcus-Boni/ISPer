//! Perfis de decodificação do Whisper — todos os parâmetros num lugar só.
//!
//! Antes desta fase, `engine.rs` fixava `Greedy { best_of: 1 }` e deixava o
//! resto nos defaults do whisper.cpp, espalhados entre o código C++ e
//! comentários. Um perfil é o conjunto COMPLETO de parâmetros de decodificação,
//! nomeado pelo uso:
//!
//! - [`TranscriptionProfile::Dictation`] — ditado: frases curtas, latência
//!   manda (o texto é colado enquanto a pessoa espera);
//! - [`TranscriptionProfile::Live`] — legenda durante a reunião: o mesmo
//!   compromisso do ditado, aplicado a blocos;
//! - [`TranscriptionProfile::MeetingFinal`] — a transcrição OFICIAL, gerada
//!   depois que a reunião acaba: qualidade acima de tudo (beam search,
//!   timestamps por token, contexto entre blocos).
//!
//! Os números têm origem declarada: ou são o default do whisper.cpp
//! (`whisper_full_default_params`, conferido na versão que o projeto compila)
//! ou uma escolha nossa com o motivo escrito ao lado.

use serde::{Deserialize, Serialize};
use whisper_rs::{FullParams, SamplingStrategy};

/// Para que serve esta transcrição — escolhe o [`DecodeConfig`] padrão.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TranscriptionProfile {
    /// Ditado (Fase 1): uma frase, colada na hora.
    Dictation,
    /// Transcrição ao vivo da reunião, bloco a bloco.
    Live,
    /// Passe final da reunião, depois que ela termina.
    MeetingFinal,
}

impl TranscriptionProfile {
    /// Nome estável do perfil (o que vai para relatórios e para a CLI).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::Live => "live",
            Self::MeetingFinal => "meeting-final",
        }
    }

    /// Lê um nome de perfil, em inglês ou português, sem diferenciar caixa.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "dictation" | "ditado" => Some(Self::Dictation),
            "live" | "ao-vivo" => Some(Self::Live),
            "meeting-final" | "final" => Some(Self::MeetingFinal),
            _ => None,
        }
    }

    /// Decodificação padrão do perfil.
    pub fn decode(self) -> DecodeConfig {
        match self {
            Self::Dictation => DecodeConfig::dictation(),
            Self::Live => DecodeConfig::live(),
            Self::MeetingFinal => DecodeConfig::meeting_final(),
        }
    }
}

/// Todos os parâmetros que vão para o `whisper_full_params`.
///
/// `Serialize` de propósito: o relatório do benchmark grava exatamente os
/// parâmetros que produziram aquela transcrição — comparar duas rodadas é
/// comparar dois JSONs, não relembrar o que estava no código naquele dia.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecodeConfig {
    /// Largura do beam search. 0 ou 1 = Greedy (mais rápido).
    pub beam_size: u8,
    /// Candidatos amostrados quando a temperatura sobe (fallback). O default
    /// do whisper.cpp é 5; usar 1 desliga na prática o fallback por amostragem.
    pub best_of: u8,
    /// Temperatura inicial. 0 = determinístico (default do whisper.cpp).
    pub temperature: f32,
    /// Passo do fallback de temperatura. 0 desliga o fallback.
    /// Default do whisper.cpp: 0,2.
    pub temperature_inc: f32,
    /// Acima desta entropia o decoder tenta de novo mais quente (default 2,4).
    pub entropy_thold: f32,
    /// Abaixo deste logprob médio o decoder tenta de novo (default -1,0).
    pub logprob_thold: f32,
    /// Acima disto o trecho é considerado silêncio pelo whisper.cpp (default 0,6).
    pub no_speech_thold: f32,
    /// Limite do primeiro timestamp, em unidades de 20 ms (default 1,0).
    pub max_initial_ts: f32,
    /// Penalidade de comprimento no beam search (default -1,0 = desligada).
    pub length_penalty: f32,
    /// Timestamps por token — insumo do alinhamento palavra→falante.
    pub token_timestamps: bool,
    /// Corta segmentos com mais de N caracteres (0 = sem corte).
    pub max_len: u16,
    /// Com `max_len`, corta em fronteira de palavra.
    pub split_on_word: bool,
    /// Threads da inferência (0 = decide pelo hardware, no máximo 8).
    pub n_threads: u8,
    /// Reaproveita o texto do bloco anterior como prompt — só faz sentido em
    /// blocos contíguos (ver [`crate::pipeline`]).
    pub carry_context: bool,
    /// Suprime tokens não-fala (`♪`, `[Music]`…). Default do whisper.cpp: false;
    /// aqui ligamos por padrão — reunião não tem trilha sonora.
    pub suppress_nst: bool,
}

impl DecodeConfig {
    /// Base = defaults do whisper.cpp, com o que o ISPer sempre quis ligado.
    fn base() -> Self {
        Self {
            beam_size: 0,
            best_of: 5,
            temperature: 0.0,
            temperature_inc: 0.2,
            entropy_thold: 2.4,
            logprob_thold: -1.0,
            no_speech_thold: 0.6,
            max_initial_ts: 1.0,
            length_penalty: -1.0,
            token_timestamps: false,
            max_len: 0,
            split_on_word: false,
            n_threads: 0,
            carry_context: false,
            suppress_nst: true,
        }
    }

    /// Ditado: uma frase curta colada na hora — greedy puro.
    pub fn dictation() -> Self {
        Self::base()
    }

    /// Ao vivo: igual ao ditado. O passe final conserta o que escapar.
    pub fn live() -> Self {
        Self::base()
    }

    /// Legenda provisória: o mais rápido que o decoder consegue — greedy sem
    /// candidatos extras e sem fallback de temperatura. O texto é substituído
    /// pelo bloco final em segundos; não vale gastar GPU para acertar vírgula.
    pub fn partial() -> Self {
        Self {
            best_of: 1,
            temperature_inc: 0.0,
            ..Self::base()
        }
    }

    /// Passe final da reunião. As diferenças em relação ao ao vivo:
    ///
    /// - **beam search 5**: o default do whisper.cpp quando se escolhe
    ///   `WHISPER_SAMPLING_BEAM_SEARCH`, e o que o `whisper-cli` usa;
    /// - **timestamps por token**: sem eles não há atribuição de falante por
    ///   palavra (duas pessoas dentro do mesmo segmento viram uma só);
    /// - **contexto entre blocos**: o texto anterior entra como prompt.
    pub fn meeting_final() -> Self {
        Self {
            beam_size: 5,
            token_timestamps: true,
            carry_context: true,
            ..Self::base()
        }
    }

    /// Quantas threads usar de fato.
    pub fn threads(&self) -> i32 {
        if self.n_threads > 0 {
            return self.n_threads as i32;
        }
        std::thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(4)
            .clamp(1, 8)
    }

    /// Aplica tudo num `FullParams` recém-criado com a estratégia certa.
    ///
    /// `lang` é "pt", "en"… ou "auto"; `prompt` é o `initial_prompt` (glossário);
    /// `prompt_tokens` é o contexto do bloco anterior (vazio = sem contexto).
    pub fn to_full_params<'a, 'b>(
        &self,
        lang: &'a str,
        prompt: Option<&'a str>,
        prompt_tokens: &'b [std::os::raw::c_int],
    ) -> FullParams<'a, 'b> {
        let strategy = if self.beam_size >= 2 {
            SamplingStrategy::BeamSearch {
                beam_size: self.beam_size as i32,
                patience: -1.0,
            }
        } else {
            SamplingStrategy::Greedy {
                best_of: self.best_of.max(1) as i32,
            }
        };
        let mut p = FullParams::new(strategy);
        p.set_language(Some(lang));
        if let Some(prompt) = prompt.map(str::trim).filter(|s| !s.is_empty()) {
            p.set_initial_prompt(prompt);
        }
        if !prompt_tokens.is_empty() {
            p.set_tokens(prompt_tokens);
        }
        p.set_n_threads(self.threads());
        p.set_temperature(self.temperature);
        p.set_temperature_inc(self.temperature_inc);
        p.set_entropy_thold(self.entropy_thold);
        p.set_logprob_thold(self.logprob_thold);
        p.set_no_speech_thold(self.no_speech_thold);
        p.set_max_initial_ts(self.max_initial_ts);
        p.set_length_penalty(self.length_penalty);
        p.set_token_timestamps(self.token_timestamps);
        if self.max_len > 0 {
            p.set_max_len(self.max_len as i32);
            p.set_split_on_word(self.split_on_word);
        }
        p.set_suppress_blank(true);
        p.set_suppress_nst(self.suppress_nst);
        p.set_print_special(false);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_timestamps(false);
        p
    }
}

/// Idioma no formato que o whisper.cpp entende.
///
/// O ISPer aceita "pt-br" na configuração (é o que a pessoa escreve), mas o
/// whisper.cpp só conhece códigos de duas letras: passar "pt-br" faz
/// `whisper_lang_id` devolver -1 e a inferência inteira falhar.
pub fn normalize_lang(lang: &str) -> String {
    let l = lang.trim().to_ascii_lowercase();
    if l.is_empty() || l == "auto" {
        return "auto".into();
    }
    match l.split(['-', '_']).next() {
        Some(base) if !base.is_empty() => base.to_string(),
        _ => "auto".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfil_final_usa_beam_search_e_timestamps_por_token() {
        let d = TranscriptionProfile::MeetingFinal.decode();
        assert!(d.beam_size >= 2, "final precisa de beam search");
        assert!(d.token_timestamps, "final precisa de timestamps por token");
        assert!(d.carry_context);
    }

    #[test]
    fn perfil_ao_vivo_e_ditado_ficam_no_greedy() {
        for p in [TranscriptionProfile::Live, TranscriptionProfile::Dictation] {
            let d = p.decode();
            assert!(d.beam_size < 2, "{p:?} deveria ser greedy");
            assert!(!d.carry_context, "{p:?} não carrega contexto");
        }
    }

    #[test]
    fn best_of_padrao_e_o_do_whisper_cpp() {
        // Antes desta fase o ISPer passava best_of = 1, que desliga na prática
        // o fallback de temperatura: com 1 candidato não há o que escolher.
        assert_eq!(DecodeConfig::base().best_of, 5);
        assert!(DecodeConfig::base().temperature_inc > 0.0);
    }

    #[test]
    fn perfil_provisorio_e_o_mais_barato() {
        let p = DecodeConfig::partial();
        assert_eq!(p.beam_size, 0, "greedy");
        assert_eq!(p.best_of, 1, "sem candidatos extras");
        assert_eq!(p.temperature_inc, 0.0, "sem fallback");
        assert!(!p.token_timestamps);
    }

    #[test]
    fn threads_respeita_o_limite() {
        let mut c = DecodeConfig::base();
        c.n_threads = 3;
        assert_eq!(c.threads(), 3);
        c.n_threads = 0;
        assert!((1..=8).contains(&c.threads()));
    }

    #[test]
    fn idioma_regional_vira_o_codigo_que_o_whisper_entende() {
        assert_eq!(normalize_lang("pt-BR"), "pt");
        assert_eq!(normalize_lang("pt_br"), "pt");
        assert_eq!(normalize_lang("en-US"), "en");
        assert_eq!(normalize_lang("pt"), "pt");
        assert_eq!(normalize_lang("auto"), "auto");
        assert_eq!(normalize_lang("  "), "auto");
    }

    #[test]
    fn perfis_vao_e_voltam_do_texto() {
        for p in [
            TranscriptionProfile::Dictation,
            TranscriptionProfile::Live,
            TranscriptionProfile::MeetingFinal,
        ] {
            assert_eq!(TranscriptionProfile::parse(p.as_str()), Some(p));
        }
        assert_eq!(TranscriptionProfile::parse("nao-existe"), None);
    }
}
