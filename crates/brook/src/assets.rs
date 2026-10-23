//! Icons and brand art, compiled into the binary.
//!
//! gpui-kit-assets supplies Lucide icons: its default set, plus the extras
//! named here. Brand art lives under `brand/`.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

gpui_kit_assets::icon_assets!(
    ExtraIcons,
    [
        ArrowUpRight,
        Bookmark,
        BookmarkCheck,
        CalendarDays,
        CheckCheck,
        Compass,
        Download,
        FolderPlus,
        Glasses,
        Link,
        ListFilter,
        MailOpen,
        Mail,
        Pencil,
        Trash,
        Upload
    ]
);

/// The brand mark, drawn in the text color.
pub const MARK: &str = "brand/mark.svg";

const BRAND: &[(&str, &[u8])] = &[(MARK, include_bytes!("../../../assets/brand/mark.svg"))];

/// Everything Brook draws from.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = BRAND.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit_assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.extend(
            BRAND.iter().map(|(name, _)| *name).filter(|name| name.starts_with(path)).map(Into::into),
        );
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

/// Icon paths, so views don't spell them out.
pub mod icon {
    #![allow(missing_docs)]
    pub const ADD: &str = "icons/plus.svg";
    pub const ALL: &str = "icons/inbox.svg";
    pub const TODAY: &str = "icons/calendar-days.svg";
    pub const STAR: &str = "icons/star.svg";
    pub const STAR_FILL: &str = "icons/star-fill.svg";
    pub const SAVED: &str = "icons/bookmark.svg";
    pub const SAVED_FILL: &str = "icons/bookmark-check.svg";
    pub const FOLDER_ADD: &str = "icons/folder-plus.svg";
    pub const CHEVRON_RIGHT: &str = "icons/chevron-right.svg";
    pub const CHEVRON_DOWN: &str = "icons/chevron-down.svg";
    pub const DISCOVER: &str = "icons/compass.svg";
    pub const SETTINGS: &str = "icons/settings-2.svg";
    pub const REFRESH: &str = "icons/refresh-cw.svg";
    pub const SEARCH: &str = "icons/search.svg";
    pub const SIDEBAR: &str = "icons/panel-left.svg";
    pub const EXTERNAL: &str = "icons/arrow-up-right.svg";
    pub const READER: &str = "icons/glasses.svg";
    pub const MARK_READ: &str = "icons/mail-open.svg";
    pub const MARK_UNREAD: &str = "icons/mail.svg";
    pub const MARK_ALL_READ: &str = "icons/check-check.svg";
    pub const FILTER: &str = "icons/list-filter.svg";
    pub const LINK: &str = "icons/link.svg";
    pub const RENAME: &str = "icons/pencil.svg";
    pub const DELETE: &str = "icons/trash.svg";
    pub const IMPORT: &str = "icons/download.svg";
    pub const EXPORT: &str = "icons/upload.svg";
    pub const PLAY: &str = "icons/play.svg";
    pub const CHECK: &str = "icons/check.svg";
    pub const GLOBE: &str = "icons/globe.svg";
    pub const WARNING: &str = "icons/triangle-alert.svg";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_resolves() {
        let all = [
            icon::ADD,
            icon::ALL,
            icon::TODAY,
            icon::STAR,
            icon::STAR_FILL,
            icon::SAVED,
            icon::SAVED_FILL,
            icon::FOLDER_ADD,
            icon::CHEVRON_RIGHT,
            icon::CHEVRON_DOWN,
            icon::DISCOVER,
            icon::SETTINGS,
            icon::REFRESH,
            icon::SEARCH,
            icon::SIDEBAR,
            icon::EXTERNAL,
            icon::READER,
            icon::MARK_READ,
            icon::MARK_UNREAD,
            icon::MARK_ALL_READ,
            icon::FILTER,
            icon::LINK,
            icon::RENAME,
            icon::DELETE,
            icon::IMPORT,
            icon::EXPORT,
            icon::PLAY,
            icon::CHECK,
            icon::GLOBE,
            icon::WARNING,
            MARK,
        ];
        for path in all {
            assert!(Assets.load(path).unwrap().is_some(), "{path} is missing");
        }
    }
}
