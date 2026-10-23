//! OPML import and export, the format every feed reader trades
//! subscriptions in.

use quick_xml::events::{BytesStart, Event};

use crate::error::{Error, Result};
use crate::model::{Feed, Folder};
use crate::parse::escape_html;
use crate::store::{NewFeed, Store};

/// A subscription read from an OPML file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpmlFeed {
    /// The feed's address.
    pub url: String,
    /// Its title, falling back to the address.
    pub title: String,
    /// The website, if the file names one.
    pub site_url: Option<String>,
    /// The folder it was filed under. Nested folders keep only the
    /// innermost name, since Brook's folders don't nest.
    pub folder: Option<String>,
}

/// What an import changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    /// Feeds newly subscribed.
    pub added: usize,
    /// Feeds that were already subscribed.
    pub existing: usize,
}

/// Reads the subscriptions in an OPML document. Outlines with an `xmlUrl`
/// are feeds; outlines without one are folders.
pub fn parse(xml: &str) -> Result<Vec<OpmlFeed>> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut feeds = Vec::new();
    // One entry per open outline: the folder name it opened, if any.
    let mut folders: Vec<Option<String>> = Vec::new();
    let mut saw_opml = false;

    loop {
        let event = reader.read_event().map_err(|error| Error::Opml(error.to_string()))?;
        match event {
            Event::Start(ref tag) | Event::Empty(ref tag) => {
                let empty = matches!(event, Event::Empty(_));
                match tag.local_name().as_ref() {
                    "opml" => saw_opml = true,
                    "outline" => {
                        let attr = |name: &str| attribute(tag, name);
                        if let Some(url) = attr("xmlUrl").filter(|url| !url.trim().is_empty()) {
                            let url = url.trim().to_owned();
                            let title = attr("title").or_else(|| attr("text")).unwrap_or_else(|| url.clone());
                            feeds.push(OpmlFeed {
                                title,
                                site_url: attr("htmlUrl").filter(|u| !u.is_empty()),
                                folder: folders.iter().rev().flatten().next().cloned(),
                                url,
                            });
                            if !empty {
                                folders.push(None);
                            }
                        } else if !empty {
                            folders.push(
                                attr("title").or_else(|| attr("text")).filter(|n| !n.trim().is_empty()),
                            );
                        }
                    }
                    _ => {}
                }
            }
            Event::End(ref tag) if tag.local_name().as_ref() == "outline" => {
                folders.pop();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_opml {
        return Err(Error::Opml("this file isn't OPML".into()));
    }
    Ok(feeds)
}

fn attribute(tag: &BytesStart<'_>, name: &str) -> Option<String> {
    let value = tag.try_get_attribute(name).ok().flatten()?;
    let value = value.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?;
    Some(value.trim().to_owned())
}

/// Subscribes to every feed in `feeds`, creating folders as needed. Feeds
/// already subscribed are left where they are.
pub fn import(store: &Store, feeds: &[OpmlFeed]) -> Result<ImportSummary> {
    let mut summary = ImportSummary::default();
    for feed in feeds {
        if store.feed_by_url(&feed.url)?.is_some() {
            summary.existing += 1;
            continue;
        }
        let folder_id = feed.folder.as_deref().map(|name| store.create_folder(name)).transpose()?;
        store.add_feed(&NewFeed {
            url: feed.url.clone(),
            title: feed.title.clone(),
            site_url: feed.site_url.clone(),
            description: None,
            folder_id,
        })?;
        summary.added += 1;
    }
    Ok(summary)
}

/// Writes subscriptions as OPML 2.0, folders first, then unfiled feeds.
pub fn export(folders: &[Folder], feeds: &[Feed]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<opml version=\"2.0\">\n  <head>\n    <title>Brook subscriptions</title>\n  </head>\n  <body>\n",
    );
    let outline = |feed: &Feed, indent: &str| {
        let site = feed
            .site_url
            .as_deref()
            .map(|url| format!(" htmlUrl=\"{}\"", escape_html(url)))
            .unwrap_or_default();
        format!(
            "{indent}<outline type=\"rss\" text=\"{title}\" title=\"{title}\" xmlUrl=\"{url}\"{site}/>\n",
            title = escape_html(&feed.title),
            url = escape_html(&feed.url),
        )
    };
    for folder in folders {
        out.push_str(&format!("    <outline text=\"{0}\" title=\"{0}\">\n", escape_html(&folder.name)));
        for feed in feeds.iter().filter(|feed| feed.folder_id == Some(folder.id)) {
            out.push_str(&outline(feed, "      "));
        }
        out.push_str("    </outline>\n");
    }
    for feed in feeds.iter().filter(|feed| feed.folder_id.is_none()) {
        out.push_str(&outline(feed, "    "));
    }
    out.push_str("  </body>\n</opml>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="1.0">
  <head><title>Exported</title></head>
  <body>
    <outline text="Tech" title="Tech">
      <outline type="rss" text="Rust Blog" xmlUrl="https://blog.rust-lang.org/feed.xml" htmlUrl="https://blog.rust-lang.org/"/>
      <outline text="Deeper">
        <outline type="rss" text="Nested &amp; Co" xmlUrl="https://nested.test/rss"/>
      </outline>
    </outline>
    <outline type="rss" title="Loose" text="ignored" xmlUrl=" https://loose.test/atom "/>
    <outline type="rss" text="No URL"/>
  </body>
</opml>"#;

    #[test]
    fn parses_folders_and_feeds() {
        let feeds = parse(SAMPLE).unwrap();
        assert_eq!(
            feeds,
            [
                OpmlFeed {
                    url: "https://blog.rust-lang.org/feed.xml".into(),
                    title: "Rust Blog".into(),
                    site_url: Some("https://blog.rust-lang.org/".into()),
                    folder: Some("Tech".into()),
                },
                OpmlFeed {
                    url: "https://nested.test/rss".into(),
                    title: "Nested & Co".into(),
                    site_url: None,
                    folder: Some("Deeper".into()),
                },
                OpmlFeed {
                    url: "https://loose.test/atom".into(),
                    title: "Loose".into(),
                    site_url: None,
                    folder: None,
                },
            ]
        );
    }

    #[test]
    fn rejects_other_documents() {
        assert!(matches!(parse("<rss><channel/></rss>"), Err(Error::Opml(_))));
        assert!(matches!(parse("<opml><body><outline></opml>"), Err(Error::Opml(_))));
    }

    #[test]
    fn imports_once() {
        let store = Store::open_in_memory().unwrap();
        let feeds = parse(SAMPLE).unwrap();
        assert_eq!(import(&store, &feeds).unwrap(), ImportSummary { added: 3, existing: 0 });
        assert_eq!(import(&store, &feeds).unwrap(), ImportSummary { added: 0, existing: 3 });
        let folders: Vec<_> = store.folders().unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(folders, ["Tech", "Deeper"]);
    }

    #[test]
    fn export_round_trips() {
        let store = Store::open_in_memory().unwrap();
        import(&store, &parse(SAMPLE).unwrap()).unwrap();
        let exported = export(&store.folders().unwrap(), &store.feeds().unwrap());
        assert!(exported.contains("Nested &amp; Co"));

        let mut again = parse(&exported).unwrap();
        let mut original = parse(SAMPLE).unwrap();
        again.sort_by(|a, b| a.url.cmp(&b.url));
        original.sort_by(|a, b| a.url.cmp(&b.url));
        assert_eq!(again, original);
    }
}
