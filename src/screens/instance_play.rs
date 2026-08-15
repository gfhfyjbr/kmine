use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ObjectFit, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, StyledImage, Window, div, img, px,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use kmine_engine::{InstanceSummary, QuickPlay, QuickPlayLists, QuickPlayWorld};

use crate::chrome::{format_last_played, format_playtime, section_label};

pub fn play_tab(
    instance: &InstanceSummary,
    quick_play: &QuickPlayLists,
    preparing: bool,
    on_play: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_quick: impl Fn(QuickPlay, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let running = instance.running;
    let blocked = running || preparing;
    v_flex()
        .id("instance-play")
        .w_full()
        .gap_5()
        .child(launch_row(instance, preparing, on_play, cx))
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
            "No servers in servers.dat",
            cx,
        ))
}

fn launch_row(
    instance: &InstanceSummary,
    preparing: bool,
    on_play: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
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
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            v_flex()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(format_last_played(instance.last_played_at)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(format_playtime(instance.playtime_secs)),
                ),
        )
        .child(
            Button::new("play")
                .when(running, |this| this.danger())
                .when(!running, |this| this.primary())
                .disabled(preparing)
                .on_click(on_play)
                .child(Icon::new(icon).with_size(px(12.)))
                .child(label),
        )
}

fn worlds_section(
    worlds: &[QuickPlayWorld],
    disabled: bool,
    on_quick: impl Fn(QuickPlay, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .gap_2()
        .child(section_label("Worlds", cx))
        .when(worlds.is_empty(), |this| {
            this.child(
                div()
                    .w_full()
                    .px_3()
                    .py_3()
                    .rounded(px(10.))
                    .bg(cx.theme().muted)
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No worlds in saves/"),
            )
        })
        .children(worlds.iter().map(|world| {
            let on_quick = on_quick.clone();
            let target = QuickPlay::World {
                folder: world.folder.clone(),
            };
            world_row(
                world,
                disabled,
                move |event, window, cx| {
                    on_quick(target.clone(), event, window, cx);
                },
                cx,
            )
        }))
}

fn world_row(
    world: &QuickPlayWorld,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .id(SharedString::from(format!("world-{}", world.folder)))
        .w_full()
        .px_2()
        .py_2()
        .items_center()
        .justify_between()
        .gap_3()
        .rounded(px(10.))
        .bg(cx.theme().muted)
        .when(!disabled, |this| {
            this.cursor_pointer()
                .hover(|this| this.bg(cx.theme().secondary_hover))
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
                        .child(div().text_sm().text_ellipsis().child(world.label.clone()))
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
    let size = px(40.);
    let radius = px(6.);
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
    empty: &'static str,
    cx: &App,
) -> impl IntoElement {
    let items: Vec<_> = items.into_iter().collect();
    v_flex()
        .w_full()
        .gap_2()
        .child(section_label(title, cx))
        .when(items.is_empty(), |this| {
            this.child(
                div()
                    .w_full()
                    .px_3()
                    .py_3()
                    .rounded(px(10.))
                    .bg(cx.theme().muted)
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(empty),
            )
        })
        .children(items.into_iter().map(|(id, title, detail, target)| {
            let on_quick = on_quick.clone();
            quick_row(
                id,
                title,
                detail,
                disabled,
                move |event, window, cx| {
                    on_quick(target.clone(), event, window, cx);
                },
                cx,
            )
        }))
}

fn quick_row(
    id: SharedString,
    title: String,
    detail: String,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .id(id)
        .w_full()
        .px_3()
        .py_2()
        .items_center()
        .justify_between()
        .gap_3()
        .rounded(px(10.))
        .bg(cx.theme().muted)
        .when(!disabled, |this| {
            this.cursor_pointer()
                .hover(|this| this.bg(cx.theme().secondary_hover))
                .on_click(on_click)
        })
        .when(disabled, |this| this.opacity(0.55))
        .child(
            v_flex()
                .min_w_0()
                .child(div().text_sm().text_ellipsis().child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .text_ellipsis()
                        .child(detail),
                ),
        )
        .child(
            Icon::new(IconName::Play)
                .text_sm()
                .text_color(cx.theme().muted_foreground),
        )
}
