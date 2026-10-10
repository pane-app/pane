//! Root search's recent queries (#206) through the launcher's public
//! interface: a query cleared while not blank — by Escape, by the field
//! being emptied, by a command's opening taking it, or by root search
//! being shown fresh over it — is recorded with the argument values
//! typed with it, a password's value empty and its text never written;
//! consecutive duplicates are not added, at most 64 entries are kept;
//! the restore seam brings a query and its values back, walking further
//! back by entry; the "Learn from what I choose" switch stops recording
//! (#200: one switch for both); the Launcher page's reset clears
//! everything; the record survives a restart over the same data folder,
//! and an uninstall forgets the entries that carry its commands' values;
//! and an unreadable record is reported and never replaced. The
//! arguments are the Rust arguments sample's (a no-view "Greet" command
//! that declares a required name, a secret and a tone, from
//! `cargo xtask guests`); the command that opens, for a query taken by
//! its opening, is a registered view sample.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::executor::block_on;
use pane_core::{CommandRegistration, Launcher, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/guests.rs"]
mod guests;

use guests::guest;

/// How long recording a query may take: a thread writes it.
const RECORDED: Duration = Duration::from_secs(30);

/// A password typed into the fields, which no record of Pane's may hold.
const SECRET: &str = "hunter2-Zq9-never-kept";

/// A registered view command, which opens when invoked: a query typed
/// toward it is taken by its opening, since returning to root starts
/// empty.
fn view_command() -> CommandRegistration {
    CommandRegistration {
        id: "sample".into(),
        title: "Sample command".into(),
        subtitle: None,
        component: guest("sample_rust"),
        takes_query: false,
        search: false,
        keywords: Vec::new(),
        when: pane_core::CommandWhen::Always,
        matches: pane_core::CommandMatches::Title,
    }
}

/// A launcher keeping its records in `data`, as Pane does each time it
/// starts, with the view command registered.
fn launcher_in(data: &Path) -> Launcher {
    Launcher::with_packages(
        Runtime::start(),
        vec![view_command()],
        data.join("extensions"),
    )
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

    /// The assembled Rust arguments sample, copied into the source
    /// folder.
    fn source(&self) -> PathBuf {
        let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages")
            .join("sample-arguments");
        assert!(
            assembled.exists(),
            "{} is missing; run `cargo xtask guests`",
            assembled.display()
        );
        let folder = self.sources.path().join("arguments");
        fs::create_dir_all(&folder).unwrap();
        for entry in fs::read_dir(&assembled).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
        }
        folder
    }

    /// Installs the arguments sample; its identity.
    fn install(&self) -> pane_core::PackageIdentity {
        let folder = self.source();
        block_on(self.launcher.install_package(&folder));
        assert!(
            matches!(self.launcher.view().status, Status::Result(_)),
            "{:?}",
            self.launcher.view().status
        );
        self.launcher.back();
        pane_core::PackageIdentity::local(&folder).unwrap()
    }

    /// Where the search history is recorded.
    fn record(&self) -> PathBuf {
        self.data.path().join("extensions/search-history.json")
    }
}

/// Root search, with `query` typed.
fn search(launcher: &Launcher, query: &str) {
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
    block_on(launcher.set_query(query));
}

/// Types `query` into root search and fills the selected row's argument
/// fields as the window's fields do, then clears the query the way
/// `clearing` says: `escape` backs out of it, `empty` empties the field,
/// `fresh` shows root search over it. Waits for what was recorded.
fn type_and_clear(pane: &Pane, query: &str, values: &[(&str, &str)], clearing: &str) {
    let launcher = &pane.launcher;
    search(launcher, query);
    for (name, value) in values {
        launcher.set_argument_value(name, value);
    }
    match clearing {
        "escape" => assert!(launcher.back(), "{query} was cleared by Escape"),
        "empty" => block_on(launcher.set_query("")),
        "fresh" => launcher.show_root_search(),
        other => panic!("unknown clearing {other:?}"),
    }
    assert!(
        launcher.wait_for_history_recorded(RECORDED),
        "the query was recorded"
    );
}

#[test]
fn a_query_cleared_by_each_path_is_recorded_with_its_argument_values() {
    for clearing in ["escape", "empty", "fresh"] {
        let pane = Pane::new();
        pane.install();
        type_and_clear(
            &pane,
            "greet",
            &[("name", "Ada"), ("secret", SECRET)],
            clearing,
        );
        let launcher = &pane.launcher;
        assert_eq!(
            launcher.recent_query(0).as_deref(),
            Some("greet"),
            "cleared by {clearing}"
        );
        // The values the entry was recorded with come back with the
        // query: the name typed, the password empty as it was recorded.
        let query = launcher.recent_query(0).expect("the most recent");
        search(launcher, &query);
        launcher.restore_recent_arguments(0);
        let fields = launcher.argument_fields().expect("the fields show");
        assert_eq!(fields.fields[0].value, "Ada", "cleared by {clearing}");
        assert_eq!(
            fields.fields[1].value, "",
            "the password is empty, cleared by {clearing}"
        );
        // The record's text never holds the password.
        let record = fs::read_to_string(pane.record()).unwrap();
        assert!(
            !record.contains(SECRET),
            "cleared by {clearing}: the password is never recorded: {record}"
        );
        assert!(
            record.contains("\"secret\": \"\""),
            "cleared by {clearing}: the password is recorded empty: {record}"
        );
    }
}

#[test]
fn a_query_taken_by_a_commands_opening_is_recorded() {
    let pane = Pane::new();
    pane.install();
    // A query typed toward a command that opens is taken by its opening:
    // returning to root starts empty, so the query is recorded then,
    // with nothing to restore it but Up.
    search(&pane.launcher, "sample");
    block_on(pane.launcher.activate_selected());
    assert!(
        matches!(pane.launcher.view().screen, Screen::Command),
        "the view command opened: {:?}",
        pane.launcher.view().screen
    );
    assert!(
        pane.launcher.wait_for_history_recorded(RECORDED),
        "the query was recorded"
    );
    pane.launcher.back();
    assert_eq!(pane.launcher.view().screen, Screen::Root { query: "".into() });
    assert_eq!(
        pane.launcher.recent_query(0).as_deref(),
        Some("sample"),
        "the query was recorded when the command opened"
    );
}

#[test]
fn a_query_without_values_is_recorded_without_a_command_and_a_blank_one_is_not() {
    let pane = Pane::new();
    pane.install();
    type_and_clear(&pane, "greet", &[], "escape");
    let record = fs::read_to_string(pane.record()).unwrap();
    assert!(
        !record.contains("command"),
        "an entry without values carries no command: {record}"
    );
    // A blank query is never recorded: there is nothing to recall.
    type_and_clear(&pane, "   ", &[], "escape");
    assert_eq!(
        pane.launcher.recent_query(0).as_deref(),
        Some("greet"),
        "a blank query is not recorded"
    );
}

#[test]
fn consecutive_duplicates_are_not_added_and_at_most_64_entries_are_kept() {
    let pane = Pane::new();
    pane.install();
    // Three queries cleared, the middle one twice in a row: the repeat
    // is not added, and a query that is not one is entered again.
    type_and_clear(&pane, "one", &[], "escape");
    type_and_clear(&pane, "two", &[], "escape");
    type_and_clear(&pane, "two", &[], "escape");
    type_and_clear(&pane, "one", &[], "escape");
    let launcher = &pane.launcher;
    assert_eq!(launcher.recent_query(0).as_deref(), Some("one"));
    assert_eq!(launcher.recent_query(1).as_deref(), Some("two"));
    assert_eq!(launcher.recent_query(2).as_deref(), None);

    // The 64 entries kept, newest first: the oldest beyond them goes
    // when one is added.
    for query in 0..=64 {
        type_and_clear(&pane, &format!("q{query}"), &[], "empty");
    }
    assert_eq!(launcher.recent_query(0).as_deref(), Some("q64"));
    assert_eq!(launcher.recent_query(63).as_deref(), Some("q1"));
    assert_eq!(launcher.recent_query(64).as_deref(), None);
}

#[test]
fn the_walk_restores_the_query_and_its_values_and_goes_one_back_each_time() {
    let pane = Pane::new();
    pane.install();
    // Three searches recorded, the newest first: the second carries the
    // argument values typed with it.
    type_and_clear(&pane, "third", &[], "escape");
    type_and_clear(&pane, "greet", &[("name", "Ada")], "escape");
    type_and_clear(&pane, "first", &[], "escape");
    let launcher = &pane.launcher;

    // Up's walk, as the window drives it: the most recent query, its
    // values with it, then one entry back each time, and nothing past
    // the oldest.
    let query = launcher.recent_query(0).expect("the most recent");
    assert_eq!(query, "first");
    search(launcher, &query);
    launcher.restore_recent_arguments(0);
    let query = launcher.recent_query(1).expect("the one before");
    assert_eq!(query, "greet");
    search(launcher, &query);
    launcher.restore_recent_arguments(1);
    let fields = launcher.argument_fields().expect("the fields show");
    assert_eq!(fields.fields[0].value, "Ada");
    assert_eq!(launcher.recent_query(2).as_deref(), Some("third"));
    assert_eq!(launcher.recent_query(3).as_deref(), None);
    // Restoring past the last does nothing: the values stand.
    launcher.restore_recent_arguments(3);
    assert_eq!(
        launcher
            .argument_fields()
            .expect("the fields still show")
            .fields[0]
            .value,
        "Ada"
    );
}

#[test]
fn the_learn_switch_stops_recording_queries() {
    let pane = Pane::new();
    pane.install();
    type_and_clear(&pane, "greet", &[("name", "Ada")], "escape");
    let launcher = &pane.launcher;

    // The same switch that stops the ranking learning (#200: one switch
    // for both) stops the history: nothing is recorded while it is off,
    // and what was recorded is kept.
    launcher.set_learning(false);
    type_and_clear(&pane, "stamp", &[], "escape");
    assert_eq!(
        launcher.recent_query(0).as_deref(),
        Some("greet"),
        "nothing was recorded while the switch was off"
    );
    let record = fs::read_to_string(pane.record()).unwrap();
    assert!(!record.contains("stamp"), "nothing was written: {record}");

    // Turned on again, the walk still reaches what was kept.
    launcher.set_learning(true);
    assert_eq!(launcher.recent_query(0).as_deref(), Some("greet"));
}

#[test]
fn the_page_reset_clears_every_entry() {
    let pane = Pane::new();
    pane.install();
    type_and_clear(&pane, "greet", &[("name", "Ada")], "escape");
    type_and_clear(&pane, "stamp", &[], "escape");
    let launcher = &pane.launcher;

    let (ran, recorded) = launcher.reset_search_history();
    assert!(ran, "there was something to reset");
    block_on(recorded).unwrap();
    assert_eq!(launcher.recent_query(0), None);
    let record = fs::read_to_string(pane.record()).unwrap();
    assert!(
        record.contains("\"queries\": []"),
        "the record holds nothing: {record}"
    );

    // Nothing is recalled after a restart either.
    let pane = pane.restart();
    assert_eq!(pane.launcher.recent_query(0), None);
}

#[test]
fn the_record_survives_a_restart_over_the_same_data_folder() {
    let pane = Pane::new();
    pane.install();
    type_and_clear(
        &pane,
        "greet",
        &[("name", "Ada"), ("secret", SECRET)],
        "escape",
    );

    let pane = pane.restart();
    let launcher = &pane.launcher;
    assert_eq!(launcher.recent_query(0).as_deref(), Some("greet"));
    let record = fs::read_to_string(pane.record()).unwrap();
    assert!(
        !record.contains(SECRET),
        "the password is never recorded: {record}"
    );
    // The values come back with the query, the password empty.
    search(launcher, "greet");
    launcher.restore_recent_arguments(0);
    let fields = launcher.argument_fields().expect("the fields show");
    assert_eq!(fields.fields[0].value, "Ada");
    assert_eq!(fields.fields[1].value, "");
}

#[test]
fn uninstalling_a_package_forgets_the_entries_that_carry_its_values() {
    let pane = Pane::new();
    let identity = pane.install();
    type_and_clear(&pane, "greet", &[("name", "Ada")], "escape");
    type_and_clear(&pane, "sample", &[], "escape");
    let launcher = &pane.launcher;

    // The entry that carries the package's command's values is the
    // package's: it is forgotten when the package is uninstalled. A
    // query that carried no values is no package's, and stays.
    block_on(launcher.uninstall(&identity, pane_core::SavedData::Keep));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
    launcher.back();
    assert_eq!(
        launcher.recent_query(0).as_deref(),
        Some("sample"),
        "the plain query stayed"
    );
    assert_eq!(
        launcher.recent_query(1).as_deref(),
        None,
        "the entry that carried the package's values went"
    );
}

#[test]
fn an_unreadable_record_is_reported_and_never_replaced() {
    for garbage in ["{ not a record", r#"{ "version": 2, "queries": [] }"#] {
        let pane = Pane::new();
        fs::create_dir_all(pane.data.path().join("extensions")).unwrap();
        fs::write(pane.record(), garbage).unwrap();
        let pane = pane.restart();
        let launcher = &pane.launcher;
        assert!(
            launcher.history_problem().is_some(),
            "{garbage} is reported"
        );
        assert!(
            matches!(launcher.view().status, Status::Error(problem)
                if problem.starts_with("Pane could not read the search history")),
            "{garbage}: {:?}",
            launcher.view().status
        );
        // Up recalls nothing, and the reset cannot run.
        assert_eq!(launcher.recent_query(0), None);
        let (ran, recorded) = launcher.reset_search_history();
        assert!(!ran, "nothing is offered while it cannot be read");
        assert!(block_on(recorded).is_err(), "{garbage} is not replaced");

        // A cleared query is recorded nowhere, and the record stays as
        // it was.
        pane.install();
        type_and_clear(&pane, "greet", &[("name", "Ada")], "escape");
        assert_eq!(
            fs::read_to_string(pane.record()).unwrap(),
            garbage,
            "{garbage}: the record stays as it was"
        );
    }
}
