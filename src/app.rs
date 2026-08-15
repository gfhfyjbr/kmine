use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    App, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, TitlebarOptions, WeakEntity, Window, WindowBounds,
    WindowOptions, div, px, size,
};
use gpui_component::{
    ActiveTheme, Root,
    dialog::Cancel,
    h_flex,
    input::{InputEvent, InputState},
    v_flex,
};

use crate::chrome::{chip, loader_label};
use crate::providers::CurseForgeProvider;
use kmine_engine::{
    AccountId, CancellationToken, CatalogError, CatalogFileFilter, CatalogProject,
    CatalogProjectDetail, CatalogProjectId, CatalogQuery, CatalogSort, ContentClass, ContentEntry,
    ContentFolder, Engine, EngineError, Event, InstanceId, InstancePatch, InstanceSummary, Loader,
    QuickPlay, QuickPlayLists, SandboxStatus,
};

use crate::modals::accounts::{self, AccountsModal};
use crate::modals::catalog::{self, CatalogModal, CatalogTarget};
use crate::modals::confirm;
use crate::modals::create_instance::{self, CreateInstanceForm};
use crate::modals::progress::{self, EventProgressSink, ProgressModal};
use crate::screens::{instance_content, instance_play, instance_settings, instances};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum InstancePane {
    #[default]
    Play,
    Content,
    Settings,
}

pub struct KmineApp {
    engine: Arc<Engine>,
    rt: tokio::runtime::Handle,
    instances: Vec<InstanceSummary>,
    selected: Option<InstanceId>,
    show_create: bool,
    show_accounts: bool,
    status: String,
    create: Option<CreateInstanceForm>,
    catalog: Option<CatalogModal>,
    search_gen: u64,
    detail_gen: u64,
    accounts: AccountsModal,
    progress: Option<ProgressModal>,
    cancel: Option<CancellationToken>,
    rename: Option<instances::RenameForm>,
    instance_pane: InstancePane,
    content_mods: Vec<ContentEntry>,
    content_resourcepacks: Vec<ContentEntry>,
    content_shaderpacks: Vec<ContentEntry>,
    quick_play: QuickPlayLists,
    settings: Option<instance_settings::SettingsForm>,
    skin_face: Option<PathBuf>,
    skin_for: Option<AccountId>,
    pinned: HashSet<InstanceId>,
}

impl KmineApp {
    pub fn new(engine: Arc<Engine>, cx: &mut Context<Self>) -> Self {
        engine.add_provider(Arc::new(CurseForgeProvider::new()));
        engine.start_catalog_key_refresh();
        let accounts = AccountsModal::from_engine(&engine);
        let instances = engine.list_instances().unwrap_or_default();
        let mut this = Self {
            engine,
            rt: tokio::runtime::Handle::current(),
            instances,
            selected: None,
            show_create: false,
            show_accounts: false,
            status: String::new(),
            create: None,
            catalog: None,
            search_gen: 0,
            detail_gen: 0,
            accounts,
            progress: None,
            cancel: None,
            rename: None,
            instance_pane: InstancePane::Play,
            content_mods: Vec::new(),
            content_resourcepacks: Vec::new(),
            content_shaderpacks: Vec::new(),
            quick_play: QuickPlayLists::default(),
            settings: None,
            skin_face: None,
            skin_for: None,
            pinned: HashSet::new(),
        };
        this.listen_engine_events(cx);
        this.ensure_skin(cx);
        this
    }

    fn refresh_instances(&mut self) {
        self.instances = self.engine.list_instances().unwrap_or_default();
    }

    fn refresh_accounts(&mut self) {
        self.accounts.refresh(&self.engine);
    }

    fn selected_account_id(&self) -> Option<AccountId> {
        self.accounts
            .accounts
            .iter()
            .find(|account| account.selected)
            .map(|account| account.uuid)
    }

    fn ensure_skin(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected_account_id();
        if selected != self.skin_for {
            self.skin_for = selected;
            self.skin_face = selected.and_then(|id| self.engine.cached_skin_face(id));
        }
        let Some(id) = selected else {
            self.skin_face = None;
            return;
        };
        if self.skin_face.is_some() {
            return;
        }
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let fetched = rt
                .spawn(async move { engine.ensure_skin_face(id).await })
                .await;
            this.update(cx, |this, cx| {
                if this.skin_for == Some(id) {
                    if let Ok(Ok(path)) = fetched {
                        this.skin_face = Some(path);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn selected_instance(&self) -> Option<&InstanceSummary> {
        let id = self.selected?;
        self.instances.iter().find(|instance| instance.id == id)
    }

    fn select_instance(&mut self, id: InstanceId, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(id);
        self.reload_content();
        self.reload_quick_play();
        if self.instance_pane == InstancePane::Settings {
            self.load_settings(id, window, cx);
        }
        cx.notify();
    }

    fn set_pane(&mut self, pane: InstancePane, window: &mut Window, cx: &mut Context<Self>) {
        self.instance_pane = pane;
        match pane {
            InstancePane::Play => self.reload_quick_play(),
            InstancePane::Content => self.reload_content(),
            InstancePane::Settings => {
                if let Some(id) = self.selected {
                    self.load_settings(id, window, cx);
                }
            }
        }
        cx.notify();
    }

    fn reload_content(&mut self) {
        let Some(id) = self.selected else {
            self.content_mods.clear();
            self.content_resourcepacks.clear();
            self.content_shaderpacks.clear();
            return;
        };
        self.content_mods = self
            .engine
            .list_content(id, ContentFolder::Mods)
            .unwrap_or_default();
        self.content_resourcepacks = self
            .engine
            .list_content(id, ContentFolder::Resourcepacks)
            .unwrap_or_default();
        self.content_shaderpacks = self
            .engine
            .list_content(id, ContentFolder::Shaderpacks)
            .unwrap_or_default();
    }

    fn reload_quick_play(&mut self) {
        let Some(id) = self.selected else {
            self.quick_play = QuickPlayLists::default();
            return;
        };
        self.quick_play = self.engine.list_quick_play(id).unwrap_or_default();
    }

    fn load_settings(&mut self, id: InstanceId, window: &mut Window, cx: &mut Context<Self>) {
        match self.engine.get_instance(id) {
            Ok(Some(row)) => {
                self.settings = Some(instance_settings::SettingsForm::from_row(
                    &row,
                    &self.accounts.accounts,
                    window,
                    cx,
                ));
            }
            Ok(None) => {
                self.settings = None;
                self.status = "instance not found".into();
            }
            Err(err) => self.status = err.to_string(),
        }
    }

    fn apply_patch(&mut self, id: InstanceId, patch: InstancePatch, cx: &mut Context<Self>) {
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = rt
                .spawn(async move { engine.update_instance(id, patch).await })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(())) => this.status.clear(),
                    Ok(Err(err)) => this.status = err.to_string(),
                    Err(err) => this.status = err.to_string(),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.settings.as_ref() else {
            return;
        };
        let id = form.instance_id;
        let patch = form.patch(cx);
        self.apply_patch(id, patch, cx);
    }

    fn set_sandbox(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let Some(form) = self.settings.as_mut() else {
            return;
        };
        form.sandbox = enabled;
        let id = form.instance_id;
        self.apply_patch(
            id,
            InstancePatch {
                sandbox: Some(enabled),
                ..Default::default()
            },
            cx,
        );
    }

    fn toggle_content(&mut self, path: PathBuf, enabled: bool, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        match self.engine.set_content_enabled(id, &path, enabled) {
            Ok(()) => {
                self.status.clear();
                self.reload_content();
            }
            Err(err) => self.status = err.to_string(),
        }
        cx.notify();
    }

    fn confirm_delete_content(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("this file")
            .to_string();
        let this = cx.weak_entity();
        confirm::danger(
            window,
            cx,
            "Delete file",
            format!("\"{name}\" will be removed from this instance."),
            "Delete",
            move |_, cx| {
                this.update(cx, |this, cx| this.delete_content(path.clone(), cx))
                    .ok();
            },
        );
    }

    fn delete_content(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        match self.engine.delete_content(id, &path) {
            Ok(()) => {
                self.status.clear();
                self.reload_content();
            }
            Err(err) => self.status = err.to_string(),
        }
        cx.notify();
    }

    fn listen_engine_events(&self, cx: &mut Context<Self>) {
        let mut rx = self.engine.subscribe();
        let rt = self.rt.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                let joined = rt.spawn(async move {
                    let result = rx.recv().await;
                    (rx, result)
                });
                let (next_rx, result) = match joined.await {
                    Ok(pair) => pair,
                    Err(_) => break,
                };
                rx = next_rx;
                match result {
                    Ok(event) => {
                        if this
                            .update(cx, |this, cx| {
                                this.handle_engine_event(event);
                                this.ensure_skin(cx);
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                }
            }
        })
        .detach();
    }

    fn handle_engine_event(&mut self, event: Event) {
        match event {
            Event::AccountsChanged => self.refresh_accounts(),
            Event::InstancesChanged => {
                self.refresh_instances();
                if let Some(id) = self.selected {
                    if !self.instances.iter().any(|instance| instance.id == id) {
                        self.selected = None;
                        self.settings = None;
                    }
                }
                self.reload_content();
                self.reload_quick_play();
            }
            Event::Progress {
                id,
                title,
                done,
                total,
            } => {
                if let Some(progress) = self.progress.as_mut().filter(|p| p.id == id) {
                    progress.title = title;
                    progress.done = done;
                    progress.total = total;
                }
            }
            Event::PrepareFinished { id, .. } => {
                if self.progress.as_ref().is_some_and(|p| p.id == id) {
                    self.progress = None;
                    self.cancel = None;
                }
                self.refresh_instances();
            }
            Event::AuthRequired => {
                self.refresh_accounts();
                self.show_accounts = true;
            }
            Event::ProcessExited { .. } => {
                self.refresh_instances();
                self.reload_quick_play();
            }
            Event::Error(message) => self.status = message,
            Event::LogLine { .. } => {}
        }
    }

    fn open_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.progress.is_some() {
            return;
        }
        self.close_catalog(cx);
        self.close_accounts(cx);
        let form = CreateInstanceForm::new(window, cx);
        cx.subscribe(&form.name, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit_create(cx);
            }
        })
        .detach();
        cx.subscribe(&form.version, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit_create(cx);
            }
        })
        .detach();
        self.create = Some(form);
        self.status.clear();
        self.show_create = true;
        cx.notify();
    }

    fn set_create_kind(&mut self, loader: Loader, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = self.create.as_mut() {
            form.phase = create_instance::CreatePhase::Loader(loader);
            form.name.update(cx, |state, cx| state.focus(window, cx));
            self.status.clear();
            cx.notify();
        }
    }

    fn create_back(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = self.create.as_mut() {
            form.phase = create_instance::CreatePhase::Kind;
            self.status.clear();
            cx.notify();
        }
    }

    fn close_create(&mut self, cx: &mut Context<Self>) {
        self.show_create = false;
        self.create = None;
        cx.notify();
    }

    fn open_modpack_catalog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.progress.is_some() {
            return;
        }
        self.show_create = false;
        self.create = None;
        self.open_catalog(
            ContentClass::Modpacks,
            CatalogTarget::NewInstance,
            None,
            None,
            window,
            cx,
        );
    }

    fn open_content_catalog(
        &mut self,
        class: ContentClass,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.progress.is_some() {
            return;
        }
        let Some(instance) = self.selected_instance().cloned() else {
            return;
        };
        self.open_catalog(
            class,
            CatalogTarget::Instance(instance.id),
            Some(instance.minecraft_version),
            Some(instance.loader),
            window,
            cx,
        );
    }

    fn open_catalog(
        &mut self,
        class: ContentClass,
        target: CatalogTarget,
        game_version: Option<String>,
        loader: Option<Loader>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.progress.is_some() {
            return;
        }
        self.close_accounts(cx);
        let pack_filter = matches!(target, CatalogTarget::NewInstance);
        let modal = CatalogModal::new(class, target, game_version, loader, window, cx);
        cx.subscribe(&modal.search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.schedule_catalog_search(cx);
            }
        })
        .detach();
        if pack_filter {
            cx.subscribe(&modal.version_filter, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.schedule_catalog_search(cx);
                }
            })
            .detach();
        }
        self.catalog = Some(modal);
        self.status.clear();
        self.search_gen = self.search_gen.wrapping_add(1);
        self.detail_gen = self.detail_gen.wrapping_add(1);
        self.load_catalog_categories(cx);
        cx.notify();
    }

    fn close_catalog(&mut self, cx: &mut Context<Self>) {
        self.catalog = None;
        self.search_gen = self.search_gen.wrapping_add(1);
        self.detail_gen = self.detail_gen.wrapping_add(1);
        cx.notify();
    }

    fn catalog_query(&self, cx: &App, index: u32, page_size: u32) -> Option<CatalogQuery> {
        let catalog = self.catalog.as_ref()?;
        let typed = catalog.search.read(cx).value();
        let trimmed = typed.trim();
        let search = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        let game_version = match catalog.target {
            CatalogTarget::Instance(_) => catalog.game_version.clone(),
            CatalogTarget::NewInstance => {
                let value = catalog.version_filter.read(cx).value();
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
        };
        let loader = match catalog.target {
            CatalogTarget::Instance(_) => catalog.loader,
            CatalogTarget::NewInstance => None,
        };
        Some(CatalogQuery {
            class: catalog.class,
            provider: catalog.provider,
            search,
            category_ids: catalog.selected_categories.clone(),
            game_version,
            loader,
            sort: catalog.sort,
            index,
            page_size,
        })
    }

    fn load_catalog_categories(&mut self, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let provider = catalog.provider;
        let class = catalog.class;
        let generation = self.search_gen;
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = rt
                .spawn(async move { engine.catalog_categories(provider, class).await })
                .await;
            this.update(cx, |this, cx| {
                let Some(catalog) = this.catalog.as_mut() else {
                    return;
                };
                match result {
                    Ok(Ok(categories)) => {
                        catalog.categories = categories;
                        catalog.no_key = false;
                        catalog.error = None;
                        if this.search_gen == generation {
                            this.run_catalog_search(false, cx);
                        } else {
                            catalog.loading = false;
                            cx.notify();
                        }
                    }
                    Ok(Err(EngineError::Catalog(CatalogError::Unavailable))) => {
                        catalog.no_key = true;
                        catalog.loading = false;
                        cx.notify();
                    }
                    Ok(Err(err)) => {
                        catalog.error = Some(err.to_string());
                        catalog.loading = false;
                        cx.notify();
                    }
                    Err(err) => {
                        catalog.error = Some(err.to_string());
                        catalog.loading = false;
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn schedule_catalog_search(&mut self, cx: &mut Context<Self>) {
        if self.catalog.as_ref().is_some_and(|catalog| catalog.no_key) {
            return;
        }
        self.search_gen = self.search_gen.wrapping_add(1);
        let generation = self.search_gen;
        let rt = self.rt.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let _ = rt
                .spawn(async { tokio::time::sleep(Duration::from_millis(300)).await })
                .await;
            this.update(cx, |this, cx| {
                if this.search_gen != generation {
                    return;
                }
                this.run_catalog_search(false, cx);
            })
            .ok();
        })
        .detach();
    }

    fn run_catalog_search(&mut self, append: bool, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        if catalog.no_key {
            return;
        }
        let (index, page_size) = if append {
            catalog
                .page
                .as_ref()
                .map(|page| (page.index.saturating_add(page.page_size), page.page_size))
                .unwrap_or((0, catalog::PAGE_SIZE))
        } else {
            (0, catalog::PAGE_SIZE)
        };
        let Some(query) = self.catalog_query(cx, index, page_size) else {
            return;
        };
        if let Some(catalog) = self.catalog.as_mut() {
            catalog.loading = true;
            if !append {
                catalog.error = None;
            }
        }
        let generation = self.search_gen;
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        cx.notify();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = rt
                .spawn(async move { engine.catalog_search(&query).await })
                .await;
            this.update(cx, |this, cx| {
                if this.search_gen != generation {
                    return;
                }
                let Some(catalog) = this.catalog.as_mut() else {
                    return;
                };
                catalog.loading = false;
                match result {
                    Ok(Ok(page)) => {
                        catalog.error = None;
                        if append {
                            if let Some(existing) = catalog.page.as_mut() {
                                existing.items.extend(page.items);
                                existing.index = page.index;
                                existing.page_size = page.page_size;
                                existing.total = page.total;
                            } else {
                                catalog.page = Some(page);
                            }
                        } else {
                            catalog.page = Some(page);
                        }
                        this.prefetch_catalog_images(cx);
                    }
                    Ok(Err(EngineError::Catalog(CatalogError::Unavailable))) => {
                        catalog.no_key = true;
                    }
                    Ok(Err(err)) => catalog.error = Some(err.to_string()),
                    Err(err) => catalog.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn catalog_load_more(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.page.as_ref())
        else {
            return;
        };
        if !catalog::can_page_more(page.index, page.page_size, page.total) {
            return;
        }
        self.run_catalog_search(true, cx);
    }

    fn toggle_catalog_category(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.as_mut() else {
            return;
        };
        if catalog.no_key {
            return;
        }
        if let Some(index) = catalog
            .selected_categories
            .iter()
            .position(|item| item == &id)
        {
            catalog.selected_categories.remove(index);
        } else if catalog.selected_categories.len() < 10 {
            catalog.selected_categories.push(id);
        } else {
            return;
        }
        self.search_gen = self.search_gen.wrapping_add(1);
        self.run_catalog_search(false, cx);
    }

    fn set_catalog_sort(&mut self, sort: CatalogSort, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.as_mut() else {
            return;
        };
        if catalog.no_key || catalog.sort == sort {
            return;
        }
        catalog.sort = sort;
        self.search_gen = self.search_gen.wrapping_add(1);
        self.run_catalog_search(false, cx);
    }

    fn open_catalog_project(&mut self, id: CatalogProjectId, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.as_mut() else {
            return;
        };
        if let Some(card) = catalog
            .page
            .as_ref()
            .and_then(|page| page.items.iter().find(|project| project.id == id))
            .cloned()
        {
            catalog.project = Some(stub_project(card));
        }
        catalog.files = None;
        catalog.selected_file = None;
        catalog.loading = true;
        catalog.error = None;
        self.detail_gen = self.detail_gen.wrapping_add(1);
        let generation = self.detail_gen;
        let provider = catalog.provider;
        let filter = CatalogFileFilter {
            game_version: match catalog.target {
                CatalogTarget::Instance(_) => catalog.game_version.clone(),
                CatalogTarget::NewInstance => {
                    let value = catalog.version_filter.read(cx).value();
                    let trimmed = value.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                }
            },
            loader: match catalog.target {
                CatalogTarget::Instance(_) => catalog.loader,
                CatalogTarget::NewInstance => None,
            },
            index: 0,
            page_size: catalog::PAGE_SIZE,
        };
        let engine = self.engine.clone();
        let project_engine = engine.clone();
        let files_engine = engine;
        let rt = self.rt.clone();
        let project_id = id.clone();
        let files_id = id;
        cx.notify();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let fetched = rt
                .spawn(async move {
                    let project = project_engine.catalog_project(provider, &project_id).await;
                    let files = files_engine
                        .catalog_files(provider, &files_id, &filter)
                        .await;
                    (project, files)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.detail_gen != generation {
                    return;
                }
                let Some(catalog) = this.catalog.as_mut() else {
                    return;
                };
                catalog.loading = false;
                match fetched {
                    Ok((project, files)) => {
                        match project {
                            Ok(detail) => catalog.project = Some(detail),
                            Err(EngineError::Catalog(CatalogError::Unavailable)) => {
                                catalog.no_key = true;
                            }
                            Err(err) => catalog.error = Some(err.to_string()),
                        }
                        match files {
                            Ok(page) => {
                                if page.items.len() == 1 {
                                    catalog.selected_file = Some(page.items[0].file_id.clone());
                                }
                                catalog.files = Some(page);
                            }
                            Err(err) => {
                                if catalog.error.is_none() {
                                    catalog.error = Some(err.to_string());
                                }
                            }
                        }
                        this.prefetch_catalog_images(cx);
                    }
                    Err(err) => catalog.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn catalog_back(&mut self, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.as_mut() else {
            return;
        };
        catalog.project = None;
        catalog.files = None;
        catalog.selected_file = None;
        catalog.error = None;
        catalog.loading = false;
        self.detail_gen = self.detail_gen.wrapping_add(1);
        cx.notify();
    }

    fn select_catalog_file(&mut self, file_id: String, cx: &mut Context<Self>) {
        if let Some(catalog) = self.catalog.as_mut() {
            catalog.selected_file = Some(file_id);
            cx.notify();
        }
    }

    fn catalog_load_more_files(&mut self, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let Some(project) = catalog.project.as_ref() else {
            return;
        };
        let Some(page) = catalog.files.as_ref() else {
            return;
        };
        if !catalog::can_page_more(page.index, page.page_size, page.total) {
            return;
        }
        let provider = catalog.provider;
        let project_id = project.project.id.clone();
        let filter = CatalogFileFilter {
            game_version: match catalog.target {
                CatalogTarget::Instance(_) => catalog.game_version.clone(),
                CatalogTarget::NewInstance => {
                    let value = catalog.version_filter.read(cx).value();
                    let trimmed = value.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                }
            },
            loader: match catalog.target {
                CatalogTarget::Instance(_) => catalog.loader,
                CatalogTarget::NewInstance => None,
            },
            index: page.index.saturating_add(page.page_size),
            page_size: page.page_size,
        };
        self.detail_gen = self.detail_gen.wrapping_add(1);
        let generation = self.detail_gen;
        if let Some(catalog) = self.catalog.as_mut() {
            catalog.loading = true;
        }
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        cx.notify();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = rt
                .spawn(async move { engine.catalog_files(provider, &project_id, &filter).await })
                .await;
            this.update(cx, |this, cx| {
                if this.detail_gen != generation {
                    return;
                }
                let Some(catalog) = this.catalog.as_mut() else {
                    return;
                };
                catalog.loading = false;
                match result {
                    Ok(Ok(page)) => {
                        if let Some(existing) = catalog.files.as_mut() {
                            existing.items.extend(page.items);
                            existing.index = page.index;
                            existing.page_size = page.page_size;
                            existing.total = page.total;
                        } else {
                            catalog.files = Some(page);
                        }
                    }
                    Ok(Err(err)) => catalog.error = Some(err.to_string()),
                    Err(err) => catalog.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn prefetch_catalog_images(&mut self, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let mut urls = Vec::new();
        if let Some(page) = catalog.page.as_ref() {
            urls.extend(page.items.iter().filter_map(|item| item.logo_url.clone()));
        }
        if let Some(detail) = catalog.project.as_ref() {
            urls.extend(detail.screenshot_urls.iter().cloned());
            if let Some(logo) = detail.project.logo_url.clone() {
                urls.push(logo);
            }
        }
        urls.retain(|url| !url.is_empty() && !catalog.images.contains_key(url));
        urls.sort();
        urls.dedup();
        for url in urls {
            self.cache_catalog_image(url, cx);
        }
    }

    fn cache_catalog_image(&mut self, url: String, cx: &mut Context<Self>) {
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        let key = url.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = rt
                .spawn(async move { engine.cache_remote_image(&url).await })
                .await;
            this.update(cx, |this, cx| {
                if let (Some(catalog), Ok(Ok(path))) = (this.catalog.as_mut(), result) {
                    catalog.images.insert(key, path);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn open_catalog_website(&mut self, url: String) {
        let _ = open::that(url);
    }

    fn install_catalog(&mut self, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let Some(detail) = catalog.project.as_ref() else {
            return;
        };
        let Some(file_id) = catalog.selected_file.clone() else {
            return;
        };
        let provider = catalog.provider;
        let target = catalog.target;
        let project_id = detail.project.id.clone();
        let name = detail.project.name.clone();
        self.catalog = None;
        self.show_create = false;
        self.create = None;
        self.search_gen = self.search_gen.wrapping_add(1);
        self.detail_gen = self.detail_gen.wrapping_add(1);
        if self.progress.is_some() {
            return;
        }
        let progress_id = match target {
            CatalogTarget::Instance(id) => id,
            CatalogTarget::NewInstance => InstanceId::new(),
        };
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        self.progress = Some(ProgressModal {
            id: progress_id,
            name: name.clone(),
            title: "Installing…".into(),
            done: 0,
            total: 0,
        });
        self.status.clear();
        cx.notify();

        let engine = self.engine.clone();
        let rt = self.rt.clone();
        let events = engine.event_sender();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = rt
                .spawn(async move {
                    let sink = EventProgressSink::new(events, progress_id);
                    match target {
                        CatalogTarget::NewInstance => engine
                            .install_pack(provider, &project_id, &file_id, None, &sink, &cancel)
                            .await
                            .map(InstallOutcome::Pack),
                        CatalogTarget::Instance(id) => engine
                            .install_content(id, provider, &project_id, &file_id, &sink, &cancel)
                            .await
                            .map(|()| InstallOutcome::Content),
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.progress = None;
                this.cancel = None;
                match result {
                    Ok(Ok(InstallOutcome::Pack(id))) => {
                        this.selected = Some(id);
                        this.refresh_instances();
                        this.reload_content();
                        this.reload_quick_play();
                        this.status.clear();
                    }
                    Ok(Ok(InstallOutcome::Content)) => {
                        this.status.clear();
                        this.reload_content();
                    }
                    Ok(Err(EngineError::Cancelled)) => this.status.clear(),
                    Ok(Err(err)) => this.status = err.to_string(),
                    Err(err) => this.status = err.to_string(),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open_accounts(&mut self, cx: &mut Context<Self>) {
        self.close_catalog(cx);
        self.show_create = false;
        self.create = None;
        self.refresh_accounts();
        self.show_accounts = true;
        cx.notify();
    }

    fn close_accounts(&mut self, cx: &mut Context<Self>) {
        self.show_accounts = false;
        self.engine.cancel_login();
        cx.notify();
    }

    fn submit_create(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.create.as_ref() else {
            return;
        };
        if self.status == "Creating…" {
            return;
        }
        let Some(spec) = form.spec(cx) else {
            return;
        };
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        self.status = "Creating…".into();
        cx.notify();

        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = rt
                .spawn(async move { engine.create_instance(spec).await })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(id)) => {
                        this.show_create = false;
                        this.create = None;
                        this.selected = Some(id);
                        this.refresh_instances();
                        this.reload_content();
                        this.reload_quick_play();
                        this.status.clear();
                    }
                    Ok(Err(err)) => this.status = err.to_string(),
                    Err(err) => this.status = err.to_string(),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open_rename(&mut self, id: InstanceId, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .instances
            .iter()
            .find(|instance| instance.id == id)
            .map(|instance| instance.name.clone())
            .unwrap_or_default();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit_rename(cx);
            }
        })
        .detach();
        self.rename = Some(instances::RenameForm { id, name: input });
        self.status.clear();
        cx.notify();
    }

    fn close_rename(&mut self, cx: &mut Context<Self>) {
        self.rename = None;
        cx.notify();
    }

    fn submit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.rename.as_ref() else {
            return;
        };
        let id = form.id;
        let name = form.name.read(cx).value().to_string();
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        self.status = "Renaming…".into();
        cx.notify();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = rt
                .spawn(async move { engine.rename_instance(id, name).await })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(())) => {
                        this.rename = None;
                        this.refresh_instances();
                        this.status.clear();
                    }
                    Ok(Err(err)) => this.status = err.to_string(),
                    Err(err) => this.status = err.to_string(),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn toggle_pin(&mut self, id: InstanceId, cx: &mut Context<Self>) {
        if !self.pinned.remove(&id) {
            self.pinned.insert(id);
        }
        cx.notify();
    }

    fn open_delete(&mut self, id: InstanceId, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .instances
            .iter()
            .find(|instance| instance.id == id)
            .map(|instance| instance.name.clone())
            .unwrap_or_else(|| "this instance".into());
        let this = cx.weak_entity();
        confirm::danger(
            window,
            cx,
            "Delete instance",
            format!("\"{name}\" and its world files will be removed from this machine."),
            "Delete",
            move |_, cx| {
                this.update(cx, |this, cx| this.delete_instance(id, cx))
                    .ok();
            },
        );
    }

    fn delete_instance(&mut self, id: InstanceId, cx: &mut Context<Self>) {
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        self.rename = None;
        cx.notify();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = rt
                .spawn(async move { engine.delete_instance(id).await })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(())) => {
                        this.pinned.remove(&id);
                        if this.selected == Some(id) {
                            this.selected = None;
                            this.settings = None;
                        }
                        this.refresh_instances();
                        this.reload_content();
                        this.reload_quick_play();
                        this.status.clear();
                    }
                    Ok(Err(err)) => this.status = err.to_string(),
                    Err(err) => this.status = err.to_string(),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_login(&mut self, cx: &mut Context<Self>) {
        if self.accounts.busy {
            return;
        }
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        self.accounts.busy = true;
        self.accounts.error = None;
        cx.notify();

        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = rt.spawn(async move { engine.start_login().await }).await;
            this.update(cx, |this, cx| {
                this.accounts.busy = false;
                match result {
                    Ok(Ok(_)) => {
                        this.refresh_accounts();
                        this.accounts.error = None;
                        this.ensure_skin(cx);
                    }
                    Ok(Err(EngineError::AuthNotConfigured)) => {
                        this.accounts.error = Some(accounts::AUTH_NOT_CONFIGURED_HINT.into());
                    }
                    Ok(Err(EngineError::Cancelled)) => {
                        this.accounts.error = None;
                    }
                    Ok(Err(err)) => this.accounts.error = Some(err.to_string()),
                    Err(err) => this.accounts.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn select_account(&mut self, id: AccountId, cx: &mut Context<Self>) {
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = rt
                .spawn(async move { engine.select_account(id).await })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(())) => {
                        this.refresh_accounts();
                        this.accounts.error = None;
                        this.ensure_skin(cx);
                    }
                    Ok(Err(err)) => this.accounts.error = Some(err.to_string()),
                    Err(err) => this.accounts.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn confirm_delete_account(
        &mut self,
        id: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self
            .accounts
            .accounts
            .iter()
            .find(|account| account.uuid == id)
            .map(|account| account.username.clone())
            .unwrap_or_else(|| "this account".into());
        let this = cx.weak_entity();
        confirm::danger(
            window,
            cx,
            "Remove account",
            format!("\"{name}\" will be signed out of kmine. Worlds stay on disk."),
            "Remove",
            move |_, cx| {
                this.update(cx, |this, cx| this.delete_account(id, cx)).ok();
            },
        );
    }

    fn delete_account(&mut self, id: AccountId, cx: &mut Context<Self>) {
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = rt
                .spawn(async move { engine.delete_account(id).await })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(())) => {
                        this.refresh_accounts();
                        this.accounts.error = None;
                        this.ensure_skin(cx);
                    }
                    Ok(Err(err)) => this.accounts.error = Some(err.to_string()),
                    Err(err) => this.accounts.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn play_or_stop(
        &mut self,
        id: InstanceId,
        quick_play: Option<QuickPlay>,
        cx: &mut Context<Self>,
    ) {
        let Some(instance) = self.instances.iter().find(|i| i.id == id).cloned() else {
            return;
        };
        if instance.running {
            if quick_play.is_some() {
                return;
            }
            let _ = self.engine.kill(id);
            return;
        }
        if self.progress.is_some() {
            return;
        }

        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        self.progress = Some(ProgressModal {
            id,
            name: instance.name.clone(),
            title: "Preparing…".into(),
            done: 0,
            total: 0,
        });
        self.status.clear();
        cx.notify();

        let engine = self.engine.clone();
        let rt = self.rt.clone();
        let name = instance.name.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let prepared = rt
                .spawn(async move {
                    let sink = EventProgressSink::new(engine.event_sender(), id);
                    let plan = engine.prepare(id, &sink, cancel, quick_play).await?;
                    engine.spawn(id, plan)
                })
                .await;

            match prepared {
                Ok(Ok(_)) => {
                    this.update(cx, |this, cx| {
                        this.progress = None;
                        this.cancel = None;
                        this.status.clear();
                        this.refresh_instances();
                        this.open_game_output(id, name, cx);
                        cx.notify();
                    })
                    .ok();
                }
                Ok(Err(err)) => {
                    this.update(cx, |this, cx| {
                        this.progress = None;
                        this.cancel = None;
                        match err {
                            EngineError::NoAccount => {
                                this.refresh_accounts();
                                this.show_accounts = true;
                                this.status = EngineError::NoAccount.to_string();
                            }
                            EngineError::Cancelled => this.status.clear(),
                            other => this.status = other.to_string(),
                        }
                        cx.notify();
                    })
                    .ok();
                }
                Err(err) => {
                    this.update(cx, |this, cx| {
                        this.progress = None;
                        this.cancel = None;
                        this.status = err.to_string();
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    fn cancel_prepare(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
        cx.notify();
    }

    fn open_game_output(&self, id: InstanceId, name: String, cx: &mut Context<Self>) {
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(860.), px(520.)), cx)),
            titlebar: Some(TitlebarOptions {
                title: Some(format!("{name} — output").into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let _ = cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| crate::game_output::GameOutput::new(engine, rt, id, name, cx));
            cx.new(|cx| Root::new(view, window, cx))
        });
    }
}

impl Render for KmineApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        let selected = self.selected_instance().cloned();
        let identity = self.accounts.identity_label().to_string();
        let status = self.status.clone();
        let pane = self.instance_pane;
        let sandbox_status = self.engine.sandbox_status();

        div()
            .size_full()
            .relative()
            .when(
                self.show_create || self.show_accounts || self.catalog.is_some(),
                |el| {
                    el.key_context("Modal").on_action({
                        let this = this.clone();
                        move |_: &Cancel, _, cx| {
                            this.update(cx, |this, cx| {
                                if this.catalog.is_some() {
                                    this.close_catalog(cx);
                                } else if this.show_create && this.status != "Creating…" {
                                    this.close_create(cx);
                                } else if this.show_accounts && !this.accounts.busy {
                                    this.close_accounts(cx);
                                }
                            })
                            .ok();
                        }
                    })
                },
            )
            .when(!crate::sidebar_is_glass(), |this| {
                this.bg(cx.theme().background)
            })
            .text_color(cx.theme().foreground)
            .child(
                v_flex().size_full().child(
                    h_flex()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .child(instances::sidebar(
                            &self.instances,
                            self.selected,
                            &identity,
                            self.skin_face.as_deref(),
                            {
                                let this = this.clone();
                                move |id, window, cx| {
                                    this.update(cx, |this, cx| {
                                        this.select_instance(id, window, cx);
                                    })
                                    .ok();
                                }
                            },
                            {
                                let this = this.clone();
                                move |_, window, cx| {
                                    this.update(cx, |this, cx| this.open_create(window, cx))
                                        .ok();
                                }
                            },
                            {
                                let this = this.clone();
                                move |id, _, window, cx| {
                                    this.update(cx, |this, cx| {
                                        this.open_rename(id, window, cx);
                                    })
                                    .ok();
                                }
                            },
                            {
                                let this = this.clone();
                                move |_, _, cx| {
                                    this.update(cx, |this, cx| this.submit_rename(cx)).ok();
                                }
                            },
                            {
                                let this = this.clone();
                                move |id, _, window, cx| {
                                    this.update(cx, |this, cx| this.open_delete(id, window, cx))
                                        .ok();
                                }
                            },
                            {
                                let this = this.clone();
                                move |id, _, _, cx| {
                                    this.update(cx, |this, cx| this.toggle_pin(id, cx)).ok();
                                }
                            },
                            {
                                let this = this.clone();
                                move |_, _, cx| {
                                    this.update(cx, |this, cx| this.open_accounts(cx)).ok();
                                }
                            },
                            self.rename.as_ref(),
                            &self.pinned,
                            cx,
                        ))
                        .child(
                            v_flex()
                                .flex_1()
                                .h_full()
                                .min_w_0()
                                .bg(cx.theme().background)
                                .border_l_1()
                                .border_color(cx.theme().border)
                                .overflow_hidden()
                                .child(match selected {
                                    Some(instance) => right_pane(
                                        &instance,
                                        pane,
                                        &status,
                                        &self.content_mods,
                                        &self.content_resourcepacks,
                                        &self.content_shaderpacks,
                                        &self.quick_play,
                                        self.settings.as_ref(),
                                        &sandbox_status,
                                        self.progress.as_ref().is_some_and(|p| p.id == instance.id),
                                        self.progress.is_none(),
                                        this.clone(),
                                        cx,
                                    )
                                    .into_any_element(),
                                    None => empty_state(
                                        {
                                            let this = this.clone();
                                            move |_, window, cx| {
                                                this.update(cx, |this, cx| {
                                                    this.open_create(window, cx)
                                                })
                                                .ok();
                                            }
                                        },
                                        cx,
                                    )
                                    .into_any_element(),
                                }),
                        ),
                ),
            )
            .when_some(self.progress.as_ref(), |el, modal| {
                el.child(progress::render(
                    modal,
                    {
                        let this = this.clone();
                        move |_, _, cx| {
                            this.update(cx, |this, cx| {
                                this.cancel_prepare(cx);
                            })
                            .ok();
                        }
                    },
                    cx,
                ))
            })
            .when_some(
                self.create.as_ref().filter(|_| self.show_create),
                |el, form| {
                    el.child(create_instance::render(
                        form,
                        &self.status,
                        {
                            let this = this.clone();
                            move |loader, window, cx| {
                                this.update(cx, |this, cx| {
                                    this.set_create_kind(loader, window, cx)
                                })
                                .ok();
                            }
                        },
                        {
                            let this = this.clone();
                            move |_, window, cx| {
                                this.update(cx, |this, cx| this.open_modpack_catalog(window, cx))
                                    .ok();
                            }
                        },
                        {
                            let this = this.clone();
                            move |_, _, cx| {
                                this.update(cx, |this, cx| this.create_back(cx)).ok();
                            }
                        },
                        {
                            let this = this.clone();
                            move |_, _, cx| {
                                this.update(cx, |this, cx| this.submit_create(cx)).ok();
                            }
                        },
                        {
                            let this = this.clone();
                            move |_, _, cx| {
                                this.update(cx, |this, cx| this.close_create(cx)).ok();
                            }
                        },
                        cx,
                    ))
                },
            )
            .when_some(self.catalog.as_ref(), |el, modal| {
                el.child(catalog::render(
                    modal,
                    {
                        let this = this.clone();
                        move |_, _, cx| {
                            this.update(cx, |this, cx| this.close_catalog(cx)).ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |id, _, _, cx| {
                            this.update(cx, |this, cx| this.toggle_catalog_category(id, cx))
                                .ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |sort, _, _, cx| {
                            this.update(cx, |this, cx| this.set_catalog_sort(sort, cx))
                                .ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |_, _, cx| {
                            this.update(cx, |this, cx| this.catalog_load_more(cx)).ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |id, _, _, cx| {
                            this.update(cx, |this, cx| this.open_catalog_project(id, cx))
                                .ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |_, _, cx| {
                            this.update(cx, |this, cx| this.catalog_back(cx)).ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |id, _, _, cx| {
                            this.update(cx, |this, cx| this.select_catalog_file(id, cx))
                                .ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |_, _, cx| {
                            this.update(cx, |this, cx| this.catalog_load_more_files(cx))
                                .ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |_, _, cx| {
                            this.update(cx, |this, cx| this.install_catalog(cx)).ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |url, _, _, _cx| {
                            this.update(_cx, |this, _| this.open_catalog_website(url))
                                .ok();
                        }
                    },
                    cx,
                ))
            })
            .when(self.show_accounts, |el| {
                el.child(accounts::render(
                    &self.accounts,
                    |id| self.engine.cached_skin_face(id),
                    {
                        let this = this.clone();
                        move |id, _, cx| {
                            this.update(cx, |this, cx| this.select_account(id, cx)).ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |id, window, cx| {
                            this.update(cx, |this, cx| {
                                this.confirm_delete_account(id, window, cx);
                            })
                            .ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |_, _, cx| {
                            this.update(cx, |this, cx| this.start_login(cx)).ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |_, _, cx| {
                            this.update(cx, |this, cx| this.close_accounts(cx)).ok();
                        }
                    },
                    cx,
                ))
            })
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

fn right_pane(
    instance: &InstanceSummary,
    pane: InstancePane,
    status: &str,
    mods: &[ContentEntry],
    resourcepacks: &[ContentEntry],
    shaderpacks: &[ContentEntry],
    quick_play: &QuickPlayLists,
    settings: Option<&instance_settings::SettingsForm>,
    sandbox_status: &SandboxStatus,
    preparing: bool,
    add_enabled: bool,
    this: WeakEntity<KmineApp>,
    cx: &App,
) -> impl IntoElement {
    let id = instance.id;
    v_flex()
        .size_full()
        .min_w_0()
        .pt(px(36.))
        .px_8()
        .pb_8()
        .id("instance-main")
        .overflow_y_scroll()
        .child(
            v_flex()
                .w_full()
                .gap_5()
                .child(instance_header(instance, cx))
                .child(pane_switcher(pane, this.clone(), cx))
                .when(
                    !status.is_empty() && pane != InstancePane::Settings,
                    |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(status.to_string()),
                        )
                    },
                )
                .child(match pane {
                    InstancePane::Play => instance_play::play_tab(
                        instance,
                        quick_play,
                        preparing,
                        {
                            let this = this.clone();
                            move |_, _, cx| {
                                this.update(cx, |this, cx| {
                                    this.play_or_stop(id, None, cx);
                                })
                                .ok();
                            }
                        },
                        {
                            let this = this.clone();
                            move |target, _, _, cx| {
                                this.update(cx, |this, cx| {
                                    this.play_or_stop(id, Some(target), cx);
                                })
                                .ok();
                            }
                        },
                        cx,
                    )
                    .into_any_element(),
                    InstancePane::Content => instance_content::content_tab(
                        mods,
                        resourcepacks,
                        shaderpacks,
                        instance.loader,
                        add_enabled,
                        {
                            let this = this.clone();
                            move |path, enabled, _, cx| {
                                this.update(cx, |this, cx| {
                                    this.toggle_content(path, enabled, cx);
                                })
                                .ok();
                            }
                        },
                        {
                            let this = this.clone();
                            move |path, _, window, cx| {
                                this.update(cx, |this, cx| {
                                    this.confirm_delete_content(path, window, cx);
                                })
                                .ok();
                            }
                        },
                        {
                            let this = this.clone();
                            move |class, _, window, cx| {
                                this.update(cx, |this, cx| {
                                    this.open_content_catalog(class, window, cx);
                                })
                                .ok();
                            }
                        },
                        cx,
                    )
                    .into_any_element(),
                    InstancePane::Settings => match settings {
                        Some(form) => instance_settings::settings_tab(
                            form,
                            sandbox_status,
                            status,
                            {
                                let this = this.clone();
                                move |enabled, _, cx| {
                                    this.update(cx, |this, cx| {
                                        this.set_sandbox(enabled, cx);
                                    })
                                    .ok();
                                }
                            },
                            {
                                let this = this.clone();
                                move |_, _, cx| {
                                    this.update(cx, |this, cx| {
                                        this.save_settings(cx);
                                    })
                                    .ok();
                                }
                            },
                            cx,
                        )
                        .into_any_element(),
                        None => div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("Loading settings…")
                            .into_any_element(),
                    },
                }),
        )
}

fn instance_header(instance: &InstanceSummary, cx: &App) -> impl IntoElement {
    v_flex()
        .w_full()
        .gap_2()
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .min_w_0()
                        .text_xl()
                        .font_weight(FontWeight::MEDIUM)
                        .text_ellipsis()
                        .child(instance.name.clone()),
                )
                .when(instance.running, |this| {
                    this.child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .h(px(22.))
                            .rounded(px(6.))
                            .bg(cx.theme().success.opacity(0.18))
                            .child(div().size(px(6.)).rounded_full().bg(cx.theme().success))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().success)
                                    .child("Running"),
                            ),
                    )
                }),
        )
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(chip(instance.minecraft_version.clone(), cx))
                .child(chip(loader_label(instance.loader), cx)),
        )
}

fn pane_switcher(pane: InstancePane, this: WeakEntity<KmineApp>, cx: &App) -> impl IntoElement {
    h_flex()
        .id("instance-pane-switcher")
        .flex_shrink_0()
        .gap_1()
        .p(px(3.))
        .rounded(px(10.))
        .bg(cx.theme().muted)
        .child(pane_tab(
            "pane-play",
            "Play",
            pane == InstancePane::Play,
            {
                let this = this.clone();
                move |_, window, cx| {
                    this.update(cx, |this, cx| {
                        this.set_pane(InstancePane::Play, window, cx);
                    })
                    .ok();
                }
            },
            cx,
        ))
        .child(pane_tab(
            "pane-content",
            "Content",
            pane == InstancePane::Content,
            {
                let this = this.clone();
                move |_, window, cx| {
                    this.update(cx, |this, cx| {
                        this.set_pane(InstancePane::Content, window, cx);
                    })
                    .ok();
                }
            },
            cx,
        ))
        .child(pane_tab(
            "pane-settings",
            "Settings",
            pane == InstancePane::Settings,
            {
                let this = this.clone();
                move |_, window, cx| {
                    this.update(cx, |this, cx| {
                        this.set_pane(InstancePane::Settings, window, cx);
                    })
                    .ok();
                }
            },
            cx,
        ))
}

fn pane_tab(
    id: &'static str,
    label: &'static str,
    active: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let (bg, fg) = if active {
        (cx.theme().primary, cx.theme().primary_foreground)
    } else {
        (cx.theme().transparent, cx.theme().muted_foreground)
    };
    h_flex()
        .id(id)
        .h(px(28.))
        .px_3()
        .items_center()
        .rounded(px(8.))
        .bg(bg)
        .text_color(fg)
        .cursor_pointer()
        .when(!active, |this| {
            this.hover(|this| this.text_color(cx.theme().foreground))
        })
        .on_click(on_click)
        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
}

enum InstallOutcome {
    Pack(InstanceId),
    Content,
}

fn stub_project(project: CatalogProject) -> CatalogProjectDetail {
    CatalogProjectDetail {
        project,
        description_html: String::new(),
        screenshot_urls: Vec::new(),
        website_url: None,
    }
}

fn empty_state(
    on_create: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    use gpui_component::button::{Button, ButtonVariants};

    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .child(
            div()
                .text_lg()
                .font_weight(FontWeight::MEDIUM)
                .child("No instance selected"),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Create one to download, launch, and keep local mods."),
        )
        .child(
            Button::new("empty-create")
                .primary()
                .label("New instance")
                .on_click(on_create),
        )
}
