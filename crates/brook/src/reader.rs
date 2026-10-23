//! The reader: one article in a calm column.

use brook_core::model::ArticleId;
use chrono::Local;
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, FontWeight, ScrollHandle, SharedString, Window, div, point, px, relative, svg,
};
use gpui_component::ActiveTheme;

use crate::actions::{OpenInBrowser, ToggleRead, ToggleReaderMode, ToggleSaved, ToggleStarred};
use crate::assets::{MARK, icon};
use crate::components::{icon as draw_icon, icon_button, site_icon, spinner};
use crate::images::Images;
use crate::library::{Library, ReaderMode};
use crate::theme::palette;
use crate::typeset::{self, Typography};

/// The column's widest measure.
const MEASURE: f32 = 680.;

/// The reader view.
pub struct Reader {
    library: Entity<Library>,
    images: Entity<Images>,
    scroll: ScrollHandle,
    shown: Option<ArticleId>,
}

impl Reader {
    /// A reader over `library`.
    pub fn new(library: Entity<Library>, images: Entity<Images>, cx: &mut Context<Self>) -> Self {
        cx.observe(&library, |this, library, cx| {
            let open = library.read(cx).open.as_ref().map(|open| open.article.summary.id);
            if open != this.shown {
                this.shown = open;
                this.scroll.set_offset(point(px(0.), px(0.)));
            }
            cx.notify();
        })
        .detach();
        cx.observe(&images, |_, _, cx| cx.notify()).detach();
        Self { library, images, scroll: ScrollHandle::new(), shown: None }
    }

    /// Scrolls by a page, for the space bar.
    pub fn page(&self, down: bool) {
        let height = self.scroll.bounds().size.height;
        let offset = self.scroll.offset();
        let step = height * 0.85;
        self.scroll.set_offset(point(offset.x, if down { offset.y - step } else { offset.y + step }));
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let library = self.library.read(cx);
        let Some(open) = &library.open else { return div() };
        let summary = &open.article.summary;
        let has_url = open.article.url.is_some();
        let mode = open.mode.clone();

        let dispatch = |action: Box<dyn gpui::Action>| {
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                window.dispatch_action(action.boxed_clone(), cx)
            }
        };
        div()
            .h(px(44.))
            .flex_none()
            .px(px(10.))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(2.))
            .child(icon_button(
                "toggle-read",
                if summary.read { icon::MARK_UNREAD } else { icon::MARK_READ },
                if summary.read { "Mark unread  M" } else { "Mark read  M" },
                None,
                cx,
                dispatch(Box::new(ToggleRead)),
            ))
            .child(icon_button(
                "toggle-star",
                if summary.starred { icon::STAR_FILL } else { icon::STAR },
                if summary.starred { "Unstar  S" } else { "Star  S" },
                summary.starred.then(rgb_star),
                cx,
                dispatch(Box::new(ToggleStarred)),
            ))
            .child(icon_button(
                "toggle-saved",
                if summary.saved { icon::SAVED_FILL } else { icon::SAVED },
                if summary.saved { "Remove from Read later  L" } else { "Read later  L" },
                summary.saved.then_some(p.accent),
                cx,
                dispatch(Box::new(ToggleSaved)),
            ))
            .when(has_url, |bar| {
                bar.child(icon_button(
                    "reader-mode",
                    icon::READER,
                    "Reader mode  V",
                    (mode == ReaderMode::On).then_some(p.text),
                    cx,
                    dispatch(Box::new(ToggleReaderMode)),
                ))
                .child(icon_button(
                    "open-in-browser",
                    icon::EXTERNAL,
                    "Open in browser  O",
                    None,
                    cx,
                    dispatch(Box::new(OpenInBrowser)),
                ))
            })
    }

    fn article(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let library = self.library.read(cx);
        let open = library.open.clone().expect("only called with an open article");
        let feed = library.feed(open.article.summary.feed_id).cloned();
        let size = library.settings.text_size.pixels();
        let summary = &open.article.summary;
        let feed_title = feed.as_ref().map(|f| f.title.clone()).unwrap_or_default();
        let feed_icon =
            feed.as_ref().and_then(|feed| self.images.update(cx, |images, cx| images.feed_icon(feed, cx)));
        let date = brook_core::time::long(summary.published_at, &Local);
        let minutes = open.document.reading_minutes();
        let byline = match &open.article.author {
            Some(author) => format!("{author}  ·  {minutes} min read"),
            None => format!("{minutes} min read"),
        };
        let typography = Typography {
            size,
            palette: p,
            sans: cx.theme().font_family.clone(),
            mono: cx.theme().mono_font_family.clone(),
        };
        let mut ids = 0;
        let body = typeset::blocks(&open.document.blocks, &typography, &self.images, &mut ids, cx);
        let url = open.article.url.clone();
        let _ = window;

        div().id("reader-scroll").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.scroll).child(
            div()
                .mx_auto()
                .max_w(px(MEASURE + 64.))
                .px(px(32.))
                .pt(px(12.))
                .pb(px(72.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.5))
                        .text_color(p.muted())
                        .child(site_icon(feed_icon, &feed_title, 16., cx))
                        .child(div().truncate().child(feed_title))
                        .child(div().text_color(p.faint()).child("·"))
                        .child(div().flex_none().child(date)),
                )
                .child(
                    div()
                        .mt(px(14.))
                        .text_size(px(size * 1.65))
                        .line_height(relative(1.22))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.text)
                        .child(summary.title.clone()),
                )
                .child(div().mt(px(10.)).text_size(px(13.)).text_color(p.faint()).child(byline))
                .child(self.mode_notice(&open.mode, cx))
                .child(
                    div()
                        .mt(px(28.))
                        .flex()
                        .flex_col()
                        .gap(px(size * 0.95))
                        .text_size(px(size))
                        .line_height(relative(1.68))
                        .text_color(p.text)
                        .children(body),
                )
                .when_some(url, |column, url| {
                    column.child(
                        div()
                            .id("open-original")
                            .mt(px(40.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(13.))
                            .text_color(p.muted())
                            .cursor_pointer()
                            .hover(move |link| link.text_color(p.text))
                            .child("Read on the web")
                            .child(svg().path(icon::EXTERNAL).size(px(13.)))
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    )
                }),
        )
    }

    fn mode_notice(&self, mode: &ReaderMode, cx: &App) -> impl IntoElement {
        let p = *palette(cx);
        let notice =
            div().mt(px(18.)).flex().items_center().gap(px(8.)).text_size(px(12.5)).text_color(p.muted());
        match mode {
            ReaderMode::Off => notice,
            ReaderMode::Loading => notice.child(spinner(12., cx)).child("Fetching the full article…"),
            ReaderMode::On => notice
                .child(draw_icon(icon::READER, 14., p.faint()))
                .child("Reader mode: the full article from its web page."),
            ReaderMode::Failed(error) => notice
                .child(draw_icon(icon::WARNING, 14., p.faint()))
                .child(SharedString::from(format!("Reader mode didn't work here: {error}"))),
        }
    }

    fn placeholder(&self, cx: &App) -> impl IntoElement {
        let p = *palette(cx);
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(14.))
            .pb(px(44.))
            .child(svg().path(MARK).size(px(44.)).text_color(p.ink(0.14)))
            .child(div().text_size(px(13.)).text_color(p.faint()).child("Pick something to read"))
            .child(div().text_size(px(11.5)).text_color(p.ink(0.3)).child("J and K move through the list"))
    }
}

/// The star's gold.
fn rgb_star() -> gpui::Hsla {
    gpui::rgb(0xf2b53b).into()
}

impl Render for Reader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_article = self.library.read(cx).open.is_some();
        div().flex_1().min_w_0().h_full().flex().flex_col().map(|reader| {
            if has_article {
                reader.child(self.toolbar(cx)).child(self.article(window, cx))
            } else {
                reader.child(self.placeholder(cx))
            }
        })
    }
}
