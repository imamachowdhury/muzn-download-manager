//! SQLite persistence: one file, schema versioned by `PRAGMA user_version`.
//! Calls are short and synchronous behind one mutex; the manager calls them
//! from async code, which is fine at this size (a few rows written a second).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::{CoreError, Result};
use crate::model::*;

/// Newest schema this build knows.
pub const SCHEMA_VERSION: i64 = 1;

const SCHEMA_V1: &str = r#"
CREATE TABLE downloads (
    id            TEXT PRIMARY KEY,
    kind          TEXT NOT NULL CHECK (kind IN ('http', 'torrent')),
    url           TEXT NOT NULL,
    final_url     TEXT,
    filename      TEXT,
    dir           TEXT NOT NULL,
    size          INTEGER,
    status        TEXT NOT NULL CHECK (status IN ('QUEUED','PROBING','DOWNLOADING','PAUSED','COMPLETED','FAILED','CANCELLED','SEEDING')),
    etag          TEXT,
    last_modified TEXT,
    mime          TEXT,
    referrer      TEXT,
    headers_json  TEXT NOT NULL DEFAULT '[]',
    cookies_json  TEXT NOT NULL DEFAULT '[]',
    error_code    TEXT,
    error_message TEXT,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    completed_at  INTEGER
);
CREATE INDEX downloads_queue ON downloads (status, created_at);
CREATE TABLE segments (
    download_id TEXT NOT NULL REFERENCES downloads (id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    start_byte  INTEGER NOT NULL,
    end_byte    INTEGER NOT NULL,
    downloaded  INTEGER NOT NULL,
    PRIMARY KEY (download_id, idx)
);
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

const ROW_SELECT: &str =
    "SELECT d.id, d.kind, d.url, d.final_url, d.filename, d.dir, d.size, d.status,
    d.etag, d.last_modified, d.mime, d.referrer, d.headers_json, d.cookies_json, d.error_code,
    d.error_message, d.created_at, d.updated_at, d.completed_at,
    (SELECT COALESCE(SUM(s.downloaded), 0) FROM segments s WHERE s.download_id = d.id)
    FROM downloads d";

/// The database.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Open (creating folders and the file if needed) and migrate.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    /// A private in-memory database (tests).
    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(CoreError::InvalidState(format!(
                "database schema {version} is newer than this build ({SCHEMA_VERSION})"
            )));
        }
        if version < 1 {
            conn.execute_batch(SCHEMA_V1)?;
            conn.pragma_update(None, "user_version", 1)?;
        }
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    /// `PRAGMA user_version`.
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// Add a row.
    pub fn insert(
        &self,
        id: &DownloadId,
        dir: &Path,
        new: &NewDownload,
        status: DownloadStatus,
        now: i64,
    ) -> Result<DownloadRow> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO downloads (id, kind, url, filename, dir, status, referrer, headers_json, cookies_json, created_at, updated_at)
             VALUES (?1, 'http', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                id.as_str(),
                new.url,
                new.filename,
                dir.to_string_lossy(),
                status.as_str(),
                new.referrer,
                serde_json::to_string(&new.headers)?,
                serde_json::to_string(&new.cookies)?,
                now
            ],
        )?;
        self.get(id)?
            .ok_or_else(|| CoreError::NotFound(id.to_string()))
    }

    /// One row.
    pub fn get(&self, id: &DownloadId) -> Result<Option<DownloadRow>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                &format!("{ROW_SELECT} WHERE d.id = ?1"),
                [id.as_str()],
                raw_row,
            )
            .optional()?;
        row.map(decode).transpose()
    }

    /// Every row, newest first.
    pub fn list(&self) -> Result<Vec<DownloadRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "{ROW_SELECT} ORDER BY d.created_at DESC, d.rowid DESC"
        ))?;
        let raws = stmt
            .query_map([], raw_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        raws.into_iter().map(decode).collect()
    }

    /// The oldest QUEUED row not in `exclude`.
    pub fn next_queued(&self, exclude: &[DownloadId]) -> Result<Option<DownloadId>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id FROM downloads WHERE status = 'QUEUED' ORDER BY created_at, rowid",
        )?;
        let ids = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for id in ids {
            let id = DownloadId(id?);
            if !exclude.contains(&id) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Change the status; `error = None` clears any stored error.
    pub fn set_status(
        &self,
        id: &DownloadId,
        status: DownloadStatus,
        error: Option<(&str, &str)>,
        now: i64,
    ) -> Result<()> {
        let (code, message) = match error {
            Some((c, m)) => (Some(c), Some(m)),
            None => (None, None),
        };
        let completed_at = (status == DownloadStatus::Completed).then_some(now);
        let n = self.conn.lock().unwrap().execute(
            "UPDATE downloads SET status = ?2, error_code = ?3, error_message = ?4, updated_at = ?5, completed_at = ?6 WHERE id = ?1",
            params![id.as_str(), status.as_str(), code, message, now, completed_at],
        )?;
        found(n, id)
    }

    /// Record what the probe learned.
    pub fn set_probe(&self, id: &DownloadId, p: &ProbeInfo, now: i64) -> Result<()> {
        let n = self.conn.lock().unwrap().execute(
            "UPDATE downloads SET final_url = ?2, filename = ?3, size = ?4, etag = ?5, last_modified = ?6, mime = ?7, updated_at = ?8 WHERE id = ?1",
            params![id.as_str(), p.final_url, p.filename, p.size.map(|s| s as i64), p.etag, p.last_modified, p.mime, now],
        )?;
        found(n, id)
    }

    /// Change the file name (the engine may pick `name (1).ext`).
    pub fn set_filename(&self, id: &DownloadId, filename: &str, now: i64) -> Result<()> {
        let n = self.conn.lock().unwrap().execute(
            "UPDATE downloads SET filename = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.as_str(), filename, now],
        )?;
        found(n, id)
    }

    /// At startup: rows that were probing or downloading when the app stopped go back to the queue.
    pub fn reset_interrupted(&self, now: i64) -> Result<usize> {
        Ok(self.conn.lock().unwrap().execute(
            "UPDATE downloads SET status = 'QUEUED', updated_at = ?1 WHERE status IN ('PROBING', 'DOWNLOADING')",
            [now],
        )?)
    }

    /// Delete a row and its segments.
    pub fn delete(&self, id: &DownloadId) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM downloads WHERE id = ?1", [id.as_str()])?;
        Ok(())
    }
}

fn found(n: usize, id: &DownloadId) -> Result<()> {
    if n == 0 {
        Err(CoreError::NotFound(id.to_string()))
    } else {
        Ok(())
    }
}

type Raw = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    Option<i64>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    i64,
    Option<i64>,
    i64,
);

fn raw_row(r: &Row<'_>) -> rusqlite::Result<Raw> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
        r.get(11)?,
        r.get(12)?,
        r.get(13)?,
        r.get(14)?,
        r.get(15)?,
        r.get(16)?,
        r.get(17)?,
        r.get(18)?,
        r.get(19)?,
    ))
}

fn decode(raw: Raw) -> Result<DownloadRow> {
    let (
        id,
        kind,
        url,
        final_url,
        filename,
        dir,
        size,
        status,
        etag,
        last_modified,
        mime,
        referrer,
        headers,
        cookies,
        error_code,
        error_message,
        created_at,
        updated_at,
        completed_at,
        seg_sum,
    ) = raw;
    let status = DownloadStatus::parse(&status)
        .ok_or_else(|| CoreError::InvalidState(format!("unknown status {status}")))?;
    let size = size.map(|s| s as u64);
    Ok(DownloadRow {
        id: DownloadId(id),
        kind: if kind == "torrent" {
            DownloadKind::Torrent
        } else {
            DownloadKind::Http
        },
        url,
        final_url,
        filename,
        dir: PathBuf::from(dir),
        size,
        downloaded: if status == DownloadStatus::Completed {
            size.unwrap_or(0)
        } else {
            seg_sum as u64
        },
        status,
        etag,
        last_modified,
        mime,
        referrer,
        headers: serde_json::from_str(&headers)?,
        cookies: serde_json::from_str(&cookies)?,
        error_code,
        error_message,
        created_at,
        updated_at,
        completed_at,
    })
}
