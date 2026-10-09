//! As ações de "Eu" de uma reunião gravada vão para a caixa de entrada da
//! tela Hoje quando ela acaba (Fase 10.3), com o minuto em que foram ditas.
//!
//! Juntam-se as que você validou no Copilot com dono "Eu" e as que a IA
//! configurada acha na transcrição (`isper_llm::meeting_actions`), sem
//! repetir; quem guarda e impede a segunda vez é o `isper-assist`. Sem IA,
//! vão só as do Copilot. A transcrição só sai da máquina para o provider que
//! já faz o resumo, como texto.

use crate::prelude::*;
use crate::today::{TASKS_EVENT, open_assist};
use chrono::NaiveDate;
use isper_assist::meeting::{MeetingAction, MeetingRef, drafts, merge};
use isper_core::store::StoredDecision;
use isper_llm::{TimedLine, extract_my_actions};

/// O dono que o Copilot escreve para quem gravou.
fn is_me(owner: &str) -> bool {
    matches!(owner.trim().to_lowercase().as_str(), "eu" | "me" | "i")
}

/// As validadas no Copilot que são de "Eu".
pub(crate) fn copilot_actions(decisions: &[StoredDecision]) -> Vec<MeetingAction> {
    decisions
        .iter()
        .filter(|d| d.kind == "action" && d.owner.as_deref().is_some_and(is_me))
        .map(|d| MeetingAction {
            title: d.title.clone(),
            at_secs: Some(d.at_secs),
            due: d.due_date.clone(),
            quote: None,
            from_copilot: true,
        })
        .collect()
}

/// O dia de uma reunião a partir do `dd/mm/aaaa hh:mm` guardado.
pub(crate) fn meeting_day(started_at: &str) -> NaiveDate {
    started_at
        .split_whitespace()
        .next()
        .and_then(|d| NaiveDate::parse_from_str(d, "%d/%m/%Y").ok())
        .unwrap_or_else(|| chrono::Local::now().date_naive())
}

/// Manda as ações de "Eu" para a caixa de entrada e devolve quantas
/// entraram (0 se a reunião já tinha mandado, se a opção está desligada ou
/// se não havia nenhuma).
pub(crate) fn collect(
    app: &AppHandle,
    meeting_id: i64,
    title: &str,
    day: NaiveDate,
    lines: &[TimedLine],
    decisions: &[StoredDecision],
) -> usize {
    let on = app
        .state::<AppState>()
        .config
        .lock_or_recover()
        .meeting_inbox;
    if !on {
        return 0;
    }
    let extracted: Vec<MeetingAction> =
        match isper_llm::provider_from_settings(&isper_llm::load_settings()) {
            Ok(provider) => {
                let me = crate::connectors::me_first_name();
                match extract_my_actions(provider.as_ref(), lines, me.as_deref()) {
                    Ok(found) => found
                        .into_iter()
                        .map(|a| MeetingAction {
                            title: a.title,
                            at_secs: a.at_secs,
                            due: a.due,
                            quote: a.quote,
                            from_copilot: false,
                        })
                        .collect(),
                    Err(e) => {
                        tracing::warn!(meeting_id, "ações da reunião sem a IA: {e}");
                        Vec::new()
                    }
                }
            }
            Err(isper_llm::LlmError::NotConfigured) => Vec::new(),
            Err(e) => {
                tracing::warn!(meeting_id, "ações da reunião sem a IA: {e}");
                Vec::new()
            }
        };
    let actions = merge(copilot_actions(decisions), extracted);
    if actions.is_empty() {
        return 0;
    }
    let meeting = MeetingRef {
        id: meeting_id,
        title: title.to_string(),
        day,
    };
    let created = open_assist().and_then(|store| {
        Ok(store.import_meeting_actions(meeting_id, drafts(&meeting, &actions))?)
    });
    match created {
        Ok(tasks) => {
            if !tasks.is_empty() {
                tracing::info!(
                    meeting_id,
                    n = tasks.len(),
                    "ações de \"Eu\" na caixa de entrada"
                );
                let _ = app.emit(TASKS_EVENT, ());
            }
            tasks.len()
        }
        Err(e) => {
            tracing::warn!(meeting_id, "não consegui mandar as ações para a caixa: {e}");
            0
        }
    }
}

/// As falas de uma reunião ao vivo, no formato da extração.
pub(crate) fn lines_of(result: &isper_core::meeting::MeetingResult) -> Vec<TimedLine> {
    result
        .segments
        .iter()
        .map(|s| TimedLine {
            at_secs: s.start_secs,
            speaker: s.speaker.label(),
            text: s.text.clone(),
        })
        .collect()
}

/// As falas de uma reunião do banco, no formato da extração.
pub(crate) fn lines_from_rows(rows: &[(String, f32, f32, String)]) -> Vec<TimedLine> {
    rows.iter()
        .map(|(speaker, start, _, text)| TimedLine {
            at_secs: *start,
            speaker: speaker.clone(),
            text: text.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(kind: &str, owner: Option<&str>) -> StoredDecision {
        StoredDecision {
            kind: kind.into(),
            title: format!("{kind} de {owner:?}"),
            description: String::new(),
            owner: owner.map(String::from),
            due_date: Some("sexta".into()),
            urgency: "medium".into(),
            at_secs: 120.0,
        }
    }

    #[test]
    fn so_acoes_de_eu_vem_do_copilot() {
        let out = copilot_actions(&[
            decision("action", Some("Eu")),
            decision("action", Some("Carlos")),
            decision("decision", Some("Eu")),
            decision("action", None),
            decision("action", Some(" eu ")),
        ]);
        assert_eq!(out.len(), 2);
        assert!(
            out.iter()
                .all(|a| a.from_copilot && a.at_secs == Some(120.0))
        );
        assert_eq!(out[0].due.as_deref(), Some("sexta"));
    }

    #[test]
    fn dia_da_reuniao_pelo_texto_guardado() {
        assert_eq!(
            meeting_day("08/10/2026 14:36"),
            NaiveDate::from_ymd_opt(2026, 10, 8).unwrap()
        );
    }
}
