//! The sidebar: smart views, folders, and feeds, sitting on the glass.

use brook_core::model::{Feed, FeedId, Folder, FolderId, Scope};
use gpui::prelude::*;
use gpui::{App, Context, Entity, FontWeight, SharedString, Window, div, px};
use gpui_component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_component::tooltip::Tooltip;

use crate::actions::{AddFeed, OpenSettings};
use crate::assets::icon;
use crate::components::{
    count_label, icon as draw_icon, icon_button, nav_row, section_label, site_icon, spinner,
};
use crate::dialogs;
use crate::images::Images;
use crate::library::{Library, Page};
use crate::theme::palette;

/// The sidebar's width when shown.
pub const WIDTH: f32 = 236.;

/// The sidebar view.
pub struct Sidebar {
    library: Entity<Library>,
    images: Entity<Images>,
}

impl Sidebar {
    /// A sidebar over `library`.
    pub fn new(library: Entity<Library>, images: Entity<Images>, cx: &mut Context<Self>) -> Self {
        cx.observe(&library, |_, _, cx| cx.notify()).detach();
        cx.observe(&images, |_, _, cx| cx.notify()).detach();
        Self { library, images }
    }

    fn select(&self, scope: Scope, cx: &mut App) {
        self.library.update(cx, |library, cx| library.select_scope(scope, cx));
    }

    fn smart_row(
        &self,
        id: &'static str,
        icon_path: &'static str,
        label: &'static str,
        scope: Scope,
        count: u32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let library = self.library.read(cx);
        let active = library.page == Page::Reading && library.scope == scope;
        nav_row(id, icon_path, label, Some(count), active, cx).on_click(cx.listener(move |this, _, _, cx| {
            this.select(scope, cx);
        }))
    }

    fn feed_row(&self, feed: &Feed, indent: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let library = self.library.read(cx);
        let active = library.page == Page::Reading && library.scope == Scope::Feed(feed.id);
        let count = library.counts.feed(feed.id);
        let folders = library.folders.clone();
        let icon_path = self.images.update(cx, |images, cx| images.feed_icon(feed, cx));
        let (id, title, error) = (feed.id, feed.title.clone(), feed.last_error.clone());
        let site = feed.site_url.clone();
        let url = feed.url.clone();
        let folder_id = feed.folder_id;
        let library = self.library.clone();

        div()
            .id(("feed", id.0 as u64))
            .h(px(30.))
            .pl(px(if indent { 26. } else { 8. }))
            .pr(px(8.))
            .flex()
            .items_center()
            .gap(px(9.))
            .rounded(px(6.))
            .cursor_pointer()
            .text_color(if active { p.text } else { p.ink(0.62) })
            .when(active, |row| row.bg(p.selection()))
            .hover(move |row| row.bg(if active { p.selection() } else { p.hover() }).text_color(p.text))
            .child(site_icon(icon_path, &title, 15., cx))
            .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(title.clone()))
            .when_some(error, |row, error| {
                row.child(
                    div()
                        .id(("feed-error", id.0 as u64))
                        .child(draw_icon(icon::WARNING, 12., p.faint()))
                        .tooltip(move |window, cx| Tooltip::new(error.clone()).build(window, cx)),
                )
            })
            .when(count > 0, |row| row.child(count_label(count, cx)))
            .on_click(cx.listener(move |this, _, _, cx| this.select(Scope::Feed(id), cx)))
            .context_menu(move |menu, window, cx| {
                feed_menu(menu, &library, id, &title, &url, site.clone(), folder_id, &folders, window, cx)
            })
    }

    fn folder_row(&self, folder: &Folder, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let library = self.library.read(cx);
        let active = library.page == Page::Reading && library.scope == Scope::Folder(folder.id);
        let count = library.counts.sum(library.feeds_in(Some(folder.id)));
        let (id, name, expanded) = (folder.id, folder.name.clone(), folder.expanded);
        let library = self.library.clone();

        div()
            .id(("folder", id.0 as u64))
            .h(px(30.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(9.))
            .rounded(px(6.))
            .cursor_pointer()
            .text_color(if active { p.text } else { p.ink(0.62) })
            .when(active, |row| row.bg(p.selection()))
            .hover(move |row| row.bg(if active { p.selection() } else { p.hover() }).text_color(p.text))
            .child(
                div()
                    .id(("folder-toggle", id.0 as u64))
                    .size(px(15.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .hover(move |s| s.bg(p.hover()))
                    .child(draw_icon(
                        if expanded { icon::CHEVRON_DOWN } else { icon::CHEVRON_RIGHT },
                        13.,
                        p.faint(),
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.library.update(cx, |library, cx| library.toggle_folder(id, cx));
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(name.clone()),
            )
            .when(count > 0, |row| row.child(count_label(count, cx)))
            .on_click(cx.listener(move |this, _, _, cx| this.select(Scope::Folder(id), cx)))
            .context_menu(move |menu, _, _| {
                let (rename, mark, delete) = (library.clone(), library.clone(), library.clone());
                let rename_name = name.clone();
                let delete_name = name.clone();
                menu.item(PopupMenuItem::new("Mark all read").icon(icon(icon::MARK_ALL_READ)).on_click(
                    move |_, _, cx| {
                        mark.update(cx, |library, cx| library.mark_all_read(Scope::Folder(id), cx))
                    },
                ))
                .item(PopupMenuItem::new("Rename…").icon(icon(icon::RENAME)).on_click(
                    move |_, window, cx| {
                        let library = rename.clone();
                        dialogs::ask_text(
                            window,
                            cx,
                            "Rename folder",
                            &rename_name,
                            "Rename",
                            move |name, _, cx| {
                                library.update(cx, |library, cx| library.rename_folder(id, &name, cx));
                            },
                        );
                    },
                ))
                .separator()
                .item(
                    PopupMenuItem::new("Delete folder").icon(icon(icon::DELETE)).on_click(
                        move |_, window, cx| {
                            let library = delete.clone();
                            dialogs::confirm(
                                window,
                                cx,
                                format!("Delete “{delete_name}”?"),
                                "Its feeds move to the top level. Nothing is unsubscribed.",
                                "Delete",
                                move |_, cx| library.update(cx, |library, cx| library.delete_folder(id, cx)),
                            );
                        },
                    ),
                )
            })
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = *palette(cx);
        let library = self.library.read(cx);
        let discovering = library.page == Page::Discover;
        let progress = library.progress;
        let feeds = library.feeds.len();

        div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .pt(px(6.))
            .pb(px(10.))
            .child(nav_row("discover", icon::DISCOVER, "Discover", None, discovering, cx).on_click(
                cx.listener(|this, _, _, cx| {
                    this.library.update(cx, |library, cx| library.show_page(Page::Discover, cx))
                }),
            ))
            .child(
                nav_row("settings", icon::SETTINGS, "Settings", None, false, cx)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx)),
            )
            .child(
                div()
                    .h(px(22.))
                    .px(px(8.))
                    .mt(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(11.5))
                    .text_color(p.faint())
                    .map(|status| match progress {
                        Some(progress) => status.child(spinner(11., cx)).child(format!(
                            "Refreshing {} of {}",
                            progress.done.min(progress.total),
                            progress.total
                        )),
                        None if feeds == 0 => status,
                        None => status.child(SharedString::from(if feeds == 1 {
                            "1 feed".to_owned()
                        } else {
                            format!("{feeds} feeds")
                        })),
                    }),
            )
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let library = self.library.read(cx);
        let counts = library.counts.clone();
        let folders = library.folders.clone();
        let loose: Vec<Feed> = library.feeds_in(None).cloned().collect();
        let nested: Vec<(Folder, Vec<Feed>)> = folders
            .into_iter()
            .map(|folder| {
                let feeds = library.feeds_in(Some(folder.id)).cloned().collect();
                (folder, feeds)
            })
            .collect();
        let has_feeds = !library.feeds.is_empty();

        let mut tree = div().flex().flex_col().gap(px(1.));
        for (folder, feeds) in &nested {
            tree = tree.child(self.folder_row(folder, cx));
            if folder.expanded {
                for feed in feeds {
                    tree = tree.child(self.feed_row(feed, true, cx));
                }
            }
        }
        for feed in &loose {
            tree = tree.child(self.feed_row(feed, false, cx));
        }

        div()
            .w(px(WIDTH))
            .h_full()
            .flex()
            .flex_col()
            .flex_none()
            .px(px(8.))
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .pt(px(2.))
                    .child(self.smart_row("all", icon::ALL, "All articles", Scope::All, counts.all, cx))
                    .child(self.smart_row("today", icon::TODAY, "Today", Scope::Today, counts.today, cx))
                    .child(self.smart_row(
                        "starred",
                        icon::STAR,
                        "Starred",
                        Scope::Starred,
                        counts.starred,
                        cx,
                    ))
                    .child(self.smart_row("saved", icon::SAVED, "Read later", Scope::Saved, counts.saved, cx))
                    .child(
                        div()
                            .mt(px(18.))
                            .mb(px(2.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(section_label("Feeds", cx))
                            .child(
                                div()
                                    .flex()
                                    .child(icon_button(
                                        "new-folder",
                                        icon::FOLDER_ADD,
                                        "New folder",
                                        None,
                                        cx,
                                        {
                                            let library = self.library.clone();
                                            move |_, window, cx| {
                                                let library = library.clone();
                                                dialogs::ask_text(
                                                    window,
                                                    cx,
                                                    "New folder",
                                                    "",
                                                    "Create",
                                                    move |name, _, cx| {
                                                        library.update(cx, |library, cx| {
                                                            library.create_folder(&name, cx);
                                                        });
                                                    },
                                                );
                                            }
                                        },
                                    ))
                                    .child(icon_button(
                                        "add-feed",
                                        icon::ADD,
                                        "Add a feed",
                                        None,
                                        cx,
                                        |_, window, cx| window.dispatch_action(Box::new(AddFeed), cx),
                                    )),
                            ),
                    )
                    .when(!has_feeds, |list| {
                        list.child(
                            div()
                                .px(px(8.))
                                .py(px(6.))
                                .text_size(px(12.))
                                .text_color(palette(cx).faint())
                                .child("Feeds you follow show up here."),
                        )
                    })
                    .child(tree),
            )
            .child(self.footer(cx))
    }
}

/// The right-click menu for a feed.
#[allow(clippy::too_many_arguments)]
fn feed_menu(
    menu: PopupMenu,
    library: &Entity<Library>,
    id: FeedId,
    title: &str,
    url: &str,
    site: Option<String>,
    folder: Option<FolderId>,
    folders: &[Folder],
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (mark, rename, delete) = (library.clone(), library.clone(), library.clone());
    let (rename_title, delete_title) = (title.to_owned(), title.to_owned());
    let copy_url = url.to_owned();
    let folders = folders.to_vec();
    let mover = library.clone();

    menu.item(
        PopupMenuItem::new("Mark all read").icon(icon(icon::MARK_ALL_READ)).on_click(move |_, _, cx| {
            mark.update(cx, |library, cx| library.mark_all_read(Scope::Feed(id), cx))
        }),
    )
    .separator()
    .item(PopupMenuItem::new("Rename…").icon(icon(icon::RENAME)).on_click(move |_, window, cx| {
        let library = rename.clone();
        dialogs::ask_text(window, cx, "Rename feed", &rename_title, "Rename", move |title, _, cx| {
            library.update(cx, |library, cx| library.rename_feed(id, &title, cx));
        });
    }))
    .submenu("Move to", window, cx, move |mut menu, _, _| {
        let to_top = mover.clone();
        menu = menu.item(
            PopupMenuItem::new("No folder")
                .checked(folder.is_none())
                .on_click(move |_, _, cx| to_top.update(cx, |library, cx| library.move_feed(id, None, cx))),
        );
        for target in &folders {
            let (library, target_id) = (mover.clone(), target.id);
            menu = menu.item(
                PopupMenuItem::new(target.name.clone()).checked(folder == Some(target_id)).on_click(
                    move |_, _, cx| {
                        library.update(cx, |library, cx| library.move_feed(id, Some(target_id), cx))
                    },
                ),
            );
        }
        menu
    })
    .separator()
    .when_some(site, |menu, site| {
        menu.item(
            PopupMenuItem::new("Open website")
                .icon(icon(icon::GLOBE))
                .on_click(move |_, _, cx| cx.open_url(&site)),
        )
    })
    .item(
        PopupMenuItem::new("Copy feed address").icon(icon(icon::LINK)).on_click(move |_, _, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_url.clone()))
        }),
    )
    .separator()
    .item(PopupMenuItem::new("Unsubscribe").icon(icon(icon::DELETE)).on_click(move |_, window, cx| {
        let library = delete.clone();
        dialogs::confirm(
            window,
            cx,
            format!("Unsubscribe from “{delete_title}”?"),
            "Its articles are deleted too, starred ones included.",
            "Unsubscribe",
            move |_, cx| library.update(cx, |library, cx| library.delete_feed(id, cx)),
        );
    }))
}

/// A menu icon from a path.
fn icon(path: &'static str) -> gpui_component::Icon {
    gpui_component::Icon::default().path(path)
}
