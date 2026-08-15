use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    App, Context, FontWeight, IntoElement, ParentElement, Render, SharedString, Styled, WeakEntity,
    Window, div, px,
};
use gpui_component::{
    ActiveTheme, Disableable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use kmine_engine::{CancellationToken, Engine, EngineError, Event, InstanceId, LogStream};

use crate::modals::progress::EventProgressSink;

const MAX_LINES: usize = 4000;

pub struct GameOutput {
    engine: Arc<Engine>,
    rt: tokio::runtime::Handle,
    instance_id: InstanceId,
    name: String,
    lines: Vec<LogLine>,
    running: bool,
    preparing: bool,
    prepare_status: String,
}

struct LogLine {
    kind: LineKind,
    text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Stderr,
    System,
}

impl GameOutput {
    pub fn new(
        engine: Arc<Engine>,
        rt: tokio::runtime::Handle,
        instance_id: InstanceId,
        name: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let running = engine
            .list_instances()
            .ok()
            .into_iter()
            .flatten()
            .any(|instance| instance.id == instance_id && instance.running);
        let this = Self {
            engine,
            rt,
            instance_id,
            name,
            lines: Vec::new(),
            running,
            preparing: false,
            prepare_status: String::new(),
        };
        this.listen(cx);
        this
    }

    fn listen(&self, cx: &mut Context<Self>) {
        let mut rx = self.engine.subscribe();
        let instance_id = self.instance_id;
        let rt = self.rt.clone();
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
                    Ok(event) => {
                        if this
                            .update(cx, |this, cx| {
                                this.handle_event(event, instance_id);
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                }
            }
        })
        .detach();
    }

    fn handle_event(&mut self, event: Event, instance_id: InstanceId) {
        match event {
            Event::LogLine {
                instance_id: id,
                stream,
                text,
            } if id == instance_id => {
                self.running = true;
                self.push_game_line(stream, text);
            }
            Event::ProcessExited {
                instance_id: id,
                code,
            } if id == instance_id => {
                self.running = false;
                self.preparing = false;
                let detail = match code {
                    Some(0) => "exit code 0".into(),
                    Some(code) => format!("exit code {code}"),
                    None => "killed".into(),
                };
                self.push_system(format!("Instance stopped ({detail})"));
            }
            Event::Progress {
                id,
                title,
                done,
                total,
            } if id == instance_id && self.preparing => {
                self.prepare_status = format_progress(&title, done, total);
            }
            Event::PrepareFinished { id, ok } if id == instance_id && !ok => {
                self.preparing = false;
                self.prepare_status.clear();
            }
            Event::InstancesChanged => self.refresh_running(),
            _ => {}
        }
    }

    fn refresh_running(&mut self) {
        self.running = self
            .engine
            .list_instances()
            .ok()
            .into_iter()
            .flatten()
            .any(|instance| instance.id == self.instance_id && instance.running);
    }

    fn start_instance(&mut self, cx: &mut Context<Self>) {
        if self.running || self.preparing {
            return;
        }
        self.preparing = true;
        self.prepare_status = "Preparing…".into();
        self.push_system("Starting instance…");
        cx.notify();

        let engine = self.engine.clone();
        let rt = self.rt.clone();
        let id = self.instance_id;
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let prepared = rt
                .spawn(async move {
                    let sink = EventProgressSink::new(engine.event_sender(), id);
                    let plan = engine
                        .prepare(id, &sink, CancellationToken::new(), None)
                        .await?;
                    engine.spawn(id, plan)
                })
                .await;
            this.update(cx, |this, cx| {
                this.preparing = false;
                this.prepare_status.clear();
                match prepared {
                    Ok(Ok(_)) => {
                        this.running = true;
                        this.push_system("Instance started");
                    }
                    Ok(Err(EngineError::Cancelled)) => {
                        this.push_system("Start cancelled");
                        this.refresh_running();
                    }
                    Ok(Err(err)) => {
                        this.push_system(format!("Failed to start: {err}"));
                        this.refresh_running();
                    }
                    Err(err) => {
                        this.push_system(format!("Failed to start: {err}"));
                        this.refresh_running();
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn kill_instance(&mut self, cx: &mut Context<Self>) {
        if !self.running {
            return;
        }
        let _ = self.engine.kill(self.instance_id);
        self.push_system("Kill signal sent");
        cx.notify();
    }

    fn push_system(&mut self, text: impl Into<String>) {
        self.push(LogLine {
            kind: LineKind::System,
            text: text.into(),
        });
    }

    fn push_game_line(&mut self, stream: LogStream, text: String) {
        let kind = classify_line(stream, &text);
        self.push(LogLine { kind, text });
    }

    fn push(&mut self, line: LogLine) {
        self.lines.push(line);
        if self.lines.len() > MAX_LINES {
            let extra = self.lines.len() - MAX_LINES;
            self.lines.drain(..extra);
        }
    }
}

impl Render for GameOutput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = if self.preparing {
            if self.prepare_status.is_empty() {
                "Preparing".into()
            } else {
                self.prepare_status.clone()
            }
        } else if self.running {
            "Running".into()
        } else {
            "Stopped".into()
        };
        let status_color = if self.preparing {
            cx.theme().warning
        } else if self.running {
            cx.theme().success
        } else {
            cx.theme().muted_foreground
        };

        v_flex()
            .size_full()
            .p_3()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().foreground)
            .child(
                v_flex()
                    .size_full()
                    .rounded(px(16.))
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .w_full()
                            .px_4()
                            .py_3()
                            .gap_3()
                            .items_center()
                            .justify_between()
                            .flex_shrink_0()
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(self.name.clone()),
                                    )
                                    .child(div().text_sm().text_color(status_color).child(status)),
                            )
                            .child(header_action(self, cx)),
                    )
                    .child(
                        v_flex()
                            .id("game-output-lines")
                            .flex_1()
                            .min_h_0()
                            .m_3()
                            .mt_0()
                            .p_3()
                            .gap_1()
                            .rounded(cx.theme().radius_lg)
                            .bg(cx.theme().secondary)
                            .font_family(SharedString::from("Menlo"))
                            .overflow_y_scroll()
                            .children(self.lines.iter().map(|line| render_line(line, cx))),
                    ),
            )
    }
}

fn header_action(this: &GameOutput, cx: &mut Context<GameOutput>) -> impl IntoElement {
    if this.preparing {
        return Button::new("output-starting")
            .label("Starting…")
            .disabled(true)
            .into_any_element();
    }
    if this.running {
        let entity = cx.weak_entity();
        return Button::new("output-kill")
            .danger()
            .label("Kill instance")
            .on_click(move |_, _, cx| {
                entity.update(cx, |this, cx| this.kill_instance(cx)).ok();
            })
            .into_any_element();
    }
    let entity = cx.weak_entity();
    Button::new("output-start")
        .primary()
        .label("Start instance")
        .on_click(move |_, _, cx| {
            entity.update(cx, |this, cx| this.start_instance(cx)).ok();
        })
        .into_any_element()
}

fn render_line(line: &LogLine, cx: &App) -> impl IntoElement {
    let color = match line.kind {
        LineKind::Error => cx.theme().danger,
        LineKind::Warn => cx.theme().warning,
        LineKind::Info => cx.theme().foreground,
        LineKind::Debug | LineKind::Trace => cx.theme().muted_foreground,
        LineKind::Stderr => cx.theme().warning,
        LineKind::System => cx.theme().success,
    };
    div().text_sm().text_color(color).child(line.text.clone())
}

fn classify_line(stream: LogStream, text: &str) -> LineKind {
    if let Some(level) = parse_level(text) {
        return level;
    }
    match stream {
        LogStream::Stderr => LineKind::Stderr,
        LogStream::Stdout => LineKind::Info,
    }
}

fn parse_level(text: &str) -> Option<LineKind> {
    let rest = text.strip_prefix('[')?;
    let (level, _) = rest.split_once(']')?;
    match level.trim().to_ascii_uppercase().as_str() {
        "TRACE" => Some(LineKind::Trace),
        "DEBUG" => Some(LineKind::Debug),
        "INFO" => Some(LineKind::Info),
        "WARN" | "WARNING" => Some(LineKind::Warn),
        "ERROR" | "FATAL" => Some(LineKind::Error),
        _ => None,
    }
}

fn format_progress(title: &str, done: u64, total: u64) -> String {
    if total == 0 {
        title.to_string()
    } else if total >= 1_000_000 {
        format!("{title} — {} / {}", fmt_bytes(done), fmt_bytes(total))
    } else {
        format!("{title} — {done} / {total}")
    }
}

fn fmt_bytes(n: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / MB)
    } else if n >= 1024 {
        format!("{:.0} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}
