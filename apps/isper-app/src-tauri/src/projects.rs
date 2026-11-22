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
use isper_core::store::MeetingRow;
use std::collections::HashMap;

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

// ------------------------------------------------ a tela Projetos (11.2)

/// Vai para a tela Projetos.
/// `async`: criar a janela na thread principal é proibido no Windows.
#[tauri::command]
pub(crate) async fn open_projects_window(app: AppHandle) -> Result<(), String> {
    navigate(&app, Route::to("projects"));
    Ok(())
}

/// As reuniões de um grupo da tela Projetos: um projeto ou "sem projeto".
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub(crate) struct MeetingStats {
    meetings: i64,
    meeting_secs: f64,
    /// Quando foi a mais recente (`dd/mm/aaaa hh:mm`).
    last_meeting: Option<String>,
}

/// Um projeto na lista da tela Projetos.
#[derive(Debug, serde::Serialize)]
pub(crate) struct ProjectCard {
    project: Project,
    #[serde(flatten)]
    stats: MeetingStats,
    open_tasks: i64,
}

/// A lista da tela Projetos: os ativos, os arquivados e as reuniões sem projeto.
#[derive(Debug, serde::Serialize)]
pub(crate) struct ProjectsOverview {
    active: Vec<ProjectCard>,
    archived: Vec<ProjectCard>,
    no_project: MeetingStats,
    total_meetings: i64,
}

/// O grupo de cada reunião: o projeto dela, se ele existe; senão, "sem
/// projeto" (`None`). Toda reunião cai em exatamente um grupo.
fn bucket_of(row: &MeetingRow, known: &[&str]) -> Option<String> {
    row.project_id
        .as_deref()
        .filter(|id| known.contains(id))
        .map(String::from)
}

/// As contas de cada grupo; `rows` vem da mais recente para a mais antiga.
fn group_meetings(rows: &[MeetingRow], known: &[&str]) -> HashMap<Option<String>, MeetingStats> {
    let mut out: HashMap<Option<String>, MeetingStats> = HashMap::new();
    for r in rows {
        let s = out.entry(bucket_of(r, known)).or_default();
        s.meetings += 1;
        s.meeting_secs += f64::from(r.duration_secs);
        s.last_meeting.get_or_insert_with(|| r.started_at.clone());
    }
    out
}

/// As reuniões, sem as que estão saindo (janela do Desfazer aberta).
fn visible_meetings() -> anyhow::Result<Vec<MeetingRow>> {
    let mut rows = open_store()?.list_meetings()?;
    let hidden = crate::undo::hidden_meetings();
    rows.retain(|r| !hidden.contains(&r.id));
    Ok(rows)
}

/// A lista da tela Projetos.
#[tauri::command]
pub(crate) fn projects_overview() -> Result<ProjectsOverview, String> {
    (|| {
        let assist = open_assist()?;
        let (active, archived) = (assist.projects(false)?, assist.projects(true)?);
        let tasks: HashMap<String, i64> = assist.open_task_counts()?.into_iter().collect();
        let rows = visible_meetings()?;
        let known: Vec<&str> = active
            .iter()
            .chain(&archived)
            .map(|p| p.id.as_str())
            .collect();
        let mut groups = group_meetings(&rows, &known);
        let mut card = |project: Project| ProjectCard {
            stats: groups.remove(&Some(project.id.clone())).unwrap_or_default(),
            open_tasks: tasks.get(&project.id).copied().unwrap_or(0),
            project,
        };
        let active: Vec<ProjectCard> = active.into_iter().map(&mut card).collect();
        let archived: Vec<ProjectCard> = archived.into_iter().map(&mut card).collect();
        Ok(ProjectsOverview {
            active,
            archived,
            no_project: groups.remove(&None).unwrap_or_default(),
            total_meetings: rows.len() as i64,
        })
    })()
    .map_err(|e: anyhow::Error| e.to_string())
}

/// Uma reunião na página do projeto, com o começo do resumo.
#[derive(Debug, serde::Serialize)]
pub(crate) struct PageMeeting {
    #[serde(flatten)]
    meeting: MeetingRow,
    summary_excerpt: Option<String>,
}

/// O que foi validado no Copilot numa reunião do projeto.
#[derive(Debug, serde::Serialize)]
pub(crate) struct PageDecision {
    meeting_id: i64,
    meeting_title: String,
    started_at: String,
    #[serde(flatten)]
    decision: isper_core::store::StoredDecision,
}

/// A página de um projeto (ou de "sem projeto", com `project_id` nulo).
#[derive(Debug, serde::Serialize)]
pub(crate) struct ProjectPage {
    project: Option<Project>,
    stats: MeetingStats,
    meetings: Vec<PageMeeting>,
    decisions: Vec<PageDecision>,
    tasks: Vec<isper_assist::Task>,
}

/// Quantas decisões validadas a página mostra, das mais recentes.
const PAGE_DECISIONS: usize = 30;
/// Tamanho do começo do resumo em cada reunião.
const EXCERPT_CHARS: usize = 280;

/// O primeiro parágrafo do resumo: o que vem debaixo de "Resumo", ou o
/// primeiro que não é título. Encurtado em [`EXCERPT_CHARS`].
fn summary_excerpt(summary: &str) -> Option<String> {
    let lines: Vec<&str> = summary.lines().map(str::trim).collect();
    let start = lines
        .iter()
        .position(|l| {
            l.trim_start_matches('#')
                .trim()
                .eq_ignore_ascii_case("resumo")
        })
        .map_or(0, |i| i + 1);
    let para: Vec<&str> = lines[start..]
        .iter()
        .skip_while(|l| l.is_empty())
        .take_while(|l| !l.is_empty() && !l.starts_with('#'))
        .copied()
        .collect();
    let text = para.join(" ");
    let text = text.trim_start_matches(['-', '*', ' ']).trim();
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= EXCERPT_CHARS {
        return Some(text.to_string());
    }
    let cut: String = text.chars().take(EXCERPT_CHARS).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    Some(format!("{}…", cut.trim_end_matches([',', ';', ':', ' '])))
}

/// A página do projeto: as reuniões (mais recentes primeiro, com o começo do
/// resumo), o que se validou nelas e as tarefas abertas. Sem `project_id`, a
/// de "sem projeto", para arrumar as reuniões antigas.
#[tauri::command]
pub(crate) fn project_page(project_id: Option<String>) -> Result<ProjectPage, String> {
    (|| {
        let assist = open_assist()?;
        let all: Vec<Project> = assist
            .projects(false)?
            .into_iter()
            .chain(assist.projects(true)?)
            .collect();
        let known: Vec<&str> = all.iter().map(|p| p.id.as_str()).collect();
        let want = project_id
            .as_deref()
            .filter(|id| known.contains(id))
            .map(String::from);
        if let (Some(asked), None) = (&project_id, &want) {
            anyhow::bail!("o projeto {asked} não existe");
        }
        let store = open_store()?;
        let rows: Vec<MeetingRow> = visible_meetings()?
            .into_iter()
            .filter(|r| bucket_of(r, &known) == want)
            .collect();
        let stats = group_meetings(&rows, &known)
            .remove(&want)
            .unwrap_or_default();
        let mut decisions = Vec::new();
        for r in rows.iter().filter(|r| r.decisions > 0) {
            if decisions.len() >= PAGE_DECISIONS {
                break;
            }
            for d in store.decisions(r.id)? {
                decisions.push(PageDecision {
                    meeting_id: r.id,
                    meeting_title: r.title.clone(),
                    started_at: r.started_at.clone(),
                    decision: d,
                });
            }
        }
        decisions.truncate(PAGE_DECISIONS);
        let meetings = rows
            .into_iter()
            .map(|m| {
                let summary_excerpt = if m.has_summary {
                    store
                        .meeting_summary(m.id)?
                        .as_deref()
                        .and_then(summary_excerpt)
                } else {
                    None
                };
                Ok(PageMeeting {
                    meeting: m,
                    summary_excerpt,
                })
            })
            .collect::<anyhow::Result<_>>()?;
        let tasks = match &want {
            Some(id) => assist.open_tasks_in_project(id)?,
            None => Vec::new(),
        };
        Ok(ProjectPage {
            project: all.into_iter().find(|p| Some(&p.id) == want.as_ref()),
            stats,
            meetings,
            decisions,
            tasks,
        })
    })()
    .map_err(|e: anyhow::Error| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, project: Option<&str>, at: &str, secs: f32) -> MeetingRow {
        MeetingRow {
            id,
            title: format!("R{id}"),
            started_at: at.into(),
            duration_secs: secs,
            segments: 0,
            participants: 0,
            has_summary: false,
            md_path: None,
            moments: 0,
            decisions: 0,
            source_name: None,
            project_id: project.map(String::from),
        }
    }

    #[test]
    fn toda_reuniao_cai_em_exatamente_um_grupo() {
        let rows = [
            row(5, Some("a"), "21/11/2026 10:00", 600.0),
            row(4, Some("arq"), "20/11/2026 10:00", 60.0),
            row(3, None, "19/11/2026 10:00", 60.0),
            row(2, Some("sumiu"), "18/11/2026 10:00", 60.0),
            row(1, Some("a"), "17/11/2026 10:00", 300.0),
        ];
        let g = group_meetings(&rows, &["a", "arq"]);
        assert_eq!(
            g.values().map(|s| s.meetings).sum::<i64>(),
            rows.len() as i64
        );
        let a = &g[&Some("a".to_string())];
        assert_eq!((a.meetings, a.meeting_secs), (2, 900.0));
        assert_eq!(a.last_meeting.as_deref(), Some("21/11/2026 10:00"));
        assert_eq!(g[&None].meetings, 2, "sem projeto e projeto que não existe");
    }

    #[test]
    fn o_comeco_do_resumo_e_o_paragrafo_de_resumo() {
        let s = "## Resumo\nFechamos o escopo\ndo portal.\n\n## Pontos principais\n- x";
        assert_eq!(
            summary_excerpt(s).as_deref(),
            Some("Fechamos o escopo do portal.")
        );
        assert_eq!(
            summary_excerpt("Só um texto.").as_deref(),
            Some("Só um texto.")
        );
        assert_eq!(summary_excerpt("## Resumo\n\n## Outro"), None);
        let long = format!("## Resumo\n{}", "palavra ".repeat(80));
        let cut = summary_excerpt(&long).unwrap();
        assert!(
            cut.ends_with('…') && cut.chars().count() <= EXCERPT_CHARS + 1,
            "{cut}"
        );
    }
}
