//! Folders and feeds.

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params};

use super::{Store, from_secs, to_secs};
use crate::error::{Error, Result};
use crate::model::{Feed, FeedId, Folder, FolderId};

/// What it takes to subscribe.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewFeed {
    /// The feed document's address.
    pub url: String,
    /// Display title.
    pub title: String,
    /// The website the feed belongs to.
    pub site_url: Option<String>,
    /// The feed's description.
    pub description: Option<String>,
    /// Where to file it.
    pub folder_id: Option<FolderId>,
}

/// Validators from the last successful fetch, sent back on the next one so
/// unchanged feeds cost a `304 Not Modified`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpCache {
    /// The `ETag` header.
    pub etag: Option<String>,
    /// The `Last-Modified` header.
    pub last_modified: Option<String>,
}

const FEED_COLUMNS: &str =
    "id, url, title, site_url, description, folder_id, position, last_fetched_at, last_error";

fn feed_from_row(row: &Row<'_>) -> rusqlite::Result<Feed> {
    Ok(Feed {
        id: FeedId(row.get(0)?),
        url: row.get(1)?,
        title: row.get(2)?,
        site_url: row.get(3)?,
        description: row.get(4)?,
        folder_id: row.get::<_, Option<i64>>(5)?.map(FolderId),
        position: row.get(6)?,
        last_fetched_at: row.get::<_, Option<i64>>(7)?.map(from_secs),
        last_error: row.get(8)?,
    })
}

impl Store {
    /// Every folder, in sidebar order.
    pub fn folders(&self) -> Result<Vec<Folder>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, position, expanded FROM folders ORDER BY position, name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Folder {
                id: FolderId(row.get(0)?),
                name: row.get(1)?,
                position: row.get(2)?,
                expanded: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Creates a folder, or returns the existing one with the same name
    /// (ignoring case). Surrounding whitespace is trimmed.
    pub fn create_folder(&self, name: &str) -> Result<FolderId> {
        let name = name.trim();
        if let Some(id) = self
            .conn
            .query_row("SELECT id FROM folders WHERE name = ?1", [name], |row| row.get(0))
            .optional()?
        {
            return Ok(FolderId(id));
        }
        self.conn.execute(
            "INSERT INTO folders (name, position)
             VALUES (?1, (SELECT COALESCE(MAX(position), -1) + 1 FROM folders))",
            [name],
        )?;
        Ok(FolderId(self.conn.last_insert_rowid()))
    }

    /// Renames a folder.
    pub fn rename_folder(&self, id: FolderId, name: &str) -> Result<()> {
        let changed =
            self.conn.execute("UPDATE folders SET name = ?2 WHERE id = ?1", params![id.0, name.trim()])?;
        if changed == 0 { Err(Error::NotFound("folder")) } else { Ok(()) }
    }

    /// Deletes a folder. Its feeds move to the top level.
    pub fn delete_folder(&self, id: FolderId) -> Result<()> {
        self.conn.execute("DELETE FROM folders WHERE id = ?1", [id.0])?;
        Ok(())
    }

    /// Opens or closes a folder in the sidebar.
    pub fn set_folder_expanded(&self, id: FolderId, expanded: bool) -> Result<()> {
        self.conn.execute("UPDATE folders SET expanded = ?2 WHERE id = ?1", params![id.0, expanded])?;
        Ok(())
    }

    /// Every feed, in sidebar order within each folder.
    pub fn feeds(&self) -> Result<Vec<Feed>> {
        let sql = format!("SELECT {FEED_COLUMNS} FROM feeds ORDER BY position, title COLLATE NOCASE");
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map([], feed_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// One feed.
    pub fn feed(&self, id: FeedId) -> Result<Feed> {
        let sql = format!("SELECT {FEED_COLUMNS} FROM feeds WHERE id = ?1");
        self.conn.query_row(&sql, [id.0], feed_from_row).optional()?.ok_or(Error::NotFound("feed"))
    }

    /// The feed subscribed at `url`, if any.
    pub fn feed_by_url(&self, url: &str) -> Result<Option<Feed>> {
        let sql = format!("SELECT {FEED_COLUMNS} FROM feeds WHERE url = ?1");
        Ok(self.conn.query_row(&sql, [url], feed_from_row).optional()?)
    }

    /// Subscribes to a feed. Subscribing twice to the same URL returns the
    /// existing feed's id and changes nothing.
    pub fn add_feed(&self, feed: &NewFeed) -> Result<FeedId> {
        if let Some(existing) = self.feed_by_url(&feed.url)? {
            return Ok(existing.id);
        }
        self.conn.execute(
            "INSERT INTO feeds (url, title, site_url, description, folder_id, position, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5,
                     (SELECT COALESCE(MAX(position), -1) + 1 FROM feeds), ?6)",
            params![
                feed.url,
                feed.title.trim(),
                feed.site_url,
                feed.description,
                feed.folder_id.map(|id| id.0),
                to_secs(Utc::now()),
            ],
        )?;
        Ok(FeedId(self.conn.last_insert_rowid()))
    }

    /// Renames a feed.
    pub fn rename_feed(&self, id: FeedId, title: &str) -> Result<()> {
        let changed =
            self.conn.execute("UPDATE feeds SET title = ?2 WHERE id = ?1", params![id.0, title.trim()])?;
        if changed == 0 { Err(Error::NotFound("feed")) } else { Ok(()) }
    }

    /// Files a feed in a folder, or at the top level for `None`.
    pub fn move_feed(&self, id: FeedId, folder: Option<FolderId>) -> Result<()> {
        self.conn
            .execute("UPDATE feeds SET folder_id = ?2 WHERE id = ?1", params![id.0, folder.map(|f| f.0)])?;
        Ok(())
    }

    /// Unsubscribes, deleting the feed's articles too.
    pub fn delete_feed(&self, id: FeedId) -> Result<()> {
        self.conn.execute("DELETE FROM feeds WHERE id = ?1", [id.0])?;
        Ok(())
    }

    /// The validators to send with the next fetch.
    pub fn http_cache(&self, id: FeedId) -> Result<HttpCache> {
        self.conn
            .query_row("SELECT etag, last_modified FROM feeds WHERE id = ?1", [id.0], |row| {
                Ok(HttpCache { etag: row.get(0)?, last_modified: row.get(1)? })
            })
            .optional()?
            .ok_or(Error::NotFound("feed"))
    }

    /// Records a successful fetch: new validators, no error. When the feed
    /// reports a website and the feed has none yet, it is filled in.
    pub fn record_fetch_success(
        &self,
        id: FeedId,
        cache: &HttpCache,
        site_url: Option<&str>,
        at: DateTime<Utc>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE feeds SET etag = ?2, last_modified = ?3, last_fetched_at = ?4, last_error = NULL,
                              site_url = COALESCE(site_url, ?5)
             WHERE id = ?1",
            params![id.0, cache.etag, cache.last_modified, to_secs(at), site_url],
        )?;
        Ok(())
    }

    /// Records a failed fetch. The validators are kept.
    pub fn record_fetch_error(&self, id: FeedId, error: &str, at: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "UPDATE feeds SET last_fetched_at = ?2, last_error = ?3 WHERE id = ?1",
            params![id.0, to_secs(at), error],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_feed(url: &str) -> NewFeed {
        NewFeed { url: url.into(), title: format!("  {url}  "), ..NewFeed::default() }
    }

    #[test]
    fn folders_are_unique_ignoring_case_and_ordered() {
        let store = Store::open_in_memory().unwrap();
        let tech = store.create_folder(" Tech ").unwrap();
        let art = store.create_folder("Art").unwrap();
        assert_eq!(store.create_folder("tech").unwrap(), tech);
        let names: Vec<_> = store.folders().unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["Tech", "Art"]);

        store.rename_folder(art, "Arts").unwrap();
        store.set_folder_expanded(art, false).unwrap();
        let arts = store.folders().unwrap().into_iter().find(|f| f.id == art).unwrap();
        assert_eq!(arts.name, "Arts");
        assert!(!arts.expanded);
        assert!(matches!(store.rename_folder(FolderId(99), "x"), Err(Error::NotFound(_))));
    }

    #[test]
    fn deleting_a_folder_keeps_its_feeds() {
        let store = Store::open_in_memory().unwrap();
        let folder = store.create_folder("News").unwrap();
        let feed =
            store.add_feed(&NewFeed { folder_id: Some(folder), ..new_feed("https://a.test/rss") }).unwrap();
        store.delete_folder(folder).unwrap();
        assert_eq!(store.feed(feed).unwrap().folder_id, None);
    }

    #[test]
    fn subscribing_twice_returns_the_same_feed() {
        let store = Store::open_in_memory().unwrap();
        let first = store.add_feed(&new_feed("https://a.test/rss")).unwrap();
        let again = store.add_feed(&new_feed("https://a.test/rss")).unwrap();
        assert_eq!(first, again);
        assert_eq!(store.feeds().unwrap().len(), 1);
        assert_eq!(store.feed(first).unwrap().title, "https://a.test/rss");
    }

    #[test]
    fn feeds_rename_move_and_delete() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_feed(&new_feed("https://a.test/rss")).unwrap();
        let folder = store.create_folder("F").unwrap();
        store.rename_feed(id, "Alpha").unwrap();
        store.move_feed(id, Some(folder)).unwrap();
        let feed = store.feed(id).unwrap();
        assert_eq!((feed.title.as_str(), feed.folder_id), ("Alpha", Some(folder)));
        store.delete_feed(id).unwrap();
        assert!(matches!(store.feed(id), Err(Error::NotFound(_))));
        assert_eq!(store.feed_by_url("https://a.test/rss").unwrap(), None);
    }

    #[test]
    fn fetch_results_are_recorded() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_feed(&new_feed("https://a.test/rss")).unwrap();
        let at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let cache = HttpCache { etag: Some("\"v1\"".into()), last_modified: None };

        store.record_fetch_error(id, "boom", at).unwrap();
        let feed = store.feed(id).unwrap();
        assert_eq!(feed.last_error.as_deref(), Some("boom"));
        assert_eq!(feed.last_fetched_at, Some(at));

        store.record_fetch_success(id, &cache, Some("https://a.test/"), at).unwrap();
        store.record_fetch_success(id, &cache, Some("https://other.test/"), at).unwrap();
        let feed = store.feed(id).unwrap();
        assert_eq!(feed.last_error, None);
        assert_eq!(feed.site_url.as_deref(), Some("https://a.test/"));
        assert_eq!(store.http_cache(id).unwrap(), cache);
    }
}
