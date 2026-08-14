use gpui::prelude::*;
use gpui::{App, ClickEvent, IntoElement, ParentElement, Styled, Window, div};
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
    h_flex()
        .id("progress-status")
        .w_full()
        .px_4()
        .py_2()
        .gap_3()
        .items_center()
        .justify_between()
        .flex_shrink_0()
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .child(
            v_flex()
                .min_w_0()
                .gap_1()
                .child(div().font_semibold().child("Preparing"))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(counts),
                ),
        )
        .child(
            Button::new("progress-cancel")
                .label("Cancel")
                .on_click(on_cancel),
        )
}
