//! The article list: what's in the selected scope, newest first.

use std::ops::Range;

use brook_core::model::{ArticleId, ArticleSummary, Scope};
use chrono::Local;
use gpui::prelude::*;
use gpui::{
    App, ClipboardItem, Context, Entity, FocusHandle, FontWeight, ObjectFit, ScrollStrategy, SharedString,
    Subscription, UniformListScrollHandle, Window, div, img, px, uniform_list,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{ContextMenuExt, PopupMenuItem};

use crate::assets::icon;
use crate::components::{icon as draw_icon, icon_button, site_icon};
use crate::images::Images;
use crate::library::Library;
use crate::theme::palette;

/// The list's width.
pub const WIDTH: f32 = 360.;

/// Every row is this tall, so the list can virtualize.
const ROW_HEIGHT: f32 = 104.;

/// The article list view.
pub struct ArticleList {
    library: Entity<Library>,
    images: Entity<Images>,
    search: Entity<InputState>,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    shown: Option<ArticleId>,
    _subscriptions: Vec<Subscription>,
}

impl ArticleList {
    /// A list over `library`. `focus` is the reading area's focus handle,
    /// taken when a row is clicked so single-key shortcuts work.
    pub fn new(
        library: Entity<Library>,
        images: Entity<Images>,
        focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search").clean_on_escape());
        let subscriptions = vec![
            cx.observe(&library, |this, library, cx| {
                // Keep the open article in view as it changes.
                let open = library.read(cx).open.as_ref().map(|open| open.article.summary.id);
                if open != this.shown {
                    this.shown = open;
                    if let Some(index) = library.read(cx).selected_index() {
                        this.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
                    }
                }
                cx.notify();
            }),
            cx.observe(&images, |_, _, cx| cx.notify()),
            cx.subscribe_in(&search, window, |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let text = input.read(cx).value().to_string();
                    this.library.update(cx, |library, cx| library.set_search(&text, cx));
                }
                InputEvent::PressEnter { .. } => {
                    this.focus.focus(window, cx);
                    this.library.update(cx, |library, cx| library.step(0, cx));
                }
                _ => {}
            }),
        ];
        Self {
            library,
            images,
            search,
            scroll: UniformListScrollHandle::new(),
            focus,
            shown: None,
            _subscriptions: subscriptions,
        }
    }

    /// Moves the cursor into the search field.
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| search.focus(window, cx));
    }

    /// Clears the search. Returns whether there was one.
    pub fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.search.read(cx).value().is_empty() {
            return false;
        }
        self.search.update(cx, |search, cx| search.set_value("", window, cx));
        self.library.update(cx, |library, cx| library.set_search("", cx));
        true
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let library = self.library.read(cx);
        let title = library.scope_title();
        let unread = library.scope_unread();
        let scope = library.scope;
        let unread_only = library.settings.unread_only;
        let filterable = !matches!(scope, Scope::Starred | Scope::Saved);

        div()
            .flex()
            .flex_col()
            .flex_none()
            .px(px(14.))
            .pt(px(12.))
            .pb(px(8.))
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_baseline()
                            .gap(px(8.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text)
                                    .child(title),
                            )
                            .when(unread > 0, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .text_size(px(12.))
                                        .text_color(p.faint())
                                        .child(format!("{unread} unread")),
                                )
                            }),
                    )
                    .when(filterable, |row| {
                        row.child(icon_button(
                            "unread-only",
                            icon::FILTER,
                            if unread_only { "Show all" } else { "Show unread only" },
                            unread_only.then_some(p.text),
                            cx,
                            cx.listener(|this, _, _, cx| {
                                this.library.update(cx, |library, cx| {
                                    library.update_settings(|s| s.unread_only = !s.unread_only, cx)
                                })
                            }),
                        ))
                    })
                    .child(icon_button(
                        "mark-all-read",
                        icon::MARK_ALL_READ,
                        "Mark all read",
                        None,
                        cx,
                        cx.listener(move |this, _, _, cx| {
                            this.library.update(cx, |library, cx| library.mark_all_read(scope, cx))
                        }),
                    )),
            )
            .child(
                Input::new(&self.search)
                    .prefix(draw_icon(icon::SEARCH, 14., p.faint()))
                    .cleanable(true)
                    .h(px(30.)),
            )
    }

    fn rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let library = self.library.read(cx);
        let open = library.open.as_ref().map(|open| open.article.summary.id);
        let show_thumbnails = library.settings.show_thumbnails;
        let rows: Vec<(ArticleSummary, Option<brook_core::model::Feed>)> = range
            .filter_map(|index| library.articles.get(index))
            .map(|article| (article.clone(), library.feed(article.feed_id).cloned()))
            .collect();
        rows.into_iter()
            .map(|(article, feed)| self.row(article, feed, open, show_thumbnails, cx).into_any_element())
            .collect()
    }

    fn row(
        &mut self,
        article: ArticleSummary,
        feed: Option<brook_core::model::Feed>,
        open: Option<ArticleId>,
        show_thumbnails: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = *palette(cx);
        let selected = open == Some(article.id);
        let feed_title = feed.as_ref().map(|f| f.title.clone()).unwrap_or_default();
        let feed_icon =
            feed.as_ref().and_then(|feed| self.images.update(cx, |images, cx| images.feed_icon(feed, cx)));
        let thumbnail = article
            .image_url
            .as_deref()
            .filter(|_| show_thumbnails)
            .and_then(|url| self.images.update(cx, |images, cx| images.image(url, cx)));
        let id = article.id;
        let when = brook_core::time::short(article.published_at, Local::now());
        let library = self.library.clone();
        let (read, starred, saved) = (article.read, article.starred, article.saved);

        div().h(px(ROW_HEIGHT)).px(px(6.)).py(px(2.)).child(
            div()
                .id(("article", id.0 as u64))
                .size_full()
                .flex()
                .gap(px(12.))
                .px(px(12.))
                .py(px(11.))
                .rounded(px(8.))
                .cursor_pointer()
                .when(selected, |row| row.bg(p.selection()))
                .when(!selected, |row| row.hover(move |row| row.bg(p.hover())))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(11.5))
                                .text_color(p.faint())
                                .child(site_icon(feed_icon, &feed_title, 13., cx))
                                .child(div().flex_1().min_w_0().truncate().child(feed_title.clone()))
                                .when(starred, |meta| meta.child(draw_icon(icon::STAR_FILL, 11., p.ink(0.5))))
                                .when(saved, |meta| meta.child(draw_icon(icon::SAVED_FILL, 11., p.ink(0.5))))
                                .child(div().flex_none().child(when))
                                .when(!read, |meta| {
                                    meta.child(div().size(px(7.)).flex_none().rounded_full().bg(p.accent))
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(13.5))
                                .line_height(px(18.))
                                .font_weight(if read { FontWeight::NORMAL } else { FontWeight::MEDIUM })
                                .text_color(if read { p.ink(0.58) } else { p.text })
                                .line_clamp(2)
                                .child(article.title.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(p.faint())
                                .line_clamp(1)
                                .child(article.excerpt.clone()),
                        ),
                )
                .when_some(thumbnail, |row, path| {
                    row.child(
                        img(path)
                            .size(px(54.))
                            .flex_none()
                            .mt(px(18.))
                            .rounded(px(6.))
                            .object_fit(ObjectFit::Cover),
                    )
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.focus.focus(window, cx);
                    this.library.update(cx, |library, cx| library.open_article(id, cx));
                }))
                .context_menu(move |menu, _, _| {
                    let (a, b, c) = (library.clone(), library.clone(), library.clone());
                    let url_library = library.clone();
                    menu.item(
                        PopupMenuItem::new(if read { "Mark unread" } else { "Mark read" })
                            .icon(menu_icon(if read { icon::MARK_UNREAD } else { icon::MARK_READ }))
                            .on_click(move |_, _, cx| a.update(cx, |l, cx| l.toggle_read(id, cx))),
                    )
                    .item(
                        PopupMenuItem::new(if starred { "Unstar" } else { "Star" })
                            .icon(menu_icon(icon::STAR))
                            .on_click(move |_, _, cx| b.update(cx, |l, cx| l.toggle_starred(id, cx))),
                    )
                    .item(
                        PopupMenuItem::new(if saved { "Remove from Read later" } else { "Read later" })
                            .icon(menu_icon(icon::SAVED))
                            .on_click(move |_, _, cx| c.update(cx, |l, cx| l.toggle_saved(id, cx))),
                    )
                    .separator()
                    .item(PopupMenuItem::new("Open in browser").icon(menu_icon(icon::EXTERNAL)).on_click({
                        let library = url_library.clone();
                        move |_, _, cx| {
                            if let Some(url) = article_url(&library, id, cx) {
                                cx.open_url(&url);
                            }
                        }
                    }))
                    .item(
                        PopupMenuItem::new("Copy link").icon(menu_icon(icon::LINK)).on_click({
                            let library = url_library.clone();
                            move |_, _, cx| {
                                if let Some(url) = article_url(&library, id, cx) {
                                    cx.write_to_clipboard(ClipboardItem::new_string(url));
                                }
                            }
                        }),
                    )
                }),
        )
    }

    fn empty_state(&self, cx: &App) -> impl IntoElement {
        let p = *palette(cx);
        let library = self.library.read(cx);
        let (title, body): (&str, String) = if !library.search.trim().is_empty() {
            ("No matches", format!("Nothing here mentions “{}”.", library.search.trim()))
        } else if library.feeds.is_empty() {
            ("Nothing to read yet", "Add a feed, or browse Discover for ideas.".into())
        } else {
            match library.scope {
                Scope::Starred => ("No stars yet", "Star an article to keep it here.".into()),
                Scope::Saved => ("Nothing saved", "Save an article to read it later.".into()),
                Scope::Today => ("All quiet today", "New articles from today land here.".into()),
                _ if library.settings.unread_only => ("All caught up", "You've read everything here.".into()),
                _ => ("Nothing here", "This feed hasn't published anything yet.".into()),
            }
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .px(px(32.))
            .pb(px(48.))
            .text_center()
            .child(
                div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(p.ink(0.75)).child(title),
            )
            .child(div().text_size(px(12.5)).text_color(p.faint()).child(SharedString::from(body)))
    }
}

impl Render for ArticleList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.library.read(cx).articles.len();
        div().w(px(WIDTH)).h_full().flex().flex_col().flex_none().child(self.header(cx)).map(|list| {
            if count == 0 {
                list.child(self.empty_state(cx))
            } else {
                list.child(
                    uniform_list("articles", count, cx.processor(|this, range, _, cx| this.rows(range, cx)))
                        .track_scroll(&self.scroll)
                        .flex_1()
                        .pb(px(6.)),
                )
            }
        })
    }
}

fn article_url(library: &Entity<Library>, id: ArticleId, cx: &App) -> Option<String> {
    let library = library.read(cx);
    if let Some(open) = library.open.as_ref().filter(|open| open.article.summary.id == id) {
        return open.article.url.clone();
    }
    // Rows don't carry the address; the store does.
    library.article_url(id)
}

fn menu_icon(path: &'static str) -> gpui_component::Icon {
    gpui_component::Icon::default().path(path)
}
