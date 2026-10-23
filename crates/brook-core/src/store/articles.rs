//! Articles: storing fetched entries, listing, reading state, and search.

use chrono::{DateTime, Duration, Utc};
use rusqlite::types::Value;
use rusqlite::{OptionalExtension, Row, params, params_from_iter};

use super::{Store, from_secs, to_secs};
use crate::error::{Error, Result};
use crate::model::{Article, ArticleId, ArticleSummary, Counts, FeedId, Scope};
use crate::parse::ParsedEntry;
use crate::text::html_to_text;

/// Longest article body indexed for search, in characters. Long enough for
/// any real article, short enough to keep the index small.
const SEARCH_BODY_CHARS: usize = 20_000;

/// What to list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArticleQuery {
    /// Which articles.
    pub scope: Scope,
    /// Only unread ones.
    pub unread_only: bool,
    /// Words to search for. Each word matches as a prefix.
    pub search: Option<String>,
    /// The start of the local day, for [`Scope::Today`].
    pub today_start: DateTime<Utc>,
    /// The most rows to return.
    pub limit: usize,
}

impl ArticleQuery {
    /// Everything in `scope`, up to 500 rows.
    pub fn new(scope: Scope, today_start: DateTime<Utc>) -> Self {
        Self { scope, unread_only: false, search: None, today_start, limit: 500 }
    }
}

const SUMMARY_COLUMNS: &str = "a.id, a.feed_id, a.title, a.excerpt, a.image_url, a.published_at,
     a.read_at IS NOT NULL, a.starred_at IS NOT NULL, a.saved_at IS NOT NULL";

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<ArticleSummary> {
    Ok(ArticleSummary {
        id: ArticleId(row.get(0)?),
        feed_id: FeedId(row.get(1)?),
        title: row.get(2)?,
        excerpt: row.get(3)?,
        image_url: row.get(4)?,
        published_at: from_secs(row.get(5)?),
        read: row.get(6)?,
        starred: row.get(7)?,
        saved: row.get(8)?,
    })
}

/// The `WHERE` clause and parameters selecting `scope`.
fn scope_filter(scope: Scope, today_start: DateTime<Utc>) -> (String, Vec<Value>) {
    match scope {
        Scope::All => ("1".into(), vec![]),
        Scope::Today => ("a.published_at >= ?".into(), vec![Value::Integer(to_secs(today_start))]),
        Scope::Starred => ("a.starred_at IS NOT NULL".into(), vec![]),
        Scope::Saved => ("a.saved_at IS NOT NULL".into(), vec![]),
        Scope::Feed(id) => ("a.feed_id = ?".into(), vec![Value::Integer(id.0)]),
        Scope::Folder(id) => {
            ("a.feed_id IN (SELECT id FROM feeds WHERE folder_id = ?)".into(), vec![Value::Integer(id.0)])
        }
    }
}

/// Turns what someone typed into an FTS5 query: every word must appear,
/// each as a prefix. Quotes keep FTS5 syntax characters literal.
fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> =
        input.split_whitespace().map(|word| format!("\"{}\"*", word.replace('"', "\"\""))).collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

impl Store {
    /// Stores a feed's entries. New entries are inserted unread; entries
    /// already stored get their text refreshed but keep their reading
    /// state. Returns how many were new.
    ///
    /// Entries without a date are dated `now`, minus one second per position
    /// so they keep the feed's order.
    pub fn upsert_entries(
        &mut self,
        feed: FeedId,
        entries: &[ParsedEntry],
        now: DateTime<Utc>,
    ) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut inserted = 0;
        {
            let mut existing =
                tx.prepare_cached("SELECT id FROM articles WHERE feed_id = ?1 AND guid = ?2")?;
            let mut insert = tx.prepare_cached(
                "INSERT INTO articles (feed_id, guid, title, url, author, excerpt, content_html,
                                       image_url, published_at, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            let mut update = tx.prepare_cached(
                "UPDATE articles SET title = ?2, url = ?3, author = ?4, excerpt = ?5,
                                     content_html = ?6, image_url = ?7
                 WHERE id = ?1",
            )?;
            let mut index_delete = tx.prepare_cached("DELETE FROM articles_fts WHERE rowid = ?1")?;
            let mut index_insert =
                tx.prepare_cached("INSERT INTO articles_fts (rowid, title, body) VALUES (?1, ?2, ?3)")?;

            for (position, entry) in entries.iter().enumerate() {
                let id: Option<i64> =
                    existing.query_row(params![feed.0, entry.guid], |row| row.get(0)).optional()?;
                let id = match id {
                    Some(id) => {
                        update.execute(params![
                            id,
                            entry.title,
                            entry.url,
                            entry.author,
                            entry.excerpt,
                            entry.content_html,
                            entry.image_url,
                        ])?;
                        index_delete.execute([id])?;
                        id
                    }
                    None => {
                        let published =
                            entry.published_at.unwrap_or_else(|| now - Duration::seconds(position as i64));
                        insert.execute(params![
                            feed.0,
                            entry.guid,
                            entry.title,
                            entry.url,
                            entry.author,
                            entry.excerpt,
                            entry.content_html,
                            entry.image_url,
                            to_secs(published),
                            to_secs(now),
                        ])?;
                        inserted += 1;
                        tx.last_insert_rowid()
                    }
                };
                let body = entry.content_html.as_deref().map(html_to_text).unwrap_or_default();
                let body: String = body.chars().take(SEARCH_BODY_CHARS).collect();
                index_insert.execute(params![id, entry.title, body])?;
            }
        }
        tx.commit()?;
        Ok(inserted)
    }

    /// Lists articles, newest first.
    pub fn articles(&self, query: &ArticleQuery) -> Result<Vec<ArticleSummary>> {
        let (scope_sql, mut values) = scope_filter(query.scope, query.today_start);
        let mut sql = format!("SELECT {SUMMARY_COLUMNS} FROM articles a WHERE {scope_sql}");
        if query.unread_only {
            sql.push_str(" AND a.read_at IS NULL");
        }
        if let Some(search) = query.search.as_deref().and_then(fts_query) {
            sql.push_str(" AND a.id IN (SELECT rowid FROM articles_fts WHERE articles_fts MATCH ?)");
            values.push(Value::Text(search));
        }
        sql.push_str(" ORDER BY a.published_at DESC, a.id DESC LIMIT ?");
        values.push(Value::Integer(query.limit as i64));

        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), summary_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// One article with its body.
    pub fn article(&self, id: ArticleId) -> Result<Article> {
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS}, a.url, a.author, a.content_html, a.reader_html
             FROM articles a WHERE a.id = ?1"
        );
        self.conn
            .query_row(&sql, [id.0], |row| {
                Ok(Article {
                    summary: summary_from_row(row)?,
                    url: row.get(9)?,
                    author: row.get(10)?,
                    content_html: row.get(11)?,
                    reader_html: row.get(12)?,
                })
            })
            .optional()?
            .ok_or(Error::NotFound("article"))
    }

    /// Marks articles read or unread.
    pub fn set_read(&self, ids: &[ArticleId], read: bool, at: DateTime<Utc>) -> Result<()> {
        let mut statement = self.conn.prepare_cached(
            "UPDATE articles SET read_at = CASE WHEN ?2 THEN COALESCE(read_at, ?3) END WHERE id = ?1",
        )?;
        for id in ids {
            statement.execute(params![id.0, read, to_secs(at)])?;
        }
        Ok(())
    }

    /// Stars or unstars an article.
    pub fn set_starred(&self, id: ArticleId, starred: bool, at: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "UPDATE articles SET starred_at = CASE WHEN ?2 THEN COALESCE(starred_at, ?3) END WHERE id = ?1",
            params![id.0, starred, to_secs(at)],
        )?;
        Ok(())
    }

    /// Saves an article for later, or takes it off the list.
    pub fn set_saved(&self, id: ArticleId, saved: bool, at: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "UPDATE articles SET saved_at = CASE WHEN ?2 THEN COALESCE(saved_at, ?3) END WHERE id = ?1",
            params![id.0, saved, to_secs(at)],
        )?;
        Ok(())
    }

    /// Marks everything in `scope` read, except articles fetched after
    /// `fetched_before`, so a refresh that lands mid-click isn't swallowed.
    /// Returns how many changed.
    pub fn mark_all_read(
        &self,
        scope: Scope,
        today_start: DateTime<Utc>,
        fetched_before: DateTime<Utc>,
    ) -> Result<usize> {
        let (scope_sql, mut values) = scope_filter(scope, today_start);
        let sql = format!(
            "UPDATE articles AS a SET read_at = ?
             WHERE a.read_at IS NULL AND a.fetched_at <= ? AND {scope_sql}"
        );
        let mut all = vec![Value::Integer(to_secs(Utc::now())), Value::Integer(to_secs(fetched_before))];
        all.append(&mut values);
        Ok(self.conn.execute(&sql, params_from_iter(all))?)
    }

    /// Caches the body that reader mode extracted from the web page.
    pub fn set_reader_html(&self, id: ArticleId, html: &str) -> Result<()> {
        self.conn.execute("UPDATE articles SET reader_html = ?2 WHERE id = ?1", params![id.0, html])?;
        Ok(())
    }

    /// Unread counts for the sidebar.
    pub fn counts(&self, today_start: DateTime<Utc>) -> Result<Counts> {
        let mut counts = Counts::default();
        let mut statement = self
            .conn
            .prepare("SELECT feed_id, COUNT(*) FROM articles WHERE read_at IS NULL GROUP BY feed_id")?;
        let rows = statement.query_map([], |row| Ok((FeedId(row.get(0)?), row.get::<_, u32>(1)?)))?;
        for row in rows {
            let (feed, unread) = row?;
            counts.all += unread;
            counts.feeds.insert(feed, unread);
        }
        (counts.today, counts.starred, counts.saved) = self.conn.query_row(
            "SELECT
                COUNT(*) FILTER (WHERE read_at IS NULL AND published_at >= ?1),
                COUNT(*) FILTER (WHERE starred_at IS NOT NULL),
                COUNT(*) FILTER (WHERE saved_at IS NOT NULL AND read_at IS NULL)
             FROM articles",
            [to_secs(today_start)],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(counts)
    }

    /// Frees space by dropping the bodies of old articles nobody kept:
    /// read, not starred, not saved, and published before `before`. Titles
    /// and excerpts stay, so the article still lists and links out, and its
    /// guid stays so the next fetch doesn't bring it back as new.
    /// Returns how many were compacted.
    pub fn compact(&self, before: DateTime<Utc>) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE articles SET content_html = NULL, reader_html = NULL
             WHERE published_at < ?1 AND read_at IS NOT NULL AND starred_at IS NULL
               AND saved_at IS NULL AND (content_html IS NOT NULL OR reader_html IS NOT NULL)",
            [to_secs(before)],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::NewFeed;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn entry(guid: &str, title: &str, published: Option<i64>) -> ParsedEntry {
        ParsedEntry {
            guid: guid.into(),
            title: title.into(),
            url: Some(format!("https://a.test/{guid}")),
            author: None,
            excerpt: format!("about {title}"),
            content_html: Some(format!("<p>The body of {title}</p>")),
            image_url: None,
            published_at: published.map(at),
        }
    }

    fn store_with_feed() -> (Store, FeedId) {
        let store = Store::open_in_memory().unwrap();
        let feed = store
            .add_feed(&NewFeed { url: "https://a.test/rss".into(), title: "A".into(), ..Default::default() })
            .unwrap();
        (store, feed)
    }

    fn titles(articles: &[ArticleSummary]) -> Vec<&str> {
        articles.iter().map(|a| a.title.as_str()).collect()
    }

    #[test]
    fn upsert_inserts_once_and_refreshes_text() {
        let (mut store, feed) = store_with_feed();
        let entries = [entry("1", "One", Some(100)), entry("2", "Two", Some(200))];
        assert_eq!(store.upsert_entries(feed, &entries, at(1000)).unwrap(), 2);

        let id = store.articles(&ArticleQuery::new(Scope::All, at(0))).unwrap()[1].id;
        store.set_read(&[id], true, at(1001)).unwrap();

        let edited = [entry("1", "One, edited", Some(100))];
        assert_eq!(store.upsert_entries(feed, &edited, at(2000)).unwrap(), 0);
        let article = store.article(id).unwrap();
        assert_eq!(article.summary.title, "One, edited");
        assert!(article.summary.read, "reading state survives a refresh");
    }

    #[test]
    fn undated_entries_keep_feed_order() {
        let (mut store, feed) = store_with_feed();
        let entries = [entry("1", "First", None), entry("2", "Second", None)];
        store.upsert_entries(feed, &entries, at(1000)).unwrap();
        let list = store.articles(&ArticleQuery::new(Scope::All, at(0))).unwrap();
        assert_eq!(titles(&list), ["First", "Second"]);
        assert_eq!(list[0].published_at, at(1000));
    }

    #[test]
    fn scopes_filter() {
        let (mut store, feed) = store_with_feed();
        let folder = store.create_folder("F").unwrap();
        let other = store
            .add_feed(&NewFeed {
                url: "https://b.test/rss".into(),
                title: "B".into(),
                folder_id: Some(folder),
                ..Default::default()
            })
            .unwrap();
        store.upsert_entries(feed, &[entry("1", "Old", Some(100))], at(1000)).unwrap();
        store.upsert_entries(other, &[entry("2", "New", Some(900))], at(1000)).unwrap();
        let all = store.articles(&ArticleQuery::new(Scope::All, at(500))).unwrap();
        let (new, old) = (all[0].id, all[1].id);
        store.set_starred(old, true, at(1000)).unwrap();
        store.set_saved(new, true, at(1000)).unwrap();

        let list = |scope| {
            titles(&store.articles(&ArticleQuery::new(scope, at(500))).unwrap())
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert_eq!(list(Scope::All), ["New", "Old"]);
        assert_eq!(list(Scope::Today), ["New"]);
        assert_eq!(list(Scope::Starred), ["Old"]);
        assert_eq!(list(Scope::Saved), ["New"]);
        assert_eq!(list(Scope::Feed(feed)), ["Old"]);
        assert_eq!(list(Scope::Folder(folder)), ["New"]);

        store.set_starred(old, false, at(1000)).unwrap();
        assert!(list(Scope::Starred).is_empty());
    }

    #[test]
    fn unread_only_and_limit() {
        let (mut store, feed) = store_with_feed();
        let entries: Vec<_> = (0..5).map(|i| entry(&i.to_string(), &format!("T{i}"), Some(i))).collect();
        store.upsert_entries(feed, &entries, at(1000)).unwrap();
        let newest = store.articles(&ArticleQuery::new(Scope::All, at(0))).unwrap()[0].id;
        store.set_read(&[newest], true, at(1000)).unwrap();

        let mut query = ArticleQuery::new(Scope::All, at(0));
        query.unread_only = true;
        query.limit = 2;
        assert_eq!(titles(&store.articles(&query).unwrap()), ["T3", "T2"]);
    }

    #[test]
    fn search_matches_prefixes_in_titles_and_bodies() {
        let (mut store, feed) = store_with_feed();
        store
            .upsert_entries(
                feed,
                &[entry("1", "Rust ownership", Some(1)), entry("2", "Café culture", Some(2))],
                at(1000),
            )
            .unwrap();
        let search = |text: &str| {
            let mut query = ArticleQuery::new(Scope::All, at(0));
            query.search = Some(text.into());
            titles(&store.articles(&query).unwrap()).into_iter().map(str::to_owned).collect::<Vec<_>>()
        };
        assert_eq!(search("own"), ["Rust ownership"]);
        assert_eq!(search("body rust"), ["Rust ownership"]);
        assert_eq!(search("cafe"), ["Café culture"], "diacritics fold");
        assert_eq!(search("\"unbalanced OR"), Vec::<String>::new(), "syntax is literal");
        assert_eq!(search("   ").len(), 2, "blank search lists everything");
    }

    #[test]
    fn search_index_follows_edits_and_deletes() {
        let (mut store, feed) = store_with_feed();
        store.upsert_entries(feed, &[entry("1", "Apples", Some(1))], at(1000)).unwrap();
        store.upsert_entries(feed, &[entry("1", "Pears", Some(1))], at(1000)).unwrap();
        let mut query = ArticleQuery::new(Scope::All, at(0));
        query.search = Some("apples".into());
        assert!(store.articles(&query).unwrap().is_empty());
        query.search = Some("pears".into());
        assert_eq!(store.articles(&query).unwrap().len(), 1);

        store.delete_feed(feed).unwrap();
        let indexed: i64 =
            store.conn.query_row("SELECT COUNT(*) FROM articles_fts", [], |r| r.get(0)).unwrap();
        assert_eq!(indexed, 0);
    }

    #[test]
    fn reading_state_round_trips() {
        let (mut store, feed) = store_with_feed();
        store.upsert_entries(feed, &[entry("1", "One", Some(1))], at(1000)).unwrap();
        let id = store.articles(&ArticleQuery::new(Scope::All, at(0))).unwrap()[0].id;

        store.set_read(&[id], true, at(5)).unwrap();
        store.set_read(&[id], true, at(9)).unwrap();
        let read_at: i64 =
            store.conn.query_row("SELECT read_at FROM articles WHERE id = ?1", [id.0], |r| r.get(0)).unwrap();
        assert_eq!(read_at, 5, "marking read twice keeps the first time");

        store.set_read(&[id], false, at(10)).unwrap();
        assert!(!store.article(id).unwrap().summary.read);

        store.set_reader_html(id, "<p>clean</p>").unwrap();
        let article = store.article(id).unwrap();
        assert_eq!(article.reader_html.as_deref(), Some("<p>clean</p>"));
        assert_eq!(article.url.as_deref(), Some("https://a.test/1"));
        assert!(matches!(store.article(ArticleId(404)), Err(Error::NotFound(_))));
    }

    #[test]
    fn mark_all_read_spares_late_arrivals() {
        let (mut store, feed) = store_with_feed();
        store.upsert_entries(feed, &[entry("1", "Before", Some(1))], at(1000)).unwrap();
        store.upsert_entries(feed, &[entry("2", "After", Some(2))], at(3000)).unwrap();
        assert_eq!(store.mark_all_read(Scope::Feed(feed), at(0), at(2000)).unwrap(), 1);
        let counts = store.counts(at(0)).unwrap();
        assert_eq!(counts.all, 1);
    }

    #[test]
    fn counts_unread() {
        let (mut store, feed) = store_with_feed();
        store
            .upsert_entries(feed, &[entry("1", "Old", Some(10)), entry("2", "New", Some(500))], at(1000))
            .unwrap();
        let list = store.articles(&ArticleQuery::new(Scope::All, at(0))).unwrap();
        store.set_starred(list[1].id, true, at(1000)).unwrap();
        store.set_saved(list[0].id, true, at(1000)).unwrap();

        let counts = store.counts(at(100)).unwrap();
        assert_eq!((counts.all, counts.today, counts.starred, counts.saved), (2, 1, 1, 1));
        assert_eq!(counts.feed(feed), 2);

        store.set_read(&[list[0].id, list[1].id], true, at(1000)).unwrap();
        let counts = store.counts(at(100)).unwrap();
        assert_eq!((counts.all, counts.today, counts.starred, counts.saved), (0, 0, 1, 0));
        assert!(counts.feeds.is_empty());
    }

    #[test]
    fn compact_keeps_what_people_kept() {
        let (mut store, feed) = store_with_feed();
        let entries: Vec<_> = ["read", "starred", "unread", "fresh"]
            .iter()
            .enumerate()
            .map(|(i, name)| entry(name, name, Some(if *name == "fresh" { 900 } else { i as i64 })))
            .collect();
        store.upsert_entries(feed, &entries, at(1000)).unwrap();
        let list = store.articles(&ArticleQuery::new(Scope::All, at(0))).unwrap();
        let id = |title: &str| list.iter().find(|a| a.title == title).unwrap().id;
        store.set_read(&[id("read"), id("starred"), id("fresh")], true, at(1000)).unwrap();
        store.set_starred(id("starred"), true, at(1000)).unwrap();

        assert_eq!(store.compact(at(500)).unwrap(), 1);
        assert_eq!(store.article(id("read")).unwrap().content_html, None);
        assert!(store.article(id("starred")).unwrap().content_html.is_some());
        assert!(store.article(id("unread")).unwrap().content_html.is_some());
        assert!(store.article(id("fresh")).unwrap().content_html.is_some());
        assert_eq!(store.compact(at(500)).unwrap(), 0, "already compacted");
        assert_eq!(store.upsert_entries(feed, &entries, at(2000)).unwrap(), 0, "no resurrection");
    }

    #[test]
    fn fts_query_quotes_terms() {
        assert_eq!(fts_query("a b").as_deref(), Some("\"a\"* \"b\"*"));
        assert_eq!(fts_query("say \"hi\"").as_deref(), Some("\"say\"* \"\"\"hi\"\"\"*"));
        assert_eq!(fts_query("  "), None);
    }
}
