use std::collections::HashMap;
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Entity, FontWeight, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, StyledImage, Window, div, img, px,
};
use gpui_component::{
    ActiveTheme, Disableable, IconName, Sizable,
    alert::Alert,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    spinner::Spinner,
    v_flex,
};
use kmine_engine::{
    CatalogCategory, CatalogFile, CatalogPage, CatalogProject, CatalogProjectDetail,
    CatalogProjectId, CatalogSort, ContentClass, InstanceId, Loader, ProviderId,
    catalog::CatalogRelease,
};

use crate::chrome::{
    chip, empty_panel, loader_label, modal_body, modal_close, modal_footer, modal_header,
    sheet_wide,
};

pub const PAGE_SIZE: u32 = 20;
const INDEX_CAP: u32 = 10_000;

#[derive(Clone, Copy)]
pub enum CatalogTarget {
    NewInstance,
    Instance(InstanceId),
}

pub struct CatalogModal {
    pub provider: ProviderId,
    pub class: ContentClass,
    pub target: CatalogTarget,
    pub search: Entity<InputState>,
    pub categories: Vec<CatalogCategory>,
    pub selected_categories: Vec<String>,
    pub sort: CatalogSort,
    pub page: Option<CatalogPage<CatalogProject>>,
    pub project: Option<CatalogProjectDetail>,
    pub files: Option<CatalogPage<CatalogFile>>,
    pub selected_file: Option<String>,
    pub error: Option<String>,
    pub loading: bool,
    pub no_key: bool,
    pub game_version: Option<String>,
    pub loader: Option<Loader>,
    pub version_filter: Entity<InputState>,
    pub images: HashMap<String, PathBuf>,
}

impl CatalogModal {
    pub fn new(
        class: ContentClass,
        target: CatalogTarget,
        game_version: Option<String>,
        loader: Option<Loader>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        Self {
            provider: ProviderId::CURSEFORGE,
            class,
            target,
            search: cx.new(|cx| InputState::new(window, cx).placeholder("Search")),
            categories: Vec::new(),
            selected_categories: Vec::new(),
            sort: CatalogSort::Popularity,
            page: None,
            project: None,
            files: None,
            selected_file: None,
            error: None,
            loading: true,
            no_key: false,
            game_version,
            loader,
            version_filter: cx
                .new(|cx| InputState::new(window, cx).placeholder("Minecraft version")),
            images: HashMap::new(),
        }
    }
}

pub fn can_page_more(index: u32, page_size: u32, total: u32) -> bool {
    let next = index.saturating_add(page_size);
    next < total && index.saturating_add(page_size.saturating_mul(2)) <= INDEX_CAP
}

fn class_title(class: ContentClass) -> &'static str {
    match class {
        ContentClass::Modpacks => "Modpacks",
        ContentClass::Mods => "Mods",
        ContentClass::ResourcePacks => "Resource packs",
        ContentClass::Shaders => "Shader packs",
    }
}

fn class_subtitle(class: ContentClass) -> &'static str {
    match class {
        ContentClass::Modpacks => "Search CurseForge and install a pack as a new instance.",
        ContentClass::Mods => "Search CurseForge and add a mod to this instance.",
        ContentClass::ResourcePacks => {
            "Search CurseForge and add a resource pack to this instance."
        }
        ContentClass::Shaders => "Search CurseForge and add a shader pack to this instance.",
    }
}

pub fn render(
    modal: &CatalogModal,
    on_dismiss: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_toggle_category: impl Fn(String, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_sort: impl Fn(CatalogSort, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_more: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_open_project: impl Fn(CatalogProjectId, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_back: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_select_file: impl Fn(String, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_more_files: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_install: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_website: impl Fn(String, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let project_open = modal.project.is_some();
    crate::chrome::modal("catalog-overlay", true, on_dismiss.clone(), cx).child(
        sheet_wide(cx)
            .child(modal_header(
                IconName::Folder,
                class_title(modal.class),
                class_subtitle(modal.class),
                cx,
            ))
            .child(
                modal_body()
                    .flex_1()
                    .min_h(px(280.))
                    .overflow_hidden()
                    .child(if project_open {
                        render_project(
                            modal,
                            on_back.clone(),
                            on_select_file,
                            on_more_files,
                            on_website,
                            cx,
                        )
                        .into_any_element()
                    } else {
                        render_list(
                            modal,
                            on_toggle_category,
                            on_sort,
                            on_more,
                            on_open_project,
                            cx,
                        )
                        .into_any_element()
                    }),
            )
            .child(if project_open {
                modal_footer(cx)
                    .child(
                        Button::new("catalog-back")
                            .outline()
                            .label("Back")
                            .on_click(on_back),
                    )
                    .child(
                        Button::new("catalog-install")
                            .primary()
                            .label("Install")
                            .disabled(modal.selected_file.is_none())
                            .on_click(on_install),
                    )
                    .into_any_element()
            } else {
                modal_footer(cx)
                    .child(
                        Button::new("catalog-close")
                            .outline()
                            .label("Close")
                            .on_click(on_dismiss.clone()),
                    )
                    .into_any_element()
            })
            .child(modal_close(on_dismiss)),
    )
}

fn render_list(
    modal: &CatalogModal,
    on_toggle_category: impl Fn(String, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_sort: impl Fn(CatalogSort, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_more: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_open_project: impl Fn(CatalogProjectId, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let disabled = modal.no_key;
    let more = modal
        .page
        .as_ref()
        .is_some_and(|page| can_page_more(page.index, page.page_size, page.total));
    v_flex()
        .w_full()
        .flex_1()
        .min_h_0()
        .gap_3()
        .child(list_toolbar(modal, disabled, cx))
        .child(sort_row(modal.sort, disabled, on_sort, cx))
        .child(category_row(modal, disabled, on_toggle_category, cx))
        .when_some(modal.error.clone(), |this, error| {
            this.child(Alert::error("catalog-error", error))
        })
        .when(modal.no_key, |this| {
            this.child(empty_panel(
                IconName::TriangleAlert,
                "No CurseForge key",
                "The backend GET /get_cf_api_key has never returned a key.",
                cx,
            ))
        })
        .when(!modal.no_key, |this| {
            this.child(
                v_flex()
                    .id("catalog-results")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .overflow_y_scroll()
                    .when(modal.loading && modal.page.is_none(), |this| {
                        this.child(loading_row(cx))
                    })
                    .when(
                        !modal.loading
                            && modal.page.as_ref().is_none_or(|page| page.items.is_empty()),
                        |this| {
                            this.child(empty_panel(
                                IconName::Search,
                                "No projects found",
                                "Try another search or clear a category.",
                                cx,
                            ))
                        },
                    )
                    .children(modal.page.iter().flat_map(|page| {
                        page.items.iter().map(|project| {
                            let id = project.id.clone();
                            let on_open = on_open_project.clone();
                            project_card(
                                project,
                                modal.images.get(project.logo_url.as_deref().unwrap_or("")),
                                move |ev, window, cx| {
                                    on_open(id.clone(), ev, window, cx);
                                },
                                cx,
                            )
                        })
                    }))
                    .when(modal.loading && modal.page.is_some(), |this| {
                        this.child(loading_row(cx))
                    }),
            )
            .when(more, |this| {
                this.child(
                    Button::new("catalog-more")
                        .outline()
                        .label("More")
                        .disabled(modal.loading)
                        .on_click(on_more),
                )
            })
        })
}

fn list_toolbar(modal: &CatalogModal, disabled: bool, cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_center()
        .gap_2()
        .child(chip("CurseForge", cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Input::new(&modal.search).small().disabled(disabled)),
        )
        .when(matches!(modal.target, CatalogTarget::NewInstance), |this| {
            this.child(
                div()
                    .w(px(160.))
                    .child(Input::new(&modal.version_filter).small().disabled(disabled)),
            )
        })
        .when(matches!(modal.target, CatalogTarget::Instance(_)), |this| {
            this.children(modal.game_version.clone().map(|version| chip(version, cx)))
                .children(modal.loader.map(|loader| chip(loader_label(loader), cx)))
        })
}

fn sort_row(
    selected: CatalogSort,
    disabled: bool,
    on_sort: impl Fn(CatalogSort, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let options = [
        (CatalogSort::Popularity, "Popularity"),
        (CatalogSort::LastUpdated, "Updated"),
        (CatalogSort::Downloads, "Downloads"),
        (CatalogSort::Name, "Name"),
    ];
    h_flex()
        .id("catalog-sort")
        .flex_shrink_0()
        .gap_1()
        .p(px(3.))
        .rounded(px(10.))
        .bg(cx.theme().muted)
        .children(options.into_iter().map(|(sort, label)| {
            let on_sort = on_sort.clone();
            sort_tab(
                label,
                selected == sort,
                disabled,
                move |ev, window, cx| on_sort(sort, ev, window, cx),
                cx,
            )
        }))
}

fn sort_tab(
    label: &'static str,
    active: bool,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let (bg, fg) = if active {
        (cx.theme().primary, cx.theme().primary_foreground)
    } else {
        (cx.theme().transparent, cx.theme().muted_foreground)
    };
    h_flex()
        .id(SharedString::from(format!("catalog-sort-{label}")))
        .h(px(26.))
        .px_3()
        .items_center()
        .rounded(px(8.))
        .bg(bg)
        .text_color(fg)
        .when(!disabled, |this| this.cursor_pointer())
        .when(!active && !disabled, |this| {
            this.hover(|this| this.text_color(cx.theme().foreground))
        })
        .when(!disabled, |this| this.on_click(on_click))
        .child(div().text_xs().font_weight(FontWeight::MEDIUM).child(label))
}

fn category_row(
    modal: &CatalogModal,
    disabled: bool,
    on_toggle_category: impl Fn(String, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .id("catalog-categories")
        .w_full()
        .flex_wrap()
        .gap_1()
        .max_h(px(72.))
        .overflow_y_scroll()
        .children(modal.categories.iter().map(|category| {
            let id = category.id.clone();
            let selected = modal.selected_categories.iter().any(|item| item == &id);
            let on_toggle = on_toggle_category.clone();
            category_chip(
                id.clone(),
                category.name.clone(),
                selected,
                disabled,
                move |ev, window, cx| on_toggle(id.clone(), ev, window, cx),
                cx,
            )
        }))
}

fn category_chip(
    id: String,
    name: String,
    selected: bool,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let key = SharedString::from(format!("cat-{id}"));
    div()
        .id(key)
        .h(px(22.))
        .px_2()
        .flex()
        .items_center()
        .rounded(px(6.))
        .bg(if selected {
            cx.theme().primary
        } else {
            cx.theme().muted
        })
        .text_color(if selected {
            cx.theme().primary_foreground
        } else {
            cx.theme().muted_foreground
        })
        .text_xs()
        .when(!disabled, |this| {
            this.cursor_pointer()
                .hover(|this| this.bg(cx.theme().secondary_hover))
                .on_click(on_click)
        })
        .child(name)
}

fn project_card(
    project: &CatalogProject,
    logo: Option<&PathBuf>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let key = SharedString::from(format!("project-{}", project.id.0));
    let authors = if project.authors.is_empty() {
        "Unknown author".into()
    } else {
        project.authors.join(", ")
    };
    h_flex()
        .id(key)
        .w_full()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded(px(10.))
        .bg(cx.theme().muted)
        .cursor_pointer()
        .hover(|this| this.bg(cx.theme().secondary_hover))
        .on_click(on_click)
        .child(logo_view(logo, px(44.), cx))
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_ellipsis()
                        .child(project.name.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .text_ellipsis()
                        .child(authors),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format_downloads(project.download_count)),
        )
}

fn render_project(
    modal: &CatalogModal,
    on_back: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_select_file: impl Fn(String, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_more_files: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    on_website: impl Fn(String, &ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let Some(detail) = modal.project.as_ref() else {
        return v_flex().into_any_element();
    };
    let more_files = modal
        .files
        .as_ref()
        .is_some_and(|page| can_page_more(page.index, page.page_size, page.total));
    v_flex()
        .id("catalog-project")
        .w_full()
        .flex_1()
        .min_h_0()
        .gap_3()
        .child(
            Button::new("catalog-project-back")
                .ghost()
                .compact()
                .label("Back")
                .on_click(on_back),
        )
        .child(
            h_flex()
                .w_full()
                .items_start()
                .gap_3()
                .child(logo_view(
                    detail
                        .project
                        .logo_url
                        .as_ref()
                        .and_then(|url| modal.images.get(url)),
                    px(56.),
                    cx,
                ))
                .child(
                    v_flex()
                        .min_w_0()
                        .flex_1()
                        .gap_1()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::MEDIUM)
                                .child(detail.project.name.clone()),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(detail.project.summary.clone()),
                        )
                        .when_some(detail.website_url.clone(), |this, url| {
                            let on_website = on_website.clone();
                            let href = url.clone();
                            this.child(
                                div()
                                    .id("catalog-website")
                                    .text_sm()
                                    .text_color(cx.theme().link)
                                    .cursor_pointer()
                                    .hover(|this| this.text_color(cx.theme().link_hover))
                                    .on_click(move |ev, window, cx| {
                                        on_website(url.clone(), ev, window, cx);
                                    })
                                    .child(href),
                            )
                        }),
                ),
        )
        .when(!detail.screenshot_urls.is_empty(), |this| {
            this.child(
                h_flex()
                    .id("catalog-screenshots")
                    .w_full()
                    .gap_2()
                    .overflow_x_scroll()
                    .children(
                        detail
                            .screenshot_urls
                            .iter()
                            .enumerate()
                            .map(|(index, url)| screenshot_view(index, modal.images.get(url), cx)),
                    ),
            )
        })
        .when_some(modal.error.clone(), |this, error| {
            this.child(Alert::error("catalog-project-error", error))
        })
        .child(
            v_flex()
                .id("catalog-files")
                .w_full()
                .flex_1()
                .min_h_0()
                .gap_1()
                .overflow_y_scroll()
                .when(modal.loading && modal.files.is_none(), |this| {
                    this.child(loading_row(cx))
                })
                .when(
                    !modal.loading
                        && modal
                            .files
                            .as_ref()
                            .is_none_or(|page| page.items.is_empty()),
                    |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("No files match this filter."),
                        )
                    },
                )
                .children(modal.files.iter().flat_map(|page| {
                    page.items.iter().map(|file| {
                        let id = file.file_id.clone();
                        let on_select = on_select_file.clone();
                        file_row(
                            file,
                            modal.selected_file.as_deref() == Some(file.file_id.as_str()),
                            move |ev, window, cx| on_select(id.clone(), ev, window, cx),
                            cx,
                        )
                    })
                })),
        )
        .when(more_files, |this| {
            this.child(
                Button::new("catalog-more-files")
                    .outline()
                    .label("More")
                    .disabled(modal.loading)
                    .on_click(on_more_files),
            )
        })
        .into_any_element()
}

fn file_row(
    file: &CatalogFile,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let key = SharedString::from(format!("file-{}", file.file_id));
    let versions = if file.game_versions.is_empty() {
        String::new()
    } else {
        file.game_versions.join(", ")
    };
    let meta = [
        release_label(file.release),
        versions.as_str(),
        file.file_date.as_deref().unwrap_or(""),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    v_flex()
        .id(key)
        .w_full()
        .gap_1()
        .px_3()
        .py_2()
        .rounded(px(10.))
        .border_1()
        .border_color(if selected {
            cx.theme().foreground.opacity(0.16)
        } else {
            cx.theme().border.opacity(0.)
        })
        .bg(if selected {
            cx.theme().muted
        } else {
            cx.theme().popover
        })
        .cursor_pointer()
        .hover(|this| this.bg(cx.theme().muted))
        .on_click(on_click)
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_ellipsis()
                .child(file.display_name.clone()),
        )
        .when(!meta.is_empty(), |this| {
            this.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .text_ellipsis()
                    .child(meta),
            )
        })
}

fn logo_view(path: Option<&PathBuf>, size: gpui::Pixels, cx: &App) -> impl IntoElement {
    let frame = div()
        .size(size)
        .flex_shrink_0()
        .rounded(px(8.))
        .overflow_hidden()
        .bg(cx.theme().secondary_active)
        .border_1()
        .border_color(cx.theme().border.opacity(0.55));
    match path {
        Some(path) => frame
            .child(
                img(path.clone())
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .rounded(px(8.)),
            )
            .into_any_element(),
        None => frame.into_any_element(),
    }
}

fn screenshot_view(index: usize, path: Option<&PathBuf>, cx: &App) -> impl IntoElement {
    let frame = div()
        .id(SharedString::from(format!("shot-{index}")))
        .w(px(200.))
        .h(px(112.))
        .flex_shrink_0()
        .rounded(px(8.))
        .overflow_hidden()
        .bg(cx.theme().muted)
        .border_1()
        .border_color(cx.theme().border.opacity(0.55));
    match path {
        Some(path) => frame
            .child(
                img(path.clone())
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .rounded(px(8.)),
            )
            .into_any_element(),
        None => frame.into_any_element(),
    }
}

fn loading_row(cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .child(Spinner::new().small().color(cx.theme().muted_foreground))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Loading…"),
        )
}

fn release_label(release: CatalogRelease) -> &'static str {
    match release {
        CatalogRelease::Release => "Release",
        CatalogRelease::Beta => "Beta",
        CatalogRelease::Alpha => "Alpha",
        CatalogRelease::Other => "Other",
    }
}

fn format_downloads(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M downloads", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k downloads", n as f64 / 1_000.0)
    } else {
        format!("{n} downloads")
    }
}
