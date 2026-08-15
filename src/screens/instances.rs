use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window, div, img, px, rgba,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use kmine_engine::{InstanceId, InstanceSummary};
use std::collections::HashSet;
use std::path::Path;

use crate::chrome::{loader_icon, loader_label, loader_tint};

pub struct RenameForm {
    pub id: InstanceId,
    pub name: Entity<InputState>,
}

pub fn sidebar(
    instances: &[InstanceSummary],
    selected: Option<InstanceId>,
    identity: &str,
    skin: Option<&Path>,
    on_select: impl Fn(InstanceId, &mut Window, &mut App) + Clone + 'static,
    on_create: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_rename: impl Fn(InstanceId, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_confirm_rename: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_delete: impl Fn(InstanceId, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_pin: impl Fn(InstanceId, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_accounts: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    renaming: Option<&RenameForm>,
    pinned: &HashSet<InstanceId>,
    cx: &App,
) -> impl IntoElement {
    let mut rows: Vec<&InstanceSummary> = instances.iter().collect();
    rows.sort_by_key(|instance| !pinned.contains(&instance.id));
    let glass = crate::sidebar_is_glass();
    v_flex()
        .id("instance-sidebar")
        .w(px(260.))
        .h_full()
        .flex_shrink_0()
        .when(glass, |this| this.bg(rgba(0x121110d6)))
        .when(!glass, |this| this.bg(cx.theme().sidebar))
        .text_color(cx.theme().sidebar_foreground)
        .child(
            h_flex()
                .h(if glass { px(52.) } else { px(40.) })
                .pl(if glass { px(92.) } else { px(12.) })
                .pr_3()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child("kmine"),
                )
                .child(
                    Button::new("sidebar-create")
                        .ghost()
                        .compact()
                        .icon(IconName::Plus)
                        .tooltip("New instance")
                        .on_click(on_create.clone()),
                ),
        )
        .child(
            v_flex()
                .id("instance-list")
                .flex_1()
                .px_2()
                .gap_1()
                .overflow_y_scroll()
                .when(rows.is_empty(), |this| {
                    this.child(
                        div()
                            .px_3()
                            .py_6()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("No instances yet"),
                    )
                })
                .children(rows.into_iter().map(|instance| {
                    let id = instance.id;
                    let on_select = on_select.clone();
                    let on_rename = on_rename.clone();
                    let on_confirm_rename = on_confirm_rename.clone();
                    let on_delete = on_delete.clone();
                    let on_pin = on_pin.clone();
                    instance_row(
                        instance,
                        selected == Some(id),
                        pinned.contains(&id),
                        renaming.filter(|form| form.id == id),
                        move |_, window, cx| {
                            on_select(id, window, cx);
                        },
                        move |event, window, cx| {
                            cx.stop_propagation();
                            on_rename(id, event, window, cx);
                        },
                        move |event, window, cx| {
                            cx.stop_propagation();
                            on_confirm_rename(event, window, cx);
                        },
                        move |event, window, cx| {
                            cx.stop_propagation();
                            on_delete(id, event, window, cx);
                        },
                        move |event, window, cx| {
                            cx.stop_propagation();
                            on_pin(id, event, window, cx);
                        },
                        cx,
                    )
                })),
        )
        .child(
            v_flex()
                .px_2()
                .pb_2()
                .pt_1()
                .border_t_1()
                .border_color(if glass {
                    rgba(0xffffff1a).into()
                } else {
                    cx.theme().border
                })
                .child(identity_row(identity, skin, on_accounts, cx)),
        )
}

fn identity_row(
    identity: &str,
    skin: Option<&Path>,
    on_accounts: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let signed_in = identity != "Not signed in";
    let subtitle = if signed_in {
        "Microsoft account"
    } else {
        "Add an account"
    };
    h_flex()
        .id("accounts-identity")
        .w_full()
        .px_2()
        .py_2()
        .items_center()
        .justify_between()
        .gap_2()
        .rounded(px(10.))
        .cursor_pointer()
        .hover(|this| {
            this.bg(if crate::sidebar_is_glass() {
                rgba(0xffffff18).into()
            } else {
                cx.theme().muted
            })
        })
        .on_click(on_accounts)
        .child(
            h_flex()
                .min_w_0()
                .items_center()
                .gap_2()
                .child(player_face(skin, cx))
                .child(
                    v_flex()
                        .min_w_0()
                        .child(
                            div()
                                .id("accounts-nick")
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_ellipsis()
                                .child(identity.to_string()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .text_ellipsis()
                                .child(subtitle),
                        ),
                ),
        )
        .child(
            Icon::new(IconName::Settings)
                .text_sm()
                .text_color(cx.theme().muted_foreground),
        )
}

fn player_face(skin: Option<&Path>, cx: &App) -> impl IntoElement {
    let radius = px(7.);
    let face = div()
        .size(px(28.))
        .flex_shrink_0()
        .rounded(radius)
        .overflow_hidden()
        .bg(cx.theme().muted)
        .border_1()
        .border_color(cx.theme().border);
    match skin {
        Some(path) => face.child(img(path.to_path_buf()).size_full().rounded(radius)),
        None => face
            .flex()
            .items_center()
            .justify_center()
            .child(Icon::new(IconName::User).text_color(cx.theme().muted_foreground)),
    }
}

fn instance_row(
    instance: &InstanceSummary,
    selected: bool,
    pinned: bool,
    renaming: Option<&RenameForm>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_rename: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_confirm_rename: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_delete: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_pin: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let key = instance.id.as_hyphenated();
    let row_id = SharedString::from(key.clone());
    let group = SharedString::from(format!("instance-row-{key}"));
    let muted = cx.theme().muted_foreground;
    let editing = renaming.is_some();
    let name_color = if selected {
        cx.theme().foreground
    } else {
        muted
    };
    h_flex()
        .id(row_id)
        .group(group.clone())
        .w_full()
        .px_2()
        .py_1()
        .gap_2()
        .items_center()
        .rounded(px(10.))
        .when(selected && !editing, |this| {
            this.bg(if crate::sidebar_is_glass() {
                rgba(0xffffff22).into()
            } else {
                cx.theme().muted
            })
        })
        .hover(|this| {
            this.bg(if crate::sidebar_is_glass() {
                rgba(0xffffff18).into()
            } else {
                cx.theme().muted
            })
        })
        .cursor_pointer()
        .on_click(on_click)
        .child(instance_mark(instance, cx))
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .when_some(renaming, |this, form| {
                    this.child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .id("inline-rename-input")
                                    .min_w_0()
                                    .flex_1()
                                    .on_click(|_, _, cx| cx.stop_propagation())
                                    .child(Input::new(&form.name).small()),
                            )
                            .child(icon_btn(
                                format!("rename-ok-{}", form.id.as_hyphenated()),
                                IconName::Check,
                                false,
                                on_confirm_rename,
                                cx,
                            )),
                    )
                })
                .when(!editing, |this| {
                    this.child(
                        div()
                            .id(SharedString::from(format!(
                                "instance-name-{}",
                                instance.id.as_hyphenated()
                            )))
                            .text_sm()
                            .font_weight(if selected {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_ellipsis()
                            .text_color(name_color)
                            .child(instance.name.clone()),
                    )
                    .child(div().text_xs().text_color(muted).child(format!(
                        "{} · {}",
                        instance.minecraft_version,
                        loader_label(instance.loader)
                    )))
                }),
        )
        .when(instance.running && !editing, |this| {
            this.child(
                div()
                    .size(px(7.))
                    .rounded_full()
                    .bg(cx.theme().success)
                    .flex_shrink_0(),
            )
        })
        .when(!editing, |this| {
            this.child(
                h_flex()
                    .gap_1()
                    .invisible()
                    .group_hover(group, |style| style.visible())
                    .child(icon_btn(
                        format!("pin-{key}"),
                        asset_icon(if pinned {
                            "icons/pin-fill.svg"
                        } else {
                            "icons/pin.svg"
                        }),
                        pinned,
                        on_pin,
                        cx,
                    ))
                    .child(icon_btn(
                        format!("rename-{key}"),
                        IconName::ALargeSmall,
                        false,
                        on_rename,
                        cx,
                    ))
                    .child(icon_btn(
                        format!("delete-{key}"),
                        asset_icon("icons/trash.svg"),
                        false,
                        on_delete,
                        cx,
                    )),
            )
        })
}

fn instance_mark(instance: &InstanceSummary, cx: &App) -> impl IntoElement {
    let (bg, fg) = loader_tint(instance.loader, cx);
    div()
        .size(px(28.))
        .flex_shrink_0()
        .rounded(px(7.))
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .child(
            Icon::new(loader_icon(instance.loader))
                .text_sm()
                .text_color(fg),
        )
}

fn asset_icon(path: &'static str) -> Icon {
    Icon::empty().path(path)
}

fn icon_btn(
    id: impl Into<SharedString>,
    icon: impl Into<Icon>,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let color = if active {
        cx.theme().foreground
    } else {
        cx.theme().muted_foreground
    };
    div()
        .id(id.into())
        .size(px(22.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|this| this.bg(cx.theme().secondary_hover))
        .on_click(on_click)
        .child(icon.into().text_sm().text_color(color))
}
