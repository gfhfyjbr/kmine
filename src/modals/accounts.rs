use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    div, img, px, App, ClickEvent, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    alert::Alert,
    avatar::Avatar,
    button::{Button, ButtonVariants},
    h_flex,
    spinner::Spinner,
    tag::Tag,
    v_flex, ActiveTheme, Disableable, Icon, IconName, Sizable,
};
use kmine_engine::{AccountId, AccountSummary, Engine};

use crate::chrome::{
    cta, empty_panel, modal, modal_body, modal_close, modal_footer, modal_header, sheet,
};
use crate::smooth_scroll::SmoothScroll;

pub const AUTH_NOT_CONFIGURED_HINT: &str = "Set CLIENT_ID in crates/engine/src/auth/constants.rs and register redirect http://127.0.0.1:47821/auth";

pub struct AccountsModal {
    pub accounts: Vec<AccountSummary>,
    pub error: Option<String>,
    pub busy: bool,
}

impl AccountsModal {
    pub fn from_engine(engine: &Engine) -> Self {
        Self {
            accounts: engine.list_accounts().unwrap_or_default(),
            error: None,
            busy: false,
        }
    }

    pub fn refresh(&mut self, engine: &Engine) {
        self.accounts = engine.list_accounts().unwrap_or_default();
    }

    pub fn identity_label(&self) -> &str {
        identity_label(&self.accounts)
    }
}

pub fn identity_label(accounts: &[AccountSummary]) -> &str {
    accounts
        .iter()
        .find(|account| account.selected)
        .map(|account| account.username.as_str())
        .unwrap_or("Not signed in")
}

pub fn render(
    modal_state: &AccountsModal,
    skin: impl Fn(AccountId) -> Option<PathBuf>,
    on_select: impl Fn(AccountId, &mut Window, &mut App) + Clone + 'static,
    on_delete: impl Fn(AccountId, &mut Window, &mut App) + Clone + 'static,
    on_add: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    scroll: &SmoothScroll,
    cx: &App,
) -> impl IntoElement {
    modal("accounts-overlay", !modal_state.busy, on_close.clone(), cx).child(
        sheet(cx)
            .max_h(px(560.))
            .child(modal_header(
                IconName::User,
                "Accounts",
                "The selected account is used unless an instance overrides it.",
                cx,
            ))
            .child(
                modal_body()
                    .child(account_list(
                        modal_state,
                        skin,
                        on_select,
                        on_delete,
                        scroll,
                        cx,
                    ))
                    .when(modal_state.busy, |this| {
                        this.child(
                            h_flex()
                                .w_full()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .py_2()
                                .rounded(px(10.))
                                .bg(cx.theme().muted)
                                .child(Spinner::new().small().color(cx.theme().muted_foreground))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("Finish signing in in the browser"),
                                ),
                        )
                    })
                    .when_some(modal_state.error.as_ref(), |this, error| {
                        this.child(Alert::error("accounts-error", error.clone()))
                    }),
            )
            .child(
                modal_footer(cx)
                    .child(
                        Button::new("accounts-close")
                            .outline()
                            .label("Close")
                            .on_click(on_close.clone()),
                    )
                    .child(
                        cta("accounts-add")
                            .label("Add account")
                            .loading(modal_state.busy)
                            .disabled(modal_state.busy)
                            .on_click(on_add),
                    ),
            )
            .child(modal_close(on_close)),
    )
}

fn account_list(
    modal_state: &AccountsModal,
    skin: impl Fn(AccountId) -> Option<PathBuf>,
    on_select: impl Fn(AccountId, &mut Window, &mut App) + Clone + 'static,
    on_delete: impl Fn(AccountId, &mut Window, &mut App) + Clone + 'static,
    scroll: &SmoothScroll,
    cx: &App,
) -> impl IntoElement {
    scroll
        .vertical(
            v_flex()
                .id("accounts-list")
                .min_h(px(140.))
                .max_h(px(280.))
                .gap_1(),
        )
        .when(modal_state.accounts.is_empty(), |this| {
            this.child(empty_panel(
                IconName::User,
                "No Microsoft accounts yet",
                "Add one to launch the game.",
                cx,
            ))
        })
        .children(modal_state.accounts.iter().map(|account| {
            let id = account.uuid;
            let on_select = on_select.clone();
            let on_delete = on_delete.clone();
            account_row(
                account,
                skin(id),
                move |_, window, cx| {
                    on_select(id, window, cx);
                },
                move |_, window, cx| {
                    cx.stop_propagation();
                    on_delete(id, window, cx);
                },
                cx,
            )
        }))
}

fn account_row(
    account: &AccountSummary,
    skin: Option<PathBuf>,
    on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_delete: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let key = SharedString::from(account.uuid.as_hyphenated());
    let delete_id = SharedString::from(format!("account-delete-{}", account.uuid.as_hyphenated()));
    div()
        .id(key)
        .w_full()
        .rounded(px(10.))
        .px_3()
        .py_2()
        .border_1()
        .border_color(if account.selected {
            cx.theme().border
        } else {
            cx.theme().border.opacity(0.)
        })
        .bg(if account.selected {
            cx.theme().muted
        } else {
            cx.theme().transparent
        })
        .cursor_pointer()
        .hover(|this| this.bg(cx.theme().muted))
        .on_click(on_select)
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    h_flex()
                        .min_w_0()
                        .items_center()
                        .gap_3()
                        .child(account_face(account, skin.as_deref(), cx))
                        .child(
                            v_flex()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_ellipsis()
                                        .child(account.username.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(if account.selected {
                                            "Used to launch"
                                        } else {
                                            "Microsoft account"
                                        }),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .when(account.selected, |this| {
                            this.child(Tag::secondary().small().child("Selected"))
                        })
                        .child(
                            Button::new(delete_id)
                                .ghost()
                                .compact()
                                .icon(Icon::empty().path("icons/trash.svg"))
                                .tooltip("Remove account")
                                .on_click(move |event, window, cx| {
                                    cx.stop_propagation();
                                    on_delete(event, window, cx);
                                }),
                        ),
                ),
        )
}

fn account_face(
    account: &AccountSummary,
    skin: Option<&std::path::Path>,
    cx: &App,
) -> impl IntoElement {
    match skin {
        Some(path) => div()
            .size(px(32.))
            .flex_shrink_0()
            .rounded(px(8.))
            .overflow_hidden()
            .bg(cx.theme().muted)
            .border_1()
            .border_color(cx.theme().border)
            .child(img(path.to_path_buf()).size_full().rounded(px(8.)))
            .into_any_element(),
        None => Avatar::new()
            .name(account.username.clone())
            .small()
            .rounded(px(8.))
            .into_any_element(),
    }
}
