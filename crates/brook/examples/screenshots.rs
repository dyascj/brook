//! Renders Brook off-screen and saves PNG screenshots, without needing the
//! window on screen or screen-recording permission.
//!
//! ```sh
//! BROOK_NO_GLASS=1 cargo run -p brook --example screenshots --features screenshots -- \
//!     <library.db> <cache-dir> <output-dir>
//! ```
//!
//! `STEPS=n` opens the n-th article in the reading shots (default 2). The
//! library is copied first, so the screenshots never change it. macOS
//! only: GPUI's visual test platform renders through Metal.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    use std::path::PathBuf;
    use std::sync::Arc;

    use brook::actions::{AddFeed, NextArticle, OpenSettings, ShowDiscover};
    use brook_core::settings::{Appearance, Settings};
    use gpui::{AnyWindowHandle, VisualTestAppContext, px, size};

    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let (Some(library), Some(cache), Some(out)) = (args.next(), args.next(), args.next()) else {
        anyhow::bail!("usage: screenshots <library.db> <cache-dir> <output-dir>");
    };
    std::fs::create_dir_all(&out)?;
    let scratch = tempfile::tempdir()?;

    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        Arc::new(brook::assets::Assets),
    );
    cx.update(brook::init);

    // (file name, appearance, an empty library, action to run first, open an article)
    type Shot = (&'static str, Appearance, bool, Option<Box<dyn Fn() -> Box<dyn gpui::Action>>>, bool);
    let steps: usize = std::env::var("STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    let shots: Vec<Shot> = vec![
        ("reading-light", Appearance::Light, false, None, true),
        ("reading-dark", Appearance::Dark, false, None, true),
        ("discover", Appearance::Light, false, Some(Box::new(|| Box::new(ShowDiscover))), false),
        ("add-feed", Appearance::Dark, false, Some(Box::new(|| Box::new(AddFeed))), false),
        ("settings", Appearance::Light, false, Some(Box::new(|| Box::new(OpenSettings))), false),
        ("welcome", Appearance::Light, true, None, false),
    ];

    for (name, appearance, empty, action, open_article) in shots {
        let db = scratch.path().join(format!("{name}.db"));
        if !empty {
            std::fs::copy(&library, &db)?;
        }
        let store = brook_core::Store::open(&db)?;
        let settings =
            Settings { appearance, refresh_minutes: 0, check_for_updates: false, ..Settings::load(&store) };
        settings.save(&store)?;
        drop(store);

        let cache = cache.clone();
        let window: AnyWindowHandle = cx
            .open_offscreen_window(size(px(1280.), px(820.)), |window, cx| {
                brook::root(&db, &cache, window, cx)
            })?
            .into();
        cx.run_until_parked();
        if open_article {
            for _ in 0..steps {
                cx.dispatch_action(window, NextArticle);
            }
        }
        if let Some(action) = action {
            cx.update_window(window, |_, window, cx| window.dispatch_action(action(), cx))?;
        }
        for _ in 0..20 {
            cx.run_until_parked();
            cx.advance_clock(std::time::Duration::from_millis(50));
        }
        let image = cx.capture_screenshot(window)?;
        let path = out.join(format!("{name}.png"));
        image.save(&path)?;
        println!("saved {}", path.display());
        cx.update_window(window, |_, window, _| window.remove_window())?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("screenshots render through Metal, so they only work on macOS");
}
