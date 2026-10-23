//! Finding the feed behind whatever someone pastes.
//!
//! People paste a feed address, a blog's home page, a YouTube channel, or
//! just `example.com`. Discovery tries, in order:
//!
//! 1. the address itself, if it is a feed;
//! 2. the feeds a web page advertises with `<link rel="alternate">` (this
//!    also covers YouTube channels, which link their own feed);
//! 3. the usual feed paths on the site (`/feed`, `/rss.xml`, ...).
//!
//! Every candidate is fetched and parsed, so the caller gets a preview of
//! each feed's recent entries, not a guess.

use std::thread;

use scraper::{Html, Selector};
use url::Url;

use crate::error::{Error, Result};
use crate::http::{ACCEPT_FEED, Client, Page};
use crate::parse::{ParsedFeed, parse_feed};

/// Paths tried when a page advertises no feed.
const COMMON_PATHS: &[&str] =
    &["/feed", "/rss", "/feed.xml", "/rss.xml", "/atom.xml", "/index.xml", "/feed.json", "/blog/feed"];

/// The most advertised feeds checked for one page.
const MAX_ADVERTISED: usize = 6;

/// A feed found by [`discover`], parsed and ready to preview.
#[derive(Clone, Debug, PartialEq)]
pub struct Discovered {
    /// The feed's address, after redirects.
    pub url: String,
    /// The parsed feed.
    pub feed: ParsedFeed,
}

/// Turns typed input into an absolute web address: trims it, and assumes
/// `https://` when no scheme is given.
pub fn normalize_input(input: &str) -> Result<Url> {
    let input = input.trim();
    if input.is_empty() {
        return Err(Error::InvalidUrl(String::new()));
    }
    let with_scheme = if input.contains("://") { input.to_owned() } else { format!("https://{input}") };
    let url = Url::parse(&with_scheme).map_err(|_| Error::InvalidUrl(input.to_owned()))?;
    if !matches!(url.scheme(), "http" | "https")
        || url
            .host_str()
            .is_none_or(|h| !h.contains('.') && h != "localhost" && h.parse::<std::net::IpAddr>().is_err())
    {
        return Err(Error::InvalidUrl(input.to_owned()));
    }
    Ok(url)
}

/// Finds the feeds for `input`. Returns at least one feed or an error.
pub fn discover(client: &Client, input: &str) -> Result<Vec<Discovered>> {
    let url = normalize_input(input)?;
    let page = client.get(url.as_str(), ACCEPT_FEED)?;

    if let Some(found) = as_feed(&page) {
        return Ok(vec![found]);
    }

    let base = Url::parse(&page.url).unwrap_or(url);
    let advertised: Vec<String> = feed_links(&page.text(), &base).into_iter().take(MAX_ADVERTISED).collect();
    let found = fetch_all(client, &advertised);
    if !found.is_empty() {
        return Ok(found);
    }

    let guesses: Vec<String> =
        COMMON_PATHS.iter().filter_map(|path| base.join(path).ok()).map(String::from).collect();
    // Many sites answer several common paths with the same feed.
    let mut found = fetch_all(client, &guesses);
    found.dedup_by(|a, b| a.url == b.url);
    found.truncate(1);
    if found.is_empty() { Err(Error::NoFeedFound(input.trim().to_owned())) } else { Ok(found) }
}

/// Fetches and parses each address in parallel, keeping the order of
/// `urls` and dropping the ones that are not feeds.
fn fetch_all(client: &Client, urls: &[String]) -> Vec<Discovered> {
    thread::scope(|scope| {
        let handles: Vec<_> = urls
            .iter()
            .map(|url| scope.spawn(move || client.get(url, ACCEPT_FEED).ok().and_then(|page| as_feed(&page))))
            .collect();
        handles.into_iter().filter_map(|handle| handle.join().ok().flatten()).collect()
    })
}

/// The page as a feed, if it is one with at least a title.
fn as_feed(page: &Page) -> Option<Discovered> {
    if page.is_html() {
        return None;
    }
    let feed = parse_feed(&page.body, &page.url).ok()?;
    Some(Discovered { url: page.url.clone(), feed })
}

/// The feed addresses an HTML page advertises, resolved against `base`,
/// without duplicates, in page order.
pub fn feed_links(html: &str, base: &Url) -> Vec<String> {
    let document = Html::parse_document(html);
    let selector =
        Selector::parse("link[rel~=alternate][href], link[rel~=feed][href]").expect("static selector");
    let mut links: Vec<String> = Vec::new();
    for link in document.select(&selector) {
        let element = link.value();
        let kind = element.attr("type").unwrap_or_default().to_ascii_lowercase();
        let is_feed = kind.contains("rss")
            || kind.contains("atom")
            || kind.contains("feed+json")
            || (kind.is_empty() && element.attr("rel").is_some_and(|rel| rel.contains("feed")));
        if !is_feed {
            continue;
        }
        let Some(url) = element.attr("href").and_then(|href| base.join(href.trim()).ok()) else { continue };
        let url = String::from(url);
        // Comment feeds are rarely what anyone wants.
        if url.contains("/comments/feed") || links.contains(&url) {
            continue;
        }
        links.push(url);
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Route, TestServer};

    const RSS: &str = r#"<rss version="2.0"><channel><title>Blog</title><link>https://blog.test/</link>
        <item><title>Hello</title><link>https://blog.test/hello</link><guid>1</guid></item></channel></rss>"#;
    const ATOM: &str =
        r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Atom Blog</title><id>x</id></feed>"#;

    fn html(head: &str) -> String {
        format!("<!doctype html><html><head>{head}</head><body>hi</body></html>")
    }

    #[test]
    fn normalizes_typed_addresses() {
        assert_eq!(normalize_input("  example.com ").unwrap().as_str(), "https://example.com/");
        assert_eq!(normalize_input("http://a.test/x").unwrap().as_str(), "http://a.test/x");
        assert_eq!(normalize_input("http://127.0.0.1:8080/").unwrap().as_str(), "http://127.0.0.1:8080/");
        assert!(normalize_input("").is_err());
        assert!(normalize_input("hello").is_err());
        assert!(normalize_input("ftp://a.test/").is_err());
        assert!(normalize_input("https://").is_err());
    }

    #[test]
    fn reads_advertised_links() {
        let base = Url::parse("https://site.test/blog/").unwrap();
        let page = html(
            r#"<link rel="alternate" type="application/rss+xml" href="/feed.xml">
               <link rel="alternate" type="application/atom+xml" href="atom.xml">
               <link rel="alternate" type="application/feed+json" href="https://cdn.test/f.json">
               <link rel="alternate" type="application/rss+xml" href="/comments/feed">
               <link rel="alternate" type="application/rss+xml" href="/feed.xml">
               <link rel="alternate" hreflang="fr" href="/fr/">
               <link rel="stylesheet" href="/s.css">"#,
        );
        assert_eq!(
            feed_links(&page, &base),
            ["https://site.test/feed.xml", "https://site.test/blog/atom.xml", "https://cdn.test/f.json"]
        );
    }

    #[test]
    fn a_feed_address_is_its_own_feed() {
        let server =
            TestServer::start(vec![Route::new("/rss", RSS).header("Content-Type", "application/rss+xml")]);
        let found = discover(&Client::default(), &server.url("/rss")).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].url, server.url("/rss"));
        assert_eq!(found[0].feed.title, "Blog");
        assert_eq!(found[0].feed.entries.len(), 1);
    }

    #[test]
    fn pages_lead_to_their_advertised_feeds() {
        let page = html(
            r#"<link rel="alternate" type="application/rss+xml" href="/rss">
               <link rel="alternate" type="application/atom+xml" href="/broken">
               <link rel="alternate" type="application/atom+xml" href="/atom">"#,
        );
        let server = TestServer::start(vec![
            Route::new("/", page).header("Content-Type", "text/html; charset=utf-8"),
            Route::new("/rss", RSS),
            Route::new("/broken", "<html>nope</html>").header("Content-Type", "text/html"),
            Route::new("/atom", ATOM),
        ]);
        let found = discover(&Client::default(), &server.url("/")).unwrap();
        let titles: Vec<_> = found.iter().map(|d| d.feed.title.as_str()).collect();
        assert_eq!(titles, ["Blog", "Atom Blog"]);
    }

    #[test]
    fn falls_back_to_common_paths() {
        let server = TestServer::start(vec![
            Route::new("/", html("")).header("Content-Type", "text/html"),
            Route::new("/rss.xml", RSS),
            Route::new("/atom.xml", ATOM),
        ]);
        let found = discover(&Client::default(), &server.url("/")).unwrap();
        assert_eq!(found.len(), 1, "only the first common path that works");
        assert_eq!(found[0].url, server.url("/rss.xml"));
    }

    #[test]
    fn reports_when_nothing_is_found() {
        let server = TestServer::start(vec![Route::new("/", html("")).header("Content-Type", "text/html")]);
        let error = discover(&Client::default(), &server.url("/")).unwrap_err();
        assert!(matches!(error, Error::NoFeedFound(_)), "{error}");
    }
}
