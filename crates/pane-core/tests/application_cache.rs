//! The host's live list in front of each system's application adapter
//! (ADR 0038), over a fake system whose sources, watcher and clock the
//! tests drive: after the first scan the list is returned at once and is
//! watched; a reported change is rescanned once it settles and told; an
//! application whose last source went stays for its grace by the list's
//! clock, and one that comes back meanwhile never leaves; lost changes and
//! the reconciling period rescan in full; a failed rescan keeps the list;
//! releasing it drops the list and stops watching; and an application's
//! id, or the path that was its id before identities, finds the source
//! that opens it. The launcher's side (asking for results again, the
//! selection, disabling) is in `application_changes.rs`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use pane_core::applications::{
    Applications, Cached, Change, Changes, DEBOUNCE, Discovery, GRACE, Key, Source, Watch,
};
use pane_core::clipboard::ManualClock;

/// A system that counts its scans, and whose watcher the test reports
/// through.
#[derive(Default)]
struct FakeSystem {
    names: Mutex<Vec<&'static str>>,
    fails: Mutex<Option<String>>,
    scans: AtomicUsize,
    opened: Mutex<Vec<String>>,
    /// Where the latest watch reports.
    reports: Mutex<Option<Changes>>,
    /// How many watches were started, and how many are still held.
    watches_started: AtomicUsize,
    watching: Arc<AtomicUsize>,
    /// How many watches to start partial, missing a place.
    partial: AtomicUsize,
}

/// Held by a watch: counts it as watching until dropped.
struct Watching(Arc<AtomicUsize>);

impl Drop for Watching {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl FakeSystem {
    fn with(names: &[&'static str]) -> Arc<FakeSystem> {
        let system = FakeSystem::default();
        *system.names.lock().unwrap() = names.to_vec();
        Arc::new(system)
    }

    fn scans(&self) -> usize {
        self.scans.load(Ordering::SeqCst)
    }

    fn watching(&self) -> usize {
        self.watching.load(Ordering::SeqCst)
    }

    fn set(&self, names: &[&'static str]) {
        *self.names.lock().unwrap() = names.to_vec();
    }

    /// What the system's watcher would tell after a change.
    fn report(&self, change: Change) {
        let reports = self.reports.lock().unwrap().clone();
        reports.expect("the list watches the system")(change);
    }
}

fn path(name: &str) -> String {
    format!("/apps/{name}")
}

impl Discovery for FakeSystem {
    fn sources(&self) -> Result<Vec<Source>, String> {
        self.scans.fetch_add(1, Ordering::SeqCst);
        if let Some(problem) = self.fails.lock().unwrap().clone() {
            return Err(problem);
        }
        Ok(self
            .names
            .lock()
            .unwrap()
            .iter()
            .map(|name| Source::new(Key::Path(path(name)), path(name), *name, "/apps", 0))
            .collect())
    }

    fn open(&self, path: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(path.into());
        Ok(())
    }

    fn watch(&self, changes: Changes) -> Result<Watch, String> {
        *self.reports.lock().unwrap() = Some(changes);
        self.watches_started.fetch_add(1, Ordering::SeqCst);
        self.watching.fetch_add(1, Ordering::SeqCst);
        let held = Watching(self.watching.clone());
        let partial = self
            .partial
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok();
        Ok(if partial {
            Watch::partial(held)
        } else {
            Watch::new(held)
        })
    }
}

fn names(cached: &Cached) -> Vec<String> {
    let mut names: Vec<String> = cached
        .installed()
        .unwrap()
        .into_iter()
        .map(|app| app.name)
        .collect();
    names.sort();
    names
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what} did not happen");
        thread::sleep(Duration::from_millis(5));
    }
}

/// A list of `system`'s applications told by `clock`, reconciled hourly.
fn live(system: &Arc<FakeSystem>, clock: &Arc<ManualClock>) -> Cached {
    Cached::new(system.clone(), Duration::from_secs(3600)).with_clock(clock.clone())
}

/// Counts how often `cached` told of a change.
fn counting(cached: &Cached) -> Arc<AtomicUsize> {
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    cached.on_change(Arc::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    }));
    count
}

#[test]
fn nothing_is_scanned_or_watched_until_asked_and_then_once() {
    let system = FakeSystem::with(&["Files"]);
    let cached = live(&system, &ManualClock::at(0));
    thread::sleep(Duration::from_millis(50));
    assert_eq!((system.scans(), system.watching()), (0, 0));

    assert_eq!(names(&cached), ["Files"]);
    wait_until("watching", || system.watching() == 1);
    assert_eq!(names(&cached), ["Files"]);
    assert_eq!(names(&cached), ["Files"]);

    assert_eq!(system.scans(), 1);
}

#[test]
fn a_reported_change_is_rescanned_once_it_settles_and_told() {
    let system = FakeSystem::with(&["Files"]);
    let cached = live(&system, &ManualClock::at(0));
    let told = counting(&cached);
    names(&cached);
    wait_until("watching", || system.watching() == 1);

    system.set(&["Files", "Firefox"]);
    let reported = Instant::now();
    for _ in 0..3 {
        system.report(Change::Changed);
    }
    // Not before the changes settle.
    assert_eq!(system.scans(), 1);

    wait_until("the new list", || names(&cached) == ["Files", "Firefox"]);
    assert!(reported.elapsed() >= DEBOUNCE, "{:?}", reported.elapsed());
    // One rescan for the burst, told once.
    assert_eq!(system.scans(), 2);
    wait_until("the listeners told", || told.load(Ordering::SeqCst) == 1);
}

#[test]
fn a_rescan_that_finds_the_same_applications_tells_nothing() {
    let system = FakeSystem::with(&["Files"]);
    let cached = live(&system, &ManualClock::at(0));
    let told = counting(&cached);
    names(&cached);
    wait_until("watching", || system.watching() == 1);

    system.report(Change::Changed);
    wait_until("the rescan", || system.scans() == 2);
    thread::sleep(Duration::from_millis(50));

    assert_eq!(told.load(Ordering::SeqCst), 0);
}

#[test]
fn an_application_whose_last_source_went_leaves_after_its_grace_by_the_clock() {
    let clock = ManualClock::at(0);
    let system = FakeSystem::with(&["Files", "Firefox"]);
    let cached = live(&system, &clock);
    let told = counting(&cached);
    names(&cached);
    wait_until("watching", || system.watching() == 1);

    system.set(&["Files"]);
    system.report(Change::Changed);
    wait_until("the rescan", || system.scans() == 2);
    // Still listed, and so still opened, in its grace.
    assert_eq!(names(&cached), ["Files", "Firefox"]);
    cached.open(&path("Firefox")).unwrap();

    clock.advance(GRACE - Duration::from_millis(1));
    thread::sleep(Duration::from_millis(100));
    assert_eq!(names(&cached), ["Files", "Firefox"]);
    assert_eq!(told.load(Ordering::SeqCst), 0);

    clock.advance(Duration::from_millis(1));
    wait_until("it leaving", || names(&cached) == ["Files"]);
    wait_until("the listeners told", || told.load(Ordering::SeqCst) == 1);
    assert_eq!(
        cached.open(&Key::Path(path("Firefox")).id()),
        Err("it is no longer installed".into())
    );
}

#[test]
fn an_application_reinstalled_within_its_grace_never_leaves() {
    let clock = ManualClock::at(0);
    let system = FakeSystem::with(&["Files", "Firefox"]);
    let cached = live(&system, &clock);
    let told = counting(&cached);
    names(&cached);
    wait_until("watching", || system.watching() == 1);

    // An update uninstalls it...
    system.set(&["Files"]);
    system.report(Change::Changed);
    wait_until("the first rescan", || system.scans() == 2);
    clock.advance(GRACE / 2);
    // ...and installs it again.
    system.set(&["Files", "Firefox"]);
    system.report(Change::Changed);
    wait_until("the second rescan", || system.scans() == 3);

    clock.advance(GRACE * 4);
    thread::sleep(Duration::from_millis(100));
    assert_eq!(names(&cached), ["Files", "Firefox"]);
    assert_eq!(told.load(Ordering::SeqCst), 0, "the list never changed");
}

#[test]
fn lost_changes_rescan_at_once_and_a_completing_change_again_later() {
    let system = FakeSystem::with(&["Files"]);
    let cached = live(&system, &ManualClock::at(0));
    names(&cached);
    wait_until("watching", || system.watching() == 1);

    system.set(&["Files", "Firefox"]);
    system.report(Change::Lost);
    wait_until("the new list", || names(&cached) == ["Files", "Firefox"]);

    // A package being registered: rescanned once it settles, and again
    // a few seconds later, when the system completed it.
    let scans = system.scans();
    system.report(Change::Completing);
    wait_until("the rescan", || system.scans() == scans + 1);
    system.set(&["Files", "Firefox", "Calculator"]);
    wait_until("the second rescan", || {
        names(&cached) == ["Calculator", "Files", "Firefox"]
    });
    assert_eq!(system.scans(), scans + 2);
}

#[test]
fn the_list_is_reconciled_every_period_by_the_clock_whatever_was_reported() {
    let clock = ManualClock::at(0);
    let system = FakeSystem::with(&["Files"]);
    let cached = Cached::new(system.clone(), Duration::from_secs(60)).with_clock(clock.clone());
    names(&cached);
    wait_until("watching", || system.watching() == 1);

    // Installed without the watcher seeing it.
    system.set(&["Files", "Firefox"]);
    clock.advance(Duration::from_secs(59));
    thread::sleep(Duration::from_millis(100));
    assert_eq!(system.scans(), 1);

    clock.advance(Duration::from_secs(1));
    wait_until("the new list", || names(&cached) == ["Files", "Firefox"]);
}

#[test]
fn a_failed_rescan_keeps_the_list_and_a_failed_first_scan_is_an_error() {
    let system = FakeSystem::with(&["Files"]);
    *system.fails.lock().unwrap() = Some("no applications folder".into());
    let cached = live(&system, &ManualClock::at(0));
    assert_eq!(cached.installed(), Err("no applications folder".into()));
    assert_eq!(system.watching(), 0);

    *system.fails.lock().unwrap() = None;
    assert_eq!(names(&cached), ["Files"]);
    wait_until("watching", || system.watching() == 1);
    *system.fails.lock().unwrap() = Some("gone".into());
    system.report(Change::Changed);
    wait_until("the failed rescan", || system.scans() == 3);
    assert_eq!(names(&cached), ["Files"]);
}

#[test]
fn releasing_drops_the_list_and_stops_watching_until_asked_again() {
    let system = FakeSystem::with(&["Files"]);
    let cached = live(&system, &ManualClock::at(0));
    names(&cached);
    wait_until("watching", || system.watching() == 1);
    assert!(cached.current_id(&path("Files")).is_some());

    cached.release();
    wait_until("the watch to stop", || system.watching() == 0);
    assert_eq!(cached.current_id(&path("Files")), None);

    system.set(&["Files", "Firefox"]);
    assert_eq!(names(&cached), ["Files", "Firefox"]);
    assert_eq!(system.scans(), 2);
    wait_until("watching again", || system.watching() == 1);
}

#[test]
fn dropping_the_list_stops_watching() {
    let system = FakeSystem::with(&["Files"]);
    let cached = live(&system, &ManualClock::at(0));
    names(&cached);
    wait_until("watching", || system.watching() == 1);

    drop(cached);

    wait_until("the watch to stop", || system.watching() == 0);
}

#[test]
fn a_watch_missing_a_place_is_made_again_after_a_rescan() {
    let system = FakeSystem::with(&["Files"]);
    system.partial.store(1, Ordering::SeqCst);
    let cached = live(&system, &ManualClock::at(0));
    names(&cached);
    wait_until("watching", || system.watching() == 1);

    system.report(Change::Changed);
    wait_until("the watch made again", || {
        system.watches_started.load(Ordering::SeqCst) == 2
    });
    // The new one replaced the old one, and is complete: no third.
    wait_until("one watch", || system.watching() == 1);
    system.report(Change::Changed);
    wait_until("the rescan", || system.scans() == 3);
    thread::sleep(Duration::from_millis(50));
    assert_eq!(system.watches_started.load(Ordering::SeqCst), 2);
}

#[test]
fn opening_finds_the_application_by_its_id_or_its_path_from_before() {
    let system = FakeSystem::with(&["Files"]);
    let cached = live(&system, &ManualClock::at(0));
    let id = Key::Path(path("Files")).id();

    // Nothing was listed yet: a scan finds the id, and is neither kept nor
    // watched (that is for the packages asking for the list).
    cached.open(&id).unwrap();
    assert_eq!(*system.opened.lock().unwrap(), ["/apps/Files"]);
    assert_eq!(system.scans(), 1);
    assert_eq!(cached.current_id(&path("Files")), None);
    thread::sleep(Duration::from_millis(50));
    assert_eq!(system.watching(), 0);

    // Once listed, the path that was its id before identities opens it
    // too, from the kept list.
    names(&cached);
    cached.open(&path("Files")).unwrap();
    assert_eq!(system.scans(), 2);
    assert_eq!(cached.current_id(&path("Files")), Some(id));

    // An id no application has now.
    assert_eq!(
        cached.open(&Key::Path(path("Gone")).id()),
        Err("it is no longer installed".into())
    );
    // A path no application has now: the adapter says why it cannot open it.
    cached.open("/apps/Gone").unwrap();
    assert_eq!(
        *system.opened.lock().unwrap(),
        ["/apps/Files", "/apps/Files", "/apps/Gone"]
    );
}
