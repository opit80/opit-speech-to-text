//! Local dictation history: SQLite with an FTS5 index.

pub mod audio_store;

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

pub use audio_store::AudioStore;

use crate::pipeline::TranscriptStatus;
use crate::text::{fold, is_word_char};

const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS dictations (
    id            INTEGER PRIMARY KEY,
    created_at_ms INTEGER NOT NULL,
    profile_id    TEXT    NOT NULL,
    raw_text      TEXT    NOT NULL,
    text          TEXT    NOT NULL,
    status        TEXT    NOT NULL,
    audio_ms      INTEGER NOT NULL,
    latency_ms    INTEGER NOT NULL,
    audio_path    TEXT,
    -- Turkish-folded copies (see text::fold) that the FTS index is built from.
    search_text   TEXT    NOT NULL,
    search_raw    TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS dictations_created ON dictations(created_at_ms);
CREATE VIRTUAL TABLE IF NOT EXISTS dictations_fts USING fts5(
    search_text, search_raw,
    content = 'dictations', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER IF NOT EXISTS dictations_ai AFTER INSERT ON dictations BEGIN
    INSERT INTO dictations_fts(rowid, search_text, search_raw) VALUES (new.id, new.search_text, new.search_raw);
END;
CREATE TRIGGER IF NOT EXISTS dictations_ad AFTER DELETE ON dictations BEGIN
    INSERT INTO dictations_fts(dictations_fts, rowid, search_text, search_raw)
    VALUES ('delete', old.id, old.search_text, old.search_raw);
END;
CREATE TRIGGER IF NOT EXISTS dictations_au AFTER UPDATE OF search_text, search_raw ON dictations BEGIN
    INSERT INTO dictations_fts(dictations_fts, rowid, search_text, search_raw)
    VALUES ('delete', old.id, old.search_text, old.search_raw);
    INSERT INTO dictations_fts(rowid, search_text, search_raw) VALUES (new.id, new.search_text, new.search_raw);
END;
";

const COLUMNS: &str = "id, created_at_ms, profile_id, raw_text, text, status, audio_ms, latency_ms, audio_path";

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("history database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("history file error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Dictation {
    pub id: i64,
    pub created_at_ms: i64,
    pub profile_id: String,
    pub raw_text: String,
    pub text: String,
    pub status: TranscriptStatus,
    pub audio_ms: u64,
    pub latency_ms: u64,
    pub audio_path: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct NewDictation<'a> {
    pub created_at_ms: i64,
    pub profile_id: &'a str,
    pub raw_text: &'a str,
    pub text: &'a str,
    pub status: TranscriptStatus,
    pub audio_ms: u64,
    pub latency_ms: u64,
}

pub struct HistoryStore {
    conn: Connection,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct UsageStats {
    pub dictations: u64,
    pub successful: u64,
    pub audio_ms: u64,
    pub characters: u64,
    pub average_latency_ms: f64,
}

impl HistoryStore {
    pub fn open(path: &Path) -> Result<Self, HistoryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, HistoryError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self, HistoryError> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version < SCHEMA_VERSION {
            // One transaction, idempotent statements: a crash mid-setup can neither
            // leave a half schema behind nor block the next start.
            let tx = conn.transaction()?;
            tx.execute_batch(SCHEMA)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        Ok(Self { conn })
    }

    pub fn insert(&self, d: &NewDictation<'_>) -> Result<i64, HistoryError> {
        self.conn.execute(
            "INSERT INTO dictations
               (created_at_ms, profile_id, raw_text, text, status, audio_ms, latency_ms, search_text, search_raw)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                d.created_at_ms,
                d.profile_id,
                d.raw_text,
                d.text,
                d.status.as_str(),
                d.audio_ms as i64,
                d.latency_ms as i64,
                fold(d.text),
                fold(d.raw_text)
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn set_audio_path(&self, id: i64, path: &str) -> Result<(), HistoryError> {
        self.conn.execute("UPDATE dictations SET audio_path = ?1 WHERE id = ?2", params![path, id])?;
        Ok(())
    }

    /// Aggregates all retained history, without returning transcript content to the caller.
    /// `since_ms` is inclusive and `until_ms` is exclusive (local-day bounds supplied by the UI).
    pub fn usage(&self, since_ms: Option<i64>, until_ms: i64) -> Result<UsageStats, HistoryError> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(status = 'ok'), 0), COALESCE(SUM(audio_ms), 0),
             COALESCE(SUM(CASE WHEN status = 'ok' THEN LENGTH(text) ELSE 0 END), 0),
             COALESCE(AVG(CASE WHEN status = 'ok' THEN latency_ms END), 0)
             FROM dictations WHERE created_at_ms >= ?1 AND created_at_ms < ?2",
            params![since_ms.unwrap_or(i64::MIN), until_ms],
            |row| {
                Ok(UsageStats {
                    dictations: row.get::<_, i64>(0)?.max(0) as u64,
                    successful: row.get::<_, i64>(1)?.max(0) as u64,
                    audio_ms: row.get::<_, i64>(2)?.max(0) as u64,
                    characters: row.get::<_, i64>(3)?.max(0) as u64,
                    average_latency_ms: row.get(4)?,
                })
            },
        )?)
    }

    pub fn get(&self, id: i64) -> Result<Option<Dictation>, HistoryError> {
        let sql = format!("SELECT {COLUMNS} FROM dictations WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id], from_row).optional()?)
    }

    pub fn recent(&self, limit: usize, before_id: Option<i64>) -> Result<Vec<Dictation>, HistoryError> {
        let sql = format!("SELECT {COLUMNS} FROM dictations WHERE id < ?1 ORDER BY id DESC LIMIT ?2");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![before_id.unwrap_or(i64::MAX), limit as i64], from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Dictation>, HistoryError> {
        let Some(fts) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let sql = format!(
            "SELECT {COLUMNS} FROM dictations
             WHERE id IN (SELECT rowid FROM dictations_fts WHERE dictations_fts MATCH ?1)
             ORDER BY id DESC LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![fts, limit as i64], from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn delete(&self, id: i64) -> Result<Option<String>, HistoryError> {
        let path = self
            .conn
            .query_row("SELECT audio_path FROM dictations WHERE id = ?1", [id], |row| row.get::<_, Option<String>>(0))
            .optional()?
            .flatten();
        self.conn.execute("DELETE FROM dictations WHERE id = ?1", [id])?;
        Ok(path)
    }

    pub fn clear(&self) -> Result<Vec<String>, HistoryError> {
        let paths = self.audio_paths_before(i64::MAX)?;
        self.conn.execute("DELETE FROM dictations", [])?;
        Ok(paths)
    }

    /// Detaches audio files of dictations created before `older_than_ms` and
    /// returns their paths for deletion.
    pub fn take_expired_audio(&self, older_than_ms: i64) -> Result<Vec<String>, HistoryError> {
        let paths = self.audio_paths_before(older_than_ms)?;
        self.conn.execute(
            "UPDATE dictations SET audio_path = NULL WHERE audio_path IS NOT NULL AND created_at_ms < ?1",
            [older_than_ms],
        )?;
        Ok(paths)
    }

    fn audio_paths_before(&self, before_ms: i64) -> Result<Vec<String>, HistoryError> {
        let mut stmt = self.conn.prepare(
            "SELECT audio_path FROM dictations WHERE audio_path IS NOT NULL AND created_at_ms < ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([before_ms], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Dictation> {
    let status: String = row.get(5)?;
    Ok(Dictation {
        id: row.get(0)?,
        created_at_ms: row.get(1)?,
        profile_id: row.get(2)?,
        raw_text: row.get(3)?,
        text: row.get(4)?,
        status: TranscriptStatus::parse(&status).unwrap_or(TranscriptStatus::Ok),
        audio_ms: row.get::<_, i64>(6)? as u64,
        latency_ms: row.get::<_, i64>(7)? as u64,
        audio_path: row.get(8)?,
    })
}

/// Turns free text into an FTS5 query of quoted prefix terms, so user input can
/// never be parsed as FTS5 syntax. Returns `None` when nothing searchable is left.
pub fn fts_query(input: &str) -> Option<String> {
    let cleaned: String = fold(input).chars().map(|c| if is_word_char(c) { c } else { ' ' }).collect();
    let terms: Vec<String> = cleaned.split_whitespace().map(|t| format!("\"{t}\"*")).collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_aggregates_all_rows_with_inclusive_start_and_exclusive_end() {
        let store = HistoryStore::open_in_memory().unwrap();
        for i in 0..12 {
            store.insert(&entry("üç", "x", i)).unwrap();
        }
        let mut empty = entry("", "", 12);
        empty.status = TranscriptStatus::Empty;
        empty.latency_ms = 9_000;
        store.insert(&empty).unwrap();
        let stats = store.usage(Some(5), 13).unwrap();
        assert_eq!(stats.dictations, 8);
        assert_eq!(stats.successful, 7);
        assert_eq!(stats.characters, 14);
        assert_eq!(stats.audio_ms, 8_000);
        assert_eq!(stats.average_latency_ms, 800.0);
        assert_eq!(store.usage(None, 12).unwrap().dictations, 12);
        store.clear().unwrap();
        assert_eq!(store.usage(None, 13).unwrap(), UsageStats::default());
    }

    fn entry<'a>(text: &'a str, raw_text: &'a str, created_at_ms: i64) -> NewDictation<'a> {
        NewDictation {
            created_at_ms,
            profile_id: "groq",
            raw_text,
            text,
            status: TranscriptStatus::Ok,
            audio_ms: 1_000,
            latency_ms: 800,
        }
    }

    fn ids(list: &[Dictation]) -> Vec<i64> {
        list.iter().map(|d| d.id).collect()
    }

    #[test]
    fn insert_and_get_round_trip() {
        let store = HistoryStore::open_in_memory().unwrap();
        let id = store.insert(&entry("Claude Code'u aç", "cloud code'u aç", 1_000)).unwrap();
        let d = store.get(id).unwrap().unwrap();
        assert_eq!(d.text, "Claude Code'u aç");
        assert_eq!(d.raw_text, "cloud code'u aç");
        assert_eq!((d.status, d.audio_ms, d.latency_ms, d.audio_path), (TranscriptStatus::Ok, 1_000, 800, None));
        assert!(store.get(id + 1).unwrap().is_none());
    }

    #[test]
    fn recent_is_newest_first_and_pages() {
        let store = HistoryStore::open_in_memory().unwrap();
        for i in 0..5 {
            store.insert(&entry("x", "x", i)).unwrap();
        }
        assert_eq!(ids(&store.recent(2, None).unwrap()), [5, 4]);
        assert_eq!(ids(&store.recent(2, Some(4)).unwrap()), [3, 2]);
    }

    #[test]
    fn search_covers_text_raw_text_prefixes_and_diacritics() {
        let store = HistoryStore::open_in_memory().unwrap();
        let a = store.insert(&entry("Claude Code'u aç", "cloud code'u aç", 1)).unwrap();
        let b = store.insert(&entry("o hâlâ burada", "o hala burada", 2)).unwrap();
        assert_eq!(ids(&store.search("claude", 10).unwrap()), [a]);
        assert_eq!(ids(&store.search("cloud", 10).unwrap()), [a]);
        assert_eq!(ids(&store.search("burad", 10).unwrap()), [b]);
        assert_eq!(ids(&store.search("hâlâ", 10).unwrap()), [b]);
        assert_eq!(ids(&store.search("code'u", 10).unwrap()), [a]);
        let c = store.insert(&entry("kalıcı ayar", "kalıcı ayar", 3)).unwrap();
        assert_eq!(ids(&store.search("KALICI", 10).unwrap()), [c]);
        assert_eq!(ids(&store.search("kalici", 10).unwrap()), [c]);
    }

    #[test]
    fn search_tolerates_fts_syntax_and_empty_input() {
        let store = HistoryStore::open_in_memory().unwrap();
        store.insert(&entry("merhaba", "merhaba", 1)).unwrap();
        for query in ["", "   ", "\"(*", "AND OR NOT", "merhaba\" OR \"x"] {
            store.search(query, 10).unwrap_or_else(|e| panic!("{query:?}: {e}"));
        }
        assert!(store.search("\"(*", 10).unwrap().is_empty());
    }

    #[test]
    fn delete_and_clear_return_audio_paths() {
        let store = HistoryStore::open_in_memory().unwrap();
        let a = store.insert(&entry("bir", "bir", 1)).unwrap();
        let b = store.insert(&entry("iki", "iki", 2)).unwrap();
        store.insert(&entry("üç", "üç", 3)).unwrap();
        store.set_audio_path(a, "a.wav").unwrap();
        store.set_audio_path(b, "b.wav").unwrap();

        assert_eq!(store.delete(a).unwrap().as_deref(), Some("a.wav"));
        assert!(store.search("bir", 10).unwrap().is_empty());
        assert_eq!(store.clear().unwrap(), ["b.wav"]);
        assert!(store.recent(10, None).unwrap().is_empty());
    }

    #[test]
    fn expired_audio_is_detached_once() {
        let store = HistoryStore::open_in_memory().unwrap();
        let old = store.insert(&entry("eski", "eski", 100)).unwrap();
        let new = store.insert(&entry("yeni", "yeni", 900)).unwrap();
        store.set_audio_path(old, "old.wav").unwrap();
        store.set_audio_path(new, "new.wav").unwrap();
        assert_eq!(store.take_expired_audio(500).unwrap(), ["old.wav"]);
        assert_eq!(store.get(old).unwrap().unwrap().audio_path, None);
        assert_eq!(store.get(new).unwrap().unwrap().audio_path.as_deref(), Some("new.wav"));
        assert!(store.take_expired_audio(500).unwrap().is_empty());
    }

    #[test]
    fn file_database_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data").join("history.db");
        {
            let store = HistoryStore::open(&path).unwrap();
            store.insert(&entry("kalıcı", "kalıcı", 1)).unwrap();
        }
        let store = HistoryStore::open(&path).unwrap();
        assert_eq!(store.recent(10, None).unwrap().len(), 1);
        assert_eq!(store.search("kalıcı", 10).unwrap().len(), 1);
    }

    #[test]
    fn fts_query_quotes_word_tokens() {
        assert_eq!(fts_query("Claude code'u").as_deref(), Some("\"claude\"* \"code\"* \"u\"*"));
        assert_eq!(fts_query(" \"(* "), None);
    }

    #[test]
    fn half_created_schema_is_repaired_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE dictations (id INTEGER PRIMARY KEY, created_at_ms INTEGER NOT NULL,
                 profile_id TEXT NOT NULL, raw_text TEXT NOT NULL, text TEXT NOT NULL, status TEXT NOT NULL,
                 audio_ms INTEGER NOT NULL, latency_ms INTEGER NOT NULL, audio_path TEXT,
                 search_text TEXT NOT NULL, search_raw TEXT NOT NULL);",
            )
            .unwrap();
        }
        let store = HistoryStore::open(&path).unwrap();
        store.insert(&entry("merhaba", "merhaba", 1)).unwrap();
        assert_eq!(store.search("merhaba", 10).unwrap().len(), 1);
    }
}
