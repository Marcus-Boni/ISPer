//! Resumo de reuniões: transforma o transcript em título, resumo, pontos
//! principais, action items e decisões — sempre em pt-BR, numa única chamada.

use crate::Result;
use crate::providers::LlmProvider;

/// Transcripts maiores que isso são encurtados pelo MEIO (início e fim
/// carregam abertura e encaminhamentos — o miolo é o mais comprimível).
const MAX_TRANSCRIPT_CHARS: usize = 60_000;
/// Títulos maiores que isso viram reticências (a lista da Biblioteca é estreita).
const MAX_TITLE_CHARS: usize = 80;

const SYSTEM: &str = "Você é um assistente especialista em resumir reuniões de trabalho. \
Responda SEMPRE em português do Brasil, em Markdown. Seja fiel ao que foi dito — \
NÃO invente informações que não estão no transcript. O transcript foi gerado por \
transcrição automática de fala e pode conter pequenos erros; interprete com bom senso.";

/// Resultado da chamada: o título vem na primeira linha da resposta e é
/// separado do corpo; se o modelo não obedecer, `title` fica `None`.
#[derive(Debug, Clone, PartialEq)]
pub struct MeetingSummary {
    pub title: Option<String>,
    pub body: String,
}

/// Só o corpo do resumo (compatibilidade com a CLI).
pub fn summarize_meeting(provider: &dyn LlmProvider, transcript: &str) -> Result<String> {
    Ok(summarize_meeting_titled(provider, transcript)?.body)
}

/// Título curto + resumo estruturado, numa chamada só.
pub fn summarize_meeting_titled(
    provider: &dyn LlmProvider,
    transcript: &str,
) -> Result<MeetingSummary> {
    let excerpt = shorten_middle(transcript, MAX_TRANSCRIPT_CHARS);
    let user = format!(
        "Abaixo está a transcrição de uma reunião. \"Eu\" é a pessoa que gravou; \
\"Participantes\" (ou \"Participante N\") são as demais vozes.\n\n---\n{excerpt}\n---\n\n\
Comece a resposta com UMA linha exatamente neste formato, sem markdown:\n\
TÍTULO: <título curto e específico do assunto da reunião, até 60 caracteres, sem aspas>\n\n\
Se a transcrição tiver a seção \"Momentos marcados\", são trechos que a pessoa que \
gravou marcou como importantes durante a reunião: dê prioridade a eles no resumo, \
nos pontos principais e nos action items.\n\n\
Depois gere exatamente estas seções, nesta ordem:\n\n\
## Resumo\nUm parágrafo objetivo.\n\n\
## Pontos principais\nBullets curtos.\n\n\
## Action items\nBullets no formato \"- [ ] tarefa — responsável\" quando o responsável \
for identificável; escreva \"Nenhum identificado.\" se não houver.\n\n\
## Decisões\nBullets; escreva \"Nenhuma registrada.\" se não houver."
    );
    let raw = provider.complete(SYSTEM, &user)?;
    let (title, body) = split_title(&raw);
    Ok(MeetingSummary { title, body })
}

/// Separa a linha `TÍTULO: …` (se vier) do resto. Tolera variações comuns de
/// modelo: sem acento, em negrito, com `#`, com aspas.
fn split_title(raw: &str) -> (Option<String>, String) {
    let mut lines = raw.lines();
    let mut consumed = 0usize;
    let mut title: Option<String> = None;
    for line in lines.by_ref() {
        consumed += 1;
        let trimmed = line.trim().trim_start_matches(['#', '*', ' ']).trim();
        if trimmed.is_empty() {
            continue;
        }
        let lowered = trimmed
            .chars()
            .map(|c| match c {
                'í' | 'Í' => 'i',
                other => other.to_ascii_lowercase(),
            })
            .collect::<String>();
        if let Some(rest) = lowered.strip_prefix("titulo:") {
            // Recorta o original na mesma posição para preservar acentos, e
            // descasca camadas de negrito/aspas/espaços até estabilizar.
            let cut = trimmed.len() - rest.len();
            let mut value = &trimmed[cut..];
            loop {
                let peeled = value
                    .trim()
                    .trim_matches(['*', '"', '“', '”', '\'', '`', ':']);
                if peeled == value {
                    break;
                }
                value = peeled;
            }
            if !value.is_empty() {
                title = Some(clamp_title(value));
            }
        } else {
            consumed -= 1; // primeira linha útil não é o título: fica no corpo
        }
        break;
    }
    let body: String = raw.lines().skip(consumed).collect::<Vec<_>>().join("\n");
    (title, body.trim().to_string())
}

fn clamp_title(value: &str) -> String {
    if value.chars().count() <= MAX_TITLE_CHARS {
        return value.to_string();
    }
    let mut out: String = value.chars().take(MAX_TITLE_CHARS - 1).collect();
    out.push('…');
    out
}

/// Encurta pelo meio respeitando limites de caracteres UTF-8.
fn shorten_middle(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let half = max_chars / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = {
        let all: Vec<char> = text.chars().collect();
        all[all.len() - half..].iter().collect()
    };
    format!("{head}\n\n[... trecho do meio omitido por tamanho ...]\n\n{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separa_titulo_do_corpo() {
        let (t, body) = split_title("TÍTULO: Planejamento PCP da semana 37\n\n## Resumo\nTexto.");
        assert_eq!(t.as_deref(), Some("Planejamento PCP da semana 37"));
        assert_eq!(body, "## Resumo\nTexto.");
    }

    #[test]
    fn tolera_variacoes_do_modelo() {
        let (t, _) = split_title("**Titulo:** \"Revisão de lead time\"\n## Resumo");
        assert_eq!(t.as_deref(), Some("Revisão de lead time"));
        let (t, body) = split_title("\n# TÍTULO: Kickoff\n## Resumo\nx");
        assert_eq!(t.as_deref(), Some("Kickoff"));
        assert_eq!(body, "## Resumo\nx");
    }

    #[test]
    fn sem_titulo_mantem_tudo_no_corpo() {
        let (t, body) = split_title("## Resumo\nSem linha de título.");
        assert_eq!(t, None);
        assert_eq!(body, "## Resumo\nSem linha de título.");
    }

    #[test]
    fn titulo_longo_e_recortado() {
        let long = "a".repeat(120);
        let (t, _) = split_title(&format!("TÍTULO: {long}"));
        assert_eq!(t.unwrap().chars().count(), MAX_TITLE_CHARS);
    }
}
