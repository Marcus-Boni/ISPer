//! As listas "Quando der tempo" (Fase 11.0, [ADR 0025]): o que não tem data
//! nem pressa — estudar, ler, ideias — fora do dia.
//!
//! - **A lista é livre**: a pessoa cria, renomeia e arquiva ("Estudar",
//!   "Ler"); arquivar não apaga nada, e os itens voltam com ela.
//! - **O item é uma tarefa** sem dia com `list_id`: herda o diário, o
//!   desfazer, o celular e o assistente. Agendado, vai para o Hoje e volta à
//!   lista se perder o dia; concluído ou descartado, sai dela.
//! - **Colar várias linhas** (a lista copiada do Notion, de um e-mail) vira um
//!   item por linha, sem os marcadores ("- [ ]", "•", "1.").
//! - **Parado**: sem mudança há [`STALE_DAYS`] dias. "Manter" conta como
//!   mudança e vai para o diário, para a revisão não mostrar de novo.
//!
//! [ADR 0025]: ../../../../docs/adr/0025-quando-der-tempo-e-projetos-locais.md

use chrono::Duration;
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;
use serde_json::json;

use super::AssistStore;
use crate::model::{Actor, NewTask, Task, TaskStatus};
use crate::text::fold;
use crate::{AssistError, Result};

/// Sem mudança há tantos dias, o item ganha o selo "parado".
pub const STALE_DAYS: i64 = 60;
/// Tamanho máximo do nome de uma lista.
const MAX_LIST_NAME: usize = 60;
/// Quantos itens um colar cria de uma vez, no máximo.
pub const MAX_PASTE_ITEMS: usize = 200;

const LIST_COLUMNS: &str = "id, name, position, created_at, updated_at, archived_at";

/// Uma lista "Quando der tempo".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskList {
    /// UUID v7.
    pub id: String,
    pub name: String,
    /// A ordem na tela.
    pub position: f64,
    pub created_at: i64,
    pub updated_at: i64,
    /// Arquivada em (ms UTC): some da tela, não do banco.
    pub archived_at: Option<i64>,
}

/// Uma lista com os itens abertos dela.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ListWithItems {
    #[serde(flatten)]
    pub list: TaskList,
    /// Os itens sem dia, na ordem em que entraram.
    pub items: Vec<Task>,
    /// Desses, os parados (sem mudança há [`STALE_DAYS`] dias).
    pub stale: Vec<String>,
}

fn clean_name(name: &str) -> Result<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(AssistError::Invalid("a lista precisa de um nome".into()));
    }
    if name.chars().count() > MAX_LIST_NAME {
        return Err(AssistError::Invalid(format!(
            "nome de lista com mais de {MAX_LIST_NAME} caracteres"
        )));
    }
    Ok(name)
}

/// Uma linha colada sem o marcador de lista ("- [ ] ", "* ", "• ", "1. ",
/// "[x] "). Vazia vira `None`.
pub fn clean_line(line: &str) -> Option<String> {
    let mut s = line.trim();
    loop {
        let before = s;
        for marker in ["- ", "* ", "+ ", "• ", "– ", "— "] {
            if let Some(rest) = s.strip_prefix(marker) {
                s = rest.trim_start();
            }
        }
        for check in ["[ ]", "[x]", "[X]", "☐", "☑", "✅"] {
            if let Some(rest) = s.strip_prefix(check) {
                s = rest.trim_start();
            }
        }
        // "1. " / "1) "
        let digits = s.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && digits < 4 {
            let rest = &s[digits..];
            if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
                s = r.trim_start();
            }
        }
        if s == before {
            break;
        }
    }
    (!s.is_empty()).then(|| s.to_string())
}

fn list_from_row(r: &Row<'_>) -> rusqlite::Result<TaskList> {
    Ok(TaskList {
        id: r.get(0)?,
        name: r.get(1)?,
        position: r.get(2)?,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
        archived_at: r.get(5)?,
    })
}

impl AssistStore {
    /// As listas ativas (`archived = false`) ou as arquivadas, na ordem da tela.
    pub fn lists(&self, archived: bool) -> Result<Vec<TaskList>> {
        let filter = if archived {
            "archived_at IS NOT NULL"
        } else {
            "archived_at IS NULL"
        };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {LIST_COLUMNS} FROM lists WHERE {filter} ORDER BY position, id"
        ))?;
        let rows = stmt.query_map([], list_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Uma lista pelo id.
    pub fn task_list(&self, id: &str) -> Result<TaskList> {
        self.conn
            .query_row(
                &format!("SELECT {LIST_COLUMNS} FROM lists WHERE id = ?1"),
                params![id],
                list_from_row,
            )
            .optional()?
            .ok_or_else(|| AssistError::NotFound(format!("a lista {id}")))
    }

    /// Cria uma lista. O mesmo nome de uma ativa (sem diferença de maiúscula
    /// ou acento) é recusado: duas "Estudar" só confundem.
    pub fn create_list(&self, name: &str, actor: Actor) -> Result<TaskList> {
        let name = clean_name(name)?;
        self.check_unique_name(&name, None)?;
        let now = self.clock.now_ms();
        let last: f64 =
            self.conn
                .query_row("SELECT COALESCE(MAX(position), 0) FROM lists", [], |r| {
                    r.get(0)
                })?;
        let list = TaskList {
            id: uuid::Uuid::now_v7().to_string(),
            name,
            position: last + 1.0,
            created_at: now,
            updated_at: now,
            archived_at: None,
        };
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            &format!("INSERT INTO lists ({LIST_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"),
            params![
                list.id,
                list.name,
                list.position,
                list.created_at,
                list.updated_at,
                list.archived_at
            ],
        )?;
        self.journal_row(
            &tx,
            now,
            actor,
            "list.created",
            "list",
            &list.id,
            &list.name,
            &json!({ "before": null, "after": list }),
        )?;
        tx.commit()?;
        Ok(list)
    }

    fn check_unique_name(&self, name: &str, except: Option<&str>) -> Result<()> {
        let key = fold(name);
        if self
            .lists(false)?
            .iter()
            .any(|l| fold(&l.name) == key && Some(l.id.as_str()) != except)
        {
            return Err(AssistError::Invalid(format!(
                "já existe a lista \"{name}\""
            )));
        }
        Ok(())
    }

    /// Renomeia.
    pub fn rename_list(&self, id: &str, name: &str, actor: Actor) -> Result<TaskList> {
        let before = self.task_list(id)?;
        let name = clean_name(name)?;
        if name == before.name {
            return Ok(before);
        }
        self.check_unique_name(&name, Some(id))?;
        let after = TaskList {
            name,
            updated_at: self.clock.now_ms(),
            ..before.clone()
        };
        self.write_list(&before, &after, actor, "list.renamed")?;
        Ok(after)
    }

    /// Arquiva (`true`) ou traz de volta. Os itens ficam onde estão.
    pub fn set_list_archived(&self, id: &str, archived: bool, actor: Actor) -> Result<TaskList> {
        let before = self.task_list(id)?;
        if before.archived_at.is_some() == archived {
            return Ok(before);
        }
        if !archived {
            self.check_unique_name(&before.name, Some(id))?;
        }
        let now = self.clock.now_ms();
        let after = TaskList {
            archived_at: archived.then_some(now),
            updated_at: now,
            ..before.clone()
        };
        let action = if archived {
            "list.archived"
        } else {
            "list.restored"
        };
        self.write_list(&before, &after, actor, action)?;
        Ok(after)
    }

    fn write_list(
        &self,
        before: &TaskList,
        after: &TaskList,
        actor: Actor,
        action: &str,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE lists SET name = ?2, position = ?3, updated_at = ?4, archived_at = ?5
              WHERE id = ?1",
            params![
                after.id,
                after.name,
                after.position,
                after.updated_at,
                after.archived_at
            ],
        )?;
        self.journal_row(
            &tx,
            after.updated_at,
            actor,
            action,
            "list",
            &after.id,
            &after.name,
            &json!({ "before": before, "after": after }),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Os itens abertos e sem dia de uma lista, na ordem em que entraram.
    pub fn list_items(&self, list_id: &str) -> Result<Vec<Task>> {
        self.tasks_where(
            "list_id = ?1 AND status = 'open' AND planned_on IS NULL ORDER BY position, id",
            params![list_id],
        )
    }

    /// Todos os itens abertos e sem dia das listas ativas: o que pode caber
    /// num tempo livre.
    pub fn spare_items(&self) -> Result<Vec<Task>> {
        self.tasks_where(
            "status = 'open' AND planned_on IS NULL
               AND list_id IN (SELECT id FROM lists WHERE archived_at IS NULL)
             ORDER BY updated_at, position, id",
            params![],
        )
    }

    /// As listas ativas com os itens e quais deles estão parados.
    pub fn lists_with_items(&self) -> Result<Vec<ListWithItems>> {
        let cutoff = self.clock.now_ms() - Duration::days(STALE_DAYS).num_milliseconds();
        self.lists(false)?
            .into_iter()
            .map(|list| {
                let items = self.list_items(&list.id)?;
                let stale = items
                    .iter()
                    .filter(|t| t.updated_at < cutoff)
                    .map(|t| t.id.clone())
                    .collect();
                Ok(ListWithItems { list, items, stale })
            })
            .collect()
    }

    /// Um item novo numa lista (sem dia).
    pub fn add_to_list(&self, list_id: &str, new: NewTask, actor: Actor) -> Result<Task> {
        let list = self.task_list(list_id)?;
        if list.archived_at.is_some() {
            return Err(AssistError::Invalid(format!(
                "a lista \"{}\" está arquivada",
                list.name
            )));
        }
        self.create_task(
            NewTask {
                status: TaskStatus::Open,
                planned_on: None,
                planned_time: None,
                list_id: Some(list.id),
                ..new
            },
            actor,
        )
    }

    /// Um item por linha do texto colado (sem os marcadores de lista). As
    /// linhas vazias ficam de fora; devolve os itens criados.
    pub fn paste_into_list(&self, list_id: &str, text: &str, actor: Actor) -> Result<Vec<Task>> {
        let lines: Vec<String> = text.lines().filter_map(clean_line).collect();
        if lines.len() > MAX_PASTE_ITEMS {
            return Err(AssistError::Invalid(format!(
                "{} linhas de uma vez; o máximo é {MAX_PASTE_ITEMS}",
                lines.len()
            )));
        }
        lines
            .into_iter()
            .map(|line| self.add_to_list(list_id, NewTask::titled(line), actor))
            .collect()
    }

    /// "Manter" um item parado: conta como mudança (vai para o diário), e o
    /// selo some por mais [`STALE_DAYS`] dias.
    pub fn keep_item(&self, task_id: &str, actor: Actor) -> Result<Task> {
        let before = self.task(task_id)?;
        if before.list_id.is_none() {
            return Err(AssistError::Invalid("a tarefa não está numa lista".into()));
        }
        let mut after = before.clone();
        after.updated_at = self.clock.now_ms();
        self.write_at(&before, &after, actor, "task.kept", None, None)?;
        Ok(after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::model::TaskPatch;
    use crate::store::tests::store_at;
    use chrono::NaiveDate;

    fn at(path: &std::path::Path, when: &str) -> AssistStore {
        AssistStore::open_with_clock(path, Box::new(FixedClock::at(when, -3))).unwrap()
    }

    #[test]
    fn colar_a_lista_do_notion_vira_um_item_por_linha() {
        let (store, _) = store_at(FixedClock::at("2026-10-12 10:00", -3));
        let estudar = store.create_list("Estudar", Actor::User).unwrap();
        let pasted = store
            .paste_into_list(
                &estudar.id,
                "- [ ] CRDT e sincronia\n\n* Rust: lifetimes\n1. Artigo do Martin Kleppmann\n☐ Ler sobre iroh\n   \n",
                Actor::User,
            )
            .unwrap();
        let titles: Vec<&str> = pasted.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "CRDT e sincronia",
                "Rust: lifetimes",
                "Artigo do Martin Kleppmann",
                "Ler sobre iroh"
            ]
        );
        assert_eq!(store.list_items(&estudar.id).unwrap().len(), 4);
        assert!(
            store.create_list(" estudar ", Actor::User).is_err(),
            "nome repetido"
        );
        assert!(clean_line("- [x] Feito").as_deref() == Some("Feito"));
        assert!(clean_line("   ").is_none());
    }

    #[test]
    fn item_de_lista_fica_fora_do_dia_e_volta_se_perder_o_dia() {
        let (store, _) = store_at(FixedClock::at("2026-10-12 10:00", -3));
        let ler = store.create_list("Ler", Actor::User).unwrap();
        let item = store
            .add_to_list(
                &ler.id,
                NewTask {
                    estimate_min: Some(30),
                    ..NewTask::titled("Ler o capítulo 3")
                },
                Actor::User,
            )
            .unwrap();
        let solta = store
            .create_task(NewTask::titled("Algum dia sem lista"), Actor::User)
            .unwrap();
        let today = store.today().unwrap();
        assert!(
            today.later.iter().any(|t| t.id == solta.id),
            "sem lista, vai para o Depois"
        );
        assert!(
            !today.later.iter().any(|t| t.id == item.id),
            "na lista, fora do dia"
        );
        assert!(today.planned.is_empty());

        // Agendar para hoje: vai para o Hoje e sai da lista.
        let day = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        store
            .update_task(
                &item.id,
                TaskPatch {
                    planned_on: Some(Some(day)),
                    ..TaskPatch::default()
                },
                Actor::User,
            )
            .unwrap();
        assert_eq!(store.today().unwrap().planned[0].id, item.id);
        assert!(store.list_items(&ler.id).unwrap().is_empty());
        // Voltar para "algum dia": volta para a lista, não para o Depois.
        store
            .update_task(
                &item.id,
                TaskPatch {
                    planned_on: Some(None),
                    ..TaskPatch::default()
                },
                Actor::User,
            )
            .unwrap();
        assert_eq!(store.list_items(&ler.id).unwrap()[0].estimate_min, Some(30));
        assert!(!store.today().unwrap().later.iter().any(|t| t.id == item.id));
        // Concluída, sai da lista.
        store
            .set_status(&item.id, TaskStatus::Done, Actor::User)
            .unwrap();
        assert!(store.list_items(&ler.id).unwrap().is_empty());
    }

    #[test]
    fn parado_depois_de_60_dias_e_manter_tira_o_selo() {
        let (store, path) = store_at(FixedClock::at("2026-07-01 10:00", -3));
        let ideias = store.create_list("Ideias", Actor::User).unwrap();
        let velho = store
            .add_to_list(&ideias.id, NewTask::titled("App de receitas"), Actor::User)
            .unwrap();
        let store = at(&path, "2026-10-12 10:00");
        let novo = store
            .add_to_list(
                &ideias.id,
                NewTask::titled("Plugin do Obsidian"),
                Actor::User,
            )
            .unwrap();
        let l = &store.lists_with_items().unwrap()[0];
        assert_eq!(l.stale, std::slice::from_ref(&velho.id));
        assert!(!l.stale.contains(&novo.id));
        store.keep_item(&velho.id, Actor::User).unwrap();
        assert!(store.lists_with_items().unwrap()[0].stale.is_empty());
        assert!(store.keep_item(&novo.id, Actor::User).is_ok());
        let solta = store
            .create_task(NewTask::titled("x"), Actor::User)
            .unwrap();
        assert!(store.keep_item(&solta.id, Actor::User).is_err());
    }

    #[test]
    fn arquivar_a_lista_tira_os_itens_do_tempo_livre_e_volta_com_eles() {
        let (store, _) = store_at(FixedClock::at("2026-10-12 10:00", -3));
        let l = store.create_list("Estudar", Actor::User).unwrap();
        store
            .add_to_list(&l.id, NewTask::titled("Grafos"), Actor::User)
            .unwrap();
        assert_eq!(store.spare_items().unwrap().len(), 1);
        store.set_list_archived(&l.id, true, Actor::User).unwrap();
        assert!(store.spare_items().unwrap().is_empty());
        assert!(store.lists(false).unwrap().is_empty());
        assert!(
            store
                .add_to_list(&l.id, NewTask::titled("x"), Actor::User)
                .is_err()
        );
        store.set_list_archived(&l.id, false, Actor::User).unwrap();
        assert_eq!(store.spare_items().unwrap().len(), 1);
        let renamed = store
            .rename_list(&l.id, "Estudar (TCC)", Actor::User)
            .unwrap();
        assert_eq!(renamed.name, "Estudar (TCC)");
        let day = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let actions: Vec<String> = store
            .journal_for_day(day)
            .unwrap()
            .into_iter()
            .filter(|e| e.object_kind == "list")
            .map(|e| e.action)
            .collect();
        assert_eq!(
            actions,
            [
                "list.created",
                "list.archived",
                "list.restored",
                "list.renamed"
            ]
        );
    }
}
