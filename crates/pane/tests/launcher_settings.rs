//! The Launcher settings page (#78): the display the launcher opens on
//! and what reopening it starts from, driven through the real windows on
//! GPUI's test platform. The system is a fake placement — the display
//! layout it reports is chosen by the test, and every move it is asked
//! for is recorded — so the placement is exercised at the same boundary a
//! user sees it, the launcher window's opening, without owning this
//! machine's real displays. The hotkey system is the same fake the Open
//! Pane tests use, so the Open Pane hotkey can be pressed and a command's
//! global hotkey can be recorded ahead of the run. The real platform
//! halves (the GDI, RandR and CoreGraphics display lists, and the moves
//! through `SetWindowPos`, a configure request and `setFrameTopLeftPoint`)
//! are recorded natively in `docs/evidence/settings-78/`.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use gpui::{
    AnyWindowHandle, Modifiers, MouseButton, TestAppContext, VisualTestContext, WindowHandle,
    prelude::*, px, size,
};
use pane::placement::Placement;
use pane::{LauncherWindow, SettingsWindow};
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::placement::{Display, DisplayId, DisplayLayout, Point, Rect, Size};
use pane_core::{Launcher, PackageIdentity, Runtime, Screen};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

#[path = "support/paint.rs"]
mod paint;

use settle::settle;

/// The fake system for global hotkeys, as the Open Pane tests' one: what
/// Pane registered, and no shortcut another application has.
#[derive(Default)]
struct FakeSystem {
    registered: Mutex<Vec<Shortcut>>,
}

impl Hotkeys for FakeSystem {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
        self.registered.lock().unwrap().push(shortcut.clone());
        Ok(())
    }

    fn unregister(&self, shortcut: &Shortcut) {
        self.registered
            .lock()
            .unwrap()
            .retain(|kept| kept != shortcut);
    }
}

/// The fake placement: the display layout the test chooses, what it
/// refuses to tell, and a record of every move the launcher window was
/// placed with — the boundary the placement's decisions are observed at,
/// as the fake hotkey system's registrations are.
#[derive(Default)]
struct FakePlacement {
    layout: RefCell<DisplayLayout>,
    unavailable: RefCell<Option<String>>,
    refuse: RefCell<Option<String>>,
    moves: RefCell<Vec<Rect>>,
}

impl FakePlacement {
    /// The two-display layout the placement tests use: a primary display
    /// on the left and a second one to its right, with the pointer and
    /// the active window where the test puts them, or neither told.
    fn layout(&self, pointer: Option<Point>, active: Option<DisplayId>) {
        *self.layout.borrow_mut() = DisplayLayout {
            displays: vec![
                display(1, 0., 0., 1920., 1080., 40.),
                display(2, 1920., 0., 2560., 1440., 60.),
            ],
            primary: Some(DisplayId(1)),
            pointer,
            active,
        };
    }

    /// The origin of the move number `index`, the part of a placement a
    /// platform applies: the bounds it was asked for, at the top left.
    fn origin(&self, index: usize) -> Point {
        self.moves.borrow()[index].origin
    }
}

impl Placement for FakePlacement {
    fn unavailable(&self) -> Option<String> {
        self.unavailable.borrow().clone()
    }

    fn layout(&self) -> DisplayLayout {
        self.layout.borrow().clone()
    }

    fn place(&self, _window: &mut gpui::Window, bounds: Rect) -> Result<(), String> {
        if let Some(why) = self.refuse.borrow().clone() {
            return Err(why);
        }
        self.moves.borrow_mut().push(bounds);
        Ok(())
    }
}

/// A display with identity `id`, covering the square from (`x`, `y`) of
/// the given size, whose usable area is inset by the same amount on every
/// side.
fn display(id: u64, x: f32, y: f32, width: f32, height: f32, inset: f32) -> Display {
    Display {
        id: DisplayId(id),
        bounds: Rect {
            origin: Point { x, y },
            size: Size { width, height },
        },
        usable: Rect {
            origin: Point {
                x: x + inset,
                y: y + inset,
            },
            size: Size {
                width: width - 2. * inset,
                height: height - 2. * inset,
            },
        },
    }
}

/// Initializes the settings record of `data` in `cx`, as the binary does
/// before its first window opens.
fn init_settings(data: Option<&Path>, cx: &mut TestAppContext) {
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            data.map(|data| data.to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
}

/// The keystroke that opens Settings on this platform.
fn settings_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-,"
    } else {
        "ctrl-,"
    }
}

/// Opens the Settings window and moves it to the Launcher page by
/// clicking its sidebar row, ready for the page's own controls.
fn open_launcher_page(
    cx: &mut VisualTestContext,
) -> (WindowHandle<SettingsWindow>, VisualTestContext) {
    cx.simulate_keystrokes(settings_shortcut());
    cx.run_until_parked();
    let settings = cx
        .cx
        .update(|cx| {
            cx.windows()
                .into_iter()
                .filter_map(|window| window.downcast::<SettingsWindow>())
                .next()
        })
        .expect("Settings opened");
    let mut settings_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);
    // The page is reached by leaving the page the window opens on and
    // coming back, so a call on a window already showing the page still
    // redraws it — the placement's layout may have changed since.
    click(&mut settings_cx, "section-General");
    settings_cx.run_until_parked();
    click(&mut settings_cx, "section-Launcher");
    settings_cx.run_until_parked();
    assert!(
        settings_cx.debug_bounds("launcher-title").is_some(),
        "the Launcher page is drawn"
    );
    // The page arrives over the section transition's span, shifted from
    // its rest until the frames carry it there; the tests measure the
    // page's controls at rest, so those frames are delivered first.
    settle_frames(&mut settings_cx);
    (settings, settings_cx)
}

/// Presses the Open Pane hotkey `shortcut`, as the system's adapter would
/// report it while any application has focus, moved past the window's
/// repeat guard so two of these are two genuine presses.
fn press(window: &gpui::Entity<LauncherWindow>, shortcut: &Shortcut, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(700));
    window.update_in(cx, |window, w, cx| window.hotkey_pressed(shortcut, w, cx));
}

/// Whether the launcher window is hidden — hidden, not closed: Pane keeps
/// running, and the same live window answers the next opening.
fn hidden(window: &gpui::Entity<LauncherWindow>, cx: &VisualTestContext) -> bool {
    cx.read_entity(window, |window, _| window.hidden())
}

/// Hides the launcher with the Open Pane hotkey, as a dismissal does,
/// saying how many moves the placement has been asked for so far: neither
/// hiding nor being summoned moves anything. The hotkey hides a launcher
/// that has focus; one that does not — the window as the test platform
/// opens it, or while another window holds the focus — is brought
/// forward first, so a second press hides it.
fn dismiss(
    window: &gpui::Entity<LauncherWindow>,
    shortcut: &Shortcut,
    cx: &mut VisualTestContext,
    placement: &FakePlacement,
) -> usize {
    let before = placement.moves.borrow().len();
    press(window, shortcut, cx);
    cx.run_until_parked();
    if !hidden(window, cx) {
        // The launcher was visible without focus: the press brought it
        // forward, and the next one hides it.
        press(window, shortcut, cx);
        cx.run_until_parked();
    }
    assert!(hidden(window, cx), "the launcher hid");
    assert_eq!(
        placement.moves.borrow().len(),
        before,
        "hiding and being summoned move nothing"
    );
    before
}

/// Clicks the element whose debug selector is `selector` in `cx`. The
/// pointer moves to the element first, as a real one does: the move is
/// what tells an element it is hovered, and a click's landing alone does
/// not — without the move the pointer-feedback fade would see a hover in
/// the paint's style pass and not in the layout's, and never settle.
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"));
    cx.simulate_mouse_move(bounds.center(), None::<MouseButton>, Modifiers::none());
    cx.simulate_click(bounds.center(), Modifiers::none());
}

/// Moves the pointer off the window, as a user's does when it leaves.
/// The test platform parks the pointer wherever a move or click last
/// put it, and a wash a parked pointer implies keeps running — so the
/// tests that count the window's frames send the pointer away first,
/// leaving the window to the keyboard's modality.
fn pointer_leaves(cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(
        gpui::point(px(-100.), px(-100.)),
        None::<MouseButton>,
        Modifiers::none(),
    );
}

/// Chooses the opening monitor whose choice row's debug selector is
/// `selector`, through the page's own select control: the trigger
/// opens the popup, then the choice's row takes it — as a user choosing
/// a display does. The popup's rows keep the choice's own debug
/// selector, so a choice that cannot be used here can be clicked the
/// same way, to prove nothing happens.
fn choose_monitor(cx: &mut VisualTestContext, selector: &'static str) {
    click(cx, "launcher-monitor");
    cx.run_until_parked();
    click(cx, selector);
    cx.run_until_parked();
}

/// Delivers the animation frame the window `cx` drives has asked for, as
/// the native frame loop would, with `elapsed` passing first on the test
/// platform's controlled clock. The test platform delivers no frames on
/// its own, so this is the only thing that advances a running
/// transition; one call draws at most one frame. Returns how many
/// next-frame callbacks ran — `0` means the window had asked for no
/// frame, so nothing drew.
fn frame(cx: &mut VisualTestContext, elapsed: Duration) -> usize {
    cx.executor().advance_clock(elapsed);
    let ran = cx.update(|window, cx| window.simulate_next_frame(cx));
    cx.run_until_parked();
    ran
}

/// Delivers frames until the window asks for none, so a transition in
/// flight completes, and returns the frames it delivered. `0` means the
/// window was already idle: no cosmetic and no functional frame was
/// pending. Bounded, so a window that never stopped asking for frames
/// fails the test instead of hanging it.
fn settle_frames(cx: &mut VisualTestContext) -> usize {
    let mut delivered = 0;
    for _ in 0..20 {
        let ran = frame(cx, Duration::from_millis(25));
        if ran == 0 {
            return delivered;
        }
        delivered += ran;
    }
    panic!("the window never stopped asking for animation frames");
}

/// The select popup's presentation as the last frame drew it — the
/// offset from rest toward the trigger in px (negative: the popup hangs
/// below the trigger, so toward it is up) and the opacity; `None` when
/// the last frame drew the popup settled (at rest while open, absent
/// while closed). See [`SettingsWindow::monitor_select_popup`].
fn popup_presentation(
    settings: &WindowHandle<SettingsWindow>,
    cx: &mut VisualTestContext,
) -> Option<(f32, f32)> {
    settings
        .read_with(cx, |window, cx| window.monitor_select_popup(cx))
        .expect("the Settings window is open")
}

/// The accessibility tree of the window `cx` drives, as raw JSON, forced
/// on so the tree is built regardless of platform accessibility.
fn a11y(cx: &mut VisualTestContext) -> String {
    cx.update(|window, _| window.set_a11y_forced(true));
    cx.run_until_parked();
    cx.update(|window, _| window.debug_a11y_tree_json())
        .expect("an accessibility tree")
}

/// Runs `cx` until the settings record in `data` holds `text`: the save
/// the page started is written off the window's thread.
fn until_record(cx: &mut VisualTestContext, data: &Path, text: &str) {
    let record = data.join("settings.json");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if fs::read_to_string(&record)
            .ok()
            .is_some_and(|held| held.contains(text))
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {text} in the record"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Writes a package folder whose one command is the Rust sample, as the
/// hotkey tests' fixture does.
fn package(folder: &Path) -> PathBuf {
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
    folder.to_path_buf()
}

/// The command id of the package at `folder`: its identity's key and its
/// manifest's command id.
fn command_id(folder: &Path) -> String {
    format!("{}#hello", PackageIdentity::local(folder).unwrap().key())
}

/// Records the hotkey `shortcut` for the package at `folder`'s command
/// into `data` before a launcher is made of it, as a Pane that ran before
/// would have.
fn seed_hotkey(data: &TempDir, folder: &Path, shortcut: &str) {
    let dir = data.path().join("extensions");
    fs::create_dir_all(&dir).unwrap();
    let hotkeys = [(command_id(folder), serde_json::json!(shortcut))]
        .into_iter()
        .collect::<serde_json::Map<String, serde_json::Value>>();
    fs::write(
        dir.join("hotkeys.json"),
        serde_json::json!({ "version": 1, "hotkeys": hotkeys }).to_string(),
    )
    .unwrap();
}

/// The launcher window over a launcher whose hotkey system and placement
/// are the fakes, with the settings record of `data`, and the placement
/// the window places through.
fn open<'a>(
    cx: &'a mut TestAppContext,
    data: Option<&Path>,
    placement: &Rc<FakePlacement>,
) -> (gpui::Entity<LauncherWindow>, &'a mut VisualTestContext) {
    cx.update(|cx| pane::placement::init(placement.clone() as Rc<dyn Placement>, cx));
    init_settings(data, cx);
    let launcher =
        Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(Arc::new(FakeSystem::default()));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (window, cx)
}

#[gpui::test]
fn the_launcher_opens_on_the_chosen_display_and_never_moves_settings(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(Some(Point { x: 2500., y: 700. }), Some(DisplayId(2)));
    let (window, cx) = open(cx, Some(data.path()), &placement);

    // The record holds nothing yet: the provisional default is the primary
    // display, and the window was placed on it as it opened — centered in
    // its usable area, which the first recorded move's origin names.
    let default = Shortcut::open_pane_default();
    assert_eq!(
        placement.origin(0),
        Point { x: 40., y: 40. },
        "the primary display's usable area"
    );

    // Reopening the launcher, with the pointer on the second display,
    // keeps the primary placement until the choice says otherwise.
    let before = dismiss(&window, &default, cx, &placement);
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx));
    assert_eq!(placement.moves.borrow().len(), before + 1);
    assert_eq!(
        placement.origin(before),
        Point { x: 40., y: 40. },
        "the default still opens on the primary display"
    );

    // The choice, taken through the Launcher page's own control: the
    // pointer's display.
    let (settings, mut settings_cx) = open_launcher_page(cx);
    choose_monitor(&mut settings_cx, "launcher-monitor-Pointer");
    settings_cx.run_until_parked();
    until_record(
        &mut settings_cx,
        data.path(),
        "\"openingMonitor\": \"pointer\"",
    );
    assert!(
        a11y(&mut settings_cx).contains("Pointer's display"),
        "the select's trigger shows the committed choice"
    );

    // The Settings window sits where it was opened; the launcher's next
    // opening moves the launcher only.
    let settings_before = settings_cx.update(|window, _| window.bounds());
    let before = dismiss(&window, &default, cx, &placement);
    press(&window, &default, cx);
    cx.run_until_parked();
    assert_eq!(
        placement.moves.borrow().len(),
        before + 1,
        "the launcher was placed"
    );
    assert_eq!(
        placement.origin(before),
        Point { x: 1980., y: 60. },
        "the pointer's display's usable area"
    );
    // The Settings window is exactly where it was: the launcher's
    // placement never moves it.
    let mut settings_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);
    let still = settings_cx.update(|window, _| window.bounds());
    assert_eq!(still, settings_before, "the Settings window did not move");
}

#[gpui::test]
fn choices_the_system_does_not_answer_are_explained_not_offered(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    // Nothing is told: no pointer, no active window.
    placement.layout(None, None);
    let (window, cx) = open(cx, Some(data.path()), &placement);

    let (_settings, mut settings_cx) = open_launcher_page(cx);
    // The select's popup, open: its rows carry the reasons, as the radio
    // rows it replaced did.
    click(&mut settings_cx, "launcher-monitor");
    settings_cx.run_until_parked();
    let tree = a11y(&mut settings_cx);
    // The two choices the system cannot answer are shown with their
    // reason, not offered.
    assert!(
        tree.contains("This system does not tell Pane where the pointer is"),
        "the pointer's display is explained, {tree}"
    );
    assert!(
        tree.contains("This system does not tell Pane which window is active"),
        "the active window's display is explained, {tree}"
    );
    // The primary display is always offered, and the default named as
    // provisional.
    assert!(tree.contains("Primary display"));
    assert!(tree.contains("provisional default"));
    // Choosing a choice that cannot be answered does nothing: the row
    // is not clickable, and nothing is saved. The popup is still open
    // from the reading above — the click lands on the row itself.
    click(&mut settings_cx, "launcher-monitor-Pointer");
    settings_cx.run_until_parked();
    assert!(
        !data.path().join("settings.json").exists(),
        "nothing was kept"
    );
    let _ = window;
}

#[gpui::test]
fn a_disconnected_or_unanswered_choice_falls_back_and_says_so(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(Some(Point { x: 2500., y: 700. }), Some(DisplayId(1)));
    let (window, cx) = open(cx, Some(data.path()), &placement);

    // The active window's display, taken through the page's own control
    // while the system answers it: the record holds the choice.
    let (_settings, mut settings_cx) = open_launcher_page(cx);
    choose_monitor(&mut settings_cx, "launcher-monitor-ActiveWindow");
    settings_cx.run_until_parked();
    until_record(
        &mut settings_cx,
        data.path(),
        "\"openingMonitor\": \"active-window\"",
    );

    // The system stops telling Pane which window is active: the choice
    // stays recorded, the page says what the launcher would open on
    // instead, and the choice's own row explains itself — inside the
    // select's popup, which is where the choices are listed.
    placement.layout(Some(Point { x: 2500., y: 700. }), None);
    let (_settings, mut settings_cx) = open_launcher_page(cx);
    click(&mut settings_cx, "launcher-monitor");
    settings_cx.run_until_parked();
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains("This system does not tell Pane which window is active"),
        "the choice is explained, {tree}"
    );
    assert!(
        tree.contains("cannot open on the display of the window you are working in"),
        "the fallback is explained, {tree}"
    );
    assert!(
        tree.contains("opens on the primary display instead"),
        "the fallback names what happens, {tree}"
    );

    // The launcher still opens on an available display: the fallback, not
    // nowhere.
    let default = Shortcut::open_pane_default();
    let before = dismiss(&window, &default, cx, &placement);
    press(&window, &default, cx);
    cx.run_until_parked();
    assert_eq!(placement.moves.borrow().len(), before + 1);
    assert_eq!(
        placement.origin(before),
        Point { x: 40., y: 40. },
        "the fallback placed the launcher on the primary display"
    );

    // A display that is gone: the choice falls back the same way, because
    // the layout no longer lists it.
    *placement.layout.borrow_mut() = DisplayLayout {
        displays: vec![display(2, 0., 0., 2560., 1440., 60.)],
        primary: Some(DisplayId(2)),
        pointer: Some(Point { x: 2500., y: 700. }),
        active: Some(DisplayId(1)),
    };
    let (_settings, mut settings_cx) = open_launcher_page(cx);
    assert!(
        a11y(&mut settings_cx).contains("it is not connected"),
        "the disconnected display is explained"
    );
}

#[gpui::test]
fn a_platform_that_cannot_choose_the_display_explains_and_offers_nothing(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    *placement.unavailable.borrow_mut() =
        Some("Not available on Linux with Wayland: the compositor places windows itself".into());
    let (window, cx) = open(cx, Some(data.path()), &placement);

    let (_settings, mut settings_cx) = open_launcher_page(cx);
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains("Not available on Linux with Wayland"),
        "the platform's reason is shown, {tree}"
    );
    // No opening-monitor choice is offered at all.
    assert!(
        !tree.contains("Primary display"),
        "no monitor choice is offered, {tree}"
    );
    // The reopening choice is unaffected: it is no platform integration.
    assert!(
        tree.contains("Restore the current view"),
        "reopening is offered, {tree}"
    );
    // The launcher still opens: nothing is placed, and nothing fails.
    let default = Shortcut::open_pane_default();
    let before = placement.moves.borrow().len();
    press(&window, &default, cx);
    cx.run_until_parked();
    assert_eq!(placement.moves.borrow().len(), before, "nothing was placed");
}

#[gpui::test]
fn reopening_restores_the_view_and_focuses_its_search_by_default(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(Some(Point { x: 100., y: 100. }), None);
    let (window, cx) = open(cx, Some(data.path()), &placement);

    // A query typed into root search, then a dismissal: the default — the
    // parent specification's provisional one — restores the view the
    // launcher was left on, with its search focused, so typing lands in
    // the query. Nothing is dispatched by the reopening.
    cx.simulate_input("zz");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("zz"));
    let default = Shortcut::open_pane_default();
    dismiss(&window, &default, cx, &placement);
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx));
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    assert_eq!(
        view.query(),
        Some("zz"),
        "the restored view kept what was typed"
    );
    cx.simulate_input("q");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("zzq"), "the search has focus");
    assert!(matches!(view.screen, Screen::Root { .. }));
    assert_eq!(
        view.status,
        pane_core::Status::Idle,
        "nothing was dispatched"
    );
}

#[gpui::test]
fn choosing_root_search_starts_the_reopening_from_root_search(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(None, None);
    let (window, cx) = open(cx, Some(data.path()), &placement);

    // The root-search choice, taken through the page's own control.
    let (_settings, mut settings_cx) = open_launcher_page(cx);
    click(&mut settings_cx, "launcher-reopening-RootSearch");
    settings_cx.run_until_parked();
    until_record(
        &mut settings_cx,
        data.path(),
        "\"reopening\": \"root-search\"",
    );

    // A query typed, then a dismissal: the reopening starts from root
    // search with an empty query, whatever was left.
    cx.simulate_input("zz");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("zz"));
    let default = Shortcut::open_pane_default();
    dismiss(&window, &default, cx, &placement);
    press(&window, &default, cx);
    cx.run_until_parked();
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some(""), "the query was left behind");
    assert!(matches!(view.screen, Screen::Root { .. }));
    cx.simulate_input("q");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("q"), "root search's query has focus");
}

#[gpui::test]
fn a_view_whose_command_is_gone_returns_safely_to_root_search(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let placement = Rc::new(FakePlacement::default());
    placement.layout(None, None);
    cx.update(|cx| pane::placement::init(placement.clone() as Rc<dyn Placement>, cx));
    init_settings(Some(data.path()), cx);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_hotkeys(Arc::new(FakeSystem::default()));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) =
        cx.add_window_view(|window, cx| LauncherWindow::new(launcher.clone(), window, cx));

    // Install the package and open its command: the view is a command's,
    // which the default reopening restores while its package is installed
    // and enabled.
    let installing = launcher.install_package(&folder);
    cx.foreground_executor().block_on(installing);
    // Typing the query redraws the window, which the install finished
    // without telling: no event of the user's drove the change.
    cx.simulate_input("hello");
    let view = settle(&window, cx);
    assert!(
        view.rows.iter().any(|row| row.title == "Say hello"),
        "the package's command is listed"
    );
    assert_eq!(view.selected, Some(0), "the command is the selected row");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Command), "{:?}", view.screen);

    // The reopening restores the command's view.
    let default = Shortcut::open_pane_default();
    dismiss(&window, &default, cx, &placement);
    press(&window, &default, cx);
    cx.run_until_parked();
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    assert!(
        matches!(view.screen, Screen::Command),
        "the command's view was restored, {:?}",
        view.screen
    );

    // The package disabled: its commands offer none, so the view is not
    // one to return to, and the reopening goes safely to root search.
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(hidden(&window, cx));
    let identity = launcher
        .packages()
        .first()
        .expect("the installed package")
        .identity
        .clone();
    block_on(launcher.set_enabled(&identity, false));
    press(&window, &default, cx);
    cx.run_until_parked();
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "an invalid view returned safely to root search, {:?}",
        view.screen
    );
}

#[gpui::test]
fn a_commands_hotkey_still_opens_its_command_with_the_root_preference(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    // A command hotkey recorded by a Pane that ran before.
    let hello_hotkey = Shortcut::parse("ctrl+alt+h").unwrap();
    seed_hotkey(&data, &folder, "ctrl+alt+h");
    let placement = Rc::new(FakePlacement::default());
    placement.layout(None, None);
    cx.update(|cx| pane::placement::init(placement.clone() as Rc<dyn Placement>, cx));
    // The root-search preference, written ahead of the run as a Pane that
    // ran before would have.
    fs::write(
        data.path().join("settings.json"),
        "{ \"version\": 1, \"reopening\": \"root-search\" }",
    )
    .unwrap();
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_hotkeys(Arc::new(FakeSystem::default()));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) =
        cx.add_window_view(|window, cx| LauncherWindow::new(launcher.clone(), window, cx));

    // Install the package and open its command, then leave it open.
    let installing = launcher.install_package(&folder);
    cx.foreground_executor().block_on(installing);
    cx.simulate_input("hello");
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0));
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Command), "{:?}", view.screen);

    // Dismiss, then press the command's own global hotkey: it opens its
    // named command, not root search — the reopening preference governs
    // the launcher's opening, never a command's binding.
    let default = Shortcut::open_pane_default();
    dismiss(&window, &default, cx, &placement);
    press(&window, &hello_hotkey, cx);
    cx.run_until_parked();
    let view = settle(&window, cx);
    assert!(
        matches!(view.screen, Screen::Command),
        "the command's hotkey opened its command, {:?}",
        view.screen
    );
}

#[gpui::test]
fn the_recorded_choices_are_applied_by_a_fresh_application(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(Some(Point { x: 2500., y: 700. }), None);
    let (_window, cx) = open(cx, Some(data.path()), &placement);

    // Both choices, taken through the page's own controls.
    let (_settings, mut settings_cx) = open_launcher_page(cx);
    choose_monitor(&mut settings_cx, "launcher-monitor-Pointer");
    settings_cx.run_until_parked();
    click(&mut settings_cx, "launcher-reopening-RootSearch");
    settings_cx.run_until_parked();
    until_record(
        &mut settings_cx,
        data.path(),
        "\"openingMonitor\": \"pointer\"",
    );
    until_record(
        &mut settings_cx,
        data.path(),
        "\"reopening\": \"root-search\"",
    );

    // A fresh application over the same data folder: a new app, nothing
    // carried over but the executors; the settings the record alone, and
    // a fresh placement whose layout still reports the same displays.
    let mut fresh = cx.cx.new_app();
    let fresh_placement = Rc::new(FakePlacement::default());
    fresh_placement.layout(Some(Point { x: 2500., y: 700. }), None);
    fresh.update(|cx| pane::placement::init(fresh_placement.clone() as Rc<dyn Placement>, cx));
    fresh.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    let launcher =
        Launcher::new(Runtime::start(), Vec::new()).with_hotkeys(Arc::new(FakeSystem::default()));
    let (window, fresh_cx) =
        fresh.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    fresh_cx.run_until_parked();

    // The fresh window opened on the display the record names.
    assert_eq!(
        fresh_placement.origin(0),
        Point { x: 1980., y: 60. },
        "the recorded choice placed the fresh launcher"
    );
    // And the recorded reopening starts from root search.
    fresh_cx.simulate_input("zz");
    let view = settle(&window, fresh_cx);
    assert_eq!(view.query(), Some("zz"));
    let default = Shortcut::open_pane_default();
    dismiss(&window, &default, fresh_cx, &fresh_placement);
    press(&window, &default, fresh_cx);
    fresh_cx.run_until_parked();
    let view = settle(&window, fresh_cx);
    assert_eq!(view.query(), Some(""), "root search, as recorded");
}

#[gpui::test]
fn a_save_that_fails_is_reported_and_the_shown_choice_stays_what_was_saved(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(Some(Point { x: 2500., y: 700. }), None);
    let (window, cx) = open(cx, Some(data.path()), &placement);

    let default = Shortcut::open_pane_default();

    // A change that lands and is saved: the pointer's display.
    let (_settings, mut settings_cx) = open_launcher_page(cx);
    choose_monitor(&mut settings_cx, "launcher-monitor-Pointer");
    settings_cx.run_until_parked();
    until_record(
        &mut settings_cx,
        data.path(),
        "\"openingMonitor\": \"pointer\"",
    );

    // Break the record's replacement: a folder where the record belongs,
    // so the atomic write cannot rename over it.
    fs::remove_file(data.path().join("settings.json")).unwrap();
    fs::create_dir(data.path().join("settings.json")).unwrap();

    // Another change: it cannot be saved, and the failure is reported.
    click(&mut settings_cx, "launcher-reopening-RootSearch");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        settings_cx.run_until_parked();
        let tree = a11y(&mut settings_cx);
        if tree.contains("Pane could not save your choice") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timed out: the page shows {tree}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    // The shown choice is what the record last held: the save's failure
    // rolled the reopening choice back to the default.
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains("Restore the current view"),
        "the shown choice is the one that was saved, {tree}"
    );

    // The choice that could not be saved never took effect: the launcher's
    // next opening places as the record holds.
    let before = dismiss(&window, &default, cx, &placement);
    press(&window, &default, cx);
    cx.run_until_parked();
    assert_eq!(placement.moves.borrow().len(), before + 1);
    assert_eq!(
        placement.origin(before),
        Point { x: 1980., y: 60. },
        "the saved choice is the one that works"
    );
}

#[gpui::test]
fn a_move_that_fails_is_said_not_hidden(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(Some(Point { x: 2500., y: 700. }), None);
    let (window, cx) = open(cx, Some(data.path()), &placement);

    // The platform refuses the move: the launcher's own status line says
    // so, so an opening that did not go where the choice says is never
    // mistaken for one that did.
    *placement.refuse.borrow_mut() = Some("the window manager refused the move".into());
    let default = Shortcut::open_pane_default();
    let before = dismiss(&window, &default, cx, &placement);
    press(&window, &default, cx);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let view = cx.read_entity(&window, |window, _| window.launcher().view());
        if matches!(view.status, pane_core::Status::Error(_)) {
            let message = match view.status {
                pane_core::Status::Error(message) => message,
                _ => unreachable!(),
            };
            assert!(
                message.contains("the window manager refused the move"),
                "the failure is said: {message}"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timed out: the launcher shows {view:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        placement.moves.borrow().len(),
        before,
        "the refused move recorded nothing"
    );
}

#[gpui::test]
fn escape_at_root_search_with_an_empty_query_hides_the_launcher(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(None, None);
    let (window, cx) = open(cx, Some(data.path()), &placement);

    // A query typed: Escape clears it, as it always has, and hides
    // nothing.
    cx.simulate_input("zz");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some("zz"));
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some(""));
    assert!(!hidden(&window, cx), "the query was cleared, nothing hid");

    // Root search, an empty query: the end of the Escape chain — nothing
    // is left to back out of — dismisses the launcher. Hidden, not
    // closed: the same live window answers the next opening, and a
    // placement is applied to it as to any opening.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(hidden(&window, cx), "the launcher hid");
    let default = Shortcut::open_pane_default();
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx), "the hidden launcher was shown again");
}

#[gpui::test]
fn the_page_registers_its_settings_in_the_settings_search(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(Some(Point { x: 100., y: 100. }), Some(DisplayId(1)));
    let (window, cx) = open(cx, Some(data.path()), &placement);

    // The Launcher page's settings are in the catalog the sidebar's search
    // filters: each choice, named as the page names it, in the group it
    // sits in. The reopening choices are found by their own words.
    let (settings, _settings_cx) = open_launcher_page(cx);
    let mut search_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);
    search_cx.simulate_keystrokes(find_shortcut());
    search_cx.simulate_input("monitor");
    search_cx.run_until_parked();
    assert!(
        search_cx
            .debug_bounds("settings-search-result-Primary display")
            .is_some(),
        "the opening monitor's choices are found"
    );
    let tree = a11y(&mut search_cx);
    assert!(
        tree.contains("Launcher \u{b7} Opening monitor"),
        "the result names the page and the group, {tree}"
    );
    // Escape clears the query, and the reopening choices are found by
    // their own words.
    search_cx.simulate_keystrokes("escape");
    search_cx.run_until_parked();
    search_cx.simulate_input("reopening");
    search_cx.run_until_parked();
    assert!(
        search_cx
            .debug_bounds("settings-search-result-Start at root search")
            .is_some(),
        "the reopening choices are found"
    );
    // Escape again leaves the search, and the sections are back.
    search_cx.simulate_keystrokes("escape");
    search_cx.run_until_parked();
    search_cx.simulate_keystrokes("escape");
    search_cx.run_until_parked();
    assert!(
        search_cx.debug_bounds("section-Launcher").is_some(),
        "the sections are back"
    );
    let _ = window;
}

/// The label of the node assistive technology treats as focused in the
/// window `cx` drives: the focused node's own label, or its active
/// descendant's, as the select's open popup reads its highlighted
/// choice.
fn focused_label(cx: &mut VisualTestContext) -> Option<String> {
    let tree = a11y(cx);
    let tree: serde_json::Value = serde_json::from_str(&tree).unwrap();
    let nodes = tree["nodes"].as_object().unwrap();
    let field = |node: &serde_json::Value, key: &str| {
        node["aria"][key].as_str().unwrap_or_default().to_owned()
    };
    ["active_descendant_focus", "gpui_focus"]
        .iter()
        .find_map(|key| tree[key].as_str())
        .map(|id| field(&nodes[id], "label"))
}

/// The aria maps of the accessibility tree's nodes, for assertions on
/// the select's own exposure: the trigger's role, value and expanded
/// state, the choices' selection and availability.
fn aria_nodes(cx: &mut VisualTestContext) -> Vec<serde_json::Value> {
    let tree = a11y(cx);
    let tree: serde_json::Value = serde_json::from_str(&tree).unwrap();
    nodes(tree)
}

/// The accessibility tree's nodes' aria maps.
fn nodes(tree: serde_json::Value) -> Vec<serde_json::Value> {
    tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .map(|node| node["aria"].clone())
        .collect()
}

/// The select, on the Launcher page it lives in, with the window it
/// drives and the data folder its record saves into. The placement
/// answers everything, so every choice is offered.
fn open_select(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<LauncherWindow>,
    WindowHandle<SettingsWindow>,
    VisualTestContext,
    TempDir,
) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(Some(Point { x: 2500., y: 700. }), Some(DisplayId(2)));
    let (window, cx) = open(cx, Some(data.path()), &placement);
    let (settings, settings_cx) = open_launcher_page(cx);
    (window, settings, settings_cx, data)
}

#[gpui::test]
fn the_select_opens_below_the_trigger_and_commits_the_highlighted_choice(cx: &mut TestAppContext) {
    let (_window, settings, mut sc, data) = open_select(cx);

    // The trigger opens the popup: below the trigger, as wide as it, and
    // the keyboard moves into the field, so typing filters at once.
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    let trigger = sc.debug_bounds("launcher-monitor").expect("the trigger");
    // The popup enters from the trigger: the first frame draws it a tiny
    // shift above its rest (toward the trigger it hangs below) and
    // already faintly visible — the entrance's floor — while the
    // anchoring measured it at its resting size, so the placement is
    // where it settles.
    let entering =
        popup_presentation(&settings, &mut sc).expect("the popup is entering from the trigger");
    assert!(
        entering.0 < -2.5 && entering.0 > -3.5,
        "the entrance starts the full shift toward the trigger: {}",
        entering.0
    );
    assert!(
        entering.1 < 0.45,
        "the entrance starts faint: {}",
        entering.1
    );
    let moving = sc
        .debug_bounds("launcher-monitor-popup")
        .expect("the popup opens");
    // Frames pass, and the entrance progresses without restarting.
    assert!(frame(&mut sc, Duration::from_millis(40)) >= 1);
    let progressed = popup_presentation(&settings, &mut sc).expect("the popup is still entering");
    assert!(
        progressed.0 > entering.0 && progressed.0 < 0.,
        "the entrance progressed toward rest: {} from {}",
        progressed.0,
        entering.0
    );
    // The popup's width is the entrance's own coordination with the
    // trigger: it does not change while the popup moves (the anchored
    // element measured it at its resting size).
    let in_flight = sc
        .debug_bounds("launcher-monitor-popup")
        .expect("the popup is drawn");
    assert_eq!(
        in_flight.size.width, moving.size.width,
        "the popup's width is stable while it enters"
    );
    // Past the entrance's span, the next delivered frame lands the popup
    // at rest and asks for no further frame: the window is idle.
    assert!(frame(&mut sc, Duration::from_millis(130)) >= 1);
    assert!(popup_presentation(&settings, &mut sc).is_none());
    let popup = sc
        .debug_bounds("launcher-monitor-popup")
        .expect("the popup is drawn");
    assert_eq!(popup.left(), trigger.left(), "the popup is left-aligned");
    assert!(
        (popup.top() - trigger.bottom()).abs() <= px(1.),
        "the popup settles below the trigger: {popup:?} under {trigger:?}"
    );
    // The popup is as wide as its contents ask (the field, the rows),
    // bounded by the trigger's width — content-width dropdowns, as
    // Raycast's are, with the trigger's as the ceiling. On the canonical
    // page the trigger spans the page's 824px column (#97), wider than
    // the contents ask.
    assert!(
        popup.size.width <= trigger.size.width,
        "the popup stays within the trigger's width: {popup:?} vs {trigger:?}"
    );
    assert_eq!(
        moving.origin.y - popup.origin.y,
        px(entering.0),
        "the popup was shifted exactly the entrance's offset above its rest"
    );
    // The keyboard moves into the field — the a11y focus is the field's
    // node, whose active descendant (honored only under a focused
    // ancestor) is the committed choice the highlight starts on.
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Primary display"),
        "the popup's field takes the focus, the highlight on the committed choice"
    );

    // The exposure: the trigger is a combo box carrying the committed
    // choice as its value and the open state; the choices are a list
    // box's options, the committed one selected.
    let trigger_aria = aria_nodes(&mut sc)
        .into_iter()
        .find(|aria| aria["role"] == "ComboBox" && aria["label"] == "Display")
        .expect("the trigger is a named combo box");
    assert_eq!(trigger_aria["value"].as_str(), Some("Primary display"));
    assert!(trigger_aria["expanded"].as_bool().unwrap());
    // The choices are a list box's options — among the window's other
    // options, the sidebar's sections — so they are found by their
    // labels, and exactly the committed one is selected.
    let options = aria_nodes(&mut sc)
        .into_iter()
        .filter(|aria| {
            [
                "Primary display",
                "Pointer's display",
                "Active window's display",
            ]
            .iter()
            .any(|label| aria["label"].as_str() == Some(*label))
        })
        .collect::<Vec<_>>();
    assert_eq!(options.len(), 3, "the three choices are listed");
    assert_eq!(
        options
            .iter()
            .filter(|aria| aria["selected"].as_bool().unwrap_or(false))
            .count(),
        1,
        "the committed choice is the selected option"
    );

    // Typing filters locally, over the labels and the declared keywords
    // alike: "mouse" is the pointer's choice's keyword, not its label.
    sc.simulate_input("mouse");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-monitor-Pointer").is_some(),
        "the keyword finds the pointer's choice"
    );
    assert!(
        sc.debug_bounds("launcher-monitor-Primary").is_none(),
        "the other choices are filtered out"
    );

    // Enter commits the highlighted choice through the host settings:
    // the record is written, the popup closes, and the keyboard returns
    // to the trigger, which shows what was kept.
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    until_record(&mut sc, data.path(), "\"openingMonitor\": \"pointer\"");
    // The keyboard returned to the trigger the frame the popup closed,
    // while the exit still paints it.
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Display"),
        "the trigger has the focus back"
    );
    settle_frames(&mut sc);
    assert!(
        sc.debug_bounds("launcher-monitor-popup").is_none(),
        "the popup closed"
    );
    let trigger_aria = aria_nodes(&mut sc)
        .into_iter()
        .find(|aria| aria["role"] == "ComboBox" && aria["label"] == "Display")
        .expect("the trigger");
    assert_eq!(
        trigger_aria["value"].as_str(),
        Some("Pointer's display"),
        "the trigger shows the committed choice"
    );
    assert!(!trigger_aria["expanded"].as_bool().unwrap());
}

#[gpui::test]
fn escape_cancels_the_draft_and_the_saved_choice_stands(cx: &mut TestAppContext) {
    let (_window, _settings, mut sc, data) = open_select(cx);

    // A draft: a query that narrows the list and a highlight that moved.
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    sc.simulate_input("pointer");
    sc.run_until_parked();
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Pointer's display"),
        "the highlight is on the query's match"
    );

    // Escape cancels it all: the popup closes, the keyboard returns to
    // the trigger, and nothing was kept — the committed choice stands.
    sc.simulate_keystrokes("escape");
    sc.run_until_parked();
    // The keyboard returns the frame the popup closes; the exit still
    // paints it.
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Display"),
        "the trigger has the focus back"
    );
    settle_frames(&mut sc);
    assert!(
        sc.debug_bounds("launcher-monitor-popup").is_none(),
        "the popup closed"
    );
    assert!(
        !data.path().join("settings.json").exists(),
        "nothing was kept by opening, filtering or cancelling"
    );

    // The keyboard reopens it: Enter on the focused trigger. The draft
    // is fresh — the query starts empty, and the highlight starts on the
    // committed choice.
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-monitor-popup").is_some(),
        "Enter on the trigger opens it"
    );
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Primary display"),
        "the highlight starts on the committed choice, the query empty"
    );
    assert!(
        sc.debug_bounds("launcher-monitor-Primary").is_some(),
        "a fresh draft lists every choice"
    );
}

#[gpui::test]
fn tab_and_shift_tab_leave_without_committing(cx: &mut TestAppContext) {
    let (_window, _settings, mut sc, data) = open_select(cx);

    // A query that names a choice, so a commit would be identifiable.
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    sc.simulate_input("pointer");
    sc.run_until_parked();

    // Tab closes the popup and continues traversal forward from the
    // trigger: the field is left behind, and nothing was committed.
    sc.simulate_keystrokes("tab");
    sc.run_until_parked();
    settle_frames(&mut sc);
    assert!(
        sc.debug_bounds("launcher-monitor-popup").is_none(),
        "Tab closed the popup"
    );
    assert_ne!(
        focused_label(&mut sc).as_deref(),
        Some("Search choices"),
        "traversal left the popup's field"
    );
    assert!(
        !data.path().join("settings.json").exists(),
        "Tab committed nothing"
    );

    // Shift-Tab, the other way: closes and traverses backward, to the
    // sidebar's sections — the selected section read, as page navigation
    // is when the keyboard arrives there by any other way.
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    sc.simulate_keystrokes("shift-tab");
    sc.run_until_parked();
    settle_frames(&mut sc);
    assert!(
        sc.debug_bounds("launcher-monitor-popup").is_none(),
        "Shift-Tab closed the popup"
    );
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Launcher"),
        "traversal continued backward to the sections"
    );
    assert!(
        !data.path().join("settings.json").exists(),
        "Shift-Tab committed nothing"
    );
}

#[gpui::test]
fn the_trigger_toggles_and_an_outside_click_respects_its_target(cx: &mut TestAppContext) {
    let (_window, _settings, mut sc, data) = open_select(cx);

    // The trigger toggles: a click opens, a click on it again closes —
    // the click cannot reopen what it came to close.
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    assert!(sc.debug_bounds("launcher-monitor-popup").is_some());
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    settle_frames(&mut sc);
    assert!(
        sc.debug_bounds("launcher-monitor-popup").is_none(),
        "the trigger's click closed the popup it had opened"
    );

    // An outside click cancels the draft and lands: the clicked field
    // takes the focus as it would without the popup, and nothing was
    // committed underneath.
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    sc.simulate_input("pointer");
    sc.run_until_parked();
    let field = sc
        .debug_bounds("settings-search-field")
        .expect("the sidebar's search field");
    // The pointer travels there first, as a real one does (see [`click`]):
    // a click's landing alone leaves the trigger it left believing it is
    // still hovered, and its pointer fade would never settle.
    sc.simulate_mouse_move(field.center(), None::<MouseButton>, Modifiers::none());
    sc.simulate_click(field.center(), Modifiers::none());
    sc.run_until_parked();
    settle_frames(&mut sc);
    assert!(
        sc.debug_bounds("launcher-monitor-popup").is_none(),
        "the outside click closed the popup"
    );
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Search settings"),
        "the clicked field took the focus, not the trigger"
    );
    assert!(
        !data.path().join("settings.json").exists(),
        "the outside click committed nothing"
    );
}

#[gpui::test]
fn arrows_home_and_end_move_the_highlight_without_saving(cx: &mut TestAppContext) {
    let (_window, _settings, mut sc, data) = open_select(cx);

    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    // The highlight starts on the committed choice, and the arrows move
    // it among the choices that can be used, clamped at the ends — the
    // field keeps the focus, so the query stays editable throughout.
    assert_eq!(focused_label(&mut sc).as_deref(), Some("Primary display"));
    sc.simulate_keystrokes("down");
    sc.run_until_parked();
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Pointer's display"),
        "Down moved the highlight"
    );
    sc.simulate_keystrokes("down");
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Active window's display")
    );
    sc.simulate_keystrokes("down");
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Active window's display"),
        "Down at the end stays at the last choice"
    );
    sc.simulate_keystrokes("home");
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Primary display"),
        "Home is the first choice"
    );
    sc.simulate_keystrokes("end");
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Active window's display"),
        "End is the last choice"
    );
    sc.simulate_keystrokes("up");
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Pointer's display"),
        "Up moved the highlight back"
    );
    assert!(
        !data.path().join("settings.json").exists(),
        "moving the highlight saved nothing"
    );
}

#[gpui::test]
fn no_results_say_so_and_enter_does_nothing(cx: &mut TestAppContext) {
    let (_window, _settings, mut sc, data) = open_select(cx);

    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    sc.simulate_input("zzz");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-monitor-empty").is_some(),
        "the no-results line shows"
    );
    assert!(
        a11y(&mut sc).contains("No choices match \u{201c}zzz\u{201d}"),
        "the query is quoted back"
    );
    assert!(
        sc.debug_bounds("launcher-monitor-Primary").is_none(),
        "no choice is drawn"
    );

    // Enter on no result does nothing: the popup stays, nothing is
    // committed, and the field keeps its focus and text.
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-monitor-popup").is_some(),
        "the popup stayed open"
    );
    assert!(
        sc.debug_bounds("launcher-monitor-empty").is_some(),
        "the no-results line still shows"
    );
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Search choices"),
        "the field keeps the focus"
    );
    assert!(
        !data.path().join("settings.json").exists(),
        "Enter on no result saved nothing"
    );
}

#[gpui::test]
fn composition_filters_as_typed_and_commits_committed_text(cx: &mut TestAppContext) {
    use gpui::{EntityInputHandler, Focusable};

    let (_window, settings, mut sc, data) = open_select(cx);
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();

    // The field the window's input handler forwards to, as the form's
    // composition test reaches its own: the popup's field, which holds
    // the keyboard focus.
    let view = settings.entity(&sc.cx).expect("the Settings window's view");
    let input = sc
        .cx
        .read_entity(&view, |window, cx| window.monitor_select_field(cx));
    assert!(
        sc.update(|window, cx| input.focus_handle(cx).is_focused(window)),
        "the popup's field has the keyboard focus"
    );

    // What a platform input method does: mark composing text, which the
    // field holds and the list filters by as it is typed, then replace
    // it with the committed text and commit the highlighted choice.
    sc.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "poi", None, window, cx);
        })
    });
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-monitor-Pointer").is_some(),
        "composing text filters the list as it is typed"
    );
    sc.update(|window, cx| {
        input.update(cx, |input, cx| {
            assert_eq!(input.marked_text_range(window, cx), Some(0..3));
            input.replace_text_in_range(None, "pointer", window, cx);
            assert_eq!(input.marked_text_range(window, cx), None);
        })
    });
    sc.run_until_parked();
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    until_record(&mut sc, data.path(), "\"openingMonitor\": \"pointer\"");
}

#[gpui::test]
fn the_popup_stays_inside_the_window_when_the_room_is_short(cx: &mut TestAppContext) {
    let (_window, settings, mut sc, _data) = open_select(cx);

    // A short window — shorter than the floor the app asks the system
    // for, which the layout still handles — so the popup, opened below
    // the trigger, would reach past the window's bottom edge and must
    // be constrained inside it.
    sc.cx
        .simulate_window_resize(AnyWindowHandle::from(settings), size(px(560.), px(300.)));
    sc.run_until_parked();
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    let window = sc.update(|window, _| window.bounds());
    let popup = sc
        .debug_bounds("launcher-monitor-popup")
        .expect("the popup opens");
    assert!(
        popup.bottom() <= window.bottom(),
        "the popup stays inside the window: {popup:?} in {window:?}"
    );
    assert!(popup.top() >= px(0.), "the popup is on screen");
    // Every choice is reachable in the constrained popup: the list
    // scrolls, the popup does not grow past the window for them.
    for selector in [
        "launcher-monitor-Primary",
        "launcher-monitor-Pointer",
        "launcher-monitor-ActiveWindow",
    ] {
        assert!(
            sc.debug_bounds(selector).is_some(),
            "{selector} is drawn inside the constrained popup"
        );
    }
}

#[gpui::test]
fn the_popup_exits_toward_the_trigger_inert_and_unmounts(cx: &mut TestAppContext) {
    let (_window, settings, mut sc, data) = open_select(cx);

    // A draft the exit will paint: a query that narrows the list.
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    settle_frames(&mut sc);
    // The pointer leaves the trigger it opened the popup with, and the
    // wash it held settles with it, so the frames that follow are the
    // popup's own — none of the pointer's.
    pointer_leaves(&mut sc);
    settle_frames(&mut sc);
    sc.simulate_input("poi");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-monitor-Pointer").is_some(),
        "the draft narrowed the list"
    );

    // Escape closes the popup: the frame that drew this has already
    // returned the keyboard to the trigger, and the exit that follows
    // recedes toward the trigger over the shorter span, painting the
    // draft exactly as the user left it.
    sc.simulate_keystrokes("escape");
    sc.run_until_parked();
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Display"),
        "the focus returned the frame the popup closed"
    );
    let exiting = popup_presentation(&settings, &mut sc).expect("the exit is painting");
    assert!(
        sc.debug_bounds("launcher-monitor-Pointer").is_some(),
        "the exit paints the draft the user saw"
    );
    // Frames pass and the exit recedes: the offset moves toward the
    // trigger and the fade runs all the way out.
    assert!(frame(&mut sc, Duration::from_millis(30)) >= 1);
    let receding = popup_presentation(&settings, &mut sc).expect("the exit is still painting");
    assert!(
        receding.0 < exiting.0,
        "the exit recedes toward the trigger: {} from {}",
        receding.0,
        exiting.0
    );
    assert!(
        receding.1 < 1.,
        "the exit fades the popup out: {}",
        receding.1
    );

    // The exit's visuals are inert: a click on the row that would commit
    // lands on the fading overlay and does nothing — the draft is not
    // saved by the popup's own afterimage.
    click(&mut sc, "launcher-monitor-Pointer");
    sc.run_until_parked();
    assert!(
        !data.path().join("settings.json").exists(),
        "the exiting popup's rows cannot commit"
    );
    // The pointer leaves the fading popup before it unmounts: the page
    // rows it covers are revealed as it goes, and a control the pointer
    // never moved onto takes no wash for its paint's say-so alone.
    pointer_leaves(&mut sc);

    // Past the exit's span the popup unmounts invisible and the window
    // asks for no further frame: nothing of the closed popup is left.
    assert!(frame(&mut sc, Duration::from_millis(90)) >= 1);
    assert!(popup_presentation(&settings, &mut sc).is_none());
    assert!(
        sc.debug_bounds("launcher-monitor-popup").is_none(),
        "the exit unmounted the popup"
    );
    assert_eq!(
        settle_frames(&mut sc),
        0,
        "a closed select schedules no frame"
    );
}

/// A reopen during the exit reverses it: the entrance continues from the
/// presentation on screen instead of restarting, the reopened popup is
/// the interactive one again, and once everything settles nothing of the
/// popup is left to intercept a click — the page under it answers.
#[gpui::test]
fn a_popup_reopened_during_its_exit_retargets_and_blocks_nothing(cx: &mut TestAppContext) {
    let (_window, settings, mut sc, data) = open_select(cx);

    // Open, and let the entrance settle.
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    settle_frames(&mut sc);
    // Close: the exit starts, and part of it runs.
    sc.simulate_keystrokes("escape");
    sc.run_until_parked();
    assert!(popup_presentation(&settings, &mut sc).is_some());
    assert!(frame(&mut sc, Duration::from_millis(25)) >= 1);
    let mid_exit = popup_presentation(&settings, &mut sc).expect("the exit is painting");

    // The choice the popup covers while it exits: a click on the part of
    // it under the popup must not reach it — the overlay, open or
    // exiting, takes the clicks that land on it. (The popup is as wide as
    // its contents, so it covers the reopening track's second segment
    // only at that segment's start.)
    let segment = sc
        .debug_bounds("launcher-reopening-RootSearch")
        .expect("a segment under the popup");
    let popup = sc
        .debug_bounds("launcher-monitor-popup")
        .expect("the exiting popup");
    let covered = segment.intersect(&popup);
    assert!(
        covered.size.width > px(2.) && covered.size.height > px(2.),
        "the popup covers part of the segment: {segment:?} under {popup:?}"
    );
    sc.simulate_click(covered.center(), Modifiers::none());
    sc.run_until_parked();
    assert!(
        !data.path().join("settings.json").exists(),
        "the overlay's click reached nothing underneath"
    );

    // Reopen — Enter on the trigger, which the close returned the
    // keyboard to — with the exit still in flight: the entrance
    // retargets from the presentation on screen, not a fresh start from
    // the trigger.
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    let reversing = popup_presentation(&settings, &mut sc).expect("the popup is reopening");
    assert!(
        reversing.0 > mid_exit.0 - 0.5 && reversing.0 < -1.,
        "the reopen continued from the exit's presentation: {} from {}",
        reversing.0,
        mid_exit.0
    );
    assert!(
        reversing.1 > 0.4,
        "the reopen did not reset to the entrance's floor: {}",
        reversing.1
    );
    // The reopened popup is the interactive one: typing filters it.
    sc.simulate_input("poi");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-monitor-Pointer").is_some(),
        "the reopened popup filters as the user types"
    );
    settle_frames(&mut sc);
    assert!(
        popup_presentation(&settings, &mut sc).is_none(),
        "the reopened popup settled open"
    );

    // Close and settle: the popup unmounts, and the page under where it
    // was answers a click — no invisible overlay survives the exit.
    sc.simulate_keystrokes("escape");
    sc.run_until_parked();
    settle_frames(&mut sc);
    assert!(sc.debug_bounds("launcher-monitor-popup").is_none());
    assert_eq!(settle_frames(&mut sc), 0, "nothing of the popup is left");
    sc.simulate_click(covered.center(), Modifiers::none());
    sc.run_until_parked();
    until_record(&mut sc, data.path(), "\"reopening\": \"root-search\"");
}

/// Reduced motion lands the popup at its endpoint with no frame at all:
/// opening draws it at rest, closing unmounts it at once, and a
/// preference engaged mid-entrance settles it on the next frame.
#[gpui::test]
fn reduced_motion_lands_the_select_popup_at_once(cx: &mut TestAppContext) {
    let (_window, settings, mut sc, _data) = open_select(cx);
    // The page's own opening arrivals began under full motion; let them
    // settle before the preference is flipped, so what follows is the
    // preference's own behavior and nothing pending.
    settle_frames(&mut sc);
    sc.update(|_, cx| cx.set_reduce_motion(true));

    // Opening under reduced motion: no entrance starts — the frame that
    // draws the popup draws it at rest, and asks for no frame.
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    assert!(popup_presentation(&settings, &mut sc).is_none());
    assert!(
        sc.debug_bounds("launcher-monitor-popup").is_some(),
        "the popup is drawn at rest"
    );
    assert_eq!(settle_frames(&mut sc), 0, "no frame was asked for");

    // Closing under reduced motion: the popup unmounts at once.
    sc.simulate_keystrokes("escape");
    sc.run_until_parked();
    assert!(popup_presentation(&settings, &mut sc).is_none());
    assert!(
        sc.debug_bounds("launcher-monitor-popup").is_none(),
        "the popup unmounted at once"
    );
    assert_eq!(settle_frames(&mut sc), 0, "no frame was asked for");

    // Reduced motion engaged mid-entrance ends it on the next frame.
    sc.update(|_, cx| cx.set_reduce_motion(false));
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    assert!(
        popup_presentation(&settings, &mut sc).is_some(),
        "the entrance began under full motion"
    );
    sc.update(|_, cx| cx.set_reduce_motion(true));
    assert!(frame(&mut sc, Duration::ZERO) >= 1);
    assert!(
        popup_presentation(&settings, &mut sc).is_none(),
        "the entrance settled the moment reduced motion engaged"
    );
    assert_eq!(
        settle_frames(&mut sc),
        0,
        "the window asked for no further frame"
    );
}

/// The popup's contents never animate: a query that narrows the rows is
/// a content update — no cascade, no per-option fade, no frame.
#[gpui::test]
fn filtering_never_animates_the_popup_contents(cx: &mut TestAppContext) {
    let (_window, _settings, mut sc, _data) = open_select(cx);

    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    settle_frames(&mut sc);
    // The pointer leaves the trigger it opened the popup with, and the
    // wash it held settles with it: the frames this test counts are the
    // popup's own, none of the pointer's.
    pointer_leaves(&mut sc);
    settle_frames(&mut sc);
    // Typing narrows the list to the choices the query matches: the rows
    // change at once, and the window asks for no cosmetic frame for them.
    // ("poi" matches only the pointer's display; a plain "p" would match
    // every label, whose names all carry the word "display".)
    sc.simulate_input("poi");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-monitor-Pointer").is_some(),
        "the narrowed list is drawn"
    );
    assert!(
        sc.debug_bounds("launcher-monitor-Primary").is_none()
            && sc.debug_bounds("launcher-monitor-ActiveWindow").is_none(),
        "the narrowed list dropped the rest"
    );
    assert_eq!(
        settle_frames(&mut sc),
        0,
        "a content update requested no frame"
    );
    // The draft clears back to every choice — also a content update.
    for _ in 0..3 {
        sc.simulate_keystrokes("backspace");
        sc.run_until_parked();
    }
    assert!(
        sc.debug_bounds("launcher-monitor-ActiveWindow").is_some(),
        "the cleared draft lists every choice"
    );
    // So does the keyboard's own navigation of the highlight: it moves
    // at once, and nothing fades for it.
    sc.simulate_keystrokes("down");
    sc.run_until_parked();
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Pointer's display"),
        "the highlight moved with the keyboard"
    );
    assert_eq!(
        settle_frames(&mut sc),
        0,
        "the highlight's move requested no frame"
    );
}

/// The select is a Settings field (#99): its label over the trigger, a
/// 34px well (black 24% under its ring) showing the committed choice, its
/// description under it. Nothing about it fades: the pointer over the
/// trigger asks for no frame (the Settings board's controls change at
/// once). Its list's rows are the Actions panel's entry family: at least
/// 36 high, the highlighted one on the white 11% wash, never a root row's.
#[gpui::test]
fn the_select_is_a_settings_field_whose_washes_change_at_once(cx: &mut TestAppContext) {
    let (_window, _settings, mut sc, _data) = open_select(cx);
    pointer_leaves(&mut sc);
    settle_frames(&mut sc);

    let trigger = sc.debug_bounds("launcher-monitor").expect("the trigger");
    let label = sc
        .debug_bounds("launcher-monitor-label")
        .expect("the field's label");
    assert_eq!(trigger.size.height, px(34.), "a field's well");
    assert_eq!(
        trigger.top(),
        label.bottom() + px(8.),
        "the label 8px over it"
    );
    assert!(
        paint::paints_fill_at(&mut sc, trigger, 0x0000003D),
        "the well's black 24%"
    );
    for root_wash in [0xFFFFFF09, 0xFFFFFF16] {
        assert!(!paint::paints_fill_at(&mut sc, trigger, root_wash));
    }
    sc.simulate_mouse_move(trigger.center(), None::<MouseButton>, Modifiers::none());
    sc.run_until_parked();
    assert_eq!(
        frame(&mut sc, Duration::ZERO),
        0,
        "the pointer over the trigger asks for no frame"
    );

    // The list: the committed choice highlighted on white 11%.
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    settle_frames(&mut sc);
    let row = sc
        .debug_bounds("launcher-monitor-Primary")
        .expect("the committed choice's row");
    assert!(row.size.height >= px(36.), "an entry's floor: {row:?}");
    assert!(
        paint::paints_fill_at(&mut sc, row, 0xFFFFFF1C),
        "the highlighted entry's white 11%"
    );
    assert!(!paint::paints_fill_at(&mut sc, row, 0xFFFFFF16));

    // The reopening choice is a segmented choice: two 30px segments, the
    // chosen one on white 12%.
    sc.simulate_keystrokes("escape");
    sc.run_until_parked();
    pointer_leaves(&mut sc);
    settle_frames(&mut sc);
    let restore = sc
        .debug_bounds("launcher-reopening-RestoreView")
        .expect("the default's segment");
    let root = sc
        .debug_bounds("launcher-reopening-RootSearch")
        .expect("the other segment");
    assert_eq!(restore.size.height, px(30.));
    assert_eq!(restore.size.width, root.size.width, "equal shares");
    assert!(paint::paints_fill_at(&mut sc, restore, 0xFFFFFF1F));
    assert!(!paint::paints_fill_at(&mut sc, root, 0xFFFFFF1F));
}

#[gpui::test]
fn a_jump_from_the_settings_search_focuses_the_select(cx: &mut TestAppContext) {
    let (_window, _settings, mut sc, _data) = open_select(cx);

    // Leave the Launcher page first, so the jump is a real navigation.
    click(&mut sc, "section-General");
    sc.run_until_parked();
    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("pointer");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("settings-search-result-Pointer's display")
            .is_some(),
        "the choice is a search result"
    );
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();

    // The jump opens the Launcher page and focuses the select's trigger
    // — the one keyboard control the page has — without opening it.
    assert!(
        sc.debug_bounds("launcher-title").is_some(),
        "the Launcher page opened"
    );
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Display"),
        "the jump focused the select's trigger"
    );
    assert!(
        sc.debug_bounds("launcher-monitor-popup").is_none(),
        "the jump opened nothing"
    );
}

/// The keystroke that focuses the Settings search: Cmd+F on macOS,
/// Ctrl+F on Windows and Linux.
fn find_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-f"
    } else {
        "ctrl-f"
    }
}
