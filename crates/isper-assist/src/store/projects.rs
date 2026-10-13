//! Os projetos (Fase 11.1, [ADR 0025]): uma lente local sobre as tarefas
//! (e os itens das listas), as rotinas e as reuniões. Os ditados não têm.
//!
//! - **O projeto é do ISPer**: nome, cor, apelidos ("Portal", "PRT") e
//!   arquivado. Uma integração só preenche: `external_ref = opttime:<id>`
//!   liga um projeto local a um do OptTime que a pessoa escolheu trazer.
//! - **A reunião já sai com o projeto** quando o ISPer o reconhece: o da
//!   reunião anterior da mesma série (ou do mesmo evento); senão, o nome ou
//!   um apelido no assunto do convite. A marca fica no diário com o
//!   assistente como autor, e é isso que a tela mostra como "sugerido" até a
//!   pessoa trocar ou confirmar. As tarefas da reunião herdam o projeto e o
//!   acompanham quando ele muda.
//!
//! [ADR 0025]: ../../../../docs/adr/0025-quando-der-tempo-e-projetos-locais.md

use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::AssistStore;
use crate::model::{Actor, TaskPatch, double_option};
use crate::text::fold;
use crate::{AssistError, Result};

/// Tamanho máximo do nome (e de cada apelido) de um projeto.
pub const MAX_PROJECT_NAME: usize = 60;
/// Quantos apelidos um projeto tem, no máximo.
pub const MAX_ALIASES: usize = 12;
/// O prefixo do `external_ref` de um projeto ligado ao OptTime.
pub const OPTTIME_REF: &str = "opttime:";
/// As cores dos projetos novos, em rodízio (a pessoa troca depois).
pub const PALETTE: [&str; 8] = [
    "#e07a5f", "#3d8bfd", "#2a9d8f", "#e9c46a", "#9b5de5", "#f15bb5", "#00b4d8", "#8ac926",
];

const PROJECT_COLUMNS: &str =
    "id, name, color, aliases, external_ref, created_at, updated_at, archived_at";

/// Um projeto.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Project {
    /// UUID v7.
    pub id: String,
    pub name: String,
    /// `#rrggbb`.
    pub color: Option<String>,
    /// Outros nomes com que o projeto aparece (no assunto de um convite, o
    /// código no OptTime).
    pub aliases: Vec<String>,
    /// `opttime:<id>` quando ligado ao OptTime.
    pub external_ref: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// Arquivado em (ms UTC): some das escolhas, não do que já foi marcado.
    pub archived_at: Option<i64>,
}

/// Um projeto novo.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct NewProject {
    pub name: String,
    /// `#rrggbb`; sem cor, a próxima da [`PALETTE`].
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub external_ref: Option<String>,
}

/// Mudança num projeto: só os campos presentes mudam.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ProjectPatch {
    #[serde(default)]
    pub name: Option<String>,
    /// A cor (`null` tira).
    #[serde(default, deserialize_with = "double_option")]
    pub color: Option<Option<String>>,
    /// Os apelidos, todos de uma vez.
    #[serde(default)]
    pub aliases: Option<Vec<String>>,
}

/// Um projeto de fora (do OptTime) que a pessoa escolheu trazer.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LinkedProject {
    /// `opttime:<id>`.
    pub external_ref: String,
    pub name: String,
    /// A cor de lá, se for `#rrggbb`.
    #[serde(default)]
    pub color: Option<String>,
    /// O código de lá ("PRT"), que vira apelido.
    #[serde(default)]
    pub code: Option<String>,
}

/// O projeto de uma reunião, e se a marca foi do ISPer (a tela mostra
/// "sugerido" até a pessoa trocar ou confirmar).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeetingProject {
    pub project_id: Option<String>,
    pub suggested: bool,
}

fn clean_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_name(name: &str) -> Result<String> {
    let name = clean_text(name);
    if name.is_empty() {
        return Err(AssistError::Invalid("o projeto precisa de um nome".into()));
    }
    if name.chars().count() > MAX_PROJECT_NAME {
        return Err(AssistError::Invalid(format!(
            "nome de projeto com mais de {MAX_PROJECT_NAME} caracteres"
        )));
    }
    Ok(name)
}

fn clean_color(color: Option<String>) -> Result<Option<String>> {
    let Some(c) = color
        .map(|c| c.trim().to_lowercase())
        .filter(|c| !c.is_empty())
    else {
        return Ok(None);
    };
    if c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit()) {
        Ok(Some(c))
    } else {
        Err(AssistError::Invalid(format!("a cor \"{c}\" não é #rrggbb")))
    }
}

/// Aparados, sem repetir (nem o próprio nome), sem diferença de maiúscula ou
/// acento.
fn clean_aliases(name: &str, aliases: Vec<String>) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for a in aliases {
        let a = clean_text(&a);
        if a.is_empty() || fold(&a) == fold(name) || out.iter().any(|o| fold(o) == fold(&a)) {
            continue;
        }
        if a.chars().count() > MAX_PROJECT_NAME {
            return Err(AssistError::Invalid(format!(
                "apelido com mais de {MAX_PROJECT_NAME} caracteres"
            )));
        }
        out.push(a);
    }
    if out.len() > MAX_ALIASES {
        return Err(AssistError::Invalid(format!(
            "mais de {MAX_ALIASES} apelidos"
        )));
    }
    Ok(out)
}

fn project_from_row(r: &Row<'_>) -> rusqlite::Result<Project> {
    let aliases: String = r.get(3)?;
    Ok(Project {
        id: r.get(0)?,
        name: r.get(1)?,
        color: r.get(2)?,
        // Apelidos que não abrem não escondem o projeto.
        aliases: serde_json::from_str(&aliases).unwrap_or_default(),
        external_ref: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
        archived_at: r.get(7)?,
    })
}

/// `needle` aparece em `hay` como palavra inteira (os dois já dobrados).
fn contains_word(hay: &str, needle: &str) -> bool {
    hay.match_indices(needle).any(|(i, _)| {
        let before = hay[..i].chars().next_back();
        let after = hay[i + needle.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// O projeto cujo nome ou apelido aparece em `text` como palavra inteira, sem
/// diferença de maiúscula ou acento. Vence o nome mais longo ("Portal RH"
/// antes de "Portal"); empate entre projetos diferentes é dúvida, e dúvida
/// não marca nada.
pub fn match_project<'a>(projects: &'a [Project], text: &str) -> Option<&'a Project> {
    let hay = fold(text);
    let mut best: Option<(usize, &Project)> = None;
    let mut tie = false;
    for p in projects {
        let Some(len) = std::iter::once(&p.name)
            .chain(&p.aliases)
            .map(|n| fold(n.trim()))
            .filter(|n| !n.is_empty() && contains_word(&hay, n))
            .map(|n| n.chars().count())
            .max()
        else {
            continue;
        };
        match best {
            Some((b, _)) if len < b => {}
            Some((b, _)) if len == b => tie = true,
            _ => {
                best = Some((len, p));
                tie = false;
            }
        }
    }
    if tie { None } else { best.map(|(_, p)| p) }
}

impl AssistStore {
    /// Os projetos ativos (`archived = false`) ou os arquivados, por nome.
    pub fn projects(&self, archived: bool) -> Result<Vec<Project>> {
        let filter = if archived {
            "archived_at IS NOT NULL"
        } else {
            "archived_at IS NULL"
        };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects WHERE {filter} ORDER BY name COLLATE NOCASE, id"
        ))?;
        let rows = stmt.query_map([], project_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Um projeto pelo id.
    pub fn project(&self, id: &str) -> Result<Project> {
        self.conn
            .query_row(
                &format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE id = ?1"),
                params![id],
                project_from_row,
            )
            .optional()?
            .ok_or_else(|| AssistError::NotFound(format!("o projeto {id}")))
    }

    fn project_by_ref(&self, external_ref: &str) -> Result<Option<Project>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE external_ref = ?1"),
                params![external_ref],
                project_from_row,
            )
            .optional()?)
    }

    fn check_unique_project(&self, name: &str, except: Option<&str>) -> Result<()> {
        let key = fold(name);
        if self
            .projects(false)?
            .iter()
            .any(|p| fold(&p.name) == key && Some(p.id.as_str()) != except)
        {
            return Err(AssistError::Invalid(format!(
                "já existe o projeto \"{name}\""
            )));
        }
        Ok(())
    }

    /// Cria um projeto. O mesmo nome de um ativo é recusado, e um projeto de
    /// fora só se liga a um projeto local.
    pub fn create_project(&self, new: NewProject, actor: Actor) -> Result<Project> {
        let name = clean_name(&new.name)?;
        self.check_unique_project(&name, None)?;
        let aliases = clean_aliases(&name, new.aliases)?;
        let external_ref = new
            .external_ref
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty());
        if let Some(r) = &external_ref
            && self.project_by_ref(r)?.is_some()
        {
            return Err(AssistError::Invalid(format!(
                "o projeto {r} já está ligado a outro"
            )));
        }
        let color = match clean_color(new.color)? {
            Some(c) => c,
            None => {
                let n: i64 = self
                    .conn
                    .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?;
                PALETTE[n.unsigned_abs() as usize % PALETTE.len()].to_string()
            }
        };
        let now = self.clock.now_ms();
        let project = Project {
            id: uuid::Uuid::now_v7().to_string(),
            name,
            color: Some(color),
            aliases,
            external_ref,
            created_at: now,
            updated_at: now,
            archived_at: None,
        };
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            &format!(
                "INSERT INTO projects ({PROJECT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
            ),
            params![
                project.id,
                project.name,
                project.color,
                serde_json::to_string(&project.aliases)?,
                project.external_ref,
                project.created_at,
                project.updated_at,
                project.archived_at
            ],
        )?;
        self.journal_row(
            &tx,
            now,
            actor,
            "project.created",
            "project",
            &project.id,
            &project.name,
            &json!({ "before": null, "after": project }),
        )?;
        tx.commit()?;
        Ok(project)
    }

    /// Muda o nome, a cor ou os apelidos.
    pub fn update_project(&self, id: &str, patch: ProjectPatch, actor: Actor) -> Result<Project> {
        let before = self.project(id)?;
        let mut after = before.clone();
        if let Some(name) = patch.name {
            let name = clean_name(&name)?;
            if name != before.name {
                self.check_unique_project(&name, Some(id))?;
                after.name = name;
            }
        }
        if let Some(color) = patch.color {
            after.color = clean_color(color)?;
        }
        if let Some(aliases) = patch.aliases {
            after.aliases = clean_aliases(&after.name, aliases)?;
        }
        if after == before {
            return Ok(before);
        }
        after.updated_at = self.clock.now_ms();
        self.write_project(&before, &after, actor, "project.updated")?;
        Ok(after)
    }

    /// Arquiva (`true`) ou traz de volta. O que já foi marcado continua.
    pub fn set_project_archived(&self, id: &str, archived: bool, actor: Actor) -> Result<Project> {
        let before = self.project(id)?;
        if before.archived_at.is_some() == archived {
            return Ok(before);
        }
        if !archived {
            self.check_unique_project(&before.name, Some(id))?;
        }
        let now = self.clock.now_ms();
        let after = Project {
            archived_at: archived.then_some(now),
            updated_at: now,
            ..before.clone()
        };
        let action = if archived {
            "project.archived"
        } else {
            "project.restored"
        };
        self.write_project(&before, &after, actor, action)?;
        Ok(after)
    }

    /// Os projetos de fora que a pessoa escolheu: o já ligado fica como está;
    /// um local sem ligação e de mesmo nome é ligado (o código vira apelido);
    /// os outros viram projetos novos. Devolve os projetos locais, na ordem.
    pub fn link_projects(&self, items: Vec<LinkedProject>, actor: Actor) -> Result<Vec<Project>> {
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            if let Some(p) = self.project_by_ref(&item.external_ref)? {
                out.push(p);
                continue;
            }
            let code: Vec<String> = item.code.into_iter().collect();
            let key = fold(&clean_text(&item.name));
            let same_name = self
                .projects(false)?
                .into_iter()
                .find(|p| p.external_ref.is_none() && fold(&p.name) == key);
            if let Some(before) = same_name {
                let mut after = before.clone();
                after.external_ref = Some(item.external_ref);
                after.aliases = clean_aliases(
                    &after.name,
                    before.aliases.iter().cloned().chain(code).collect(),
                )?;
                after.updated_at = self.clock.now_ms();
                self.write_project(&before, &after, actor, "project.linked")?;
                out.push(after);
                continue;
            }
            out.push(self.create_project(
                NewProject {
                    name: item.name,
                    // Uma cor de lá que não é #rrggbb fica para a paleta.
                    color: clean_color(item.color).ok().flatten(),
                    aliases: code,
                    external_ref: Some(item.external_ref),
                },
                actor,
            )?);
        }
        Ok(out)
    }

    fn write_project(
        &self,
        before: &Project,
        after: &Project,
        actor: Actor,
        action: &str,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE projects SET name = ?2, color = ?3, aliases = ?4, external_ref = ?5,
                    updated_at = ?6, archived_at = ?7
              WHERE id = ?1",
            params![
                after.id,
                after.name,
                after.color,
                serde_json::to_string(&after.aliases)?,
                after.external_ref,
                after.updated_at,
                after.archived_at
            ],
        )?;
        self.journal_row(
            &tx,
            after.updated_at,
            actor,
            action,
            "project",
            &after.id,
            &after.name,
            &json!({ "before": before, "after": after }),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// O projeto de uma reunião e se a marca foi do ISPer: a última marca
    /// no diário não é da pessoa.
    pub fn meeting_project(&self, meeting_id: i64) -> Result<MeetingProject> {
        let project_id: Option<String> = self
            .conn
            .query_row(
                "SELECT project_id FROM meetings WHERE id = ?1",
                params![meeting_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| AssistError::NotFound(format!("a reunião {meeting_id}")))?;
        let by: Option<String> = self
            .conn
            .query_row(
                "SELECT actor FROM journal
                  WHERE object_kind = 'meeting' AND object_id = ?1 AND action = 'meeting.project'
                  ORDER BY id DESC LIMIT 1",
                params![meeting_id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(MeetingProject {
            suggested: project_id.is_some()
                && by.as_deref().is_some_and(|a| a != Actor::User.as_str()),
            project_id,
        })
    }

    /// Marca (ou tira) o projeto de uma reunião. A pessoa marcar o mesmo
    /// projeto confirma a sugestão. As tarefas da reunião ainda abertas que
    /// estavam com o projeto de antes acompanham, cada uma no diário.
    pub fn set_meeting_project(
        &self,
        meeting_id: i64,
        project_id: Option<&str>,
        actor: Actor,
    ) -> Result<MeetingProject> {
        let current = self.meeting_project(meeting_id)?;
        let before = current.project_id.clone();
        let after = project_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let name = match &after {
            Some(id) => {
                let p = self.project(id)?;
                if p.archived_at.is_some() {
                    return Err(AssistError::Invalid(format!(
                        "o projeto \"{}\" está arquivado",
                        p.name
                    )));
                }
                p.name
            }
            None => String::new(),
        };
        let confirms = actor == Actor::User && current.suggested;
        if before == after && !confirms {
            return Ok(current);
        }
        let now = self.clock.now_ms();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE meetings SET project_id = ?2 WHERE id = ?1",
            params![meeting_id, after],
        )?;
        self.journal_row(
            &tx,
            now,
            actor,
            "meeting.project",
            "meeting",
            &meeting_id.to_string(),
            &name,
            &json!({ "before": before, "after": after }),
        )?;
        tx.commit()?;
        if before != after {
            let prefix = crate::meeting::external_prefix(meeting_id);
            let mut stmt = self.conn.prepare(
                "SELECT id FROM tasks
                  WHERE substr(external_ref, 1, length(?1)) = ?1
                    AND status IN ('open', 'inbox') AND project_id IS ?2",
            )?;
            let ids = stmt
                .query_map(params![prefix, before], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for id in ids {
                let patch = TaskPatch {
                    project_id: Some(after.clone()),
                    ..TaskPatch::default()
                };
                self.update_task(&id, patch, actor)?;
            }
        }
        self.meeting_project(meeting_id)
    }

    /// A reunião casada com um evento ganha o projeto que o ISPer reconhece,
    /// marcado pelo assistente: o da reunião anterior da mesma série (ou do
    /// mesmo evento); senão, o nome ou um apelido no assunto. A que já tem
    /// projeto fica como está.
    pub fn suggest_meeting_project(
        &self,
        meeting_id: i64,
        ical_uid: &str,
        series_id: Option<&str>,
        subject: &str,
    ) -> Result<Option<Project>> {
        if self.meeting_project(meeting_id)?.project_id.is_some() {
            return Ok(None);
        }
        let active = self.projects(false)?;
        let from_series: Option<String> = self
            .conn
            .query_row(
                "SELECT m.project_id FROM meeting_events e JOIN meetings m ON m.id = e.meeting_id
                  WHERE m.id != ?1 AND m.project_id IS NOT NULL
                    AND (e.ical_uid = ?2 OR (?3 IS NOT NULL AND e.series_id = ?3))
                  ORDER BY m.started_ts IS NULL, m.started_ts DESC, m.id DESC
                  LIMIT 1",
                params![meeting_id, ical_uid, series_id],
                |r| r.get(0),
            )
            .optional()?;
        let found = from_series
            .and_then(|id| active.iter().find(|p| p.id == id))
            .or_else(|| match_project(&active, subject))
            .cloned();
        if let Some(p) = &found {
            self.set_meeting_project(meeting_id, Some(&p.id), Actor::Assistant)?;
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use crate::meeting::{MeetingAction, MeetingRef, drafts};
    use crate::store::tests::store_at;
    use chrono::NaiveDate;

    fn store() -> AssistStore {
        store_at(FixedClock::at("2026-10-13 09:00", -3)).0
    }

    fn named(name: &str, aliases: &[&str]) -> NewProject {
        NewProject {
            name: name.into(),
            aliases: aliases.iter().map(|a| a.to_string()).collect(),
            ..NewProject::default()
        }
    }

    /// Uma reunião gravada, casada com um evento da série `series`.
    fn meeting(store: &AssistStore, id: i64, ts: i64, series: Option<&str>, subject: &str) {
        store
            .conn
            .execute(
                "INSERT INTO meetings (id, title, started_at, duration_secs, started_ts)
                 VALUES (?1, ?2, '13/10/2026 10:00', 1800, ?3)",
                params![id, subject, ts],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO meeting_events (meeting_id, ical_uid, series_id, subject, starts_at, ends_at, data)
                 VALUES (?1, ?2, ?3, ?4, '2026-10-13T10:00:00-03:00', '2026-10-13T10:30:00-03:00', '{}')",
                params![id, format!("uid-{id}"), series, subject],
            )
            .unwrap();
    }

    #[test]
    fn cria_com_cor_da_paleta_apelidos_limpos_e_nome_unico() {
        let store = store();
        let p = store
            .create_project(
                named("  Portal   RH ", &["PRT", "prt", "portal rh", ""]),
                Actor::User,
            )
            .unwrap();
        assert_eq!(p.name, "Portal RH");
        assert_eq!(p.aliases, ["PRT"], "sem repetir nem o próprio nome");
        assert_eq!(p.color.as_deref(), Some(PALETTE[0]));
        let err = store.create_project(named("portal rh", &[]), Actor::User);
        assert!(matches!(err, Err(AssistError::Invalid(_))), "{err:?}");
        let bad = store.create_project(
            NewProject {
                color: Some("azul".into()),
                ..named("Estudos", &[])
            },
            Actor::User,
        );
        assert!(matches!(bad, Err(AssistError::Invalid(_))));
        let renamed = store
            .update_project(
                &p.id,
                ProjectPatch {
                    name: Some("Portal".into()),
                    color: Some(Some("#ABCDEF".into())),
                    ..ProjectPatch::default()
                },
                Actor::User,
            )
            .unwrap();
        assert_eq!(
            (renamed.name.as_str(), renamed.color.as_deref()),
            ("Portal", Some("#abcdef"))
        );
        store
            .set_project_archived(&p.id, true, Actor::User)
            .unwrap();
        assert!(store.projects(false).unwrap().is_empty());
        assert_eq!(store.projects(true).unwrap().len(), 1);
    }

    #[test]
    fn trazer_do_opttime_liga_o_de_mesmo_nome_e_nao_duplica() {
        let store = store();
        let local = store
            .create_project(named("Portal", &[]), Actor::User)
            .unwrap();
        let items = vec![
            LinkedProject {
                external_ref: "opttime:p1".into(),
                name: "portal".into(),
                color: Some("#123456".into()),
                code: Some("PRT".into()),
            },
            LinkedProject {
                external_ref: "opttime:p2".into(),
                name: "MRP".into(),
                color: Some("blue".into()),
                code: None,
            },
        ];
        let got = store.link_projects(items.clone(), Actor::User).unwrap();
        assert_eq!(got[0].id, local.id, "o local de mesmo nome é ligado");
        assert_eq!(got[0].external_ref.as_deref(), Some("opttime:p1"));
        assert_eq!(got[0].aliases, ["PRT"]);
        assert_eq!(got[1].name, "MRP");
        assert!(
            got[1].color.as_deref().is_some_and(|c| c.starts_with('#')),
            "cor que não é #rrggbb vai para a paleta"
        );
        let again = store.link_projects(items, Actor::User).unwrap();
        assert_eq!(
            again.iter().map(|p| &p.id).collect::<Vec<_>>(),
            got.iter().map(|p| &p.id).collect::<Vec<_>>()
        );
        assert_eq!(
            store.projects(false).unwrap().len(),
            2,
            "trazer de novo não duplica"
        );
    }

    #[test]
    fn reconhece_o_projeto_pela_palavra_inteira_e_desiste_no_empate() {
        let store = store();
        let portal = store
            .create_project(named("Portal", &["PRT"]), Actor::User)
            .unwrap();
        let rh = store
            .create_project(named("Portal RH", &[]), Actor::User)
            .unwrap();
        let mrp = store
            .create_project(named("MRP", &["Planejamento"]), Actor::User)
            .unwrap();
        let all = store.projects(false).unwrap();
        let id = |text: &str| match_project(&all, text).map(|p| p.id.clone());
        assert_eq!(id("Daily do Portal"), Some(portal.id.clone()));
        assert_eq!(id("Refino [PRT-12] backlog"), Some(portal.id.clone()));
        assert_eq!(
            id("Alinhamento portal rh"),
            Some(rh.id.clone()),
            "o nome mais longo vence"
        );
        assert_eq!(
            id("Revisão do planejamento"),
            Some(mrp.id.clone()),
            "sem acento"
        );
        assert_eq!(id("Portabilidade"), None, "só palavra inteira");
        assert_eq!(
            id("PRT e MRP"),
            None,
            "dois projetos do mesmo tamanho: dúvida"
        );
        assert_eq!(id("1:1 com a Ana"), None);
    }

    #[test]
    fn a_reuniao_sai_com_o_projeto_da_serie_e_as_tarefas_acompanham_a_troca() {
        let store = store();
        let portal = store
            .create_project(named("Portal", &[]), Actor::User)
            .unwrap();
        let mrp = store
            .create_project(named("MRP", &[]), Actor::User)
            .unwrap();

        // A primeira da série: o assunto não diz o projeto; a pessoa marca.
        meeting(&store, 1, 100, Some("serie-daily"), "Daily");
        assert_eq!(
            store
                .suggest_meeting_project(1, "uid-1", Some("serie-daily"), "Daily")
                .unwrap(),
            None
        );
        store
            .set_meeting_project(1, Some(&portal.id), Actor::User)
            .unwrap();
        assert!(!store.meeting_project(1).unwrap().suggested);

        // A seguinte da série já sai com o Portal, sugerido.
        meeting(&store, 2, 200, Some("serie-daily"), "Daily");
        let s = store
            .suggest_meeting_project(2, "uid-2", Some("serie-daily"), "Daily")
            .unwrap();
        assert_eq!(s.map(|p| p.id), Some(portal.id.clone()));
        let mp = store.meeting_project(2).unwrap();
        assert_eq!(
            (mp.project_id.as_deref(), mp.suggested),
            (Some(portal.id.as_str()), true)
        );

        // As tarefas da reunião herdam o projeto.
        let day = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let actions = [MeetingAction {
            title: "Enviar a proposta".into(),
            at_secs: Some(60.0),
            due: None,
            quote: None,
            from_copilot: false,
        }];
        let r = MeetingRef {
            id: 2,
            title: "Daily".into(),
            day,
        };
        let tasks = store
            .import_meeting_actions(2, drafts(&r, &actions))
            .unwrap();
        assert_eq!(tasks[0].project_id.as_deref(), Some(portal.id.as_str()));

        // A pessoa troca: a reunião deixa de ser sugerida e a tarefa acompanha.
        let mp = store
            .set_meeting_project(2, Some(&mrp.id), Actor::User)
            .unwrap();
        assert_eq!(
            (mp.project_id.as_deref(), mp.suggested),
            (Some(mrp.id.as_str()), false)
        );
        assert_eq!(
            store.task(&tasks[0].id).unwrap().project_id.as_deref(),
            Some(mrp.id.as_str())
        );

        // Sem série, o assunto decide; quem já tem projeto não muda.
        meeting(&store, 3, 300, None, "Revisão do MRP");
        let s = store
            .suggest_meeting_project(3, "uid-3", None, "Revisão do MRP")
            .unwrap();
        assert_eq!(s.map(|p| p.id), Some(mrp.id.clone()));
        assert_eq!(
            store
                .suggest_meeting_project(3, "uid-3", None, "Portal")
                .unwrap(),
            None
        );

        // Confirmar (marcar o mesmo) tira o "sugerido".
        assert!(store.meeting_project(3).unwrap().suggested);
        let mp = store
            .set_meeting_project(3, Some(&mrp.id), Actor::User)
            .unwrap();
        assert!(!mp.suggested);
    }
}
