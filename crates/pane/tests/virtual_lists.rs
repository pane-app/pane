//! Virtualized lists in the launcher's window (#165): a list of ten
//! thousand rows lays out and paints only the rows in view and a few past
//! its edges; Up, Down, Page Down and Page Up keep the selection in view
//! and reach the first and the last rows; assistive technology still
//! hears the list's size and the selected row; and a row out of view
//! requests no icon. The ignored benchmark at the end measures
//! keystroke-to-frame latency in root search and the frame time of
//! scrolling, which the ticket's results comment records on Windows.
//! Settings' long lists draw only the rows near the page's view, as the
//! Shortcuts page does: the File Search page's is tested with its fixture
//! (`file_search_settings`), the Extensions group's sidebar entries and an
//! extension's Commands share the same `PageWindow`.

#[path = "support/a11y.rs"]
mod a11y;
#[path = "../../pane-core/tests/support/image_server.rs"]
mod image_server;
#[path = "support/settle.rs"]
mod settle;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, prelude::*, px};
use image_server::{ImageServer, png};
use pane::LauncherWindow;
use pane_core::system_icons::{SystemIcon, SystemIcons};
use pane_core::{CommandRegistration, Launcher, PackageIdentity, Runtime, Screen, Status};
use serde_json::json;
use settle::settle;
use tempfile::TempDir;

/// How many root commands the long list has.
const ROWS: usize = 10_000;

/// How many rows root search lists over them with a blank query: the
/// commands, then Pane's own "Settings…" row, always listed last (#72).
const LISTED: usize = ROWS + 1;

/// The title of the last row root search lists: Pane's "Settings…".
const LAST: &str = "Settings…";

/// The title of root command `index`: zero-padded, so root search's order
/// is the commands' order.
fn title(index: usize) -> String {
    format!("Command {index:05}")
}

/// `count` root commands titled by `title`. Their component is never run:
/// root search only lists them.
fn commands(count: usize, title: impl Fn(usize) -> String) -> Vec<CommandRegistration> {
    (0..count)
        .map(|index| CommandRegistration {
            id: format!("command-{index}"),
            title: title(index),
            subtitle: Some("A command root search lists".into()),
            component: PathBuf::from("never-run.wasm"),
            takes_query: false,
            search: false,
            when: pane_core::CommandWhen::Always,
            matches: pane_core::CommandMatches::Title,
        })
        .collect()
}

/// The launcher window over `commands`, at the launcher's own size, on
/// root search with a blank query: every command listed.
fn open(
    cx: &mut TestAppContext,
    commands: Vec<CommandRegistration>,
) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher = Launcher::new(Runtime::start(), commands);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_resize(pane::launcher_client_size());
    settle(&window, cx);
    redraw(&window, cx);
    (window, cx)
}

/// Draws the window again, as a change outside it would.
fn redraw(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    for _ in 0..2 {
        window.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }
}

fn selector(name: &str) -> &'static str {
    name.to_owned().leak()
}

/// Whether the row titled `title` was drawn wholly inside the list.
fn row_is_visible(cx: &mut VisualTestContext, title: &str) -> bool {
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    let Some(row) = cx.debug_bounds(selector(&format!("row-{title}"))) else {
        return false;
    };
    row.top() >= list.top() && row.bottom() <= list.bottom()
}

/// The rows the last frame drew, by index.
fn drawn_rows(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Vec<usize> {
    cx.read_entity(window, |window, _| window.drawn_rows())
}

/// The selected row's index.
fn selected(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Option<usize> {
    settle(window, cx).selected
}

/// At most how many rows a list `height` high shows, part-shown ones
/// included: a row is at least 44 high, with 2 between rows.
fn fitting(height: gpui::Pixels) -> usize {
    (f32::from(height) / 46.).ceil() as usize + 1
}

/// A list of ten thousand rows draws the rows in view and the few it lays
/// out ahead past its edges (three rows' height either side), not the
/// rest: the first row is drawn, the last is not, and no more rows are
/// drawn than fit in the list and its overscan.
#[gpui::test]
fn a_ten_thousand_row_list_draws_only_the_rows_in_view(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, commands(ROWS, title));
    let view = settle(&window, cx);
    assert_eq!(view.rows.len(), LISTED);
    assert_eq!(view.rows[LISTED - 1].title, LAST);
    assert_eq!(view.selected, Some(0));

    let list = cx.debug_bounds("rows").expect("the list is rendered");
    let drawn = drawn_rows(&window, cx);
    let most = fitting(list.size.height) + 3;
    assert!(
        !drawn.is_empty() && drawn.len() <= most,
        "{} rows drawn, at most {most} fit with the overscan: {drawn:?}",
        drawn.len()
    );
    assert_eq!(drawn.first(), Some(&0));
    assert!(row_is_visible(cx, &title(0)));
    assert!(
        cx.debug_bounds(selector(&format!("row-{}", title(ROWS - 1))))
            .is_none()
    );
    assert!(cx.debug_bounds(selector(&format!("row-{LAST}"))).is_none());
    assert!(!drawn.contains(&(ROWS - 1)));
    assert!(!drawn.contains(&(LISTED - 1)));
}

/// Up and Down move the selection a row and Page Down and Page Up a page
/// — the rows in view — each keeping the selected row in view; the first
/// and the last rows are reached, and the list never draws more than its
/// view and overscan on the way.
#[gpui::test]
fn the_keys_keep_the_selection_in_view_and_reach_both_ends(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, commands(ROWS, title));
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    let most = fitting(list.size.height) + 3;

    for _ in 0..20 {
        cx.simulate_keystrokes("down");
    }
    assert_eq!(selected(&window, cx), Some(20));
    redraw(&window, cx);
    assert!(
        row_is_visible(cx, &title(20)),
        "Down scrolls the row into view"
    );
    assert!(!row_is_visible(cx, &title(0)));

    cx.simulate_keystrokes("pagedown");
    let paged = selected(&window, cx).expect("a selection");
    assert!(paged > 21, "Page Down moves by the rows in view: {paged}");
    redraw(&window, cx);
    assert!(
        row_is_visible(cx, &title(paged)),
        "Page Down keeps it in view"
    );
    assert!(drawn_rows(&window, cx).len() <= most);

    cx.simulate_keystrokes("pageup");
    assert_eq!(selected(&window, cx), Some(20));
    redraw(&window, cx);
    assert!(row_is_visible(cx, &title(20)), "Page Up keeps it in view");

    // The last row: Page Down stops there.
    cx.read_entity(&window, |window, _| window.launcher().select(LISTED - 3));
    redraw(&window, cx);
    cx.simulate_keystrokes("pagedown");
    assert_eq!(selected(&window, cx), Some(LISTED - 1));
    redraw(&window, cx);
    assert!(row_is_visible(cx, LAST), "the last row is reached");
    assert!(row_is_visible(cx, &title(ROWS - 1)));
    assert!(drawn_rows(&window, cx).len() <= most);
    cx.simulate_keystrokes("down");
    assert_eq!(
        selected(&window, cx),
        Some(LISTED - 1),
        "and Down stays there"
    );

    // The first row: Page Up stops there.
    cx.read_entity(&window, |window, _| window.launcher().select(2));
    redraw(&window, cx);
    cx.simulate_keystrokes("pageup");
    assert_eq!(selected(&window, cx), Some(0));
    redraw(&window, cx);
    assert!(row_is_visible(cx, &title(0)), "the first row is reached");
    cx.simulate_keystrokes("up");
    assert_eq!(selected(&window, cx), Some(0), "and Up stays there");
}

/// Only the rows in view are drawn, so each says where it is in the whole
/// list and how long the list is; the search field keeps the focus, and
/// the window's announcer says the selected row with its place (#132).
#[gpui::test]
fn assistive_technology_still_hears_the_lists_size_and_the_selected_row(cx: &mut TestAppContext) {
    let (window, cx) = open(cx, commands(ROWS, title));
    for _ in 0..30 {
        cx.simulate_keystrokes("down");
    }
    assert_eq!(selected(&window, cx), Some(30));
    redraw(&window, cx);

    cx.update(|window, _| window.set_a11y_forced(true));
    cx.run_until_parked();
    let json = cx
        .update(|window, _| window.debug_a11y_tree_json())
        .expect("an accessibility tree");
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes = tree["nodes"].as_object().unwrap();
    let options: Vec<&serde_json::Value> = nodes
        .values()
        .filter(|node| node["aria"]["size_of_set"].is_number())
        .collect();
    assert!(!options.is_empty(), "the drawn rows say the list's size");
    assert!(options.len() < 30, "{} options drawn", options.len());
    for option in &options {
        assert_eq!(option["aria"]["size_of_set"], LISTED, "{option}");
    }
    // The search field keeps the focus (#132): no row claims it, and the
    // selected row says where it is in the whole list.
    assert!(tree["active_descendant_focus"].is_null(), "{tree}");
    let focused = tree["gpui_focus"].as_str().map(|id| &nodes[id]);
    let focused = focused.expect("a focused node");
    assert_eq!(focused["aria"]["label"], "Search");
    let chosen = options
        .iter()
        .find(|option| option["aria"]["selected"] == true)
        .expect("the selected row is drawn");
    assert_eq!(chosen["aria"]["label"], title(30));
    assert_eq!(chosen["aria"]["position_in_set"], 31);
    assert_eq!(chosen["aria"]["size_of_set"], LISTED);
    // The announcer says it, with its place in the whole list.
    let (said, value) = a11y::announcer_of(&json);
    assert_eq!(said, value);
    assert!(
        said.ends_with(&format!("{}, 31 of {LISTED}", title(30))),
        "{said}"
    );
}

/// The host's icon extraction, stood in for: a small PNG for any path that
/// exists, counting the paths it was asked for.
#[derive(Default)]
struct FakeIcons {
    asked: AtomicUsize,
}

impl SystemIcons for FakeIcons {
    fn icon(&self, path: &Path) -> Result<SystemIcon, String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        if path.exists() {
            Ok(SystemIcon::Png(png(8, [48, 164, 108, 255])))
        } else {
            Err(format!("{} does not exist", path.display()))
        }
    }
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

/// A list's rows out of view request no icon: in a short window, the
/// icons sample's list (#139) draws its first rows only, and lays out
/// the few past its view's edge; its file icon's row, near the end, has
/// its system icon extracted only once the selection brings it into
/// view, and the favicon's row, past the view and its overscan too,
/// downloads nothing until then.
#[gpui::test]
fn rows_out_of_view_request_no_icons(cx: &mut TestAppContext) {
    let server = ImageServer::start();
    server.release();
    let (sources, data): (TempDir, TempDir) =
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = sources.path().join("sample-icons");
    let assembled =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/sample-icons");
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    copy_folder(&assembled, &folder);
    let notes = sources.path().join("notes.txt");
    fs::write(&notes, "notes").unwrap();
    let key = PackageIdentity::local(&folder).unwrap().key();
    let packages = data.path().join("extensions");
    fs::create_dir_all(&packages).unwrap();
    let settings = json!({
        "version": 1,
        "packages": { key: {
            "imageServer": server.url(),
            "iconFile": notes.to_string_lossy(),
            "iconApplication": sources.path().join("gone.exe").to_string_lossy(),
        } }
    });
    fs::write(packages.join("settings.json"), settings.to_string()).unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let icons = Arc::new(FakeIcons::default());
    let launcher = Launcher::with_packages(Runtime::start(), vec![], packages)
        .with_system_icons(icons.clone());
    cx.foreground_executor()
        .block_on(launcher.install_package(&folder));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Icons sample".into())
    );
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
    let (window, cx) =
        cx.add_window_view(|window, cx| LauncherWindow::new(launcher.clone(), window, cx));
    // A command's list about 170 px high: room for three or four rows,
    // which with the three rows' overscan the list lays out ahead of its
    // view are still short of the favicon's row, the ninth.
    cx.simulate_resize(gpui::size(px(640.), px(220.)));
    settle(&window, cx);

    cx.simulate_input("icons");
    let view = settle(&window, cx);
    let at = view
        .rows
        .iter()
        .position(|row| row.title == "Icons")
        .unwrap_or_else(|| panic!("no Icons row in {:?}", view.rows));
    cx.read_entity(&window, |window, _| window.launcher().select(at));
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.screen, Screen::Command, "{:?}", view.status);
    redraw(&window, cx);
    let file = view
        .rows
        .iter()
        .position(|row| row.title == "File icon")
        .expect("the file icon's row");
    let favicon = view
        .rows
        .iter()
        .position(|row| row.title == "Favicon")
        .expect("the favicon's row");
    let drawn = drawn_rows(&window, cx);
    assert!(
        !drawn.contains(&file) && !drawn.contains(&favicon),
        "both rows are out of view: {drawn:?}"
    );
    assert!(launcher.wait_for_icons(Duration::from_secs(30)));
    assert_eq!(
        icons.asked.load(Ordering::SeqCst),
        0,
        "no system icon asked"
    );
    assert_eq!(server.count("/favicon.ico"), 0, "{:?}", server.requests());

    // Selected, the file icon's row comes into view and its icon loads.
    cx.read_entity(&window, |window, _| window.launcher().select(file));
    redraw(&window, cx);
    assert!(drawn_rows(&window, cx).contains(&file));
    assert!(launcher.wait_for_icons(Duration::from_secs(30)));
    assert!(icons.asked.load(Ordering::SeqCst) >= 1);
    // The favicon's row is now in the overscan above the file's row, which
    // the list lays out ahead: its download may have started then, once.
    assert!(server.count("/favicon.ico") <= 1, "{:?}", server.requests());
}

/// A launcher first laid out shorter than its pinned home — as the window
/// is while the system sizes it at start — shows the home once it grows,
/// over a list longer than the window: the first row's reveal shows what
/// is above it, so no offset left from the short layout hides the
/// "Pinned" label, the strip or the rows' label.
#[gpui::test]
fn the_pinned_home_shows_when_the_launcher_grows_after_a_short_layout(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let pins: Vec<_> = ["one", "two", "three", "four", "five", "six", "seven"]
        .iter()
        .map(|id| json!({ "command": id }))
        .collect();
    let record = json!({ "version": 2, "pins": pins });
    fs::write(data.path().join("quick-slots.json"), record.to_string()).unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::new(Runtime::start(), commands(40, title)).with_quick_slots(data.path());
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    cx.simulate_resize(gpui::size(px(640.), px(140.)));
    settle(&window, cx);
    redraw(&window, cx);
    cx.simulate_resize(pane::launcher_client_size());
    settle(&window, cx);
    redraw(&window, cx);

    assert_eq!(selected(&window, cx), Some(0));
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    let label = cx
        .debug_bounds("section-Pinned")
        .expect("the home's label is drawn");
    assert!(
        label.top() >= list.top(),
        "the label shows: {label:?} in {list:?}"
    );
    let commands = cx
        .debug_bounds("section-Commands")
        .expect("the rows' label is drawn");
    assert!(
        commands.bottom() <= list.bottom(),
        "{commands:?} in {list:?}"
    );
    assert!(row_is_visible(cx, &title(0)));
}

/// The `p`th percentile of `samples` (sorted in place).
fn percentile(samples: &mut [Duration], p: f64) -> Duration {
    samples.sort();
    let at = ((samples.len() as f64 - 1.) * p).round() as usize;
    samples[at]
}

/// The benchmark the ticket records on Windows (#165): keystroke-to-frame
/// latency in root search over 1,000 applications and 10,000 indexed
/// results, and the frame time of scrolling a 10,000-row list. The
/// targets (proposed): a keystroke shows its results within 16 ms at the
/// 95th percentile, and scrolling holds 60 frames per second.
///
/// The results stand in as root commands — titled as applications and
/// files are — which root search ranks with the same matching an
/// application or an indexed file goes through. Each sample is the wall
/// time from the key (or the wheel) to the window's frame drawn on GPUI's
/// test platform: the launcher's work and the window's layout and paint,
/// without the GPU. Run it optimized, on Windows:
///
/// ```text
/// CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true cargo test -p pane --release \
///   --test integration virtual_lists::benchmark -- --ignored --nocapture
/// ```
///
/// (The window tests read what a frame drew through debug-build support,
/// hence the debug assertions.)
#[gpui::test]
#[ignore = "a benchmark: run it optimized, on Windows, with --ignored --nocapture"]
fn benchmark(cx: &mut TestAppContext) {
    const APPLICATIONS: usize = 1_000;
    const FILES: usize = 10_000;
    let words = [
        "Studio", "Notes", "Player", "Editor", "Viewer", "Manager", "Tools", "Code",
    ];
    let mut listed = commands(APPLICATIONS, |index| {
        format!("{} {index}", words[index % words.len()])
    });
    listed.extend(commands(FILES, |index| {
        let kind = ["pdf", "docx", "png", "txt", "xlsx", "md"][index % 6];
        format!("report {index} draft.{kind}")
    }));
    let (window, cx) = open(cx, listed);
    // The commands, then Pane's own "Settings…".
    assert_eq!(settle(&window, cx).rows.len(), APPLICATIONS + FILES + 1);

    // Keystrokes: a query typed and erased, again and again, each key's
    // frame timed.
    let mut keys = Vec::new();
    for round in 0..20 {
        let query = ["rep", "stu", "dra", "not", "e", "co"][round % 6];
        for character in query.chars() {
            keys.push(character.to_string());
        }
        for _ in query.chars() {
            keys.push("backspace".to_owned());
        }
    }
    let mut keystrokes = Vec::with_capacity(keys.len());
    for key in &keys {
        let started = Instant::now();
        if key == "backspace" {
            cx.simulate_keystrokes("backspace");
        } else {
            cx.simulate_input(key);
        }
        cx.run_until_parked();
        keystrokes.push(started.elapsed());
    }
    assert!(matches!(
        settle(&window, cx).screen,
        Screen::Root { ref query } if query.is_empty()
    ));

    // Scrolling: the wheel over the whole list, each frame timed.
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    let mut frames = Vec::new();
    for step in 0..400 {
        let delta = if step < 300 { -120. } else { 120. };
        let started = Instant::now();
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: list.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(delta))),
            modifiers: Modifiers::none(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        cx.run_until_parked();
        frames.push(started.elapsed());
    }

    let typed = percentile(&mut keystrokes, 0.95);
    let scrolled = percentile(&mut frames, 0.95);
    let fps = 1. / scrolled.as_secs_f64().max(f64::EPSILON);
    println!(
        "keystroke-to-frame over {} results: p50 {:?}, p95 {:?}, max {:?} (target p95 <= 16 ms: {})",
        APPLICATIONS + FILES,
        percentile(&mut keystrokes, 0.5),
        typed,
        keystrokes.last().unwrap(),
        if typed <= Duration::from_millis(16) {
            "met"
        } else {
            "missed"
        },
    );
    println!(
        "scroll frame over {} rows: p50 {:?}, p95 {:?}, max {:?} — {fps:.0} frames per second at p95 (target 60: {})",
        APPLICATIONS + FILES,
        percentile(&mut frames, 0.5),
        scrolled,
        frames.last().unwrap(),
        if fps >= 60. { "met" } else { "missed" },
    );
}
