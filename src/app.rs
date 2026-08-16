use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnimationExt, App, Context, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, TitlebarOptions, WeakEntity, Window,
    WindowBounds, WindowOptions, div, point, px, relative, size, transparent_black,
};
use gpui_component::{
    ActiveTheme, IconName, Root,
    dialog::Cancel,
    h_flex,
    input::{InputEvent, InputState},
    select::SelectEvent,
    slider::SliderEvent,
    v_flex,
};

use crate::chrome::{
    empty_panel, filled_segment, is_success_status, motion, status_alert, FILES_VERIFIED,
};
use crate::providers::CurseForgeProvider;
use kmine_engine::{
    AccountId, CancellationToken, CatalogError, CatalogFileFilter, CatalogProject,
    CatalogProjectDetail, CatalogProjectId, CatalogQuery, CatalogSort, ContentClass, ContentEntry,
    ContentFolder, Engine, EngineError, Event, InstanceId, InstanceSummary, Loader, PrepareMode,
    QuickPlay, QuickPlayLists, SandboxStatus,
};

use crate::modals::accounts::{self, AccountsModal};
use crate::modals::catalog::{self, CatalogModal, CatalogTarget};
use crate::modals::confirm;
use crate::modals::create_instance::{self, CreateInstanceForm};
use crate::modals::progress::{self, EventProgressSink, ProgressModal, VERIFY_HEADING};
use crate::modals::settings;
use crate::screens::{instance_content, instance_play, instance_settings, instances};
use crate::smooth_scroll::SmoothScroll;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum InstancePane {
    #[default]
    Play,
    Content,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContentAnim {
    Instance,
    Tab,
}

pub struct KmineApp {
    engine: Arc<Engine>,
    rt: tokio::runtime::Handle,
    instances: Vec<InstanceSummary>,
    selected: Option<InstanceId>,
    show_create: bool,
    show_accounts: bool,
    show_settings: bool,
    status: String,
    status_for: Option<InstanceId>,
    status_epoch: u64,
    create: Option<CreateInstanceForm>,
    catalog: Option<CatalogModal>,
    search_gen: u64,
    detail_gen: u64,
    accounts: AccountsModal,
    progress: Option<ProgressModal>,
    cancel: Option<CancellationToken>,
    rename: Option<instances::RenameForm>,
    instance_pane: InstancePane,
    pane_from: InstancePane,
    content_anim: ContentAnim,
    content_mods: Vec<ContentEntry>,
    content_resourcepacks: Vec<ContentEntry>,
    content_shaderpacks: Vec<ContentEntry>,
    quick_play: QuickPlayLists,
    settings: Option<instance_settings::SettingsForm>,
    settings_saving: bool,
    settings_dirty: bool,
    skin_face: Option<PathBuf>,
    skin_for: Option<AccountId>,
    pinned: HashSet<InstanceId>,
    sidebar_scroll: SmoothScroll,
    play_scroll: SmoothScroll,
    content_scroll: SmoothScroll,
    settings_scroll: SmoothScroll,
    accounts_scroll: SmoothScroll,
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
            show_settings: false,
            status: String::new(),
            status_for: None,
            status_epoch: 0,
            create: None,
            catalog: None,
            search_gen: 0,
            detail_gen: 0,
            accounts,
            progress: None,
            cancel: None,
            rename: None,
            instance_pane: InstancePane::Play,
            pane_from: InstancePane::Play,
            content_anim: ContentAnim::Instance,
            content_mods: Vec::new(),
            content_resourcepacks: Vec::new(),
            content_shaderpacks: Vec::new(),
            quick_play: QuickPlayLists::default(),
            settings: None,
            settings_saving: false,
            settings_dirty: false,
            skin_face: None,
            skin_for: None,
            pinned: HashSet::new(),
            sidebar_scroll: SmoothScroll::new(),
            play_scroll: SmoothScroll::new(),
            content_scroll: SmoothScroll::new(),
            settings_scroll: SmoothScroll::new(),
            accounts_scroll: SmoothScroll::new(),
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

    fn clear_status(&mut self) {
        self.status.clear();
        self.status_for = None;
        self.status_epoch = self.status_epoch.wrapping_add(1);
    }

    fn dismiss_status(&mut self, cx: &mut Context<Self>) {
        self.clear_status();
        cx.notify();
    }

    fn arm_success_status_timeout(&mut self, cx: &mut Context<Self>) {
        if !is_success_status(&self.status) {
            return;
        }
        self.status_epoch = self.status_epoch.wrapping_add(1);
        let epoch = self.status_epoch;
        let rt = self.rt.clone();
        cx.spawn(async move |this, cx| {
            rt.spawn(async { tokio::time::sleep(Duration::from_secs(4)).await })
                .await
                .ok();
            this.update(cx, |this, cx| {
                if this.status_epoch == epoch {
                    this.clear_status();
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.status_for = None;
    }

    fn set_instance_status(&mut self, id: InstanceId, message: impl Into<String>) {
        self.status = message.into();
        self.status_for = Some(id);
    }

    fn visible_status(&self) -> &str {
        match self.status_for {
            Some(id) if self.selected != Some(id) => "",
            _ => self.status.as_str(),
        }
    }

    fn select_instance(&mut self, id: InstanceId, _window: &mut Window, cx: &mut Context<Self>) {
        if self.selected == Some(id) {
            return;
        }
        if is_success_status(&self.status) {
            self.clear_status();
        }
        self.pane_from = self.instance_pane;
        self.instance_pane = InstancePane::Play;
        self.content_anim = ContentAnim::Instance;
        self.selected = Some(id);
        self.reload_content();
        self.reload_quick_play();
        cx.notify();
    }

    fn set_pane(&mut self, pane: InstancePane, window: &mut Window, cx: &mut Context<Self>) {
        if pane == self.instance_pane {
            return;
        }
        self.pane_from = self.instance_pane;
        self.instance_pane = pane;
        self.content_anim = ContentAnim::Tab;
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
                self.bind_settings(cx);
            }
            Ok(None) => {
                self.settings = None;
                self.set_instance_status(id, "instance not found");
            }
            Err(err) => self.set_instance_status(id, err.to_string()),
        }
    }

    fn bind_settings(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.settings.as_ref() else {
            return;
        };
        let memory_min = form.memory_min.clone();
        let memory_max = form.memory_max.clone();
        let jvm_flags = form.jvm_flags.clone();
        let java_path = form.java_path.clone();
        let account = form.account.clone();
        cx.subscribe(&memory_min, |this, _, event: &SliderEvent, cx| match event {
            SliderEvent::Change(_) => cx.notify(),
            SliderEvent::Release(_) => this.save_settings(cx),
        })
        .detach();
        cx.subscribe(&memory_max, |this, _, event: &SliderEvent, cx| match event {
            SliderEvent::Change(_) => cx.notify(),
            SliderEvent::Release(_) => this.save_settings(cx),
        })
        .detach();
        cx.subscribe(&jvm_flags, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change | InputEvent::Blur) {
                this.save_settings(cx);
            }
        })
        .detach();
        cx.subscribe(&java_path, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change | InputEvent::Blur) {
                this.save_settings(cx);
            }
        })
        .detach();
        cx.subscribe(&account, |this, _, event: &SelectEvent<Vec<String>>, cx| {
            if matches!(event, SelectEvent::Confirm(_)) {
                this.save_settings(cx);
            }
        })
        .detach();
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.settings.as_ref() else {
            return;
        };
        if self.settings_saving {
            self.settings_dirty = true;
            return;
        }
        let id = form.instance_id;
        let patch = form.patch(cx);
        self.settings_saving = true;
        self.settings_dirty = false;
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = rt
                .spawn(async move { engine.update_instance(id, patch).await })
                .await;
            this.update(cx, |this, cx| {
                this.settings_saving = false;
                match result {
                    Ok(Ok(())) => this.clear_status(),
                    Ok(Err(err)) => this.set_instance_status(id, err.to_string()),
                    Err(err) => this.set_instance_status(id, err.to_string()),
                }
                if this.settings_dirty {
                    this.settings_dirty = false;
                    this.save_settings(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_sandbox(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let Some(form) = self.settings.as_mut() else {
            return;
        };
        form.sandbox = enabled;
        self.save_settings(cx);
    }

    fn toggle_content(&mut self, path: PathBuf, enabled: bool, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        match self.engine.set_content_enabled(id, &path, enabled) {
            Ok(()) => {
                self.clear_status();
                self.reload_content();
            }
            Err(err) => self.set_instance_status(id, err.to_string()),
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
                self.clear_status();
                self.reload_content();
            }
            Err(err) => self.set_instance_status(id, err.to_string()),
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
                self.show_settings = false;
                self.show_accounts = true;
            }
            Event::ProcessExited { .. } => {
                self.refresh_instances();
                self.reload_quick_play();
            }
            Event::Error(message) => self.set_status(message),
            Event::LogLine { .. } => {}
        }
    }

    fn open_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.progress.is_some() {
            return;
        }
        self.close_catalog(cx);
        self.show_settings = false;
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
        self.clear_status();
        self.show_create = true;
        cx.notify();
    }

    fn set_create_kind(&mut self, loader: Loader, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(form) = self.create.as_mut() {
            form.phase = create_instance::CreatePhase::Loader(loader);
            form.name.update(cx, |state, cx| state.focus(window, cx));
            self.clear_status();
            cx.notify();
        }
    }

    fn create_back(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = self.create.as_mut() {
            form.phase = create_instance::CreatePhase::Kind;
            self.clear_status();
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
        self.clear_status();
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
                if this.search_gen != generation {
                    return;
                }
                let Some(catalog) = this.catalog.as_mut() else {
                    return;
                };
                match result {
                    Ok(Ok(categories)) => {
                        catalog.categories = categories;
                        catalog.no_key = false;
                        catalog.error = None;
                        this.run_catalog_search(false, cx);
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
            heading: format!("Preparing {name}"),
            title: "Installing…".into(),
            done: 0,
            total: 0,
        });
        self.clear_status();
        cx.notify();

        let engine = self.engine.clone();
        let rt = self.rt.clone();
        let events = engine.event_sender();
        let reload_content_on_fail = matches!(target, CatalogTarget::Instance(_));
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
                        this.clear_status();
                    }
                    Ok(Ok(InstallOutcome::Content)) => {
                        this.clear_status();
                        this.reload_content();
                    }
                    Ok(Err(EngineError::Cancelled)) => {
                        this.clear_status();
                        if reload_content_on_fail {
                            this.reload_content();
                        }
                    }
                    Ok(Err(err)) => {
                        this.set_status(err.to_string());
                        if reload_content_on_fail {
                            this.reload_content();
                        }
                    }
                    Err(err) => {
                        this.set_status(err.to_string());
                        if reload_content_on_fail {
                            this.reload_content();
                        }
                    }
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
        self.show_settings = false;
        self.refresh_accounts();
        self.show_accounts = true;
        cx.notify();
    }

    fn close_accounts(&mut self, cx: &mut Context<Self>) {
        self.show_accounts = false;
        self.engine.cancel_login();
        cx.notify();
    }

    fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.show_create = false;
        self.create = None;
        self.show_accounts = false;
        self.engine.cancel_login();
        self.show_settings = true;
        cx.notify();
    }

    fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.show_settings = false;
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
        self.set_status("Creating…");
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
                        this.clear_status();
                    }
                    Ok(Err(err)) => this.set_status(err.to_string()),
                    Err(err) => this.set_status(err.to_string()),
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
        self.clear_status();
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
        self.set_status("Renaming…");
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
                        this.clear_status();
                    }
                    Ok(Err(err)) => this.set_instance_status(id, err.to_string()),
                    Err(err) => this.set_instance_status(id, err.to_string()),
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
                        this.clear_status();
                    }
                    Ok(Err(err)) => this.set_status(err.to_string()),
                    Err(err) => this.set_status(err.to_string()),
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
            heading: format!("Preparing {}", instance.name),
            title: "Preparing…".into(),
            done: 0,
            total: 0,
        });
        self.clear_status();
        cx.notify();

        let engine = self.engine.clone();
        let rt = self.rt.clone();
        let name = instance.name.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let prepared = rt
                .spawn(async move {
                    let sink = EventProgressSink::new(engine.event_sender(), id);
                    let plan = engine
                        .prepare(id, &sink, cancel, quick_play, PrepareMode::Warm)
                        .await?;
                    engine.spawn(id, plan)
                })
                .await;

            match prepared {
                Ok(Ok(_)) => {
                    this.update(cx, |this, cx| {
                        this.progress = None;
                        this.cancel = None;
                        this.clear_status();
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
                                this.show_settings = false;
                                this.show_accounts = true;
                                this.set_status(EngineError::NoAccount.to_string());
                            }
                            EngineError::Cancelled => this.clear_status(),
                            other => this.set_instance_status(id, other.to_string()),
                        }
                        cx.notify();
                    })
                    .ok();
                }
                Err(err) => {
                    this.update(cx, |this, cx| {
                        this.progress = None;
                        this.cancel = None;
                        this.set_instance_status(id, err.to_string());
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    fn verify_files(&mut self, id: InstanceId, cx: &mut Context<Self>) {
        let Some(instance) = self.instances.iter().find(|i| i.id == id).cloned() else {
            return;
        };
        if instance.running || self.progress.is_some() {
            return;
        }
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        self.progress = Some(ProgressModal {
            id,
            heading: VERIFY_HEADING.into(),
            title: VERIFY_HEADING.into(),
            done: 0,
            total: 0,
        });
        self.clear_status();
        cx.notify();
        let engine = self.engine.clone();
        let rt = self.rt.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let prepared = rt
                .spawn(async move {
                    let sink = EventProgressSink::new(engine.event_sender(), id);
                    engine
                        .prepare(id, &sink, cancel, None, PrepareMode::Verify)
                        .await
                        .map(|_| ())
                })
                .await;
            match prepared {
                Ok(Ok(())) => {
                    this.update(cx, |this, cx| {
                        this.progress = None;
                        this.cancel = None;
                        this.set_instance_status(id, FILES_VERIFIED);
                        this.arm_success_status_timeout(cx);
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
                                this.show_settings = false;
                                this.show_accounts = true;
                                this.set_status(EngineError::NoAccount.to_string());
                            }
                            EngineError::Cancelled => this.clear_status(),
                            other => this.set_instance_status(id, other.to_string()),
                        }
                        cx.notify();
                    })
                    .ok();
                }
                Err(err) => {
                    this.update(cx, |this, cx| {
                        this.progress = None;
                        this.cancel = None;
                        this.set_instance_status(id, err.to_string());
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
        let glass = crate::sidebar_is_glass();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(860.), px(520.)), cx)),
            window_background: crate::window_background(),
            titlebar: Some(TitlebarOptions {
                title: Some(format!("{name} — output").into()),
                appears_transparent: cfg!(target_os = "macos"),
                traffic_light_position: Some(point(px(16.), px(18.))),
                ..Default::default()
            }),
            ..Default::default()
        };
        let engine_for_close = engine.clone();
        let opened = cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| {
                crate::game_output::GameOutput::new(engine, rt, id, name, window, cx)
            });
            cx.new(|cx| {
                let mut root = Root::new(view, window, cx);
                if glass {
                    root = root.bg(transparent_black());
                }
                root
            })
        });
        if let Ok(handle) = opened {
            handle
                .update(cx, |_, window, cx| {
                    window.on_window_should_close(cx, move |_, _| {
                        let _ = engine_for_close.kill(id);
                        true
                    });
                })
                .ok();
        }
    }
}

impl Render for KmineApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        let selected = self.selected_instance().cloned();
        let identity = self.accounts.identity_label().to_string();
        let status = self.visible_status().to_string();
        let pane = self.instance_pane;
        let sandbox_status = self.engine.sandbox_status();

        div()
            .size_full()
            .relative()
            .when(
                self.show_create
                    || self.show_accounts
                    || self.show_settings
                    || self.catalog.is_some(),
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
                                } else if this.show_settings {
                                    this.close_settings(cx);
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
                            {
                                let this = this.clone();
                                move |_, _, cx| {
                                    this.update(cx, |this, cx| this.open_settings(cx)).ok();
                                }
                            },
                            self.rename.as_ref(),
                            &self.pinned,
                            &self.sidebar_scroll,
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
                                        self.pane_from,
                                        self.content_anim,
                                        &status,
                                        &self.content_mods,
                                        &self.content_resourcepacks,
                                        &self.content_shaderpacks,
                                        &self.quick_play,
                                        self.settings.as_ref(),
                                        &sandbox_status,
                                        self.progress.as_ref().is_some_and(|p| p.id == instance.id),
                                        self.progress.as_ref().is_some_and(|p| {
                                            p.id == instance.id && p.heading == VERIFY_HEADING
                                        }),
                                        self.progress.is_none(),
                                        &self.play_scroll,
                                        &self.content_scroll,
                                        &self.settings_scroll,
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
            .when(self.show_settings, |el| {
                el.child(settings::render(
                    self.engine.library_dir(),
                    {
                        let this = this.clone();
                        move |_, _, cx| {
                            this.update(cx, |this, _| {
                                settings::reveal_library(this.engine.library_dir());
                            })
                            .ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |_, _, cx| {
                            this.update(cx, |this, cx| this.close_settings(cx)).ok();
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
                    &self.accounts_scroll,
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
    pane_from: InstancePane,
    content_anim: ContentAnim,
    status: &str,
    mods: &[ContentEntry],
    resourcepacks: &[ContentEntry],
    shaderpacks: &[ContentEntry],
    quick_play: &QuickPlayLists,
    settings: Option<&instance_settings::SettingsForm>,
    sandbox_status: &SandboxStatus,
    preparing: bool,
    verifying: bool,
    add_enabled: bool,
    play_scroll: &SmoothScroll,
    content_scroll: &SmoothScroll,
    settings_scroll: &SmoothScroll,
    this: WeakEntity<KmineApp>,
    cx: &App,
) -> impl IntoElement {
    let id = instance.id;
    v_flex()
        .size_full()
        .min_w_0()
        .pt(px(36.))
        .px_6()
        .pb_6()
        .id("instance-main")
        .child(
            v_flex()
                .id(SharedString::from(format!(
                    "instance-shell-{}",
                    instance.id.as_hyphenated()
                )))
                .size_full()
                .w_full()
                .gap_4()
                .with_animation(
                    SharedString::from(format!("instance-in-{}", instance.id.as_hyphenated())),
                    motion(),
                    |this, delta| this.opacity(delta).mt(px(8. * (1. - delta))),
                )
                .child(pane_switcher(pane_from, pane, this.clone(), cx))
                .child(instance_play::launch_hero(
                    instance,
                    preparing,
                    verifying,
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
                        move |_, _, cx| {
                            this.update(cx, |this, cx| {
                                this.verify_files(id, cx);
                            })
                            .ok();
                        }
                    },
                    cx,
                ))
                .child(tab_body(
                    instance,
                    pane,
                    content_anim,
                    status,
                    mods,
                    resourcepacks,
                    shaderpacks,
                    quick_play,
                    settings,
                    sandbox_status,
                    preparing,
                    add_enabled,
                    play_scroll,
                    content_scroll,
                    settings_scroll,
                    this.clone(),
                    cx,
                ))
        )
}

fn tab_body(
    instance: &InstanceSummary,
    pane: InstancePane,
    content_anim: ContentAnim,
    status: &str,
    mods: &[ContentEntry],
    resourcepacks: &[ContentEntry],
    shaderpacks: &[ContentEntry],
    quick_play: &QuickPlayLists,
    settings: Option<&instance_settings::SettingsForm>,
    sandbox_status: &SandboxStatus,
    preparing: bool,
    add_enabled: bool,
    play_scroll: &SmoothScroll,
    content_scroll: &SmoothScroll,
    settings_scroll: &SmoothScroll,
    this: WeakEntity<KmineApp>,
    cx: &App,
) -> gpui::AnyElement {
    let id = instance.id;
    let body = v_flex()
        .id(match pane {
            InstancePane::Play => "instance-pane-play",
            InstancePane::Content => "instance-pane-content",
            InstancePane::Settings => "instance-pane-settings",
        })
        .flex_1()
        .min_h_0()
        .w_full()
        .gap_4()
                .when(
                    !status.is_empty() && pane != InstancePane::Settings,
                    |el| {
                        el.child(status_alert(
                            status,
                            {
                                let this = this.clone();
                                move |_, _, cx| {
                                    this.update(cx, |this, cx| this.dismiss_status(cx)).ok();
                                }
                            },
                            cx,
                        ))
                    },
                )
                .child(match pane {
                    InstancePane::Play => instance_play::play_tab(
                        instance,
                        quick_play,
                        preparing,
                        {
                            let this = this.clone();
                            move |target, _, _, cx| {
                                this.update(cx, |this, cx| {
                                    this.play_or_stop(id, Some(target), cx);
                                })
                                .ok();
                            }
                        },
                        play_scroll,
                        cx,
                    )
                    .into_any_element(),
                    InstancePane::Content => content_scroll
                        .vertical(
                            v_flex()
                                .id("instance-content-scroll")
                                .flex_1()
                                .min_h_0()
                                .w_full(),
                        )
                        .child(instance_content::content_tab(
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
                        ))
                        .into_any_element(),
                    InstancePane::Settings => settings_scroll
                        .vertical(
                            v_flex()
                                .id("instance-settings-scroll")
                                .flex_1()
                                .min_h_0()
                                .w_full(),
                        )
                        .child(match settings {
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
                                        this.update(cx, |this, cx| this.dismiss_status(cx)).ok();
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
                        })
                        .into_any_element(),
                });
    if content_anim == ContentAnim::Tab {
        body.with_animation(
            match pane {
                InstancePane::Play => "tab-in-play",
                InstancePane::Content => "tab-in-content",
                InstancePane::Settings => "tab-in-settings",
            },
            motion(),
            |this, delta| this.opacity(delta).mt(px(8. * (1. - delta))),
        )
        .into_any_element()
    } else {
        body.into_any_element()
    }
}

fn pane_slot(pane: InstancePane) -> f32 {
    match pane {
        InstancePane::Play => 0.0,
        InstancePane::Content => 1.0 / 3.0,
        InstancePane::Settings => 2.0 / 3.0,
    }
}

fn pane_switcher(
    from: InstancePane,
    pane: InstancePane,
    this: WeakEntity<KmineApp>,
    cx: &App,
) -> impl IntoElement {
    let start = pane_slot(from);
    let end = pane_slot(pane);
    h_flex()
        .id("instance-pane-switcher")
        .relative()
        .w_full()
        .flex_shrink_0()
        .p(px(3.))
        .rounded(px(10.))
        .bg(cx.theme().muted)
        .child(
            div()
                .absolute()
                .top(px(3.))
                .right(px(3.))
                .bottom(px(3.))
                .left(px(3.))
                .child(pane_thumb(from, pane, start, end, cx)),
        )
        .children([
            filled_segment(
                "pane-play",
                "Play",
                None,
                pane == InstancePane::Play,
                false,
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
            )
            .into_any_element(),
            filled_segment(
                "pane-content",
                "Content",
                None,
                pane == InstancePane::Content,
                false,
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
            )
            .into_any_element(),
            filled_segment(
                "pane-settings",
                "Settings",
                None,
                pane == InstancePane::Settings,
                false,
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
            )
            .into_any_element(),
        ])
}

fn pane_thumb(
    from: InstancePane,
    pane: InstancePane,
    start: f32,
    end: f32,
    cx: &App,
) -> impl IntoElement {
    let pill = div()
        .absolute()
        .top_0()
        .bottom_0()
        .w(relative(1.0 / 3.0))
        .rounded(px(8.))
        .bg(cx.theme().secondary_hover);
    if from == pane {
        return pill.left(relative(end)).into_any_element();
    }
    pill.id("instance-pane-thumb")
        .with_animation(
            match (from, pane) {
                (InstancePane::Play, InstancePane::Content) => "thumb-play-content",
                (InstancePane::Play, InstancePane::Settings) => "thumb-play-settings",
                (InstancePane::Content, InstancePane::Play) => "thumb-content-play",
                (InstancePane::Content, InstancePane::Settings) => "thumb-content-settings",
                (InstancePane::Settings, InstancePane::Play) => "thumb-settings-play",
                (InstancePane::Settings, InstancePane::Content) => "thumb-settings-content",
                _ => "thumb-idle",
            },
            motion(),
            move |this, delta| this.left(relative(start + (end - start) * delta)),
        )
        .into_any_element()
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
    v_flex().size_full().items_center().justify_center().child(
        v_flex()
            .w(px(320.))
            .items_center()
            .gap_2()
            .child(empty_panel(
                IconName::Plus,
                "Pick an instance",
                "Or create one to download the game, launch, and keep local mods.",
                cx,
            ))
            .child(
                crate::chrome::cta("empty-create")
                    .label("New instance")
                    .on_click(on_create),
            ),
    )
}
