//! A agenda do Outlook no ISPer (Fase 10.3), lida pelo conector do OptTime.
//!
//! - **Na tela Hoje:** os eventos do dia, com o que está acontecendo agora,
//!   o link para entrar e o selo de "gravada" quando uma reunião do ISPer foi
//!   casada com o evento.
//! - **Preparo:** até 10 minutos antes de cada reunião, um aviso do Windows
//!   leva ao preparo na tela Hoje: quem organiza, o link e o que ficou da
//!   última vez (o resumo e as tarefas ainda abertas da reunião anterior da
//!   mesma série).
//! - **Casar a gravação:** ao encerrar uma reunião, o evento em que ela
//!   aconteceu dá o nome da reunião e fica ligado a ela no banco.
//!
//! A agenda tem cache de 5 minutos aqui (o OptTime guarda 60 s do lado
//! dele): a tela, o aviso e o casamento leem do mesmo lugar, e ler é livre
//! ([ADR 0023]).
//!
//! [ADR 0023]: ../../../../docs/adr/0023-escada-de-confianca.md

use std::collections::HashSet;
use std::sync::LazyLock;

use crate::connectors::{ConnectorError, opttime, token_present};
use crate::prelude::*;
use chrono::{DateTime, FixedOffset, NaiveDate};
use isper_core::store::{EventMeeting, MeetingEvent};
use isper_mcp::{Agenda, AgendaEvent, McpError, match_event};

/// Quanto a agenda lida vale.
const TTL: Duration = Duration::from_secs(5 * 60);
/// Quanto um erro de leitura vale (para não martelar o OptTime fora do ar).
const ERROR_TTL: Duration = Duration::from_secs(60);
/// A antecedência do aviso de preparo.
pub(crate) const PREP_LEAD_SECS: i64 = 10 * 60;
/// De quanto em quanto o vigia do preparo olha a agenda.
const PREP_TICK: Duration = Duration::from_secs(60);
/// Evento que leva a tela Hoje ao preparo de um evento.
pub(crate) const PREP_EVENT: &str = "isper-agenda-prep";

struct Cached {
    day: NaiveDate,
    at: Instant,
    agenda: Result<Agenda, ConnectorError>,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);
/// Os eventos que já tiveram aviso de preparo (iCalUId e início).
static PREPPED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

fn connector_error(e: &McpError) -> ConnectorError {
    ConnectorError::from(e)
}

/// A agenda de um dia, do cache quando ainda vale.
pub(crate) async fn agenda_for(
    app: &AppHandle,
    day: NaiveDate,
    force: bool,
) -> Result<Agenda, ConnectorError> {
    if !force && let Some(c) = CACHE.lock_or_recover().as_ref() {
        let ttl = if c.agenda.is_ok() { TTL } else { ERROR_TTL };
        if c.day == day && c.at.elapsed() < ttl {
            return c.agenda.clone();
        }
    }
    let agenda = match opttime(app) {
        Ok(ot) => ot
            .agenda(Some(&day.to_string()), 1)
            .await
            .map_err(|e| connector_error(&e)),
        Err(e) => Err(connector_error(&e)),
    };
    if let Err(e) = &agenda {
        tracing::warn!(code = %e.code, "agenda do OptTime: {}", e.message);
    }
    *CACHE.lock_or_recover() = Some(Cached {
        day,
        at: Instant::now(),
        agenda: agenda.clone(),
    });
    agenda
}

/// [`agenda_for`] fora do runtime assíncrono (threads do app).
pub(crate) fn agenda_blocking(
    app: &AppHandle,
    day: NaiveDate,
    force: bool,
) -> Result<Agenda, ConnectorError> {
    tauri::async_runtime::block_on(agenda_for(app, day, force))
}

/// Esquece a agenda guardada (o conector mudou).
pub(crate) fn forget() {
    *CACHE.lock_or_recover() = None;
}

/// Um evento como a tela Hoje mostra.
#[derive(Debug, serde::Serialize)]
pub(crate) struct EventView {
    #[serde(flatten)]
    event: AgendaEvent,
    /// A reunião do ISPer casada com o evento, se houve gravação.
    recorded: Option<i64>,
    /// É reunião de que a pessoa participa (não recusada, livre ou de dia inteiro).
    meeting: bool,
}

/// A agenda de hoje para a tela.
#[derive(Debug, serde::Serialize)]
pub(crate) struct AgendaView {
    connected: bool,
    day: String,
    events: Vec<EventView>,
    error: Option<ConnectorError>,
}

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

/// As reuniões gravadas casadas com eventos do dia: iCalUId e início → id.
fn recorded_on(day: NaiveDate) -> Vec<(i64, MeetingEvent)> {
    open_store()
        .and_then(|s| Ok(s.meeting_events_on(&day.to_string())?))
        .unwrap_or_default()
}

#[tauri::command]
pub(crate) async fn agenda_today(app: AppHandle, force: bool) -> AgendaView {
    let day = today();
    if !token_present() {
        return AgendaView {
            connected: false,
            day: day.to_string(),
            events: Vec::new(),
            error: None,
        };
    }
    match agenda_for(&app, day, force).await {
        Ok(agenda) => {
            let recorded = recorded_on(day);
            let events = agenda
                .events
                .into_iter()
                .map(|event| {
                    let found = recorded
                        .iter()
                        .find(|(_, e)| e.ical_uid == event.ical_uid && e.starts_at == event.start)
                        .or_else(|| recorded.iter().find(|(_, e)| e.ical_uid == event.ical_uid))
                        .map(|(id, _)| *id);
                    EventView {
                        meeting: event.is_meeting(),
                        recorded: found,
                        event,
                    }
                })
                .collect();
            AgendaView {
                connected: true,
                day: day.to_string(),
                events,
                error: None,
            }
        }
        Err(e) => AgendaView {
            connected: true,
            day: day.to_string(),
            events: Vec::new(),
            error: Some(e),
        },
    }
}

/// O que ficou da última vez, para o preparo de um evento.
#[derive(Debug, serde::Serialize)]
pub(crate) struct PrepView {
    /// A reunião anterior da mesma série (ou do mesmo evento).
    last: Option<EventMeeting>,
    /// O começo do resumo dela (o parágrafo de "## Resumo").
    lead: Option<String>,
    /// As tarefas ainda por fazer que saíram das reuniões anteriores.
    open_tasks: Vec<isper_assist::Task>,
}

/// O parágrafo da seção "## Resumo" de um resumo em Markdown.
fn summary_lead(md: &str) -> Option<String> {
    let mut in_summary = false;
    let mut out = String::new();
    for line in md.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            if in_summary && !out.is_empty() {
                break;
            }
            in_summary = t
                .trim_start_matches('#')
                .trim()
                .eq_ignore_ascii_case("resumo");
            continue;
        }
        if in_summary && !t.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(t);
        }
    }
    let out = out.trim().to_string();
    (!out.is_empty()).then(|| {
        if out.chars().count() > 400 {
            format!("{}…", out.chars().take(400).collect::<String>().trim_end())
        } else {
            out
        }
    })
}

/// O preparo de um evento: a reunião anterior da série e o que ficou aberto
/// das anteriores. A gravação deste mesmo evento (o mesmo início) não conta
/// como "última vez".
#[tauri::command]
pub(crate) fn agenda_prep(
    ical_uid: String,
    series_id: Option<String>,
    start: String,
) -> Result<PrepView, String> {
    let store = open_store().map_err(|e| e.to_string())?;
    let mine: Vec<i64> = store
        .meeting_events_on(start.get(..10).unwrap_or_default())
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|(_, e)| e.ical_uid == ical_uid && e.starts_at == start)
        .map(|(id, _)| id)
        .collect();
    let earlier: Vec<EventMeeting> = store
        .meetings_for_event(&ical_uid, series_id.as_deref().filter(|s| !s.is_empty()), 6)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|m| !mine.contains(&m.meeting_id))
        .collect();
    let ids: Vec<i64> = earlier.iter().map(|m| m.meeting_id).collect();
    let open_tasks = crate::today::open_assist()
        .and_then(|s| Ok(s.open_tasks_from_meetings(&ids)?))
        .map_err(|e| e.to_string())?;
    let last = earlier.into_iter().next();
    let lead = last
        .as_ref()
        .and_then(|m| m.summary.as_deref())
        .and_then(summary_lead);
    Ok(PrepView {
        last,
        lead,
        open_tasks,
    })
}

/// Abre um link de reunião no navegador ou no Teams. Só `https`, e sem
/// passar por um shell: o `&` de uma URL não vira comando.
#[tauri::command]
pub(crate) fn open_link(url: String) -> Result<(), String> {
    let parsed = url::Url::parse(url.trim()).map_err(|e| e.to_string())?;
    if parsed.scheme() != "https" {
        return Err("só links https".into());
    }
    std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", parsed.as_str()])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// O evento em que uma gravação aconteceu, pela agenda do dia em que ela
/// começou. Sem conector, ou com a agenda fora do ar, nenhum.
pub(crate) fn event_for_recording(
    app: &AppHandle,
    started: DateTime<FixedOffset>,
    ended: DateTime<FixedOffset>,
) -> Option<AgendaEvent> {
    if !token_present() {
        return None;
    }
    let agenda = agenda_blocking(app, started.date_naive(), false).ok()?;
    match_event(&agenda.events, started, ended).cloned()
}

/// O evento como o banco guarda.
pub(crate) fn stored_event(event: &AgendaEvent) -> MeetingEvent {
    MeetingEvent {
        ical_uid: event.ical_uid.clone(),
        series_id: event.series().map(String::from),
        subject: event.subject.clone(),
        starts_at: event.start.clone(),
        ends_at: event.end.clone(),
        data: serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
    }
}

/// Os eventos que começam em até [`PREP_LEAD_SECS`] e ainda não tiveram
/// aviso; marca-os como avisados.
fn due_for_prep(events: &[AgendaEvent], now: DateTime<FixedOffset>) -> Vec<AgendaEvent> {
    let mut seen = PREPPED.lock_or_recover();
    events
        .iter()
        .filter(|e| e.is_meeting())
        .filter(|e| {
            e.starts().is_some_and(|s| {
                let ahead = (s - now).num_seconds();
                ahead > 0 && ahead <= PREP_LEAD_SECS
            })
        })
        .filter(|e| seen.insert(format!("{}|{}", e.ical_uid, e.start)))
        .cloned()
        .collect()
}

fn notify_prep(app: &AppHandle, event: &AgendaEvent, now: DateTime<FixedOffset>) {
    let minutes = event
        .starts()
        .map(|s| ((s - now).num_seconds() + 59) / 60)
        .unwrap_or(10)
        .max(1);
    let title = crate::i18n::trv(
        app,
        "notify.prep-title",
        &[
            ("n", minutes.to_string()),
            ("subject", event.subject.clone()),
        ],
    );
    let hour = |s: &str| s.get(11..16).unwrap_or_default().to_string();
    let who = event
        .organizer
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .unwrap_or_default();
    let line1 = crate::i18n::trv(
        app,
        "notify.prep-line1",
        &[
            ("start", hour(&event.start)),
            ("end", hour(&event.end)),
            ("who", who),
        ],
    );
    let pending = agenda_prep(
        event.ical_uid.clone(),
        event.series().map(String::from),
        event.start.clone(),
    )
    .map(|p| p.open_tasks.len())
    .unwrap_or(0);
    let line2 = if pending > 0 {
        crate::i18n::trv(app, "notify.prep-pending", &[("n", pending.to_string())])
    } else {
        crate::i18n::tr(app, "notify.prep-open")
    };
    let app2 = app.clone();
    let payload = json!({ "ical_uid": event.ical_uid, "start": event.start });
    let shown = notify::show(
        notify::Toast {
            title: &title,
            line1: &line1,
            line2: Some(&line2),
            silent: false,
        },
        move || {
            open_today(&app2);
            let _ = app2.emit(PREP_EVENT, payload.clone());
        },
    );
    if let Err(e) = shown {
        tracing::warn!("aviso de preparo indisponível: {e}");
    }
}

/// O vigia do preparo: uma thread que olha a agenda de minuto em minuto.
pub(crate) fn start_prep_watch(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("isper-agenda".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(20));
            loop {
                let on = app
                    .state::<AppState>()
                    .config
                    .lock_or_recover()
                    .meeting_prep;
                if on && token_present() {
                    let now = chrono::Local::now().fixed_offset();
                    if let Ok(agenda) = agenda_blocking(&app, now.date_naive(), false) {
                        for event in due_for_prep(&agenda.events, now) {
                            notify_prep(&app, &event, now);
                        }
                    }
                }
                std::thread::sleep(PREP_TICK);
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("vigia do preparo não subiu: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evento(uid: &str, start: &str, status: &str) -> AgendaEvent {
        serde_json::from_value(json!({
            "id": uid, "iCalUId": uid, "seriesMasterId": "serie", "type": "occurrence",
            "subject": "Daily", "start": start, "end": "2026-10-09T23:59:00-03:00",
            "durationMinutes": 30, "isAllDay": false, "isOnline": true, "joinUrl": null,
            "organizer": { "name": "Ana", "email": null }, "isOrganizer": false,
            "responseStatus": status, "attendeeCount": 3, "attendees": [], "showAs": "busy",
            "sensitivity": "normal", "loggedMinutes": 0, "attendance": null
        }))
        .unwrap()
    }

    #[test]
    fn preparo_avisa_uma_vez_nos_dez_minutos_antes() {
        let now = DateTime::parse_from_rfc3339("2026-10-09T08:50:30-03:00").unwrap();
        let eventos = vec![
            evento("prep-agora", "2026-10-09T09:00:00-03:00", "accepted"),
            evento("prep-cedo", "2026-10-09T09:30:00-03:00", "accepted"),
            evento("prep-passou", "2026-10-09T08:45:00-03:00", "accepted"),
            evento("prep-recusado", "2026-10-09T08:55:00-03:00", "declined"),
        ];
        let due: Vec<String> = due_for_prep(&eventos, now)
            .into_iter()
            .map(|e| e.ical_uid)
            .collect();
        assert_eq!(due, ["prep-agora"]);
        assert!(due_for_prep(&eventos, now).is_empty(), "uma vez só");
    }

    #[test]
    fn primeiro_paragrafo_do_resumo() {
        let md =
            "## Resumo\nA equipe revisou o portal.\nFicou para sexta.\n\n## Pontos principais\n- x";
        assert_eq!(
            summary_lead(md).as_deref(),
            Some("A equipe revisou o portal. Ficou para sexta.")
        );
        assert_eq!(summary_lead("sem seções"), None);
    }

    #[test]
    fn so_https_abre() {
        assert!(open_link("http://example.com".into()).is_err());
        assert!(open_link("javascript:alert(1)".into()).is_err());
        assert!(open_link("file:///C:/Windows/notepad.exe".into()).is_err());
    }
}
