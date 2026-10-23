//! A hand-picked catalog of feeds for the Discover screen, shipped inside
//! the app so it works offline.

use std::sync::OnceLock;

use serde::Deserialize;

/// A group of catalog feeds.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Category {
    /// A stable identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One line about what's inside.
    pub description: String,
    /// The feeds, best known first.
    pub feeds: Vec<CatalogFeed>,
}

/// A feed in the catalog.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct CatalogFeed {
    /// Display name.
    pub name: String,
    /// One line about it.
    pub description: String,
    /// The feed's address.
    pub url: String,
    /// The website.
    pub site_url: Option<String>,
}

#[derive(Deserialize)]
struct Catalog {
    categories: Vec<Category>,
}

/// Every category, in display order.
pub fn categories() -> &'static [Category] {
    static CATALOG: OnceLock<Vec<Category>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str::<Catalog>(include_str!("catalog.json"))
            .expect("catalog.json is valid; a unit test checks it")
            .categories
    })
}

/// Catalog feeds whose name or description contains every word of
/// `query`, ignoring case, with their category.
pub fn search(query: &str) -> Vec<(&'static Category, &'static CatalogFeed)> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    categories()
        .iter()
        .flat_map(|category| category.feeds.iter().map(move |feed| (category, feed)))
        .filter(|(_, feed)| {
            let haystack = format!("{} {}", feed.name, feed.description).to_lowercase();
            words.iter().all(|word| haystack.contains(word))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn catalog_is_well_formed() {
        let categories = categories();
        assert!(categories.len() >= 10);
        let mut urls = HashSet::new();
        for category in categories {
            assert!(!category.feeds.is_empty(), "{} is empty", category.id);
            for feed in &category.feeds {
                assert!(feed.url.starts_with("http"), "{}", feed.url);
                assert!(!feed.name.is_empty() && !feed.description.is_empty());
                assert!(urls.insert(&feed.url), "{} is listed twice", feed.url);
            }
        }
    }

    #[test]
    fn searches_names_and_descriptions() {
        let found = search("RUST weekly");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1.name, "This Week in Rust");
        assert_eq!(found[0].0.id, "tech-programming");
        assert!(search("zzzz-nothing").is_empty());
        assert_eq!(search("").len(), categories().iter().map(|c| c.feeds.len()).sum::<usize>());
    }

    /// Checks every catalog feed against the live web. Run by hand before a
    /// release: `cargo test -p brook-core catalog_feeds_are_live -- --ignored`.
    #[test]
    #[ignore = "uses the network"]
    fn catalog_feeds_are_live() {
        let client = crate::http::Client::default();
        let dead: Vec<String> = categories()
            .iter()
            .flat_map(|category| &category.feeds)
            .filter_map(|feed| {
                let page = client.get(&feed.url, crate::http::ACCEPT_FEED);
                let ok = page.is_ok_and(|page| crate::parse::parse_feed(&page.body, &page.url).is_ok());
                (!ok).then(|| feed.url.clone())
            })
            .collect();
        assert!(dead.is_empty(), "dead feeds: {dead:#?}");
    }
}
