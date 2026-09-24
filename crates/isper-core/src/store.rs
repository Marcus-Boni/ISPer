//! Biblioteca de reuniões e histórico de ditados: SQLite (arquivo único,
//! local). Além de gravar, oferece leitura, busca e exclusão para a
//! janela "Biblioteca" do app.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::meeting::MeetingResult;
use crate::{IsperError, Result};

/// O banco do ISPer (`isper.db`): reuniões, ditados, decisões, momentos,
/// embeddings e métricas locais. Abrir migra o schema, com cópia antes.
pub struct MeetingStore {
    conn: Connection,
}

/// Linha da lista de reuniões (sem o transcript, que pode ser grande).
#[derive(Debug, Clone, Serialize)]
pub struct MeetingRow {
    /// Id da reunião no banco.
    pub id: i64,
    /// Título (o dado pela IA, o renomeado ou o padrão com a data).
    pub title: String,
    /// Quando começou, como exibido: `dd/mm/aaaa hh:mm`.
    pub started_at: String,
    /// Duração da reunião, em segundos.
    pub duration_secs: f32,
    /// Quantas falas a transcrição tem.
    pub segments: i64,
    /// Falantes distintos além de "Eu".
    pub participants: i64,
    /// Já tem resumo por IA.
    pub has_summary: bool,
    /// Caminho do `.md` da reunião, quando ele existe.
    pub md_path: Option<String>,
    /// Momentos marcados durante a reunião.
    pub moments: i64,
    /// Decisões validadas no Copilot (a Biblioteca marca a reunião).
    pub decisions: i64,
    /// Nome do arquivo de áudio de onde a reunião veio, quando ela foi
    /// importada (Fase 9.0) em vez de gravada.
    pub source_name: Option<String>,
}

/// Uma reunião transcrita a partir de um arquivo de áudio (Fase 9.0), pronta
/// para o banco — ver [`MeetingStore::save_imported`].
#[derive(Debug, Clone)]
pub struct NewImportedMeeting<'a> {
    /// Título: o nome do arquivo, quando descritivo, ou o padrão com a data.
    pub title: &'a str,
    /// Quando a gravação aconteceu: `dd/mm/aaaa hh:mm`.
    pub started_at: &'a str,
    /// Duração do áudio, em segundos.
    pub duration_secs: f32,
    /// Caminho do `.md` da reunião.
    pub md_path: &'a str,
    /// As falas: falante, início, fim e texto.
    pub segments: &'a [(String, f32, f32, String)],
    /// Nome do arquivo de áudio, como o usuário o vê.
    pub source_name: &'a str,
    /// SHA-256 do arquivo, em hexadecimal — o mesmo áudio não entra duas vezes.
    pub source_sha256: &'a str,
}

/// Uma fala da transcrição, como está no banco.
#[derive(Debug, Clone, Serialize)]
pub struct StoredSegment {
    /// Rótulo do falante ("Eu", "Participante N" ou um nome dado pelo usuário).
    pub speaker: String,
    /// Início, em segundos da reunião.
    pub start_secs: f32,
    /// Fim, em segundos da reunião.
    pub end_secs: f32,
    /// Texto da fala.
    pub text: String,
}

/// Uma reunião inteira, para a tela da Biblioteca.
#[derive(Debug, Clone, Serialize)]
pub struct MeetingDetail {
    /// Os dados da lista (título, data, contagens).
    pub meeting: MeetingRow,
    /// O resumo por IA, em Markdown.
    pub summary: Option<String>,
    /// A transcrição, em ordem cronológica.
    pub segments: Vec<StoredSegment>,
    /// Instantes marcados (segundos desde o início), em ordem.
    pub moments: Vec<f32>,
    /// O que você validou no Copilot durante a reunião, em ordem de fala.
    pub decisions: Vec<StoredDecision>,
    /// As notas que você escreveu no Copilot, como você as deixou.
    pub notes: Option<String>,
}

/// Um card que o usuário confirmou no Copilot durante a reunião.
///
/// Guarda o texto já resolvido, não uma referência ao card vivo: o Copilot é
/// de memória e some quando a reunião acaba. Os tipos vêm do `isper-llm` como
/// texto (`"decision"`, `"action"`, …) porque o banco não conhece aquele crate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredDecision {
    /// `decision`, `action`, `risk` ou `question`.
    pub kind: String,
    /// Título curto do card.
    pub title: String,
    /// Descrição (pode ser vazia).
    pub description: String,
    /// Responsável, quando a IA identificou um.
    pub owner: Option<String>,
    /// Prazo, como a IA escreveu (texto livre).
    pub due_date: Option<String>,
    /// `low`, `medium` ou `high`.
    pub urgency: String,
    /// Instante da fala que originou o card.
    pub at_secs: f32,
}

/// Um ditado do histórico.
#[derive(Debug, Clone, Serialize)]
pub struct DictationRow {
    /// Id do ditado no banco.
    pub id: i64,
    /// Quando foi ditado: `dd/mm/aaaa hh:mm:ss`.
    pub at: String,
    /// O texto colado (o polido, quando houve polimento).
    pub text: String,
    /// Duração do áudio, em segundos, quando registrada.
    pub audio_secs: Option<f32>,
    /// Texto como saiu do Whisper, quando o polimento por IA o alterou.
    pub raw_text: Option<String>,
}

/// Totais para a tela Início (uma consulta por tabela, sem carregar linhas).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Stats {
    /// Reuniões no histórico.
    pub meetings: i64,
    /// Soma das durações das reuniões.
    pub meeting_secs: f64,
    /// Reuniões com resumo por IA.
    pub with_summary: i64,
    /// Ditados no histórico.
    pub dictations: i64,
    /// Soma do áudio ditado.
    pub dictation_secs: f64,
    /// Palavras ditadas (aproximação: espaços + 1 por ditado).
    pub dictation_words: i64,
}

/// Cobertura do índice semântico para um modelo de embeddings.
#[derive(Debug, Clone, Default, Serialize)]
pub struct EmbeddingStats {
    /// Reuniões no histórico.
    pub meetings_total: i64,
    /// Reuniões com vetores deste modelo.
    pub meetings_indexed: i64,
    /// Ditados no histórico.
    pub dictations_total: i64,
    /// Ditados com vetores deste modelo.
    pub dictations_indexed: i64,
    /// Trechos (vetores) guardados para o modelo.
    pub chunks: i64,
}

/// Melhor trecho de um item na busca semântica.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticHit {
    /// `meeting` ou `dictation`.
    pub kind: String,
    /// Id da reunião ou do ditado.
    pub ref_id: i64,
    /// Cosseno entre a pergunta e o trecho (vetores normalizados).
    pub score: f32,
    /// O trecho que casou com a pergunta.
    pub text: String,
    /// Instante do trecho no relógio da reunião (`None` para resumo/ditado).
    pub start_secs: Option<f32>,
}

const MEETING_COLUMNS: &str = "m.id, m.title, m.started_at, m.duration_secs,
    (m.summary IS NOT NULL AND m.summary != ''), m.md_path,
    (SELECT COUNT(*) FROM segments s WHERE s.meeting_id = m.id),
    (SELECT COUNT(DISTINCT s.speaker) FROM segments s WHERE s.meeting_id = m.id AND s.speaker != 'Eu'),
    (SELECT COUNT(*) FROM moments mo WHERE mo.meeting_id = m.id),
    (SELECT COUNT(*) FROM decisions de WHERE de.meeting_id = m.id),
    m.source_name";

fn row_to_meeting(row: &rusqlite::Row<'_>) -> rusqlite::Result<MeetingRow> {
    Ok(MeetingRow {
        id: row.get(0)?,
        title: row.get(1)?,
        started_at: row.get(2)?,
        duration_secs: row.get::<_, f64>(3)? as f32,
        has_summary: row.get::<_, i64>(4)? != 0,
        md_path: row.get(5)?,
        segments: row.get(6)?,
        participants: row.get(7)?,
        moments: row.get(8)?,
        decisions: row.get(9)?,
        source_name: row.get(10)?,
    })
}

/// `%texto%` com `%`, `_` e `\` escapados — a busca é literal, não curinga.
fn like_pattern(q: &str) -> String {
    let mut out = String::from("%");
    for c in q.trim().chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

// ------------------------------------------------------------- schema

/// Versão do schema, gravada em `PRAGMA user_version`. Cada passo de
/// `migrate` leva o banco de `n` para `n + 1` numa transação própria; um banco
/// de versão maior (criado por um ISPer mais novo) é recusado em vez de
/// alterado às cegas. Bancos anteriores a esta numeração chegam como 0 e
/// passam pelo passo 1, que é idempotente sobre o que eles já têm.
pub const SCHEMA_VERSION: i64 = 5;

/// Eventos de métrica mais antigos que isto (90 dias) saem do banco.
pub const EVENTS_KEEP_SECS: i64 = 90 * 86_400;

/// Reunião apagada pela retenção: o Markdown é do app apagar.
#[derive(Debug, Clone, Serialize)]
pub struct PurgedMeeting {
    /// Id da reunião apagada.
    pub id: i64,
    /// O `.md` dela, que o app apaga em seguida.
    pub md_path: Option<String>,
}

/// O que uma passada de retenção apagou.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Purged {
    /// Reuniões apagadas.
    pub meetings: Vec<PurgedMeeting>,
    /// Quantos ditados foram apagados.
    pub dictations: usize,
}

/// Métricas de um tipo de evento (`dictation`, `meeting_block`…) num período.
#[derive(Debug, Clone, Serialize)]
pub struct KindMetrics {
    /// Tipo de evento (`dictation`, `meeting_block`…).
    pub kind: String,
    /// Eventos no período.
    pub total: i64,
    /// Eventos que falharam.
    pub errors: i64,
    /// Duração da inferência, em segundos (só eventos com sucesso).
    pub p50_secs: Option<f64>,
    /// Percentil 95 da duração da inferência, em segundos.
    pub p95_secs: Option<f64>,
    /// Fator de tempo real: inferência ÷ áudio (0,1 = dez vezes mais rápido).
    pub p50_rtf: Option<f64>,
    /// Percentil 95 do fator de tempo real.
    pub p95_rtf: Option<f64>,
}

/// Segundos de um relógio local "naive" (sem fuso) a partir das datas que o
/// app grava como texto — `dd/mm/aaaa HH:MM` ou `dd/mm/aaaa HH:MM:SS`. Só
/// servem para comparar entre si: o corte da retenção vem do mesmo relógio.
/// Texto fora do formato dá `None`, e uma linha sem instante nunca é apagada.
pub fn parse_local_stamp(s: &str) -> Option<i64> {
    let (date, time) = s.trim().split_once(' ')?;
    let mut d = date.split('/');
    let day: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let year: i64 = d.next()?.parse().ok()?;
    if d.next().is_some()
        || !(1..=31).contains(&day)
        || !(1..=12).contains(&month)
        || !(1970..=9999).contains(&year)
    {
        return None;
    }
    let mut t = time.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let minute: i64 = t.next()?.parse().ok()?;
    let second: i64 = match t.next() {
        Some(v) => v.parse().ok()?,
        None => 0,
    };
    if t.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Dias desde 1970-01-01 no calendário gregoriano (algoritmo de Howard Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt.query_map([], |r| r.get::<_, String>(1))?;
    for name in names {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn add_column_if_missing(conn: &Connection, table: &str, column: &str, decl: &str) -> Result<()> {
    if !column_exists(conn, table, column)? {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
            [],
        )?;
    }
    Ok(())
}

/// Leva o banco até [`SCHEMA_VERSION`], um passo por transação.
fn migrate(conn: &Connection) -> Result<()> {
    let mut version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(IsperError::Schema(format!(
            "o banco está na versão {version}, mais nova do que este ISPer entende \
             ({SCHEMA_VERSION}) — atualize o app ou restaure um backup"
        )));
    }
    while version < SCHEMA_VERSION {
        let tx = conn.unchecked_transaction()?;
        match version {
            0 => migrate_to_v1(&tx)?,
            1 => migrate_to_v2(&tx)?,
            2 => migrate_to_v3(&tx)?,
            3 => migrate_to_v4(&tx)?,
            4 => migrate_to_v5(&tx)?,
            other => {
                return Err(IsperError::Schema(format!(
                    "sem migração a partir da versão {other}"
                )));
            }
        }
        version += 1;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
        tracing::info!(version, "schema do banco migrado");
    }
    Ok(())
}

/// Onde fica a cópia de um banco que estava na versão `from` antes de migrar:
/// `isper.db` → `isper.db.v2.bak`. O nome diz em que versão a cópia abre — é
/// o arquivo que alguém voltando para o app anterior precisa.
pub fn pre_migration_backup_path(path: &Path, from: i64) -> std::path::PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".v{from}.bak"));
    path.with_file_name(name)
}

/// Antes de levar um banco com dados para um schema mais novo, guarda uma
/// cópia dele como estava.
///
/// A migração é de mão única: `migrate` recusa um banco de versão maior do
/// que a que conhece, então quem atualiza e depois quer voltar para o app
/// anterior fica sem a Biblioteca. Esta cópia é o caminho de volta.
///
/// Só existe uma por versão de origem (a primeira é a que vale — mais perto do
/// original), e um banco recém-criado não tem o que guardar. Se a cópia
/// falhar (disco cheio, permissão), a migração segue mesmo assim: os passos
/// só acrescentam estrutura, e travar o app por causa do backup seria pior do
/// que o risco que ele cobre. O aviso fica no log.
fn backup_before_migration(conn: &Connection, path: &Path) {
    let Ok(version) = conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)) else {
        return;
    };
    if version >= SCHEMA_VERSION {
        return; // nada a migrar (ou mais novo, que o migrate recusa)
    }
    let has_data = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meetings')",
            [],
            |r| r.get::<_, bool>(0),
        )
        .unwrap_or(false);
    if !has_data {
        return; // banco novo
    }
    let dest = pre_migration_backup_path(path, version);
    if dest.exists() {
        return;
    }
    match conn.execute("VACUUM INTO ?1", params![dest.to_string_lossy()]) {
        Ok(_) => tracing::info!(
            from = version,
            to = SCHEMA_VERSION,
            path = %dest.display(),
            "cópia do banco antes de migrar o schema"
        ),
        Err(e) => tracing::warn!(
            from = version,
            "não consegui copiar o banco antes de migrar (a migração segue): {e}"
        ),
    }
}

/// Passo 1 — o schema que existia antes da numeração: tabelas e as três
/// colunas que versões antigas acrescentavam com `ALTER TABLE` ignorando o
/// erro. Idempotente: um banco antigo já tem parte disto, um novo nada.
fn migrate_to_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meetings (
            id            INTEGER PRIMARY KEY,
            title         TEXT NOT NULL,
            started_at    TEXT NOT NULL,
            duration_secs REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS segments (
            id         INTEGER PRIMARY KEY,
            meeting_id INTEGER NOT NULL REFERENCES meetings(id),
            speaker    TEXT NOT NULL,
            start_secs REAL NOT NULL,
            end_secs   REAL NOT NULL,
            text       TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_segments_meeting ON segments(meeting_id);
        CREATE TABLE IF NOT EXISTS dictations (
            id         INTEGER PRIMARY KEY,
            at         TEXT NOT NULL,
            text       TEXT NOT NULL,
            audio_secs REAL,
            infer_secs REAL
        );
        CREATE TABLE IF NOT EXISTS moments (
            id         INTEGER PRIMARY KEY,
            meeting_id INTEGER NOT NULL REFERENCES meetings(id),
            at_secs    REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_moments_meeting ON moments(meeting_id);
        CREATE TABLE IF NOT EXISTS embeddings (
            id         INTEGER PRIMARY KEY,
            kind       TEXT NOT NULL,
            ref_id     INTEGER NOT NULL,
            chunk      INTEGER NOT NULL,
            start_secs REAL,
            text       TEXT NOT NULL,
            model      TEXT NOT NULL,
            dim        INTEGER NOT NULL,
            vector     BLOB NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_embeddings_ref ON embeddings(kind, ref_id);
        CREATE INDEX IF NOT EXISTS idx_embeddings_model ON embeddings(model, kind);",
    )?;
    add_column_if_missing(conn, "meetings", "summary", "TEXT")?;
    add_column_if_missing(conn, "meetings", "md_path", "TEXT")?;
    add_column_if_missing(conn, "dictations", "raw_text", "TEXT")?;
    Ok(())
}

/// Passo 2 (fase 7.4) — instantes numéricos para a retenção (as datas eram só
/// texto de exibição), preenchidos a partir do texto existente, e a tabela de
/// eventos das métricas locais.
fn migrate_to_v2(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "meetings", "started_ts", "INTEGER")?;
    add_column_if_missing(conn, "dictations", "at_ts", "INTEGER")?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_meetings_started_ts ON meetings(started_ts);
        CREATE INDEX IF NOT EXISTS idx_dictations_at_ts ON dictations(at_ts);
        CREATE TABLE IF NOT EXISTS events (
            id         INTEGER PRIMARY KEY,
            at_ts      INTEGER NOT NULL,
            kind       TEXT NOT NULL,
            ok         INTEGER NOT NULL,
            secs       REAL,
            audio_secs REAL
        );
        CREATE INDEX IF NOT EXISTS idx_events_kind_ts ON events(kind, at_ts);",
    )?;
    backfill_stamps(conn, "meetings", "started_at", "started_ts")?;
    backfill_stamps(conn, "dictations", "at", "at_ts")?;
    Ok(())
}

/// Passo 3 — decisões validadas no Copilot (Fase 8). Tabela nova e vazia:
/// reuniões antigas simplesmente não têm linhas aqui.
fn migrate_to_v3(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS decisions (
            id          INTEGER PRIMARY KEY,
            meeting_id  INTEGER NOT NULL REFERENCES meetings(id),
            kind        TEXT NOT NULL,
            title       TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            owner       TEXT,
            due_date    TEXT,
            urgency     TEXT NOT NULL DEFAULT 'medium',
            at_secs     REAL NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_decisions_meeting ON decisions(meeting_id);",
    )?;
    Ok(())
}

/// Passo 4 — as notas que o usuário escreve no Copilot durante a reunião.
/// Coluna nova e vazia: reuniões antigas simplesmente não têm notas.
fn migrate_to_v4(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "meetings", "notes", "TEXT")
}

/// Passo 5 — a origem de uma reunião importada de um arquivo de áudio
/// (Fase 9.0): o nome do arquivo, que o `.md` e a Biblioteca mostram, e o
/// SHA-256, que impede importar o mesmo áudio duas vezes. Reuniões gravadas
/// ficam com as duas colunas vazias.
fn migrate_to_v5(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "meetings", "source_name", "TEXT")?;
    add_column_if_missing(conn, "meetings", "source_sha256", "TEXT")?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_meetings_source_sha256 ON meetings(source_sha256)",
        [],
    )?;
    Ok(())
}

/// Preenche `ts_col` a partir do texto de `text_col` onde ainda está vazio.
fn backfill_stamps(conn: &Connection, table: &str, text_col: &str, ts_col: &str) -> Result<()> {
    let rows: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(&format!(
            "SELECT id, {text_col} FROM {table} WHERE {ts_col} IS NULL"
        ))?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut update = conn.prepare(&format!("UPDATE {table} SET {ts_col} = ?1 WHERE id = ?2"))?;
    for (id, text) in rows {
        if let Some(ts) = parse_local_stamp(&text) {
            update.execute(params![ts, id])?;
        }
    }
    Ok(())
}

/// Percentil por posto mais próximo numa lista já ordenada.
fn percentile(sorted: &[f64], p: u32) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((f64::from(p) / 100.0) * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.clamp(1, sorted.len()) - 1])
}

impl MeetingStore {
    /// Abre (ou cria) o banco no caminho dado.
    ///
    /// WAL + `busy_timeout`: o app abre várias conexões (janelas, diarização
    /// em segundo plano gravando enquanto a Biblioteca lê) — sem isso, um
    /// leitor e um escritor simultâneos dariam "database is locked".
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        backup_before_migration(&conn, path);
        migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Versão do schema deste banco (`PRAGMA user_version`).
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    // ------------------------------------------------------------ escrita

    /// Salva uma reunião completa e devolve o id.
    pub fn save(
        &self,
        title: &str,
        started_at: &str,
        result: &MeetingResult,
        md_path: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO meetings (title, started_at, started_ts, duration_secs, md_path)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                title,
                started_at,
                parse_local_stamp(started_at),
                result.duration_secs,
                md_path
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        let mut stmt = self.conn.prepare(
            "INSERT INTO segments (meeting_id, speaker, start_secs, end_secs, text)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for s in &result.segments {
            stmt.execute(params![
                id,
                s.speaker.label(),
                s.start_secs,
                s.end_secs,
                s.text
            ])?;
        }
        Ok(id)
    }

    /// Guarda os momentos marcados durante a reunião (segundos desde o início).
    pub fn save_moments(&self, meeting_id: i64, moments: &[f32]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt =
                tx.prepare("INSERT INTO moments (meeting_id, at_secs) VALUES (?1, ?2)")?;
            for at in moments {
                stmt.execute(params![meeting_id, f64::from(*at)])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Guarda as decisões validadas no Copilot. Regrava do zero: salvar duas
    /// vezes a mesma reunião não duplica as linhas.
    pub fn save_decisions(&self, meeting_id: i64, items: &[StoredDecision]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM decisions WHERE meeting_id = ?1",
            params![meeting_id],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO decisions
                 (meeting_id, kind, title, description, owner, due_date, urgency, at_secs)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for d in items {
                stmt.execute(params![
                    meeting_id,
                    d.kind,
                    d.title,
                    d.description,
                    d.owner,
                    d.due_date,
                    d.urgency,
                    f64::from(d.at_secs),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Decisões de uma reunião, na ordem em que apareceram na conversa.
    pub fn decisions(&self, meeting_id: i64) -> Result<Vec<StoredDecision>> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, title, description, owner, due_date, urgency, at_secs
             FROM decisions WHERE meeting_id = ?1 ORDER BY at_secs, id",
        )?;
        let rows = stmt
            .query_map(params![meeting_id], |r| {
                Ok(StoredDecision {
                    kind: r.get(0)?,
                    title: r.get(1)?,
                    description: r.get(2)?,
                    owner: r.get(3)?,
                    due_date: r.get(4)?,
                    urgency: r.get(5)?,
                    at_secs: r.get::<_, f64>(6)? as f32,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Guarda o resumo gerado por IA (Fase 5).
    pub fn set_summary(&self, meeting_id: i64, summary: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE meetings SET summary = ?1 WHERE id = ?2",
            params![summary, meeting_id],
        )?;
        Ok(())
    }

    /// Guarda as notas do Copilot da reunião (o `.md` é regravado pelo app).
    /// Texto vazio apaga: a reunião volta a não ter notas.
    pub fn set_notes(&self, meeting_id: i64, notes: &str) -> Result<()> {
        let notes = notes.trim();
        self.conn.execute(
            "UPDATE meetings SET notes = ?1 WHERE id = ?2",
            params![(!notes.is_empty()).then_some(notes), meeting_id],
        )?;
        Ok(())
    }

    /// Troca o título da reunião (o `.md` é regravado pelo app).
    pub fn rename_meeting(&self, meeting_id: i64, title: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE meetings SET title = ?1 WHERE id = ?2",
            params![title.trim(), meeting_id],
        )?;
        Ok(())
    }

    /// Aplica rótulos de falante segmento a segmento (casados pelo início, com
    /// tolerância) — usado quando a diarização termina em segundo plano, depois
    /// de a reunião já estar salva com "Participantes". Devolve quantos mudaram.
    pub fn relabel_segments(&self, meeting_id: i64, labels: &[(f32, &str)]) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let mut changed = 0usize;
        {
            let mut stmt = tx.prepare(
                "UPDATE segments SET speaker = ?1
                 WHERE meeting_id = ?2 AND abs(start_secs - ?3) < 0.002 AND speaker != ?1",
            )?;
            for (start, speaker) in labels {
                changed += stmt.execute(params![speaker, meeting_id, *start as f64])?;
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Já existe uma reunião vinda deste `.md` (ou começada neste instante,
    /// com este título)?
    ///
    /// O `md_path` é a chave natural: um arquivo, uma reunião. Início e
    /// título cobrem as reuniões antigas, gravadas antes de o caminho ser
    /// guardado. Só o início não basta: ele tem precisão de minuto, e duas
    /// gravações exportadas juntas do celular podem começar no mesmo minuto —
    /// uma delas sumiria numa reimportação. Renomear na Biblioteca troca o
    /// título no banco e no `.md` juntos, então continua casando.
    pub fn has_meeting_from(&self, md_path: &str, started_at: &str, title: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM meetings
             WHERE md_path = ?1 OR (started_at = ?2 AND title = ?3)",
            params![md_path, started_at, title],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// Salva uma reunião transcrita a partir de um arquivo de áudio
    /// (Fase 9.0) e devolve o id. Tudo numa transação: a reunião entra com
    /// as falas e a origem, ou não entra.
    pub fn save_imported(&self, m: &NewImportedMeeting<'_>) -> Result<i64> {
        if m.segments.is_empty() {
            return Err(IsperError::Schema(
                "a transcrição do áudio não tem nenhuma fala".into(),
            ));
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO meetings
             (title, started_at, started_ts, duration_secs, md_path, source_name, source_sha256)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                m.title,
                m.started_at,
                parse_local_stamp(m.started_at),
                m.duration_secs,
                m.md_path,
                m.source_name,
                m.source_sha256
            ],
        )?;
        let id = tx.last_insert_rowid();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO segments (meeting_id, speaker, start_secs, end_secs, text)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (speaker, start, end, text) in m.segments {
                stmt.execute(params![id, speaker, start, end, text])?;
            }
        }
        tx.commit()?;
        Ok(id)
    }

    /// A reunião que veio de um áudio com este SHA-256, se já foi importado.
    pub fn meeting_with_source(&self, sha256: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM meetings WHERE source_sha256 = ?1 ORDER BY id LIMIT 1",
                params![sha256],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Reinsere no índice uma reunião lida do Markdown
    /// ([`crate::import::ImportedMeeting`]).
    ///
    /// Devolve `None` quando a reunião já está no banco — reimportar a pasta
    /// inteira tem de ser seguro de repetir. Tudo numa transação: ou a
    /// reunião entra completa, com falas, resumo, momentos e notas, ou não
    /// entra.
    pub fn import_meeting(
        &self,
        m: &crate::import::ImportedMeeting,
        md_path: &str,
    ) -> Result<Option<i64>> {
        if self.has_meeting_from(md_path, &m.started_at, &m.title)? {
            return Ok(None);
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO meetings
             (title, started_at, started_ts, duration_secs, md_path, summary, notes, source_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                m.title,
                m.started_at,
                parse_local_stamp(&m.started_at),
                m.duration_secs,
                md_path,
                m.summary,
                m.notes,
                m.source_name
            ],
        )?;
        let id = tx.last_insert_rowid();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO segments (meeting_id, speaker, start_secs, end_secs, text)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for seg in &m.segments {
                stmt.execute(params![
                    id,
                    seg.speaker,
                    seg.start_secs,
                    seg.end_secs,
                    seg.text
                ])?;
            }
            let mut stmt =
                tx.prepare("INSERT INTO moments (meeting_id, at_secs) VALUES (?1, ?2)")?;
            for at in &m.moments {
                stmt.execute(params![id, f64::from(*at)])?;
            }
        }
        tx.commit()?;
        Ok(Some(id))
    }

    /// Troca TODOS os segmentos da reunião pelos do passe final.
    ///
    /// O ao vivo grava enquanto a reunião acontece; o passe final refaz o
    /// mesmo áudio inteiro depois, com VAD, beam search e falante por
    /// palavra. Quando ele termina, a transcrição oficial é a dele — numa
    /// transação, para a Biblioteca nunca ver meia reunião.
    ///
    /// Devolve quantos segmentos ficaram no lugar.
    pub fn replace_segments(
        &self,
        meeting_id: i64,
        segments: &[(String, f32, f32, String)],
    ) -> Result<usize> {
        if segments.is_empty() {
            return Err(IsperError::Schema(
                "passe final sem segmentos — a transcrição ao vivo foi mantida".into(),
            ));
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM segments WHERE meeting_id = ?1",
            params![meeting_id],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO segments (meeting_id, speaker, start_secs, end_secs, text)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (speaker, start, end, text) in segments {
                stmt.execute(params![meeting_id, speaker, start, end, text])?;
            }
        }
        tx.commit()?;
        Ok(segments.len())
    }

    /// Renomeia um falante em todos os segmentos da reunião ("Participante 1"
    /// → "Tatiana"). Devolve quantos segmentos mudaram.
    pub fn rename_speaker(&self, meeting_id: i64, from: &str, to: &str) -> Result<usize> {
        let n = self.conn.execute(
            "UPDATE segments SET speaker = ?3 WHERE meeting_id = ?1 AND speaker = ?2",
            params![meeting_id, from, to.trim()],
        )?;
        Ok(n)
    }

    /// Remove a reunião do histórico. O arquivo Markdown NÃO é apagado —
    /// apagar arquivos do usuário é decisão dele, fora daqui.
    pub fn delete_meeting(&self, meeting_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM embeddings WHERE kind = 'meeting' AND ref_id = ?1",
            params![meeting_id],
        )?;
        self.conn.execute(
            "DELETE FROM moments WHERE meeting_id = ?1",
            params![meeting_id],
        )?;
        self.conn.execute(
            "DELETE FROM decisions WHERE meeting_id = ?1",
            params![meeting_id],
        )?;
        self.conn.execute(
            "DELETE FROM segments WHERE meeting_id = ?1",
            params![meeting_id],
        )?;
        self.conn
            .execute("DELETE FROM meetings WHERE id = ?1", params![meeting_id])?;
        Ok(())
    }

    /// Histórico de ditados (Fase 3): cada texto colado fica pesquisável.
    /// `raw_text` é o original do Whisper quando o polimento por IA o mudou.
    /// Devolve o id da linha (a indexação semântica precisa dele).
    pub fn save_dictation(
        &self,
        at: &str,
        text: &str,
        raw_text: Option<&str>,
        audio_secs: f32,
        infer_secs: f32,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO dictations (at, at_ts, text, raw_text, audio_secs, infer_secs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                at,
                parse_local_stamp(at),
                text,
                raw_text,
                audio_secs,
                infer_secs
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Apaga um ditado e os vetores dele.
    pub fn delete_dictation(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM embeddings WHERE kind = 'dictation' AND ref_id = ?1",
            params![id],
        )?;
        self.conn
            .execute("DELETE FROM dictations WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ------------------------------------------------- retenção e backup

    /// Apaga reuniões (com segmentos, momentos e vetores) e ditados cujo
    /// instante é anterior a `cutoff_ts` (no relógio de [`parse_local_stamp`]).
    /// Linhas sem instante conhecido ficam. Devolve o que saiu — inclusive os
    /// caminhos dos Markdowns, que são do app apagar.
    pub fn purge_older_than(&self, cutoff_ts: i64) -> Result<Purged> {
        let tx = self.conn.unchecked_transaction()?;
        let meetings: Vec<PurgedMeeting> = {
            let mut stmt = tx.prepare(
                "SELECT id, md_path FROM meetings WHERE started_ts IS NOT NULL AND started_ts < ?1",
            )?;
            stmt.query_map(params![cutoff_ts], |r| {
                Ok(PurgedMeeting {
                    id: r.get(0)?,
                    md_path: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        for m in &meetings {
            tx.execute(
                "DELETE FROM embeddings WHERE kind = 'meeting' AND ref_id = ?1",
                params![m.id],
            )?;
            tx.execute("DELETE FROM moments WHERE meeting_id = ?1", params![m.id])?;
            tx.execute("DELETE FROM decisions WHERE meeting_id = ?1", params![m.id])?;
            tx.execute("DELETE FROM segments WHERE meeting_id = ?1", params![m.id])?;
            tx.execute("DELETE FROM meetings WHERE id = ?1", params![m.id])?;
        }
        tx.execute(
            "DELETE FROM embeddings WHERE kind = 'dictation' AND ref_id IN
                (SELECT id FROM dictations WHERE at_ts IS NOT NULL AND at_ts < ?1)",
            params![cutoff_ts],
        )?;
        let dictations = tx.execute(
            "DELETE FROM dictations WHERE at_ts IS NOT NULL AND at_ts < ?1",
            params![cutoff_ts],
        )?;
        tx.commit()?;
        Ok(Purged {
            meetings,
            dictations,
        })
    }

    /// Quantas reuniões e quantos ditados uma varredura com este corte
    /// apagaria — para o app decidir se faz um backup antes.
    pub fn count_older_than(&self, cutoff_ts: i64) -> Result<(i64, i64)> {
        let meetings: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM meetings WHERE started_ts IS NOT NULL AND started_ts < ?1",
            params![cutoff_ts],
            |r| r.get(0),
        )?;
        let dictations: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM dictations WHERE at_ts IS NOT NULL AND at_ts < ?1",
            params![cutoff_ts],
            |r| r.get(0),
        )?;
        Ok((meetings, dictations))
    }

    /// Cópia íntegra e compactada do banco em `dest` (`VACUUM INTO`): funciona
    /// com o app aberto e outras conexões escrevendo, sem parar nada. `dest`
    /// não pode existir — o SQLite não sobrescreve.
    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        self.conn
            .execute("VACUUM INTO ?1", params![dest.to_string_lossy()])?;
        Ok(())
    }

    // ------------------------------------------------------------ métricas

    /// Registra um evento medido (`kind` = `dictation`, `meeting_block`…):
    /// sucesso ou falha, duração da inferência e do áudio. Eventos com mais
    /// de [`EVENTS_KEEP_SECS`] saem no mesmo passo — métrica é tendência,
    /// não histórico. Nada disto sai da máquina.
    pub fn record_event(
        &self,
        at_ts: i64,
        kind: &str,
        ok: bool,
        secs: Option<f32>,
        audio_secs: Option<f32>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO events (at_ts, kind, ok, secs, audio_secs) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![at_ts, kind, i64::from(ok), secs, audio_secs],
        )?;
        self.conn.execute(
            "DELETE FROM events WHERE at_ts < ?1",
            params![at_ts - EVENTS_KEEP_SECS],
        )?;
        Ok(())
    }

    /// Métricas por tipo de evento desde `since_ts`: total, falhas e p50/p95
    /// da inferência (s) e do fator de tempo real (inferência ÷ áudio).
    pub fn metrics(&self, since_ts: i64) -> Result<Vec<KindMetrics>> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, ok, secs, audio_secs FROM events WHERE at_ts >= ?1 ORDER BY kind",
        )?;
        let rows = stmt.query_map(params![since_ts], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)? != 0,
                r.get::<_, Option<f64>>(2)?,
                r.get::<_, Option<f64>>(3)?,
            ))
        })?;
        // (total, falhas, inferências, fatores de tempo real)
        let mut by_kind: BTreeMap<String, (i64, i64, Vec<f64>, Vec<f64>)> = BTreeMap::new();
        for row in rows {
            let (kind, ok, secs, audio) = row?;
            let entry = by_kind.entry(kind).or_default();
            entry.0 += 1;
            if !ok {
                entry.1 += 1;
                continue;
            }
            if let Some(s) = secs {
                entry.2.push(s);
                if let Some(a) = audio.filter(|a| *a > 0.0) {
                    entry.3.push(s / a);
                }
            }
        }
        Ok(by_kind
            .into_iter()
            .map(|(kind, (total, errors, mut secs, mut rtf))| {
                secs.sort_by(f64::total_cmp);
                rtf.sort_by(f64::total_cmp);
                KindMetrics {
                    kind,
                    total,
                    errors,
                    p50_secs: percentile(&secs, 50),
                    p95_secs: percentile(&secs, 95),
                    p50_rtf: percentile(&rtf, 50),
                    p95_rtf: percentile(&rtf, 95),
                }
            })
            .collect())
    }

    // ------------------------------------------------------ busca semântica

    /// Substitui os vetores de um item (`kind` = `meeting` | `dictation`):
    /// `chunks` = (instante, texto do trecho, vetor normalizado), em ordem.
    pub fn replace_embeddings(
        &self,
        kind: &str,
        ref_id: i64,
        model: &str,
        chunks: &[(Option<f32>, &str, &[f32])],
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM embeddings WHERE kind = ?1 AND ref_id = ?2",
            params![kind, ref_id],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO embeddings (kind, ref_id, chunk, start_secs, text, model, dim, vector)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for (i, (start, text, vector)) in chunks.iter().enumerate() {
                stmt.execute(params![
                    kind,
                    ref_id,
                    i as i64,
                    start.map(f64::from),
                    text,
                    model,
                    vector.len() as i64,
                    crate::embed::to_blob(vector)
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Itens de `kind` que já têm vetores do modelo dado.
    pub fn embedded_ids(&self, kind: &str, model: &str) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT ref_id FROM embeddings WHERE kind = ?1 AND model = ?2 ORDER BY ref_id",
        )?;
        let rows = stmt.query_map(params![kind, model], |r| r.get::<_, i64>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Apaga vetores de qualquer outro modelo (troca de provider/modelo). Devolve quantos.
    pub fn delete_embeddings_except_model(&self, model: &str) -> Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM embeddings WHERE model != ?1", params![model])?)
    }

    /// Os itens mais parecidos com a pergunta: percorre os vetores de `kind`
    /// gerados por `model` (produto escalar = cosseno, ambos normalizados) e
    /// guarda o melhor trecho de cada item. Força bruta em memória — alguns
    /// milhares de vetores de 768 dimensões levam milissegundos.
    pub fn semantic_search(
        &self,
        kind: &str,
        model: &str,
        query: &[f32],
        limit: usize,
    ) -> Result<Vec<SemanticHit>> {
        let mut stmt = self.conn.prepare(
            "SELECT ref_id, start_secs, text, vector FROM embeddings WHERE kind = ?1 AND model = ?2",
        )?;
        let rows = stmt.query_map(params![kind, model], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<f64>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Vec<u8>>(3)?,
            ))
        })?;
        let mut best: HashMap<i64, SemanticHit> = HashMap::new();
        for row in rows {
            let (ref_id, start, text, blob) = row?;
            let score = crate::embed::dot(query, &crate::embed::from_blob(&blob));
            let better = best.get(&ref_id).is_none_or(|h| score > h.score);
            if better {
                best.insert(
                    ref_id,
                    SemanticHit {
                        kind: kind.to_string(),
                        ref_id,
                        score,
                        text,
                        start_secs: start.map(|s| s as f32),
                    },
                );
            }
        }
        let mut hits: Vec<SemanticHit> = best.into_values().collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(limit);
        Ok(hits)
    }

    /// Quanto do histórico está indexado para o modelo dado.
    pub fn embeddings_stats(&self, model: &str) -> Result<EmbeddingStats> {
        let count = |sql: &str, with_model: bool| -> Result<i64> {
            Ok(if with_model {
                self.conn.query_row(sql, params![model], |r| r.get(0))?
            } else {
                self.conn.query_row(sql, [], |r| r.get(0))?
            })
        };
        Ok(EmbeddingStats {
            meetings_total: count("SELECT COUNT(*) FROM meetings", false)?,
            meetings_indexed: count(
                "SELECT COUNT(DISTINCT ref_id) FROM embeddings
                 WHERE kind = 'meeting' AND model = ?1 AND ref_id IN (SELECT id FROM meetings)",
                true,
            )?,
            dictations_total: count("SELECT COUNT(*) FROM dictations", false)?,
            dictations_indexed: count(
                "SELECT COUNT(DISTINCT ref_id) FROM embeddings
                 WHERE kind = 'dictation' AND model = ?1 AND ref_id IN (SELECT id FROM dictations)",
                true,
            )?,
            chunks: count("SELECT COUNT(*) FROM embeddings WHERE model = ?1", true)?,
        })
    }

    /// Ids de todas as reuniões (para a indexação completa).
    pub fn meeting_ids(&self) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare("SELECT id FROM meetings ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Todos os ditados (id, texto), para a indexação completa.
    pub fn dictation_texts(&self) -> Result<Vec<(i64, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, text FROM dictations ORDER BY id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Uma reunião sem o transcript (linha da lista).
    pub fn meeting_row(&self, meeting_id: i64) -> Result<Option<MeetingRow>> {
        let sql = format!("SELECT {MEETING_COLUMNS} FROM meetings m WHERE m.id = ?1");
        Ok(self
            .conn
            .query_row(&sql, params![meeting_id], row_to_meeting)
            .optional()?)
    }

    /// Um ditado pelo id.
    pub fn dictation_row(&self, id: i64) -> Result<Option<DictationRow>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, at, text, audio_secs, raw_text FROM dictations WHERE id = ?1",
                params![id],
                |r| {
                    Ok(DictationRow {
                        id: r.get(0)?,
                        at: r.get(1)?,
                        text: r.get(2)?,
                        audio_secs: r.get::<_, Option<f64>>(3)?.map(|v| v as f32),
                        raw_text: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    // ------------------------------------------------------------ leitura

    /// Todas as reuniões, mais recente primeiro.
    /// Reuniões da mais recente para a mais antiga.
    ///
    /// Ordena pela hora da REUNIÃO (`started_ts`), não pela ordem de
    /// inserção: uma reunião reimportada de um `.md`
    /// ([`Self::import_meeting`]) recebe um id novo e apareceria fora de
    /// lugar se o `id` mandasse.
    pub fn list_meetings(&self) -> Result<Vec<MeetingRow>> {
        let sql = format!(
            "SELECT {MEETING_COLUMNS} FROM meetings m ORDER BY m.started_ts DESC, m.id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_meeting)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// As `limit` reuniões mais recentes (tela Início).
    pub fn recent_meetings(&self, limit: i64) -> Result<Vec<MeetingRow>> {
        let sql = format!(
            "SELECT {MEETING_COLUMNS} FROM meetings m ORDER BY m.started_ts DESC, m.id DESC LIMIT ?1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![limit.max(0)], row_to_meeting)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Totais de reuniões e ditados.
    pub fn stats(&self) -> Result<Stats> {
        let (meetings, meeting_secs, with_summary) = self.conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(duration_secs), 0),
                    COALESCE(SUM(summary IS NOT NULL AND summary != ''), 0)
             FROM meetings",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )?;
        let (dictations, dictation_secs, dictation_words) = self.conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(audio_secs), 0),
                    COALESCE(SUM(CASE WHEN trim(text) = '' THEN 0
                                 ELSE length(trim(text)) - length(replace(trim(text), ' ', '')) + 1 END), 0)
             FROM dictations",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?, r.get::<_, i64>(2)?)),
        )?;
        Ok(Stats {
            meetings,
            meeting_secs,
            with_summary,
            dictations,
            dictation_secs,
            dictation_words,
        })
    }

    /// Reuniões cujo título, resumo ou transcript contém `q` (busca literal,
    /// sem diferenciar maiúsculas).
    pub fn search_meetings(&self, q: &str) -> Result<Vec<MeetingRow>> {
        if q.trim().is_empty() {
            return self.list_meetings();
        }
        let pattern = like_pattern(q);
        let sql = format!(
            "SELECT {MEETING_COLUMNS} FROM meetings m
             WHERE m.title LIKE ?1 ESCAPE '\\'
                OR m.summary LIKE ?1 ESCAPE '\\'
                OR m.id IN (SELECT meeting_id FROM segments WHERE text LIKE ?1 ESCAPE '\\')
             ORDER BY m.started_ts DESC, m.id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![pattern], row_to_meeting)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Uma reunião com resumo e transcript completo.
    pub fn get_meeting(&self, meeting_id: i64) -> Result<Option<MeetingDetail>> {
        let sql = format!("SELECT {MEETING_COLUMNS} FROM meetings m WHERE m.id = ?1");
        let meeting = self
            .conn
            .query_row(&sql, params![meeting_id], row_to_meeting)
            .optional()?;
        let Some(meeting) = meeting else {
            return Ok(None);
        };
        let (summary, notes): (Option<String>, Option<String>) = self
            .conn
            .query_row(
                "SELECT summary, notes FROM meetings WHERE id = ?1",
                params![meeting_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or_default();
        let mut stmt = self.conn.prepare(
            "SELECT speaker, start_secs, end_secs, text FROM segments
             WHERE meeting_id = ?1 ORDER BY start_secs, id",
        )?;
        let segments = stmt
            .query_map(params![meeting_id], |r| {
                Ok(StoredSegment {
                    speaker: r.get(0)?,
                    start_secs: r.get::<_, f64>(1)? as f32,
                    end_secs: r.get::<_, f64>(2)? as f32,
                    text: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut moments_stmt = self
            .conn
            .prepare("SELECT at_secs FROM moments WHERE meeting_id = ?1 ORDER BY at_secs")?;
        let moments = moments_stmt
            .query_map(params![meeting_id], |r| {
                r.get::<_, f64>(0).map(|v| v as f32)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let decisions = self.decisions(meeting_id)?;
        Ok(Some(MeetingDetail {
            meeting,
            summary,
            segments,
            moments,
            decisions,
            notes,
        }))
    }

    /// Distintos falantes de uma reunião, na ordem de aparição.
    pub fn speakers(&self, meeting_id: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT speaker FROM segments WHERE meeting_id = ?1 GROUP BY speaker ORDER BY MIN(start_secs)",
        )?;
        let rows = stmt.query_map(params![meeting_id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Ditados mais recentes (filtrados por `q`, se houver), até `limit`.
    pub fn list_dictations(&self, q: Option<&str>, limit: i64) -> Result<Vec<DictationRow>> {
        let map = |r: &rusqlite::Row<'_>| {
            Ok(DictationRow {
                id: r.get(0)?,
                at: r.get(1)?,
                text: r.get(2)?,
                audio_secs: r.get::<_, Option<f64>>(3)?.map(|v| v as f32),
                raw_text: r.get(4)?,
            })
        };
        let rows = match q.map(str::trim).filter(|s| !s.is_empty()) {
            Some(q) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, at, text, audio_secs, raw_text FROM dictations
                     WHERE text LIKE ?1 ESCAPE '\\' ORDER BY id DESC LIMIT ?2",
                )?;
                stmt.query_map(params![like_pattern(q), limit], map)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            }
            None => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, at, text, audio_secs, raw_text FROM dictations ORDER BY id DESC LIMIT ?1",
                )?;
                stmt.query_map(params![limit], map)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            }
        };
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::{MeetingResult, MeetingSegment, Speaker};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("isper-store-test-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        // Cópias de antes da migração que um teste anterior tenha deixado.
        for v in 0..=SCHEMA_VERSION {
            let _ = std::fs::remove_file(pre_migration_backup_path(&path, v));
        }
        path
    }

    #[test]
    fn migrar_um_banco_com_dados_guarda_a_copia_de_antes() {
        let path = temp_path("pre-migracao");
        // Monta um banco na v2: cria na atual e recua (a v3 acrescentou a
        // tabela de decisões; a v4, a coluna de notas, que o passo 4 aceita já
        // existente).
        {
            let store = MeetingStore::open(&path).unwrap();
            store
                .save("Reunião antiga", "10/09/2026 14:00", &sample_result(), None)
                .unwrap();
            store
                .conn
                .execute_batch("DROP TABLE decisions; PRAGMA user_version = 2;")
                .unwrap();
        }
        let backup = pre_migration_backup_path(&path, 2);
        assert!(!backup.exists());

        let store = MeetingStore::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(
            backup.exists(),
            "a cópia de antes da migração não foi feita"
        );

        // A cópia é o banco como estava: abre na versão antiga, com os dados.
        let copia = Connection::open(&backup).unwrap();
        let v: i64 = copia
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 2, "a cópia precisa continuar na versão de origem");
        let n: i64 = copia
            .query_row("SELECT COUNT(*) FROM meetings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let tem_decisoes: bool = copia
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'decisions')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!tem_decisoes, "a cópia é de ANTES da migração");
        drop(copia);

        // Reabrir um banco já migrado não mexe na cópia.
        drop(store);
        let antes = std::fs::metadata(&backup).unwrap().len();
        MeetingStore::open(&path).unwrap();
        assert_eq!(std::fs::metadata(&backup).unwrap().len(), antes);
    }

    #[test]
    fn banco_novo_nao_gera_copia() {
        let path = temp_path("novo-sem-copia");
        MeetingStore::open(&path).unwrap();
        for v in 0..SCHEMA_VERSION {
            assert!(
                !pre_migration_backup_path(&path, v).exists(),
                "banco recém-criado não tem o que guardar"
            );
        }
    }

    #[test]
    fn nome_da_copia_diz_em_que_versao_ela_abre() {
        let p = Path::new("dados/ISPer/isper.db");
        assert_eq!(
            pre_migration_backup_path(p, 2),
            Path::new("dados/ISPer/isper.db.v2.bak")
        );
    }

    fn temp_store(name: &str) -> MeetingStore {
        MeetingStore::open(&temp_path(name)).expect("abrir banco temporário")
    }

    fn sample_decision(kind: &str, title: &str, at: f32) -> StoredDecision {
        StoredDecision {
            kind: kind.into(),
            title: title.into(),
            description: "detalhe".into(),
            owner: None,
            due_date: None,
            urgency: "medium".into(),
            at_secs: at,
        }
    }

    #[test]
    fn decisoes_voltam_na_ordem_da_conversa() {
        let store = temp_store("decisoes");
        let id = store
            .save("Reunião", "21/09/2026 10:00", &sample_result(), None)
            .unwrap();

        store
            .save_decisions(
                id,
                &[
                    sample_decision("action", "Enviar proposta", 90.0),
                    sample_decision("decision", "Entrega dia 30", 45.0),
                ],
            )
            .unwrap();

        let got = store.decisions(id).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(
            got[0].title, "Entrega dia 30",
            "ordena pelo instante da fala"
        );
        assert_eq!(got[1].title, "Enviar proposta");
        assert_eq!(got[0].kind, "decision");

        // E chegam junto com o resto do detalhe da reunião.
        let detail = store.get_meeting(id).unwrap().unwrap();
        assert_eq!(detail.decisions.len(), 2);
    }

    #[test]
    fn salvar_de_novo_regrava_em_vez_de_duplicar() {
        let store = temp_store("decisoes-regrava");
        let id = store
            .save("Reunião", "21/09/2026 10:00", &sample_result(), None)
            .unwrap();
        let uma = [sample_decision("decision", "Única", 10.0)];
        store.save_decisions(id, &uma).unwrap();
        store.save_decisions(id, &uma).unwrap();
        assert_eq!(store.decisions(id).unwrap().len(), 1);

        // Lista vazia limpa o que havia.
        store.save_decisions(id, &[]).unwrap();
        assert!(store.decisions(id).unwrap().is_empty());
    }

    #[test]
    fn campos_opcionais_sobrevivem_a_ida_e_volta() {
        let store = temp_store("decisoes-campos");
        let id = store
            .save("Reunião", "21/09/2026 10:00", &sample_result(), None)
            .unwrap();
        let mut d = sample_decision("action", "Avaliar SLA", 73.5);
        d.owner = Some("Carlos".into());
        d.due_date = Some("Sem prazo definido".into());
        d.urgency = "high".into();
        store.save_decisions(id, std::slice::from_ref(&d)).unwrap();

        let got = store.decisions(id).unwrap();
        assert_eq!(got, vec![d]);
    }

    #[test]
    fn apagar_a_reuniao_leva_as_decisoes_junto() {
        let store = temp_store("decisoes-apaga");
        let id = store
            .save("Reunião", "21/09/2026 10:00", &sample_result(), None)
            .unwrap();
        store
            .save_decisions(id, &[sample_decision("decision", "X", 1.0)])
            .unwrap();
        store.delete_meeting(id).unwrap();
        assert!(
            store.decisions(id).unwrap().is_empty(),
            "decisão órfã ficaria para sempre no banco"
        );
    }

    #[test]
    fn reuniao_sem_copilot_nao_tem_decisoes() {
        let store = temp_store("decisoes-vazio");
        let id = store
            .save("Reunião", "21/09/2026 10:00", &sample_result(), None)
            .unwrap();
        assert!(store.decisions(id).unwrap().is_empty());
        assert!(store.get_meeting(id).unwrap().unwrap().decisions.is_empty());
    }

    #[test]
    fn notas_do_copilot_gravam_trocam_e_apagam() {
        let store = temp_store("notas");
        let id = store
            .save("Reunião", "23/09/2026 10:00", &sample_result(), None)
            .unwrap();
        let notas = |s: &MeetingStore| s.get_meeting(id).unwrap().unwrap().notes;
        assert_eq!(notas(&store), None, "reunião nova nasce sem notas");

        store.set_notes(id, "  - valor: 40 mil\n").unwrap();
        assert_eq!(notas(&store).as_deref(), Some("- valor: 40 mil"));

        store.set_notes(id, "- valor: 45 mil").unwrap();
        assert_eq!(
            notas(&store).as_deref(),
            Some("- valor: 45 mil"),
            "regravar troca"
        );

        store.set_notes(id, " \n ").unwrap();
        assert_eq!(notas(&store), None, "texto vazio apaga");
    }

    #[test]
    fn banco_da_v3_ganha_a_coluna_de_notas_sem_perder_nada() {
        let path = temp_path("v3-para-v4");
        let id = {
            let store = MeetingStore::open(&path).unwrap();
            let id = store
                .save("Da 0.18", "22/09/2026 09:00", &sample_result(), None)
                .unwrap();
            store
                .save_decisions(id, &[sample_decision("decision", "Lançar dia 30", 4.0)])
                .unwrap();
            // Recua para a v3: a mesma estrutura, menos a coluna de notas.
            store
                .conn
                .execute_batch("ALTER TABLE meetings DROP COLUMN notes; PRAGMA user_version = 3;")
                .unwrap();
            id
        };

        let store = MeetingStore::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(
            pre_migration_backup_path(&path, 3).exists(),
            "cópia da v3 antes de migrar"
        );
        let detalhe = store
            .get_meeting(id)
            .unwrap()
            .expect("a reunião continua lá");
        assert_eq!(detalhe.meeting.title, "Da 0.18");
        assert_eq!(detalhe.decisions.len(), 1, "as decisões continuam");
        assert_eq!(detalhe.notes, None);
        store.set_notes(id, "anotado depois").unwrap();
        assert_eq!(
            store.get_meeting(id).unwrap().unwrap().notes.as_deref(),
            Some("anotado depois")
        );
    }

    #[test]
    fn banco_da_v4_ganha_a_origem_do_audio_sem_perder_nada() {
        let path = temp_path("v4-para-v5");
        let id = {
            let store = MeetingStore::open(&path).unwrap();
            let id = store
                .save("Da 0.20", "23/09/2026 09:00", &sample_result(), None)
                .unwrap();
            store.set_notes(id, "anotado").unwrap();
            // Recua para a v4: a mesma estrutura, menos as colunas da origem.
            store
                .conn
                .execute_batch(
                    "DROP INDEX idx_meetings_source_sha256;
                     ALTER TABLE meetings DROP COLUMN source_name;
                     ALTER TABLE meetings DROP COLUMN source_sha256;
                     PRAGMA user_version = 4;",
                )
                .unwrap();
            id
        };

        let store = MeetingStore::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(
            pre_migration_backup_path(&path, 4).exists(),
            "cópia da v4 antes de migrar"
        );
        let detalhe = store
            .get_meeting(id)
            .unwrap()
            .expect("a reunião continua lá");
        assert_eq!(detalhe.meeting.title, "Da 0.20");
        assert_eq!(
            detalhe.notes.as_deref(),
            Some("anotado"),
            "as notas continuam"
        );
        assert_eq!(detalhe.meeting.source_name, None, "gravada, não importada");
    }

    #[test]
    fn audio_importado_guarda_a_origem_e_e_achado_pelo_sha() {
        let path = temp_path("importado");
        let store = MeetingStore::open(&path).unwrap();
        let falas = vec![
            (
                "Participante 1".to_string(),
                0.0,
                4.0,
                "Bom dia.".to_string(),
            ),
            (
                "Participante 2".to_string(),
                4.0,
                9.0,
                "Vamos começar.".to_string(),
            ),
        ];
        let id = store
            .save_imported(&NewImportedMeeting {
                title: "Reunião com fornecedor",
                started_at: "22/09/2026 15:30",
                duration_secs: 9.0,
                md_path: "C:/ISPer/Reunioes/reuniao-20260922-153000.md",
                segments: &falas,
                source_name: "Reunião com fornecedor.mp3",
                source_sha256: "ab12",
            })
            .unwrap();
        let d = store.get_meeting(id).unwrap().unwrap();
        assert_eq!(
            d.meeting.source_name.as_deref(),
            Some("Reunião com fornecedor.mp3")
        );
        assert_eq!(d.meeting.participants, 2);
        assert_eq!(d.segments.len(), 2);
        assert_eq!(store.meeting_with_source("ab12").unwrap(), Some(id));
        assert_eq!(store.meeting_with_source("outro").unwrap(), None);
        let lista = store.list_meetings().unwrap();
        assert_eq!(
            lista[0].source_name.as_deref(),
            Some("Reunião com fornecedor.mp3")
        );

        let vazio = store.save_imported(&NewImportedMeeting {
            title: "x",
            started_at: "22/09/2026 15:30",
            duration_secs: 1.0,
            md_path: "y.md",
            segments: &[],
            source_name: "x.mp3",
            source_sha256: "cd34",
        });
        assert!(vazio.is_err(), "sem fala não vira reunião");
        assert_eq!(store.meeting_with_source("cd34").unwrap(), None);
    }

    fn sample_result() -> MeetingResult {
        let seg = |speaker, start: f32, text: &str| MeetingSegment {
            speaker,
            start_secs: start,
            end_secs: start + 1.0,
            text: text.into(),
        };
        MeetingResult {
            segments: vec![
                seg(Speaker::Me, 0.0, "Olá."),
                seg(Speaker::Others, 2.0, "Oi."),
                seg(Speaker::Others, 4.5, "Tudo bem?"),
            ],
            duration_secs: 6.0,
            others_audio: crate::meeting::ChannelAudio::empty(),
            me_audio: crate::meeting::ChannelAudio::empty(),
            forced_cuts: 0,
        }
    }

    #[test]
    fn banco_novo_nasce_na_versao_atual_e_reabrir_e_idempotente() {
        let path = temp_path("versao");
        let store = MeetingStore::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        drop(store);
        let store = MeetingStore::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(column_exists(&store.conn, "meetings", "started_ts").unwrap());
        assert!(column_exists(&store.conn, "dictations", "at_ts").unwrap());
        assert!(column_exists(&store.conn, "events", "kind").unwrap());
        assert!(!column_exists(&store.conn, "events", "inexistente").unwrap());
    }

    #[test]
    fn banco_antigo_sem_versao_e_migrado_com_as_datas_preenchidas() {
        let path = temp_path("legado");
        {
            // O schema como era antes da numeração (sem summary, md_path, raw_text,
            // moments e embeddings), com dados.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE meetings (id INTEGER PRIMARY KEY, title TEXT NOT NULL,
                    started_at TEXT NOT NULL, duration_secs REAL NOT NULL);
                 CREATE TABLE segments (id INTEGER PRIMARY KEY,
                    meeting_id INTEGER NOT NULL REFERENCES meetings(id), speaker TEXT NOT NULL,
                    start_secs REAL NOT NULL, end_secs REAL NOT NULL, text TEXT NOT NULL);
                 CREATE TABLE dictations (id INTEGER PRIMARY KEY, at TEXT NOT NULL,
                    text TEXT NOT NULL, audio_secs REAL, infer_secs REAL);
                 INSERT INTO meetings (title, started_at, duration_secs)
                    VALUES ('Antiga', '10/09/2026 14:00', 60.0);
                 INSERT INTO meetings (title, started_at, duration_secs)
                    VALUES ('Sem data', 'ontem', 10.0);
                 INSERT INTO dictations (at, text) VALUES ('11/09/2026 09:15:30', 'oi');",
            )
            .unwrap();
            let v: i64 = conn
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(v, 0, "banco legado não tem versão");
        }
        let store = MeetingStore::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        let ts: Option<i64> = store
            .conn
            .query_row(
                "SELECT started_ts FROM meetings WHERE title = 'Antiga'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ts, parse_local_stamp("10/09/2026 14:00"));
        let none: Option<i64> = store
            .conn
            .query_row(
                "SELECT started_ts FROM meetings WHERE title = 'Sem data'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            none, None,
            "data ilegível fica sem instante — e a retenção não a toca"
        );
        let at: Option<i64> = store
            .conn
            .query_row("SELECT at_ts FROM dictations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(at, parse_local_stamp("11/09/2026 09:15:30"));
        // As colunas que as versões antigas acrescentavam com ALTER estão lá
        // (as consultas normais funcionam) e as tabelas novas também.
        let rows = store.list_meetings().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|m| m.md_path.is_none() && !m.has_summary));
        assert_eq!(store.list_dictations(None, 10).unwrap().len(), 1);
        assert!(store.metrics(0).unwrap().is_empty());
    }

    #[test]
    fn banco_de_versao_mais_nova_e_recusado() {
        let path = temp_path("futuro");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .unwrap();
        }
        let err = MeetingStore::open(&path).err().expect("deve recusar");
        assert!(err.to_string().contains("mais nova"), "{err}");
    }

    #[test]
    fn parse_local_stamp_le_o_formato_do_app_e_ordena() {
        assert_eq!(parse_local_stamp("01/01/1970 00:00"), Some(0));
        assert_eq!(parse_local_stamp("02/01/1970 00:00:00"), Some(86_400));
        assert_eq!(parse_local_stamp("01/03/2000 00:00"), Some(951_868_800));
        assert_eq!(
            parse_local_stamp("12/09/2026 14:30:15"),
            Some(parse_local_stamp("12/09/2026 14:30").unwrap() + 15)
        );
        assert!(
            parse_local_stamp("10/09/2026 14:00").unwrap()
                < parse_local_stamp("11/09/2026 09:00").unwrap()
        );
        for bad in [
            "",
            "ontem",
            "2026-09-12 14:00",
            "32/01/2026 00:00",
            "10/13/2026 00:00",
            "10/09/2026 24:00",
            "10/09/2026",
            "10/09/2026 10:00:00:00",
        ] {
            assert_eq!(parse_local_stamp(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn retencao_apaga_so_o_que_passou_do_corte_e_devolve_os_markdowns() {
        let store = temp_store("retencao");
        let old = store
            .save(
                "Velha",
                "01/06/2026 10:00",
                &sample_result(),
                Some("/tmp/velha.md"),
            )
            .unwrap();
        let new = store
            .save("Nova", "10/09/2026 10:00", &sample_result(), None)
            .unwrap();
        store.save_moments(old, &[1.0]).unwrap();
        store
            .save_dictation("01/06/2026 10:05:00", "antigo", None, 1.0, 0.5)
            .unwrap();
        store
            .save_dictation("10/09/2026 10:05:00", "recente", None, 1.0, 0.5)
            .unwrap();
        let cutoff = parse_local_stamp("01/09/2026 00:00").unwrap();
        assert_eq!(store.count_older_than(cutoff).unwrap(), (1, 1));
        let purged = store.purge_older_than(cutoff).unwrap();
        assert_eq!(purged.meetings.len(), 1);
        assert_eq!(purged.meetings[0].id, old);
        assert_eq!(purged.meetings[0].md_path.as_deref(), Some("/tmp/velha.md"));
        assert_eq!(purged.dictations, 1);
        let ids: Vec<i64> = store
            .list_meetings()
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, vec![new]);
        let left = store.list_dictations(None, 10).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].text, "recente");
        let moments: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM moments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(moments, 0, "momentos da reunião apagada somem junto");
        let again = store.purge_older_than(cutoff).unwrap();
        assert!(
            again.meetings.is_empty() && again.dictations == 0,
            "segunda passada não acha nada"
        );
        assert_eq!(store.count_older_than(cutoff).unwrap(), (0, 0));
    }

    #[test]
    fn backup_e_uma_copia_integra_que_abre_sozinha() {
        let store = temp_store("backup");
        store
            .save("R", "10/09/2026 10:00", &sample_result(), None)
            .unwrap();
        let dest = temp_path("backup-copia");
        store.backup_to(&dest).unwrap();
        let copy = MeetingStore::open(&dest).unwrap();
        assert_eq!(copy.list_meetings().unwrap().len(), 1);
        assert_eq!(copy.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(
            store.backup_to(&dest).is_err(),
            "VACUUM INTO não sobrescreve"
        );
    }

    #[test]
    fn metricas_dao_p50_p95_e_falhas_por_tipo_e_esquecem_o_muito_antigo() {
        let store = temp_store("metricas");
        let now = 1_000_000_000;
        store
            .record_event(
                now - EVENTS_KEEP_SECS - 1,
                "dictation",
                true,
                Some(9.0),
                None,
            )
            .unwrap();
        for (i, secs) in [0.5, 0.7, 0.9, 1.1, 5.0].iter().enumerate() {
            store
                .record_event(now + i as i64, "dictation", true, Some(*secs), Some(10.0))
                .unwrap();
        }
        store
            .record_event(now + 10, "dictation", false, None, Some(3.0))
            .unwrap();
        store
            .record_event(now + 11, "meeting_block", true, Some(2.0), Some(20.0))
            .unwrap();
        let m = store.metrics(now - 3600).unwrap();
        assert_eq!(m.len(), 2);
        let d = m.iter().find(|k| k.kind == "dictation").unwrap();
        assert_eq!((d.total, d.errors), (6, 1));
        // f32 no evento, REAL (f64) no banco: compara com tolerância.
        assert!((d.p50_secs.unwrap() - 0.9).abs() < 1e-6);
        assert!((d.p95_secs.unwrap() - 5.0).abs() < 1e-6);
        assert!((d.p50_rtf.unwrap() - 0.09).abs() < 1e-6);
        let b = m.iter().find(|k| k.kind == "meeting_block").unwrap();
        assert_eq!((b.total, b.errors, b.p50_secs), (1, 0, Some(2.0)));
        let total: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 7, "o evento de mais de 90 dias foi esquecido");
        assert_eq!(percentile(&[], 50), None);
    }

    #[test]
    fn relabel_troca_so_os_segmentos_casados() {
        let store = temp_store("relabel");
        let id = store
            .save("Reunião", "09/09/2026 10:00", &sample_result(), None)
            .unwrap();
        let n = store
            .relabel_segments(
                id,
                &[
                    (2.0, "Participante 1"),
                    (4.5, "Participante 2"),
                    (99.0, "Ninguém"),
                ],
            )
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(
            store.speakers(id).unwrap(),
            vec!["Eu", "Participante 1", "Participante 2"]
        );
    }

    #[test]
    fn a_biblioteca_ordena_pela_hora_da_reuniao_e_nao_pela_de_insercao() {
        // Reunião recuperada de um .md entra DEPOIS no banco (id maior) mas é
        // mais ANTIGA. Ordenar por id a jogaria para o topo.
        use crate::import::parse_markdown;
        use crate::meeting::{SegmentRef, render_markdown};

        let store = temp_store("ordem");
        let md_de = |titulo: &str, quando: &str| {
            let segs = [SegmentRef {
                speaker: "Eu",
                start_secs: 0.0,
                end_secs: 2.0,
                text: "Oi.",
            }];
            render_markdown(titulo, quando, 10.0, &segs, None, &[])
        };
        // Grava primeiro a mais NOVA, depois a mais VELHA.
        for (titulo, quando, arquivo) in [
            ("Mais nova", "18/09/2026 14:58", "/tmp/nova.md"),
            ("Mais velha", "08/09/2026 14:47", "/tmp/velha.md"),
        ] {
            let m = parse_markdown(arquivo, &md_de(titulo, quando)).expect("parse");
            store.import_meeting(&m, arquivo).expect("importa");
        }
        let titulos: Vec<String> = store
            .list_meetings()
            .expect("lista")
            .into_iter()
            .map(|r| r.title)
            .collect();
        assert_eq!(
            titulos,
            vec!["Mais nova".to_string(), "Mais velha".to_string()],
            "a lista tem que sair pela data da reunião"
        );
    }

    #[test]
    fn gravacoes_no_mesmo_minuto_com_titulos_diferentes_entram_as_duas() {
        use crate::import::parse_markdown;
        use crate::meeting::{SegmentRef, render_markdown};

        let store = temp_store("mesmo-minuto");
        let segs = [SegmentRef {
            speaker: "Participante 1",
            start_secs: 0.0,
            end_secs: 2.0,
            text: "Oi.",
        }];
        let md_a = render_markdown("Com fornecedor", "22/09/2026 15:30", 2.0, &segs, None, &[]);
        let md_b = render_markdown("Com o time", "22/09/2026 15:30", 2.0, &segs, None, &[]);
        let a = parse_markdown("a.md", &md_a).unwrap();
        let b = parse_markdown("b.md", &md_b).unwrap();
        assert!(store.import_meeting(&a, "C:/r/a.md").unwrap().is_some());
        assert!(
            store.import_meeting(&b, "C:/r/b.md").unwrap().is_some(),
            "mesmo minuto, outra reunião: não pode sumir"
        );
        // A mesma reunião (início e título), vinda de outra cópia do `.md`.
        assert!(store.import_meeting(&a, "D:/copia/a.md").unwrap().is_none());
        assert_eq!(store.list_meetings().unwrap().len(), 2);
    }

    #[test]
    fn reimportar_a_pasta_recupera_a_reuniao_e_pode_ser_repetido() {
        use crate::import::parse_markdown;
        use crate::meeting::{SegmentRef, append_copilot_sections, render_markdown};

        let store = temp_store("import");
        let segs = [
            SegmentRef {
                speaker: "Eu",
                start_secs: 0.0,
                end_secs: 2.0,
                text: "Bom dia.",
            },
            SegmentRef {
                speaker: "Participante 1",
                start_secs: 10.0,
                end_secs: 14.0,
                text: "Bom dia, Ana.",
            },
        ];
        let mut md = render_markdown(
            "Reunião recuperada",
            "16/09/2026 11:23",
            60.0,
            &segs,
            Some(
                "## Resumo
Curto.",
            ),
            &[10.0],
        );
        append_copilot_sections(&mut md, None, Some("- ligar para a Ana"));
        let imported = parse_markdown("r.md", &md).expect("parse");

        let id = store
            .import_meeting(&imported, "/tmp/reuniao-recuperada.md")
            .expect("importa")
            .expect("id novo");
        let det = store.get_meeting(id).expect("busca").expect("existe");
        assert_eq!(det.meeting.title, "Reunião recuperada");
        assert_eq!(det.segments.len(), 2);
        assert_eq!(det.segments[1].speaker, "Participante 1");
        assert!(det.summary.as_deref().is_some_and(|s| s.contains("Curto")));
        assert!(
            det.summary.as_deref().is_some_and(|s| !s.contains("ligar")),
            "as notas não entram no resumo"
        );
        assert_eq!(det.moments.len(), 1);
        assert_eq!(det.notes.as_deref(), Some("- ligar para a Ana"));

        // Repetir a importação não duplica — é o que permite rodar a
        // recuperação na pasta inteira sem medo.
        assert_eq!(
            store
                .import_meeting(&imported, "/tmp/reuniao-recuperada.md")
                .expect("repete"),
            None
        );
        assert_eq!(store.list_meetings().expect("lista").len(), 1);
    }

    #[test]
    fn passe_final_troca_a_transcricao_inteira_ou_nao_troca_nada() {
        let store = temp_store("replace");
        let id = store
            .save("Reunião", "09/09/2026 10:00", &sample_result(), None)
            .unwrap();
        let segmentos = |store: &MeetingStore| {
            store
                .get_meeting(id)
                .unwrap()
                .expect("a reunião existe")
                .segments
        };
        assert!(!segmentos(&store).is_empty());

        let finais = vec![
            ("Eu".to_string(), 0.0f32, 2.0f32, "Bom dia.".to_string()),
            (
                "Participante 1".to_string(),
                2.5,
                6.0,
                "Bom dia, Ana.".to_string(),
            ),
        ];
        assert_eq!(store.replace_segments(id, &finais).unwrap(), 2);
        let depois = segmentos(&store);
        assert_eq!(depois.len(), 2);
        assert_eq!(depois[0].text, "Bom dia.");
        assert_eq!(
            store.speakers(id).unwrap(),
            vec!["Eu", "Participante 1"],
            "os rótulos vêm do passe final"
        );

        // Passe final vazio não apaga a transcrição que existe: a do ao vivo
        // é sempre melhor que nenhuma.
        assert!(store.replace_segments(id, &[]).is_err());
        assert_eq!(segmentos(&store).len(), 2);
    }

    #[test]
    fn momentos_sao_salvos_ordenados_e_apagados_com_a_reuniao() {
        let store = temp_store("moments");
        let id = store
            .save("Reunião", "09/09/2026 10:00", &sample_result(), None)
            .unwrap();
        store.save_moments(id, &[12.5, 3.0]).unwrap();
        let detail = store.get_meeting(id).unwrap().unwrap();
        assert_eq!(detail.moments, vec![3.0, 12.5]);
        assert_eq!(detail.meeting.moments, 2);
        assert_eq!(store.list_meetings().unwrap()[0].moments, 2);
        store.delete_meeting(id).unwrap();
        let left: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM moments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn rename_speaker_vale_para_a_reuniao_inteira() {
        let store = temp_store("rename");
        let id = store
            .save("Reunião", "09/09/2026 10:00", &sample_result(), None)
            .unwrap();
        assert_eq!(
            store
                .rename_speaker(id, "Participantes", "Tatiana")
                .unwrap(),
            2
        );
        let detail = store.get_meeting(id).unwrap().unwrap();
        assert!(
            detail
                .segments
                .iter()
                .filter(|s| s.speaker == "Tatiana")
                .count()
                == 2
        );
        assert_eq!(detail.meeting.participants, 1);
    }

    #[test]
    fn ditado_guarda_o_original_quando_polido() {
        let store = temp_store("dictation");
        store
            .save_dictation(
                "09/09/2026 10:00:00",
                "Bom dia, tudo bem?",
                Some("é bom dia hã tudo bem"),
                2.0,
                0.3,
            )
            .unwrap();
        store
            .save_dictation("09/09/2026 10:00:05", "sem polimento", None, 1.0, 0.2)
            .unwrap();
        let rows = store.list_dictations(None, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].raw_text.as_deref(), Some("é bom dia hã tudo bem"));
        assert_eq!(rows[0].raw_text, None);
        let hits = store.list_dictations(Some("polimento"), 10).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn embeddings_indexam_buscam_e_somem_com_o_item() {
        let store = temp_store("embed");
        let id = store
            .save("Reunião", "10/09/2026 10:00", &sample_result(), None)
            .unwrap();
        store
            .replace_embeddings(
                "meeting",
                id,
                "fake/m",
                &[
                    (Some(0.0), "planejamento do MRP", &[1.0, 0.0]),
                    (Some(30.0), "horário do almoço", &[0.0, 1.0]),
                ],
            )
            .unwrap();
        // Melhor trecho por item, ordenado pelo cosseno.
        let hits = store
            .semantic_search("meeting", "fake/m", &[0.9, 0.1], 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].ref_id, id);
        assert_eq!(hits[0].text, "planejamento do MRP");
        assert_eq!(hits[0].start_secs, Some(0.0));
        assert!((hits[0].score - 0.9).abs() < 1e-6);
        // Outro modelo não enxerga estes vetores.
        assert!(
            store
                .semantic_search("meeting", "outro/modelo", &[1.0, 0.0], 10)
                .unwrap()
                .is_empty()
        );
        let stats = store.embeddings_stats("fake/m").unwrap();
        assert_eq!(
            (stats.meetings_total, stats.meetings_indexed, stats.chunks),
            (1, 1, 2)
        );
        assert_eq!(store.embedded_ids("meeting", "fake/m").unwrap(), vec![id]);
        assert_eq!(store.meeting_ids().unwrap(), vec![id]);
        assert_eq!(store.meeting_row(id).unwrap().unwrap().title, "Reunião");

        // Reindexar substitui; trocar de modelo descarta os antigos.
        store
            .replace_embeddings("meeting", id, "fake/m2", &[(None, "resumo", &[1.0])])
            .unwrap();
        assert_eq!(store.embeddings_stats("fake/m").unwrap().chunks, 0);
        assert_eq!(store.delete_embeddings_except_model("fake/m3").unwrap(), 1);

        // Ditados: id devolvido, vetor próprio, tudo somem com o item.
        let d = store
            .save_dictation("10/09/2026 10:01:00", "comprar pão", None, 1.0, 0.1)
            .unwrap();
        assert_eq!(store.dictation_row(d).unwrap().unwrap().text, "comprar pão");
        assert_eq!(
            store.dictation_texts().unwrap(),
            vec![(d, "comprar pão".to_string())]
        );
        store
            .replace_embeddings("dictation", d, "fake/m3", &[(None, "comprar pão", &[1.0])])
            .unwrap();
        let hits = store
            .semantic_search("dictation", "fake/m3", &[1.0], 10)
            .unwrap();
        assert_eq!(hits[0].ref_id, d);
        store.delete_dictation(d).unwrap();
        store.delete_meeting(id).unwrap();
        assert_eq!(store.embeddings_stats("fake/m3").unwrap().chunks, 0);
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;
    use rusqlite::Connection;

    use super::like_pattern;

    /// O mesmo `LIKE … ESCAPE '\\'` das consultas, avaliado pelo próprio SQLite.
    fn sql_like(text: &str, pattern: &str) -> bool {
        let conn = Connection::open_in_memory().expect("sqlite em memória");
        conn.query_row("SELECT ?1 LIKE ?2 ESCAPE '\\'", [text, pattern], |r| {
            r.get::<_, bool>(0)
        })
        .expect("avaliar LIKE")
    }

    proptest! {
        /// A busca é literal: qualquer texto que contenha o termo casa com o padrão…
        #[test]
        fn termo_contido_no_texto_casa(
            prefix in "\\PC{0,8}",
            q in "\\PC{1,12}",
            suffix in "\\PC{0,8}",
        ) {
            let term = q.trim();
            prop_assume!(!term.is_empty());
            let text = format!("{prefix}{term}{suffix}");
            prop_assert!(sql_like(&text, &like_pattern(&q)), "{text:?} deveria casar com {q:?}");
        }

        /// …e os curingas do SQL não valem: `%` e `_` no termo só casam com eles mesmos.
        #[test]
        fn curingas_do_termo_nao_expandem(q in "[a-z%_\\\\ ]{1,12}") {
            prop_assume!(q.contains(['%', '_']));
            let sem_curingas: String = q
                .chars()
                .map(|c| if matches!(c, '%' | '_') { 'x' } else { c })
                .collect();
            prop_assert!(
                !sql_like(&sem_curingas, &like_pattern(&q)),
                "{sem_curingas:?} não deveria casar com {q:?}"
            );
        }

        /// Forma do padrão: `%` nas pontas e, no meio, o termo (sem espaços nas
        /// bordas) com cada `%`, `_` e `\` precedido de `\` — e nada mais escapado.
        #[test]
        fn padrao_tem_a_forma_esperada(q in "\\PC{0,20}") {
            let p = like_pattern(&q);
            prop_assert!(p.starts_with('%') && p.ends_with('%'));
            let inner = &p[1..p.len() - 1];
            let mut chars = inner.chars();
            let mut rebuilt = String::new();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    let escaped = chars.next().expect("uma barra sempre escapa algo");
                    prop_assert!(matches!(escaped, '%' | '_' | '\\'));
                    rebuilt.push(escaped);
                } else {
                    prop_assert!(!matches!(c, '%' | '_'));
                    rebuilt.push(c);
                }
            }
            prop_assert_eq!(rebuilt, q.trim());
        }
    }
}
