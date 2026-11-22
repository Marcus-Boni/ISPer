//! Os projetos na tela (Fase 11.1, [ADR 0025]): cuidar deles em
//! Configurações → Projetos, trazer do OptTime os que a pessoa escolher e
//! marcar o projeto de uma reunião na Biblioteca.
//!
//! Toda mudança avisa as telas pelo evento [`PROJECTS_EVENT`]; a da reunião
//! também pelo `isper-tasks`, porque as tarefas dela acompanham o projeto.
//!
//! [ADR 0025]: ../../../../docs/adr/0025-quando-der-tempo-e-projetos-locais.md

use crate::connectors::{ConnectorError, opttime};
use crate::prelude::*;
use crate::today::{TASKS_EVENT, open_assist};
use isper_assist::{Actor, LinkedProject, MeetingProject, NewProject, Project, ProjectPatch};

/// Evento que avisa as telas de que os projetos (ou o de uma reunião) mudaram.
pub(crate) const PROJECTS_EVENT: &str = "isper-projects";

fn changed<T>(app: &AppHandle, out: anyhow::Result<T>) -> Result<T, String> {
    let out = out.map_err(|e| e.to_string())?;
    let _ = app.emit(PROJECTS_EVENT, ());
    Ok(out)
}

/// Os projetos ativos, ou os arquivados.
#[tauri::command]
pub(crate) fn projects_list(archived: bool) -> Result<Vec<Project>, String> {
    (|| Ok(open_assist()?.projects(archived)?))().map_err(|e: anyhow::Error| e.to_string())
}

/// Cria um projeto.
#[tauri::command]
pub(crate) fn project_create(app: AppHandle, project: NewProject) -> Result<Project, String> {
    changed(
        &app,
        (|| Ok(open_assist()?.create_project(project, Actor::User)?))(),
    )
}

/// Muda o nome, a cor ou os apelidos.
#[tauri::command]
pub(crate) fn project_update(
    app: AppHandle,
    id: String,
    patch: ProjectPatch,
) -> Result<Project, String> {
    changed(
        &app,
        (|| Ok(open_assist()?.update_project(&id, patch, Actor::User)?))(),
    )
}

/// Arquiva (sai das escolhas, o que já foi marcado fica) ou traz de volta.
#[tauri::command]
pub(crate) fn project_set_archived(
    app: AppHandle,
    id: String,
    archived: bool,
) -> Result<Project, String> {
    changed(
        &app,
        (|| Ok(open_assist()?.set_project_archived(&id, archived, Actor::User)?))(),
    )
}

/// Os projetos do OptTime em que a pessoa aponta horas, para ela escolher
/// quais trazer. A tela sabe quais já vieram pelo `external_ref`.
#[tauri::command]
pub(crate) async fn opttime_projects(
    app: AppHandle,
) -> Result<isper_mcp::ProjectList, ConnectorError> {
    let ot = opttime(&app).map_err(|e| ConnectorError::from(&e))?;
    ot.projects().await.map_err(|e| ConnectorError::from(&e))
}

/// Traz os escolhidos: liga o local de mesmo nome ou cria; trazer de novo
/// não duplica.
#[tauri::command]
pub(crate) fn projects_link(
    app: AppHandle,
    items: Vec<LinkedProject>,
) -> Result<Vec<Project>, String> {
    changed(
        &app,
        (|| Ok(open_assist()?.link_projects(items, Actor::User)?))(),
    )
}

/// O projeto de uma reunião, e se a marca ainda é só uma sugestão.
#[tauri::command]
pub(crate) fn meeting_project_get(meeting_id: i64) -> Result<MeetingProject, String> {
    (|| Ok(open_assist()?.meeting_project(meeting_id)?))().map_err(|e: anyhow::Error| e.to_string())
}

/// Marca, troca, confirma (o mesmo de novo) ou tira o projeto da reunião.
#[tauri::command]
pub(crate) fn meeting_project_set(
    app: AppHandle,
    meeting_id: i64,
    project_id: Option<String>,
) -> Result<MeetingProject, String> {
    let out = changed(
        &app,
        (|| {
            Ok(open_assist()?.set_meeting_project(
                meeting_id,
                project_id.as_deref(),
                Actor::User,
            )?)
        })(),
    )?;
    let _ = app.emit(TASKS_EVENT, ());
    Ok(out)
}

/// Depois que o título da reunião assenta (a IA, o nome do arquivo): o
/// projeto cujo nome ou apelido aparece nele, se a reunião ainda não tem e a
/// pessoa não decidiu. Devolve `false` só quando vale perguntar: a reunião
/// ficou sem projeto e há projetos para escolher. Um erro aqui
/// não pode derrubar o salvar: vira aviso no log.
pub(crate) fn suggest_from_title(app: &AppHandle, meeting_id: i64, title: &str) -> bool {
    let done = open_assist().and_then(|s| {
        if let Some(p) = s.suggest_project_from_title(meeting_id, title)? {
            tracing::info!(meeting_id, project = %p.name, "projeto sugerido pelo título");
            let _ = app.emit(PROJECTS_EVENT, ());
            let _ = app.emit(TASKS_EVENT, ());
        }
        // Sem projeto nenhum cadastrado, não há o que perguntar.
        Ok(s.meeting_project(meeting_id)?.project_id.is_some() || s.projects(false)?.is_empty())
    });
    done.unwrap_or_else(|e| {
        tracing::warn!(
            meeting_id,
            "não consegui sugerir o projeto pelo título: {e}"
        );
        // Sem saber, a notificação não pergunta.
        true
    })
}
