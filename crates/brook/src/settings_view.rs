//! Settings: appearance, reading, refreshing, your library, and updates.

use brook_core::settings::{Appearance, REFRESH_CHOICES, TextSize};
use brook_core::update;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, FontWeight, SharedString, Window, div, px, svg};
use gpui_component::WindowExt;
use gpui_component::switch::Switch;

use crate::actions;
use crate::assets::{MARK, icon};
use crate::components::{hairline, icon as draw_icon, segmented, spinner};
use crate::library::Library;
use crate::theme::palette;
use crate::updater::{Status, Updater};

/// The Settings dialog's content.
pub struct SettingsView {
    library: Entity<Library>,
    updater: Entity<Updater>,
}

/// Opens Settings.
pub fn open(library: Entity<Library>, updater: Entity<Updater>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| {
        cx.observe(&library, |_, _, cx| cx.notify()).detach();
        cx.observe(&updater, |_, _, cx| cx.notify()).detach();
        SettingsView { library, updater }
    });
    window.open_dialog(cx, move |dialog, _, cx| {
        dialog.bg(crate::theme::palette(cx).surface).title("Settings").w(px(560.)).child(view.clone())
    });
}

fn refresh_label(minutes: u32) -> &'static str {
    match minutes {
        0 => "Manual",
        15 => "15 min",
        30 => "30 min",
        60 => "Hourly",
        180 => "3 hours",
        _ => "Custom",
    }
}

impl SettingsView {
    fn section(&self, title: &'static str, cx: &App) -> gpui::Div {
        div().flex().flex_col().gap(px(2.)).child(
            div()
                .pb(px(6.))
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette(cx).faint())
                .child(title),
        )
    }

    fn row(
        &self,
        label: &'static str,
        hint: Option<&'static str>,
        control: AnyElement,
        cx: &App,
    ) -> impl IntoElement {
        let p = *palette(cx);
        div()
            .min_h(px(36.))
            .flex()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(13.)).text_color(p.text).child(label))
                    .when_some(hint, |label, hint| {
                        label.child(div().text_size(px(11.5)).text_color(p.faint()).child(hint))
                    }),
            )
            .child(control)
    }

    fn switch(
        &self,
        id: &'static str,
        on: bool,
        change: fn(&mut brook_core::settings::Settings, bool),
    ) -> AnyElement {
        let library = self.library.clone();
        Switch::new(id)
            .checked(on)
            .on_click(move |checked, _, cx| {
                let checked = *checked;
                library.update(cx, |library, cx| library.update_settings(|s| change(s, checked), cx));
            })
            .into_any_element()
    }

    fn button(
        &self,
        id: &'static str,
        label: impl Into<SharedString>,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let p = *palette(cx);
        div()
            .id(id)
            .h(px(28.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(7.))
            .bg(p.well())
            .text_size(px(12.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(p.text)
            .cursor_pointer()
            .hover(move |b| b.bg(p.selection()))
            .child(label.into())
    }

    fn updates(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let status = self.updater.read(cx).status.clone();
        let updater = self.updater.clone();
        let line: AnyElement = match &status {
            Status::Idle => div().child(format!("Brook {}", update::CURRENT)).into_any_element(),
            Status::Checking => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(spinner(11., cx))
                .child("Checking…")
                .into_any_element(),
            Status::Available(release) => {
                div().child(format!("Brook {} is ready.", release.version)).into_any_element()
            }
            Status::Installing(release) => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(spinner(11., cx))
                .child(format!("Installing {}…", release.version))
                .into_any_element(),
            Status::Failed(_, error) => div().child(format!("The update failed: {error}")).into_any_element(),
        };
        let action = match status {
            Status::Available(_) | Status::Failed(..) => Some(
                self.button("install-update", "Update and restart", cx)
                    .on_click(move |_, _, cx| updater.update(cx, |updater, cx| updater.install(cx))),
            ),
            Status::Idle => Some(
                self.button("check-updates", "Check now", cx)
                    .on_click(move |_, _, cx| updater.update(cx, |updater, cx| updater.check(true, cx))),
            ),
            Status::Checking | Status::Installing(_) => None,
        };
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(svg().path(MARK).size(px(28.)).text_color(p.accent))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(13.)).text_color(p.text).child(line))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(p.faint())
                            .child("Signed builds from GitHub. Your library never leaves this computer."),
                    ),
            )
            .children(action)
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let settings = self.library.read(cx).settings.clone();
        let library = self.library.clone();
        let (appearance_library, size_library, refresh_library) =
            (library.clone(), library.clone(), library.clone());
        let (import_library, export_library) = (library.clone(), library.clone());
        let refresh_options: Vec<(u32, &'static str)> =
            REFRESH_CHOICES.iter().map(|&minutes| (minutes, refresh_label(minutes))).collect();
        let modifier = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" };

        div()
            .id("settings")
            .max_h(px(560.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(18.))
            .pr(px(4.))
            .child(
                self.section("Appearance", cx)
                    .child(
                        self.row(
                            "Theme",
                            None,
                            segmented(
                                "appearance",
                                &[
                                    (Appearance::System, "System"),
                                    (Appearance::Light, "Light"),
                                    (Appearance::Dark, "Dark"),
                                ],
                                settings.appearance,
                                cx,
                                move |appearance, window, cx| {
                                    appearance_library.update(cx, |library, cx| {
                                        library.update_settings(|s| s.appearance = appearance, cx)
                                    });
                                    crate::theme::apply(appearance, window, cx);
                                    window.refresh();
                                },
                            )
                            .into_any_element(),
                            cx,
                        ),
                    )
                    .child(
                        self.row(
                            "Reading size",
                            None,
                            segmented(
                                "text-size",
                                &[
                                    (TextSize::Small, "Small"),
                                    (TextSize::Medium, "Medium"),
                                    (TextSize::Large, "Large"),
                                ],
                                settings.text_size,
                                cx,
                                move |size, _, cx| {
                                    size_library.update(cx, |library, cx| {
                                        library.update_settings(|s| s.text_size = size, cx)
                                    })
                                },
                            )
                            .into_any_element(),
                            cx,
                        ),
                    )
                    .child(self.row(
                        "Images in the list",
                        None,
                        self.switch("thumbnails", settings.show_thumbnails, |s, on| s.show_thumbnails = on),
                        cx,
                    )),
            )
            .child(hairline(cx))
            .child(
                self.section("Reading", cx)
                    .child(self.row(
                        "Mark read when opened",
                        Some("Otherwise press M to mark an article read."),
                        self.switch("mark-read", settings.mark_read_on_open, |s, on| {
                            s.mark_read_on_open = on
                        }),
                        cx,
                    ))
                    .child(
                        self.row(
                            "Check feeds",
                            None,
                            segmented(
                                "refresh",
                                &refresh_options,
                                settings.refresh_minutes,
                                cx,
                                move |minutes, _, cx| {
                                    refresh_library.update(cx, |library, cx| {
                                        library.update_settings(|s| s.refresh_minutes = minutes, cx)
                                    })
                                },
                            )
                            .into_any_element(),
                            cx,
                        ),
                    ),
            )
            .child(hairline(cx))
            .child(
                self.section("Library", cx).child(
                    self.row(
                        "Subscriptions",
                        Some("OPML moves your feeds to and from other readers."),
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(
                                self.button("import", "Import…", cx)
                                    .child(draw_icon(icon::IMPORT, 13., p.muted()))
                                    .on_click(move |_, _, cx| {
                                        crate::workspace::import_opml(import_library.clone(), cx)
                                    }),
                            )
                            .child(
                                self.button("export", "Export…", cx)
                                    .child(draw_icon(icon::EXPORT, 13., p.muted()))
                                    .on_click(move |_, _, cx| {
                                        crate::workspace::export_opml(export_library.clone(), cx)
                                    }),
                            )
                            .into_any_element(),
                        cx,
                    ),
                ),
            )
            .child(hairline(cx))
            .child(
                self.section("Updates", cx)
                    .child(self.row(
                        "Check for updates automatically",
                        Some("Once a day."),
                        self.switch("auto-update", settings.check_for_updates, |s, on| {
                            s.check_for_updates = on
                        }),
                        cx,
                    ))
                    .child(div().pt(px(6.)).child(self.updates(cx))),
            )
            .child(hairline(cx))
            .child(self.section("Shortcuts", cx).child(div().flex().flex_wrap().children(
                actions::CHEAT_SHEET.iter().map(|(keys, what)| {
                    let keys = keys.replace("Cmd", modifier);
                    div()
                        .w_1_2()
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .w(px(64.))
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.muted())
                                .child(keys),
                        )
                        .child(div().text_size(px(12.5)).text_color(p.text).child(*what))
                }),
            )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refresh_choice_has_a_label() {
        for &minutes in REFRESH_CHOICES {
            assert_ne!(refresh_label(minutes), "Custom", "{minutes}");
        }
    }
}
