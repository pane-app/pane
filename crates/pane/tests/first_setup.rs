//! First setup's choice of the official extensions (#283) in the native
//! window, on GPUI's test platform, with real key events: on a fresh data
//! folder Pane opens on the official extensions this system sets up, each
//! row drawn with its title, its description and its check mark, every
//! default ticked and the Set Up row selected, and an extension's own
//! icon drawn once its revision, fetched as the list opens, lands
//! (#466); Space and the check mark
//! untick one, the row's own Enter ticks it back, and Enter on Set Up
//! sets up what stays ticked, back at root search, the unticked one
//! declined. The defaults' repositories are made from the samples and
//! served from 127.0.0.1 (`pane-core`'s test support); nothing reaches the
//! network.

use std::fs;

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{Launcher, Runtime, Screen};

#[path = "../../pane-core/tests/support/repo_server.rs"]
mod repo_server;

#[path = "../../pane-core/tests/support/defaults.rs"]
mod defaults;

#[path = "support/settle.rs"]
mod settle;

#[path = "support/a11y.rs"]
mod a11y;

use settle::{settle, until};

/// The window over `launcher`, following what the launcher changes in the
/// background, with the keys bound, as the binary opens it.
fn open(
    cx: &mut TestAppContext,
    launcher: Launcher,
) -> (Entity<LauncherWindow>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (sender, changes) = pane_core::changes::channel();
    let launcher = launcher.with_changes(sender);
    cx.add_window_view(|window, cx| {
        let mut launcher_window = LauncherWindow::new(launcher, window, cx);
        launcher_window.follow_changes(changes, window, cx);
        launcher_window
    })
}

/// The titles of the rows on screen.
fn titles(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.read_entity(window, |window, _| window.launcher().view())
        .rows
        .into_iter()
        .map(|row| row.title)
        .collect()
}

/// Clicks the element whose debug selector is `name`, the pointer moving
/// onto it first, as its user's does.
fn click(cx: &mut VisualTestContext, name: &str) {
    let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
    let bounds = cx
        .debug_bounds(name)
        .unwrap_or_else(|| panic!("no {name} is drawn"));
    cx.simulate_mouse_move(
        bounds.center(),
        None::<gpui::MouseButton>,
        gpui::Modifiers::none(),
    );
    cx.simulate_click(bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

/// Whether the accessibility tree holds a node of `role` labelled
/// `label`.
fn has_node(cx: &mut VisualTestContext, role: &str, label: &str) -> bool {
    let (_, json) = a11y::accessibility(cx);
    let tree: serde_json::Value = serde_json::from_str(&json).unwrap();
    tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .any(|node| node["aria"]["role"] == role && node["aria"]["label"] == label)
}

#[gpui::test]
fn first_setup_lists_the_official_extensions_ticked_and_sets_up_what_stays_ticked(
    cx: &mut TestAppContext,
) {
    let server = repo_server::Server::start();
    let repos = tempfile::tempdir().unwrap();
    let pins = vec![
        defaults::from_sample(
            &server,
            repos.path(),
            "sample-icons",
            "Icons sample",
            "sample-icons",
        ),
        defaults::from_sample(
            &server,
            repos.path(),
            "sample-greet",
            "Greet sample",
            "sample-greet",
        ),
    ];
    let data = tempfile::tempdir().unwrap();
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_defaults(pins);
    let (window, cx) = open(cx, launcher);
    // Pane's own window size.
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));
    settle(&window, cx);

    // As Pane starts, first setup asks: the window draws the choice.
    let launcher = cx.read_entity(&window, |window, _| window.launcher().clone());
    futures::executor::block_on(launcher.first_setup());
    let view = until(&window, cx, |view| matches!(view.screen, Screen::Choice(_)));
    assert_eq!(view.title, "Pane's Official Extensions");
    assert_eq!(
        titles(&window, cx),
        ["Icons sample", "Greet sample", "Set Up"]
    );
    let Screen::Choice(choice) = &view.screen else {
        unreachable!("checked above");
    };
    assert_eq!(choice.ticked, [true, true]);
    // Each row is drawn with its title, its description and its check
    // mark, ticked; the lines under the title say where they come from and
    // that what is left out stays available.
    for title in ["Icons sample", "Greet sample"] {
        assert!(
            cx.debug_bounds(format!("row-{title}").leak()).is_some(),
            "the row of {title} is drawn"
        );
        assert!(
            cx.debug_bounds(format!("choice-tick-{title}").leak())
                .is_some(),
            "the check mark of {title} is drawn"
        );
        assert!(
            has_node(cx, "CheckBox", &format!("Untick {title}")),
            "{title} is ticked"
        );
    }
    let (_, json) = a11y::accessibility(cx);
    assert!(json.contains("What the Icons sample does"), "{json}");
    assert!(json.contains("What the Greet sample does"), "{json}");
    for line in [
        "Pane sets these up from their own repositories, at the releases this version of Pane \
         was tested with",
        "Untick what you do not want: you can install it later from the Extensions group in \
         Settings",
    ] {
        assert!(
            cx.debug_bounds(format!("detail-{line}").leak()).is_some(),
            "{line:?} is drawn"
        );
    }
    // The Set Up row is selected and drawn: Enter would set both up.
    assert_eq!(view.selected, Some(2));
    assert!(cx.debug_bounds("row-Set Up").is_some());
    assert_eq!(
        view.rows[2].subtitle.as_deref(),
        Some("Set up the 2 ticked extensions")
    );
    // Each pinned revision is fetched as the list opens (#466): the window
    // draws the icon the icons sample's own manifest names once it lands,
    // in place, the selection kept.
    let view = until(&window, cx, |view| match &view.screen {
        Screen::Choice(choice) => matches!(
            choice.extensions[0].icon.source,
            pane_core::IconSource::Image { .. }
        ),
        _ => false,
    });
    assert_eq!(view.selected, Some(2));
    assert!(cx.debug_bounds("row-Icons sample").is_some());

    // Space unticks the extension selected; the row's own Enter ticks it
    // again, and the check mark unticks it once more.
    cx.simulate_keystrokes("up");
    cx.simulate_keystrokes("space");
    let view = settle(&window, cx);
    let Screen::Choice(choice) = &view.screen else {
        panic!("not the choice: {view:?}");
    };
    assert_eq!(choice.ticked, [true, false]);
    assert!(has_node(cx, "CheckBox", "Tick Greet sample"));
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    let Screen::Choice(choice) = &view.screen else {
        panic!("not the choice: {view:?}");
    };
    assert_eq!(choice.ticked, [true, true]);
    click(cx, "choice-tick-Greet sample");
    let view = settle(&window, cx);
    let Screen::Choice(choice) = &view.screen else {
        panic!("not the choice: {view:?}");
    };
    assert_eq!(choice.ticked, [true, false]);
    assert_eq!(
        view.rows[2].subtitle.as_deref(),
        Some("Set up the ticked extension")
    );

    // Enter on Set Up: back at root search, the ticked one set up in the
    // background and its command listed, the unticked one declined.
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    let view = until(&window, cx, |view| {
        matches!(view.screen, Screen::Root { .. })
            && view.rows.iter().any(|row| row.title == "Icons")
    });
    assert!(
        !view.rows.iter().any(|row| row.title.contains("Greet")),
        "{:?}",
        titles(&window, cx)
    );
    let titles: Vec<String> = launcher.packages().iter().map(|p| p.title()).collect();
    assert_eq!(titles, ["Icons sample"]);
    let registry: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(data.path().join("extensions").join("installed.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(registry["firstSetupChosen"], true);
    assert_eq!(
        registry["declinedDefaults"],
        serde_json::json!(["sample-greet"])
    );
}
