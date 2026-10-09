//! As sugestões de rotina (Fase 10.5): o [`crate::learn`] acha o padrão nas
//! tarefas das últimas 4 semanas; aqui ficam a leitura do banco e as duas
//! decisões da pessoa.
//!
//! - **Aceitar** cria a rotina com a evidência em `learned_from` (os dias e as
//!   tarefas que a mostraram) e registra a decisão no diário.
//! - **Recusar** só registra a decisão no diário: é de lá que o
//!   [`crate::learn::suggest`] sabe o que calar.
//!
//! As decisões ficam no diário (objeto `routine_suggestion`), que só cresce,
//! e não numa tabela à parte: o histórico é o próprio ajuste.

use chrono::{Duration, NaiveDate};
use rusqlite::params;
use serde_json::{Value, json};

use super::AssistStore;
use crate::learn::{Decision, RoutineSuggestion, Seen, suggest, window_start};
use crate::model::{Actor, NewRoutine, Routine};
use crate::{AssistError, Result};

const ACCEPTED: &str = "suggestion.accepted";
const DECLINED: &str = "suggestion.declined";
const OBJECT: &str = "routine_suggestion";

impl AssistStore {
    /// As sugestões de rotina para hoje (no máximo 3), com a evidência.
    pub fn routine_suggestions(&self) -> Result<Vec<RoutineSuggestion>> {
        let today = self.clock.today();
        let routines: Vec<String> = self.routines()?.into_iter().map(|r| r.title).collect();
        Ok(suggest(
            &self.seen_since(window_start(today), today)?,
            &routines,
            &self.suggestion_decisions()?,
            today,
        ))
    }

    /// Aceita a sugestão `key`: cria a rotina (ligada, pedindo o toque como
    /// qualquer outra) com a evidência em `learned_from`.
    pub fn accept_suggestion(&self, key: &str, actor: Actor) -> Result<Routine> {
        let s = self.find_suggestion(key)?;
        let learned = json!({
            "key": s.key,
            "hits": s.hits,
            "of": s.of,
            "days": s.days,
            "task_ids": s.task_ids,
        });
        let new = NewRoutine {
            title: s.title.clone(),
            rrule: s.rrule.clone(),
            verifier: None,
            action: None,
            mode: crate::model::RoutineMode::Ask,
        };
        let tx = self.conn.unchecked_transaction()?;
        let routine = self.insert_routine(&tx, new, Some(learned), actor)?;
        self.journal_row(
            &tx,
            routine.created_at,
            actor,
            ACCEPTED,
            OBJECT,
            &s.key,
            &s.title,
            &decision_data(&s, Some(&routine.id)),
        )?;
        tx.commit()?;
        Ok(routine)
    }

    /// Recusa a sugestão `key`: fica calada por 8 semanas e, depois, só volta
    /// com 4 de 4.
    pub fn decline_suggestion(&self, key: &str, actor: Actor) -> Result<()> {
        let s = self.find_suggestion(key)?;
        let at = self.clock.now_ms();
        self.journal_row(
            &self.conn,
            at,
            actor,
            DECLINED,
            OBJECT,
            &s.key,
            &s.title,
            &decision_data(&s, None),
        )?;
        Ok(())
    }

    fn find_suggestion(&self, key: &str) -> Result<RoutineSuggestion> {
        self.routine_suggestions()?
            .into_iter()
            .find(|s| s.key == key)
            .ok_or_else(|| {
                AssistError::NotFound(
                    "a sugestão (as tarefas mudaram desde que ela apareceu)".into(),
                )
            })
    }

    /// As tarefas que contam para o padrão entre `start` e `today`: abertas
    /// ou feitas, fora as de rotina; o dia é o planejado ou, sem ele, o da
    /// conclusão.
    fn seen_since(&self, start: NaiveDate, today: NaiveDate) -> Result<Vec<Seen>> {
        // A conclusão em ms: um dia a mais de folga, o fuso decide depois.
        let since_ms =
            self.clock.now_ms() - Duration::days((today - start).num_days() + 2).num_milliseconds();
        let mut stmt = self.conn.prepare(
            "SELECT id, title, planned_on, planned_time, completed_at FROM tasks
              WHERE routine_id IS NULL AND source_kind <> 'routine'
                AND status IN ('open', 'done')
                AND ((planned_on >= ?1 AND planned_on <= ?2)
                     OR (planned_on IS NULL AND completed_at >= ?3))",
        )?;
        let rows = stmt.query_map(
            params![start.to_string(), today.to_string(), since_ms],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                ))
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (task_id, title, planned_on, time, completed_at) = row?;
            let day = match planned_on {
                Some(d) => NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok(),
                None => completed_at.map(|ms| self.clock.day_of(ms)),
            };
            if let Some(day) = day.filter(|d| *d >= start && *d <= today) {
                out.push(Seen {
                    task_id,
                    title,
                    day,
                    time,
                });
            }
        }
        Ok(out)
    }

    /// As decisões anteriores, do diário, da mais antiga à mais nova.
    fn suggestion_decisions(&self) -> Result<Vec<Decision>> {
        let mut stmt = self.conn.prepare(
            "SELECT action, day, data FROM journal
              WHERE object_kind = ?1 AND action IN (?2, ?3) ORDER BY id",
        )?;
        let rows = stmt.query_map(params![OBJECT, ACCEPTED, DECLINED], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (action, day, data) = row?;
            let data: Value = data
                .map(|d| serde_json::from_str(&d))
                .transpose()?
                .unwrap_or(Value::Null);
            let Ok(day) = NaiveDate::parse_from_str(&day, "%Y-%m-%d") else {
                continue;
            };
            out.push(Decision {
                accepted: action == ACCEPTED,
                day,
                title: data["title"].as_str().unwrap_or_default().to_string(),
                weekdays: data["weekdays"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
        Ok(out)
    }
}

fn decision_data(s: &RoutineSuggestion, routine_id: Option<&str>) -> Value {
    json!({
        "title": s.title,
        "weekdays": s.weekdays,
        "rrule": s.rrule,
        "hits": s.hits,
        "of": s.of,
        "days": s.days,
        "routine_id": routine_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::model::{NewTask, TaskStatus};
    use crate::store::tests::store_at;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn at(path: &std::path::Path, when: &str) -> AssistStore {
        AssistStore::open_with_clock(path, Box::new(FixedClock::at(when, -3))).unwrap()
    }

    fn planned(store: &AssistStore, title: &str, on: &str, time: Option<&str>) -> String {
        store
            .create_task(
                NewTask {
                    planned_on: Some(day(on)),
                    planned_time: time.map(String::from),
                    ..NewTask::titled(title)
                },
                Actor::User,
            )
            .unwrap()
            .id
    }

    #[test]
    fn padrao_das_segundas_vira_sugestao_e_aceitar_cria_a_rotina() {
        // Quinta, 08/10/2026.
        let (store, path) = store_at(FixedClock::at("2026-10-08 10:00", -3));
        let a = planned(
            &store,
            "Revisar os PRs abertos",
            "2026-09-14",
            Some("09:00"),
        );
        planned(&store, "Revisar PRs", "2026-09-21", Some("09:00"));
        let dropped = planned(&store, "Revisar PRs", "2026-09-28", Some("09:00"));
        store
            .set_status(&dropped, TaskStatus::Dropped, Actor::User)
            .unwrap();
        store.set_status(&a, TaskStatus::Done, Actor::User).unwrap();
        assert!(
            store.routine_suggestions().unwrap().is_empty(),
            "a descartada não conta: 2 de 4"
        );
        // Sem dia planejado, conta o dia em que foi concluída (05/10, segunda).
        let monday = at(&path, "2026-10-05 09:05");
        let loose = monday
            .create_task(NewTask::titled("revisar os PRs"), Actor::User)
            .unwrap();
        monday
            .set_status(&loose.id, TaskStatus::Done, Actor::User)
            .unwrap();

        let store = at(&path, "2026-10-08 10:00");
        let s = store.routine_suggestions().unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].weekdays, ["MO"]);
        assert_eq!((s[0].hits, s[0].of), (3, 4));
        assert_eq!(s[0].days.last(), Some(&day("2026-10-05")));
        assert_eq!(
            s[0].rule.time, None,
            "a de 05/10 não tinha hora: 2 com hora não bastam"
        );

        let routine = store.accept_suggestion(&s[0].key, Actor::User).unwrap();
        assert_eq!(routine.title, "revisar os PRs");
        assert_eq!(routine.rrule, "FREQ=WEEKLY;BYDAY=MO");
        let learned = routine.learned_from.unwrap();
        assert_eq!(learned["hits"], 3);
        assert_eq!(learned["days"].as_array().unwrap().len(), 3);
        assert!(
            store.routine_suggestions().unwrap().is_empty(),
            "virou rotina"
        );
        assert!(matches!(
            store.accept_suggestion(&s[0].key, Actor::User),
            Err(AssistError::NotFound(_))
        ));
        let journal = store.journal_for_day(day("2026-10-08")).unwrap();
        assert!(journal.iter().any(|e| e.action == "routine.created"));
        assert!(
            journal
                .iter()
                .any(|e| e.action == ACCEPTED && e.object_id == s[0].key)
        );
    }

    #[test]
    fn recusar_cala_a_sugestao() {
        let (store, path) = store_at(FixedClock::at("2026-10-08 10:00", -3));
        for on in ["2026-09-17", "2026-09-24", "2026-10-01"] {
            planned(&store, "Atualizar o quadro do sprint", on, None);
        }
        let s = store.routine_suggestions().unwrap();
        assert_eq!(s[0].weekdays, ["TH"]);
        store.decline_suggestion(&s[0].key, Actor::User).unwrap();
        assert!(store.routine_suggestions().unwrap().is_empty());
        // Uma semana depois, com 4 de 4, ainda calada (8 semanas).
        let later = at(&path, "2026-10-15 10:00");
        planned(&later, "Atualizar o quadro do sprint", "2026-10-08", None);
        assert!(later.routine_suggestions().unwrap().is_empty());
        assert!(
            later.routines().unwrap().is_empty(),
            "recusar não cria nada"
        );
    }
}
