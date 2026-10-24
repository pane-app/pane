//! Restart to Update Pane in the native launcher window (ADR 0050), driven
//! on GPUI's test platform: with an update staged, a blank root search
//! draws it first of all, under its own "Update" label and above the
//! pinned home, selected; and the compact window shows it under the search
//! field, above the pins.
//!
//! The update is staged by hand, as a download leaves it (the program in
//! the install folder's `update` folder, its marker written), so nothing
//! is checked or downloaded here: `pane-core`'s own tests cover that.

use std::path::{Path, PathBuf};

use gpui::{Entity, TestAppContext, VisualTestContext, px};
use pane::LauncherWindow;
use pane_core::{
    CommandMatches, CommandRegistration, CommandWhen, Launcher, ReleaseSource, Runtime,
};

#[path = "support/settle.rs"]
mod settle;

use settle::settle;

/// The search field's height, the reference's 64px header.
const BAR: f32 = 64.;

/// The pins' row under it.
const PINS: f32 = 48.;

/// The update's row under it: a result row, 44px, and 6px above and below.
const UPDATE: f32 = 56.;

/// The row's title.
const RESTART: &str = "Restart to Update Pane";

/// The launcher window over a launcher running Pane 0.1.0 from
/// `install`'s program, with Pane 99.0.0 staged beside it, over the Rust
/// sample as Alpha, and a data folder whose settings record holds
/// `settings` and whose quick slots' record pins Alpha.
fn staged<'a>(
    cx: &'a mut TestAppContext,
    install: &Path,
    data: &Path,
    settings: serde_json::Value,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    std::fs::write(install.join("pane"), b"the 0.1.0 program").unwrap();
    std::fs::create_dir_all(install.join("update")).unwrap();
    std::fs::write(install.join("update/pane"), b"the 99.0.0 program").unwrap();
    std::fs::write(
        install.join("update/.pane-staged.json"),
        r#"{ "version": "99.0.0", "sha256": "unchecked here" }"#,
    )
    .unwrap();
    std::fs::write(data.join("settings.json"), settings.to_string()).unwrap();
    let record = serde_json::json!({ "version": 2, "pins": [{ "command": "sample_rust" }] });
    std::fs::write(data.join("quick-slots.json"), record.to_string()).unwrap();
    cx.update(|cx| {
        pane::settings::init_with_overrides(
            Some(data.to_owned()),
            pane::settings::Overrides::default(),
            cx,
        )
    });
    // A release source nothing is asked of: no check runs in these tests.
    let component =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/sample_rust.wasm");
    assert!(
        component.exists(),
        "{} is missing; run `cargo xtask guests`",
        component.display()
    );
    let alpha = CommandRegistration {
        id: "sample_rust".into(),
        title: "Alpha".into(),
        subtitle: None,
        component,
        takes_query: false,
        keywords: Vec::new(),
        when: CommandWhen::Always,
        matches: CommandMatches::Title,
    };
    let launcher = Launcher::new(Runtime::start(), vec![alpha])
        .with_quick_slots(data)
        .with_application_update(
            "0.1.0",
            ReleaseSource::local("http://127.0.0.1:9/").unwrap(),
            install.join("pane"),
        );
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    settle(&window, cx);
    (window, cx)
}

/// Reports the size the launcher asked for as the system's resize, and
/// waits for the window to draw at it.
fn applied(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) {
    let size = cx.update(|window, _| window.bounds().size);
    cx.simulate_resize(size);
    settle(window, cx);
}

#[gpui::test]
fn a_blank_query_draws_the_restart_first_above_the_pinned_home(cx: &mut TestAppContext) {
    let install = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = staged(
        cx,
        install.path(),
        data.path(),
        serde_json::json!({ "version": 1 }),
    );
    let view = settle(&window, cx);
    assert_eq!(view.rows[0].title, RESTART);
    assert_eq!(view.selected, Some(0), "selected, so Enter restarts");
    let label = cx.debug_bounds("section-Update").expect("its section");
    let row = cx
        .debug_bounds("row-Restart to Update Pane")
        .expect("the row");
    let home = cx.debug_bounds("pinned-strip").expect("the pinned home");
    let commands = cx
        .debug_bounds("section-Commands")
        .expect("the other rows' section");
    assert!(label.bottom() <= row.top(), "the label above the row");
    assert!(row.bottom() <= home.top(), "the row above the pinned home");
    assert!(home.bottom() <= commands.top(), "the rest below the home");
    let shown = cx.update(|_, app| window.read(app).launcher().present_row(0));
    assert!(shown.highlighted, "drawn with the accent");
}

#[gpui::test]
fn the_compact_window_shows_the_restart_under_the_search_field(cx: &mut TestAppContext) {
    let install = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let (window, cx) = staged(
        cx,
        install.path(),
        data.path(),
        serde_json::json!({ "version": 1, "windowMode": "compact", "compactPinned": true }),
    );
    assert_eq!(
        cx.update(|window, _| window.bounds().size.height),
        px(BAR + UPDATE + PINS),
        "the search field, the update's row and the pins' row"
    );
    applied(&window, cx);
    let row = cx
        .debug_bounds("row-Restart to Update Pane")
        .expect("the row, collapsed");
    let pins = cx.debug_bounds("compact-pins").expect("the pins' row");
    assert!(row.top() >= px(BAR), "under the search field");
    assert!(row.bottom() <= pins.top(), "above the pins");
    assert!(
        cx.debug_bounds("pinned-strip").is_none(),
        "the pinned home stays collapsed"
    );
    let view = settle(&window, cx);
    assert_eq!(view.selected, Some(0), "selected, so Enter restarts");
}
