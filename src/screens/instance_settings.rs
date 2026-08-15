use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{App, ClickEvent, Entity, IntoElement, ParentElement, Styled, Window, div, px};
use gpui_component::{
    ActiveTheme, Disableable, IndexPath, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputState},
    select::{Select, SelectState},
    slider::{Slider, SliderState},
    v_flex,
};
use kmine_engine::{
    AccountId, AccountSummary, InstanceId, InstancePatch, InstanceRow, SandboxStatus,
};

use crate::chrome::section_label;

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
    on_save: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
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
                .gap_3()
                .child(section_label("Memory", cx))
                .child(field_block(
                    format!("Minimum ({min_label})"),
                    div()
                        .w_full()
                        .px_2()
                        .py_2()
                        .child(Slider::new(&form.memory_min)),
                    cx,
                ))
                .child(field_block(
                    format!("Maximum ({max_label})"),
                    div()
                        .w_full()
                        .px_2()
                        .py_2()
                        .child(Slider::new(&form.memory_max)),
                    cx,
                )),
        )
        .child(
            v_flex()
                .w_full()
                .gap_3()
                .child(section_label("Launch", cx))
                .child(field_block(
                    "JVM flags",
                    Input::new(&form.jvm_flags).small(),
                    cx,
                ))
                .child(field_block(
                    "Java path",
                    Input::new(&form.java_path).small(),
                    cx,
                ))
                .child(field_block("Account", Select::new(&form.account), cx)),
        )
        .child(
            v_flex()
                .w_full()
                .gap_2()
                .p_4()
                .rounded(cx.theme().radius_lg)
                .bg(cx.theme().muted)
                .child(
                    Checkbox::new("sandbox")
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
        )
        .when(!status.is_empty(), |this| {
            this.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(status.to_string()),
            )
        })
        .child(
            h_flex().justify_end().child(
                Button::new("settings-save")
                    .primary()
                    .label("Save")
                    .w(px(120.))
                    .on_click(on_save),
            ),
        )
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
        .child(field)
}

fn ram_from_slider(value: f32) -> Option<u32> {
    let mb = value.round() as u32;
    if mb == 0 { None } else { Some(mb) }
}

fn ram_label(value: f32) -> String {
    match ram_from_slider(value) {
        None => "default".into(),
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
