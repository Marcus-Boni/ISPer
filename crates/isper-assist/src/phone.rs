//! Tarefas no bolso (Fase 10.6): o celular guarda uma cópia das tarefas e
//! das rotinas, e o PC continua sendo a fonte.
//!
//! ```text
//!  celular                                    PC (isper.db)
//!  ───────                                    ─────────────
//!  muda offline ─► fila de PhoneOp (com a hora)
//!  sincronia   ──► TasksRequest { agora, ops } ─► aplica campo a campo
//!              ◄── TasksResponse { resultados, retrato } (abertas, caixa,
//!                                                feitas nos últimos 7 dias,
//!                                                rotinas)
//!  troca a cópia pelo retrato e tira da fila o que o PC respondeu
//! ```
//!
//! - **O último a escrever vence, por campo.** Cada mudança do celular leva a
//!   hora em que foi feita ([`PhoneOp::at_ms`], corrigida pela diferença de
//!   relógio entre os dois). O PC sabe quando cada campo de cada tarefa mudou
//!   pelo diário, que guarda o antes e o depois; se o campo mudou no PC
//!   depois, o PC vence e o celular fica sabendo ([`OpResult::kept_pc`]).
//! - **Criar não duplica.** A tarefa criada no celular entra com
//!   `external_ref = phone:<op_id>`: a mesma operação mandada de novo (a
//!   resposta se perdeu no caminho) devolve a mesma tarefa.
//! - **Sem apagar.** O retrato substitui a cópia: o que saiu dele (descartada
//!   ou feita há mais de 7 dias) some do celular, mas nunca do PC.
//!
//! O celular usa [`apply_local`] para mostrar as mudanças da fila antes de
//! elas chegarem ao PC.

use std::collections::{BTreeMap, HashMap};

use chrono::{Duration, NaiveDate};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Actor, NewTask, SourceKind, Task, TaskPatch, TaskStatus};
use crate::recur::Freq;
use crate::store::AssistStore;
use crate::{AssistError, Result};

/// Os campos que o celular pode mudar.
pub const FIELDS: [&str; 6] = [
    "title",
    "notes",
    "status",
    "planned_on",
    "planned_time",
    "due_on",
];

/// Prefixo do id de uma tarefa criada no celular que o PC ainda não viu.
pub const LOCAL_PREFIX: &str = "local:";
/// Prefixo do `external_ref` da tarefa criada no celular.
pub const EXTERNAL_PREFIX: &str = "phone:";
/// Quantos dias de tarefas feitas vão no retrato.
pub const DONE_DAYS: i64 = 7;

/// Uma mudança feita no celular.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhoneOp {
    /// UUID v7 da operação: identifica a resposta e, numa criação, a tarefa.
    pub op_id: String,
    /// Quando foi feita, no relógio do celular (ms UTC).
    pub at_ms: i64,
    /// A tarefa; `None` cria uma nova. Pode ser `local:<op_id>` de uma
    /// criação que ainda não voltou do PC.
    pub task_id: Option<String>,
    /// Campo → valor novo (texto, ou `null` para limpar).
    pub changes: BTreeMap<String, Value>,
    /// Ditada (a tarefa nova nasce com a origem "voz").
    #[serde(default)]
    pub voice: bool,
}

impl PhoneOp {
    /// Uma operação nova, com id e hora.
    pub fn new(task_id: Option<String>, changes: BTreeMap<String, Value>, at_ms: i64) -> Self {
        Self {
            op_id: uuid::Uuid::now_v7().to_string(),
            at_ms,
            task_id,
            changes,
            voice: false,
        }
    }
}

/// O que o PC fez com uma operação.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OpResult {
    pub op_id: String,
    /// A tarefa no PC (numa criação, o id que ela ganhou).
    pub task_id: Option<String>,
    /// Os campos aplicados.
    pub applied: Vec<String>,
    /// Os campos em que o PC venceu (mudaram lá depois).
    pub kept_pc: Vec<String>,
    /// Não deu para aplicar (o motivo, para o celular mostrar).
    pub error: Option<String>,
}

/// Uma tarefa como o celular a vê.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhoneTask {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    /// `inbox`, `open`, `done` ou `dropped`.
    pub status: String,
    pub planned_on: Option<NaiveDate>,
    pub planned_time: Option<String>,
    pub due_on: Option<NaiveDate>,
    /// A origem (`voice`, `meeting`, `routine`…).
    pub source: String,
    /// A rotina que a criou.
    pub routine_id: Option<String>,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}

impl From<&Task> for PhoneTask {
    fn from(t: &Task) -> Self {
        Self {
            id: t.id.clone(),
            title: t.title.clone(),
            notes: t.notes.clone(),
            status: t.status.as_str().to_string(),
            planned_on: t.planned_on,
            planned_time: t.planned_time.clone(),
            due_on: t.due_on,
            source: t.source_kind.as_str().to_string(),
            routine_id: t.routine_id.clone(),
            updated_at: t.updated_at,
            completed_at: t.completed_at,
        }
    }
}

/// Uma rotina como o celular a vê (só para ler, por enquanto).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhoneRoutine {
    pub id: String,
    pub title: String,
    pub rrule: String,
    /// `daily`, `weekly` ou `monthly`.
    pub freq: String,
    /// Os dias da semana (`MO`, `TU`…), quando há.
    pub weekdays: Vec<String>,
    /// A hora, `HH:MM`.
    pub time: Option<String>,
    pub active: bool,
}

/// O retrato que o PC manda: o que o celular mostra.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// O dia de hoje no PC.
    pub today: NaiveDate,
    /// As abertas, a caixa de entrada e as feitas nos últimos 7 dias.
    pub tasks: Vec<PhoneTask>,
    pub routines: Vec<PhoneRoutine>,
}

/// O pedido do celular.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TasksRequest {
    /// O relógio do celular agora (ms UTC): o PC corrige a hora das
    /// operações pela diferença.
    pub now_ms: i64,
    /// As mudanças, na ordem em que foram feitas.
    pub ops: Vec<PhoneOp>,
}

/// A resposta do PC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TasksResponse {
    /// O relógio do PC agora (ms UTC).
    pub now_ms: i64,
    /// Uma por operação, na mesma ordem.
    pub results: Vec<OpResult>,
    pub snapshot: Snapshot,
}

fn text(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn day(v: &Value) -> Result<Option<NaiveDate>> {
    match text(v) {
        None => Ok(None),
        Some(s) => NaiveDate::parse_from_str(&s, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| AssistError::Invalid(format!("dia inválido: {s}"))),
    }
}

/// Monta o pedido de mudança e o estado novo a partir dos campos.
fn patch_of(changes: &BTreeMap<String, Value>) -> Result<(TaskPatch, Option<TaskStatus>)> {
    let mut patch = TaskPatch::default();
    let mut status = None;
    for (field, value) in changes {
        match field.as_str() {
            "title" => patch.title = Some(text(value).unwrap_or_default()),
            "notes" => patch.notes = Some(value.as_str().unwrap_or_default().to_string()),
            "status" => {
                status = Some(TaskStatus::parse(
                    &text(value).unwrap_or_else(|| "open".into()),
                )?)
            }
            "planned_on" => patch.planned_on = Some(day(value)?),
            "planned_time" => patch.planned_time = Some(text(value)),
            "due_on" => patch.due_on = Some(day(value)?),
            other => {
                return Err(AssistError::Invalid(format!(
                    "campo que o celular não muda: {other}"
                )));
            }
        }
    }
    Ok((patch, status))
}

impl AssistStore {
    /// Aplica as mudanças do celular e devolve o retrato atualizado.
    pub fn handle_phone(&self, req: &TasksRequest) -> Result<TasksResponse> {
        let now = self.clock.now_ms();
        // O relógio do celular pode estar adiantado ou atrasado.
        let skew = now - req.now_ms;
        let results = req
            .ops
            .iter()
            .map(|op| {
                self.apply_phone_op(op, op.at_ms + skew)
                    .unwrap_or_else(|e| OpResult {
                        op_id: op.op_id.clone(),
                        task_id: op.task_id.clone(),
                        error: Some(e.to_string()),
                        ..OpResult::default()
                    })
            })
            .collect();
        // A tarefa de hoje das rotinas aparece mesmo com a tela do PC fechada.
        self.materialize_routines(self.clock.today())?;
        Ok(TasksResponse {
            now_ms: now,
            results,
            snapshot: self.phone_snapshot()?,
        })
    }

    /// O retrato: as abertas, a caixa de entrada, as feitas nos últimos 7
    /// dias e as rotinas.
    pub fn phone_snapshot(&self) -> Result<Snapshot> {
        let since = self.clock.now_ms() - Duration::days(DONE_DAYS).num_milliseconds();
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {} FROM tasks
              WHERE status IN ('open', 'inbox') OR (status = 'done' AND completed_at >= ?1)
              ORDER BY position, id",
            crate::store::TASK_COLUMNS
        ))?;
        let rows = stmt.query_map(params![since], crate::store::task_from_row)?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(PhoneTask::from(&row??));
        }
        let routines = self
            .routines()?
            .into_iter()
            .map(|r| PhoneRoutine {
                freq: match r.rule.freq {
                    Freq::Daily => "daily",
                    Freq::Weekly => "weekly",
                    Freq::Monthly => "monthly",
                }
                .into(),
                weekdays: r.rule.by_day.iter().map(ToString::to_string).collect(),
                time: r.rule.time_hhmm(),
                id: r.id,
                title: r.title,
                rrule: r.rrule,
                active: r.active,
            })
            .collect();
        Ok(Snapshot {
            today: self.clock.today(),
            tasks,
            routines,
        })
    }

    fn task_by_external(&self, external: &str) -> Result<Option<Task>> {
        let id: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM tasks WHERE external_ref = ?1",
                params![external],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| self.task(&id)).transpose()
    }

    /// Quando cada campo da tarefa mudou pela última vez, pelo diário: a hora
    /// da mudança de fato (`op_at`, a do celular) ou a da linha.
    fn field_times(&self, task_id: &str) -> Result<HashMap<String, i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT at, data FROM journal WHERE object_kind = 'task' AND object_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![task_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        let mut times = HashMap::new();
        for row in rows {
            let (at, data) = row?;
            let data: Value = match data {
                Some(d) => serde_json::from_str(&d)?,
                None => continue,
            };
            let at = data["op_at"].as_i64().unwrap_or(at);
            let (before, after) = (&data["before"], &data["after"]);
            for f in FIELDS {
                if before.is_null() || before[f] != after[f] {
                    times.insert(f.to_string(), at);
                }
            }
        }
        Ok(times)
    }

    fn apply_phone_op(&self, op: &PhoneOp, at: i64) -> Result<OpResult> {
        let mut result = OpResult {
            op_id: op.op_id.clone(),
            ..OpResult::default()
        };
        let (patch, status) = patch_of(&op.changes)?;
        let existing = match op.task_id.as_deref() {
            None => self.task_by_external(&format!("{EXTERNAL_PREFIX}{}", op.op_id))?,
            Some(id) => match id.strip_prefix(LOCAL_PREFIX) {
                Some(local) => Some(
                    self.task_by_external(&format!("{EXTERNAL_PREFIX}{local}"))?
                        .ok_or_else(|| {
                            AssistError::NotFound(format!("a tarefa criada no celular ({id})"))
                        })?,
                ),
                None => Some(self.task(id)?),
            },
        };
        let Some(before) = existing else {
            // Criação.
            let new = NewTask {
                planned_on: patch.planned_on.flatten(),
                planned_time: patch.planned_time.clone().flatten(),
                due_on: patch.due_on.flatten(),
                notes: patch.notes.clone().unwrap_or_default(),
                source_kind: if op.voice {
                    SourceKind::Voice
                } else {
                    SourceKind::Manual
                },
                source_ref: Some(serde_json::json!({"celular": true})),
                external_ref: Some(format!("{EXTERNAL_PREFIX}{}", op.op_id)),
                ..NewTask::titled(patch.title.clone().unwrap_or_default())
            };
            let mut task = self.create_task_at(new, Actor::Phone, at)?;
            if let Some(s) = status.filter(|s| *s != task.status) {
                task = self.phone_write(&task, TaskPatch::default(), Some(s), at)?;
            }
            result.task_id = Some(task.id);
            result.applied = op.changes.keys().cloned().collect();
            return Ok(result);
        };
        result.task_id = Some(before.id.clone());
        // Só os campos que não mudaram no PC depois.
        let times = self.field_times(&before.id)?;
        let mut kept = BTreeMap::new();
        for (field, value) in &op.changes {
            if times.get(field).is_some_and(|t| *t > at) {
                result.kept_pc.push(field.clone());
            } else {
                kept.insert(field.clone(), value.clone());
                result.applied.push(field.clone());
            }
        }
        let (patch, status) = patch_of(&kept)?;
        self.phone_write(&before, patch, status, at)?;
        Ok(result)
    }

    /// Grava a mudança do celular numa linha do diário, com a hora dela.
    fn phone_write(
        &self,
        before: &Task,
        patch: TaskPatch,
        status: Option<TaskStatus>,
        at: i64,
    ) -> Result<Task> {
        let mut after = patch.apply(before)?;
        let now = self.clock.now_ms();
        if let Some(s) = status.filter(|s| *s != before.status) {
            after.status = s;
            after.completed_at = (s == TaskStatus::Done).then_some(now);
        }
        if after == *before {
            return Ok(after);
        }
        after.updated_at = now;
        let action = match (before.status != after.status, after.status) {
            (true, TaskStatus::Done) => "task.completed",
            (true, TaskStatus::Dropped) => "task.dropped",
            (true, TaskStatus::Open) => "task.reopened",
            (true, TaskStatus::Inbox) => "task.to_inbox",
            (false, _) => "task.updated",
        };
        self.write_at(before, &after, Actor::Phone, action, None, Some(at))?;
        Ok(after)
    }
}

/// O que o celular mostra: o retrato com a fila aplicada por cima, na ordem.
/// Uma criação vira a tarefa `local:<op_id>` até o PC responder.
pub fn apply_local(snapshot: &Snapshot, ops: &[PhoneOp]) -> Vec<PhoneTask> {
    let mut tasks = snapshot.tasks.clone();
    for op in ops {
        let Ok((patch, status)) = patch_of(&op.changes) else {
            continue;
        };
        let at = op.at_ms;
        let target = match op.task_id.as_deref() {
            None => {
                tasks.push(PhoneTask {
                    id: format!("{LOCAL_PREFIX}{}", op.op_id),
                    title: String::new(),
                    notes: String::new(),
                    status: "open".into(),
                    planned_on: None,
                    planned_time: None,
                    due_on: None,
                    source: if op.voice { "voice" } else { "manual" }.into(),
                    routine_id: None,
                    updated_at: at,
                    completed_at: None,
                });
                tasks.len() - 1
            }
            Some(id) => match tasks.iter().position(|t| t.id == id) {
                Some(i) => i,
                None => continue,
            },
        };
        let t = &mut tasks[target];
        if let Some(v) = patch.title {
            t.title = v;
        }
        if let Some(v) = patch.notes {
            t.notes = v;
        }
        if let Some(v) = patch.planned_on {
            t.planned_on = v;
        }
        if let Some(v) = patch.planned_time {
            t.planned_time = v;
        }
        if let Some(v) = patch.due_on {
            t.due_on = v;
        }
        if let Some(s) = status {
            t.status = s.as_str().to_string();
            t.completed_at = (s == TaskStatus::Done).then_some(at);
        }
        t.updated_at = at;
    }
    tasks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::model::NewRoutine;
    use crate::store::tests::store_at;
    use serde_json::json;

    fn changes(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn op(task_id: Option<&str>, pairs: &[(&str, Value)], at: &str) -> PhoneOp {
        PhoneOp {
            op_id: uuid::Uuid::now_v7().to_string(),
            at_ms: FixedClock::at(at, -3).now_ms(),
            task_id: task_id.map(String::from),
            changes: changes(pairs),
            voice: false,
        }
    }

    fn reopen(path: &std::path::Path, at: &str) -> AssistStore {
        AssistStore::open_with_clock(path, Box::new(FixedClock::at(at, -3))).unwrap()
    }

    use crate::clock::Clock;

    #[test]
    fn criada_no_celular_entra_no_pc_uma_vez_so() {
        let (store, _) = store_at(FixedClock::at("2026-10-10 10:00", -3));
        let mut create = op(
            None,
            &[
                ("title", json!("Ligar pro contador")),
                ("planned_on", json!("2026-10-12")),
                ("planned_time", json!("15:00")),
            ],
            "2026-10-10 09:50",
        );
        create.voice = true;
        let req = TasksRequest {
            now_ms: FixedClock::at("2026-10-10 10:00", -3).now_ms(),
            ops: vec![create.clone()],
        };
        let resp = store.handle_phone(&req).unwrap();
        let id = resp.results[0].task_id.clone().unwrap();
        let t = store.task(&id).unwrap();
        assert_eq!(t.title, "Ligar pro contador");
        assert_eq!(t.planned_time.as_deref(), Some("15:00"));
        assert_eq!(t.source_kind, SourceKind::Voice);
        assert!(resp.snapshot.tasks.iter().any(|p| p.id == id));
        // A resposta se perdeu e o celular manda de novo: a mesma tarefa.
        let again = store.handle_phone(&req).unwrap();
        assert_eq!(again.results[0].task_id.as_deref(), Some(id.as_str()));
        assert_eq!(
            again
                .snapshot
                .tasks
                .iter()
                .filter(|p| p.title == "Ligar pro contador")
                .count(),
            1
        );
        let journal = store
            .journal_for_day(NaiveDate::from_ymd_opt(2026, 10, 10).unwrap())
            .unwrap();
        assert_eq!(journal[0].actor, "phone");
    }

    #[test]
    fn o_ultimo_a_escrever_vence_campo_a_campo() {
        let (store, path) = store_at(FixedClock::at("2026-10-10 09:00", -3));
        let t = store
            .create_task(NewTask::titled("Revisar o contrato"), Actor::User)
            .unwrap();
        // 10:00 no PC: muda o título.
        let pc = reopen(&path, "2026-10-10 10:00");
        pc.update_task(
            &t.id,
            TaskPatch {
                title: Some("Revisar o contrato da Marca".into()),
                ..TaskPatch::default()
            },
            Actor::User,
        )
        .unwrap();
        // O celular, offline às 09:30, tinha mudado o título e o dia; chega às 11:00.
        let late = reopen(&path, "2026-10-10 11:00");
        let resp = late
            .handle_phone(&TasksRequest {
                now_ms: FixedClock::at("2026-10-10 11:00", -3).now_ms(),
                ops: vec![op(
                    Some(&t.id),
                    &[
                        ("title", json!("Revisar contrato")),
                        ("planned_on", json!("2026-10-13")),
                    ],
                    "2026-10-10 09:30",
                )],
            })
            .unwrap();
        assert_eq!(resp.results[0].kept_pc, ["title"]);
        assert_eq!(resp.results[0].applied, ["planned_on"]);
        let now = late.task(&t.id).unwrap();
        assert_eq!(
            now.title, "Revisar o contrato da Marca",
            "o PC mudou depois"
        );
        assert_eq!(now.planned_on, NaiveDate::from_ymd_opt(2026, 10, 13));

        // Uma mudança do celular depois da do PC vence; e a hora vem do
        // celular, não da chegada: uma segunda mudança dele, mais nova, também.
        let resp = late
            .handle_phone(&TasksRequest {
                now_ms: FixedClock::at("2026-10-10 11:00", -3).now_ms(),
                ops: vec![
                    op(Some(&t.id), &[("title", json!("A"))], "2026-10-10 10:30"),
                    op(Some(&t.id), &[("title", json!("B"))], "2026-10-10 10:40"),
                    op(
                        Some(&t.id),
                        &[("status", json!("done"))],
                        "2026-10-10 10:45",
                    ),
                ],
            })
            .unwrap();
        assert!(resp.results.iter().all(|r| r.kept_pc.is_empty()));
        let now = late.task(&t.id).unwrap();
        assert_eq!(now.title, "B");
        assert_eq!(now.status, TaskStatus::Done);
    }

    #[test]
    fn relogio_do_celular_adiantado_e_corrigido() {
        let (store, path) = store_at(FixedClock::at("2026-10-10 09:00", -3));
        let t = store
            .create_task(NewTask::titled("Pagar o boleto"), Actor::User)
            .unwrap();
        let pc = reopen(&path, "2026-10-10 10:00");
        pc.update_task(
            &t.id,
            TaskPatch {
                title: Some("Pagar o boleto da luz".into()),
                ..TaskPatch::default()
            },
            Actor::User,
        )
        .unwrap();
        // O celular está 1 h adiantado: diz 10:30 para o que fez às 09:30.
        let resp = pc
            .handle_phone(&TasksRequest {
                now_ms: FixedClock::at("2026-10-10 11:00", -3).now_ms(),
                ops: vec![op(
                    Some(&t.id),
                    &[("title", json!("Pagar"))],
                    "2026-10-10 10:30",
                )],
            })
            .unwrap();
        assert_eq!(
            resp.results[0].kept_pc,
            ["title"],
            "corrigida, a mudança é das 09:30"
        );
    }

    #[test]
    fn editar_a_criada_offline_antes_de_o_pc_ver() {
        let (store, _) = store_at(FixedClock::at("2026-10-10 10:00", -3));
        let create = op(
            None,
            &[("title", json!("Comprar café"))],
            "2026-10-10 09:00",
        );
        let local = format!("{LOCAL_PREFIX}{}", create.op_id);
        let done = op(
            Some(&local),
            &[("status", json!("done"))],
            "2026-10-10 09:10",
        );
        let resp = store
            .handle_phone(&TasksRequest {
                now_ms: FixedClock::at("2026-10-10 10:00", -3).now_ms(),
                ops: vec![create, done],
            })
            .unwrap();
        let id = resp.results[0].task_id.clone().unwrap();
        assert_eq!(resp.results[1].task_id.as_deref(), Some(id.as_str()));
        assert_eq!(store.task(&id).unwrap().status, TaskStatus::Done);
    }

    #[test]
    fn retrato_tem_rotinas_e_deixa_de_fora_o_descartado_e_o_antigo() {
        let (store, path) = store_at(FixedClock::at("2026-10-01 09:00", -3));
        let old = store
            .create_task(NewTask::titled("Feita há tempo"), Actor::User)
            .unwrap();
        store
            .set_status(&old.id, TaskStatus::Done, Actor::User)
            .unwrap();
        let store = reopen(&path, "2026-10-10 09:00");
        let dropped = store
            .create_task(NewTask::titled("Descartada"), Actor::User)
            .unwrap();
        store
            .set_status(&dropped.id, TaskStatus::Dropped, Actor::User)
            .unwrap();
        store
            .create_routine(
                NewRoutine {
                    title: "Revisar os PRs".into(),
                    rrule: "FREQ=WEEKLY;BYDAY=SA;BYHOUR=9".into(),
                    verifier: None,
                    action: None,
                    mode: crate::model::RoutineMode::Ask,
                },
                Actor::User,
            )
            .unwrap();
        let resp = store
            .handle_phone(&TasksRequest {
                now_ms: store.clock().now_ms(),
                ops: Vec::new(),
            })
            .unwrap();
        let titles: Vec<&str> = resp
            .snapshot
            .tasks
            .iter()
            .map(|t| t.title.as_str())
            .collect();
        assert_eq!(
            titles,
            ["Revisar os PRs"],
            "a de hoje da rotina, criada na hora"
        );
        let r = &resp.snapshot.routines[0];
        assert_eq!(
            (r.freq.as_str(), r.time.as_deref()),
            ("weekly", Some("09:00"))
        );
        assert_eq!(r.weekdays, ["SA"]);
    }

    #[test]
    fn erro_numa_operacao_nao_derruba_as_outras() {
        let (store, _) = store_at(FixedClock::at("2026-10-10 10:00", -3));
        let resp = store
            .handle_phone(&TasksRequest {
                now_ms: FixedClock::at("2026-10-10 10:00", -3).now_ms(),
                ops: vec![
                    op(
                        Some("nao-existe"),
                        &[("title", json!("x"))],
                        "2026-10-10 09:00",
                    ),
                    op(None, &[("title", json!("Vale"))], "2026-10-10 09:01"),
                    op(None, &[("cor", json!("azul"))], "2026-10-10 09:02"),
                ],
            })
            .unwrap();
        assert!(resp.results[0].error.is_some());
        assert!(resp.results[1].error.is_none());
        assert!(resp.results[2].error.as_deref().unwrap().contains("cor"));
    }

    #[test]
    fn a_fila_aparece_no_celular_antes_de_chegar_ao_pc() {
        let snapshot = Snapshot {
            today: NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(),
            tasks: vec![PhoneTask {
                id: "t1".into(),
                title: "Revisar".into(),
                notes: String::new(),
                status: "open".into(),
                planned_on: None,
                planned_time: None,
                due_on: None,
                source: "manual".into(),
                routine_id: None,
                updated_at: 0,
                completed_at: None,
            }],
            routines: Vec::new(),
        };
        let create = op(
            None,
            &[
                ("title", json!("Nova")),
                ("planned_on", json!("2026-10-10")),
            ],
            "2026-10-10 09:00",
        );
        let local = format!("{LOCAL_PREFIX}{}", create.op_id);
        let ops = vec![
            create,
            op(Some("t1"), &[("status", json!("done"))], "2026-10-10 09:05"),
            op(
                Some(&local),
                &[("planned_time", json!("15:00"))],
                "2026-10-10 09:06",
            ),
            op(Some("sumiu"), &[("title", json!("x"))], "2026-10-10 09:07"),
        ];
        let view = apply_local(&snapshot, &ops);
        assert_eq!(view.len(), 2);
        assert_eq!(view[0].status, "done");
        assert!(view[0].completed_at.is_some());
        assert_eq!(view[1].id, local);
        assert_eq!(view[1].planned_time.as_deref(), Some("15:00"));
    }
}
