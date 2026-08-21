use std::path::Path;

use gpui::{App, ClickEvent, IntoElement, ParentElement, Styled, Window, div};
use gpui_component::{ActiveTheme, IconName, button::Button, h_flex, v_flex};

use crate::chrome::{
    card, modal, modal_body, modal_close, modal_footer, modal_header, section_label, sheet,
};

pub fn render(
    library: &Path,
    on_reveal: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    modal("settings-overlay", true, on_close.clone(), cx).child(
        sheet(cx)
            .child(modal_header(
                IconName::Settings,
                "Settings",
                "Launcher data lives in this folder.",
                cx,
            ))
            .child(
                modal_body()
                    .pb_6()
                    .child(
                        v_flex().gap_2().child(section_label("Library", cx)).child(
                            card(cx).child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .text_sm()
                                            .text_ellipsis()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(library.display().to_string()),
                                    )
                                    .child(
                                        Button::new("settings-reveal")
                                            .outline()
                                            .label(reveal_label())
                                            .on_click(on_reveal),
                                    ),
                            ),
                        ),
                    )
                    .child(
                        v_flex().gap_2().child(section_label("About", cx)).child(
                            card(cx).child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("kmine {}", env!("CARGO_PKG_VERSION"))),
                            ),
                        ),
                    ),
            )
            .child(
                modal_footer(cx).child(
                    Button::new("settings-close")
                        .outline()
                        .label("Close")
                        .on_click(on_close.clone()),
                ),
            )
            .child(modal_close(on_close)),
    )
}

pub fn reveal_library(path: &Path) {
    let _ = {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open").arg(path).spawn()
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("explorer").arg(path).spawn()
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            std::process::Command::new("xdg-open").arg(path).spawn()
        }
    };
}

fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else if cfg!(target_os = "windows") {
        "Open in Explorer"
    } else {
        "Open folder"
    }
}
