//! Article HTML as a small document the reader can typeset.
//!
//! Feeds deliver anything from clean paragraphs to whole web pages. Brook
//! keeps only what reads well in a calm column: paragraphs with bold,
//! italic, code, and links; headings; quotes; lists; code blocks; images;
//! video cards; and rules. Layout markup, scripts, forms, and tracking
//! pixels fall away. Relative links and images resolve against the
//! article's address.

use ego_tree::NodeRef;
use scraper::{Html, Node};
use url::Url;

use crate::video::Video;

/// A typeset-ready article body.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    /// Top-level blocks in reading order.
    pub blocks: Vec<Block>,
}

/// A block of content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// A paragraph of styled text. Contains `\n` where the source had `<br>`.
    Paragraph(Vec<Span>),
    /// A heading, level 1 to 6.
    Heading {
        /// 1 for the largest.
        level: u8,
        /// The heading's text.
        spans: Vec<Span>,
    },
    /// A block quotation.
    Quote(Vec<Block>),
    /// A bulleted or numbered list. Each item is its own run of blocks.
    List {
        /// Numbered when true.
        ordered: bool,
        /// The items.
        items: Vec<Vec<Block>>,
    },
    /// Preformatted text, whitespace kept.
    Code(String),
    /// An image.
    Image {
        /// Absolute address.
        url: String,
        /// Alternative text.
        alt: String,
    },
    /// A caption under an image.
    Caption(Vec<Span>),
    /// An embedded video.
    Video(Video),
    /// A thematic break.
    Rule,
}

/// A run of text with one style.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    /// The text.
    pub text: String,
    /// How it looks.
    pub marks: Marks,
    /// Where it links, if anywhere.
    pub link: Option<String>,
}

/// Inline styles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Marks {
    /// Strong emphasis.
    pub bold: bool,
    /// Emphasis.
    pub italic: bool,
    /// Inline code.
    pub code: bool,
    /// Struck through.
    pub strike: bool,
}

/// Elements dropped with everything inside them.
const SKIPPED: &[&str] = &[
    "script", "style", "noscript", "template", "head", "svg", "math", "form", "input", "button", "select",
    "textarea", "nav", "object", "canvas", "audio", "source", "track", "map",
];

/// Elements that only group other blocks.
const CONTAINERS: &[&str] = &[
    "p", "div", "section", "article", "header", "footer", "main", "aside", "figure", "center", "details",
    "summary", "dl", "dt", "dd", "table", "thead", "tbody", "tfoot", "tr", "caption", "address", "hgroup",
    "body", "html", "picture",
];

impl Document {
    /// Converts an HTML fragment. `base` is the article's address, for
    /// resolving relative links and images.
    pub fn from_html(html: &str, base: Option<&str>) -> Self {
        let base = base.and_then(|base| Url::parse(base).ok());
        let fragment = Html::parse_fragment(html);
        let mut sink = Sink::default();
        let context = Context { base: base.as_ref(), marks: Marks::default(), link: None };
        sink.children(fragment.tree.root(), &context);
        sink.flush();
        Document { blocks: sink.blocks }
    }

    /// Converts plain text: blank lines separate paragraphs.
    pub fn from_text(text: &str) -> Self {
        let blocks = text
            .split("\n\n")
            .map(str::trim)
            .filter(|paragraph| !paragraph.is_empty())
            .map(|paragraph| Block::Paragraph(vec![Span { text: paragraph.to_owned(), ..Span::default() }]))
            .collect();
        Document { blocks }
    }

    /// Puts a video card first when the article itself is a video page
    /// that the body doesn't already embed (YouTube feeds work this way).
    pub fn with_lead_video(mut self, article_url: Option<&str>) -> Self {
        let Some(video) = article_url.and_then(Video::from_url) else { return self };
        let embedded = self.blocks.iter().any(|block| matches!(block, Block::Video(v) if *v == video));
        if !embedded {
            self.blocks.insert(0, Block::Video(video));
        }
        self
    }

    /// Puts the article's lead image first when the body shows no image or
    /// video of its own (common in feeds that attach the image separately).
    pub fn with_lead_image(mut self, image_url: Option<&str>) -> Self {
        let Some(url) = image_url.filter(|url| url.starts_with("http")) else { return self };
        let has_media =
            self.blocks.iter().any(|block| matches!(block, Block::Image { .. } | Block::Video(_)));
        if !has_media {
            self.blocks.insert(0, Block::Image { url: url.to_owned(), alt: String::new() });
        }
        self
    }

    /// Whether there is nothing to show.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// The number of words, for a reading-time estimate.
    pub fn word_count(&self) -> usize {
        fn spans(spans: &[Span]) -> usize {
            spans.iter().map(|span| span.text.split_whitespace().count()).sum()
        }
        fn blocks(list: &[Block]) -> usize {
            list.iter()
                .map(|block| match block {
                    Block::Paragraph(s) | Block::Caption(s) | Block::Heading { spans: s, .. } => spans(s),
                    Block::Quote(inner) => blocks(inner),
                    Block::List { items, .. } => items.iter().map(|item| blocks(item)).sum(),
                    Block::Code(code) => code.split_whitespace().count(),
                    Block::Image { .. } | Block::Video(_) | Block::Rule => 0,
                })
                .sum()
        }
        blocks(&self.blocks)
    }

    /// Minutes to read at 230 words a minute, at least one.
    pub fn reading_minutes(&self) -> usize {
        self.word_count().div_ceil(230).max(1)
    }
}

impl Span {
    /// Joins a run of spans into plain text.
    pub fn plain(spans: &[Span]) -> String {
        spans.iter().map(|span| span.text.as_str()).collect()
    }
}

#[derive(Clone)]
struct Context<'a> {
    base: Option<&'a Url>,
    marks: Marks,
    link: Option<String>,
}

impl Context<'_> {
    fn resolve(&self, href: &str) -> Option<String> {
        let href = href.trim();
        if href.is_empty() || href.starts_with('#') {
            return None;
        }
        let url = match self.base {
            Some(base) => base.join(href).ok()?,
            None => Url::parse(href).ok()?,
        };
        matches!(url.scheme(), "http" | "https" | "mailto").then(|| url.into())
    }
}

/// Collects blocks, with a buffer for the paragraph being built.
#[derive(Default)]
struct Sink {
    blocks: Vec<Block>,
    inline: Vec<Span>,
}

impl Sink {
    fn children(&mut self, node: NodeRef<'_, Node>, context: &Context<'_>) {
        for child in node.children() {
            self.node(child, context);
        }
    }

    fn node(&mut self, node: NodeRef<'_, Node>, context: &Context<'_>) {
        let element = match node.value() {
            Node::Text(text) => return self.text(text, context),
            Node::Element(element) => element,
            _ => return,
        };
        let name = element.name();
        if SKIPPED.contains(&name) || element.attr("hidden").is_some() {
            return;
        }
        let mut inner = context.clone();
        match name {
            "b" | "strong" => inner.marks.bold = true,
            "i" | "em" | "cite" | "dfn" | "var" => inner.marks.italic = true,
            "code" | "kbd" | "samp" | "tt" => inner.marks.code = true,
            "s" | "del" | "strike" => inner.marks.strike = true,
            "a" => inner.link = element.attr("href").and_then(|href| context.resolve(href)),
            "br" => return self.push(Span { text: "\n".into(), marks: context.marks, link: None }),
            "img" => return self.image(element, context),
            "iframe" | "embed" | "video" => return self.video(node, element, context),
            "hr" => return self.block(Block::Rule),
            "pre" => {
                let code = text_content(node);
                let code = code.trim_matches('\n').trim_end();
                if !code.is_empty() {
                    self.block(Block::Code(code.to_owned()));
                }
                return;
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name.as_bytes()[1] - b'0';
                let (spans, rest) = self.nested(node, &inner);
                if !spans.is_empty() {
                    self.block(Block::Heading { level, spans });
                }
                self.blocks.extend(rest);
                return;
            }
            "figcaption" => {
                let (spans, rest) = self.nested(node, &inner);
                self.blocks.extend(rest);
                if !spans.is_empty() {
                    self.block(Block::Caption(spans));
                }
                return;
            }
            "blockquote" => {
                let (spans, mut blocks) = self.nested(node, &inner);
                if !spans.is_empty() {
                    blocks.insert(0, Block::Paragraph(spans));
                }
                if !blocks.is_empty() {
                    self.block(Block::Quote(blocks));
                }
                return;
            }
            "ul" | "ol" => {
                let items: Vec<Vec<Block>> = node
                    .children()
                    .filter(|child| child.value().as_element().is_some_and(|e| e.name() == "li"))
                    .map(|item| {
                        let (spans, mut blocks) = self.nested(item, &inner);
                        if !spans.is_empty() {
                            blocks.insert(0, Block::Paragraph(spans));
                        }
                        blocks
                    })
                    .filter(|item| !item.is_empty())
                    .collect();
                if !items.is_empty() {
                    self.block(Block::List { ordered: name == "ol", items });
                }
                return;
            }
            "td" | "th" => {
                self.children(node, &inner);
                return self.push(Span { text: "  ".into(), ..Span::default() });
            }
            _ if CONTAINERS.contains(&name) => {
                self.flush();
                self.children(node, &inner);
                return self.flush();
            }
            _ => {}
        }
        self.children(node, &inner);
    }

    /// Converts `node`'s children in a fresh sink. Returns the trailing
    /// inline text (trimmed) and the blocks before it, with any leading
    /// inline text already turned into a paragraph.
    fn nested(&self, node: NodeRef<'_, Node>, context: &Context<'_>) -> (Vec<Span>, Vec<Block>) {
        let mut sink = Sink::default();
        sink.children(node, context);
        let spans = tidy(std::mem::take(&mut sink.inline));
        if sink.blocks.is_empty() {
            (spans, vec![])
        } else {
            if !spans.is_empty() {
                sink.blocks.push(Block::Paragraph(spans));
            }
            (vec![], sink.blocks)
        }
    }

    fn text(&mut self, text: &str, context: &Context<'_>) {
        let mut collapsed = String::with_capacity(text.len());
        let mut last_space = self.inline.last().is_none_or(|span| span.text.ends_with([' ', '\n']));
        for c in text.chars() {
            if c.is_whitespace() {
                if !last_space {
                    collapsed.push(' ');
                    last_space = true;
                }
            } else {
                collapsed.push(c);
                last_space = false;
            }
        }
        if !collapsed.is_empty() {
            self.push(Span { text: collapsed, marks: context.marks, link: context.link.clone() });
        }
    }

    fn push(&mut self, span: Span) {
        match self.inline.last_mut() {
            Some(last) if last.marks == span.marks && last.link == span.link => {
                last.text.push_str(&span.text)
            }
            _ => self.inline.push(span),
        }
    }

    fn image(&mut self, element: &scraper::node::Element, context: &Context<'_>) {
        let is_pixel = ["width", "height"].iter().any(|side| {
            element
                .attr(side)
                .and_then(|v| v.trim_end_matches("px").parse::<u32>().ok())
                .is_some_and(|v| v <= 2)
        });
        let source = element
            .attr("src")
            .filter(|src| !src.starts_with("data:"))
            .or_else(|| element.attr("data-src"))
            .or_else(|| element.attr("srcset").and_then(|set| set.split_whitespace().next()));
        let Some(url) = source.and_then(|src| context.resolve(src)) else { return };
        if is_pixel {
            return;
        }
        let alt = element.attr("alt").unwrap_or_default().trim().to_owned();
        self.block(Block::Image { url, alt });
    }

    fn video(&mut self, node: NodeRef<'_, Node>, element: &scraper::node::Element, context: &Context<'_>) {
        let source = element.attr("src").or_else(|| {
            node.children().filter_map(|child| child.value().as_element()).find_map(|child| child.attr("src"))
        });
        if let Some(video) = source.and_then(|src| context.resolve(src)).and_then(|src| Video::from_url(&src))
        {
            self.block(Block::Video(video));
        }
    }

    fn block(&mut self, block: Block) {
        self.flush();
        self.blocks.push(block);
    }

    fn flush(&mut self) {
        let spans = tidy(std::mem::take(&mut self.inline));
        if !spans.is_empty() {
            self.blocks.push(Block::Paragraph(spans));
        }
    }
}

/// Trims whitespace at both ends of a run of spans, and around line
/// breaks, dropping spans left empty.
fn tidy(mut spans: Vec<Span>) -> Vec<Span> {
    for span in &mut spans {
        if span.text.contains('\n') {
            span.text = span.text.split('\n').map(str::trim).collect::<Vec<_>>().join("\n");
        }
    }
    while let Some(first) = spans.first_mut() {
        first.text = first.text.trim_start().to_owned();
        if first.text.is_empty() {
            spans.remove(0);
        } else {
            break;
        }
    }
    while let Some(last) = spans.last_mut() {
        last.text = last.text.trim_end().to_owned();
        if last.text.is_empty() {
            spans.pop();
        } else {
            break;
        }
    }
    spans
}

fn text_content(node: NodeRef<'_, Node>) -> String {
    node.descendants()
        .filter_map(|descendant| match descendant.value() {
            Node::Text(text) => Some(&**text),
            Node::Element(element) if element.name() == "br" => Some("\n"),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(html: &str) -> Vec<Block> {
        Document::from_html(html, Some("https://site.test/posts/one")).blocks
    }

    fn text(text: &str) -> Span {
        Span { text: text.into(), ..Span::default() }
    }

    fn styled(text: &str, marks: Marks) -> Span {
        Span { text: text.into(), marks, link: None }
    }

    fn para(spans: Vec<Span>) -> Block {
        Block::Paragraph(spans)
    }

    #[test]
    fn paragraphs_and_inline_styles() {
        let bold = Marks { bold: true, ..Marks::default() };
        let italic = Marks { italic: true, ..Marks::default() };
        assert_eq!(
            doc("<p>Plain <b>bold</b> and <em>soft</em>.</p>\n\n<p>Second</p>"),
            [
                para(vec![
                    text("Plain "),
                    styled("bold", bold),
                    text(" and "),
                    styled("soft", italic),
                    text(".")
                ]),
                para(vec![text("Second")]),
            ]
        );
    }

    #[test]
    fn whitespace_collapses_across_tags() {
        assert_eq!(doc("<p>  one \n <span> two </span>  three  </p>"), [para(vec![text("one two three")])]);
    }

    #[test]
    fn bare_text_and_line_breaks() {
        assert_eq!(doc("first line <br/>  second line"), [para(vec![text("first line\nsecond line")])]);
    }

    #[test]
    fn links_resolve_against_the_article() {
        let blocks = doc(
            r##"<p><a href="/about">About</a> <a href="#top">top</a> <a href="javascript:x()">x</a></p>"##,
        );
        let Block::Paragraph(spans) = &blocks[0] else { panic!() };
        assert_eq!(spans[0].link.as_deref(), Some("https://site.test/about"));
        assert_eq!(spans[1], text(" top x"));
    }

    #[test]
    fn headings_quotes_lists_code_and_rules() {
        let blocks = doc("<h2>Title</h2><blockquote><p>Quoted</p></blockquote>\
             <ol><li>One</li><li><p>Two</p><ul><li>Inner</li></ul></li></ol>\
             <pre><code>fn main() {\n    hi();\n}\n</code></pre><hr>");
        assert_eq!(
            blocks,
            [
                Block::Heading { level: 2, spans: vec![text("Title")] },
                Block::Quote(vec![para(vec![text("Quoted")])]),
                Block::List {
                    ordered: true,
                    items: vec![
                        vec![para(vec![text("One")])],
                        vec![
                            para(vec![text("Two")]),
                            Block::List { ordered: false, items: vec![vec![para(vec![text("Inner")])]] },
                        ],
                    ],
                },
                Block::Code("fn main() {\n    hi();\n}".into()),
                Block::Rule,
            ]
        );
    }

    #[test]
    fn images_become_blocks_and_pixels_disappear() {
        let blocks = doc(r#"<p>Before <img src="img/a.png" alt=" A "> after</p>
               <img src="https://t.test/p.gif" width="1" height="1">
               <img data-src="/lazy.jpg" src="data:image/gif;base64,R0">
               <figure><img src="https://cdn.test/b.jpg"><figcaption>Caption <i>here</i></figcaption></figure>"#);
        assert_eq!(
            blocks,
            [
                para(vec![text("Before")]),
                Block::Image { url: "https://site.test/posts/img/a.png".into(), alt: "A".into() },
                para(vec![text("after")]),
                Block::Image { url: "https://site.test/lazy.jpg".into(), alt: String::new() },
                Block::Image { url: "https://cdn.test/b.jpg".into(), alt: String::new() },
                Block::Caption(vec![
                    text("Caption "),
                    styled("here", Marks { italic: true, ..Marks::default() })
                ]),
            ]
        );
    }

    #[test]
    fn embeds_become_video_cards() {
        let blocks = doc(r#"<iframe src="https://www.youtube.com/embed/dQw4w9WgXcQ"></iframe>
               <iframe src="https://ads.test/frame"></iframe>
               <video><source src="https://player.vimeo.com/video/42"></video>"#);
        assert_eq!(
            blocks,
            [
                Block::Video(Video::from_url("https://youtu.be/dQw4w9WgXcQ").unwrap()),
                Block::Video(Video::from_url("https://vimeo.com/42").unwrap()),
            ]
        );
    }

    #[test]
    fn page_furniture_is_dropped() {
        assert_eq!(
            doc(
                "<nav>Menu</nav><script>x()</script><form><input></form><div hidden>no</div><div><p>Body</p></div>"
            ),
            [para(vec![text("Body")])]
        );
    }

    #[test]
    fn tables_read_row_by_row() {
        assert_eq!(
            doc("<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>"),
            [para(vec![text("A  B")]), para(vec![text("1  2")])]
        );
    }

    #[test]
    fn lead_video_for_video_pages() {
        let url = Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        let document = Document::from_text("Description").with_lead_video(url);
        assert!(matches!(document.blocks[0], Block::Video(_)));
        assert_eq!(document.blocks.len(), 2);

        let embedded = Document::from_html(r#"<iframe src="https://youtu.be/dQw4w9WgXcQ"></iframe>"#, None);
        assert_eq!(embedded.clone().with_lead_video(url), embedded);
        assert_eq!(Document::default().with_lead_video(Some("https://a.test/")), Document::default());
    }

    #[test]
    fn lead_image_when_the_body_has_none() {
        let image = Some("https://cdn.test/hero.jpg");
        let document = Document::from_text("Words").with_lead_image(image);
        assert_eq!(
            document.blocks[0],
            Block::Image { url: "https://cdn.test/hero.jpg".into(), alt: String::new() }
        );

        let illustrated = Document::from_html(r#"<p>Hi</p><img src="https://cdn.test/a.png">"#, None);
        assert_eq!(illustrated.clone().with_lead_image(image), illustrated);
        let plain = Document::from_text("Words");
        assert_eq!(plain.clone().with_lead_image(None), plain);
        assert_eq!(plain.clone().with_lead_image(Some("data:image/png;base64,x")), plain);
    }

    #[test]
    fn counts_words_and_minutes() {
        let words = vec!["word"; 461].join(" ");
        let document = Document::from_html(&format!("<p>{words}</p><ul><li>one more</li></ul>"), None);
        assert_eq!(document.word_count(), 463);
        assert_eq!(document.reading_minutes(), 3);
        assert_eq!(Document::default().reading_minutes(), 1);
    }

    #[test]
    fn plain_text_paragraphs() {
        assert_eq!(Document::from_text("a\n\n\n b ").blocks, [para(vec![text("a")]), para(vec![text("b")])]);
    }
}
