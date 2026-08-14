use gpui::prelude::*;
use gpui::{App, ClickEvent, IntoElement, ParentElement, Styled, Window, div, px};
use gpui_component::{ActiveTheme, StyledExt, button::Button, h_flex, v_flex};
use kmine_engine::{Event, InstanceId, ProgressSink};

pub struct ProgressModal {
    pub id: InstanceId,
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
    let counts = if modal.total == 0 {
        modal.title.clone()
    } else {
        format!("{} — {} / {}", modal.title, modal.done, modal.total)
    };
    div()
        .id("progress-overlay")
        .absolute()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(cx.theme().overlay.opacity(0.5))
        .child(
            v_flex()
                .w(px(420.))
                .gap_4()
                .p_5()
                .rounded(cx.theme().radius_lg)
                .bg(cx.theme().background)
                .border_1()
                .border_color(cx.theme().border)
                .shadow_lg()
                .child(div().font_semibold().child("Preparing"))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(counts),
                )
                .child(
                    h_flex().justify_end().child(
                        Button::new("progress-cancel")
                            .label("Cancel")
                            .on_click(on_cancel),
                    ),
                ),
        )
}
