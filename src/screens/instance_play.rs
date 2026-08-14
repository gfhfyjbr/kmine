use gpui::prelude::*;
use gpui::{App, ClickEvent, IntoElement, ParentElement, Styled, Window, div, px};
use gpui_component::{
    ActiveTheme, StyledExt,
    button::{Button, ButtonVariants},
    v_flex,
};
use kmine_engine::InstanceSummary;

pub fn play_tab(
    instance: &InstanceSummary,
    status: &str,
    on_play: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let label = if instance.running { "Stop" } else { "Play" };
    v_flex()
        .size_full()
        .p_6()
        .gap_3()
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
}
