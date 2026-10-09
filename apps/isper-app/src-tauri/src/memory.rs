//! A memória do assistente nas Configurações (Fase 10.5, [ADR 0021]): os
//! fatos curtos que ele lê antes de responder, que a pessoa vê, escreve,
//! edita, fixa, arquiva e restaura.
//!
//! O assistente também guarda, pela ferramenta `lembrar`, mas só depois do
//! toque no cartão ([ADR 0023]). Toda mudança avisa as telas pelo evento
//! [`MEMORY_EVENT`].
//!
//! [ADR 0021]: ../../../../docs/adr/0021-assistente-pessoal-no-isper.md
//! [ADR 0023]: ../../../../docs/adr/0023-escada-de-confianca.md

use crate::prelude::*;
use crate::today::open_assist;
use isper_assist::{Actor, Memory, MemoryKind, MemoryPatch, NewMemory};

/// Evento que avisa as telas de que a memória mudou.
pub(crate) const MEMORY_EVENT: &str = "isper-memory";

fn changed(app: &AppHandle) {
    let _ = app.emit(MEMORY_EVENT, ());
}

/// As memórias ativas, ou as arquivadas.
#[tauri::command]
pub(crate) fn memory_list(archived: bool) -> Result<Vec<Memory>, String> {
    (|| Ok(open_assist()?.memories(archived)?))().map_err(|e: anyhow::Error| e.to_string())
}

/// Guarda um fato (ou uma preferência) digitado.
#[tauri::command]
pub(crate) fn memory_add(app: AppHandle, text: String, kind: MemoryKind) -> Result<Memory, String> {
    let m = (|| {
        Ok(open_assist()?.create_memory(
            NewMemory {
                text,
                kind,
                evidence: None,
            },
            Actor::User,
        )?)
    })()
    .map_err(|e: anyhow::Error| e.to_string())?;
    changed(&app);
    Ok(m)
}

/// Muda o texto, o tipo ou a fixação.
#[tauri::command]
pub(crate) fn memory_update(
    app: AppHandle,
    id: String,
    patch: MemoryPatch,
) -> Result<Memory, String> {
    let m = (|| Ok(open_assist()?.update_memory(&id, patch, Actor::User)?))()
        .map_err(|e: anyhow::Error| e.to_string())?;
    changed(&app);
    Ok(m)
}

/// Arquiva: sai do assistente, mas fica para restaurar.
#[tauri::command]
pub(crate) fn memory_archive(app: AppHandle, id: String) -> Result<Memory, String> {
    let m = (|| Ok(open_assist()?.archive_memory(&id, Actor::User)?))()
        .map_err(|e: anyhow::Error| e.to_string())?;
    changed(&app);
    Ok(m)
}

/// Restaura uma arquivada (o "Desfazer" do arquivar).
#[tauri::command]
pub(crate) fn memory_restore(app: AppHandle, id: String) -> Result<Memory, String> {
    let m = (|| Ok(open_assist()?.restore_memory(&id, Actor::User)?))()
        .map_err(|e: anyhow::Error| e.to_string())?;
    changed(&app);
    Ok(m)
}
