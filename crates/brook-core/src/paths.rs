//! Where Brook keeps its files.
//!
//! - macOS: `~/Library/Application Support/Brook` and `~/Library/Caches/Brook`
//! - Linux: `$XDG_DATA_HOME/brook` (`~/.local/share/brook`) and
//!   `$XDG_CACHE_HOME/brook` (`~/.cache/brook`)
//!
//! `BROOK_DATA_DIR` overrides both, putting the cache inside it. Handy for
//! trying things without touching your real library.

use std::path::PathBuf;

const OVERRIDE: &str = "BROOK_DATA_DIR";

fn app_dir_name() -> &'static str {
    if cfg!(target_os = "macos") { "Brook" } else { "brook" }
}

/// The directory holding the database.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(OVERRIDE) {
        return PathBuf::from(dir);
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join(app_dir_name())
}

/// The directory holding downloaded images.
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(OVERRIDE) {
        return PathBuf::from(dir).join("cache");
    }
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join(app_dir_name())
}

/// The SQLite database.
pub fn database() -> PathBuf {
    data_dir().join("brook.db")
}
