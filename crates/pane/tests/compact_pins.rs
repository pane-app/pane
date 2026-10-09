//! The compact window's pins, driven through the native launcher window on
//! GPUI's test platform: with the Launcher page's "Show pinned in Compact
//! mode" switch on, the collapsed launcher shows its pins as a row of icon
//! tiles under the search field, and the window is the field's height
//! plus the row's.

use std::path::{Path, PathBuf};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, px};
use pane::LauncherWindow;
use pane_core::{CommandMatches, CommandRegistration, CommandWhen, Launcher, Runtime, Screen};

#[path = "support/settle.rs"]
mod settle;

use settle::settle;

/// The search field's height, the reference's 64px header.
const BAR: f32 = 64.;

/// The pins' row under it.
const ROW: f32 = 48.;

/// A sample command registered as `title`, run by the guest `guest`, as the
/// window tests register theirs.
fn command(title: &str, guest: &str) -> CommandRegistration {
    let component = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(format!("{guest}.wasm"));
    assert!(
        component.exists(),
        "{} is missing; run `cargo xtask guests`",
        component.display()
    );
    CommandRegistration {
        id: guest.into(),
        title: title.into(),
        subtitle: None,
        component,
        takes_query: false,
        search: false,
        when: CommandWhen::Always,
        matches: CommandMatches::Title,
    }
}

/// The launcher window over Alpha, Bravo and Charlie (the Rust, JavaScript
/// and TypeScript samples, by the ids `sample_rust`, `sample_js` and
/// `sample_ts`), with a data folder whose settings record holds `settings`
/// and whose quick slots' record pins `pins` (command ids, in order).
fn compact<'a>(
    cx: &'a mut TestAppContext,
    data: &Path,
    settings: serde_json::Value,
    pins: &[&str],
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    std::fs::write(data.join("settings.json"), settings.to_string()).unwrap();
    let pins: Vec<serde_json::Value> = pins
        .iter()
        .map(|id| serde_json::json!({ "command": id }))
        .collect();
    let record = serde_json::json!({ "version": 2, "pins": pins });
    std::fs::write(data.join("quick-slots.json"), record.to_string()).unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    let launcher = Launcher::new(
        Runtime::start(),
        vec![
            command("Alpha", "sample_rust"),
            command("Bravo", "sample_js"),
            command("Charlie", "sample_ts"),
        ],
    )
    .with_quick_slots(data);
    // Guest replies arrive from the real runtime thread, outside the test
    // scheduler's deterministic control.
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    settle(&window, cx);
    (window, cx)
}

/// The height the launcher asked the system for. The test platform keeps
/// the window's drawn size until the system reports the resize, so the
/// asked-for size is applied with [`applied`] before the window is
/// measured.
fn asked_height(cx: &mut VisualTestContext) -> gpui::Pixels {
    cx.update(|window, _| window.bounds().size.height)
}

/// Reports the size the launcher asked for as the system's resize, as a
/// platform does once it resized the window, and waits for the window to
/// draw at it.
fn applied(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    let size = cx.update(|window, _| window.bounds().size);
    cx.simulate_resize(size);
    settle(window, cx);
}

/// Every accessibility node's properties, as GPUI reports them to assistive
/// technology.
fn accessible_nodes(cx: &mut VisualTestContext) -> Vec<serde_json::Value> {
    cx.update(|window, _| window.set_a11y_forced(true));
    cx.run_until_parked();
    let json = cx
        .update(|window, _| window.debug_a11y_tree_json())
        .expect("an accessibility tree");
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes = tree["nodes"].as_object().unwrap();
    nodes.values().map(|node| node["aria"].clone()).collect()
}

/// The node with this role and label.
fn node<'a>(nodes: &'a [serde_json::Value], role: &str, label: &str) -> &'a serde_json::Value {
    nodes
        .iter()
        .find(|node| node["role"] == role && node["label"] == label)
        .unwrap_or_else(|| panic!("no {role} labelled {label:?} in {nodes:#?}"))
}

/// With the switch on and two pins, the compact window is the search field
/// and a row of the two pins under it — no footer — and a click on a pin
/// opens what it holds, the window growing back to its expanded size.
#[gpui::test]
fn the_compact_window_shows_the_pins_under_the_search_field(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = compact(
        cx,
        data.path(),
        serde_json::json!({ "version": 1, "windowMode": "compact", "compactPinned": true }),
        &["sample_rust", "sample_js"],
    );
    assert_eq!(
        asked_height(cx),
        px(BAR + ROW),
        "the search field and the pins' row"
    );
    applied(&window, cx);
    assert_eq!(asked_height(cx), px(BAR + ROW), "it stays fitted");

    let row = cx.debug_bounds("compact-pins").expect("the pins' row");
    assert_eq!(row.top(), px(BAR), "under the search field");
    assert_eq!(row.size.height, px(ROW));
    let first = cx.debug_bounds("compact-pin-1").expect("the first pin");
    let second = cx.debug_bounds("compact-pin-2").expect("the second pin");
    assert!(first.right() <= second.left(), "in the pins' order");
    assert!(
        cx.debug_bounds("compact-pin-3").is_none(),
        "one tile per pin"
    );
    assert!(
        cx.debug_bounds("pinned-strip").is_none(),
        "the pinned home stays collapsed"
    );
    assert!(
        cx.debug_bounds("status-idle").is_none(),
        "the footer stays hidden"
    );
    // Each pin is a named button with the chord that picks it.
    let nodes = accessible_nodes(cx);
    let alpha = node(&nodes, "Button", "Pinned 1: Alpha");
    let chord = if cfg!(target_os = "macos") {
        "Control+1"
    } else {
        "Ctrl+1"
    };
    assert_eq!(alpha["keyboard_shortcut"], chord, "{alpha:#}");
    node(&nodes, "Button", "Pinned 2: Bravo");

    // A click opens what the pin holds, and the window grows back.
    let center = first.center();
    cx.simulate_mouse_move(center, None::<gpui::MouseButton>, Modifiers::none());
    cx.simulate_click(center, Modifiers::none());
    let view = settle(&window, cx);
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "Rust sample")
    );
    assert!(
        asked_height(cx) > px(BAR + ROW),
        "the window is expanded again"
    );
}

/// With the switch off, the compact window is the search field alone,
/// whatever is pinned.
#[gpui::test]
fn without_the_switch_the_compact_window_is_the_search_field_alone(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = compact(
        cx,
        data.path(),
        serde_json::json!({ "version": 1, "windowMode": "compact" }),
        &["sample_rust", "sample_js"],
    );
    assert_eq!(asked_height(cx), px(BAR), "the search field alone");
    applied(&window, cx);
    assert!(cx.debug_bounds("compact-pins").is_none());
    assert!(cx.debug_bounds("compact-pin-1").is_none());
}

/// With the switch on and nothing pinned, the compact window is the search
/// field alone: an empty row is never shown.
#[gpui::test]
fn with_nothing_pinned_the_compact_window_is_the_search_field_alone(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = compact(
        cx,
        data.path(),
        serde_json::json!({ "version": 1, "windowMode": "compact", "compactPinned": true }),
        &[],
    );
    assert_eq!(asked_height(cx), px(BAR), "the search field alone");
    applied(&window, cx);
    assert!(cx.debug_bounds("compact-pins").is_none());
}
