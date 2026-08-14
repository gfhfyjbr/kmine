use std::sync::Arc;

use gpui::prelude::*;
use gpui::{App, Context, IntoElement, ParentElement, Render, Styled, WeakEntity, Window, div};
use gpui_component::{ActiveTheme, Root, h_flex, v_flex};
use kmine_engine::{Engine, InstanceId, InstanceSummary};

use crate::modals::create_instance::{self, CreateInstanceForm};
use crate::screens::{instance_play::PlayTab, instances};

pub struct KmineApp {
    engine: Arc<Engine>,
    rt: tokio::runtime::Handle,
    instances: Vec<InstanceSummary>,
    selected: Option<InstanceId>,
    show_create: bool,
    status: String,
    create: Option<CreateInstanceForm>,
}

impl KmineApp {
    pub fn new(engine: Arc<Engine>, _cx: &mut Context<Self>) -> Self {
        let instances = engine.list_instances().unwrap_or_default();
        Self {
            engine,
            rt: tokio::runtime::Handle::current(),
            instances,
            selected: None,
            show_create: false,
            status: String::new(),
            create: None,
        }
    }

    fn refresh_instances(&mut self) {
        self.instances = self.engine.list_instances().unwrap_or_default();
    }

    fn selected_instance(&self) -> Option<&InstanceSummary> {
        let id = self.selected?;
        self.instances.iter().find(|instance| instance.id == id)
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
}

impl Render for KmineApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        let selected = self.selected_instance().cloned();

        div()
            .size_full()
            .relative()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                h_flex()
                    .size_full()
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
                                Some(instance) => PlayTab::new(&instance).into_any_element(),
                                None => empty_state(cx).into_any_element(),
                            }),
                    ),
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
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

fn empty_state(cx: &App) -> impl IntoElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child("Select an instance")
}
