//! A reunião "pronta para agente" (Fase 11.3, ADR 0029): um Markdown que se
//! entrega inteiro a outro agente de IA, que responde sobre a reunião sem
//! pedir contexto.
//!
//! Nada de IA aqui: só reorganiza o que o banco já tem. Primeiro um
//! cabeçalho YAML (o que um programa lê), depois uma linha de contexto (o que
//! "Eu" e "Participante N" querem dizer), o resumo, o que foi validado no
//! Copilot, as notas e os momentos, e por fim as falas com horário. A ata
//! `.md` de sempre não muda: o import a lê de volta.

use crate::meeting::{SegmentRef, fmt_ts, group_speech};
use crate::store::MeetingDetail;

/// `"dd/mm/aaaa hh:mm"` → `("aaaa-mm-dd", "hh:mm")`.
fn iso_parts(started_at: &str) -> Option<(String, String)> {
    let (date, time) = started_at.trim().split_once(' ')?;
    let mut p = date.split('/');
    let (d, m, y) = (p.next()?, p.next()?, p.next()?);
    if d.len() != 2 || m.len() != 2 || y.len() != 4 {
        return None;
    }
    Some((format!("{y}-{m}-{d}"), time.chars().take(5).collect()))
}

/// Texto numa linha, entre aspas, seguro para YAML.
fn yaml_str(s: &str) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("\"{}\"", one.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Os falantes na ordem em que aparecem.
fn speakers(d: &MeetingDetail) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for s in &d.segments {
        if !s.text.trim().is_empty() && !out.contains(&s.speaker.as_str()) {
            out.push(&s.speaker);
        }
    }
    out
}

const KIND_PT: [(&str, &str); 4] = [
    ("decision", "Decisão"),
    ("action", "Ação"),
    ("risk", "Risco"),
    ("question", "Pergunta"),
];

/// O Markdown para agente. `d.segments` já vem na versão que vai (a
/// otimizada, quando `optimized`); `project` é o nome do projeto.
pub fn to_agent_markdown(d: &MeetingDetail, project: Option<&str>, optimized: bool) -> String {
    let m = &d.meeting;
    let (date, time) = iso_parts(&m.started_at).unwrap_or((m.started_at.clone(), String::new()));
    let people = speakers(d);
    let mut out = String::from("---\ntipo: reuniao\n");
    out.push_str(&format!("titulo: {}\n", yaml_str(&m.title)));
    out.push_str(&format!("data: {}\n", yaml_str(&date)));
    if !time.is_empty() {
        out.push_str(&format!("inicio: {}\n", yaml_str(&time)));
    }
    out.push_str(&format!(
        "duracao_min: {}\n",
        (m.duration_secs / 60.0).round() as i64
    ));
    out.push_str(&format!(
        "projeto: {}\n",
        project.map(yaml_str).unwrap_or_else(|| "null".into())
    ));
    out.push_str(&format!(
        "participantes: [{}]\n",
        people
            .iter()
            .map(|p| yaml_str(p))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    match &m.source_name {
        Some(file) => out.push_str(&format!(
            "origem: {}\n",
            yaml_str(&format!("importada de {file}"))
        )),
        None => out.push_str("origem: \"gravada no ISPer\"\n"),
    }
    if let Some(e) = &d.event {
        out.push_str(&format!("evento_da_agenda: {}\n", yaml_str(&e.subject)));
    }
    out.push_str(&format!(
        "transcricao: {}\n---\n\n",
        if optimized { "otimizada" } else { "original" }
    ));

    out.push_str(&format!("# {}\n\n", m.title.trim()));
    let when = if time.is_empty() {
        date.clone()
    } else {
        format!("{date} às {time}")
    };
    let proj = project
        .map(|p| format!(", projeto {p}"))
        .unwrap_or_default();
    out.push_str(&format!(
        "> Reunião de {when}, {} min{proj}. \"Eu\" é quem gravou; \"Participante N\" são as outras vozes, sem nome identificado. Transcrição automática{}; pode ter erros de reconhecimento.\n",
        (m.duration_secs / 60.0).round() as i64,
        if optimized {
            ", limpa pelo ISPer (hesitações e repetições removidas; [?] marca trecho incerto)"
        } else {
            ""
        }
    ));

    if let Some(summary) = d
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        out.push_str("\n## Resumo (gerado por IA)\n\n");
        // Os títulos do resumo descem um nível, para caber debaixo deste.
        for line in summary.lines() {
            if line.starts_with('#') {
                out.push('#');
            }
            out.push_str(line);
            out.push('\n');
        }
    }
    if !d.decisions.is_empty() {
        out.push_str("\n## Validado durante a reunião\n\n");
        for c in &d.decisions {
            let kind = KIND_PT
                .iter()
                .find(|(k, _)| *k == c.kind)
                .map_or(c.kind.as_str(), |(_, pt)| pt);
            out.push_str(&format!(
                "- [{}] **{kind}:** {}",
                fmt_ts(c.at_secs),
                c.title.trim()
            ));
            if !c.description.trim().is_empty() {
                out.push_str(&format!(" — {}", c.description.trim()));
            }
            let mut extra = Vec::new();
            if let Some(o) = c.owner.as_deref().filter(|o| !o.trim().is_empty()) {
                extra.push(format!("responsável: {}", o.trim()));
            }
            if let Some(due) = c.due_date.as_deref().filter(|o| !o.trim().is_empty()) {
                extra.push(format!("prazo: {}", due.trim()));
            }
            if !extra.is_empty() {
                out.push_str(&format!(" ({})", extra.join("; ")));
            }
            out.push('\n');
        }
    }
    if let Some(notes) = d.notes.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str(&format!("\n## Notas de quem gravou\n\n{notes}\n"));
    }
    if !d.moments.is_empty() {
        out.push_str("\n## Momentos marcados como importantes\n\n");
        for at in &d.moments {
            out.push_str(&format!("- [{}]\n", fmt_ts(*at)));
        }
    }
    out.push_str("\n## Transcrição\n");
    let refs = d.segments.iter().map(|s| SegmentRef {
        speaker: &s.speaker,
        start_secs: s.start_secs,
        end_secs: s.end_secs,
        text: &s.text,
    });
    for g in group_speech(refs) {
        out.push_str(&format!(
            "\n[{}] {}: {}\n",
            fmt_ts(g.start_secs),
            g.speaker,
            g.text
        ));
    }
    out
}

/// O nome padronizado do arquivo: `aaaa-mm-dd hhhmm - Projeto - Assunto.md`
/// (sem projeto, sem o meio), sem o que o Windows não aceita num nome.
pub fn agent_file_name(d: &MeetingDetail, project: Option<&str>) -> String {
    let m = &d.meeting;
    let stamp = iso_parts(&m.started_at)
        .map(|(date, time)| format!("{date} {}", time.replacen(':', "h", 1)))
        .unwrap_or_else(|| format!("reuniao-{}", m.id));
    let clean = |s: &str, max: usize| -> String {
        let s: String = s
            .chars()
            .map(|c| {
                if "<>:\"/\\|?*".contains(c) || c.is_control() {
                    ' '
                } else {
                    c
                }
            })
            .collect();
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        s.chars()
            .take(max)
            .collect::<String>()
            .trim_end_matches(['.', ' '])
            .to_string()
    };
    let mut parts = vec![stamp];
    if let Some(p) = project.map(|p| clean(p, 40)).filter(|p| !p.is_empty()) {
        parts.push(p);
    }
    let title = clean(&m.title, 80);
    if !title.is_empty() {
        parts.push(title);
    }
    format!("{}.md", parts.join(" - "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{MeetingRow, StoredDecision, StoredSegment};

    fn seg(speaker: &str, at: f32, text: &str) -> StoredSegment {
        StoredSegment {
            speaker: speaker.into(),
            start_secs: at,
            end_secs: at + 3.0,
            text: text.into(),
            detected_speaker: None,
            clean_text: None,
        }
    }

    fn detail() -> MeetingDetail {
        MeetingDetail {
            meeting: MeetingRow {
                id: 7,
                title: "Revisão: escopo \"v2\" / portal?".into(),
                started_at: "20/11/2026 10:05".into(),
                duration_secs: 1830.0,
                segments: 3,
                participants: 1,
                has_summary: true,
                md_path: None,
                moments: 1,
                decisions: 1,
                source_name: None,
                project_id: Some("p1".into()),
            },
            summary: Some(
                "## Resumo\nFechamos o escopo.\n\n## Action items\n- [ ] Proposta — Eu".into(),
            ),
            segments: vec![
                seg("Eu", 0.0, "Vamos fechar o escopo."),
                seg("Eu", 4.0, "Na sexta."),
                seg("Participante 1", 70.0, "Fechado."),
            ],
            moments: vec![70.0],
            decisions: vec![StoredDecision {
                kind: "action".into(),
                title: "Mandar a proposta".into(),
                description: String::new(),
                owner: Some("Eu".into()),
                due_date: Some("sexta".into()),
                urgency: "high".into(),
                at_secs: 4.0,
            }],
            notes: Some("Cliente quer o portal antes do Natal".into()),
            event: None,
        }
    }

    #[test]
    fn markdown_para_agente_tem_cabecalho_contexto_e_falas_com_horario() {
        let md = to_agent_markdown(&detail(), Some("Portal do Cliente"), true);
        assert!(
            md.starts_with(
                "---\ntipo: reuniao\ntitulo: \"Revisão: escopo \\\"v2\\\" / portal?\"\n"
            ),
            "{md}"
        );
        for want in [
            "data: \"2026-11-20\"\n",
            "inicio: \"10:05\"\n",
            "duracao_min: 31\n",
            "projeto: \"Portal do Cliente\"\n",
            "participantes: [\"Eu\", \"Participante 1\"]\n",
            "origem: \"gravada no ISPer\"\n",
            "transcricao: otimizada\n---\n",
            "> Reunião de 2026-11-20 às 10:05, 31 min, projeto Portal do Cliente.",
            "[?] marca trecho incerto",
            "## Resumo (gerado por IA)\n\n### Resumo\nFechamos o escopo.",
            "- [00:04] **Ação:** Mandar a proposta (responsável: Eu; prazo: sexta)\n",
            "## Notas de quem gravou\n\nCliente quer o portal antes do Natal\n",
            "- [01:10]\n",
            "\n[00:00] Eu: Vamos fechar o escopo. Na sexta.\n",
            "\n[01:10] Participante 1: Fechado.\n",
        ] {
            assert!(md.contains(want), "faltou {want:?} em:\n{md}");
        }
        let sem = to_agent_markdown(&detail(), None, false);
        assert!(sem.contains("projeto: null\n") && sem.contains("transcricao: original\n"));
        assert!(!sem.contains("[?] marca"));
    }

    #[test]
    fn nome_do_arquivo_padronizado_e_aceito_pelo_windows() {
        assert_eq!(
            agent_file_name(&detail(), Some("Portal do Cliente")),
            "2026-11-20 10h05 - Portal do Cliente - Revisão escopo v2 portal.md"
        );
        assert_eq!(
            agent_file_name(&detail(), None),
            "2026-11-20 10h05 - Revisão escopo v2 portal.md"
        );
        let mut odd = detail();
        odd.meeting.started_at = "ontem".into();
        assert!(agent_file_name(&odd, None).starts_with("reuniao-7 - "));
    }
}
