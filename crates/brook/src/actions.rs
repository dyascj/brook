//! Commands and their keyboard shortcuts.
//!
//! Shortcuts with modifiers work anywhere in the window. Single keys work
//! while reading (the article list or the reader has focus), so they never
//! fight the search field.

use gpui::{App, KeyBinding, actions};

actions!(
    brook,
    [
        /// Quit Brook.
        Quit,
        /// Show the About window.
        About,
        /// Check GitHub for a newer version.
        CheckForUpdates,
        /// Open Settings.
        OpenSettings,
        /// Subscribe to a feed.
        AddFeed,
        /// Fetch every feed now.
        RefreshAll,
        /// Show or hide the sidebar.
        ToggleSidebar,
        /// Put the cursor in the search field.
        FocusSearch,
        /// Browse the feed catalog.
        ShowDiscover,
        /// Import subscriptions from an OPML file.
        ImportOpml,
        /// Export subscriptions to an OPML file.
        ExportOpml,
        /// Open the next article.
        NextArticle,
        /// Open the previous article.
        PreviousArticle,
        /// Flip read and unread.
        ToggleRead,
        /// Star or unstar.
        ToggleStarred,
        /// Save for later, or take off the list.
        ToggleSaved,
        /// Open the article's web page.
        OpenInBrowser,
        /// Switch reader mode.
        ToggleReaderMode,
        /// Mark everything in the list read.
        MarkAllRead,
        /// Show all articles.
        ShowAll,
        /// Show today's articles.
        ShowToday,
        /// Show starred articles.
        ShowStarred,
        /// Show articles saved for later.
        ShowSaved,
        /// Close the reader or clear the search.
        Dismiss,
        /// Scroll the reader down a page.
        PageDown,
        /// Scroll the reader up a page.
        PageUp,
    ]
);

/// The key context of the main window.
pub const WORKSPACE: &str = "Workspace";
/// The key context of the article list and reader.
pub const READING: &str = "Reading";

/// Registers every shortcut.
pub fn bind_keys(cx: &mut App) {
    let workspace = Some(WORKSPACE);
    let reading = Some(READING);
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("ctrl-q", Quit, None),
        KeyBinding::new("cmd-,", OpenSettings, workspace),
        KeyBinding::new("ctrl-,", OpenSettings, workspace),
        KeyBinding::new("cmd-n", AddFeed, workspace),
        KeyBinding::new("ctrl-n", AddFeed, workspace),
        KeyBinding::new("cmd-r", RefreshAll, workspace),
        KeyBinding::new("ctrl-r", RefreshAll, workspace),
        KeyBinding::new("cmd-\\", ToggleSidebar, workspace),
        KeyBinding::new("ctrl-\\", ToggleSidebar, workspace),
        KeyBinding::new("cmd-f", FocusSearch, workspace),
        KeyBinding::new("ctrl-f", FocusSearch, workspace),
        KeyBinding::new("cmd-1", ShowAll, workspace),
        KeyBinding::new("cmd-2", ShowToday, workspace),
        KeyBinding::new("cmd-3", ShowStarred, workspace),
        KeyBinding::new("cmd-4", ShowSaved, workspace),
        KeyBinding::new("ctrl-1", ShowAll, workspace),
        KeyBinding::new("ctrl-2", ShowToday, workspace),
        KeyBinding::new("ctrl-3", ShowStarred, workspace),
        KeyBinding::new("ctrl-4", ShowSaved, workspace),
        KeyBinding::new("j", NextArticle, reading),
        KeyBinding::new("down", NextArticle, reading),
        KeyBinding::new("k", PreviousArticle, reading),
        KeyBinding::new("up", PreviousArticle, reading),
        KeyBinding::new("m", ToggleRead, reading),
        KeyBinding::new("s", ToggleStarred, reading),
        KeyBinding::new("l", ToggleSaved, reading),
        KeyBinding::new("o", OpenInBrowser, reading),
        KeyBinding::new("enter", OpenInBrowser, reading),
        KeyBinding::new("v", ToggleReaderMode, reading),
        KeyBinding::new("shift-a", MarkAllRead, reading),
        KeyBinding::new("r", RefreshAll, reading),
        KeyBinding::new("/", FocusSearch, reading),
        KeyBinding::new("escape", Dismiss, reading),
        KeyBinding::new("space", PageDown, reading),
        KeyBinding::new("shift-space", PageUp, reading),
    ]);
}

/// Shortcuts listed in Settings, as (keys, what they do). "Cmd" reads as
/// the platform's command key.
pub const CHEAT_SHEET: &[(&str, &str)] = &[
    ("J  K", "Next and previous article"),
    ("M", "Mark read or unread"),
    ("S", "Star"),
    ("L", "Read later"),
    ("V", "Reader mode"),
    ("O", "Open in browser"),
    ("Shift A", "Mark all read"),
    ("R", "Refresh"),
    ("/", "Search"),
    ("Space", "Page through an article"),
    ("Cmd N", "Add a feed"),
    ("Cmd 1–4", "All, Today, Starred, Read later"),
    ("Cmd \\", "Show or hide the sidebar"),
];
