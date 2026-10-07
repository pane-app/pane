//! Web images in the launcher's window (#142), with the Rust icons sample
//! pointed at the image server, a local test server
//! (`pane-core/tests/support/image_server.rs`): the list draws each web
//! image's fallback at once, and, once Pane downloaded the image, the
//! window draws it by itself, told by the launcher that it changed, in a
//! row as in an action's icon in the Actions panel; an address without an
//! image keeps its fallback. The rules (limits,
//! de-duplication, the cache, system icons) are `pane-core`'s
//! `web_icons.rs`.

#[path = "../../pane-core/tests/support/image_server.rs"]
mod image_server;
#[path = "support/settle.rs"]
mod settle;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use image_server::ImageServer;
use pane::LauncherWindow;
use pane_core::develop::Toolchains;
use pane_core::icons::web_image_stem;
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};
use serde_json::json;
use settle::settle;
use tempfile::TempDir;

/// How long a download may take on a slow machine.
const LOADED: Duration = Duration::from_secs(30);

/// Open actions' default binding on this system.
const OPEN_ACTIONS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// The assembled sample package `name` under `target/guests/packages`.
fn assembled(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(name);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

fn copy_folder(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The launcher window in the dark theme, telling it of background changes
/// as Pane's does, with the Rust icons sample installed and pointed at
/// `server`, on root search; and the folders it keeps.
fn window<'a>(
    cx: &'a mut TestAppContext,
    server: &ImageServer,
) -> (
    Entity<LauncherWindow>,
    &'a mut VisualTestContext,
    [TempDir; 2],
) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            None,
            pane::settings::Overrides::parse(Some("dark"), None),
            cx,
        )
    });
    let folder = sources.path().join("sample-icons");
    copy_folder(&assembled("sample-icons"), &folder);
    let key = PackageIdentity::local(&folder).unwrap().key();
    let packages = data.path().join("extensions");
    fs::create_dir_all(&packages).unwrap();
    let settings = json!({
        "version": 1,
        "packages": { key: { "imageServer": server.url() } }
    });
    fs::write(packages.join("settings.json"), settings.to_string()).unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (changed, changes) = pane_core::changes::channel();
    let launcher = Launcher::with_packages(Runtime::start(), vec![], packages)
        .with_development(Arc::new(Toolchains::from_env(None)), changed);
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Icons sample".into())
    );
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher
    });
    settle(&window, cx);
    (window, cx, [sources, data])
}

/// Opens the sample's "Icons" command, its list drawn.
fn open_icons(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    cx.simulate_input("icons");
    let view = settle(window, cx);
    let at = view
        .rows
        .iter()
        .position(|row| row.title == "Icons")
        .unwrap_or_else(|| panic!("no Icons row in {:?}", view.rows));
    cx.read_entity(window, |window, _| window.launcher().select(at));
    cx.simulate_keystrokes("enter");
    let view = settle(window, cx);
    assert_eq!(
        (&view.screen, view.title.as_str()),
        (&Screen::Command, "Icons sample"),
        "{:?}",
        view.status
    );
}

fn selector(name: impl Into<String>) -> &'static str {
    Box::leak(name.into().into_boxed_str())
}

fn drawn(cx: &mut VisualTestContext, name: impl Into<String>) -> bool {
    cx.debug_bounds(selector(name)).is_some()
}

/// Runs the window, never making it draw, until its last frame drew what
/// `name` names.
fn until_drawn(cx: &mut VisualTestContext, name: &str) {
    let deadline = Instant::now() + LOADED;
    loop {
        cx.run_until_parked();
        if drawn(cx, name) {
            return;
        }
        assert!(Instant::now() < deadline, "{name} was never drawn");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The list draws the slow image's fallback at once, then the image once it
/// arrives, by itself; the favicon arrives alike; the broken image keeps
/// its fallback.
#[gpui::test]
fn a_web_image_draws_its_fallback_while_it_loads_then_the_image(cx: &mut TestAppContext) {
    let server = ImageServer::start();
    let (window, cx, _folders) = window(cx, &server);
    open_icons(&window, cx);
    let slow = format!(
        "icon-Slow web image-image-{}.png",
        web_image_stem(&format!("{}/images/slow.png", server.url()))
    );
    // Held back by the server: the fallback, drawn with the list.
    assert!(drawn(cx, "icon-Slow web image-glyph-clock"));
    assert!(drawn(cx, "icon-Same slow image-glyph-clock"));
    assert!(!drawn(cx, slow.as_str()));

    let favicon = format!(
        "icon-Favicon-image-{}.png",
        web_image_stem(&format!("{}/favicon.ico", server.url()))
    );
    until_drawn(cx, &favicon);
    assert!(!drawn(cx, "icon-Favicon-glyph-global"));

    server.release();
    until_drawn(cx, &slow);
    assert!(!drawn(cx, "icon-Slow web image-glyph-clock"));
    until_drawn(
        cx,
        &format!(
            "icon-Same slow image-image-{}.png",
            web_image_stem(&format!("{}/images/slow.png", server.url()))
        ),
    );
    assert!(drawn(cx, "icon-Broken image-glyph-link-broken"));
    assert_eq!(server.count("/images/slow.png"), 1);
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    assert_eq!(view.screen, Screen::Command);
}

/// The Actions panel draws an action's web image alike: its fallback while
/// the server holds the image back, then the image once it arrives, by
/// itself while the panel stays open.
#[gpui::test]
fn an_actions_web_image_draws_its_fallback_then_the_image_in_the_panel(cx: &mut TestAppContext) {
    let server = ImageServer::start();
    let (window, cx, _folders) = window(cx, &server);
    open_icons(&window, cx);
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    let at = view
        .rows
        .iter()
        .position(|row| row.title == "Built-in icon")
        .expect("the row");
    cx.read_entity(&window, |window, _| window.launcher().select(at));
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(cx.read_entity(&window, |window, _| window.actions_open()));
    assert!(drawn(cx, "icon-action-Open Image-glyph-clock"));

    server.release();
    until_drawn(
        cx,
        &format!(
            "icon-action-Open Image-image-{}.png",
            web_image_stem(&format!("{}/images/slow.png", server.url()))
        ),
    );
    assert!(!drawn(cx, "icon-action-Open Image-glyph-clock"));
    assert!(cx.read_entity(&window, |window, _| window.actions_open()));
    assert_eq!(server.count("/images/slow.png"), 1);
}
