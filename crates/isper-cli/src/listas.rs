//! `isper-cli listas` (Fase 11.0): as listas "Quando der tempo" no banco do
//! app (ou num de teste), sem abrir o ISPer.

use std::path::Path;

use isper_assist::{Actor, AssistStore, NewTask, TaskList};

fn open(db: &Path) -> anyhow::Result<AssistStore> {
    // O MeetingStore migra o banco (a cadeia é dele).
    drop(isper_core::store::MeetingStore::open(db)?);
    Ok(AssistStore::open(db)?)
}

/// A lista pelo nome (sem diferença de maiúscula), entre as ativas.
fn find(store: &AssistStore, name: &str) -> anyhow::Result<TaskList> {
    let key = name.trim().to_lowercase();
    store
        .lists(false)?
        .into_iter()
        .find(|l| l.name.to_lowercase() == key)
        .ok_or_else(|| {
            anyhow::anyhow!("não há a lista \"{name}\" (crie com `isper-cli listas nova`)")
        })
}

/// As listas ativas com os itens; "parado" marca os sem mudança há 60 dias.
pub fn ver(db: &Path) -> anyhow::Result<()> {
    let store = open(db)?;
    let lists = store.lists_with_items()?;
    if lists.is_empty() {
        println!("(nenhuma lista)");
    }
    for l in lists {
        println!("{} ({})", l.list.name, l.items.len());
        for t in &l.items {
            let min = t
                .estimate_min
                .map(|m| format!(" · {m} min"))
                .unwrap_or_default();
            let stale = if l.stale.contains(&t.id) {
                " · parado"
            } else {
                ""
            };
            println!("  - {}{min}{stale}", t.title);
        }
    }
    Ok(())
}

/// Cria uma lista.
pub fn nova(db: &Path, nome: &str) -> anyhow::Result<()> {
    let l = open(db)?.create_list(nome, Actor::User)?;
    println!("lista criada: {}", l.name);
    Ok(())
}

/// Um item numa lista, com a estimativa em minutos se houver.
pub fn adicionar(db: &Path, lista: &str, titulo: &str, minutos: Option<u16>) -> anyhow::Result<()> {
    let store = open(db)?;
    let l = find(&store, lista)?;
    let t = store.add_to_list(
        &l.id,
        NewTask {
            estimate_min: minutos,
            ..NewTask::titled(titulo)
        },
        Actor::User,
    )?;
    println!("em {}: {}", l.name, t.title);
    Ok(())
}

/// Um item por linha de um arquivo de texto (a lista copiada de outro lugar).
pub fn colar(db: &Path, lista: &str, arquivo: &Path) -> anyhow::Result<()> {
    let store = open(db)?;
    let l = find(&store, lista)?;
    let text = std::fs::read_to_string(arquivo)?;
    let items = store.paste_into_list(&l.id, &text, Actor::User)?;
    println!("{} item(ns) em {}", items.len(), l.name);
    Ok(())
}
