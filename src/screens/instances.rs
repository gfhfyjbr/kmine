use gpui::prelude::*;
use gpui::{
    div, img, px, rgb, rgba, App, ClickEvent, Entity, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    v_flex, ActiveTheme, Icon, IconName, Sizable,
};
use kmine_engine::{InstanceId, InstanceSummary};
use std::collections::HashSet;
use std::path::Path;

use crate::chrome::{instance_cover, loader_label};
use crate::smooth_scroll::SmoothScroll;

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
    on_settings: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    renaming: Option<&RenameForm>,
    pinned: &HashSet<InstanceId>,
    scroll: &SmoothScroll,
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
                        .icon(IconName::Plus)
                        .with_size(px(28.))
                        .rounded(px(8.))
                        .tooltip("New instance")
                        .on_click(on_create.clone()),
                ),
        )
        .child(
            scroll
                .vertical(v_flex().id("instance-list").flex_1().px_2().gap_1())
                .when(rows.is_empty(), |this| {
                    this.child(
                        div()
                            .px_3()
                            .py_6()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("No instances"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Create one to get started"),
                            ),
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
        .child(identity_footer(
            identity,
            skin,
            on_accounts,
            on_settings,
            cx,
        ))
}

fn identity_footer(
    identity: &str,
    skin: Option<&Path>,
    on_accounts: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_settings: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let signed_in = identity != "Not signed in";
    let subtitle = if signed_in {
        "Microsoft account"
    } else {
        "Add an account"
    };
    let glass = crate::sidebar_is_glass();
    h_flex()
        .id("accounts-identity")
        .w_full()
        .px_3()
        .py_2()
        .items_center()
        .gap_2()
        .border_t_1()
        .border_color(if glass {
            rgba(0xffffff1a).into()
        } else {
            cx.theme().border
        })
        .cursor_pointer()
        .hover(|this| {
            this.bg(if glass {
                rgba(0xffffff18).into()
            } else {
                cx.theme().muted
            })
        })
        .on_click(on_accounts)
        .child(player_face(skin, cx))
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
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
        )
        .child(
            Button::new("sidebar-settings")
                .ghost()
                .compact()
                .icon(IconName::Settings)
                .tooltip("Settings")
                .on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    on_settings(event, window, cx);
                }),
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
    let name_color = cx.theme().foreground;
    const ACTIONS_W: f32 = 80.0;
    h_flex()
        .id(row_id)
        .group(group.clone())
        .relative()
        .w_full()
        .px_2()
        .py(px(6.))
        .gap_2()
        .items_center()
        .rounded(px(10.))
        .when(selected && !editing, |this| {
            let fill: gpui::Hsla = if crate::sidebar_is_glass() {
                rgba(0xffffff20).into()
            } else {
                rgb(0x2a2824).into()
            };
            this.bg(fill)
        })
        .hover(|this| {
            this.bg(if crate::sidebar_is_glass() {
                rgba(0xffffff1c).into()
            } else {
                cx.theme().muted
            })
        })
        .cursor_pointer()
        .on_click(on_click)
        .child(instance_cover(
            instance.icon.as_deref(),
            instance.loader,
            52.0,
            cx,
        ))
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .when(!editing, |this| {
                    this.group_hover(group.clone(), |style| style.pr(px(ACTIONS_W)))
                })
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
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "instance-name-{}",
                                        instance.id.as_hyphenated()
                                    )))
                                    .min_w_0()
                                    .flex_1()
                                    .text_sm()
                                    .font_weight(if selected {
                                        FontWeight::MEDIUM
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .whitespace_normal()
                                    .line_clamp(2)
                                    .text_color(name_color)
                                    .child(instance.name.clone()),
                            )
                            .when(instance.running, |this| {
                                this.child(
                                    div()
                                        .size(px(7.))
                                        .rounded_full()
                                        .bg(cx.theme().success)
                                        .flex_shrink_0(),
                                )
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_color(muted)
                            .child(format!(
                                "{} · {}",
                                instance.minecraft_version,
                                loader_label(instance.loader)
                            )),
                    )
                }),
        )
        .when(!editing, |this| {
            this.child(
                h_flex()
                    .absolute()
                    .right(px(8.))
                    .top_0()
                    .bottom_0()
                    .items_center()
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
                        asset_icon("icons/pencil.svg"),
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
    let hover_bg = if crate::sidebar_is_glass() {
        rgba(0xffffff38).into()
    } else {
        cx.theme().selection
    };
    div()
        .id(id.into())
        .size(px(22.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(color)
        .hover(|this| this.bg(hover_bg).text_color(cx.theme().foreground))
        .on_click(on_click)
        .child(icon.into().text_sm())
}
