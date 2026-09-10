//! Busca semântica — a parte que não depende de provider: recortar reuniões e
//! ditados em trechos do tamanho certo para virar vetores (embeddings),
//! guardar/ler vetores como bytes e ranquear por similaridade.
//!
//! Os vetores em si vêm de fora (crate `isper-llm`); aqui só há aritmética.

use crate::meeting::{SegmentRef, group_speech};

/// Tamanho alvo de um trecho, em bytes UTF-8 (~150 tokens): pequeno o
/// bastante para a similaridade apontar o assunto certo, grande o bastante
/// para ter contexto.
pub const CHUNK_TARGET_CHARS: usize = 700;
/// Parágrafos maiores que isso são fatiados por frases.
pub const CHUNK_MAX_CHARS: usize = 1400;

/// Um trecho pronto para virar vetor. `start_secs` é `None` quando o texto
/// não está no relógio da reunião (resumo, ditado).
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub start_secs: Option<f32>,
    pub text: String,
}

/// Recorta o transcript em trechos: parágrafos consecutivos (regras de
/// [`group_speech`]) são juntados até [`CHUNK_TARGET_CHARS`]; cada trecho
/// carrega o instante em que começa, para a Biblioteca rolar até ele.
/// Os nomes dos falantes ficam de fora: eles mudam depois (diarização,
/// renomear) e o índice não precisa ser refeito por isso.
pub fn chunk_meeting(segments: &[SegmentRef<'_>]) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();
    let mut current: Option<Chunk> = None;
    for group in group_speech(segments.iter().copied()) {
        let piece = group.text.trim();
        if piece.is_empty() {
            continue;
        }
        if let Some(chunk) = current.as_mut()
            && chunk.text.len() + 1 + piece.len() <= CHUNK_TARGET_CHARS
        {
            chunk.text.push(' ');
            chunk.text.push_str(piece);
            continue;
        }
        if let Some(chunk) = current.take() {
            out.push(chunk);
        }
        if piece.len() > CHUNK_MAX_CHARS {
            out.extend(
                chunk_text(piece, CHUNK_TARGET_CHARS)
                    .into_iter()
                    .map(|text| Chunk {
                        start_secs: Some(group.start_secs),
                        text,
                    }),
            );
        } else {
            current = Some(Chunk {
                start_secs: Some(group.start_secs),
                text: piece.to_string(),
            });
        }
    }
    if let Some(chunk) = current {
        out.push(chunk);
    }
    out
}

/// Fatia um texto corrido em pedaços de até ~`target` bytes, cortando em fim
/// de frase (`.`, `!`, `?`, quebra de linha); uma frase enorme sem pontuação
/// é cortada por palavras. Nunca corta no meio de um caractere.
pub fn chunk_text(text: &str, target: usize) -> Vec<String> {
    let target = target.max(16);
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let flush = |current: &mut String, out: &mut Vec<String>| {
        let t = current.trim();
        if !t.is_empty() {
            out.push(t.to_string());
        }
        current.clear();
    };
    for sentence in sentences(text) {
        if !current.is_empty() && current.len() + 1 + sentence.len() > target {
            flush(&mut current, &mut out);
        }
        if sentence.len() > target {
            for word in sentence.split_whitespace() {
                if !current.is_empty() && current.len() + 1 + word.len() > target {
                    flush(&mut current, &mut out);
                }
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(sentence);
        }
    }
    flush(&mut current, &mut out);
    out
}

/// Frases: termina em `.`, `!`, `?` seguido de espaço (ou fim), ou em quebra de linha.
fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let bytes = text.as_bytes();
    let mut iter = text.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        let end_here = match c {
            '\n' => true,
            '.' | '!' | '?' => iter.peek().is_none_or(|(_, next)| next.is_whitespace()),
            _ => false,
        };
        if end_here {
            let end = i + c.len_utf8();
            let piece = text[start..end].trim();
            if !piece.is_empty() {
                out.push(piece);
            }
            start = end;
        }
    }
    if start < bytes.len() {
        let piece = text[start..].trim();
        if !piece.is_empty() {
            out.push(piece);
        }
    }
    out
}

/// Normaliza para norma 1 — assim o produto escalar É o cosseno.
pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Produto escalar (cosseno, se os dois estiverem normalizados). Dimensões
/// diferentes valem zero — nunca comparar vetores de modelos distintos.
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Vetor → bytes (f32 little-endian), o formato guardado no SQLite.
pub fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn from_blob(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&c| f32::from_le_bytes(c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg<'a>(speaker: &'a str, start: f32, text: &'a str) -> SegmentRef<'a> {
        SegmentRef {
            speaker,
            start_secs: start,
            end_secs: start + 1.0,
            text,
        }
    }

    #[test]
    fn junta_paragrafos_ate_o_alvo_e_guarda_o_inicio() {
        let long = "palavra ".repeat(60); // ~480 bytes
        let segs = [
            seg("Eu", 0.0, "Bom dia."),
            seg("Participante 1", 10.0, "Bom dia, vamos começar."),
            // Ainda cabe no primeiro trecho (~515 bytes).
            seg("Eu", 20.0, &long),
            // Outro falante, 10 s depois: parágrafo novo que não cabe mais.
            seg("Participante 1", 30.0, &long),
        ];
        let chunks = chunk_meeting(&segs);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].start_secs, Some(0.0));
        assert!(
            chunks[0]
                .text
                .starts_with("Bom dia. Bom dia, vamos começar. palavra")
        );
        assert!(
            !chunks[0].text.contains("Participante 1"),
            "sem nomes de falante"
        );
        assert_eq!(chunks[1].start_secs, Some(30.0));
    }

    #[test]
    fn paragrafo_gigante_e_fatiado_por_frases() {
        let giant = "Uma frase razoável de teste. ".repeat(80); // ~2,3 KB
        let chunks = chunk_meeting(&[seg("Eu", 5.0, &giant)]);
        assert!(chunks.len() >= 3);
        assert!(chunks.iter().all(|c| c.text.len() <= CHUNK_TARGET_CHARS));
        assert!(chunks.iter().all(|c| c.start_secs == Some(5.0)));
        assert!(chunks.iter().all(|c| c.text.ends_with('.')));
    }

    #[test]
    fn chunk_text_corta_em_frases_e_por_palavras_sem_pontuacao() {
        let parts = chunk_text("Primeira frase. Segunda frase! Terceira?", 20);
        assert_eq!(
            parts,
            vec!["Primeira frase.", "Segunda frase!", "Terceira?"]
        );
        let words = chunk_text(&"abc ".repeat(30), 24);
        assert!(words.len() > 3);
        assert!(words.iter().all(|w| w.len() <= 24));
        // Acentos: nunca corta no meio de um caractere.
        let acc = chunk_text(&"ação ".repeat(40), 41);
        assert!(acc.iter().all(|p| p.chars().all(|c| c != '\u{FFFD}')));
        assert!(chunk_text("   ", 100).is_empty());
    }

    #[test]
    fn vetores_normalizam_e_sobrevivem_ao_blob() {
        let mut v = vec![3.0, 4.0];
        normalize(&mut v);
        assert!((dot(&v, &v) - 1.0).abs() < 1e-6);
        assert_eq!(from_blob(&to_blob(&v)), v);
        assert_eq!(dot(&[1.0, 0.0], &[1.0]), 0.0, "dimensões diferentes = 0");
        let mut zero = vec![0.0, 0.0];
        normalize(&mut zero);
        assert_eq!(zero, vec![0.0, 0.0]);
    }
}
