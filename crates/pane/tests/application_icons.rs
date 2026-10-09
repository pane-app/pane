//! Installed applications' own icons in the launcher's window (#172), with
//! the real Applications guest, a fake system of applications and a fake
//! extraction holding its icons back until the test lets them through: an
//! application's row draws a faded application glyph without a tile while
//! its icon is not there, then its own icon, bare, in the same box, the
//! light or dark file as the theme is; Pane's own rows keep their tiles; a
//! pinned application's slot draws its icon; and assistive technology reads
//! an application's row by its title and subtitle only. The rules and the
//! cache are `pane-core`'s `application_icons.rs`.

#[path = "support/settle.rs"]
mod settle;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use gpui::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::applications::icons::{Extracted, IconExtractor};
use pane_core::applications::{Application, Applications};
use pane_core::develop::Toolchains;
use pane_core::icons::encode_png;
use pane_core::system_icons::SystemIcon;
use pane_core::{IconSource, Launcher, ResultAction, Runtime, Screen, SlotChange};
use settle::{settle, until};
use tempfile::TempDir;

/// How long extraction may take on a slow machine.
const LOADED: Duration = Duration::from_secs(30);

/// Open actions' default binding on this system.
const OPEN_ACTIONS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// Firefox's id, and the source its icon is extracted from.
const FIREFOX: &str = "/apps/firefox.desktop";

/// A system with Firefox alone.
struct OneApplication;

impl Applications for OneApplication {
    fn installed(&self) -> Result<Vec<Application>, String> {
        Ok(vec![Application {
            id: FIREFOX.into(),
            name: "Firefox".into(),
            location: "/apps".into(),
            ..Application::default()
        }])
    }

    fn open(&self, _id: &str) -> Result<(), String> {
        Ok(())
    }

    fn icon_source(&self, id: &str) -> Option<String> {
        (id == FIREFOX).then(|| id.to_owned())
    }
}

/// Firefox's icon, a light and a dark one, held back while the gate is
/// closed.
struct HeldIcons {
    closed: Mutex<bool>,
    opened: Condvar,
}

impl HeldIcons {
    fn closed() -> Arc<HeldIcons> {
        Arc::new(HeldIcons {
            closed: Mutex::new(true),
            opened: Condvar::new(),
        })
    }

    fn open(&self) {
        *self.closed.lock().unwrap() = false;
        self.opened.notify_all();
    }
}

fn png(rgba: [u8; 4]) -> Vec<u8> {
    let pixels: Vec<u8> = (0..64 * 64).flat_map(|_| rgba).collect();
    encode_png(64, 64, &pixels).unwrap()
}

impl IconExtractor for HeldIcons {
    fn fingerprint(&self, _source: &str) -> Option<String> {
        Some("one".into())
    }

    fn extract(&self, source: &str) -> Result<Extracted, String> {
        let mut closed = self.closed.lock().unwrap();
        while *closed {
            closed = self.opened.wait(closed).unwrap();
        }
        assert_eq!(source, FIREFOX);
        Ok(Extracted {
            light: SystemIcon::Png(png([30, 30, 30, 255])),
            dark: Some(SystemIcon::Png(png([230, 230, 230, 255]))),
        })
    }
}

fn applications_package() -> PathBuf {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/applications");
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// The launcher window in `theme`, told of background changes as Pane's
/// is, with the Applications package installed over Firefox alone, whose
/// icon `icons` holds back; and the folders it keeps.
fn window<'a>(
    cx: &'a mut TestAppContext,
    theme: &str,
    icons: &Arc<HeldIcons>,
) -> (
    Entity<LauncherWindow>,
    &'a mut VisualTestContext,
    [TempDir; 2],
) {
    let (data, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            None,
            pane::settings::Overrides::parse(Some(theme), None),
            cx,
        )
    });
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let runtime = Runtime::start().unwrap();
    runtime.set_applications(Arc::new(OneApplication));
    let (changed, changes) = pane_core::changes::channel();
    let launcher = Launcher::with_packages(Ok(runtime), vec![], data.path().join("extensions"))
        .with_quick_slots(data.path())
        .with_development(Arc::new(Toolchains::from_env(None)), changed)
        .with_application_icons(cache.path().join("application-icons"), icons.clone());
    cx.foreground_executor()
        .block_on(launcher.install_package(&applications_package()));
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher
    });
    settle(&window, cx);
    (window, cx, [data, cache])
}

fn selector(name: impl Into<String>) -> &'static str {
    Box::leak(name.into().into_boxed_str())
}

fn bounds(cx: &mut VisualTestContext, name: impl Into<String>) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(selector(name))
}

fn drawn(cx: &mut VisualTestContext, name: impl Into<String>) -> bool {
    bounds(cx, name).is_some()
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

fn launcher(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Launcher {
    cx.read_entity(window, |window, _| window.launcher().clone())
}

/// Types `query` and runs the window until it drew Firefox's row: the
/// Applications provider lists it after the query's own results, so the
/// window may first settle on a frame without it.
fn search_firefox(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    query: &str,
) -> pane_core::LauncherView {
    cx.simulate_input(query);
    until(window, cx, |view| {
        view.rows.iter().any(|row| row.title == "Firefox")
    })
}

/// The file names of Firefox's light and dark icons, once kept.
fn icon_files(launcher: &Launcher) -> (String, String) {
    let deadline = Instant::now() + LOADED;
    loop {
        let (view, presentation) = launcher.presented_view();
        let at = view.rows.iter().position(|row| row.title == "Firefox");
        if let Some(at) = at
            && let Some(icon) = &presentation.rows[at].icon
            && let IconSource::Image { light, dark } = &icon.source
        {
            let name = |path: &Path| path.file_name().unwrap().to_string_lossy().into_owned();
            return (name(light), name(dark));
        }
        assert!(Instant::now() < deadline, "Firefox's icon was never kept");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// An application's row draws the placeholder, bare, while its icon is
/// held back, then its own icon in the same box; in the dark theme the
/// dark file. Nothing of Pane's is drawn behind either (ADR 0035), and
/// the Actions panel's header draws the icon bare too. Pane's own row
/// keeps its tile.
#[gpui::test]
fn an_applications_row_draws_its_own_icon_bare_where_its_placeholder_was(cx: &mut TestAppContext) {
    let icons = HeldIcons::closed();
    let (window, cx, _folders) = window(cx, "dark", &icons);
    search_firefox(&window, cx, "fire");

    assert!(drawn(cx, "row-Firefox"));
    assert!(
        drawn(cx, "icon-Firefox-glyph-category"),
        "the placeholder, drawn bare"
    );
    assert!(
        !drawn(cx, "icon-Firefox-tile"),
        "the placeholder draws bare: no tile behind it"
    );
    let waiting = bounds(cx, "icon-Firefox").expect("the icon's box");
    let row = bounds(cx, "row-Firefox").unwrap();

    icons.open();
    let (light, dark) = icon_files(&launcher(&window, cx));
    assert_ne!(light, dark);
    until_drawn(cx, &format!("icon-Firefox-image-{dark}"));
    assert!(!drawn(cx, format!("icon-Firefox-image-{light}")));
    assert!(!drawn(cx, "icon-Firefox-glyph-category"));
    assert!(
        !drawn(cx, "icon-Firefox-tile"),
        "the application's own icon draws bare"
    );
    // Nothing moved: the icon is where the placeholder was, the row as tall.
    assert_eq!(bounds(cx, "icon-Firefox").unwrap(), waiting);
    assert_eq!(bounds(cx, "row-Firefox").unwrap().size, row.size);

    // The Actions panel's header: the application's own icon, bare too.
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    let at = view
        .rows
        .iter()
        .position(|row| row.title == "Firefox")
        .expect("Firefox's row");
    cx.read_entity(&window, |window, _| window.launcher().select(at));
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(drawn(cx, format!("icon-actions-header-image-{dark}")));
    assert!(!drawn(cx, "icon-actions-header-tile"));
    cx.simulate_keystrokes("escape");
    settle(&window, cx);

    // Pane's own row: its tile, no icon drawn bare.
    for _ in 0.."fire".len() {
        cx.simulate_keystrokes("backspace");
    }
    cx.simulate_input("settings");
    let view = settle(&window, cx);
    assert!(view.rows.iter().any(|row| row.title == "Settings…"));
    assert!(drawn(cx, "row-Settings…"));
    assert!(!drawn(cx, "icon-Settings…"));
}

/// In the light theme, the light file.
#[gpui::test]
fn an_applications_row_draws_its_light_icon_in_the_light_theme(cx: &mut TestAppContext) {
    let icons = HeldIcons::closed();
    let (window, cx, _folders) = window(cx, "light", &icons);
    search_firefox(&window, cx, "fire");
    icons.open();
    let (light, dark) = icon_files(&launcher(&window, cx));
    until_drawn(cx, &format!("icon-Firefox-image-{light}"));
    assert!(!drawn(cx, format!("icon-Firefox-image-{dark}")));
}

/// A pinned application's slot draws its own icon, bare.
#[gpui::test]
fn a_pinned_applications_slot_draws_its_icon(cx: &mut TestAppContext) {
    let icons = HeldIcons::closed();
    icons.open();
    let (window, cx, _folders) = window(cx, "dark", &icons);
    let view = search_firefox(&window, cx, "fire");
    let launcher = launcher(&window, cx);
    let at = view
        .rows
        .iter()
        .position(|row| row.title == "Firefox")
        .expect("Firefox's row");
    let (change, recorded) = launcher.change_quick_slots(&view.rows[at].id, ResultAction::Pin);
    assert!(matches!(change, SlotChange::Changed(_)), "{change:?}");
    cx.foreground_executor().block_on(recorded);
    let (_, dark) = icon_files(&launcher);

    for _ in 0..4 {
        cx.simulate_keystrokes("backspace");
    }
    settle(&window, cx);
    until_drawn(cx, &format!("icon-slot-1-image-{dark}"));
    assert!(!drawn(cx, "icon-slot-1-tile"), "the slot draws the icon bare");
}

/// Assistive technology reads an application's row by its title and
/// subtitle only: its icon is decoration.
#[gpui::test]
fn an_applications_icon_is_decorative_to_assistive_technology(cx: &mut TestAppContext) {
    let icons = HeldIcons::closed();
    icons.open();
    let (window, cx, _folders) = window(cx, "dark", &icons);
    search_firefox(&window, cx, "fire");
    let (_, dark) = icon_files(&launcher(&window, cx));
    until_drawn(cx, &format!("icon-Firefox-image-{dark}"));

    cx.update(|window, _| window.set_a11y_forced(true));
    cx.run_until_parked();
    let json = cx
        .update(|window, _| window.debug_a11y_tree_json())
        .expect("an accessibility tree");
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes: Vec<serde_json::Value> = tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .map(|node| node["aria"].clone())
        .collect();
    let option = nodes
        .iter()
        .find(|node| node["role"] == "ListBoxOption" && node["label"] == "Firefox")
        .unwrap_or_else(|| panic!("no Firefox option in {nodes:?}"));
    let description = option["description"].as_str().unwrap_or_default();
    assert_eq!(description, "Application", "{option}");
    assert!(
        !nodes.iter().any(|node| node["role"] == "Image"
            && node["label"]
                .as_str()
                .is_some_and(|label| label.contains("Firefox"))),
        "no icon is read: {nodes:?}"
    );
}
