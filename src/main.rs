mod app;
mod modals;
mod screens;

use std::sync::Arc;

use gpui::*;
use gpui_component::{Root, Theme, ThemeMode};
use kmine_engine::{Engine, LauncherPaths};

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio");
    let paths = LauncherPaths::new(LauncherPaths::default_root());
    let engine = runtime.block_on(Engine::open(paths)).expect("engine");
    let engine = Arc::new(engine);
    let _enter = runtime.enter();

    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        cx.activate(true);
        let engine = engine.clone();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(960.), px(640.)), cx)),
            titlebar: Some(TitlebarOptions {
                title: Some("kmine".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        cx.spawn(async move |cx| {
            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| crate::app::KmineApp::new(engine, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("window");
        })
        .detach();
    });
}
