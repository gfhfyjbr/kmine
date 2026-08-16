use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    div, img, px, App, ClickEvent, FontWeight, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, StyledImage, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Disableable, Icon, IconName, Sizable,
};
use kmine_engine::{InstanceSummary, QuickPlay, QuickPlayLists, QuickPlayWorld};

use crate::chrome::{
    chip, empty_list, format_last_played, format_playtime, instance_cover, list_frame,
    list_row_corners, loader_label, row_rule, running_pill, section_header, style_cta,
};
use crate::smooth_scroll::SmoothScroll;

pub fn play_tab(
    instance: &InstanceSummary,
    quick_play: &QuickPlayLists,
    preparing: bool,
    on_quick: impl Fn(QuickPlay, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    scroll: &SmoothScroll,
    cx: &App,
) -> impl IntoElement {
    let running = instance.running;
    let blocked = running || preparing;
    scroll
        .vertical(
            v_flex()
                .id("instance-play")
                .w_full()
                .flex_1()
                .min_h_0()
                .gap_4(),
        )
        .child(worlds_section(
            &quick_play.worlds,
            blocked,
            on_quick.clone(),
            cx,
        ))
        .child(quick_play_section(
            "Servers",
            quick_play.servers.iter().map(|server| {
                (
                    SharedString::from(format!("server-{}", server.address)),
                    server.name.clone(),
                    server.address.clone(),
                    QuickPlay::Server {
                        address: server.address.clone(),
                    },
                )
            }),
            blocked,
            on_quick,
            IconName::Network,
            "No servers yet",
            "Servers you add in Minecraft show up here.",
            cx,
        ))
}

pub fn launch_hero(
    instance: &InstanceSummary,
    preparing: bool,
    on_play: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_verify: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let running = instance.running;
    let (label, icon) = if preparing {
        ("Preparing", IconName::Loader)
    } else if running {
        ("Stop", IconName::Pause)
    } else {
        ("Play", IconName::Play)
    };
    h_flex()
        .w_full()
        .flex_shrink_0()
        .px_4()
        .py_3()
        .gap_3()
        .items_center()
        .rounded(cx.theme().radius_lg)
        .bg(cx.theme().muted)
        .when(running, |this| {
            this.border_1()
                .border_color(cx.theme().success.opacity(0.28))
        })
        .child(instance_cover(
            instance.icon.as_deref(),
            instance.loader,
            72.0,
            cx,
        ))
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .gap_2()
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .text_xl()
                                .font_weight(FontWeight::MEDIUM)
                                .text_ellipsis()
                                .child(instance.name.clone()),
                        )
                        .when(running, |this| this.child(running_pill(cx))),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(chip(instance.minecraft_version.clone(), cx))
                        .child(chip(loader_label(instance.loader), cx)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "{} · {}",
                            format_last_played(instance.last_played_at),
                            format_playtime(instance.playtime_secs)
                        )),
                ),
        )
        .child(
            v_flex()
                .gap_2()
                .child(
                    style_cta(Button::new("play").large())
                        .when(running, |this| this.danger())
                        .when(!running, |this| this.primary())
                        .disabled(preparing)
                        .loading(preparing)
                        .when(preparing, |this| {
                            this.icon(IconName::Loader).label("Preparing")
                        })
                        .when(!preparing, |this| {
                            this.child(
                                h_flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        Icon::new(icon)
                                            .with_size(px(13.))
                                            .when(!running, |icon| icon.ml(px(1.))),
                                    )
                                    .child(label),
                            )
                        })
                        .on_click(on_play),
                )
                .child(
                    Button::new("verify-files")
                        .label("Verify files")
                        .disabled(preparing || running)
                        .on_click(on_verify),
                ),
        )
}

fn worlds_section(
    worlds: &[QuickPlayWorld],
    disabled: bool,
    on_quick: impl Fn(QuickPlay, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let last = worlds.len().saturating_sub(1);
    v_flex()
        .w_full()
        .flex_shrink_0()
        .gap_2()
        .child(section_header("Worlds", Some(worlds.len()), cx))
        .child(if worlds.is_empty() {
            list_frame(cx)
                .child(empty_list(
                    IconName::Globe,
                    "No worlds yet",
                    "They appear here after you play and save.",
                    cx,
                ))
                .into_any_element()
        } else {
            list_frame(cx)
                .children(worlds.iter().enumerate().flat_map(|(index, world)| {
                    let on_quick = on_quick.clone();
                    let target = QuickPlay::World {
                        folder: world.folder.clone(),
                    };
                    let mut items = Vec::new();
                    if index > 0 {
                        items.push(row_rule(cx).into_any_element());
                    }
                    items.push(
                        world_row(
                            world,
                            disabled,
                            index == 0,
                            index == last,
                            move |event, window, cx| {
                                on_quick(target.clone(), event, window, cx);
                            },
                            cx,
                        )
                        .into_any_element(),
                    );
                    items
                }))
                .into_any_element()
        })
}

fn world_row(
    world: &QuickPlayWorld,
    disabled: bool,
    first: bool,
    last: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    list_row_corners(
        h_flex()
            .id(SharedString::from(format!("world-{}", world.folder)))
            .w_full()
            .px_3()
            .py_2()
            .items_center()
            .justify_between()
            .gap_3(),
        first,
        last,
    )
    .when(!disabled, |this| {
        this.cursor_pointer()
            .hover(move |this| list_row_corners(this.bg(cx.theme().secondary_hover), first, last))
            .on_click(on_click)
    })
    .when(disabled, |this| this.opacity(0.55))
    .child(
        h_flex()
            .min_w_0()
            .items_center()
            .gap_3()
            .child(world_icon(world.icon.clone(), cx))
            .child(
                v_flex()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_ellipsis()
                            .child(world.label.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .text_ellipsis()
                            .child(world.folder.clone()),
                    ),
            ),
    )
    .child(
        Icon::new(IconName::Play)
            .text_sm()
            .text_color(cx.theme().muted_foreground),
    )
}

fn world_icon(icon: Option<PathBuf>, cx: &App) -> impl IntoElement {
    let size = px(44.);
    let radius = px(8.);
    match icon {
        Some(path) => img(path)
            .size(size)
            .flex_shrink_0()
            .rounded(radius)
            .object_fit(ObjectFit::Fill)
            .overflow_hidden()
            .into_any_element(),
        None => div()
            .size(size)
            .flex_shrink_0()
            .rounded(radius)
            .bg(cx.theme().secondary_active)
            .flex()
            .items_center()
            .justify_center()
            .child(
                Icon::new(IconName::Globe)
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
            .into_any_element(),
    }
}

fn quick_play_section(
    title: &'static str,
    items: impl IntoIterator<Item = (SharedString, String, String, QuickPlay)>,
    disabled: bool,
    on_quick: impl Fn(QuickPlay, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    empty_icon: IconName,
    empty_title: &'static str,
    empty_hint: &'static str,
    cx: &App,
) -> impl IntoElement {
    let items: Vec<_> = items.into_iter().collect();
    let last = items.len().saturating_sub(1);
    v_flex()
        .w_full()
        .flex_shrink_0()
        .gap_2()
        .child(section_header(title, Some(items.len()), cx))
        .child(if items.is_empty() {
            list_frame(cx)
                .child(empty_list(empty_icon, empty_title, empty_hint, cx))
                .into_any_element()
        } else {
            list_frame(cx)
                .children(items.into_iter().enumerate().flat_map(
                    |(index, (id, title, detail, target))| {
                        let on_quick = on_quick.clone();
                        let mut rows = Vec::new();
                        if index > 0 {
                            rows.push(row_rule(cx).into_any_element());
                        }
                        rows.push(
                            quick_row(
                                id,
                                title,
                                detail,
                                disabled,
                                index == 0,
                                index == last,
                                move |event, window, cx| {
                                    on_quick(target.clone(), event, window, cx);
                                },
                                cx,
                            )
                            .into_any_element(),
                        );
                        rows
                    },
                ))
                .into_any_element()
        })
}

fn quick_row(
    id: SharedString,
    title: String,
    detail: String,
    disabled: bool,
    first: bool,
    last: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    list_row_corners(
        h_flex()
            .id(id)
            .w_full()
            .px_3()
            .py_2()
            .items_center()
            .justify_between()
            .gap_3(),
        first,
        last,
    )
    .when(!disabled, |this| {
        this.cursor_pointer()
            .hover(move |this| list_row_corners(this.bg(cx.theme().secondary_hover), first, last))
            .on_click(on_click)
    })
    .when(disabled, |this| this.opacity(0.55))
    .child(
        h_flex()
            .min_w_0()
            .items_center()
            .gap_3()
            .child(
                div()
                    .size(px(44.))
                    .flex_shrink_0()
                    .rounded(px(8.))
                    .bg(cx.theme().secondary_active)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::Network)
                            .text_sm()
                            .text_color(cx.theme().muted_foreground),
                    ),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_ellipsis()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .text_ellipsis()
                            .child(detail),
                    ),
            ),
    )
    .child(
        Icon::new(IconName::Play)
            .text_sm()
            .text_color(cx.theme().muted_foreground),
    )
}
