//! Biblioteca de reuniões e histórico de ditados: SQLite (arquivo único,
//! local). Além de gravar, oferece leitura, busca e exclusão para a
//! janela "Biblioteca" do app.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::meeting::MeetingResult;
use crate::Result;

pub struct MeetingStore {
    conn: Connection,
}

/// Linha da lista de reuniões (sem o transcript, que pode ser grande).
#[derive(Debug, Clone, Serialize)]
pub struct MeetingRow {
    pub id: i64,
    pub title: String,
    pub started_at: String,
    pub duration_secs: f32,
    pub segments: i64,
    /// Falantes distintos além de "Eu".
    pub participants: i64,
    pub has_summary: bool,
    pub md_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredSegment {
    pub speaker: String,
    pub start_secs: f32,
    pub end_secs: f32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeetingDetail {
    pub meeting: MeetingRow,
    pub summary: Option<String>,
    pub segments: Vec<StoredSegment>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DictationRow {
    pub id: i64,
    pub at: String,
    pub text: String,
    pub audio_secs: Option<f32>,
    /// Texto como saiu do Whisper, quando o polimento por IA o alterou.
    pub raw_text: Option<String>,
}

/// Totais para a tela Início (uma consulta por tabela, sem carregar linhas).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Stats {
    pub meetings: i64,
    /// Soma das durações das reuniões.
    pub meeting_secs: f64,
    pub with_summary: i64,
    pub dictations: i64,
    /// Soma do áudio ditado.
    pub dictation_secs: f64,
    /// Palavras ditadas (aproximação: espaços + 1 por ditado).
    pub dictation_words: i64,
}

const MEETING_COLUMNS: &str = "m.id, m.title, m.started_at, m.duration_secs,
    (m.summary IS NOT NULL AND m.summary != ''), m.md_path,
    (SELECT COUNT(*) FROM segments s WHERE s.meeting_id = m.id),
    (SELECT COUNT(DISTINCT s.speaker) FROM segments s WHERE s.meeting_id = m.id AND s.speaker != 'Eu')";

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
            );",
        )?;
        // Migrações leves: colunas novas em bancos antigos (erro = já existe).
        let _ = conn.execute("ALTER TABLE meetings ADD COLUMN summary TEXT", []);
        let _ = conn.execute("ALTER TABLE meetings ADD COLUMN md_path TEXT", []);
        let _ = conn.execute("ALTER TABLE dictations ADD COLUMN raw_text TEXT", []);
        Ok(Self { conn })
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
            "INSERT INTO meetings (title, started_at, duration_secs, md_path) VALUES (?1, ?2, ?3, ?4)",
            params![title, started_at, result.duration_secs, md_path],
        )?;
        let id = self.conn.last_insert_rowid();
        let mut stmt = self.conn.prepare(
            "INSERT INTO segments (meeting_id, speaker, start_secs, end_secs, text)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for s in &result.segments {
            stmt.execute(params![id, s.speaker.label(), s.start_secs, s.end_secs, s.text])?;
        }
        Ok(id)
    }

    /// Guarda o resumo gerado por IA (Fase 5).
    pub fn set_summary(&self, meeting_id: i64, summary: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE meetings SET summary = ?1 WHERE id = ?2",
            params![summary, meeting_id],
        )?;
        Ok(())
    }

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
        self.conn
            .execute("DELETE FROM segments WHERE meeting_id = ?1", params![meeting_id])?;
        self.conn
            .execute("DELETE FROM meetings WHERE id = ?1", params![meeting_id])?;
        Ok(())
    }

    /// Histórico de ditados (Fase 3): cada texto colado fica pesquisável.
    /// `raw_text` é o original do Whisper quando o polimento por IA o mudou.
    pub fn save_dictation(
        &self,
        at: &str,
        text: &str,
        raw_text: Option<&str>,
        audio_secs: f32,
        infer_secs: f32,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO dictations (at, text, raw_text, audio_secs, infer_secs) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![at, text, raw_text, audio_secs, infer_secs],
        )?;
        Ok(())
    }

    pub fn delete_dictation(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM dictations WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ------------------------------------------------------------ leitura

    /// Todas as reuniões, mais recente primeiro.
    pub fn list_meetings(&self) -> Result<Vec<MeetingRow>> {
        let sql = format!("SELECT {MEETING_COLUMNS} FROM meetings m ORDER BY m.id DESC");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_meeting)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// As `limit` reuniões mais recentes (tela Início).
    pub fn recent_meetings(&self, limit: i64) -> Result<Vec<MeetingRow>> {
        let sql = format!("SELECT {MEETING_COLUMNS} FROM meetings m ORDER BY m.id DESC LIMIT ?1");
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
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?, r.get::<_, i64>(2)?)),
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
             ORDER BY m.id DESC"
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
        let summary: Option<String> = self
            .conn
            .query_row(
                "SELECT summary FROM meetings WHERE id = ?1",
                params![meeting_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
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
        Ok(Some(MeetingDetail {
            meeting,
            summary,
            segments,
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

    fn temp_store(name: &str) -> MeetingStore {
        let path = std::env::temp_dir().join(format!("isper-store-test-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        MeetingStore::open(&path).expect("abrir banco temporário")
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
            others_audio_16k: Vec::new(),
            others_blocks: Vec::new(),
        }
    }

    #[test]
    fn relabel_troca_so_os_segmentos_casados() {
        let store = temp_store("relabel");
        let id = store.save("Reunião", "09/09/2026 10:00", &sample_result(), None).unwrap();
        let n = store
            .relabel_segments(id, &[(2.0, "Participante 1"), (4.5, "Participante 2"), (99.0, "Ninguém")])
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(store.speakers(id).unwrap(), vec!["Eu", "Participante 1", "Participante 2"]);
    }

    #[test]
    fn rename_speaker_vale_para_a_reuniao_inteira() {
        let store = temp_store("rename");
        let id = store.save("Reunião", "09/09/2026 10:00", &sample_result(), None).unwrap();
        assert_eq!(store.rename_speaker(id, "Participantes", "Tatiana").unwrap(), 2);
        let detail = store.get_meeting(id).unwrap().unwrap();
        assert!(detail.segments.iter().filter(|s| s.speaker == "Tatiana").count() == 2);
        assert_eq!(detail.meeting.participants, 1);
    }

    #[test]
    fn ditado_guarda_o_original_quando_polido() {
        let store = temp_store("dictation");
        store.save_dictation("09/09/2026 10:00:00", "Bom dia, tudo bem?", Some("é bom dia hã tudo bem"), 2.0, 0.3).unwrap();
        store.save_dictation("09/09/2026 10:00:05", "sem polimento", None, 1.0, 0.2).unwrap();
        let rows = store.list_dictations(None, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].raw_text.as_deref(), Some("é bom dia hã tudo bem"));
        assert_eq!(rows[0].raw_text, None);
        let hits = store.list_dictations(Some("polimento"), 10).unwrap();
        assert_eq!(hits.len(), 1);
    }
}
