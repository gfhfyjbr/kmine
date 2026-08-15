use std::rc::Rc;

use gpui::{App, Styled, Window};
use gpui_component::{
    ActiveTheme, Icon, IconName, WindowExt, button::ButtonVariant, dialog::DialogButtonProps,
};

pub fn danger(
    window: &mut Window,
    cx: &mut App,
    title: impl Into<String>,
    description: impl Into<String>,
    action: impl Into<String>,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
) {
    let title = title.into();
    let description = description.into();
    let action = action.into();
    let on_confirm = Rc::new(on_confirm);
    window.open_alert_dialog(cx, move |alert, _, cx| {
        let on_confirm = on_confirm.clone();
        alert
            .icon(Icon::new(IconName::TriangleAlert).text_color(cx.theme().danger))
            .title(title.clone())
            .description(description.clone())
            .button_props(
                DialogButtonProps::default()
                    .ok_text(action.clone())
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text("Keep")
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| {
                on_confirm(window, cx);
                true
            })
    });
}
