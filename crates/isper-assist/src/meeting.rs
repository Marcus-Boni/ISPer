//! As ações de uma reunião gravada viram tarefas na caixa de entrada (Fase
//! 10.3), com o minuto em que foram ditas.
//!
//! Duas fontes: o que você validou no Copilot com dono "Eu" e o que a IA
//! achou na transcrição quando a reunião acabou. A validada vence: uma
//! extraída com título parecido e dita até [`SAME_ACTION_SECS`] de distância
//! é a mesma ação e não entra de novo. Caixa de entrada, e não "para hoje":
//! quem decide o dia é você, ao aceitar.

use chrono::NaiveDate;
use serde_json::json;

use crate::model::{NewTask, SourceKind, TaskStatus};
use crate::when;

/// Duas ações parecidas ditas até este tanto de distância são a mesma.
pub const SAME_ACTION_SECS: f32 = 180.0;
/// A partir de quanto dois títulos são "parecidos" (Jaccard das palavras).
const SAME_TITLE: f32 = 0.5;

/// Uma ação de reunião, antes de virar tarefa.
#[derive(Debug, Clone, PartialEq)]
pub struct MeetingAction {
    /// O que fazer.
    pub title: String,
    /// Segundos desde o começo da reunião, quando se sabe.
    pub at_secs: Option<f32>,
    /// O prazo como foi dito ("até sexta").
    pub due: Option<String>,
    /// A frase em que a ação aparece.
    pub quote: Option<String>,
    /// Validada por você no Copilot (e não achada pela IA no fim).
    pub from_copilot: bool,
}

/// A reunião de onde as ações vieram.
#[derive(Debug, Clone, PartialEq)]
pub struct MeetingRef {
    /// Id da reunião no banco.
    pub id: i64,
    /// Título da reunião.
    pub title: String,
    /// O dia em que ela aconteceu: "até sexta" é a sexta depois dele.
    pub day: NaiveDate,
}

/// Prefixo do `external_ref` das tarefas de uma reunião.
pub fn external_prefix(meeting_id: i64) -> String {
    format!("meeting:{meeting_id}:")
}

fn similar(a: &str, b: &str) -> bool {
    crate::text::similarity(a, b) >= SAME_TITLE
}

fn same(a: &MeetingAction, b: &MeetingAction) -> bool {
    let near = match (a.at_secs, b.at_secs) {
        (Some(x), Some(y)) => (x - y).abs() <= SAME_ACTION_SECS,
        _ => true,
    };
    near && similar(&a.title, &b.title)
}

/// As validadas no Copilot e as achadas pela IA, sem repetir, na ordem em
/// que foram ditas.
pub fn merge(copilot: Vec<MeetingAction>, extracted: Vec<MeetingAction>) -> Vec<MeetingAction> {
    let mut out = copilot;
    for a in extracted {
        if !out.iter().any(|kept| same(kept, &a)) {
            out.push(a);
        }
    }
    out.sort_by(|a, b| {
        a.at_secs
            .unwrap_or(f32::MAX)
            .total_cmp(&b.at_secs.unwrap_or(f32::MAX))
    });
    out
}

/// As tarefas da caixa de entrada, uma por ação. O prazo dito vira data
/// pelo parser (a partir do dia da reunião); o minuto e a frase vão na
/// origem, para a tela Hoje abrir a reunião no ponto certo.
pub fn drafts(meeting: &MeetingRef, actions: &[MeetingAction]) -> Vec<NewTask> {
    actions
        .iter()
        .enumerate()
        .map(|(n, a)| {
            let due_on = a
                .due
                .as_deref()
                .and_then(|phrase| when::resolve(phrase, meeting.day).date);
            NewTask {
                status: TaskStatus::Inbox,
                due_on,
                notes: a.quote.clone().unwrap_or_default(),
                source_kind: if a.from_copilot {
                    SourceKind::Copilot
                } else {
                    SourceKind::Meeting
                },
                source_ref: Some(json!({
                    "meeting_id": meeting.id,
                    "meeting_title": meeting.title,
                    "at_secs": a.at_secs,
                    "quote": a.quote,
                })),
                external_ref: Some(format!("{}{n}", external_prefix(meeting.id))),
                ..NewTask::titled(a.title.clone())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(title: &str, at: Option<f32>, copilot: bool) -> MeetingAction {
        MeetingAction {
            title: title.into(),
            at_secs: at,
            due: None,
            quote: None,
            from_copilot: copilot,
        }
    }

    #[test]
    fn a_validada_no_copilot_vence_a_repetida_e_a_ordem_e_a_da_fala() {
        let out = merge(
            vec![action(
                "Mandar a planilha de estimativas",
                Some(760.0),
                true,
            )],
            vec![
                action("Revisar o contrato da Marca", Some(1500.0), false),
                action(
                    "Mandar planilha de estimativas para a Ana",
                    Some(754.0),
                    false,
                ),
                action("Ligar pro João", Some(30.0), false),
            ],
        );
        let titles: Vec<&str> = out.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Ligar pro João",
                "Mandar a planilha de estimativas",
                "Revisar o contrato da Marca"
            ]
        );
        assert!(out[1].from_copilot);
    }

    #[test]
    fn mesmo_titulo_longe_no_tempo_sao_duas() {
        let out = merge(
            vec![action("Mandar o relatório", Some(60.0), true)],
            vec![action("Mandar o relatório", Some(3000.0), false)],
        );
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn rascunho_vai_para_a_caixa_com_o_minuto_e_o_prazo() {
        let meeting = MeetingRef {
            id: 42,
            title: "Daily do Portal".into(),
            // Quarta-feira.
            day: NaiveDate::from_ymd_opt(2026, 10, 7).unwrap(),
        };
        let mut a = action("Mandar a planilha", Some(754.6), false);
        a.due = Some("até sexta".into());
        a.quote = Some("Marcus, você me manda a planilha até sexta?".into());
        let b = action("Ver o deploy", None, true);
        let tasks = drafts(&meeting, &[a, b]);
        assert_eq!(tasks.len(), 2);
        let t = &tasks[0];
        assert_eq!(t.status, TaskStatus::Inbox);
        assert_eq!(t.source_kind, SourceKind::Meeting);
        assert_eq!(t.due_on, NaiveDate::from_ymd_opt(2026, 10, 9));
        assert_eq!(t.planned_on, None, "o dia é você que escolhe ao aceitar");
        assert_eq!(t.external_ref.as_deref(), Some("meeting:42:0"));
        let src = t.source_ref.as_ref().unwrap();
        assert_eq!(src["meeting_id"], 42);
        assert!((src["at_secs"].as_f64().unwrap() - 754.6).abs() < 0.01);
        assert_eq!(src["meeting_title"], "Daily do Portal");
        assert!(t.notes.contains("até sexta"));
        assert_eq!(tasks[1].source_kind, SourceKind::Copilot);
        assert!(tasks[1].source_ref.as_ref().unwrap()["at_secs"].is_null());
    }
}
