use gpui::{IntoElement, ParentElement, RenderOnce, Styled, Window, div, px};
use gpui_component::{
    ActiveTheme, Disableable, StyledExt,
    button::{Button, ButtonVariants},
    v_flex,
};
use kmine_engine::InstanceSummary;

#[derive(IntoElement)]
pub struct PlayTab {
    name: String,
    minecraft_version: String,
    loader: String,
}

impl PlayTab {
    pub fn new(instance: &InstanceSummary) -> Self {
        Self {
            name: instance.name.clone(),
            minecraft_version: instance.minecraft_version.clone(),
            loader: instance.loader.as_str().to_string(),
        }
    }
}

impl RenderOnce for PlayTab {
    fn render(self, _: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_6()
            .gap_3()
            .child(div().text_lg().font_semibold().child(self.name))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} · {}", self.minecraft_version, self.loader)),
            )
            .child(
                Button::new("play")
                    .primary()
                    .label("Play")
                    .disabled(true)
                    .w(px(120.)),
            )
    }
}
