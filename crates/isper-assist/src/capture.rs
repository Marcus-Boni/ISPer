//! Da fala extraída ao rascunho de tarefa (Fase 10.1).
//!
//! A LLM devolve o título e os trechos falados de quando e de prazo; aqui eles
//! viram datas pelo parser ([`crate::when`]). O título também passa pelo
//! parser: se a LLM deixou "amanhã" dentro dele, a data sai do título e vale.

use chrono::NaiveDate;

use crate::model::{NewTask, SourceKind, TaskStatus};
use crate::when::{parse_task, resolve};

/// Um rascunho de tarefa a partir do que a LLM separou da fala.
///
/// O trecho de quando ganha do que estiver no título; um "quando" que é na
/// verdade prazo ("até sexta") vai para o prazo. Hora sem dia é hoje.
pub fn draft(
    title: &str,
    when: Option<&str>,
    due: Option<&str>,
    notes: Option<&str>,
    today: NaiveDate,
) -> NewTask {
    let parsed = parse_task(title, today);
    let when = when.map(|w| resolve(w, today)).unwrap_or_default();
    let due_when = due.map(|d| resolve(d, today)).unwrap_or_default();

    let (planned_from_when, due_from_when) = if when.due {
        (None, when.date)
    } else {
        (when.date, None)
    };
    let planned_time = when
        .time
        .map(|t| t.format("%H:%M").to_string())
        .or(parsed.planned_time);
    let planned_on = planned_from_when
        .or(parsed.planned_on)
        .or_else(|| planned_time.as_ref().map(|_| today));
    let due_on = due_when.date.or(due_from_when).or(parsed.due_on);

    NewTask {
        notes: notes.map(str::trim).unwrap_or_default().to_string(),
        status: TaskStatus::Open,
        planned_on,
        planned_time,
        due_on,
        source_kind: SourceKind::Voice,
        ..NewTask::titled(parsed.title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quarta() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 7).unwrap()
    }

    fn d(s: &str) -> Option<NaiveDate> {
        Some(NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap())
    }

    #[test]
    fn trecho_de_quando_vira_dia_e_hora() {
        let t = draft("Ligar pro João", Some("amanhã às 3"), None, None, quarta());
        assert_eq!(t.title, "Ligar pro João");
        assert_eq!(t.planned_on, d("2026-10-08"));
        assert_eq!(t.planned_time.as_deref(), Some("15:00"));
        assert_eq!(t.source_kind, SourceKind::Voice);
    }

    #[test]
    fn prazo_vai_para_o_prazo_mesmo_vindo_no_quando() {
        let t = draft("Pagar o boleto", Some("até sexta"), None, None, quarta());
        assert_eq!(t.planned_on, None);
        assert_eq!(t.due_on, d("2026-10-09"));
        let t = draft("Pagar o boleto", None, Some("até sexta"), None, quarta());
        assert_eq!(t.due_on, d("2026-10-09"));
    }

    #[test]
    fn data_esquecida_no_titulo_sai_dele() {
        let t = draft("Revisar o contrato amanhã", None, None, None, quarta());
        assert_eq!(t.title, "Revisar o contrato");
        assert_eq!(t.planned_on, d("2026-10-08"));
    }

    #[test]
    fn o_trecho_ganha_do_titulo() {
        let t = draft("Revisar amanhã", Some("sexta"), None, None, quarta());
        assert_eq!(t.planned_on, d("2026-10-09"));
        assert_eq!(t.title, "Revisar");
    }

    #[test]
    fn sem_quando_fica_sem_dia_e_notas_vao_junto() {
        let t = draft("Ler o ADR", None, None, Some(" o 0021 "), quarta());
        assert_eq!(t.planned_on, None);
        assert_eq!(t.notes, "o 0021");
    }

    #[test]
    fn so_hora_e_hoje() {
        let t = draft("Daily", Some("às 9 e meia"), None, None, quarta());
        assert_eq!(t.planned_on, d("2026-10-07"));
        assert_eq!(t.planned_time.as_deref(), Some("09:30"));
    }
}
