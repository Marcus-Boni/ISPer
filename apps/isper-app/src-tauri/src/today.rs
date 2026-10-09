//! A tela Hoje (Fase 10, [ADR 0021]): as tarefas do dia sobre o `isper-assist`.
//!
//! Cada comando abre o domínio, faz uma coisa e solta a conexão, como a
//! Biblioteca faz com o `MeetingStore`. Toda mudança devolve o id da linha do
//! diário que a registrou: é o que o "Desfazer" do aviso usa. Depois de mudar,
//! o evento `isper-tasks` avisa as janelas (a barra lateral relê o número).
//!
//! [ADR 0021]: ../../../../docs/adr/0021-assistente-pessoal-no-isper.md

use crate::prelude::*;
use isper_assist::when::{ParsedTask, parse_task};
use isper_assist::{
    Actor, AssistStore, Clock, NewTask, Routine, SystemClock, Task, TaskPatch, TaskStatus, Today,
};

/// Evento que avisa as janelas de que as tarefas mudaram.
pub(crate) const TASKS_EVENT: &str = "isper-tasks";

/// Uma tarefa depois de mudar, com o id da mudança no diário (para desfazer).
#[derive(Debug, serde::Serialize)]
pub(crate) struct TaskChange {
    task: Task,
    undo: Option<i64>,
}

/// O domínio do assistente sobre o `isper.db` do perfil. O `MeetingStore`
/// abre antes porque é ele quem migra o banco (a v7 é da cadeia dele).
pub(crate) fn open_assist() -> anyhow::Result<AssistStore> {
    let path = db_path()?;
    drop(isper_core::store::MeetingStore::open(&path)?);
    Ok(AssistStore::open(&path)?)
}

fn changed(app: &AppHandle, store: &AssistStore, task: Task) -> Result<TaskChange, String> {
    let undo = store.last_change(&task.id).map_err(|e| e.to_string())?;
    let _ = app.emit(TASKS_EVENT, ());
    Ok(TaskChange { task, undo })
}

/// O dia como a tela Hoje mostra: as tarefas, as rotinas (para as tarefas
/// de rotina saberem o verificador e o modo) e se o OptTime tem token.
#[derive(Debug, serde::Serialize)]
pub(crate) struct TodayView {
    #[serde(flatten)]
    today: Today,
    routines: Vec<Routine>,
    opttime: bool,
}

/// O checklist do dia antes de montar o dia: as rotinas que caem hoje ganham
/// a tarefa sem esperar o agendador (que dá a volta de minuto em minuto).
fn today_with_routines(app: &AppHandle, store: &AssistStore) -> anyhow::Result<Today> {
    if !store
        .materialize_routines(store.clock().today())?
        .is_empty()
    {
        let _ = app.emit(TASKS_EVENT, ());
    }
    Ok(store.today()?)
}

/// O dia: para hoje (e atrasadas), caixa de entrada, depois e feito hoje.
#[tauri::command]
pub(crate) fn today_load(app: AppHandle) -> Result<TodayView, String> {
    (|| {
        let store = open_assist()?;
        Ok(TodayView {
            today: today_with_routines(&app, &store)?,
            routines: store.routines()?,
            opttime: crate::connectors::token_present(),
        })
    })()
    .map_err(|e: anyhow::Error| e.to_string())
}

/// Quantas tarefas abertas são para hoje (ou estão atrasadas): o número ao
/// lado de "Hoje" na barra lateral.
#[tauri::command]
pub(crate) fn today_badge(app: AppHandle) -> Result<usize, String> {
    open_assist()
        .and_then(|s| Ok(today_with_routines(&app, &s)?.planned.len()))
        .map_err(|e| e.to_string())
}

/// Cria uma tarefa digitada.
#[tauri::command]
pub(crate) fn task_add(app: AppHandle, task: NewTask) -> Result<TaskChange, String> {
    let store = open_assist().map_err(|e| e.to_string())?;
    let created = store
        .create_task(task, Actor::User)
        .map_err(|e| e.to_string())?;
    changed(&app, &store, created)
}

/// Como o texto digitado vai virar tarefa: título sem as datas, dia, hora e
/// prazo (a prévia da tela Hoje e o item "Criar tarefa" do Ctrl+K).
#[tauri::command]
pub(crate) fn task_parse(text: String) -> ParsedTask {
    parse_task(&text, SystemClock.today())
}

/// Cria uma tarefa a partir do texto digitado, com as datas tiradas dele
/// ("amanhã às 3 ligar pro João"). Sem dia no texto, vale `fallback` (o
/// "para quando" escolhido na tela, ou hoje pelo Ctrl+K).
#[tauri::command]
pub(crate) fn task_add_text(
    app: AppHandle,
    text: String,
    fallback: Option<chrono::NaiveDate>,
) -> Result<TaskChange, String> {
    let parsed = parse_task(&text, SystemClock.today());
    let task = NewTask {
        planned_on: parsed.planned_on.or(fallback),
        planned_time: parsed.planned_time,
        due_on: parsed.due_on,
        ..NewTask::titled(parsed.title)
    };
    let store = open_assist().map_err(|e| e.to_string())?;
    let created = store
        .create_task(task, Actor::User)
        .map_err(|e| e.to_string())?;
    changed(&app, &store, created)
}

/// Muda título, notas, dia, hora, prazo, prioridade ou área.
#[tauri::command]
pub(crate) fn task_update(
    app: AppHandle,
    id: String,
    patch: TaskPatch,
) -> Result<TaskChange, String> {
    let store = open_assist().map_err(|e| e.to_string())?;
    let task = store
        .update_task(&id, patch, Actor::User)
        .map_err(|e| e.to_string())?;
    changed(&app, &store, task)
}

/// Conclui, reabre, descarta ou devolve à caixa de entrada.
#[tauri::command]
pub(crate) fn task_set_status(
    app: AppHandle,
    id: String,
    status: TaskStatus,
) -> Result<TaskChange, String> {
    let store = open_assist().map_err(|e| e.to_string())?;
    let task = store
        .set_status(&id, status, Actor::User)
        .map_err(|e| e.to_string())?;
    changed(&app, &store, task)
}

/// Aceita uma tarefa da caixa de entrada para um dia (ou sem dia).
#[tauri::command]
pub(crate) fn task_accept(
    app: AppHandle,
    id: String,
    planned_on: Option<chrono::NaiveDate>,
) -> Result<TaskChange, String> {
    let store = open_assist().map_err(|e| e.to_string())?;
    let task = store
        .accept(&id, planned_on, Actor::User)
        .map_err(|e| e.to_string())?;
    changed(&app, &store, task)
}

/// Desfaz a mudança `change` do diário (o "Desfazer" do aviso e o Ctrl+Z).
#[tauri::command]
pub(crate) fn task_undo(app: AppHandle, change: i64) -> Result<TaskChange, String> {
    let store = open_assist().map_err(|e| e.to_string())?;
    let task = store.undo(change, Actor::User).map_err(|e| e.to_string())?;
    changed(&app, &store, task)
}

/// Vai para a tela Hoje.
/// `async`: criar a janela na thread principal é proibido no Windows.
#[tauri::command]
pub(crate) async fn open_today_window(app: AppHandle) -> Result<(), String> {
    open_today(&app);
    Ok(())
}
