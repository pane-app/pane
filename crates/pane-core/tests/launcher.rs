//! Drives the launcher through its public interface against real guest
//! components built by `cargo xtask guests`.

use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::{CallError, CommandRegistration, Launcher, Runtime, Screen, Status};

fn guest(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(format!("{name}.wasm"));
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

fn command(id: &str, component: PathBuf) -> CommandRegistration {
    CommandRegistration {
        id: id.into(),
        title: format!("{id} command"),
        subtitle: None,
        component,
    }
}

fn launcher(commands: Vec<CommandRegistration>) -> Launcher {
    Launcher::new(Runtime::start(), commands)
}

fn titles(launcher: &Launcher) -> Vec<String> {
    launcher
        .view()
        .rows
        .into_iter()
        .map(|row| row.title)
        .collect()
}

fn open_faulty_item(launcher: &Launcher, item: &str) {
    block_on(launcher.activate_selected());
    let index = titles(launcher)
        .iter()
        .position(|title| title == item)
        .unwrap();
    launcher.move_selection(index as isize);
}

fn error(launcher: &Launcher) -> String {
    match launcher.view().status {
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

    assert_eq!(launcher.view().screen, Screen::Root);
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
    assert_eq!(view.screen, Screen::Root);
    assert_eq!(view.status, Status::Idle);
    assert_eq!(titles(&launcher), ["sample command"]);
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
    assert_eq!(launcher.view().status, Status::Result("fine".into()));
}

#[test]
fn a_missing_component_explains_itself_on_root() {
    let launcher = launcher(vec![command("gone", PathBuf::from("does-not-exist.wasm"))]);

    block_on(launcher.activate_selected());

    assert_eq!(launcher.view().screen, Screen::Root);
    assert!(error(&launcher).contains("does-not-exist.wasm"));
}

#[test]
fn an_unavailable_runtime_leaves_root_navigable() {
    let unavailable = Err(CallError::RuntimeUnavailable("no engine".into()));
    let launcher = Launcher::new(unavailable, vec![command("sample", guest("sample_rust"))]);

    block_on(launcher.activate_selected());

    assert_eq!(launcher.view().screen, Screen::Root);
    assert_eq!(error(&launcher), "Extension runtime unavailable: no engine");
}

#[test]
fn with_no_commands_root_is_empty_and_activation_does_nothing() {
    let launcher = launcher(vec![]);

    block_on(launcher.activate_selected());

    let view = launcher.view();
    assert_eq!(
        (view.rows.len(), view.selected, view.status),
        (0, None, Status::Idle)
    );
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
    assert_eq!((view.screen, view.status), (Screen::Root, Status::Idle));
}

#[test]
fn selecting_a_row_directly_ignores_indexes_past_the_list() {
    let launcher = launcher(vec![command("sample", guest("sample_rust"))]);
    block_on(launcher.activate_selected());

    launcher.select(1);
    assert_eq!(launcher.view().selected, Some(1));
    launcher.select(5);
    assert_eq!(launcher.view().selected, Some(1));
}
