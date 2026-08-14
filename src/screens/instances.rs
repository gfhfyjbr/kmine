use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Entity, InteractiveElement, IntoElement, ParentElement, SharedString, Styled,
    Window, div, px,
};
use gpui_component::{
    ActiveTheme, StyledExt,
    button::{Button, ButtonVariants},
    form::{field, v_form},
    h_flex,
    input::{Input, InputState},
    list::ListItem,
    v_flex,
};
use kmine_engine::{InstanceId, InstanceSummary};

pub struct RenameForm {
    pub id: InstanceId,
    pub name: Entity<InputState>,
}

pub fn sidebar(
    instances: &[InstanceSummary],
    selected: Option<InstanceId>,
    on_select: impl Fn(InstanceId, &mut Window, &mut App) + Clone + 'static,
    on_create: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_rename: impl Fn(InstanceId, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_delete: impl Fn(InstanceId, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .id("instance-sidebar")
        .w(px(260.))
        .h_full()
        .flex_shrink_0()
        .bg(cx.theme().sidebar)
        .text_color(cx.theme().sidebar_foreground)
        .border_r_1()
        .border_color(cx.theme().sidebar_border)
        .child(h_flex().px_3().py_3().font_semibold().child("kmine"))
        .child(
            v_flex()
                .id("instance-list")
                .flex_1()
                .px_2()
                .gap_1()
                .overflow_y_scroll()
                .children(instances.iter().map(|instance| {
                    let id = instance.id;
                    let on_select = on_select.clone();
                    let on_rename = on_rename.clone();
                    let on_delete = on_delete.clone();
                    instance_row(
                        instance,
                        selected == Some(id),
                        move |_, window, cx| {
                            on_select(id, window, cx);
                        },
                        move |event, window, cx| {
                            cx.stop_propagation();
                            on_rename(id, event, window, cx);
                        },
                        move |event, window, cx| {
                            cx.stop_propagation();
                            on_delete(id, event, window, cx);
                        },
                        cx,
                    )
                })),
        )
        .child(
            div().p_2().child(
                Button::new("create-instance")
                    .w_full()
                    .label("+ Create")
                    .on_click(on_create),
            ),
        )
}

fn instance_row(
    instance: &InstanceSummary,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_rename: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_delete: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> ListItem {
    let rename_id = SharedString::from(format!("rename-{}", instance.id.as_hyphenated()));
    let delete_id = SharedString::from(format!("delete-{}", instance.id.as_hyphenated()));
    let muted = cx.theme().muted_foreground;
    ListItem::new(SharedString::from(instance.id.as_hyphenated()))
        .selected(selected)
        .child(
            v_flex()
                .w_full()
                .gap_1()
                .py_1()
                .child(div().font_semibold().child(instance.name.clone()))
                .child(div().text_sm().text_color(muted).child(format!(
                    "{} · {}",
                    instance.minecraft_version,
                    instance.loader.as_str()
                )))
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(last_played_label(instance.last_played_at)),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new(rename_id)
                                .ghost()
                                .compact()
                                .label("Rename")
                                .on_click(move |event, window, cx| {
                                    cx.stop_propagation();
                                    on_rename(event, window, cx);
                                }),
                        )
                        .child(
                            Button::new(delete_id)
                                .ghost()
                                .compact()
                                .label("Delete")
                                .on_click(move |event, window, cx| {
                                    cx.stop_propagation();
                                    on_delete(event, window, cx);
                                }),
                        ),
                ),
        )
        .on_click(on_click)
}

pub fn rename_overlay(
    form: &RenameForm,
    status: &str,
    on_submit: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    div()
        .id("rename-instance-overlay")
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
                .child(div().font_semibold().child("Rename instance"))
                .child(v_form().child(field().label("Name").child(Input::new(&form.name))))
                .when(!status.is_empty(), |this| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(status.to_string()),
                    )
                })
                .child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("rename-cancel")
                                .label("Cancel")
                                .on_click(on_cancel),
                        )
                        .child(
                            Button::new("rename-submit")
                                .primary()
                                .label("Rename")
                                .on_click(on_submit),
                        ),
                ),
        )
}

pub fn delete_overlay(
    name: &str,
    status: &str,
    on_confirm: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    div()
        .id("delete-instance-overlay")
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
                .child(div().font_semibold().child("Delete instance"))
                .child(
                    div()
                        .text_sm()
                        .child(format!("Delete “{name}”? This cannot be undone.")),
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
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("delete-cancel")
                                .label("Cancel")
                                .on_click(on_cancel),
                        )
                        .child(
                            Button::new("delete-confirm")
                                .primary()
                                .label("Delete")
                                .on_click(on_confirm),
                        ),
                ),
        )
}

fn last_played_label(ts_ms: Option<i64>) -> String {
    match civil_date_utc(ts_ms) {
        Some((year, month, day)) => format!("Last played {year:04}-{month:02}-{day:02}"),
        None => "Never played".into(),
    }
}

fn civil_date_utc(ts_ms: Option<i64>) -> Option<(u32, u32, u32)> {
    let ms = ts_ms?;
    let secs = u64::try_from(ms.div_euclid(1000)).ok()?;
    let mut rem = secs / 86400;
    let mut year = 1970u32;
    loop {
        let len = if is_leap(year) { 366 } else { 365 };
        if rem < len {
            break;
        }
        rem -= len;
        year = year.checked_add(1)?;
        if year > 9999 {
            return None;
        }
    }
    let leap = is_leap(year);
    let dims = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u32;
    for (i, dim) in dims.iter().enumerate() {
        if rem < *dim {
            month = (i as u32) + 1;
            break;
        }
        rem -= *dim;
    }
    Some((year, month, rem as u32 + 1))
}

fn is_leap(year: u32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}
