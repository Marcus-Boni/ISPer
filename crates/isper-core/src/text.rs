//! Pós-processamento do texto transcrito — tudo local, determinístico e
//! testado:
//!
//! - **filtro de alucinações**: o Whisper foi treinado em legendas e, em
//!   silêncio ou ruído, inventa "Legendas pela comunidade", repete uma palavra
//!   em loop ou emite só símbolos. Usamos a probabilidade de "não é fala" que o
//!   próprio modelo devolve por segmento + heurísticas de texto;
//! - **comandos de voz** do ditado ("nova linha", "ponto final", "vírgula",
//!   "apagar isso", "tudo em maiúsculas"…), casados por palavra inteira, sem
//!   diferenciar acentos nem maiúsculas;
//! - **correção pelo dicionário pessoal**: termos parecidos com um termo do
//!   dicionário (grafia, acento, maiúsculas) viram o termo exato.

use crate::engine::TranscriptSegment;

// ------------------------------------------------------------ utilidades

/// Minúsculas e sem acentos, caractere a caractere (1:1 — o tamanho em chars
/// não muda, o que permite casar posições com o texto original).
pub fn fold(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

fn fold_char(c: char) -> char {
    let lower = c.to_lowercase().next().unwrap_or(c);
    match lower {
        'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        other => other,
    }
}

/// Distância de Levenshtein em chars (textos curtos — O(n·m) é suficiente).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Uma palavra do texto original: onde está (índices de char), a forma
/// dobrada sem pontuação nas pontas, e a pontuação que a seguia.
#[derive(Debug, Clone)]
struct Tok {
    start: usize,
    end: usize,
    core: String,
    trailing: String,
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation() || matches!(c, '…' | '—' | '–' | '“' | '”' | '‘' | '’' | '«' | '»')
}

fn tokenize(text: &str) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        let core_raw = word.trim_matches(is_punct);
        let trailing: String = word
            .chars()
            .rev()
            .take_while(|c| is_punct(*c))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        toks.push(Tok {
            start,
            end: i,
            core: fold(core_raw),
            trailing,
        });
    }
    toks
}

// ---------------------------------------------------------- alucinações

/// Frases que o Whisper inventa em silêncio/ruído (herança das legendas em
/// que foi treinado). Comparadas no texto dobrado.
const HALLUCINATION_PHRASES: &[&str] = &[
    "legendas pela comunidade",
    "amara.org",
    "legendas por ",
    "legendado por",
    "subtitles by",
    "traducao e legendas",
    "tradução e legendas",
    "obrigado por assistir",
    "obrigada por assistir",
    "inscreva-se no canal",
    "inscreva se no canal",
    "curta e compartilhe",
    "ate o proximo video",
    "thank you for watching",
    "thanks for watching",
    "transcricao por",
    "legendagem",
    "www.",
    "http://",
    "https://",
    ".com.br",
    "[musica]",
    "[music]",
    "(musica)",
    "♪",
];

/// Probabilidade de "não é fala" acima da qual um segmento com texto é
/// descartado. O whisper.cpp usa 0,6 no seu próprio fallback; para jogar
/// fora texto já gerado somos mais exigentes.
pub const NO_SPEECH_DROP: f32 = 0.75;

/// `true` quando o segmento não parece fala de verdade.
pub fn is_hallucination(text: &str, no_speech_prob: f32) -> bool {
    let t = text.trim();
    if t.is_empty() || !t.chars().any(char::is_alphanumeric) {
        return true; // vazio ou só pontuação/símbolos
    }
    let f = fold(t);
    // "[Música]", "(risos)" etc.: um único bloco entre colchetes/parênteses.
    if (f.starts_with('[') && f.ends_with(']')) || (f.starts_with('(') && f.ends_with(')')) {
        return true;
    }
    if HALLUCINATION_PHRASES.iter().any(|p| f.contains(p)) {
        return true;
    }
    if no_speech_prob > NO_SPEECH_DROP {
        return true;
    }
    is_repetition_loop(&f)
}

/// "não não não não não" / "ok tudo bem ok tudo bem ok tudo bem…": um n-grama
/// (1 a 4 palavras) repetido 4+ vezes seguidas cobrindo ≥ 80% do texto.
fn is_repetition_loop(folded: &str) -> bool {
    let words: Vec<&str> = folded.split_whitespace().collect();
    if words.len() < 6 {
        return false;
    }
    for n in 1..=4 {
        if words.len() < n * 4 {
            continue;
        }
        let pattern = &words[..n];
        let mut reps = 0;
        let mut i = 0;
        while i + n <= words.len() && &words[i..i + n] == pattern {
            reps += 1;
            i += n;
        }
        if reps >= 4 && i * 10 >= words.len() * 8 {
            return true;
        }
    }
    false
}

/// Remove segmentos alucinados e corta o "loop" de fim de bloco: o mesmo
/// segmento repetido três ou mais vezes seguidas fica só com duas ocorrências
/// (gente repete "sim, sim" de verdade; três iguais já é o modelo travado).
pub fn filter_hallucinations(segments: Vec<TranscriptSegment>) -> Vec<TranscriptSegment> {
    let mut out: Vec<TranscriptSegment> = Vec::with_capacity(segments.len());
    let mut last = String::new();
    let mut run = 0usize;
    for s in segments {
        if is_hallucination(&s.text, s.no_speech_prob) {
            continue;
        }
        // Compara só letras/números: "Sim.", "sim" e "Sim" são a mesma repetição.
        let f: String = fold(s.text.trim())
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect();
        if f == last {
            run += 1;
            if run >= 2 {
                continue;
            }
        } else {
            run = 0;
            last = f;
        }
        out.push(s);
    }
    out
}

// ------------------------------------------------------- comandos de voz

/// Comando falado → o que entra no texto. Frases dobradas (sem acento),
/// casadas por palavra inteira; as mais longas têm prioridade.
const COMMANDS: &[(&str, &str)] = &[
    ("ponto de interrogacao", "?"),
    ("ponto de exclamacao", "!"),
    ("ponto e virgula", ";"),
    ("abre parenteses", "("),
    ("abrir parenteses", "("),
    ("fecha parenteses", ")"),
    ("fechar parenteses", ")"),
    ("quebra de linha", "\n"),
    ("proxima linha", "\n"),
    ("novo paragrafo", "\n\n"),
    ("nova linha", "\n"),
    ("ponto final", "."),
    ("dois pontos", ":"),
    ("abre aspas", "\""),
    ("abrir aspas", "\""),
    ("fecha aspas", "\""),
    ("fechar aspas", "\""),
    ("reticencias", "…"),
    ("travessao", "—"),
    ("virgula", ","),
    ("arroba", "@"),
];

/// Ditos no FIM do ditado, descartam tudo.
const DISCARD_PHRASES: &[&str] = &[
    "apagar isso",
    "apaga isso",
    "descartar isso",
    "descarta isso",
    "cancelar isso",
    "cancela isso",
    "cancelar",
    "cancela",
    "descartar",
];

/// Ditos no FIM do ditado, mudam a caixa do texto inteiro.
const UPPER_PHRASES: &[&str] = &["tudo em maiusculas", "tudo maiusculo", "em maiusculas"];
const LOWER_PHRASES: &[&str] = &["tudo em minusculas", "tudo minusculo", "em minusculas"];

#[derive(Debug, Clone, PartialEq)]
pub struct CommandResult {
    pub text: String,
    /// O usuário pediu para jogar o ditado fora ("apagar isso").
    pub discard: bool,
}

fn ends_with_phrase(toks: &[Tok], phrases: &[&str]) -> Option<usize> {
    for p in phrases {
        let k = p.split_whitespace().count();
        if toks.len() < k {
            continue;
        }
        let tail: Vec<&str> = toks[toks.len() - k..]
            .iter()
            .map(|t| t.core.as_str())
            .collect();
        if tail.join(" ") == *p {
            return Some(k);
        }
    }
    None
}

/// Interpreta os comandos de voz de um ditado.
pub fn apply_voice_commands(text: &str) -> CommandResult {
    let chars: Vec<char> = text.chars().collect();
    let mut toks = tokenize(text);
    if toks.is_empty() {
        return CommandResult {
            text: text.trim().to_string(),
            discard: false,
        };
    }
    if ends_with_phrase(&toks, DISCARD_PHRASES).is_some() {
        return CommandResult {
            text: String::new(),
            discard: true,
        };
    }
    let mut case: Option<bool> = None; // Some(true) = maiúsculas
    if let Some(k) = ends_with_phrase(&toks, UPPER_PHRASES) {
        toks.truncate(toks.len() - k);
        case = Some(true);
    } else if let Some(k) = ends_with_phrase(&toks, LOWER_PHRASES) {
        toks.truncate(toks.len() - k);
        case = Some(false);
    }

    let mut out = String::new();
    let mut i = 0;
    while i < toks.len() {
        let mut matched = false;
        for (phrase, replacement) in COMMANDS {
            let k = phrase.split_whitespace().count();
            if i + k > toks.len() {
                continue;
            }
            let window: Vec<&str> = toks[i..i + k].iter().map(|t| t.core.as_str()).collect();
            if window.join(" ") != *phrase {
                continue;
            }
            // Pontuação que o Whisper colou no fim do comando ("nova linha.") sai junto.
            push_replacement(&mut out, replacement);
            i += k;
            matched = true;
            break;
        }
        if !matched {
            let word: String = chars[toks[i].start..toks[i].end].iter().collect();
            if !out.is_empty() && !out.ends_with(['\n', '(', '"', '“', '@']) {
                out.push(' ');
            }
            out.push_str(&word);
            i += 1;
        }
    }
    let mut text = tidy(&out);
    if let Some(upper) = case {
        text = if upper {
            text.to_uppercase()
        } else {
            text.to_lowercase()
        };
    }
    CommandResult {
        text,
        discard: false,
    }
}

fn push_replacement(out: &mut String, rep: &str) {
    match rep {
        "\n" | "\n\n" => {
            while out.ends_with(' ') {
                out.pop();
            }
            out.push_str(rep);
        }
        "(" | "\"" | "“" | "—" | "@" => {
            if rep == "@" {
                while out.ends_with(' ') {
                    out.pop();
                }
            } else if !out.is_empty() && !out.ends_with(['\n', ' ']) {
                out.push(' ');
            }
            out.push_str(rep);
        }
        _ => {
            // pontuação de fecho: cola na palavra anterior
            while out.ends_with(' ') {
                out.pop();
            }
            out.push_str(rep);
        }
    }
}

/// Espaçamento e maiúsculas depois das trocas: sem espaço antes de ",.;:!?)",
/// um espaço depois de pontuação de fecho seguida de palavra, e letra
/// maiúscula depois de ". ", "? ", "! " e de quebra de linha.
fn tidy(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 8);
    let mut capitalize_next = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == ' ' {
            // espaço antes de pontuação de fecho: fora
            if let Some(&n) = chars.get(i + 1)
                && matches!(n, ',' | '.' | ';' | ':' | '!' | '?' | ')' | '…')
            {
                i += 1;
                continue;
            }
            if out.ends_with(' ') || out.ends_with('\n') || out.is_empty() {
                i += 1;
                continue;
            }
        }
        if capitalize_next && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
        }
        if matches!(c, '.' | '!' | '?' | '\n') {
            capitalize_next = true;
            // garante espaço depois de . ! ? quando vem letra colada
            if c != '\n'
                && let Some(&n) = chars.get(i + 1)
                && n.is_alphanumeric()
            {
                out.push(' ');
            }
        } else if c == '…' || c == ')' || c == '"' {
            // mantém a maiúscula pendente de um ponto anterior
        } else if !c.is_whitespace() && c != '(' && c != '"' {
            capitalize_next = false;
        }
        i += 1;
    }
    out.trim_matches(|c: char| c == ' ')
        .trim_end_matches('\n')
        .to_string()
}

// ------------------------------------------------------------ dicionário

/// Tolerância de grafia em função do tamanho do termo (chars dobrados).
fn max_distance(len: usize) -> usize {
    match len {
        0..=3 => 0,
        4..=6 => 1,
        7..=10 => 2,
        _ => 3,
    }
}

/// Troca palavras (ou sequências de palavras) parecidas com um termo do
/// dicionário pelo termo exato — corrige grafia, acento e maiúsculas
/// ("ísper" → "ISPer", "opt solve" → "OptSolv"). A pontuação que seguia a
/// palavra é mantida; quebras de linha também (cada linha é tratada à parte).
/// Termos com menos de 4 caracteres só casam exatamente.
pub fn apply_dictionary(text: &str, terms: &[String]) -> String {
    let terms: Vec<(Vec<String>, &str)> = terms
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| (t.split_whitespace().map(fold).collect::<Vec<_>>(), t))
        .collect();
    if terms.is_empty() {
        return text.to_string();
    }
    text.split('\n')
        .map(|line| apply_dictionary_line(line, &terms))
        .collect::<Vec<_>>()
        .join("\n")
}

fn apply_dictionary_line(text: &str, terms: &[(Vec<String>, &str)]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let toks = tokenize(text);
    let mut out = String::with_capacity(text.len() + 16);
    let mut i = 0;
    while i < toks.len() {
        let mut replaced = false;
        'terms: for (words, original) in terms {
            let k = words.len();
            if k == 0 {
                continue;
            }
            let target = words.join(" ");
            let target_glued = words.concat();
            // Duas janelas: k palavras (como no dicionário) e k+1 palavras
            // coladas — o Whisper às vezes parte um nome em dois ("opt solve").
            let windows: [(usize, String, &str); 2] =
                [(k, target.clone(), " "), (k + 1, target_glued.clone(), "")];
            for (span, target, sep) in windows {
                if i + span > toks.len() {
                    continue;
                }
                let window: Vec<&str> = toks[i..i + span].iter().map(|t| t.core.as_str()).collect();
                let candidate = window.join(sep);
                if candidate.is_empty() || target.is_empty() {
                    continue;
                }
                let exact = candidate == target;
                let close = !exact
                    && candidate.chars().next() == target.chars().next()
                    && levenshtein(&candidate, &target) <= max_distance(target.chars().count());
                if !(exact || close) {
                    continue;
                }
                // Já está grafado exatamente como no dicionário? Não mexe.
                let raw: String = chars[toks[i].start..toks[i + span - 1].end]
                    .iter()
                    .collect();
                if raw.trim_matches(is_punct) == *original {
                    break 'terms;
                }
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(original);
                out.push_str(&toks[i + span - 1].trailing);
                i += span;
                replaced = true;
                break 'terms;
            }
        }
        if !replaced {
            let word: String = chars[toks[i].start..toks[i].end].iter().collect();
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(&word);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, p: f32) -> TranscriptSegment {
        TranscriptSegment {
            start_secs: 0.0,
            end_secs: 1.0,
            text: text.into(),
            no_speech_prob: p,
        }
    }

    // ---- alucinações
    #[test]
    fn detecta_frases_de_legenda_e_simbolos() {
        assert!(is_hallucination("Legendas pela comunidade Amara.org", 0.1));
        assert!(is_hallucination("Obrigado por assistir!", 0.1));
        assert!(is_hallucination("♪ ♪ ♪", 0.1));
        assert!(is_hallucination("[Música]", 0.1));
        assert!(is_hallucination("...", 0.1));
        assert!(!is_hallucination("Vamos revisar o plano da semana.", 0.1));
    }

    #[test]
    fn usa_probabilidade_de_nao_fala() {
        assert!(is_hallucination("Tá bom.", 0.9));
        assert!(!is_hallucination("Tá bom.", 0.5));
    }

    #[test]
    fn detecta_loop_de_repeticao() {
        assert!(is_hallucination("não não não não não não não", 0.1));
        assert!(is_hallucination(
            "tudo bem tudo bem tudo bem tudo bem tudo bem",
            0.1
        ));
        assert!(!is_hallucination(
            "não, não vamos fazer isso agora, não faz sentido",
            0.1
        ));
    }

    #[test]
    fn filtra_segmentos_e_corta_repeticoes_consecutivas() {
        let segs = vec![
            seg("Olá a todos.", 0.1),
            seg("Legendas pela comunidade", 0.2),
            seg("Sim.", 0.1),
            seg("Sim.", 0.1),
            seg("sim", 0.1),
            seg("Sim.", 0.1),
            seg("Vamos começar.", 0.95),
            seg("Vamos começar.", 0.1),
        ];
        let out: Vec<String> = filter_hallucinations(segs)
            .into_iter()
            .map(|s| s.text)
            .collect();
        assert_eq!(out, vec!["Olá a todos.", "Sim.", "Sim.", "Vamos começar."]);
    }

    // ---- comandos de voz
    #[test]
    fn pontuacao_e_quebras_de_linha() {
        let r = apply_voice_commands(
            "Bom dia vírgula tudo bem ponto de interrogação nova linha segue o relatório ponto final",
        );
        assert!(!r.discard);
        assert_eq!(r.text, "Bom dia, tudo bem?\nSegue o relatório.");
    }

    #[test]
    fn comandos_com_pontuacao_colada_e_acentos_variados() {
        let r =
            apply_voice_commands("Primeiro item Nova Linha. segundo item novo parágrafo Terceiro");
        assert_eq!(r.text, "Primeiro item\nSegundo item\n\nTerceiro");
        let r = apply_voice_commands("marcus arroba gmail ponto final");
        assert_eq!(r.text, "marcus@gmail.");
    }

    #[test]
    fn apagar_isso_descarta_tudo() {
        let r = apply_voice_commands("isso aqui ficou errado apagar isso");
        assert!(r.discard);
        assert_eq!(r.text, "");
        assert!(apply_voice_commands("cancela").discard);
        assert!(!apply_voice_commands("vamos cancelar a reunião de sexta").discard);
    }

    #[test]
    fn caixa_alta_e_baixa_no_fim() {
        assert_eq!(
            apply_voice_commands("atenção urgente tudo em maiúsculas").text,
            "ATENÇÃO URGENTE"
        );
        assert_eq!(
            apply_voice_commands("Bom Dia Marcus tudo em minúsculas").text,
            "bom dia marcus"
        );
    }

    #[test]
    fn texto_sem_comandos_passa_intacto() {
        let r = apply_voice_commands("Segue em anexo o plano de produção revisado.");
        assert_eq!(r.text, "Segue em anexo o plano de produção revisado.");
    }

    // ---- dicionário
    #[test]
    fn corrige_grafia_acento_e_caixa() {
        let terms = vec![
            "ISPer".to_string(),
            "OptSolv".to_string(),
            "Tatiana".to_string(),
        ];
        assert_eq!(
            apply_dictionary("o ísper transcreve bem", &terms),
            "o ISPer transcreve bem"
        );
        assert_eq!(
            apply_dictionary("falei com a opt solve hoje.", &terms),
            "falei com a OptSolv hoje."
        );
        assert_eq!(
            apply_dictionary("a tatiana confirmou, ótimo", &terms),
            "a Tatiana confirmou, ótimo"
        );
    }

    #[test]
    fn nao_mexe_no_que_nao_parece() {
        let terms = vec!["Renan".to_string(), "PCP".to_string()];
        let t = "a reunião foi boa e o pcb chegou, renato";
        // "pcb" ≠ "PCP" (termo curto: só exato) e "renato" não é "Renan".
        assert_eq!(apply_dictionary(t, &terms), t);
    }

    #[test]
    fn preserva_quebras_de_linha() {
        let terms = vec!["ISPer".to_string()];
        assert_eq!(
            apply_dictionary("isper aqui\nisper ali", &terms),
            "ISPer aqui\nISPer ali"
        );
    }

    #[test]
    fn termos_curtos_so_casam_exatamente() {
        let terms = vec!["PCP".to_string()];
        assert_eq!(apply_dictionary("o pcp e o pcb", &terms), "o PCP e o pcb");
    }

    #[test]
    fn levenshtein_basico() {
        assert_eq!(levenshtein("isper", "isper"), 0);
        assert_eq!(levenshtein("isper", "ispe"), 1);
        assert_eq!(levenshtein("opt solve", "optsolv"), 2);
    }
}
