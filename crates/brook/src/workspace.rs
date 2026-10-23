//! The window's root: a title bar and the sidebar on the glass, and the
//! workspace floating above them in a rounded card.

use brook_core::model::Scope;
use brook_core::settings::Appearance;
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, FontWeight, PathPromptOptions, Subscription, Window, div,
    px, svg,
};
use gpui_component::notification::Notification;
use gpui_component::{TitleBar, WindowExt};

use crate::actions::*;
use crate::article_list::ArticleList;
use crate::assets::{MARK, icon};
use crate::components::{icon_button, spinner};
use crate::discover::Discover;
use crate::images::Images;
use crate::library::{Library, Notice, Page};
use crate::reader::Reader;
use crate::sidebar::Sidebar;
use crate::theme::{self, palette};
use crate::updater::{Status, Updater};
use crate::{add_feed, settings_view};

/// The main window's content.
pub struct Workspace {
    library: Entity<Library>,
    updater: Entity<Updater>,
    sidebar: Entity<Sidebar>,
    list: Entity<ArticleList>,
    reader: Entity<Reader>,
    discover: Entity<Discover>,
    /// The reading area: the list and reader, where single keys work.
    reading: FocusHandle,
    /// The whole window, for when the reading area isn't on screen.
    focus: FocusHandle,
    sidebar_open: bool,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    /// Builds the window's views over `library`.
    pub fn new(
        library: Entity<Library>,
        images: Entity<Images>,
        updater: Entity<Updater>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let reading = cx.focus_handle();
        let focus = cx.focus_handle();
        let sidebar = cx.new(|cx| Sidebar::new(library.clone(), images.clone(), cx));
        let list =
            cx.new(|cx| ArticleList::new(library.clone(), images.clone(), reading.clone(), window, cx));
        let reader = cx.new(|cx| Reader::new(library.clone(), images.clone(), cx));
        let discover = cx.new(|cx| Discover::new(library.clone(), images.clone(), window, cx));

        let subscriptions = vec![
            cx.observe_in(&library, window, |this, _, window, cx| {
                this.place_focus(window, cx);
                cx.notify();
            }),
            cx.observe(&updater, |_, _, cx| cx.notify()),
            cx.subscribe_in(&library, window, |_, _, notice: &Notice, window, cx| show(notice, window, cx)),
            cx.subscribe_in(&updater, window, |_, _, notice: &Notice, window, cx| show(notice, window, cx)),
            cx.observe_window_appearance(window, |this, window, cx| {
                let appearance = this.library.read(cx).settings.appearance;
                if appearance == Appearance::System {
                    theme::apply(appearance, window, cx);
                }
            }),
        ];
        let mut workspace = Self {
            library,
            updater,
            sidebar,
            list,
            reader,
            discover,
            reading,
            focus,
            sidebar_open: true,
            _subscriptions: subscriptions,
        };
        workspace.focus.focus(window, cx);
        workspace.place_focus(window, cx);
        workspace
    }

    /// Whether the article list and reader are on screen.
    fn reading_visible(&self, cx: &App) -> bool {
        let library = self.library.read(cx);
        library.page == Page::Reading && !library.feeds.is_empty()
    }

    /// Keeps focus somewhere that's on screen, so shortcuts and menus always
    /// reach the workspace: the reading area when it shows, otherwise the
    /// window. Focus inside a field or dialog is left alone.
    fn place_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let visible = self.reading_visible(cx);
        if visible && self.focus.is_focused(window) {
            self.reading.focus(window, cx);
        } else if !visible && self.reading.is_focused(window) {
            self.focus.focus(window, cx);
        }
    }

    fn with_open(
        &self,
        cx: &mut Context<Self>,
        act: impl FnOnce(&mut Library, brook_core::model::ArticleId, &mut Context<Library>),
    ) {
        self.library.update(cx, |library, cx| {
            if let Some(id) = library.open.as_ref().map(|open| open.article.summary.id) {
                act(library, id, cx);
            }
        });
    }

    fn select(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        self.library.update(cx, |library, cx| library.select_scope(scope, cx));
        self.reading.focus(window, cx);
    }

    fn import_opml(&mut self, _: &ImportOpml, _: &mut Window, cx: &mut Context<Self>) {
        import_opml(self.library.clone(), cx);
    }

    fn export_opml(&mut self, _: &ExportOpml, _: &mut Window, cx: &mut Context<Self>) {
        export_opml(self.library.clone(), cx);
    }

    fn about(&mut self, _: &About, window: &mut Window, cx: &mut Context<Self>) {
        let p = *palette(cx);
        window.open_dialog(cx, move |dialog, _, cx| {
        let dialog = dialog.bg(crate::theme::palette(cx).surface);
            dialog.w(px(340.)).child(
                div()
                    .py(px(12.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(svg().path(MARK).size(px(48.)).text_color(p.accent))
                    .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(p.text).child("Brook"))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(p.muted())
                            .child(format!("Version {}", brook_core::update::CURRENT)),
                    )
                    .child(
                        div()
                            .pt(px(4.))
                            .text_size(px(12.5))
                            .text_color(p.faint())
                            .text_center()
                            .child("A calm place for the sites you love. No accounts, no algorithms, nothing in the cloud."),
                    ),
            )
        });
    }

    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let refreshing = self.library.read(cx).progress.is_some();
        let update = match &self.updater.read(cx).status {
            Status::Available(release) => Some((format!("Update to {}", release.version), false)),
            Status::Installing(_) => Some(("Updating…".to_owned(), true)),
            Status::Failed(..) => Some(("Update failed, try again".to_owned(), false)),
            _ => None,
        };
        let updater = self.updater.clone();

        TitleBar::new().bg(gpui::transparent_black()).child(
            div()
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_between()
                .pr(px(10.))
                .child(icon_button(
                    "toggle-sidebar",
                    icon::SIDEBAR,
                    "Toggle sidebar",
                    None,
                    cx,
                    |_, window, cx| window.dispatch_action(Box::new(ToggleSidebar), cx),
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .when_some(update, |bar, (label, busy)| {
                            bar.child(
                                div()
                                    .id("update-pill")
                                    .mr(px(6.))
                                    .h(px(24.))
                                    .px(px(10.))
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .rounded_full()
                                    .bg(p.accent.opacity(0.14))
                                    .text_color(p.accent)
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .when(busy, |pill| pill.child(spinner(11., cx)))
                                    .child(label)
                                    .when(!busy, |pill| {
                                        pill.cursor_pointer().on_click(move |_, _, cx| {
                                            updater.update(cx, |updater, cx| updater.install(cx))
                                        })
                                    }),
                            )
                        })
                        .map(|bar| {
                            if refreshing {
                                bar.child(
                                    div()
                                        .size(px(28.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(spinner(13., cx)),
                                )
                            } else {
                                bar.child(icon_button(
                                    "refresh",
                                    icon::REFRESH,
                                    "Refresh  R",
                                    None,
                                    cx,
                                    |_, window, cx| window.dispatch_action(Box::new(RefreshAll), cx),
                                ))
                            }
                        })
                        .child(icon_button("add", icon::ADD, "Add a feed", None, cx, |_, window, cx| {
                            window.dispatch_action(Box::new(AddFeed), cx)
                        })),
                ),
        )
    }

    fn welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .h(px(34.))
                .px(px(16.))
                .flex()
                .items_center()
                .rounded(px(17.))
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .map(|b| {
                    if primary {
                        b.bg(p.accent).text_color(gpui::white()).hover(|b| b.opacity(0.9))
                    } else {
                        b.bg(p.well()).text_color(p.text).hover(move |b| b.bg(p.selection()))
                    }
                })
                .child(label)
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(14.))
            .pb(px(40.))
            .child(svg().path(MARK).size(px(56.)).text_color(p.accent))
            .child(
                div()
                    .text_size(px(24.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.text)
                    .child("Welcome to Brook"),
            )
            .child(
                div()
                    .max_w(px(380.))
                    .text_center()
                    .text_size(px(13.5))
                    .line_height(px(20.))
                    .text_color(p.muted())
                    .child("A calm place for the sites you love. Everything you follow and read stays on this computer."),
            )
            .child(
                div()
                    .pt(px(10.))
                    .flex()
                    .gap(px(8.))
                    .child(button("welcome-add", "Add a feed", true).on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(AddFeed), cx)
                    }))
                    .child(button("welcome-discover", "Browse Discover", false).on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(ShowDiscover), cx)
                    }))
                    .child(button("welcome-import", "Import OPML", false).on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(ImportOpml), cx)
                    })),
            )
    }

    fn card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let library = self.library.read(cx);
        let (page, empty) = (library.page, library.feeds.is_empty());
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .rounded(px(12.))
            .bg(p.surface)
            .border_1()
            .border_color(p.stroke())
            .overflow_hidden()
            .when(!p.dark, |card| card.shadow_sm())
            .map(|card| match page {
                Page::Discover => card.child(self.discover.clone()),
                Page::Reading if empty => card.child(self.welcome(cx)),
                Page::Reading => card.child(
                    div()
                        .size_full()
                        .flex()
                        .key_context(READING)
                        .track_focus(&self.reading)
                        .child(self.list.clone())
                        .child(div().w(px(1.)).h_full().flex_none().bg(p.stroke()))
                        .child(self.reader.clone()),
                ),
            })
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            // Root already paints the window color; painting it again would
            // double the glass tint.
            .text_color(p.text)
            .key_context(WORKSPACE)
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, _: &AddFeed, window, cx| add_feed::open(this.library.clone(), window, cx)),
            )
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                settings_view::open(this.library.clone(), this.updater.clone(), window, cx)
            }))
            .on_action(cx.listener(|this, _: &RefreshAll, _, cx| {
                this.library.update(cx, |library, cx| library.refresh_all(cx))
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.sidebar_open = !this.sidebar_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.library.update(cx, |library, cx| library.show_page(Page::Reading, cx));
                this.list.update(cx, |list, cx| list.focus_search(window, cx));
            }))
            .on_action(cx.listener(|this, _: &ShowDiscover, _, cx| {
                this.library.update(cx, |library, cx| library.show_page(Page::Discover, cx))
            }))
            .on_action(cx.listener(Self::import_opml))
            .on_action(cx.listener(Self::export_opml))
            .on_action(cx.listener(Self::about))
            .on_action(cx.listener(|this, _: &CheckForUpdates, _, cx| {
                this.updater.update(cx, |updater, cx| updater.check(true, cx))
            }))
            .on_action(cx.listener(|this, _: &ShowAll, window, cx| this.select(Scope::All, window, cx)))
            .on_action(cx.listener(|this, _: &ShowToday, window, cx| this.select(Scope::Today, window, cx)))
            .on_action(
                cx.listener(|this, _: &ShowStarred, window, cx| this.select(Scope::Starred, window, cx)),
            )
            .on_action(cx.listener(|this, _: &ShowSaved, window, cx| this.select(Scope::Saved, window, cx)))
            .on_action(cx.listener(|this, _: &NextArticle, _, cx| {
                this.library.update(cx, |library, cx| library.step(1, cx))
            }))
            .on_action(cx.listener(|this, _: &PreviousArticle, _, cx| {
                this.library.update(cx, |library, cx| library.step(-1, cx))
            }))
            .on_action(cx.listener(|this, _: &ToggleRead, _, cx| {
                this.with_open(cx, |l, id, cx| l.toggle_read(id, cx))
            }))
            .on_action(cx.listener(|this, _: &ToggleStarred, _, cx| {
                this.with_open(cx, |l, id, cx| l.toggle_starred(id, cx))
            }))
            .on_action(cx.listener(|this, _: &ToggleSaved, _, cx| {
                this.with_open(cx, |l, id, cx| l.toggle_saved(id, cx))
            }))
            .on_action(cx.listener(|this, _: &ToggleReaderMode, _, cx| {
                this.library.update(cx, |library, cx| library.toggle_reader_mode(cx))
            }))
            .on_action(cx.listener(|this, _: &OpenInBrowser, _, cx| {
                let url = this.library.read(cx).open.as_ref().and_then(|open| open.article.url.clone());
                if let Some(url) = url {
                    cx.open_url(&url);
                }
            }))
            .on_action(cx.listener(|this, _: &MarkAllRead, _, cx| {
                this.library.update(cx, |library, cx| library.mark_all_read(library.scope, cx))
            }))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.reader.read(cx).page(true)))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.reader.read(cx).page(false)))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| {
                let cleared = this.list.update(cx, |list, cx| list.clear_search(window, cx));
                if !cleared {
                    this.library.update(cx, |library, cx| library.close_article(cx));
                }
                this.reading.focus(window, cx);
            }))
            .child(self.title_bar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .pb(px(8.))
                    .pr(px(8.))
                    .when(!self.sidebar_open, |row| row.pl(px(8.)))
                    .when(self.sidebar_open, |row| row.child(self.sidebar.clone()))
                    .child(self.card(cx)),
            )
    }
}

/// Asks for an OPML file and imports it.
pub fn import_opml(library: Entity<Library>, cx: &mut App) {
    let paths = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Import".into()),
    });
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = paths.await
            && let Some(path) = paths.first()
        {
            library.update(cx, |library, cx| library.import_opml(path, cx));
        }
    })
    .detach();
}

/// Asks where to save, and exports subscriptions as OPML there.
pub fn export_opml(library: Entity<Library>, cx: &mut App) {
    let folder = dirs_home().join("Documents");
    let path = cx.prompt_for_new_path(&folder, Some("Brook subscriptions.opml"));
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(path))) = path.await {
            library.update(cx, |library, cx| library.export_opml(&path, cx));
        }
    })
    .detach();
}

/// Shows a notice as a toast.
fn show(notice: &Notice, window: &mut Window, cx: &mut App) {
    let note = match notice {
        Notice::Info(message) => Notification::info(message.clone()),
        Notice::Error(message) => Notification::error(message.clone()),
    };
    window.push_notification(note, cx);
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};
    use gpui_component::{Root, WindowExt};

    /// Opens the real window content over an empty library in `dir`.
    fn open<'a>(cx: &'a mut TestAppContext, dir: &std::path::Path) -> &'a mut VisualTestContext {
        cx.update(crate::init);
        let (db, cache) = (dir.join("brook.db"), dir.join("cache"));
        let (_, cx) = cx.add_window_view(|window, cx| {
            let workspace = crate::workspace(&db, &cache, false, window, cx).unwrap();
            Root::new(workspace, window, cx)
        });
        cx.run_until_parked();
        cx
    }

    #[gpui::test]
    fn shortcuts_work_on_the_welcome_screen(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let cx = open(cx, dir.path());
        cx.simulate_keystrokes("ctrl-n");
        cx.run_until_parked();
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)), "Add a feed opened");
    }

    #[gpui::test]
    fn shortcuts_work_from_discover(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let cx = open(cx, dir.path());
        cx.simulate_keystrokes("ctrl-,");
        cx.run_until_parked();
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)), "Settings opened");
        cx.update(|window, cx| window.close_all_dialogs(cx));
        cx.dispatch_action(crate::actions::ShowDiscover);
        cx.simulate_keystrokes("ctrl-n");
        cx.run_until_parked();
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)), "Add a feed opened from Discover");
    }
}
