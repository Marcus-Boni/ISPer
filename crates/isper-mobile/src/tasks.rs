//! As tarefas no celular (Fase 10.6): a cópia do que o PC mandou e a fila do
//! que mudou aqui ([`isper_assist::phone`]).
//!
//! O estado mora em dois arquivos na pasta interna do app:
//!
//! - `tarefas.json`: o último retrato do PC e quando ele chegou;
//! - `tarefas-fila.json`: as mudanças feitas aqui que o PC ainda não viu,
//!   cada uma com a hora em que foi feita.
//!
//! A tela mostra o retrato com a fila aplicada por cima: criar, concluir ou
//! mudar o dia aparece na hora, com ou sem o PC por perto. Na sincronia
//! ([`crate::PcLink::sync`]), a fila vai junto e volta o retrato novo; o PC
//! decide campo a campo quem vence (o último a escrever).
//!
//! O dia e o fuso vêm do app ([`TaskBook::day`]): o Kotlin sabe o fuso do
//! aparelho, e o núcleo não depende do banco de fusos do Android.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{NaiveDate, NaiveTime};
use isper_assist::phone::{LOCAL_PREFIX, PhoneOp, PhoneTask, Snapshot, TasksResponse, apply_local};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::MobileError;
use crate::sync::write_atomic;

const BOOK_FILE: &str = "tarefas.json";
const QUEUE_FILE: &str = "tarefas-fila.json";
/// Tamanho máximo de um título (o mesmo do PC).
const MAX_TITLE: usize = 500;

/// Leitura e escrita dos dois arquivos, uma de cada vez: a tela e a
/// sincronia (o WorkManager) mexem neles ao mesmo tempo.
static FILES: Mutex<()> = Mutex::new(());

/// `tarefas.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Book {
    snapshot: Option<Snapshot>,
    /// Quando o retrato chegou (ms UTC, relógio do celular).
    synced_at_ms: Option<i64>,
    /// Por que a última sincronia das tarefas não deu (o PC antigo, por exemplo).
    #[serde(default)]
    error: Option<String>,
    /// Mudanças do celular em que o PC venceu, na última sincronia.
    #[serde(default)]
    kept_pc: u32,
    /// A última tentativa, deu certo ou não: um PC sem tarefas não é
    /// procurado de novo a cada rodada.
    #[serde(default)]
    tried_at_ms: Option<i64>,
}

fn read<T: Default + for<'de> Deserialize<'de>>(path: &Path) -> T {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write<T: Serialize>(path: &Path, value: &T) -> Result<(), MobileError> {
    let json = serde_json::to_vec_pretty(value).map_err(|e| MobileError::Sync(e.to_string()))?;
    write_atomic(path, &json).map_err(|e| MobileError::Sync(e.to_string()))
}

/// Uma tarefa como a tela do celular mostra.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MobileTask {
    /// O id no PC, ou `local:…` enquanto o PC não viu a tarefa.
    pub id: String,
    pub title: String,
    pub notes: String,
    /// `inbox`, `open`, `done` ou `dropped`.
    pub status: String,
    /// `AAAA-MM-DD`.
    pub planned_on: Option<String>,
    /// `HH:MM`.
    pub planned_time: Option<String>,
    /// `AAAA-MM-DD`.
    pub due_on: Option<String>,
    /// A origem (`voice`, `meeting`, `routine`…).
    pub source: String,
    /// Criada por uma rotina.
    pub from_routine: bool,
    /// Atrasada: aberta, com o dia antes de hoje.
    pub overdue: bool,
    /// Tem mudança que o PC ainda não viu.
    pub pending: bool,
}

/// Uma rotina (só para ler, por enquanto).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MobileRoutine {
    pub id: String,
    pub title: String,
    /// `daily`, `weekly` ou `monthly`.
    pub freq: String,
    /// `MO`, `TU`…
    pub weekdays: Vec<String>,
    /// `HH:MM`.
    pub time: Option<String>,
    pub active: bool,
}

/// O dia como a tela Hoje do celular mostra.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MobileDay {
    /// Para fazer: as de hoje e as atrasadas, as com hora primeiro.
    pub planned: Vec<MobileTask>,
    /// Caixa de entrada (o que chegou das reuniões).
    pub inbox: Vec<MobileTask>,
    /// Depois: de outro dia ou sem dia.
    pub later: Vec<MobileTask>,
    /// Feitas hoje.
    pub done_today: Vec<MobileTask>,
    pub routines: Vec<MobileRoutine>,
    /// Mudanças esperando a sincronia.
    pub pending: u32,
    /// Quando o retrato do PC chegou (ms UTC); `None` se nunca chegou.
    pub synced_at_ms: Option<i64>,
    /// Por que a última sincronia das tarefas não deu.
    pub sync_error: Option<String>,
    /// Mudanças daqui em que o PC venceu na última sincronia.
    pub kept_pc: u32,
}

/// O que a frase digitada ou ditada vira, antes de salvar.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ParsedTask {
    pub title: String,
    pub planned_on: Option<String>,
    pub planned_time: Option<String>,
    pub due_on: Option<String>,
}

fn parse_day(s: &str) -> Result<NaiveDate, MobileError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| MobileError::Sync(format!("dia inválido: {s}")))
}

fn clean_time(t: Option<String>) -> Result<Option<String>, MobileError> {
    match t.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(t) => NaiveTime::parse_from_str(&t, "%H:%M")
            .map(|v| Some(v.format("%H:%M").to_string()))
            .map_err(|_| MobileError::Sync(format!("hora inválida: {t}"))),
    }
}

/// A tela do celular sobre os dois arquivos.
#[derive(uniffi::Object)]
pub struct TaskBook {
    dir: PathBuf,
}

impl TaskBook {
    fn book_path(&self) -> PathBuf {
        self.dir.join(BOOK_FILE)
    }

    fn queue_path(&self) -> PathBuf {
        self.dir.join(QUEUE_FILE)
    }

    /// Acrescenta uma mudança à fila.
    fn push(&self, op: PhoneOp) -> Result<(), MobileError> {
        let _g = FILES.lock().unwrap_or_else(PoisonError::into_inner);
        let mut queue: Vec<PhoneOp> = read(&self.queue_path());
        queue.push(op);
        write(&self.queue_path(), &queue)
    }

    fn change(
        &self,
        id: &str,
        changes: BTreeMap<String, Value>,
        now_ms: i64,
    ) -> Result<(), MobileError> {
        if id.trim().is_empty() {
            return Err(MobileError::Sync("tarefa sem id".into()));
        }
        self.push(PhoneOp::new(Some(id.to_string()), changes, now_ms))
    }

    /// A fila, para a sincronia.
    pub(crate) fn pending_ops(&self) -> Vec<PhoneOp> {
        let _g = FILES.lock().unwrap_or_else(PoisonError::into_inner);
        read(&self.queue_path())
    }

    /// O retrato está velho (ou nunca veio): vale sincronizar mesmo sem fila.
    pub(crate) fn stale(&self, now_ms: i64, max_age_ms: i64) -> bool {
        let book: Book = read(&self.book_path());
        book.tried_at_ms
            .or(book.synced_at_ms)
            .is_none_or(|at| now_ms - at > max_age_ms)
    }

    /// O PC respondeu: guarda o retrato e tira da fila o que foi mandado. O
    /// que entrou na fila durante a sincronia fica, com o id da tarefa criada
    /// trocado pelo do PC.
    pub(crate) fn applied(
        &self,
        sent: &[PhoneOp],
        resp: TasksResponse,
        now_ms: i64,
    ) -> Result<u32, MobileError> {
        let _g = FILES.lock().unwrap_or_else(PoisonError::into_inner);
        let mut queue: Vec<PhoneOp> = read(&self.queue_path());
        queue.retain(|op| !sent.iter().any(|s| s.op_id == op.op_id));
        for r in &resp.results {
            let (Some(real), Some(op)) = (&r.task_id, sent.iter().find(|s| s.op_id == r.op_id))
            else {
                continue;
            };
            if op.task_id.is_none() {
                let local = format!("{LOCAL_PREFIX}{}", op.op_id);
                for later in &mut queue {
                    if later.task_id.as_deref() == Some(local.as_str()) {
                        later.task_id = Some(real.clone());
                    }
                }
            }
            if let Some(e) = &r.error {
                tracing::warn!(op = %r.op_id, "o PC não aplicou a mudança: {e}");
            }
        }
        let kept: u32 = resp.results.iter().map(|r| r.kept_pc.len() as u32).sum();
        write(&self.queue_path(), &queue)?;
        write(
            &self.book_path(),
            &Book {
                snapshot: Some(resp.snapshot),
                synced_at_ms: Some(now_ms),
                error: None,
                kept_pc: kept,
                tried_at_ms: Some(now_ms),
            },
        )?;
        Ok(kept)
    }

    /// A sincronia das tarefas não deu (o motivo aparece na tela).
    pub(crate) fn failed(&self, error: &str, now_ms: i64) {
        let _g = FILES.lock().unwrap_or_else(PoisonError::into_inner);
        let mut book: Book = read(&self.book_path());
        book.error = Some(error.to_string());
        book.tried_at_ms = Some(now_ms);
        if let Err(e) = write(&self.book_path(), &book) {
            tracing::warn!("não consegui anotar o erro da sincronia das tarefas: {e}");
        }
    }
}

#[uniffi::export]
impl TaskBook {
    /// As tarefas na pasta interna do app (a mesma do [`crate::PcLink`]).
    #[uniffi::constructor]
    pub fn new(state_dir: String) -> Arc<Self> {
        Arc::new(Self {
            dir: PathBuf::from(state_dir),
        })
    }

    /// O dia: `today` é `AAAA-MM-DD` e `utc_offset_minutes` o fuso do
    /// aparelho agora (para saber o que foi feito hoje).
    pub fn day(&self, today: String, utc_offset_minutes: i32) -> Result<MobileDay, MobileError> {
        let today = parse_day(&today)?;
        let (book, queue): (Book, Vec<PhoneOp>) = {
            let _g = FILES.lock().unwrap_or_else(PoisonError::into_inner);
            (read(&self.book_path()), read(&self.queue_path()))
        };
        let snapshot = book.snapshot.clone().unwrap_or(Snapshot {
            today,
            tasks: Vec::new(),
            routines: Vec::new(),
        });
        let pending_ids: Vec<String> = queue
            .iter()
            .map(|op| {
                op.task_id
                    .clone()
                    .unwrap_or_else(|| format!("{LOCAL_PREFIX}{}", op.op_id))
            })
            .collect();
        let offset_ms = i64::from(utc_offset_minutes) * 60_000;
        let local_day = |ms: i64| {
            chrono::DateTime::from_timestamp_millis(ms + offset_ms).map(|d| d.date_naive())
        };
        let to_mobile = |t: &PhoneTask| MobileTask {
            id: t.id.clone(),
            title: t.title.clone(),
            notes: t.notes.clone(),
            status: t.status.clone(),
            planned_on: t.planned_on.map(|d| d.to_string()),
            planned_time: t.planned_time.clone(),
            due_on: t.due_on.map(|d| d.to_string()),
            source: t.source.clone(),
            from_routine: t.routine_id.is_some(),
            overdue: t.status == "open" && t.planned_on.is_some_and(|d| d < today),
            pending: pending_ids.contains(&t.id),
        };
        let mut day = MobileDay {
            planned: Vec::new(),
            inbox: Vec::new(),
            later: Vec::new(),
            done_today: Vec::new(),
            routines: snapshot
                .routines
                .iter()
                .map(|r| MobileRoutine {
                    id: r.id.clone(),
                    title: r.title.clone(),
                    freq: r.freq.clone(),
                    weekdays: r.weekdays.clone(),
                    time: r.time.clone(),
                    active: r.active,
                })
                .collect(),
            pending: queue.len() as u32,
            synced_at_ms: book.synced_at_ms,
            sync_error: book.error.clone(),
            kept_pc: book.kept_pc,
        };
        let mut tasks = apply_local(&snapshot, &queue);
        // As com hora primeiro, na ordem da hora; depois as outras, na ordem do PC.
        tasks.sort_by(|a, b| {
            let key = |t: &PhoneTask| (t.planned_time.is_none(), t.planned_time.clone());
            key(a).cmp(&key(b))
        });
        for t in &tasks {
            let m = to_mobile(t);
            match t.status.as_str() {
                "inbox" => day.inbox.push(m),
                "open" if t.planned_on.is_some_and(|d| d <= today) => day.planned.push(m),
                "open" => day.later.push(m),
                "done" if t.completed_at.and_then(local_day) == Some(today) => {
                    day.done_today.push(m)
                }
                _ => {}
            }
        }
        Ok(day)
    }

    /// O que a frase vira: o título sem as datas, o dia, a hora e o prazo
    /// ("amanhã às 3 ligar pro João"), como no PC.
    pub fn parse(&self, text: String, today: String) -> Result<ParsedTask, MobileError> {
        let p = isper_assist::when::parse_task(&text, parse_day(&today)?);
        Ok(ParsedTask {
            title: p.title,
            planned_on: p.planned_on.map(|d| d.to_string()),
            planned_time: p.planned_time,
            due_on: p.due_on.map(|d| d.to_string()),
        })
    }

    /// Cria uma tarefa; devolve o id `local:…` que ela tem até o PC responder.
    /// Hora sem dia é hoje (`today`).
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &self,
        title: String,
        planned_on: Option<String>,
        planned_time: Option<String>,
        due_on: Option<String>,
        voice: bool,
        today: String,
        now_ms: i64,
    ) -> Result<String, MobileError> {
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            return Err(MobileError::Sync("a tarefa precisa de um título".into()));
        }
        if title.chars().count() > MAX_TITLE {
            return Err(MobileError::Sync(format!(
                "o título passa de {MAX_TITLE} caracteres"
            )));
        }
        let time = clean_time(planned_time)?;
        let mut planned_on = planned_on.filter(|d| !d.trim().is_empty());
        if time.is_some() && planned_on.is_none() {
            planned_on = Some(today);
        }
        let mut changes = BTreeMap::new();
        changes.insert("title".to_string(), json!(title));
        if let Some(d) = &planned_on {
            changes.insert("planned_on".into(), json!(parse_day(d)?.to_string()));
        }
        if let Some(t) = &time {
            changes.insert("planned_time".into(), json!(t));
        }
        if let Some(d) = due_on.filter(|d| !d.trim().is_empty()) {
            changes.insert("due_on".into(), json!(parse_day(&d)?.to_string()));
        }
        let mut op = PhoneOp::new(None, changes, now_ms);
        op.voice = voice;
        let id = format!("{LOCAL_PREFIX}{}", op.op_id);
        self.push(op)?;
        Ok(id)
    }

    /// Conclui (`done`), reabre (`open`) ou descarta (`dropped`). Aceitar da
    /// caixa de entrada é `open` com o dia ([`TaskBook::set_day`]).
    pub fn set_status(&self, id: String, status: String, now_ms: i64) -> Result<(), MobileError> {
        if !matches!(status.as_str(), "open" | "done" | "dropped") {
            return Err(MobileError::Sync(format!("estado desconhecido: {status}")));
        }
        self.change(&id, [("status".to_string(), json!(status))].into(), now_ms)
    }

    /// Muda o dia (`None` é "algum dia", e tira a hora). Na caixa de entrada,
    /// aceita a tarefa para esse dia.
    pub fn set_day(&self, id: String, day: Option<String>, now_ms: i64) -> Result<(), MobileError> {
        let mut changes = BTreeMap::new();
        match day.filter(|d| !d.trim().is_empty()) {
            Some(d) => {
                changes.insert("planned_on".to_string(), json!(parse_day(&d)?.to_string()));
            }
            None => {
                changes.insert("planned_on".to_string(), Value::Null);
                changes.insert("planned_time".to_string(), Value::Null);
            }
        }
        changes.insert("status".to_string(), json!("open"));
        self.change(&id, changes, now_ms)
    }

    /// Muda a hora (`None` tira).
    pub fn set_time(
        &self,
        id: String,
        time: Option<String>,
        now_ms: i64,
    ) -> Result<(), MobileError> {
        let time = clean_time(time)?;
        self.change(
            &id,
            [(
                "planned_time".to_string(),
                time.map_or(Value::Null, |t| json!(t)),
            )]
            .into(),
            now_ms,
        )
    }

    /// Muda o título.
    pub fn rename(&self, id: String, title: String, now_ms: i64) -> Result<(), MobileError> {
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            return Err(MobileError::Sync("a tarefa precisa de um título".into()));
        }
        self.change(&id, [("title".to_string(), json!(title))].into(), now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use isper_assist::phone::{OpResult, PhoneRoutine};

    const TODAY: &str = "2026-10-10";
    const NOW: i64 = 1_791_666_000_000; // 10/10/2026 18:00 -03

    fn book(name: &str) -> Arc<TaskBook> {
        let dir =
            std::env::temp_dir().join(format!("isper-mobile-tasks-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TaskBook::new(dir.to_string_lossy().into())
    }

    fn pc_task(id: &str, title: &str, status: &str, planned_on: Option<&str>) -> PhoneTask {
        PhoneTask {
            id: id.into(),
            title: title.into(),
            notes: String::new(),
            status: status.into(),
            planned_on: planned_on.map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()),
            planned_time: None,
            due_on: None,
            source: "manual".into(),
            routine_id: None,
            updated_at: 0,
            completed_at: (status == "done").then_some(NOW - 3_600_000),
        }
    }

    #[test]
    fn criar_offline_aparece_na_hora_e_a_fila_conta() {
        let b = book("criar");
        let p = b
            .parse("amanhã às 15h ligar pro contador".into(), TODAY.into())
            .unwrap();
        assert_eq!(p.title, "Ligar pro contador");
        assert_eq!(p.planned_on.as_deref(), Some("2026-10-11"));
        let id = b
            .add(
                p.title,
                p.planned_on,
                p.planned_time,
                None,
                true,
                TODAY.into(),
                NOW,
            )
            .unwrap();
        assert!(id.starts_with("local:"));
        b.add(
            "Pagar o boleto".into(),
            None,
            Some("17:00".into()),
            None,
            false,
            TODAY.into(),
            NOW,
        )
        .unwrap();
        let d = b.day(TODAY.into(), -180).unwrap();
        assert_eq!(d.pending, 2);
        assert_eq!(d.later[0].title, "Ligar pro contador");
        assert_eq!(d.later[0].source, "voice");
        assert!(d.later[0].pending);
        assert_eq!(d.planned[0].title, "Pagar o boleto", "hora sem dia é hoje");
        assert_eq!(d.synced_at_ms, None);
        assert!(
            b.add("  ".into(), None, None, None, false, TODAY.into(), NOW)
                .is_err()
        );
        assert!(
            b.add(
                "x".into(),
                None,
                Some("25:00".into()),
                None,
                false,
                TODAY.into(),
                NOW
            )
            .is_err()
        );
    }

    #[test]
    fn a_resposta_do_pc_esvazia_a_fila_e_troca_o_id_local() {
        let b = book("resposta");
        let local = b
            .add(
                "Comprar café".into(),
                Some(TODAY.into()),
                None,
                None,
                false,
                TODAY.into(),
                NOW,
            )
            .unwrap();
        let sent = b.pending_ops();
        // Durante a sincronia, a pessoa conclui a tarefa recém-criada.
        b.set_status(local.clone(), "done".into(), NOW + 1).unwrap();
        let resp = TasksResponse {
            now_ms: NOW,
            results: vec![OpResult {
                op_id: sent[0].op_id.clone(),
                task_id: Some("pc-1".into()),
                applied: vec!["title".into()],
                ..OpResult::default()
            }],
            snapshot: Snapshot {
                today: NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(),
                tasks: vec![pc_task("pc-1", "Comprar café", "open", Some(TODAY))],
                routines: vec![PhoneRoutine {
                    id: "r1".into(),
                    title: "Revisar os PRs".into(),
                    rrule: "FREQ=WEEKLY;BYDAY=SA".into(),
                    freq: "weekly".into(),
                    weekdays: vec!["SA".into()],
                    time: Some("09:00".into()),
                    active: true,
                }],
            },
        };
        assert_eq!(b.applied(&sent, resp, NOW).unwrap(), 0);
        let queue = b.pending_ops();
        assert_eq!(queue.len(), 1, "a conclusão feita durante a sincronia fica");
        assert_eq!(
            queue[0].task_id.as_deref(),
            Some("pc-1"),
            "já com o id do PC"
        );
        let d = b.day(TODAY.into(), -180).unwrap();
        assert_eq!(d.done_today.len(), 1, "concluída aqui, antes de o PC saber");
        assert_eq!(d.routines[0].time.as_deref(), Some("09:00"));
        assert_eq!(d.synced_at_ms, Some(NOW));
        assert!(!b.stale(NOW + 60_000, 5 * 60_000));
        assert!(b.stale(NOW + 6 * 60_000, 5 * 60_000));
    }

    #[test]
    fn dia_separa_atrasada_caixa_depois_e_feita_hoje() {
        let b = book("dia");
        let resp = TasksResponse {
            now_ms: NOW,
            results: Vec::new(),
            snapshot: Snapshot {
                today: NaiveDate::from_ymd_opt(2026, 10, 10).unwrap(),
                tasks: vec![
                    pc_task("a", "Atrasada", "open", Some("2026-10-08")),
                    pc_task("b", "Da reunião", "inbox", None),
                    pc_task("c", "Semana que vem", "open", Some("2026-10-15")),
                    pc_task("d", "Feita", "done", Some(TODAY)),
                ],
                routines: Vec::new(),
            },
        };
        b.applied(&[], resp, NOW).unwrap();
        let d = b.day(TODAY.into(), -180).unwrap();
        assert_eq!(d.planned[0].title, "Atrasada");
        assert!(d.planned[0].overdue);
        assert_eq!(d.inbox[0].title, "Da reunião");
        assert_eq!(d.later[0].title, "Semana que vem");
        assert_eq!(d.done_today[0].title, "Feita");
        // Aceitar da caixa para hoje.
        b.set_day("b".into(), Some(TODAY.into()), NOW).unwrap();
        let d = b.day(TODAY.into(), -180).unwrap();
        assert!(d.inbox.is_empty());
        assert!(
            d.planned
                .iter()
                .any(|t| t.title == "Da reunião" && t.pending)
        );
        b.failed("este PC não sincroniza tarefas", NOW + 60_000);
        assert!(b.day(TODAY.into(), -180).unwrap().sync_error.is_some());
        assert!(
            !b.stale(NOW + 120_000, 5 * 60_000),
            "não insiste a cada rodada"
        );
    }
}
