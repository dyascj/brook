//! Fetching feeds and storing what's new.
//!
//! Network work runs on a few threads at once; every database write goes
//! through the caller's [`Store`] on the caller's thread, one feed at a time.

use std::sync::Mutex;
use std::sync::mpsc;
use std::thread;

use chrono::{DateTime, Duration, Utc};

use crate::error::Result;
use crate::http::{ACCEPT_FEED, Client, Fetched};
use crate::model::{Feed, FeedId};
use crate::parse::{ParsedFeed, parse_feed};
use crate::store::{HttpCache, Store};

/// How many feeds are fetched at once.
pub const PARALLEL_FETCHES: usize = 6;

/// What happened to one feed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// The feed.
    pub feed: FeedId,
    /// New articles stored, or the error shown next to the feed.
    pub result: Result<usize, String>,
}

/// Whether `feed` should be fetched now, given how often feeds refresh.
pub fn is_due(feed: &Feed, now: DateTime<Utc>, every: Duration) -> bool {
    feed.last_fetched_at.is_none_or(|last| now - last >= every)
}

/// A fetched feed, not yet stored.
#[derive(Debug)]
enum Fetch {
    Unchanged,
    Fresh { feed: Box<ParsedFeed>, cache: HttpCache },
}

fn fetch(client: &Client, url: &str, cache: &HttpCache) -> Result<Fetch> {
    match client.get_if_changed(url, ACCEPT_FEED, cache)? {
        Fetched::NotModified => Ok(Fetch::Unchanged),
        Fetched::Page(page) => {
            let feed = parse_feed(&page.body, &page.url)?;
            Ok(Fetch::Fresh { feed: Box::new(feed), cache: page.cache })
        }
    }
}

/// Stores a fetch result and returns the outcome.
fn store_result(store: &mut Store, feed: FeedId, fetched: Result<Fetch>, now: DateTime<Utc>) -> Outcome {
    let result = match fetched {
        Ok(Fetch::Unchanged) => store.http_cache(feed).and_then(|cache| {
            store.record_fetch_success(feed, &cache, None, now)?;
            Ok(0)
        }),
        Ok(Fetch::Fresh { feed: parsed, cache }) => {
            store.upsert_entries(feed, &parsed.entries, now).and_then(|new| {
                store.record_fetch_success(feed, &cache, parsed.site_url.as_deref(), now)?;
                Ok(new)
            })
        }
        Err(error) => {
            let message = error.to_string();
            // If even the error can't be stored the database is gone; the
            // outcome still carries the original message.
            let _ = store.record_fetch_error(feed, &message, now);
            Err(error)
        }
    };
    Outcome { feed, result: result.map_err(|error| error.to_string()) }
}

/// Refreshes `feeds`, calling `on_outcome` as each one finishes, and
/// returns every outcome in completion order.
///
/// Up to [`PARALLEL_FETCHES`] requests run at once. Unchanged feeds cost a
/// `304`; their entries aren't touched.
pub fn refresh(
    store: &mut Store,
    client: &Client,
    feeds: &[Feed],
    mut on_outcome: impl FnMut(&Outcome),
) -> Vec<Outcome> {
    // Validators are read up front: the store isn't shared with the workers.
    let jobs: Vec<(FeedId, String, HttpCache)> = feeds
        .iter()
        .map(|feed| (feed.id, feed.url.clone(), store.http_cache(feed.id).unwrap_or_default()))
        .collect();
    let queue = Mutex::new(jobs.into_iter());
    let (sender, receiver) = mpsc::channel();

    thread::scope(|scope| {
        for _ in 0..PARALLEL_FETCHES.min(feeds.len()) {
            let sender = sender.clone();
            let queue = &queue;
            scope.spawn(move || {
                loop {
                    let next = queue.lock().map(|mut jobs| jobs.next()).ok().flatten();
                    let Some((id, url, cache)) = next else { break };
                    if sender.send((id, fetch(client, &url, &cache))).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);

        receiver
            .into_iter()
            .map(|(id, fetched)| {
                let outcome = store_result(store, id, fetched, Utc::now());
                on_outcome(&outcome);
                outcome
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Scope;
    use crate::store::{ArticleQuery, NewFeed};
    use crate::testing::{Route, TestServer};

    fn rss(items: &[&str]) -> String {
        let items: String = items
            .iter()
            .map(|id| {
                format!("<item><title>{id}</title><guid>{id}</guid><link>https://x.test/{id}</link></item>")
            })
            .collect();
        format!(
            r#"<rss version="2.0"><channel><title>T</title><link>https://x.test/</link>{items}</channel></rss>"#
        )
    }

    fn subscribe(store: &Store, url: &str) -> Feed {
        let id =
            store.add_feed(&NewFeed { url: url.into(), title: "T".into(), ..Default::default() }).unwrap();
        store.feed(id).unwrap()
    }

    #[test]
    fn due_feeds() {
        let now = DateTime::from_timestamp(10_000, 0).unwrap();
        let mut feed = Feed {
            id: FeedId(1),
            url: String::new(),
            title: String::new(),
            site_url: None,
            description: None,
            folder_id: None,
            position: 0,
            last_fetched_at: None,
            last_error: None,
        };
        assert!(is_due(&feed, now, Duration::minutes(30)));
        feed.last_fetched_at = Some(now - Duration::minutes(10));
        assert!(!is_due(&feed, now, Duration::minutes(30)));
        feed.last_fetched_at = Some(now - Duration::minutes(30));
        assert!(is_due(&feed, now, Duration::minutes(30)));
    }

    #[test]
    fn refreshes_stores_and_reports() {
        let server = TestServer::start(vec![
            Route::new("/a", rss(&["1", "2"])).header("ETag", "\"a1\""),
            Route::new("/b", rss(&["3"])),
            Route::new("/bad", "<html>not a feed</html>"),
        ]);
        let mut store = Store::open_in_memory().unwrap();
        let feeds = vec![
            subscribe(&store, &server.url("/a")),
            subscribe(&store, &server.url("/b")),
            subscribe(&store, &server.url("/bad")),
            subscribe(&store, &server.url("/gone")),
        ];
        let mut seen = 0;
        let outcomes = refresh(&mut store, &Client::default(), &feeds, |_| seen += 1);
        assert_eq!(seen, 4);

        let result = |id: FeedId| outcomes.iter().find(|o| o.feed == id).unwrap().result.clone();
        assert_eq!(result(feeds[0].id), Ok(2));
        assert_eq!(result(feeds[1].id), Ok(1));
        assert!(result(feeds[2].id).unwrap_err().starts_with("not a feed"));
        assert_eq!(result(feeds[3].id), Err("the server answered 404".into()));

        let stored = store.articles(&ArticleQuery::new(Scope::All, Utc::now())).unwrap();
        assert_eq!(stored.len(), 3);
        assert_eq!(store.feed(feeds[3].id).unwrap().last_error.as_deref(), Some("the server answered 404"));
        assert_eq!(store.feed(feeds[0].id).unwrap().site_url.as_deref(), Some("https://x.test/"));
    }

    #[test]
    fn unchanged_feeds_cost_a_304_and_clear_errors() {
        let server = TestServer::start(vec![Route::new("/a", rss(&["1"])).header("ETag", "\"a1\"")]);
        let mut store = Store::open_in_memory().unwrap();
        let feed = subscribe(&store, &server.url("/a"));
        let client = Client::default();

        assert_eq!(refresh(&mut store, &client, std::slice::from_ref(&feed), |_| {})[0].result, Ok(1));
        store.record_fetch_error(feed.id, "earlier failure", Utc::now()).unwrap();
        assert_eq!(refresh(&mut store, &client, std::slice::from_ref(&feed), |_| {})[0].result, Ok(0));
        assert_eq!(store.feed(feed.id).unwrap().last_error, None);
        assert_eq!(server.hits(), 2);
    }

    #[test]
    fn nothing_to_do() {
        let mut store = Store::open_in_memory().unwrap();
        assert!(refresh(&mut store, &Client::default(), &[], |_| {}).is_empty());
    }
}
