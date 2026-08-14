use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, Window,
    div,
};
use gpui_component::{
    ActiveTheme, StyledExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex, v_flex,
};
use kmine_engine::{ContentEntry, ContentFolder};

pub fn content_tab(
    mods: &[ContentEntry],
    resourcepacks: &[ContentEntry],
    shaderpacks: &[ContentEntry],
    on_toggle: impl Fn(PathBuf, bool, &mut Window, &mut App) + Clone + 'static,
    on_delete: impl Fn(PathBuf, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .id("instance-content")
        .size_full()
        .p_6()
        .gap_5()
        .overflow_y_scroll()
        .child(folder_section(
            ContentFolder::Mods,
            mods,
            on_toggle.clone(),
            on_delete.clone(),
            cx,
        ))
        .child(folder_section(
            ContentFolder::Resourcepacks,
            resourcepacks,
            on_toggle.clone(),
            on_delete.clone(),
            cx,
        ))
        .child(folder_section(
            ContentFolder::Shaderpacks,
            shaderpacks,
            on_toggle,
            on_delete,
            cx,
        ))
}

fn folder_section(
    folder: ContentFolder,
    entries: &[ContentEntry],
    on_toggle: impl Fn(PathBuf, bool, &mut Window, &mut App) + Clone + 'static,
    on_delete: impl Fn(PathBuf, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_2()
        .child(div().font_semibold().child(folder_label(folder)))
        .when(entries.is_empty(), |this| {
            this.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No files"),
            )
        })
        .children(
            entries
                .iter()
                .map(|entry| content_row(entry, on_toggle.clone(), on_delete.clone())),
        )
}

fn content_row(
    entry: &ContentEntry,
    on_toggle: impl Fn(PathBuf, bool, &mut Window, &mut App) + 'static,
    on_delete: impl Fn(PathBuf, &ClickEvent, &mut Window, &mut App) + 'static,
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

fn folder_label(folder: ContentFolder) -> &'static str {
    match folder {
        ContentFolder::Mods => "Mods",
        ContentFolder::Resourcepacks => "Resource packs",
        ContentFolder::Shaderpacks => "Shader packs",
    }
}
