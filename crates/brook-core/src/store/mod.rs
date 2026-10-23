//! Brook's SQLite database: subscriptions, articles, and settings.
//!
//! A [`Store`] owns one connection. Open one per thread: the UI keeps one for
//! quick reads and the refresher opens its own for writes. The database runs
//! in WAL mode, so readers never wait for the writer.

mod articles;
mod feeds;
mod schema;

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;

pub use articles::ArticleQuery;
pub use feeds::{HttpCache, NewFeed};

/// A connection to Brook's database.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating if needed) the database at `path` and migrates it.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::configure(conn)
    }

    /// Opens a private database in memory. For tests and previews.
    pub fn open_in_memory() -> Result<Self> {
        Self::configure(Connection::open_in_memory()?)
    }

    fn configure(mut conn: Connection) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        schema::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// Reads a raw setting.
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0))
            .optional()?)
    }

    /// Writes a raw setting.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }
}

/// Seconds since the Unix epoch, as stored in the database.
fn to_secs(time: DateTime<Utc>) -> i64 {
    time.timestamp()
}

/// The inverse of [`to_secs`]. Out-of-range values clamp to the epoch.
fn from_secs(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, 0).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_a_new_database() {
        let store = Store::open_in_memory().unwrap();
        let version: i64 = store.conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version, schema::latest_version());
    }

    #[test]
    fn reopening_keeps_data_and_does_not_remigrate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/brook.db");
        Store::open(&path).unwrap().set_setting("k", "v").unwrap();
        let store = Store::open(&path).unwrap();
        assert_eq!(store.setting("k").unwrap().as_deref(), Some("v"));
    }

    #[test]
    fn settings_upsert() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.setting("theme").unwrap(), None);
        store.set_setting("theme", "dark").unwrap();
        store.set_setting("theme", "light").unwrap();
        assert_eq!(store.setting("theme").unwrap().as_deref(), Some("light"));
    }
}
