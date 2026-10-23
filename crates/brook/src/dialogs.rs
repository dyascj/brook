//! Small modal questions: a name to type, or a yes-or-no.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{App, SharedString, Window, px};
use gpui_component::WindowExt;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_component::input::{Input, InputEvent, InputState};

/// Asks for a line of text. `on_submit` gets the trimmed text, and only
/// when it isn't empty.
pub fn ask_text(
    window: &mut Window,
    cx: &mut App,
    title: impl Into<SharedString>,
    initial: &str,
    confirm: &'static str,
    on_submit: impl Fn(String, &mut Window, &mut App) + 'static,
) {
    let title = title.into();
    let initial = initial.to_owned();
    let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
    let on_submit = Rc::new(on_submit);

    let submit = {
        let (input, on_submit) = (input.clone(), on_submit.clone());
        move |window: &mut Window, cx: &mut App| {
            let text = input.read(cx).value().trim().to_owned();
            if !text.is_empty() {
                on_submit(text, window, cx);
            }
        }
    };
    let submit = Rc::new(submit);

    // Enter submits too.
    let on_enter = submit.clone();
    window
        .subscribe(&input, cx, move |_, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                on_enter(window, cx);
                window.close_dialog(cx);
            }
        })
        .detach();

    input.update(cx, |input, cx| input.focus(window, cx));
    window.open_dialog(cx, move |dialog, _, cx| {
        let dialog = dialog.bg(crate::theme::palette(cx).surface);
        let submit = submit.clone();
        dialog
            .title(title.clone())
            .w(px(380.))
            .child(Input::new(&input))
            .footer(
                DialogFooter::new()
                    .child(DialogClose::new().trigger(|button| button.ghost().label("Cancel")))
                    .child(DialogAction::new().child(Button::new("confirm").primary().label(confirm))),
            )
            .on_ok(move |_, window, cx| {
                submit(window, cx);
                true
            })
    });
}

/// Asks before doing something that can't be undone.
pub fn confirm(
    window: &mut Window,
    cx: &mut App,
    title: impl Into<SharedString>,
    body: impl Into<SharedString>,
    action: &'static str,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
) {
    let (title, body) = (title.into(), body.into());
    let on_confirm = Rc::new(on_confirm);
    window.open_alert_dialog(cx, move |alert, _, cx| {
        let alert = alert.bg(crate::theme::palette(cx).surface);
        let on_confirm = on_confirm.clone();
        alert
            .title(title.clone())
            .description(body.clone())
            .ok_text(action)
            .ok_variant(gpui_component::button::ButtonVariant::Danger)
            .show_cancel(true)
            .on_ok(move |_, window, cx| {
                on_confirm(window, cx);
                true
            })
    });
}
