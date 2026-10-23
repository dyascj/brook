//! Discover: a hand-picked catalog of feeds, searchable, one click to
//! follow.

use brook_core::catalog::{self, CatalogFeed, Category};
use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, SharedString, Subscription, Window, div, px};
use gpui_component::input::{Input, InputEvent, InputState};

use crate::actions::AddFeed;
use crate::assets::icon;
use crate::components::{icon as draw_icon, site_icon};
use crate::images::Images;
use crate::library::Library;
use crate::theme::palette;

/// The Discover page.
pub struct Discover {
    library: Entity<Library>,
    images: Entity<Images>,
    search: Entity<InputState>,
    query: String,
    _subscriptions: Vec<Subscription>,
}

impl Discover {
    /// A catalog page subscribing through `library`.
    pub fn new(
        library: Entity<Library>,
        images: Entity<Images>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search the catalog").clean_on_escape());
        let subscriptions = vec![
            cx.observe(&library, |_, _, cx| cx.notify()),
            cx.observe(&images, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    this.query = input.read(cx).value().to_string();
                    cx.notify();
                }
            }),
        ];
        Self { library, images, search, query: String::new(), _subscriptions: subscriptions }
    }

    fn card(&self, category: &Category, feed: &CatalogFeed, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let followed = self.library.read(cx).is_subscribed(&feed.url);
        let site = feed.site_url.clone().unwrap_or_else(|| feed.url.clone());
        let icon_path = self.images.update(cx, |images, cx| images.icon(&site, cx));
        let (url, name, site_url) = (feed.url.clone(), feed.name.clone(), feed.site_url.clone());

        div()
            .id(SharedString::from(format!("{}:{}", category.id, feed.url)))
            .w(px(252.))
            .h(px(132.))
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(px(10.))
            .border_1()
            .border_color(p.stroke())
            .bg(p.surface)
            .hover(move |card| card.border_color(p.ink(0.14)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(site_icon(icon_path, &feed.name, 22., cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child(feed.name.clone()),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(p.muted())
                    .line_clamp(2)
                    .child(feed.description.clone()),
            )
            .child(
                div().flex().justify_end().child(
                    div()
                        .id(SharedString::from(format!("follow:{}", feed.url)))
                        .h(px(26.))
                        .px(px(11.))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .rounded(px(13.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .map(|button| {
                            if followed {
                                button
                                    .text_color(p.muted())
                                    .child(draw_icon(icon::CHECK, 13., p.muted()))
                                    .child("Following")
                            } else {
                                button
                                    .cursor_pointer()
                                    .bg(p.accent)
                                    .text_color(gpui::white())
                                    .hover(|b| b.opacity(0.9))
                                    .child("Follow")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let (url, name, site_url) =
                                            (url.clone(), name.clone(), site_url.clone());
                                        this.library.update(cx, |library, cx| {
                                            library.subscribe_url(&url, &name, site_url.as_deref(), cx)
                                        });
                                    }))
                            }
                        }),
                ),
            )
    }
}

impl Render for Discover {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let query = self.query.trim().to_owned();

        let sections: Vec<(&'static Category, Vec<&'static CatalogFeed>)> = if query.is_empty() {
            catalog::categories().iter().map(|category| (category, category.feeds.iter().collect())).collect()
        } else {
            let mut grouped: Vec<(&'static Category, Vec<&'static CatalogFeed>)> = Vec::new();
            for (category, feed) in catalog::search(&query) {
                match grouped.iter_mut().find(|(c, _)| c.id == category.id) {
                    Some((_, feeds)) => feeds.push(feed),
                    None => grouped.push((category, vec![feed])),
                }
            }
            grouped
        };
        let empty = sections.is_empty();

        div().id("discover").size_full().overflow_y_scroll().child(
            div()
                .max_w(px(1100.))
                .mx_auto()
                .px(px(36.))
                .pt(px(32.))
                .pb(px(64.))
                .flex()
                .flex_col()
                .gap(px(28.))
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap(px(24.))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(24.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(p.text)
                                        .child("Discover"),
                                )
                                .child(div().text_size(px(13.)).text_color(p.muted()).child(
                                    "A few good places to start. Follow one, and it shows up in the sidebar.",
                                )),
                        )
                        .child(
                            div().w(px(260.)).flex_none().child(
                                Input::new(&self.search)
                                    .prefix(draw_icon(icon::SEARCH, 14., p.faint()))
                                    .cleanable(true)
                                    .h(px(32.)),
                            ),
                        ),
                )
                .when(empty, |page| {
                    page.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(13.))
                            .text_color(p.muted())
                            .child(format!("Nothing in the catalog matches “{query}”."))
                            .child(
                                div()
                                    .id("add-instead")
                                    .text_color(p.accent)
                                    .cursor_pointer()
                                    .child("Add a feed by address")
                                    .on_click(|_, window, cx| window.dispatch_action(Box::new(AddFeed), cx)),
                            ),
                    )
                })
                .children(sections.into_iter().map(|(category, feeds)| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(p.text)
                                        .child(category.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(p.faint())
                                        .child(category.description.clone()),
                                ),
                        )
                        .child({
                            let mut grid = div().flex().flex_wrap().gap(px(12.));
                            for feed in feeds {
                                grid = grid.child(self.card(category, feed, cx));
                            }
                            grid
                        })
                })),
        )
    }
}
