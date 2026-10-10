//! Applications, a default extension, through the launcher's public
//! interface: the installed applications are found by name in root search,
//! ranked with commands by title, and invoking one opens it. The real guest
//! (`target/guests/packages/applications`) supplies them; the system's
//! applications come from a fake [`Applications`], so what is installed and
//! what opening does are deterministic. The real systems are checked in
//! `application_adapters.rs`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::applications::{Application, Applications};
use pane_core::{Launcher, PackageIdentity, Runtime, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::titles;

fn built(path: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(path);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// The system as the tests set it up: its applications, what was opened,
/// and how often Pane looked for applications.
#[derive(Default)]
struct FakeSystem {
    applications: Mutex<Vec<Application>>,
    /// Why listing fails, if it does.
    listing_fails: Mutex<Option<String>>,
    /// Why opening fails, if it does.
    opening_fails: Mutex<Option<String>>,
    opened: Mutex<Vec<String>>,
    listed: AtomicUsize,
    /// While true, listing waits.
    held: Mutex<bool>,
    released: Condvar,
    /// The host's change notification, which the tests fire as the
    /// system's watcher would when the applications changed by
    /// themselves.
    changed: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl FakeSystem {
    fn with(names: &[&str]) -> Arc<FakeSystem> {
        let system = FakeSystem::default();
        *system.applications.lock().unwrap() = names.iter().map(|name| app(name)).collect();
        Arc::new(system)
    }

    fn listed(&self) -> usize {
        self.listed.load(Ordering::SeqCst)
    }

    fn opened(&self) -> Vec<String> {
        self.opened.lock().unwrap().clone()
    }

    /// Tells the host the applications changed, as the system's watcher
    /// would: the commands that asked for them are asked for their results
    /// again (see `application_changes`).
    fn tell_changed(&self) {
        if let Some(changed) = self.changed.lock().unwrap().clone() {
            changed();
        }
    }

    fn hold(&self) {
        *self.held.lock().unwrap() = true;
    }

    fn release(&self) {
        *self.held.lock().unwrap() = false;
        self.released.notify_all();
    }
}

fn app(name: &str) -> Application {
    Application {
        id: format!("/apps/{name}.app"),
        name: name.into(),
        location: "/apps".into(),
        ..Application::default()
    }
}

/// An application with the other names and keywords its sources give it,
/// and what tells it apart from another of its name.
fn found_as(name: &str, alternate: &[&str], keywords: &[&str]) -> Application {
    Application {
        alternate_titles: alternate.iter().map(|title| (*title).to_owned()).collect(),
        keywords: keywords
            .iter()
            .map(|keyword| (*keyword).to_owned())
            .collect(),
        ..app(name)
    }
}

impl Applications for FakeSystem {
    fn installed(&self) -> Result<Vec<Application>, String> {
        let mut held = self.held.lock().unwrap();
        while *held {
            held = self.released.wait(held).unwrap();
        }
        self.listed.fetch_add(1, Ordering::SeqCst);
        if let Some(problem) = self.listing_fails.lock().unwrap().clone() {
            return Err(problem);
        }
        Ok(self.applications.lock().unwrap().clone())
    }

    fn open(&self, id: &str) -> Result<(), String> {
        if let Some(problem) = self.opening_fails.lock().unwrap().clone() {
            return Err(problem);
        }
        self.opened.lock().unwrap().push(id.to_owned());
        Ok(())
    }

    fn on_change(&self, changed: Arc<dyn Fn() + Send + Sync>) {
        *self.changed.lock().unwrap() = Some(changed);
    }
}

/// Pane's data location and compiled code cache for one test.
struct Dirs {
    data: TempDir,
    cache: TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            data: tempfile::tempdir().unwrap(),
            cache: tempfile::tempdir().unwrap(),
        }
    }

    /// A launcher whose runtime finds the applications of `system`, with
    /// the applications package and then `others` installed.
    fn launcher(&self, system: &Arc<FakeSystem>, others: &[&str]) -> (Launcher, Runtime) {
        let runtime = Runtime::start_with_cache(self.cache.path().to_path_buf()).unwrap();
        runtime.set_applications(system.clone());
        let launcher = Launcher::with_packages(
            Ok(runtime.clone()),
            vec![],
            self.data.path().join("extensions"),
        );
        install(&launcher, &applications());
        for other in others {
            install(&launcher, &built(&format!("packages/{other}")));
        }
        launcher.back();
        (launcher, runtime)
    }
}

fn applications() -> PathBuf {
    built("packages/applications")
}

fn install(launcher: &Launcher, folder: &Path) {
    block_on(launcher.install_package(folder));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
}

fn search(launcher: &Launcher, query: &str) {
    block_on(launcher.set_query(query));
}

fn identity() -> PackageIdentity {
    PackageIdentity::local(&applications()).unwrap()
}

fn component_running(runtime: &Runtime) -> bool {
    block_on(runtime.running())
        .iter()
        .any(|component| component.ends_with("applications.wasm"))
}

#[test]
fn typing_an_applications_name_lists_it_and_enter_opens_it() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&["Firefox", "Files", "Terminal"]);
    let (launcher, _) = dirs.launcher(&system, &[]);

    search(&launcher, "fire");

    // Pane's install row matches the four letters fuzzily below the
    // application's prefix match (#193).
    assert_eq!(
        titles(&launcher),
        ["Firefox", "Install extension from Git…"]
    );
    let view = launcher.view();
    assert_eq!(view.rows[0].subtitle.as_deref(), Some("Application"));
    assert_eq!(view.selected, Some(0));

    block_on(launcher.activate_selected());

    assert_eq!(system.opened(), ["/apps/Firefox.app"]);
    let view = launcher.view();
    assert_eq!(view.status, Status::Result("Opened Firefox".into()));
    assert_eq!(view.query(), Some("fire"), "root search stays as it was");
}

#[test]
fn applications_rank_with_commands_by_title() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&["Rust", "Trusty Notes"]);
    let (launcher, _) = dirs.launcher(&system, &["sample-rust"]);

    // Low holds the mid-word containment of "Trusty", which the default
    // High drops — and Pane's own rows, which Low holds too (#193).
    launcher.set_search_sensitivity(pane_core::SearchSensitivity::Low);
    search(&launcher, "rust");

    // The exact title first, then the title starting with the query, then
    // the one containing it, then the fuzzy matches.
    assert_eq!(
        titles(&launcher),
        [
            "Rust",
            "Rust sample",
            "Trusty Notes",
            "Manage Extensions",
            "Install extension from npm…"
        ]
    );
}

/// An application is found by the other names and keywords its sources
/// give it, as a command's title and subtitle are found (#124's fields,
/// scored and ranked by #197).
#[test]
fn an_application_is_found_by_its_other_names_and_keywords() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&[]);
    *system.applications.lock().unwrap() = vec![
        found_as("Windows Terminal", &["wt"], &[]),
        found_as("Firefox", &["Nightly"], &["browser"]),
    ];
    let (launcher, _) = dirs.launcher(&system, &[]);

    // A program name is an alternate title: found as the title is.
    search(&launcher, "wt");
    assert_eq!(titles(&launcher), ["Windows Terminal"]);
    search(&launcher, "term");
    assert_eq!(titles(&launcher), ["Windows Terminal"]);
    // A keyword, as the subtitle is; an untranslated name, as a title is.
    search(&launcher, "browser");
    assert_eq!(titles(&launcher), ["Firefox"]);
    search(&launcher, "nightly");
    assert_eq!(titles(&launcher), ["Firefox"]);
    // The row keeps its title and its "Application" subtitle.
    let view = launcher.view();
    assert_eq!(view.rows[0].title, "Firefox");
    assert_eq!(view.rows[0].subtitle.as_deref(), Some("Application"));
}

/// Applications of one name each show what tells them apart: their
/// distinction, as their provider supplies it, as the subtitle itself.
#[test]
fn two_applications_of_one_name_show_their_distinctions() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&[]);
    let (mut first, mut second) = (app("Firefox"), app("Firefox"));
    first.id = "/programs/firefox/firefox.app".into();
    first.distinction = Some("firefox.exe".into());
    second.id = "/programs/nightly/firefox.app".into();
    second.distinction = Some("firefox-dev.exe".into());
    *system.applications.lock().unwrap() = vec![first, second];
    let (launcher, _) = dirs.launcher(&system, &[]);

    search(&launcher, "firefox");
    let view = launcher.view();
    assert_eq!(titles(&launcher), ["Firefox", "Firefox"]);
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("firefox.exe"),
        "the first, as the system listed it, keeps its place"
    );
    assert_eq!(view.rows[1].subtitle.as_deref(), Some("firefox-dev.exe"));
    // Opening the second, not the first, opens the second.
    launcher.select(1);
    block_on(launcher.activate_selected());
    assert_eq!(
        system.opened(),
        ["/programs/nightly/firefox.app"],
        "the distinction names the row, not just the list"
    );
}

/// A command ranks above an application of the same title, and each row
/// shows what tells it apart (step 11, and the same-name rule).
#[test]
fn a_command_ranks_above_an_application_of_the_same_title() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&["Rust sample"]);
    let (launcher, _) = dirs.launcher(&system, &["sample-rust"]);

    search(&launcher, "rust sample");
    let view = launcher.view();
    assert_eq!(
        titles(&launcher),
        ["Rust sample", "Rust sample"],
        "the command above the application, an exact title both"
    );
    let command = view.rows[0].subtitle.as_deref().unwrap_or_default();
    assert!(
        command.starts_with("A sample command implemented by a Rust extension")
            && command.contains("local folder "),
        "the command names its package's source: {command}"
    );
    assert_eq!(view.rows[1].subtitle.as_deref(), Some("Application"));
    // An application, ranked below, is still chosen by moving to it.
    launcher.select(1);
    block_on(launcher.activate_selected());
    assert_eq!(system.opened(), ["/apps/Rust sample.app"]);
}

#[test]
fn a_blank_query_asks_for_no_applications_and_starts_nothing() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&["Firefox"]);
    let (launcher, runtime) = dirs.launcher(&system, &[]);

    search(&launcher, "   ");
    search(&launcher, "");

    assert!(!titles(&launcher).contains(&"Firefox".to_owned()));
    assert_eq!(system.listed(), 0, "no one looked for applications");
    assert!(!component_running(&runtime));

    // After a search, clearing the query keeps what it found listed — the
    // blank query ranks the kept results in the no-query order (#199),
    // an application below the commands — and asks for nothing: the
    // system is not looked at again.
    search(&launcher, "fire");
    let asked = system.listed();
    search(&launcher, "");
    assert_eq!(
        titles(&launcher),
        [
            "Install extension from folder…",
            "Install extension from Git…",
            "Install extension from npm…",
            "Manage Extensions",
            "Settings…",
            "Firefox"
        ]
    );
    assert_eq!(system.listed(), asked, "the blank query asks for nothing");
}

#[test]
fn applications_are_looked_for_once_per_visit_of_root_search() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&["Firefox"]);
    let (launcher, _) = dirs.launcher(&system, &[]);

    search(&launcher, "f");
    search(&launcher, "fi");
    search(&launcher, "fir");
    assert_eq!(system.listed(), 1);

    // Installed meanwhile: a search that nothing told of the change lists
    // the results kept from the earlier asks (#202) — a command that asked
    // for the host's applications is not asked again by a query, and not
    // by a show of root search that changed nothing either. The host's
    // list is told of the change, as the system's watcher would, and the
    // commands that asked are asked for their results again, listed in
    // place (see `application_changes`).
    system.applications.lock().unwrap().push(app("Firewall"));
    search(&launcher, "fire");
    assert_eq!(
        titles(&launcher),
        ["Firefox", "Install extension from Git…"]
    );

    system.tell_changed();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !titles(&launcher).contains(&"Firewall".to_owned()) {
        assert!(
            Instant::now() < deadline,
            "the changed applications were never listed"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        titles(&launcher),
        ["Firefox", "Firewall", "Install extension from Git…"]
    );
}

#[test]
fn typing_does_not_wait_for_applications_to_be_found() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&["Rust"]);
    let (launcher, _) = dirs.launcher(&system, &["sample-rust"]);
    system.hold();

    let searching = launcher.set_query("rust");
    let answered = thread::spawn(move || block_on(searching));

    // The command is found once the query's list is published (#201) —
    // the sample's provider answers within the hold — while the
    // applications are still being looked for.
    let deadline = Instant::now() + Duration::from_secs(10);
    while titles(&launcher) != ["Rust sample"] {
        assert!(
            Instant::now() < deadline,
            "the command was never listed: {:?}",
            titles(&launcher)
        );
        thread::sleep(Duration::from_millis(5));
    }
    system.release();
    answered.join().unwrap();
    assert_eq!(titles(&launcher), ["Rust", "Rust sample"]);
}

#[test]
fn an_application_that_does_not_open_is_explained() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&["Firefox"]);
    *system.opening_fails.lock().unwrap() = Some("permission denied".into());
    let (launcher, _) = dirs.launcher(&system, &[]);

    search(&launcher, "firefox");
    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        Status::Error("Could not open Firefox: permission denied".into())
    );
}

#[test]
fn applications_that_cannot_be_listed_are_explained_for_any_query() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&[]);
    *system.listing_fails.lock().unwrap() = Some("no Start menu".into());
    let (launcher, _) = dirs.launcher(&system, &[]);

    search(&launcher, "firefox");

    let view = launcher.view();
    assert_eq!(titles(&launcher), ["Applications"]);
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("Could not list: The extension reported an error: no Start menu")
    );
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Error(
            "Applications could not list its results: The extension reported an error: \
             no Start menu"
                .into()
        )
    );
}

#[test]
fn disabling_applications_removes_them_and_stops_looking_while_others_still_answer() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&["Firefox", "Calculator"]);
    let (launcher, runtime) = dirs.launcher(&system, &["calculator"]);
    search(&launcher, "firefox");
    assert_eq!(titles(&launcher), ["Firefox"]);

    block_on(launcher.set_enabled(&identity(), false));

    assert!(titles(&launcher).is_empty(), "{:?}", titles(&launcher));
    assert!(!component_running(&runtime));
    search(&launcher, "fire");
    search(&launcher, "calc");
    assert_eq!(system.listed(), 1, "nothing looks for applications");
    assert!(!titles(&launcher).contains(&"Firefox".to_owned()));
    // The calculator still answers; neither has a row of its own.
    search(&launcher, "6*7");
    assert_eq!(titles(&launcher), ["42"]);
    // Pane's install row matches "calc" fuzzily (#193); no calculator row.
    search(&launcher, "calc");
    assert_eq!(titles(&launcher), ["Install extension from folder…"]);

    block_on(launcher.set_enabled(&identity(), true));
    search(&launcher, "fire");
    assert_eq!(
        titles(&launcher),
        ["Firefox", "Install extension from Git…"]
    );
}

#[test]
fn an_answer_arriving_after_disabling_is_discarded() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&["Firefox"]);
    let (launcher, _) = dirs.launcher(&system, &[]);
    system.hold();

    let searching = launcher.set_query("fire");
    let answered = thread::spawn(move || block_on(searching));
    // Give the guest time to start asking the system; if it has not, it is
    // refused as disabled, which must not list anything either.
    thread::sleep(Duration::from_millis(200));
    block_on(launcher.set_enabled(&identity(), false));
    system.release();
    answered.join().unwrap();

    search(&launcher, "firef");
    assert!(
        !titles(&launcher).contains(&"Firefox".to_owned()),
        "{:?}",
        titles(&launcher)
    );
}

/// Applications is a root provider (#164): typing "applications" offers
/// no "Applications" row to open, and each application is still its own
/// root result.
#[test]
fn applications_has_no_row_of_its_own_and_each_application_is_found() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&["terminal", "Files", "Firefox"]);
    let (launcher, _) = dirs.launcher(&system, &[]);

    search(&launcher, "applications");
    // The Files application matches the word through the composite of
    // its subtitle and title (#193); no Applications row is listed.
    assert!(!titles(&launcher).contains(&"Applications".to_owned()));
    assert!(
        launcher
            .view()
            .rows
            .iter()
            .all(|row| row.title != "Applications")
    );

    search(&launcher, "files");
    assert_eq!(titles(&launcher), ["Files"]);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Opened Files".into())
    );
    assert_eq!(system.opened(), ["/apps/Files.app"]);
}

#[test]
fn a_package_saying_it_supplies_indexed_results_without_the_interface_is_refused() {
    let dirs = Dirs::new();
    let folder = dirs.data.path().join("source");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("pane.json"),
        r#"{"manifestVersion": 1, "title": "Pretender", "apiVersion": "0.1",
            "commands": [{"id": "pretend", "title": "Pretend",
                          "component": "command.wasm", "indexedResults": true}]}"#,
    )
    .unwrap();
    std::fs::copy(built("calculator.wasm"), folder.join("command.wasm")).unwrap();
    let runtime = Runtime::start_with_cache(dirs.cache.path().to_path_buf()).unwrap();
    let launcher = Launcher::with_packages(Ok(runtime), vec![], dirs.data.path().join("x"));

    block_on(launcher.install_package(&folder));

    let Status::Error(error) = launcher.view().status else {
        panic!("{:?}", launcher.view().status);
    };
    assert!(
        error.contains(
            "supplies indexed results, but it does not export pane:extension/indexed-results"
        ),
        "{error}"
    );
    assert!(launcher.packages().is_empty());
}

/// The JavaScript and TypeScript author examples: each supplies "Launch
/// <name>" for every installed application, found through
/// `pane:extension/applications`, and its command lists and opens them.
fn a_js_command_finds_and_opens_applications(package: &str, language: &str) {
    let dirs = Dirs::new();
    let system = FakeSystem::with(&["Firefox", "Files"]);
    let (launcher, _) = dirs.launcher(&system, &[package]);

    search(&launcher, "launch fire");

    // The Files launcher matches too, through the composite of its
    // command's row and the sample's subtitle (#193).
    assert_eq!(titles(&launcher), ["Launch Firefox", "Launch Files"]);
    let view = launcher.view();
    let sample = format!("{language} applications sample");
    assert_eq!(view.rows[0].subtitle.as_deref(), Some(sample.as_str()));
    block_on(launcher.activate_selected());
    // Pane names the opened result by its title.
    assert_eq!(
        launcher.view().status,
        Status::Result("Opened Launch Firefox".into())
    );
    assert_eq!(system.opened(), ["/apps/Firefox.app"]);

    // Its command lists them by name and opens one through the import.
    search(&launcher, &sample.to_lowercase());
    // The command first; its results match by their subtitle below it.
    assert_eq!(titles(&launcher)[0], sample);
    block_on(launcher.activate_selected());
    assert_eq!(titles(&launcher), ["Files", "Firefox"]);
    block_on(launcher.activate_selected());
    assert_eq!(shown(&launcher), Status::Result("Opened Files".into()));

    // A failure the system reports reaches the command as an error.
    *system.opening_fails.lock().unwrap() = Some("permission denied".into());
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(&launcher),
        Status::Error("The extension reported an error: permission denied".into())
    );
}

#[test]
fn a_javascript_command_finds_and_opens_applications() {
    a_js_command_finds_and_opens_applications("sample-applications-js", "JavaScript");
}

#[test]
fn a_typescript_command_finds_and_opens_applications() {
    a_js_command_finds_and_opens_applications("sample-applications-ts", "TypeScript");
}
