//! Article images and site icons, downloaded once and kept on disk.
//!
//! Files are named by a stable hash of their address (images) or by host
//! (icons). Writes go to a temporary file first and are renamed into place,
//! so a reader never sees half an image. Formats are left to the UI, which
//! sniffs the bytes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use scraper::{Html, Selector};
use url::Url;

use crate::error::{Error, Result};
use crate::http::{ACCEPT_HTML, Client};

const ACCEPT_IMAGE: &str = "image/avif, image/webp, image/png, image/svg+xml, image/*;q=0.8, */*;q=0.5";

/// How long a site without an icon waits before Brook asks again.
const MISSING_ICON_RETRY: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// A directory of downloaded images.
#[derive(Clone, Debug)]
pub struct DiskCache {
    dir: PathBuf,
}

impl DiskCache {
    /// A cache rooted at `dir`. Nothing is created until the first write.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Where the image at `url` is, or would be, stored.
    pub fn image_path(&self, url: &str) -> PathBuf {
        self.dir.join("images").join(format!("{:016x}", fnv1a(url.as_bytes())))
    }

    /// The stored image for `url`, if it has been downloaded.
    pub fn cached_image(&self, url: &str) -> Option<PathBuf> {
        let path = self.image_path(url);
        is_non_empty(&path).then_some(path)
    }

    /// Downloads the image at `url` unless it is already stored.
    pub fn fetch_image(&self, client: &Client, url: &str) -> Result<PathBuf> {
        if let Some(path) = self.cached_image(url) {
            return Ok(path);
        }
        let page = client.get(url, ACCEPT_IMAGE)?;
        if page.is_html() || page.body.is_empty() {
            return Err(Error::NotFound("image"));
        }
        let path = self.image_path(url);
        write_atomically(&path, &page.body)?;
        Ok(path)
    }

    /// Where the icon for `host` is, or would be, stored.
    pub fn icon_path(&self, host: &str) -> PathBuf {
        let safe: String = host
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
            .collect();
        self.dir.join("icons").join(safe)
    }

    /// The stored icon for `host`, if one was found.
    pub fn cached_icon(&self, host: &str) -> Option<PathBuf> {
        let path = self.icon_path(host);
        is_non_empty(&path).then_some(path)
    }

    /// Finds and stores the icon for the website at `site_url`. A site
    /// with no icon is remembered for a week so it isn't asked every
    /// launch; until then this returns [`Error::NotFound`].
    pub fn fetch_icon(&self, client: &Client, site_url: &str) -> Result<PathBuf> {
        let site = Url::parse(site_url).map_err(|_| Error::InvalidUrl(site_url.to_owned()))?;
        let host = site.host_str().ok_or_else(|| Error::InvalidUrl(site_url.to_owned()))?;
        let path = self.icon_path(host);
        if let Ok(metadata) = fs::metadata(&path) {
            let fresh = metadata
                .modified()
                .ok()
                .and_then(|at| at.elapsed().ok())
                .is_some_and(|age| age < MISSING_ICON_RETRY);
            if metadata.len() > 0 {
                return Ok(path);
            } else if fresh {
                return Err(Error::NotFound("icon"));
            }
        }

        let origin = site.join("/").expect("an absolute URL joins a path");
        let declared =
            client.get(origin.as_str(), ACCEPT_HTML).ok().filter(|page| page.is_html()).map(|page| {
                let base = Url::parse(&page.url).unwrap_or(origin.clone());
                icon_links(&page.text(), &base)
            });
        let candidates =
            declared.into_iter().flatten().chain(origin.join("/favicon.ico").ok().map(String::from));
        for candidate in candidates {
            if let Ok(page) = client.get(&candidate, ACCEPT_IMAGE)
                && !page.is_html()
                && !page.body.is_empty()
            {
                write_atomically(&path, &page.body)?;
                return Ok(path);
            }
        }
        write_atomically(&path, &[])?;
        Err(Error::NotFound("icon"))
    }

    /// Deletes images not used for `max_age`. Icons are kept. Returns how
    /// many files were removed.
    pub fn prune_images(&self, max_age: Duration) -> io::Result<usize> {
        let entries = match fs::read_dir(self.dir.join("images")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        let now = SystemTime::now();
        let mut removed = 0;
        for entry in entries.flatten() {
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|at| now.duration_since(at).ok())
                .is_some_and(|age| age > max_age);
            if stale && fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// Icon addresses a page declares, best first: touch icons are large and
/// square; otherwise larger declared sizes win.
pub fn icon_links(html: &str, base: &Url) -> Vec<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("link[rel][href]").expect("static selector");
    let mut found: Vec<(u32, String)> = document
        .select(&selector)
        .filter_map(|link| {
            let element = link.value();
            let rel = element.attr("rel")?.to_ascii_lowercase();
            let rank = if rel.contains("apple-touch-icon") {
                10_000
            } else if rel.split_whitespace().any(|r| r == "icon") {
                element
                    .attr("sizes")
                    .and_then(|sizes| sizes.split(['x', 'X']).next()?.parse().ok())
                    .unwrap_or(16)
            } else {
                return None;
            };
            Some((rank, base.join(element.attr("href")?.trim()).ok()?.into()))
        })
        .collect();
    found.sort_by_key(|(rank, _)| std::cmp::Reverse(*rank));
    found.into_iter().map(|(_, url)| url).collect()
}

fn is_non_empty(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.len() > 0)
}

fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().expect("cache paths have a parent");
    fs::create_dir_all(dir)?;
    let temp = dir.join(format!(".{}-{}.part", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

/// 64-bit FNV-1a: stable across builds and platforms, unlike `std`'s hasher.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Route, TestServer};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake";

    #[test]
    fn hashes_are_stable() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn downloads_images_once() {
        let server = TestServer::start(vec![
            Route::new("/a.png", PNG).header("Content-Type", "image/png"),
            Route::new("/page", "<html></html>").header("Content-Type", "text/html"),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        let client = Client::default();
        let url = server.url("/a.png");

        assert_eq!(cache.cached_image(&url), None);
        let path = cache.fetch_image(&client, &url).unwrap();
        assert_eq!(fs::read(&path).unwrap(), PNG);
        assert_eq!(cache.fetch_image(&client, &url).unwrap(), path);
        assert_eq!(server.hits(), 1);

        assert!(cache.fetch_image(&client, &server.url("/page")).is_err());
        assert!(cache.fetch_image(&client, &server.url("/missing.png")).is_err());
    }

    #[test]
    fn ranks_declared_icons() {
        let base = Url::parse("https://site.test/blog/").unwrap();
        let html = r#"<link rel="icon" href="/small.png" sizes="16x16">
                      <link rel="icon" href="/big.png" sizes="192x192">
                      <link rel="apple-touch-icon" href="touch.png">
                      <link rel="stylesheet" href="/s.css">"#;
        assert_eq!(
            icon_links(html, &base),
            ["https://site.test/blog/touch.png", "https://site.test/big.png", "https://site.test/small.png"]
        );
    }

    #[test]
    fn finds_declared_icons_then_the_favicon() {
        let server = TestServer::start(vec![
            Route::new("/", r#"<html><head><link rel="icon" href="/broken.png"><link rel="icon" href="/i.png" sizes="8x8"></head></html>"#)
                .header("Content-Type", "text/html"),
            Route::new("/i.png", PNG),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        let path = cache.fetch_icon(&Client::default(), &server.url("/some/page")).unwrap();
        assert_eq!(fs::read(&path).unwrap(), PNG);
        assert_eq!(Some(path), cache.cached_icon("127.0.0.1"));

        let favicon_only = TestServer::start(vec![Route::new("/favicon.ico", PNG)]);
        let other = DiskCache::new(dir.path().join("other"));
        assert!(other.fetch_icon(&Client::default(), &favicon_only.url("/")).is_ok());
    }

    #[test]
    fn remembers_sites_without_icons() {
        let server = TestServer::start(vec![]);
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        let client = Client::default();
        assert!(matches!(cache.fetch_icon(&client, &server.url("/")), Err(Error::NotFound(_))));
        let hits = server.hits();
        assert!(matches!(cache.fetch_icon(&client, &server.url("/")), Err(Error::NotFound(_))));
        assert_eq!(server.hits(), hits, "the miss is remembered");
        assert_eq!(cache.cached_icon("127.0.0.1"), None);
    }

    #[test]
    fn prunes_old_images() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path());
        assert_eq!(cache.prune_images(Duration::ZERO).unwrap(), 0, "no directory yet");
        write_atomically(&cache.image_path("https://a.test/x.png"), PNG).unwrap();
        assert_eq!(cache.prune_images(Duration::from_secs(3600)).unwrap(), 0);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(cache.prune_images(Duration::from_millis(1)).unwrap(), 1);
        assert_eq!(cache.cached_image("https://a.test/x.png"), None);
    }
}
