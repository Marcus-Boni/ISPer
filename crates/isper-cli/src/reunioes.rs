//! `isper-cli reunioes acoes <id>`: as ações de "Eu" numa reunião já gravada
//! (Fase 10.3), como a caixa de entrada as receberia, sem gravar nada.
//!
//! Serve para medir a extração em reuniões reais antes de confiar nela: lê a
//! reunião do banco (só leitura), manda o texto da transcrição ao provider
//! configurado (o mesmo que já faz o resumo) e mostra cada ação com o minuto
//! conferido, o prazo dito e a data que ele vira.

use std::path::Path;
use std::time::Instant;

use anyhow::Context;
use chrono::NaiveDate;
use isper_assist::meeting::{MeetingAction, MeetingRef, drafts, merge};
use isper_core::store::MeetingStore;
use isper_llm::meeting_actions::clock;
use isper_llm::{TimedLine, extract_my_actions};

/// O dia de uma reunião a partir do `dd/mm/aaaa hh:mm` guardado.
fn meeting_day(started_at: &str) -> Option<NaiveDate> {
    let date = started_at.split_whitespace().next()?;
    NaiveDate::parse_from_str(date, "%d/%m/%Y").ok()
}

/// Extrai e mostra as ações de "Eu" na reunião `id`.
pub fn acoes(
    db: &Path,
    id: i64,
    provider: Option<&str>,
    model: Option<&str>,
    eu: Option<&str>,
) -> anyhow::Result<()> {
    let store = MeetingStore::open(db)?;
    let detail = store
        .get_meeting(id)?
        .with_context(|| format!("não há reunião {id} em {}", db.display()))?;
    let day = meeting_day(&detail.meeting.started_at)
        .unwrap_or_else(|| chrono::Local::now().date_naive());
    let lines: Vec<TimedLine> = detail
        .segments
        .iter()
        .map(|s| TimedLine {
            at_secs: s.start_secs,
            speaker: s.speaker.clone(),
            text: s.text.clone(),
        })
        .collect();
    println!(
        "{} ({}) · {} falas · {}",
        detail.meeting.title,
        detail.meeting.started_at,
        lines.len(),
        clock(detail.meeting.duration_secs)
    );
    let provider = crate::tarefas::provider(provider, model)?;
    println!("via {} ({})", provider.name(), provider.model());
    let started = Instant::now();
    let found = extract_my_actions(provider.as_ref(), &lines, eu)?;
    let secs = started.elapsed().as_secs_f32();

    let copilot: Vec<MeetingAction> = detail
        .decisions
        .iter()
        .filter(|d| d.kind == "action" && d.owner.as_deref().is_some_and(is_me))
        .map(|d| MeetingAction {
            title: d.title.clone(),
            at_secs: Some(d.at_secs),
            due: d.due_date.clone(),
            quote: None,
            from_copilot: true,
        })
        .collect();
    let extracted: Vec<MeetingAction> = found
        .into_iter()
        .map(|a| MeetingAction {
            title: a.title,
            at_secs: a.at_secs,
            due: a.due,
            quote: a.quote,
            from_copilot: false,
        })
        .collect();
    let actions = merge(copilot, extracted);
    let meeting = MeetingRef {
        id,
        title: detail.meeting.title.clone(),
        day,
    };
    let tasks = drafts(&meeting, &actions);
    println!("{} ação(ões) de \"Eu\" em {secs:.1} s:", actions.len());
    for (a, t) in actions.iter().zip(&tasks) {
        let at = a.at_secs.map(clock).unwrap_or_else(|| "--:--".into());
        let origem = if a.from_copilot { " [Copilot]" } else { "" };
        let prazo = match (&a.due, t.due_on) {
            (Some(p), Some(d)) => format!(" · prazo \"{p}\" → {d}"),
            (Some(p), None) => format!(" · prazo \"{p}\" (sem data)"),
            _ => String::new(),
        };
        println!("  [{at}] {}{prazo}{origem}", a.title);
        if let Some(q) = &a.quote {
            println!("         “{q}”");
        }
    }
    Ok(())
}

/// O dono que o Copilot escreve para quem gravou.
fn is_me(owner: &str) -> bool {
    matches!(owner.trim().to_lowercase().as_str(), "eu" | "me" | "i")
}
