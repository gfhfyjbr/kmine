use gpui::prelude::*;
use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, Window,
    div, px,
};
use gpui_component::{ActiveTheme, StyledExt, button::Button, h_flex, list::ListItem, v_flex};
use kmine_engine::{InstanceId, InstanceSummary};

pub fn sidebar(
    instances: &[InstanceSummary],
    selected: Option<InstanceId>,
    on_select: impl Fn(InstanceId, &mut Window, &mut App) + Clone + 'static,
    on_create: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .id("instance-sidebar")
        .w(px(240.))
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
                    instance_row(instance, selected == Some(id), move |_, window, cx| {
                        on_select(id, window, cx);
                    })
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
) -> ListItem {
    ListItem::new(SharedString::from(instance.id.as_hyphenated()))
        .selected(selected)
        .child(instance.name.clone())
        .on_click(on_click)
}
