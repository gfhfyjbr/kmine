mod app;
mod assets;
mod chrome;
mod game_output;
mod modals;
mod providers;
mod screens;
mod theme;

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;
use gpui_component::{Root, Theme, ThemeMode, dialog::Cancel};
use kmine_engine::{Engine, LauncherPaths};

actions!(kmine, [Quit]);

fn window_background() -> WindowBackgroundAppearance {
    if cfg!(target_os = "macos") {
        WindowBackgroundAppearance::Blurred
    } else if cfg!(target_os = "windows") {
        WindowBackgroundAppearance::MicaBackdrop
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

pub fn sidebar_is_glass() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio");
    let paths = LauncherPaths::new(LauncherPaths::default_root());
    let engine = runtime.block_on(Engine::open(paths)).expect("engine");
    let engine = Arc::new(engine);
    let _enter = runtime.enter();

    gpui_platform::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            crate::theme::apply_launcher_colors(cx);
            cx.bind_keys([
                #[cfg(target_os = "macos")]
                KeyBinding::new("cmd-q", Quit, None),
                #[cfg(not(target_os = "macos"))]
                KeyBinding::new("alt-f4", Quit, None),
                KeyBinding::new("escape", Cancel, Some("Modal")),
            ]);
            cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
            cx.set_menus(vec![Menu {
                name: "kmine".into(),
                items: vec![MenuItem::action("Quit", Quit)],
                disabled: false,
            }]);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
            let engine = engine.clone();
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1080.), px(700.)), cx)),
                window_background: window_background(),
                titlebar: Some(TitlebarOptions {
                    title: Some("kmine".into()),
                    appears_transparent: cfg!(target_os = "macos"),
                    traffic_light_position: Some(point(px(16.), px(18.))),
                    ..Default::default()
                }),
                ..Default::default()
            };
            cx.spawn(async move |cx| {
                cx.open_window(options, |window, cx| {
                    let view = cx.new(|cx| crate::app::KmineApp::new(engine, cx));
                    cx.new(|cx| {
                        let mut root = Root::new(view, window, cx);
                        if sidebar_is_glass() {
                            root = root.bg(transparent_black());
                        }
                        root
                    })
                })
                .expect("window");
            })
            .detach();
        });
}
