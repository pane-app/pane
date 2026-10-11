//! Installing an extension from a Git repository through the native window,
//! on GPUI's test platform, with real key events: the preview of a
//! source-only revision and of a release tag (as Settings' install field
//! names them, #168; the field itself is tested in `settings.rs`),
//! Install, then its command running — and, since #307, one extension of a
//! collection named by its id (`#clock`), with its page in Settings. The
//! repository is made by the test and served from 127.0.0.1 (`pane-core`'s
//! test support); nothing reaches the network.

use std::path::PathBuf;

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::{Launcher, LauncherView, Runtime, Screen, Status};

#[path = "../../pane-core/tests/support/repo_server.rs"]
mod repo_server;

use repo_server::{Repo, Server, collection_files, extension_collection_files, greeter_files};

#[path = "support/settle.rs"]
mod settle;

#[path = "support/setup.rs"]
mod setup;

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

/// Previews the Git repository `spec` names in the window, as Settings'
/// install field does once it is shown (#168).
fn preview(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    spec: &str,
) -> LauncherView {
    window.update_in(cx, |launcher, window, cx| {
        launcher.preview_git(spec, window, cx)
    });
    settle(window, cx)
}

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
fn a_repository_named_in_the_form_is_previewed_installed_and_run(cx: &mut TestAppContext) {
    let guests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests");
    let server = Server::start();
    let repos = tempfile::tempdir().unwrap();
    let repo = Repo::init(&repos.path().join("greeter"), server.home());
    repo.commit(&greeter_files(&guests, false), "Greeter 0.1.0 source");
    repo.git(&["switch", "--quiet", "-c", "release"]);
    repo.commit(&greeter_files(&guests, true), "Release 0.1.0");
    repo.tag("v0.1.0");
    repo.git(&["switch", "--quiet", "main"]);
    let url = server.serve("greeter", &repo);

    let data = tempfile::tempdir().unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    // Pane's own window size.
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));
    settle(&window, cx);

    // The default branch holds the source only: explained, nothing offered.
    let view = preview(&window, cx, &url);
    assert!(matches!(view.screen, Screen::Package { .. }));
    assert!(titles(&view).is_empty(), "{view:#?}");
    assert!(
        matches!(&view.status, Status::Error(text) if text.contains("holds only the source")),
        "{:?}",
        view.status
    );
    assert!(cx.debug_bounds("status-error").is_some());

    // Its release tag installs.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    let view = preview(&window, cx, &format!("{url}@v0.1.0"));
    assert_eq!(view.title, "Greeter from Git", "{view:#?}");
    assert_eq!(titles(&view), ["Install"]);
    assert!(
        view.details().contains(
            &"Revision: tag v0.1.0, which you named: installing pins it to that revision"
                .to_owned()
        ),
        "{:#?}",
        view.details()
    );
    // The Install row stays in view below the longer details.
    for _ in 0..2 {
        window.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }
    let list = cx.debug_bounds("rows").expect("the list is rendered");
    let row = cx
        .debug_bounds("row-Install")
        .expect("the Install row is rendered");
    assert!(
        row.top() >= list.top() && row.bottom() <= list.bottom(),
        "{row:?} not in {list:?}"
    );
    assert!(list.bottom() <= gpui::px(420.), "{list:?}");
    // And the Git lines above it are whole, not cut behind the list: the
    // last of them ends above the list's top.
    let last_git_line = "Pane builds nothing and runs no repository hooks, scripts or submodules";
    let line = cx
        .debug_bounds(format!("detail-{last_git_line}").leak())
        .expect("the preview's last Git line is rendered");
    assert!(line.bottom() <= list.top(), "{line:?} runs into {list:?}");
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Installed Greeter from Git".into())
    );
    assert_eq!(titles(&view)[0], "Greeter from Git");

    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    press_enter_on(&window, cx, "Say hello");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Hello from the Git repository".into())
    );
    assert!(cx.debug_bounds("toast-success").is_some());
}

#[gpui::test]
fn one_extension_of_a_collection_named_by_its_id_is_previewed_installed_and_run(
    cx: &mut TestAppContext,
) {
    let guests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests");
    let server = Server::start();
    let repos = tempfile::tempdir().unwrap();
    let index = r#"{ "extensions": [ { "id": "clock", "path": "extensions/clock" } ] }"#;
    let repo = Repo::init(&repos.path().join("tools"), server.home());
    repo.commit(
        &collection_files(&guests, index, false),
        "Clock 0.1.0 source",
    );
    repo.git(&["switch", "--quiet", "-c", "release"]);
    repo.commit(&collection_files(&guests, index, true), "Release 0.1.0");
    repo.tag("v0.1.0");
    repo.git(&["switch", "--quiet", "main"]);
    let url = server.serve("tools", &repo);

    let data = tempfile::tempdir().unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    // Pane's own window size.
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));
    settle(&window, cx);

    // The collection itself opens the choice of its extensions (#308):
    // listed, none ticked, the Install row after them.
    let view = preview(&window, cx, &url);
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    assert_eq!(view.title, "tools");
    assert_eq!(titles(&view), ["Clock from Git", "Install"]);
    assert!(cx.debug_bounds("choice-tick-Clock from Git").is_some());
    cx.simulate_keystrokes("escape");
    settle(&window, cx);

    // Its extension `clock`, named by its id, is previewed as any package
    // from Git is, with the extension named, and installs.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    let view = preview(&window, cx, &format!("{url}#clock@v0.1.0"));
    assert_eq!(view.title, "Clock from Git", "{view:#?}");
    assert_eq!(titles(&view), ["Install"]);
    assert!(
        view.details()
            .contains(&"Extension: clock, one of the extensions its collection lists".to_owned()),
        "{:#?}",
        view.details()
    );
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert_eq!(
        view.status,
        Status::Result("Installed Clock from Git".into())
    );
    assert_eq!(titles(&view)[0], "Clock from Git");

    // Its command runs.
    cx.simulate_keystrokes("enter");
    settle(&window, cx);
    press_enter_on(&window, cx, "Say hello");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Hello from the Git repository".into())
    );

    // Its page in Settings shows its source, the repository's address with
    // its id (#307).
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
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
    click(&mut sc, "extension-entry-Clock from Git");
    assert!(
        sc.debug_bounds("extension-page-source").is_some(),
        "its page shows where it comes from"
    );
}

/// The choice of a collection's extensions in the native window (#308):
/// the list — each row the extension's own icon, title, description and
/// version, its check mark at the right end, nothing ticked — ticked by
/// keyboard (Space) and by the pointer (the check mark), the ordinary
/// preview of the extension under the selection opened by Enter (whose
/// Back returns to the choice), and the ticked ones installed through
/// their own previews, each as its own package.
#[gpui::test]
fn a_collection_opens_the_choice_ticks_previews_and_installs_several(cx: &mut TestAppContext) {
    let guests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests");
    let server = Server::start();
    let repos = tempfile::tempdir().unwrap();
    let index = r#"{ "extensions": [
        { "id": "clock", "path": "extensions/clock" },
        { "id": "timers", "path": "extensions/timers" },
        { "id": "notes", "path": "extensions/notes" } ] }"#;
    let extensions = [("clock", true), ("timers", true), ("notes", true)];
    let repo = Repo::init(&repos.path().join("tools"), server.home());
    let source_only: Vec<(&'static str, bool)> =
        extensions.iter().map(|(id, _)| (*id, false)).collect();
    let mut source_files = extension_collection_files(&guests, &source_only);
    source_files.push(("pane-collection.json", index.as_bytes().to_vec()));
    repo.commit(&source_files, "Collection 0.1.0 source");
    repo.git(&["switch", "--quiet", "-c", "release"]);
    let mut release_files = extension_collection_files(&guests, &extensions);
    release_files.push(("pane-collection.json", index.as_bytes().to_vec()));
    repo.commit(&release_files, "Release 0.1.0");
    repo.tag("v0.1.0");
    repo.git(&["switch", "--quiet", "main"]);
    let url = server.serve("tools", &repo);

    let data = tempfile::tempdir().unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let (window, cx) = cx.add_window_view(|window, cx| LauncherWindow::new(launcher, window, cx));
    // Pane's own window size.
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(420.)));
    settle(&window, cx);

    // The collection, named without an id, opens the choice: the
    // extensions listed, each row what its own manifest says of it,
    // nothing ticked, the Install row after them.
    let view = preview(&window, cx, &format!("{url}@v0.1.0"));
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:#?}");
    assert_eq!(view.title, "tools");
    assert_eq!(
        titles(&view),
        [
            "Clock from Git",
            "Timers from Git",
            "Notes from Git",
            "Install"
        ]
    );
    let Screen::Choice(choice) = &view.screen else {
        unreachable!("checked above");
    };
    assert!(choice.ticked.iter().all(|ticked| !ticked), "{choice:?}");
    // The rows are drawn, with each extension's own icon, its description
    // and version, and its check mark at the right end.
    for title in ["Clock from Git", "Timers from Git", "Notes from Git"] {
        assert!(
            cx.debug_bounds(format!("row-{title}").leak()).is_some(),
            "the row of {title} is rendered"
        );
        assert!(
            cx.debug_bounds(format!("choice-tick-{title}").leak())
                .is_some(),
            "the check mark of {title} is rendered"
        );
    }
    assert!(cx.debug_bounds("row-Install").is_some());
    for line in [
        format!("Source: Git repository {}", &url["http://".len()..]),
        "Tick the extensions to install: each one installs on its own, with its own preview \
         and record"
            .into(),
    ] {
        assert!(
            cx.debug_bounds(format!("detail-{line}").leak()).is_some(),
            "{line:?} is rendered"
        );
    }

    // Choosing Install with nothing ticked is refused.
    press_enter_on(&window, cx, "Install");
    assert!(
        matches!(&settle(&window, cx).status, Status::Error(text)
            if text == "No extensions are ticked: tick the ones to install"),
        "nothing is ticked"
    );

    // Space ticks the extension selected; the arrows move the selection
    // as any list's.
    cx.simulate_keystrokes("space");
    let view = settle(&window, cx);
    let Screen::Choice(choice) = &view.screen else {
        unreachable!("checked above");
    };
    assert_eq!(choice.ticked, [true, false, false]);
    cx.simulate_keystrokes("down");
    // The pointer ticks another by its check mark alone.
    click(cx, "choice-tick-Timers from Git");
    let view = settle(&window, cx);
    let Screen::Choice(choice) = &view.screen else {
        unreachable!("checked above");
    };
    assert_eq!(choice.ticked, [true, true, false]);
    assert_eq!(view.selected, Some(1));
    assert_eq!(
        view.rows[3].subtitle.as_deref(),
        Some("Copy the 2 ticked extensions into Pane and add their commands")
    );

    // The per-extension preview: Enter opens the ordinary preview of the
    // extension under the selection, and Escape returns to the choice
    // with the ticks kept.
    let view = press_enter_on(&window, cx, "Timers from Git");
    assert!(matches!(view.screen, Screen::Package { .. }), "{view:#?}");
    assert_eq!(view.title, "Timers from Git");
    assert!(
        view.details()
            .contains(&"Extension: timers, one of the extensions its collection lists".to_owned()),
        "{:#?}",
        view.details()
    );
    assert_eq!(titles(&view), ["Install"]);
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    let Screen::Choice(choice) = &view.screen else {
        unreachable!("checked above");
    };
    assert_eq!(choice.ticked, [true, true, false]);

    // Backing out of the choice itself leaves for root search, as a
    // package preview's Back does.
    cx.simulate_keystrokes("escape");
    let view = settle(&window, cx);
    assert!(matches!(view.screen, Screen::Root { .. }), "{view:?}");

    // The install command's path opens the choice as the Settings flow
    // does: naming the collection again.
    let view = preview(&window, cx, &format!("{url}@v0.1.0"));
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    cx.simulate_keystrokes("space");
    click(cx, "choice-tick-Timers from Git");
    settle(&window, cx);

    // Choosing Install runs the ticked extensions through their own
    // previews, each as its own package: the ending lists what was
    // installed, and the last preview the run showed stays under the
    // title.
    let view = press_enter_on(&window, cx, "Install");
    assert!(
        matches!(&view.status, Status::Result(text)
            if text == "Installed Clock from Git and Timers from Git"),
        "{:?}",
        view.status
    );
    assert!(matches!(view.screen, Screen::Choice(_)), "{view:?}");
    let Screen::Choice(choice) = &view.screen else {
        unreachable!("checked above");
    };
    assert_eq!(
        choice.outcomes,
        [
            Some(pane_core::ChoiceOutcome::Installed),
            Some(pane_core::ChoiceOutcome::Installed),
            None
        ]
    );
    assert!(
        view.details()
            .contains(&"Commands: Timers from Git".to_owned()),
        "{:#?}",
        view.details()
    );

    // Their commands run, from root search.
    cx.simulate_keystrokes("escape");
    settle(&window, cx);
    press_enter_on(&window, cx, "Clock from Git");
    press_enter_on(&window, cx, "Say hello");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Hello from the Git repository".into())
    );
}
