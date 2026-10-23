//! Preferences, stored as one JSON value in the database so adding a
//! field never needs a migration.

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::store::Store;

const KEY: &str = "settings";

/// Light, dark, or whatever the system uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    /// Follow the system.
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// The reader's text size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSize {
    /// 15 px body text.
    Small,
    /// 17 px body text.
    #[default]
    Medium,
    /// 19 px body text.
    Large,
}

impl TextSize {
    /// Body text size in pixels.
    pub fn pixels(self) -> f32 {
        match self {
            TextSize::Small => 15.,
            TextSize::Medium => 17.,
            TextSize::Large => 19.,
        }
    }
}

/// How often feeds refresh in the background, in minutes. Zero means only
/// when asked.
pub const REFRESH_CHOICES: &[u32] = &[0, 15, 30, 60, 180];

/// Everything someone can change in Settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Light or dark.
    pub appearance: Appearance,
    /// Minutes between background refreshes; zero for manual only.
    pub refresh_minutes: u32,
    /// Mark an article read as soon as it opens.
    pub mark_read_on_open: bool,
    /// Lists hide read articles.
    pub unread_only: bool,
    /// Show lead images in lists.
    pub show_thumbnails: bool,
    /// Reader text size.
    pub text_size: TextSize,
    /// Check GitHub for new versions once a day.
    pub check_for_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            refresh_minutes: 30,
            mark_read_on_open: true,
            unread_only: false,
            show_thumbnails: true,
            text_size: TextSize::Medium,
            check_for_updates: true,
        }
    }
}

impl Settings {
    /// Reads settings, falling back to defaults for anything missing or
    /// unreadable. A damaged value never stops the app from starting.
    pub fn load(store: &Store) -> Self {
        store
            .setting(KEY)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// Writes settings.
    pub fn save(&self, store: &Store) -> Result<()> {
        let json = serde_json::to_string(self).expect("settings serialize");
        store.set_setting(KEY, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(Settings::load(&store), Settings::default());
        let settings = Settings { appearance: Appearance::Dark, refresh_minutes: 60, ..Settings::default() };
        settings.save(&store).unwrap();
        assert_eq!(Settings::load(&store), settings);
    }

    #[test]
    fn tolerates_old_and_damaged_values() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting(KEY, r#"{"appearance":"light","retired_field":1}"#).unwrap();
        let settings = Settings::load(&store);
        assert_eq!(settings.appearance, Appearance::Light);
        assert_eq!(settings.refresh_minutes, 30, "missing fields take defaults");

        store.set_setting(KEY, "not json").unwrap();
        assert_eq!(Settings::load(&store), Settings::default());
    }
}
