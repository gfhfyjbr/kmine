use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Context, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Window, div, img, px, rgb, rgba,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use kmine_engine::{InstanceId, InstanceSummary, Loader};
use std::path::{Path, PathBuf};

use crate::chrome::{empty_panel, instance_cover, loader_label, running_mark};
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
    on_reorder: impl Fn(InstanceId, InstanceId, &mut Window, &mut App) + Clone + 'static,
    on_accounts: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_settings: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    renaming: Option<&RenameForm>,
    pinned: &[InstanceId],
    scroll: &SmoothScroll,
    cx: &App,
) -> impl IntoElement {
    let mut rows: Vec<(usize, &InstanceSummary)> = instances.iter().enumerate().collect();
    rows.sort_by_key(
        |(orig, instance)| match pinned.iter().position(|id| *id == instance.id) {
            Some(i) => (0u8, i, 0usize),
            None => (1u8, 0, *orig),
        },
    );
    let rows: Vec<&InstanceSummary> = rows.into_iter().map(|(_, instance)| instance).collect();
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
                        .font_weight(FontWeight::SEMIBOLD)
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
                    this.child(empty_panel(
                        IconName::Plus,
                        "No instances",
                        "Create one to get started",
                        cx,
                    ))
                })
                .children({
                    let mut kids = Vec::new();
                    let mut saw_pinned = false;
                    let mut gap = false;
                    for instance in rows {
                        let is_pinned = pinned.contains(&instance.id);
                        if is_pinned {
                            saw_pinned = true;
                        } else if saw_pinned && !gap {
                            kids.push(
                                div()
                                    .mx_2()
                                    .my(px(5.))
                                    .h(px(1.))
                                    .bg(if glass {
                                        rgba(0xffffff14).into()
                                    } else {
                                        cx.theme().border
                                    })
                                    .into_any_element(),
                            );
                            gap = true;
                        }
                        let id = instance.id;
                        let on_select = on_select.clone();
                        let on_rename = on_rename.clone();
                        let on_confirm_rename = on_confirm_rename.clone();
                        let on_delete = on_delete.clone();
                        let on_pin = on_pin.clone();
                        let on_reorder = on_reorder.clone();
                        kids.push(
                            instance_row(
                                instance,
                                selected == Some(id),
                                is_pinned,
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
                                move |dragged, target, window, cx| {
                                    on_reorder(dragged, target, window, cx);
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                    }
                    kids
                }),
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
    v_flex().w_full().p_2().child(
        h_flex()
            .id("accounts-identity")
            .w_full()
            .px_2()
            .py_2()
            .items_center()
            .gap_2()
            .rounded(px(12.))
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
            ),
    )
}

fn player_face(skin: Option<&Path>, cx: &App) -> impl IntoElement {
    let radius = px(8.);
    let face = div()
        .size(px(30.))
        .flex_shrink_0()
        .rounded(radius)
        .overflow_hidden()
        .bg(cx.theme().muted)
        .border_1()
        .border_color(cx.theme().border.opacity(0.6));
    match skin {
        Some(path) => face.child(img(path.to_path_buf()).size_full().rounded(radius)),
        None => face
            .flex()
            .items_center()
            .justify_center()
            .child(Icon::new(IconName::User).text_color(cx.theme().muted_foreground)),
    }
}

struct PinDragPreview {
    name: String,
    meta: String,
    loader: Loader,
    icon: Option<PathBuf>,
}

impl Render for PinDragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w(px(236.))
            .px_2()
            .py(px(7.))
            .gap_2()
            .items_center()
            .rounded(px(12.))
            .bg(if crate::sidebar_is_glass() {
                rgba(0x121110f0).into()
            } else {
                cx.theme().sidebar
            })
            .text_color(cx.theme().foreground)
            .border_1()
            .border_color(cx.theme().border)
            .shadow_lg()
            .child(instance_cover(self.icon.as_deref(), self.loader, 52.0, cx))
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_ellipsis()
                            .text_color(cx.theme().foreground)
                            .child(self.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.meta.clone()),
                    ),
            )
    }
}

fn instance_row(
    instance: &InstanceSummary,
    selected: bool,
    pinned: bool,
    renaming: Option<&RenameForm>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_rename: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_confirm_rename: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_delete: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_pin: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_reorder: impl Fn(InstanceId, InstanceId, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let id = instance.id;
    let key = id.as_hyphenated();
    let row_id = SharedString::from(key.clone());
    let group = SharedString::from(format!("instance-row-{key}"));
    let muted = cx.theme().muted_foreground;
    let editing = renaming.is_some();
    let draggable = pinned && !editing;
    let name_color = cx.theme().foreground;
    const ACTIONS_W: f32 = 80.0;
    h_flex()
        .id(row_id)
        .group(group.clone())
        .relative()
        .w_full()
        .px_2()
        .py(px(7.))
        .gap_2()
        .items_center()
        .rounded(px(12.))
        .when(selected && !editing, |this| {
            let fill: gpui::Hsla = if crate::sidebar_is_glass() {
                rgba(0xffffff28).into()
            } else {
                rgb(0x2e2b26).into()
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
        .when(!draggable, |this| this.on_click(on_click.clone()))
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
                        h_flex().w_full().min_w_0().items_center().gap_1().child(
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
                        ),
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
        .when(instance.running && !editing, |this| {
            this.child(
                div()
                    .absolute()
                    .right(px(10.))
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .group_hover(group.clone(), |style| style.invisible())
                    .child(running_mark(20.0, cx)),
            )
        })
        .when(draggable, |this| {
            let name = instance.name.clone();
            let meta = format!(
                "{} · {}",
                instance.minecraft_version,
                loader_label(instance.loader)
            );
            let loader = instance.loader;
            let icon = instance.icon.clone();
            this.child(
                div()
                    .id(SharedString::from(format!("instance-drag-{key}")))
                    .absolute()
                    .inset_0()
                    .on_click(on_click)
                    .on_drag(id, move |_, _, _, cx| {
                        cx.new(|_| PinDragPreview {
                            name: name.clone(),
                            meta: meta.clone(),
                            loader,
                            icon: icon.clone(),
                        })
                    })
                    .drag_over::<InstanceId>(move |style, dragged, _, cx| {
                        if *dragged == id {
                            style
                        } else if crate::sidebar_is_glass() {
                            style.bg(rgba(0xffffff38))
                        } else {
                            style.bg(cx.theme().secondary_hover)
                        }
                    })
                    .on_drop(move |dragged: &InstanceId, window, cx| {
                        if *dragged != id {
                            on_reorder(*dragged, id, window, cx);
                        }
                    }),
            )
        })
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
