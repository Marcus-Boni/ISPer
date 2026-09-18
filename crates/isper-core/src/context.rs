//! Contexto da reunião: quem está na sala e que palavras vão aparecer.
//!
//! O `initial_prompt` do Whisper é um PREFIXO DE TEXTO, não uma lista de
//! configuração: o modelo continua escrevendo no estilo do que acabou de ler.
//! Por isso o prompt daqui sai como frase ("Reunião da Optsolv com a Tommasi
//! sobre o ERP. Participantes: …"), não como um despejo de termos.
//!
//! Há um limite duro: o whisper.cpp só aproveita os últimos
//! `n_text_ctx / 2 - 1` tokens do prompt (≈ 223 no large-v3). Um prompt
//! gigante não "cabe mais vocabulário" — ele empurra para fora justamente o
//! começo, que é onde está o glossário. Daí o corte em [`MAX_PROMPT_CHARS`].

use serde::{Deserialize, Serialize};

/// Teto do prompt montado aqui, em caracteres.
///
/// O whisper.cpp aproveita ~223 tokens de prompt. Em português, um token do
/// tokenizador do Whisper vale grosso modo 3 caracteres, então ~670 chars já
/// encostam no teto. Ficamos em 420 para sobrar espaço para o contexto do
/// bloco anterior, que o passe final acrescenta depois.
pub const MAX_PROMPT_CHARS: usize = 420;

/// O que se sabe da reunião antes de transcrevê-la.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MeetingContext {
    /// Nomes de quem participa — o erro mais caro de uma ata é o nome errado.
    #[serde(default)]
    pub participants: Vec<String>,
    /// Cliente/empresa da reunião.
    #[serde(default)]
    pub client: Option<String>,
    /// Projeto ou produto em pauta.
    #[serde(default)]
    pub project: Option<String>,
    /// Termos técnicos e nomes de produto que o modelo costuma errar.
    #[serde(default)]
    pub vocabulary: Vec<String>,
    /// Siglas (ERP, MRP, PCP…) — separadas porque entram no prompt com outra
    /// moldura, o que ajuda o modelo a não "traduzi-las" em palavras.
    #[serde(default)]
    pub acronyms: Vec<String>,
}

impl MeetingContext {
    /// Contexto a partir do dicionário pessoal que já existia na configuração.
    /// Um termo todo em maiúsculas com 2 a 6 letras é tratado como sigla.
    pub fn from_dictionary(terms: &[String]) -> Self {
        let mut ctx = Self::default();
        for t in terms.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
            if is_acronym(t) {
                ctx.acronyms.push(t.to_string());
            } else {
                ctx.vocabulary.push(t.to_string());
            }
        }
        ctx
    }

    pub fn is_empty(&self) -> bool {
        self.participants.is_empty()
            && self.client.is_none()
            && self.project.is_none()
            && self.vocabulary.is_empty()
            && self.acronyms.is_empty()
    }

    /// Todos os termos que valem para a correção por semelhança
    /// ([`crate::text::apply_dictionary`]) — inclusive os nomes.
    pub fn terms(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in self
            .participants
            .iter()
            .chain(self.client.iter())
            .chain(self.project.iter())
            .chain(&self.vocabulary)
            .chain(&self.acronyms)
        {
            let t = t.trim();
            if !t.is_empty() && !out.iter().any(|o| o == t) {
                out.push(t.to_string());
            }
        }
        out
    }

    /// O prefixo que vai ao Whisper. `None` quando não há nada a dizer.
    ///
    /// A ordem é deliberada: primeiro a moldura da reunião (que orienta o
    /// registro e a pontuação), depois os nomes, por último os termos — e
    /// tudo cortado em [`MAX_PROMPT_CHARS`], descartando termo inteiro em vez
    /// de cortar palavra no meio.
    pub fn initial_prompt(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut out = String::new();
        match (self.client.as_deref(), self.project.as_deref()) {
            (Some(c), Some(p)) => {
                out.push_str(&format!("Reunião com {} sobre {}.", c.trim(), p.trim()))
            }
            (Some(c), None) => out.push_str(&format!("Reunião com {}.", c.trim())),
            (None, Some(p)) => out.push_str(&format!("Reunião sobre {}.", p.trim())),
            (None, None) => out.push_str("Reunião de trabalho."),
        }
        append_list(&mut out, "Participantes", &self.participants);
        append_list(&mut out, "Termos", &self.vocabulary);
        append_list(&mut out, "Siglas", &self.acronyms);
        Some(truncate_at_boundary(&out, MAX_PROMPT_CHARS))
    }
}

/// Um termo todo em maiúsculas e curto é sigla (ERP, MRP, PCP, LGPD).
fn is_acronym(t: &str) -> bool {
    let letters: Vec<char> = t.chars().filter(|c| c.is_alphabetic()).collect();
    !letters.is_empty()
        && letters.len() <= 6
        && t.chars().all(|c| !c.is_lowercase())
        && t.split_whitespace().count() == 1
}

fn append_list(out: &mut String, label: &str, items: &[String]) {
    let items: Vec<&str> = items
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if items.is_empty() {
        return;
    }
    out.push(' ');
    out.push_str(label);
    out.push_str(": ");
    out.push_str(&items.join(", "));
    out.push('.');
}

/// Corta em `max` caracteres, recuando até a última vírgula/espaço para não
/// deixar meia palavra (que o modelo leria como um termo inventado).
fn truncate_at_boundary(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cortado: String = s.chars().take(max).collect();
    let corte = cortado
        .rfind(", ")
        .or_else(|| cortado.rfind(". "))
        .or_else(|| cortado.rfind(' '))
        .unwrap_or(cortado.len());
    let mut out = cortado[..corte]
        .trim_end()
        .trim_end_matches(',')
        .to_string();
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

/// Monta o prompt de uma janela: o contexto da reunião mais o fim do texto da
/// janela anterior.
///
/// A defesa contra "um erro no começo contamina a reunião inteira" está aqui:
/// só entra texto que o chamador já julgou confiável (ver
/// [`crate::pipeline`]), e no máximo `carry_chars` caracteres — uma frase ou
/// duas, não a reunião toda.
pub fn window_prompt(
    base: Option<&str>,
    previous_text: &str,
    carry_chars: usize,
) -> Option<String> {
    let tail = tail_sentences(previous_text, carry_chars);
    match (
        base.map(str::trim).filter(|s| !s.is_empty()),
        tail.as_deref(),
    ) {
        (Some(b), Some(t)) => Some(format!("{b} {t}")),
        (Some(b), None) => Some(b.to_string()),
        (None, Some(t)) => Some(t.to_string()),
        (None, None) => None,
    }
}

/// As últimas `max` letras de um texto, começando numa fronteira de frase
/// quando houver uma por perto (senão, numa fronteira de palavra).
fn tail_sentences(text: &str, max: usize) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || max == 0 {
        return None;
    }
    let chars: Vec<char> = t.chars().collect();
    if chars.len() <= max {
        return Some(t.to_string());
    }
    let inicio = chars.len() - max;
    let janela: String = chars[inicio..].iter().collect();
    // Procura o começo de frase mais próximo dentro da janela.
    let corte = janela
        .find(". ")
        .map(|i| i + 2)
        .or_else(|| janela.find(' ').map(|i| i + 1))
        .unwrap_or(0);
    let out = janela[corte..].trim();
    (!out.is_empty()).then(|| out.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> MeetingContext {
        MeetingContext {
            participants: vec!["Paulo Rocha".into(), "Marcus".into()],
            client: Some("Tommasi".into()),
            project: Some("o ERP".into()),
            vocabulary: vec!["Optsolv".into(), "SharePoint".into(), "Javé".into()],
            acronyms: vec!["MRP".into(), "PCP".into()],
        }
    }

    #[test]
    fn prompt_sai_como_frase_e_nao_como_lista_solta() {
        let p = ctx().initial_prompt().expect("prompt");
        assert!(p.starts_with("Reunião com Tommasi sobre o ERP."), "{p}");
        assert!(p.contains("Participantes: Paulo Rocha, Marcus."), "{p}");
        assert!(p.contains("Termos: Optsolv, SharePoint, Javé."), "{p}");
        assert!(p.contains("Siglas: MRP, PCP."), "{p}");
    }

    #[test]
    fn prompt_respeita_o_teto_do_whisper_sem_partir_palavra() {
        let muitos: Vec<String> = (0..300).map(|i| format!("Termo{i:03}")).collect();
        let c = MeetingContext {
            vocabulary: muitos,
            ..Default::default()
        };
        let p = c.initial_prompt().expect("prompt");
        assert!(
            p.chars().count() <= MAX_PROMPT_CHARS,
            "{}",
            p.chars().count()
        );
        assert!(p.ends_with('.'), "{p}");
        // O último termo tem que estar inteiro.
        let ultimo = p
            .trim_end_matches('.')
            .rsplit(", ")
            .next()
            .expect("último termo");
        assert!(
            ultimo.len() == "Termo000".len(),
            "termo cortado no meio: {ultimo:?}"
        );
    }

    #[test]
    fn contexto_vazio_nao_gera_prompt() {
        assert!(MeetingContext::default().initial_prompt().is_none());
        assert!(MeetingContext::default().is_empty());
    }

    #[test]
    fn dicionario_antigo_vira_contexto_com_siglas_separadas() {
        let dic = vec![
            "MRP".into(),
            "Optsolv".into(),
            "PCP".into(),
            "fluxo de caixa".into(),
        ];
        let c = MeetingContext::from_dictionary(&dic);
        assert_eq!(c.acronyms, vec!["MRP", "PCP"]);
        assert_eq!(c.vocabulary, vec!["Optsolv", "fluxo de caixa"]);
    }

    #[test]
    fn termos_reunem_tudo_sem_repetir() {
        let mut c = ctx();
        c.vocabulary.push("Tommasi".into()); // já está como cliente
        let t = c.terms();
        assert_eq!(t.iter().filter(|x| *x == "Tommasi").count(), 1);
        assert!(t.contains(&"Paulo Rocha".to_string()));
        assert!(t.contains(&"MRP".to_string()));
    }

    #[test]
    fn contexto_da_janela_junta_base_e_fim_do_texto_anterior() {
        let p = window_prompt(Some("Reunião com Tommasi."), "…e o prazo fecha sexta.", 200)
            .expect("prompt");
        assert_eq!(p, "Reunião com Tommasi. …e o prazo fecha sexta.");
    }

    #[test]
    fn contexto_carregado_e_curto_e_comeca_em_fronteira() {
        let longo =
            "primeira frase bem comprida sobre o assunto. segunda frase. terceira frase que fecha.";
        let p = window_prompt(None, longo, 30).expect("prompt");
        assert!(p.chars().count() <= 30, "{p}");
        assert!(!p.starts_with(' '), "{p}");
        // Não começa no meio de uma palavra.
        assert!(longo.contains(&p), "{p}");
        assert!(
            longo.split_whitespace().any(|w| p.starts_with(w)),
            "começou no meio de palavra: {p}"
        );
    }

    #[test]
    fn sem_base_e_sem_texto_nao_ha_prompt() {
        assert!(window_prompt(None, "   ", 100).is_none());
        assert!(window_prompt(Some("  "), "", 100).is_none());
    }
}
