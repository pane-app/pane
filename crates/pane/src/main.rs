#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};
use pane::LauncherWindow;
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
        let launcher = match pane::data_dir() {
            Some(dir) => {
                Launcher::with_packages(runtime, pane::sample_commands(), dir.join("extensions"))
            }
            None => Launcher::new(runtime, pane::sample_commands()),
        }
        .with_link_opener(Arc::new(pane::SystemLinks));
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
            cx.new(|cx| {
                let mut launcher = LauncherWindow::new(launcher, window, cx);
                if let Some(folder) = &preview {
                    launcher.preview_package(folder, window, cx);
                }
                launcher
            })
        })
        .expect("failed to open the Pane window");
        cx.activate(true);
    });
}
