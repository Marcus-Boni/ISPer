//! O tempo livre (Fase 11.0, [ADR 0025]): as janelas da agenda em que cabe um
//! item "Quando der tempo", e qual item sugerir.
//!
//! - **As janelas** vão de agora ao fim do expediente, entre os compromissos
//!   (sobrepostos ou colados contam como um só). Menos de
//!   [`MIN_WINDOW_MIN`] minutos não é tempo livre.
//! - **O item** é o que cabe na janela (a estimativa; sem ela, conta
//!   [`DEFAULT_ESTIMATE_MIN`]) e espera há mais tempo (o `updated_at` mais
//!   antigo). "Outra" pula o sugerido.
//!
//! Sugestão, nunca mudança: quem decide é a pessoa ([ADR 0023]). Quais
//! eventos ocupam a agenda (aceitos, não "livre") é de quem chama.
//!
//! [ADR 0023]: ../../../docs/adr/0023-escada-de-confianca.md
//! [ADR 0025]: ../../../docs/adr/0025-quando-der-tempo-e-projetos-locais.md

use chrono::{DateTime, FixedOffset, NaiveTime, TimeZone};
use serde::Serialize;

use crate::model::Task;

/// Menos que isso não é tempo livre.
pub const MIN_WINDOW_MIN: i64 = 15;
/// O item sem estimativa conta como se levasse isso.
pub const DEFAULT_ESTIMATE_MIN: u16 = 30;

/// Uma janela livre na agenda.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FreeWindow {
    pub start: DateTime<FixedOffset>,
    pub end: DateTime<FixedOffset>,
    /// Quantos minutos ela tem.
    pub minutes: i64,
}

/// As janelas livres de `now` até `day_end` (no fuso de `now`), entre os
/// compromissos `busy` (início e fim). Vazio depois do fim do expediente.
pub fn free_windows(
    busy: &[(DateTime<FixedOffset>, DateTime<FixedOffset>)],
    now: DateTime<FixedOffset>,
    day_end: NaiveTime,
) -> Vec<FreeWindow> {
    let Some(end) = now
        .offset()
        .from_local_datetime(&now.date_naive().and_time(day_end))
        .single()
    else {
        return Vec::new();
    };
    if now >= end {
        return Vec::new();
    }
    let mut slots: Vec<_> = busy
        .iter()
        .filter(|(s, e)| e > s && *e > now && *s < end)
        .copied()
        .collect();
    slots.sort();
    let mut out = Vec::new();
    let mut cursor = now;
    let mut push = |from: DateTime<FixedOffset>, to: DateTime<FixedOffset>| {
        let minutes = (to - from).num_minutes();
        if minutes >= MIN_WINDOW_MIN {
            out.push(FreeWindow {
                start: from,
                end: to,
                minutes,
            });
        }
    };
    for (s, e) in slots {
        if s > cursor {
            push(cursor, s.min(end));
        }
        cursor = cursor.max(e);
        if cursor >= end {
            return out;
        }
    }
    push(cursor, end);
    out
}

/// Quanto o item leva, para caber numa janela.
pub fn estimate(task: &Task) -> u16 {
    task.estimate_min.unwrap_or(DEFAULT_ESTIMATE_MIN)
}

/// O item para uma janela de `minutes`: entre os que cabem, o que espera há
/// mais tempo; `skip` são os já recusados ("Outra").
pub fn pick<'a>(items: &'a [Task], minutes: i64, skip: &[String]) -> Option<&'a Task> {
    items
        .iter()
        .filter(|t| i64::from(estimate(t)) <= minutes && !skip.contains(&t.id))
        .min_by_key(|t| (t.updated_at, t.position.to_bits()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NewTask, SourceKind, TaskStatus};

    fn t(hhmm: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(&format!("2026-10-12T{hhmm}:00-03:00")).unwrap()
    }

    fn item(id: &str, estimate: Option<u16>, updated_at: i64) -> Task {
        let n = NewTask::titled(id);
        Task {
            id: id.into(),
            title: n.title,
            notes: String::new(),
            status: TaskStatus::Open,
            planned_on: None,
            planned_time: None,
            due_on: None,
            priority: 0,
            area: None,
            source_kind: SourceKind::Manual,
            source_ref: None,
            external_ref: None,
            routine_id: None,
            position: 0.0,
            created_at: 0,
            updated_at,
            completed_at: None,
            list_id: Some("l".into()),
            estimate_min: estimate,
            project_id: None,
        }
    }

    #[test]
    fn janelas_entre_compromissos_ate_o_fim_do_expediente() {
        let busy = [
            (t("09:30"), t("10:15")), // começou antes de agora
            (t("10:45"), t("11:00")),
            (t("11:00"), t("12:00")), // colado no anterior
            (t("14:00"), t("15:30")),
            (t("13:50"), t("14:10")), // sobreposto
            (t("12:20"), t("12:30")), // deixa 20 min antes (cabe) e 80 depois
        ];
        let w = free_windows(
            &busy,
            t("10:00"),
            NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
        );
        let got: Vec<(String, i64)> = w
            .iter()
            .map(|w| (w.start.format("%H:%M").to_string(), w.minutes))
            .collect();
        assert_eq!(
            got,
            [
                ("10:15".to_string(), 30),
                ("12:00".to_string(), 20),
                ("12:30".to_string(), 80),
                ("15:30".to_string(), 150)
            ]
        );
    }

    #[test]
    fn janela_curta_nao_conta_e_depois_do_expediente_nada() {
        let end = NaiveTime::from_hms_opt(18, 0, 0).unwrap();
        let w = free_windows(&[(t("10:10"), t("17:55"))], t("10:00"), end);
        assert!(w.is_empty(), "10 e 5 minutos não são tempo livre: {w:?}");
        assert!(free_windows(&[], t("18:30"), end).is_empty());
        let livre = free_windows(&[], t("17:00"), end);
        assert_eq!(livre[0].minutes, 60);
    }

    #[test]
    fn sugere_o_que_cabe_e_espera_ha_mais_tempo() {
        let items = [
            item("artigo", Some(120), 1),
            item("crdt", Some(60), 5),
            item("rust", None, 3), // sem estimativa: 30 min
            item("video", Some(15), 9),
        ];
        assert_eq!(pick(&items, 60, &[]).unwrap().id, "rust");
        assert_eq!(pick(&items, 20, &[]).unwrap().id, "video");
        assert_eq!(pick(&items, 150, &[]).unwrap().id, "artigo");
        assert_eq!(pick(&items, 60, &["rust".into()]).unwrap().id, "crdt");
        assert!(pick(&items, 10, &[]).is_none());
    }
}
