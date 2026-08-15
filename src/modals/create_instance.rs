use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Entity, FontWeight, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, StyledImage, Window, div, img,
    px,
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
    default_cover, loader_label, modal, modal_body, modal_close, modal_footer, modal_header,
    section_label, sheet,
};

const LOADERS: [Loader; 5] = [
    Loader::Vanilla,
    Loader::Fabric,
    Loader::Forge,
    Loader::NeoForge,
    Loader::Quilt,
];

#[derive(Clone, Copy)]
pub enum CreatePhase {
    Kind,
    Loader(Loader),
}

pub struct CreateInstanceForm {
    pub name: Entity<InputState>,
    pub version: Entity<InputState>,
    pub phase: CreatePhase,
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
            phase: CreatePhase::Kind,
        }
    }

    pub fn spec(&self, cx: &App) -> Option<CreateInstance> {
        let CreatePhase::Loader(loader) = self.phase else {
            return None;
        };
        Some(CreateInstance {
            name: self.name.read(cx).value().to_string(),
            minecraft_version: self.version.read(cx).value().to_string(),
            loader,
            loader_version: None,
            icon_png: None,
        })
    }
}

pub fn render(
    form: &CreateInstanceForm,
    status: &str,
    on_kind: impl Fn(Loader, &mut Window, &mut App) + Clone + 'static,
    on_back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_submit: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> AnyElement {
    let creating = status == "Creating…";
    let error = (!status.is_empty() && !creating).then(|| status.to_string());
    match form.phase {
        CreatePhase::Kind => render_kind(on_kind, on_cancel, cx).into_any_element(),
        CreatePhase::Loader(loader) => render_loader(
            form,
            loader,
            creating,
            error,
            on_back,
            on_submit,
            on_cancel,
            cx,
        )
        .into_any_element(),
    }
}

fn render_kind(
    on_kind: impl Fn(Loader, &mut Window, &mut App) + Clone + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    modal("create-instance-overlay", true, on_cancel.clone(), cx).child(
        sheet(cx)
            .child(modal_header(
                IconName::Plus,
                "New instance",
                "Choose a loader to get started.",
                cx,
            ))
            .child(
                modal_body().child(
                    h_flex()
                        .id("create-kind-grid")
                        .w_full()
                        .flex_wrap()
                        .gap_2()
                        .children(LOADERS.iter().copied().map(|loader| {
                            let on_kind = on_kind.clone();
                            kind_cell(loader, move |_, window, cx| on_kind(loader, window, cx), cx)
                        })),
                ),
            )
            .child(
                modal_footer(cx).child(
                    Button::new("create-cancel")
                        .outline()
                        .label("Cancel")
                        .on_click(on_cancel.clone()),
                ),
            )
            .child(modal_close(on_cancel)),
    )
}

fn render_loader(
    form: &CreateInstanceForm,
    loader: Loader,
    creating: bool,
    error: Option<String>,
    on_back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_submit: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let subtitle = format!("Name it and pick a version for {}.", loader_label(loader));
    modal("create-instance-overlay", !creating, on_cancel.clone(), cx).child(
        sheet(cx)
            .child(modal_header(
                IconName::Plus,
                "New instance",
                subtitle,
                cx,
            ))
            .child(
                modal_body()
                    .child(labeled_field("Name", Input::new(&form.name), cx))
                    .child(labeled_field("Version", Input::new(&form.version), cx))
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
                        Button::new("create-back")
                            .outline()
                            .label("Back")
                            .disabled(creating)
                            .on_click(on_back),
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

fn kind_cell(
    loader: Loader,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let id = SharedString::from(format!("kind-{}", loader_label(loader)));
    let label = loader_label(loader);
    let radius = px(10.);
    v_flex()
        .id(id)
        .w(px(124.))
        .gap_2()
        .p_2()
        .items_center()
        .rounded(radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted.opacity(0.35))
        .cursor_pointer()
        .hover(|this| this.bg(cx.theme().muted))
        .on_click(on_click)
        .child(
            div()
                .size(px(72.))
                .rounded(px(8.))
                .overflow_hidden()
                .border_1()
                .border_color(cx.theme().border.opacity(0.55))
                .bg(cx.theme().secondary_active)
                .child(
                    img(default_cover(loader))
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .rounded(px(8.)),
                ),
        )
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().foreground)
                .child(label),
        )
}
