use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, Window,
    div, px,
};
use gpui_component::{
    ActiveTheme, Disableable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex, v_flex,
};
use kmine_engine::{ContentClass, ContentEntry, ContentFolder, Loader};

use crate::chrome::section_header;

pub fn content_tab(
    mods: &[ContentEntry],
    resourcepacks: &[ContentEntry],
    shaderpacks: &[ContentEntry],
    loader: Loader,
    add_enabled: bool,
    on_toggle: impl Fn(PathBuf, bool, &mut Window, &mut App) + Clone + 'static,
    on_delete: impl Fn(PathBuf, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_add: impl Fn(ContentClass, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .id("instance-content")
        .w_full()
        .gap_5()
        .child(folder_section(
            ContentFolder::Mods,
            mods,
            loader,
            add_enabled,
            on_toggle.clone(),
            on_delete.clone(),
            on_add.clone(),
            cx,
        ))
        .child(folder_section(
            ContentFolder::Resourcepacks,
            resourcepacks,
            loader,
            add_enabled,
            on_toggle.clone(),
            on_delete.clone(),
            on_add.clone(),
            cx,
        ))
        .child(folder_section(
            ContentFolder::Shaderpacks,
            shaderpacks,
            loader,
            add_enabled,
            on_toggle,
            on_delete,
            on_add,
            cx,
        ))
}

fn folder_section(
    folder: ContentFolder,
    entries: &[ContentEntry],
    loader: Loader,
    add_enabled: bool,
    on_toggle: impl Fn(PathBuf, bool, &mut Window, &mut App) + Clone + 'static,
    on_delete: impl Fn(PathBuf, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_add: impl Fn(ContentClass, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let count = entries.len();
    let show_add = !(loader == Loader::Vanilla && folder == ContentFolder::Mods);
    let class = folder_class(folder);
    let add_id = SharedString::from(format!("content-add-{}", folder_label(folder)));
    v_flex()
        .w_full()
        .gap_2()
        .child(section_header(
            folder_label(folder),
            Some(
                h_flex()
                    .items_center()
                    .gap_2()
                    .when(show_add, |this| {
                        this.child(
                            Button::new(add_id)
                                .ghost()
                                .compact()
                                .label("Add")
                                .disabled(!add_enabled)
                                .on_click(move |event, window, cx| {
                                    on_add(class, event, window, cx);
                                }),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(count.to_string()),
                    )
                    .into_any_element(),
            ),
            cx,
        ))
        .when(entries.is_empty(), |this| {
            this.child(
                div()
                    .w_full()
                    .px_3()
                    .py_3()
                    .rounded(px(10.))
                    .bg(cx.theme().muted)
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(folder_empty(folder)),
            )
        })
        .children(
            entries
                .iter()
                .map(|entry| content_row(entry, on_toggle.clone(), on_delete.clone(), cx)),
        )
}

fn content_row(
    entry: &ContentEntry,
    on_toggle: impl Fn(PathBuf, bool, &mut Window, &mut App) + 'static,
    on_delete: impl Fn(PathBuf, &ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let path = entry.path.clone();
    let delete_path = path.clone();
    let key = SharedString::from(entry.path.to_string_lossy().into_owned());
    let delete_id = SharedString::from(format!("content-delete-{key}"));
    h_flex()
        .id(key.clone())
        .w_full()
        .items_center()
        .justify_between()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(px(10.))
        .bg(cx.theme().muted)
        .hover(|this| this.bg(cx.theme().secondary_hover))
        .child(
            Checkbox::new(key)
                .label(entry.name.clone())
                .checked(entry.enabled)
                .on_click(move |checked, window, cx| {
                    on_toggle(path.clone(), *checked, window, cx);
                }),
        )
        .child(
            Button::new(delete_id)
                .ghost()
                .compact()
                .label("Delete")
                .on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    on_delete(delete_path.clone(), event, window, cx);
                }),
        )
}

fn folder_class(folder: ContentFolder) -> ContentClass {
    match folder {
        ContentFolder::Mods => ContentClass::Mods,
        ContentFolder::Resourcepacks => ContentClass::ResourcePacks,
        ContentFolder::Shaderpacks => ContentClass::Shaders,
    }
}

fn folder_label(folder: ContentFolder) -> &'static str {
    match folder {
        ContentFolder::Mods => "Mods",
        ContentFolder::Resourcepacks => "Resource packs",
        ContentFolder::Shaderpacks => "Shader packs",
    }
}

fn folder_empty(folder: ContentFolder) -> &'static str {
    match folder {
        ContentFolder::Mods => "No mods in this instance",
        ContentFolder::Resourcepacks => "No resource packs",
        ContentFolder::Shaderpacks => "No shader packs",
    }
}
