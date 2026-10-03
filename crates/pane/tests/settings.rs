//! Pane's Settings window: its three entry points converge on one window,
//! which closes without quitting Pane, keeps its keyboard input to itself,
//! and answers for its titlebar controls. Drives the real windows through
//! GPUI's test platform, as `window.rs` drives the launcher's. The
//! Appearance page is driven the same way — through the page's own
//! controls — with what the windows paint checked on their quads, so a
//! choice is observed at the same boundary a user sees it. The General
//! page's launch-at-login toggle is driven the same way, through a fake
//! login system the tests script — no test ever touches the real login
//! configuration of the machine running it; and the Extensions page
//! manages extensions through the launcher's own operations.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, Modifiers, TestAppContext, VisualTestContext, WindowHandle, prelude::*, px,
};
use pane::{APP_VERSION, LauncherWindow, SettingsWindow};
use pane_core::autostart::{Autostart, Registration};
use pane_core::develop::{Build, BuildJob, BuildOutcome, Builder};
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

use settle::settle;

/// The keystroke that opens Settings on this platform: Cmd+, on macOS,
/// Ctrl+, on Windows and Linux.
fn settings_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-,"
    } else {
        "ctrl-,"
    }
}

/// Records the links the launcher is asked to open, so that no browser
/// opens; taking a moment to answer, as a system handler does.
#[derive(Default)]
struct RecordedLinks(std::sync::Mutex<Vec<String>>);

impl pane_core::LinkOpener for RecordedLinks {
    fn open(&self, url: &str) -> Result<(), String> {
        std::thread::sleep(Duration::from_millis(10));
        self.0.lock().unwrap().push(url.into());
        Ok(())
    }
}

/// A link opener that refuses, to see the About page explain it.
struct RefusingLinks;

impl pane_core::LinkOpener for RefusingLinks {
    fn open(&self, _url: &str) -> Result<(), String> {
        Err("no program to open web links is installed".into())
    }
}

/// A fake login system, standing where the pane binary puts the
/// platform's own: the registration its fake platform holds, and the
/// refusals its changes are scripted to answer with. The tests drive the
/// General page's launch-at-login toggle through it, so nothing here
/// touches the login configuration of the machine running the test.
#[derive(Default)]
struct FakeLogin {
    /// The registration the fake platform holds now.
    registration: std::sync::Mutex<Registration>,
    /// The refusals the next enable and the next disable answer with,
    /// when a test scripts failures; each fires once.
    refusals: std::sync::Mutex<(Option<String>, Option<String>)>,
}

impl FakeLogin {
    /// The registration the fake platform holds.
    fn registration(&self) -> Registration {
        *self.registration.lock().unwrap()
    }
}

impl Autostart for FakeLogin {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn registered(&self) -> Result<Registration, String> {
        Ok(self.registration())
    }

    fn enable(&self) -> Result<Registration, String> {
        if let Some(why) = self.refusals.lock().unwrap().0.take() {
            return Err(why);
        }
        *self.registration.lock().unwrap() = Registration::Enabled;
        Ok(Registration::Enabled)
    }

    fn disable(&self) -> Result<Registration, String> {
        if let Some(why) = self.refusals.lock().unwrap().1.take() {
            return Err(why);
        }
        *self.registration.lock().unwrap() = Registration::Disabled;
        Ok(Registration::Disabled)
    }
}

type Opened<'a> = (
    gpui::Entity<LauncherWindow>,
    Arc<RecordedLinks>,
    &'a mut VisualTestContext,
);

/// Opens the launcher window over a launcher whose links are recorded,
/// ready for the Settings window's documentation entry.
fn open_launcher(cx: &mut TestAppContext) -> Opened<'_> {
    let links = Arc::new(RecordedLinks::default());
    let launcher =
        Launcher::new(Runtime::start(), pane::sample_commands()).with_link_opener(links.clone());
    // Guest replies arrive from the real runtime thread, outside the test
    // scheduler's deterministic control.
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (window, links, cx)
}

/// Opens the launcher window over a launcher whose links refuse.
fn open_refusing(
    cx: &mut TestAppContext,
) -> (gpui::Entity<LauncherWindow>, &mut VisualTestContext) {
    let launcher = Launcher::new(Runtime::start(), pane::sample_commands())
        .with_link_opener(Arc::new(RefusingLinks));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (window, cx)
}

/// The assembled Rust settings sample, copied into `folder` as a package
/// with its own identity, as the management-flow tests' fixture.
fn settings_package(folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages/sample-settings");
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    for file in ["pane.json", "sample_settings.wasm"] {
        fs::copy(assembled.join(file), folder.join(file)).unwrap();
    }
    folder.to_path_buf()
}

/// Writes the package the reload tests use in `folder`: the Rust sample
/// guest as `hello.wasm`, replaceable with another guest to fake a new
/// build of the source.
fn hello_package(folder: &Path) -> PathBuf {
    let guest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/sample_rust.wasm");
    assert!(
        guest.exists(),
        "{} is missing; run `cargo xtask guests`",
        guest.display()
    );
    fs::create_dir_all(folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{
  "manifestVersion": 1,
  "title": "Hello",
  "version": "1.0.0",
  "apiVersion": "0.1",
  "commands": [{ "id": "hello", "title": "Say hello", "component": "hello.wasm" }]
}"#,
    )
    .unwrap();
    fs::copy(guest, folder.join("hello.wasm")).unwrap();
    // The source the fake development builder reads: the guest it copies
    // on a successful save, or an error it fails on.
    fs::write(folder.join("source.txt"), "sample_rust").unwrap();
    folder.to_path_buf()
}

/// Replaces the package's component with the built guest `name`, as a new
/// build of the package would.
fn rebuild(folder: &Path, name: &str) {
    let guest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(format!("{name}.wasm"));
    fs::copy(guest, folder.join("hello.wasm")).unwrap();
}

/// Writes an operations fixture package titled `title` in `folder`,
/// publishing `echo` 1 and declaring `dependencies` (JSON array contents),
/// for the required-dependent confirmation paths.
fn operations_package(folder: &Path, title: &str, dependencies: &str) -> PathBuf {
    let guest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/operations_fixture.wasm");
    assert!(
        guest.exists(),
        "{} is missing; run `cargo xtask guests`",
        guest.display()
    );
    fs::create_dir_all(folder).unwrap();
    fs::copy(guest, folder.join("fixture.wasm")).unwrap();
    let manifest = format!(
        r#"{{
            "manifestVersion": 1,
            "title": "{title}",
            "apiVersion": "0.1",
            "operations": [{{ "id": "echo", "version": 1, "component": "fixture.wasm" }}],
            "dependencies": [{dependencies}]
        }}"#
    );
    fs::write(folder.join("pane.json"), manifest).unwrap();
    folder.to_path_buf()
}

/// Opens the launcher window over a launcher that installs packages in
/// `data`'s extensions folder, with `folder`'s package installed: the
/// Extensions page's tests manage those, through the launcher the Settings
/// window shares with this one.
fn open_installed<'a>(
    cx: &'a mut TestAppContext,
    data: &TempDir,
    folder: &Path,
) -> (gpui::Entity<LauncherWindow>, &'a mut VisualTestContext) {
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    install(&launcher, folder);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (window, cx)
}

/// Installs `folder`'s package into `launcher`, as the window's install
/// flow does once its checks pass.
fn install(launcher: &Launcher, folder: &Path) {
    futures::executor::block_on(launcher.install_package(folder));
    let identity = PackageIdentity::local(folder).expect("a local package");
    assert!(
        launcher
            .packages()
            .iter()
            .any(|package| package.identity == identity),
        "the package was installed"
    );
}

/// Opens the Settings window with the local `Ctrl+,` shortcut and returns
/// a context driving it, on its Extensions page — the window opens on the
/// Appearance page, so this walks the sidebar to Extensions first. The
/// window is made tall enough that the page's whole list is in reach of a
/// click without scrolling it — the page itself scrolls when the window is
/// smaller.
fn open_extensions(
    cx: &mut VisualTestContext,
) -> (WindowHandle<SettingsWindow>, VisualTestContext) {
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.simulate_resize(gpui::size(px(740.), px(1100.)));
    settings_cx.run_until_parked();
    let extensions = settings_cx
        .debug_bounds("section-Extensions")
        .expect("the Extensions section");
    settings_cx.simulate_click(extensions.center(), Modifiers::none());
    settings_cx.run_until_parked();
    (settings, settings_cx)
}

/// Clicks the row whose debug selector is `row` on the Extensions page,
/// as its user would.
fn click_row(settings_cx: &mut VisualTestContext, row: &'static str) {
    let bounds = settings_cx
        .debug_bounds(row)
        .unwrap_or_else(|| panic!("no {row} on the Extensions page"));
    settings_cx.simulate_click(bounds.center(), Modifiers::none());
    settings_cx.run_until_parked();
}

/// The titles of the launcher's current rows, to assert what it shows.
fn titles(launcher: &gpui::Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.read_entity(launcher, |window, _| {
        window
            .launcher()
            .view()
            .rows
            .into_iter()
            .map(|row| row.title)
            .collect()
    })
}

/// The open Settings windows: the window list is the one-window registry
/// the app itself uses.
fn settings_windows(cx: &TestAppContext) -> Vec<WindowHandle<SettingsWindow>> {
    cx.update(|cx| {
        cx.windows()
            .into_iter()
            .filter_map(|window| window.downcast::<SettingsWindow>())
            .collect()
    })
}

/// A test context for the Settings window, to drive it as its own window.
fn settings_context(
    settings: &WindowHandle<SettingsWindow>,
    cx: &mut VisualTestContext,
) -> VisualTestContext {
    VisualTestContext::from_window(AnyWindowHandle::from(*settings), &cx.cx)
}

/// The label of the node assistive technology treats as focused in the
/// window `cx` drives.
fn focused_label(cx: &mut VisualTestContext) -> Option<String> {
    let (label, _) = accessibility(cx);
    label
}

/// The window's accessibility tree as (focused label, raw JSON), forced on
/// so the tree is built regardless of platform accessibility.
fn accessibility(cx: &mut VisualTestContext) -> (Option<String>, String) {
    cx.update(|window, _| window.set_a11y_forced(true));
    cx.run_until_parked();
    let json = cx
        .update(|window, _| window.debug_a11y_tree_json())
        .expect("an accessibility tree");
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes = tree["nodes"].as_object().unwrap();
    let field = |node: &serde_json::Value, key: &str| {
        node["aria"][key].as_str().unwrap_or_default().to_owned()
    };
    let focused = ["active_descendant_focus", "gpui_focus"]
        .iter()
        .find_map(|key| tree[key].as_str())
        .map(|id| field(&nodes[id], "label"));
    (focused, json)
}

/// Runs `cx` until `done` returns a value, so that work arriving from
/// other threads (a link opening) has been drawn.
fn until<T>(
    cx: &mut VisualTestContext,
    mut done: impl FnMut(&mut VisualTestContext) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if let Some(value) = done(cx) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the window to draw"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// One of the theme's panel colors, as the window paints it on a quad:
/// the solid panel, or the glass tint over the window's blur. The values
/// mirror `ui::theme`'s dark and light palettes — the solid panel and
/// the glass tint of each.
fn panel(hex: u32) -> gpui::Background {
    gpui::solid_background(gpui::rgb_to_hsla(gpui::rgba(hex)))
}

/// The dark theme's panel colors: the solid panel and the glass tint.
fn dark_panel() -> [gpui::Background; 2] {
    [panel(0x16171AFF), panel(0x16171AB3)]
}

/// The light theme's panel colors: the solid panel and the glass tint.
fn light_panel() -> [gpui::Background; 2] {
    [panel(0xF6F6F8FF), panel(0xF6F6F8CC)]
}

/// Whether the window `cx` drives painted one of the panel `colors` in
/// its last frame — the panel surface, which follows the theme and
/// material the host settings hold.
fn paints_panel(cx: &mut VisualTestContext, colors: &[gpui::Background]) -> bool {
    cx.update(|window, _| {
        window
            .painted_quads()
            .iter()
            .any(|quad| colors.contains(&quad.background))
    })
}

/// Whether the Appearance page's RadioButton named `label` is the choice
/// in effect, as assistive technology reads it.
fn chosen(cx: &mut VisualTestContext, label: &str) -> bool {
    let (_, json) = accessibility(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    tree["nodes"].as_object().unwrap().values().any(|node| {
        let aria = &node["aria"];
        aria["role"] == "RadioButton" && aria["label"] == label && aria["toggled"] == "True"
    })
}

/// Clicks the Appearance page's choice whose debug selector is
/// `selector`, through the page's own control.
fn choose(cx: &mut VisualTestContext, selector: &'static str) {
    let choice = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"));
    cx.simulate_click(choice.center(), Modifiers::none());
}

/// Opens the Settings window over the launcher `cx` drives, on the
/// Appearance page — one sidebar section from the General page the
/// window opens on — as its own window context. Every caller of this
/// helper drives the Appearance page; the General page's own tests live
/// in `open_pane.rs`.
fn open_settings(cx: &mut VisualTestContext) -> VisualTestContext {
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    let appearance = settings_cx
        .debug_bounds("section-Appearance")
        .expect("the Appearance section");
    settings_cx.simulate_click(appearance.center(), Modifiers::none());
    settings_cx.run_until_parked();
    settings_cx
}

/// Opens the Settings window over the launcher `cx` drives, on the
/// General page it first shows, as its own window context: the page the
/// Open Pane hotkey's recorder and the launch-at-login switch live on.
fn general_page(cx: &mut VisualTestContext) -> VisualTestContext {
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    settings_context(&settings, cx)
}

/// Runs the window until the settings record exists in `data`: the save
/// the Appearance page started is written off the window's thread.
fn until_record(cx: &mut VisualTestContext, data: &std::path::Path) {
    let record = data.join("settings.json");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !record.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the record to be written"
        );
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[gpui::test]
fn the_three_entry_points_converge_on_one_focused_settings_window(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);

    // The Settings root result opens the window.
    cx.simulate_input("settings");
    settle(&launcher, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let first = settings_windows(cx)
        .pop()
        .expect("the root result opened Settings");
    assert_eq!(settings_windows(cx).len(), 1, "no second window appeared");
    assert!(cx.cx.update(|cx| first.is_active(cx)).unwrap_or(false));

    // The local shortcut focuses the same window.
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx), vec![first], "the shortcut focused it");

    // The ellipsis menu's Settings entry, too.
    let button = cx.debug_bounds("footer-menu").expect("the menu button");
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    let item = cx
        .debug_bounds("menu-item-Settings")
        .expect("the menu item");
    cx.simulate_click(item.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx), vec![first], "the menu focused it");

    // All three ways left the launcher where it was.
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.status, Status::Idle);
}

#[gpui::test]
fn closing_settings_reopens_a_new_window_without_ending_the_launcher(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);
    let launcher_window = cx.update(|window, _| window.window_handle());

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    // The test platform's quit is a no-op, so the application staying
    // alive is what closing Settings must leave observable: the launcher
    // window stays and still works. (Closing the launcher itself quits
    // Pane in the binary, whose close hook compares the window ids; the
    // same behavior is checked natively in docs/evidence/settings-72/.)
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx).len(), 0, "Settings closed");
    assert!(
        cx.windows().contains(&launcher_window),
        "the launcher window stayed"
    );

    // The launcher still works: its rows still open.
    cx.simulate_keystrokes("enter");
    let view = settle(&launcher, cx);
    assert_eq!(view.screen, Screen::Command, "the launcher answers");

    // Reopening makes a new window, focused.
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let second = settings_windows(cx).pop().expect("Settings reopened");
    assert_ne!(second, settings, "a new window, not the closed one");
    assert!(cx.cx.update(|cx| second.is_active(cx)).unwrap_or(false));
}

#[gpui::test]
fn hiding_the_launcher_leaves_settings_open_and_usable(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    let launcher_window = cx.update(|window, _| window.window_handle());

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");

    // Hiding the launcher window — as its later global hotkey will —
    // closes nothing: Settings' lifetime is its own.
    cx.cx
        .update_window(launcher_window, |_, window, _| window.set_visible(false))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(settings_windows(cx), vec![settings], "Settings stayed");

    // And it still answers: the page the window first shows — General,
    // the first registered section — is drawn, with both of the choices
    // that live on it: the Open Pane hotkey's recorder and the
    // launch-at-login switch.
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds("open-pane-recorder").is_some());
    assert!(
        settings_cx
            .debug_bounds("general-launch-at-login")
            .is_some()
    );
    // The Appearance page — one sidebar section away — answers too.
    let appearance = settings_cx
        .debug_bounds("section-Appearance")
        .expect("the Appearance section");
    settings_cx.simulate_click(appearance.center(), Modifiers::none());
    settings_cx.run_until_parked();
    assert!(
        settings_cx
            .debug_bounds("appearance-theme-System")
            .is_some()
    );
    // And the Extensions page — a sidebar section away — answers too:
    // this launcher installs no packages, so the page lists none and
    // offers no install rows.
    let extensions = settings_cx
        .debug_bounds("section-Extensions")
        .expect("the Extensions section");
    settings_cx.simulate_click(extensions.center(), Modifiers::none());
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds("extensions-title").is_some());
    assert!(settings_cx.debug_bounds("extension-empty").is_some());
    assert!(
        settings_cx
            .debug_bounds("extension-install-Install extension from npm…")
            .is_none(),
        "a launcher that installs no packages offers no install rows"
    );
}

#[gpui::test]
fn keys_in_settings_and_the_launcher_stay_in_their_windows(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);

    // Typing in the launcher narrows its results and touches nothing in
    // Settings, which shows its own page (the General page it opened
    // on here).
    cx.simulate_input("rust");
    settle(&launcher, cx);
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert_eq!(view.query(), Some("rust"));
    settings_cx.run_until_parked();
    assert!(
        settings_cx
            .debug_bounds("general-launch-at-login")
            .is_some(),
        "the General page is unchanged"
    );

    // Keys in Settings — the sidebar's navigation, over the sections
    // it offers — reach no launcher key: the query stays, no selection
    // moves. The pages draw their readings of the launcher without
    // entering it, so even the sections the sidebar navigates through
    // move nothing.
    settings_cx.simulate_keystrokes("down up enter");
    settings_cx.run_until_parked();
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert_eq!(view.query(), Some("rust"), "the query was untouched");
    assert_eq!(view.selected, Some(0), "no row was selected");
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );

    // And what Settings shows is still its own page: the General page
    // it opened on (the two keys end where they began), whose content is
    // drawn — not the launcher's — with both of the choices that live on
    // it.
    assert!(
        settings_cx.debug_bounds("general").is_some(),
        "the page is drawn"
    );
    assert!(
        settings_cx.debug_bounds("open-pane-recorder").is_some(),
        "the hotkey's row is drawn"
    );
    assert!(
        settings_cx
            .debug_bounds("general-launch-at-login")
            .is_some(),
        "the General page is unchanged"
    );
    assert_eq!(
        settings_cx.debug_bounds("section-About").map(|_| "About"),
        Some("About"),
        "the About section is still offered in the sidebar"
    );

    // The Appearance page — one sidebar section away — answers too, and
    // walking to it reaches no launcher key either.
    let appearance = settings_cx
        .debug_bounds("section-Appearance")
        .expect("the Appearance section");
    settings_cx.simulate_click(appearance.center(), Modifiers::none());
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("appearance").is_some(),
        "the Appearance page is drawn"
    );

    // The launcher's keys, in turn, never reach Settings.
    cx.simulate_keystrokes("escape");
    settle(&launcher, cx);
    let query = cx.read_entity(&launcher, |window, _| {
        window.launcher().view().query().map(str::to_owned)
    });
    assert_eq!(
        query,
        Some("".into()),
        "the launcher's Escape cleared its own query"
    );
}

#[gpui::test]
fn the_footer_menu_opens_traverses_dismisses_and_restores_focus(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);
    assert!(
        cx.debug_bounds("footer-menu").is_some(),
        "the menu button is the footer's leftmost control"
    );
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("Rust sample"),
        "root search's selected result has focus"
    );

    // The keyboard reaches the menu: Tab from the query field, then Enter
    // presses the button.
    cx.simulate_keystrokes("tab");
    assert_eq!(focused_label(cx).as_deref(), Some("More actions"));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_some(), "the menu is open");
    // Focus moved into it, onto its one item.
    assert_eq!(focused_label(cx).as_deref(), Some("Settings"));

    // The popup overlays the list: it sits above the footer strip, over
    // the results.
    let menu = cx.debug_bounds("menu").expect("the menu");
    let footer = cx.debug_bounds("status-idle").expect("the footer");
    assert!(
        menu.bottom() <= footer.top(),
        "the menu is above the footer"
    );

    // Escape dismisses it, restoring the focus it took — the menu's
    // button, which had focus when the menu opened, back in command of
    // the footer.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_none(), "the menu is closed");
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("More actions"),
        "focus is restored to what had it: the menu's button"
    );
    let view = settle(&launcher, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert_eq!(view.status, Status::Idle, "nothing was activated");

    // Enter activates the menu's selected entry — the keyboard path to
    // the Settings window, converging on the same window as the other
    // entry points. Focus is back on the button, so Enter opens the menu
    // again.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_some(), "the menu is open again");
    assert_eq!(focused_label(cx).as_deref(), Some("Settings"));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let settings = settings_windows(cx)
        .pop()
        .expect("the menu's item opened the Settings window");
    assert!(cx.debug_bounds("menu").is_none(), "the menu closed with it");
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("More actions"),
        "focus is restored to what had it: the menu's button"
    );
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx).len(), 0);

    // An outside click dismisses it the same way, and consumes the click:
    // the launcher result underneath is not activated.
    let button = cx.debug_bounds("footer-menu").expect("the menu button");
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_some());
    let row = cx.debug_bounds("row-Rust sample").expect("a result row");
    cx.simulate_click(row.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_none(), "the menu closed");
    let view = settle(&launcher, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "no row opened");
    assert_eq!(view.status, Status::Idle);
    assert_eq!(view.selected, Some(0), "the selection did not move");
    assert_eq!(settings_windows(cx).len(), 0, "no Settings window opened");
}

#[gpui::test]
fn the_menu_button_toggles_and_assistive_technology_sees_it_named(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);

    // Clicking the button opens; clicking it again with the menu open
    // closes it — the popup's outside-click dismissal consumes the second
    // click before the button can reopen it.
    let button = cx.debug_bounds("footer-menu").expect("the menu button");
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_some());
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("menu").is_none(),
        "the button toggled closed"
    );

    // The button and its item are named controls, with the open state.
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    let (_, json) = accessibility(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes: Vec<&serde_json::Value> = tree["nodes"].as_object().unwrap().values().collect();
    let button = nodes
        .iter()
        .find(|node| node["aria"]["role"] == "Button" && node["aria"]["label"] == "More actions")
        .expect("the menu button is named");
    assert_eq!(
        button["aria"]["expanded"],
        serde_json::json!(true),
        "the open state is announced"
    );
    assert!(
        nodes
            .iter()
            .any(|node| node["aria"]["role"] == "Menu" && node["aria"]["label"] == "More actions"),
        "the menu is announced"
    );
    assert!(
        nodes
            .iter()
            .any(|node| node["aria"]["role"] == "MenuItem" && node["aria"]["label"] == "Settings"),
        "the menu's item is announced"
    );

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let view = settle(&launcher, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
}

#[gpui::test]
fn the_about_page_shows_the_real_version_and_opens_the_documentation(cx: &mut TestAppContext) {
    let (launcher, links, cx) = open_launcher(cx);
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    // The window opens on the General page; the About page is reached
    // through the sidebar.
    settings_cx.run_until_parked();
    let about = settings_cx
        .debug_bounds("section-About")
        .expect("the About section");
    settings_cx.simulate_click(about.center(), Modifiers::none());
    settings_cx.run_until_parked();

    // The version is the real one this build runs, drawn and announced.
    assert!(
        settings_cx.debug_bounds("about-version").is_some(),
        "the version row is drawn"
    );
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains(&format!("\"Pane {APP_VERSION}\"")),
        "the real version is rendered"
    );

    // The documentation entry opens Pane's repository with the launcher's
    // link opener, off the window's thread, and reports what happened.
    let link = settings_cx
        .debug_bounds("about-documentation")
        .expect("the documentation entry");
    settings_cx.simulate_click(link.center(), Modifiers::none());
    until(&mut settings_cx, |cx| {
        cx.debug_bounds("about-status").map(|_| ())
    });
    assert_eq!(
        links.0.lock().unwrap().as_slice(),
        ["https://github.com/hoangvu12/pane"],
        "the repository documentation was opened"
    );
    // Opening it left the launcher where it was.
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
}

#[gpui::test]
fn a_refused_documentation_link_is_explained_on_the_page(cx: &mut TestAppContext) {
    let (launcher, cx) = open_refusing(cx);

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();
    // The About page is a few sidebar sections away from the one the window
    // opens on.
    let about = settings_cx
        .debug_bounds("section-About")
        .expect("the About section");
    settings_cx.simulate_click(about.center(), Modifiers::none());
    settings_cx.run_until_parked();

    let link = settings_cx
        .debug_bounds("about-documentation")
        .expect("the documentation entry");
    settings_cx.simulate_click(link.center(), Modifiers::none());
    until(&mut settings_cx, |cx| {
        cx.debug_bounds("about-status").map(|_| ())
    });

    // The refusal is the page's status, not a silent failure.
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains(
            "Could not open the documentation: no program to open web links is installed"
        ),
        "the refusal is explained, {json}"
    );
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert_eq!(view.status, Status::Idle, "the launcher is untouched");
}

/// Runs the window until its accessibility tree contains `text`, so what
/// is waited for is a drawn state, not a reading of the launcher.
fn until_text(cx: &mut VisualTestContext, text: &str) {
    // Generous, as the window tests are: a build on the development
    // thread, and CI's runners, are slow.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let (_, json) = accessibility(cx);
        if json.contains(text) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {text:?} to be drawn, {json}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A development builder that fakes a build: it copies the guest that the
/// folder's `source.txt` names, or fails when the source names an error, as
/// `develop.rs`'s does, so a build can fail in the background without any
/// real toolchain.
struct FakeBuilder;

struct FakeBuild(PathBuf);

impl Builder for FakeBuilder {
    fn build_for(&self, folder: &Path) -> Result<Arc<dyn Build>, String> {
        Ok(Arc::new(FakeBuild(folder.to_path_buf())))
    }
}

impl Build for FakeBuild {
    fn command(&self) -> String {
        "fake build".into()
    }

    fn ignores(&self, path: &Path) -> bool {
        path == Path::new("hello.wasm")
    }

    fn run(&self, job: &BuildJob) -> BuildOutcome {
        let source = fs::read_to_string(self.0.join("source.txt")).unwrap();
        let source = source.trim();
        if source.starts_with("error") {
            job.line(source);
            return BuildOutcome::Failed("fake build failed".into());
        }
        fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/guests")
                .join(format!("{source}.wasm")),
            job.staging().join("hello.wasm"),
        )
        .unwrap();
        BuildOutcome::Built
    }
}

#[gpui::test]
fn the_extensions_page_lists_the_installed_extensions_and_their_reach(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    let (launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);

    // The page lists the package with its state and identity, the
    // management rows the launcher's own list holds, the package's
    // commands, and the launcher's install rows.
    for row in [
        "extension-row-Settings sample",
        "extension-row-Reload Settings sample",
        "extension-row-Clear cache of Settings sample",
        "extension-row-Uninstall Settings sample",
        "extension-row-Hotkey for Greeting",
        "extension-row-Alias for Greeting",
        "extension-row-Develop Settings sample",
        "extension-row-Update extensions automatically",
        "extension-command-Greeting",
        "extension-install-Install extension from folder…",
        "extension-install-Install extension from npm…",
        "extension-install-Install extension from Git…",
    ] {
        assert!(settings_cx.debug_bounds(row).is_some(), "{row} is drawn");
    }
    // The package's state and identity are announced, from the same
    // subtitle the launcher's own list holds.
    let (_, json) = accessibility(&mut settings_cx);
    assert!(json.contains("Enabled · "), "the state shows, {json}");

    // Reading the page moved nothing: the launcher stayed where it was,
    // with the install's own outcome still on it.
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(
        view.status,
        Status::Result("Installed Settings sample".into())
    );
}

#[gpui::test]
fn disabling_a_required_extension_from_the_page_confirms_and_disables_all(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    operations_package(&sources.path().join("greeter"), "Greeter", "");
    let caller = operations_package(
        &sources.path().join("caller"),
        "Caller",
        r#"{ "id": "greeter", "source": "local:../greeter",
             "operations": [{ "id": "echo", "version": 1 }] }"#,
    );
    let (launcher, cx) = open_installed(cx, &data, &caller);
    let (_settings, mut settings_cx) = open_extensions(cx);

    // The page's row for the dependency, clicked, enters the launcher's
    // own flow and asks the required-dependent confirmation there — the
    // same question, rows and records the launcher's list asks.
    click_row(&mut settings_cx, "extension-row-Greeter");
    for row in ["extension-row-Disable all 2", "extension-row-Cancel"] {
        assert!(settings_cx.debug_bounds(row).is_some(), "{row} is drawn");
    }
    assert!(
        settings_cx
            .debug_bounds(
                "extension-detail-These extensions require Greeter, directly or through each \
                 other, and cannot work without it, so they are disabled with it:"
            )
            .is_some(),
        "the dependents are listed"
    );

    // Cancel keeps everything enabled and returns to the list.
    click_row(&mut settings_cx, "extension-row-Cancel");
    assert!(
        settings_cx.debug_bounds("extension-row-Greeter").is_some(),
        "back on the list"
    );
    assert!(
        settings_cx.debug_bounds("extensions-status").is_none(),
        "nothing was done"
    );
    let enabled = |cx: &mut VisualTestContext| {
        cx.read_entity(&launcher, |window, _| {
            window
                .launcher()
                .packages()
                .into_iter()
                .map(|package| package.enabled)
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(enabled(cx), [true, true]);

    // Disable all does what it says, through the same one-write change the
    // launcher's confirmation makes.
    click_row(&mut settings_cx, "extension-row-Greeter");
    click_row(&mut settings_cx, "extension-row-Disable all 2");
    until_text(
        &mut settings_cx,
        "Disabled Greeter and Caller, which requires it",
    );
    assert_eq!(enabled(cx), [false, false]);

    // The launcher window shares the flow: it shows the extension list
    // too, and the page's status is the launcher's own outcome.
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert!(
        matches!(view.screen, Screen::Extensions { .. }),
        "{:?}",
        view.screen
    );
    // Root search offers the commands of neither disabled package.
    cx.read_entity(&launcher, |window, _| window.launcher().back());
    assert_eq!(
        titles(&launcher, cx),
        [
            "Install extension from folder…",
            "Install extension from npm…",
            "Install extension from Git…",
            "Manage extensions…",
            "Settings…"
        ]
    );
}

/// Whether the settings file keeps settings for `key`. The file is parsed,
/// since JSON escapes the backslashes of a Windows path in a key.
fn keeps_settings(settings: &Path, key: &str) -> bool {
    let text = fs::read_to_string(settings).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    saved["packages"].get(key).is_some()
}

#[gpui::test]
fn uninstalling_from_the_page_offers_the_saved_data_choice_and_keeps_it(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    // A setting the package saved earlier, as its commands would.
    let key = PackageIdentity::local(&folder).unwrap().key();
    let extensions = data.path().join("extensions");
    let settings = extensions.join("settings.json");
    fs::create_dir_all(&extensions).unwrap();
    let saved = serde_json::json!({ "version": 1, "packages": { &key: { "style": "formal" } } });
    fs::write(&settings, saved.to_string()).unwrap();
    let (launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);

    // The uninstall row asks first, with the saved-data choice the
    // launcher's own confirmation offers.
    click_row(&mut settings_cx, "extension-row-Uninstall Settings sample");
    for row in [
        "extension-row-Uninstall and keep saved data",
        "extension-row-Uninstall and delete saved data",
        "extension-row-Cancel",
    ] {
        assert!(settings_cx.debug_bounds(row).is_some(), "{row} is drawn");
    }
    assert!(
        settings_cx
            .debug_bounds("extension-detail-Saved data: 1 setting")
            .is_some(),
        "what is kept is listed"
    );

    // Keeping the saved data uninstalls without running the extension and
    // keeps the settings, as the same choice in the launcher does.
    click_row(
        &mut settings_cx,
        "extension-row-Uninstall and keep saved data",
    );
    until_text(
        &mut settings_cx,
        "Uninstalled Settings sample; its settings and content are kept",
    );
    assert!(keeps_settings(&settings, &key), "the saved data is kept");
    // Nothing is installed; the retained data is listed for the same
    // identity, with its own row and confirmation, as in the launcher.
    assert!(
        cx.read_entity(&launcher, |window, _| window.launcher().packages())
            .is_empty(),
        "nothing is installed"
    );
    assert!(
        settings_cx
            .debug_bounds("extension-row-Delete retained data of Settings sample")
            .is_some(),
        "the retained data is listed"
    );
    let source = folder.join("pane.json");
    assert!(source.exists(), "the source folder is kept");
}

#[gpui::test]
fn a_reload_that_fails_to_start_is_explained_on_the_page_and_offers_retry(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = hello_package(&sources.path().join("hello"));
    let (_launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);

    // A source that no longer builds a startable package: the reload's
    // failure is the page's status, and the paused package's Retry row is
    // offered, as in the launcher's list.
    rebuild(&folder, "failing_start");
    click_row(&mut settings_cx, "extension-row-Reload Hello");
    until_text(&mut settings_cx, "Reloaded Hello, but it failed to start");
    assert!(
        settings_cx
            .debug_bounds("extension-row-Retry starting Hello")
            .is_some(),
        "the recovery row is offered"
    );

    // Retry starts it again, through the same row the launcher's list
    // holds.
    click_row(&mut settings_cx, "extension-row-Retry starting Hello");
    until_text(&mut settings_cx, "Started Hello");
    assert!(
        settings_cx
            .debug_bounds("extension-row-Retry starting Hello")
            .is_none(),
        "the package is no longer paused"
    );
}

#[gpui::test]
fn opening_an_extensions_command_from_the_page_summons_the_launcher(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    let (launcher, cx) = open_installed(cx, &data, &folder);
    let launcher_window = cx
        .update(|window, _| window.window_handle())
        .downcast::<LauncherWindow>()
        .expect("the launcher window");
    let (_settings, mut settings_cx) = open_extensions(cx);

    // The settings sample's command is an extension-owned settings
    // command: the page opens it where it lives — in the launcher window,
    // summoned and focused — not a form of its own.
    click_row(&mut settings_cx, "extension-command-Greeting");
    let view = settle(&launcher, cx);
    assert_eq!(view.screen, Screen::Command);
    assert_eq!(view.title, "Greeting");
    assert!(
        cx.cx
            .update(|cx| launcher_window.is_active(cx))
            .unwrap_or(false),
        "the launcher window took focus"
    );

    // The Settings window stayed open, its page back to a reading: the
    // launcher left the extension flow when the command opened.
    assert!(
        settings_cx
            .debug_bounds("extension-row-Settings sample")
            .is_some(),
        "the page is still drawn"
    );
}

#[gpui::test]
fn the_install_rows_from_the_page_open_the_launcher_windows_flows(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    let (launcher, cx) = open_installed(cx, &data, &folder);
    let launcher_window = cx
        .update(|window, _| window.window_handle())
        .downcast::<LauncherWindow>()
        .expect("the launcher window");
    let (_settings, mut settings_cx) = open_extensions(cx);

    // The install rows are the launcher's own root rows: clicking one opens
    // the flow in the launcher window, where its form (or folder picker)
    // lives, focused there.
    click_row(
        &mut settings_cx,
        "extension-install-Install extension from npm…",
    );
    let view = settle(&launcher, cx);
    assert!(matches!(view.screen, Screen::Form(_)), "{:?}", view.screen);
    assert!(
        cx.debug_bounds("field-package").is_some(),
        "the form is drawn"
    );
    assert!(
        cx.cx
            .update(|cx| launcher_window.is_active(cx))
            .unwrap_or(false),
        "the launcher window took focus"
    );
    // The Settings window stayed open.
    assert!(settings_cx.debug_bounds("extensions-title").is_some());
}

#[gpui::test]
fn the_page_follows_a_change_the_launcher_window_made(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    let (launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);

    // The launcher window enters its own Manage extensions flow and
    // disables the package: the page, open on the Extensions page,
    // redraws with the state the launcher now holds.
    cx.simulate_input("manage");
    settle(&launcher, cx);
    cx.simulate_keystrokes("enter");
    settle(&launcher, cx);
    cx.simulate_keystrokes("enter");
    settle(&launcher, cx);
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("Disabled · "),
        "the page shows the new state, {json}"
    );
    // A disabled package's command rows are gone from the page too.
    assert!(
        settings_cx
            .debug_bounds("extension-command-Greeting")
            .is_none(),
        "the command is no longer offered"
    );
}

#[gpui::test]
fn the_page_follows_a_background_build_failure_by_itself(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = hello_package(&sources.path().join("hello"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (sender, changes) = pane_core::changes::channel();
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_development(Arc::new(FakeBuilder), sender);
    install(&launcher, &folder);
    let (_launcher, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher
    });
    let (_settings, mut settings_cx) = open_extensions(cx);

    // Development starts from the page: the same row the launcher's list
    // holds, entered through the same flow.
    click_row(&mut settings_cx, "extension-row-Develop Hello");
    until_text(&mut settings_cx, "Developing Hello");

    // A save that does not build: the development thread reports it, the
    // changes channel wakes the launcher window, and the page redraws with
    // what the launcher holds — by itself, with no action on it.
    fs::write(folder.join("source.txt"), "error: expected `;`").unwrap();
    until_text(&mut settings_cx, "Hello did not build: error: expected `;`");
    assert!(
        settings_cx
            .debug_bounds("extension-row-Why Hello did not build")
            .is_some(),
        "the build-failure row is drawn"
    );
}

#[gpui::test]
fn the_settings_window_keeps_its_layout_at_small_sizes(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();
    // The window opens on the General page; the Appearance page — the
    // demanding one — is one sidebar click away.
    let appearance = settings_cx
        .debug_bounds("section-Appearance")
        .expect("the Appearance section");
    settings_cx.simulate_click(appearance.center(), Modifiers::none());
    settings_cx.run_until_parked();

    // The window's floor: at its smallest usable size the sidebar and the
    // page both stay laid out — nothing reaches past the panel's edge.
    settings_cx.simulate_resize(gpui::size(px(560.), px(400.)));
    settings_cx.run_until_parked();
    let sidebar = settings_cx
        .debug_bounds("section-Appearance")
        .expect("the sidebar is laid out");
    let page = settings_cx
        .debug_bounds("settings-page")
        .expect("the page is laid out");
    let choice = settings_cx
        .debug_bounds("appearance-theme-System")
        .expect("the page's choice row is laid out");
    let preview = settings_cx
        .debug_bounds("appearance-preview")
        .expect("the preview is laid out");
    assert!(
        sidebar.right() <= page.left(),
        "the sidebar is beside the page"
    );
    assert!(
        choice.right() <= page.right(),
        "the choices stay within the page"
    );
    // The page scrolls when the window is short, so vertical position is
    // not containment; the preview must stay within the page's width.
    assert!(
        preview.right() <= page.right(),
        "the preview stays within the page's width"
    );

    // The About page keeps its own rows laid out at the same floor,
    // reached through the sidebar.
    let about = settings_cx
        .debug_bounds("section-About")
        .expect("the About section");
    settings_cx.simulate_click(about.center(), Modifiers::none());
    settings_cx.run_until_parked();
    let sidebar = settings_cx
        .debug_bounds("section-About")
        .expect("the sidebar is laid out");
    let page = settings_cx
        .debug_bounds("settings-page")
        .expect("the page is laid out");
    let version = settings_cx
        .debug_bounds("about-version")
        .expect("the version row is laid out");
    assert!(
        sidebar.right() <= page.left(),
        "the sidebar is beside the page"
    );
    assert!(
        version.right() <= page.right(),
        "the version stays within the page"
    );
}

/// Windows-only: the titlebar's painted caption buttons, whose window
/// control areas route to the system's close, minimize and maximize.
#[cfg(target_os = "windows")]
#[gpui::test]
fn the_titlebars_window_controls_close_only_the_settings_window(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);
    let launcher_window = cx.update(|window, _| window.window_handle());

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();

    // The three platform controls are there, and named.
    assert!(settings_cx.debug_bounds("window-minimize").is_some());
    assert!(settings_cx.debug_bounds("window-maximize").is_some());
    let close = settings_cx
        .debug_bounds("window-close")
        .expect("the close button");
    let (_, json) = accessibility(&mut settings_cx);
    for label in ["Minimize", "Maximize", "Close"] {
        assert!(
            json.contains(&format!("\"label\": \"{label}\"")),
            "the {label} control is named"
        );
    }

    // Clicking close closes only the Settings window: on Windows the
    // system takes the click through the hit test; on the test platform
    // the same behavior comes from the button's own handler.
    settings_cx.simulate_click(close.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(settings_windows(cx).len(), 0, "Settings closed");
    assert!(
        cx.windows().contains(&launcher_window),
        "the launcher window stayed"
    );

    // The launcher still answers.
    cx.simulate_keystrokes("escape");
    let view = settle(&launcher, cx);
    assert!(matches!(view.screen, Screen::Root { .. }));
}

#[gpui::test]
fn a_missing_record_starts_from_the_reference_defaults(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);

    // No record exists: the reference's dark palette and the glass
    // material are what the defaults name, and nothing is written until a
    // choice is made.
    assert!(paints_panel(cx, &dark_panel()), "the dark panel is drawn");
    let mut settings_cx = open_settings(cx);
    assert!(chosen(&mut settings_cx, "Dark"), "Dark is in effect");
    assert!(chosen(&mut settings_cx, "Glass"), "Glass is in effect");
    assert!(!data.path().join("settings.json").exists());
}

#[gpui::test]
fn choosing_a_theme_re_renders_both_windows_and_the_preview(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);
    let mut settings_cx = open_settings(cx);

    // The Light choice, taken through the page's own control: no
    // restart, no second window — both windows re-render with it at
    // once.
    choose(&mut settings_cx, "appearance-theme-Light");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(
        paints_panel(cx, &light_panel()),
        "the launcher paints the light palette"
    );
    assert!(
        paints_panel(&mut settings_cx, &light_panel()),
        "Settings paints the light palette"
    );
    assert!(
        !paints_panel(cx, &dark_panel()) && !paints_panel(&mut settings_cx, &dark_panel()),
        "no dark panel remains in either window"
    );
    assert!(
        chosen(&mut settings_cx, "Light"),
        "the page follows its own choice"
    );

    // The preview is drawn with the choice in effect: it is laid out on
    // the page, and the page paints the light surface. (Whether the
    // preview's own panel quad reaches the painted scene depends on the
    // page's scroll and the platform's culling of fully-clipped quads, so
    // the preview is asserted by its layout, not by its quad.)
    assert!(
        settings_cx.debug_bounds("appearance-preview").is_some(),
        "the preview is laid out beside the light choice"
    );

    // Dark returns the same way.
    choose(&mut settings_cx, "appearance-theme-Dark");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(paints_panel(cx, &dark_panel()));
    assert!(
        !paints_panel(cx, &light_panel()),
        "no light panel remains in the launcher"
    );
    until_record(cx, data.path());
}

#[gpui::test]
fn the_system_choice_renders_the_appearance_the_system_reports(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);
    let mut settings_cx = open_settings(cx);

    choose(&mut settings_cx, "appearance-theme-System");
    cx.run_until_parked();
    settings_cx.run_until_parked();

    // The system's own appearance is what renders: the test platform
    // reports the light one, so both windows follow it. (The platform's
    // change *notification* — the observer that re-renders a following
    // theme when the operating system switches — cannot be driven from
    // this harness: GPUI's test window hides its simulation behind a
    // crate-private API. The entity's half is covered by the unit tests
    // in `pane::settings`; the notification itself is native validation,
    // recorded in docs/evidence/settings-73/.)
    assert!(
        paints_panel(cx, &light_panel()),
        "the system's light is followed"
    );
    assert!(paints_panel(&mut settings_cx, &light_panel()));
    assert!(chosen(&mut settings_cx, "System"));
    until_record(cx, data.path());
}

#[gpui::test]
fn the_material_choice_switches_the_panel_surface(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);
    let mut settings_cx = open_settings(cx);

    // The light palette first, so the two materials differ by surface.
    choose(&mut settings_cx, "appearance-theme-Light");
    cx.run_until_parked();

    // The solid material: the opaque window's solid panel, the same on
    // every platform.
    choose(&mut settings_cx, "appearance-material-Solid");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(
        paints_panel(cx, &[panel(0xF6F6F8FF)]),
        "the solid panel is drawn"
    );
    assert!(
        !paints_panel(cx, &[panel(0xF6F6F8CC)]),
        "no glass tint remains"
    );

    // Glass returns: the tint where the platform provides frost, the
    // solid surface where it does not (Linux; a Windows with transparency
    // off) — either way the page's note under the group explains the
    // truth that holds, and the row carries the preference.
    choose(&mut settings_cx, "appearance-material-Glass");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(
        paints_panel(cx, &light_panel()),
        "one of the two surfaces is drawn"
    );
    assert!(
        chosen(&mut settings_cx, "Glass"),
        "the glass preference is held"
    );
    assert!(
        settings_cx
            .debug_bounds("appearance-material-note")
            .is_some(),
        "the material's truth is explained"
    );
    #[cfg(target_os = "linux")]
    {
        let (_, json) = accessibility(&mut settings_cx);
        assert!(
            json.contains("Glass is unavailable here"),
            "the fallback is named, {json}"
        );
    }

    // The solid surface needs no note.
    choose(&mut settings_cx, "appearance-material-Solid");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(
        settings_cx
            .debug_bounds("appearance-material-note")
            .is_none(),
        "nothing to explain about the solid surface"
    );
    until_record(cx, data.path());
}

#[gpui::test]
fn the_saved_choice_is_reloaded_by_a_fresh_application(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);
    let mut settings_cx = open_settings(cx);

    // A user-visible change, through the page's own controls.
    choose(&mut settings_cx, "appearance-theme-Light");
    choose(&mut settings_cx, "appearance-material-Solid");
    cx.run_until_parked();
    until_record(cx, data.path());

    // A fresh application over the same data folder: a new app, nothing
    // carried over but the executors, the settings read from the record
    // alone.
    let mut fresh = cx.cx.new_app();
    fresh.update(pane::bind_keys);
    fresh.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    let launcher = Launcher::new(Runtime::start(), Vec::new());
    let (_window, fresh_cx) =
        fresh.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    fresh_cx.run_until_parked();

    // It renders the last choice the record holds, not the defaults.
    assert!(
        paints_panel(fresh_cx, &[panel(0xF6F6F8FF)]),
        "the saved light, solid choice is reloaded"
    );
    assert!(!paints_panel(fresh_cx, &dark_panel()));

    // And its own Settings page says the same: what was saved is what is
    // shown, as assistive technology reads it.
    let mut fresh_settings = open_settings(fresh_cx);
    fresh_settings.run_until_parked();
    assert!(chosen(&mut fresh_settings, "Light"));
    assert!(chosen(&mut fresh_settings, "Solid"));
}

#[gpui::test]
fn a_failed_save_is_reported_and_the_shown_choice_stays_what_was_saved(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);
    let mut settings_cx = open_settings(cx);

    // A choice that saves, so the record holds it.
    choose(&mut settings_cx, "appearance-theme-Light");
    cx.run_until_parked();
    until_record(cx, data.path());

    // Break the record's replacement: a folder where the record belongs,
    // so the atomic write cannot rename over it.
    std::fs::remove_file(data.path().join("settings.json")).unwrap();
    std::fs::create_dir(data.path().join("settings.json")).unwrap();

    // A choice that cannot be saved.
    choose(&mut settings_cx, "appearance-theme-Dark");
    cx.run_until_parked();
    settings_cx.run_until_parked();

    // The failure is the page's status, on screen and announced.
    assert!(
        settings_cx.debug_bounds("appearance-status").is_some(),
        "the failure is drawn"
    );
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("Pane could not save your choice"),
        "the failure is explained, {json}"
    );

    // The shown choice stays what was actually saved — not Dark, which
    // could not be written: the page shows what a fresh start would
    // reload.
    assert!(
        chosen(&mut settings_cx, "Light"),
        "the saved choice is shown"
    );
    assert!(
        paints_panel(cx, &light_panel()),
        "the windows keep the saved choice"
    );
}

#[gpui::test]
fn an_unreadable_record_is_reported_and_never_replaced(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let garbage = "{ not the settings record";
    std::fs::write(data.path().join("settings.json"), garbage).unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);

    // Startup is not prevented: the defaults stand in.
    assert!(paints_panel(cx, &dark_panel()), "the dark default is drawn");
    let mut settings_cx = open_settings(cx);

    // The page says choices are not saved, and why.
    assert!(
        settings_cx.debug_bounds("appearance-status").is_some(),
        "the problem is drawn"
    );
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("Pane could not read the settings record"),
        "the problem is explained, {json}"
    );

    // A choice is refused...
    choose(&mut settings_cx, "appearance-theme-Light");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(paints_panel(cx, &dark_panel()), "the choice was not taken");
    assert!(
        chosen(&mut settings_cx, "Dark"),
        "the page still shows the default"
    );

    // ...and the record is left exactly as it was, for diagnosis.
    assert_eq!(
        std::fs::read_to_string(data.path().join("settings.json")).unwrap(),
        garbage,
        "the source data is retained, not silently erased"
    );
}

#[gpui::test]
fn a_development_override_wins_is_indicated_and_is_never_saved(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides {
                theme: Some(pane_core::ThemePreference::Light),
                material: None,
            },
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);

    // The override wins: no record exists, yet the window renders the
    // overridden palette.
    assert!(paints_panel(cx, &light_panel()), "the override is in force");

    let mut settings_cx = open_settings(cx);
    // The page says which variables override what...
    assert!(
        settings_cx.debug_bounds("appearance-override").is_some(),
        "the override is drawn"
    );
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("PANE_THEME=light"),
        "the override is named, {json}"
    );
    // ...and shows the overridden choice as the one in effect.
    assert!(chosen(&mut settings_cx, "Light"));

    // Nothing is offered while the override is in force: choosing Dark
    // changes nothing, and saves nothing.
    choose(&mut settings_cx, "appearance-theme-Dark");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(
        paints_panel(cx, &light_panel()),
        "the override still wins over the click"
    );
    assert!(
        chosen(&mut settings_cx, "Light"),
        "the page still shows the override"
    );
    assert!(
        !data.path().join("settings.json").exists(),
        "the override was not written back"
    );

    // A fresh application without the override renders what the record
    // holds — here the dark default, since nothing was ever saved: the
    // override was a preference of this process only.
    let mut fresh = cx.cx.new_app();
    fresh.update(pane::bind_keys);
    fresh.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    let launcher = Launcher::new(Runtime::start(), Vec::new());
    let (_window, fresh_cx) =
        fresh.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    fresh_cx.run_until_parked();
    assert!(paints_panel(fresh_cx, &dark_panel()));
}

// The General page's launch-at-login toggle, driven through the fake
// login system above: the user-visible toggle, repeat operations, the
// restart that reconciles the saved choice with the registration the
// platform reports, and the failure paths — a registration the platform
// refuses, a save that fails, an integration that cannot manage the
// registration here at all, a record that cannot be read.

/// Whether the launch-at-login switch reads as on, as assistive
/// technology sees it — the saved preference the switch carries.
fn login_chosen(cx: &mut VisualTestContext) -> bool {
    let (_, json) = accessibility(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    tree["nodes"].as_object().unwrap().values().any(|node| {
        let aria = &node["aria"];
        aria["role"] == "Switch"
            && aria["label"] == "Launch Pane at login"
            && aria["toggled"] == "True"
    })
}

/// The settings record's text, as it stands in `data`.
fn record_of(data: &std::path::Path) -> String {
    std::fs::read_to_string(data.join("settings.json")).expect("the settings record")
}

/// Runs the window until the settings record in `data` holds `text`: the
/// save the choice started is written off the window's thread, so what is
/// waited for is the record the platform's change was kept with — a
/// record not yet written reads as the empty string, which holds nothing.
fn until_record_holds(cx: &mut VisualTestContext, data: &std::path::Path, text: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let record = std::fs::read_to_string(data.join("settings.json")).unwrap_or_default();
        if record.contains(text) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the record to hold {text:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[gpui::test]
fn the_launch_at_login_toggle_takes_the_choice_and_saves_it(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let login = Arc::new(FakeLogin::default());
    cx.update(|cx| {
        pane::settings::init_with_login(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            login.clone(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);

    // The window opens on the General page, whose switch carries the
    // saved preference: off, the record's default.
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();
    assert!(
        settings_cx
            .debug_bounds("general-launch-at-login")
            .is_some(),
        "the switch is drawn"
    );
    assert!(!login_chosen(&mut settings_cx), "the choice is off");
    assert_eq!(login.registration(), Registration::Disabled);

    // Taking the choice: the fake platform's registration changes, the
    // switch follows, and the record keeps it.
    choose(&mut settings_cx, "general-launch-at-login");
    cx.run_until_parked();
    until_record_holds(&mut settings_cx, data.path(), "\"launchAtLogin\": true");
    assert!(login_chosen(&mut settings_cx), "the choice is taken");
    assert_eq!(login.registration(), Registration::Enabled);
    assert!(record_of(data.path()).contains("\"launchAtLogin\": true"));

    // Taking it back, and taking it again: repeated changes replace the
    // one registration rather than piling up, and each lands in the
    // record.
    choose(&mut settings_cx, "general-launch-at-login");
    cx.run_until_parked();
    until_record_holds(&mut settings_cx, data.path(), "\"launchAtLogin\": false");
    assert!(!login_chosen(&mut settings_cx));
    assert_eq!(login.registration(), Registration::Disabled);
    assert!(record_of(data.path()).contains("\"launchAtLogin\": false"));
    choose(&mut settings_cx, "general-launch-at-login");
    cx.run_until_parked();
    until_record_holds(&mut settings_cx, data.path(), "\"launchAtLogin\": true");
    assert!(login_chosen(&mut settings_cx));
    assert_eq!(login.registration(), Registration::Enabled);
    assert!(record_of(data.path()).contains("\"launchAtLogin\": true"));
}

#[gpui::test]
fn a_fresh_application_reconciles_a_registration_the_platform_lost(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let first = Arc::new(FakeLogin::default());
    cx.update(|cx| {
        pane::settings::init_with_login(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            first.clone(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);
    let mut settings_cx = general_page(cx);
    choose(&mut settings_cx, "general-launch-at-login");
    cx.run_until_parked();
    until_record(&mut settings_cx, data.path());
    assert_eq!(
        first.registration(),
        Registration::Enabled,
        "the first Pane registered"
    );

    // The platform's registration is gone — an OS reset, a login item
    // removed by hand — while the record still holds the user's choice.
    // A fresh application over the same data folder repairs the
    // registration to what the user chose: the preference is neither
    // silently dropped nor silently changed.
    let lost = Arc::new(FakeLogin::default());
    let mut fresh = cx.cx.new_app();
    fresh.update(pane::bind_keys);
    fresh.update(|cx| {
        pane::settings::init_with_login(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            lost.clone(),
            cx,
        )
    });
    let launcher = Launcher::new(Runtime::start(), Vec::new());
    let (_window, fresh_cx) =
        fresh.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    fresh_cx.run_until_parked();
    assert_eq!(
        lost.registration(),
        Registration::Enabled,
        "the missing registration was repaired at start"
    );

    // And the page it reports is the one the user chose, unchanged: the
    // preference still on, the registration back, nothing to explain
    // about the choice or the registration — the one note Linux carries
    // with the choice on is the freedesktop convention's limit, which
    // says nothing about this repair.
    fresh_cx.simulate_keystrokes(settings_shortcut());
    fresh_cx.run_until_parked();
    let settings = settings_windows(fresh_cx).pop().expect("Settings opened");
    let mut fresh_settings = settings_context(&settings, fresh_cx);
    fresh_settings.run_until_parked();
    assert!(login_chosen(&mut fresh_settings));
    if cfg!(target_os = "linux") {
        // The convention's caveat, not a problem with the repair.
        assert!(
            fresh_settings.debug_bounds("general-login-note").is_some(),
            "the freedesktop convention's limit is explained"
        );
    } else {
        assert!(
            fresh_settings.debug_bounds("general-login-note").is_none(),
            "nothing needs explaining: the choice and the registration agree"
        );
    }
}

#[gpui::test]
fn a_stale_registration_is_removed_without_enabling_the_choice(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    // A record whose user left the choice off...
    let record = r#"{ "version": 1, "launchAtLogin": false }"#;
    std::fs::write(data.path().join("settings.json"), record).unwrap();
    // ...beside a platform registration that says otherwise: stale, not
    // the user's.
    let stale = Arc::new(FakeLogin::default());
    *stale.registration.lock().unwrap() = Registration::Enabled;
    cx.update(|cx| {
        pane::settings::init_with_login(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            stale.clone(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);

    // At start the registration is reconciled to the record: removed,
    // not obeyed — a user-disabled preference is never silently enabled
    // by what the platform happens to hold.
    assert_eq!(
        stale.registration(),
        Registration::Disabled,
        "the stale registration was removed"
    );
    assert_eq!(record_of(data.path()), record);

    // The page shows the preference the record holds: off.
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();
    assert!(!login_chosen(&mut settings_cx));
}

#[gpui::test]
fn a_failed_registration_is_explained_and_nothing_is_saved(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let record = r#"{ "version": 1 }"#;
    std::fs::write(data.path().join("settings.json"), record).unwrap();
    let login = Arc::new(FakeLogin::default());
    *login.refusals.lock().unwrap() = (
        Some("the system refused it: another program owns the startup list".into()),
        None,
    );
    cx.update(|cx| {
        pane::settings::init_with_login(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            login.clone(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();

    // The switch is offered and off; taking it fails, and the failure is
    // the page's note — not a switch that pretends it succeeded.
    choose(&mut settings_cx, "general-launch-at-login");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(!login_chosen(&mut settings_cx), "the choice was not taken");
    assert!(
        settings_cx.debug_bounds("general-login-note").is_some(),
        "the refusal is drawn"
    );
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("the system refused it"),
        "the refusal is explained, {json}"
    );

    // The preference, the registration and the record all stand: a
    // failed registration never masquerades as a saved choice.
    assert_eq!(login.registration(), Registration::Disabled);
    assert_eq!(record_of(data.path()), record);
}

#[gpui::test]
fn a_failed_save_rolls_the_registration_back_to_what_was_saved(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let login = Arc::new(FakeLogin::default());
    cx.update(|cx| {
        pane::settings::init_with_login(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            login.clone(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);
    let mut settings_cx = general_page(cx);

    // A choice that saves, so the record holds it.
    choose(&mut settings_cx, "general-launch-at-login");
    cx.run_until_parked();
    until_record(&mut settings_cx, data.path());
    assert!(login.registration().registered());

    // Break the record's replacement: a folder where the record belongs,
    // so the atomic write cannot rename over it.
    std::fs::remove_file(data.path().join("settings.json")).unwrap();
    std::fs::create_dir(data.path().join("settings.json")).unwrap();

    // Turning the choice off: the platform's registration is released,
    // but the choice cannot be kept — so the registration is put back to
    // what the record last held, and the failure is the page's status.
    choose(&mut settings_cx, "general-launch-at-login");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("general-status").is_some(),
        "the failure is drawn"
    );
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("Pane could not save your choice"),
        "the failure is explained, {json}"
    );
    assert!(
        login_chosen(&mut settings_cx),
        "the switch shows what was saved: on"
    );
    assert!(
        login.registration().registered(),
        "the registration was rolled back to what was saved"
    );
}

#[gpui::test]
fn an_unavailable_login_integration_is_explained_and_not_offered(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        pane::settings::init_with_login(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            Arc::new(pane_core::autostart::Unavailable(
                "Not available in this development build: Pane registers itself at login only as \
                 the installed application"
                    .into(),
            )),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();

    // The switch is there and says off, but the reason it cannot be
    // taken is the page's note: the integration's own explanation, not a
    // toggle that pretends to work.
    assert!(
        settings_cx
            .debug_bounds("general-launch-at-login")
            .is_some(),
        "the switch is drawn"
    );
    assert!(!login_chosen(&mut settings_cx));
    assert!(
        settings_cx.debug_bounds("general-login-note").is_some(),
        "the limitation is drawn"
    );
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("Not available in this development build"),
        "the limitation is explained, {json}"
    );

    // Taking the choice anyway changes nothing: the platform was never
    // asked, and nothing was saved.
    choose(&mut settings_cx, "general-launch-at-login");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(!login_chosen(&mut settings_cx));
    assert!(!data.path().join("settings.json").exists());
}

#[gpui::test]
fn a_registration_awaiting_approval_is_explained(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    std::fs::write(
        data.path().join("settings.json"),
        r#"{ "version": 1, "launchAtLogin": true }"#,
    )
    .unwrap();
    // The platform holds the registration, but its approval is still
    // the user's to give, as macOS's login items are.
    let login = Arc::new(FakeLogin::default());
    *login.registration.lock().unwrap() = Registration::NeedsApproval;
    cx.update(|cx| {
        pane::settings::init_with_login(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            login.clone(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);

    // At start the registration already matches the choice, so nothing
    // is re-registered: what is left is the approval, which is the
    // page's to say.
    assert_eq!(login.registration(), Registration::NeedsApproval);
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();
    assert!(login_chosen(&mut settings_cx), "the choice is on");
    assert!(
        settings_cx.debug_bounds("general-login-note").is_some(),
        "the approval is drawn"
    );
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("macOS asks for your approval"),
        "the approval is explained, {json}"
    );
}

#[gpui::test]
fn an_unreadable_record_refuses_the_login_choice(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let garbage = "{ not the settings record";
    std::fs::write(data.path().join("settings.json"), garbage).unwrap();
    let login = Arc::new(FakeLogin::default());
    cx.update(|cx| {
        pane::settings::init_with_login(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            login.clone(),
            cx,
        )
    });
    let (_launcher, _links, cx) = open_launcher(cx);

    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();

    // The page says choices are not saved, and why.
    assert!(
        settings_cx.debug_bounds("general-status").is_some(),
        "the problem is drawn"
    );
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("Pane could not read the settings record"),
        "the problem is explained, {json}"
    );

    // The choice is refused: the record's rule is not to replace what
    // cannot be read, and the platform is not asked to hold a choice
    // nothing would remember.
    choose(&mut settings_cx, "general-launch-at-login");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(!login_chosen(&mut settings_cx), "the choice was refused");
    assert_eq!(login.registration(), Registration::Disabled);
    assert_eq!(record_of(data.path()), garbage);
}
