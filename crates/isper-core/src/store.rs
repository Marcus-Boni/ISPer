//! Biblioteca de reuniões: persistência em SQLite (arquivo único, local).

use std::path::Path;

use rusqlite::{params, Connection};

use crate::meeting::MeetingResult;
use crate::Result;

pub struct MeetingStore {
    conn: Connection,
}

impl MeetingStore {
    /// Abre (ou cria) o banco no caminho dado.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
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
            CREATE INDEX IF NOT EXISTS idx_segments_meeting ON segments(meeting_id);",
        )?;
        // Migração leve: coluna nova em bancos antigos (erro = já existe).
        let _ = conn.execute("ALTER TABLE meetings ADD COLUMN summary TEXT", []);
        Ok(Self { conn })
    }

    /// Guarda o resumo gerado por IA (Fase 5).
    pub fn set_summary(&self, meeting_id: i64, summary: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE meetings SET summary = ?1 WHERE id = ?2",
            params![summary, meeting_id],
        )?;
        Ok(())
    }

    /// Salva uma reunião completa e devolve o id.
    pub fn save(&self, title: &str, started_at: &str, result: &MeetingResult) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO meetings (title, started_at, duration_secs) VALUES (?1, ?2, ?3)",
            params![title, started_at, result.duration_secs],
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
}
