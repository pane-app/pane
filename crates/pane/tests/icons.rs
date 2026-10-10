//! Icons, accessories and tooltips (#139) in the launcher's window, with
//! the Rust icons sample, its plain copy and a package whose command
//! names one of Pane's built-in glyphs as its icon installed, and the
//! launcher's clock at 2026-01-01T02:00:00Z: root search draws an
//! installed command's own icon, its package's, or a package's
//! first-letter tile, bare, where Pane's own rows keep their tiles — and
//! a built-in glyph a command names on Pane's neutral command tile, at
//! the row's, a pinned slot's and the Actions panel header's sizes
//! (ADR 0035, #247); an open command's rows draw their icons by theme (a
//! packaged image's `@light` or `@dark` variant, a light and dark pair),
//! tinted, masked and failing to their fallback, and up to three
//! accessories, a relative date advancing as the clock does; the Actions
//! panel draws an action's icon in place of Pane's glyph; hovering a
//! title, a subtitle or an accessory shows its tooltip; assistive
//! technology reads the accessories with the row and skips icons without
//! a tooltip. The core's rules and the other languages are `pane-core`'s
//! `icons.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{Entity, Modifiers, Pixels, TestAppContext, VisualTestContext, prelude::*, px};
use pane::LauncherWindow;
use pane_core::clipboard::ManualClock;
use pane_core::{Launcher, LauncherView, ResultAction, Runtime, Screen, SlotChange, Status};
use tempfile::TempDir;

#[path = "support/packages.rs"]
mod packages;
#[path = "support/settle.rs"]
mod settle;

use settle::{settle, settle_shown};

/// 2026-01-01T00:00:00Z, the "Packaged image" row's date.
const NEW_YEAR: u64 = 1_767_225_600_000;

/// Open actions' default binding on this system.
const OPEN_ACTIONS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// How long the pointer rests before a tooltip shows, with a margin.
const TOOLTIP_DELAY: Duration = Duration::from_millis(700);

/// A result row tile's side, a pinned slot's and the Actions panel
/// header's (the theme's tiles), for the neutral tile's own size.
const ROW_TILE: Pixels = px(28.);
const SLOT_TILE: Pixels = px(30.);
const MINI_TILE: Pixels = px(18.);

/// What a test keeps: its folders and the launcher's clock.
struct Fixture {
    _sources: TempDir,
    _data: TempDir,
    clock: Arc<ManualClock>,
}

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

/// The launcher window in the `theme` (`light` or `dark`), with the Rust
/// icons sample, its plain copy and a package whose command names a
/// built-in glyph installed, on root search.
fn window<'a>(
    cx: &'a mut TestAppContext,
    theme: &str,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext, Fixture) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            None,
            pane::settings::Overrides::parse(Some(theme), None),
            cx,
        )
    });
    let clock = ManualClock::at(NEW_YEAR + 2 * 3_600_000);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_clock(clock.clone())
            .with_quick_slots(data.path());
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    for (name, title) in [
        ("sample-icons", "Icons sample"),
        ("sample-icons-plain", "Plain icons sample"),
    ] {
        let folder = sources.path().join(name);
        copy_folder(&assembled(name), &folder);
        cx.foreground_executor()
            .block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {title}"))
        );
        while !matches!(launcher.view().screen, Screen::Root { .. }) {
            launcher.back();
        }
    }
    // The package whose command names one of Pane's built-in glyphs
    // (#247): its icon draws on Pane's neutral command tile.
    let glyph = packages::glyph_package(&sources.path().join("glyph"));
    cx.foreground_executor()
        .block_on(launcher.install_package(&glyph));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Star".into())
    );
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    settle(&window, cx);
    let fixture = Fixture {
        _sources: sources,
        _data: data,
        clock,
    };
    (window, cx, fixture)
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

/// The width and height of what `name` names, for a tile's own size.
fn drawn_size(cx: &mut VisualTestContext, name: impl Into<String>) -> (Pixels, Pixels) {
    let name = name.into();
    let bounds = cx
        .debug_bounds(selector(name.clone()))
        .unwrap_or_else(|| panic!("{name} is not drawn"));
    (bounds.size.width, bounds.size.height)
}

/// Every accessibility node's properties, as GPUI reports them to
/// assistive technology.
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

fn find<'a>(
    nodes: &'a [serde_json::Value],
    role: &str,
    label: &str,
) -> Option<&'a serde_json::Value> {
    nodes
        .iter()
        .find(|node| node["role"] == role && node["label"] == label)
}

/// Rests the pointer on what `name` names, long enough for its tooltip.
fn hover(cx: &mut VisualTestContext, name: impl Into<String>) {
    let name = name.into();
    let at = cx
        .debug_bounds(selector(name.clone()))
        .unwrap_or_else(|| panic!("{name} is not drawn"))
        .center();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.executor().advance_clock(TOOLTIP_DELAY);
    cx.run_until_parked();
}

fn view(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> LauncherView {
    cx.read_entity(window, |window, _| window.launcher().view())
}

/// Root search draws an installed command's own icon, its package's for a
/// command without one, and a first-letter tile for a package without
/// one, bare; a built-in glyph a command names draws on Pane's neutral
/// command tile; Pane's own rows keep their tiles (ADR 0035, #247).
#[gpui::test]
fn root_search_draws_the_extensions_icons_and_pane_keeps_its_tiles(cx: &mut TestAppContext) {
    let (window, cx, _fixture) = window(cx, "dark");
    cx.simulate_input("icons");
    settle(&window, cx);
    assert!(drawn(cx, "icon-Icons-image-command.svg"));
    assert!(drawn(cx, "icon-Icons (package icon)-image-icon.png"));
    assert!(drawn(cx, "icon-Plain icons-letter-P"));
    // An extension's image icon draws bare, and so does a package's
    // first-letter tile: no neutral command tile behind either.
    assert!(!drawn(cx, "icon-Icons-tile"), "an image icon draws bare");
    assert!(!drawn(cx, "icon-Icons (package icon)-tile"));
    assert!(
        !drawn(cx, "icon-Plain icons-tile"),
        "the letter tile is its own"
    );
    // A built-in glyph a command names: on Pane's neutral command tile.
    cx.simulate_keystrokes("backspace backspace backspace backspace backspace");
    cx.simulate_input("star");
    let view = settle(&window, cx);
    assert!(view.rows.iter().any(|row| row.title == "Star command"));
    assert!(drawn(cx, "row-Star command"));
    assert!(drawn(cx, "icon-Star command-glyph-star"));
    assert!(drawn(cx, "icon-Star command-tile"));
    assert_eq!(drawn_size(cx, "icon-Star command"), (ROW_TILE, ROW_TILE));
    // Pane's own row: its tile, no extension icon.
    for _ in 0.."star".len() {
        cx.simulate_keystrokes("backspace");
    }
    cx.simulate_input("settings");
    let view = settle(&window, cx);
    assert!(view.rows.iter().any(|row| row.title == "Settings…"));
    assert!(drawn(cx, "row-Settings…"));
    assert!(!drawn(cx, "icon-Settings…"));
}

/// A packaged image's `@dark` variant and a pair's dark file in the dark
/// theme, with the tint, mask and fallback each row asks for. A command's
/// list draws every icon bare, a named built-in glyph among them: the
/// neutral command tile is a command's own glyph's, drawn in root search,
/// a slot and the Actions panel's header (ADR 0035, #247).
#[gpui::test]
fn icons_draw_for_the_dark_theme_tinted_masked_and_failing_to_their_fallback(
    cx: &mut TestAppContext,
) {
    let (window, cx, _fixture) = window(cx, "dark");
    open_icons(&window, cx);
    assert!(drawn(cx, "icon-Built-in icon-glyph-star"));
    assert!(
        !drawn(cx, "icon-Built-in icon-tile"),
        "a list row's icon is bare"
    );
    assert!(drawn(cx, "icon-Packaged image-image-logo@dark.png"));
    assert!(!drawn(cx, "icon-Packaged image-image-logo@light.png"));
    assert!(drawn(cx, "icon-Light and dark pair-image-moon.svg"));
    assert!(
        !drawn(cx, "icon-Light and dark pair-tile"),
        "images stay bare"
    );
    // The raw colour reads on the dark panel as it is.
    assert!(drawn(cx, "icon-Tinted icon-glyph-heart"));
    assert!(!drawn(cx, "icon-Tinted icon-tile"));
    assert!(drawn(cx, "icon-Tinted icon-color-ff6363ff"));
    assert!(drawn(cx, "icon-Masked image-mask-circle"));
    assert!(drawn(cx, "icon-Masked image-image-photo.png"));
    assert!(
        !drawn(cx, "icon-Masked image-tile"),
        "a masked image stays bare"
    );
    // The image the package does not ship: its fallback, bare where the
    // image would have drawn.
    assert!(drawn(cx, "icon-Failing image-glyph-warning"));
    assert!(!drawn(cx, "icon-Failing image-tile"));
    assert!(drawn(cx, "icon-Avatar and progress-data"));
    assert!(drawn(cx, "icon-Avatar and progress-mask-circle"));
    assert!(!drawn(cx, "icon-Avatar and progress-tile"));
}

/// The same rows in the light theme: the `@light` variant and the pair's
/// light file, every icon bare.
#[gpui::test]
fn icons_draw_for_the_light_theme(cx: &mut TestAppContext) {
    let (window, cx, _fixture) = window(cx, "light");
    open_icons(&window, cx);
    assert!(drawn(cx, "icon-Packaged image-image-logo@light.png"));
    assert!(!drawn(cx, "icon-Packaged image-image-logo@dark.png"));
    assert!(drawn(cx, "icon-Light and dark pair-image-sun.svg"));
    assert!(drawn(cx, "icon-Tinted icon-glyph-heart"));
    assert!(!drawn(cx, "icon-Built-in icon-tile"));
}

/// A built-in glyph a command names draws on Pane's neutral command tile
/// at the row's, a pinned slot's and the Actions panel header's sizes —
/// each its own — while an image icon and a package's first-letter tile
/// draw bare wherever they are (ADR 0035, #247).
#[gpui::test]
fn a_named_glyph_draws_on_panes_neutral_tile_at_every_tile_size(cx: &mut TestAppContext) {
    let (window, cx, _fixture) = window(cx, "dark");
    cx.simulate_input("star");
    let view = settle(&window, cx);
    let at = view
        .rows
        .iter()
        .position(|row| row.title == "Star command")
        .expect("the row");
    let id = view.rows[at].id.clone();
    // The row: the glyph, on the tile, in the row tile's box.
    assert!(drawn(cx, "icon-Star command-glyph-star"));
    assert!(drawn(cx, "icon-Star command-tile"));
    assert_eq!(drawn_size(cx, "icon-Star command"), (ROW_TILE, ROW_TILE));

    // The Actions panel's header, at its own size.
    cx.read_entity(&window, |window, _| window.launcher().select(at));
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(drawn(cx, "icon-actions-header-glyph-star"));
    assert!(drawn(cx, "icon-actions-header-tile"));
    assert_eq!(
        drawn_size(cx, "icon-actions-header"),
        (MINI_TILE, MINI_TILE)
    );
    cx.simulate_keystrokes("escape");
    settle(&window, cx);

    // A pinned slot, at its own size.
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    let (change, recorded) = launcher.change_quick_slots(&id, ResultAction::Pin);
    assert!(matches!(change, SlotChange::Changed(_)), "{change:?}");
    cx.foreground_executor().block_on(recorded);
    for _ in 0.."star".len() {
        cx.simulate_keystrokes("backspace");
    }
    settle(&window, cx);
    assert!(drawn(cx, "icon-slot-1-glyph-star"));
    assert!(drawn(cx, "icon-slot-1-tile"));
    assert_eq!(drawn_size(cx, "icon-slot-1"), (SLOT_TILE, SLOT_TILE));
}

/// The Actions panel draws an action's own icon in place of Pane's glyph
/// (a web image's fallback until the image arrives), and Pane's glyph for
/// an action without one; the action chosen there tells the user what it
/// did in a toast.
#[gpui::test]
fn the_actions_panel_draws_the_actions_icons(cx: &mut TestAppContext) {
    let (window, cx, _fixture) = window(cx, "dark");
    open_icons(&window, cx);
    let at = view(&window, cx)
        .rows
        .iter()
        .position(|row| row.title == "Built-in icon")
        .expect("the row");
    cx.read_entity(&window, |window, _| window.launcher().select(at));
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(cx.read_entity(&window, |window, _| window.actions_open()));
    assert!(drawn(cx, "action-Run item"));
    assert!(!drawn(cx, "icon-action-Run item"), "Pane's glyph");
    assert!(drawn(cx, "icon-action-Copy Name-glyph-copy"));
    assert!(drawn(cx, "icon-action-Open Image-glyph-clock"));

    cx.simulate_input("copy");
    settle(&window, cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Chose Copy Name".into())
    );
}

/// Up to three accessories per row, a tag among them, and a relative date
/// that advances as the clock does while the list stays open.
#[gpui::test]
fn rows_draw_three_accessories_and_a_date_that_stays_current(cx: &mut TestAppContext) {
    let (window, cx, fixture) = window(cx, "dark");
    open_icons(&window, cx);
    assert!(drawn(cx, "accessory-Built-in icon-0-3"));
    assert!(drawn(cx, "accessory-Light and dark pair-0-Open"));
    assert!(drawn(cx, "accessory-Masked image-0-"));
    assert!(drawn(cx, "icon-Masked image-accessory-0-glyph-user"));
    assert!(drawn(cx, "accessory-Crowded row-2-3"));
    assert!(!drawn(cx, "accessory-Crowded row-3-4"));
    assert!(drawn(cx, "accessory-Packaged image-0-2h"));

    // An hour later, the list redraws by itself.
    fixture.clock.advance(Duration::from_secs(3600));
    cx.executor().advance_clock(Duration::from_secs(31));
    cx.run_until_parked();
    assert!(drawn(cx, "accessory-Packaged image-0-3h"));
    assert!(!drawn(cx, "accessory-Packaged image-0-2h"));
    assert_eq!(view(&window, cx).screen, Screen::Command);
}

/// Hovering a title, a subtitle or an accessory shows its tooltip; a
/// date's is its time in full.
#[gpui::test]
fn hovering_shows_the_tooltips(cx: &mut TestAppContext) {
    let (window, cx, _fixture) = window(cx, "dark");
    open_icons(&window, cx);
    hover(cx, "row-title-Built-in icon");
    assert!(drawn(
        cx,
        "tooltip-A built-in icon from the whole reicon set"
    ));
    hover(cx, "row-subtitle-Packaged image");
    assert!(drawn(
        cx,
        "tooltip-logo@light.png in the light theme, logo@dark.png in the dark"
    ));
    hover(cx, "accessory-Built-in icon-0-3");
    assert!(drawn(cx, "tooltip-Unread"));
    hover(cx, "accessory-Packaged image-0-2h");
    let offset = pane_core::clipboard_view::local_offset_ms(NEW_YEAR);
    let full = pane_core::absolute_date(NEW_YEAR as i64, offset);
    assert!(drawn(cx, format!("tooltip-{full}")), "{full}");
    // An icon with a tooltip shows it too.
    hover(cx, "icon-Failing image");
    assert!(drawn(cx, "tooltip-Image missing"));
    assert_eq!(view(&window, cx).screen, Screen::Command);
}

/// Assistive technology reads a row's accessories with it, and skips an
/// icon unless it has a tooltip.
#[gpui::test]
fn accessories_are_read_with_the_row_and_decorative_icons_are_skipped(cx: &mut TestAppContext) {
    let (window, cx, _fixture) = window(cx, "dark");
    open_icons(&window, cx);
    let nodes = accessible_nodes(cx);
    let row = find(&nodes, "ListBoxOption", "Built-in icon").expect("the row");
    let description = row["description"].as_str().unwrap_or_default();
    assert_eq!(description, "reicon's star, by name. 3, Unread");
    let tagged = find(&nodes, "ListBoxOption", "Light and dark pair").expect("the row");
    assert!(
        tagged["description"]
            .as_str()
            .unwrap_or_default()
            .ends_with(". Open"),
        "{tagged}"
    );
    // Icons with tooltips are images assistive technology reads.
    assert!(
        find(&nodes, "Image", "Image missing").is_some(),
        "{nodes:#?}"
    );
    assert!(find(&nodes, "Image", "Owner").is_some());
    assert!(find(&nodes, "Image", "Ada Lovelace").is_some());
    // The others are decoration, hidden: no image without a name.
    assert!(
        !nodes
            .iter()
            .any(|node| node["role"] == "Image"
                && node["label"].as_str().unwrap_or_default().is_empty()),
        "{nodes:#?}"
    );
    assert_eq!(view(&window, cx).screen, Screen::Command);
}
