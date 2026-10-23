//! Parsing RSS, Atom, and JSON Feed documents into one normalized shape.
//!
//! `feed-rs` does the format work. This module decides which of its many
//! optional fields Brook uses: the article link, the lead image, the body,
//! and a plain-text excerpt.

use chrono::{DateTime, Utc};
use feed_rs::model::{Entry, Feed, Link, Text};

use crate::error::{Error, Result};
use crate::text::{excerpt, html_to_text};

/// Longest excerpt kept for list rows and search, in characters.
pub const EXCERPT_CHARS: usize = 280;

/// A feed document, normalized.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedFeed {
    /// The feed's title, or its host when it has none.
    pub title: String,
    /// The website the feed belongs to.
    pub site_url: Option<String>,
    /// The feed's description, as plain text.
    pub description: Option<String>,
    /// Entries in document order.
    pub entries: Vec<ParsedEntry>,
}

/// One entry of a [`ParsedFeed`].
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedEntry {
    /// A stable identifier, unique within the feed.
    pub guid: String,
    /// Headline as plain text.
    pub title: String,
    /// The article on the web.
    pub url: Option<String>,
    /// Byline.
    pub author: Option<String>,
    /// A plain-text excerpt of the body.
    pub excerpt: String,
    /// The body as HTML.
    pub content_html: Option<String>,
    /// The lead image.
    pub image_url: Option<String>,
    /// Publication time, if the feed gives one.
    pub published_at: Option<DateTime<Utc>>,
}

/// Parses a feed document. `source_url` is where it was fetched from; it
/// resolves relative links and names untitled feeds.
pub fn parse_feed(bytes: &[u8], source_url: &str) -> Result<ParsedFeed> {
    let feed = feed_rs::parser::Builder::new()
        .base_uri(Some(source_url))
        .build()
        .parse(bytes)
        .map_err(|error| Error::NotAFeed(error.to_string()))?;
    let mut parsed = normalize(feed, source_url);
    // feed-rs skips a few common image fields; fill the gaps by position.
    for (entry, image) in parsed.entries.iter_mut().zip(extra_images(bytes)) {
        if entry.image_url.is_none() {
            entry.image_url = image;
        }
    }
    Ok(parsed)
}

/// Per-entry images that feed-rs drops, in document order: a JSON Feed
/// item's `image` or `banner_image`, and a bare `media:thumbnail` outside
/// any `media:content` (BBC and The Guardian publish those).
fn extra_images(bytes: &[u8]) -> Vec<Option<String>> {
    let trimmed = bytes.trim_ascii_start();
    if trimmed.starts_with(b"{") {
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(trimmed) else { return vec![] };
        let items = json["items"].as_array().map(Vec::as_slice).unwrap_or_default();
        return items
            .iter()
            .map(|item| item["image"].as_str().or(item["banner_image"].as_str()).map(str::to_owned))
            .collect();
    }

    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(trimmed);
    let mut images: Vec<Option<String>> = Vec::new();
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(tag) | Event::Empty(tag)) => match tag.name().as_ref() {
                "item" | "entry" => images.push(None),
                "media:thumbnail" => {
                    if let Some(slot @ None) = images.last_mut() {
                        *slot = tag.try_get_attribute("url").ok().flatten().and_then(|url| {
                            url.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .ok()
                                .map(|value| value.into_owned())
                        });
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buffer.clear();
    }
    images
}

fn normalize(feed: Feed, source_url: &str) -> ParsedFeed {
    let site_url = feed
        .links
        .iter()
        .find(|link| is_page_link(link) && link.href != source_url)
        .map(|link| link.href.clone());
    let title = feed
        .title
        .as_ref()
        .map(plain)
        .filter(|title| !title.is_empty())
        .or_else(|| host_of(site_url.as_deref().unwrap_or(source_url)))
        .unwrap_or_else(|| source_url.to_owned());
    let description = feed.description.as_ref().map(plain).filter(|d| !d.is_empty());
    let entries = feed.entries.into_iter().map(normalize_entry).collect();
    ParsedFeed { title, site_url, description, entries }
}

fn normalize_entry(entry: Entry) -> ParsedEntry {
    let url = entry
        .links
        .iter()
        .find(|link| is_page_link(link))
        .or_else(|| entry.links.first())
        .map(|link| link.href.clone());

    let content_html = entry
        .content
        .as_ref()
        .and_then(|content| content.body.clone())
        .or_else(|| entry.summary.as_ref().map(as_html))
        .or_else(|| media_description(&entry).map(|text| text_to_html(&text)))
        .filter(|html| !html.trim().is_empty());

    let excerpt_source = entry
        .summary
        .as_ref()
        .map(plain)
        .filter(|summary| !summary.is_empty())
        .or_else(|| content_html.as_deref().map(html_to_text))
        .unwrap_or_default();

    let title = entry
        .title
        .as_ref()
        .map(plain)
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| excerpt(&excerpt_source, 80))
        .if_empty("Untitled");

    let author = entry
        .authors
        .iter()
        .filter_map(|person| person.name.as_deref())
        .map(str::trim)
        .find(|name| !name.is_empty())
        .map(str::to_owned);

    let image_url = lead_image(&entry, content_html.as_deref());
    // Many bodies open by repeating the headline; the excerpt shouldn't.
    let excerpt_source = match excerpt_source.strip_prefix(title.as_str()) {
        Some(rest) if !rest.trim().is_empty() => {
            rest.trim_start_matches(|c: char| c.is_whitespace() || c == ':').to_owned()
        }
        _ => excerpt_source,
    };

    ParsedEntry {
        guid: entry.id,
        title,
        url,
        author,
        excerpt: excerpt(&excerpt_source, EXCERPT_CHARS),
        content_html,
        image_url,
        published_at: entry.published.or(entry.updated),
    }
}

/// A link to a web page rather than to an enclosure, a comments feed, or
/// the feed itself.
fn is_page_link(link: &Link) -> bool {
    let rel_ok = matches!(link.rel.as_deref(), None | Some("alternate"));
    let type_ok = link.media_type.as_deref().is_none_or(|media_type| media_type.starts_with("text/html"));
    rel_ok && type_ok
}

/// The lead image: Media RSS first (YouTube and most news sites), then
/// image enclosures, then the first `<img>` in the body.
fn lead_image(entry: &Entry, content_html: Option<&str>) -> Option<String> {
    let from_media = entry.media.iter().find_map(|media| {
        media.thumbnails.first().map(|thumbnail| thumbnail.image.uri.clone()).or_else(|| {
            media.content.iter().find_map(|content| {
                let url = content.url.as_ref()?;
                let is_image = content
                    .content_type
                    .as_ref()
                    .map(|t| t.ty() == "image")
                    .unwrap_or_else(|| looks_like_image(url.as_str()));
                is_image.then(|| url.to_string())
            })
        })
    });
    let from_enclosure = || {
        entry.links.iter().find_map(|link| {
            let is_enclosure = link.rel.as_deref() == Some("enclosure");
            let is_image = link
                .media_type
                .as_deref()
                .map(|t| t.starts_with("image/"))
                .unwrap_or_else(|| looks_like_image(&link.href));
            (is_enclosure && is_image).then(|| link.href.clone())
        })
    };
    let from_body = || content_html.and_then(first_img_src);
    from_media.or_else(from_enclosure).or_else(from_body)
}

fn looks_like_image(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    [".jpg", ".jpeg", ".png", ".gif", ".webp", ".avif"].iter().any(|ext| path.ends_with(ext))
}

fn first_img_src(html: &str) -> Option<String> {
    let fragment = scraper::Html::parse_fragment(html);
    let selector = scraper::Selector::parse("img[src]").expect("static selector");
    fragment
        .select(&selector)
        .filter_map(|img| img.value().attr("src"))
        .find(|src| src.starts_with("http"))
        .map(str::to_owned)
}

/// YouTube puts the video's description in `media:group/media:description`.
fn media_description(entry: &Entry) -> Option<String> {
    entry
        .media
        .iter()
        .find_map(|media| media.description.as_ref())
        .map(|text| text.content.clone())
        .filter(|text| !text.trim().is_empty())
}

/// A `Text` as plain text. Even feeds that mark text as plain often carry
/// entities and stray tags, so every kind goes through the HTML decoder.
fn plain(text: &Text) -> String {
    html_to_text(&text.content)
}

/// A `Text` as HTML, escaping it when the feed marked it as plain.
fn as_html(text: &Text) -> String {
    if text.content_type.ty() == "text" && text.content_type.subty() == "plain" {
        text_to_html(&text.content)
    } else {
        text.content.clone()
    }
}

/// Plain text to HTML paragraphs: blank lines split paragraphs, single
/// newlines become line breaks.
pub fn text_to_html(text: &str) -> String {
    text.split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .map(|paragraph| format!("<p>{}</p>", escape_html(paragraph).replace('\n', "<br>")))
        .collect()
}

/// Escapes the five HTML-special characters.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok()?.host_str().map(|h| h.trim_start_matches("www.").to_owned())
}

trait IfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.trim().is_empty() { fallback.to_owned() } else { self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/"
     xmlns:media="http://search.yahoo.com/mrss/" xmlns:dc="http://purl.org/dc/elements/1.1/">
<channel>
  <title>Example &amp; Co</title>
  <link>https://example.com/</link>
  <description>Notes from &lt;b&gt;Example&lt;/b&gt;</description>
  <item>
    <title>First post</title>
    <link>https://example.com/first</link>
    <guid>first-guid</guid>
    <dc:creator>Ada</dc:creator>
    <pubDate>Tue, 03 Jun 2025 09:00:00 GMT</pubDate>
    <description>A short &lt;em&gt;summary&lt;/em&gt;.</description>
    <content:encoded><![CDATA[<p>Full <b>body</b>.</p><img src="https://example.com/a.png">]]></content:encoded>
  </item>
  <item>
    <link>https://example.com/second</link>
    <description>No title here, only a description that becomes the title.</description>
    <enclosure url="https://example.com/cover.jpg" type="image/jpeg" length="1"/>
  </item>
  <item>
    <title>Media</title>
    <link>https://example.com/third</link>
    <media:thumbnail url="https://example.com/thumb.jpg"/>
  </item>
</channel>
</rss>"#;

    #[test]
    fn parses_rss() {
        let feed = parse_feed(RSS.as_bytes(), "https://example.com/feed.xml").unwrap();
        assert_eq!(feed.title, "Example & Co");
        assert_eq!(feed.site_url.as_deref(), Some("https://example.com/"));
        assert_eq!(feed.description.as_deref(), Some("Notes from Example"));
        assert_eq!(feed.entries.len(), 3);

        let first = &feed.entries[0];
        assert_eq!(first.guid, "first-guid");
        assert_eq!(first.title, "First post");
        assert_eq!(first.url.as_deref(), Some("https://example.com/first"));
        assert_eq!(first.author.as_deref(), Some("Ada"));
        assert_eq!(first.excerpt, "A short summary.");
        assert!(first.content_html.as_deref().unwrap().contains("<b>body</b>"));
        assert_eq!(first.image_url.as_deref(), Some("https://example.com/a.png"));
        assert_eq!(first.published_at.unwrap().to_rfc3339(), "2025-06-03T09:00:00+00:00");
    }

    #[test]
    fn untitled_entries_borrow_their_excerpt() {
        let feed = parse_feed(RSS.as_bytes(), "https://example.com/feed.xml").unwrap();
        let second = &feed.entries[1];
        assert!(second.title.starts_with("No title here"), "{}", second.title);
        assert!(!second.guid.is_empty());
        assert_eq!(second.image_url.as_deref(), Some("https://example.com/cover.jpg"));
        assert_eq!(second.published_at, None);
    }

    #[test]
    fn media_thumbnails_are_lead_images() {
        let feed = parse_feed(RSS.as_bytes(), "https://example.com/feed.xml").unwrap();
        assert_eq!(feed.entries[2].image_url.as_deref(), Some("https://example.com/thumb.jpg"));
    }

    #[test]
    fn parses_atom_and_skips_non_page_links() {
        let atom = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title type="html">Atom &lt;i&gt;Blog&lt;/i&gt;</title>
  <link href="https://blog.example.org/atom.xml" rel="self"/>
  <link href="https://blog.example.org/"/>
  <id>urn:uuid:1</id>
  <updated>2025-01-01T00:00:00Z</updated>
  <entry>
    <title>Hello</title>
    <link href="https://blog.example.org/hello/comments.xml" rel="replies" type="application/atom+xml"/>
    <link href="https://blog.example.org/hello" rel="alternate" type="text/html"/>
    <id>urn:uuid:2</id>
    <updated>2025-01-02T00:00:00Z</updated>
    <author><name>Grace</name></author>
    <content type="html">&lt;p&gt;Hi there&lt;/p&gt;</content>
  </entry>
</feed>"#;
        let feed = parse_feed(atom.as_bytes(), "https://blog.example.org/atom.xml").unwrap();
        assert_eq!(feed.title, "Atom Blog");
        assert_eq!(feed.site_url.as_deref(), Some("https://blog.example.org/"));
        let entry = &feed.entries[0];
        assert_eq!(entry.url.as_deref(), Some("https://blog.example.org/hello"));
        assert_eq!(entry.author.as_deref(), Some("Grace"));
        assert_eq!(entry.excerpt, "Hi there");
        assert_eq!(entry.published_at.unwrap().to_rfc3339(), "2025-01-02T00:00:00+00:00");
    }

    #[test]
    fn parses_json_feed() {
        let json = r#"{
            "version": "https://jsonfeed.org/version/1.1",
            "title": "JSON Blog",
            "home_page_url": "https://json.example.com/",
            "items": [{
                "id": "1",
                "url": "https://json.example.com/1",
                "title": "One",
                "content_html": "<p>Body one</p>",
                "image": "https://json.example.com/1.png",
                "date_published": "2025-02-03T04:05:06Z",
                "authors": [{"name": "Lin"}]
            }]
        }"#;
        let feed = parse_feed(json.as_bytes(), "https://json.example.com/feed.json").unwrap();
        assert_eq!(feed.title, "JSON Blog");
        let entry = &feed.entries[0];
        assert_eq!(entry.guid, "1");
        assert_eq!(entry.excerpt, "Body one");
        assert_eq!(entry.author.as_deref(), Some("Lin"));
        assert_eq!(entry.image_url.as_deref(), Some("https://json.example.com/1.png"));
    }

    #[test]
    fn youtube_descriptions_become_the_body() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns:media="http://search.yahoo.com/mrss/" xmlns="http://www.w3.org/2005/Atom">
 <title>A Channel</title>
 <link rel="alternate" href="https://www.youtube.com/channel/UC123"/>
 <entry>
  <id>yt:video:abcdefghijk</id>
  <title>A video</title>
  <link rel="alternate" href="https://www.youtube.com/watch?v=abcdefghijk"/>
  <published>2025-03-01T10:00:00+00:00</published>
  <media:group>
   <media:title>A video</media:title>
   <media:content url="https://www.youtube.com/v/abcdefghijk?version=3" type="application/x-shockwave-flash" width="640" height="390"/>
   <media:thumbnail url="https://i1.ytimg.com/vi/abcdefghijk/hqdefault.jpg" width="480" height="360"/>
   <media:description>Line one
Line two

Second paragraph &amp; more</media:description>
  </media:group>
 </entry>
</feed>"#;
        let feed =
            parse_feed(xml.as_bytes(), "https://www.youtube.com/feeds/videos.xml?channel_id=UC123").unwrap();
        let entry = &feed.entries[0];
        assert_eq!(entry.url.as_deref(), Some("https://www.youtube.com/watch?v=abcdefghijk"));
        assert_eq!(entry.image_url.as_deref(), Some("https://i1.ytimg.com/vi/abcdefghijk/hqdefault.jpg"));
        assert_eq!(
            entry.content_html.as_deref(),
            Some("<p>Line one<br>Line two</p><p>Second paragraph &amp; more</p>")
        );
    }

    #[test]
    fn excerpts_skip_a_repeated_headline() {
        let xml = r#"<rss version="2.0"><channel><title>T</title>
            <item><title>Big news</title><guid>1</guid><description>&lt;p&gt;&lt;a href="x"&gt;Big news&lt;/a&gt;&lt;/p&gt;&lt;p&gt;The details.&lt;/p&gt;</description></item>
            <item><title>Only</title><guid>2</guid><description>Only</description></item>
        </channel></rss>"#;
        let feed = parse_feed(xml.as_bytes(), "https://x.test/rss").unwrap();
        assert_eq!(feed.entries[0].excerpt, "The details.");
        assert_eq!(feed.entries[1].excerpt, "Only", "an excerpt that is only the title stays");
    }

    #[test]
    fn untitled_feeds_take_their_host() {
        let xml = r#"<rss version="2.0"><channel><link>https://www.quiet.example/</link></channel></rss>"#;
        let feed = parse_feed(xml.as_bytes(), "https://www.quiet.example/rss").unwrap();
        assert_eq!(feed.title, "quiet.example");
    }

    #[test]
    fn rejects_html_pages() {
        let error =
            parse_feed(b"<!doctype html><html><body>hi</body></html>", "https://x.test/").unwrap_err();
        assert!(matches!(error, Error::NotAFeed(_)));
    }

    #[test]
    fn text_to_html_escapes_and_splits() {
        assert_eq!(text_to_html("a < b\nc\n\n\nd"), "<p>a &lt; b<br>c</p><p>d</p>");
    }
}
