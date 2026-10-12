//! As listas "Quando der tempo" na tela Hoje (Fase 11.0, [ADR 0025]): o que
//! não tem data, separado por lista, fora do dia e do número da barra.
//!
//! Os itens são tarefas: fazer hoje, voltar à lista, a estimativa, mudar de
//! lista, concluir e descartar passam pelos comandos de tarefa de sempre
//! (com o Desfazer). Aqui ficam as listas, o colar várias linhas, o "manter"
//! do item parado e a sugestão de tempo livre, lida da agenda.
//!
//! [ADR 0025]: ../../../../docs/adr/0025-quando-der-tempo-e-projetos-locais.md

use crate::prelude::*;
use crate::today::{TASKS_EVENT, TaskChange, changed, open_assist};
use chrono::{DateTime, FixedOffset, NaiveTime};
use isper_assist::spare::{FreeWindow, free_windows, pick};
use isper_assist::{Actor, NewTask, Task, TaskList, clean_line};

// shortcut: o fim do expediente é fixo às 18h; vira configuração (ou o
// expediente do Outlook) quando alguém trabalhar noutro horário.
const DAY_END: NaiveTime = match NaiveTime::from_hms_opt(18, 0, 0) {
    Some(t) => t,
    None => panic!("18:00 é uma hora válida"),
};

fn list_changed<T>(app: &AppHandle, out: anyhow::Result<T>) -> Result<T, String> {
    let out = out.map_err(|e| e.to_string())?;
    let _ = app.emit(TASKS_EVENT, ());
    Ok(out)
}

/// Cria uma lista ("Estudar", "Ler").
#[tauri::command]
pub(crate) fn list_create(app: AppHandle, name: String) -> Result<TaskList, String> {
    list_changed(
        &app,
        (|| Ok(open_assist()?.create_list(&name, Actor::User)?))(),
    )
}

/// Renomeia uma lista.
#[tauri::command]
pub(crate) fn list_rename(app: AppHandle, id: String, name: String) -> Result<TaskList, String> {
    list_changed(
        &app,
        (|| Ok(open_assist()?.rename_list(&id, &name, Actor::User)?))(),
    )
}

/// Arquiva (sai da tela, nada se apaga) ou restaura (o Desfazer do aviso).
#[tauri::command]
pub(crate) fn list_set_archived(
    app: AppHandle,
    id: String,
    archived: bool,
) -> Result<TaskList, String> {
    list_changed(
        &app,
        (|| Ok(open_assist()?.set_list_archived(&id, archived, Actor::User)?))(),
    )
}

/// Um item na lista; várias linhas (a lista copiada de outro lugar) viram um
/// item por linha. A estimativa vale para o item digitado. Devolve quantos
/// itens entraram.
#[tauri::command]
pub(crate) fn list_add(
    app: AppHandle,
    list_id: String,
    text: String,
    estimate_min: Option<u16>,
) -> Result<usize, String> {
    list_changed(
        &app,
        (|| {
            let store = open_assist()?;
            if text.lines().filter(|l| clean_line(l).is_some()).count() > 1 {
                return Ok(store.paste_into_list(&list_id, &text, Actor::User)?.len());
            }
            let title = clean_line(&text).ok_or_else(|| anyhow::anyhow!("item vazio"))?;
            let new = NewTask {
                estimate_min,
                ..NewTask::titled(title)
            };
            store.add_to_list(&list_id, new, Actor::User)?;
            Ok(1)
        })(),
    )
}

/// Mantém um item parado: conta como mudança (e vai para o diário).
#[tauri::command]
pub(crate) fn list_keep(app: AppHandle, id: String) -> Result<TaskChange, String> {
    let store = open_assist().map_err(|e| e.to_string())?;
    let task = store
        .keep_item(&id, Actor::User)
        .map_err(|e| e.to_string())?;
    changed(&app, &store, task)
}

/// Um item que cabe num tempo livre da agenda.
#[derive(Debug, serde::Serialize)]
pub(crate) struct SpareView {
    window: FreeWindow,
    /// A janela é a de agora (e não uma mais tarde).
    starts_now: bool,
    task: Task,
    list: String,
}

/// O item para o próximo tempo livre de hoje: a primeira janela da agenda
/// (de agora ao fim do expediente) em que cabe algum item das listas. Sem o
/// OptTime ou sem agenda, nada. `skip` são os recusados ("Outra"); `day_end`
/// troca o fim do expediente (os e2e passam outro).
#[tauri::command]
pub(crate) async fn spare_suggest(
    app: AppHandle,
    skip: Vec<String>,
    day_end: Option<NaiveTime>,
) -> Result<Option<SpareView>, String> {
    if !crate::connectors::token_present() {
        return Ok(None);
    }
    let now = chrono::Local::now().fixed_offset();
    let Ok(agenda) = crate::agenda::agenda_for(&app, now.date_naive(), false).await else {
        return Ok(None);
    };
    let busy: Vec<(DateTime<FixedOffset>, DateTime<FixedOffset>)> = agenda
        .events
        .iter()
        .filter(|e| e.is_meeting())
        .filter_map(|e| {
            let span = DateTime::parse_from_rfc3339(&e.start)
                .and_then(|s| Ok((s, DateTime::parse_from_rfc3339(&e.end)?)));
            if span.is_err() {
                tracing::warn!(start = %e.start, end = %e.end, "evento com hora que não se lê");
            }
            span.ok()
        })
        .collect();
    let end = day_end.unwrap_or(DAY_END);
    let windows = free_windows(&busy, now, end);
    if windows.is_empty() {
        return Ok(None);
    }
    let store = open_assist().map_err(|e| e.to_string())?;
    let items = store.spare_items().map_err(|e| e.to_string())?;
    let lists = store.lists(false).map_err(|e| e.to_string())?;
    Ok(windows.into_iter().find_map(|window| {
        let task = pick(&items, window.minutes, &skip)?.clone();
        let list = lists
            .iter()
            .find(|l| Some(&l.id) == task.list_id.as_ref())
            .map(|l| l.name.clone())
            .unwrap_or_default();
        Some(SpareView {
            starts_now: window.start == now,
            window,
            task,
            list,
        })
    }))
}
