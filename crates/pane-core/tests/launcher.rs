//! Drives the launcher through its public interface against real guest
//! components built by `cargo xtask guests`.

use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::{
    CallError, CommandRegistration, Key, Launcher, Point, Runtime, Screen, Status, Unavailable,
    ViewEvent,
};

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
        search: false,
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

/// A launcher with the Rust sample's form ("Greet someone") opened.
fn sample_form() -> Launcher {
    let launcher = launcher(vec![command("sample", guest("sample_rust"))]);
    block_on(launcher.activate_selected());
    launcher.select(4);
    block_on(launcher.activate_selected());
    assert!(
        matches!(launcher.view().screen, Screen::Form(_)),
        "{:?}",
        launcher.view().screen
    );
    launcher
}

fn field_errors(launcher: &Launcher) -> Vec<Option<String>> {
    let view = launcher.view();
    let form = view.form().expect("a form");
    form.fields
        .iter()
        .map(|field| field.error.clone())
        .collect()
}

#[test]
fn back_from_a_form_returns_to_the_command_with_its_item_selected() {
    let launcher = sample_form();

    launcher.back();

    let view = launcher.view();
    assert_eq!(
        (view.screen, view.title.as_str(), view.selected),
        (Screen::Command, "Rust sample", Some(4))
    );
    launcher.back();
    assert!(
        matches!(launcher.view().screen, Screen::Root { .. }),
        "{:?}",
        launcher.view().screen
    );
}

#[test]
fn editing_an_invalid_field_clears_its_error() {
    let launcher = sample_form();
    block_on(launcher.submit_form());
    assert_eq!(field_errors(&launcher), [Some("Enter a name".into()), None]);

    launcher.set_field_value("name", "A");

    assert_eq!(field_errors(&launcher), [None, None]);
}

#[test]
fn a_choice_the_form_does_not_offer_is_ignored() {
    let launcher = sample_form();

    launcher.set_field_value("greeting", "howdy");
    launcher.set_field_value("missing", "value");

    let view = launcher.view();
    let values: Vec<&str> = view
        .form()
        .unwrap()
        .fields
        .iter()
        .map(|field| field.value.as_str())
        .collect();
    assert_eq!(values, ["", "hello"]);
}

#[test]
fn a_form_level_error_is_shown_without_marking_a_field() {
    let launcher = launcher(vec![command("faulty", guest("faulty"))]);
    open_faulty_item(&launcher, "form");
    block_on(launcher.activate_selected());

    block_on(launcher.submit_form());

    assert_eq!(error(&launcher), "the guest refused the form");
    assert_eq!(field_errors(&launcher), [None]);
}

#[test]
fn going_back_while_a_form_is_submitted_discards_its_answer() {
    let launcher = sample_form();
    launcher.set_field_value("name", "Ada");

    let pending = launcher.submit_form();
    assert_eq!(launcher.view().status, Status::Running);
    launcher.back();
    block_on(pending);

    let view = launcher.view();
    assert_eq!((view.screen, view.status), (Screen::Command, Status::Idle));
}

#[test]
fn submitting_again_while_a_submission_is_pending_is_ignored() {
    let launcher = sample_form();
    let first = launcher.submit_form();
    launcher.set_field_value("name", "Ada");

    let second = launcher.submit_form();
    block_on(second);
    assert_eq!(launcher.view().status, Status::Running);
    block_on(first);

    // Only the empty submission ran. Its rejection reports the name, but
    // does not mark the field, which the user has edited since.
    assert_eq!(error(&launcher), "Name: Enter a name");
    assert_eq!(field_errors(&launcher), [None, None]);
    block_on(launcher.submit_form());
    assert_eq!(
        launcher.view().status,
        Status::Result("Hello, Ada, from the Rust guest".into())
    );
}

#[test]
fn submitting_outside_a_form_does_nothing() {
    let launcher = launcher(vec![command("sample", guest("sample_rust"))]);
    block_on(launcher.activate_selected());

    block_on(launcher.submit_form());

    assert_eq!(launcher.view().status, Status::Idle);
}

#[test]
fn selecting_a_row_directly_ignores_indexes_past_the_list() {
    let launcher = launcher(vec![command("sample", guest("sample_rust"))]);
    block_on(launcher.activate_selected());

    launcher.select(1);
    assert_eq!(launcher.view().selected, Some(1));
    launcher.select(8);
    assert_eq!(launcher.view().selected, Some(1));
}

#[test]
fn an_unavailable_form_explains_itself_instead_of_opening() {
    let launcher = launcher(vec![command("faulty", guest("faulty"))]);
    // The fixture's "nowhere" item has a form and declares no operating
    // system at all, so it is unavailable wherever the test runs.
    open_faulty_item(&launcher, "nowhere");
    let view = launcher.view();
    let reason = view.rows[view.selected.unwrap()].unavailable.clone();
    let reason = reason.expect("the row says why it is unavailable");
    assert_eq!(
        reason,
        Unavailable::OnThisSystem(platforms::nowhere("this action"))
    );
    let reason = reason.reason().to_owned();

    block_on(launcher.activate_selected());

    let view = launcher.view();
    assert_eq!(
        (view.screen, view.status),
        (Screen::Command, Status::Error(reason))
    );
    launcher.select(0);
    block_on(launcher.activate_selected());
    assert_eq!(shown(&launcher), Status::Result("fine".into()));
}

/// A launcher over `runtime` with the Rust sample's color picker opened.
fn sample_color_view(runtime: &Runtime) -> Launcher {
    let launcher = Launcher::new(
        Ok(runtime.clone()),
        vec![command("sample", guest("sample_rust"))],
    );
    block_on(launcher.activate_selected());
    launcher.select(5);
    block_on(launcher.activate_selected());
    assert!(
        matches!(launcher.view().screen, Screen::CustomView(_)),
        "{:?}",
        launcher.view().screen
    );
    launcher
}

/// A launcher over `runtime` with the faulty fixture's counting view opened.
fn faulty_view(runtime: &Runtime) -> Launcher {
    let launcher = Launcher::new(
        Ok(runtime.clone()),
        vec![command("faulty", guest("faulty"))],
    );
    open_faulty_item(&launcher, "view");
    block_on(launcher.activate_selected());
    assert!(
        matches!(launcher.view().screen, Screen::CustomView(_)),
        "{:?}",
        launcher.view().screen
    );
    launcher
}

fn view_value(launcher: &Launcher) -> String {
    let view = launcher.view();
    view.custom_view()
        .expect("a view is open")
        .frame
        .value
        .clone()
}

const RIGHT: ViewEvent = ViewEvent::Key(Key::Right);
const ORIGIN: Point = Point { x: 0, y: 0 };

#[test]
fn back_from_a_custom_view_closes_it_and_returns_to_the_command() {
    let runtime = Runtime::start().unwrap();
    let launcher = sample_color_view(&runtime);
    assert_eq!(block_on(runtime.view_count()), 1);

    launcher.back();

    let view = launcher.view();
    assert_eq!(
        (view.screen, view.selected, view.status),
        (Screen::Command, Some(5), Status::Idle)
    );
    assert_eq!(block_on(runtime.view_count()), 0);
}

#[test]
fn a_view_that_opens_after_the_user_left_is_closed_again() {
    let runtime = Runtime::start().unwrap();
    let launcher = Launcher::new(
        Ok(runtime.clone()),
        vec![command("sample", guest("sample_rust"))],
    );
    block_on(launcher.activate_selected());
    launcher.select(5);

    let opening = launcher.activate_selected();
    assert_eq!(launcher.view().status, Status::Running);
    launcher.back();
    block_on(opening);

    assert!(
        matches!(launcher.view().screen, Screen::Root { .. }),
        "{:?}",
        launcher.view().screen
    );
    assert_eq!(block_on(runtime.view_count()), 0);
}

#[test]
fn an_event_answer_arriving_after_back_is_discarded() {
    let runtime = Runtime::start().unwrap();
    let launcher = sample_color_view(&runtime);

    let pending = launcher.send_view_event(RIGHT);
    launcher.back();
    block_on(pending);

    let view = launcher.view();
    assert_eq!((view.screen, view.status), (Screen::Command, Status::Idle));
    assert_eq!(block_on(runtime.view_count()), 0);
    // Events sent with no view open go nowhere.
    block_on(launcher.send_view_event(RIGHT));
    assert_eq!(launcher.view().status, Status::Idle);
}

#[test]
fn a_reopened_view_does_not_show_the_closed_views_answers() {
    let runtime = Runtime::start().unwrap();
    let launcher = sample_color_view(&runtime);

    let pending = launcher.send_view_event(RIGHT);
    launcher.back();
    block_on(launcher.activate_selected());
    block_on(pending);

    assert_eq!(view_value(&launcher), "Blue, #1E88E5");
    assert_eq!(block_on(runtime.view_count()), 1);
}

#[test]
fn an_older_answer_arriving_late_does_not_replace_a_newer_one() {
    let runtime = Runtime::start().unwrap();
    let launcher = sample_color_view(&runtime);

    let first = launcher.send_view_event(RIGHT);
    let second = launcher.send_view_event(RIGHT);
    block_on(second);
    block_on(first);

    assert_eq!(view_value(&launcher), "Pink, #D81B60");
}

#[test]
fn pointer_moves_and_releases_are_sent_only_while_pressed() {
    let runtime = Runtime::start().unwrap();
    let launcher = faulty_view(&runtime);
    let send = |event| block_on(launcher.send_view_event(event));

    send(ViewEvent::PointerMove(ORIGIN));
    send(ViewEvent::PointerUp(ORIGIN));
    assert_eq!(view_value(&launcher), "0 events");
    send(ViewEvent::PointerDown(ORIGIN));
    send(ViewEvent::PointerMove(ORIGIN));
    send(ViewEvent::PointerUp(ORIGIN));
    assert_eq!(view_value(&launcher), "3 events");
    send(ViewEvent::PointerMove(ORIGIN));
    assert_eq!(view_value(&launcher), "3 events");
}

#[test]
fn a_frame_over_the_limits_is_an_error_and_the_view_stays_usable() {
    let runtime = Runtime::start().unwrap();
    let launcher = faulty_view(&runtime);
    // Down, Home and End make the fixture draw too many shapes, too long a
    // text and too wide a frame.
    let cases = [
        (
            Key::Down,
            "the frame has 4097 shapes; at most 4096 are drawn",
        ),
        (
            Key::Home,
            "a text of the frame has 257 characters; at most 256 are drawn",
        ),
        (
            Key::End,
            "the frame is 4097 x 20 pixels; at most 4096 x 4096 are drawn",
        ),
    ];
    for (key, problem) in cases {
        block_on(launcher.send_view_event(ViewEvent::Key(key)));

        let view = launcher.view();
        assert!(matches!(view.screen, Screen::CustomView(_)), "{key:?}");
        assert_eq!(
            error(&launcher),
            format!("The extension reported an error: {problem}")
        );
        // The last good drawing stays.
        assert_eq!(view_value(&launcher), "0 events");
    }

    block_on(launcher.send_view_event(ViewEvent::Key(Key::Up)));
    assert_eq!(launcher.view().status, Status::Idle);
    assert_eq!(view_value(&launcher), "4 events");
}

#[test]
fn the_launcher_says_whether_the_pointer_is_held_over_the_view() {
    let runtime = Runtime::start().unwrap();
    let launcher = faulty_view(&runtime);
    assert!(!launcher.pointer_held());

    block_on(launcher.send_view_event(ViewEvent::PointerDown(ORIGIN)));
    assert!(launcher.pointer_held());
    block_on(launcher.send_view_event(ViewEvent::PointerUp(ORIGIN)));
    assert!(!launcher.pointer_held());
}

#[test]
fn a_drag_sends_only_the_latest_move_while_one_is_being_handled() {
    let runtime = Runtime::start().unwrap();
    let launcher = faulty_view(&runtime);
    block_on(launcher.send_view_event(ViewEvent::PointerDown(ORIGIN)));

    // The first move is on its way; the next two wait, and only the later
    // one is sent once the first is answered.
    let moves: Vec<_> = (1..=3)
        .map(|x| launcher.send_view_event(ViewEvent::PointerMove(Point { x, y: 0 })))
        .collect();
    for pending in moves.into_iter().rev() {
        block_on(pending);
    }

    assert_eq!(view_value(&launcher), "3 events");
}

#[test]
fn a_coalesced_move_is_sent_before_the_release_that_follows_it() {
    let runtime = Runtime::start().unwrap();
    let launcher = sample_color_view(&runtime);
    let send = |event| launcher.send_view_event(event);
    block_on(send(ViewEvent::PointerDown(Point { x: 10, y: 10 })));

    let first = send(ViewEvent::PointerMove(Point { x: 80, y: 80 }));
    let second = send(ViewEvent::PointerMove(Point { x: 45, y: 10 }));
    let released = send(ViewEvent::PointerUp(Point { x: 45, y: 10 }));
    block_on(released);
    block_on(second);
    block_on(first);

    // The waiting move reached the guest before the release: its swatch,
    // not the first move's, is chosen.
    assert_eq!(view_value(&launcher), "Light orange, #FFCC80");
    assert!(!launcher.pointer_held());
}

#[test]
fn an_error_from_a_view_is_shown_and_the_view_stays_open() {
    let runtime = Runtime::start().unwrap();
    let launcher = faulty_view(&runtime);

    block_on(launcher.send_view_event(ViewEvent::Key(Key::Left)));

    assert!(
        matches!(launcher.view().screen, Screen::CustomView(_)),
        "{:?}",
        launcher.view().screen
    );
    assert_eq!(
        error(&launcher),
        "The extension reported an error: the view refused"
    );
    // The next handled event clears the error.
    block_on(launcher.send_view_event(ViewEvent::Key(Key::Up)));
    assert_eq!(launcher.view().status, Status::Idle);
    assert_eq!(view_value(&launcher), "1 events");
}

#[test]
fn a_crash_in_a_view_closes_it_and_the_command_keeps_working() {
    let runtime = Runtime::start().unwrap();
    let launcher = faulty_view(&runtime);

    block_on(launcher.send_view_event(RIGHT));

    let view = launcher.view();
    assert_eq!(view.screen, Screen::Command);
    assert!(error(&launcher).contains("crashed"), "{:?}", view.status);
    assert_eq!(block_on(runtime.view_count()), 0);
    launcher.select(0);
    block_on(launcher.activate_selected());
    assert_eq!(shown(&launcher), Status::Result("fine".into()));
}

/// The pre-release extension API 0.1 changes shape between slices without a
/// version bump: `item` gained `platforms` in #19, the command gained custom
/// views in #21, and its list moved into ADR 0036's envelope (`render` and
/// `handle-event`) in #135. A component built against an older shape
/// declares the same API version; its exports' types are checked when it
/// loads, here for a command built into Pane (installing checks the same,
/// see `packages.rs`), and the first mismatch is named.
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
             rebuild it against Pane's current extension API 0.1"
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
    assert_eq!(
        error(&launcher),
        "The extension reported an error: the guest refused the view"
    );
}

#[test]
fn a_package_preview_closes_an_open_view() {
    let runtime = Runtime::start().unwrap();
    let launcher = sample_color_view(&runtime);

    block_on(launcher.preview_package(std::path::Path::new("no-such-folder")));

    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Package { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(block_on(runtime.view_count()), 0);
}

#[test]
fn replacing_a_components_code_closes_its_views() {
    let runtime = Runtime::start().unwrap();
    let launcher = sample_color_view(&runtime);

    runtime.forget([guest("sample_rust")]);
    block_on(launcher.send_view_event(RIGHT));

    let view = launcher.view();
    assert_eq!(view.screen, Screen::Command);
    assert_eq!(error(&launcher), "The extension's view is no longer open");
    assert_eq!(block_on(runtime.view_count()), 0);
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
