//! Preferences through the launcher's public interface (#143), with the
//! preferences sample in Rust, JavaScript and TypeScript, real guests
//! `cargo xtask guests` assembles: a package declares typed preferences in
//! `pane.json` for itself and for single commands; a command whose
//! required preferences are unset shows the Setup screen before it runs (only
//! the required, unset fields, with their descriptions and the package's
//! `HELP.md`; a declared default never triggers it, and a stored value that
//! no longer fits counts as unset), and submitting it launches the command
//! with its original launch record while cancelling launches nothing; every
//! other way in (a schedule, a background launch) does not run it, says
//! "Needs setup" and is no failure; the command receives its effective
//! values typed (the samples' no-view commands show them in a toast, #141);
//! and the values follow the rules of extension data: a password is a local
//! credential, disabling keeps them, an update keeps the still-declared ones
//! and drops a value whose type changed, and uninstalling keeps the others
//! when asked while always removing the credentials.
//! Prior art: `no_view.rs`, `schedules.rs`, `uninstall.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::executor::block_on;
use pane_core::clipboard::{Clock, ManualClock, SystemClock};
use pane_core::{
    FieldKind, Launcher, PackageIdentity, PreferenceKind, PreferencesTarget, ResultAction, Runtime,
    SavedData, Screen, Status,
};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{select_title, titles};

/// One language's preferences sample package.
struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    /// Its package's title.
    title: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-preferences",
    title: "Preferences sample",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-preferences-js",
    title: "JavaScript preferences sample",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-preferences-ts",
    title: "TypeScript preferences sample",
};

/// How long the scheduler and a launch may take: compiling the guest once
/// is included; a slow, busy machine is not.
const PROMPTLY: Duration = Duration::from_secs(30);

const MINUTE: Duration = Duration::from_secs(60);

/// The paragraphs of the sample's `HELP.md`, as the Setup screen shows
/// them.
const HELP: [&str; 3] = [
    "Setting up the preferences sample",
    "The API key is any text: the sample's service accepts every key. In a real extension, \
     say here where to find one, such as the account page of the service it uses.",
    "The notes folder is any folder on this computer. The notes file and the editor are \
     optional.",
];

/// The description of the API key preference.
const API_KEY: &str =
    "The key the sample's service gives you; the help beside says where to find it.";

/// One test's Pane: its folders, the clock its schedules follow, and the
/// launcher.
struct Pane {
    sources: TempDir,
    _data: TempDir,
    clock: Arc<ManualClock>,
    launcher: Launcher,
}

impl Pane {
    fn new() -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let clock = ManualClock::at(SystemClock.now() + 365 * 86_400_000);
        let launcher =
            Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
                .with_clock(clock.clone());
        Pane {
            sources,
            _data: data,
            clock,
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

    /// Installs the package in the source folder `folder`, titled `title`.
    fn install_from(&self, folder: &Path, title: &str) {
        block_on(self.launcher.install_package(folder));
        assert_eq!(
            self.launcher.view().status,
            Status::Result(format!("Installed {title}"))
        );
    }

    /// Installs `fixture`'s sample from the source folder `folder`.
    fn install(&self, fixture: &Fixture, folder: &str) -> PathBuf {
        let folder = self.source(fixture.package, folder);
        self.install_from(&folder, fixture.title);
        folder
    }

    /// A folder that exists, for the notes folder.
    fn notes(&self) -> String {
        let notes = self.sources.path().join("notes");
        fs::create_dir_all(&notes).unwrap();
        notes.to_str().unwrap().to_owned()
    }

    /// Root search, with `query` typed.
    fn search(&self, query: &str) {
        while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.launcher.back();
        }
        block_on(self.launcher.set_query(query));
    }

    /// Types `query` in root search, chooses the row titled `title` and
    /// waits for what it does.
    fn choose(&self, query: &str, title: &str) {
        self.search(query);
        select_title(&self.launcher, title);
        block_on(self.launcher.activate_selected());
    }

    /// Types `query` in root search, chooses the row titled `title` and
    /// waits for what it does; what it showed: its toast, or the status
    /// line.
    fn run(&self, query: &str, title: &str) -> Status {
        self.choose(query, title);
        shown(&self.launcher)
    }

    /// Types `query` in root search and chooses the row titled `title`,
    /// which opens a screen (the command's, or the Setup screen before it)
    /// and says nothing in the status line. A toast an earlier run showed
    /// may still be in the footer.
    fn open(&self, query: &str, title: &str) {
        self.choose(query, title);
        assert_eq!(self.launcher.view().status, Status::Idle, "{title}");
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

    /// Sets the preference kept as `key` of the package from `folder`, as
    /// its card in Settings does.
    fn set(&self, folder: &Path, key: &str, value: &str) {
        let identity = PackageIdentity::local(folder).unwrap();
        block_on(self.launcher.set_preference(&identity, key, Some(value)))
            .unwrap_or_else(|why| panic!("{key} = {value}: {why}"));
    }

    /// What "Last tick" of the package from `folder` shows: its toast, or
    /// the status line.
    fn last(&self, folder: &Path) -> Status {
        self.search("last tick");
        let wanted = id(folder, "last");
        let index = self
            .launcher
            .view()
            .rows
            .iter()
            .position(|row| row.id == wanted)
            .unwrap_or_else(|| panic!("no row {wanted} in {:?}", titles(&self.launcher)));
        self.launcher.select(index);
        block_on(self.launcher.activate_selected());
        shown(&self.launcher)
    }

    /// Whether root search, with `query` typed, says the row titled
    /// `title` needs setup.
    fn needs_setup(&self, query: &str, title: &str) -> bool {
        self.search(query);
        let (view, presentation) = self.launcher.presented_view();
        let index = view
            .rows
            .iter()
            .position(|row| row.title == title)
            .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(&self.launcher)));
        assert_eq!(view.rows[index].unavailable, None, "{title} is not paused");
        presentation.rows[index].needs_setup
    }

    /// The fields of the Setup screen on display: each's id, label,
    /// description and whether it is hidden as it is typed.
    fn setup_fields(&self) -> Vec<(String, String, Option<String>, bool)> {
        let view = self.launcher.view();
        let form = view
            .form()
            .unwrap_or_else(|| panic!("no Setup screen: {view:?}"));
        assert!(form.setup.is_some(), "a Setup screen");
        form.fields
            .iter()
            .map(|field| {
                (
                    field.id.clone(),
                    field.label.clone(),
                    field.description.clone(),
                    matches!(field.kind, FieldKind::Password { .. }),
                )
            })
            .collect()
    }
}

/// The command id of the command `command` of the package from `folder`.
fn id(folder: &Path, command: &str) -> String {
    format!(
        "{}#{command}",
        PackageIdentity::local(folder).unwrap().key()
    )
}

/// A Setup screen field, as [`Pane::setup_fields`] lists it.
fn field(
    id: &str,
    label: &str,
    description: &str,
    secret: bool,
) -> (String, String, Option<String>, bool) {
    (id.into(), label.into(), Some(description.into()), secret)
}

/// What "Report preferences" shows in its toast, launched from `source`,
/// with an API key of `key` characters and the rest as given.
fn report(source: &str, key: usize, units: &str, greeting: &str, verbose: bool) -> String {
    format!(
        "Report from {source}: API key of {key} characters; units: {units}; greeting: \
         {greeting}; verbose: {verbose}"
    )
}

fn the_setup_screen_asks_only_for_required_unset_values_then_launches(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture, "preferences");
    let launcher = &pane.launcher;
    let alias = launcher
        .set_alias(&id(&folder, "report"), "rp")
        .expect("the alias is accepted");
    block_on(alias);

    // Its alias shows the Setup screen: only the API key, the required
    // value with no default; the units have one, and its own "Loud" too.
    assert_eq!(pane.send("rp"), Status::Idle);
    assert_eq!(
        pane.setup_fields(),
        [field("apiKey", "API key", API_KEY, true)]
    );
    let view = launcher.view();
    assert_eq!(view.title, fixture.title);
    let setup = view.form().unwrap().setup.clone().unwrap();
    assert_eq!(
        setup.sentence,
        "Set these up before using Report preferences"
    );
    assert_eq!(setup.help, HELP);
    assert_eq!(
        setup.package,
        PackageIdentity::local(&folder).unwrap().key()
    );

    // Cancelling launches nothing.
    assert!(launcher.back());
    let view = launcher.view();
    assert_eq!(view.screen, Screen::Root { query: "rp".into() });
    assert_eq!(shown(launcher), Status::Idle, "nothing ran");

    // Submitting saves the value and launches it with its original launch
    // record: from its alias.
    assert_eq!(pane.send("rp"), Status::Idle);
    block_on(launcher.submit_form());
    assert_eq!(
        launcher.view().form().unwrap().fields[0].error.as_deref(),
        Some("Required"),
        "an empty value is not saved"
    );
    launcher.set_field_value("apiKey", "abc");
    block_on(launcher.submit_form());
    let view = launcher.view();
    assert_eq!(view.screen, Screen::Root { query: "rp".into() });
    assert_eq!(
        shown(launcher),
        Status::Result(report("alias", 3, "metric", "none", false))
    );

    // Another command asks only for what is still unset: its own folder.
    pane.open("show preferences", "Show preferences");
    assert_eq!(
        pane.setup_fields(),
        [field(
            "show#folder",
            "Notes folder",
            "Where the notes are kept.",
            false
        )]
    );
    // A folder that does not exist is no value.
    let gone = pane.sources.path().join("gone");
    launcher.set_field_value("show#folder", gone.to_str().unwrap());
    block_on(launcher.submit_form());
    assert_eq!(
        launcher.view().form().unwrap().fields[0].error.as_deref(),
        Some("No folder has this path")
    );
    let notes = pane.notes();
    launcher.set_field_value("show#folder", &notes);
    block_on(launcher.submit_form());
    let view = launcher.view();
    assert_eq!(view.screen, Screen::Command);
    assert_eq!(view.title, "Preferences");
    assert_eq!(
        titles(launcher),
        [
            "API key: 3 characters".to_owned(),
            "Units: metric".into(),
            "Greeting: none".into(),
            "Verbose: false".into(),
            format!("Notes folder: {notes}"),
            "Notes file: none".into(),
            "Editor: none".into(),
        ]
    );

    // Once set up, it opens at once.
    pane.open("show preferences", "Show preferences");
    assert_eq!(launcher.view().screen, Screen::Command);

    // A stored value that no longer fits counts as unset: the folder went.
    fs::remove_dir_all(&notes).unwrap();
    pane.open("show preferences", "Show preferences");
    assert_eq!(
        pane.setup_fields(),
        [field(
            "show#folder",
            "Notes folder",
            "Where the notes are kept.",
            false
        )]
    );
}

fn the_command_receives_its_typed_effective_values(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture, "preferences");
    let launcher = &pane.launcher;
    let identity = PackageIdentity::local(&folder).unwrap();

    // The card lists the package's preferences, then each command's.
    let card = launcher
        .preferences_of(&identity)
        .expect("it declares some");
    assert_eq!(card.title, fixture.title);
    let keys: Vec<(&str, PreferenceKind, bool)> = card
        .fields
        .iter()
        .map(|field| (field.key.as_str(), field.preference.kind, field.missing))
        .collect();
    assert_eq!(
        keys,
        [
            ("apiKey", PreferenceKind::Password, true),
            ("units", PreferenceKind::Dropdown, false),
            ("greeting", PreferenceKind::Text, false),
            ("verbose", PreferenceKind::Checkbox, false),
        ]
    );
    let commands: Vec<(&str, Vec<&str>)> = card
        .commands
        .iter()
        .map(|command| {
            let keys = command.fields.iter().map(|field| field.key.as_str());
            (command.title.as_str(), keys.collect())
        })
        .collect();
    assert_eq!(
        commands,
        [
            (
                "Show preferences",
                vec!["show#folder", "show#notes", "show#editor"]
            ),
            ("Report preferences", vec!["report#loud"]),
        ]
    );

    // Values the card sets reach the command, typed, the defaults for the
    // others.
    pane.set(&folder, "apiKey", "secret");
    assert_eq!(
        pane.run("report preferences", "Report preferences"),
        Status::Result(report("root-search", 6, "metric", "none", false))
    );
    pane.set(&folder, "units", "imperial");
    pane.set(&folder, "greeting", "Hi");
    pane.set(&folder, "verbose", "true");
    pane.set(&folder, "report#loud", "true");
    assert_eq!(
        pane.run("report preferences", "Report preferences"),
        Status::Result(report("root-search", 6, "imperial", "Hi", true).to_uppercase())
    );
    let notes = pane.notes();
    let file = Path::new(&notes).join("notes.txt");
    fs::write(&file, "notes").unwrap();
    let file = file.to_str().unwrap().to_owned();
    pane.set(&folder, "show#folder", &notes);
    pane.set(&folder, "show#notes", &file);
    pane.set(&folder, "show#editor", "notepad");
    pane.open("show preferences", "Show preferences");
    assert_eq!(launcher.view().screen, Screen::Command);
    assert_eq!(
        titles(launcher),
        [
            "API key: 6 characters".to_owned(),
            "Units: imperial".into(),
            "Greeting: Hi".into(),
            "Verbose: true".into(),
            format!("Notes folder: {notes}"),
            format!("Notes file: {file}"),
            "Editor: notepad".into(),
        ]
    );

    // A value that does not fit is refused, and changes nothing.
    let refused = block_on(launcher.set_preference(&identity, "units", Some("kelvin")));
    assert_eq!(
        refused,
        Err("\"kelvin\" is not one of the options of Units".into())
    );
    let refused = block_on(launcher.set_preference(&identity, "verbose", Some("yes")));
    assert!(refused.unwrap_err().contains("is a checkbox"));
    let refused = block_on(launcher.set_preference(&identity, "nothing", Some("x")));
    assert!(refused.unwrap_err().contains("has no preference `nothing`"));

    // Removing a required value makes it unset again.
    block_on(launcher.set_preference(&identity, "apiKey", None)).unwrap();
    let card = launcher.preferences_of(&identity).unwrap();
    assert!(card.fields[0].missing);
    assert_eq!(card.fields[0].value, None);
    pane.open("report preferences", "Report preferences");
    assert!(launcher.view().form().is_some(), "the Setup screen");
}

fn other_ways_in_do_not_run_it_and_say_needs_setup(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture, "preferences");
    // A package of another language's sample launches its command in the
    // background.
    let launcher_folder = pane.source("sample-no-view", "no-view");
    pane.install_from(&launcher_folder, "No-view sample");
    let alias = pane
        .launcher
        .set_alias(&id(&launcher_folder, "launch"), "ln")
        .expect("the alias is accepted");
    block_on(alias);
    assert!(pane.launcher.wait_for_schedules(PROMPTLY));

    // Its schedule comes due, twice: it does not run.
    pane.clock.advance(MINUTE);
    assert!(pane.launcher.wait_for_schedules(PROMPTLY));
    pane.clock.advance(MINUTE);
    assert!(pane.launcher.wait_for_schedules(PROMPTLY));
    // Another command's background launch does not run it either, and the
    // launch is no refusal.
    let key = PackageIdentity::local(&folder).unwrap().key();
    assert_eq!(
        pane.send(&format!("ln background {key}#tick")),
        Status::Result(format!("Launched {key}#tick in the background"))
    );
    assert!(pane.launcher.wait_for_launches(PROMPTLY));

    // Its rows say so, and none is paused (none of it is a failure).
    assert!(pane.needs_setup("tick", "Tick"));
    assert!(pane.needs_setup("show preferences", "Show preferences"));
    assert!(!pane.needs_setup("report launch", "Report launch"));

    // Once set up, nothing had run, and the schedule runs from then on.
    pane.set(&folder, "apiKey", "abc");
    assert!(!pane.needs_setup("tick", "Tick"));
    assert!(
        pane.needs_setup("show preferences", "Show preferences"),
        "its own folder is still unset"
    );
    assert_eq!(pane.last(&folder), Status::Result("Ticks: 0".into()));
    assert!(pane.launcher.wait_for_schedules(PROMPTLY));
    pane.clock.advance(MINUTE);
    assert!(pane.launcher.wait_for_schedules(PROMPTLY));
    assert_eq!(pane.last(&folder), Status::Result("Ticks: 1".into()));
}

fn the_values_follow_the_rules_of_extension_data(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture, "preferences");
    let launcher = &pane.launcher;
    let identity = PackageIdentity::local(&folder).unwrap();
    let notes = pane.notes();
    pane.set(&folder, "apiKey", "abc");
    pane.set(&folder, "units", "imperial");
    pane.set(&folder, "greeting", "Hi");
    pane.set(&folder, "verbose", "true");
    pane.set(&folder, "show#folder", &notes);
    let value = |key: &str| {
        let card = launcher.preferences_of(&identity).unwrap();
        card.fields
            .iter()
            .chain(card.commands.iter().flat_map(|command| &command.fields))
            .find(|field| field.key == key)
            .map(|field| field.value.clone())
    };

    // Disabling keeps them.
    block_on(launcher.set_enabled(&identity, false));
    block_on(launcher.set_enabled(&identity, true));
    assert_eq!(value("apiKey"), Some(Some("abc".into())));
    assert_eq!(value("greeting"), Some(Some("Hi".into())));

    // An update keeps the values still declared; one whose type changed so
    // that it no longer fits goes, as does one no longer declared.
    let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    manifest["version"] = "0.2.0".into();
    let preferences = manifest["preferences"].as_array_mut().unwrap();
    preferences.retain(|preference| preference["name"] != "verbose");
    preferences[2]["type"] = "checkbox".into();
    fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
    block_on(launcher.reload(&identity));
    assert_eq!(
        launcher.view().status,
        Status::Result(format!("Reloaded {}", fixture.title))
    );
    assert_eq!(value("apiKey"), Some(Some("abc".into())));
    assert_eq!(value("units"), Some(Some("imperial".into())));
    assert_eq!(value("show#folder"), Some(Some(notes.clone())));
    assert_eq!(
        value("greeting"),
        Some(None),
        "a checkbox's value is not Hi"
    );
    assert_eq!(value("verbose"), None, "no longer declared");

    // Uninstalling keeping the saved data keeps the settings, but never the
    // password, a local credential.
    block_on(launcher.uninstall(&identity, SavedData::Keep));
    pane.install_from(&folder, fixture.title);
    assert_eq!(value("apiKey"), Some(None));
    assert_eq!(value("units"), Some(Some("imperial".into())));
    assert_eq!(value("show#folder"), Some(Some(notes)));

    // Deleting the saved data deletes them.
    block_on(launcher.uninstall(&identity, SavedData::Delete));
    pane.install_from(&folder, fixture.title);
    assert_eq!(value("units"), Some(None));
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
    the_setup_screen_asks_only_for_required_unset_values_then_launches,
    the_command_receives_its_typed_effective_values,
    other_ways_in_do_not_run_it_and_say_needs_setup,
    the_values_follow_the_rules_of_extension_data,
);

/// Installs the Rust sample with its `pane.json` changed by `change`;
/// the status line.
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

/// The error an install answered with; panics if it installed.
fn refused(change: impl FnOnce(&mut serde_json::Value)) -> String {
    let (pane, status) = install_changed(change);
    let Status::Error(error) = status else {
        panic!("installed: {status:?}");
    };
    assert!(pane.launcher.packages().is_empty());
    error
}

#[test]
fn every_type_is_accepted_and_invalid_declarations_are_refused_at_install() {
    let (pane, status) = install_changed(|_| {});
    assert_eq!(
        status,
        Status::Result("Installed Preferences sample".into())
    );
    let packages = pane.launcher.packages();
    let manifest = packages[0].manifest.as_ref().unwrap();
    let mut kinds: Vec<PreferenceKind> = manifest
        .preferences
        .iter()
        .chain(manifest.commands.iter().flat_map(|c| &c.preferences))
        .map(|preference| preference.kind)
        .collect();
    kinds.sort_by_key(|kind| kind.id());
    kinds.dedup();
    let mut every = PreferenceKind::ALL.to_vec();
    every.sort_by_key(|kind| kind.id());
    assert_eq!(kinds, every, "the sample declares every type");

    let error = refused(|manifest| {
        manifest["commands"][0]["preferences"][0]["name"] = "apiKey".into();
    });
    assert!(
        error.contains(
            "preference `apiKey` of command `show` is declared twice; a preference's `name` is \
             unique among the package's preferences and each command's together"
        ),
        "{error}"
    );
    let error = refused(|manifest| {
        manifest["preferences"][2]["type"] = "colour".into();
    });
    assert!(
        error.contains("preference `greeting` of the package has the type \"colour\""),
        "{error}"
    );
    let error = refused(|manifest| {
        manifest["preferences"][1]["default"] = "kelvin".into();
    });
    assert!(
        error.contains(
            "the default \"kelvin\" of preference `units` of the package is not among its options"
        ),
        "{error}"
    );
}

#[test]
fn configure_entries_appear_only_where_preferences_are_declared() {
    let pane = Pane::new();
    let folder = pane.install(&RUST, "preferences");
    let other = pane.source("sample-no-view", "no-view");
    pane.install_from(&other, "No-view sample");
    let launcher = &pane.launcher;
    let configuration = |query: &str, title: &str| {
        pane.search(query);
        select_title(launcher, title);
        let actions = launcher.result_actions().expect("a selected row");
        actions
            .items
            .iter()
            .filter(|item| {
                matches!(
                    item.action,
                    ResultAction::ConfigureCommand | ResultAction::ConfigureExtension
                )
            })
            .map(|item| item.label.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        configuration("show preferences", "Show preferences"),
        ["Configure Command…", "Configure Extension…"]
    );
    assert_eq!(configuration("tick", "Tick"), ["Configure Extension…"]);
    assert!(configuration("report launch", "Report launch").is_empty());

    // They name the card to open, and are ready.
    pane.search("show preferences");
    select_title(launcher, "Show preferences");
    let row = id(&folder, "show");
    assert!(launcher.result_action_ready(&row, ResultAction::ConfigureCommand));
    assert_eq!(
        launcher.preferences_target(&row),
        Some(PreferencesTarget {
            identity: PackageIdentity::local(&folder).unwrap(),
            command: "show".into(),
        })
    );
    assert_eq!(launcher.preferences_target(&id(&other, "report")), None);
}
