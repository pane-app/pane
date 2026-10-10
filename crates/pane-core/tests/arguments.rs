//! A command's arguments through the launcher's public interface, with the
//! arguments sample in Rust, JavaScript and TypeScript, real guests `cargo
//! xtask guests` assembles: root search shows the selected row's fields
//! inline after the query (#205) — Enter with a required one blank marks
//! it, says "Enter <placeholder>" and runs nothing, the values typed run
//! the command with them by name and the empty optional ones absent, and
//! they survive the list being re-ranked and a restart's remembered
//! dropdowns (dropped once the choice is gone); a launch that leaves root
//! search — a global hotkey, a quick slot, another command's — still asks
//! through Pane's argument form, which submits and runs the command once;
//! a background launch with a required argument missing is refused, as are
//! values another command passes that are not the target's; text sent as a
//! fallback fills the first text argument, while an alias followed by a
//! space fills the fields and runs the command from its alias, a command
//! without arguments opening at once instead; and a password's value is in
//! none of Pane's records. The manifest's mistakes are refused at install.
//! The sample tells what it ran with in a toast; the refusals are Pane's
//! own, in the status line.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::executor::block_on;
use pane_core::clipboard::ManualClock;
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{
    AliasFlow, Choice, FieldKind, FormField, FormView, Launcher, Limits, PackageIdentity,
    ResultAction, Runtime, Screen, Status,
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

fn root_searchs_fields_refuse_a_blank_required_argument_and_run_with_the_values(fixture: &Fixture) {
    let pane = Pane::new();
    pane.install(fixture);
    let launcher = &pane.launcher;
    assert_eq!(launcher.arguments_asked_for(), None);

    // "Greet" selected: its fields show after the query, one per argument,
    // the dropdown with no choice chosen — the empty choice its list leads
    // with — and nothing marked before anything has been left blank.
    pane.search("greet");
    assert_eq!(titles(launcher), ["Greet"]);
    let fields = launcher.argument_fields().expect("the fields show");
    assert_eq!(fields.title, "Greet");
    assert_eq!(fields.fields, greet_fields(""));

    // Enter with the required name blank marks it, names it in the status
    // line, and runs nothing: root search stays as it was.
    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert_eq!(view.status, Status::Error("Enter Name".into()));
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "greet".into()
        }
    );
    let fields = launcher.argument_fields().expect("the fields show");
    assert_eq!(fields.fields[0].error.as_deref(), Some(MISSING));
    assert_eq!(fields.fields[1].error, None);

    // The value typed clears the mark and runs the command once with it;
    // the empty password is absent, and the dropdown with no choice too.
    launcher.set_argument_value("name", "Ada");
    assert_eq!(launcher.argument_fields().unwrap().fields[0].error, None);
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(launcher),
        Status::Result("Greet run 1 from root-search: name=Ada; fallback text: none".into())
    );
    assert_eq!(
        launcher.view().screen,
        Screen::Root {
            query: "greet".into()
        }
    );

    // The fields hold what was typed, and the next run carries every
    // value, the dropdown's among them.
    assert_eq!(launcher.argument_fields().unwrap().fields[0].value, "Ada");
    launcher.set_argument_value("name", "Grace");
    launcher.set_argument_value("secret", "s3cret");
    launcher.set_argument_value("tone", "brief");
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(launcher),
        Status::Result(
            "Greet run 2 from root-search: name=Grace, secret (6 characters), tone=brief; \
             fallback text: none"
                .into()
        )
    );

    // The values go with the query: another search of the same row starts
    // them empty.
    pane.search("gre");
    assert_eq!(launcher.argument_fields().unwrap().fields[0].value, "");
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

fn an_alias_and_a_space_fill_the_first_argument_and_a_fallback_the_query(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture);
    let launcher = &pane.launcher;

    // The alias followed by a space: no row sends the text after it (the
    // command declares arguments), and its own row, hoisted by the alias,
    // is selected with its fields showing — the space and Tab after the
    // alias alike are the way into them. Pane's own Git row matches the
    // two letters fuzzily through its subtitle (#193), below the row the
    // alias hoists.
    pane.alias(&folder, "greet", "gr");
    pane.search("gr ");
    assert_eq!(titles(launcher), ["Greet", "Install extension from Git…"]);
    assert_eq!(launcher.view().selected, Some(0));
    assert_eq!(launcher.alias_after_space(), Some(AliasFlow::Fields));
    pane.search("gr");
    assert_eq!(launcher.alias_after_tab(), Some(AliasFlow::Fields));

    // What is typed after the alias fills the first argument, and Enter
    // runs the command from its alias with it; the alias stays in the
    // query, and the text is not the fallback text — it is the value.
    launcher.set_argument_value("name", "Ada");
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(launcher),
        Status::Result("Greet run 1 from alias: name=Ada; fallback text: none".into())
    );
    assert_eq!(launcher.view().screen, Screen::Root { query: "gr".into() });

    // A command that takes a query and declares no arguments keeps the row
    // that sends the text after its alias: Relay, listed first.
    pane.search("rl stamp");
    assert_eq!(launcher.alias_after_space(), None);
    assert_eq!(titles(launcher), ["Relay"]);

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
    // Nothing else is listed, so the first fallback is selected and Enter
    // sends the text, which fills the name (ADR 0031).
    assert_eq!(launcher.view().selected, Some(0));
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(launcher),
        Status::Result(
            "Greet run 2 from fallback: name=zqx  words; fallback text: zqx  words".into()
        )
    );
}

/// Installs the Rust sample with Greet's arguments changed by `change`
/// and checks that the extension list offers no fallback row for it,
/// while Stamp's and Relay's are offered: Stamp's only argument is text
/// and Relay takes a query.
fn greet_not_offered_as_a_fallback(change: impl FnOnce(&mut serde_json::Value)) {
    let (pane, status) = install_changed(change);
    assert!(matches!(status, Status::Result(_)), "{status:?}");
    manage(&pane.launcher);
    let rows = titles(&pane.launcher);
    assert!(
        !rows.iter().any(|row| row == "Fallback: Greet"),
        "Greet cannot take the query as its fallback text: {rows:?}"
    );
    select_title(&pane.launcher, "Fallback: Stamp");
    select_title(&pane.launcher, "Fallback: Relay");
}

/// A command whose first argument is not text, or whose first is text
/// but a later one is required, cannot be sent the query, so the
/// extension list offers no fallback row for it (#194, #120).
#[test]
fn a_command_whose_arguments_cannot_take_the_query_is_not_offered_as_a_fallback() {
    // The first argument is a password.
    greet_not_offered_as_a_fallback(|manifest| {
        manifest["commands"][0]["arguments"][0]["type"] = "password".into();
    });
    // A required argument follows the first.
    greet_not_offered_as_a_fallback(|manifest| {
        manifest["commands"][0]["arguments"][1]["required"] = true.into();
    });
}

fn the_last_dropdown_is_remembered_and_dropped_once_the_choice_is_gone(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture);
    let launcher = &pane.launcher;

    // Run with a dropdown choice and a password: the choice is remembered,
    // the password recorded nowhere.
    pane.search("greet");
    launcher.set_argument_value("name", "Ada");
    launcher.set_argument_value("secret", SECRET);
    launcher.set_argument_value("tone", "formal");
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(launcher),
        Status::Result(format!(
            "Greet run 1 from root-search: name=Ada, secret ({} characters), tone=formal; \
             fallback text: none",
            SECRET.chars().count()
        ))
    );
    assert_no_record_holds(pane.data.path(), SECRET);

    // After a restart the tone is offered again, and no secret is held.
    let pane = pane.restart();
    pane.search("greet");
    assert_eq!(
        pane.launcher.argument_fields().unwrap().fields[2].value,
        "formal"
    );
    assert_no_record_holds(pane.data.path(), SECRET);

    // The command updated with "formal" no longer among its options: the
    // remembered choice is dropped, the field showing no choice. The
    // changed manifest is installed by reloading the package — replacing
    // its copy with the source folder's current contents.
    let file = folder.join("pane.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    fs::write(
        &file,
        serde_json::to_string(&without_formal(&manifest)).unwrap(),
    )
    .unwrap();
    let identity = PackageIdentity::local(&folder).unwrap();
    block_on(pane.launcher.reload(&identity));
    assert!(matches!(pane.launcher.view().status, Status::Result(_)));
    pane.search("greet");
    assert_eq!(pane.launcher.argument_fields().unwrap().fields[2].value, "");
    assert_no_record_holds(pane.data.path(), SECRET);
}

/// `manifest` with the option "formal" left out of "Greet"'s tone.
fn without_formal(manifest: &serde_json::Value) -> serde_json::Value {
    let mut manifest = manifest.clone();
    let options = &mut manifest["commands"][0]["arguments"][2]["options"];
    let options = options.as_array_mut().unwrap();
    options.retain(|option| option["value"] != "formal" && option != "formal");
    manifest
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
    root_searchs_fields_refuse_a_blank_required_argument_and_run_with_the_values,
    hotkey_and_quick_slot_launches_ask_for_the_arguments,
    another_command_launch_asks_for_missing_arguments_and_a_background_one_is_refused,
    an_alias_and_a_space_fill_the_first_argument_and_a_fallback_the_query,
    the_last_dropdown_is_remembered_and_dropped_once_the_choice_is_gone,
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

/// A package folder holding one command: `manifest`'s entry, with
/// `component` as its component.
fn package_of(sources: &Path, name: &str, manifest: &str, component: &Path) -> PathBuf {
    let folder = sources.join(name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("pane.json"), manifest).unwrap();
    fs::copy(component, folder.join("command.wasm")).unwrap();
    folder
}

/// A compiled fixture under `target/guests`, from `cargo xtask guests`.
fn built(path: &str) -> PathBuf {
    let built = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests");
    let built = built.join(path);
    assert!(
        built.exists(),
        "{} is missing; run `cargo xtask guests`",
        built.display()
    );
    built
}

/// "Greet" as a command titled "Sum 0 + 0", matching the slow query by
/// title, so its inline fields show for the list the budget publishes.
fn sums(sources: &Path) -> PathBuf {
    package_of(
        sources,
        "sums",
        r#"{ "manifestVersion": 1, "title": "Sums", "apiVersion": "0.1",
  "commands": [{ "id": "greet", "title": "Sum 0 + 0",
    "component": "command.wasm", "mode": "no-view",
    "arguments": [{ "name": "name", "type": "text", "placeholder": "Name", "required": true }] }] }"#,
        &built("sample_arguments.wasm"),
    )
}

/// The slow fixture's package: a root provider that answers "0 + 0" after
/// about a second of busy work, merging into whatever the budget
/// published.
fn slow(sources: &Path) -> PathBuf {
    package_of(
        sources,
        "slow",
        r#"{ "manifestVersion": 1, "title": "Slow", "apiVersion": "0.1",
  "commands": [{ "id": "command", "title": "Slow answers",
    "component": "command.wasm", "rootResults": true }] }"#,
        &built("faulty.wasm"),
    )
}

/// Searches `query` on its own thread, reporting once the search has
/// answered: a query's future is not polled until it is awaited, and the
/// test advances the launcher's clock meanwhile.
fn searched(launcher: &Launcher, query: &str) -> std::sync::mpsc::Receiver<()> {
    let (sent, answered) = std::sync::mpsc::channel();
    let launcher = launcher.clone();
    let query = query.to_owned();
    std::thread::spawn(move || {
        block_on(launcher.set_query(&query));
        sent.send(()).unwrap();
    });
    answered
}

#[test]
fn the_typed_values_survive_the_list_being_rebuilt() {
    let sources = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    // The slow fixture computes for about a second, which a loaded machine
    // can stretch past the default computing limit.
    let runtime = Runtime::start().unwrap();
    runtime.set_limits(Limits {
        compute: Duration::from_secs(180),
        ..Limits::default()
    });
    let launcher = Launcher::with_packages(Ok(runtime), vec![], data.path().join("extensions"))
        .with_hotkeys(Arc::new(FakeHotkeys::default()))
        .with_quick_slots(data.path());
    let sums = sums(sources.path());
    let slow = slow(sources.path());
    for folder in [&sums, &slow] {
        block_on(launcher.install_package(folder));
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
    }
    let clock = ManualClock::at(1_000);
    let launcher = launcher.with_clock(clock.clone());

    // The query's list is held while the slow provider answers; the budget
    // publishes it with the arguments command's row selected, its field
    // given a value.
    let answered = searched(&launcher, "0 + 0");
    clock.advance(Duration::from_millis(200));
    assert_eq!(titles(&launcher), ["Sum 0 + 0"]);
    assert!(launcher.argument_fields().is_some());
    launcher.set_argument_value("name", "Ada");

    // The late answer re-ranks the list; selecting the same row again, the
    // value is kept, and the run carries it.
    answered.recv_timeout(Duration::from_secs(240)).unwrap();
    clock.advance(Duration::from_millis(16));
    assert_eq!(titles(&launcher), ["Slow answer", "Sum 0 + 0"]);
    select_title(&launcher, "Sum 0 + 0");
    assert_eq!(
        launcher.argument_fields().expect("the fields show").fields[0].value,
        "Ada"
    );
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(&launcher),
        Status::Result("Greet run 1 from root-search: name=Ada; fallback text: none".into())
    );
}

#[test]
fn an_alias_and_a_space_open_a_command_without_arguments_at_once() {
    let pane = Pane::new();
    let search = pane.source("sample-search", "search");
    let no_view = pane.source("sample-no-view", "no-view");
    block_on(pane.launcher.install_package(&search));
    block_on(pane.launcher.install_package(&no_view));
    let launcher = &pane.launcher;
    pane.alias(&search, "packages", "ps");
    pane.alias(&no_view, "last", "ls");

    // A view command that searches opens its screen at once, and what is
    // typed next is its own search's text. The space's flow needs the
    // space after the alias in the query; Tab's is the alias alone.
    pane.search("ps");
    assert_eq!(launcher.alias_after_tab(), Some(AliasFlow::Opens));
    pane.search("ps ");
    assert_eq!(launcher.alias_after_space(), Some(AliasFlow::Opens));
    block_on(launcher.activate_selected());
    assert!(matches!(
        launcher.view().screen,
        Screen::CommandSearch { query } if query.is_empty()
    ));
    block_on(launcher.set_query("hello"));
    assert!(matches!(
        launcher.view().screen,
        Screen::CommandSearch { query } if query == "hello"
    ));

    // A no-view command runs at once.
    pane.search("ls");
    assert_eq!(launcher.alias_after_space(), Some(AliasFlow::Opens));
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(launcher),
        Status::Result("Last report: none. Ticks: 0; last tick: none".into())
    );
}
