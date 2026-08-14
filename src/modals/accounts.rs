use gpui::prelude::*;
use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, Window,
    div, px,
};
use gpui_component::{
    ActiveTheme, Disableable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex,
    list::ListItem,
    v_flex,
};
use kmine_engine::{AccountId, AccountSummary, Engine};

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
    modal: &AccountsModal,
    on_select: impl Fn(AccountId, &mut Window, &mut App) + Clone + 'static,
    on_delete: impl Fn(AccountId, &mut Window, &mut App) + Clone + 'static,
    on_add: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    div()
        .id("accounts-overlay")
        .absolute()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(cx.theme().overlay.opacity(0.5))
        .child(
            v_flex()
                .w(px(420.))
                .max_h(px(480.))
                .gap_4()
                .p_5()
                .rounded(cx.theme().radius_lg)
                .bg(cx.theme().background)
                .border_1()
                .border_color(cx.theme().border)
                .shadow_lg()
                .child(div().font_semibold().child("Accounts"))
                .child(account_list(modal, on_select, on_delete, cx))
                .when(modal.busy, |this| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("Signing in…"),
                    )
                })
                .when_some(modal.error.as_ref(), |this, error| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(error.clone()),
                    )
                })
                .child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("accounts-close")
                                .label("Close")
                                .on_click(on_close),
                        )
                        .child(
                            Button::new("accounts-add")
                                .primary()
                                .label("Add account")
                                .disabled(modal.busy)
                                .on_click(on_add),
                        ),
                ),
        )
}

fn account_list(
    modal: &AccountsModal,
    on_select: impl Fn(AccountId, &mut Window, &mut App) + Clone + 'static,
    on_delete: impl Fn(AccountId, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .id("accounts-list")
        .min_h(px(120.))
        .max_h(px(280.))
        .gap_1()
        .overflow_y_scroll()
        .when(modal.accounts.is_empty(), |this| {
            this.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No accounts"),
            )
        })
        .children(modal.accounts.iter().map(|account| {
            let id = account.uuid;
            let on_select = on_select.clone();
            let on_delete = on_delete.clone();
            account_row(
                account,
                move |_, window, cx| {
                    on_select(id, window, cx);
                },
                move |_, window, cx| {
                    on_delete(id, window, cx);
                },
            )
        }))
}

fn account_row(
    account: &AccountSummary,
    on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_delete: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> ListItem {
    let key = SharedString::from(account.uuid.as_hyphenated());
    let delete_id = SharedString::from(format!("account-delete-{}", account.uuid.as_hyphenated()));
    ListItem::new(key)
        .selected(account.selected)
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .child(account.username.clone())
                .child(
                    Button::new(delete_id)
                        .ghost()
                        .compact()
                        .label("Delete")
                        .on_click(on_delete),
                ),
        )
        .on_click(on_select)
}
