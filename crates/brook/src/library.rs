//! The app's state: subscriptions, the article list, the open article, and
//! background work. Views observe the [`Library`] entity and call its
//! methods; it owns the UI thread's database connection.
//!
//! Blocking work (network, bulk writes) runs on background threads that
//! open their own [`Store`]. Results come back through `cx.spawn` and land
//! here, followed by a reload of whatever they changed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use brook_core::document::Document;
use brook_core::model::{Article, ArticleId, ArticleSummary, Counts, Feed, FeedId, Folder, FolderId, Scope};
use brook_core::store::{ArticleQuery, NewFeed};
use brook_core::{Store, discover, http, opml, reader, refresh, settings::Settings, time};
use chrono::Utc;
use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{AppContext, Context, EventEmitter, SharedString, Task};

/// How often the background loop checks for feeds due a refresh.
const DUE_CHECK: Duration = Duration::from_secs(60);

/// Bodies of read articles older than this are dropped to save space.
const COMPACT_AFTER_DAYS: i64 = 90;

/// Something worth telling the person, shown as a toast.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// Good news.
    Info(SharedString),
    /// Something failed.
    Error(SharedString),
}

impl EventEmitter<Notice> for Library {}

/// Progress of a refresh.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Feeds finished.
    pub done: usize,
    /// Feeds in this refresh.
    pub total: usize,
}

/// Reader mode for the open article.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReaderMode {
    /// The feed's own body.
    Off,
    /// Fetching the web page.
    Loading,
    /// Showing the extracted article.
    On,
    /// Extraction failed; the feed's body is shown.
    Failed(SharedString),
}

/// The article in the reader.
#[derive(Clone, Debug)]
pub struct OpenArticle {
    /// The stored article.
    pub article: Article,
    /// Its body, typeset.
    pub document: Document,
    /// Reader mode.
    pub mode: ReaderMode,
}

impl OpenArticle {
    fn new(article: Article) -> Self {
        let document = feed_document(&article);
        Self { article, document, mode: ReaderMode::Off }
    }
}

fn feed_document(article: &Article) -> Document {
    let url = article.url.as_deref();
    match article.content_html.as_deref() {
        Some(html) => Document::from_html(html, url),
        None => Document::from_text(&article.summary.excerpt),
    }
    .with_lead_video(url)
    .with_lead_image(article.summary.image_url.as_deref())
}

/// What the main area shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// The article list and reader.
    Reading,
    /// The feed catalog.
    Discover,
}

/// Subscriptions, articles, selection, and background work.
pub struct Library {
    store: Store,
    db_path: PathBuf,
    client: http::Client,
    /// Preferences.
    pub settings: Settings,
    /// Folders in sidebar order.
    pub folders: Vec<Folder>,
    /// Feeds in sidebar order.
    pub feeds: Vec<Feed>,
    /// Unread counts.
    pub counts: Counts,
    /// What the main area shows.
    pub page: Page,
    /// Which articles the list shows.
    pub scope: Scope,
    /// The search text, if searching.
    pub search: String,
    /// The list, newest first.
    pub articles: Vec<ArticleSummary>,
    /// The open article.
    pub open: Option<OpenArticle>,
    /// The running refresh.
    pub progress: Option<Progress>,
    _due_loop: Task<()>,
}

impl Library {
    /// The library in `store`, which was opened from `db_path`. Compacts old
    /// articles and starts the background refresh loop.
    pub fn new(store: Store, db_path: &Path, cx: &mut Context<Self>) -> Self {
        let settings = Settings::load(&store);
        let cutoff = Utc::now() - chrono::Duration::days(COMPACT_AFTER_DAYS);
        if let Err(error) = store.compact(cutoff) {
            log::warn!("compacting old articles failed: {error}");
        }
        let due_loop = cx.spawn(async move |this, cx| {
            loop {
                if this.update(cx, |this, cx| this.refresh_due(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(DUE_CHECK).await;
            }
        });
        let mut library = Self {
            store,
            db_path: db_path.to_owned(),
            client: http::Client::default(),
            settings,
            folders: vec![],
            feeds: vec![],
            counts: Counts::default(),
            page: Page::Reading,
            scope: Scope::All,
            search: String::new(),
            articles: vec![],
            open: None,
            progress: None,
            _due_loop: due_loop,
        };
        library.reload_quietly();
        library
    }

    /// The shared HTTP client.
    pub fn client(&self) -> &http::Client {
        &self.client
    }

    /// A feed by id.
    pub fn feed(&self, id: FeedId) -> Option<&Feed> {
        self.feeds.iter().find(|feed| feed.id == id)
    }

    /// The feeds filed in `folder`, or unfiled for `None`.
    pub fn feeds_in(&self, folder: Option<FolderId>) -> impl Iterator<Item = &Feed> {
        self.feeds.iter().filter(move |feed| feed.folder_id == folder)
    }

    /// A display title for the current scope.
    pub fn scope_title(&self) -> SharedString {
        match self.scope {
            Scope::All => "All articles".into(),
            Scope::Today => "Today".into(),
            Scope::Starred => "Starred".into(),
            Scope::Saved => "Read later".into(),
            Scope::Folder(id) => {
                self.folders.iter().find(|f| f.id == id).map(|f| f.name.clone()).unwrap_or_default().into()
            }
            Scope::Feed(id) => self.feed(id).map(|f| f.title.clone()).unwrap_or_default().into(),
        }
    }

    /// Unread articles in the current scope.
    pub fn scope_unread(&self) -> u32 {
        match self.scope {
            Scope::All => self.counts.all,
            Scope::Today => self.counts.today,
            Scope::Starred => 0,
            Scope::Saved => self.counts.saved,
            Scope::Folder(id) => self.counts.sum(self.feeds_in(Some(id))),
            Scope::Feed(id) => self.counts.feed(id),
        }
    }

    fn report<T>(&self, result: brook_core::Result<T>, cx: &mut Context<Self>) -> Option<T> {
        result.map_err(|error| cx.emit(Notice::Error(error.to_string().into()))).ok()
    }

    // Loading ---------------------------------------------------------------

    fn reload_quietly(&mut self) {
        let loaded = (|| -> brook_core::Result<()> {
            self.folders = self.store.folders()?;
            self.feeds = self.store.feeds()?;
            self.counts = self.store.counts(time::local_start_of_today())?;
            self.articles = self.store.articles(&self.query())?;
            Ok(())
        })();
        if let Err(error) = loaded {
            log::error!("loading the library failed: {error}");
        }
    }

    /// Reloads everything from the database.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.reload_quietly();
        cx.notify();
    }

    fn reload_counts(&mut self, cx: &mut Context<Self>) {
        if let Some(counts) = self.report(self.store.counts(time::local_start_of_today()), cx) {
            self.counts = counts;
        }
        cx.notify();
    }

    fn query(&self) -> ArticleQuery {
        let mut query = ArticleQuery::new(self.scope, time::local_start_of_today());
        let searching = !self.search.trim().is_empty();
        // Starred and saved lists are collections; hiding read ones there
        // would hide most of them.
        query.unread_only =
            self.settings.unread_only && !searching && !matches!(self.scope, Scope::Starred | Scope::Saved);
        query.search = searching.then(|| self.search.clone());
        query
    }

    fn reload_articles(&mut self, cx: &mut Context<Self>) {
        if let Some(articles) = self.report(self.store.articles(&self.query()), cx) {
            self.articles = articles;
        }
        cx.notify();
    }

    // Navigation ------------------------------------------------------------

    /// Shows `scope` in the list.
    pub fn select_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        self.page = Page::Reading;
        if self.scope != scope {
            self.scope = scope;
            self.open = None;
        }
        self.reload_articles(cx);
    }

    /// Switches the main area.
    pub fn show_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        cx.notify();
    }

    /// Filters the list by `text`.
    pub fn set_search(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.search != text {
            self.search = text.to_owned();
            self.reload_articles(cx);
        }
    }

    /// An article's web address.
    pub fn article_url(&self, id: ArticleId) -> Option<String> {
        self.store.article(id).ok()?.url
    }

    /// The open article's position in the list.
    pub fn selected_index(&self) -> Option<usize> {
        let open = self.open.as_ref()?.article.summary.id;
        self.articles.iter().position(|article| article.id == open)
    }

    /// Opens an article, marking it read if the settings say so.
    pub fn open_article(&mut self, id: ArticleId, cx: &mut Context<Self>) {
        let Some(mut article) = self.report(self.store.article(id), cx) else { return };
        if self.settings.mark_read_on_open && !article.summary.read {
            self.report(self.store.set_read(&[id], true, Utc::now()), cx);
            article.summary.read = true;
            self.patch_summary(id, |summary| summary.read = true);
            self.reload_counts(cx);
        }
        self.open = Some(OpenArticle::new(article));
        cx.notify();
    }

    /// Opens the article `offset` rows from the open one (or the first).
    pub fn step(&mut self, offset: isize, cx: &mut Context<Self>) {
        let next = match self.selected_index() {
            Some(index) => index.checked_add_signed(offset).filter(|&i| i < self.articles.len()),
            None => (!self.articles.is_empty()).then_some(0),
        };
        if let Some(id) = next.map(|index| self.articles[index].id) {
            self.open_article(id, cx);
        }
    }

    /// Closes the reader.
    pub fn close_article(&mut self, cx: &mut Context<Self>) {
        self.open = None;
        cx.notify();
    }

    // Reading state -----------------------------------------------------------

    fn patch_summary(&mut self, id: ArticleId, change: impl Fn(&mut ArticleSummary)) {
        if let Some(summary) = self.articles.iter_mut().find(|a| a.id == id) {
            change(summary);
        }
        if let Some(open) = self.open.as_mut().filter(|open| open.article.summary.id == id) {
            change(&mut open.article.summary);
        }
    }

    fn summary(&self, id: ArticleId) -> Option<&ArticleSummary> {
        self.articles
            .iter()
            .find(|a| a.id == id)
            .or_else(|| self.open.as_ref().map(|open| &open.article.summary).filter(|s| s.id == id))
    }

    /// Flips read and unread.
    pub fn toggle_read(&mut self, id: ArticleId, cx: &mut Context<Self>) {
        let Some(read) = self.summary(id).map(|s| !s.read) else { return };
        if self.report(self.store.set_read(&[id], read, Utc::now()), cx).is_some() {
            self.patch_summary(id, |s| s.read = read);
            self.reload_counts(cx);
        }
    }

    /// Stars or unstars.
    pub fn toggle_starred(&mut self, id: ArticleId, cx: &mut Context<Self>) {
        let Some(starred) = self.summary(id).map(|s| !s.starred) else { return };
        if self.report(self.store.set_starred(id, starred, Utc::now()), cx).is_some() {
            self.patch_summary(id, |s| s.starred = starred);
            self.reload_counts(cx);
        }
    }

    /// Saves for later, or takes it off the list.
    pub fn toggle_saved(&mut self, id: ArticleId, cx: &mut Context<Self>) {
        let Some(saved) = self.summary(id).map(|s| !s.saved) else { return };
        if self.report(self.store.set_saved(id, saved, Utc::now()), cx).is_some() {
            self.patch_summary(id, |s| s.saved = saved);
            self.reload_counts(cx);
        }
    }

    /// Marks everything in `scope` read.
    pub fn mark_all_read(&mut self, scope: Scope, cx: &mut Context<Self>) {
        let now = Utc::now();
        if self.report(self.store.mark_all_read(scope, time::local_start_of_today(), now), cx).is_some() {
            if let Some(open) = &mut self.open {
                open.article.summary.read |= scope == self.scope;
            }
            self.reload_counts(cx);
            self.reload_articles(cx);
        }
    }

    /// Switches reader mode for the open article, fetching the page the
    /// first time.
    pub fn toggle_reader_mode(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &mut self.open else { return };
        match open.mode {
            ReaderMode::On | ReaderMode::Failed(_) => {
                open.mode = ReaderMode::Off;
                open.document = feed_document(&open.article);
            }
            ReaderMode::Loading => {}
            ReaderMode::Off => {
                if let Some(html) = open.article.reader_html.clone() {
                    open.document = Document::from_html(&html, open.article.url.as_deref());
                    open.mode = ReaderMode::On;
                } else if let Some(url) = open.article.url.clone() {
                    open.mode = ReaderMode::Loading;
                    let id = open.article.summary.id;
                    let client = self.client.clone();
                    let fetch = cx.background_spawn(async move { reader::fetch(&client, &url) });
                    cx.spawn(async move |this, cx| {
                        let result = fetch.await;
                        this.update(cx, |this, cx| this.reader_loaded(id, result, cx)).ok();
                    })
                    .detach();
                } else {
                    open.mode = ReaderMode::Failed("This article has no web page.".into());
                }
            }
        }
        cx.notify();
    }

    fn reader_loaded(&mut self, id: ArticleId, result: brook_core::Result<String>, cx: &mut Context<Self>) {
        if let Ok(html) = &result {
            self.report(self.store.set_reader_html(id, html), cx);
        }
        let Some(open) = self.open.as_mut().filter(|open| open.article.summary.id == id) else { return };
        if open.mode != ReaderMode::Loading {
            return;
        }
        match result {
            Ok(html) => {
                open.document = Document::from_html(&html, open.article.url.as_deref());
                open.article.reader_html = Some(html);
                open.mode = ReaderMode::On;
            }
            Err(error) => open.mode = ReaderMode::Failed(error.to_string().into()),
        }
        cx.notify();
    }

    // Subscriptions -----------------------------------------------------------

    /// Subscribes to a feed found by discovery, storing the entries it
    /// already fetched, and opens it.
    pub fn subscribe(
        &mut self,
        found: &discover::Discovered,
        folder: Option<FolderId>,
        cx: &mut Context<Self>,
    ) {
        let new = NewFeed {
            url: found.url.clone(),
            title: found.feed.title.clone(),
            site_url: found.feed.site_url.clone(),
            description: found.feed.description.clone(),
            folder_id: folder,
        };
        let Some(id) = self.report(self.store.add_feed(&new), cx) else { return };
        let stored = self.store.upsert_entries(id, &found.feed.entries, Utc::now());
        self.report(stored, cx);
        self.reload_quietly();
        self.select_scope(Scope::Feed(id), cx);
        cx.emit(Notice::Info(format!("Subscribed to {}", found.feed.title).into()));
    }

    /// Subscribes to a feed by address and fetches it.
    pub fn subscribe_url(&mut self, url: &str, title: &str, site_url: Option<&str>, cx: &mut Context<Self>) {
        let new = NewFeed {
            url: url.to_owned(),
            title: title.to_owned(),
            site_url: site_url.map(str::to_owned),
            ..NewFeed::default()
        };
        let Some(id) = self.report(self.store.add_feed(&new), cx) else { return };
        self.reload_quietly();
        cx.notify();
        if let Some(feed) = self.feed(id).cloned() {
            self.refresh(vec![feed], cx);
        }
    }

    /// Whether a feed with this address is subscribed.
    pub fn is_subscribed(&self, url: &str) -> bool {
        self.feeds.iter().any(|feed| feed.url == url)
    }

    /// Renames a feed.
    pub fn rename_feed(&mut self, id: FeedId, title: &str, cx: &mut Context<Self>) {
        if !title.trim().is_empty() {
            self.report(self.store.rename_feed(id, title), cx);
            self.reload(cx);
        }
    }

    /// Files a feed in a folder.
    pub fn move_feed(&mut self, id: FeedId, folder: Option<FolderId>, cx: &mut Context<Self>) {
        self.report(self.store.move_feed(id, folder), cx);
        self.reload(cx);
    }

    /// Unsubscribes.
    pub fn delete_feed(&mut self, id: FeedId, cx: &mut Context<Self>) {
        if self.report(self.store.delete_feed(id), cx).is_none() {
            return;
        }
        if self.scope == Scope::Feed(id) {
            self.scope = Scope::All;
        }
        if self.open.as_ref().is_some_and(|open| open.article.summary.feed_id == id) {
            self.open = None;
        }
        self.reload(cx);
    }

    /// Creates a folder and returns it.
    pub fn create_folder(&mut self, name: &str, cx: &mut Context<Self>) -> Option<FolderId> {
        if name.trim().is_empty() {
            return None;
        }
        let id = self.report(self.store.create_folder(name), cx);
        self.reload(cx);
        id
    }

    /// Renames a folder.
    pub fn rename_folder(&mut self, id: FolderId, name: &str, cx: &mut Context<Self>) {
        if !name.trim().is_empty() {
            self.report(self.store.rename_folder(id, name), cx);
            self.reload(cx);
        }
    }

    /// Deletes a folder; its feeds move to the top level.
    pub fn delete_folder(&mut self, id: FolderId, cx: &mut Context<Self>) {
        self.report(self.store.delete_folder(id), cx);
        if self.scope == Scope::Folder(id) {
            self.scope = Scope::All;
        }
        self.reload(cx);
    }

    /// Opens or closes a folder in the sidebar.
    pub fn toggle_folder(&mut self, id: FolderId, cx: &mut Context<Self>) {
        let Some(folder) = self.folders.iter_mut().find(|f| f.id == id) else { return };
        folder.expanded = !folder.expanded;
        let expanded = folder.expanded;
        self.report(self.store.set_folder_expanded(id, expanded), cx);
        cx.notify();
    }

    // OPML ------------------------------------------------------------------

    /// Imports subscriptions from an OPML file and fetches the new ones.
    pub fn import_opml(&mut self, path: &Path, cx: &mut Context<Self>) {
        let result = std::fs::read_to_string(path)
            .map_err(brook_core::Error::from)
            .and_then(|xml| opml::parse(&xml))
            .and_then(|feeds| opml::import(&self.store, &feeds));
        let Some(summary) = self.report(result, cx) else { return };
        self.reload(cx);
        let message = match (summary.added, summary.existing) {
            (0, 0) => "That file has no feeds in it.".to_owned(),
            (0, _) => "You already follow every feed in that file.".to_owned(),
            (1, _) => "Imported 1 feed.".to_owned(),
            (added, _) => format!("Imported {added} feeds."),
        };
        cx.emit(Notice::Info(message.into()));
        let fresh: Vec<Feed> =
            self.feeds.iter().filter(|feed| feed.last_fetched_at.is_none()).cloned().collect();
        self.refresh(fresh, cx);
    }

    /// Writes subscriptions to an OPML file.
    pub fn export_opml(&mut self, path: &Path, cx: &mut Context<Self>) {
        let xml = opml::export(&self.folders, &self.feeds);
        if self.report(std::fs::write(path, xml).map_err(brook_core::Error::from), cx).is_some() {
            cx.emit(Notice::Info(format!("Exported {} feeds.", self.feeds.len()).into()));
        }
    }

    // Settings --------------------------------------------------------------

    /// Changes settings and saves them.
    pub fn update_settings(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let before = self.settings.unread_only;
        change(&mut self.settings);
        self.report(self.settings.save(&self.store), cx);
        if self.settings.unread_only != before {
            self.reload_articles(cx);
        }
        cx.notify();
    }

    /// A raw stored value, for state that isn't a preference.
    pub fn state(&self, key: &str) -> Option<String> {
        self.store.setting(key).ok().flatten()
    }

    /// Stores a raw value.
    pub fn set_state(&self, key: &str, value: &str) {
        if let Err(error) = self.store.set_setting(key, value) {
            log::warn!("saving {key} failed: {error}");
        }
    }

    // Refreshing ------------------------------------------------------------

    /// Refreshes every feed.
    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        let feeds = self.feeds.clone();
        self.refresh(feeds, cx);
    }

    /// Refreshes feeds whose interval has passed.
    pub fn refresh_due(&mut self, cx: &mut Context<Self>) {
        if self.settings.refresh_minutes == 0 {
            return;
        }
        let every = chrono::Duration::minutes(i64::from(self.settings.refresh_minutes));
        let now = Utc::now();
        let due: Vec<Feed> =
            self.feeds.iter().filter(|feed| refresh::is_due(feed, now, every)).cloned().collect();
        self.refresh(due, cx);
    }

    /// Fetches `feeds` in the background. Does nothing while a refresh runs.
    pub fn refresh(&mut self, feeds: Vec<Feed>, cx: &mut Context<Self>) {
        if feeds.is_empty() || self.progress.is_some() {
            return;
        }
        self.progress = Some(Progress { done: 0, total: feeds.len() });
        cx.notify();

        let (sender, mut receiver) = mpsc::unbounded();
        let db_path = self.db_path.clone();
        let client = self.client.clone();
        let work = cx.background_spawn(async move {
            let mut store = Store::open(&db_path)?;
            Ok::<_, brook_core::Error>(refresh::refresh(&mut store, &client, &feeds, |outcome| {
                let _ = sender.unbounded_send(outcome.clone());
            }))
        });
        cx.spawn(async move |this, cx| {
            while let Some(_outcome) = receiver.next().await {
                let updated = this.update(cx, |this, cx| {
                    if let Some(progress) = &mut this.progress {
                        progress.done += 1;
                    }
                    this.reload_counts(cx);
                });
                if updated.is_err() {
                    return;
                }
            }
            let result = work.await;
            this.update(cx, |this, cx| {
                this.progress = None;
                if let Err(error) = result {
                    cx.emit(Notice::Error(format!("Refresh failed: {error}").into()));
                }
                this.reload_quietly_keeping_list(cx);
            })
            .ok();
        })
        .detach();
    }

    /// After a refresh: new articles appear, but a list someone is reading
    /// doesn't jump. Rows already shown keep their place; new ones join at
    /// the top.
    fn reload_quietly_keeping_list(&mut self, cx: &mut Context<Self>) {
        let (folders, feeds) = (self.store.folders(), self.store.feeds());
        if let (Ok(folders), Ok(feeds)) = (folders, feeds) {
            self.folders = folders;
            self.feeds = feeds;
        }
        self.reload_counts(cx);
        self.reload_articles(cx);
    }
}

#[cfg(test)]
mod tests {
    use brook_core::parse::ParsedEntry;
    use gpui::{Entity, TestAppContext};

    use super::*;

    fn library(cx: &mut TestAppContext) -> (Entity<Library>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("brook.db");
        let store = Store::open(&path).unwrap();
        let library = cx.new(|cx| Library::new(store, &path, cx));
        (library, dir)
    }

    fn seed(library: &Entity<Library>, cx: &mut TestAppContext, titles: &[&str]) -> FeedId {
        library.update(cx, |library, cx| {
            let id = library
                .store
                .add_feed(&NewFeed {
                    url: "https://a.test/rss".into(),
                    title: "A".into(),
                    ..NewFeed::default()
                })
                .unwrap();
            let entries: Vec<ParsedEntry> = titles
                .iter()
                .enumerate()
                .map(|(i, title)| ParsedEntry {
                    guid: title.to_string(),
                    title: title.to_string(),
                    url: None,
                    author: None,
                    excerpt: String::new(),
                    content_html: Some(format!("<p>{title}</p>")),
                    image_url: None,
                    published_at: Some(Utc::now() - chrono::Duration::minutes(i as i64)),
                })
                .collect();
            library.store.upsert_entries(id, &entries, Utc::now()).unwrap();
            library.reload(cx);
            id
        })
    }

    #[gpui::test]
    fn opening_marks_read_and_steps(cx: &mut TestAppContext) {
        let (library, _dir) = library(cx);
        seed(&library, cx, &["one", "two", "three"]);
        library.update(cx, |library, cx| {
            assert_eq!(library.counts.all, 3);
            library.step(1, cx);
            assert_eq!(library.selected_index(), Some(0), "the first step opens the top article");
            library.step(1, cx);
            library.step(1, cx);
            library.step(1, cx);
            assert_eq!(library.selected_index(), Some(2), "stepping stops at the end");
            assert_eq!(library.counts.all, 0);
            assert!(library.articles.iter().all(|a| a.read));
            library.step(-5, cx);
            assert_eq!(library.selected_index(), Some(2), "out-of-range steps are ignored");
        });
    }

    #[gpui::test]
    fn toggles_update_the_list_and_counts(cx: &mut TestAppContext) {
        let (library, _dir) = library(cx);
        seed(&library, cx, &["one"]);
        library.update(cx, |library, cx| {
            let id = library.articles[0].id;
            library.toggle_starred(id, cx);
            library.toggle_saved(id, cx);
            library.toggle_read(id, cx);
            let row = &library.articles[0];
            assert!(row.starred && row.saved && row.read);
            assert_eq!((library.counts.starred, library.counts.saved, library.counts.all), (1, 0, 0));
            library.toggle_read(id, cx);
            assert_eq!(library.counts.saved, 1);
        });
    }

    #[gpui::test]
    fn scopes_search_and_unread_only(cx: &mut TestAppContext) {
        let (library, _dir) = library(cx);
        let feed = seed(&library, cx, &["apples", "pears"]);
        library.update(cx, |library, cx| {
            library.select_scope(Scope::Feed(feed), cx);
            assert_eq!(library.scope_title(), "A");
            assert_eq!(library.scope_unread(), 2);

            library.set_search("pea", cx);
            assert_eq!(library.articles.len(), 1);
            library.set_search("", cx);

            let first = library.articles[0].id;
            library.toggle_read(first, cx);
            library.update_settings(|s| s.unread_only = true, cx);
            assert_eq!(library.articles.len(), 1);

            library.mark_all_read(Scope::Feed(feed), cx);
            assert!(library.articles.is_empty());
            assert_eq!(library.scope_unread(), 0);
        });
    }

    #[gpui::test]
    fn unsubscribing_clears_the_scope(cx: &mut TestAppContext) {
        let (library, _dir) = library(cx);
        let feed = seed(&library, cx, &["one"]);
        library.update(cx, |library, cx| {
            library.select_scope(Scope::Feed(feed), cx);
            library.step(1, cx);
            library.delete_feed(feed, cx);
            assert_eq!(library.scope, Scope::All);
            assert!(library.open.is_none());
            assert!(library.feeds.is_empty() && library.articles.is_empty());
        });
    }

    #[gpui::test]
    fn folders(cx: &mut TestAppContext) {
        let (library, _dir) = library(cx);
        let feed = seed(&library, cx, &["one"]);
        library.update(cx, |library, cx| {
            let folder = library.create_folder("News", cx).unwrap();
            library.move_feed(feed, Some(folder), cx);
            assert_eq!(library.feeds_in(Some(folder)).count(), 1);
            library.toggle_folder(folder, cx);
            assert!(!library.folders[0].expanded);
            library.select_scope(Scope::Folder(folder), cx);
            assert_eq!(library.scope_title(), "News");
            library.delete_folder(folder, cx);
            assert_eq!(library.scope, Scope::All);
            assert_eq!(library.feeds_in(None).count(), 1);
            assert_eq!(library.create_folder("  ", cx), None);
        });
    }

    #[gpui::test]
    fn opml_round_trip(cx: &mut TestAppContext) {
        let (library, dir) = library(cx);
        seed(&library, cx, &["one"]);
        let path = dir.path().join("subs.opml");
        library.update(cx, |library, cx| {
            library.export_opml(&path, cx);
            library.delete_feed(library.feeds[0].id, cx);
            // Importing kicks off a refresh of the restored feed; it fails
            // offline, which is fine here.
            library.import_opml(&path, cx);
            assert_eq!(library.feeds.len(), 1);
            assert_eq!(library.feeds[0].url, "https://a.test/rss");
        });
    }
}
