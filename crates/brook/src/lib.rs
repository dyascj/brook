//! Brook, a calm, local RSS reader.
//!
//! The GPUI app. Storage, fetching, and parsing live in `brook-core`; this
//! crate is the window, the views, and the glue. `main.rs` only starts it.

// Views hold entities, tasks, and subscriptions; a Debug impl would print
// nothing useful.
#![allow(missing_debug_implementations)]

pub mod actions;
pub mod add_feed;
pub mod article_list;
pub mod assets;
pub mod components;
pub mod dialogs;
pub mod discover;
pub mod images;
pub mod library;
pub mod reader;
pub mod settings_view;
pub mod sidebar;
pub mod theme;
pub mod typeset;
pub mod updater;
pub mod workspace;

use std::path::Path;

use brook_core::cache::DiskCache;
use brook_core::paths;
use gpui::prelude::*;
use gpui::{
    App, Bounds, Entity, Menu, MenuItem, QuitMode, SharedString, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, div, point, px, size,
};
use gpui_component::{Root, TitleBar};

use crate::actions::*;
use crate::images::Images;
use crate::library::Library;
use crate::updater::Updater;
use crate::workspace::Workspace;

/// Runs the app until it quits.
pub fn run() {
    gpui_platform::application().with_assets(assets::Assets).with_quit_mode(QuitMode::LastWindowClosed).run(
        |cx: &mut App| {
            init(cx);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.set_menus(menus());
            open_main_window(cx);
            cx.activate(true);
        },
    );
}

/// Sets up gpui-component and Brook's shortcuts.
pub fn init(cx: &mut App) {
    gpui_component::init(cx);
    actions::bind_keys(cx);
}

fn open_main_window(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(900.), px(560.))),
        window_background: if theme::supports_glass() {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Opaque
        },
        app_id: Some("io.github.dyascj.brook".into()),
        titlebar: Some(TitlebarOptions {
            title: Some("Brook".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(14.), px(13.))),
        }),
        ..TitleBar::window_options()
    };

    let opened =
        cx.open_window(options, |window, cx| root(&paths::database(), &paths::cache_dir(), window, cx));
    if let Err(error) = opened {
        log::error!("couldn't open the window: {error}");
    }
}

/// Builds the window's content over the library at `db_path`, with images
/// cached in `cache_dir`. A library that can't be opened shows why instead.
pub fn root(db_path: &Path, cache_dir: &Path, window: &mut Window, cx: &mut App) -> Entity<Root> {
    let view: gpui::AnyView = match workspace(db_path, cache_dir, true, window, cx) {
        Ok(workspace) => workspace.into(),
        Err(error) => {
            log::error!("opening {} failed: {error}", db_path.display());
            let message = format!("Brook couldn't open its library at {}: {error}", db_path.display());
            cx.new(|_| Failure(message.into())).into()
        }
    };
    cx.new(|cx| Root::new(view, window, cx))
}

/// The workspace over the library at `db_path`, with images cached in
/// `cache_dir`. Offline, images come only from the cache; tests use that.
pub fn workspace(
    db_path: &Path,
    cache_dir: &Path,
    online: bool,
    window: &mut Window,
    cx: &mut App,
) -> brook_core::Result<Entity<Workspace>> {
    let store = brook_core::Store::open(db_path)?;
    let library = cx.new(|cx| Library::new(store, db_path, cx));
    theme::apply(library.read(cx).settings.appearance, window, cx);
    let client = library.read(cx).client().clone();
    let images = cx.new(|_| Images::new(DiskCache::new(cache_dir), online.then_some(client)));
    let updater = cx.new(|cx| Updater::new(library.clone(), cx));
    Ok(cx.new(|cx| Workspace::new(library, images, updater, window, cx)))
}

fn menus() -> Vec<Menu> {
    vec![
        Menu {
            name: "Brook".into(),
            items: vec![
                MenuItem::action("About Brook", About),
                MenuItem::action("Check for Updates…", CheckForUpdates),
                MenuItem::separator(),
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Quit Brook", Quit),
            ],
            disabled: false,
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("Add a Feed…", AddFeed),
                MenuItem::action("Refresh All", RefreshAll),
                MenuItem::separator(),
                MenuItem::action("Import OPML…", ImportOpml),
                MenuItem::action("Export OPML…", ExportOpml),
            ],
            disabled: false,
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("All Articles", ShowAll),
                MenuItem::action("Today", ShowToday),
                MenuItem::action("Starred", ShowStarred),
                MenuItem::action("Read Later", ShowSaved),
                MenuItem::separator(),
                MenuItem::action("Discover", ShowDiscover),
                MenuItem::action("Toggle Sidebar", ToggleSidebar),
            ],
            disabled: false,
        },
        Menu {
            name: "Article".into(),
            items: vec![
                MenuItem::action("Next", NextArticle),
                MenuItem::action("Previous", PreviousArticle),
                MenuItem::separator(),
                MenuItem::action("Mark Read or Unread", ToggleRead),
                MenuItem::action("Star", ToggleStarred),
                MenuItem::action("Read Later", ToggleSaved),
                MenuItem::action("Reader Mode", ToggleReaderMode),
                MenuItem::action("Open in Browser", OpenInBrowser),
                MenuItem::separator(),
                MenuItem::action("Mark All Read", MarkAllRead),
            ],
            disabled: false,
        },
    ]
}

/// Shown instead of the app when the library can't be opened.
struct Failure(SharedString);

impl Render for Failure {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div().size_full().flex().items_center().justify_center().p(px(40.)).child(self.0.clone())
    }
}
