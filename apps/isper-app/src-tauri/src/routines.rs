//! Rotinas (Fase 10.2): o checklist do dia e o verificador do OptTime.
//!
//! - A cada minuto (e ao abrir a tela Hoje), as rotinas ligadas criam a
//!   tarefa do dia, uma vez só.
//! - Na hora da rotina, e de meia em meia hora depois enquanto ela estiver
//!   aberta, o verificador confere no OptTime se o dia fechou a meta. Ler é
//!   livre ([ADR 0023]). Fechou: no modo automático a tarefa se conclui
//!   sozinha; no "perguntar", ganha o botão Concluir. Não é dia útil: no
//!   automático ela sai, no "perguntar" diz por quê. Falta: um aviso do
//!   Windows, uma vez por tarefa, e as sugestões do "Preencher meu dia"
//!   esperando o toque.
//! - Lançar no OptTime é sempre depois do toque, numa transação do OptTime
//!   com chave de idempotência (repetir depois de uma queda de rede não
//!   duplica) e com uma linha no diário.
//!
//! [ADR 0023]: ../../../../docs/adr/0023-escada-de-confianca.md

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::connectors::{ConnectorError, opttime};
use crate::prelude::*;
use crate::today::{TASKS_EVENT, open_assist};
use chrono::NaiveDate;
use isper_assist::{
    Actor, AssistStore, Clock, NewRoutine, Occurrence, Routine, RoutineMode, RoutinePatch,
    TaskStatus, VERIFY_OPTTIME_DAY,
};
use isper_mcp::{Applied, ApplyItem, ApplyRequest, McpError, Suggestions};

/// De quanto em quanto o agendador olha as rotinas.
const TICK: Duration = Duration::from_secs(60);
/// Depois da hora, a rotina aberta é conferida de novo a cada meia hora (a
/// pessoa pode ter lançado as horas na web).
const RECHECK_MS: i64 = 30 * 60_000;
/// O OptTime sugere até 30 dias para trás.
const SUGGEST_MAX_DAYS: i64 = 30;

/// A última conferência de uma tarefa de rotina, guardada nela
/// (`source_ref.check`) para a tela Hoje mostrar sem consultar de novo.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum Check {
    /// Dia útil com a meta batida.
    Complete {
        at: i64,
        total_minutes: i64,
        target_minutes: i64,
    },
    /// Falta para a meta.
    Missing {
        at: i64,
        total_minutes: i64,
        target_minutes: i64,
        missing_minutes: i64,
        /// O aviso do Windows já saiu para esta tarefa.
        #[serde(default)]
        notified: bool,
    },
    /// Fim de semana, folga do expediente ou ausência no OptTime.
    NotWorkday { at: i64 },
    /// Sem token do OptTime.
    NoConnector { at: i64 },
    /// O OptTime respondeu com erro, ou não respondeu.
    Error {
        at: i64,
        code: String,
        message: String,
        hint: Option<String>,
    },
}

impl Check {
    fn at(&self) -> i64 {
        match self {
            Self::Complete { at, .. }
            | Self::Missing { at, .. }
            | Self::NotWorkday { at }
            | Self::NoConnector { at }
            | Self::Error { at, .. } => *at,
        }
    }

    fn error(at: i64, e: &McpError) -> Self {
        let e = ConnectorError::from(e);
        Self::Error {
            at,
            code: e.code,
            message: e.message,
            hint: e.hint,
        }
    }
}

fn last_check(occ: &Occurrence) -> Option<Check> {
    let v = occ.task.source_ref.as_ref()?.get("check")?.clone();
    serde_json::from_value(v).ok()
}

fn local_error(e: impl std::fmt::Display) -> ConnectorError {
    ConnectorError {
        code: "LOCAL".into(),
        message: e.to_string(),
        hint: None,
    }
}

/// Pergunta ao OptTime como está o dia da tarefa.
async fn probe(app: &AppHandle, occ: &Occurrence, now: i64) -> Check {
    if occ.routine.verifier.as_deref() != Some(VERIFY_OPTTIME_DAY) {
        return Check::Error {
            at: now,
            code: "NO_VERIFIER".into(),
            message: "rotina sem verificador".into(),
            hint: None,
        };
    }
    let ot = match opttime(app) {
        Ok(ot) => ot,
        Err(McpError::NoToken) => return Check::NoConnector { at: now },
        Err(e) => return Check::error(now, &e),
    };
    match ot.day_summary(Some(&occ.day.to_string())).await {
        Ok(s) if !s.is_workday => Check::NotWorkday { at: now },
        Ok(s) if s.reached_target() => Check::Complete {
            at: now,
            total_minutes: s.total_minutes,
            target_minutes: s.target_minutes,
        },
        Ok(s) => Check::Missing {
            at: now,
            total_minutes: s.total_minutes,
            target_minutes: s.target_minutes,
            missing_minutes: s.missing_minutes(),
            notified: false,
        },
        Err(e) => {
            tracing::warn!(code = %e.code(), "verificador do OptTime: {e}");
            Check::error(now, &e)
        }
    }
}

/// Guarda a conferência na tarefa e age conforme o modo da rotina. Avisa
/// pelo Windows quando falta, uma vez por tarefa e só quando quem conferiu
/// foi o agendador (quem tocou em "Conferir" já está vendo).
fn settle(
    app: &AppHandle,
    store: &AssistStore,
    occ: &Occurrence,
    mut check: Check,
    by_user: bool,
) -> anyhow::Result<Check> {
    if let Check::Missing {
        notified,
        missing_minutes,
        ..
    } = &mut check
    {
        let already = matches!(last_check(occ), Some(Check::Missing { notified: true, .. }));
        if !already && !by_user {
            notify_missing(app, store, occ, *missing_minutes);
        }
        *notified = true;
    }
    store.set_occurrence_check(&occ.task.id, serde_json::to_value(&check)?)?;
    if occ.routine.mode == RoutineMode::Auto && occ.task.status == TaskStatus::Open {
        match check {
            Check::Complete { .. } => {
                store.set_status(&occ.task.id, TaskStatus::Done, Actor::Routine)?;
            }
            Check::NotWorkday { .. } => {
                store.set_status(&occ.task.id, TaskStatus::Dropped, Actor::Routine)?;
            }
            _ => {}
        }
    }
    let _ = app.emit(TASKS_EVENT, ());
    Ok(check)
}

/// "3h20", "40min".
fn hm(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}min"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h{m:02}"),
    }
}

fn notify_missing(app: &AppHandle, store: &AssistStore, occ: &Occurrence, missing: i64) {
    let line1 = if occ.day == store.clock().today() {
        crate::i18n::trv(
            app,
            "notify.routine-missing-today",
            &[("missing", hm(missing))],
        )
    } else {
        crate::i18n::trv(
            app,
            "notify.routine-missing-day",
            &[
                ("missing", hm(missing)),
                ("day", occ.day.format("%d/%m").to_string()),
            ],
        )
    };
    let line2 = crate::i18n::tr(app, "notify.routine-missing-2");
    let app2 = app.clone();
    let shown = notify::show(
        notify::Toast {
            title: &occ.task.title,
            line1: &line1,
            line2: Some(&line2),
            silent: false,
        },
        move || open_today(&app2),
    );
    if let Err(e) = shown {
        tracing::warn!("aviso da rotina indisponível: {e}");
    }
}

/// Se é hora de conferir: rotina com verificador, já passou da hora da
/// tarefa (as de dias anteriores, sempre, até onde o OptTime ainda sugere) e
/// a última conferência tem mais de meia hora. "Sem token" confere de novo
/// na volta seguinte: guardar o token não espera meia hora.
fn due(occ: &Occurrence, today: NaiveDate, now: i64, clock: &dyn Clock) -> bool {
    if occ.routine.verifier.as_deref() != Some(VERIFY_OPTTIME_DAY)
        || (today - occ.day).num_days() > SUGGEST_MAX_DAYS
    {
        return false;
    }
    if occ.day >= today
        && let Some(time) = occ.task.planned_time.as_deref()
        && let Ok(t) = chrono::NaiveTime::parse_from_str(time, "%H:%M")
    {
        let (start, _) = clock.day_bounds(today);
        let at = start + i64::from(chrono::Timelike::num_seconds_from_midnight(&t)) * 1000;
        if now < at {
            return false;
        }
    }
    match last_check(occ) {
        None | Some(Check::NoConnector { .. }) => true,
        Some(c) => now - c.at() >= RECHECK_MS,
    }
}

/// Uma volta do agendador: o checklist do dia e as conferências vencidas.
fn tick(app: &AppHandle) -> anyhow::Result<()> {
    let store = open_assist()?;
    let today = store.clock().today();
    if !store.materialize_routines(today)?.is_empty() {
        let _ = app.emit(TASKS_EVENT, ());
    }
    let now = store.clock().now_ms();
    let pending: Vec<Occurrence> = store
        .open_occurrences(today)?
        .into_iter()
        .filter(|o| due(o, today, now, store.clock()))
        .collect();
    for occ in pending {
        let check = tauri::async_runtime::block_on(probe(app, &occ, now));
        settle(app, &store, &occ, check, false)?;
    }
    Ok(())
}

/// Liga o agendador das rotinas (uma thread, uma volta por minuto).
pub(crate) fn start(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("isper-rotinas".into())
        .spawn(move || {
            // Deixa o app terminar de abrir antes da primeira consulta.
            std::thread::sleep(Duration::from_secs(5));
            loop {
                if let Err(e) = tick(&app) {
                    tracing::warn!("rotinas: {e:#}");
                }
                std::thread::sleep(TICK);
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("agendador das rotinas não subiu: {e}");
    }
}

fn store() -> Result<AssistStore, String> {
    open_assist().map_err(|e| e.to_string())
}

/// Todas as rotinas, ligadas primeiro.
#[tauri::command]
pub(crate) fn routines_list() -> Result<Vec<Routine>, String> {
    store()?.routines().map_err(|e| e.to_string())
}

/// Cria uma rotina; se ela cai hoje, a tarefa de hoje já aparece.
#[tauri::command]
pub(crate) fn routine_create(app: AppHandle, routine: NewRoutine) -> Result<Routine, String> {
    let store = store()?;
    let created = store
        .create_routine(routine, Actor::User)
        .map_err(|e| e.to_string())?;
    store
        .materialize_routines(store.clock().today())
        .map_err(|e| e.to_string())?;
    let _ = app.emit(TASKS_EVENT, ());
    Ok(created)
}

/// Muda título, recorrência, verificador, ação ou modo. A tarefa de hoje,
/// se ainda aberta, acompanha o título e a hora novos; se a rotina passou a
/// cair hoje, a tarefa aparece.
#[tauri::command]
pub(crate) fn routine_update(
    app: AppHandle,
    id: String,
    patch: RoutinePatch,
) -> Result<Routine, String> {
    let store = store()?;
    let routine = store
        .update_routine(&id, patch, Actor::User)
        .map_err(|e| e.to_string())?;
    follow_routine(&store, &routine).map_err(|e| e.to_string())?;
    let _ = app.emit(TASKS_EVENT, ());
    Ok(routine)
}

fn follow_routine(store: &AssistStore, routine: &Routine) -> anyhow::Result<()> {
    let today = store.clock().today();
    for occ in store.open_occurrences(today)? {
        if occ.routine.id != routine.id || occ.day != today {
            continue;
        }
        let time = routine.rule.time_hhmm();
        let patch = isper_assist::TaskPatch {
            title: (occ.task.title != routine.title).then(|| routine.title.clone()),
            planned_time: (occ.task.planned_time != time).then_some(time),
            ..isper_assist::TaskPatch::default()
        };
        if patch != isper_assist::TaskPatch::default() {
            store.update_task(&occ.task.id, patch, Actor::User)?;
        }
    }
    if routine.active {
        store.materialize_routines(today)?;
    }
    Ok(())
}

/// Pausa ou retoma. Retomada, a tarefa de hoje aparece se a rotina cai hoje.
#[tauri::command]
pub(crate) fn routine_set_active(
    app: AppHandle,
    id: String,
    active: bool,
) -> Result<Routine, String> {
    let store = store()?;
    let routine = store
        .set_routine_active(&id, active, Actor::User)
        .map_err(|e| e.to_string())?;
    if active {
        store
            .materialize_routines(store.clock().today())
            .map_err(|e| e.to_string())?;
    }
    let _ = app.emit(TASKS_EVENT, ());
    Ok(routine)
}

/// "Conferir agora" numa tarefa de rotina.
#[tauri::command]
pub(crate) async fn routine_check(app: AppHandle, task_id: String) -> Result<Check, String> {
    let (occ, now) = {
        let store = store()?;
        let occ = store.occurrence(&task_id).map_err(|e| e.to_string())?;
        (occ, store.clock().now_ms())
    };
    let check = probe(&app, &occ, now).await;
    let store = store()?;
    settle(&app, &store, &occ, check, true).map_err(|e| e.to_string())
}

/// O dia de uma tarefa de rotina, se o OptTime ainda sugere para ele.
fn suggest_day(store: &AssistStore, occ: &Occurrence) -> Result<NaiveDate, ConnectorError> {
    let age = (store.clock().today() - occ.day).num_days();
    if !(0..=SUGGEST_MAX_DAYS).contains(&age) {
        return Err(ConnectorError {
            code: "OUT_OF_RANGE".into(),
            message: format!("o OptTime só sugere de hoje até {SUGGEST_MAX_DAYS} dias para trás"),
            hint: None,
        });
    }
    Ok(occ.day)
}

/// As sugestões do "Preencher meu dia" para o dia da tarefa de rotina.
#[tauri::command]
pub(crate) async fn opttime_day_suggestions(
    app: AppHandle,
    task_id: String,
) -> Result<Suggestions, ConnectorError> {
    let day = {
        let store = open_assist().map_err(local_error)?;
        let occ = store.occurrence(&task_id).map_err(local_error)?;
        suggest_day(&store, &occ)?
    };
    let ot = opttime(&app).map_err(|e| ConnectorError::from(&e))?;
    ot.suggest(Some(&day.to_string()))
        .await
        .map_err(|e| ConnectorError::from(&e))
}

/// Chaves de idempotência por tarefa, até o lançamento dar certo. Repetir o
/// mesmo pedido depois de uma queda reusa a chave, e o OptTime devolve o que
/// já gravou em vez de duplicar; mudar a seleção depois de um pedido que
/// chegou lá vira `IDEMPOTENCY_CONFLICT`, e não lançamento em dobro. O
/// conflito solta a chave: a conferência seguinte mostra o dia como ficou.
static PENDING_KEYS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Mutex::default);

/// O que "Lançar no OptTime" fez.
#[derive(Debug, serde::Serialize)]
pub(crate) struct ApplyOutcome {
    applied: Applied,
    check: Check,
    completed: bool,
}

/// Lança as sugestões aprovadas no OptTime (o toque do usuário), registra
/// no diário, confere o dia de novo e, se a meta fechou, conclui a tarefa.
#[tauri::command]
pub(crate) async fn opttime_day_apply(
    app: AppHandle,
    task_id: String,
    items: Vec<ApplyItem>,
    rejected: Vec<String>,
) -> Result<ApplyOutcome, ConnectorError> {
    let (occ, day) = {
        let store = open_assist().map_err(local_error)?;
        let occ = store.occurrence(&task_id).map_err(local_error)?;
        let day = suggest_day(&store, &occ)?;
        (occ, day)
    };
    let fresh = ApplyRequest::new(day.to_string(), items, rejected);
    let key = PENDING_KEYS
        .lock_or_recover()
        .entry(task_id.clone())
        .or_insert_with(|| fresh.idempotency_key.clone())
        .clone();
    let request = ApplyRequest {
        idempotency_key: key,
        ..fresh
    };
    let ot = opttime(&app).map_err(|e| ConnectorError::from(&e))?;
    let applied = match ot.apply(&request).await {
        Ok(applied) => applied,
        Err(e) => {
            if e.code() == "IDEMPOTENCY_CONFLICT" {
                PENDING_KEYS.lock_or_recover().remove(&task_id);
            }
            return Err(ConnectorError::from(&e));
        }
    };
    PENDING_KEYS.lock_or_recover().remove(&task_id);

    let now = chrono::Utc::now().timestamp_millis();
    let check = probe(&app, &occ, now).await;
    let store = open_assist().map_err(local_error)?;
    let summary = crate::i18n::trv(
        &app,
        "routines.journal.applied",
        &[
            ("n", applied.created_entry_ids.len().to_string()),
            ("total", hm(applied.day_total_minutes)),
        ],
    );
    store
        .record(
            Actor::User,
            "opttime.applied",
            "opttime",
            &day.to_string(),
            &summary,
            &json!({
                "task_id": task_id,
                "idempotencyKey": request.idempotency_key,
                "items": request.items,
                "rejected": request.rejected_suggestion_ids,
                "createdEntryIds": applied.created_entry_ids,
                "dayTotalMinutes": applied.day_total_minutes,
                "replayed": applied.replayed,
            }),
        )
        .map_err(local_error)?;
    let check = settle(&app, &store, &occ, check, true).map_err(local_error)?;
    // Fechou a meta com o toque de lançar: o mesmo toque conclui a tarefa
    // (no automático, o `settle` já concluiu).
    let completed = matches!(check, Check::Complete { .. });
    if completed
        && store
            .task(&task_id)
            .is_ok_and(|t| t.status == TaskStatus::Open)
    {
        store
            .set_status(&task_id, TaskStatus::Done, Actor::User)
            .map_err(local_error)?;
    }
    let _ = app.emit(TASKS_EVENT, ());
    Ok(ApplyOutcome {
        applied,
        check,
        completed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use isper_assist::{FixedClock, NewTask};

    fn occurrence(day: &str, time: Option<&str>, check: Option<Check>) -> Occurrence {
        let r = NewRoutine::opttime_hours("Registrar 8h");
        let rule = isper_assist::Rule::parse(&r.rrule).unwrap();
        let routine = Routine {
            id: "r".into(),
            title: r.title,
            rrule: r.rrule,
            rule,
            verifier: r.verifier,
            action: r.action,
            mode: r.mode,
            active: true,
            learned_from: None,
            created_at: 0,
            updated_at: 0,
        };
        let d = NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap();
        let mut source = json!({ "routine_id": "r", "day": day });
        if let Some(c) = check {
            source["check"] = serde_json::to_value(c).unwrap();
        }
        let new = NewTask {
            planned_on: Some(d),
            planned_time: time.map(String::from),
            ..NewTask::titled("Registrar 8h")
        };
        let task = isper_assist::Task {
            id: "t".into(),
            title: new.title,
            notes: String::new(),
            status: TaskStatus::Open,
            planned_on: new.planned_on,
            planned_time: new.planned_time,
            due_on: None,
            priority: 0,
            area: None,
            source_kind: isper_assist::SourceKind::Routine,
            source_ref: Some(source),
            external_ref: None,
            routine_id: Some("r".into()),
            position: 0.0,
            created_at: 0,
            updated_at: 0,
            completed_at: None,
        };
        Occurrence {
            task,
            routine,
            day: d,
        }
    }

    #[test]
    fn confere_so_depois_da_hora_e_de_meia_em_meia_hora() {
        let clock = FixedClock::at("2026-10-08 16:59", -3);
        let today = clock.today();
        let occ = occurrence("2026-10-08", Some("17:00"), None);
        assert!(!due(&occ, today, clock.now_ms, &clock), "antes das 17:00");
        let five = clock.plus_minutes(1);
        assert!(due(&occ, today, five.now_ms, &five), "às 17:00");

        let checked = Check::Missing {
            at: five.now_ms,
            total_minutes: 280,
            target_minutes: 480,
            missing_minutes: 200,
            notified: true,
        };
        let occ = occurrence("2026-10-08", Some("17:00"), Some(checked));
        let later = five.plus_minutes(29);
        assert!(!due(&occ, today, later.now_ms, &later));
        let later = five.plus_minutes(30);
        assert!(due(&occ, today, later.now_ms, &later));
    }

    #[test]
    fn a_de_ontem_confere_a_qualquer_hora() {
        let clock = FixedClock::at("2026-10-09 08:00", -3);
        let occ = occurrence("2026-10-08", Some("17:00"), None);
        assert!(due(&occ, clock.today(), clock.now_ms, &clock));
    }

    #[test]
    fn sem_token_confere_na_volta_seguinte_e_velha_demais_nao_confere() {
        let clock = FixedClock::at("2026-10-08 18:00", -3);
        let sem_token = Check::NoConnector {
            at: clock.now_ms - 60_000,
        };
        let occ = occurrence("2026-10-08", Some("17:00"), Some(sem_token));
        assert!(due(&occ, clock.today(), clock.now_ms, &clock));
        let velha = occurrence("2026-09-01", Some("17:00"), None);
        assert!(
            !due(&velha, clock.today(), clock.now_ms, &clock),
            "o OptTime só sugere até 30 dias para trás"
        );
    }

    #[test]
    fn conferencia_vai_e_volta_do_json() {
        let c = Check::Error {
            at: 1,
            code: "MICROSOFT_NOT_CONNECTED".into(),
            message: "m".into(),
            hint: Some("h".into()),
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(serde_json::from_value::<Check>(v).unwrap(), c);
        let antigo = json!({ "status": "missing", "at": 1, "total_minutes": 1, "target_minutes": 2, "missing_minutes": 1 });
        assert!(matches!(
            serde_json::from_value::<Check>(antigo).unwrap(),
            Check::Missing {
                notified: false,
                ..
            }
        ));
    }

    #[test]
    fn horas_legiveis() {
        assert_eq!(hm(200), "3h20");
        assert_eq!(hm(40), "40min");
        assert_eq!(hm(480), "8h");
    }
}
