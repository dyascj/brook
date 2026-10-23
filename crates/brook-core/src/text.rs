//! Turning feed HTML into short plain text for lists and search.

use scraper::{Html, Node};

/// Elements whose text is never shown to a reader.
const HIDDEN: &[&str] = &["script", "style", "noscript", "template", "head", "iframe", "svg"];

/// Elements that end a line of text, so their neighbors don't run together.
const BREAKING: &[&str] = &[
    "p",
    "div",
    "br",
    "li",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "pre",
    "tr",
    "figure",
    "figcaption",
    "section",
    "article",
    "header",
    "footer",
    "hr",
    "dd",
    "dt",
];

/// The readable text of an HTML fragment, with runs of whitespace collapsed
/// to single spaces. Entities are decoded. Scripts and styles are dropped.
///
/// Text that does not look like HTML is only whitespace-collapsed, so plain
/// summaries pass through unchanged.
pub fn html_to_text(html: &str) -> String {
    if !html.contains('<') && !html.contains('&') {
        return collapse_whitespace(html);
    }
    let fragment = Html::parse_fragment(html);
    let mut out = String::with_capacity(html.len() / 2);
    let mut stack = vec![fragment.tree.root()];
    // Depth-first, children pushed in reverse so they pop in document order.
    while let Some(node) = stack.pop() {
        match node.value() {
            Node::Text(text) => out.push_str(text),
            Node::Element(element) => {
                let name = element.name();
                if HIDDEN.contains(&name) {
                    continue;
                }
                if BREAKING.contains(&name) {
                    out.push(' ');
                }
                if name == "img" {
                    continue;
                }
            }
            _ => {}
        }
        let children: Vec<_> = node.children().collect();
        stack.extend(children.into_iter().rev());
    }
    collapse_whitespace(&out)
}

/// Collapses every run of whitespace to one space and trims the ends.
pub fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Shortens text to at most `max_chars` characters, cutting at a word
/// boundary and ending with an ellipsis when anything was removed.
pub fn excerpt(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max_chars).collect();
    let cut = match cut.rfind(char::is_whitespace) {
        // Only back up to a space if it keeps most of the text.
        Some(space) if space > cut.len() / 2 => &cut[..space],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches(|c: char| c.is_whitespace() || c.is_ascii_punctuation()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_decodes_entities() {
        assert_eq!(
            html_to_text("<p>Fish &amp; <b>chips</b></p><p>are&nbsp;good</p>"),
            "Fish & chips are good"
        );
    }

    #[test]
    fn drops_scripts_and_styles() {
        let html = "<style>p{}</style><p>Hello</p><script>alert(1)</script>";
        assert_eq!(html_to_text(html), "Hello");
    }

    #[test]
    fn breaks_between_blocks() {
        assert_eq!(html_to_text("<h1>Title</h1><p>Body</p>"), "Title Body");
        assert_eq!(html_to_text("one<br>two"), "one two");
    }

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(html_to_text("  just\n  words "), "just words");
        assert_eq!(html_to_text("1 < 2"), "1 < 2");
    }

    #[test]
    fn excerpt_cuts_at_a_word() {
        assert_eq!(excerpt("short", 10), "short");
        assert_eq!(excerpt("the quick brown fox jumps", 12), "the quick…");
        assert_eq!(excerpt("unbreakablewordhere", 5), "unbre…");
    }

    #[test]
    fn excerpt_counts_characters_not_bytes() {
        assert_eq!(excerpt("ééééé ééééé", 7), "ééééé…");
    }

    #[test]
    fn excerpt_drops_trailing_punctuation_before_the_ellipsis() {
        assert_eq!(excerpt("Hello, world and more", 7), "Hello…");
    }
}
