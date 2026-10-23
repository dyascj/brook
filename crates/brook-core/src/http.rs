//! A small HTTP client for feeds, web pages, and images.
//!
//! Wraps a shared `ureq` agent with Brook's user agent, timeouts, a size
//! cap, and conditional requests. Responses are read fully into memory;
//! nothing Brook downloads is large.

use std::time::Duration;

use ureq::ResponseExt;

use crate::error::{Error, Result};
use crate::store::HttpCache;

/// The `User-Agent` Brook sends. Some sites refuse requests without one.
pub const USER_AGENT: &str =
    concat!("Brook/", env!("CARGO_PKG_VERSION"), " (+https://github.com/dyascj/brook)");

/// What feed requests accept, most specific first.
pub const ACCEPT_FEED: &str = "application/rss+xml, application/atom+xml, application/feed+json, \
     application/xml;q=0.9, text/xml;q=0.9, application/json;q=0.8, */*;q=0.5";

/// What page requests accept.
pub const ACCEPT_HTML: &str = "text/html, application/xhtml+xml;q=0.9, */*;q=0.5";

/// The largest feed, page, or image Brook reads.
const MAX_BODY_BYTES: u64 = 25 * 1024 * 1024;

/// The largest download, for app updates.
const MAX_DOWNLOAD_BYTES: u64 = 500 * 1024 * 1024;

/// A fetched document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// The address after redirects.
    pub url: String,
    /// The `Content-Type` header's media type, lowercased, without parameters.
    pub content_type: Option<String>,
    /// The raw body.
    pub body: Vec<u8>,
    /// Validators to send next time.
    pub cache: HttpCache,
}

impl Page {
    /// Whether the server says this is HTML.
    pub fn is_html(&self) -> bool {
        self.content_type.as_deref().is_some_and(|t| t == "text/html" || t == "application/xhtml+xml")
    }

    /// The body as text. Invalid UTF-8 is replaced rather than rejected.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// The result of a conditional request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetched {
    /// The server answered `304 Not Modified`.
    NotModified,
    /// A fresh copy.
    Page(Page),
}

/// A cloneable, thread-safe HTTP client.
#[derive(Clone, Debug)]
pub struct Client {
    agent: ureq::Agent,
    max_body: u64,
}

impl Default for Client {
    fn default() -> Self {
        Self::new(Duration::from_secs(20))
    }
}

impl Client {
    /// A client whose requests give up after `timeout`, for feeds, pages,
    /// and images.
    pub fn new(timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_global(Some(timeout))
            .max_redirects(10)
            .build()
            .into();
        Self { agent, max_body: MAX_BODY_BYTES }
    }

    /// A client for large downloads: no limit on the whole transfer, so a
    /// slow connection still finishes, but it gives up when the connection
    /// stalls for `stall`.
    pub fn for_downloads(stall: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_connect(Some(stall))
            .timeout_recv_response(Some(stall))
            .timeout_per_read(Some(stall))
            .max_redirects(10)
            .build()
            .into();
        Self { agent, max_body: MAX_DOWNLOAD_BYTES }
    }

    /// Fetches `url` unconditionally.
    pub fn get(&self, url: &str, accept: &str) -> Result<Page> {
        match self.get_if_changed(url, accept, &HttpCache::default())? {
            Fetched::Page(page) => Ok(page),
            Fetched::NotModified => Err(Error::Status { status: 304 }),
        }
    }

    /// Fetches `url`, sending `cache`'s validators so an unchanged document
    /// comes back as [`Fetched::NotModified`].
    pub fn get_if_changed(&self, url: &str, accept: &str, cache: &HttpCache) -> Result<Fetched> {
        let parsed = url::Url::parse(url).map_err(|_| Error::InvalidUrl(url.to_owned()))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(Error::InvalidUrl(url.to_owned()));
        }
        let mut request = self.agent.get(url).header("Accept", accept);
        if let Some(etag) = &cache.etag {
            request = request.header("If-None-Match", etag);
        }
        if let Some(modified) = &cache.last_modified {
            request = request.header("If-Modified-Since", modified);
        }
        let mut response = request.call().map_err(Error::from)?;
        if response.status() == 304 {
            return Ok(Fetched::NotModified);
        }
        let header = |name: &str| {
            response.headers().get(name).and_then(|value| value.to_str().ok()).map(str::to_owned)
        };
        let cache = HttpCache { etag: header("etag"), last_modified: header("last-modified") };
        let content_type = header("content-type")
            .and_then(|value| value.split(';').next().map(|t| t.trim().to_ascii_lowercase()))
            .filter(|t| !t.is_empty());
        let final_url = response.get_uri().to_string();
        let body = response
            .body_mut()
            .with_config()
            .limit(self.max_body)
            .read_to_vec()
            .map_err(|error| Error::Network(error.to_string()))?;
        Ok(Fetched::Page(Page { url: final_url, content_type, body, cache }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Route, TestServer};

    #[test]
    fn fetches_with_validators_and_content_type() {
        let server = TestServer::start(vec![
            Route::new("/feed", "<rss/>")
                .header("Content-Type", "application/rss+xml; charset=utf-8")
                .header("ETag", "\"v1\"")
                .header("Last-Modified", "Mon, 02 Jun 2025 10:00:00 GMT"),
        ]);
        let client = Client::default();
        let page = client.get(&server.url("/feed"), ACCEPT_FEED).unwrap();
        assert_eq!(page.text(), "<rss/>");
        assert_eq!(page.content_type.as_deref(), Some("application/rss+xml"));
        assert_eq!(page.cache.etag.as_deref(), Some("\"v1\""));
        assert_eq!(page.cache.last_modified.as_deref(), Some("Mon, 02 Jun 2025 10:00:00 GMT"));
        assert!(!page.is_html());

        let again = client.get_if_changed(&server.url("/feed"), ACCEPT_FEED, &page.cache).unwrap();
        assert_eq!(again, Fetched::NotModified);
    }

    #[test]
    fn follows_redirects_and_reports_the_final_url() {
        let server = TestServer::start(vec![
            Route::new("/old", "").status(301).header("Location", "/new"),
            Route::new("/new", "<html></html>").header("Content-Type", "text/html"),
        ]);
        let page = Client::default().get(&server.url("/old"), ACCEPT_HTML).unwrap();
        assert_eq!(page.url, server.url("/new"));
        assert!(page.is_html());
    }

    #[test]
    fn download_clients_fetch_too() {
        let server = TestServer::start(vec![Route::new("/big", vec![7u8; 64 * 1024])]);
        let page = Client::for_downloads(Duration::from_secs(5)).get(&server.url("/big"), "*/*").unwrap();
        assert_eq!(page.body.len(), 64 * 1024);
    }

    #[test]
    fn errors_are_typed() {
        let server = TestServer::start(vec![]);
        let client = Client::default();
        assert!(matches!(
            client.get(&server.url("/missing"), ACCEPT_HTML),
            Err(Error::Status { status: 404 })
        ));
        assert!(matches!(client.get("ftp://example.com/", ACCEPT_HTML), Err(Error::InvalidUrl(_))));
        assert!(matches!(client.get("not a url", ACCEPT_HTML), Err(Error::InvalidUrl(_))));
        // Nothing listens on port 9 (discard) in a test sandbox.
        assert!(matches!(client.get("http://127.0.0.1:9/", ACCEPT_HTML), Err(Error::Network(_))));
    }
}
