use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    AnimationExt, App, ClickEvent, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex, v_flex,
};
use kmine_engine::{ContentClass, ContentEntry, ContentFolder, Loader};

use crate::chrome::{
    empty_list, list_frame, list_row_corners, row_rule, section_header, tab_motion,
};

pub fn content_tab(
    mods: &[ContentEntry],
    resourcepacks: &[ContentEntry],
    shaderpacks: &[ContentEntry],
    loader: Loader,
    add_enabled: bool,
    mods_expanded: bool,
    on_toggle: impl Fn(PathBuf, bool, &mut Window, &mut App) + Clone + 'static,
    on_delete: impl Fn(PathBuf, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_add: impl Fn(ContentClass, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_expand_mods: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
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
            true,
            mods_expanded,
            on_expand_mods,
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
            false,
            true,
            |_, _, _| {},
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
            false,
            true,
            |_, _, _| {},
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
    collapsible: bool,
    expanded: bool,
    on_expand: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_toggle: impl Fn(PathBuf, bool, &mut Window, &mut App) + Clone + 'static,
    on_delete: impl Fn(PathBuf, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_add: impl Fn(ContentClass, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let show_add = !(loader == Loader::Vanilla && folder == ContentFolder::Mods);
    let class = folder_class(folder);
    let add_id = SharedString::from(format!("content-add-{}", folder_label(folder)));
    let curtain = collapsible && !entries.is_empty();
    v_flex()
        .w_full()
        .gap_2()
        .child(
            h_flex()
                .id(SharedString::from(format!(
                    "content-header-{}",
                    folder_label(folder)
                )))
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .when(curtain, |this| {
                    let on_expand = on_expand.clone();
                    this.cursor_pointer()
                        .on_click(move |event, window, cx| on_expand(event, window, cx))
                })
                .child(
                    h_flex()
                        .min_w_0()
                        .flex_1()
                        .items_center()
                        .gap_2()
                        .when(curtain, |this| {
                            this.child(
                                Icon::new(if expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .text_xs()
                                .text_color(cx.theme().muted_foreground),
                            )
                        })
                        .child(section_header(
                            folder_label(folder),
                            Some(entries.len()),
                            cx,
                        )),
                )
                .when(show_add, |this| {
                    this.child(
                        Button::new(add_id)
                            .ghost()
                            .compact()
                            .label("Add")
                            .disabled(!add_enabled)
                            .on_click(move |event, window, cx| {
                                cx.stop_propagation();
                                on_add(class, event, window, cx);
                            }),
                    )
                }),
        )
        .when(entries.is_empty(), |this| {
            this.child(list_frame(cx).child(empty_list(
                folder_icon(folder),
                folder_empty_title(folder),
                folder_empty(folder),
                cx,
            )))
        })
        .when(!entries.is_empty() && expanded, |this| {
            let last = entries.len().saturating_sub(1);
            let list_h = (last as f32 + 1.0) * 44.0;
            let list =
                list_frame(cx).children(entries.iter().enumerate().flat_map(|(index, entry)| {
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
                }));
            this.child(if collapsible {
                list.with_animation("mods-expand", tab_motion(), move |this, delta| {
                    if delta >= 1.0 {
                        this
                    } else {
                        this.max_h(px((list_h * delta).max(1.0)))
                            .opacity((0.35 + 0.65 * delta).min(1.0))
                    }
                })
                .into_any_element()
            } else {
                list.into_any_element()
            })
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
            .py(px(9.)),
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
        ContentFolder::Mods => "Use Add, or drop jars into the mods folder.",
        ContentFolder::Resourcepacks => "Use Add, or drop packs into resourcepacks.",
        ContentFolder::Shaderpacks => "Use Add, or drop shaders into shaderpacks.",
    }
}
