use gpui::prelude::*;
use gpui::{App, ClickEvent, IntoElement, ParentElement, SharedString, Styled, Window, div, px};
use gpui_component::{
    ActiveTheme, StyledExt,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use kmine_engine::{InstanceSummary, QuickPlay, QuickPlayLists, QuickPlayServer, QuickPlayWorld};

pub fn play_tab(
    instance: &InstanceSummary,
    status: &str,
    lists: &QuickPlayLists,
    on_play: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_quick_play: impl Fn(QuickPlay, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let label = if instance.running { "Stop" } else { "Play" };
    v_flex()
        .id("instance-play")
        .size_full()
        .p_6()
        .gap_4()
        .overflow_y_scroll()
        .child(div().text_lg().font_semibold().child(instance.name.clone()))
        .child(div().text_color(cx.theme().muted_foreground).child(format!(
            "{} · {}",
            instance.minecraft_version,
            instance.loader.as_str()
        )))
        .child(
            Button::new("play")
                .primary()
                .label(label)
                .w(px(120.))
                .on_click(on_play),
        )
        .when(!status.is_empty(), |this| {
            this.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(status.to_string()),
            )
        })
        .child(worlds_section(&lists.worlds, on_quick_play.clone(), cx))
        .child(servers_section(&lists.servers, on_quick_play, cx))
}

fn worlds_section(
    worlds: &[QuickPlayWorld],
    on_quick_play: impl Fn(QuickPlay, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_2()
        .child(div().font_semibold().child("Worlds"))
        .when(worlds.is_empty(), |this| {
            this.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No worlds"),
            )
        })
        .children(worlds.iter().map(|world| {
            let folder = world.folder.clone();
            let on_quick_play = on_quick_play.clone();
            let id = SharedString::from(format!("qp-world-{folder}"));
            Button::new(id)
                .ghost()
                .label(world.label.clone())
                .on_click(move |event, window, cx| {
                    on_quick_play(
                        QuickPlay::World {
                            folder: folder.clone(),
                        },
                        event,
                        window,
                        cx,
                    );
                })
        }))
}

fn servers_section(
    servers: &[QuickPlayServer],
    on_quick_play: impl Fn(QuickPlay, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_2()
        .child(div().font_semibold().child("Servers"))
        .when(servers.is_empty(), |this| {
            this.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No servers"),
            )
        })
        .children(servers.iter().enumerate().map(|(i, server)| {
            let address = server.address.clone();
            let on_quick_play = on_quick_play.clone();
            let id = SharedString::from(format!("qp-server-{i}-{address}"));
            h_flex()
                .id(id.clone())
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .child(Button::new(id).ghost().label(server.name.clone()).on_click(
                    move |event, window, cx| {
                        on_quick_play(
                            QuickPlay::Server {
                                address: address.clone(),
                            },
                            event,
                            window,
                            cx,
                        );
                    },
                ))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(server.address.clone()),
                )
        }))
}
