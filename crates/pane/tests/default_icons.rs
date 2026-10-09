//! The default extensions' own tile icons (#163) in the launcher's window,
//! with Calculator, Applications, Files, Clipboard History and Quicklinks
//! installed from the packages `cargo xtask guests` assembles: each
//! command's row in root search draws its package's tile, or the tile of
//! its own, in the light and the dark theme, as the image it is (Pane adds
//! no tile behind it, ADR 0035), in the box of Pane's own command tiles,
//! so its title starts where a Pane row's does. The tiles are drawn at the
//! row tile's size, so GPUI's rasterization (twice an SVG's own size)
//! gives them the device pixels of a 2x display, and more than enough for
//! 1x and 1.5x. That a built-in glyph an extension names draws on Pane's
//! neutral command tile, and its other icons bare, is `icons.rs`'s.

use std::path::{Path, PathBuf};

use gpui::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext, prelude::*, px};
use pane::LauncherWindow;
use pane_core::{IconSource, Launcher, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/packages.rs"]
mod packages;
#[path = "support/settle.rs"]
mod settle;

use settle::settle;

/// A default package: its name, its title, and its rows' tiles (the row's
/// title, the image file it draws).
type DefaultPackage = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

/// Each default package, its title, and the tiles its commands' rows
/// draw: (the row's title, the image file it draws), for the commands that
/// have a row. Calculator and Applications answer root search; their
/// package icon is checked through the launcher.
const DEFAULTS: [DefaultPackage; 5] = [
    ("calculator", "Calculator", &[]),
    ("applications", "Applications", &[]),
    ("files", "Files", &[("Search Files", "search.svg")]),
    (
        "clipboard-history",
        "Clipboard History",
        &[("Clipboard History", "icon.svg")],
    ),
    (
        "quicklinks",
        "Quicklinks",
        &[
            ("Search Quicklinks", "search.svg"),
            ("Create Quicklink", "create.svg"),
            ("Import Quicklinks", "import.svg"),
            ("Export Quicklinks", "export.svg"),
        ],
    ),
];

/// A row tile's side in the reference (the theme's `tile`): 28.
const ROW_TILE: Pixels = px(28.);

/// Open actions' default binding on this system.
const OPEN_ACTIONS: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// The launcher window in `theme` (`light` or `dark`) with every default
/// extension installed from a copy of its assembled package, on root
/// search; the folders it keeps.
fn window<'a>(
    cx: &'a mut TestAppContext,
    theme: &str,
) -> (
    Entity<LauncherWindow>,
    &'a mut VisualTestContext,
    [TempDir; 2],
) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            None,
            pane::settings::Overrides::parse(Some(theme), None),
            cx,
        )
    });
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    for (package, title, _) in DEFAULTS {
        let folder = packages::assembled_package(package, &sources.path().join(package));
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
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    settle(&window, cx);
    (window, cx, [sources, data])
}

fn selector(name: impl Into<String>) -> &'static str {
    Box::leak(name.into().into_boxed_str())
}

fn bounds(cx: &mut VisualTestContext, name: impl Into<String>) -> Bounds<Pixels> {
    let name = name.into();
    cx.debug_bounds(selector(name.clone()))
        .unwrap_or_else(|| panic!("{name} is not drawn"))
}

fn drawn(cx: &mut VisualTestContext, name: impl Into<String>) -> bool {
    cx.debug_bounds(selector(name)).is_some()
}

/// Replaces the search field's text with `query` and waits for its rows.
fn search(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, query: &str) {
    let typed = cx.read_entity(window, |window, _| {
        let view = window.launcher().view();
        view.query().map_or(0, |typed| typed.chars().count())
    });
    if typed > 0 {
        cx.simulate_keystrokes(&vec!["backspace"; typed].join(" "));
    }
    cx.simulate_input(query);
    settle(window, cx);
}

/// The selected row's title.
fn selected_title(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Option<String> {
    cx.read_entity(window, |window, _| {
        let view = window.launcher().view();
        Some(view.rows.get(view.selected?)?.title.clone())
    })
}

/// Where the row titled `title` starts its title, from the row's left.
fn title_offset(cx: &mut VisualTestContext, title: &str) -> Pixels {
    bounds(cx, format!("row-title-{title}")).left() - bounds(cx, format!("row-{title}")).left()
}

/// Each default extension's commands draw their own tiles in root search,
/// as images in their own colours (no tint, no mask, nothing behind), in
/// the box of Pane's command tile and centred on the row as it is.
fn the_tiles_draw_in(theme: &str, cx: &mut TestAppContext) {
    let (window, cx, _folders) = window(cx, theme);
    // Pane's own row: its tile, and where its title starts.
    search(&window, cx, "settings");
    assert!(!drawn(cx, "icon-Settings…"), "Pane keeps its own tile");
    let pane_title = title_offset(cx, "Settings…");

    for (_, _, rows) in DEFAULTS {
        for (title, file) in rows {
            search(&window, cx, title);
            assert!(
                drawn(cx, format!("icon-{title}-image-{file}")),
                "{title} draws {file} in the {theme} theme"
            );
            assert!(
                !drawn(cx, format!("icon-{title}-tile")),
                "{title}'s tile is the image it ships: nothing of Pane's behind it"
            );
            assert!(
                !drawn(cx, format!("icon-{title}-mask-rounded")),
                "{title}'s tile is drawn as its file is, unclipped"
            );
            let icon = bounds(cx, format!("icon-{title}"));
            assert_eq!(
                (icon.size.width, icon.size.height),
                (ROW_TILE, ROW_TILE),
                "{title}'s tile has the row tile's size"
            );
            let row = bounds(cx, format!("row-{title}"));
            let middle = |bounds: Bounds<Pixels>| bounds.top() + bounds.size.height / 2.;
            assert!(
                (middle(icon) - middle(row)).abs() <= px(0.5),
                "{title}'s tile is centred on its row"
            );
            assert_eq!(
                title_offset(cx, title),
                pane_title,
                "{title}'s title starts where a Pane row's does"
            );
        }
    }

    // The packages' own icons, which their commands without one show:
    // their tiles, the same file in both themes.
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    assert_eq!(launcher.packages().len(), DEFAULTS.len());
    for package in launcher.packages() {
        let key = package.identity.key();
        let icon = launcher.icon_of(&key).expect("a package's icon");
        let IconSource::Image { light, dark } = &icon.source else {
            panic!("{key}'s icon is not its tile: {icon:?}");
        };
        assert_eq!(light.file_name().unwrap(), "icon.svg", "{key}");
        assert_eq!(light, dark, "{key}");
        assert!(light.starts_with(&package.location), "{light:?}");
    }
}

#[gpui::test]
fn the_default_extensions_draw_their_tiles_in_the_dark_theme(cx: &mut TestAppContext) {
    the_tiles_draw_in("dark", cx);
}

#[gpui::test]
fn the_default_extensions_draw_their_tiles_in_the_light_theme(cx: &mut TestAppContext) {
    the_tiles_draw_in("light", cx);
}

/// The Actions panel's header draws the selected row's own tile, as the
/// row does: each default extension command's, not Pane's command tile;
/// Pane's own row keeps its tile.
#[gpui::test]
fn the_actions_panel_names_a_default_command_with_its_tile(cx: &mut TestAppContext) {
    let (window, cx, _folders) = window(cx, "dark");
    for (_, _, rows) in DEFAULTS {
        for (title, file) in rows {
            search(&window, cx, title);
            cx.simulate_keystrokes(OPEN_ACTIONS);
            settle(&window, cx);
            assert!(drawn(cx, "actions-header"), "{title}'s actions are open");
            assert!(
                drawn(cx, format!("icon-actions-header-image-{file}")),
                "the header draws {title}'s {file}"
            );
            assert!(
                !drawn(cx, "icon-actions-header-tile"),
                "the header draws the tile {title} ships, bare as the row does"
            );
            cx.simulate_keystrokes("escape");
            settle(&window, cx);
        }
    }
    // Applications lists the system's own Settings app on Windows and
    // macOS ("System Settings"), which can rank above Pane's row.
    search(&window, cx, "settings");
    let rows = cx.read_entity(&window, |window, _| window.launcher().view().rows.len());
    for _ in 0..rows {
        if selected_title(&window, cx).as_deref() == Some("Settings…") {
            break;
        }
        cx.simulate_keystrokes("down");
        settle(&window, cx);
    }
    assert_eq!(selected_title(&window, cx).as_deref(), Some("Settings…"));
    cx.simulate_keystrokes(OPEN_ACTIONS);
    settle(&window, cx);
    assert!(drawn(cx, "actions-header"));
    assert!(
        !drawn(cx, "icon-actions-header"),
        "Pane's own row keeps its tile in the header"
    );
}

/// The repository's package folders: every default extension's tile is an
/// SVG of the row tile's own size (28 by 28, its viewBox too) with the
/// row tile's corner radius (7), so it lines up with Pane's tiles and GPUI
/// rasterizes it at twice that, a 2x display's device pixels.
#[test]
fn every_default_tile_has_the_row_tiles_size_and_radius() {
    let packages = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../guests/packages");
    let mut tiles = 0;
    for (package, _, _) in DEFAULTS {
        let folder = packages.join(package);
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "svg") {
                continue;
            }
            let svg = std::fs::read_to_string(&path).unwrap();
            let head = svg.split('>').next().unwrap();
            for attribute in [r#"width="28""#, r#"height="28""#, r#"viewBox="0 0 28 28""#] {
                assert!(head.contains(attribute), "{}: {attribute}", shown(&path));
            }
            assert!(
                svg.contains(r#"<rect width="28" height="28" rx="7""#),
                "{}: the tile's square",
                shown(&path)
            );
            tiles += 1;
        }
    }
    // One per package, and Files' and Quicklinks' commands' own.
    assert_eq!(tiles, 5 + 1 + 4);
}

fn shown(path: &Path) -> String {
    path.display().to_string()
}
