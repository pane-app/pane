//! Installing an extension from npm through the native window, on GPUI's
//! test platform, with real key events: the "Install extension from npm…"
//! row, its form, the preview and Install, then its command running. The
//! registry is a local one on 127.0.0.1 (`pane-core`'s test support);
//! nothing reaches the network.

use std::path::PathBuf;

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::npm::Registry as NpmRegistry;
use pane_core::{Launcher, LauncherView, Runtime, Screen, Status};

#[path = "../../pane-core/tests/support/npm_registry.rs"]
mod npm_registry;

use npm_registry::{Registry, greeter_files, pack};

#[path = "support/settle.rs"]
mod settle;

use settle::{settle, settle_shown};

fn titles(view: &LauncherView) -> Vec<&str> {
    view.rows.iter().map(|row| row.title.as_str()).collect()
}

/// Selects the row titled `title` on the screen shown and presses Enter.
fn press_enter_on(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    title: &str,
) -> LauncherView {
    let launcher = cx.read_entity(window, |window, _| window.launcher().clone());
    let view = launcher.view();
    let index = titles(&view)
        .iter()
        .position(|row| *row == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(&view)));
    launcher.select(index);
    cx.simulate_keystrokes("enter");
    settle(window, cx)
}

#[gpui::test]
fn a_package_named_in_the_npm_form_is_previewed_installed_and_run(cx: &mut TestAppContext) {
    let guests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests");
    let registry = Registry::start();
    registry.publish(
        "@pane-samples/greeter",
        "0.1.0",
        pack(&greeter_files(&guests, "0.1.0")),
    );
    let data = tempfile::tempdir().unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_npm_registry(NpmRegistry::local(registry.url()).unwrap());
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    // Pane's own window size.
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));
    settle(&window, cx);

    let view = press_enter_on(&window, cx, "Install extension from npm…");
    assert_eq!(view.title, "Install extension from npm");
    assert!(matches!(view.screen, Screen::Form(_)));
    cx.simulate_input("@pane-samples/greeter");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(view.title, "Greeter from npm", "{view:#?}");
    assert_eq!(titles(&view), ["Install"]);
    assert!(
        view.details()
            .contains(&"npm version: 0.1.0, the latest".to_owned()),
        "{:#?}",
        view.details()
    );

    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Installed Greeter from npm".into())
    );
    assert_eq!(titles(&view)[0], "Greeter from npm");

    // Named again, it is offered as an Update, whose row stays in view below
    // the npm package's longer details.
    press_enter_on(&window, cx, "Install extension from npm…");
    cx.simulate_input("@pane-samples/greeter");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(titles(&view), ["Update"]);
    for _ in 0..2 {
        window.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    let row = cx
        .debug_bounds("row-Update")
        .expect("the Update row is rendered");
    assert!(
        row.top() >= list.top() && row.bottom() <= list.bottom(),
        "{row:?} not in {list:?}"
    );
    assert!(list.bottom() <= gpui::px(420.), "{list:?}");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Updated Greeter from npm to 0.1.0".into())
    );
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    press_enter_on(&window, cx, "Say hello");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Hello from the npm package".into())
    );
    assert!(cx.debug_bounds("toast-success").is_some());
}
