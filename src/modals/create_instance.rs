use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{
    ActiveTheme, Disableable, IconName,
    alert::Alert,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use kmine_engine::{CreateInstance, Loader};

use crate::chrome::{
    loader_label, modal, modal_body, modal_close, modal_footer, modal_header, section_label, sheet,
};

const LOADERS: [Loader; 3] = [Loader::Vanilla, Loader::Fabric, Loader::Forge];

pub struct CreateInstanceForm {
    pub name: Entity<InputState>,
    pub version: Entity<InputState>,
    pub loader: Loader,
}

impl CreateInstanceForm {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            name: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Survival")
                    .default_value("New Instance")
            }),
            version: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("1.21.1")
                    .default_value("1.21.1")
            }),
            loader: Loader::Vanilla,
        }
    }

    pub fn spec(&self, cx: &App) -> CreateInstance {
        CreateInstance {
            name: self.name.read(cx).value().to_string(),
            minecraft_version: self.version.read(cx).value().to_string(),
            loader: self.loader,
            loader_version: None,
            icon_png: None,
        }
    }
}

pub fn render(
    form: &CreateInstanceForm,
    status: &str,
    on_loader: impl Fn(Loader, &mut Window, &mut App) + Clone + 'static,
    on_submit: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let creating = status == "Creating…";
    let error = (!status.is_empty() && !creating).then(|| status.to_string());
    modal("create-instance-overlay", !creating, on_cancel.clone(), cx).child(
        sheet(cx)
            .child(modal_header(
                IconName::Plus,
                "New instance",
                "Name it, pick a version, choose a loader.",
                cx,
            ))
            .child(
                modal_body()
                    .child(labeled_field("Name", Input::new(&form.name), cx))
                    .child(labeled_field("Version", Input::new(&form.version), cx))
                    .child(
                        v_flex()
                            .gap_2()
                            .child(section_label("Loader", cx))
                            .child(loader_picker(form.loader, on_loader, cx)),
                    )
                    .when(creating, |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("Creating instance…"),
                        )
                    })
                    .when_some(error, |this, error| {
                        this.child(Alert::error("create-error", error))
                    }),
            )
            .child(
                modal_footer(cx)
                    .child(
                        Button::new("create-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(creating)
                            .on_click(on_cancel.clone()),
                    )
                    .child(
                        Button::new("create-submit")
                            .primary()
                            .label("Create")
                            .loading(creating)
                            .disabled(creating)
                            .on_click(on_submit),
                    ),
            )
            .child(modal_close(on_cancel)),
    )
}

fn labeled_field(label: &str, field: impl IntoElement, cx: &App) -> impl IntoElement {
    v_flex()
        .gap_2()
        .child(section_label(label, cx))
        .child(field)
}

fn loader_picker(
    selected: Loader,
    on_loader: impl Fn(Loader, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .id("create-loader")
        .w_full()
        .h(px(34.))
        .p(px(3.))
        .gap_1()
        .rounded(px(10.))
        .bg(cx.theme().muted)
        .children(LOADERS.iter().copied().map(|loader| {
            let on_loader = on_loader.clone();
            loader_segment(
                loader,
                selected == loader,
                move |_, window, cx| on_loader(loader, window, cx),
                cx,
            )
        }))
}

fn loader_segment(
    loader: Loader,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let id = SharedString::from(format!("loader-{}", loader_label(loader)));
    let (bg, fg) = if selected {
        (cx.theme().primary, cx.theme().primary_foreground)
    } else {
        (cx.theme().transparent, cx.theme().muted_foreground)
    };
    h_flex()
        .id(id)
        .flex_1()
        .h_full()
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .bg(bg)
        .text_color(fg)
        .cursor_pointer()
        .when(!selected, |this| {
            this.hover(|this| this.text_color(cx.theme().foreground))
        })
        .on_click(on_click)
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(loader_label(loader)),
        )
}
