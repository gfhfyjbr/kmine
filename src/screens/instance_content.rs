use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex, v_flex,
};
use kmine_engine::{ContentClass, ContentEntry, ContentFolder, Loader};

use crate::chrome::{empty_list, list_frame, list_row_corners, row_rule, section_header};

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
        .gap_6()
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
    let show_add = !(loader == Loader::Vanilla && folder == ContentFolder::Mods);
    let class = folder_class(folder);
    let add_id = SharedString::from(format!("content-add-{}", folder_label(folder)));
    v_flex()
        .w_full()
        .gap_2()
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .child(section_header(folder_label(folder), Some(entries.len()), cx))
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
                }),
        )
        .child(if entries.is_empty() {
            list_frame(cx)
                .child(empty_list(
                    folder_icon(folder),
                    folder_empty_title(folder),
                    folder_empty(folder),
                    cx,
                ))
                .into_any_element()
        } else {
            let last = entries.len().saturating_sub(1);
            list_frame(cx)
                .children(entries.iter().enumerate().flat_map(|(index, entry)| {
                    let mut rows = Vec::new();
                    if index > 0 {
                        rows.push(row_rule(cx).into_any_element());
                    }
                    rows.push(
                        content_row(
                            entry,
                            index == 0,
                            index == last,
                            on_toggle.clone(),
                            on_delete.clone(),
                            cx,
                        )
                        .into_any_element(),
                    );
                    rows
                }))
                .into_any_element()
        })
}

fn content_row(
    entry: &ContentEntry,
    first: bool,
    last: bool,
    on_toggle: impl Fn(PathBuf, bool, &mut Window, &mut App) + 'static,
    on_delete: impl Fn(PathBuf, &ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let path = entry.path.clone();
    let delete_path = path.clone();
    let key = SharedString::from(entry.path.to_string_lossy().into_owned());
    let delete_id = SharedString::from(format!("content-delete-{key}"));
    let group = SharedString::from(format!("content-row-{key}"));
    list_row_corners(
        h_flex()
            .id(key.clone())
            .group(group.clone())
            .w_full()
            .items_center()
            .justify_between()
            .gap_2()
            .px_3()
            .py_2(),
        first,
        last,
    )
    .hover(move |this| list_row_corners(this.bg(cx.theme().secondary_hover), first, last))
    .child(
        Checkbox::new(key)
            .label(entry.name.clone())
            .checked(entry.enabled)
            .on_click(move |checked, window, cx| {
                on_toggle(path.clone(), *checked, window, cx);
            }),
    )
    .child(
        div()
            .id(delete_id)
            .size(px(22.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .invisible()
            .group_hover(group, |style| style.visible())
            .cursor_pointer()
            .hover(|this| this.bg(cx.theme().secondary_active))
            .on_click(move |event, window, cx| {
                cx.stop_propagation();
                on_delete(delete_path.clone(), event, window, cx);
            })
            .child(
                Icon::empty()
                    .path("icons/trash.svg")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            ),
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

fn folder_icon(folder: ContentFolder) -> IconName {
    match folder {
        ContentFolder::Mods => IconName::Folder,
        ContentFolder::Resourcepacks => IconName::Palette,
        ContentFolder::Shaderpacks => IconName::Sun,
    }
}

fn folder_empty_title(folder: ContentFolder) -> &'static str {
    match folder {
        ContentFolder::Mods => "No mods",
        ContentFolder::Resourcepacks => "No resource packs",
        ContentFolder::Shaderpacks => "No shader packs",
    }
}

fn folder_empty(folder: ContentFolder) -> &'static str {
    match folder {
        ContentFolder::Mods => "Drop jars into this instance’s mods folder.",
        ContentFolder::Resourcepacks => "Add packs to the resourcepacks folder.",
        ContentFolder::Shaderpacks => "Add shaders to the shaderpacks folder.",
    }
}
