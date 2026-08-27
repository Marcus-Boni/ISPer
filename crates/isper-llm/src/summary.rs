//! Resumo de reuniões: transforma o transcript em resumo, pontos principais,
//! action items e decisões — sempre em pt-BR.

use crate::providers::LlmProvider;
use crate::Result;

/// Transcripts maiores que isso são encurtados pelo MEIO (início e fim
/// carregam abertura e encaminhamentos — o miolo é o mais comprimível).
const MAX_TRANSCRIPT_CHARS: usize = 60_000;

const SYSTEM: &str = "Você é um assistente especialista em resumir reuniões de trabalho. \
Responda SEMPRE em português do Brasil, em Markdown. Seja fiel ao que foi dito — \
NÃO invente informações que não estão no transcript. O transcript foi gerado por \
transcrição automática de fala e pode conter pequenos erros; interprete com bom senso.";

pub fn summarize_meeting(provider: &dyn LlmProvider, transcript: &str) -> Result<String> {
    let excerpt = shorten_middle(transcript, MAX_TRANSCRIPT_CHARS);
    let user = format!(
        "Abaixo está a transcrição de uma reunião. \"Eu\" é a pessoa que gravou; \
\"Participantes\" são as demais vozes.\n\n---\n{excerpt}\n---\n\n\
Gere exatamente estas seções, nesta ordem:\n\n\
## Resumo\nUm parágrafo objetivo.\n\n\
## Pontos principais\nBullets curtos.\n\n\
## Action items\nBullets no formato \"- [ ] tarefa — responsável\" quando o responsável \
for identificável; escreva \"Nenhum identificado.\" se não houver.\n\n\
## Decisões\nBullets; escreva \"Nenhuma registrada.\" se não houver."
    );
    provider.complete(SYSTEM, &user)
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
