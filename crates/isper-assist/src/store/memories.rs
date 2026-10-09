//! A memória do assistente (Fase 10.5): fatos curtos sobre a pessoa, que ela
//! vê e edita, e que o assistente lê antes de responder.
//!
//! Nada aqui apaga: arquivar tira a memória do assistente, e restaurar a põe
//! de volta. Cada mudança vai para o diário com o "antes" e o "depois". O
//! assistente só propõe; a memória dele nasce depois do toque da pessoa
//! ([ADR 0023]).
//!
//! [ADR 0023]: ../../../../docs/adr/0023-escada-de-confianca.md

use rusqlite::{OptionalExtension, Row, params};
use serde_json::{Value, json};

use super::AssistStore;
use crate::model::{Actor, Memory, MemoryKind, MemoryPatch, NewMemory, clean_memory};
use crate::text::fold;
use crate::{AssistError, Result};

const MEMORY_COLUMNS: &str = "id, text, kind, origin, evidence, pinned, created_at, updated_at, \
     last_used_at, archived_at";

/// Quantas memórias vão para o assistente, no máximo (as fixadas primeiro).
pub const PROMPT_MEMORIES: usize = 40;

impl AssistStore {
    /// As memórias ativas (`archived = false`) ou as arquivadas: as fixadas
    /// primeiro, depois as mudadas por último.
    pub fn memories(&self, archived: bool) -> Result<Vec<Memory>> {
        let filter = if archived {
            "archived_at IS NOT NULL"
        } else {
            "archived_at IS NULL"
        };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {MEMORY_COLUMNS} FROM memories WHERE {filter}
              ORDER BY pinned DESC, updated_at DESC, id"
        ))?;
        let rows = stmt.query_map([], memory_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row??);
        }
        Ok(out)
    }

    /// Uma memória pelo id.
    pub fn memory(&self, id: &str) -> Result<Memory> {
        self.conn
            .query_row(
                &format!("SELECT {MEMORY_COLUMNS} FROM memories WHERE id = ?1"),
                params![id],
                memory_from_row,
            )
            .optional()?
            .ok_or_else(|| AssistError::NotFound(format!("a memória {id}")))?
    }

    /// O que o assistente lê antes de responder: as ativas, até
    /// [`PROMPT_MEMORIES`]. Marca a leitura (`last_used_at`).
    pub fn memories_for_prompt(&self) -> Result<Vec<Memory>> {
        let mut out = self.memories(false)?;
        out.truncate(PROMPT_MEMORIES);
        if !out.is_empty() {
            let now = self.clock.now_ms();
            let tx = self.conn.unchecked_transaction()?;
            for m in &mut out {
                tx.execute(
                    "UPDATE memories SET last_used_at = ?2 WHERE id = ?1",
                    params![m.id, now],
                )?;
                m.last_used_at = Some(now);
            }
            tx.commit()?;
        }
        Ok(out)
    }

    /// Guarda uma memória. A mesma frase de uma ativa (sem diferença de
    /// maiúscula ou acento) devolve a que já existe, sem repetir.
    pub fn create_memory(&self, new: NewMemory, actor: Actor) -> Result<Memory> {
        let text = clean_memory(&new.text)?;
        let key = fold(&text);
        if let Some(same) = self
            .memories(false)?
            .into_iter()
            .find(|m| fold(&m.text) == key)
        {
            return Ok(same);
        }
        let now = self.clock.now_ms();
        let memory = Memory {
            id: uuid::Uuid::now_v7().to_string(),
            text,
            kind: new.kind,
            origin: actor.as_str().to_string(),
            evidence: new.evidence,
            pinned: false,
            created_at: now,
            updated_at: now,
            last_used_at: None,
            archived_at: None,
        };
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            &format!(
                "INSERT INTO memories ({MEMORY_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
            ),
            params![
                memory.id,
                memory.text,
                memory.kind.as_str(),
                memory.origin,
                memory.evidence.as_ref().map(Value::to_string),
                memory.pinned,
                memory.created_at,
                memory.updated_at,
                memory.last_used_at,
                memory.archived_at,
            ],
        )?;
        self.journal_row(
            &tx,
            now,
            actor,
            "memory.created",
            "memory",
            &memory.id,
            &memory.text,
            &json!({ "before": null, "after": memory }),
        )?;
        tx.commit()?;
        Ok(memory)
    }

    /// Muda o texto, o tipo ou a fixação.
    pub fn update_memory(&self, id: &str, patch: MemoryPatch, actor: Actor) -> Result<Memory> {
        let before = self.memory(id)?;
        let mut after = before.clone();
        if let Some(text) = patch.text {
            after.text = clean_memory(&text)?;
        }
        if let Some(kind) = patch.kind {
            after.kind = kind;
        }
        if let Some(pinned) = patch.pinned {
            after.pinned = pinned;
        }
        if after == before {
            return Ok(before);
        }
        after.updated_at = self.clock.now_ms();
        self.write_memory(&before, &after, actor, "memory.updated")?;
        Ok(after)
    }

    /// Arquiva: sai do assistente, mas fica para restaurar.
    pub fn archive_memory(&self, id: &str, actor: Actor) -> Result<Memory> {
        let before = self.memory(id)?;
        if before.archived_at.is_some() {
            return Ok(before);
        }
        let mut after = before.clone();
        after.updated_at = self.clock.now_ms();
        after.archived_at = Some(after.updated_at);
        self.write_memory(&before, &after, actor, "memory.archived")?;
        Ok(after)
    }

    /// Restaura uma arquivada (o "Desfazer" do arquivar).
    pub fn restore_memory(&self, id: &str, actor: Actor) -> Result<Memory> {
        let before = self.memory(id)?;
        if before.archived_at.is_none() {
            return Ok(before);
        }
        let mut after = before.clone();
        after.updated_at = self.clock.now_ms();
        after.archived_at = None;
        self.write_memory(&before, &after, actor, "memory.restored")?;
        Ok(after)
    }

    fn write_memory(
        &self,
        before: &Memory,
        after: &Memory,
        actor: Actor,
        action: &str,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE memories SET text = ?2, kind = ?3, pinned = ?4, updated_at = ?5,
                    archived_at = ?6
              WHERE id = ?1",
            params![
                after.id,
                after.text,
                after.kind.as_str(),
                after.pinned,
                after.updated_at,
                after.archived_at,
            ],
        )?;
        self.journal_row(
            &tx,
            after.updated_at,
            actor,
            action,
            "memory",
            &after.id,
            &after.text,
            &json!({ "before": before, "after": after }),
        )?;
        tx.commit()?;
        Ok(())
    }
}

fn memory_from_row(r: &Row<'_>) -> rusqlite::Result<Result<Memory>> {
    let kind: String = r.get(2)?;
    let evidence: Option<String> = r.get(4)?;
    let fields = (
        r.get::<_, String>(0)?,
        r.get::<_, String>(1)?,
        r.get::<_, String>(3)?,
        r.get::<_, bool>(5)?,
        r.get::<_, i64>(6)?,
        r.get::<_, i64>(7)?,
        r.get::<_, Option<i64>>(8)?,
        r.get::<_, Option<i64>>(9)?,
    );
    Ok((|| {
        let (id, text, origin, pinned, created_at, updated_at, last_used_at, archived_at) = fields;
        Ok(Memory {
            id,
            text,
            kind: MemoryKind::parse(&kind)?,
            origin,
            evidence: evidence.map(|s| serde_json::from_str(&s)).transpose()?,
            pinned,
            created_at,
            updated_at,
            last_used_at,
            archived_at,
        })
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::store::tests::store_at;

    #[test]
    fn guardar_editar_fixar_arquivar_e_restaurar() {
        let (store, _) = store_at(FixedClock::at("2026-10-09 10:00", -3));
        let gestor = store
            .create_memory(NewMemory::fact("Meu gestor é o Carlos"), Actor::User)
            .unwrap();
        assert_eq!(gestor.origin, "user");
        let again = store
            .create_memory(
                NewMemory::fact("  meu gestor é o carlos "),
                Actor::Assistant,
            )
            .unwrap();
        assert_eq!(again.id, gestor.id, "a mesma frase não repete");
        let pref = store
            .create_memory(
                NewMemory {
                    text: "Prefiro reuniões depois das 10h".into(),
                    kind: MemoryKind::Preference,
                    evidence: Some(json!({"pergunta": "lembra disso"})),
                },
                Actor::Assistant,
            )
            .unwrap();
        assert_eq!(pref.origin, "assistant");

        let gestor = store
            .update_memory(
                &gestor.id,
                MemoryPatch {
                    text: Some("Meu gestor é o Carlos Lima".into()),
                    pinned: Some(true),
                    ..Default::default()
                },
                Actor::User,
            )
            .unwrap();
        let list = store.memories(false).unwrap();
        assert_eq!(list[0].id, gestor.id, "a fixada vem primeiro");
        assert_eq!(list[0].text, "Meu gestor é o Carlos Lima");

        store.archive_memory(&pref.id, Actor::User).unwrap();
        assert_eq!(store.memories(false).unwrap().len(), 1);
        assert_eq!(store.memories(true).unwrap()[0].id, pref.id);
        store.restore_memory(&pref.id, Actor::User).unwrap();
        assert_eq!(store.memories(false).unwrap().len(), 2);

        let prompt = store.memories_for_prompt().unwrap();
        assert_eq!(prompt.len(), 2);
        assert!(store.memory(&pref.id).unwrap().last_used_at.is_some());

        let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 9).unwrap();
        let actions: Vec<String> = store
            .journal_for_day(day)
            .unwrap()
            .into_iter()
            .filter(|e| e.object_kind == "memory")
            .map(|e| e.action)
            .collect();
        assert_eq!(
            actions,
            [
                "memory.created",
                "memory.created",
                "memory.updated",
                "memory.archived",
                "memory.restored"
            ]
        );
    }

    #[test]
    fn memoria_longa_ou_vazia_e_recusada() {
        let (store, _) = store_at(FixedClock::at("2026-10-09 10:00", -3));
        assert!(
            store
                .create_memory(NewMemory::fact("  "), Actor::User)
                .is_err()
        );
        assert!(
            store
                .create_memory(NewMemory::fact("x".repeat(400)), Actor::User)
                .is_err()
        );
    }
}
