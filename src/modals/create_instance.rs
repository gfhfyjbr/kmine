use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Entity, InteractiveElement, IntoElement, ParentElement, Styled, Window, div,
    px,
};
use gpui_component::{
    ActiveTheme, IndexPath, StyledExt,
    button::{Button, ButtonVariants},
    form::{field, v_form},
    h_flex,
    input::{Input, InputState},
    select::{Select, SelectState},
    v_flex,
};
use kmine_engine::{CreateInstance, Loader};

const LOADERS: [&str; 3] = ["Vanilla", "Fabric", "Forge"];

pub struct CreateInstanceForm {
    pub name: Entity<InputState>,
    pub version: Entity<InputState>,
    pub loader: Entity<SelectState<Vec<&'static str>>>,
}

impl CreateInstanceForm {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            name: cx.new(|cx| InputState::new(window, cx).default_value("New Instance")),
            version: cx.new(|cx| InputState::new(window, cx).default_value("1.21.1")),
            loader: cx.new(|cx| {
                SelectState::new(LOADERS.to_vec(), Some(IndexPath::default()), window, cx)
            }),
        }
    }

    pub fn spec(&self, cx: &App) -> CreateInstance {
        CreateInstance {
            name: self.name.read(cx).value().to_string(),
            minecraft_version: self.version.read(cx).value().to_string(),
            loader: loader_from_label(
                self.loader
                    .read(cx)
                    .selected_value()
                    .copied()
                    .unwrap_or("Vanilla"),
            ),
            loader_version: None,
            icon_png: None,
        }
    }
}

pub fn render(
    form: &CreateInstanceForm,
    status: &str,
    on_submit: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    div()
        .id("create-instance-overlay")
        .absolute()
        .inset_0()
        .occlude()
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
                .child(div().font_semibold().child("Create instance"))
                .child(
                    v_form()
                        .child(field().label("Name").child(Input::new(&form.name)))
                        .child(field().label("Version").child(Input::new(&form.version)))
                        .child(field().label("Loader").child(Select::new(&form.loader))),
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
                            Button::new("create-cancel")
                                .label("Cancel")
                                .on_click(on_cancel),
                        )
                        .child(
                            Button::new("create-submit")
                                .primary()
                                .label("Create")
                                .on_click(on_submit),
                        ),
                ),
        )
}

fn loader_from_label(label: &str) -> Loader {
    match label {
        "Fabric" => Loader::Fabric,
        "Forge" => Loader::Forge,
        _ => Loader::Vanilla,
    }
}
