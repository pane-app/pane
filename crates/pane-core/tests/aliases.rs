//! Aliases and fallbacks through the launcher's public interface: the user
//! gives an installed command an alias, or makes it a fallback, in Manage
//! extensions, and reaches it from root search, where the text typed is sent
//! to a command that takes a query only when the user invokes it, as its
//! launch record's fallback text. The command is a real guest: Echo, the
//! query sample from `cargo xtask guests`, a no-view command, in Rust,
//! JavaScript and TypeScript alike.

use std::fs;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::titles;

const MANAGE_ROW: &str = "Manage Extensions";

struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    component: &'static str,
    title: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-query",
    component: "sample_query.wasm",
    title: "Query sample",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-query-js",
    component: "sample_query_js.wasm",
    title: "JavaScript query sample",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-query-ts",
    component: "sample_query_ts.wasm",
    title: "TypeScript query sample",
};

/// Copies the assembled package `name` under `target/guests/packages` to
/// `folder`.
fn package(name: &str, folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(name);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    for entry in fs::read_dir(&assembled).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
    }
    folder.to_path_buf()
}

struct Dirs {
    sources: TempDir,
    data: TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
        }
    }

    fn packages_dir(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    fn aliases_file(&self) -> PathBuf {
        self.packages_dir().join("aliases.json")
    }

    /// A launcher on this data folder with its own runtime, returned too;
    /// a new one is a restart of Pane.
    fn launcher(&self) -> (Launcher, Runtime) {
        let runtime = Runtime::start().unwrap();
        let launcher = Launcher::with_packages(Ok(runtime.clone()), vec![], self.packages_dir());
        (launcher, runtime)
    }

    /// Copies the package `name` to the source folder `folder` (under the
    /// sources).
    fn source(&self, name: &str, folder: &str) -> PathBuf {
        package(name, &self.sources.path().join(folder))
    }

    /// Installs the package `name` from the source folder `folder` (under
    /// the sources).
    fn install(&self, launcher: &Launcher, name: &str, folder: &str) -> PathBuf {
        let folder = self.source(name, folder);
        install_from(launcher, &folder);
        folder
    }
}

fn install_from(launcher: &Launcher, folder: &Path) {
    block_on(launcher.install_package(folder));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
}

fn selected_title(launcher: &Launcher) -> Option<String> {
    let view = launcher.view();
    view.selected.map(|index| view.rows[index].title.clone())
}

fn subtitle(launcher: &Launcher, index: usize) -> String {
    launcher.view().rows[index]
        .subtitle
        .clone()
        .unwrap_or_default()
}

fn row_subtitle(launcher: &Launcher, title: &str) -> String {
    let view = launcher.view();
    let row = view
        .rows
        .iter()
        .find(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)));
    row.subtitle.clone().unwrap_or_default()
}

fn activate(launcher: &Launcher, title: &str) {
    let index = titles(launcher)
        .iter()
        .position(|row| row == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)));
    launcher.select(index);
    block_on(launcher.activate_selected());
}

/// Selects the row titled `title` whose subtitle ends with `ending`, such as
/// one copy's source.
fn select_ending(launcher: &Launcher, title: &str, ending: &str) {
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| {
            row.title == title && row.subtitle.as_deref().unwrap_or("").ends_with(ending)
        })
        .unwrap_or_else(|| panic!("no row {title:?} ending {ending:?}"));
    launcher.select(index);
}

fn to_root(launcher: &Launcher) {
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
    block_on(launcher.set_query(""));
}

/// Opens the extension manager from root search.
fn manage(launcher: &Launcher) {
    to_root(launcher);
    activate(launcher, MANAGE_ROW);
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
}

/// Submits `alias` in the open alias form; returns the status.
fn submit_alias(launcher: &Launcher, alias: &str) -> Status {
    let form = launcher.view().form().cloned().expect("the alias form");
    launcher.set_field_value(&form.fields[0].id, alias);
    block_on(launcher.submit_form());
    launcher.view().status
}

/// In Manage extensions, submits `alias` in the alias form of the row
/// titled `row` (such as "Alias for Echo"); returns the status.
fn set_alias_at(launcher: &Launcher, row: &str, alias: &str) -> Status {
    manage(launcher);
    activate(launcher, row);
    submit_alias(launcher, alias)
}

fn set_alias(launcher: &Launcher, alias: &str) -> Status {
    set_alias_at(launcher, "Alias for Echo", alias)
}

fn toggle_fallback(launcher: &Launcher) -> Status {
    manage(launcher);
    activate(launcher, "Fallback: Echo");
    launcher.view().status
}

fn search(launcher: &Launcher, query: &str) {
    to_root(launcher);
    block_on(launcher.set_query(query));
}

fn running(runtime: &Runtime) -> Vec<PathBuf> {
    block_on(runtime.running())
}

/// The identity of the package in `folder`, as Pane resolves the folder (the
/// temporary folder's own path differs on macOS and Windows).
fn identity_of(folder: &Path) -> PackageIdentity {
    PackageIdentity::local(folder).unwrap()
}

/// The component `component` of the installed package from `folder`.
fn component_of(launcher: &Launcher, folder: &Path, component: &str) -> PathBuf {
    let package = launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == identity_of(folder))
        .expect("installed");
    package.location.join(component)
}

/// The identity, as Manage extensions names the source, of the package
/// installed from `folder`.
fn source_of(launcher: &Launcher, folder: &Path) -> String {
    launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == identity_of(folder))
        .expect("installed")
        .identity
        .to_string()
}

fn an_alias_finds_the_command_first_and_sends_the_text_after_it_only_when_invoked(
    fixture: &Fixture,
) {
    let dirs = Dirs::new();
    let (launcher, runtime) = dirs.launcher();
    let query = dirs.install(&launcher, fixture.package, "query");
    dirs.install(&launcher, "calculator", "calculator");
    let echo = component_of(&launcher, &query, fixture.component);

    assert_eq!(
        set_alias(&launcher, "ec"),
        Status::Result("Typing “ec” now finds Echo".into())
    );
    assert!(row_subtitle(&launcher, "Alias for Echo").starts_with("“ec” · local folder "));
    let recorded = fs::read_to_string(dirs.aliases_file()).unwrap();
    assert!(recorded.contains("#echo\": \"ec\""), "{recorded}");

    // The alias alone lists the command first; nothing runs.
    search(&launcher, "EC");
    assert_eq!(selected_title(&launcher).as_deref(), Some("Echo"));
    assert_eq!(launcher.view().selected, Some(0));
    assert!(!running(&runtime).contains(&echo));
    // Root search presents the row with its alias, as a command.
    let presented = &launcher.presentation().rows[0];
    assert_eq!(presented.alias.as_deref(), Some("ec"));
    assert_eq!(presented.kind, Some(pane_core::RowKind::Command));

    // The alias and text list a row that sends the text; still nothing
    // runs while typing.
    search(&launcher, "ec hello  world ");
    assert_eq!(titles(&launcher)[0], "Echo");
    assert_eq!(subtitle(&launcher, 0), "Send “hello  world” · alias ec");
    assert_eq!(launcher.view().selected, Some(0));
    assert!(!running(&runtime).contains(&echo));

    // Invoking it sends the text and its toast shows the answer; root
    // search stays.
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(&launcher),
        Status::Result("Echo heard “hello  world”".into())
    );
    assert_eq!(launcher.view().query(), Some("ec hello  world "));

    // An error is shown as a failure toast.
    search(&launcher, "ec fail");
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(&launcher),
        Status::Error(
            "The extension reported an error: Echo refuses “fail”, to show how an error looks"
                .into()
        )
    );

    // Kept after a restart.
    drop(launcher);
    let (launcher, _runtime) = dirs.launcher();
    search(&launcher, "ec again");
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(&launcher),
        Status::Result("Echo heard “again”".into())
    );

    // Removed by leaving the field empty; its form starts with it.
    manage(&launcher);
    activate(&launcher, "Alias for Echo");
    assert_eq!(launcher.view().form().unwrap().fields[0].value, "ec");
    assert_eq!(
        submit_alias(&launcher, ""),
        Status::Result("Echo has no alias now".into())
    );
    search(&launcher, "ec hello");
    assert!(!titles(&launcher).contains(&"Echo".to_string()));

    // What the alias names comes before a computed result too.
    set_alias(&launcher, "2+2");
    search(&launcher, "2+2");
    assert_eq!(titles(&launcher)[..2], ["Echo", "4"]);
}

fn a_fallback_is_listed_last_for_any_text_and_is_never_chosen_by_itself(fixture: &Fixture) {
    let dirs = Dirs::new();
    let (launcher, runtime) = dirs.launcher();
    dirs.install(&launcher, fixture.package, "query");
    manage(&launcher);
    assert!(row_subtitle(&launcher, "Fallback: Echo").starts_with("Off · "));

    assert_eq!(
        toggle_fallback(&launcher),
        Status::Result("Echo is now offered for any text typed in root search".into())
    );
    assert!(row_subtitle(&launcher, "Fallback: Echo").starts_with("On · "));
    let recorded = fs::read_to_string(dirs.aliases_file()).unwrap();
    assert!(
        recorded.contains("\"fallbacks\": [\n    \"local:"),
        "{recorded}"
    );

    // Nothing else matches: the fallback is listed, not selected, so Enter
    // sends nothing.
    search(&launcher, "zqx words");
    assert_eq!(titles(&launcher), ["Echo"]);
    assert_eq!(subtitle(&launcher, 0), "Send “zqx words” · fallback");
    assert_eq!(launcher.view().selected, None);
    block_on(launcher.activate_selected());
    assert_eq!(shown(&launcher), Status::Idle);
    assert_eq!(running(&runtime), Vec::<PathBuf>::new());

    // Choosing it sends the whole query.
    launcher.move_selection(1);
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(&launcher),
        Status::Result("Echo heard “zqx words”".into())
    );

    // Below the matches, which stay selected first.
    search(&launcher, "manage");
    assert_eq!(titles(&launcher), [MANAGE_ROW, "Echo"]);
    assert_eq!(selected_title(&launcher).as_deref(), Some(MANAGE_ROW));
    // None for an empty query.
    search(&launcher, "");
    let sending = launcher
        .view()
        .rows
        .iter()
        .filter(|row| {
            row.subtitle
                .as_deref()
                .is_some_and(|s| s.starts_with("Send "))
        })
        .count();
    assert_eq!(sending, 0);

    // No longer a fallback.
    assert_eq!(
        toggle_fallback(&launcher),
        Status::Result("Echo is no longer a fallback".into())
    );
    search(&launcher, "zqx words");
    assert_eq!(titles(&launcher), Vec::<String>::new());
}

fn an_answer_is_cleared_once_the_query_changes(fixture: &Fixture) {
    let dirs = Dirs::new();
    let (launcher, _runtime) = dirs.launcher();
    dirs.install(&launcher, fixture.package, "query");
    set_alias(&launcher, "ec");

    search(&launcher, "ec one");
    block_on(launcher.activate_selected());
    assert_eq!(shown(&launcher), Status::Result("Echo heard “one”".into()));
    block_on(launcher.set_query("ec one more"));
    assert_eq!(launcher.view().status, Status::Idle);

    // An answer that arrives once the query has changed is not shown in
    // the status line.
    search(&launcher, "ec two");
    let answer = launcher.activate_selected();
    assert_eq!(launcher.view().status, Status::Running);
    block_on(launcher.set_query("ec three"));
    assert_eq!(launcher.view().status, Status::Idle);
    block_on(answer);
    assert_eq!(launcher.view().status, Status::Idle);
}

fn disabling_the_target_removes_its_alias_and_fallback_without_enabling_it_again(
    fixture: &Fixture,
) {
    let dirs = Dirs::new();
    let (launcher, runtime) = dirs.launcher();
    dirs.install(&launcher, fixture.package, "query");
    set_alias(&launcher, "ec");
    toggle_fallback(&launcher);

    manage(&launcher);
    activate(&launcher, fixture.title);
    assert_eq!(
        launcher.view().status,
        Status::Result(format!("Disabled {}", fixture.title))
    );
    let not_active = format!("Not active: {} is disabled", fixture.title);
    assert!(row_subtitle(&launcher, "Alias for Echo").contains(&not_active));
    assert!(row_subtitle(&launcher, "Fallback: Echo").contains(&not_active));

    search(&launcher, "ec hello");
    assert_eq!(titles(&launcher), Vec::<String>::new());
    search(&launcher, "ec");
    // Pane's own rows match the two letters fuzzily (#193); the disabled
    // command's alias and fallback rows are gone.
    assert!(!titles(&launcher).contains(&"Echo".to_owned()));
    assert_eq!(running(&runtime), Vec::<PathBuf>::new());

    // Changing them does not enable it either.
    set_alias(&launcher, "echo2");
    toggle_fallback(&launcher);
    toggle_fallback(&launcher);
    assert!(!launcher.packages()[0].enabled);

    // Nor does a restart.
    drop(launcher);
    let (launcher, _runtime) = dirs.launcher();
    assert!(!launcher.packages()[0].enabled);
    search(&launcher, "echo2 hi");
    assert_eq!(titles(&launcher), Vec::<String>::new());

    // Enabled again by the user, both work again.
    manage(&launcher);
    activate(&launcher, fixture.title);
    search(&launcher, "echo2 hi");
    let rows = launcher.view().rows;
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].subtitle.as_deref(), Some("Send “hi” · alias echo2"));
    assert_eq!(
        rows[1].subtitle.as_deref(),
        Some("Send “echo2 hi” · fallback")
    );
    search(&launcher, "zqx");
    assert_eq!(titles(&launcher), ["Echo"]);
}

fn copies_from_other_sources_with_the_same_title_stay_distinct(fixture: &Fixture) {
    let dirs = Dirs::new();
    let (launcher, runtime) = dirs.launcher();
    let first = dirs.install(&launcher, fixture.package, "first");
    let second = dirs.install(&launcher, fixture.package, "second");
    let (first_source, second_source) =
        (source_of(&launcher, &first), source_of(&launcher, &second));

    // The second copy's alias, and both copies as fallbacks, each chosen
    // by its source.
    manage(&launcher);
    select_ending(&launcher, "Alias for Echo", &second_source);
    block_on(launcher.activate_selected());
    submit_alias(&launcher, "ec");
    for source in [&first_source, &second_source] {
        manage(&launcher);
        select_ending(&launcher, "Fallback: Echo", source);
        block_on(launcher.activate_selected());
    }

    search(&launcher, "ec");
    // The command's alias row first and its fallbacks last; between them,
    // Pane's own rows match the two letters fuzzily (#193).
    let listed = titles(&launcher);
    assert_eq!(&listed[..1], ["Echo"]);
    assert_eq!(&listed[listed.len() - 2..], ["Echo", "Echo"]);
    assert!(launcher.view().rows[0].id.ends_with("second#echo"));

    // The rows name their sources, since the titles are the same.
    search(&launcher, "ec hi");
    assert_eq!(titles(&launcher), ["Echo", "Echo", "Echo"]);
    assert_eq!(
        subtitle(&launcher, 0),
        format!("Send “hi” · alias ec · {second_source}")
    );
    assert_eq!(
        subtitle(&launcher, 1),
        format!("Send “ec hi” · fallback · {first_source}")
    );
    assert_eq!(
        subtitle(&launcher, 2),
        format!("Send “ec hi” · fallback · {second_source}")
    );
    block_on(launcher.activate_selected());
    assert_eq!(
        running(&runtime),
        [component_of(&launcher, &second, fixture.component)]
    );
}

fn a_query_command_that_keeps_crashing_is_paused(fixture: &Fixture) {
    let dirs = Dirs::new();
    let (launcher, runtime) = dirs.launcher();
    let folder = dirs.install(&launcher, fixture.package, "query");
    set_alias(&launcher, "ec");

    for _ in 0..3 {
        search(&launcher, "ec crash");
        block_on(launcher.activate_selected());
        assert!(
            matches!(launcher.view().status, Status::Error(_)),
            "{:?}",
            launcher.view().status
        );
    }
    let Status::Error(error) = launcher.view().status else {
        unreachable!()
    };
    assert!(error.contains("is paused"), "{error}");

    // Its alias row is still listed, says why, and runs nothing.
    search(&launcher, "ec hi");
    assert!(launcher.view().rows[0].unavailable.is_some());
    block_on(launcher.activate_selected());
    assert!(
        matches!(&launcher.view().status, Status::Error(reason) if reason.contains("paused")),
        "{:?}",
        launcher.view().status
    );
    assert!(!running(&runtime).contains(&component_of(&launcher, &folder, fixture.component)));
    manage(&launcher);
    assert!(
        row_subtitle(&launcher, "Alias for Echo").contains(&format!(
            "Not active: {} is paused after an error",
            fixture.title
        )),
        "{}",
        row_subtitle(&launcher, "Alias for Echo")
    );
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
    an_alias_finds_the_command_first_and_sends_the_text_after_it_only_when_invoked,
    a_fallback_is_listed_last_for_any_text_and_is_never_chosen_by_itself,
    an_answer_is_cleared_once_the_query_changes,
    disabling_the_target_removes_its_alias_and_fallback_without_enabling_it_again,
    copies_from_other_sources_with_the_same_title_stay_distinct,
    a_query_command_that_keeps_crashing_is_paused,
);

#[test]
fn an_alias_that_is_not_one_word_or_is_another_commands_is_refused() {
    let dirs = Dirs::new();
    let (launcher, _runtime) = dirs.launcher();
    dirs.install(&launcher, "sample-query", "query");
    dirs.install(&launcher, "sample-settings", "settings");
    assert_eq!(
        set_alias_at(&launcher, "Alias for Greeting", "straße"),
        Status::Result("Typing “straße” now finds Greeting".into())
    );

    // Compared caselessly, with full case folding.
    let status = set_alias(&launcher, "STRASSE");
    assert_eq!(
        status,
        Status::Error(
            "Alias: “STRASSE” is already the alias of Greeting: change it there first, or \
             choose another"
                .into()
        )
    );
    let form = launcher.view().form().cloned().expect("the form stays");
    assert!(form.fields[0].error.is_some());

    let status = set_alias(&launcher, "e c");
    assert_eq!(
        status,
        Status::Error("Alias: An alias is one word, without spaces".into())
    );
    let status = set_alias(&launcher, &"e".repeat(33));
    assert_eq!(
        status,
        Status::Error("Alias: An alias has at most 32 characters".into())
    );

    // Greeting takes no query: its alias finds it, typed in any case, and
    // text after it does not send anything; it has no fallback row.
    search(&launcher, "STRASSE");
    assert_eq!(selected_title(&launcher).as_deref(), Some("Greeting"));
    search(&launcher, "straße hello");
    assert!(!titles(&launcher).contains(&"Greeting".to_string()));
    manage(&launcher);
    assert!(!titles(&launcher).contains(&"Fallback: Greeting".to_string()));
}

#[test]
fn a_conflict_in_the_record_is_shown_and_neither_alias_is_used() {
    let dirs = Dirs::new();
    let (launcher, _runtime) = dirs.launcher();
    let query = dirs.install(&launcher, "sample-query", "query");
    let settings = dirs.install(&launcher, "sample-settings", "settings");
    drop(launcher);
    let id = |folder: &Path, command: &str| format!("{}#{command}", identity_of(folder).key());
    let record = serde_json::json!({
        "version": 1,
        "aliases": { id(&query, "echo"): "same", id(&settings, "greeting"): "SAME" },
        "fallbacks": [],
    });
    fs::write(dirs.aliases_file(), record.to_string()).unwrap();

    let (launcher, _runtime) = dirs.launcher();
    manage(&launcher);
    for row in ["Alias for Echo", "Alias for Greeting"] {
        assert!(
            row_subtitle(&launcher, row).contains("Not active: another command has the same alias"),
            "{}",
            row_subtitle(&launcher, row)
        );
    }
    search(&launcher, "same");
    assert_eq!(titles(&launcher), Vec::<String>::new());
}

/// The name of a system Pane runs on other than this one.
fn another_system() -> &'static str {
    if cfg!(target_os = "windows") {
        "linux"
    } else {
        "windows"
    }
}

#[test]
fn a_command_unavailable_on_this_system_shows_its_alias_as_not_active() {
    let dirs = Dirs::new();
    let (launcher, runtime) = dirs.launcher();
    let folder = dirs.source("sample-query", "query");
    let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
    let only = format!("\"platforms\": [\"{}\"], \"component\"", another_system());
    fs::write(
        folder.join("pane.json"),
        manifest.replacen("\"component\"", &only, 1),
    )
    .unwrap();
    install_from(&launcher, &folder);
    set_alias(&launcher, "ec");
    toggle_fallback(&launcher);

    manage(&launcher);
    for row in ["Alias for Echo", "Fallback: Echo"] {
        assert!(
            row_subtitle(&launcher, row).contains(" · Not active: Not available on "),
            "{}",
            row_subtitle(&launcher, row)
        );
    }
    // Root search lists the row with the reason and runs nothing.
    search(&launcher, "ec hi");
    assert!(launcher.view().rows[0].unavailable.is_some());
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().status, Status::Error(_)));
    assert_eq!(running(&runtime), Vec::<PathBuf>::new());
}

#[test]
fn a_package_that_cannot_load_shows_its_choices_as_not_active_with_why() {
    let dirs = Dirs::new();
    let (launcher, _runtime) = dirs.launcher();
    dirs.install(&launcher, "sample-query", "query");
    set_alias(&launcher, "ec");
    let location = launcher.packages()[0].location.clone();
    drop(launcher);
    fs::write(location.join("pane.json"), "{ not json").unwrap();

    let (launcher, _runtime) = dirs.launcher();
    manage(&launcher);
    let row = "Alias “ec” of `echo`";
    let why = row_subtitle(&launcher, row);
    assert!(why.starts_with("Not active: "), "{why}");
    assert!(why.contains("cannot load"), "{why}");
    assert!(!why.contains("has no command"), "{why}");
}

#[test]
fn a_choice_whose_command_is_gone_is_shown_and_can_be_forgotten() {
    let dirs = Dirs::new();
    let (launcher, _runtime) = dirs.launcher();
    let query = dirs.install(&launcher, "sample-query", "query");
    drop(launcher);
    let key = identity_of(&query).key();
    let record = serde_json::json!({
        "version": 1,
        "aliases": { format!("{key}#gone"): "gn", "local:/nowhere#echo": "nw" },
        "fallbacks": [format!("{key}#gone")],
    });
    fs::write(dirs.aliases_file(), record.to_string()).unwrap();

    let (launcher, _runtime) = dirs.launcher();
    manage(&launcher);
    let gone = "Alias “gn” and fallback of a missing command";
    assert!(
        row_subtitle(&launcher, gone)
            .starts_with("Not active: Query sample has no command `gone` now; Enter forgets it")
    );
    let elsewhere = "Alias “nw” of a missing command";
    assert!(
        row_subtitle(&launcher, elsewhere)
            .starts_with("Not active: its extension is not installed")
    );

    activate(&launcher, gone);
    assert_eq!(
        launcher.view().status,
        Status::Result("Forgot the alias and fallback".into())
    );
    assert!(!titles(&launcher).contains(&gone.to_string()));
    let recorded = fs::read_to_string(dirs.aliases_file()).unwrap();
    assert!(!recorded.contains("#gone"), "{recorded}");
    assert!(recorded.contains("nowhere#echo"), "{recorded}");
}

/// Uninstalls the package installed from `folder`, keeping its saved data.
fn uninstall(launcher: &Launcher, folder: &Path, title: &str) {
    let source = source_of(launcher, folder);
    manage(launcher);
    select_ending(launcher, &format!("Uninstall {title}"), &source);
    block_on(launcher.activate_selected());
    activate(launcher, "Uninstall and keep saved data");
}

#[test]
fn uninstalling_forgets_the_aliases_and_fallbacks() {
    let dirs = Dirs::new();
    let (launcher, _runtime) = dirs.launcher();
    let folder = dirs.install(&launcher, "sample-query", "query");
    set_alias(&launcher, "ec");
    toggle_fallback(&launcher);

    uninstall(&launcher, &folder, "Query sample");
    assert!(
        launcher.packages().is_empty(),
        "{:?}",
        launcher.view().status
    );
    let recorded = fs::read_to_string(dirs.aliases_file()).unwrap();
    assert!(!recorded.contains("#echo"), "{recorded}");
}

#[test]
fn uninstalling_forgets_exactly_its_own_commands_not_those_of_a_longer_source() {
    let dirs = Dirs::new();
    let (launcher, _runtime) = dirs.launcher();
    // "x#y" starts with "x" and `#`, as "x"'s command ids do.
    let short = dirs.install(&launcher, "sample-query", "x");
    let long = dirs.install(&launcher, "sample-query", "x#y");
    let long_source = source_of(&launcher, &long);
    manage(&launcher);
    select_ending(&launcher, "Alias for Echo", &long_source);
    block_on(launcher.activate_selected());
    submit_alias(&launcher, "ec");

    uninstall(&launcher, &short, "Query sample");
    assert_eq!(launcher.packages().len(), 1);
    manage(&launcher);
    assert!(row_subtitle(&launcher, "Alias for Echo").starts_with("“ec” · "));
    let recorded = fs::read_to_string(dirs.aliases_file()).unwrap();
    assert!(recorded.contains("x#y#echo\": \"ec\""), "{recorded}");
    search(&launcher, "ec hi");
    // Pane's install row matches the letters fuzzily below the send row
    // (#193).
    assert_eq!(
        titles(&launcher),
        ["Echo", "Install extension from folder…"]
    );
}

#[test]
fn a_change_that_cannot_be_kept_never_brings_back_an_uninstalled_packages_choices() {
    let dirs = Dirs::new();
    // A record Pane cannot read is never replaced, so every write fails.
    fs::create_dir_all(dirs.aliases_file()).unwrap();
    let (launcher, _runtime) = dirs.launcher();
    let folder = dirs.install(&launcher, "sample-query", "query");

    // The alias applies at once; its write is still to come.
    manage(&launcher);
    activate(&launcher, "Alias for Echo");
    let form = launcher.view().form().cloned().unwrap();
    launcher.set_field_value(&form.fields[0].id, "ec");
    let write = launcher.submit_form();
    // Meanwhile the package is uninstalled, forgetting it.
    uninstall(&launcher, &folder, "Query sample");
    assert!(launcher.packages().is_empty());
    // The write fails, which must not bring the alias back.
    block_on(write);
    assert!(matches!(launcher.view().status, Status::Error(_)));

    install_from(&launcher, &folder);
    manage(&launcher);
    assert!(
        row_subtitle(&launcher, "Alias for Echo").starts_with("None · "),
        "{}",
        row_subtitle(&launcher, "Alias for Echo")
    );
    search(&launcher, "ec hi");
    // Pane's install row matches the letters fuzzily (#193); the command's
    // row is gone.
    assert_eq!(titles(&launcher), ["Install extension from folder…"]);
}

/// Echo opened from its row, with no text sent: it runs without a screen
/// and says how to send it text.
#[test]
fn enter_on_echo_runs_it_without_text_and_opens_no_screen() {
    let dirs = Dirs::new();
    let (launcher, _runtime) = dirs.launcher();
    dirs.install(&launcher, "sample-query", "query");
    search(&launcher, "echo");
    activate(&launcher, "Echo");
    assert_eq!(
        shown(&launcher),
        Status::Result(
            "Echo heard nothing: give it an alias or make it a fallback in Settings, \
             then send it text from root search"
                .into()
        )
    );
    assert_eq!(
        launcher.view().screen,
        Screen::Root {
            query: "echo".into()
        }
    );
}
