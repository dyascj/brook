//! Turning a [`Document`](brook_core::document::Document) into elements.
//!
//! Text is set in one column with generous leading. Inline styles become
//! text runs; links become clickable ranges that open in the browser.

use brook_core::document::{Block, Marks, Span};
use brook_core::video::Video;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Entity, FontStyle, FontWeight, InteractiveText, ObjectFit, SharedString,
    StrikethroughStyle, StyledText, TextRun, UnderlineStyle, div, font, img, px, relative,
};

use crate::assets::icon;
use crate::components::icon as draw_icon;
use crate::images::Images;
use crate::theme::Palette;

/// How a document is set.
#[derive(Clone, Debug)]
pub struct Typography {
    /// Body text size in pixels.
    pub size: f32,
    /// Colors.
    pub palette: Palette,
    /// The text face.
    pub sans: SharedString,
    /// The code face.
    pub mono: SharedString,
}

/// Lays out `blocks`. `ids` numbers interactive elements so each gets a
/// stable, unique id within the document.
pub fn blocks(
    blocks: &[Block],
    typography: &Typography,
    images: &Entity<Images>,
    ids: &mut usize,
    cx: &mut App,
) -> Vec<AnyElement> {
    blocks.iter().filter_map(|block| self::block(block, typography, images, ids, cx)).collect()
}

fn block(
    block: &Block,
    t: &Typography,
    images: &Entity<Images>,
    ids: &mut usize,
    cx: &mut App,
) -> Option<AnyElement> {
    let p = t.palette;
    let element = match block {
        Block::Paragraph(spans) => div().child(text(spans, t, Base::Body, ids)).into_any_element(),
        Block::Heading { level, spans } => {
            let scale = match level {
                1 | 2 => 1.4,
                3 => 1.2,
                _ => 1.06,
            };
            div()
                .pt(px(t.size * 0.6))
                .text_size(px(t.size * scale))
                .line_height(relative(1.3))
                .child(text(spans, t, Base::Heading, ids))
                .into_any_element()
        }
        Block::Quote(inner) => div()
            .flex()
            .flex_col()
            .gap(px(t.size * 0.75))
            .pl(px(16.))
            .border_l(px(2.))
            .border_color(p.ink(0.16))
            .text_color(p.ink(0.75))
            .children(blocks(inner, t, images, ids, cx))
            .into_any_element(),
        Block::List { ordered, items } => div()
            .flex()
            .flex_col()
            .gap(px(t.size * 0.4))
            .children(items.iter().enumerate().map(|(index, item)| {
                let marker = if *ordered { format!("{}.", index + 1) } else { "•".to_owned() };
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().w(px(20.)).flex_none().text_right().text_color(p.faint()).child(marker))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(t.size * 0.4))
                            .children(blocks(item, t, images, ids, cx)),
                    )
            }))
            .into_any_element(),
        Block::Code(code) => {
            *ids += 1;
            div()
                .id(("code", *ids))
                .overflow_x_scroll()
                .rounded(px(8.))
                .bg(p.well())
                .px(px(14.))
                .py(px(12.))
                .font_family(t.mono.clone())
                .text_size(px(t.size * 0.8))
                .line_height(relative(1.55))
                .text_color(p.ink(0.85))
                .child(div().whitespace_nowrap().child(SharedString::from(code.clone())))
                .into_any_element()
        }
        Block::Image { url, alt } => {
            let path = images.update(cx, |images, cx| images.image(url, cx));
            match path {
                Some(path) => div()
                    .flex()
                    .justify_center()
                    .child(img(path).max_w_full().rounded(px(8.)).object_fit(ObjectFit::Contain))
                    .into_any_element(),
                None if images.read(cx).is_missing(url) => return None,
                None => div()
                    .h(px(180.))
                    .w_full()
                    .rounded(px(8.))
                    .bg(p.well())
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(p.faint())
                    .child(SharedString::from(alt.clone()))
                    .into_any_element(),
            }
        }
        Block::Caption(spans) => div()
            .mt(px(-t.size * 0.4))
            .text_size(px(t.size * 0.78))
            .text_color(p.muted())
            .text_center()
            .child(text(spans, t, Base::Body, ids))
            .into_any_element(),
        Block::Video(video) => video_card(video, t, images, ids, cx),
        Block::Rule => {
            div().my(px(t.size * 0.5)).h(px(1.)).w(px(64.)).mx_auto().bg(p.stroke()).into_any_element()
        }
    };
    Some(element)
}

/// The face a run of text starts from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Base {
    Body,
    Heading,
}

/// Styled, clickable text for a run of spans.
fn text(spans: &[Span], t: &Typography, base: Base, ids: &mut usize) -> AnyElement {
    let p = t.palette;
    let content: String = Span::plain(spans);
    let mut runs = Vec::with_capacity(spans.len());
    let mut links: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut offset = 0;
    for span in spans {
        let Marks { bold, italic, code, strike } = span.marks;
        let mut face = font(if code { t.mono.clone() } else { t.sans.clone() });
        face.weight = if bold || base == Base::Heading { FontWeight::SEMIBOLD } else { FontWeight::NORMAL };
        face.style = if italic { FontStyle::Italic } else { FontStyle::Normal };
        let len = span.text.len();
        if let Some(url) = &span.link {
            links.push((offset..offset + len, url.clone()));
        }
        runs.push(TextRun {
            len,
            font: face,
            color: if span.link.is_some() { p.accent } else { p.text },
            background_color: code.then(|| p.well()),
            underline: span.link.as_ref().map(|_| UnderlineStyle {
                thickness: px(1.),
                color: Some(p.accent.opacity(0.35)),
                wavy: false,
            }),
            strikethrough: strike.then(|| StrikethroughStyle { thickness: px(1.), color: Some(p.muted()) }),
        });
        offset += len;
    }
    let styled = StyledText::new(content).with_runs(runs);
    if links.is_empty() {
        return styled.into_any_element();
    }
    *ids += 1;
    let (ranges, urls): (Vec<_>, Vec<_>) = links.into_iter().unzip();
    InteractiveText::new(("text", *ids), styled)
        .on_click(ranges, move |index, _, cx| cx.open_url(&urls[index]))
        .into_any_element()
}

fn video_card(
    video: &Video,
    t: &Typography,
    images: &Entity<Images>,
    ids: &mut usize,
    cx: &mut App,
) -> AnyElement {
    let p = t.palette;
    let thumbnail =
        video.thumbnail_url().and_then(|url| images.update(cx, |images, cx| images.image(&url, cx)));
    let watch = video.watch_url();
    *ids += 1;
    div()
        .id(("video", *ids))
        .w_full()
        .rounded(px(10.))
        .overflow_hidden()
        .border_1()
        .border_color(p.stroke())
        .cursor_pointer()
        .hover(move |card| card.border_color(p.ink(0.16)))
        .child(
            div()
                .relative()
                .w_full()
                .aspect_ratio(16. / 9.)
                .bg(p.well())
                .when_some(thumbnail, |frame, path| {
                    frame.child(img(path).absolute().inset_0().size_full().object_fit(ObjectFit::Cover))
                })
                .child(
                    div().absolute().inset_0().flex().items_center().justify_center().child(
                        div()
                            .size(px(56.))
                            .rounded_full()
                            .bg(gpui::black().opacity(0.55))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(draw_icon(icon::PLAY, 22., gpui::white())),
                    ),
                ),
        )
        .child(
            div()
                .px(px(14.))
                .py(px(10.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.5))
                .text_color(p.muted())
                .child(format!("Watch on {}", video.provider.name()))
                .child(draw_icon(icon::EXTERNAL, 13., p.faint())),
        )
        .on_click(move |_, _, cx| cx.open_url(&watch))
        .into_any_element()
}
