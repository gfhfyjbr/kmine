use std::sync::Arc;

use gpui::prelude::*;
use gpui::{App, Context, IntoElement, ParentElement, Render, Styled, WeakEntity, Window, div, px};
use gpui_component::{
    ActiveTheme, Root,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use kmine_engine::{AccountId, Engine, EngineError, Event, InstanceId, InstanceSummary};

use crate::modals::accounts::{self, AccountsModal};
use crate::modals::create_instance::{self, CreateInstanceForm};
use crate::screens::{instance_play::PlayTab, instances};

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
                    Ok(Event::AccountsChanged) => {
                        if this
                            .update(cx, |this, cx| {
                                this.refresh_accounts();
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                }
            }
        })
        .detach();
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
}

impl Render for KmineApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        let selected = self.selected_instance().cloned();
        let identity = self.accounts.identity_label().to_string();

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
                                            PlayTab::new(&instance).into_any_element()
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
