use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{App, Entity, IntoElement, ParentElement, Styled, Window, div};
use gpui_component::{
    ActiveTheme, Disableable, IndexPath,
    input::{Input, InputState},
    select::{Select, SelectState},
    slider::{Slider, SliderState},
    switch::Switch,
    v_flex,
};
use kmine_engine::{
    AccountId, AccountSummary, InstanceId, InstancePatch, InstanceRow, SandboxStatus,
};

use crate::chrome::{card, section_label, status_alert};

pub const DEFAULT_ACCOUNT: &str = "Default account";
const SANDBOX_WARNING: &str = "Native mods and RPC may break.";

const RAM_MIN_MAX: f32 = 16384.0;
const RAM_MAX_MAX: f32 = 32768.0;
const RAM_STEP: f32 = 256.0;

pub struct SettingsForm {
    pub instance_id: InstanceId,
    pub memory_min: Entity<SliderState>,
    pub memory_max: Entity<SliderState>,
    pub jvm_flags: Entity<InputState>,
    pub java_path: Entity<InputState>,
    pub sandbox: bool,
    pub account: Entity<SelectState<Vec<String>>>,
    pub account_labels: Vec<String>,
    pub account_ids: Vec<Option<AccountId>>,
}

impl SettingsForm {
    pub fn from_row(
        row: &InstanceRow,
        accounts: &[AccountSummary],
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let mut account_labels = vec![DEFAULT_ACCOUNT.to_string()];
        let mut account_ids = vec![None];
        for account in accounts {
            account_labels.push(account.username.clone());
            account_ids.push(Some(account.uuid));
        }
        let selected = row
            .account_uuid
            .and_then(|uuid| account_ids.iter().position(|id| *id == Some(uuid)))
            .unwrap_or(0);
        let labels = account_labels.clone();
        Self {
            instance_id: row.id,
            memory_min: cx.new(|_| {
                SliderState::new()
                    .min(0.0)
                    .max(RAM_MIN_MAX)
                    .step(RAM_STEP)
                    .default_value(row.memory_min_mb.unwrap_or(0) as f32)
            }),
            memory_max: cx.new(|_| {
                SliderState::new()
                    .min(0.0)
                    .max(RAM_MAX_MAX)
                    .step(RAM_STEP)
                    .default_value(row.memory_max_mb.unwrap_or(0) as f32)
            }),
            jvm_flags: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Extra JVM flags")
                    .default_value(row.jvm_flags.clone().unwrap_or_default())
            }),
            java_path: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Mojang runtime")
                    .default_value(row.java_path.clone().unwrap_or_default())
            }),
            sandbox: row.sandbox,
            account: cx
                .new(|cx| SelectState::new(labels, Some(IndexPath::new(selected)), window, cx)),
            account_labels,
            account_ids,
        }
    }

    pub fn patch(&self, cx: &App) -> InstancePatch {
        InstancePatch {
            memory_min_mb: Some(ram_from_slider(self.memory_min.read(cx).value().end())),
            memory_max_mb: Some(ram_from_slider(self.memory_max.read(cx).value().end())),
            jvm_flags: Some(optional_text(&self.jvm_flags.read(cx).value())),
            java_path: Some(optional_text(&self.java_path.read(cx).value()).map(PathBuf::from)),
            sandbox: Some(self.sandbox),
            account_uuid: Some(self.selected_account(cx)),
            icon_png: None,
            minecraft_version: None,
            loader: None,
            loader_version: None,
        }
    }

    fn selected_account(&self, cx: &App) -> Option<AccountId> {
        let label = self
            .account
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_else(|| DEFAULT_ACCOUNT.to_string());
        self.account_labels
            .iter()
            .position(|item| item == &label)
            .and_then(|idx| self.account_ids.get(idx).copied())
            .flatten()
    }
}

pub fn settings_tab(
    form: &SettingsForm,
    sandbox_status: &SandboxStatus,
    status: &str,
    on_sandbox: impl Fn(bool, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let sandbox_available = matches!(sandbox_status, SandboxStatus::Available);
    let min_label = ram_label(form.memory_min.read(cx).value().end());
    let max_label = ram_label(form.memory_max.read(cx).value().end());
    v_flex()
        .id("instance-settings")
        .w_full()
        .gap_5()
        .child(
            v_flex()
                .w_full()
                .gap_2()
                .child(section_label("Memory", cx))
                .child(
                    card(cx)
                        .child(field_block(
                            format!("Minimum · {min_label}"),
                            div().w_full().px_1().child(Slider::new(&form.memory_min)),
                            cx,
                        ))
                        .child(field_block(
                            format!("Maximum · {max_label}"),
                            div().w_full().px_1().child(Slider::new(&form.memory_max)),
                            cx,
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Zero leaves the launcher default."),
                        ),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .gap_2()
                .child(section_label("Launch", cx))
                .child(
                    card(cx)
                        .child(field_block(
                            "JVM flags",
                            Input::new(&form.jvm_flags).w_full(),
                            cx,
                        ))
                        .child(field_block(
                            "Java path",
                            Input::new(&form.java_path).w_full(),
                            cx,
                        ))
                        .child(field_block(
                            "Account",
                            Select::new(&form.account).w_full(),
                            cx,
                        )),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .gap_2()
                .child(section_label("Sandbox", cx))
                .child(
                    card(cx)
                        .child(
                            Switch::new("sandbox")
                                .label("Sandbox the game process")
                                .checked(form.sandbox)
                                .disabled(!sandbox_available)
                                .on_click(move |checked, window, cx| {
                                    on_sandbox(*checked, window, cx);
                                }),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(SANDBOX_WARNING.to_string()),
                        )
                        .when(
                            matches!(sandbox_status, SandboxStatus::Unavailable { .. }),
                            |this| {
                                let reason = match sandbox_status {
                                    SandboxStatus::Unavailable { reason } => reason.clone(),
                                    SandboxStatus::Available => String::new(),
                                };
                                this.child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(reason),
                                )
                            },
                        ),
                ),
        )
        .when(!status.is_empty(), |this| {
            this.child(status_alert(status, cx))
        })
}

fn field_block(label: impl Into<String>, field: impl IntoElement, cx: &App) -> impl IntoElement {
    v_flex()
        .w_full()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(label.into()),
        )
        .child(div().w_full().child(field))
}

fn ram_from_slider(value: f32) -> Option<u32> {
    let mb = value.round() as u32;
    if mb == 0 { None } else { Some(mb) }
}

fn ram_label(value: f32) -> String {
    match ram_from_slider(value) {
        None => "Default".into(),
        Some(mb) if mb % 1024 == 0 => format!("{} GB", mb / 1024),
        Some(mb) if mb >= 1024 => format!("{:.1} GB", mb as f32 / 1024.0),
        Some(mb) => format!("{mb} MB"),
    }
}

fn optional_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
