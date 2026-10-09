//! `isper-cli memoria` e `isper-cli tarefas sugestoes` (Fase 10.5): a
//! memória do assistente e as rotinas sugeridas, para conferir no banco do
//! app (ou num de teste) sem abrir o ISPer.

use std::path::Path;

use isper_assist::{Actor, AssistStore, MemoryKind, NewMemory};

fn open(db: &Path) -> anyhow::Result<AssistStore> {
    // O MeetingStore migra o banco (a cadeia é dele).
    drop(isper_core::store::MeetingStore::open(db)?);
    Ok(AssistStore::open(db)?)
}

/// Lista as memórias ativas ou as arquivadas.
pub fn listar(db: &Path, arquivadas: bool) -> anyhow::Result<()> {
    let store = open(db)?;
    let list = store.memories(arquivadas)?;
    if list.is_empty() {
        println!("(nenhuma)");
    }
    for m in list {
        let tipo = match m.kind {
            MemoryKind::Fact => "fato",
            MemoryKind::Preference => "preferência",
        };
        let fixada = if m.pinned { " · fixada" } else { "" };
        println!("{}  [{tipo} · {}{fixada}]  {}", m.id, m.origin, m.text);
    }
    Ok(())
}

/// Guarda um fato (ou uma preferência).
pub fn guardar(db: &Path, texto: &str, preferencia: bool) -> anyhow::Result<()> {
    let store = open(db)?;
    let m = store.create_memory(
        NewMemory {
            text: texto.to_string(),
            kind: if preferencia {
                MemoryKind::Preference
            } else {
                MemoryKind::Fact
            },
            evidence: None,
        },
        Actor::User,
    )?;
    println!("{}  {}", m.id, m.text);
    Ok(())
}

/// Arquiva (`arquivar = true`) ou restaura.
pub fn arquivar(db: &Path, id: &str, arquivar: bool) -> anyhow::Result<()> {
    let store = open(db)?;
    let m = if arquivar {
        store.archive_memory(id, Actor::User)?
    } else {
        store.restore_memory(id, Actor::User)?
    };
    let estado = if m.archived_at.is_some() {
        "arquivada"
    } else {
        "ativa"
    };
    println!("{estado}: {}", m.text);
    Ok(())
}

/// As rotinas sugeridas hoje, com a evidência. Só lê.
pub fn sugestoes(db: &Path) -> anyhow::Result<()> {
    let store = open(db)?;
    let list = store.routine_suggestions()?;
    if list.is_empty() {
        println!("nenhuma sugestão: nada se repetiu em 3 das últimas 4 semanas no mesmo dia");
    }
    for s in list {
        let dias: Vec<String> = s
            .days
            .iter()
            .map(|d| d.format("%d/%m").to_string())
            .collect();
        println!("{}  ({} de {})", s.title, s.hits, s.of);
        println!("  {}", s.rrule);
        println!("  dias: {}", dias.join(", "));
    }
    Ok(())
}
