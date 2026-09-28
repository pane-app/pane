#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};
use pane::LauncherWindow;
use pane_core::{Launcher, Runtime};

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        pane::bind_keys(cx);
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        let runtime = match pane::cache_dir() {
            Some(dir) => Runtime::start_with_cache(dir),
            None => Runtime::start(),
        };
        let launcher = Launcher::new(runtime, pane::sample_commands());
        let bounds = Bounds::centered(None, size(px(640.), px(420.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Pane".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            cx.new(|cx| LauncherWindow::new(launcher, window, cx))
        })
        .expect("failed to open the Pane window");
        cx.activate(true);
    });
}
