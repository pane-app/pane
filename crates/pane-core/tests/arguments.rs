//! A command's arguments through the launcher's public interface, with the
//! arguments sample in Rust, JavaScript and TypeScript, real guests `cargo
//! xtask guests` assembles: a launch the user started that leaves a
//! required argument without a value shows Pane's argument form (from root
//! search, a global hotkey, a quick slot and another command's launch), and
//! the command runs once it is submitted, with the values by name and the
//! empty optional ones absent; submitting with a required field empty runs
//! nothing and marks it, and Back runs nothing; a background launch with a
//! required argument missing is refused, as are values another command
//! passes that are not the target's; text sent through an alias or as a
//! fallback fills the first text argument; the last dropdown value is
//! remembered across a restart; and a password's value is in none of
//! Pane's records. The manifest's mistakes are refused at install. The
//! sample tells what it ran with in a toast; the form's refusals are Pane's
//! own, in the status line.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::executor::block_on;
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{
    Choice, FieldKind, FormField, FormView, Launcher, PackageIdentity, ResultAction, Runtime,
    Screen, Status,
};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{manage, select_title, titles};

/// One language's arguments sample package.
struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    /// Its package's title.
    title: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-arguments",
    title: "Arguments sample",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-arguments-js",
    title: "JavaScript arguments sample",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-arguments-ts",
    title: "TypeScript arguments sample",
};

/// How long the commands other commands launched may take to run:
/// compiling the guest once is included; a slow, busy machine is not.
const PROMPTLY: Duration = Duration::from_secs(30);

/// What a required field left empty says, after its label in the status
/// line.
const MISSING: &str = "Enter a value to run the command";

/// A password typed into the form, which no record of Pane's may hold.
const SECRET: &str = "hunter2-Zq9-never-kept";

/// An error the sample answered with, as Pane shows it (a failure toast).
fn answered_error(message: &str) -> Status {
    Status::Error(format!("The extension reported an error: {message}"))
}

/// A system whose global hotkeys always register.
#[derive(Default)]
struct FakeHotkeys {
    registered: Mutex<Vec<Shortcut>>,
}

impl Hotkeys for FakeHotkeys {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
        self.registered.lock().unwrap().push(shortcut.clone());
        Ok(())
    }

    fn unregister(&self, shortcut: &Shortcut) {
        self.registered
            .lock()
            .unwrap()
            .retain(|kept| kept != shortcut);
    }
}

/// A launcher keeping its records in `data`, as Pane does each time it
/// starts.
fn launcher_in(data: &Path) -> Launcher {
    Launcher::with_packages(Runtime::start(), vec![], data.join("extensions"))
        .with_hotkeys(Arc::new(FakeHotkeys::default()))
        .with_quick_slots(data)
}

/// One test's Pane: its folders and the launcher.
struct Pane {
    sources: TempDir,
    data: TempDir,
    launcher: Launcher,
}

impl Pane {
    fn new() -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let launcher = launcher_in(data.path());
        Pane {
            sources,
            data,
            launcher,
        }
    }

    /// The same Pane after a restart: the launcher stops, and a new one
    /// reads the same records.
    fn restart(self) -> Pane {
        let Pane {
            sources,
            data,
            launcher,
        } = self;
        drop(launcher);
        let launcher = launcher_in(data.path());
        Pane {
            sources,
            data,
            launcher,
        }
    }

    /// The assembled package `name` copied into the source folder
    /// `folder`.
    fn source(&self, name: &str, folder: &str) -> PathBuf {
        let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages")
            .join(name);
        assert!(
            assembled.exists(),
            "{} is missing; run `cargo xtask guests`",
            assembled.display()
        );
        let folder = self.sources.path().join(folder);
        fs::create_dir_all(&folder).unwrap();
        for entry in fs::read_dir(&assembled).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
        }
        folder
    }

    /// Installs `fixture`'s sample.
    fn install(&self, fixture: &Fixture) -> PathBuf {
        let folder = self.source(fixture.package, "arguments");
        block_on(self.launcher.install_package(&folder));
        assert_eq!(
            self.launcher.view().status,
            Status::Result(format!("Installed {}", fixture.title))
        );
        folder
    }

    /// Root search, with `query` typed.
    fn search(&self, query: &str) {
        while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.launcher.back();
        }
        block_on(self.launcher.set_query(query));
    }

    /// Types `query` in root search, chooses the row titled `title` and
    /// waits for what it does; what it showed: its toast, or the status
    /// line.
    fn run(&self, query: &str, title: &str) -> Status {
        self.search(query);
        select_title(&self.launcher, title);
        block_on(self.launcher.activate_selected());
        shown(&self.launcher)
    }

    /// Types `query` in root search, which the alias it starts with makes
    /// select its command's row, and presses Enter; what it showed: its
    /// toast, or the status line.
    fn send(&self, query: &str) -> Status {
        self.search(query);
        assert_eq!(self.launcher.view().selected, Some(0), "{query}");
        block_on(self.launcher.activate_selected());
        shown(&self.launcher)
    }

    /// The argument form on screen, titled `title`.
    fn form(&self, title: &str) -> FormView {
        let view = self.launcher.view();
        assert_eq!(view.title, title, "{:?}", view.screen);
        view.form().expect("the argument form is shown").clone()
    }

    /// Sets the form's fields as `values` says, then submits it and waits
    /// for what it runs; what was shown: the command's toast, or the status
    /// line (the form's refusal).
    fn submit(&self, values: &[(&str, &str)]) -> Status {
        for (field, value) in values {
            self.launcher.set_field_value(field, value);
        }
        block_on(self.launcher.submit_form());
        shown(&self.launcher)
    }

    /// Gives the command with manifest id `command` of the package from
    /// `folder` the alias `alias`.
    fn alias(&self, folder: &Path, command: &str, alias: &str) {
        let outcome = self.launcher.set_alias(&id(folder, command), alias);
        block_on(outcome.expect("the alias is accepted"));
    }

    /// Waits until the commands launched by other commands have run, or
    /// asked for their arguments.
    fn launched(&self) {
        assert!(self.launcher.wait_for_launches(PROMPTLY));
    }
}

/// The command id of the command `command` of the package from `folder`.
fn id(folder: &Path, command: &str) -> String {
    format!(
        "{}#{command}",
        PackageIdentity::local(folder).unwrap().key()
    )
}

/// The argument form's field for an argument, empty.
fn field(id: &str, label: &str, kind: FieldKind, required: bool) -> FormField {
    FormField {
        id: id.into(),
        label: label.into(),
        kind,
        value: String::new(),
        error: None,
        description: None,
        required,
    }
}

/// "Greet"'s fields as the form first shows them.
fn greet_fields(tone: &str) -> Vec<FormField> {
    let choice = |id: &str, label: &str| Choice {
        id: id.into(),
        label: label.into(),
    };
    vec![
        field(
            "name",
            "Name",
            FieldKind::Text {
                placeholder: Some("Name".into()),
            },
            true,
        ),
        field(
            "secret",
            "Secret",
            FieldKind::Password {
                placeholder: Some("Secret".into()),
            },
            false,
        ),
        FormField {
            value: tone.into(),
            ..field(
                "tone",
                "Tone",
                FieldKind::Choice(vec![
                    choice("warm", "Warm"),
                    choice("brief", "Brief"),
                    choice("formal", "Formal"),
                ]),
                false,
            )
        },
    ]
}

fn the_form_asks_for_a_required_argument_and_runs_the_command_once(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture);
    let launcher = &pane.launcher;
    assert_eq!(launcher.arguments_asked_for(), None);

    // Enter on "Greet" asks for its arguments, in declaration order, the
    // dropdown on its first option.
    pane.run("greet", "Greet");
    let form = pane.form("Greet");
    assert_eq!(form.fields, greet_fields("warm"));
    assert_eq!(form.submit_label, "Run command");
    assert_eq!(launcher.arguments_asked_for(), Some(id(&folder, "greet")));

    // Submitting with the required name empty (or blank) marks it and runs
    // nothing.
    for blank in ["", "   "] {
        assert_eq!(
            pane.submit(&[("name", blank)]),
            Status::Error(format!("Name: {MISSING}"))
        );
        let form = pane.form("Greet");
        assert_eq!(form.fields[0].error.as_deref(), Some(MISSING));
        assert_eq!(form.fields[1].error, None);
    }

    // Filled, it runs once with the values by name; the empty password is
    // absent. Root search is back as it was, its query kept.
    assert_eq!(
        pane.submit(&[("name", "Ada")]),
        Status::Result(
            "Greet run 1 from root-search: name=Ada, tone=warm; fallback text: none".into()
        )
    );
    assert_eq!(
        launcher.view().screen,
        Screen::Root {
            query: "greet".into()
        }
    );

    // Back runs nothing: the next run is the second.
    pane.run("greet", "Greet");
    pane.form("Greet");
    assert!(launcher.back());
    let view = launcher.view();
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "greet".into()
        }
    );
    assert_eq!(view.status, Status::Idle);
    pane.run("greet", "Greet");
    assert_eq!(
        pane.submit(&[("name", "Grace"), ("secret", "s3cret"), ("tone", "brief")]),
        Status::Result(
            "Greet run 2 from root-search: name=Grace, secret (6 characters), tone=brief; \
             fallback text: none"
                .into()
        )
    );
}

fn hotkey_and_quick_slot_launches_ask_for_the_arguments(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture);
    let launcher = &pane.launcher;

    // Its global hotkey: the window is shown for the form, though the
    // command is no-view; one without a required argument runs without it.
    let shortcut = Shortcut::parse("ctrl+alt+s").unwrap();
    let set = launcher.set_hotkey(&id(&folder, "stamp"), Some(shortcut.clone()));
    block_on(set.expect("the hotkey is accepted"));
    let relay = Shortcut::parse("ctrl+alt+r").unwrap();
    let set = launcher.set_hotkey(&id(&folder, "relay"), Some(relay.clone()));
    block_on(set.expect("the hotkey is accepted"));
    assert!(launcher.hotkey_shows_window(&shortcut));
    assert!(!launcher.hotkey_shows_window(&relay));
    pane.search("abc");
    block_on(
        launcher
            .press_hotkey(&shortcut)
            .expect("the hotkey is registered"),
    );
    let form = pane.form("Stamp");
    assert_eq!(
        form.fields,
        [field(
            "label",
            "Label",
            FieldKind::Text {
                placeholder: Some("Label".into())
            },
            true
        )]
    );
    assert_eq!(
        pane.submit(&[("label", "first")]),
        Status::Result("Stamped first from hotkey".into())
    );
    assert_eq!(
        launcher.view().screen,
        Screen::Root {
            query: "abc".into()
        }
    );

    // Its quick slot.
    pane.search("");
    select_title(launcher, "Stamp");
    let (_, recorded) = launcher.change_quick_slots(&id(&folder, "stamp"), ResultAction::Pin);
    block_on(recorded);
    pane.search("");
    block_on(launcher.activate_quick_slot(0));
    pane.form("Stamp");
    assert_eq!(
        pane.submit(&[("label", "second")]),
        Status::Result("Stamped second from quick-slot".into())
    );
}

fn another_command_launch_asks_for_missing_arguments_and_a_background_one_is_refused(
    fixture: &Fixture,
) {
    let pane = Pane::new();
    let folder = pane.install(fixture);
    let launcher = &pane.launcher;
    pane.alias(&folder, "relay", "rl");

    // In the background: refused while a required argument has no value.
    assert_eq!(
        pane.send("rl background stamp"),
        answered_error(&format!(
            "Stamp of {} needs a value for its argument `label`, which a background launch \
             cannot ask for; pass it, or launch it user-initiated",
            fixture.title
        ))
    );
    assert_eq!(
        pane.send("rl background stamp label=quiet"),
        Status::Result("Relayed stamp in the background".into())
    );
    pane.launched();
    assert_eq!(
        pane.send("rl last"),
        Status::Result("Last stamp: quiet from command, background".into())
    );

    // Values that are not the target's are refused.
    assert_eq!(
        pane.send("rl stamp colour=red"),
        answered_error(&format!(
            "Stamp of {}: it has no argument `colour`; its arguments are `label`",
            fixture.title
        ))
    );
    assert_eq!(
        pane.send("rl greet name=Ada tone=loud"),
        answered_error(&format!(
            "Greet of {}: \"loud\" is not an option of the argument `tone`; its options are \
             \"warm\", \"brief\", \"formal\"",
            fixture.title
        ))
    );

    // User-initiated with its values: it runs with them.
    pane.send("rl stamp label=given");
    pane.launched();
    assert_eq!(
        pane.send("rl last"),
        Status::Result("Last stamp: given from command, user-initiated".into())
    );

    // User-initiated without them: the form asks, and asks for the window.
    assert!(!launcher.take_window_request());
    pane.send("rl stamp");
    pane.launched();
    pane.form("Stamp");
    assert!(launcher.take_window_request());
    assert_eq!(
        pane.submit(&[("label", "asked")]),
        Status::Result("Stamped asked from command".into())
    );
    // A dropdown value another command passes is chosen in the form.
    pane.send("rl greet tone=formal");
    pane.launched();
    assert_eq!(pane.form("Greet").fields, greet_fields("formal"));
}

fn alias_and_fallback_text_fill_the_first_text_argument(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture);
    let launcher = &pane.launcher;

    // Through its alias: the text fills the name, so nothing is asked, and
    // stays the fallback text; the dropdown, given no value, is absent.
    pane.alias(&folder, "greet", "gr");
    assert_eq!(
        pane.send("gr  Ada "),
        Status::Result("Greet run 1 from alias: name=Ada; fallback text: Ada".into())
    );
    assert_eq!(
        launcher.view().screen,
        Screen::Root {
            query: "gr  Ada ".into()
        }
    );

    // Its first argument is text and the others optional, so it may be a
    // fallback, as may Stamp and Relay (which takes a query).
    manage(launcher);
    for command in ["Greet", "Stamp", "Relay"] {
        select_title(launcher, &format!("Fallback: {command}"));
    }
    select_title(launcher, "Fallback: Greet");
    block_on(launcher.activate_selected());
    pane.search("zqx  words ");
    assert_eq!(titles(launcher), ["Greet"]);
    launcher.move_selection(1);
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(launcher),
        Status::Result(
            "Greet run 2 from fallback: name=zqx  words; fallback text: zqx  words".into()
        )
    );
}

fn the_last_dropdown_is_remembered_and_a_password_is_recorded_nowhere(fixture: &Fixture) {
    let pane = Pane::new();
    pane.install(fixture);

    pane.run("greet", "Greet");
    assert_eq!(
        pane.submit(&[("name", "Ada"), ("secret", SECRET), ("tone", "formal")]),
        Status::Result(format!(
            "Greet run 1 from root-search: name=Ada, secret ({} characters), tone=formal; \
             fallback text: none",
            SECRET.chars().count()
        ))
    );
    assert_no_record_holds(pane.data.path(), SECRET);

    // After a restart, the form chooses the last tone, and holds no secret.
    let pane = pane.restart();
    pane.run("greet", "Greet");
    assert_eq!(pane.form("Greet").fields, greet_fields("formal"));
    assert!(pane.launcher.back());
    assert_no_record_holds(pane.data.path(), SECRET);
}

/// Checks that no file under `folder` (Pane's records, the packages'
/// extension data, anything Pane wrote) holds `secret`.
fn assert_no_record_holds(folder: &Path, secret: &str) {
    let mut folders = vec![folder.to_path_buf()];
    let mut files = 0;
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            files += 1;
            let bytes = fs::read(&path).unwrap();
            assert!(
                !bytes
                    .windows(secret.len())
                    .any(|window| window == secret.as_bytes()),
                "{} holds the password",
                path.display()
            );
        }
    }
    assert!(files > 0, "Pane recorded something to search");
}

macro_rules! contract {
    ($($check:ident),* $(,)?) => {
        mod rust {
            $(#[test] fn $check() { super::$check(&super::RUST) })*
        }
        mod javascript {
            $(#[test] fn $check() { super::$check(&super::JAVASCRIPT) })*
        }
        mod typescript {
            $(#[test] fn $check() { super::$check(&super::TYPESCRIPT) })*
        }
    };
}

contract!(
    the_form_asks_for_a_required_argument_and_runs_the_command_once,
    hotkey_and_quick_slot_launches_ask_for_the_arguments,
    another_command_launch_asks_for_missing_arguments_and_a_background_one_is_refused,
    alias_and_fallback_text_fill_the_first_text_argument,
    the_last_dropdown_is_remembered_and_a_password_is_recorded_nowhere,
);

/// Installs the Rust sample with its `pane.json` changed by `change`;
/// the Pane and the status line.
fn install_changed(change: impl FnOnce(&mut serde_json::Value)) -> (Pane, Status) {
    let pane = Pane::new();
    let folder = pane.source(RUST.package, "changed");
    let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    change(&mut manifest);
    fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
    block_on(pane.launcher.install_package(&folder));
    let status = pane.launcher.view().status;
    (pane, status)
}

/// Installs the Rust sample with "Greet"'s arguments changed by `change`,
/// and checks that the install is refused saying `reason`.
fn refused(change: impl FnOnce(&mut serde_json::Value), reason: &str) {
    let (pane, status) = install_changed(|manifest| change(&mut manifest["commands"][0]));
    let Status::Error(error) = status else {
        panic!("installed: {status:?}");
    };
    assert!(error.contains(reason), "{error}");
    assert!(pane.launcher.packages().is_empty());
}

#[test]
fn a_fourth_argument_is_refused_at_install() {
    refused(
        |greet| {
            greet["arguments"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({ "name": "fourth", "type": "text" }));
        },
        "command `greet` declares 4 arguments; a command has at most 3",
    );
}

#[test]
fn a_repeated_argument_name_is_refused_at_install() {
    refused(
        |greet| greet["arguments"][1]["name"] = "name".into(),
        "command `greet` declares the argument `name` twice; argument names are unique",
    );
}

#[test]
fn an_unknown_argument_type_is_refused_at_install() {
    refused(
        |greet| greet["arguments"][0]["type"] = "number".into(),
        "the argument `name` of command `greet` has the type \"number\"; an argument's `type` \
         is \"text\", \"password\" or \"dropdown\"",
    );
}

#[test]
fn a_dropdown_without_options_is_refused_at_install() {
    refused(
        |greet| {
            greet["arguments"][2]
                .as_object_mut()
                .unwrap()
                .remove("options");
        },
        "the argument `tone` of command `greet` is a dropdown without `options`; list the \
         choices it offers",
    );
}

#[test]
fn a_scheduled_command_with_a_required_argument_is_refused_at_install() {
    refused(
        |greet| greet["schedule"] = serde_json::json!({ "everySeconds": 60 }),
        "command `greet` has a schedule, but its argument `name` is required, which a \
         scheduled run cannot ask for; make it optional or remove the schedule",
    );
}

#[test]
fn a_command_whose_other_arguments_are_not_all_optional_is_no_fallback() {
    let (pane, status) = install_changed(|manifest| {
        manifest["commands"][0]["arguments"][2]["required"] = true.into();
    });
    assert_eq!(status, Status::Result("Installed Arguments sample".into()));
    let launcher = &pane.launcher;
    manage(launcher);
    let rows = titles(launcher);
    assert!(rows.iter().any(|row| row == "Alias for Greet"), "{rows:?}");
    assert!(!rows.iter().any(|row| row == "Fallback: Greet"), "{rows:?}");
    assert!(rows.iter().any(|row| row == "Fallback: Stamp"), "{rows:?}");
}
