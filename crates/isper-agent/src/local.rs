//! As ferramentas do próprio ISPer: tarefas, diário, reuniões, ditados e
//! rotinas, sobre o `isper.db`. Ler é livre; criar, mover e concluir tarefa
//! pedem um toque ([ADR 0023]).
//!
//! Cada item do resultado leva a sua referência (`tarefa:…`,
//! `reuniao:42@754`), que o modelo cita e a tela transforma em selo.
//!
//! [ADR 0023]: ../../../docs/adr/0023-escada-de-confianca.md

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use isper_assist::{Actor, AssistStore, Clock, NewTask, SystemClock, Task, TaskPatch, TaskStatus};
use isper_core::store::MeetingStore;
use isper_llm::ToolSpec;
use isper_llm::agent::{Permission, Source, ToolOutcome};
use serde_json::{Value, json};

/// Quantas tarefas de "depois" vão no resultado (o resto vira contagem).
const LATER_LIMIT: usize = 15;
/// Quantas reuniões uma listagem devolve.
const MEETINGS_LIMIT: usize = 20;
/// Trechos de transcrição por busca.
const EXCERPTS_LIMIT: usize = 20;
/// Transcrição sem busca nem resumo: até este tanto de texto.
const TRANSCRIPT_CHARS: usize = 12_000;

pub(crate) const TASKS: &str = "tarefas_do_dia";
pub(crate) const JOURNAL: &str = "diario";
pub(crate) const MEETINGS: &str = "reunioes";
pub(crate) const MEETING: &str = "reuniao";
pub(crate) const SEARCH: &str = "buscar";
pub(crate) const ROUTINES: &str = "rotinas";
pub(crate) const CREATE: &str = "criar_tarefa";
pub(crate) const MOVE: &str = "mover_tarefa";
pub(crate) const COMPLETE: &str = "concluir_tarefa";

/// As ferramentas locais sobre um banco.
pub(crate) struct LocalTools {
    db: PathBuf,
}

fn nullable(description: &str) -> Value {
    json!({"type": ["string", "null"], "description": description})
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn tool(name: &str, description: &str, input_schema: Value) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: description.into(),
        input_schema,
    }
}

fn day_arg(args: &Value, key: &str) -> Result<Option<NaiveDate>, String> {
    match args.get(key).and_then(Value::as_str).map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| format!("'{key}' precisa ser AAAA-MM-DD, veio '{s}'")),
    }
}

fn text_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn task_ref(t: &Task) -> String {
    format!("tarefa:{}", t.id)
}

fn task_source(t: &Task) -> Source {
    Source {
        reference: task_ref(t),
        kind: "tarefa".into(),
        label: t.title.clone(),
    }
}

fn task_json(t: &Task, today: NaiveDate) -> Value {
    let late = t.status == TaskStatus::Open && t.planned_on.is_some_and(|d| d < today);
    json!({
        "ref": task_ref(t),
        "titulo": t.title,
        "estado": t.status.as_str(),
        "dia": t.planned_on,
        "hora": t.planned_time,
        "prazo": t.due_on,
        "atrasada": late,
        "origem": t.source_kind.as_str(),
        "rotina": t.routine_id.is_some(),
    })
}

/// `12:34`, ou `1:02:10` com hora.
fn clock(secs: f32) -> String {
    isper_llm::meeting_actions::clock(secs)
}

fn meeting_ref(id: i64, at: Option<f32>) -> String {
    match at {
        Some(s) => format!("reuniao:{id}@{}", s.max(0.0) as u64),
        None => format!("reuniao:{id}"),
    }
}

/// Minúsculas e sem acento, para a busca dentro da transcrição.
fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

impl LocalTools {
    pub(crate) fn new(db: &Path) -> Self {
        Self {
            db: db.to_path_buf(),
        }
    }

    fn assist(&self) -> Result<AssistStore, String> {
        AssistStore::open(&self.db).map_err(|e| e.to_string())
    }

    fn meetings(&self) -> Result<MeetingStore, String> {
        MeetingStore::open(&self.db).map_err(|e| e.to_string())
    }

    pub(crate) fn specs() -> Vec<ToolSpec> {
        vec![
            tool(
                TASKS,
                "As tarefas de um dia na tela Hoje: para fazer (com as atrasadas), a caixa de entrada, as de depois e as concluídas no dia. Use para \"o que eu tenho hoje\", \"o que ficou pra trás\", \"o que eu concluí\".",
                object(
                    json!({"dia": nullable("Dia AAAA-MM-DD; null é hoje.")}),
                    &["dia"],
                ),
            ),
            tool(
                JOURNAL,
                "O diário de um dia: cada coisa que mudou, com a hora (tarefas criadas, concluídas, adiadas; rotinas; horas lançadas no OptTime). Use para \"o que eu fiz hoje/ontem\".",
                object(
                    json!({"dia": nullable("Dia AAAA-MM-DD; null é hoje.")}),
                    &["dia"],
                ),
            ),
            tool(
                MEETINGS,
                "As reuniões gravadas no ISPer num intervalo de dias, da mais recente para a mais antiga, opcionalmente filtradas por um texto do título ou da transcrição. Use para achar a reunião certa antes de abrir com `reuniao`.",
                object(
                    json!({
                        "de": nullable("Primeiro dia AAAA-MM-DD; null é 7 dias atrás."),
                        "ate": nullable("Último dia AAAA-MM-DD; null é hoje."),
                        "texto": nullable("Texto para filtrar (título ou o que foi dito); null lista todas."),
                    }),
                    &["de", "ate", "texto"],
                ),
            ),
            tool(
                MEETING,
                "Uma reunião gravada: resumo, decisões e ações validadas no Copilot, notas, o evento da agenda e, com `busca`, os trechos da transcrição que falam daquilo, com o minuto. Use para responder o que foi dito ou decidido.",
                object(
                    json!({
                        "id": {"type": "integer", "description": "O id da reunião (de `reunioes`)."},
                        "busca": nullable("Palavras para achar trechos da transcrição; null traz o resumo (ou o começo da transcrição, sem resumo)."),
                    }),
                    &["id", "busca"],
                ),
            ),
            tool(
                SEARCH,
                "Busca literal em todas as reuniões (título e transcrição) e nos ditados. Use quando não se sabe o dia.",
                object(
                    json!({"texto": {"type": "string", "description": "O que procurar."}}),
                    &["texto"],
                ),
            ),
            tool(
                ROUTINES,
                "As rotinas (o que se repete) e como está a de hoje de cada uma, inclusive a conferência das horas no OptTime.",
                object(json!({}), &[]),
            ),
            tool(
                CREATE,
                "Cria uma tarefa na tela Hoje. O ISPer pede a confirmação da pessoa antes; não pergunte em texto.",
                object(
                    json!({
                        "titulo": {"type": "string", "description": "O que fazer, começando pelo verbo."},
                        "dia": nullable("Dia AAAA-MM-DD; null é algum dia."),
                        "hora": nullable("Hora HH:MM; só com dia."),
                        "prazo": nullable("Prazo AAAA-MM-DD."),
                    }),
                    &["titulo", "dia", "hora", "prazo"],
                ),
            ),
            tool(
                MOVE,
                "Muda o dia de uma tarefa (de `tarefas_do_dia`). O ISPer pede a confirmação da pessoa antes.",
                object(
                    json!({
                        "ref": {"type": "string", "description": "A referência da tarefa (tarefa:…)."},
                        "dia": nullable("Novo dia AAAA-MM-DD; null é algum dia."),
                    }),
                    &["ref", "dia"],
                ),
            ),
            tool(
                COMPLETE,
                "Conclui uma tarefa (de `tarefas_do_dia`). O ISPer pede a confirmação da pessoa antes.",
                object(
                    json!({"ref": {"type": "string", "description": "A referência da tarefa (tarefa:…)."}}),
                    &["ref"],
                ),
            ),
        ]
    }

    pub(crate) fn is_mine(name: &str) -> bool {
        Self::specs().iter().any(|t| t.name == name)
    }

    pub(crate) fn permission(name: &str) -> Permission {
        match name {
            CREATE | MOVE | COMPLETE => Permission::Ask,
            _ => Permission::Allow,
        }
    }

    pub(crate) fn call(&self, name: &str, args: &Value) -> ToolOutcome {
        let result = match name {
            TASKS => self.tasks(args),
            JOURNAL => self.journal(args),
            MEETINGS => self.list_meetings(args),
            MEETING => self.meeting(args),
            SEARCH => self.search(args),
            ROUTINES => self.routines(),
            CREATE => self.create(args),
            MOVE => self.move_task(args),
            COMPLETE => self.complete(args),
            other => Err(format!("ferramenta desconhecida: {other}")),
        };
        result.unwrap_or_else(ToolOutcome::error)
    }

    fn ok(value: Value, sources: Vec<Source>) -> Result<ToolOutcome, String> {
        Ok(ToolOutcome {
            content: value.to_string(),
            is_error: false,
            sources,
        })
    }

    fn tasks(&self, args: &Value) -> Result<ToolOutcome, String> {
        let store = self.assist()?;
        let today = store.clock().today();
        let day = day_arg(args, "dia")?.unwrap_or(today);
        let d = store.today_for(day).map_err(|e| e.to_string())?;
        let mut sources = Vec::new();
        let mut list = |tasks: &[Task]| -> Vec<Value> {
            tasks
                .iter()
                .map(|t| {
                    sources.push(task_source(t));
                    task_json(t, today)
                })
                .collect()
        };
        let planned = list(&d.planned);
        let inbox = list(&d.inbox);
        let later = list(&d.later[..d.later.len().min(LATER_LIMIT)]);
        let done = list(&d.done_today);
        Self::ok(
            json!({
                "dia": day,
                "para_fazer": planned,
                "caixa_de_entrada": inbox,
                "depois": later,
                "depois_total": d.later.len(),
                "concluidas_no_dia": done,
            }),
            sources,
        )
    }

    fn journal(&self, args: &Value) -> Result<ToolOutcome, String> {
        let store = self.assist()?;
        let day = day_arg(args, "dia")?.unwrap_or_else(|| store.clock().today());
        let clock = store.clock();
        let mut sources = Vec::new();
        let entries: Vec<Value> = store
            .journal_for_day(day)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|e| {
                let at = chrono::DateTime::from_timestamp_millis(e.at).map(|t| {
                    t.with_timezone(&clock.offset_at(e.at))
                        .format("%H:%M")
                        .to_string()
                });
                let mut item = json!({
                    "hora": at,
                    "quem": e.actor,
                    "acao": e.action,
                    "objeto": e.object_kind,
                    "resumo": e.summary,
                });
                if e.object_kind == "task" {
                    let reference = format!("tarefa:{}", e.object_id);
                    if !sources.iter().any(|s: &Source| s.reference == reference) {
                        sources.push(Source {
                            reference: reference.clone(),
                            kind: "tarefa".into(),
                            label: e.summary.clone(),
                        });
                    }
                    item["ref"] = json!(reference);
                }
                item
            })
            .collect();
        let reference = format!("diario:{day}");
        sources.push(Source {
            reference: reference.clone(),
            kind: "diario".into(),
            label: format!("Diário de {}", day.format("%d/%m")),
        });
        Self::ok(
            json!({"dia": day, "ref": reference, "mudancas": entries}),
            sources,
        )
    }

    fn meeting_day(started_at: &str) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(started_at.split_whitespace().next()?, "%d/%m/%Y").ok()
    }

    fn list_meetings(&self, args: &Value) -> Result<ToolOutcome, String> {
        let store = self.meetings()?;
        let today = SystemClock.today();
        let to = day_arg(args, "ate")?.unwrap_or(today);
        let from = day_arg(args, "de")?.unwrap_or(to - chrono::Duration::days(7));
        let rows = match text_arg(args, "texto") {
            Some(q) => store.search_meetings(q),
            None => store.list_meetings(),
        }
        .map_err(|e| e.to_string())?;
        let mut sources = Vec::new();
        let items: Vec<Value> = rows
            .into_iter()
            .filter(|m| Self::meeting_day(&m.started_at).is_some_and(|d| d >= from && d <= to))
            .take(MEETINGS_LIMIT)
            .map(|m| {
                sources.push(Source {
                    reference: meeting_ref(m.id, None),
                    kind: "reuniao".into(),
                    label: m.title.clone(),
                });
                json!({
                    "ref": meeting_ref(m.id, None),
                    "id": m.id,
                    "titulo": m.title,
                    "inicio": m.started_at,
                    "duracao_min": (m.duration_secs / 60.0).round(),
                    "tem_resumo": m.has_summary,
                    "decisoes_validadas": m.decisions,
                })
            })
            .collect();
        Self::ok(json!({"de": from, "ate": to, "reunioes": items}), sources)
    }

    fn meeting(&self, args: &Value) -> Result<ToolOutcome, String> {
        let id = args
            .get("id")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            })
            .ok_or("'id' precisa ser o número da reunião")?;
        let store = self.meetings()?;
        let d = store
            .get_meeting(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("não há reunião {id}"))?;
        let mut sources = vec![Source {
            reference: meeting_ref(id, None),
            kind: "reuniao".into(),
            label: d.meeting.title.clone(),
        }];
        let decisions: Vec<Value> = d
            .decisions
            .iter()
            .map(|x| {
                json!({
                    "ref": meeting_ref(id, Some(x.at_secs)),
                    "tipo": x.kind,
                    "titulo": x.title,
                    "dono": x.owner,
                    "prazo": x.due_date,
                    "minuto": clock(x.at_secs),
                })
            })
            .collect();
        let mut out = json!({
            "ref": meeting_ref(id, None),
            "titulo": d.meeting.title,
            "inicio": d.meeting.started_at,
            "duracao_min": (d.meeting.duration_secs / 60.0).round(),
            "evento_da_agenda": d.event.as_ref().map(|e| json!({"assunto": e.subject, "inicio": e.starts_at})),
            "resumo": d.summary,
            "validadas_no_copilot": decisions,
            "notas": d.notes,
        });
        if let Some(q) = text_arg(args, "busca") {
            let words: Vec<String> = fold(q)
                .split_whitespace()
                .filter(|w| w.len() > 2)
                .map(String::from)
                .collect();
            let hits: Vec<Value> = d
                .segments
                .iter()
                .filter(|s| {
                    let t = fold(&s.text);
                    !words.is_empty() && words.iter().any(|w| t.contains(w.as_str()))
                })
                .take(EXCERPTS_LIMIT)
                .map(|s| {
                    let reference = meeting_ref(id, Some(s.start_secs));
                    sources.push(Source {
                        reference: reference.clone(),
                        kind: "reuniao".into(),
                        label: format!("{} · {}", d.meeting.title, clock(s.start_secs)),
                    });
                    json!({"ref": reference, "minuto": clock(s.start_secs), "quem": s.speaker, "texto": s.text})
                })
                .collect();
            out["trechos"] = json!(hits);
        } else if d.summary.is_none() {
            let mut size = 0;
            let excerpt: Vec<Value> = d
                .segments
                .iter()
                .take_while(|s| {
                    size += s.text.len();
                    size < TRANSCRIPT_CHARS
                })
                .map(|s| json!({"minuto": clock(s.start_secs), "quem": s.speaker, "texto": s.text}))
                .collect();
            out["transcricao_inicio"] = json!(excerpt);
        }
        Self::ok(out, sources)
    }

    fn search(&self, args: &Value) -> Result<ToolOutcome, String> {
        let q = text_arg(args, "texto").ok_or("'texto' vazio")?;
        let store = self.meetings()?;
        let mut sources = Vec::new();
        // A frase inteira primeiro; sem nada, cada palavra (as reuniões que
        // têm mais delas antes): "deploy portal" acha "o deploy do portal".
        let mut rows = store.search_meetings(q).map_err(|e| e.to_string())?;
        if rows.is_empty() {
            let mut hits: Vec<(usize, isper_core::store::MeetingRow)> = Vec::new();
            for word in q.split_whitespace().filter(|w| w.chars().count() > 2) {
                for m in store.search_meetings(word).map_err(|e| e.to_string())? {
                    match hits.iter_mut().find(|(_, h)| h.id == m.id) {
                        Some((n, _)) => *n += 1,
                        None => hits.push((1, m)),
                    }
                }
            }
            hits.sort_by_key(|h| std::cmp::Reverse(h.0));
            rows = hits.into_iter().map(|(_, m)| m).collect();
        }
        let meetings: Vec<Value> = rows
            .into_iter()
            .take(10)
            .map(|m| {
                sources.push(Source {
                    reference: meeting_ref(m.id, None),
                    kind: "reuniao".into(),
                    label: m.title.clone(),
                });
                json!({"ref": meeting_ref(m.id, None), "id": m.id, "titulo": m.title, "inicio": m.started_at})
            })
            .collect();
        let dictations: Vec<Value> = store
            .list_dictations(Some(q), 10)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|d| {
                let reference = format!("ditado:{}", d.id);
                sources.push(Source {
                    reference: reference.clone(),
                    kind: "ditado".into(),
                    label: format!("Ditado de {}", d.at.get(..16).unwrap_or(&d.at)),
                });
                json!({"ref": reference, "em": d.at, "texto": d.text})
            })
            .collect();
        Self::ok(
            json!({"reunioes": meetings, "ditados": dictations}),
            sources,
        )
    }

    fn routines(&self) -> Result<ToolOutcome, String> {
        let store = self.assist()?;
        let today = store.clock().today();
        let open = store.open_occurrences(today).map_err(|e| e.to_string())?;
        let mut sources = Vec::new();
        let items: Vec<Value> = store
            .routines()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|r| {
                let reference = format!("rotina:{}", r.id);
                sources.push(Source {
                    reference: reference.clone(),
                    kind: "rotina".into(),
                    label: r.title.clone(),
                });
                let todays = open.iter().find(|o| o.routine.id == r.id && o.day == today);
                json!({
                    "ref": reference,
                    "titulo": r.title,
                    "recorrencia": r.rrule,
                    "ligada": r.active,
                    "conferida_no_opttime": r.verifier.is_some(),
                    "hoje_aberta": todays.map(|o| task_ref(&o.task)),
                    "ultima_conferencia": todays.and_then(|o| o.task.source_ref.as_ref().and_then(|s| s.get("check")).cloned()),
                })
            })
            .collect();
        Self::ok(json!({"rotinas": items}), sources)
    }

    fn task_by_ref(store: &AssistStore, args: &Value) -> Result<Task, String> {
        let r = text_arg(args, "ref").ok_or("'ref' vazio")?;
        let id = r.strip_prefix("tarefa:").unwrap_or(r);
        store.task(id).map_err(|e| e.to_string())
    }

    fn create(&self, args: &Value) -> Result<ToolOutcome, String> {
        let store = self.assist()?;
        let title = text_arg(args, "titulo").ok_or("'titulo' vazio")?;
        let new = NewTask {
            planned_on: day_arg(args, "dia")?,
            planned_time: text_arg(args, "hora").map(String::from),
            due_on: day_arg(args, "prazo")?,
            source_ref: Some(json!({"assistente": true})),
            ..NewTask::titled(title)
        };
        let t = store
            .create_task(new, Actor::Assistant)
            .map_err(|e| e.to_string())?;
        let today = store.clock().today();
        Self::ok(
            json!({"criada": task_json(&t, today)}),
            vec![task_source(&t)],
        )
    }

    fn move_task(&self, args: &Value) -> Result<ToolOutcome, String> {
        let store = self.assist()?;
        let t = Self::task_by_ref(&store, args)?;
        let patch = TaskPatch {
            planned_on: Some(day_arg(args, "dia")?),
            ..TaskPatch::default()
        };
        let t = store
            .update_task(&t.id, patch, Actor::Assistant)
            .map_err(|e| e.to_string())?;
        let today = store.clock().today();
        Self::ok(
            json!({"movida": task_json(&t, today)}),
            vec![task_source(&t)],
        )
    }

    fn complete(&self, args: &Value) -> Result<ToolOutcome, String> {
        let store = self.assist()?;
        let t = Self::task_by_ref(&store, args)?;
        let t = store
            .set_status(&t.id, TaskStatus::Done, Actor::Assistant)
            .map_err(|e| e.to_string())?;
        let today = store.clock().today();
        Self::ok(
            json!({"concluida": task_json(&t, today)}),
            vec![task_source(&t)],
        )
    }

    /// A frase do cartão de confirmação.
    pub(crate) fn describe(&self, name: &str, args: &Value) -> String {
        let title_of = |args: &Value| {
            self.assist()
                .ok()
                .and_then(|s| Self::task_by_ref(&s, args).ok())
                .map(|t| t.title)
                .unwrap_or_else(|| text_arg(args, "ref").unwrap_or("?").to_string())
        };
        let day = |key: &str| {
            text_arg(args, key)
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                .map(|d| d.format("%d/%m").to_string())
        };
        match name {
            CREATE => {
                let mut s = format!(
                    "Criar a tarefa “{}”",
                    text_arg(args, "titulo").unwrap_or("?")
                );
                if let Some(d) = day("dia") {
                    s.push_str(&format!(" para {d}"));
                }
                if let Some(h) = text_arg(args, "hora") {
                    s.push_str(&format!(" às {h}"));
                }
                if let Some(p) = day("prazo") {
                    s.push_str(&format!(", prazo {p}"));
                }
                s
            }
            MOVE => match day("dia") {
                Some(d) => format!("Mover “{}” para {d}", title_of(args)),
                None => format!("Mover “{}” para algum dia", title_of(args)),
            },
            COMPLETE => format!("Concluir “{}”", title_of(args)),
            other => format!("Rodar {other}"),
        }
    }
}
