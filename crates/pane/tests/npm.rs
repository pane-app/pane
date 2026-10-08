//! Installing an extension from npm through the native window, on GPUI's
//! test platform, with real key events: the preview of the package named
//! (as Settings' install field names it, #168; the field itself is tested
//! in `settings.rs`) and Install, then its command running. The registry
//! is a local one on 127.0.0.1 (`pane-core`'s test support); nothing
//! reaches the network.

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

/// Previews the npm package `spec` names in the window, as Settings'
/// install field does once it is shown (#168).
fn preview(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    spec: &str,
) -> LauncherView {
    window.update_in(cx, |launcher, window, cx| {
        launcher.preview_npm(spec, window, cx)
    });
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

    let view = preview(&window, cx, "@pane-samples/greeter");
    assert!(matches!(view.screen, Screen::Package { .. }));
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
    let view = preview(&window, cx, "@pane-samples/greeter");
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

#[path = "support/setup.rs"]
mod setup;

/// Clicks the element whose debug selector is `name` in the window `cx`
/// drives, the pointer moving onto it first, as its user's does.
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

#[gpui::test]
fn check_for_update_on_its_settings_page_previews_it_from_npm(cx: &mut TestAppContext) {
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
    preview(&window, cx, "@pane-samples/greeter");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Installed Greeter from npm".into())
    );

    // Its page in Settings (#168): the menu's Check for Update shows the
    // package from npm again, on the page, offering its Update.
    cx.simulate_keystrokes(setup::settings_shortcut());
    cx.run_until_parked();
    let settings = cx
        .update(|_, cx| {
            cx.windows()
                .into_iter()
                .find_map(|window| window.downcast::<pane::SettingsWindow>())
        })
        .expect("Settings opened");
    let mut sc = VisualTestContext::from_window(gpui::AnyWindowHandle::from(settings), &cx.cx);
    sc.simulate_resize(gpui::size(gpui::px(760.), gpui::px(1200.)));
    sc.run_until_parked();
    click(&mut sc, "section-Extensions");
    click(&mut sc, "extension-entry-Greeter from npm");
    assert!(
        sc.debug_bounds("extension-page-source").is_some(),
        "its page shows where it comes from"
    );
    click(&mut sc, "extension-menu");
    click(&mut sc, "extension-menu-Check for Update");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while sc.debug_bounds("extension-row-Update").is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "the preview never offered its Update"
        );
        sc.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Package { .. }));
    assert_eq!(view.title, "Greeter from npm");
}
