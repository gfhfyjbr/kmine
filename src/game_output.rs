use std::sync::Arc;

use gpui::prelude::*;
use gpui::{Context, IntoElement, ParentElement, Render, Styled, WeakEntity, Window, div};
use gpui_component::{ActiveTheme, StyledExt, v_flex};
use kmine_engine::{Engine, Event, InstanceId, LogStream};

pub struct GameOutput {
    instance_id: InstanceId,
    name: String,
    lines: Vec<String>,
}

impl GameOutput {
    pub fn new(
        engine: Arc<Engine>,
        rt: tokio::runtime::Handle,
        instance_id: InstanceId,
        name: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let this = Self {
            instance_id,
            name,
            lines: Vec::new(),
        };
        this.listen(engine, rt, cx);
        this
    }

    fn listen(&self, engine: Arc<Engine>, rt: tokio::runtime::Handle, cx: &mut Context<Self>) {
        let mut rx = engine.subscribe();
        let instance_id = self.instance_id;
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                let joined = rt.spawn(async move {
                    let result = rx.recv().await;
                    (rx, result)
                });
                let (next_rx, result) = match joined.await {
                    Ok(pair) => pair,
                    Err(_) => break,
                };
                rx = next_rx;
                match result {
                    Ok(Event::LogLine {
                        instance_id: id,
                        stream,
                        text,
                    }) if id == instance_id => {
                        let prefix = match stream {
                            LogStream::Stdout => "out",
                            LogStream::Stderr => "err",
                        };
                        if this
                            .update(cx, |this, cx| {
                                this.lines.push(format!("{prefix}: {text}"));
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                }
            }
        })
        .detach();
    }
}

impl Render for GameOutput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .px_3()
                    .py_2()
                    .font_semibold()
                    .child(format!("{} — output", self.name)),
            )
            .child(
                v_flex()
                    .id("game-output-lines")
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .gap_1()
                    .overflow_y_scroll()
                    .children(self.lines.iter().cloned().map(|line| {
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(line)
                    })),
            )
    }
}
