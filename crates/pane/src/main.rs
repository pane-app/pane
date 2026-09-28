#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};
use pane::LauncherWindow;
use pane_core::develop::Toolchains;
use pane_core::{Launcher, Runtime};

/// `pane [--install <folder>]`: `--install` opens with the package in
/// `<folder>` shown for installation, as if chosen with the folder picker.
fn package_to_preview() -> Option<PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--install" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

fn main() {
    let preview = package_to_preview();
    gpui_platform::application().run(move |cx: &mut App| {
        pane::bind_keys(cx);
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        let runtime = match pane::cache_dir() {
            Some(dir) => Runtime::start_with_cache(dir),
            None => Runtime::start(),
        };
        // Quitting ends the native helpers still running, which would
        // otherwise outlive Pane.
        if let Ok(runtime) = &runtime {
            let runtime = runtime.clone();
            cx.on_app_quit(move |_| {
                runtime.stop_helpers();
                async {}
            })
            .detach();
        }
        let launcher = match pane::data_dir() {
            Some(dir) => {
                Launcher::with_packages(runtime, pane::sample_commands(), dir.join("extensions"))
            }
            None => Launcher::new(runtime, pane::sample_commands()),
        }
        .with_link_opener(Arc::new(pane::SystemLinks));
        // Global hotkeys: the system's adapter is made on the main thread,
        // whose run loop receives the presses on macOS.
        let (press_sender, mut presses) = pane_core::hotkeys::channel();
        let launcher = launcher.with_hotkeys(pane_core::hotkeys::native(press_sender));
        // Development mode builds with the author's tools; a JavaScript or
        // TypeScript package with this checkout's build unless
        // PANE_COMPONENTIZE_JS names another.
        let (change_sender, changes) = pane_core::changes::channel();
        let default_js = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/componentize-js/pane_js.py");
        let toolchains = Toolchains::from_env(Some(default_js));
        let launcher = launcher.with_development(Arc::new(toolchains), change_sender);
        let bounds = Bounds::centered(None, size(px(640.), px(420.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Pane".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let window = cx
            .open_window(options, |window, cx| {
                cx.new(|cx| {
                    let mut launcher = LauncherWindow::new(launcher, window, cx);
                    launcher.follow_changes(changes, window, cx);
                    if let Some(folder) = &preview {
                        launcher.preview_package(folder, window, cx);
                    }
                    launcher
                })
            })
            .expect("failed to open the Pane window");
        // A hotkey pressed in any application opens its command here.
        cx.spawn(async move |cx| {
            while let Some(shortcut) = presses.next().await {
                let shown = window.update(cx, |launcher, window, cx| {
                    launcher.hotkey_pressed(&shortcut, window, cx)
                });
                if shown.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.activate(true);
    });
}
