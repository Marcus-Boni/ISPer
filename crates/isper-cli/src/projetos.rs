//! `isper-cli projetos` (Fase 11.1): os projetos no banco do app (ou num de
//! teste), sem abrir o ISPer.

use std::path::Path;

use isper_assist::{Actor, AssistStore, NewProject};

fn open(db: &Path) -> anyhow::Result<AssistStore> {
    // O MeetingStore migra o banco (a cadeia é dele).
    drop(isper_core::store::MeetingStore::open(db)?);
    Ok(AssistStore::open(db)?)
}

/// Os projetos ativos, com os apelidos e a ligação com o OptTime.
pub fn ver(db: &Path) -> anyhow::Result<()> {
    let projects = open(db)?.projects(false)?;
    if projects.is_empty() {
        println!("(nenhum projeto)");
    }
    for p in projects {
        let aliases = if p.aliases.is_empty() {
            String::new()
        } else {
            format!(" · apelidos: {}", p.aliases.join(", "))
        };
        let linked = p
            .external_ref
            .map(|r| format!(" · {r}"))
            .unwrap_or_default();
        println!("{}{aliases}{linked}", p.name);
    }
    Ok(())
}

/// Cria um projeto local.
pub fn novo(db: &Path, nome: &str, apelidos: &[String]) -> anyhow::Result<()> {
    let p = open(db)?.create_project(
        NewProject {
            name: nome.to_string(),
            aliases: apelidos.to_vec(),
            ..NewProject::default()
        },
        Actor::User,
    )?;
    println!("projeto criado: {}", p.name);
    Ok(())
}
