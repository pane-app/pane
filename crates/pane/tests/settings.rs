//! Pane's Settings window: its three entry points converge on one window,
//! which closes without quitting Pane, keeps its keyboard input to itself,
//! and answers for its titlebar controls. Drives the real windows through
//! GPUI's test platform, as `window.rs` drives the launcher's. The
//! General page's Appearance section is driven the same way — through the
//! page's own controls — with what the windows paint checked on their
//! quads, so a choice is observed at the same boundary a user sees it. The
//! General page's launch-at-login toggle is driven the same way, through a fake
//! login system the tests script — no test ever touches the real login
//! configuration of the machine running it; and the Extensions page
//! manages extensions through the launcher's own operations.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, Modifiers, MouseButton, TestAppContext, VisualTestContext, WindowHandle,
    prelude::*, px,
};
use pane::{APP_VERSION, LauncherWindow, SettingsWindow};
use pane_core::autostart::{Autostart, Registration};
use pane_core::changes;
use pane_core::defaults::ArtifactSource;
use pane_core::develop::{Build, BuildJob, BuildOutcome, Builder, Toolchains};
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status, Target};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

// Pane registers no sample command (#162): the tests that drive the
// samples register them themselves.
#[path = "support/samples.rs"]
mod samples;

#[path = "support/paint.rs"]
mod paint;

#[path = "../../pane-core/tests/support/artifacts.rs"]
mod artifacts;

use artifacts::Artifacts;

use paint::paints_fill_at;
use settle::settle;

#[path = "support/a11y.rs"]
mod a11y;
#[path = "support/setup.rs"]
mod setup;
#[path = "support/wait.rs"]
mod wait;

use a11y::{accessibility, announcement, focused_label};
use setup::settings_shortcut;
use wait::{frame, settle_frames, until};

/// The keystroke that focuses the Settings search: Cmd+F on macOS,
/// Ctrl+F on Windows and Linux.
fn find_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-f"
    } else {
        "ctrl-f"
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
        Launcher::new(Runtime::start(), samples::sample_commands()).with_link_opener(links.clone());
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
    let launcher = Launcher::new(Runtime::start(), samples::sample_commands())
        .with_link_opener(Arc::new(RefusingLinks));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (window, cx)
}

/// Writes the settings sample as a package that declares what it does and
/// where its issues go: the metadata a published package carries, whose
/// description the Extensions group lists (#224).
fn described_package(folder: &Path) -> PathBuf {
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
    let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
    let described = manifest.replace(
        "\"title\": \"Settings sample\",",
        "\"title\": \"Described sample\",\n  \"description\": \"Keeps a chosen greeting style in Pane's settings\",\n  \"author\": \"Ada Lovelace\",\n  \"repository\": \"https://github.com/pane-app/pane\",\n  \"issues\": \"https://github.com/pane-app/pane/issues\",\n  \"license\": \"Apache-2.0 OR MIT\",\n  \"keywords\": [\"sample\", \"settings\"],",
    );
    assert!(
        described.contains("\"description\""),
        "the settings sample's manifest changed"
    );
    fs::write(folder.join("pane.json"), described).unwrap();
    folder.to_path_buf()
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
/// General page, so this walks the sidebar to Extensions first. The
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
/// as its user would — the pointer moving onto it first, for the reason
/// [`click_section`] gives.
fn click_row(settings_cx: &mut VisualTestContext, row: &'static str) {
    // A row below the fold is brought into view first, as a user turns
    // the page's wheel to reach it: the page scrolls, and a click past the
    // window's bottom edge would reach nothing.
    let viewport = settings_cx.update(|window, _| window.viewport_size());
    for _ in 0..10 {
        let bounds = settings_cx
            .debug_bounds(row)
            .unwrap_or_else(|| panic!("no {row} on the Extensions page"));
        if bounds.bottom() <= viewport.height {
            break;
        }
        settings_cx.simulate_event(gpui::ScrollWheelEvent {
            position: gpui::point(bounds.center().x, viewport.height / 2.),
            delta: gpui::ScrollDelta::Pixels(gpui::point(
                px(0.),
                viewport.height - bounds.bottom() - px(24.),
            )),
            modifiers: Modifiers::none(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        settings_cx.run_until_parked();
    }
    let bounds = settings_cx
        .debug_bounds(row)
        .unwrap_or_else(|| panic!("no {row} on the Extensions page"));
    assert!(
        bounds.bottom() <= viewport.height,
        "{row} scrolled into view: {bounds:?}"
    );
    settings_cx.simulate_mouse_move(bounds.center(), None::<MouseButton>, Modifiers::none());
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

/// Presses Tab until the node labelled `label` has focus, failing if a
/// dozen stops never reach it; the labels focused on the way, in order.
fn tab_to(cx: &mut VisualTestContext, label: &str) -> Vec<String> {
    let mut passed = Vec::new();
    for _ in 0..12 {
        cx.simulate_keystrokes("tab");
        match focused_label(cx) {
            Some(focused) if focused == label => return passed,
            focused => passed.push(focused.unwrap_or_default()),
        }
    }
    panic!("Tab never reached {label:?}; it passed {passed:?}");
}

/// The last drawn frame's section arrival, as the arriving page content's
/// (offset from rest in px — below rest when the sidebar moved down to
/// the section, above when it moved up — and opacity); `None` when the
/// frame drew the page settled, which is also all reduced motion ever
/// reports. See [`SettingsWindow::section_arrival`].
fn section_arrival(
    settings: &WindowHandle<SettingsWindow>,
    cx: &mut VisualTestContext,
) -> Option<(f32, f32)> {
    settings
        .read_with(cx, |window, _| window.section_arrival())
        .expect("the Settings window is open")
}

/// Clicks the sidebar's section whose debug selector is `selector`
/// ("section-<title>"), switching the window to it. The pointer moves
/// onto the row first, as a user's does: a click's landing alone does
/// not tell a row it is hovered, and the wash the row keeps would never
/// settle with the layout saying the pointer is gone and the paint
/// saying it is there.
fn click_section(cx: &mut VisualTestContext, selector: &'static str) {
    let section = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_mouse_move(section.center(), None::<MouseButton>, Modifiers::none());
    cx.simulate_click(section.center(), Modifiers::none());
    cx.run_until_parked();
}

/// Clicks the element whose debug selector is `selector`, as its user
/// would — the pointer moving onto it first, for the reason
/// [`click_section`] gives.
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_mouse_move(bounds.center(), None::<MouseButton>, Modifiers::none());
    cx.simulate_click(bounds.center(), Modifiers::none());
}

/// Moves the pointer off the window, as a user's does when it leaves.
/// The test platform parks the pointer wherever a move or click last
/// put it, and content that reflows under a parked pointer — a popup
/// unmounting over it, a list scrolling beneath it — would leave a wash
/// running that only a real move settles: the tests that count the
/// window's frames send the pointer away first.
fn pointer_leaves(cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(
        gpui::point(px(-100.), px(-100.)),
        None::<MouseButton>,
        Modifiers::none(),
    );
}

/// Waits until the sidebar's selected section is exactly `title`, as
/// assistive technology reads it — the visible selected-section state
/// updates on the frame the switch draws, while the arrival is still in
/// flight — and returns that frame's tree. (Polled, not read once: the
/// captured tree can follow the drawn frame by one on the Windows test
/// platform.)
fn selected_section(cx: &mut VisualTestContext, title: &str) {
    until(cx, |cx| {
        let (_, json) = accessibility(cx);
        let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
        let selected: Vec<&str> = tree["nodes"]
            .as_object()
            .unwrap()
            .values()
            .filter(|node| {
                node["aria"]["role"] == "ListBoxOption"
                    && node["aria"]["selected"] == serde_json::json!(true)
            })
            .map(|node| node["aria"]["label"].as_str().unwrap_or_default())
            .collect();
        (selected == [title]).then_some(())
    });
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

/// Whether the Appearance section's RadioButton named `label` is the
/// choice in effect, as assistive technology reads it.
fn chosen(cx: &mut VisualTestContext, label: &str) -> bool {
    let (_, json) = accessibility(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    tree["nodes"].as_object().unwrap().values().any(|node| {
        let aria = &node["aria"];
        aria["role"] == "RadioButton" && aria["label"] == label && aria["toggled"] == "True"
    })
}

/// Clicks the General page's control whose debug selector is `selector`
/// — an Appearance choice, the launch-at-login switch — through the
/// page's own control.
fn choose(cx: &mut VisualTestContext, selector: &'static str) {
    let choice = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"));
    cx.simulate_click(choice.center(), Modifiers::none());
}

/// Opens the Settings window over the launcher `cx` drives, on the
/// General page it first shows, as its own window context: the page the
/// Open Pane hotkey's recorder, the launch-at-login switch and the
/// Appearance section's theme and material choices live on. The Open
/// Pane hotkey's own tests live in `open_pane.rs`.
fn open_settings(cx: &mut VisualTestContext) -> VisualTestContext {
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();
    settings_cx
}

/// Runs the window until the settings record exists in `data`: the save
/// the page's choice started is written off the window's thread.
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
    // The page's Appearance section is drawn too, with its choices.
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
        settings_cx.debug_bounds("extension-install-npm…").is_none(),
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

    // The page's Appearance section is drawn too.
    assert!(
        settings_cx.debug_bounds("appearance").is_some(),
        "the Appearance section is drawn"
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
        Some("Search"),
        "root search's field has focus, not its selected result (#132)"
    );

    // The keyboard reaches the menu: Tab from the query field, past the
    // pinned home's pins (tab stops between the query field and the
    // footer; the pin hint is none), then Enter presses the button.
    let passed = tab_to(cx, "Pane menu");
    assert!(
        passed.iter().all(|label| label.starts_with("Pinned ")),
        "only the pins come between the query field and the menu: {passed:?}"
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_some(), "the menu is open");
    // Focus moved into it: the menu keeps it, and the announcer says its
    // selected item (#132).
    assert_eq!(focused_label(cx).as_deref(), Some("Pane menu"));
    assert_eq!(announcement(cx), "Settings, 1 of 1");

    // The popup overlays the list: it sits above the footer strip, over
    // the results. Its entrance has settled (the frames it asked for
    // delivered), so this reads it at rest.
    settle_frames(cx);
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
    // The focus is restored at once, while the popup's exit still paints.
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("Pane menu"),
        "focus is restored to what had it: the menu's button"
    );
    settle_frames(cx);
    assert!(cx.debug_bounds("menu").is_none(), "the menu is closed");
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
    assert_eq!(focused_label(cx).as_deref(), Some("Pane menu"));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let settings = settings_windows(cx)
        .pop()
        .expect("the menu's item opened the Settings window");
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("Pane menu"),
        "focus is restored to what had it: the menu's button"
    );
    settle_frames(cx);
    assert!(cx.debug_bounds("menu").is_none(), "the menu closed with it");
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
    click(cx, "row-Rust sample");
    cx.run_until_parked();
    settle_frames(cx);
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
    // click before the button can reopen it. The pointer stays on the
    // button throughout, as a user's does, so the wash it keeps there
    // stays settled.
    click(cx, "footer-menu");
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu").is_some());
    click(cx, "footer-menu");
    cx.run_until_parked();
    settle_frames(cx);
    assert!(
        cx.debug_bounds("menu").is_none(),
        "the button toggled closed"
    );

    // The button and its item are named controls, with the open state.
    click(cx, "footer-menu");
    cx.run_until_parked();
    let (_, json) = accessibility(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes: Vec<&serde_json::Value> = tree["nodes"].as_object().unwrap().values().collect();
    let button = nodes
        .iter()
        .find(|node| node["aria"]["role"] == "Button" && node["aria"]["label"] == "Pane menu")
        .expect("the menu button is named");
    assert_eq!(
        button["aria"]["expanded"],
        serde_json::json!(true),
        "the open state is announced"
    );
    assert!(
        nodes
            .iter()
            .any(|node| node["aria"]["role"] == "Menu" && node["aria"]["label"] == "Pane menu"),
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

/// The footer menu's popup enters from the strip and exits back into it:
/// the entrance starts the full tiny shift toward the strip, already
/// faintly visible, and settles at rest above the footer; the exit
/// recedes over the shorter span, is inert while it paints — the focus
/// is already restored, the item's click does nothing, and a click on
/// the results under the overlay reaches nothing — and unmounts leaving
/// the window idle, with the page under it answering again.
#[gpui::test]
fn the_menu_popup_enters_from_the_strip_and_exits_back_into_it(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);
    settle(&launcher, cx);
    settle_frames(cx);

    click(cx, "footer-menu");
    cx.run_until_parked();
    // The pointer leaves the strip's button it opened the menu with, and
    // the wash it held settles with it, so the frames that follow are
    // the popup's own — none of the pointer's.
    pointer_leaves(cx);
    // The entrance starts the full shift toward the strip (down, from
    // the popup's rest above it) and the fade's floor.
    let entering = menu_popup(&launcher, cx).expect("the popup is entering");
    assert!(
        entering.0 > 2.5 && entering.0 < 3.5,
        "the entrance starts the full shift toward the strip: {}",
        entering.0
    );
    assert!(
        entering.1 < 0.45,
        "the entrance starts faint: {}",
        entering.1
    );
    let moving = cx.debug_bounds("menu").expect("the menu is drawn");
    // Frames pass, and the entrance progresses toward rest.
    assert!(frame(cx, Duration::from_millis(40)) >= 1);
    let progressed = menu_popup(&launcher, cx).expect("the popup is still entering");
    assert!(
        progressed.0 < entering.0 && progressed.0 > 0.,
        "the entrance progressed toward rest: {} from {}",
        progressed.0,
        entering.0
    );
    // Past the entrance's span the popup is at rest — above the footer,
    // shifted exactly the entrance's offset up from where it started —
    // and the window is idle.
    assert!(frame(cx, Duration::from_millis(130)) >= 1);
    assert!(menu_popup(&launcher, cx).is_none());
    let rest = cx.debug_bounds("menu").expect("the menu is at rest");
    let footer = cx.debug_bounds("status-idle").expect("the footer");
    assert!(
        rest.bottom() <= footer.top(),
        "the popup settled above the footer"
    );
    assert_eq!(
        moving.bottom() - rest.bottom(),
        px(entering.0),
        "the popup was shifted exactly the entrance's offset below its rest"
    );
    assert_eq!(settle_frames(cx), 0, "a settled menu schedules no frame");

    // Escape closes it: the focus is restored the frame the menu closed,
    // while the exit still paints; the exit recedes toward the strip.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        focused_label(cx).as_deref(),
        Some("Pane menu"),
        "the focus returned the frame the menu closed"
    );
    let leaving = menu_popup(&launcher, cx).expect("the exit is painting");
    assert!(leaving.0 < 0.5, "the exit starts at rest: {}", leaving.0);
    assert!(frame(cx, Duration::from_millis(30)) >= 1);
    let receding = menu_popup(&launcher, cx).expect("the exit is still painting");
    assert!(
        receding.0 > 1.,
        "the exit recedes toward the strip: {}",
        receding.0
    );
    assert!(
        receding.1 < 1.,
        "the exit fades the popup out: {}",
        receding.1
    );

    // The exit's visuals are inert: the item's click does nothing — the
    // exit's list carries no handlers, and the overlay takes the clicks
    // that land on it, so nothing under or on it is invoked.
    click(cx, "menu-item-Settings");
    cx.run_until_parked();
    let view = settle(&launcher, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "the exiting menu's item invoked nothing"
    );
    assert_eq!(
        settings_windows(cx).len(),
        0,
        "the exiting menu's item opened no Settings window"
    );
    // The pointer leaves the fading menu before it unmounts: the
    // results it covers are revealed as it goes, and a row the pointer
    // never moved onto takes no wash for its paint's say-so alone.
    pointer_leaves(cx);

    // Past the exit's span the popup unmounts — nothing of it is drawn,
    // visible or not, so no invisible overlay survives to intercept a
    // click — and the window is idle.
    assert!(frame(cx, Duration::from_millis(90)) >= 1);
    assert!(menu_popup(&launcher, cx).is_none());
    assert!(
        cx.debug_bounds("menu").is_none(),
        "the exit unmounted the popup"
    );
    assert_eq!(settle_frames(cx), 0, "a closed menu schedules no frame");
}

/// A menu reopened during its exit reverses from the presentation on
/// screen instead of restarting, and the item the exit was still
/// painting is the item the reopened menu shows.
#[gpui::test]
fn a_menu_reopened_during_its_exit_retargets(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);
    settle(&launcher, cx);
    settle_frames(cx);

    // Open, and let the entrance settle.
    click(cx, "footer-menu");
    cx.run_until_parked();
    settle_frames(cx);
    assert!(menu_popup(&launcher, cx).is_none());

    // Close, let part of the exit run, and reopen — with the exit still
    // in flight.
    click(cx, "footer-menu");
    cx.run_until_parked();
    assert!(menu_popup(&launcher, cx).is_some());
    assert!(frame(cx, Duration::from_millis(25)) >= 1);
    let mid_exit = menu_popup(&launcher, cx).expect("the exit is painting");

    click(cx, "footer-menu");
    cx.run_until_parked();
    let reversing = menu_popup(&launcher, cx).expect("the popup is reopening");
    assert!(
        reversing.0 < mid_exit.0 + 0.5 && reversing.0 < 2.5,
        "the reopen continued from the exit's presentation, not the full \
         shift: {} from {}",
        reversing.0,
        mid_exit.0
    );
    // The fade turns around where the exit left it too: no jump up to
    // the entrance's floor, which a fresh open starts from.
    assert!(
        reversing.1 > 0. && reversing.1 <= mid_exit.1 + 0.05,
        "the reopen's fade jumped instead of continuing from the exit's: {} from {}",
        reversing.1,
        mid_exit.1
    );
    // The reopened menu is the interactive one: its item activates.
    assert!(
        cx.debug_bounds("menu-item-Settings").is_some(),
        "the reopened menu is drawn"
    );
    settle_frames(cx);
    click(cx, "menu-item-Settings");
    cx.run_until_parked();
    assert_eq!(
        settings_windows(cx).len(),
        1,
        "the reopened menu's item opened Settings"
    );
    // The pointer leaves the closing menu before it unmounts, so the
    // results it covered take no wash for a pointer that never moved
    // onto them.
    pointer_leaves(cx);
    settle_frames(cx);
    assert!(cx.debug_bounds("menu").is_none(), "the menu closed with it");
}

/// Reduced motion lands the menu popup at its endpoint with no frame at
/// all: opening draws it at rest above the footer, closing unmounts it
/// at once, and a preference engaged mid-entrance settles it on the next
/// frame.
#[gpui::test]
fn reduced_motion_lands_the_menu_popup_at_once(cx: &mut TestAppContext) {
    let (launcher, _links, cx) = open_launcher(cx);
    settle(&launcher, cx);
    settle_frames(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));

    // Opening under reduced motion: no entrance starts.
    let button = cx.debug_bounds("footer-menu").expect("the menu button");
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(menu_popup(&launcher, cx).is_none());
    let menu = cx.debug_bounds("menu").expect("the menu is drawn at rest");
    let footer = cx.debug_bounds("status-idle").expect("the footer");
    assert!(
        menu.bottom() <= footer.top(),
        "the popup is at rest above the footer"
    );
    assert_eq!(settle_frames(cx), 0, "no frame was asked for");

    // Closing under reduced motion: the popup unmounts at once.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(menu_popup(&launcher, cx).is_none());
    assert!(cx.debug_bounds("menu").is_none());
    assert_eq!(settle_frames(cx), 0, "no frame was asked for");

    // Reduced motion engaged mid-entrance ends it on the next frame.
    cx.update(|_, cx| cx.set_reduce_motion(false));
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(
        menu_popup(&launcher, cx).is_some(),
        "the entrance began under full motion"
    );
    cx.update(|_, cx| cx.set_reduce_motion(true));
    assert!(frame(cx, Duration::ZERO) >= 1);
    assert!(
        menu_popup(&launcher, cx).is_none(),
        "the entrance settled the moment reduced motion engaged"
    );
    assert_eq!(
        settle_frames(cx),
        0,
        "the window asked for no further frame"
    );
}

/// The menu popup's presentation as the last frame drew it — the offset
/// from rest toward the strip in px (positive: the popup sits above the
/// strip, so toward it is down) and the opacity; `None` when the last
/// frame drew the popup settled (at rest while open, absent while
/// closed). See [`LauncherWindow::menu_popup_presentation`].
fn menu_popup(
    launcher: &gpui::Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
) -> Option<(f32, f32)> {
    cx.read_entity(launcher, |window, _| window.menu_popup_presentation())
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
        ["https://github.com/pane-app/pane"],
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
        json.contains("Couldn't open the documentation: no program to open web links is installed"),
        "the refusal is explained, {json}"
    );
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert_eq!(view.status, Status::Idle, "the launcher is untouched");
}

/// The world of one About-page update test: the folder Pane's program is
/// installed in, the folder its data lives in, and a local artifact source
/// on 127.0.0.1 (pane-core's test support; nothing reaches the network or
/// Pane's published downloads) serving the index an update check reads —
/// the same fixture `pane-core`'s application-update tests use, driven
/// here through the highest boundary, the Settings window's own page.
struct UpdateDirs {
    install: TempDir,
    data: TempDir,
    artifacts: Artifacts,
}

impl UpdateDirs {
    fn new() -> UpdateDirs {
        UpdateDirs {
            install: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            artifacts: Artifacts::start(),
        }
    }

    /// The program Pane runs from, in the install folder.
    fn program(&self) -> PathBuf {
        self.install.path().join("pane")
    }

    /// Writes the program Pane runs from, with the given bytes.
    fn running(&self, program: &[u8]) {
        fs::write(self.program(), program).unwrap();
    }

    /// Publishes an application package `version` for this system, holding
    /// `program` as the program an install replaces this Pane's with, as
    /// `pane-core`'s own tests publish one.
    fn publish_update(&self, version: &str, program: &[u8]) {
        let zip = artifacts::pack_zip(&[("pane", program.to_vec())]);
        let target = Target::current()
            .expect("Pane names this system's target")
            .id()
            .to_owned();
        self.artifacts.publish_application(version, &target, &zip);
    }
}

/// Opens the launcher window over a launcher that checks the local
/// artifact source for updates of the version `version` it runs from
/// `dirs`' program file — the same wiring the binary's is, including the
/// changes channel whose other end the window follows, so background
/// changes (a check's answer, an install's progress) redraw the windows
/// as they do in the app.
fn open_updating_launcher<'a>(
    cx: &'a mut TestAppContext,
    dirs: &UpdateDirs,
    version: &str,
) -> (gpui::Entity<LauncherWindow>, &'a mut VisualTestContext) {
    let (sender, changes) = changes::channel();
    let launcher = Launcher::with_packages(
        Runtime::start(),
        vec![],
        dirs.data.path().join("extensions"),
    )
    .with_application_update(
        version,
        ArtifactSource::local(dirs.artifacts.url()).unwrap(),
        dirs.program(),
    )
    .with_development(Arc::new(Toolchains::from_env(None)), sender);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher
    });
    (window, cx)
}

/// Opens the Settings window with the local `Ctrl+,` shortcut and returns
/// a context driving it, on its About page: the window opens on the
/// General page, so this walks the sidebar to About first. The window
/// is made tall enough that the page's rows are in reach of a click
/// without scrolling it — the page itself scrolls when the window is
/// smaller.
fn open_about(cx: &mut VisualTestContext) -> (WindowHandle<SettingsWindow>, VisualTestContext) {
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.simulate_resize(gpui::size(px(740.), px(1100.)));
    settings_cx.run_until_parked();
    let about = settings_cx
        .debug_bounds("section-About")
        .expect("the About section");
    settings_cx.simulate_click(about.center(), Modifiers::none());
    settings_cx.run_until_parked();
    (settings, settings_cx)
}

#[gpui::test]
fn the_about_page_explains_where_no_release_source_is_configured(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    let (_settings, mut settings_cx) = open_about(cx);

    // A launcher with no updater wired — no artifact source given, as a
    // development checkout without PANE_ARTIFACTS runs — is explained as
    // what it is: no release to check, no claim of a feed or an available
    // release, and no row to click.
    until_text(&mut settings_cx, "This build has no update source.");
    assert!(
        settings_cx.debug_bounds("about-check-update").is_none(),
        "no check is offered against a source that is not there"
    );
    assert!(settings_cx.debug_bounds("about-update").is_none());
}

#[gpui::test]
fn a_check_from_the_page_that_finds_nothing_newer_says_pane_is_up_to_date(cx: &mut TestAppContext) {
    let dirs = UpdateDirs::new();
    dirs.running(b"the 0.1.0 program");
    let (launcher, cx) = open_updating_launcher(cx, &dirs, "0.1.0");
    let (_settings, mut settings_cx) = open_about(cx);

    // The check the user asked for, from the page: the page answers, and
    // so does the status line — the same answer, as the user asked both.
    click_row(&mut settings_cx, "about-check-update");
    until_text(&mut settings_cx, "Pane is up to date");
    // The row stays: the user can ask again.
    assert!(
        settings_cx.debug_bounds("about-check-update").is_some(),
        "the check is offered again"
    );
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert_eq!(view.status, Status::Result("Pane is up to date".into()));
    // Root search lists no update row: there is nothing to choose.
    let rows = titles(&launcher, cx);
    assert!(
        !rows.contains(&"Update Pane to 99.0.0".to_owned()),
        "{rows:?}"
    );
    assert!(
        !rows.contains(&"Check for a Pane update".to_owned()),
        "{rows:?}"
    );
}

#[gpui::test]
fn an_unreachable_release_source_is_explained_on_the_page(cx: &mut TestAppContext) {
    let dirs = UpdateDirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let (launcher, cx) = open_updating_launcher(cx, &dirs, "0.1.0");
    // Nothing answers where the artifact source is: the check cannot be
    // made.
    drop(dirs.artifacts);
    let (_settings, mut settings_cx) = open_about(cx);

    click_row(&mut settings_cx, "about-check-update");
    until_text(&mut settings_cx, "Couldn't check for updates: ");
    // The failure explains the unreachable source, retried as an
    // interrupted acquisition is, and the page offers the check again.
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("could not be reached") && json.contains("Pane tried 3 times"),
        "the unreachable source is explained, {json}"
    );
    assert!(
        settings_cx.debug_bounds("about-check-update").is_some(),
        "the check is offered again"
    );
    // The root search holds the same failure with its own row: the two
    // entry points share one state.
    assert!(
        titles(&launcher, cx).contains(&"Check for a Pane update".to_owned()),
        "the root row is listed"
    );
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert!(
        matches!(&view.status, Status::Error(text) if text.starts_with(
            "Could not check for a Pane update: Pane's downloads at"
        )),
        "the status line says it too: {:?}",
        view.status
    );
}

#[gpui::test]
fn a_failed_check_is_tried_again_from_the_page_and_finds_the_update(cx: &mut TestAppContext) {
    let dirs = UpdateDirs::new();
    dirs.running(b"the 0.1.0 program");
    // The source answers its index with an error, retried as an
    // interrupted acquisition is, so the check fails.
    dirs.artifacts
        .fail_status("pane-defaults.json", 503, usize::MAX);
    let (launcher, cx) = open_updating_launcher(cx, &dirs, "0.1.0");
    let (_settings, mut settings_cx) = open_about(cx);

    click_row(&mut settings_cx, "about-check-update");
    until_text(&mut settings_cx, "Couldn't check for updates: ");

    // The source works again, with a newer version published: the retry
    // from the page finds it, and both entry points show the offer.
    dirs.artifacts.stop_failing("pane-defaults.json");
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    click_row(&mut settings_cx, "about-check-update");
    until_text(&mut settings_cx, "Pane 99.0.0 is available");
    assert!(settings_cx.debug_bounds("about-update").is_some());
    assert!(titles(&launcher, cx).contains(&"Update Pane to 99.0.0".to_owned()));
}

#[gpui::test]
fn the_page_offers_the_update_and_installs_it_by_the_users_choice(cx: &mut TestAppContext) {
    let dirs = UpdateDirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let (launcher, cx) = open_updating_launcher(cx, &dirs, "0.1.0");
    let (_settings, mut settings_cx) = open_about(cx);

    // The check from the page finds the newer version: the page offers
    // what the root row offers — the version, and what installing does.
    click_row(&mut settings_cx, "about-check-update");
    until_text(&mut settings_cx, "Pane 99.0.0 is available");
    let (_, json) = accessibility(&mut settings_cx);
    assert!(json.contains("Update to 99.0.0"), "{json}");
    assert!(
        json.contains("Keeps your extensions and settings. The new version starts next time."),
        "the button says what installing does, {json}"
    );
    assert!(
        titles(&launcher, cx).contains(&"Update Pane to 99.0.0".to_owned()),
        "root search holds the same offer"
    );

    // The package arrives in two pieces, so the page is seen following the
    // download as it goes.
    let file = format!("pane-99.0.0-{}.zip", Target::current().unwrap().id());
    dirs.artifacts.stall(&file, 8, Duration::from_millis(1200));

    // Installing is the user's choice, the row clicked: the page follows
    // the install's progress, and the answer when it lands.
    click_row(&mut settings_cx, "about-update");
    until_text(&mut settings_cx, "Downloading Pane 99.0.0: ");
    // Mid-install there is no row to click: nothing else can be started
    // against the same offer.
    assert!(settings_cx.debug_bounds("about-update").is_none());
    until_text(
        &mut settings_cx,
        "Pane 99.0.0 is installed and starts next time",
    );

    // The program was swapped: the new one in place, the old one renamed
    // out of its way, nothing else in the install folder.
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
    assert_eq!(
        fs::read(dirs.install.path().join("pane.old")).unwrap(),
        b"the 0.1.0 program"
    );
    // The launcher's status line said the same thing, and the offer is
    // gone from both entry points.
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert_eq!(
        view.status,
        Status::Result(
            "Installed Pane 99.0.0; the new version is used the next time Pane starts".into()
        )
    );
    assert!(settings_cx.debug_bounds("about-update").is_none());
    assert!(!titles(&launcher, cx).contains(&"Update Pane to 99.0.0".to_owned()));
}

#[gpui::test]
fn a_failed_install_is_explained_and_the_offer_stays_to_try_again(cx: &mut TestAppContext) {
    let dirs = UpdateDirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let (launcher, cx) = open_updating_launcher(cx, &dirs, "0.1.0");
    let (_settings, mut settings_cx) = open_about(cx);
    click_row(&mut settings_cx, "about-check-update");
    until_text(&mut settings_cx, "Pane 99.0.0 is available");
    // The package the index names is not what arrives: its bytes were
    // damaged, so they do not match the integrity the index gives.
    dirs.artifacts.corrupt_application();

    click_row(&mut settings_cx, "about-update");
    until_text(&mut settings_cx, "Couldn't update to 99.0.0: ");

    // The failure is explained with the offer still offered, ready to be
    // chosen again; nothing changed.
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        json.contains("does not match the sha512 integrity"),
        "{json}"
    );
    assert!(
        settings_cx.debug_bounds("about-update").is_some(),
        "the offer stays"
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 0.1.0 program");
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert!(
        matches!(&view.status, Status::Error(text) if text.starts_with(
            "Could not update Pane to 99.0.0: "
        )),
        "the status line says it too: {:?}",
        view.status
    );

    // The source works again; choosing the offer again installs it.
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    click_row(&mut settings_cx, "about-update");
    until_text(
        &mut settings_cx,
        "Pane 99.0.0 is installed and starts next time",
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
}

#[gpui::test]
fn an_interrupted_download_from_the_page_is_tried_again_and_lands(cx: &mut TestAppContext) {
    let dirs = UpdateDirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let (launcher, cx) = open_updating_launcher(cx, &dirs, "0.1.0");
    let (_settings, mut settings_cx) = open_about(cx);
    click_row(&mut settings_cx, "about-check-update");
    until_text(&mut settings_cx, "Pane 99.0.0 is available");

    // The first download of the package is interrupted partway: the
    // connection closes after its first bytes, as an offline moment
    // does. The install the page started rides the retry the download
    // itself makes — the same retry the root row's install does — and
    // lands.
    let file = format!("pane-99.0.0-{}.zip", Target::current().unwrap().id());
    dirs.artifacts.drop_after(&file, 16, 1);

    click_row(&mut settings_cx, "about-update");
    until_text(
        &mut settings_cx,
        "Pane 99.0.0 is installed and starts next time",
    );
    assert_eq!(fs::read(dirs.program()).unwrap(), b"the 99.0.0 program");
    assert_eq!(
        fs::read(dirs.install.path().join("pane.old")).unwrap(),
        b"the 0.1.0 program"
    );
    // The package was downloaded twice: the interrupted one, and the
    // retry that landed.
    let downloads = dirs
        .artifacts
        .requests()
        .iter()
        .filter(|path| path.ends_with(".zip"))
        .count();
    assert_eq!(downloads, 2, "the interrupted download was retried");
    // The status line answered the same landing, and neither entry
    // point offers the update any more.
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert_eq!(
        view.status,
        Status::Result(
            "Installed Pane 99.0.0; the new version is used the next time Pane starts".into()
        )
    );
    assert!(settings_cx.debug_bounds("about-update").is_none());
    assert!(!titles(&launcher, cx).contains(&"Update Pane to 99.0.0".to_owned()));
}

#[gpui::test]
fn leaving_the_page_while_a_check_runs_cancels_nothing(cx: &mut TestAppContext) {
    let dirs = UpdateDirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    // The index answers slowly, so the check is still running when the
    // user leaves the page.
    dirs.artifacts
        .stall("pane-defaults.json", 8, Duration::from_millis(1200));
    let (launcher, cx) = open_updating_launcher(cx, &dirs, "0.1.0");
    let (_settings, mut settings_cx) = open_about(cx);

    // The check starts from the page; the user walks away to another
    // section while it runs.
    click_row(&mut settings_cx, "about-check-update");
    until_text(&mut settings_cx, "Checking for updates");
    let extensions = settings_cx
        .debug_bounds("section-Extensions")
        .expect("the Extensions section");
    settings_cx.simulate_click(extensions.center(), Modifiers::none());
    settings_cx.run_until_parked();

    // Leaving cancelled nothing: the check ran to its end, and its answer
    // is where both entry points read it — the launcher's state and
    // status line, with the root row listed.
    until(cx, |cx| {
        cx.read_entity(&launcher, |window, _| window.launcher().view())
            .status
            .eq(&Status::Result("Pane 99.0.0 is available".into()))
            .then_some(())
    });
    assert!(titles(&launcher, cx).contains(&"Update Pane to 99.0.0".to_owned()));

    // Back on the About page, the same state: the offer, with the row that
    // installs it.
    let about = settings_cx
        .debug_bounds("section-About")
        .expect("the About section");
    settings_cx.simulate_click(about.center(), Modifiers::none());
    settings_cx.run_until_parked();
    until_text(&mut settings_cx, "Pane 99.0.0 is available");
    assert!(settings_cx.debug_bounds("about-update").is_some());
}

#[gpui::test]
fn the_page_copies_the_diagnostics_to_the_clipboard_locally(cx: &mut TestAppContext) {
    let dirs = UpdateDirs::new();
    dirs.publish_update("99.0.0", b"the 99.0.0 program");
    dirs.running(b"the 0.1.0 program");
    let (_launcher, cx) = open_updating_launcher(cx, &dirs, "0.1.0");
    let (_settings, mut settings_cx) = open_about(cx);
    click_row(&mut settings_cx, "about-check-update");
    until_text(&mut settings_cx, "Pane 99.0.0 is available");

    // The copy is the user's explicit click, and its completion is the
    // page's own status.
    click_row(&mut settings_cx, "about-diagnostics");
    until_text(&mut settings_cx, "Copied to the clipboard");
    // What was copied is what Pane knows of this installation — the real
    // version, the system, the data folder, the update state — as plain
    // text on this computer's clipboard: nothing was sent anywhere, and
    // nothing of any extension's settings or data is in it (none is
    // read).
    let report = settings_cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("the report was copied");
    assert!(
        report.starts_with(&format!("Pane {APP_VERSION}")),
        "{report}"
    );
    assert!(
        report.contains(&format!("Built for {}", Target::current().unwrap().id())),
        "{report}"
    );
    assert!(report.contains("Data folder: "), "{report}");
    assert!(
        report.contains("Update check: Pane 99.0.0 is available"),
        "{report}"
    );
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

/// Runs the window until it has drawn the element with debug selector
/// `selector`: a line of text, which the accessibility tree does not hold.
fn until_drawn(cx: &mut VisualTestContext, selector: &'static str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.run_until_parked();
        if cx.debug_bounds(selector).is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {selector:?} to be drawn"
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

/// Opens the page of the installed extension titled `title` from its entry
/// under the sidebar's Extensions group (#168).
fn open_page(settings_cx: &mut VisualTestContext, title: &str) {
    let entry = selector(format!("extension-entry-{title}"));
    click(settings_cx, entry);
    settings_cx.run_until_parked();
    assert!(
        settings_cx
            .debug_bounds(selector(format!("extension-page-title-{title}")))
            .is_some(),
        "{title}'s page opened"
    );
}

/// Opens the extension page's Actions menu and clicks its item whose
/// debug selector is `item`.
fn choose_in_menu(settings_cx: &mut VisualTestContext, item: &'static str) {
    click_row(settings_cx, "extension-menu");
    assert!(
        settings_cx.debug_bounds("extension-menu-popup").is_some(),
        "the menu opened"
    );
    click_row(settings_cx, item);
}

/// Whether the node with `role` labelled `label` is in the window's
/// accessibility tree.
fn has_node(cx: &mut VisualTestContext, role: &str, label: &str) -> bool {
    let (_, json) = accessibility(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .any(|node| node["aria"]["role"] == role && node["aria"]["label"] == label)
}

#[gpui::test]
fn the_extensions_group_lists_each_package_with_its_description(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = described_package(&sources.path().join("described"));
    let (_launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);

    // The group's page lists the extension with what it does, one line
    // under its source, cut like it (#224); the description itself is on
    // the extension's page.
    assert!(
        settings_cx.debug_bounds("extension-item-Described sample").is_some(),
        "the extension is listed"
    );
    assert!(
        settings_cx.debug_bounds("extension-item-description").is_some(),
        "its description is drawn"
    );
}

#[gpui::test]
fn every_installed_extension_has_a_sidebar_entry_and_a_page(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    let (launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);

    // The group's page lists the extension, what governs them all, and
    // the install sources; the sidebar has the extension's own entry under
    // the group's, with its + menu.
    for drawn in [
        "extension-entry-Settings sample",
        "extensions-add",
        "extension-item-Settings sample",
        "extension-row-Update extensions automatically",
        "extension-install-Folder…",
        "extension-install-npm…",
        "extension-install-Git…",
    ] {
        assert!(
            settings_cx.debug_bounds(drawn).is_some(),
            "{drawn} is drawn"
        );
    }
    // No explanatory paragraphs, no operations as rows on the group's page.
    assert!(
        settings_cx
            .debug_bounds("extension-row-Reload Settings sample")
            .is_none()
    );

    // Its page: the large icon, the title, what it does, its source, the
    // enable switch, the Actions menu, and its commands with their alias,
    // hotkey and switch.
    open_page(&mut settings_cx, "Settings sample");
    for drawn in [
        "extension-page-icon",
        "extension-page-description",
        "extension-page-source",
        "extension-row-Settings sample",
        "extension-menu",
        "extension-commands",
    ] {
        assert!(
            settings_cx.debug_bounds(drawn).is_some(),
            "{drawn} is drawn"
        );
    }
    let key = PackageIdentity::local(&folder).unwrap().key();
    let command = format!("{key}#greeting");
    for drawn in [
        format!("extension-command-{command}"),
        format!("extension-command-toggle-{command}"),
        format!("shortcut-alias-{command}"),
        format!("shortcut-hotkey-{command}"),
    ] {
        assert!(
            settings_cx.debug_bounds(selector(drawn.clone())).is_some(),
            "{drawn} is drawn"
        );
    }
    assert!(extension_enabled(&mut settings_cx, "Settings sample"));
    assert!(has_node(&mut settings_cx, "Switch", "Greeting enabled"));
    // The sidebar's selected entry is the extension's, and the titlebar
    // names it.
    assert!(has_node(
        &mut settings_cx,
        "ListBoxOption",
        "Settings sample"
    ));
    assert!(has_node(&mut settings_cx, "Heading", "Settings sample"));

    // The Actions menu offers every operation the old flow had, and the
    // page's own: Check for Update (not for a folder's extension, which
    // says why), Show Source Folder.
    click_row(&mut settings_cx, "extension-menu");
    for drawn in [
        "extension-menu-Check for Update",
        "extension-row-Reload Settings sample",
        "extension-row-Clear cache of Settings sample",
        "extension-row-Develop Settings sample",
        "extension-menu-Show Source Folder",
        "extension-row-Uninstall Settings sample",
    ] {
        assert!(
            settings_cx.debug_bounds(drawn).is_some(),
            "{drawn} is drawn"
        );
    }

    // Reading the pages moved nothing: the launcher stayed where it was,
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
fn a_paused_extension_is_marked_in_the_sidebar_and_on_its_page(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = hello_package(&sources.path().join("hello"));
    let (_launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);
    assert!(
        settings_cx
            .debug_bounds("extension-entry-mark-Hello")
            .is_none(),
        "nothing to say yet"
    );

    // A reload that fails to start pauses it: its entry says so in a
    // word, and its page in full.
    open_page(&mut settings_cx, "Hello");
    rebuild(&folder, "failing_start");
    choose_in_menu(&mut settings_cx, "extension-row-Reload Hello");
    until_text(&mut settings_cx, "Reloaded Hello, but it failed to start");
    assert!(
        settings_cx
            .debug_bounds("extension-entry-mark-Hello")
            .is_some(),
        "the entry is marked"
    );
    assert!(has_node(&mut settings_cx, "ListBoxOption", "Hello, Paused"));
    assert!(
        settings_cx.debug_bounds("extension-page-mark").is_some(),
        "the page says why"
    );
}

#[gpui::test]
fn disabling_a_required_extension_from_its_page_confirms_and_disables_all(cx: &mut TestAppContext) {
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

    // The page's switch for the dependency enters the launcher's own flow
    // and asks the required-dependent confirmation, in place of the page —
    // the same question, rows and records the launcher's list asks.
    open_page(&mut settings_cx, "Greeter");
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

    // Cancel keeps everything enabled and returns to the page.
    click_row(&mut settings_cx, "extension-row-Cancel");
    assert!(
        settings_cx
            .debug_bounds("extension-page-title-Greeter")
            .is_some(),
        "back on the page"
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
    assert!(!extension_enabled(&mut settings_cx, "Greeter"));

    // Root search offers the commands of neither disabled package, and
    // "Manage Extensions" is a command of Pane's.
    cx.read_entity(&launcher, |window, _| window.launcher().back());
    assert_eq!(
        titles(&launcher, cx),
        [
            "Install extension from folder…",
            "Install extension from npm…",
            "Install extension from Git…",
            "Manage Extensions",
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

    // Uninstall, from the page's menu, asks first, with the saved-data
    // choice the launcher's own confirmation offers.
    open_page(&mut settings_cx, "Settings sample");
    choose_in_menu(&mut settings_cx, "extension-row-Uninstall Settings sample");
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
    // Nothing is installed: its entry and page are gone, and the group's
    // page lists the retained data with its own row and confirmation.
    assert!(
        cx.read_entity(&launcher, |window, _| window.launcher().packages())
            .is_empty(),
        "nothing is installed"
    );
    assert!(
        settings_cx
            .debug_bounds("extension-entry-Settings sample")
            .is_none()
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
    open_page(&mut settings_cx, "Hello");

    // A source that no longer builds a startable package: the reload's
    // failure is the page's status, and the paused package's Retry is
    // offered in its menu, as in the launcher's list.
    rebuild(&folder, "failing_start");
    choose_in_menu(&mut settings_cx, "extension-row-Reload Hello");
    until_text(&mut settings_cx, "Reloaded Hello, but it failed to start");

    // Retry starts it again, through the same row the launcher's list
    // holds.
    choose_in_menu(&mut settings_cx, "extension-row-Retry starting Hello");
    until_text(&mut settings_cx, "Started Hello");
    click_row(&mut settings_cx, "extension-menu");
    assert!(
        settings_cx
            .debug_bounds("extension-row-Retry starting Hello")
            .is_none(),
        "the package is no longer paused"
    );
}

#[gpui::test]
fn clearing_the_cache_from_the_page_asks_first(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    let (_launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);
    open_page(&mut settings_cx, "Settings sample");

    choose_in_menu(
        &mut settings_cx,
        "extension-row-Clear cache of Settings sample",
    );
    for row in ["extension-row-Clear cache", "extension-row-Cancel"] {
        assert!(settings_cx.debug_bounds(row).is_some(), "{row} is drawn");
    }
    click_row(&mut settings_cx, "extension-row-Clear cache");
    until_text(&mut settings_cx, "Cleared the cache of Settings sample");
    assert!(
        settings_cx
            .debug_bounds("extension-page-title-Settings sample")
            .is_some(),
        "back on the page"
    );
}

/// A link opener that also records the folders it is asked to show.
#[derive(Default)]
struct RecordedFiles(std::sync::Mutex<Vec<PathBuf>>);

impl pane_core::LinkOpener for RecordedFiles {
    fn open(&self, _url: &str) -> Result<(), String> {
        Ok(())
    }

    fn open_file(&self, path: &Path) -> Result<(), String> {
        self.0.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }
}

#[gpui::test]
fn show_source_folder_opens_the_extensions_folder(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    let files = Arc::new(RecordedFiles::default());
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_link_opener(files.clone());
    install(&launcher, &folder);
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (_launcher, cx) =
        cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let (_settings, mut settings_cx) = open_extensions(cx);
    open_page(&mut settings_cx, "Settings sample");

    // A folder's extension: its source folder, which its author edits.
    choose_in_menu(&mut settings_cx, "extension-menu-Show Source Folder");
    until_text(&mut settings_cx, "Showed");
    let identity = PackageIdentity::local(&folder).unwrap();
    assert_eq!(
        files.0.lock().unwrap().as_slice(),
        [identity.local_folder().unwrap().to_path_buf()]
    );
}

#[gpui::test]
fn check_for_update_is_explained_for_a_folders_extension(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    let (launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);
    open_page(&mut settings_cx, "Settings sample");

    // A folder's extension has no source to check: the entry says why and
    // does nothing (an npm or Git one previews its source: `npm.rs`).
    click_row(&mut settings_cx, "extension-menu");
    assert!(has_node(&mut settings_cx, "MenuItem", "Check for Update"));
    click_row(&mut settings_cx, "extension-menu-Check for Update");
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
}

#[gpui::test]
fn a_command_is_turned_off_and_on_from_its_extensions_page(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    let (launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);
    open_page(&mut settings_cx, "Settings sample");
    let key = PackageIdentity::local(&folder).unwrap().key();
    let toggle = selector(format!("extension-command-toggle-{key}#greeting"));

    // Off: root search no longer offers it, the rest of the extension
    // stays enabled, and the choice is recorded.
    click_row(&mut settings_cx, toggle);
    until_text(&mut settings_cx, "Turned off Greeting");
    assert!(!has_node_toggled(&mut settings_cx, "Greeting enabled"));
    assert!(extension_enabled(&mut settings_cx, "Settings sample"));
    cx.read_entity(&launcher, |window, _| window.launcher().back());
    assert!(
        !titles(&launcher, cx)
            .iter()
            .any(|title| title == "Greeting"),
        "root search no longer offers it"
    );
    let installed = fs::read_to_string(data.path().join("extensions/installed.json")).unwrap();
    assert!(installed.contains("\"greeting\""), "{installed}");

    // On again: it is back.
    click_row(&mut settings_cx, toggle);
    until_text(&mut settings_cx, "Turned on Greeting");
    assert!(has_node_toggled(&mut settings_cx, "Greeting enabled"));
    assert!(
        titles(&launcher, cx)
            .iter()
            .any(|title| title == "Greeting"),
        "root search offers it again"
    );
}

/// Whether the switch labelled `label` reads as on, as assistive
/// technology sees it.
fn has_node_toggled(cx: &mut VisualTestContext, label: &str) -> bool {
    let (_, json) = accessibility(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    tree["nodes"].as_object().unwrap().values().any(|node| {
        let aria = &node["aria"];
        aria["role"] == "Switch" && aria["label"] == label && aria["toggled"] == "True"
    })
}

#[gpui::test]
fn a_commands_alias_is_set_on_its_extensions_page(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    let (launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);
    open_page(&mut settings_cx, "Settings sample");
    let key = PackageIdentity::local(&folder).unwrap().key();
    let command = format!("{key}#greeting");

    // The alias field is the Shortcuts page's own: its editor opens in
    // place, Enter commits through the same rules and record.
    click_row(
        &mut settings_cx,
        selector(format!("shortcut-alias-{command}")),
    );
    assert!(settings_cx.debug_bounds("shortcut-editor").is_some());
    settings_cx.simulate_input("gr");
    settings_cx.simulate_keystrokes("enter");
    let alias = || {
        cx.read_entity(&launcher, |window, _| {
            window
                .launcher()
                .shortcut_catalog()
                .groups
                .into_iter()
                .flat_map(|group| group.commands)
                .find(|shortcut| shortcut.id == command)
                .and_then(|shortcut| shortcut.alias)
        })
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while alias().as_deref() != Some("gr") {
        assert!(Instant::now() < deadline, "the alias was never set");
        settings_cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[gpui::test]
fn an_extension_is_installed_from_a_folder_through_the_plus_menu(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = hello_package(&sources.path().join("hello"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let (_launcher, cx) =
        cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let (_settings, mut settings_cx) = open_extensions(cx);
    assert!(settings_cx.debug_bounds("extension-empty").is_some());

    // The group's + menu offers the three sources.
    click_row(&mut settings_cx, "extensions-add");
    for drawn in [
        "extensions-add-Install from Folder…",
        "extensions-add-Install from npm…",
        "extensions-add-Install from Git…",
    ] {
        assert!(
            settings_cx.debug_bounds(drawn).is_some(),
            "{drawn} is drawn"
        );
    }

    // Cancelling the folder picker changes nothing.
    click_row(&mut settings_cx, "extensions-add-Install from Folder…");
    assert!(settings_cx.did_prompt_for_paths(), "a folder picker opened");
    settings_cx.simulate_path_prompt_response(|_| None);
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds("extension-empty").is_some());

    // A folder chosen is previewed on the page, and its Install installs
    // it: the extension has its entry and its page.
    click_row(&mut settings_cx, "extensions-add");
    click_row(&mut settings_cx, "extensions-add-Install from Folder…");
    assert!(settings_cx.did_prompt_for_paths(), "a folder picker opened");
    let chosen = folder.clone();
    settings_cx.simulate_path_prompt_response(move |options| {
        assert!(options.directories && !options.files && !options.multiple);
        Some(vec![chosen])
    });
    until_drawn(&mut settings_cx, "extension-detail-Version: 1.0.0");
    click_row(&mut settings_cx, "extension-row-Install");
    until_text(&mut settings_cx, "Installed Hello");
    assert!(
        settings_cx.debug_bounds("extension-entry-Hello").is_some(),
        "its entry is listed"
    );
    open_page(&mut settings_cx, "Hello");
}

#[gpui::test]
fn the_npm_and_git_sources_ask_for_their_package_on_the_page(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let (_launcher, cx) =
        cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let (settings, mut settings_cx) = open_extensions(cx);

    // npm: the field takes the keyboard; a name that is no npm package's
    // is explained by the preview, on the page.
    click_row(&mut settings_cx, "extensions-add");
    click_row(&mut settings_cx, "extensions-add-Install from npm…");
    assert!(
        settings_cx
            .debug_bounds("extension-install-field")
            .is_some()
    );
    let field = settings
        .read_with(&settings_cx, |window, _| window.install_field())
        .unwrap()
        .expect("the install field");
    {
        use gpui::Focusable;
        assert!(
            settings_cx.update(|window, cx| field.focus_handle(cx).is_focused(window)),
            "the field has the keyboard"
        );
    }
    settings_cx.simulate_input("Not A Package!");
    click_row(&mut settings_cx, "extension-install-show");
    until_text(
        &mut settings_cx,
        "`Not A Package!` is not an npm package name",
    );
    click_row(&mut settings_cx, "extension-back");

    // Git: the same field asks for a repository.
    click_row(&mut settings_cx, "extension-install-Git…");
    assert!(
        has_node(
            &mut settings_cx,
            "TextInput",
            "Git repository: its address, and @ a branch, tag or commit to install that one"
        ),
        "the field asks for a repository"
    );
}

#[gpui::test]
fn the_window_keeps_its_keys_once_the_install_field_has_gone(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let (_launcher, cx) =
        cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    let (_settings, mut settings_cx) = open_extensions(cx);

    // The field has the keyboard, then goes once Show Package previews
    // what it named: the keyboard is the window's again, so its close
    // shortcut closes it (release run 37725624283's smokes pressed it
    // there, and it reached nothing).
    click_row(&mut settings_cx, "extensions-add");
    click_row(&mut settings_cx, "extensions-add-Install from npm…");
    settings_cx.simulate_input("Not A Package!");
    click_row(&mut settings_cx, "extension-install-show");
    until_text(
        &mut settings_cx,
        "`Not A Package!` is not an npm package name",
    );
    assert!(
        settings_cx
            .debug_bounds("extension-install-field")
            .is_none()
    );
    settings_cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-w"
    } else {
        "ctrl-w"
    });
    cx.run_until_parked();
    assert!(settings_windows(cx).is_empty(), "Settings closed");
}

#[gpui::test]
fn manage_extensions_and_the_install_rows_open_settings(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    let (launcher, cx) = open_installed(cx, &data, &folder);

    // "Manage Extensions" is a command of Pane's: it opens Settings at
    // the Extensions group, and the launcher stays on root search — it
    // has no screen of its own for extensions.
    cx.simulate_input("manage extensions");
    settle(&launcher, cx);
    assert!(settings_windows(cx).is_empty());
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();
    assert!(settings_cx.debug_bounds("extensions-title").is_some());
    assert!(has_node(&mut settings_cx, "ListBoxOption", "Extensions"));
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );

    // Root search's install rows open Settings' install flow: npm's
    // field, focused.
    cx.simulate_keystrokes("escape");
    cx.simulate_input("install extension from npm");
    settle(&launcher, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(
        settings_cx
            .debug_bounds("extension-install-field")
            .is_some(),
        "the npm field is asked for"
    );
    let view = cx.read_entity(&launcher, |window, _| window.launcher().view());
    assert!(
        !matches!(view.screen, Screen::Form(_)),
        "the launcher shows no form of its own"
    );
}

#[gpui::test]
fn the_search_finds_an_extension_and_its_preferences(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = assembled("sample-preferences", &sources.path().join("preferences"));
    let (_launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);

    // The extension by its title.
    settings_cx.simulate_keystrokes(find_shortcut());
    settings_cx.simulate_input("preferences sample");
    settings_cx.run_until_parked();
    assert!(
        settings_cx
            .debug_bounds("settings-search-result-Preferences sample")
            .is_some(),
        "the extension is found"
    );
    settings_cx.simulate_keystrokes("escape");
    settings_cx.run_until_parked();

    // A preference by its title: Enter opens the extension's page there.
    settings_cx.simulate_input("units");
    settings_cx.run_until_parked();
    assert!(
        settings_cx
            .debug_bounds("settings-search-result-Units")
            .is_some(),
        "the preference is found"
    );
    settings_cx.simulate_keystrokes("enter");
    settings_cx.run_until_parked();
    for _ in 0..3 {
        settings_cx.update(|window, _| window.refresh());
        settings_cx.run_until_parked();
    }
    assert!(
        settings_cx
            .debug_bounds("extension-page-title-Preferences sample")
            .is_some(),
        "the extension's page opened"
    );
    assert!(settings_cx.debug_bounds("preference-units").is_some());
}

/// An assembled package `name` from `cargo xtask guests`, copied into
/// `folder` with every file it has.
fn assembled(name: &str, folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(name);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    copy_tree(&assembled, folder);
    folder.to_path_buf()
}

/// Copies the folder `from` into `to`, recursively.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[gpui::test]
fn the_page_follows_a_change_made_elsewhere(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    let (launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);
    open_page(&mut settings_cx, "Settings sample");
    assert!(extension_enabled(&mut settings_cx, "Settings sample"));

    // The package is disabled elsewhere (the launcher, in the
    // background): the page, open on it, redraws with the state the
    // launcher now holds, by its watcher.
    let identity = PackageIdentity::local(&folder).unwrap();
    let core = cx.read_entity(&launcher, |window, _| window.launcher().clone());
    futures::executor::block_on(core.set_enabled(&identity, false));
    let deadline = Instant::now() + Duration::from_secs(10);
    while extension_enabled(&mut settings_cx, "Settings sample") {
        assert!(Instant::now() < deadline, "the page never followed");
        settings_cx
            .cx
            .executor()
            .advance_clock(Duration::from_millis(600));
        settings_cx.run_until_parked();
    }
}

/// Whether the switch of the extension `title` on its page reads as on, as
/// assistive technology sees it.
fn extension_enabled(cx: &mut VisualTestContext, title: &str) -> bool {
    let (_, json) = accessibility(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    tree["nodes"].as_object().unwrap().values().any(|node| {
        let aria = &node["aria"];
        aria["role"] == "Switch" && aria["label"] == title && aria["toggled"] == "True"
    })
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
    open_page(&mut settings_cx, "Hello");

    // Development starts from the page's menu: the same row the
    // launcher's list holds, entered through the same flow.
    choose_in_menu(&mut settings_cx, "extension-row-Develop Hello");
    until_text(&mut settings_cx, "Developing Hello");

    // A save that does not build: the development thread reports it, the
    // changes channel wakes the launcher window, and the page redraws with
    // what the launcher holds — by itself, with no action on it.
    fs::write(folder.join("source.txt"), "error: expected `;`").unwrap();
    until_text(&mut settings_cx, "Hello did not build: error: expected `;`");
    click_row(&mut settings_cx, "extension-menu");
    assert!(
        settings_cx
            .debug_bounds("extension-row-Why Hello did not build")
            .is_some(),
        "the build-failure operation is offered"
    );
}

#[gpui::test]
fn the_page_opens_a_developed_extension_s_logs_in_the_launcher(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = hello_package(&sources.path().join("hello"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (sender, changes) = pane_core::changes::channel();
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_development(Arc::new(FakeBuilder), sender);
    install(&launcher, &folder);
    let (launcher, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher
    });
    let (_settings, mut settings_cx) = open_extensions(cx);
    open_page(&mut settings_cx, "Hello");
    choose_in_menu(&mut settings_cx, "extension-row-Develop Hello");
    until_text(&mut settings_cx, "Developing Hello");

    // Its Logs, from the page's menu, beside Stop Developing: the launcher
    // window shows them, following the lines as they come.
    choose_in_menu(&mut settings_cx, "extension-row-Logs for Hello");
    let view = settle::until(&launcher, cx, |view| {
        matches!(view.screen, Screen::ExtensionLog { .. })
    });
    assert_eq!(view.title, "Logs for Hello");
    let shown = cx.read_entity(&launcher, |window, _| window.extension_log_shown());
    let (lines, selected, following) = shown.expect("the Logs screen shows");
    assert!(lines > 0 && following);
    assert_eq!(selected, Some(lines - 1));
}

#[gpui::test]
fn the_settings_window_keeps_its_layout_at_small_sizes(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();

    // The window opens on the General page — the demanding one: the Open
    // Pane hotkey's recorder and the Appearance section's choices sit at
    // the ends of its rows. At the window's floor, its smallest usable
    // size, the sidebar and the page both stay laid out — nothing reaches
    // past the panel's edge.
    settings_cx.simulate_resize(gpui::size(px(560.), px(400.)));
    settings_cx.run_until_parked();
    let sidebar = settings_cx
        .debug_bounds("section-General")
        .expect("the sidebar is laid out");
    let page = settings_cx
        .debug_bounds("settings-page")
        .expect("the page is laid out");
    let recorder = settings_cx
        .debug_bounds("open-pane-recorder")
        .expect("the hotkey's recorder is laid out");
    let choice = settings_cx
        .debug_bounds("appearance-theme-System")
        .expect("the page's choice row is laid out");
    let material = settings_cx
        .debug_bounds("appearance-material-track")
        .expect("the material's choice is laid out");
    assert!(
        sidebar.right() <= page.left(),
        "the sidebar is beside the page"
    );
    // The page scrolls when the window is short, so vertical position is
    // not containment; the rows' controls must stay within its width.
    assert!(
        recorder.right() <= page.right(),
        "the recorder stays within the page"
    );
    assert!(
        choice.right() <= page.right() && material.right() <= page.right(),
        "the choices stay within the page"
    );

    // The About page keeps its own rows laid out at the same floor,
    // reached through the sidebar. The sidebar's list scrolls inside it
    // when the window is short, so its wheel is turned to bring the About
    // section's row into view before it is clicked.
    let sections = settings_cx
        .debug_bounds("sections")
        .expect("the sections list");
    settings_cx.simulate_event(gpui::ScrollWheelEvent {
        position: sections.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-120.))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    settings_cx.run_until_parked();
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

/// The bounds the element with the debug selector `selector` drew at, as
/// `[x, y, width, height]` in logical px.
fn rect_of(cx: &mut VisualTestContext, selector: &'static str) -> [f32; 4] {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"));
    let (origin, size) = (bounds.origin, bounds.size);
    [
        f32::from(origin.x),
        f32::from(origin.y),
        f32::from(size.width),
        f32::from(size.height),
    ]
}

/// A debug selector built at run time, for the selectors a test derives
/// from a page's title.
fn selector(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// The Settings window opens at Pane's 860×600 client — narrower than the
/// reference board's 1120×720, as a list of settings rows needs no more —
/// and its shell lands where the board puts it (#97): the 48px titlebar
/// with its label centered and the Windows caption buttons at its right
/// edge, the 232px sidebar below it with the 34px search and the 36px
/// sections 2px apart, and the page beside the sidebar with Pane's own
/// 20px top and 24px side padding.
#[cfg(target_os = "windows")]
#[gpui::test]
fn the_settings_window_opens_at_the_reference_shell_geometry(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    let (_settings, mut settings_cx) = opened_settings(cx);

    let viewport = settings_cx.update(|window, _| window.viewport_size());
    assert_eq!(
        viewport,
        gpui::size(px(860.), px(600.)),
        "the Settings window's client"
    );
    let sc = &mut settings_cx;
    assert_eq!(rect_of(sc, "settings-titlebar"), [0., 0., 860., 48.]);
    assert_eq!(rect_of(sc, "settings-sidebar"), [0., 48., 232., 552.]);
    assert_eq!(rect_of(sc, "settings-search-field"), [10., 60., 211., 34.]);
    assert_eq!(rect_of(sc, "section-General"), [10., 104., 211., 36.]);
    assert_eq!(rect_of(sc, "section-Launcher"), [10., 142., 211., 36.]);
    assert_eq!(rect_of(sc, "settings-page"), [232., 48., 628., 552.]);
    let content = rect_of(sc, "general");
    assert_eq!([content[0], content[1]], [256., 68.], "the page's padding");

    // The label is centered over the whole window, as the reference's is;
    // the caption buttons — the Windows adaptation of its lone close
    // glyph — reach the window's right edge and fill the titlebar's
    // height above its rule.
    let label = rect_of(sc, "settings-title");
    assert!(
        (label[0] + label[2] / 2. - 430.).abs() <= 1.,
        "the label is centered: {label:?}"
    );
    let close = rect_of(sc, "window-close");
    assert_eq!(close[0] + close[2], 860.);
    assert_eq!([close[1], close[3]], [0., 47.]);

    // At the window's floor the titlebar and its controls hold: the
    // caption buttons still end at the window's edge, the label stays
    // centered clear of them, and the sidebar keeps its width.
    sc.simulate_resize(gpui::size(px(560.), px(400.)));
    sc.run_until_parked();
    let close = rect_of(sc, "window-close");
    assert_eq!(close[0] + close[2], 560.);
    let label = rect_of(sc, "settings-title");
    assert!((label[0] + label[2] / 2. - 280.).abs() <= 1., "{label:?}");
    assert_eq!(rect_of(sc, "settings-sidebar")[2], 232.);
}

/// The sidebar's sections are the reference's own item family (#97), not
/// the launcher's result rows: 36px, the white 9% selected wash and the
/// white 5% hover wash, which lands on the frame the pointer moves and
/// never fades — the reference's `.nav` authors no transition — while the
/// selected section keeps its wash under the pointer.
#[gpui::test]
fn the_sidebar_items_are_their_own_family_and_change_at_once(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    let (_settings, mut settings_cx) = opened_settings(cx);

    let general = settings_cx
        .debug_bounds("section-General")
        .expect("the General section");
    assert_eq!(general.size.height, px(36.), "the sidebar item's height");
    assert!(
        paints_fill_at(&mut settings_cx, general, 0xFFFFFF17),
        "the selected section takes the sidebar's white 9% wash"
    );

    // The pointer onto an unselected section: the hover wash at once, and
    // nothing left running.
    let launcher_row = settings_cx
        .debug_bounds("section-Launcher")
        .expect("the Launcher section");
    settings_cx.simulate_mouse_move(
        launcher_row.center(),
        None::<MouseButton>,
        Modifiers::none(),
    );
    settings_cx.run_until_parked();
    assert!(
        paints_fill_at(&mut settings_cx, launcher_row, 0xFFFFFF0D),
        "the hover wash is the sidebar's white 5%, drawn at once"
    );
    assert_eq!(
        frame(&mut settings_cx, Duration::ZERO),
        0,
        "the hover wash asks for no animation frame"
    );

    // Over the selected section the selected wash stays.
    settings_cx.simulate_mouse_move(general.center(), None::<MouseButton>, Modifiers::none());
    settings_cx.run_until_parked();
    assert!(paints_fill_at(&mut settings_cx, general, 0xFFFFFF17));
    assert!(!paints_fill_at(&mut settings_cx, general, 0xFFFFFF0D));
    assert!(
        !paints_fill_at(&mut settings_cx, launcher_row, 0xFFFFFF0D),
        "the section the pointer left lost its wash at once"
    );

    pointer_leaves(&mut settings_cx);
    settings_cx.run_until_parked();
    assert_eq!(settle_frames(&mut settings_cx), 0, "the window is idle");
}

/// The Appearance section's choices are the reference board's segmented
/// family (#98), not launcher rows: the section stands the page's 24px
/// under the General page's own card, its label over a card of two
/// settings rows — the theme's, then the material's, past the card's 1px
/// rule — each at least 48 high with its 200px track (36 high, black 24%
/// under its ring) at its end, 14px in and centered on the row; the 30px
/// segments share the track's width inside its 3px padding, 2px apart,
/// the chosen segment on the white 12% wash and no root-row wash on any of
/// them.
#[gpui::test]
fn the_appearance_choices_are_the_reference_segmented_family(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    let mut settings_cx = open_settings(cx);
    pointer_leaves(&mut settings_cx);
    settle_frames(&mut settings_cx);
    let sc = &mut settings_cx;

    let card = rect_of(sc, "general-card");
    let section = rect_of(sc, "appearance");
    assert!(
        (section[1] - (card[1] + card[3] + 24.)).abs() < 0.5,
        "the section follows the page's card: {section:?} under {card:?}"
    );
    let theme_field = rect_of(sc, "appearance-theme-field");
    let theme_track = rect_of(sc, "appearance-theme-track");
    let material_field = rect_of(sc, "appearance-material-field");
    let material_track = rect_of(sc, "appearance-material-track");
    assert!(
        theme_field[1] > section[1],
        "the rows sit under the section's label: {theme_field:?} in {section:?}"
    );
    assert!(
        (material_field[1] - (theme_field[1] + theme_field[3] + 1.)).abs() < 0.5,
        "the material's row follows the theme's: {material_field:?} under {theme_field:?}"
    );
    for (field, track) in [(theme_field, theme_track), (material_field, material_track)] {
        assert!(field[3] >= 48., "a settings row's floor: {field:?}");
        assert_eq!([track[2], track[3]], [200., 36.], "{track:?}");
        assert!(
            (track[0] + track[2] - (field[0] + field[2] - 14.)).abs() < 0.5,
            "the track at the row's end: {track:?} in {field:?}"
        );
        assert!(
            (track[1] + track[3] / 2. - (field[1] + field[3] / 2.)).abs() < 0.5,
            "the track centered on the row: {track:?} in {field:?}"
        );
    }

    // Three equal segments on the theme's track, two on the material's,
    // filling it inside its padding.
    let themes: Vec<_> = ["System", "Light", "Dark"]
        .into_iter()
        .map(|name| rect_of(sc, selector(format!("appearance-theme-{name}"))))
        .collect();
    let materials: Vec<_> = ["Glass", "Solid"]
        .into_iter()
        .map(|name| rect_of(sc, selector(format!("appearance-material-{name}"))))
        .collect();
    for (track, row) in [(theme_track, &themes), (material_track, &materials)] {
        let (first, last) = (row[0], row[row.len() - 1]);
        assert!(
            (first[0] - (track[0] + 3.)).abs() < 0.5 && (first[1] - (track[1] + 3.)).abs() < 0.5,
            "inside the track's padding: {first:?} in {track:?}"
        );
        assert!(
            (last[0] + last[2] - (track[0] + track[2] - 3.)).abs() < 0.5,
            "the segments fill the track: {row:?} in {track:?}"
        );
        for pair in row.windows(2) {
            assert!(
                (pair[1][0] - (pair[0][0] + pair[0][2] + 2.)).abs() < 0.5,
                "2px apart: {pair:?}"
            );
        }
        // Equal to the half pixel the layout's pixel snapping leaves when
        // the track does not divide evenly.
        for segment in row {
            assert!(
                (segment[2] - first[2]).abs() <= 0.5,
                "equal shares: {row:?}"
            );
            assert_eq!(segment[3], 30., "{segment:?}");
        }
    }

    // The fills: the track's black 24%, the chosen segment's white 12%
    // (Dark and Glass are the defaults), and nothing on the others.
    let track = sc
        .debug_bounds("appearance-theme-track")
        .expect("the track");
    assert!(
        paints_fill_at(sc, track, 0x0000003D),
        "the track's black 24%"
    );
    let dark = sc
        .debug_bounds("appearance-theme-Dark")
        .expect("the Dark segment");
    let system = sc
        .debug_bounds("appearance-theme-System")
        .expect("the System segment");
    assert!(paints_fill_at(sc, dark, 0xFFFFFF1F), "the chosen white 12%");
    assert!(!paints_fill_at(sc, system, 0xFFFFFF1F), "only the chosen");
    for bounds in [dark, system] {
        for root_wash in [0xFFFFFF16, 0xFFFFFF09] {
            assert!(
                !paints_fill_at(sc, bounds, root_wash),
                "no root-row wash on a segment"
            );
        }
    }

    // Choosing moves the wash at once, and leaves nothing running.
    click(sc, "appearance-material-Solid");
    sc.run_until_parked();
    assert!(chosen(sc, "Solid"), "the Solid choice is taken");
    let solid = sc
        .debug_bounds("appearance-material-Solid")
        .expect("the Solid segment");
    let glass = sc
        .debug_bounds("appearance-material-Glass")
        .expect("the Glass segment");
    assert!(paints_fill_at(sc, solid, 0xFFFFFF1F), "the wash moved");
    assert!(!paints_fill_at(sc, glass, 0xFFFFFF1F), "and left Glass");
    pointer_leaves(sc);
    sc.run_until_parked();
    assert_eq!(settle_frames(sc), 0, "the window is idle");
}

/// The keyboard reaches the segments (#98): Tab moves from the sidebar
/// onto them, and Enter or Space chooses the one it is on, as a click
/// does — and both windows follow.
#[gpui::test]
fn the_keyboard_reaches_and_chooses_a_segment(cx: &mut TestAppContext) {
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

    tab_to(&mut settings_cx, "Light");
    settings_cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(chosen(&mut settings_cx, "Light"), "Enter chose Light");
    assert!(
        paints_panel(cx, &light_panel()),
        "the launcher follows the keyboard's choice"
    );

    tab_to(&mut settings_cx, "Dark");
    settings_cx.simulate_keystrokes("space");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(chosen(&mut settings_cx, "Dark"), "Space chose Dark");
    assert!(paints_panel(cx, &dark_panel()));
    until_record(cx, data.path());
}

/// While an override is in force the segments are offered to neither the
/// pointer nor the keyboard (#98): Tab passes them by.
#[gpui::test]
fn overridden_segments_take_no_keyboard_focus(cx: &mut TestAppContext) {
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
    let mut settings_cx = open_settings(cx);
    // The choices are drawn, as segments, and offered to nothing.
    assert!(settings_cx.debug_bounds("appearance-theme-track").is_some());
    let mut reached = Vec::new();
    for _ in 0..8 {
        settings_cx.simulate_keystrokes("tab");
        reached.push(focused_label(&mut settings_cx).unwrap_or_default());
    }
    for name in ["System", "Light", "Dark", "Glass", "Solid"] {
        assert!(
            !reached.iter().any(|label| label == name),
            "{name} took the focus while overridden: {reached:?}"
        );
    }
}

/// The real pages — and only those — are the sidebar's sections, in
/// order: each opens its page, and the search finds each by its title.
/// (Six pages and the Extensions group; File Search joined with #176.)
/// Appearance is a section of the General page, not a page of its own,
/// and the reference's other labels (Window Manager, Clipboard, Privacy)
/// add no page.
#[gpui::test]
fn all_six_pages_are_listed_reachable_and_searchable(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    let (_settings, mut settings_cx) = opened_settings(cx);
    // Pane's own pages, then the Extensions group (#168).
    let pages = [
        "General",
        "Launcher",
        "Shortcuts",
        "Keyboard",
        "File Search",
        "About",
        "Extensions",
    ];
    let mut above = None;
    for title in pages {
        let section = selector(format!("section-{title}"));
        let top = settings_cx
            .debug_bounds(section)
            .unwrap_or_else(|| panic!("{section} is listed"))
            .top();
        assert!(above.is_none_or(|above| above < top), "{section} in order");
        above = Some(top);
        click_section(&mut settings_cx, section);
        let page = selector(title.to_lowercase().replace(' ', "-"));
        assert!(
            settings_cx.debug_bounds(page).is_some(),
            "{section} opens its page"
        );
    }
    for absent in [
        "section-Appearance",
        "section-Window Manager",
        "section-Clipboard",
        "section-Privacy",
    ] {
        assert!(settings_cx.debug_bounds(absent).is_none(), "{absent}");
    }
    for title in pages {
        settings_cx.simulate_keystrokes(find_shortcut());
        settings_cx.simulate_input(&title.to_lowercase());
        settings_cx.run_until_parked();
        let result = selector(format!("settings-search-result-{title}"));
        assert!(
            settings_cx.debug_bounds(result).is_some(),
            "the search finds {title}"
        );
        settings_cx.simulate_keystrokes("escape");
        settings_cx.run_until_parked();
    }
}

/// The General page is composed of the Settings families (#99), not
/// launcher rows: its own settings rows stand in a card, one under the
/// other past the card's 1px rule, each at least 48 high; the Open Pane
/// hotkey's recorder is a 36px well (black 24%) at its row's end, 14px
/// in, with the Reset button inside it, and the launch-at-login choice is
/// the board's 40x24 switch at its row's end — white 16% with its knob at
/// the left while off, the accent with the knob at the right once taken.
/// No root-row wash is painted on the rows, and nothing fades.
#[gpui::test]
fn the_general_page_draws_the_settings_control_families(cx: &mut TestAppContext) {
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
    let (_settings, mut settings_cx) = opened_settings(cx);
    let sc = &mut settings_cx;

    let card = rect_of(sc, "general-card");
    let row = rect_of(sc, "general-open-pane-row");
    let login_row = rect_of(sc, "general-launch-at-login-row");
    assert_eq!(row[1], card[1], "the card's first row");
    assert!(
        (login_row[1] - (row[1] + row[3] + 1.)).abs() < 0.5,
        "the next row past the card's rule: {login_row:?} under {row:?}"
    );
    for bounds in [row, login_row] {
        assert!(bounds[3] >= 48., "a settings row's floor: {bounds:?}");
    }

    let recorder = sc.debug_bounds("open-pane-recorder").expect("the recorder");
    let row_bounds = sc.debug_bounds("general-open-pane-row").expect("its row");
    assert_eq!(recorder.size.height, px(36.), "the recorder's field");
    assert_eq!(
        recorder.right(),
        row_bounds.right() - px(14.),
        "at the row's end"
    );
    assert!(
        paints_fill_at(sc, recorder, 0x0000003D),
        "the well's black 24%"
    );
    let reset = sc.debug_bounds("open-pane-reset").expect("the reset");
    assert!(
        recorder.contains(&reset.center()),
        "the reset inside the recorder: {reset:?} in {recorder:?}"
    );
    for root_wash in [0xFFFFFF09, 0xFFFFFF16] {
        assert!(
            !paints_fill_at(sc, row_bounds, root_wash),
            "no root-row wash on a settings row"
        );
    }

    let switch = sc
        .debug_bounds("general-launch-at-login")
        .expect("the switch");
    let switch_row = sc
        .debug_bounds("general-launch-at-login-row")
        .expect("its row");
    assert_eq!(switch.size, gpui::size(px(40.), px(24.)), "the switch");
    assert_eq!(
        switch.right(),
        switch_row.right() - px(14.),
        "at the row's end"
    );
    let knob = |switch: gpui::Bounds<gpui::Pixels>, left: f32| gpui::Bounds {
        origin: switch.origin + gpui::point(px(left), px(3.)),
        size: gpui::size(px(18.), px(18.)),
    };
    assert!(paints_fill_at(sc, switch, 0xFFFFFF29), "off: white 16%");
    assert!(
        paints_fill_at(sc, knob(switch, 3.), 0xFFFFFFFF),
        "the knob at left"
    );
    choose(sc, "general-launch-at-login");
    cx.run_until_parked();
    until_record_holds(sc, data.path(), "\"launchAtLogin\": true");
    assert!(login_chosen(sc), "the choice is taken");
    // Read the switch again: on Linux the choice adds the autostart note
    // under the row's name, the row grows and the switch, centred in it,
    // moves down with it.
    let switch = sc
        .debug_bounds("general-launch-at-login")
        .expect("the switch");
    assert!(paints_fill_at(sc, switch, 0xC9EE6AFF), "on: the accent");
    assert!(
        paints_fill_at(sc, knob(switch, 19.), 0xFFFFFFFF),
        "the knob at right"
    );

    pointer_leaves(sc);
    sc.run_until_parked();
    assert_eq!(settle_frames(sc), 0, "nothing fades: the window is idle");
}

/// The Keyboard page shows each action's binding as the reference's caps
/// in a recorder's well (#99) at the end of its settings row — the
/// binding's caps come from the launcher's own adapter — and offers a
/// reset only for an action that is not at its default.
#[gpui::test]
fn the_keyboard_page_shows_bindings_in_recorder_wells(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    let (_settings, mut settings_cx) = opened_settings(cx);
    let sc = &mut settings_cx;
    click_section(sc, "section-Keyboard");
    pointer_leaves(sc);
    settle_frames(sc);

    let row = sc
        .debug_bounds("keyboard-row-next-result")
        .expect("the action's row");
    let well = sc
        .debug_bounds("keyboard-next-result")
        .expect("the recorder");
    assert_eq!(well.size.height, px(36.), "the recorder's field");
    assert_eq!(well.right(), row.right() - px(14.), "at the row's end");
    assert!(row.size.height >= px(44.), "a settings row's floor");
    assert!(paints_fill_at(sc, well, 0x0000003D), "the well's black 24%");
    // The reset sits inside the recorder, offered only away from the
    // default.
    let reset = sc
        .debug_bounds("keyboard-reset-next-result")
        .expect("the reset");
    assert!(well.contains(&reset.center()), "{reset:?} in {well:?}");
    // The actions' section follows the Behavior section.
    let field = rect_of(sc, "keyboard-field");
    let behavior = rect_of(sc, "keyboard-escape-field");
    assert!(field[1] > behavior[1] + behavior[3]);
}

/// The About page's actions are the Settings buttons (#99): 30px, white
/// 8% at rest and white 13% under the pointer, at once — no fade, no
/// frame asked for — each at the end of its settings row in the page's
/// card, under the version's row, as the version sits at the end of its
/// own.
#[gpui::test]
fn the_about_pages_actions_are_buttons_whose_washes_change_at_once(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    let (_settings, mut settings_cx) = opened_settings(cx);
    let sc = &mut settings_cx;
    click_section(sc, "section-About");
    pointer_leaves(sc);
    settle_frames(sc);

    for selector in ["about-documentation", "about-diagnostics"] {
        let button = sc.debug_bounds(selector).expect("the button");
        assert_eq!(button.size.height, px(30.), "{selector}: a button");
        assert!(
            paints_fill_at(sc, button, 0xFFFFFF14),
            "{selector}: white 8% at rest"
        );
        sc.simulate_mouse_move(button.center(), None::<MouseButton>, Modifiers::none());
        sc.run_until_parked();
        assert!(
            paints_fill_at(sc, button, 0xFFFFFF21),
            "{selector}: white 13% under the pointer"
        );
        assert_eq!(
            frame(sc, Duration::ZERO),
            0,
            "{selector}: the hover asks for no frame"
        );
        pointer_leaves(sc);
        sc.run_until_parked();
    }
    // The rows' controls share the card's right edge, one row under the
    // other: the version, the documentation's button, the diagnostics'.
    let version = rect_of(sc, "about-version");
    let documentation = rect_of(sc, "about-documentation");
    let diagnostics = rect_of(sc, "about-diagnostics");
    for control in [documentation, diagnostics] {
        assert!(
            (control[0] + control[2] - (version[0] + version[2])).abs() < 0.5,
            "at its row's end: {control:?} beside {version:?}"
        );
    }
    assert!(
        documentation[1] >= version[1] + version[3],
        "the documentation's row is under the version's"
    );
    assert!(
        diagnostics[1] >= documentation[1] + documentation[3],
        "the diagnostics' row is under the documentation's"
    );
}

/// The Extensions group's page lists the installed extensions as Settings
/// list items (#99, #168), not root result rows: each at least 44 high, white 5%
/// under the pointer (the sidebar item's hover), at once, and no root-row
/// wash.
#[gpui::test]
fn the_extensions_page_lists_its_rows_as_list_items(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"));
    let (_launcher, cx) = open_installed(cx, &data, &folder);
    let (_settings, mut settings_cx) = open_extensions(cx);
    let sc = &mut settings_cx;
    pointer_leaves(sc);
    settle_frames(sc);

    let selector = "extension-item-Settings sample";
    let row = sc.debug_bounds(selector).expect("the row");
    assert!(row.size.height >= px(44.), "{selector}: {row:?}");
    sc.simulate_mouse_move(row.center(), None::<MouseButton>, Modifiers::none());
    sc.run_until_parked();
    assert!(
        paints_fill_at(sc, row, 0xFFFFFF0D),
        "{selector}: the list item's white 5% under the pointer"
    );
    assert!(
        !paints_fill_at(sc, row, 0xFFFFFF09),
        "{selector}: not the root row's hover"
    );
    assert_eq!(frame(sc, Duration::ZERO), 0, "{selector}: at once");
    pointer_leaves(sc);
    sc.run_until_parked();
}

/// A helper for the section-transition tests: the Settings window over
/// the sample launcher, opened and settled, as (window, its context).
fn opened_settings(
    cx: &mut VisualTestContext,
) -> (WindowHandle<SettingsWindow>, VisualTestContext) {
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = settings_windows(cx).pop().expect("Settings opened");
    let mut settings_cx = settings_context(&settings, cx);
    settings_cx.run_until_parked();
    // The window opens settled: the first frame has no section to come
    // from, and none is pending — a settled window is idle.
    assert_eq!(frame(&mut settings_cx, Duration::ZERO), 0);
    assert!(section_arrival(&settings, &mut settings_cx).is_none());
    (settings, settings_cx)
}

/// Switching sections transitions the content that changes — the page —
/// with the shared policy's short fade and tiny shift from the side the
/// sidebar moved, while the shell around it (a sidebar row, the page's
/// scroll viewport) stays exactly where it was. The switch itself is
/// immediate: the page's content is drawn on the frame the click draws,
/// the sidebar's selected row is the new section's, and the arrival is
/// driven on the controlled clock — it progresses as frames are
/// delivered, completes within its bounded span, and leaves the window
/// asking for no frame at all.
#[gpui::test]
fn switching_sections_transitions_the_content_and_keeps_the_shell_still(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    let (settings, mut settings_cx) = opened_settings(cx);

    // The shell: an unselected sidebar row, and the page's scroll
    // viewport.
    let about_row = settings_cx
        .debug_bounds("section-About")
        .expect("a sidebar row");
    let viewport = settings_cx
        .debug_bounds("settings-page")
        .expect("the page viewport");

    // Shortcuts is further down the sidebar than the General page the
    // window opens on: the page's content arrives from below.
    click_section(&mut settings_cx, "section-Shortcuts");
    assert!(
        settings_cx.debug_bounds("shortcuts-title").is_some(),
        "the Shortcuts page is drawn at once, mid-arrival"
    );
    let (offset, opacity) =
        section_arrival(&settings, &mut settings_cx).expect("the page is arriving");
    assert!(
        offset > 2.5 && offset < 3.5,
        "the arrival starts the full shift below rest: {offset}"
    );
    assert!(opacity < 0.45, "the arrival starts faint: {opacity}");
    // The shell did not move with it.
    assert_eq!(
        settings_cx.debug_bounds("section-About").expect("the row"),
        about_row,
        "the sidebar row stayed still"
    );
    assert_eq!(
        settings_cx
            .debug_bounds("settings-page")
            .expect("the viewport"),
        viewport,
        "the page viewport stayed still"
    );
    // The visible selected-section state updated immediately, on this
    // same frame — exactly one section selected, the new one.
    selected_section(&mut settings_cx, "Shortcuts");

    // The content is displaced from its rest by the arrival's shift.
    let title = settings_cx
        .debug_bounds("shortcuts-title")
        .expect("the page's title");

    // Frames pass, and the arrival progresses without restarting.
    assert!(frame(&mut settings_cx, Duration::from_millis(40)) >= 1);
    let (progressed, _) =
        section_arrival(&settings, &mut settings_cx).expect("the page is still arriving");
    assert!(
        progressed > 0.05 && progressed < offset,
        "the arrival progressed toward rest: {progressed} from {offset}"
    );
    // Past the section span, the next delivered frame lands the content
    // at rest and asks for no further frame: the window is idle.
    assert!(frame(&mut settings_cx, Duration::from_millis(130)) >= 1);
    assert!(section_arrival(&settings, &mut settings_cx).is_none());
    let settled = settings_cx
        .debug_bounds("shortcuts-title")
        .expect("the page's title");
    assert_eq!(
        title.origin.y - settled.origin.y,
        px(offset),
        "the page was shifted exactly the arrival's offset below its rest"
    );
    assert_eq!(
        settle_frames(&mut settings_cx),
        0,
        "a settled window asks for no frame"
    );
}

/// Moving back up the sidebar is the paired arrival: the page's content
/// settles down into place from above rest, over the same section span,
/// and settles leaving the window idle.
#[gpui::test]
fn moving_up_the_sidebar_arrives_from_above(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    let (settings, mut settings_cx) = opened_settings(cx);

    // Down to About — the last of Pane's own pages — and settled.
    click_section(&mut settings_cx, "section-About");
    settle_frames(&mut settings_cx);
    assert!(section_arrival(&settings, &mut settings_cx).is_none());

    // Back up to General, the first section: the content arrives from
    // above.
    click_section(&mut settings_cx, "section-General");
    assert!(
        settings_cx.debug_bounds("general").is_some(),
        "the General page is drawn at once, mid-arrival"
    );
    let (offset, _) = section_arrival(&settings, &mut settings_cx).expect("the page is arriving");
    assert!(
        offset < -2.5 && offset > -3.5,
        "the arrival starts the full shift above rest: {offset}"
    );

    // The section span settles it: 160ms — past its 150ms — leaves the
    // window idle.
    assert!(frame(&mut settings_cx, Duration::from_millis(160)) >= 1);
    assert!(section_arrival(&settings, &mut settings_cx).is_none());
    assert_eq!(
        settle_frames(&mut settings_cx),
        0,
        "a settled window asks for no frame"
    );
}

/// A rapid section switch retargets each arrival from the presentation
/// on screen — the interrupted offset carries over, so nothing restarts
/// and nothing flashes — and the outgoing page's content is unmounted at
/// once: the page drawn is always the section the user is on.
#[gpui::test]
fn rapid_section_switches_retarget_the_arrival_from_where_it_is(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    let (settings, mut settings_cx) = opened_settings(cx);

    // Switch down to Shortcuts, and — with no clock time passing between
    // them — down again to Extensions, whose page is different content.
    click_section(&mut settings_cx, "section-Shortcuts");
    let (offset, _) = section_arrival(&settings, &mut settings_cx).expect("Shortcuts is arriving");

    click_section(&mut settings_cx, "section-Extensions");
    let (continued, _) =
        section_arrival(&settings, &mut settings_cx).expect("Extensions is arriving");
    assert!(
        (continued - offset).abs() < 0.05,
        "the switch continued the presentation: {continued} from {offset}"
    );
    // The outgoing page's content is gone at once: the page drawn is
    // Extensions', and none of Shortcuts' content lingers over it.
    assert!(settings_cx.debug_bounds("extensions-title").is_some());
    assert!(settings_cx.debug_bounds("shortcuts-title").is_none());

    // And back up again, still from the presentation on screen.
    click_section(&mut settings_cx, "section-Shortcuts");
    let (back, _) =
        section_arrival(&settings, &mut settings_cx).expect("Shortcuts is arriving again");
    assert!(
        (back - offset).abs() < 0.05,
        "the return continued the presentation too: {back} from {offset}"
    );
    // What is drawn is Shortcuts' page, not a fading-out Extensions.
    assert!(settings_cx.debug_bounds("shortcuts-title").is_some());
    assert!(settings_cx.debug_bounds("extensions-title").is_none());

    // The retargeted arrival then completes like any other.
    settle_frames(&mut settings_cx);
    assert!(section_arrival(&settings, &mut settings_cx).is_none());
}

/// Reduced motion settles every section switch at once: a switch under
/// it starts no arrival, and reducing motion mid-arrival ends it on the
/// next drawn frame. Either way the window schedules no frame for
/// presentation — and at the window's floor the pages still switch and
/// lay out.
#[gpui::test]
fn reduced_motion_settles_section_switches_at_once_at_the_window_boundary(cx: &mut TestAppContext) {
    let (_launcher, _links, cx) = open_launcher(cx);
    let (settings, mut settings_cx) = opened_settings(cx);
    // The window's floor: the boundary the reduced presentation must
    // still work at. The sidebar's list scrolls inside it when the window
    // is short: turn its wheel to bring the About section's row into view
    // before it is clicked.
    settings_cx.simulate_resize(gpui::size(px(560.), px(400.)));
    settings_cx.run_until_parked();
    let sections = settings_cx
        .debug_bounds("sections")
        .expect("the sections list");
    settings_cx.simulate_event(gpui::ScrollWheelEvent {
        position: sections.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-120.))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    settings_cx.run_until_parked();
    // The wheel leaves the pointer over the list it scrolled, with rows
    // slid under it that no move ever named as hovered — the pointer
    // leaves, and the reflowed rows' washes settle, before the
    // preference is flipped and the frames are counted.
    pointer_leaves(&mut settings_cx);
    settle_frames(&mut settings_cx);

    // A switch under reduced motion starts no arrival: the frame that
    // draws the new page is already settled.
    settings_cx.update(|_, cx| cx.set_reduce_motion(true));
    click_section(&mut settings_cx, "section-About");
    assert!(
        settings_cx.debug_bounds("about").is_some(),
        "the About page is drawn at the floor"
    );
    assert!(
        section_arrival(&settings, &mut settings_cx).is_none(),
        "reduced motion drew the page settled"
    );
    assert_eq!(
        settle_frames(&mut settings_cx),
        0,
        "the window asked for no frame for the presentation"
    );

    // Reduced motion engaged mid-arrival ends it on the next frame. Begin
    // a return under full motion, then flip the preference. The wheel
    // goes back up first, for the General section's row above the
    // scrolled view.
    settings_cx.update(|_, cx| cx.set_reduce_motion(false));
    settings_cx.simulate_event(gpui::ScrollWheelEvent {
        position: sections.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(120.))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    settings_cx.run_until_parked();
    // The wheel leaves the pointer over the list again; the pointer
    // leaves before the section is clicked, for the reason it did
    // above.
    pointer_leaves(&mut settings_cx);
    click_section(&mut settings_cx, "section-General");
    assert!(
        section_arrival(&settings, &mut settings_cx).is_some(),
        "the switch began under full motion"
    );
    settings_cx.update(|_, cx| cx.set_reduce_motion(true));
    // The frame the arrival had asked for draws settled, and asks for
    // nothing further.
    assert!(frame(&mut settings_cx, Duration::ZERO) >= 1);
    assert!(
        section_arrival(&settings, &mut settings_cx).is_none(),
        "the arrival settled the moment reduced motion engaged"
    );
    assert_eq!(
        settle_frames(&mut settings_cx),
        0,
        "the window asked for no further frame"
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
fn choosing_a_theme_re_renders_both_windows(cx: &mut TestAppContext) {
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
    // off) — and the row carries the preference either way. Where the
    // solid surface stands in for glass, the material's row says so and
    // why; where the tint is drawn, there is nothing to explain.
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
    let glass_stands = paints_panel(cx, &[panel(0xF6F6F8CC)]);
    assert_eq!(
        settings_cx
            .debug_bounds("appearance-material-note")
            .is_some(),
        !glass_stands,
        "the material's row explains a fallback, and only a fallback"
    );
    #[cfg(target_os = "linux")]
    {
        let (_, json) = accessibility(&mut settings_cx);
        assert!(
            json.contains("Glass isn't available here"),
            "the fallback is named, {json}"
        );
    }

    // The solid surface needs no explaining: no caveat about glass is left
    // standing under it.
    choose(&mut settings_cx, "appearance-material-Solid");
    cx.run_until_parked();
    settings_cx.run_until_parked();
    assert!(
        settings_cx
            .debug_bounds("appearance-material-note")
            .is_none(),
        "no note under the solid choice"
    );
    let (_, json) = accessibility(&mut settings_cx);
    assert!(
        !json.contains("Glass isn't available"),
        "no glass caveat is left under the solid choice, {json}"
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
    let mut settings_cx = open_settings(cx);
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
    let mut settings_cx = open_settings(cx);

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
        json.contains("Allow Pane in System Settings, under General > Login Items."),
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
