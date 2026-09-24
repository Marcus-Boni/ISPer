//! O que o nome de um arquivo gravado diz sobre a gravação (Fase 9.0):
//! quando ela aconteceu e, se o nome for descritivo, o título da reunião.
//!
//! Gravadores e celulares costumam pôr a data no nome
//! (`2026-09-22 15-30-46.mp3`, `REC_20260922_153046.m4a`). A data de
//! modificação do arquivo é a da exportação, não a da conversa — então o nome,
//! quando traz data e hora, vale mais. Sem isso, quem chama usa a data do
//! arquivo (o núcleo não conhece o fuso horário local).

/// Data e hora escritas no nome do arquivo, no formato das reuniões
/// (`dd/mm/aaaa hh:mm`).
///
/// Reconhece ano, mês, dia, hora e minuto (segundos opcionais) com qualquer
/// separador (`2026-09-22 15-30-46`, `2026_09_22T15.30`), o formato compacto
/// (`20260922_153046`, `202609221530`) e o dia primeiro
/// (`22-09-2026 15h30`). Só data, sem hora, não conta: dá `None`.
pub fn started_at_from_name(stem: &str) -> Option<String> {
    let found = digit_groups(stem);
    let groups: Vec<String> = found.iter().map(|(g, _)| g.clone()).collect();
    // Separados: [ano, mês, dia, hora, minuto, (segundo)] ou [dia, mês, ano, ...].
    for w in groups.windows(5) {
        let [a, b, c, h, m] = [&w[0], &w[1], &w[2], &w[3], &w[4]];
        if h.len() > 2 || m.len() != 2 {
            continue;
        }
        let ymd = if a.len() == 4 && b.len() <= 2 && c.len() <= 2 {
            Some((a, b, c))
        } else if c.len() == 4 && a.len() <= 2 && b.len() <= 2 {
            Some((c, b, a))
        } else {
            None
        };
        if let Some((y, mo, d)) = ymd
            && let Some(s) = stamp(y, mo, d, h, m)
        {
            return Some(s);
        }
    }
    // Compacto: AAAAMMDD seguido de HHMM(SS), juntos ou separados.
    for (i, g) in groups.iter().enumerate() {
        let (data, hora) = match g.len() {
            12 | 14 => (&g[..8], &g[8..12]),
            // A hora separada não pode vir colada numa letra: em
            // `AUD-20260922-WA0003`, o `0003` é um contador, não 00:03.
            8 => match found.get(i + 1) {
                Some((next, false)) if next.len() == 4 || next.len() == 6 => {
                    (g.as_str(), &next[..4])
                }
                _ => continue,
            },
            _ => continue,
        };
        if let Some(s) = stamp(
            &data[..4],
            &data[4..6],
            &data[6..8],
            &hora[..2],
            &hora[2..4],
        ) {
            return Some(s);
        }
    }
    None
}

/// `dd/mm/aaaa hh:mm`, se os números formarem uma data e hora de verdade.
fn stamp(y: &str, mo: &str, d: &str, h: &str, mi: &str) -> Option<String> {
    let (y, mo, d, h, mi): (u32, u32, u32, u32, u32) = (
        y.parse().ok()?,
        mo.parse().ok()?,
        d.parse().ok()?,
        h.parse().ok()?,
        mi.parse().ok()?,
    );
    let dias = match mo {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) => 29,
        2 => 28,
        _ => return None,
    };
    ((2000..=2099).contains(&y) && (1..=dias).contains(&d) && h <= 23 && mi <= 59)
        .then(|| format!("{d:02}/{mo:02}/{y} {h:02}:{mi:02}"))
}

/// As sequências de dígitos do texto, na ordem, cada uma dizendo se vem
/// colada numa letra (`WA0003`).
fn digit_groups(s: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut atual = String::new();
    let mut colada = false;
    let mut anterior: Option<char> = None;
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            if atual.is_empty() {
                colada = anterior.is_some_and(char::is_alphabetic);
            }
            atual.push(ch);
        } else if !atual.is_empty() {
            out.push((std::mem::take(&mut atual), colada));
        }
        anterior = Some(ch);
    }
    if !atual.is_empty() {
        out.push((atual, colada));
    }
    out
}

/// Palavras que gravadores põem no nome e que não descrevem a conversa.
const GENERIC_WORDS: &[&str] = &[
    "rec",
    "record",
    "recording",
    "recordings",
    "gravacao",
    "gravação",
    "gravador",
    "audio",
    "áudio",
    "voice",
    "voz",
    "memo",
    "nota",
    "note",
    "notes",
    "nova",
    "novo",
    "new",
    "plaud",
    "track",
    "faixa",
    "file",
    "arquivo",
    "reuniao",
    "reunião",
    "meeting",
    "whatsapp",
    "ptt",
    "aud",
    "vn",
    "call",
    "chamada",
    "teams",
    "zoom",
    "sound",
    "som",
    "untitled",
    "sem",
    "titulo",
    "título",
    "de",
    "da",
    "do",
    "h",
    "m",
    "s",
    "t",
];

/// O nome descreve a conversa ("Reunião com fornecedor") ou é só um carimbo
/// de gravador ("REC_0012", "2026-09-22 15-30-46", "Nova gravação 3")?
pub fn is_descriptive_name(stem: &str) -> bool {
    let lower = stem.to_lowercase();
    let significativas: usize = lower
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty() && !GENERIC_WORDS.contains(w))
        .map(|w| w.chars().count())
        .sum();
    significativas >= 3
}

/// Título da reunião a partir do nome do arquivo: o próprio nome, arrumado,
/// quando ele é descritivo; `None` quando é só um carimbo (aí vale o título
/// que a IA sugerir, ou o padrão com a data).
pub fn title_from_name(stem: &str) -> Option<String> {
    if !is_descriptive_name(stem) {
        return None;
    }
    let limpo = stem
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let limpo: String = limpo.chars().take(120).collect();
    let limpo = limpo.trim();
    (!limpo.is_empty()).then(|| limpo.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_e_hora_no_nome_nos_formatos_de_gravador() {
        for (nome, esperado) in [
            ("2026-09-22 15-30-46", "22/09/2026 15:30"),
            ("2026_09_22T15.30", "22/09/2026 15:30"),
            ("REC_20260922_153046", "22/09/2026 15:30"),
            ("202609221530", "22/09/2026 15:30"),
            ("20260922-0905", "22/09/2026 09:05"),
            ("Reunião 22-09-2026 15h30", "22/09/2026 15:30"),
            ("Plaud 2026-9-2 8-07 cliente", "02/09/2026 08:07"),
        ] {
            assert_eq!(
                started_at_from_name(nome).as_deref(),
                Some(esperado),
                "{nome}"
            );
        }
    }

    #[test]
    fn sem_data_e_hora_validas_nao_inventa() {
        for nome in [
            "Reunião com fornecedor",
            "2026-09-22",
            "REC_0012",
            "2026-13-40 25-99",
            "2026-02-30 10-00",
            "gravacao 1999-01-01 10-00",
            "20260922",
            "AUD-20260922-WA0003",
        ] {
            assert_eq!(started_at_from_name(nome), None, "{nome}");
        }
        assert_eq!(
            started_at_from_name("2024-02-29 10-00").as_deref(),
            Some("29/02/2024 10:00"),
            "ano bissexto"
        );
    }

    #[test]
    fn nome_descritivo_vira_titulo_e_carimbo_nao() {
        assert_eq!(
            title_from_name("Reunião_com  fornecedor").as_deref(),
            Some("Reunião com fornecedor")
        );
        assert_eq!(
            title_from_name("Planejamento Q4 - 2026-09-22 15-30").as_deref(),
            Some("Planejamento Q4 - 2026-09-22 15-30")
        );
        for carimbo in [
            "REC_0012",
            "2026-09-22 15-30-46",
            "Nova gravação 3",
            "New Recording 12",
            "PTT-20260922-WA0003",
            "audio_2026_09_22",
            "Reunião 22-09-2026 15h30",
            "Plaud Note 0042",
        ] {
            assert_eq!(title_from_name(carimbo), None, "{carimbo}");
        }
    }
}
