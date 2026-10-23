//! Brook's look: neutral glass, hairlines, and one blue.
//!
//! After T3 Code Glass. Region strokes are the text color at 7%, selection
//! fills are 10% in dark and 6% in light, hover fills are 5%, and secondary
//! text is the text color at 55%. On macOS in dark mode the window is glass:
//! the desktop shows through, blurred, behind the sidebar, and the workspace
//! floats above it in a solid card.
//!
//! Views read the [`Palette`] global. gpui-component's widgets read its own
//! `Theme`, which [`apply`] keeps in step.

use brook_core::settings::Appearance;
use gpui::{App, Global, Hsla, Window, WindowAppearance, hsla, rgb};
use gpui_component::{Theme, ThemeMode};

/// The accent, shared with T3 Code Glass.
const ACCENT: u32 = 0x459bf7;

/// Colors for Brook's own drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Dark mode.
    pub dark: bool,
    /// The window is translucent glass.
    pub glass: bool,
    /// Behind everything: the glass tint, or the opaque window color.
    pub window: Hsla,
    /// The workspace card, dialogs, and popovers.
    pub surface: Hsla,
    /// Primary text.
    pub text: Hsla,
    /// Links, focus rings, unread dots.
    pub accent: Hsla,
}

impl Global for Palette {}

impl Palette {
    /// The palette for a mode. `glass` asks for a translucent window, which
    /// only dark mode uses.
    pub fn new(dark: bool, glass: bool) -> Self {
        let glass = glass && dark;
        if dark {
            Self {
                dark,
                glass,
                window: if glass { hsla(240. / 360., 0.06, 0.08, 0.62) } else { rgb(0x141416).into() },
                surface: rgb(0x1c1c1f).into(),
                text: rgb(0xededef).into(),
                accent: rgb(ACCENT).into(),
            }
        } else {
            Self {
                dark,
                glass,
                window: rgb(0xeeeef0).into(),
                surface: rgb(0xffffff).into(),
                text: rgb(0x1b1b1f).into(),
                accent: rgb(ACCENT).into(),
            }
        }
    }

    /// The text color at `alpha`.
    pub fn ink(&self, alpha: f32) -> Hsla {
        self.text.opacity(alpha)
    }

    /// Hairline borders between regions.
    pub fn stroke(&self) -> Hsla {
        self.ink(0.07)
    }

    /// Fill under a hovered row.
    pub fn hover(&self) -> Hsla {
        self.ink(0.05)
    }

    /// Fill under the selected row.
    pub fn selection(&self) -> Hsla {
        self.ink(if self.dark { 0.10 } else { 0.06 })
    }

    /// Secondary text: metadata, counts, descriptions.
    pub fn muted(&self) -> Hsla {
        self.ink(0.55)
    }

    /// Tertiary text and icons at rest.
    pub fn faint(&self) -> Hsla {
        self.ink(0.38)
    }

    /// Code backgrounds, placeholders, and other quiet fills.
    pub fn well(&self) -> Hsla {
        self.ink(if self.dark { 0.06 } else { 0.04 })
    }
}

/// The current palette.
pub fn palette(cx: &App) -> &Palette {
    cx.global::<Palette>()
}

/// Whether `appearance` resolves to dark, given the system's appearance.
pub fn is_dark(appearance: Appearance, system: WindowAppearance) -> bool {
    match appearance {
        Appearance::Light => false,
        Appearance::Dark => true,
        Appearance::System => matches!(system, WindowAppearance::Dark | WindowAppearance::VibrantDark),
    }
}

/// Whether this platform draws window glass. `BROOK_NO_GLASS` turns it
/// off, for screenshots with a solid background.
pub fn supports_glass() -> bool {
    cfg!(target_os = "macos") && std::env::var_os("BROOK_NO_GLASS").is_none()
}

/// Sets the palette and gpui-component's theme for `appearance`.
pub fn apply(appearance: Appearance, window: &mut Window, cx: &mut App) {
    let dark = is_dark(appearance, window.appearance());
    let palette = Palette::new(dark, supports_glass());
    cx.set_global(palette);

    // Switching mode reloads the stock colors, so overrides come after.
    Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, Some(window), cx);
    Theme::update(cx, |theme| {
        let p = palette;
        let colors = &mut theme.colors;
        colors.background = p.window;
        colors.foreground = p.text;
        colors.border = p.stroke();
        colors.input = p.ink(0.12);
        colors.ring = p.accent;
        colors.caret = p.accent;
        colors.selection = p.accent.opacity(0.28);
        colors.muted = p.well();
        colors.muted_foreground = p.muted();
        colors.accent = p.hover();
        colors.accent_foreground = p.text;
        colors.primary = p.accent;
        colors.primary_hover = p.accent.opacity(0.9);
        colors.primary_active = p.accent.opacity(0.8);
        colors.primary_foreground = rgb(0xffffff).into();
        colors.secondary = p.well();
        colors.secondary_hover = p.selection();
        colors.secondary_active = p.selection();
        colors.secondary_foreground = p.text;
        colors.popover = p.surface;
        colors.popover_foreground = p.text;
        colors.list = p.surface;
        colors.list_hover = p.hover();
        colors.list_active = p.selection();
        colors.list_active_border = p.selection();
        colors.link = p.accent;
        colors.link_hover = p.accent.opacity(0.85);
        colors.link_active = p.accent.opacity(0.7);
        colors.scrollbar = Hsla::transparent_black();
        colors.scrollbar_thumb = p.ink(0.16);
        colors.scrollbar_thumb_hover = p.ink(0.32);
        colors.switch = p.ink(0.16);
        colors.title_bar = Hsla::transparent_black();
        colors.title_bar_border = Hsla::transparent_black();
        colors.window_border = p.stroke();
        colors.overlay = if p.dark { hsla(0., 0., 0., 0.45) } else { hsla(0., 0., 0., 0.18) };
        colors.skeleton = p.well();
        theme.radius = gpui::px(6.);
        theme.radius_lg = gpui::px(10.);
        theme.shadow = !p.dark;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_resolves() {
        assert!(is_dark(Appearance::Dark, WindowAppearance::Light));
        assert!(!is_dark(Appearance::Light, WindowAppearance::Dark));
        assert!(is_dark(Appearance::System, WindowAppearance::VibrantDark));
        assert!(!is_dark(Appearance::System, WindowAppearance::Light));
    }

    #[test]
    fn glass_is_dark_only() {
        assert!(Palette::new(true, true).glass);
        assert!(!Palette::new(false, true).glass);
        assert!(!Palette::new(true, false).glass);
        assert!(Palette::new(true, true).window.a < 1.);
        assert_eq!(Palette::new(true, false).window.a, 1.);
    }

    #[test]
    fn fills_follow_the_glass_rules() {
        let dark = Palette::new(true, false);
        let light = Palette::new(false, false);
        assert!((dark.stroke().a - 0.07).abs() < 1e-6);
        assert!((dark.selection().a - 0.10).abs() < 1e-6);
        assert!((light.selection().a - 0.06).abs() < 1e-6);
        assert!((light.hover().a - 0.05).abs() < 1e-6);
    }
}
