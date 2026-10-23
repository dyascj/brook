//! Small building blocks shared by the views.

use std::path::Path;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, Hsla, ObjectFit, SharedString, Window, div, img, px,
    svg,
};
use gpui_component::Sizable;
use gpui_component::tooltip::Tooltip;

use crate::theme::palette;

/// A Lucide icon from the asset source, tinted `color`.
pub fn icon(path: &'static str, size: f32, color: Hsla) -> impl IntoElement {
    svg().path(path).size(px(size)).flex_none().text_color(color)
}

/// A quiet square button with an icon: muted at rest, full strength and a
/// faint fill on hover, pressed in slightly when clicked. `tint` colors the
/// icon for an "on" state (a starred star, an active filter).
pub fn icon_button(
    id: impl Into<ElementId>,
    path: &'static str,
    tooltip: &'static str,
    tint: Option<Hsla>,
    cx: &App,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let p = *palette(cx);
    let id = id.into();
    let group = SharedString::from(format!("icon-button-{id:?}"));
    div()
        .id(id)
        .group(group.clone())
        .size(px(28.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .when(tint.is_some(), |button| button.bg(p.selection()))
        .hover(move |style| style.bg(p.hover()))
        .active(move |style| style.bg(p.selection()))
        .child(
            // Icons don't inherit text color, so hover is tracked by group.
            svg()
                .path(path)
                .size(px(15.))
                .text_color(tint.unwrap_or(p.muted()))
                .group_hover(group, move |icon| icon.text_color(tint.unwrap_or(p.text))),
        )
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
        .on_click(on_click)
}

/// A sidebar-style row: icon, label, and an optional trailing count. Text
/// sits at half strength and lifts to full on hover or when `active`.
pub fn nav_row(
    id: impl Into<ElementId>,
    icon_path: &'static str,
    label: impl Into<SharedString>,
    count: Option<u32>,
    active: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let p = *palette(cx);
    let text = if active { p.text } else { p.ink(0.62) };
    div()
        .id(id)
        .h(px(30.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(9.))
        .rounded(px(6.))
        .cursor_pointer()
        .text_color(text)
        .when(active, |row| row.bg(p.selection()))
        .hover(move |row| row.bg(if active { p.selection() } else { p.hover() }).text_color(p.text))
        .child(icon(icon_path, 15., if active { p.text } else { p.ink(0.5) }))
        .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(label.into()))
        .children(count.filter(|&count| count > 0).map(|count| count_label(count, cx)))
}

/// An unread count in faint, even-width digits.
pub fn count_label(count: u32, cx: &App) -> impl IntoElement {
    div()
        .flex_none()
        .text_size(px(11.5))
        .text_color(palette(cx).faint())
        .child(SharedString::from(if count > 999 { "999+".to_owned() } else { count.to_string() }))
}

/// A site icon, or a colored monogram tile while there is none.
pub fn site_icon(path: Option<Arc<Path>>, title: &str, size: f32, _cx: &App) -> AnyElement {
    match path {
        Some(path) => img(path)
            .size(px(size))
            .flex_none()
            .rounded(px(size * 0.22))
            .object_fit(ObjectFit::Contain)
            .with_fallback({
                let title = title.to_owned();
                move || monogram(&title, size).into_any_element()
            })
            .into_any_element(),
        None => monogram(title, size).into_any_element(),
    }
}

/// A rounded tile with the first letter of `title`, colored by a hash of it
/// so each feed keeps its color.
pub fn monogram(title: &str, size: f32) -> impl IntoElement {
    let letter =
        title.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(size * 0.26))
        .bg(monogram_color(title))
        .text_color(gpui::white())
        .text_size(px(size * 0.58))
        .font_weight(FontWeight::BOLD)
        .line_height(px(size))
        .child(letter)
}

/// The tile color for `title`: one of a few calm, saturated hues.
pub fn monogram_color(title: &str) -> Hsla {
    const HUES: &[u32] =
        &[0x459bf7, 0xef6b5b, 0x3fb68b, 0xf2a93b, 0x8b6cf0, 0xe5689f, 0x2bb3c0, 0xf08a3c, 0x6a7ff2, 0x8cbf3f];
    let hash = title.bytes().fold(0u32, |hash, byte| hash.wrapping_mul(31).wrapping_add(u32::from(byte)));
    gpui::rgb(HUES[hash as usize % HUES.len()]).into()
}

/// A small uppercase heading for a group of rows.
pub fn section_label(label: &'static str, cx: &App) -> gpui::Div {
    div()
        .px(px(8.))
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette(cx).faint())
        .child(label)
}

/// A horizontal hairline.
pub fn hairline(cx: &App) -> impl IntoElement {
    div().h(px(1.)).w_full().flex_none().bg(palette(cx).stroke())
}

/// A pill-shaped segmented control.
pub fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: &[(T, &'static str)],
    selected: T,
    cx: &App,
    on_select: impl Fn(T, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let p = *palette(cx);
    let on_select = std::rc::Rc::new(on_select);
    div().flex().flex_none().p(px(2.)).gap(px(2.)).rounded(px(8.)).bg(p.well()).children(
        options.iter().enumerate().map(|(index, &(value, label))| {
            let on_select = on_select.clone();
            let chosen = value == selected;
            div()
                .id((id, index))
                .px(px(10.))
                .h(px(24.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .text_size(px(12.))
                .cursor_pointer()
                .text_color(if chosen { p.text } else { p.muted() })
                .when(chosen, |item| item.bg(p.surface).shadow_sm())
                .when(!chosen, |item| item.hover(move |s| s.text_color(p.text)))
                .child(label)
                .on_click(move |_, window, cx| on_select(value, window, cx))
        }),
    )
}

/// A thin ring that spins, for work in progress.
pub fn spinner(size: f32, cx: &App) -> impl IntoElement {
    gpui_component::spinner::Spinner::new()
        .with_size(gpui_component::Size::Size(px(size)))
        .color(palette(cx).muted())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monogram_colors_are_stable() {
        assert_eq!(monogram_color("The Verge"), monogram_color("The Verge"));
        let distinct: std::collections::HashSet<_> =
            ["a", "b", "c", "d", "e", "f"].iter().map(|t| format!("{:?}", monogram_color(t))).collect();
        assert!(distinct.len() > 3, "titles spread across hues");
    }
}
