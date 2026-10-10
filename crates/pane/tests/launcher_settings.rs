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
use pane_core::{
    CommandMatches, CommandRegistration, CommandWhen, Launcher, PackageIdentity, Runtime, Screen,
    Status,
};
use tempfile::TempDir;

#[path = "support/settle.rs"]
mod settle;

#[path = "support/paint.rs"]
mod paint;

use settle::{settle, settle_shown};

#[path = "support/a11y.rs"]
mod a11y;
#[path = "support/packages.rs"]
mod packages;
#[path = "support/setup.rs"]
mod setup;
#[path = "support/wait.rs"]
mod wait;

use a11y::a11y;
use packages::package;
use setup::{init_settings, settings_shortcut};
use wait::{frame, settle_frames, until, until_record_holds};

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

/// Where the launcher of `cx` opens on the layout's second display. The
/// layout counts physical pixels on Windows and X11, where the test
/// window (1920×1080 at a scale of 2) is larger than the usable area and
/// sits at its top left; on macOS it counts AppKit points, where the
/// window fits and is centered in it.
fn second_display_origin(placement: &FakePlacement, cx: &mut VisualTestContext) -> Point {
    if !cfg!(target_os = "macos") {
        return Point { x: 1980., y: 60. };
    }
    let size = cx.update(|window, _| window.bounds().size);
    placement.layout.borrow().displays[1]
        .window_bounds(Size {
            width: size.width.as_f32(),
            height: size.height.as_f32(),
        })
        .origin
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
        settings_cx.debug_bounds("launcher").is_some(),
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
/// not — without the move the paint's style pass would see a hover the
/// layout's did not.
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

    // The record holds nothing yet: the default is the display with the
    // mouse — the second one, where the pointer is — and the window was
    // placed on it as it opened, in its usable area, which the first
    // recorded move's origin names.
    let default = Shortcut::open_pane_default();
    let second = second_display_origin(&placement, cx);
    assert_eq!(
        placement.origin(0),
        second,
        "the pointer's display's usable area"
    );

    // Reopening the launcher, with the pointer still on the second
    // display, keeps that placement until the choice says otherwise.
    let before = dismiss(&window, &default, cx, &placement);
    press(&window, &default, cx);
    cx.run_until_parked();
    assert!(!hidden(&window, cx));
    assert_eq!(placement.moves.borrow().len(), before + 1);
    assert_eq!(
        placement.origin(before),
        second,
        "the default still opens on the display with the mouse"
    );

    // The choice, taken through the Launcher page's own control: the
    // primary display.
    let (settings, mut settings_cx) = open_launcher_page(cx);
    choose_monitor(&mut settings_cx, "launcher-monitor-Primary");
    settings_cx.run_until_parked();
    until_record_holds(
        &mut settings_cx,
        data.path(),
        "\"openingMonitor\": \"primary\"",
    );
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains("\"value\": \"Primary display\""),
        "the select's trigger shows the committed choice, {tree}"
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
        Point { x: 40., y: 40. },
        "the primary display's usable area, the pointer on the second"
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
        tree.contains("This system doesn't report where the mouse is"),
        "the pointer's display is explained, {tree}"
    );
    assert!(
        tree.contains("This system doesn't report the active window"),
        "the active window's display is explained, {tree}"
    );
    // The primary display is always offered; the default — the display
    // with the mouse, which this system cannot answer — falls back to
    // it, and the page says so.
    assert!(tree.contains("\"label\": \"Primary display\""));
    assert!(
        tree.contains("Pane can't find the display with the mouse"),
        "the default's fallback is explained, {tree}"
    );
    // Choosing a choice that cannot be answered does nothing: the row
    // is not clickable, and nothing is saved. The popup is still open
    // from the reading above — the click lands on the row itself.
    click(&mut settings_cx, "launcher-monitor-ActiveWindow");
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
    until_record_holds(
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
        tree.contains("This system doesn't report the active window"),
        "the choice is explained, {tree}"
    );
    assert!(
        tree.contains("Pane can't find the display with the active window"),
        "the fallback is explained, {tree}"
    );
    assert!(
        tree.contains("the launcher opens on the primary display instead"),
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
    // the layout no longer lists it, and says so in the same words — Pane
    // can't find it.
    *placement.layout.borrow_mut() = DisplayLayout {
        displays: vec![display(2, 0., 0., 2560., 1440., 60.)],
        primary: Some(DisplayId(2)),
        pointer: Some(Point { x: 2500., y: 700. }),
        active: Some(DisplayId(1)),
    };
    let (_settings, mut settings_cx) = open_launcher_page(cx);
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains("Pane can't find the display with the active window"),
        "the disconnected display is explained, {tree}"
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
        tree.contains("\"label\": \"Pop to root search\""),
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
    // Pop to root search, Immediately: the select's choice.
    click(&mut settings_cx, "launcher-reopening");
    settings_cx.run_until_parked();
    click(&mut settings_cx, "launcher-reopening-RootSearch");
    settings_cx.run_until_parked();
    until_record_holds(
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

    // Both choices, taken through the page's own controls: the primary
    // display — not the default, which follows the pointer to the second
    // display — and root search.
    let (_settings, mut settings_cx) = open_launcher_page(cx);
    choose_monitor(&mut settings_cx, "launcher-monitor-Primary");
    settings_cx.run_until_parked();
    // Pop to root search, Immediately: the select's choice.
    click(&mut settings_cx, "launcher-reopening");
    settings_cx.run_until_parked();
    click(&mut settings_cx, "launcher-reopening-RootSearch");
    settings_cx.run_until_parked();
    until_record_holds(
        &mut settings_cx,
        data.path(),
        "\"openingMonitor\": \"primary\"",
    );
    until_record_holds(
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

    // The fresh window opened on the display the record names, not the
    // one the pointer is on.
    assert_eq!(
        fresh_placement.origin(0),
        Point { x: 40., y: 40. },
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

    // A change that lands and is saved: the primary display, away from
    // the default's display with the mouse.
    let (_settings, mut settings_cx) = open_launcher_page(cx);
    choose_monitor(&mut settings_cx, "launcher-monitor-Primary");
    settings_cx.run_until_parked();
    until_record_holds(
        &mut settings_cx,
        data.path(),
        "\"openingMonitor\": \"primary\"",
    );

    // Break the record's replacement: a folder where the record belongs,
    // so the atomic write cannot rename over it.
    fs::remove_file(data.path().join("settings.json")).unwrap();
    fs::create_dir(data.path().join("settings.json")).unwrap();

    // Another change: it cannot be saved, and the failure is reported.
    // Pop to root search, Immediately: the select's choice.
    click(&mut settings_cx, "launcher-reopening");
    settings_cx.run_until_parked();
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
        tree.contains("\"value\": \"Never\""),
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
        Point { x: 40., y: 40. },
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

/// Chooses the reopening whose choice row's debug selector is `selector`
/// through the page's own select, and waits for the record to hold `held`.
fn choose_reopening(
    settings_cx: &mut VisualTestContext,
    data: &Path,
    selector: &'static str,
    held: &str,
) {
    click(settings_cx, "launcher-reopening");
    settings_cx.run_until_parked();
    click(settings_cx, selector);
    settings_cx.run_until_parked();
    until_record_holds(settings_cx, data, held);
}

/// Checks the delayed reopening of `after` on the executor's simulated
/// clock: a launcher hidden for less than `after` comes back to the view
/// it was left on, and one hidden for `after` starts from root search.
/// Each press moves the clock past the repeat guard first, so the time
/// hidden is the advance plus that.
fn pops_to_root_after(
    window: &gpui::Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    placement: &FakePlacement,
    after: Duration,
) {
    let default = Shortcut::open_pane_default();
    cx.simulate_input("zz");
    assert_eq!(settle(window, cx).query(), Some("zz"));

    // Long after it opened, the launcher is still shown without focus, as
    // the test platform opens it: the hotkey brings it forward, and since
    // nothing was hidden there is nothing to pop.
    cx.executor().advance_clock(after);
    press(window, &default, cx);
    cx.run_until_parked();
    assert!(!hidden(window, cx));
    assert_eq!(
        settle(window, cx).query(),
        Some("zz"),
        "a launcher never hidden keeps its view"
    );

    // Back a moment before the delay is up: the view is restored, with
    // its search focused.
    dismiss(window, &default, cx, placement);
    cx.executor().advance_clock(after - Duration::from_secs(2));
    press(window, &default, cx);
    cx.run_until_parked();
    assert!(!hidden(window, cx));
    let view = settle(window, cx);
    assert_eq!(view.query(), Some("zz"), "a quick return restores the view");

    // Away for the whole delay: the reopening starts from root search
    // with an empty query.
    dismiss(window, &default, cx, placement);
    cx.executor().advance_clock(after);
    press(window, &default, cx);
    cx.run_until_parked();
    assert!(!hidden(window, cx));
    let view = settle(window, cx);
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(
        view.query(),
        Some(""),
        "a late return starts from root search"
    );
    cx.simulate_input("q");
    assert_eq!(
        settle(window, cx).query(),
        Some("q"),
        "root search's query has focus"
    );
}

#[gpui::test]
fn after_90_seconds_restores_a_quick_return_and_pops_a_late_one_to_root(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(None, None);
    let (window, cx) = open(cx, Some(data.path()), &placement);

    let (_settings, mut settings_cx) = open_launcher_page(cx);
    choose_reopening(
        &mut settings_cx,
        data.path(),
        "launcher-reopening-After90Seconds",
        "\"reopening\": \"after-90-seconds\"",
    );
    pops_to_root_after(&window, cx, &placement, Duration::from_secs(90));
}

#[gpui::test]
fn after_3_minutes_restores_a_quick_return_and_pops_a_late_one_to_root(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(None, None);
    let (window, cx) = open(cx, Some(data.path()), &placement);

    let (_settings, mut settings_cx) = open_launcher_page(cx);
    choose_reopening(
        &mut settings_cx,
        data.path(),
        "launcher-reopening-After3Minutes",
        "\"reopening\": \"after-3-minutes\"",
    );
    pops_to_root_after(&window, cx, &placement, Duration::from_secs(180));
}

#[gpui::test]
fn the_delayed_reopening_and_the_vertical_layout_are_applied_by_a_fresh_application(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    // Two pins, kept by a Pane that ran before; their commands are not
    // installed, so they show as unavailable, which is still a pin.
    fs::write(
        data.path().join("quick-slots.json"),
        serde_json::json!({
            "version": 2,
            "pins": [{ "command": "one" }, { "command": "two" }],
        })
        .to_string(),
    )
    .unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(None, None);
    let (_window, cx) = open(cx, Some(data.path()), &placement);

    // Both choices, taken through the page's own controls. The page
    // grew with the specification's rows (#200, #206), so the pinned
    // layout's row sits below the page's fold: the settings search jumps
    // to it and reveals it, as it does for any row.
    let (_settings, mut settings_cx) = open_launcher_page(cx);
    choose_reopening(
        &mut settings_cx,
        data.path(),
        "launcher-reopening-After90Seconds",
        "\"reopening\": \"after-90-seconds\"",
    );
    settings_cx.simulate_keystrokes(find_shortcut());
    settings_cx.simulate_input("pinned");
    settings_cx.run_until_parked();
    settings_cx.simulate_keystrokes("enter");
    settings_cx.run_until_parked();
    settle_frames(&mut settings_cx);
    click(&mut settings_cx, "launcher-pinned-Vertical");
    settings_cx.run_until_parked();
    until_record_holds(
        &mut settings_cx,
        data.path(),
        "\"pinnedLayout\": \"vertical\"",
    );

    // A fresh application over the same data folder.
    let mut fresh = cx.cx.new_app();
    let fresh_placement = Rc::new(FakePlacement::default());
    fresh_placement.layout(None, None);
    fresh.update(|cx| pane::placement::init(fresh_placement.clone() as Rc<dyn Placement>, cx));
    fresh.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.path().to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    fresh.update(pane::bind_keys);
    let launcher = Launcher::new(Runtime::start(), Vec::new())
        .with_hotkeys(Arc::new(FakeSystem::default()))
        .with_quick_slots(data.path());
    let (window, fresh_cx) =
        fresh.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    fresh_cx.run_until_parked();
    settle(&window, fresh_cx);

    // The pins are result rows, not the strip of tiles.
    assert!(fresh_cx.debug_bounds("slot-1").is_some(), "the first pin");
    assert!(fresh_cx.debug_bounds("slot-2").is_some(), "the second pin");
    assert!(
        fresh_cx.debug_bounds("pinned-strip").is_none(),
        "no strip in the vertical layout"
    );

    // And the reopening waits 90 seconds before starting from root search.
    pops_to_root_after(&window, fresh_cx, &fresh_placement, Duration::from_secs(90));
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
    search_cx.simulate_input("display");
    search_cx.run_until_parked();
    for result in [
        "settings-search-result-Display with the mouse",
        "settings-search-result-Primary display",
        "settings-search-result-Display with the active window",
    ] {
        assert!(
            search_cx.debug_bounds(result).is_some(),
            "the opening display's choices are found: {result}"
        );
    }
    let tree = a11y(&mut search_cx);
    assert!(
        tree.contains("Launcher \u{b7} Display"),
        "the result names the page and the group, {tree}"
    );
    // Escape clears the query, and the reopening choices are found by
    // their own words.
    search_cx.simulate_keystrokes("escape");
    search_cx.run_until_parked();
    search_cx.simulate_input("pop to root");
    search_cx.run_until_parked();
    assert!(
        search_cx
            .debug_bounds("settings-search-result-Immediately")
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

/// The Launcher page's Search sensitivity control (#193): found through
/// the Settings search, chosen through the page's own select, recorded in
/// the settings record — and applied by the launcher on the next
/// keystroke, so the same query finds more results once the choice is
/// taken.
#[gpui::test]
fn the_search_sensitivity_control_changes_the_results_live(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(Some(Point { x: 100., y: 100. }), None);
    cx.update(|cx| pane::placement::init(placement.clone() as Rc<dyn Placement>, cx));
    init_settings(Some(data.path()), cx);
    let component =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/sample_rust.wasm");
    assert!(
        component.exists(),
        "{} is missing; run `cargo xtask guests`",
        component.display()
    );
    let commands = vec![CommandRegistration {
        id: "undownloadable".into(),
        title: "Undownloadable files".into(),
        subtitle: None,
        component,
        takes_query: false,
        search: false,
        keywords: Vec::new(),
        when: CommandWhen::Always,
        matches: CommandMatches::Title,
    }];
    let launcher =
        Launcher::new(Runtime::start(), commands).with_hotkeys(Arc::new(FakeSystem::default()));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));

    // The default is High: "download" sits mid-word in "Undownloadable
    // files", and High wants a match that starts the text or a word.
    cx.simulate_input("download");
    let view = settle(&window, cx);
    assert!(view.rows.is_empty(), "High, the default");

    // The control is found through the Settings search, and its Medium
    // choice taken through the page's own select.
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
    settings_cx.simulate_keystrokes(find_shortcut());
    settings_cx.simulate_input("sensitivity");
    settings_cx.run_until_parked();
    assert!(
        settings_cx
            .debug_bounds("settings-search-result-Medium")
            .is_some(),
        "the sensitivity's choices are found"
    );
    let tree = a11y(&mut settings_cx);
    assert!(
        tree.contains("Launcher \u{b7} Search sensitivity"),
        "the result names the page and the group, {tree}"
    );
    settings_cx.simulate_keystrokes("enter");
    settings_cx.run_until_parked();
    settle_frames(&mut settings_cx);
    click(&mut settings_cx, "launcher-sensitivity");
    settings_cx.run_until_parked();
    click(&mut settings_cx, "launcher-sensitivity-Medium");
    settings_cx.run_until_parked();
    until_record_holds(
        &mut settings_cx,
        data.path(),
        "\"searchSensitivity\": \"medium\"",
    );

    // The choice applies on the next keystroke: the list the query has
    // already made stays as it is, and the query made again holds the
    // command.
    let view = cx.read_entity(&window, |window, _| window.launcher().view());
    assert!(
        view.rows.is_empty(),
        "the list stays until the query changes"
    );
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert_eq!(view.query(), Some(""));
    cx.simulate_input("download");
    let view = settle(&window, cx);
    assert_eq!(
        view.rows
            .iter()
            .map(|row| row.title.as_str())
            .collect::<Vec<_>>(),
        ["Undownloadable files"],
        "Medium holds the mid-word match"
    );
}

/// A package folder whose two same-titled commands are the no-view
/// sample's component: invoking one keeps root search on screen, so a
/// choice is one keystroke.
fn pythons(folder: &Path) -> PathBuf {
    let component =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/sample_no_view.wasm");
    assert!(
        component.exists(),
        "{} is missing; run `cargo xtask guests`",
        component.display()
    );
    fs::create_dir_all(folder).unwrap();
    let manifest = r#"{
        "manifestVersion": 1,
        "title": "Pythons",
        "version": "0.1.0",
        "apiVersion": "0.1",
        "commands": [
            { "id": "a", "title": "Python", "component": "component.wasm", "mode": "no-view" },
            { "id": "b", "title": "Python", "component": "component.wasm", "mode": "no-view" }
        ]
    }"#;
    fs::write(folder.join("pane.json"), manifest).unwrap();
    fs::copy(&component, folder.join("component.wasm")).unwrap();
    folder.to_path_buf()
}

/// A launcher window over `data`, whose extensions folder holds a record
/// of one learned use — the second of the two same-titled commands
/// `pythons` installs, chosen with "pyt" — so what that record says can
/// be reset and turned off. Where the learned record is.
fn over_learned_data<'a>(
    cx: &'a mut TestAppContext,
    data: &Path,
) -> (
    gpui::Entity<LauncherWindow>,
    &'a mut VisualTestContext,
    PathBuf,
) {
    let folder = pythons(&data.join("sources").join("pythons"));
    let key = PackageIdentity::local(&folder).unwrap().key();
    let extensions = data.join("extensions");
    fs::create_dir_all(&extensions).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let record = extensions.join("learned.json");
    // Built with `serde_json`, so the key's path is escaped as JSON needs
    // it (a Windows path holds backslashes, which a hand-written record
    // would leave invalid and the launcher would refuse to read).
    let uses = [(
        format!("{key}#b"),
        serde_json::json!({
            "score": 3.0,
            "lastOpened": now,
            "queries": ["pyt"],
        }),
    )]
    .into_iter()
    .collect::<serde_json::Map<String, serde_json::Value>>();
    fs::write(
        &record,
        serde_json::json!({ "version": 1, "uses": uses }).to_string(),
    )
    .unwrap();
    let launcher =
        Launcher::with_packages(Ok(Runtime::start().unwrap()), vec![], extensions.clone())
            .with_hotkeys(Arc::new(FakeSystem::default()));
    cx.executor().allow_parking();
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    launcher.back();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (window, cx, record)
}

/// The Launcher page's "Reset ranking…" (#200): it asks for the
/// confirmation such a loss needs — the first press arms the row, and
/// Cancel stands it down — and the confirmed reset clears what every
/// result learned, the record and the ranking both.
#[gpui::test]
fn reset_ranking_in_settings_asks_first_and_clears_everything(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(Some(Point { x: 100., y: 100. }), Some(DisplayId(1)));
    cx.update(|cx| pane::placement::init(placement.clone() as Rc<dyn Placement>, cx));
    init_settings(Some(data.path()), cx);
    let (window, cx, record) = over_learned_data(cx, data.path());

    // What was learned ranks the chosen Python first for the query.
    cx.simulate_input("pyt");
    let view = settle(&window, cx);
    assert!(
        view.rows[0].id.ends_with("#b"),
        "the learned Python ranks first: {:?}",
        view.rows
    );

    // Settings › Launcher: the reset asks first.
    let (settings, mut sc) = open_launcher_page(cx);
    assert!(
        sc.debug_bounds("launcher-reset-ranking-field").is_some(),
        "the row is drawn"
    );
    click(&mut sc, "launcher-reset-ranking");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-reset-ranking-reset").is_some(),
        "the reset asks for its confirmation"
    );
    assert!(sc.debug_bounds("launcher-reset-ranking-cancel").is_some());
    // Cancel stands it down: nothing was reset.
    click(&mut sc, "launcher-reset-ranking-cancel");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-reset-ranking-reset").is_none(),
        "the row stood down"
    );
    assert!(
        fs::read_to_string(&record).unwrap().contains("#b"),
        "nothing went"
    );

    // The confirmed reset clears the record.
    click(&mut sc, "launcher-reset-ranking");
    sc.run_until_parked();
    click(&mut sc, "launcher-reset-ranking-reset");
    sc.run_until_parked();
    until(&mut sc, |_| {
        fs::read_to_string(&record)
            .ok()
            .filter(|text| !text.contains("#b"))
    });

    // The same query now ranks the unlearned order.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_input("pyt");
    let view = settle(&window, cx);
    assert!(
        view.rows[0].id.ends_with("#a"),
        "nothing weighs in ranking: {:?}",
        view.rows
    );
    let _ = settings;
}

/// The Launcher page's "Learn from what I choose" switch (#200): found
/// through the Settings search, turned off it stops anything being
/// recorded and ranking weighing what was learned — what was learned is
/// kept, so turning it on again uses it.
#[gpui::test]
fn the_learn_switch_is_found_through_the_settings_search(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(Some(Point { x: 100., y: 100. }), Some(DisplayId(1)));
    cx.update(|cx| pane::placement::init(placement.clone() as Rc<dyn Placement>, cx));
    init_settings(Some(data.path()), cx);
    let (window, cx, record) = over_learned_data(cx, data.path());

    // The switch is found through the Settings search, and Enter jumps to
    // the page and reveals its row.
    let (settings, mut sc) = open_launcher_page(cx);
    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("learn");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("settings-search-result-Learn from what I choose")
            .is_some(),
        "the switch is found"
    );
    let tree = a11y(&mut sc);
    assert!(
        tree.contains("Launcher \u{b7} Learn from what I choose"),
        "the result names the page and the group, {tree}"
    );
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    settle_frames(&mut sc);
    assert!(
        sc.debug_bounds("launcher-learn-row").is_some(),
        "the row is revealed"
    );
    assert!(
        switch_on(&mut sc, "Learn from what I choose"),
        "on by default"
    );

    // Turned off: the order ignores what was learned, and a choice
    // records nothing — the record stands as it was.
    click(&mut sc, "launcher-learn-row");
    until_record_holds(&mut sc, data.path(), "\"learning\": false");
    assert!(!switch_on(&mut sc, "Learn from what I choose"));
    let before = fs::read_to_string(&record).unwrap();
    cx.simulate_input("pyt");
    let view = settle(&window, cx);
    assert!(
        view.rows[0].id.ends_with("#a"),
        "the order ignores what was learned: {:?}",
        view.rows
    );
    cx.simulate_keystrokes("down enter");
    // The no-view sample's answer for the pythons' own command ids is an
    // error toast (the component knows its manifest's commands, not the
    // fixture's); either way the status line is no longer idle, and the
    // record stands as it was: nothing was recorded.
    assert_ne!(settle_shown(&window, cx), Status::Idle, "the choice ran");
    assert_eq!(
        fs::read_to_string(&record).unwrap(),
        before,
        "no use was recorded"
    );

    // Turned on again: what was kept weighs again.
    let mut sc = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);
    click(&mut sc, "launcher-learn-row");
    until_record_holds(&mut sc, data.path(), "\"learning\": true");
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    cx.simulate_input("pyt");
    let view = settle(&window, cx);
    assert!(
        view.rows[0].id.ends_with("#b"),
        "what was kept weighs again: {:?}",
        view.rows
    );
}

/// Whether the accessibility tree of the window `cx` drives has the switch
/// named `title`, on.
fn switch_on(cx: &mut VisualTestContext, title: &str) -> bool {
    aria_nodes(cx)
        .iter()
        .any(|aria| aria["role"] == "Switch" && aria["label"] == title && aria["toggled"] == "True")
}

#[gpui::test]
fn the_compact_pinned_switch_is_in_the_layout_card_and_is_saved(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(Some(Point { x: 100., y: 100. }), Some(DisplayId(1)));
    let (window, cx) = open(cx, Some(data.path()), &placement);
    let (settings, mut sc) = open_launcher_page(cx);

    // The switch is the Layout card's, right under the window mode it
    // qualifies and above the pinned items' layout.
    let mode = sc
        .debug_bounds("launcher-window-mode-field")
        .expect("the window mode's row");
    let row = sc
        .debug_bounds("launcher-compact-pinned-row")
        .expect("the switch's row");
    let pinned = sc
        .debug_bounds("launcher-pinned-field")
        .expect("the pinned items' row");
    assert!(
        mode.bottom() <= row.top() && row.bottom() <= pinned.top(),
        "between the window mode and the pinned items: {mode:?} {row:?} {pinned:?}"
    );
    assert!(
        sc.debug_bounds("launcher-compact-pinned").is_some(),
        "the switch is drawn"
    );
    // Off by default.
    assert!(
        !switch_on(&mut sc, "Show pinned in compact window mode"),
        "off by default"
    );

    // A click anywhere on the row takes the choice, and the record keeps
    // it under its camelCase name.
    click(&mut sc, "launcher-compact-pinned-row");
    until_record_holds(&mut sc, data.path(), "\"compactPinned\": true");
    assert!(
        switch_on(&mut sc, "Show pinned in compact window mode"),
        "the switch shows the choice"
    );

    // And again turns it off.
    click(&mut sc, "launcher-compact-pinned-row");
    until_record_holds(&mut sc, data.path(), "\"compactPinned\": false");
    assert!(
        !switch_on(&mut sc, "Show pinned in compact window mode"),
        "the switch shows the choice"
    );

    // The settings search finds it in the Layout group.
    let mut search_cx = VisualTestContext::from_window(AnyWindowHandle::from(settings), &cx.cx);
    search_cx.simulate_keystrokes(find_shortcut());
    search_cx.simulate_input("compact mode");
    search_cx.run_until_parked();
    assert!(
        search_cx
            .debug_bounds("settings-search-result-Show pinned in compact window mode")
            .is_some(),
        "the switch is found"
    );
    let tree = a11y(&mut search_cx);
    assert!(
        tree.contains("Launcher \u{b7} Layout"),
        "the result names the page and the group, {tree}"
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
        (popup.top() - (trigger.bottom() + px(4.))).abs() <= px(1.),
        "the popup settles 4px below the trigger: {popup:?} under {trigger:?}"
    );
    // The popup is the trigger's width — a row's choice, at the row's
    // end — so the list reads as the trigger opened.
    assert_eq!(
        popup.size.width, trigger.size.width,
        "the popup is the trigger's width: {popup:?} vs {trigger:?}"
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
        Some("Display with the mouse"),
        "the popup's field takes the focus, the highlight on the committed choice"
    );

    // The exposure: the trigger is a combo box carrying the committed
    // choice as its value and the open state; the choices are a list
    // box's options, the committed one selected.
    let trigger_aria = aria_nodes(&mut sc)
        .into_iter()
        .find(|aria| aria["role"] == "ComboBox" && aria["label"] == "Display")
        .expect("the trigger is a named combo box");
    assert_eq!(
        trigger_aria["value"].as_str(),
        Some("Display with the mouse")
    );
    assert!(trigger_aria["expanded"].as_bool().unwrap());
    // The choices are a list box's options — among the window's other
    // options, the sidebar's sections — so they are found by their
    // labels, and exactly the committed one is selected.
    let options = aria_nodes(&mut sc)
        .into_iter()
        .filter(|aria| {
            [
                "Display with the mouse",
                "Primary display",
                "Display with the active window",
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
    // alike: "main" is the primary display's keyword, not its label.
    sc.simulate_input("main");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-monitor-Primary").is_some(),
        "the keyword finds the primary display's choice"
    );
    assert!(
        sc.debug_bounds("launcher-monitor-Pointer").is_none(),
        "the other choices are filtered out"
    );

    // Enter commits the highlighted choice through the host settings:
    // the record is written, the popup closes, and the keyboard returns
    // to the trigger, which shows what was kept.
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    until_record_holds(&mut sc, data.path(), "\"openingMonitor\": \"primary\"");
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
        Some("Primary display"),
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
    sc.simulate_input("active");
    sc.run_until_parked();
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Display with the active window"),
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
        Some("Display with the mouse"),
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

    // A query that names a choice other than the committed one, so a
    // commit would be identifiable.
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    sc.simulate_input("active");
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
    sc.simulate_input("active");
    sc.run_until_parked();
    let field = sc
        .debug_bounds("settings-search-field")
        .expect("the sidebar's search field");
    // The pointer travels there first, as a real one does (see [`click`]):
    // a click's landing alone leaves the trigger it left believing it is
    // still hovered.
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
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Display with the mouse")
    );
    sc.simulate_keystrokes("down");
    sc.run_until_parked();
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Primary display"),
        "Down moved the highlight"
    );
    sc.simulate_keystrokes("down");
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Display with the active window")
    );
    sc.simulate_keystrokes("down");
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Display with the active window"),
        "Down at the end stays at the last choice"
    );
    sc.simulate_keystrokes("home");
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Display with the mouse"),
        "Home is the first choice"
    );
    sc.simulate_keystrokes("end");
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Display with the active window"),
        "End is the last choice"
    );
    sc.simulate_keystrokes("up");
    assert_eq!(
        focused_label(&mut sc).as_deref(),
        Some("Primary display"),
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
            input.replace_and_mark_text_in_range(None, "pri", None, window, cx);
        })
    });
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-monitor-Primary").is_some()
            && sc.debug_bounds("launcher-monitor-Pointer").is_none(),
        "composing text filters the list as it is typed"
    );
    sc.update(|window, cx| {
        input.update(cx, |input, cx| {
            assert_eq!(input.marked_text_range(window, cx), Some(0..3));
            input.replace_text_in_range(None, "primary", window, cx);
            assert_eq!(input.marked_text_range(window, cx), None);
        })
    });
    sc.run_until_parked();
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    until_record_holds(&mut sc, data.path(), "\"openingMonitor\": \"primary\"");
}

#[gpui::test]
fn the_popup_stays_inside_the_window_when_the_room_is_short(cx: &mut TestAppContext) {
    let (_window, settings, mut sc, _data) = open_select(cx);

    // A short window — shorter than the floor the app asks the system
    // for (560×400), which the layout still handles — so the popup,
    // opened below the trigger near the page's top, would reach past the
    // window's bottom edge and must be constrained inside it.
    sc.cx
        .simulate_window_resize(AnyWindowHandle::from(settings), size(px(560.), px(220.)));
    sc.run_until_parked();
    click(&mut sc, "launcher-monitor");
    sc.run_until_parked();
    // The window's own drawable area, in the coordinates the popup is
    // measured in (the window's bounds are the screen's).
    let viewport = sc.update(|window, _| window.viewport_size());
    let trigger = sc.debug_bounds("launcher-monitor").expect("the trigger");
    let popup = sc
        .debug_bounds("launcher-monitor-popup")
        .expect("the popup opens");
    assert!(
        trigger.bottom() + px(4.) + popup.size.height > viewport.height,
        "the room below the trigger is short: {popup:?} under {trigger:?} in {viewport:?}"
    );
    assert!(
        popup.bottom() <= viewport.height,
        "the popup stays inside the window: {popup:?} in {viewport:?}"
    );
    assert!(popup.top() >= px(0.), "the popup is on screen");
    // Every choice is reachable in the constrained popup: it moves up
    // to fit rather than growing past the window for them.
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
    sc.simulate_input("act");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-monitor-ActiveWindow").is_some()
            && sc.debug_bounds("launcher-monitor-Pointer").is_none(),
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
        sc.debug_bounds("launcher-monitor-ActiveWindow").is_some()
            && sc.debug_bounds("launcher-monitor-Pointer").is_none(),
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
    click(&mut sc, "launcher-monitor-ActiveWindow");
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

    // The control the popup covers while it exits — the Pop to root
    // search select's trigger, in the row below: a click on the part of
    // it under the popup must not reach it — the overlay, open or
    // exiting, takes the clicks that land on it.
    let segment = sc
        .debug_bounds("launcher-reopening")
        .expect("a control under the popup");
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
    // The fade turns around where the exit left it too: no jump up to
    // the entrance's floor, which a fresh open starts from.
    assert!(
        reversing.1 > 0. && reversing.1 <= mid_exit.1 + 0.05,
        "the reopen's fade jumped instead of continuing from the exit's: {} from {}",
        reversing.1,
        mid_exit.1
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
    assert!(
        sc.debug_bounds("launcher-reopening-popup").is_some(),
        "the click opened the select underneath"
    );
    click(&mut sc, "launcher-reopening-RootSearch");
    sc.run_until_parked();
    until_record_holds(&mut sc, data.path(), "\"reopening\": \"root-search\"");
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
    // ("poi" matches only the display with the mouse, by its "pointer"
    // keyword; a plain "p" would match every label, whose names all
    // carry the word "display".)
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
        Some("Primary display"),
        "the highlight moved with the keyboard"
    );
    assert_eq!(
        settle_frames(&mut sc),
        0,
        "the highlight's move requested no frame"
    );
}

/// The select is a Settings row's control (#99): the row names the
/// setting at its left, and the trigger at its right end is a 30px inline
/// well (black 24% under its ring) showing the committed choice. Nothing
/// about it fades: the pointer over the trigger asks for no frame (the
/// Settings board's controls change at once). Its list's rows are the
/// Actions panel's entry family: at least 36 high, the highlighted one on
/// the white 11% wash, never a root row's.
#[gpui::test]
fn the_select_is_a_settings_field_whose_washes_change_at_once(cx: &mut TestAppContext) {
    let (_window, _settings, mut sc, _data) = open_select(cx);
    pointer_leaves(&mut sc);
    settle_frames(&mut sc);

    let trigger = sc.debug_bounds("launcher-monitor").expect("the trigger");
    let row = sc
        .debug_bounds("launcher-monitor-field")
        .expect("the Display row");
    assert_eq!(trigger.size.height, px(30.), "an inline well");
    // At the row's end, inside its 14px padding, and centered on it.
    assert!(
        (trigger.right() - (row.right() - px(14.))).abs() <= px(1.),
        "the well ends the row: {trigger:?} in {row:?}"
    );
    assert!(
        (trigger.center().y - row.center().y).abs() <= px(1.),
        "the well is centered in the row: {trigger:?} in {row:?}"
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
        .debug_bounds("launcher-monitor-Pointer")
        .expect("the committed choice's row");
    assert!(row.size.height >= px(36.), "an entry's floor: {row:?}");
    assert!(
        paint::paints_fill_at(&mut sc, row, 0xFFFFFF1C),
        "the highlighted entry's white 11%"
    );
    assert!(!paint::paints_fill_at(&mut sc, row, 0xFFFFFF16));

    // The window mode is a segmented choice: two 30px segments, the
    // chosen one on white 12%.
    sc.simulate_keystrokes("escape");
    sc.run_until_parked();
    pointer_leaves(&mut sc);
    settle_frames(&mut sc);
    let expanded = sc
        .debug_bounds("launcher-window-Expanded")
        .expect("the default's segment");
    let compact = sc
        .debug_bounds("launcher-window-Compact")
        .expect("the other segment");
    assert_eq!(expanded.size.height, px(30.));
    assert_eq!(expanded.size.width, compact.size.width, "equal shares");
    assert!(paint::paints_fill_at(&mut sc, expanded, 0xFFFFFF1F));
    assert!(!paint::paints_fill_at(&mut sc, compact, 0xFFFFFF1F));
}

#[gpui::test]
fn a_jump_from_the_settings_search_focuses_the_select(cx: &mut TestAppContext) {
    let (_window, _settings, mut sc, _data) = open_select(cx);

    // Leave the Launcher page first, so the jump is a real navigation.
    click(&mut sc, "section-General");
    sc.run_until_parked();
    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("mouse");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("settings-search-result-Display with the mouse")
            .is_some(),
        "the choice is a search result"
    );
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();

    // The jump opens the Launcher page and focuses the trigger of the
    // select that offers the choice, without opening it.
    assert!(
        sc.debug_bounds("launcher").is_some(),
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

/// A launcher window over `data`, whose extensions folder holds a search
/// history of one query, "pyt", so what that record says can be reset
/// and recalled. Where the search history's record is.
fn over_history_data<'a>(
    cx: &'a mut TestAppContext,
    data: &Path,
) -> (
    gpui::Entity<LauncherWindow>,
    &'a mut VisualTestContext,
    PathBuf,
) {
    let extensions = data.join("extensions");
    fs::create_dir_all(&extensions).unwrap();
    let record = extensions.join("search-history.json");
    fs::write(
        &record,
        r#"{ "version": 1, "queries": [ { "query": "pyt" } ] }"#,
    )
    .unwrap();
    let launcher =
        Launcher::with_packages(Ok(Runtime::start().unwrap()), vec![], extensions.clone())
            .with_hotkeys(Arc::new(FakeSystem::default()));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    (window, cx, record)
}

/// The Launcher page's "Reset search history" (#206): found through the
/// Settings search, it asks for the confirmation such a loss needs — the
/// first press arms the row, and Cancel stands it down — and the
/// confirmed reset clears the recent queries, so Up recalls nothing.
#[gpui::test]
fn reset_search_history_in_settings_asks_first_and_clears_the_queries(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let placement = Rc::new(FakePlacement::default());
    placement.layout(Some(Point { x: 100., y: 100. }), Some(DisplayId(1)));
    cx.update(|cx| pane::placement::init(placement.clone() as Rc<dyn Placement>, cx));
    init_settings(Some(data.path()), cx);
    let (window, cx, record) = over_history_data(cx, data.path());

    // Up on the empty query recalls the recorded one.
    cx.simulate_keystrokes("up");
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "pyt".into()
        },
        "the recorded query is recalled"
    );

    // Settings › Launcher: the row is found through the Settings search,
    // and the reset asks first.
    let (settings, mut sc) = open_launcher_page(cx);
    sc.simulate_keystrokes(find_shortcut());
    sc.simulate_input("search history");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("settings-search-result-Reset search history")
            .is_some(),
        "the row is found through the Settings search"
    );
    sc.simulate_keystrokes("enter");
    sc.run_until_parked();
    settle_frames(&mut sc);
    assert!(
        sc.debug_bounds("launcher-reset-history-field").is_some(),
        "the row is revealed"
    );
    click(&mut sc, "launcher-reset-history");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-reset-history-reset").is_some(),
        "the reset asks for its confirmation"
    );
    assert!(sc.debug_bounds("launcher-reset-history-cancel").is_some());
    // Cancel stands it down: nothing was reset.
    click(&mut sc, "launcher-reset-history-cancel");
    sc.run_until_parked();
    assert!(
        sc.debug_bounds("launcher-reset-history-reset").is_none(),
        "the row stood down"
    );
    assert!(
        fs::read_to_string(&record).unwrap().contains("pyt"),
        "nothing went"
    );

    // The confirmed reset clears the record: nothing is left to recall.
    click(&mut sc, "launcher-reset-history");
    sc.run_until_parked();
    click(&mut sc, "launcher-reset-history-reset");
    sc.run_until_parked();
    until(&mut sc, |_| {
        fs::read_to_string(&record)
            .ok()
            .filter(|text| !text.contains("pyt"))
    });
    assert_eq!(
        cx.read_entity(&window, |window, _| window.launcher().recent_query(0)),
        None,
        "nothing is recalled"
    );
    // The query the walk restored still stands: the reset clears the
    // history, not the search on screen.
    let view = settle(&window, cx);
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "pyt".into()
        }
    );
    let _ = settings;
}
