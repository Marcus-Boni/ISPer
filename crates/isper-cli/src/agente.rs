//! `isper-cli agente`: o agente do ISPer pela linha de comando (Fase 10.4).
//!
//! - `semear --db <novo.db>` monta um banco **sintético** (tarefas, uma
//!   rotina, duas reuniões e um ditado inventados), para testar o agente
//!   contra as APIs de verdade sem mandar dado real a ninguém;
//! - `perguntar "<texto>"` roda o agente sobre um banco (o do app, por
//!   padrão) e mostra cada ferramenta chamada, a resposta e as fontes. Ações
//!   que pedem o toque são recusadas, a não ser com `--aprovar`;
//! - `avaliar <perguntas.jsonl>` mede um conjunto de perguntas: se a
//!   resposta traz o que devia e cita fontes do tipo certo, e quanto demora.
//!
//! As perguntas sobre dias reais (o critério da 10.4) moram fora do
//! repositório, como o corpus da 10.1; `testdata/agente-perguntas.jsonl`
//! vale para o banco sintético.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, bail};
use chrono::{Duration, Local};
use isper_agent::{CLOSING, IsperHost, MORNING, OptTimeTools, system_prompt};
use isper_assist::{Actor, AssistStore, NewRoutine, NewTask, SourceKind, TaskStatus};
use isper_core::store::{MeetingStore, NewImportedMeeting};
use isper_llm::LlmProvider;
use isper_llm::agent::{Conversation, Step, ToolHost, ToolOutcome, advance, resume};
use isper_mcp::Endpoint;
use isper_mcp::opttime::{DEFAULT_URL, SECRET_NAME};
use serde::Deserialize;
use serde_json::{Value, json};

/// Monta o banco sintético em `db` (que não pode existir).
pub fn semear(db: &Path) -> anyhow::Result<()> {
    if db.exists() {
        bail!("{} já existe; escolha um arquivo novo", db.display());
    }
    let meetings = MeetingStore::open(db)?;
    let now = Local::now();
    let today = now.date_naive();
    let yesterday = today - Duration::days(1);
    let stamp = |d: chrono::NaiveDate, hm: &str| format!("{} {hm}", d.format("%d/%m/%Y"));
    let seg = |s: &str, a: f32, b: f32, t: &str| (s.to_string(), a, b, t.to_string());

    let daily = meetings.save_imported(&NewImportedMeeting {
        title: "Daily do Portal do Cliente",
        started_at: &stamp(yesterday, "09:00"),
        duration_secs: 1500.0,
        md_path: "",
        segments: &[
            seg(
                "Ana",
                30.0,
                38.0,
                "Bom dia. Ontem fechei a tela de pedidos.",
            ),
            seg(
                "Eu",
                40.0,
                52.0,
                "Eu terminei o filtro por unidade, falta revisar com o time.",
            ),
            seg(
                "Ana",
                754.6,
                760.0,
                "Marcus, você me manda a planilha de estimativas até sexta?",
            ),
            seg("Eu", 760.2, 762.0, "Mando sim, deixa comigo."),
            seg(
                "Carlos",
                900.0,
                910.0,
                "O deploy do portal ficou para quinta, depois do teste de carga.",
            ),
        ],
        source_name: "sintetico-daily.ogg",
        source_sha256: "sintetico-daily",
    })?;
    meetings.set_summary(
        daily,
        "## Resumo\nA equipe revisou o andamento do Portal do Cliente: a tela de pedidos está pronta e o filtro por unidade falta revisar. O deploy ficou para quinta.\n\n## Action items\n- [ ] Mandar a planilha de estimativas — Marcus\n- [ ] Teste de carga — Carlos\n\n## Decisões\n- Deploy na quinta, depois do teste de carga.",
    )?;
    meetings.save_imported(&NewImportedMeeting {
        title: "Refinamento do backlog",
        started_at: &stamp(today, "08:30"),
        duration_secs: 1800.0,
        md_path: "",
        segments: &[
            seg(
                "Eu",
                10.0,
                20.0,
                "Vamos priorizar o filtro de pedidos por status.",
            ),
            seg(
                "Joana",
                300.0,
                312.0,
                "O item 4512 precisa de critério de aceite antes de entrar na sprint.",
            ),
            seg(
                "Eu",
                320.0,
                330.0,
                "Fechado: o 4512 só entra com critério de aceite.",
            ),
        ],
        source_name: "sintetico-refino.ogg",
        source_sha256: "sintetico-refino",
    })?;
    meetings.save_dictation(
        &format!("{} 18:10:00", yesterday.format("%d/%m/%Y")),
        "lembrar de pedir o acesso ao Azure para a Joana",
        None,
        4.0,
        0.5,
    )?;
    drop(meetings);

    let assist = AssistStore::open(db)?;
    assist.create_task(
        NewTask {
            planned_on: Some(today),
            planned_time: Some("10:00".into()),
            ..NewTask::titled("Revisar o contrato da Marca Ambiental")
        },
        Actor::User,
    )?;
    assist.create_task(
        NewTask {
            planned_on: Some(yesterday),
            ..NewTask::titled("Atualizar o README do portal")
        },
        Actor::User,
    )?;
    assist.create_task(NewTask::titled("Ler o ADR da sincronia"), Actor::User)?;
    assist.create_task(
        NewTask {
            status: TaskStatus::Inbox,
            source_kind: SourceKind::Meeting,
            source_ref: Some(json!({"meeting_id": daily, "at_secs": 754.6, "meeting_title": "Daily do Portal do Cliente"})),
            due_on: Some(today + Duration::days(2)),
            ..NewTask::titled("Mandar a planilha de estimativas para a Ana")
        },
        Actor::Assistant,
    )?;
    let done = assist.create_task(
        NewTask {
            planned_on: Some(today),
            ..NewTask::titled("Responder o cliente sobre o deploy")
        },
        Actor::User,
    )?;
    assist.set_status(&done.id, TaskStatus::Done, Actor::User)?;
    assist.create_routine(
        NewRoutine::opttime_hours("Registrar 8h no OptTime"),
        Actor::User,
    )?;
    assist.materialize_routines(today)?;
    println!("banco sintético em {}", db.display());
    Ok(())
}

/// Um anfitrião que mostra cada ferramenta chamada.
struct Traced<'a> {
    inner: &'a IsperHost,
    verbose: bool,
}

impl ToolHost for Traced<'_> {
    fn tools(&self) -> Vec<isper_llm::ToolSpec> {
        self.inner.tools()
    }
    fn permission(&self, name: &str) -> isper_llm::agent::Permission {
        self.inner.permission(name)
    }
    fn call(&self, name: &str, arguments: &Value) -> ToolOutcome {
        let started = Instant::now();
        let out = self.inner.call(name, arguments);
        if self.verbose {
            eprintln!(
                "  → {name}({arguments}) {}{:.1} s",
                if out.is_error { "ERRO " } else { "" },
                started.elapsed().as_secs_f32()
            );
        }
        out
    }
    fn describe(&self, name: &str, arguments: &Value) -> String {
        self.inner.describe(name, arguments)
    }
}

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("runtime do tokio")
}

/// O anfitrião sobre `db`, com o OptTime se houver token e não for pedido sem.
fn host(db: &Path, rt: &tokio::runtime::Runtime, opttime: bool) -> IsperHost {
    let ot = if opttime {
        isper_llm::get_api_key(SECRET_NAME)
            .ok()
            .flatten()
            .and_then(|token| Endpoint::new(DEFAULT_URL, &token).ok())
            .and_then(
                |endpoint| match OptTimeTools::connect(endpoint, rt.handle().clone()) {
                    Ok(t) => Some(t),
                    Err(e) => {
                        eprintln!("(OptTime fora: {e})");
                        None
                    }
                },
            )
    } else {
        None
    };
    IsperHost::new(db, ot, HashMap::new())
}

/// O que uma rodada respondeu, e quanto levou.
struct Answered {
    text: String,
    sources: Vec<isper_llm::agent::Source>,
    secs: f32,
    refused: Vec<String>,
}

fn run(
    provider: &dyn LlmProvider,
    host: &dyn ToolHost,
    system: &str,
    question: &str,
    approve: bool,
) -> anyhow::Result<Answered> {
    let started = Instant::now();
    let mut conv = Conversation::new(question);
    let mut step = advance(provider, host, system, &mut conv)?;
    let mut refused = Vec::new();
    loop {
        match step {
            Step::Answer { text, sources } => {
                return Ok(Answered {
                    text,
                    sources,
                    secs: started.elapsed().as_secs_f32(),
                    refused,
                });
            }
            Step::Confirm { actions } => {
                let ids: Vec<String> = if approve {
                    actions.iter().map(|a| a.call.id.clone()).collect()
                } else {
                    refused.extend(actions.iter().map(|a| a.description.clone()));
                    Vec::new()
                };
                step = resume(provider, host, system, &mut conv, &ids)?;
            }
        }
    }
}

/// Faz uma pergunta ao agente.
#[allow(clippy::too_many_arguments)]
pub fn perguntar(
    db: &Path,
    question: &str,
    provider: Option<&str>,
    model: Option<&str>,
    approve: bool,
    opttime: bool,
    me: Option<&str>,
) -> anyhow::Result<()> {
    let question = match question {
        "manha" | "manhã" => MORNING,
        "fechamento" => CLOSING,
        q => q,
    };
    let provider = crate::tarefas::provider(provider, model)?;
    let rt = runtime()?;
    let inner = host(db, &rt, opttime);
    let traced = Traced {
        inner: &inner,
        verbose: true,
    };
    let system = system_prompt(me, Local::now(), inner.opttime().is_some());
    eprintln!("via {} ({})", provider.name(), provider.model());
    let a = run(provider.as_ref(), &traced, &system, question, approve)?;
    println!("{}", a.text.trim());
    for r in &a.refused {
        println!("  (recusada no CLI: {r})");
    }
    for s in &a.sources {
        println!("  [[{}]] {} · {}", s.reference, s.kind, s.label);
    }
    eprintln!("{:.1} s", a.secs);
    Ok(())
}

/// Uma pergunta do conjunto de avaliação.
#[derive(Debug, Deserialize)]
struct Case {
    pergunta: String,
    /// Trechos que a resposta precisa ter (sem diferença de caixa).
    #[serde(default)]
    deve_conter: Vec<String>,
    /// Tipos de fonte que precisam aparecer citados (`tarefa`, `reuniao`…).
    #[serde(default)]
    deve_citar: Vec<String>,
}

/// Mede o agente num conjunto de perguntas.
pub fn avaliar(
    db: &Path,
    cases: &Path,
    provider: Option<&str>,
    model: Option<&str>,
    opttime: bool,
    me: Option<&str>,
    out: Option<&Path>,
) -> anyhow::Result<()> {
    let provider = crate::tarefas::provider(provider, model)?;
    let rt = runtime()?;
    let inner = host(db, &rt, opttime);
    let traced = Traced {
        inner: &inner,
        verbose: false,
    };
    let system = system_prompt(me, Local::now(), inner.opttime().is_some());
    let file = std::fs::File::open(cases).with_context(|| cases.display().to_string())?;
    let mut report = Vec::new();
    let (mut ok, mut total) = (0, 0);
    let mut times = Vec::new();
    for line in std::io::BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let case: Case = serde_json::from_str(&line).with_context(|| line.clone())?;
        total += 1;
        let result = run(provider.as_ref(), &traced, &system, &case.pergunta, false);
        let (pass, detail) = match &result {
            Ok(a) => {
                times.push(a.secs);
                let lower = a.text.to_lowercase();
                let missing: Vec<&String> = case
                    .deve_conter
                    .iter()
                    .filter(|t| !lower.contains(&t.to_lowercase()))
                    .collect();
                let kinds: Vec<&str> = a.sources.iter().map(|s| s.kind.as_str()).collect();
                let uncited: Vec<&String> = case
                    .deve_citar
                    .iter()
                    .filter(|k| !kinds.contains(&k.as_str()))
                    .collect();
                let pass = missing.is_empty() && uncited.is_empty();
                (
                    pass,
                    json!({"resposta": a.text, "faltou": missing, "sem_citar": uncited, "fontes": kinds, "segundos": a.secs}),
                )
            }
            Err(e) => (false, json!({"erro": e.to_string()})),
        };
        if pass {
            ok += 1;
        }
        println!("{} {}", if pass { "OK   " } else { "FALHA" }, case.pergunta);
        if !pass {
            println!("      {detail}");
        }
        report.push(json!({"pergunta": case.pergunta, "passou": pass, "detalhe": detail}));
    }
    times.sort_by(f32::total_cmp);
    let p = |q: f32| {
        times
            .get(((times.len() as f32 - 1.0) * q).round() as usize)
            .copied()
            .unwrap_or(0.0)
    };
    println!(
        "\n{ok}/{total} certas · p50 {:.1} s · p95 {:.1} s · via {} ({})",
        p(0.5),
        p(0.95),
        provider.name(),
        provider.model()
    );
    if let Some(out) = out {
        let mut f = std::fs::File::create(out)?;
        writeln!(f, "{}", serde_json::to_string_pretty(&report)?)?;
    }
    Ok(())
}
