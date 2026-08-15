use gpui::prelude::*;
use gpui::{App, ClickEvent, FontWeight, IntoElement, ParentElement, Styled, Window, div, px};
use gpui_component::{
    ActiveTheme, Sizable,
    button::Button,
    h_flex,
    progress::{Progress, ProgressCircle},
    spinner::Spinner,
    v_flex,
};
use kmine_engine::{Event, InstanceId, ProgressSink};

pub struct ProgressModal {
    pub id: InstanceId,
    pub name: String,
    pub title: String,
    pub done: u64,
    pub total: u64,
}

pub struct EventProgressSink {
    tx: tokio::sync::broadcast::Sender<Event>,
    id: InstanceId,
}

impl EventProgressSink {
    pub fn new(tx: tokio::sync::broadcast::Sender<Event>, id: InstanceId) -> Self {
        Self { tx, id }
    }
}

impl ProgressSink for EventProgressSink {
    fn set(&self, title: &str, done: u64, total: u64) {
        let _ = self.tx.send(Event::Progress {
            id: self.id,
            title: title.to_string(),
            done,
            total,
        });
    }
}

pub fn render(
    modal: &ProgressModal,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let counts = format_status(&modal.title, modal.done, modal.total);
    let value = if modal.total == 0 {
        0.0
    } else {
        (modal.done as f32 / modal.total as f32) * 100.0
    };
    let indeterminate = modal.total == 0;
    let percent = if indeterminate {
        String::new()
    } else {
        format!("{:.0}%", value)
    };

    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom_6()
        .flex()
        .justify_center()
        .px_6()
        .child(
            h_flex()
                .id("progress-status")
                .w(px(440.))
                .px_4()
                .py_3()
                .gap_4()
                .items_center()
                .rounded(cx.theme().radius_lg)
                .bg(cx.theme().popover)
                .border_1()
                .border_color(cx.theme().border)
                .shadow_lg()
                .child(if indeterminate {
                    div()
                        .size(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Spinner::new().color(cx.theme().foreground))
                        .into_any_element()
                } else {
                    ProgressCircle::new("prepare-circle")
                        .value(value)
                        .small()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .child(percent),
                        )
                        .into_any_element()
                })
                .child(
                    v_flex()
                        .min_w_0()
                        .flex_1()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_ellipsis()
                                .child(format!("Preparing {}", modal.name)),
                        )
                        .child(
                            Progress::new("prepare-progress")
                                .small()
                                .value(value)
                                .loading(indeterminate),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .text_ellipsis()
                                .child(counts),
                        ),
                )
                .child(
                    Button::new("progress-cancel")
                        .outline()
                        .label("Cancel")
                        .on_click(on_cancel),
                ),
        )
}

fn format_status(title: &str, done: u64, total: u64) -> String {
    if total == 0 {
        title.to_string()
    } else if total >= 1_000_000 {
        format!(
            "{}  {} / {}",
            title,
            format_bytes(done),
            format_bytes(total)
        )
    } else {
        format!("{}  {} / {}", title, done, total)
    }
}

fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    if n >= 100 * 1024 * 1024 {
        format!("{:.0} MB", n as f64 / MB)
    } else if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / MB)
    } else if n >= 1024 {
        format!("{:.0} KB", n as f64 / KB)
    } else {
        format!("{n} B")
    }
}
