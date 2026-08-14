use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    App, Context, IntoElement, ParentElement, Render, Styled, TitlebarOptions, WeakEntity, Window,
    WindowBounds, WindowOptions, div, px, size,
};
use gpui_component::{
    ActiveTheme, Root,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use kmine_engine::{
    AccountId, CancellationToken, ContentEntry, ContentFolder, Engine, EngineError, Event,
    InstanceId, InstancePatch, InstanceSummary, QuickPlay, QuickPlayLists, SandboxStatus,
};

use crate::modals::accounts::{self, AccountsModal};
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
    accounts: AccountsModal,
    progress: Option<ProgressModal>,
    cancel: Option<CancellationToken>,
    instance_pane: InstancePane,
    content_mods: Vec<ContentEntry>,
    content_resourcepacks: Vec<ContentEntry>,
    content_shaderpacks: Vec<ContentEntry>,
    quick_play: QuickPlayLists,
    settings: Option<instance_settings::SettingsForm>,
}

impl KmineApp {
    pub fn new(engine: Arc<Engine>, cx: &mut Context<Self>) -> Self {
        let accounts = AccountsModal::from_engine(&engine);
        let instances = engine.list_instances().unwrap_or_default();
        let this = Self {
            engine,
            rt: tokio::runtime::Handle::current(),
            instances,
            selected: None,
            show_create: false,
            show_accounts: false,
            status: String::new(),
            create: None,
            accounts,
            progress: None,
            cancel: None,
            instance_pane: InstancePane::Play,
            content_mods: Vec::new(),
            content_resourcepacks: Vec::new(),
            content_shaderpacks: Vec::new(),
            quick_play: QuickPlayLists::default(),
            settings: None,
        };
        this.listen_engine_events(cx);
        this
    }

    fn refresh_instances(&mut self) {
        self.instances = self.engine.list_instances().unwrap_or_default();
    }

    fn refresh_accounts(&mut self) {
        self.accounts.refresh(&self.engine);
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
        self.create = Some(CreateInstanceForm::new(window, cx));
        self.status.clear();
        self.show_create = true;
        cx.notify();
    }

    fn close_create(&mut self, cx: &mut Context<Self>) {
        self.show_create = false;
        self.create = None;
        cx.notify();
    }

    fn open_accounts(&mut self, cx: &mut Context<Self>) {
        self.refresh_accounts();
        self.show_accounts = true;
        cx.notify();
    }

    fn close_accounts(&mut self, cx: &mut Context<Self>) {
        self.show_accounts = false;
        cx.notify();
    }

    fn submit_create(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.create.as_ref() else {
            return;
        };
        if self.status == "Creating…" {
            return;
        }
        let spec = form.spec(cx);
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
                    }
                    Ok(Err(EngineError::AuthNotConfigured)) => {
                        this.accounts.error = Some(accounts::AUTH_NOT_CONFIGURED_HINT.into());
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
        if self.progress.as_ref().is_some_and(|p| p.id == id) {
            return;
        }

        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        self.progress = Some(ProgressModal {
            id,
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
            window_bounds: Some(WindowBounds::centered(size(px(720.), px(420.)), cx)),
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
        if self.selected.is_some() {
            match self.instance_pane {
                InstancePane::Play => self.reload_quick_play(),
                InstancePane::Content => self.reload_content(),
                InstancePane::Settings => {}
            }
        }
        let this = cx.weak_entity();
        let selected = self.selected_instance().cloned();
        let identity = self.accounts.identity_label().to_string();
        let status = self.status.clone();
        let pane = self.instance_pane;
        let sandbox_status = self.engine.sandbox_status();

        div()
            .size_full()
            .relative()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                v_flex()
                    .size_full()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .child(instances::sidebar(
                                &self.instances,
                                self.selected,
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
                                cx,
                            ))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .h_full()
                                    .min_w_0()
                                    .bg(cx.theme().background)
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
                                            this.clone(),
                                            cx,
                                        )
                                        .into_any_element(),
                                        None => empty_state(cx).into_any_element(),
                                    }),
                            ),
                    )
                    .child(identity_footer(
                        identity,
                        {
                            let this = this.clone();
                            move |_, _, cx| {
                                this.update(cx, |this, cx| this.open_accounts(cx)).ok();
                            }
                        },
                        cx,
                    )),
            )
            .when_some(
                self.create.as_ref().filter(|_| self.show_create),
                |el, form| {
                    el.child(create_instance::render(
                        form,
                        &self.status,
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
            .when(self.show_accounts, |el| {
                el.child(accounts::render(
                    &self.accounts,
                    {
                        let this = this.clone();
                        move |id, _, cx| {
                            this.update(cx, |this, cx| this.select_account(id, cx)).ok();
                        }
                    },
                    {
                        let this = this.clone();
                        move |id, _, cx| {
                            this.update(cx, |this, cx| this.delete_account(id, cx)).ok();
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
            .when_some(self.progress.as_ref(), |el, modal| {
                el.child(progress::render(
                    modal,
                    {
                        let this = this.clone();
                        move |_, _, cx| {
                            this.update(cx, |this, cx| this.cancel_prepare(cx)).ok();
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
    this: WeakEntity<KmineApp>,
    cx: &App,
) -> impl IntoElement {
    let id = instance.id;
    v_flex()
        .size_full()
        .min_w_0()
        .child(pane_switcher(pane, this.clone(), cx))
        .child(match pane {
            InstancePane::Play => {
                let this = this.clone();
                instance_play::play_tab(
                    instance,
                    status,
                    quick_play,
                    {
                        let this = this.clone();
                        move |_, _, cx| {
                            this.update(cx, |this, cx| {
                                this.play_or_stop(id, None, cx);
                            })
                            .ok();
                        }
                    },
                    move |quick, _, _, cx| {
                        this.update(cx, |this, cx| {
                            this.play_or_stop(id, Some(quick), cx);
                        })
                        .ok();
                    },
                    cx,
                )
                .into_any_element()
            }
            InstancePane::Content => instance_content::content_tab(
                mods,
                resourcepacks,
                shaderpacks,
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
                    move |path, _, _, cx| {
                        this.update(cx, |this, cx| {
                            this.delete_content(path, cx);
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
                None => empty_state(cx).into_any_element(),
            },
        })
}

fn pane_switcher(pane: InstancePane, this: WeakEntity<KmineApp>, cx: &App) -> impl IntoElement {
    h_flex()
        .id("instance-pane-switcher")
        .w_full()
        .px_4()
        .pt_3()
        .gap_1()
        .flex_shrink_0()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(pane_button(
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
        ))
        .child(pane_button(
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
        ))
        .child(pane_button(
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
        ))
}

fn pane_button(
    id: &'static str,
    label: &'static str,
    active: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let button = Button::new(id).label(label).on_click(on_click);
    if active {
        button.primary()
    } else {
        button.ghost()
    }
}

fn identity_footer(
    label: String,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .id("identity-footer")
        .w_full()
        .h(px(40.))
        .px_2()
        .items_center()
        .flex_shrink_0()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            Button::new("accounts-identity")
                .ghost()
                .label(label)
                .on_click(on_click),
        )
}

fn empty_state(cx: &App) -> impl IntoElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child("Select an instance")
}
