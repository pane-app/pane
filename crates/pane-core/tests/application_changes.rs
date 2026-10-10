//! The live application list through the launcher's public interface
//! (ADR 0038), with the real Applications guest
//! (`target/guests/packages/applications`) and the host's list
//! ([`Cached`]) over a fake system whose sources and watcher the tests
//! drive, on the launcher's manual clock: an application installed while
//! root search is on screen appears without leaving it; one uninstalled
//! leaves after its grace, and one reinstalled within it never does; a
//! renamed shortcut changes the title and keeps the pin; a packaged app
//! appears as quickly; a missed change is reconciled; the selected row
//! does not jump; nothing is watched until root search is used; and
//! disabling Applications stops every watcher and drops the list. The
//! list's own rules are in `application_cache.rs`, and each system's
//! watcher in `application_adapters.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::applications::{
    Application, Applications, Cached, Change, Changes, Discovery, GRACE, Key, Source, Watch,
};
use pane_core::clipboard::ManualClock;
use pane_core::{
    Launcher, PackageIdentity, PinTarget, ResultAction, Runtime, Screen, SlotChange, Status,
};
use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

use rows::{select_title, titles};

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

fn applications() -> PathBuf {
    built("packages/applications")
}

fn identity() -> PackageIdentity {
    PackageIdentity::local(&applications()).unwrap()
}

/// A system as the tests set it up: its applications' shortcuts, and its
/// watcher, which the tests report through.
#[derive(Default)]
struct FakeSystem {
    sources: Mutex<Vec<Source>>,
    scans: AtomicUsize,
    /// Where the latest watch reports.
    reports: Mutex<Option<Changes>>,
    /// How many watches are held.
    watching: Arc<AtomicUsize>,
}

/// Held by a watch: counts it as watching until dropped.
struct Watching(Arc<AtomicUsize>);

impl Drop for Watching {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl FakeSystem {
    fn with(names: &[&str]) -> Arc<FakeSystem> {
        let system = Arc::new(FakeSystem::default());
        system.set(names);
        system
    }

    /// The system's applications are now `names`.
    fn set(&self, names: &[&str]) {
        *self.sources.lock().unwrap() = names.iter().map(|name| shortcut(name)).collect();
    }

    fn scans(&self) -> usize {
        self.scans.load(Ordering::SeqCst)
    }

    fn watching(&self) -> usize {
        self.watching.load(Ordering::SeqCst)
    }

    /// Tells the list what the system's watcher would after a change.
    fn report(&self, change: Change) {
        let reports = self.reports.lock().unwrap().clone();
        reports.expect("the host watches the system")(change);
    }
}

/// The Start menu shortcut `name`, to a program of its own.
fn shortcut(name: &str) -> Source {
    Source::new(
        Key::program(&format!(r"C:\Programs\{name}\{name}.exe"), ""),
        format!(r"C:\Menu\{name}.lnk"),
        name,
        r"C:\Menu",
        2,
    )
}

/// The host's list of applications, counting each ask of it: the
/// Applications provider calls `installed` once for each ask of its
/// results (#202).
struct Counted {
    applications: Arc<dyn Applications>,
    asks: Arc<AtomicUsize>,
}

impl Applications for Counted {
    fn installed(&self) -> Result<Vec<Application>, String> {
        self.asks.fetch_add(1, Ordering::SeqCst);
        self.applications.installed()
    }

    fn open(&self, id: &str) -> Result<(), String> {
        self.applications.open(id)
    }

    fn source(&self, id: &str) -> Option<String> {
        self.applications.source(id)
    }

    fn current_id(&self, id: &str) -> Option<String> {
        self.applications.current_id(id)
    }

    fn on_change(&self, changed: Arc<dyn Fn() + Send + Sync>) {
        self.applications.on_change(changed);
    }

    fn release(&self) {
        self.applications.release();
    }

    fn icon_source(&self, id: &str) -> Option<String> {
        self.applications.icon_source(id)
    }
}

impl Discovery for FakeSystem {
    fn sources(&self) -> Result<Vec<Source>, String> {
        self.scans.fetch_add(1, Ordering::SeqCst);
        Ok(self.sources.lock().unwrap().clone())
    }

    fn open(&self, _path: &str) -> Result<(), String> {
        Ok(())
    }

    fn watch(&self, changes: Changes) -> Result<Watch, String> {
        *self.reports.lock().unwrap() = Some(changes);
        self.watching.fetch_add(1, Ordering::SeqCst);
        Ok(Watch::new(Watching(self.watching.clone())))
    }
}

/// Pane's data location and compiled code cache for one test, and the
/// clock its launcher and its host's list tell the time by.
struct Pane {
    // Dropped first, before its folders.
    launcher: Launcher,
    clock: Arc<ManualClock>,
    data: TempDir,
    _cache: TempDir,
}

impl Pane {
    /// A launcher with the Applications package installed, whose host
    /// lists `system`'s applications, at root search.
    fn new(system: &Arc<FakeSystem>) -> Pane {
        let clock = ManualClock::at(1_000_000);
        let list = Arc::new(
            Cached::new(system.clone(), Duration::from_secs(3600)).with_clock(clock.clone()),
        );
        Pane::with_list(clock, list)
    }

    /// A launcher as [`Pane::new`], whose host's list of applications is
    /// `list`: a test that counts the asks of the Applications provider
    /// wraps one around the host's list (#202).
    fn with_list(clock: Arc<ManualClock>, list: Arc<dyn Applications>) -> Pane {
        let data = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let runtime = Runtime::start_with_cache(cache.path().to_path_buf()).unwrap();
        runtime.set_applications(list);
        let launcher = Launcher::with_packages(Ok(runtime), vec![], data.path().join("extensions"))
            .with_quick_slots(data.path())
            .with_clock(clock.clone());
        install(&launcher, &applications());
        Pane {
            launcher,
            clock,
            data,
            _cache: cache,
        }
    }

    fn search(&self, query: &str) {
        block_on(self.launcher.set_query(query));
    }

    /// The applications root search lists now, in order: the rows whose
    /// title starts with "Zeta", as every application of these tests does.
    fn listed(&self) -> Vec<String> {
        titles(&self.launcher)
            .into_iter()
            .filter(|title| title.starts_with("Zeta"))
            .collect()
    }

    /// [`Pane::listed`], sorted.
    fn sorted(&self) -> Vec<String> {
        let mut listed = self.listed();
        listed.sort();
        listed
    }

    /// The title of the selected row.
    fn selected(&self) -> Option<String> {
        let view = self.launcher.view();
        view.selected.map(|index| view.rows[index].title.clone())
    }

    /// Waits until root search lists `expected`, in any order, at most
    /// `limit`.
    fn wait_for(&self, expected: &[&str], limit: Duration) {
        let deadline = Instant::now() + limit;
        let mut expected = expected.to_vec();
        expected.sort_unstable();
        while self.sorted() != expected {
            assert!(
                Instant::now() < deadline,
                "root search lists {:?}, not {expected:?}",
                self.listed()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Root search is still on screen with `query`.
    fn still_searching(&self, query: &str) {
        let view = self.launcher.view();
        assert_eq!(
            view.screen,
            Screen::Root {
                query: query.into()
            }
        );
    }
}

fn install(launcher: &Launcher, folder: &Path) {
    block_on(launcher.install_package(folder));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
    launcher.back();
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what} did not happen");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Long enough for root search to list a change, if it were going to.
fn settle() {
    thread::sleep(Duration::from_millis(300));
}

/// Generously "about a second": the changes settle for half a second,
/// then the guest answers.
const ABOUT_A_SECOND: Duration = Duration::from_secs(3);

#[test]
fn nothing_is_watched_until_root_search_is_first_used() {
    let system = FakeSystem::with(&["Zeta Editor"]);
    let pane = Pane::new(&system);
    settle();
    assert_eq!((system.scans(), system.watching()), (0, 0));

    pane.search("");
    settle();
    assert_eq!((system.scans(), system.watching()), (0, 0));

    pane.search("zeta");
    assert_eq!(pane.listed(), ["Zeta Editor"]);
    wait_until("watching", || system.watching() == 1);
}

#[test]
fn an_application_installed_while_root_search_shows_appears_without_leaving_it() {
    let system = FakeSystem::with(&["Zeta Editor"]);
    let pane = Pane::new(&system);
    pane.search("zeta");
    assert_eq!(pane.listed(), ["Zeta Editor"]);
    wait_until("watching", || system.watching() == 1);

    system.set(&["Zeta Editor", "Zeta Mail"]);
    system.report(Change::Changed);

    pane.wait_for(&["Zeta Editor", "Zeta Mail"], ABOUT_A_SECOND);
    pane.still_searching("zeta");
}

#[test]
fn a_packaged_app_installed_appears_as_quickly_as_a_program() {
    let system = FakeSystem::with(&["Zeta Editor"]);
    let pane = Pane::new(&system);
    pane.search("zeta");
    wait_until("watching", || system.watching() == 1);

    // Windows registers it: the package's folder appears first.
    system.set(&["Zeta Editor", "Zeta Calculator"]);
    system.report(Change::Completing);

    pane.wait_for(&["Zeta Calculator", "Zeta Editor"], ABOUT_A_SECOND);
    pane.still_searching("zeta");
}

#[test]
fn an_uninstalled_application_leaves_root_search_after_its_grace() {
    let system = FakeSystem::with(&["Zeta Editor", "Zeta Mail"]);
    let pane = Pane::new(&system);
    pane.search("zeta");
    wait_until("watching", || system.watching() == 1);

    system.set(&["Zeta Editor"]);
    system.report(Change::Changed);
    wait_until("the rescan", || system.scans() == 2);
    settle();
    assert_eq!(pane.sorted(), ["Zeta Editor", "Zeta Mail"]);

    pane.clock.advance(GRACE);
    pane.wait_for(&["Zeta Editor"], ABOUT_A_SECOND);
    pane.still_searching("zeta");
}

#[test]
fn an_application_reinstalled_by_its_update_never_leaves_root_search() {
    let system = FakeSystem::with(&["Zeta Editor", "Zeta Mail"]);
    let pane = Pane::new(&system);
    pane.search("zeta");
    wait_until("watching", || system.watching() == 1);

    system.set(&["Zeta Editor"]);
    system.report(Change::Changed);
    wait_until("the rescan", || system.scans() == 2);
    pane.clock.advance(GRACE / 2);
    system.set(&["Zeta Editor", "Zeta Mail"]);
    system.report(Change::Changed);
    wait_until("the second rescan", || system.scans() == 3);

    pane.clock.advance(GRACE * 4);
    settle();
    assert_eq!(pane.sorted(), ["Zeta Editor", "Zeta Mail"]);
}

#[test]
fn a_renamed_shortcut_changes_the_title_and_keeps_the_pin() {
    let system = FakeSystem::with(&["Zeta Editor"]);
    let pane = Pane::new(&system);
    pane.search("zeta");
    select_title(&pane.launcher, "Zeta Editor");
    let index = pane.launcher.view().selected.unwrap();
    let target = pane.launcher.view().rows[index].id.clone();
    let (change, recorded) = pane.launcher.change_quick_slots(&target, ResultAction::Pin);
    assert!(matches!(change, SlotChange::Changed(_)), "{change:?}");
    block_on(recorded);
    wait_until("watching", || system.watching() == 1);

    // Renamed: the same program, so the same application.
    let renamed = Source {
        name: "Zeta Code".into(),
        path: r"C:\Menu\Zeta Code.lnk".into(),
        ..shortcut("Zeta Editor")
    };
    *system.sources.lock().unwrap() = vec![renamed.clone()];
    system.report(Change::Changed);

    pane.wait_for(&["Zeta Code"], ABOUT_A_SECOND);
    let slots = pane.launcher.quick_slots();
    assert_eq!(slots[0].title, "Zeta Code");
    assert!(slots[0].ready(), "{slots:?}");
    match &slots[0].target {
        PinTarget::Indexed { result, .. } => assert_eq!(*result, renamed.key.id()),
        other => panic!("{other:?}"),
    }
    let record = fs::read_to_string(pane.data.path().join("quick-slots.json")).unwrap();
    assert!(record.contains(&renamed.key.id()), "{record}");
}

#[test]
fn a_change_the_watcher_missed_is_reconciled() {
    let system = FakeSystem::with(&["Zeta Editor"]);
    let pane = Pane::new(&system);
    pane.search("zeta");
    wait_until("watching", || system.watching() == 1);

    // The watcher's buffer overflowed: it says changes were lost.
    system.set(&["Zeta Editor", "Zeta Mail"]);
    system.report(Change::Lost);
    pane.wait_for(&["Zeta Editor", "Zeta Mail"], ABOUT_A_SECOND);

    // Nothing reported at all: the periodic rescan (an hour here, by the
    // launcher's clock) finds it.
    system.set(&["Zeta Editor", "Zeta Mail", "Zeta Notes"]);
    pane.clock.advance(Duration::from_secs(3600));
    pane.wait_for(&["Zeta Editor", "Zeta Mail", "Zeta Notes"], ABOUT_A_SECOND);
}

#[test]
fn the_selected_row_stays_on_its_application_when_the_list_changes() {
    let system = FakeSystem::with(&["Zeta Editor", "Zeta Mail"]);
    let pane = Pane::new(&system);
    pane.search("zeta");
    wait_until("watching", || system.watching() == 1);
    let first = pane.selected().unwrap();
    assert_eq!(pane.launcher.view().selected, Some(0));

    // An application whose title is exactly the query ranks first.
    system.set(&["Zeta Editor", "Zeta Mail", "Zeta"]);
    system.report(Change::Changed);
    pane.wait_for(&["Zeta", "Zeta Editor", "Zeta Mail"], ABOUT_A_SECOND);
    assert_eq!(pane.listed()[0], "Zeta");

    assert_eq!(pane.selected(), Some(first), "the selection did not jump");

    // The selected application leaves: the selection stays where it was.
    let position = pane.launcher.view().selected.unwrap();
    let remaining: Vec<&str> = ["Zeta Editor", "Zeta Mail", "Zeta"]
        .into_iter()
        .filter(|name| pane.selected().as_deref() != Some(*name))
        .collect();
    system.set(&remaining);
    system.report(Change::Changed);
    wait_until("the rescan", || system.scans() == 3);
    pane.clock.advance(GRACE);
    wait_until("it leaving", || pane.listed().len() == 2);
    assert_eq!(pane.launcher.view().selected, Some(position));
}

#[test]
fn a_change_while_the_query_is_blank_is_listed_by_the_next_query() {
    let system = FakeSystem::with(&["Zeta Editor"]);
    let pane = Pane::new(&system);
    pane.search("zeta");
    wait_until("watching", || system.watching() == 1);
    pane.search("");

    system.set(&["Zeta Editor", "Zeta Mail"]);
    system.report(Change::Changed);
    wait_until("the rescan", || system.scans() == 2);
    settle();

    // The same visit of root search: the results are asked for again.
    pane.search("zeta");
    assert_eq!(pane.sorted(), ["Zeta Editor", "Zeta Mail"]);
}

#[test]
fn a_show_of_root_search_that_changed_nothing_asks_applications_for_nothing() {
    let system = FakeSystem::with(&["Zeta Editor"]);
    let clock = ManualClock::at(1_000_000);
    let asks = Arc::new(AtomicUsize::new(0));
    let list = Arc::new(Counted {
        applications: Arc::new(
            Cached::new(system.clone(), Duration::from_secs(3600)).with_clock(clock.clone()),
        ),
        asks: asks.clone(),
    });
    let pane = Pane::with_list(clock, list);

    // The first query that is not blank asks the provider for its
    // results, which call the host's list once.
    pane.search("zeta");
    assert_eq!(pane.listed(), ["Zeta Editor"]);
    wait_until("watching", || system.watching() == 1);
    let asked = asks.load(Ordering::SeqCst);
    assert!(asked >= 1, "the provider was asked for its results");

    // Showing root search again and again — the reopening that pops to
    // root — with no application change: the provider is not asked again
    // (#202); the results kept are listed as they were.
    for _ in 0..3 {
        pane.launcher.show_root_search();
        pane.search("zeta");
        assert_eq!(pane.listed(), ["Zeta Editor"]);
    }
    assert_eq!(asks.load(Ordering::SeqCst), asked);

    // An application change does ask it again, at once.
    system.set(&["Zeta Editor", "Zeta Mail"]);
    system.report(Change::Changed);
    pane.wait_for(&["Zeta Editor", "Zeta Mail"], ABOUT_A_SECOND);
    assert!(asks.load(Ordering::SeqCst) > asked);
    pane.still_searching("zeta");
}

#[test]
fn disabling_applications_stops_every_watcher_and_drops_the_list() {
    let system = FakeSystem::with(&["Zeta Editor"]);
    let pane = Pane::new(&system);
    pane.search("zeta");
    wait_until("watching", || system.watching() == 1);

    block_on(pane.launcher.set_enabled(&identity(), false));

    wait_until("every watcher to stop", || system.watching() == 0);
    assert!(pane.listed().is_empty(), "{:?}", pane.listed());
    // A change now asks nothing and lists nothing.
    let scans = system.scans();
    system.report(Change::Changed);
    settle();
    assert_eq!(system.scans(), scans);

    // Enabled again: the next query looks, and watches, again ("zeta"
    // again would be the same query, which searches nothing).
    system.set(&["Zeta Editor", "Zeta Mail"]);
    block_on(pane.launcher.set_enabled(&identity(), true));
    pane.search("zeta m");
    assert_eq!(pane.listed(), ["Zeta Mail"]);
    assert_eq!(system.scans(), scans + 1);
    wait_until("watching again", || system.watching() == 1);
}
