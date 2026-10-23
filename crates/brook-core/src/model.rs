//! Plain data types shared by the store, the refresher, and the UI.
//!
//! Nothing in here touches the database or the network. Timestamps are UTC.

use chrono::{DateTime, Utc};

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub i64);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_type!(
    /// Primary key of a [`Feed`].
    FeedId
);
id_type!(
    /// Primary key of a [`Folder`].
    FolderId
);
id_type!(
    /// Primary key of an [`Article`].
    ArticleId
);

/// A named group of feeds in the sidebar. Folders do not nest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// Primary key.
    pub id: FolderId,
    /// Display name, unique ignoring case.
    pub name: String,
    /// Sort position among folders, ascending.
    pub position: i64,
    /// Whether the sidebar shows the folder's feeds.
    pub expanded: bool,
}

/// A subscription.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feed {
    /// Primary key.
    pub id: FeedId,
    /// The feed document's address. Unique.
    pub url: String,
    /// Display title. Taken from the feed unless the user renamed it.
    pub title: String,
    /// The website the feed belongs to, if the feed says.
    pub site_url: Option<String>,
    /// The feed's own description.
    pub description: Option<String>,
    /// The folder the feed sits in, or `None` for the top level.
    pub folder_id: Option<FolderId>,
    /// Sort position within its folder, ascending.
    pub position: i64,
    /// When the feed was last fetched, successfully or not.
    pub last_fetched_at: Option<DateTime<Utc>>,
    /// The last fetch's error, cleared by the next success.
    pub last_error: Option<String>,
}

impl Feed {
    /// The host name to show for this feed and to look up its icon by,
    /// preferring the website over the feed address.
    pub fn host(&self) -> Option<String> {
        let source = self.site_url.as_deref().unwrap_or(&self.url);
        url::Url::parse(source).ok()?.host_str().map(|host| host.trim_start_matches("www.").to_owned())
    }
}

/// An article as shown in a list: everything but the body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArticleSummary {
    /// Primary key.
    pub id: ArticleId,
    /// The feed the article came from.
    pub feed_id: FeedId,
    /// Headline.
    pub title: String,
    /// A plain-text excerpt, at most a few sentences.
    pub excerpt: String,
    /// The lead image, if the feed names one.
    pub image_url: Option<String>,
    /// Publication time, or the time it was first seen if the feed omits it.
    pub published_at: DateTime<Utc>,
    /// Whether it has been read.
    pub read: bool,
    /// Whether it is starred.
    pub starred: bool,
    /// Whether it is saved for later.
    pub saved: bool,
}

/// A full article.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Article {
    /// The list fields.
    pub summary: ArticleSummary,
    /// The original article on the web.
    pub url: Option<String>,
    /// Byline.
    pub author: Option<String>,
    /// The body as HTML, as the feed delivered it.
    pub content_html: Option<String>,
    /// The body extracted from the web page by reader mode, as HTML.
    pub reader_html: Option<String>,
}

/// Which slice of articles a list shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Scope {
    /// Everything, newest first.
    #[default]
    All,
    /// Articles published since local midnight.
    Today,
    /// Starred articles.
    Starred,
    /// Articles saved to read later.
    Saved,
    /// Every feed in a folder.
    Folder(FolderId),
    /// A single feed.
    Feed(FeedId),
}

/// Unread counts for the sidebar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Unread articles across all feeds.
    pub all: u32,
    /// Unread articles published today.
    pub today: u32,
    /// Starred articles, read or not.
    pub starred: u32,
    /// Saved articles that are still unread.
    pub saved: u32,
    /// Unread articles per feed. Feeds with none are absent.
    pub feeds: std::collections::HashMap<FeedId, u32>,
}

impl Counts {
    /// Unread articles in one feed.
    pub fn feed(&self, id: FeedId) -> u32 {
        self.feeds.get(&id).copied().unwrap_or(0)
    }

    /// Unread articles across the given feeds.
    pub fn sum<'a>(&self, feeds: impl IntoIterator<Item = &'a Feed>) -> u32 {
        feeds.into_iter().map(|feed| self.feed(feed.id)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(url: &str, site: Option<&str>) -> Feed {
        Feed {
            id: FeedId(1),
            url: url.into(),
            title: "t".into(),
            site_url: site.map(Into::into),
            description: None,
            folder_id: None,
            position: 0,
            last_fetched_at: None,
            last_error: None,
        }
    }

    #[test]
    fn host_prefers_the_site_and_drops_www() {
        let f = feed("https://feeds.example.com/rss", Some("https://www.example.com/blog"));
        assert_eq!(f.host().as_deref(), Some("example.com"));
        let f = feed("https://feeds.example.com/rss", None);
        assert_eq!(f.host().as_deref(), Some("feeds.example.com"));
        let f = feed("not a url", None);
        assert_eq!(f.host(), None);
    }

    #[test]
    fn counts_sum_over_feeds() {
        let mut counts = Counts::default();
        counts.feeds.insert(FeedId(1), 3);
        let a = feed("a", None);
        let mut b = feed("b", None);
        b.id = FeedId(2);
        assert_eq!(counts.sum([&a, &b]), 3);
        assert_eq!(counts.feed(FeedId(9)), 0);
    }
}
