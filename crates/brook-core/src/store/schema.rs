//! Schema migrations, tracked with SQLite's `user_version` pragma.
//!
//! Each entry in [`MIGRATIONS`] moves the database up one version. Never edit
//! a migration that has shipped; append a new one.

use rusqlite::Connection;

use crate::error::Result;

/// Migrations in order. The database's `user_version` is the number applied.
const MIGRATIONS: &[&str] = &[
    // 1: the initial schema.
    r#"
    CREATE TABLE folders (
        id        INTEGER PRIMARY KEY,
        name      TEXT NOT NULL UNIQUE COLLATE NOCASE,
        position  INTEGER NOT NULL DEFAULT 0,
        expanded  INTEGER NOT NULL DEFAULT 1
    );

    CREATE TABLE feeds (
        id              INTEGER PRIMARY KEY,
        url             TEXT NOT NULL UNIQUE,
        title           TEXT NOT NULL,
        site_url        TEXT,
        description     TEXT,
        folder_id       INTEGER REFERENCES folders(id) ON DELETE SET NULL,
        position        INTEGER NOT NULL DEFAULT 0,
        etag            TEXT,
        last_modified   TEXT,
        last_fetched_at INTEGER,
        last_error      TEXT,
        created_at      INTEGER NOT NULL
    );

    CREATE TABLE articles (
        id           INTEGER PRIMARY KEY,
        feed_id      INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
        guid         TEXT NOT NULL,
        title        TEXT NOT NULL,
        url          TEXT,
        author       TEXT,
        excerpt      TEXT NOT NULL DEFAULT '',
        content_html TEXT,
        reader_html  TEXT,
        image_url    TEXT,
        published_at INTEGER NOT NULL,
        fetched_at   INTEGER NOT NULL,
        read_at      INTEGER,
        starred_at   INTEGER,
        saved_at     INTEGER,
        UNIQUE (feed_id, guid)
    );
    CREATE INDEX articles_by_date ON articles (published_at DESC);
    CREATE INDEX articles_by_feed ON articles (feed_id, published_at DESC);
    CREATE INDEX articles_unread ON articles (feed_id) WHERE read_at IS NULL;

    -- Full-text search over titles and bodies. Rows are written by the store
    -- next to their article; the trigger keeps deletes in step.
    CREATE VIRTUAL TABLE articles_fts USING fts5 (title, body, tokenize = 'unicode61 remove_diacritics 2');
    CREATE TRIGGER articles_fts_delete AFTER DELETE ON articles BEGIN
        DELETE FROM articles_fts WHERE rowid = old.id;
    END;

    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    "#,
];

/// Brings the database up to the newest schema.
pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    let applied: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(applied as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", index as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

/// The schema version this build expects.
#[cfg(test)]
pub(crate) fn latest_version() -> i64 {
    MIGRATIONS.len() as i64
}
