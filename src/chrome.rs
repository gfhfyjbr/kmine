use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::prelude::*;

use gpui::{
    Animation, AnimationExt, App, ClickEvent, Div, ElementId, FontWeight,
    InteractiveElement, IntoElement, MouseButton, ObjectFit, ParentElement, Styled, StyledImage,
    Window, div, img, px,
};
use gpui_component::{
    alert::Alert,
    animation::cubic_bezier,
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Icon, IconName,
};
use kmine_engine::Loader;

/// Shared enter/transition motion. Longer settle than a snap, ease-out that
/// glides into place instead of slamming.
pub fn motion() -> Animation {
    Animation::new(Duration::from_millis(400)).with_easing(cubic_bezier(0.16, 1., 0.3, 1.))
}

/// Primary action: white capsule, system type, extra horizontal padding.
pub fn cta(id: impl Into<ElementId>) -> Button {
    style_cta(Button::new(id).primary())
}

pub fn style_cta(button: Button) -> Button {
    button.rounded(px(999.)).px_4()
}

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
        .with_animation("modal-fade", motion(), |this, delta| this.opacity(delta))
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

pub fn sheet_wide(cx: &App) -> impl ParentElement + Styled + IntoElement {
    sheet(cx).w(px(880.)).max_h(px(720.))
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
    empty_block(icon, title_text, hint, px(8.), px(8.), cx)
}

pub fn empty_list(
    icon: IconName,
    title_text: impl Into<String>,
    hint: impl Into<String>,
    cx: &App,
) -> impl IntoElement {
    empty_block(icon, title_text, hint, px(20.), px(4.), cx)
}

fn empty_block(
    icon: IconName,
    title_text: impl Into<String>,
    hint: impl Into<String>,
    pad_y: gpui::Pixels,
    icon_pad: gpui::Pixels,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .items_center()
        .justify_center()
        .gap_1()
        .px_4()
        .py(pad_y)
        .child(
            div()
                .size(px(36.))
                .rounded(px(10.))
                .bg(cx.theme().secondary_active)
                .flex()
                .items_center()
                .justify_center()
                .mb(icon_pad)
                .child(
                    Icon::new(icon)
                        .text_sm()
                        .text_color(cx.theme().muted_foreground),
                ),
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
                .text_center()
                .child(hint.into()),
        )
}

pub fn loader_icon(loader: Loader) -> IconName {
    match loader {
        Loader::Vanilla => IconName::Globe,
        Loader::Fabric => IconName::Frame,
        Loader::Forge => IconName::Cpu,
        Loader::NeoForge => IconName::Cpu,
        Loader::Quilt => IconName::Frame,
    }
}

pub fn loader_tint(loader: Loader, cx: &App) -> (gpui::Hsla, gpui::Hsla) {
    match loader {
        Loader::Vanilla => (cx.theme().muted, cx.theme().muted_foreground),
        Loader::Fabric | Loader::Quilt => (gpui::rgb(0x243036).into(), gpui::rgb(0xb7c9cc).into()),
        Loader::Forge | Loader::NeoForge => {
            (gpui::rgb(0x30261f).into(), gpui::rgb(0xd0b8a0).into())
        }
    }
}

pub fn default_cover(loader: Loader) -> &'static str {
    match loader {
        Loader::Vanilla => "icons/covers/vanilla.jpg",
        Loader::Fabric => "icons/covers/fabric.jpg",
        Loader::Forge => "icons/covers/forge.jpg",
        Loader::NeoForge => "icons/covers/neoforge.jpg",
        Loader::Quilt => "icons/covers/quilt.jpg",
    }
}

pub fn instance_cover(
    cover: Option<&Path>,
    loader: Loader,
    size: f32,
    cx: &App,
) -> impl IntoElement {
    let radius = px((size * 0.22).clamp(6.0, 12.0));
    let image = match cover {
        Some(path) => img(path.to_path_buf()),
        None => img(default_cover(loader)),
    };
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded(radius)
        .overflow_hidden()
        .border_1()
        .border_color(cx.theme().border.opacity(0.55))
        .bg(cx.theme().secondary_active)
        .child(
            image
                .size_full()
                .object_fit(ObjectFit::Cover)
                .rounded(radius),
        )
}

pub fn section_label(text: impl Into<String>, cx: &App) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub fn section_header(text: impl Into<String>, count: Option<usize>, cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_center()
        .gap_2()
        .child(section_label(text, cx))
        .when_some(count.filter(|&n| n > 0), |this, count| {
            this.child(count_badge(count, cx))
        })
}

fn count_badge(count: usize, cx: &App) -> impl IntoElement {
    div()
        .h(px(16.))
        .min_w(px(18.))
        .px(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .bg(cx.theme().secondary_active)
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(count.to_string())
}

pub fn chip(text: impl Into<String>, cx: &App) -> impl IntoElement {
    div()
        .h(px(22.))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(6.))
        .bg(cx.theme().secondary_active)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub fn running_mark(height: f32, cx: &App) -> impl IntoElement {
    div()
        .w(px(2.))
        .h(px(height))
        .rounded_full()
        .bg(cx.theme().success)
        .flex_shrink_0()
}

pub fn running_pill(cx: &App) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap_1()
        .px_2()
        .h(px(22.))
        .rounded(px(6.))
        .bg(cx.theme().success.opacity(0.16))
        .child(running_mark(10.0, cx))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().success)
                .child("Running"),
        )
}

pub fn list_frame(cx: &App) -> Div {
    v_flex()
        .w_full()
        .rounded(px(10.))
        .bg(cx.theme().muted)
        .overflow_hidden()
}

pub fn list_row_corners<T: Styled>(this: T, first: bool, last: bool) -> T {
    let radius = px(10.);
    let none = px(0.);
    this.rounded(none)
        .rounded_tl(if first { radius } else { none })
        .rounded_tr(if first { radius } else { none })
        .rounded_bl(if last { radius } else { none })
        .rounded_br(if last { radius } else { none })
}

pub fn card(cx: &App) -> impl ParentElement + Styled + IntoElement {
    v_flex()
        .w_full()
        .gap_3()
        .p_4()
        .rounded(cx.theme().radius_lg)
        .bg(cx.theme().muted)
}

pub fn row_rule(cx: &App) -> impl IntoElement {
    div().h(px(1.)).w_full().bg(cx.theme().border.opacity(0.7))
}

pub fn segmented(id: impl Into<ElementId>, cx: &App) -> impl ParentElement + Styled + IntoElement {
    h_flex()
        .id(id)
        .w_full()
        .p(px(3.))
        .gap_1()
        .rounded(px(10.))
        .bg(cx.theme().muted)
}

pub fn segment(
    id: impl Into<ElementId>,
    label: impl Into<String>,
    icon: Option<IconName>,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    filled_segment(id, label, icon, active, true, on_click, cx)
}

pub fn filled_segment(
    id: impl Into<ElementId>,
    label: impl Into<String>,
    icon: Option<IconName>,
    active: bool,
    filled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let (bg, fg) = if active {
        (
            if filled {
                cx.theme().secondary_hover
            } else {
                cx.theme().transparent
            },
            cx.theme().foreground,
        )
    } else {
        (cx.theme().transparent, cx.theme().muted_foreground)
    };
    h_flex()
        .id(id)
        .flex_1()
        .h(px(28.))
        .px_3()
        .items_center()
        .justify_center()
        .gap_1()
        .rounded(px(8.))
        .bg(bg)
        .text_color(fg)
        .cursor_pointer()
        .when(!active, |this| {
            this.hover(|this| this.text_color(cx.theme().foreground))
        })
        .on_click(on_click)
        .when_some(icon, |this, icon| {
            this.child(Icon::new(icon).text_sm().text_color(fg))
        })
        .child(
            div()
                .text_sm()
                .font_weight(if active {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .child(label.into()),
        )
}

pub const FILES_VERIFIED: &str = "Files verified";

pub fn status_alert(
    message: &str,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    if is_busy_status(message) {
        div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(message.to_string())
            .into_any_element()
    } else if is_success_status(message) {
        Alert::success("status-ok", message.to_string())
            .on_close(on_close)
            .into_any_element()
    } else {
        Alert::error("status-error", message.to_string())
            .on_close(on_close)
            .into_any_element()
    }
}

pub fn is_busy_status(status: &str) -> bool {
    status.ends_with('…') || status.ends_with("...")
}

pub fn is_success_status(status: &str) -> bool {
    status == FILES_VERIFIED
}

pub fn loader_label(loader: Loader) -> &'static str {
    match loader {
        Loader::Vanilla => "Vanilla",
        Loader::Fabric => "Fabric",
        Loader::Forge => "Forge",
        Loader::NeoForge => "NeoForge",
        Loader::Quilt => "Quilt",
    }
}

pub fn format_playtime(secs: u64) -> String {
    if secs == 0 {
        return "No playtime yet".into();
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
        return "Never launched".into();
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

#[cfg(test)]
mod tests {
    use super::{FILES_VERIFIED, is_busy_status, is_success_status};

    #[test]
    fn files_verified_is_success_not_busy() {
        assert!(is_success_status(FILES_VERIFIED));
        assert!(!is_busy_status(FILES_VERIFIED));
        assert!(!is_success_status("instance not found"));
    }
}
