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
    AccountId, CancellationToken, Engine, EngineError, Event, InstanceId, InstanceSummary,
};

use crate::modals::accounts::{self, AccountsModal};
use crate::modals::create_instance::{self, CreateInstanceForm};
use crate::modals::progress::{self, EventProgressSink, ProgressModal};
use crate::screens::{instance_play, instances};

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
            Event::InstancesChanged => self.refresh_instances(),
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
            Event::ProcessExited { .. } => self.refresh_instances(),
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

    fn play_or_stop(&mut self, id: InstanceId, cx: &mut Context<Self>) {
        let Some(instance) = self.instances.iter().find(|i| i.id == id).cloned() else {
            return;
        };
        if instance.running {
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
                    let plan = engine.prepare(id, &sink, cancel, None).await?;
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
        let this = cx.weak_entity();
        let selected = self.selected_instance().cloned();
        let identity = self.accounts.identity_label().to_string();
        let status = self.status.clone();

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
                                    move |id, _, cx| {
                                        this.update(cx, |this, cx| {
                                            this.selected = Some(id);
                                            cx.notify();
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
                                        Some(instance) => {
                                            let id = instance.id;
                                            let this = this.clone();
                                            instance_play::play_tab(
                                                &instance,
                                                &status,
                                                move |_, _, cx| {
                                                    this.update(cx, |this, cx| {
                                                        this.play_or_stop(id, cx);
                                                    })
                                                    .ok();
                                                },
                                                cx,
                                            )
                                            .into_any_element()
                                        }
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
