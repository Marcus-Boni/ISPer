//! O [`AssistStore`]: tarefas, rotinas e diário sobre o `isper.db`.
//!
//! Cada mudança numa tarefa acontece numa transação junto com a linha do
//! diário que a descreve, com o "antes" e o "depois". O desfazer
//! ([`AssistStore::undo`]) põe o "antes" de volta, e só vale para a mudança
//! mais recente da tarefa: desfazer algo que já foi mudado de novo apagaria a
//! mudança de depois sem ninguém pedir.

use std::path::Path;
use std::time::Duration;

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::json;

use crate::clock::{Clock, SystemClock};
use crate::model::{Actor, JournalEntry, NewTask, SourceKind, Task, TaskPatch, TaskStatus, Today};
use crate::{AssistError, Result};

mod routines;

/// Versão do schema do `isper.db` que trouxe as tabelas do assistente.
pub const SCHEMA_VERSION_REQUIRED: i64 = 7;

const TASK_COLUMNS: &str = "id, title, notes, status, planned_on, planned_time, due_on, priority, \
     area, source_kind, source_ref, external_ref, routine_id, position, created_at, updated_at, \
     completed_at";

/// Como a importação de uma tarefa terminou.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportOutcome {
    /// Tarefa nova.
    Created(Box<Task>),
    /// Já tinha sido importada antes (mesmo `external_ref`): nada mudou.
    AlreadyThere(String),
}

/// O domínio do assistente sobre uma conexão própria ao `isper.db`.
pub struct AssistStore {
    conn: Connection,
    clock: Box<dyn Clock>,
}

impl std::fmt::Debug for AssistStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssistStore").finish_non_exhaustive()
    }
}

impl AssistStore {
    /// Abre o banco já migrado pelo `isper-core`, com o relógio do sistema.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_clock(path, Box::new(SystemClock))
    }

    /// Abre com um relógio próprio (os testes usam um parado).
    ///
    /// WAL e `busy_timeout`, como o `MeetingStore`: o app tem outras conexões
    /// abertas no mesmo arquivo.
    pub fn open_with_clock(path: &Path, clock: Box<dyn Clock>) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < SCHEMA_VERSION_REQUIRED {
            return Err(AssistError::NotMigrated(version));
        }
        Ok(Self { conn, clock })
    }

    /// O relógio em uso.
    pub fn clock(&self) -> &dyn Clock {
        self.clock.as_ref()
    }

    // ------------------------------------------------------------ leitura

    /// Uma tarefa pelo id.
    pub fn task(&self, id: &str) -> Result<Task> {
        self.conn
            .query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"),
                params![id],
                task_from_row,
            )
            .optional()?
            .ok_or_else(|| AssistError::NotFound(format!("a tarefa {id}")))?
    }

    /// O dia de hoje no relógio do store.
    pub fn today(&self) -> Result<Today> {
        self.today_for(self.clock.today())
    }

    /// Monta um dia: o que fazer, a caixa de entrada, o depois e o feito.
    pub fn today_for(&self, day: NaiveDate) -> Result<Today> {
        let d = day.to_string();
        let planned = self.tasks_where(
            "status = 'open'
               AND ((planned_on IS NOT NULL AND planned_on <= ?1)
                 OR (due_on IS NOT NULL AND due_on <= ?1))
             ORDER BY planned_time IS NULL, planned_time, priority DESC, position, id",
            params![d],
        )?;
        let inbox = self.tasks_where("status = 'inbox' ORDER BY position DESC", params![])?;
        let later = self.tasks_where(
            "status = 'open'
               AND NOT ((planned_on IS NOT NULL AND planned_on <= ?1)
                     OR (due_on IS NOT NULL AND due_on <= ?1))
             ORDER BY planned_on IS NULL, planned_on, priority DESC, position, id",
            params![d],
        )?;
        let (start, end) = self.clock.day_bounds(day);
        let done_today = self.tasks_where(
            "status = 'done' AND completed_at >= ?1 AND completed_at < ?2
             ORDER BY completed_at DESC",
            params![start, end],
        )?;
        Ok(Today {
            day: Some(day),
            planned,
            inbox,
            later,
            done_today,
        })
    }

    fn tasks_where(&self, clause: &str, args: impl rusqlite::Params) -> Result<Vec<Task>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {TASK_COLUMNS} FROM tasks WHERE {clause}"))?;
        let rows = stmt.query_map(args, task_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row??);
        }
        Ok(out)
    }

    /// O diário de um dia, do mais antigo para o mais recente.
    pub fn journal_for_day(&self, day: NaiveDate) -> Result<Vec<JournalEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, at, day, actor, action, object_kind, object_id, summary, data
               FROM journal WHERE day = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![day.to_string()], journal_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row??);
        }
        Ok(out)
    }

    // ------------------------------------------------------------ escrita

    /// Cria uma tarefa.
    pub fn create_task(&self, new: NewTask, actor: Actor) -> Result<Task> {
        self.insert_task(new, None, actor)
    }

    /// Cria uma tarefa, ligada ou não a uma rotina.
    fn insert_task(&self, new: NewTask, routine_id: Option<String>, actor: Actor) -> Result<Task> {
        let new = new.normalized()?;
        let now = self.clock.now_ms();
        // A ordem manual nasce com o instante: as novas vão para o fim, e
        // reordenar depois cabe entre dois valores quaisquer. Duas criadas no
        // mesmo milissegundo (uma importação) ainda ficam na ordem em que
        // chegaram.
        let last: f64 =
            self.conn
                .query_row("SELECT COALESCE(MAX(position), 0) FROM tasks", [], |r| {
                    r.get(0)
                })?;
        let position = (now as f64).max(last + 1.0);
        let task = Task {
            id: uuid::Uuid::now_v7().to_string(),
            title: new.title,
            notes: new.notes,
            status: new.status,
            planned_on: new.planned_on,
            planned_time: new.planned_time,
            due_on: new.due_on,
            priority: new.priority,
            area: new.area,
            source_kind: new.source_kind,
            source_ref: new.source_ref,
            external_ref: new.external_ref,
            routine_id,
            position,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            &format!(
                "INSERT INTO tasks ({TASK_COLUMNS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)"
            ),
            params![
                task.id,
                task.title,
                task.notes,
                task.status.as_str(),
                task.planned_on.map(|d| d.to_string()),
                task.planned_time,
                task.due_on.map(|d| d.to_string()),
                task.priority,
                task.area,
                task.source_kind.as_str(),
                task.source_ref.as_ref().map(serde_json::Value::to_string),
                task.external_ref,
                task.routine_id,
                task.position,
                task.created_at,
                task.updated_at,
                task.completed_at,
            ],
        )?;
        self.journal(&tx, now, actor, "task.created", None, &task, None)?;
        tx.commit()?;
        Ok(task)
    }

    /// Importa uma tarefa de fora, uma vez só por `external_ref`.
    pub fn import_task(&self, new: NewTask) -> Result<ImportOutcome> {
        let Some(external) = new
            .external_ref
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return Err(AssistError::Invalid(
                "importação sem id de origem (external_ref)".into(),
            ));
        };
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM tasks WHERE external_ref = ?1",
                params![external],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(ImportOutcome::AlreadyThere(id));
        }
        self.create_task(new, Actor::Import)
            .map(|task| ImportOutcome::Created(Box::new(task)))
    }

    /// As tarefas de uma reunião (ver [`crate::meeting`]), uma vez só: se a
    /// reunião já mandou tarefas para a caixa de entrada (o passe final, uma
    /// importação repetida), nada entra de novo e a lista volta vazia.
    pub fn import_meeting_actions(
        &self,
        meeting_id: i64,
        drafts: Vec<NewTask>,
    ) -> Result<Vec<Task>> {
        let prefix = crate::meeting::external_prefix(meeting_id);
        let already: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE substr(external_ref, 1, length(?1)) = ?1)",
            params![prefix],
            |r| r.get(0),
        )?;
        if already {
            return Ok(Vec::new());
        }
        drafts
            .into_iter()
            .map(|d| self.create_task(d, Actor::Assistant))
            .collect()
    }

    /// Muda título, notas, dia, hora, prazo, prioridade ou área.
    pub fn update_task(&self, id: &str, patch: TaskPatch, actor: Actor) -> Result<Task> {
        let before = self.task(id)?;
        let mut after = patch.apply(&before)?;
        if after == before {
            return Ok(before);
        }
        after.updated_at = self.clock.now_ms();
        self.write(&before, &after, actor, "task.updated", None)?;
        Ok(after)
    }

    /// Conclui, reabre, descarta ou devolve à caixa de entrada.
    pub fn set_status(&self, id: &str, status: TaskStatus, actor: Actor) -> Result<Task> {
        let before = self.task(id)?;
        if before.status == status {
            return Ok(before);
        }
        let now = self.clock.now_ms();
        let mut after = before.clone();
        after.status = status;
        after.updated_at = now;
        after.completed_at = (status == TaskStatus::Done).then_some(now);
        let action = match (before.status, status) {
            (_, TaskStatus::Done) => "task.completed",
            (_, TaskStatus::Dropped) => "task.dropped",
            (TaskStatus::Inbox, TaskStatus::Open) => "task.accepted",
            (_, TaskStatus::Open) => "task.reopened",
            (_, TaskStatus::Inbox) => "task.to_inbox",
        };
        self.write(&before, &after, actor, action, None)?;
        Ok(after)
    }

    /// Aceita uma tarefa da caixa de entrada para um dia (ou para "algum
    /// dia"), numa mudança só: um desfazer devolve à caixa de entrada.
    pub fn accept(&self, id: &str, planned_on: Option<NaiveDate>, actor: Actor) -> Result<Task> {
        let before = self.task(id)?;
        if before.status != TaskStatus::Inbox {
            return Err(AssistError::Invalid(
                "a tarefa não está na caixa de entrada".into(),
            ));
        }
        let mut after = before.clone();
        after.status = TaskStatus::Open;
        after.planned_on = planned_on;
        if planned_on.is_none() {
            after.planned_time = None;
        }
        after.updated_at = self.clock.now_ms();
        self.write(&before, &after, actor, "task.accepted", None)?;
        Ok(after)
    }

    /// Desfaz a mudança registrada na linha `journal_id` do diário.
    ///
    /// Só a mudança mais recente de cada tarefa pode ser desfeita. Desfazer a
    /// criação descarta a tarefa (nada some). Desfazer um desfazer refaz.
    pub fn undo(&self, journal_id: i64, actor: Actor) -> Result<Task> {
        let entry = self
            .conn
            .query_row(
                "SELECT id, at, day, actor, action, object_kind, object_id, summary, data
                   FROM journal WHERE id = ?1",
                params![journal_id],
                journal_from_row,
            )
            .optional()?
            .ok_or_else(|| AssistError::NotFound(format!("a mudança {journal_id}")))??;
        if entry.object_kind != "task" {
            return Err(AssistError::Invalid(
                "só mudanças em tarefas se desfazem".into(),
            ));
        }
        let latest: i64 = self.conn.query_row(
            "SELECT MAX(id) FROM journal WHERE object_kind = 'task' AND object_id = ?1",
            params![entry.object_id],
            |r| r.get(0),
        )?;
        if latest != entry.id {
            return Err(AssistError::UndoStale);
        }
        let current = self.task(&entry.object_id)?;
        let before: Option<Task> = match entry.data.as_ref().and_then(|d| d.get("before")) {
            Some(v) if !v.is_null() => Some(serde_json::from_value(v.clone())?),
            _ => None,
        };
        let now = self.clock.now_ms();
        let mut restored = match before {
            Some(before) => Task {
                // Só o que a pessoa vê volta; a identidade e a origem ficam.
                id: current.id.clone(),
                source_kind: current.source_kind,
                source_ref: current.source_ref.clone(),
                external_ref: current.external_ref.clone(),
                routine_id: current.routine_id.clone(),
                created_at: current.created_at,
                ..before
            },
            // Desfazer a criação: a tarefa sai da lista, mas não do banco.
            None => Task {
                status: TaskStatus::Dropped,
                completed_at: None,
                ..current.clone()
            },
        };
        restored.updated_at = now;
        self.write(&current, &restored, actor, "task.restored", Some(entry.id))?;
        Ok(restored)
    }

    /// Grava a tarefa e a linha do diário numa transação.
    fn write(
        &self,
        before: &Task,
        after: &Task,
        actor: Actor,
        action: &str,
        undoes: Option<i64>,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE tasks SET title = ?2, notes = ?3, status = ?4, planned_on = ?5,
                    planned_time = ?6, due_on = ?7, priority = ?8, area = ?9, position = ?10,
                    updated_at = ?11, completed_at = ?12
              WHERE id = ?1",
            params![
                after.id,
                after.title,
                after.notes,
                after.status.as_str(),
                after.planned_on.map(|d| d.to_string()),
                after.planned_time,
                after.due_on.map(|d| d.to_string()),
                after.priority,
                after.area,
                after.position,
                after.updated_at,
                after.completed_at,
            ],
        )?;
        self.journal(
            &tx,
            after.updated_at,
            actor,
            action,
            Some(before),
            after,
            undoes,
        )?;
        tx.commit()?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn journal(
        &self,
        conn: &Connection,
        at: i64,
        actor: Actor,
        action: &str,
        before: Option<&Task>,
        after: &Task,
        undoes: Option<i64>,
    ) -> Result<()> {
        let mut data = json!({ "before": before, "after": after });
        if let Some(id) = undoes {
            data["undoes"] = json!(id);
        }
        self.journal_row(
            conn,
            at,
            actor,
            action,
            "task",
            &after.id,
            &after.title,
            &data,
        )
    }

    /// Uma linha do diário, de qualquer objeto.
    #[allow(clippy::too_many_arguments)]
    fn journal_row(
        &self,
        conn: &Connection,
        at: i64,
        actor: Actor,
        action: &str,
        object_kind: &str,
        object_id: &str,
        summary: &str,
        data: &serde_json::Value,
    ) -> Result<()> {
        conn.execute(
            "INSERT INTO journal (at, day, actor, action, object_kind, object_id, summary, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                at,
                self.clock.day_of(at).to_string(),
                actor.as_str(),
                action,
                object_kind,
                object_id,
                summary,
                data.to_string(),
            ],
        )?;
        Ok(())
    }

    /// Id da linha mais recente do diário para uma tarefa (o que o desfazer
    /// da interface usa logo depois de uma mudança).
    pub fn last_change(&self, task_id: &str) -> Result<Option<i64>> {
        Ok(self.conn.query_row(
            "SELECT MAX(id) FROM journal WHERE object_kind = 'task' AND object_id = ?1",
            params![task_id],
            |r| r.get(0),
        )?)
    }
}

fn parse_day(value: Option<String>) -> Result<Option<NaiveDate>> {
    value
        .map(|s| {
            NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                .map_err(|_| AssistError::Invalid(format!("dia ilegível no banco: {s}")))
        })
        .transpose()
}

fn task_from_row(r: &Row<'_>) -> rusqlite::Result<Result<Task>> {
    let status: String = r.get(3)?;
    let planned_on: Option<String> = r.get(4)?;
    let due_on: Option<String> = r.get(6)?;
    let source_kind: String = r.get(9)?;
    let source_ref: Option<String> = r.get(10)?;
    let fields = (
        r.get::<_, String>(0)?,
        r.get::<_, String>(1)?,
        r.get::<_, String>(2)?,
        r.get::<_, Option<String>>(5)?,
        r.get::<_, u8>(7)?,
        r.get::<_, Option<String>>(8)?,
        r.get::<_, Option<String>>(11)?,
        r.get::<_, Option<String>>(12)?,
        r.get::<_, f64>(13)?,
        r.get::<_, i64>(14)?,
        r.get::<_, i64>(15)?,
        r.get::<_, Option<i64>>(16)?,
    );
    Ok((|| {
        let (
            id,
            title,
            notes,
            planned_time,
            priority,
            area,
            external_ref,
            routine_id,
            position,
            created_at,
            updated_at,
            completed_at,
        ) = fields;
        Ok(Task {
            id,
            title,
            notes,
            status: TaskStatus::parse(&status)?,
            planned_on: parse_day(planned_on)?,
            planned_time,
            due_on: parse_day(due_on)?,
            priority,
            area,
            source_kind: SourceKind::parse(&source_kind)?,
            source_ref: source_ref.map(|s| serde_json::from_str(&s)).transpose()?,
            external_ref,
            routine_id,
            position,
            created_at,
            updated_at,
            completed_at,
        })
    })())
}

fn journal_from_row(r: &Row<'_>) -> rusqlite::Result<Result<JournalEntry>> {
    let day: String = r.get(2)?;
    let data: Option<String> = r.get(8)?;
    let fields = (
        r.get::<_, i64>(0)?,
        r.get::<_, i64>(1)?,
        r.get::<_, String>(3)?,
        r.get::<_, String>(4)?,
        r.get::<_, String>(5)?,
        r.get::<_, String>(6)?,
        r.get::<_, String>(7)?,
    );
    Ok((|| {
        let (id, at, actor, action, object_kind, object_id, summary) = fields;
        Ok(JournalEntry {
            id,
            at,
            day: parse_day(Some(day))?.unwrap_or_default(),
            actor,
            action,
            object_kind,
            object_id,
            summary,
            data: data.map(|s| serde_json::from_str(&s)).transpose()?,
        })
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// Um `isper.db` novo, migrado pelo `isper-core` como no app.
    pub(crate) fn store_at(clock: FixedClock) -> (AssistStore, std::path::PathBuf) {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("isper-assist-test-{}-{n}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        isper_core::store::MeetingStore::open(&path).unwrap();
        let store = AssistStore::open_with_clock(&path, Box::new(clock)).unwrap();
        (store, path)
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn quarta() -> FixedClock {
        FixedClock::at("2026-10-07 09:00", -3)
    }

    fn planned(title: &str, on: &str, time: Option<&str>) -> NewTask {
        NewTask {
            planned_on: Some(day(on)),
            planned_time: time.map(String::from),
            ..NewTask::titled(title)
        }
    }

    #[test]
    fn banco_sem_a_v7_e_recusado() {
        let path =
            std::env::temp_dir().join(format!("isper-assist-test-v6-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 6).unwrap();
        drop(conn);
        assert!(matches!(
            AssistStore::open(&path),
            Err(AssistError::NotMigrated(6))
        ));
    }

    #[test]
    fn criar_guarda_a_origem_e_escreve_no_diario() {
        let (store, _) = store_at(quarta());
        let new = NewTask {
            source_kind: SourceKind::Meeting,
            source_ref: Some(json!({ "meeting_id": 42, "at_secs": 754.5 })),
            ..NewTask::titled("Enviar a planilha de estimativas")
        };
        let task = store.create_task(new, Actor::User).unwrap();
        assert_eq!(store.task(&task.id).unwrap(), task);
        assert_eq!(task.source_kind, SourceKind::Meeting);
        assert_eq!(task.source_ref.as_ref().unwrap()["meeting_id"], 42);

        let diario = store.journal_for_day(day("2026-10-07")).unwrap();
        assert_eq!(diario.len(), 1);
        assert_eq!(diario[0].action, "task.created");
        assert_eq!(diario[0].actor, "user");
        assert_eq!(diario[0].summary, "Enviar a planilha de estimativas");
        assert!(diario[0].data.as_ref().unwrap()["before"].is_null());
    }

    #[test]
    fn o_dia_separa_hoje_caixa_depois_e_feito() {
        let (store, _) = store_at(quarta());
        let tarde = store
            .create_task(
                planned("Refinamento", "2026-10-07", Some("14:00")),
                Actor::User,
            )
            .unwrap();
        let cedo = store
            .create_task(planned("Daily", "2026-10-07", Some("09:15")), Actor::User)
            .unwrap();
        let sem_hora = store
            .create_task(planned("Revisar o PR", "2026-10-07", None), Actor::User)
            .unwrap();
        let atrasada = store
            .create_task(
                planned("Ligar pro contador", "2026-10-06", None),
                Actor::User,
            )
            .unwrap();
        let prazo = store
            .create_task(
                NewTask {
                    due_on: Some(day("2026-10-07")),
                    ..NewTask::titled("Entregar o relatório")
                },
                Actor::User,
            )
            .unwrap();
        let amanha = store
            .create_task(
                planned("Conferir o deploy", "2026-10-08", None),
                Actor::User,
            )
            .unwrap();
        let algum_dia = store
            .create_task(NewTask::titled("Ler o ADR"), Actor::User)
            .unwrap();
        let da_reuniao = store
            .create_task(
                NewTask {
                    status: TaskStatus::Inbox,
                    source_kind: SourceKind::Meeting,
                    ..NewTask::titled("Mandar o link da gravação")
                },
                Actor::User,
            )
            .unwrap();
        let feita = store
            .create_task(
                planned("Responder o cliente", "2026-10-07", None),
                Actor::User,
            )
            .unwrap();
        store
            .set_status(&feita.id, TaskStatus::Done, Actor::User)
            .unwrap();

        let hoje = store.today().unwrap();
        let ids = |v: &[Task]| v.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        assert_eq!(
            ids(&hoje.planned),
            vec![cedo.id, tarde.id, sem_hora.id, atrasada.id, prazo.id],
            "com hora primeiro, depois a ordem de criação; atrasada e com prazo entram"
        );
        assert_eq!(ids(&hoje.inbox), vec![da_reuniao.id]);
        assert_eq!(ids(&hoje.later), vec![amanha.id, algum_dia.id]);
        assert_eq!(ids(&hoje.done_today), vec![feita.id]);
    }

    #[test]
    fn concluida_ontem_nao_aparece_como_feita_hoje() {
        let (store, path) = store_at(FixedClock::at("2026-10-06 23:50", -3));
        let t = store
            .create_task(NewTask::titled("Fechar o dia"), Actor::User)
            .unwrap();
        store
            .set_status(&t.id, TaskStatus::Done, Actor::User)
            .unwrap();
        drop(store);
        // 00:10 do dia seguinte, no fuso local (ainda 03:10 UTC).
        let store =
            AssistStore::open_with_clock(&path, Box::new(FixedClock::at("2026-10-07 00:10", -3)))
                .unwrap();
        assert!(store.today().unwrap().done_today.is_empty());
        assert_eq!(
            store.today_for(day("2026-10-06")).unwrap().done_today.len(),
            1
        );
    }

    #[test]
    fn concluir_e_reabrir_mexem_no_instante_de_conclusao() {
        let (store, _) = store_at(quarta());
        let t = store
            .create_task(NewTask::titled("x"), Actor::User)
            .unwrap();
        let feita = store
            .set_status(&t.id, TaskStatus::Done, Actor::User)
            .unwrap();
        assert_eq!(feita.completed_at, Some(quarta().now_ms));
        let de_novo = store
            .set_status(&t.id, TaskStatus::Done, Actor::User)
            .unwrap();
        assert_eq!(de_novo, feita, "concluir de novo não muda nada");
        let aberta = store
            .set_status(&t.id, TaskStatus::Open, Actor::User)
            .unwrap();
        assert_eq!(aberta.completed_at, None);
        let acoes: Vec<String> = store
            .journal_for_day(day("2026-10-07"))
            .unwrap()
            .into_iter()
            .map(|e| e.action)
            .collect();
        assert_eq!(acoes, ["task.created", "task.completed", "task.reopened"]);
    }

    #[test]
    fn aceitar_da_caixa_de_entrada_e_registrado() {
        let (store, _) = store_at(quarta());
        let t = store
            .create_task(
                NewTask {
                    status: TaskStatus::Inbox,
                    ..NewTask::titled("Da reunião")
                },
                Actor::User,
            )
            .unwrap();
        store
            .set_status(&t.id, TaskStatus::Open, Actor::User)
            .unwrap();
        let ultima = store
            .journal_for_day(day("2026-10-07"))
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(ultima.action, "task.accepted");
    }

    #[test]
    fn aceitar_para_um_dia_e_uma_mudanca_so() {
        let (store, _) = store_at(quarta());
        let t = store
            .create_task(
                NewTask {
                    status: TaskStatus::Inbox,
                    source_kind: SourceKind::Meeting,
                    ..NewTask::titled("Mandar o link da gravação")
                },
                Actor::User,
            )
            .unwrap();
        let aceita = store
            .accept(&t.id, Some(day("2026-10-07")), Actor::User)
            .unwrap();
        assert_eq!(aceita.status, TaskStatus::Open);
        assert_eq!(store.today().unwrap().planned.len(), 1);
        let mudanca = store.last_change(&t.id).unwrap().unwrap();
        let de_volta = store.undo(mudanca, Actor::User).unwrap();
        assert_eq!(de_volta.status, TaskStatus::Inbox);
        assert_eq!(de_volta.planned_on, None);
        assert!(
            store.accept(&aceita.id, None, Actor::User).is_ok(),
            "depois de desfazer, está de novo na caixa"
        );
        assert!(
            store.accept(&aceita.id, None, Actor::User).is_err(),
            "aceitar de novo é erro: já não está na caixa"
        );
    }

    #[test]
    fn editar_sem_mudar_nada_nao_escreve_no_diario() {
        let (store, _) = store_at(quarta());
        let t = store
            .create_task(NewTask::titled("Mesmo"), Actor::User)
            .unwrap();
        let patch = TaskPatch {
            title: Some("  Mesmo ".into()),
            ..TaskPatch::default()
        };
        store.update_task(&t.id, patch, Actor::User).unwrap();
        assert_eq!(store.journal_for_day(day("2026-10-07")).unwrap().len(), 1);
    }

    #[test]
    fn desfazer_volta_o_antes_e_desfazer_de_novo_refaz() {
        let (store, _) = store_at(quarta());
        let t = store
            .create_task(planned("Revisar o PR", "2026-10-07", None), Actor::User)
            .unwrap();
        let feita = store
            .set_status(&t.id, TaskStatus::Done, Actor::User)
            .unwrap();
        let mudanca = store.last_change(&t.id).unwrap().unwrap();

        let desfeita = store.undo(mudanca, Actor::User).unwrap();
        assert_eq!(desfeita.status, TaskStatus::Open);
        assert_eq!(desfeita.completed_at, None);
        assert_eq!(desfeita.title, "Revisar o PR");
        assert!(store.today().unwrap().done_today.is_empty());

        // O desfazer é ele mesmo uma mudança: desfazê-lo refaz.
        let restauracao = store.last_change(&t.id).unwrap().unwrap();
        let refeita = store.undo(restauracao, Actor::User).unwrap();
        assert_eq!(refeita.status, TaskStatus::Done);
        assert_eq!(refeita.completed_at, feita.completed_at);
    }

    #[test]
    fn desfazer_a_criacao_descarta_sem_apagar() {
        let (store, _) = store_at(quarta());
        let t = store
            .create_task(NewTask::titled("Engano"), Actor::User)
            .unwrap();
        let criacao = store.last_change(&t.id).unwrap().unwrap();
        let desfeita = store.undo(criacao, Actor::User).unwrap();
        assert_eq!(desfeita.status, TaskStatus::Dropped);
        assert!(store.today().unwrap().later.is_empty());
        assert_eq!(
            store.task(&t.id).unwrap().title,
            "Engano",
            "continua no banco"
        );
    }

    #[test]
    fn so_a_mudanca_mais_recente_se_desfaz() {
        let (store, _) = store_at(quarta());
        let t = store
            .create_task(NewTask::titled("x"), Actor::User)
            .unwrap();
        store
            .set_status(&t.id, TaskStatus::Done, Actor::User)
            .unwrap();
        let concluir = store.last_change(&t.id).unwrap().unwrap();
        store
            .update_task(
                &t.id,
                TaskPatch {
                    title: Some("y".into()),
                    ..TaskPatch::default()
                },
                Actor::User,
            )
            .unwrap();
        assert!(matches!(
            store.undo(concluir, Actor::User),
            Err(AssistError::UndoStale)
        ));
    }

    #[test]
    fn importar_duas_vezes_cria_uma_vez() {
        let (store, _) = store_at(quarta());
        let new = NewTask {
            source_kind: SourceKind::Notion,
            external_ref: Some("notion:abc".into()),
            ..NewTask::titled("Registrar 8h no OptTime")
        };
        let primeira = store.import_task(new.clone()).unwrap();
        let ImportOutcome::Created(task) = primeira else {
            panic!("a primeira importação cria");
        };
        assert_eq!(
            store.import_task(new).unwrap(),
            ImportOutcome::AlreadyThere(task.id.clone())
        );
        let diario = store.journal_for_day(day("2026-10-07")).unwrap();
        assert_eq!(diario.len(), 1);
        assert_eq!(diario[0].actor, "import");
        assert!(
            store.import_task(NewTask::titled("sem id")).is_err(),
            "importação precisa do id de origem"
        );
    }

    #[test]
    fn acoes_da_reuniao_entram_uma_vez_so() {
        use crate::meeting::{MeetingAction, MeetingRef, drafts};
        let (store, _) = store_at(quarta());
        let meeting = MeetingRef {
            id: 7,
            title: "Daily".into(),
            day: day("2026-10-07"),
        };
        let acoes = vec![MeetingAction {
            title: "Mandar a planilha".into(),
            at_secs: Some(754.0),
            due: Some("até sexta".into()),
            quote: None,
            from_copilot: false,
        }];
        let criadas = store
            .import_meeting_actions(7, drafts(&meeting, &acoes))
            .unwrap();
        assert_eq!(criadas.len(), 1);
        assert_eq!(store.today().unwrap().inbox.len(), 1);
        assert!(
            store
                .import_meeting_actions(7, drafts(&meeting, &acoes))
                .unwrap()
                .is_empty(),
            "a mesma reunião não manda de novo"
        );
        // A reunião 70 não se confunde com a 7.
        let outra = MeetingRef { id: 70, ..meeting };
        assert_eq!(
            store
                .import_meeting_actions(70, drafts(&outra, &acoes))
                .unwrap()
                .len(),
            1
        );
        let diario = store.journal_for_day(day("2026-10-07")).unwrap();
        assert!(diario.iter().all(|e| e.actor == "assistant"));
    }

    #[test]
    fn tirar_o_dia_manda_para_depois() {
        let (store, _) = store_at(quarta());
        let t = store
            .create_task(planned("x", "2026-10-07", Some("10:00")), Actor::User)
            .unwrap();
        let depois = store
            .update_task(
                &t.id,
                TaskPatch {
                    planned_on: Some(None),
                    ..TaskPatch::default()
                },
                Actor::User,
            )
            .unwrap();
        assert_eq!(depois.planned_time, None);
        let hoje = store.today().unwrap();
        assert!(hoje.planned.is_empty());
        assert_eq!(hoje.later.len(), 1);
    }
}
