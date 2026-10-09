//! Comparar títulos: minúsculas, sem acento, as palavras de 3 letras ou mais.
//!
//! Serve para juntar a mesma ação dita no Copilot e achada pela IA
//! ([`crate::meeting`]) e para reconhecer a mesma tarefa em semanas
//! diferentes ([`crate::learn`]).

use std::collections::BTreeSet;

/// Minúsculas e sem acento.
pub(crate) fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// As palavras que contam (3 letras ou mais), dobradas.
pub(crate) fn words(title: &str) -> BTreeSet<String> {
    fold(title)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(String::from)
        .collect()
}

/// Jaccard das palavras: 1 é o mesmo título, 0 nada em comum.
pub(crate) fn similarity(a: &str, b: &str) -> f32 {
    let (a, b) = (words(a), words(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(&b).count() as f32;
    inter / ((a.len() + b.len()) as f32 - inter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesmo_titulo_com_acento_e_artigo() {
        assert_eq!(similarity("Revisar os PRs", "revisar PRs"), 1.0);
        assert!(similarity("Revisar os PRs abertos", "Revisar PRs") >= 0.6);
        assert_eq!(similarity("Ligar pro João", "Pagar o boleto"), 0.0);
        assert_eq!(similarity("", "algo"), 0.0);
    }
}
