//! Drives the launcher through its public interface against real guest
//! components built by `cargo xtask guests`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use std::future::Future;

use pane_core::{CallError, CommandRegistration, Launcher, Runtime, Screen, Status};

#[path = "support/platforms.rs"]
mod platforms;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guest;
use rows::titles;

fn command(id: &str, component: PathBuf) -> CommandRegistration {
    CommandRegistration {
        id: id.into(),
        title: format!("{id} command"),
        subtitle: None,
        component,
        takes_query: false,
        keywords: Vec::new(),
        when: pane_core::CommandWhen::Always,
        matches: pane_core::CommandMatches::Title,
    }
}

fn launcher(commands: Vec<CommandRegistration>) -> Launcher {
    Launcher::new(Runtime::start(), commands)
}

fn open_faulty_item(launcher: &Launcher, item: &str) {
    block_on(launcher.activate_selected());
    let index = titles(launcher)
        .iter()
        .position(|title| title == item)
        .unwrap();
    launcher.move_selection(index as isize);
}

/// The error shown: in the status line, or an action's failure toast.
fn error(launcher: &Launcher) -> String {
    match shown(launcher) {
        Status::Error(message) => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

#[test]
fn a_mixed_wasi_02_component_is_rejected_and_root_stays_usable() {
    let launcher = launcher(vec![
        command("mixed", guest("mixed_p2")),
        command("sample", guest("sample_rust")),
    ]);

    block_on(launcher.activate_selected());

    assert!(
        matches!(launcher.view().screen, Screen::Root { .. }),
        "{:?}",
        launcher.view().screen
    );
    let message = error(&launcher);
    assert!(message.contains("only WASI 0.3"), "{message}");
    assert!(message.contains("wasi:cli/stdout@0.2"), "{message}");

    launcher.move_selection(1);
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().title, "Rust sample");
}

#[test]
fn back_returns_from_a_command_to_root_search() {
    let launcher = launcher(vec![command("sample", guest("sample_rust"))]);
    block_on(launcher.activate_selected());

    launcher.back();

    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.status, Status::Idle);
    // Pane's own Settings row is listed whatever is installed, so it is
    // the command's row and it after going back.
    assert_eq!(titles(&launcher), ["sample command", "Settings…"]);
}

#[test]
fn a_guest_error_is_shown_as_an_error() {
    let launcher = launcher(vec![command("faulty", guest("faulty"))]);
    open_faulty_item(&launcher, "error");

    block_on(launcher.activate_selected());

    assert!(error(&launcher).contains("the guest refused"));
}

#[test]
fn a_guest_trap_is_shown_and_the_command_keeps_working() {
    let launcher = launcher(vec![command("faulty", guest("faulty"))]);
    open_faulty_item(&launcher, "trap");

    block_on(launcher.activate_selected());
    assert!(error(&launcher).contains("crashed"));

    launcher.move_selection(-2);
    block_on(launcher.activate_selected());
    assert_eq!(shown(&launcher), Status::Result("fine".into()));
}

#[test]
fn a_missing_component_explains_itself_on_root() {
    let launcher = launcher(vec![command("gone", PathBuf::from("does-not-exist.wasm"))]);

    block_on(launcher.activate_selected());

    assert!(
        matches!(launcher.view().screen, Screen::Root { .. }),
        "{:?}",
        launcher.view().screen
    );
    assert!(error(&launcher).contains("does-not-exist.wasm"));
}

#[test]
fn an_unavailable_runtime_leaves_root_navigable() {
    let unavailable = Err(CallError::RuntimeUnavailable("no engine".into()));
    let launcher = Launcher::new(unavailable, vec![command("sample", guest("sample_rust"))]);

    block_on(launcher.activate_selected());

    assert!(
        matches!(launcher.view().screen, Screen::Root { .. }),
        "{:?}",
        launcher.view().screen
    );
    assert_eq!(error(&launcher), "Extension runtime unavailable: no engine");
}

#[test]
fn with_no_commands_root_lists_only_settings_and_activation_does_nothing() {
    let launcher = launcher(vec![]);

    block_on(launcher.activate_selected());

    // No command is installed, but Pane's own Settings row is still
    // listed — it needs no extension — and activating it does nothing in
    // the launcher: the window opens the Settings window.
    let view = launcher.view();
    assert_eq!(titles(&launcher), ["Settings…".to_string()]);
    assert_eq!(view.selected, Some(0));
    assert_eq!(view.status, Status::Idle);
    assert!(launcher.selected_opens_settings());
}

#[test]
fn going_back_while_an_action_runs_discards_its_answer() {
    let launcher = launcher(vec![command("sample", guest("sample_rust"))]);
    block_on(launcher.activate_selected());
    launcher.move_selection(1);

    let pending = launcher.activate_selected();
    launcher.back();
    block_on(pending);

    let view = launcher.view();
    assert_eq!((view.query(), &view.status), (Some(""), &Status::Idle));
}

/// A launcher with an assembled package installed from
/// `target/guests/packages`, and the data folder it was installed into,
/// which must outlive it. Only an installed command can be launched, so
/// the designed views these tests open come from packages.
struct Installed {
    _data: tempfile::TempDir,
    launcher: Launcher,
}

/// A launcher with the Rust sample's package installed, at root search.
fn sample_installed(runtime: &Runtime) -> Installed {
    let data = tempfile::tempdir().unwrap();
    let launcher =
        Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"));
    let folder =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages/sample-rust");
    assert!(
        folder.exists(),
        "{} is missing; run `cargo xtask guests`",
        folder.display()
    );
    block_on(launcher.install_package(&folder));
    assert!(matches!(launcher.view().status, Status::Result(_)));
    Installed {
        _data: data,
        launcher,
    }
}

/// A launcher over `runtime` with the Rust sample's color command (a
/// designed view) opened.
fn sample_color_view(runtime: &Runtime) -> Installed {
    let installed = sample_installed(runtime);
    let launcher = &installed.launcher;
    open_color_command(launcher);
    assert!(
        matches!(launcher.view().screen, Screen::DesignedView(_)),
        "{:?}",
        launcher.view().screen
    );
    installed
}

/// Opens the sample's color command from root search.
fn open_color_command(launcher: &Launcher) {
    block_on(launcher.set_query("color picker"));
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.id.ends_with("#color"))
        .expect("the color command is listed");
    launcher.select(index);
    block_on(launcher.activate_selected());
}

/// A launcher over `runtime` with the faulty fixture's counting view
/// opened: its component installed as a package whose "counter" command
/// is the counting designed view.
fn faulty_view(runtime: &Runtime) -> Installed {
    let data = tempfile::tempdir().unwrap();
    let launcher =
        Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"));
    let component = guest("faulty");
    let source = data.path().join("faulty");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("pane.json"),
        r#"{"manifestVersion":1,"title":"Faulty","version":"0.1.0","apiVersion":"0.1",
            "commands":[{"id":"counter","title":"Faulty counter",
            "subtitle":"A canvas counting the events it handled",
            "component":"faulty.wasm","mode":"designed"}]}"#,
    )
    .unwrap();
    std::fs::copy(&component, source.join("faulty.wasm")).unwrap();
    block_on(launcher.install_package(&source));
    assert!(matches!(launcher.view().status, Status::Result(_)));
    block_on(launcher.set_query("counter"));
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.id.ends_with("#counter"))
        .expect("the counter command is listed");
    launcher.select(index);
    block_on(launcher.activate_selected());
    assert!(
        matches!(launcher.view().screen, Screen::DesignedView(_)),
        "{:?}",
        launcher.view().screen
    );
    assert_eq!(view_value(&launcher), "0 events");
    Installed {
        _data: data,
        launcher,
    }
}

/// The open designed view's value: what its canvas says for assistive
/// technology.
fn view_value(launcher: &Launcher) -> String {
    let view = launcher.view();
    let Screen::DesignedView(view) = &view.screen else {
        panic!("a designed view is open, not {:?}", view.screen)
    };
    fn value(node: &pane_core::Node) -> Option<String> {
        match &node.kind {
            pane_core::NodeKind::Canvas(canvas) => canvas.a11y.value.clone(),
            _ => node.children.iter().find_map(value),
        }
    }
    value(&view.tree.root).expect("the view draws a canvas")
}

/// The open designed view's canvas node: its key, its key handler, and the
/// render whose tree is drawn.
fn canvas_of(launcher: &Launcher) -> (String, Option<u32>, u64) {
    let view = launcher.view();
    let Screen::DesignedView(view) = &view.screen else {
        panic!("a designed view is open, not {:?}", view.screen)
    };
    fn of(node: &pane_core::Node) -> Option<(String, Option<u32>)> {
        match &node.kind {
            pane_core::NodeKind::Canvas(_) => {
                Some((node.key.clone().unwrap_or_default(), node.on_key))
            }
            _ => node.children.iter().find_map(of),
        }
    }
    let (key, on_key) = of(&view.tree.root).expect("the view draws a canvas");
    (key, on_key, view.render)
}

/// Sends a key event to the open designed view's canvas, pressed as `key`
/// spells it.
fn send_key(launcher: &Launcher, key: &str) {
    block_on(send_right_like(launcher, key));
}

/// Sends a key event to the open designed view's canvas, unawaited.
fn send_right_like(
    launcher: &Launcher,
    key: &str,
) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>> {
    let (canvas, on_key, render) = canvas_of(launcher);
    let callback = on_key.expect("the canvas takes keys");
    let payload = format!("{{\"key\":\"{key}\"}}");
    let pending = launcher.send_designed_seen(
        pane_core::DesignedHandler::Key,
        callback,
        (!canvas.is_empty()).then_some(canvas.as_str()),
        Some(render),
        payload,
    );
    Box::pin(pending)
}

#[test]
fn back_from_a_designed_view_closes_it_and_leaves_the_command() {
    let runtime = Runtime::start().unwrap();
    let installed = sample_color_view(&runtime);
    let launcher = &installed.launcher;
    assert_eq!(block_on(runtime.designed_view_count()), 1);

    launcher.back();

    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.status, Status::Idle);
    assert_eq!(block_on(runtime.designed_view_count()), 0);
}

#[test]
fn a_view_that_opens_after_the_user_left_is_closed_again() {
    let runtime = Runtime::start().unwrap();
    let installed = sample_installed(&runtime);
    let launcher = &installed.launcher;
    block_on(launcher.set_query("color picker"));
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.id.ends_with("#color"))
        .unwrap();
    launcher.select(index);

    let opening = launcher.activate_selected();
    assert!(matches!(launcher.view().status, Status::Running { .. }));
    launcher.back();
    block_on(opening);

    assert!(
        matches!(launcher.view().screen, Screen::Root { .. }),
        "{:?}",
        launcher.view().screen
    );
    assert_eq!(block_on(runtime.designed_view_count()), 0);
}

#[test]
fn an_event_answer_arriving_after_back_is_discarded() {
    let runtime = Runtime::start().unwrap();
    let installed = sample_color_view(&runtime);
    let launcher = &installed.launcher;

    let pending = send_right_like(launcher, "right");
    launcher.back();
    block_on(pending);

    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.status, Status::Idle);
    assert_eq!(block_on(runtime.designed_view_count()), 0);
    // Events sent with no view open go nowhere.
    send_key(launcher, "right");
    assert_eq!(launcher.view().status, Status::Idle);
}

#[test]
fn a_reopened_view_does_not_show_the_closed_views_answers() {
    let runtime = Runtime::start().unwrap();
    let installed = sample_color_view(&runtime);
    let launcher = &installed.launcher;

    let pending = send_right_like(launcher, "right");
    launcher.back();
    open_color_command(launcher);
    block_on(pending);

    assert_eq!(view_value(launcher), "Blue, #1E88E5");
    assert_eq!(block_on(runtime.designed_view_count()), 1);
}

#[test]
fn an_older_answer_arriving_late_does_not_replace_a_newer_one() {
    let runtime = Runtime::start().unwrap();
    let installed = sample_color_view(&runtime);
    let launcher = &installed.launcher;

    let first = send_right_like(launcher, "right");
    let second = send_right_like(launcher, "right");
    block_on(second);
    block_on(first);

    assert_eq!(view_value(launcher), "Pink, #D81B60");
}

#[test]
fn a_canvas_over_the_limits_is_an_error_and_the_view_stays_usable() {
    let runtime = Runtime::start().unwrap();
    let installed = faulty_view(&runtime);
    let launcher = &installed.launcher;
    // Down, Home and End make the fixture draw too many operations, too
    // long a text and too much inline image data.
    let cases = [
        (
            "down",
            "a canvas of the view has 20001 operations; at most 20000 are drawn",
        ),
        (
            "home",
            "a text of the canvas has 65537 characters; at most 65536 are drawn",
        ),
        (
            "end",
            "an image of the view holds 1048577 bytes of inline data; at most 1048576 are read",
        ),
    ];
    for (key, problem) in cases {
        send_key(launcher, key);

        let view = launcher.view();
        assert!(
            matches!(view.screen, Screen::DesignedView(_)),
            "{key:?}: {:?}",
            view.screen
        );
        assert_eq!(
            error(launcher),
            format!("The extension reported an error: {problem}")
        );
        // The last good drawing stays.
        assert_eq!(view_value(launcher), "0 events");
    }

    send_key(launcher, "up");
    assert_eq!(launcher.view().status, Status::Idle);
    assert_eq!(view_value(launcher), "4 events");
}

#[test]
fn a_crash_in_a_view_closes_it_and_the_command_keeps_working() {
    let runtime = Runtime::start().unwrap();
    let installed = faulty_view(&runtime);
    let launcher = &installed.launcher;

    // "right" makes the fixture trap.
    send_key(launcher, "right");

    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert!(error(launcher).contains("crashed"), "{:?}", view.status);
    assert_eq!(block_on(runtime.designed_view_count()), 0);
    // The package still runs: the counter command opens again.
    block_on(launcher.set_query("counter"));
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.id.ends_with("#counter"))
        .unwrap();
    launcher.select(index);
    block_on(launcher.activate_selected());
    assert_eq!(view_value(launcher), "0 events");
}

/// The extension API's shape check, against the world a component's
/// exports name (ADR 0046): before the version rule, `item` gained
/// `platforms` in #19, the command gained custom views in #21, and its list
/// moved into ADR 0036's envelope (`render` and `handle-event`) in #135 —
/// all under 0.1. A component built against the older shape declares that
/// version, which this Pane still runs, so it is routed to the previous
/// world and its exports' types are checked there, when it loads, here for
/// a command built into Pane (installing checks the same, see
/// `packages.rs`), and the first mismatch is named with the version a
/// rebuild targets.
#[test]
fn a_component_of_an_older_api_shape_is_refused_when_it_loads() {
    let launcher = launcher(vec![command("old", guest("old_api"))]);

    block_on(launcher.activate_selected());

    assert!(
        matches!(launcher.view().screen, Screen::Root { .. }),
        "{:?}",
        launcher.view().screen
    );
    let message = error(&launcher);
    assert!(
        message.starts_with(
            "Incompatible extension: it was built for an older extension API shape: \
             rebuild it against Pane's extension API 0.1"
        ),
        "{message}"
    );
    assert!(message.contains("it has no function `render`"), "{message}");
}

#[test]
fn a_view_the_guest_refuses_to_open_is_an_error() {
    let launcher = launcher(vec![command("faulty", guest("faulty"))]);
    open_faulty_item(&launcher, "no-view");

    block_on(launcher.activate_selected());

    assert_eq!(launcher.view().screen, Screen::Command);
    // The guest's refusal is shown once its answer reaches the launcher.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(launcher.view().status, Status::Error(_)) {
        assert!(
            Instant::now() < deadline,
            "no error was shown for the refusal"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        error(&launcher),
        "The extension reported an error: the guest refused the view"
    );
}

#[test]
fn a_package_preview_closes_an_open_view() {
    let runtime = Runtime::start().unwrap();
    let installed = sample_color_view(&runtime);
    let launcher = &installed.launcher;

    block_on(launcher.preview_package(std::path::Path::new("no-such-folder")));

    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Package { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(block_on(runtime.designed_view_count()), 0);
}

#[test]
fn replacing_a_components_code_closes_its_views() {
    let runtime = Runtime::start().unwrap();
    let installed = sample_color_view(&runtime);
    let launcher = &installed.launcher;

    // The installed copy's component, which is the path the runtime
    // holds: forgetting the built one forgets nothing.
    let component = installed
        .launcher
        .packages()
        .iter()
        .find(|package| package.title() == "Rust sample")
        .expect("the sample is installed")
        .commands()
        .first()
        .expect("the sample has a command")
        .component
        .clone();
    runtime.forget([component]);
    send_key(launcher, "right");

    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(error(launcher), "The extension's view is no longer open");
    assert_eq!(block_on(runtime.designed_view_count()), 0);
}

#[test]
fn back_answers_whether_it_backed_out_and_what_restoring_can_return_to() {
    let launcher = launcher(vec![command("sample", guest("sample_rust"))]);

    // Root search, an empty query: nothing is left to back out of, which
    // is what the window takes as the end of the Escape chain.
    assert!(!launcher.back(), "root search with an empty query");
    // Root search is always a view a reopened launcher can restore.
    assert!(launcher.restorable_view());

    // A query typed: Escape clears it, which is backing out.
    block_on(launcher.set_query("zz"));
    assert!(launcher.back());
    assert_eq!(launcher.view().query(), Some(""));

    // A command's view: backing out leaves it, and the view of a command
    // registered with Pane — not installed as a package — is always one a
    // reopened launcher can restore.
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Command));
    assert!(launcher.restorable_view());
    assert!(launcher.back());
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert!(!launcher.back());
}
