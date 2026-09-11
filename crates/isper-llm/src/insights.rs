//! Insights ao vivo (Fase 5): durante a reunião, a janela recente do
//! transcript vai ao provider periodicamente com as perguntas que importam
//! enquanto ainda dá tempo de agir — o que ficou pendente, o que "Eu" prometeu,
//! o que já foi decidido, o que ninguém respondeu.
//!
//! Cada rodada recebe também a resposta anterior, para consolidar em vez de
//! recomeçar: pendências resolvidas saem, novas entram.

use crate::Result;
use crate::providers::LlmProvider;

/// Janelas maiores que isso são cortadas pelo INÍCIO (o fim é o que interessa).
const MAX_WINDOW_CHARS: usize = 24_000;
/// Respostas maiores que isso são cortadas — o painel é pequeno.
const MAX_OUTPUT_CHARS: usize = 4_000;

const SYSTEM: &str = "Você acompanha uma reunião de trabalho EM ANDAMENTO a partir da transcrição \
automática de fala (pode conter erros; interprete com bom senso). Responda SEMPRE em português \
do Brasil, em Markdown compacto, sem introdução, sem comentários e sem repetir o transcript. \
Seja fiel ao que foi dito — NÃO invente nada que não esteja na transcrição.";

/// O que entra numa rodada de insights.
pub struct InsightsInput<'a> {
    /// Trecho recente do transcript, uma fala por linha (`[mm:ss] Falante: texto`).
    pub window: &'a str,
    /// Resposta da rodada anterior, para consolidar.
    pub previous: Option<&'a str>,
    /// Tempo decorrido de reunião (`mm:ss`).
    pub elapsed: &'a str,
    /// Quantos minutos a janela cobre (só informa o modelo).
    pub window_minutes: u32,
}

/// Monta o pedido — separado para ser testável sem rede.
pub(crate) fn build_prompt(input: &InsightsInput<'_>) -> String {
    let window = tail(input.window, MAX_WINDOW_CHARS);
    let previous = match input.previous.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => format!(
            "\n\nSua resposta anterior (ATUALIZE-A: mantenha o que segue pendente, remova o que \
já foi resolvido, acrescente o que surgiu):\n---\n{p}\n---"
        ),
        None => String::new(),
    };
    format!(
        "A reunião está em {elapsed}. \"Eu\" é a pessoa que grava; \"Participantes\" (ou \
\"Participante N\") são as demais vozes. Trecho dos últimos ~{minutes} minutos:\n---\n{window}\n---\
{previous}\n\nGere exatamente estas seções, nesta ordem, com até 4 bullets curtos cada uma \
(escreva \"Nenhum.\" quando não houver):\n\n\
## Pendências\nO que ficou em aberto, foi pedido e ainda não teve resposta ou encaminhamento.\n\n\
## Compromissos de \"Eu\"\nO que a pessoa que grava prometeu fazer ou recebeu como tarefa.\n\n\
## Decisões\nO que já foi decidido até agora.\n\n\
## Perguntas em aberto\nDúvidas levantadas que ninguém respondeu.",
        elapsed = input.elapsed,
        minutes = input.window_minutes.max(1),
    )
}

/// Chama o provider e devolve o Markdown das quatro seções.
pub fn live_insights(provider: &dyn LlmProvider, input: &InsightsInput<'_>) -> Result<String> {
    let raw = provider.complete(SYSTEM, &build_prompt(input))?;
    let text = sanitize(&raw);
    if text.is_empty() {
        return Err(crate::LlmError::BadResponse("resposta vazia".into()));
    }
    Ok(text)
}

/// Tira cercas de código que alguns modelos põem em volta e limita o tamanho.
pub(crate) fn sanitize(raw: &str) -> String {
    let mut text = raw.trim();
    if text.starts_with("```") {
        text = text.trim_start_matches('`');
        if let Some(rest) = text.strip_prefix("markdown") {
            text = rest;
        }
        text = text.trim_end_matches('`').trim();
    }
    if text.chars().count() > MAX_OUTPUT_CHARS {
        let cut: String = text.chars().take(MAX_OUTPUT_CHARS - 1).collect();
        return format!("{cut}…");
    }
    text.to_string()
}

/// Últimos `max_chars` caracteres, começando numa linha inteira.
fn tail(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let skipped: String = text.chars().skip(count - max_chars).collect();
    match skipped.find('\n') {
        Some(i) => format!("[… trecho anterior omitido …]\n{}", &skipped[i + 1..]),
        None => skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeProvider;

    #[test]
    fn prompt_carrega_janela_tempo_e_resposta_anterior() {
        let input = InsightsInput {
            window: "[00:10] Eu: Mando o relatório até sexta.\n[00:20] Participante 1: Combinado.",
            previous: Some("## Pendências\n- Definir a data do relatório."),
            elapsed: "12:34",
            window_minutes: 15,
        };
        let p = build_prompt(&input);
        assert!(p.contains("está em 12:34"));
        assert!(p.contains("últimos ~15 minutos"));
        assert!(p.contains("Mando o relatório até sexta."));
        assert!(p.contains("ATUALIZE-A"));
        assert!(p.contains("Definir a data do relatório."));
        assert!(p.contains("## Compromissos de \"Eu\""));
        // Sem resposta anterior, não pede atualização.
        let first = build_prompt(&InsightsInput {
            previous: None,
            ..input
        });
        assert!(!first.contains("ATUALIZE-A"));
    }

    #[test]
    fn chama_o_provider_e_limpa_a_resposta() {
        let fake = FakeProvider::replying("```markdown\n## Pendências\n- Nenhum.\n```");
        let out = live_insights(
            &fake,
            &InsightsInput {
                window: "[00:01] Eu: oi",
                previous: None,
                elapsed: "00:05",
                window_minutes: 15,
            },
        )
        .unwrap();
        assert_eq!(out, "## Pendências\n- Nenhum.");
        let (system, user) = fake.single_call();
        assert!(system.contains("EM ANDAMENTO"));
        assert!(user.contains("[00:01] Eu: oi"));
    }

    #[test]
    fn resposta_vazia_e_erro_e_a_longa_e_cortada() {
        let fake = FakeProvider::replying("   ");
        assert!(
            live_insights(
                &fake,
                &InsightsInput {
                    window: "x",
                    previous: None,
                    elapsed: "00:00",
                    window_minutes: 15,
                },
            )
            .is_err()
        );
        let long = sanitize(&"a".repeat(MAX_OUTPUT_CHARS + 50));
        assert_eq!(long.chars().count(), MAX_OUTPUT_CHARS);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn janela_grande_perde_o_inicio_em_linha_inteira() {
        let lines: Vec<String> = (0..2000)
            .map(|i| format!("[{i:05}] Eu: fala {i}"))
            .collect();
        let text = lines.join("\n");
        let t = tail(&text, 500);
        assert!(t.starts_with("[… trecho anterior omitido …]\n["));
        assert!(t.ends_with("fala 1999"));
        assert_eq!(tail("curto", 10), "curto");
    }
}
