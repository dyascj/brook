//! The Add a feed dialog: paste anything, preview what's found, follow.

use brook_core::discover::{self, Discovered};
use gpui::prelude::*;
use gpui::{App, Context, Entity, FontWeight, SharedString, Subscription, Task, Window, div, px};
use gpui_component::WindowExt;
use gpui_component::input::{Input, InputEvent, InputState};

use crate::assets::icon;
use crate::components::{icon as draw_icon, spinner};
use crate::library::Library;
use crate::theme::palette;

enum Search {
    Idle,
    Looking(#[allow(dead_code)] Task<()>),
    Found(Vec<Discovered>),
    Failed(SharedString),
}

/// The dialog's content.
pub struct AddFeed {
    library: Entity<Library>,
    input: Entity<InputState>,
    search: Search,
    _subscription: Subscription,
}

/// Opens the dialog.
pub fn open(library: Entity<Library>, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| AddFeed::new(library, window, cx));
    form.read(cx).input.clone().update(cx, |input, cx| input.focus(window, cx));
    window.open_dialog(cx, move |dialog, _, cx| {
        let dialog = dialog.bg(crate::theme::palette(cx).surface);
        dialog.title("Add a feed").w(px(520.)).child(form.clone())
    });
}

impl AddFeed {
    fn new(library: Entity<Library>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("A website, a feed, or a YouTube channel"));
        let subscription = cx.subscribe(&input, |this, _, event: &InputEvent, cx| match event {
            InputEvent::PressEnter { .. } => this.find(cx),
            InputEvent::Change if matches!(this.search, Search::Failed(_)) => {
                this.search = Search::Idle;
                cx.notify();
            }
            _ => {}
        });
        Self { library, input, search: Search::Idle, _subscription: subscription }
    }

    fn find(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let client = self.library.read(cx).client().clone();
        let lookup = cx.background_spawn(async move { discover::discover(&client, &text) });
        let task = cx.spawn(async move |this, cx| {
            let result = lookup.await;
            this.update(cx, |this, cx| {
                this.search = match result {
                    Ok(found) => Search::Found(found),
                    Err(error) => Search::Failed(friendly(&error).into()),
                };
                cx.notify();
            })
            .ok();
        });
        self.search = Search::Looking(task);
        cx.notify();
    }

    fn result(&self, index: usize, found: &Discovered, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let followed = self.library.read(cx).is_subscribed(&found.url);
        let recent: Vec<String> =
            found.feed.entries.iter().take(3).map(|entry| entry.title.clone()).collect();
        let found = found.clone();

        div()
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(px(10.))
            .border_1()
            .border_color(p.stroke())
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text)
                                    .child(found.feed.title.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(p.faint())
                                    .child(found.url.clone()),
                            ),
                    )
                    .child(
                        div()
                            .id(("follow", index))
                            .flex_none()
                            .h(px(28.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .rounded(px(14.))
                            .text_size(px(12.5))
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
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.library.update(cx, |library, cx| {
                                                library.subscribe(&found, None, cx)
                                            });
                                            window.close_dialog(cx);
                                        }))
                                }
                            }),
                    ),
            )
            .when(!recent.is_empty(), |card| {
                card.child(div().flex().flex_col().gap(px(3.)).pt(px(4.)).children(recent.into_iter().map(
                    |title| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(12.5))
                            .text_color(p.muted())
                            .child(div().size(px(4.)).flex_none().rounded_full().bg(p.ink(0.25)))
                            .child(div().truncate().child(title))
                    },
                )))
            })
    }
}

/// Discovery errors in plain words.
fn friendly(error: &brook_core::Error) -> String {
    use brook_core::Error;
    match error {
        Error::InvalidUrl(_) => "That doesn't look like a web address.".into(),
        Error::NoFeedFound(_) => {
            "Couldn't find a feed there. Try the site's home page or its feed address.".into()
        }
        Error::Status { status: 404 } => "That page doesn't exist.".into(),
        Error::Status { status } => format!("The site answered with an error ({status})."),
        Error::Network(_) => "Couldn't reach that site. Check the address and your connection.".into(),
        other => other.to_string(),
    }
}

impl Render for AddFeed {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let looking = matches!(self.search, Search::Looking(_));
        let results = match &self.search {
            Search::Found(found) => {
                found.iter().enumerate().map(|(i, f)| self.result(i, f, cx).into_any_element()).collect()
            }
            _ => vec![],
        };

        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_1().child(Input::new(&self.input).h(px(34.))))
                    .child(
                        div()
                            .id("find")
                            .h(px(34.))
                            .px(px(14.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded(px(8.))
                            .bg(p.well())
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text)
                            .cursor_pointer()
                            .hover(move |b| b.bg(p.selection()))
                            .when(looking, |b| b.child(spinner(12., cx)))
                            .child("Find")
                            .on_click(cx.listener(|this, _, _, cx| this.find(cx))),
                    ),
            )
            .map(|form| match &self.search {
                Search::Idle => form.child(div().text_size(px(12.5)).text_color(p.faint()).child(
                    "Paste a link and press Return. Brook finds the feed and shows what it publishes.",
                )),
                Search::Looking(_) => {
                    form.child(div().text_size(px(12.5)).text_color(p.faint()).child("Looking…"))
                }
                Search::Failed(message) => form.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.5))
                        .text_color(p.muted())
                        .child(draw_icon(icon::WARNING, 14., p.muted()))
                        .child(message.clone()),
                ),
                Search::Found(_) => form.child(div().flex().flex_col().gap(px(10.)).children(results)),
            })
    }
}
