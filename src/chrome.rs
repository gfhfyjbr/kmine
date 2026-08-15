use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, App, ClickEvent, Div, ElementId, FontWeight, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Styled, Window, div, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName,
    animation::cubic_bezier,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use kmine_engine::Loader;

pub fn dimmer(cx: &App) -> Div {
    div()
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(cx.theme().overlay.opacity(0.62))
}

pub fn modal(
    id: impl Into<ElementId>,
    dismissible: bool,
    on_dismiss: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl ParentElement + IntoElement {
    dimmer(cx)
        .id(id)
        .when(dismissible, |this| {
            this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                on_dismiss(&ClickEvent::default(), window, cx);
            })
        })
        .with_animation(
            "modal-fade",
            Animation::new(Duration::from_millis(220))
                .with_easing(cubic_bezier(0.32, 0.72, 0., 1.)),
            |this, delta| this.opacity(delta),
        )
}

pub fn sheet(cx: &App) -> impl ParentElement + Styled + IntoElement {
    v_flex()
        .id("modal-sheet")
        .w(px(440.))
        .relative()
        .rounded(cx.theme().radius_lg)
        .bg(cx.theme().popover)
        .border_1()
        .border_color(cx.theme().border)
        .shadow_lg()
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub fn modal_header(
    icon: IconName,
    title_text: impl Into<String>,
    description: impl Into<String>,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_start()
        .gap_3()
        .px_5()
        .pt_5()
        .pr(px(48.))
        .child(
            div()
                .size(px(36.))
                .flex_shrink_0()
                .rounded(px(10.))
                .bg(cx.theme().muted)
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon).text_sm().text_color(cx.theme().foreground)),
        )
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .gap_1()
                .child(title(title_text))
                .child(subtitle(description, cx)),
        )
}

pub fn modal_close(
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div().absolute().top(px(14.)).right(px(14.)).child(
        Button::new("modal-close")
            .ghost()
            .compact()
            .icon(IconName::Close)
            .on_click(on_close),
    )
}

pub fn modal_body() -> impl ParentElement + Styled + IntoElement {
    v_flex().w_full().px_5().py_4().gap_4()
}

pub fn modal_footer(cx: &App) -> impl ParentElement + Styled + IntoElement {
    h_flex()
        .w_full()
        .px_5()
        .py_3()
        .justify_end()
        .gap_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted.opacity(0.35))
}

pub fn title(text: impl Into<String>) -> impl IntoElement {
    div()
        .text_lg()
        .font_weight(FontWeight::MEDIUM)
        .child(text.into())
}

pub fn subtitle(text: impl Into<String>, cx: &App) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub fn empty_panel(
    icon: IconName,
    title_text: impl Into<String>,
    hint: impl Into<String>,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .items_center()
        .justify_center()
        .gap_2()
        .px_3()
        .py_8()
        .child(
            div()
                .size(px(40.))
                .rounded_full()
                .bg(cx.theme().muted)
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon).text_color(cx.theme().muted_foreground)),
        )
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(title_text.into()),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(hint.into()),
        )
}

pub fn loader_icon(loader: Loader) -> IconName {
    match loader {
        Loader::Vanilla => IconName::Globe,
        Loader::Fabric => IconName::Frame,
        Loader::Forge => IconName::Cpu,
    }
}

pub fn loader_tint(loader: Loader, cx: &App) -> (gpui::Hsla, gpui::Hsla) {
    match loader {
        Loader::Vanilla => (cx.theme().muted, cx.theme().muted_foreground),
        Loader::Fabric => (gpui::rgb(0x243036).into(), gpui::rgb(0xb7c9cc).into()),
        Loader::Forge => (gpui::rgb(0x30261f).into(), gpui::rgb(0xd0b8a0).into()),
    }
}

pub fn section_label(text: impl Into<String>, cx: &App) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub fn chip(text: impl Into<String>, cx: &App) -> impl IntoElement {
    div()
        .h(px(22.))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(6.))
        .bg(cx.theme().muted)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub fn loader_label(loader: Loader) -> &'static str {
    match loader {
        Loader::Vanilla => "Vanilla",
        Loader::Fabric => "Fabric",
        Loader::Forge => "Forge",
    }
}

pub fn format_playtime(secs: u64) -> String {
    if secs == 0 {
        return "No playtime".into();
    }
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    if hours == 0 {
        format!("{minutes}m played")
    } else if minutes == 0 {
        format!("{hours}h played")
    } else {
        format!("{hours}h {minutes}m played")
    }
}

pub fn format_last_played(ms: Option<i64>) -> String {
    let Some(ms) = ms else {
        return "Never played".into();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(ms);
    let secs = ((now - ms).max(0)) / 1000;
    if secs < 45 {
        "Just now".into()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else if secs < 86_400 * 14 {
        format!("{}d ago", secs / 86_400)
    } else {
        format!("{}w ago", secs / (86_400 * 7))
    }
}
