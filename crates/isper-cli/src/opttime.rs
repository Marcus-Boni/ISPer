//! `isper-cli opttime`: o conector do OptTime pela linha de comando, só
//! leitura (Fase 10.2). Escrever no OptTime fica no app, depois do toque
//! ([ADR 0023](../../../docs/adr/0023-escada-de-confianca.md)).
//!
//! O token é o do Credential Manager (`opttime.ISPer`); para guardar um,
//! `isper-cli llm set-key opttime`. Nada aqui imprime o token.

use anyhow::Context;
use isper_mcp::opttime::{DEFAULT_URL, SECRET_NAME};
use isper_mcp::{Endpoint, OptTime};

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("runtime do tokio")
}

fn endpoint(url: Option<&str>) -> anyhow::Result<Endpoint> {
    let token = isper_llm::get_api_key(SECRET_NAME)?.with_context(|| {
        format!("sem token do OptTime: rode `isper-cli llm set-key {SECRET_NAME}`")
    })?;
    Ok(Endpoint::new(url.unwrap_or(DEFAULT_URL), &token)?)
}

fn hours(minutes: i64) -> String {
    format!("{}h{:02}", minutes / 60, minutes % 60)
}

/// Quem é o dono do token e o que está conectado.
pub fn quem_sou(url: Option<&str>) -> anyhow::Result<()> {
    let ot = OptTime::new(endpoint(url)?);
    let me = runtime()?.block_on(ot.whoami())?;
    println!("{} <{}> · {}", me.name, me.email, me.role);
    println!("escopos: {}", me.scopes.join(", "));
    let missing = me.missing_scopes();
    if !missing.is_empty() {
        println!(
            "  faltam no token: {} (gere um com o preset \"Assistente pessoal (ISPer)\")",
            missing.join(", ")
        );
    }
    let ms = &me.microsoft;
    let microsoft = match (ms.connected, ms.token_usable, ms.needs_reconnect) {
        (false, _, _) => "não conectada",
        (true, _, true) => "precisa reconectar no OptTime",
        (true, false, false) => "vinculada, mas o token não renova",
        (true, true, false) => "conectada",
    };
    println!("Microsoft: {microsoft}");
    println!(
        "Azure DevOps: {}",
        if me.azure_dev_ops.configured {
            "configurado"
        } else {
            "não configurado"
        }
    );
    println!(
        "aviso das 17:30 do Teams: {}",
        if me.evening_digest_enabled {
            "ligado"
        } else {
            "desligado"
        }
    );
    println!(
        "hoje ({}): {} de {}",
        me.today.date,
        hours(me.today.total_minutes),
        hours(me.today.daily_capacity_minutes)
    );
    Ok(())
}

/// O resumo de um dia.
pub fn dia(data: Option<&str>, url: Option<&str>) -> anyhow::Result<()> {
    let ot = OptTime::new(endpoint(url)?);
    let d = runtime()?.block_on(ot.day_summary(data))?;
    println!(
        "{} ({}): {} em {} lançamento(s)",
        d.date, d.weekday, d.total_label, d.entry_count
    );
    if d.is_workday {
        println!(
            "meta {} · faltam {}",
            hours(d.target_minutes),
            hours(d.missing_minutes())
        );
    } else {
        println!("não é dia útil");
    }
    for p in &d.by_project {
        println!(
            "  {} ({}): {}",
            p.project_name,
            p.project_code,
            hours(p.minutes)
        );
    }
    for w in &d.warnings {
        println!("  aviso: {w}");
    }
    Ok(())
}

/// As sugestões para preencher um dia.
pub fn sugestoes(data: Option<&str>, url: Option<&str>) -> anyhow::Result<()> {
    let ot = OptTime::new(endpoint(url)?);
    let s = runtime()?.block_on(ot.suggest(data))?;
    println!(
        "{}: {} registrado, meta {}, faltam {}",
        s.date,
        hours(s.already_logged_minutes),
        hours(s.target_minutes),
        hours(s.gap_minutes)
    );
    let src = &s.sources;
    println!(
        "fontes: Outlook {} · Teams {} · DevOps {} · histórico {}",
        yes(src.outlook),
        yes(src.teams_calls),
        yes(src.azure_dev_ops),
        yes(src.history)
    );
    if s.suggestions.is_empty() {
        println!("nenhuma sugestão para este dia");
    }
    for (i, sug) in s.suggestions.iter().enumerate() {
        println!(
            "{:>2}. {:>6} · {} [{} · {}]\n    {}\n    {}",
            i + 1,
            sug.duration_label,
            sug.project_name.as_deref().unwrap_or("(sem projeto)"),
            sug.source,
            sug.confidence,
            sug.description,
            sug.evidence
        );
    }
    for w in &s.warnings {
        println!("aviso: {w}");
    }
    Ok(())
}

/// A agenda do Outlook pelo OptTime.
pub fn agenda(data: Option<&str>, dias: u8, url: Option<&str>) -> anyhow::Result<()> {
    let ot = OptTime::new(endpoint(url)?);
    let a = runtime()?.block_on(ot.agenda(data, dias))?;
    if a.events.is_empty() {
        println!("nenhum evento");
    }
    for e in &a.events {
        let hora = |s: &str| s.get(11..16).unwrap_or("--:--").to_string();
        let quando = if e.is_all_day {
            format!("{} dia inteiro", e.start.get(..10).unwrap_or(""))
        } else {
            format!(
                "{} {}–{}",
                e.start.get(..10).unwrap_or(""),
                hora(&e.start),
                hora(&e.end)
            )
        };
        let mut marcas = Vec::new();
        if e.is_online {
            marcas.push("online".to_string());
        }
        if !e.is_meeting() {
            marcas.push(format!("não conta ({}, {})", e.response_status, e.show_as));
        }
        if e.series().is_some() {
            marcas.push("série".into());
        }
        if e.logged_minutes > 0 {
            marcas.push(format!("{} lançado", hours(e.logged_minutes)));
        }
        println!(
            "{quando}  {}  [{} pessoas{}]",
            e.subject,
            e.attendee_count,
            if marcas.is_empty() {
                String::new()
            } else {
                format!(" · {}", marcas.join(" · "))
            }
        );
    }
    for w in &a.warnings {
        println!("aviso: {w}");
    }
    Ok(())
}

/// O catálogo de ferramentas, com as marcações de leitura e escrita.
pub fn ferramentas(url: Option<&str>) -> anyhow::Result<()> {
    let endpoint = endpoint(url)?;
    let rt = runtime()?;
    let tools = rt.block_on(async {
        let conn = endpoint.connect().await?;
        if let Some((name, version)) = conn.server() {
            println!("servidor: {name} {version}");
        }
        let tools = conn.tools().await;
        conn.close().await;
        tools
    })?;
    for t in tools {
        let kind = if t.read_only {
            "lê"
        } else if t.destructive {
            "APAGA"
        } else {
            "escreve"
        };
        println!("{kind:>7}  {}", t.name);
    }
    Ok(())
}

fn yes(b: bool) -> &'static str {
    if b { "sim" } else { "não" }
}
