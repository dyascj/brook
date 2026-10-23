//! Reader mode: the full article from its web page, for feeds that only
//! publish a summary.

use crate::error::{Error, Result};
use crate::http::{ACCEPT_HTML, Client};

/// Pages with less readable text than this aren't worth showing.
const MIN_TEXT_CHARS: usize = 200;

/// Extracts the main article from a web page as HTML. `url` resolves
/// relative links and images.
pub fn extract(html: &str, url: &str) -> Result<String> {
    let mut readability =
        dom_smoothie::Readability::new(html, Some(url), None).map_err(|_| Error::Unreadable)?;
    let article = readability.parse().map_err(|_| Error::Unreadable)?;
    if article.text_content.trim().chars().count() < MIN_TEXT_CHARS {
        return Err(Error::Unreadable);
    }
    Ok(article.content.to_string())
}

/// Downloads `url` and extracts its article.
pub fn fetch(client: &Client, url: &str) -> Result<String> {
    let page = client.get(url, ACCEPT_HTML)?;
    extract(&page.text(), &page.url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Route, TestServer};

    fn page() -> String {
        let paragraph = "Brook keeps reading calm. This sentence exists to give the extractor enough \
                         prose to recognize an article, the way a real post would. ";
        format!(
            r#"<!doctype html><html><head><title>A post</title></head><body>
               <nav><a href="/">Home</a> <a href="/about">About</a></nav>
               <article><h1>A post</h1><p>{0}{0}</p><p>{0}<img src="/hero.jpg"></p><p>{0}{0}</p></article>
               <footer>Copyright and other furniture</footer></body></html>"#,
            paragraph
        )
    }

    #[test]
    fn keeps_the_article_and_drops_the_furniture() {
        let html = extract(&page(), "https://blog.test/posts/a").unwrap();
        assert!(html.contains("Brook keeps reading calm"));
        assert!(html.contains("https://blog.test/hero.jpg"), "images resolve: {html}");
        assert!(!html.contains("Copyright and other furniture"));
    }

    #[test]
    fn short_pages_are_unreadable() {
        let html = "<html><body><p>Too short.</p></body></html>";
        assert!(matches!(extract(html, "https://blog.test/"), Err(Error::Unreadable)));
    }

    #[test]
    fn fetches_pages() {
        let server = TestServer::start(vec![Route::new("/post", page()).header("Content-Type", "text/html")]);
        let html = fetch(&Client::default(), &server.url("/post")).unwrap();
        assert!(html.contains("Brook keeps reading calm"));
    }
}
