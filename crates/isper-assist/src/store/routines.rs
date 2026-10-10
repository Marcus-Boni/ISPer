//! Rotinas: o cadastro, a tarefa de cada dia (o checklist) e o registro do que
//! foi feito fora do ISPer em nome delas.
//!
//! A tarefa do dia nasce com `external_ref = routine:<id>:<dia>`, que é único
//! no banco: chamar [`AssistStore::materialize_routines`] de novo no mesmo dia
//! não cria outra, e uma tarefa descartada não volta. Dias passados não são
//! preenchidos para trás: com o PC desligado ontem, não aparece a de ontem.

use chrono::NaiveDate;
use rusqlite::{OptionalExtension, Row, params};
use serde_json::{Value, json};

use super::AssistStore;
use crate::model::{
    Actor, NewRoutine, NewTask, Occurrence, Routine, RoutineMode, RoutinePatch, SourceKind, Task,
};
use crate::recur::Rule;
use crate::{AssistError, Result};

const ROUTINE_COLUMNS: &str =
    "id, title, rrule, verifier, action, mode, active, learned_from, created_at, updated_at";

impl AssistStore {
    /// Todas as rotinas: as ativas primeiro, cada grupo na ordem de criação.
    pub fn routines(&self) -> Result<Vec<Routine>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {ROUTINE_COLUMNS} FROM routines ORDER BY active DESC, created_at, id"
        ))?;
        let rows = stmt.query_map([], routine_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row??);
        }
        Ok(out)
    }

    /// Uma rotina pelo id.
    pub fn routine(&self, id: &str) -> Result<Routine> {
        self.conn
            .query_row(
                &format!("SELECT {ROUTINE_COLUMNS} FROM routines WHERE id = ?1"),
                params![id],
                routine_from_row,
            )
            .optional()?
            .ok_or_else(|| AssistError::NotFound(format!("a rotina {id}")))?
    }

    /// Cria uma rotina (ligada).
    pub fn create_routine(&self, new: NewRoutine, actor: Actor) -> Result<Routine> {
        let tx = self.conn.unchecked_transaction()?;
        let routine = self.insert_routine(&tx, new, None, actor)?;
        tx.commit()?;
        Ok(routine)
    }

    /// Grava a rotina nova e a linha do diário na conexão (ou transação)
    /// `conn`; `learned_from` é a evidência, quando nasce de uma sugestão.
    pub(super) fn insert_routine(
        &self,
        conn: &rusqlite::Connection,
        new: NewRoutine,
        learned_from: Option<Value>,
        actor: Actor,
    ) -> Result<Routine> {
        let (new, rule) = new.normalized()?;
        let now = self.clock.now_ms();
        let routine = Routine {
            id: uuid::Uuid::now_v7().to_string(),
            title: new.title,
            rrule: new.rrule,
            rule,
            verifier: new.verifier,
            action: new.action,
            mode: new.mode,
            active: true,
            learned_from,
            created_at: now,
            updated_at: now,
        };
        conn.execute(
            &format!("INSERT INTO routines ({ROUTINE_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"),
            params![
                routine.id,
                routine.title,
                routine.rrule,
                routine.verifier,
                routine.action,
                routine.mode.as_str(),
                routine.active,
                routine.learned_from.as_ref().map(Value::to_string),
                routine.created_at,
                routine.updated_at,
            ],
        )?;
        self.journal_row(
            conn,
            now,
            actor,
            "routine.created",
            "routine",
            &routine.id,
            &routine.title,
            &json!({ "before": null, "after": routine }),
        )?;
        Ok(routine)
    }

    /// Muda título, recorrência, verificador, ação ou modo.
    pub fn update_routine(&self, id: &str, patch: RoutinePatch, actor: Actor) -> Result<Routine> {
        let before = self.routine(id)?;
        let mut after = patch.apply(&before)?;
        if after == before {
            return Ok(before);
        }
        after.updated_at = self.clock.now_ms();
        self.write_routine(&before, &after, actor, "routine.updated")?;
        Ok(after)
    }

    /// Pausa ou retoma. Pausada, a rotina não cria mais tarefas; a de hoje,
    /// se já existe, fica.
    pub fn set_routine_active(&self, id: &str, active: bool, actor: Actor) -> Result<Routine> {
        let before = self.routine(id)?;
        if before.active == active {
            return Ok(before);
        }
        let mut after = before.clone();
        after.active = active;
        after.updated_at = self.clock.now_ms();
        let action = if active {
            "routine.resumed"
        } else {
            "routine.paused"
        };
        self.write_routine(&before, &after, actor, action)?;
        Ok(after)
    }

    fn write_routine(
        &self,
        before: &Routine,
        after: &Routine,
        actor: Actor,
        action: &str,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE routines SET title = ?2, rrule = ?3, verifier = ?4, action = ?5, mode = ?6,
                    active = ?7, updated_at = ?8
              WHERE id = ?1",
            params![
                after.id,
                after.title,
                after.rrule,
                after.verifier,
                after.action,
                after.mode.as_str(),
                after.active,
                after.updated_at,
            ],
        )?;
        self.journal_row(
            &tx,
            after.updated_at,
            actor,
            action,
            "routine",
            &after.id,
            &after.title,
            &json!({ "before": before, "after": after }),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Cria as tarefas das rotinas ligadas que caem em `day` e ainda não
    /// têm a sua. Devolve só as criadas agora.
    pub fn materialize_routines(&self, day: NaiveDate) -> Result<Vec<Task>> {
        let mut created = Vec::new();
        for routine in self.routines()?.into_iter().filter(|r| r.active) {
            let born = self.clock.day_of(routine.created_at);
            if !routine.rule.occurs_on(day, born) {
                continue;
            }
            let external = format!("routine:{}:{day}", routine.id);
            let exists: Option<String> = self
                .conn
                .query_row(
                    "SELECT id FROM tasks WHERE external_ref = ?1",
                    params![external],
                    |r| r.get(0),
                )
                .optional()?;
            if exists.is_some() {
                continue;
            }
            let new = NewTask {
                planned_on: Some(day),
                planned_time: routine.rule.time_hhmm(),
                source_kind: SourceKind::Routine,
                source_ref: Some(json!({ "routine_id": routine.id, "day": day })),
                external_ref: Some(external),
                ..NewTask::titled(routine.title.clone())
            };
            created.push(self.insert_task(new, Some(routine.id.clone()), Actor::Routine, None)?);
        }
        Ok(created)
    }

    /// As tarefas de rotina abertas até `up_to` (inclusive), com a rotina de
    /// cada uma: o que o verificador tem para conferir.
    pub fn open_occurrences(&self, up_to: NaiveDate) -> Result<Vec<Occurrence>> {
        let tasks = self.tasks_where(
            "status = 'open' AND routine_id IS NOT NULL
               AND planned_on IS NOT NULL AND planned_on <= ?1
             ORDER BY planned_on, planned_time IS NULL, planned_time, position",
            params![up_to.to_string()],
        )?;
        let mut out = Vec::with_capacity(tasks.len());
        for task in tasks {
            let Some(routine_id) = task.routine_id.as_deref() else {
                continue;
            };
            let routine = match self.routine(routine_id) {
                Ok(r) => r,
                Err(AssistError::NotFound(_)) => continue,
                Err(e) => return Err(e),
            };
            let day = occurrence_day(&task).unwrap_or(up_to);
            out.push(Occurrence { task, routine, day });
        }
        Ok(out)
    }

    /// A tarefa de rotina `task_id`, com a rotina e o dia de origem (o que
    /// "Conferir agora" e as sugestões do dia usam).
    pub fn occurrence(&self, task_id: &str) -> Result<Occurrence> {
        let task = self.task(task_id)?;
        let routine_id = task
            .routine_id
            .clone()
            .ok_or_else(|| AssistError::Invalid("a tarefa não é de uma rotina".into()))?;
        let routine = self.routine(&routine_id)?;
        let day = occurrence_day(&task)
            .ok_or_else(|| AssistError::Invalid("tarefa de rotina sem dia".into()))?;
        Ok(Occurrence { task, routine, day })
    }

    /// Guarda o resultado da última conferência na tarefa da rotina (em
    /// `source_ref.check`), para a tela Hoje mostrar "faltam 2h" sem consultar
    /// de novo. Ler é livre e não vai para o diário; o que muda o estado da
    /// tarefa vai, por [`AssistStore::set_status`].
    pub fn set_occurrence_check(&self, task_id: &str, check: Value) -> Result<Task> {
        let mut task = self.task(task_id)?;
        if task.routine_id.is_none() {
            return Err(AssistError::Invalid("a tarefa não é de uma rotina".into()));
        }
        let mut source = match task.source_ref.take() {
            Some(Value::Object(map)) => Value::Object(map),
            Some(other) => json!({ "original": other }),
            None => json!({}),
        };
        source["check"] = check;
        self.conn.execute(
            "UPDATE tasks SET source_ref = ?2 WHERE id = ?1",
            params![task_id, source.to_string()],
        )?;
        task.source_ref = Some(source);
        Ok(task)
    }

    /// Registra no diário algo feito fora do ISPer (um lançamento no
    /// OptTime, por exemplo), com o que for preciso para conferir ou
    /// desfazer depois. Devolve o id da linha.
    pub fn record(
        &self,
        actor: Actor,
        action: &str,
        object_kind: &str,
        object_id: &str,
        summary: &str,
        data: &Value,
    ) -> Result<i64> {
        if object_kind == "task" || object_kind == "routine" {
            return Err(AssistError::Invalid(
                "tarefas e rotinas mudam pelos seus próprios métodos".into(),
            ));
        }
        let at = self.clock.now_ms();
        self.journal_row(
            &self.conn,
            at,
            actor,
            action,
            object_kind,
            object_id,
            summary,
            data,
        )?;
        Ok(self.conn.last_insert_rowid())
    }
}

/// O dia a que a tarefa da rotina se refere: o da origem, mesmo que a pessoa
/// tenha mudado a tarefa de dia depois.
fn occurrence_day(task: &Task) -> Option<NaiveDate> {
    task.source_ref
        .as_ref()
        .and_then(|s| s.get("day"))
        .and_then(Value::as_str)
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .or(task.planned_on)
}

fn routine_from_row(r: &Row<'_>) -> rusqlite::Result<Result<Routine>> {
    let rrule: String = r.get(2)?;
    let mode: String = r.get(5)?;
    let learned_from: Option<String> = r.get(7)?;
    let fields = (
        r.get::<_, String>(0)?,
        r.get::<_, String>(1)?,
        r.get::<_, Option<String>>(3)?,
        r.get::<_, Option<String>>(4)?,
        r.get::<_, bool>(6)?,
        r.get::<_, i64>(8)?,
        r.get::<_, i64>(9)?,
    );
    Ok((|| {
        let (id, title, verifier, action, active, created_at, updated_at) = fields;
        Ok(Routine {
            id,
            title,
            rule: Rule::parse(&rrule)?,
            rrule,
            verifier,
            action,
            mode: RoutineMode::parse(&mode)?,
            active,
            learned_from: learned_from.map(|s| serde_json::from_str(&s)).transpose()?,
            created_at,
            updated_at,
        })
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::model::{TaskStatus, VERIFY_OPTTIME_DAY};
    use crate::store::tests::store_at;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn reopen(path: &std::path::Path, at: &str) -> AssistStore {
        AssistStore::open_with_clock(path, Box::new(FixedClock::at(at, -3))).unwrap()
    }

    fn daily(title: &str) -> NewRoutine {
        NewRoutine {
            title: title.into(),
            rrule: "FREQ=DAILY".into(),
            verifier: None,
            action: None,
            mode: RoutineMode::Ask,
        }
    }

    #[test]
    fn criar_mudar_e_pausar_vao_para_o_diario() {
        let (store, _) = store_at(FixedClock::at("2026-10-07 09:00", -3));
        let r = store
            .create_routine(NewRoutine::opttime_hours("Registrar 8h"), Actor::User)
            .unwrap();
        assert_eq!(store.routine(&r.id).unwrap(), r);
        let r2 = store
            .update_routine(
                &r.id,
                RoutinePatch {
                    rrule: Some("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;BYHOUR=17;BYMINUTE=30".into()),
                    mode: Some(RoutineMode::Auto),
                    ..RoutinePatch::default()
                },
                Actor::User,
            )
            .unwrap();
        assert_eq!(r2.rule.time_hhmm().as_deref(), Some("17:30"));
        assert_eq!(r2.mode, RoutineMode::Auto);
        let pausada = store.set_routine_active(&r.id, false, Actor::User).unwrap();
        assert!(!pausada.active);
        assert_eq!(
            store.set_routine_active(&r.id, false, Actor::User).unwrap(),
            pausada,
            "pausar de novo não muda nada"
        );
        assert!(
            store
                .update_routine(
                    &r.id,
                    RoutinePatch {
                        verifier: Some(None),
                        ..RoutinePatch::default()
                    },
                    Actor::User,
                )
                .is_err(),
            "tirar o verificador de uma rotina automática é recusado"
        );

        let acoes: Vec<(String, String)> = store
            .journal_for_day(day("2026-10-07"))
            .unwrap()
            .into_iter()
            .map(|e| (e.object_kind, e.action))
            .collect();
        let esperado = [
            ("routine", "routine.created"),
            ("routine", "routine.updated"),
            ("routine", "routine.paused"),
        ]
        .map(|(k, a)| (k.to_string(), a.to_string()));
        assert_eq!(acoes, esperado);
    }

    #[test]
    fn o_checklist_do_dia_nasce_uma_vez_so() {
        let (store, path) = store_at(FixedClock::at("2026-10-07 08:00", -3));
        let horas = store
            .create_routine(NewRoutine::opttime_hours("Registrar 8h"), Actor::User)
            .unwrap();
        let agua = store
            .create_routine(daily("Beber água"), Actor::User)
            .unwrap();

        let hoje = store.materialize_routines(day("2026-10-07")).unwrap();
        assert_eq!(hoje.len(), 2);
        let t = hoje.iter().find(|t| t.title == "Registrar 8h").unwrap();
        assert_eq!(t.source_kind, SourceKind::Routine);
        assert_eq!(t.routine_id.as_deref(), Some(horas.id.as_str()));
        assert_eq!(t.planned_on, Some(day("2026-10-07")));
        assert_eq!(t.planned_time.as_deref(), Some("17:00"));
        assert_eq!(t.source_ref.as_ref().unwrap()["day"], "2026-10-07");
        assert!(
            store
                .materialize_routines(day("2026-10-07"))
                .unwrap()
                .is_empty(),
            "de novo no mesmo dia não cria nada"
        );

        // Descartar a de hoje não faz ela voltar.
        let a = hoje.iter().find(|t| t.title == "Beber água").unwrap();
        store
            .set_status(&a.id, TaskStatus::Dropped, Actor::User)
            .unwrap();
        assert!(
            store
                .materialize_routines(day("2026-10-07"))
                .unwrap()
                .is_empty()
        );

        // Sábado: só a diária.
        let store = reopen(&path, "2026-10-10 08:00");
        let sabado = store.materialize_routines(day("2026-10-10")).unwrap();
        assert_eq!(sabado.len(), 1);
        assert_eq!(sabado[0].routine_id.as_deref(), Some(agua.id.as_str()));

        // Pausada não cria.
        store
            .set_routine_active(&agua.id, false, Actor::User)
            .unwrap();
        assert!(
            store
                .materialize_routines(day("2026-10-11"))
                .unwrap()
                .is_empty()
        );

        let criadas: Vec<String> = store
            .journal_for_day(day("2026-10-07"))
            .unwrap()
            .into_iter()
            .filter(|e| e.action == "task.created")
            .map(|e| e.actor)
            .collect();
        assert_eq!(criadas, ["routine", "routine"], "quem cria é a rotina");
    }

    #[test]
    fn rotina_nao_cria_antes_de_nascer() {
        let (store, _) = store_at(FixedClock::at("2026-10-07 08:00", -3));
        store
            .create_routine(daily("Beber água"), Actor::User)
            .unwrap();
        assert!(
            store
                .materialize_routines(day("2026-10-06"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn abertas_ate_hoje_com_a_rotina_e_o_dia_de_origem() {
        let (store, path) = store_at(FixedClock::at("2026-10-07 08:00", -3));
        let horas = store
            .create_routine(NewRoutine::opttime_hours("Registrar 8h"), Actor::User)
            .unwrap();
        let ontem = store
            .materialize_routines(day("2026-10-07"))
            .unwrap()
            .remove(0);
        // A pessoa empurrou a de quarta para quinta de manhã.
        store
            .update_task(
                &ontem.id,
                crate::model::TaskPatch {
                    planned_time: Some(Some("09:00".into())),
                    planned_on: Some(Some(day("2026-10-08"))),
                    ..Default::default()
                },
                Actor::User,
            )
            .unwrap();
        let store = reopen(&path, "2026-10-08 18:00");
        let hoje = store
            .materialize_routines(day("2026-10-08"))
            .unwrap()
            .remove(0);

        let abertas = store.open_occurrences(day("2026-10-08")).unwrap();
        assert_eq!(abertas.len(), 2);
        assert_eq!(abertas[0].task.id, ontem.id, "a das 09:00 primeiro");
        assert_eq!(abertas[0].day, day("2026-10-07"), "confere o dia de origem");
        assert_eq!(abertas[1].task.id, hoje.id);
        assert_eq!(abertas[1].day, day("2026-10-08"));
        assert_eq!(abertas[1].routine.id, horas.id);
        assert_eq!(
            abertas[1].routine.verifier.as_deref(),
            Some(VERIFY_OPTTIME_DAY)
        );

        let uma = store.occurrence(&ontem.id).unwrap();
        assert_eq!(uma.day, day("2026-10-07"));
        assert_eq!(uma.routine.id, horas.id);
        let solta = store
            .create_task(NewTask::titled("x"), Actor::User)
            .unwrap();
        assert!(store.occurrence(&solta.id).is_err());

        store
            .set_status(&hoje.id, TaskStatus::Done, Actor::Routine)
            .unwrap();
        assert_eq!(store.open_occurrences(day("2026-10-08")).unwrap().len(), 1);
    }

    #[test]
    fn conferencia_fica_na_tarefa_sem_ir_para_o_diario() {
        let (store, _) = store_at(FixedClock::at("2026-10-07 17:00", -3));
        store
            .create_routine(NewRoutine::opttime_hours("Registrar 8h"), Actor::User)
            .unwrap();
        let t = store
            .materialize_routines(day("2026-10-07"))
            .unwrap()
            .remove(0);
        let antes = store.journal_for_day(day("2026-10-07")).unwrap().len();
        let check = json!({ "status": "missing", "remaining_minutes": 200 });
        let t2 = store.set_occurrence_check(&t.id, check.clone()).unwrap();
        assert_eq!(t2.source_ref.as_ref().unwrap()["check"], check);
        assert_eq!(
            t2.source_ref.as_ref().unwrap()["day"],
            "2026-10-07",
            "a origem fica"
        );
        assert_eq!(store.task(&t.id).unwrap(), t2);
        assert_eq!(
            store.journal_for_day(day("2026-10-07")).unwrap().len(),
            antes
        );

        let solta = store
            .create_task(NewTask::titled("x"), Actor::User)
            .unwrap();
        assert!(store.set_occurrence_check(&solta.id, json!({})).is_err());
    }

    #[test]
    fn registro_de_fora_vai_para_o_diario() {
        let (store, _) = store_at(FixedClock::at("2026-10-07 17:05", -3));
        let id = store
            .record(
                Actor::User,
                "opttime.applied",
                "opttime",
                "2026-10-07",
                "3 lançamentos, 5h20",
                &json!({ "idempotencyKey": "k", "createdEntryIds": ["a", "b", "c"] }),
            )
            .unwrap();
        let diario = store.journal_for_day(day("2026-10-07")).unwrap();
        assert_eq!(diario.len(), 1);
        assert_eq!(diario[0].id, id);
        assert_eq!(diario[0].object_kind, "opttime");
        assert_eq!(diario[0].data.as_ref().unwrap()["createdEntryIds"][2], "c");
        assert!(
            store
                .record(Actor::User, "task.created", "task", "x", "", &json!({}))
                .is_err()
        );
        assert!(
            store.undo(id, Actor::User).is_err(),
            "o desfazer do ISPer não alcança o OptTime"
        );
    }
}
