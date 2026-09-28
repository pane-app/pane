//! The cache in front of each system's application adapter: after the first
//! scan, the list is returned at once and refreshed off the calling thread
//! (the extension runtime's) when it is older than its maximum age, so a
//! return to root search never waits for the system's folders.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use pane_core::applications::{Application, Applications, Cached};

/// A system that counts its scans; a held scan waits until released.
#[derive(Default)]
struct CountingSystem {
    names: Mutex<Vec<&'static str>>,
    fails: Mutex<Option<String>>,
    scans: AtomicUsize,
    opened: Mutex<Vec<String>>,
    held: Mutex<bool>,
    released: Condvar,
}

impl CountingSystem {
    fn with(names: &[&'static str]) -> Arc<CountingSystem> {
        let system = CountingSystem::default();
        *system.names.lock().unwrap() = names.to_vec();
        Arc::new(system)
    }

    fn scans(&self) -> usize {
        self.scans.load(Ordering::SeqCst)
    }

    fn hold(&self) {
        *self.held.lock().unwrap() = true;
    }

    fn release(&self) {
        *self.held.lock().unwrap() = false;
        self.released.notify_all();
    }
}

impl Applications for CountingSystem {
    fn installed(&self) -> Result<Vec<Application>, String> {
        let mut held = self.held.lock().unwrap();
        while *held {
            held = self.released.wait(held).unwrap();
        }
        drop(held);
        self.scans.fetch_add(1, Ordering::SeqCst);
        if let Some(problem) = self.fails.lock().unwrap().clone() {
            return Err(problem);
        }
        Ok(self
            .names
            .lock()
            .unwrap()
            .iter()
            .map(|name| Application {
                id: format!("/apps/{name}"),
                name: (*name).into(),
                location: "/apps".into(),
            })
            .collect())
    }

    fn open(&self, id: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(id.into());
        Ok(())
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

/// Calls `installed` on another thread and fails if it does not return
/// within a second: it must not wait for a scan.
fn promptly(cached: &Arc<Cached>) -> Vec<String> {
    let (reply, answer) = mpsc::channel();
    let cached = cached.clone();
    thread::spawn(move || reply.send(names(&cached)));
    answer
        .recv_timeout(Duration::from_secs(1))
        .expect("the list was not returned at once")
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what} did not happen");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_fresh_list_is_not_scanned_again() {
    let system = CountingSystem::with(&["Files"]);
    let cached = Cached::new(system.clone(), Duration::from_secs(3600));

    assert_eq!(names(&cached), ["Files"]);
    assert_eq!(names(&cached), ["Files"]);
    assert_eq!(names(&cached), ["Files"]);

    assert_eq!(system.scans(), 1);
}

#[test]
fn an_old_list_is_returned_at_once_and_refreshed_off_the_calling_thread() {
    let system = CountingSystem::with(&["Files"]);
    let cached = Arc::new(Cached::new(system.clone(), Duration::ZERO));
    assert_eq!(names(&cached), ["Files"]);

    system.names.lock().unwrap().push("Firefox");
    system.hold();
    // Old at once: the kept list comes back while the rescan waits.
    assert_eq!(promptly(&cached), ["Files"]);
    // A second call does not start another rescan meanwhile.
    assert_eq!(promptly(&cached), ["Files"]);
    system.release();
    wait_until("the rescan", || system.scans() == 2);

    wait_until("the new list", || names(&cached) == ["Files", "Firefox"]);
}

#[test]
fn a_failed_rescan_keeps_the_list_and_a_failed_first_scan_is_an_error() {
    let system = CountingSystem::with(&["Files"]);
    *system.fails.lock().unwrap() = Some("no applications folder".into());
    let cached = Cached::new(system.clone(), Duration::ZERO);
    assert_eq!(cached.installed(), Err("no applications folder".into()));

    *system.fails.lock().unwrap() = None;
    assert_eq!(names(&cached), ["Files"]);
    *system.fails.lock().unwrap() = Some("gone".into());
    assert_eq!(names(&cached), ["Files"]);
    wait_until("the failed rescan", || system.scans() >= 3);
    assert_eq!(names(&cached), ["Files"]);
}

#[test]
fn opening_goes_to_the_system() {
    let system = CountingSystem::with(&["Files"]);
    let cached = Cached::new(system.clone(), Duration::ZERO);

    cached.open("/apps/Files").unwrap();

    assert_eq!(*system.opened.lock().unwrap(), ["/apps/Files"]);
    assert_eq!(system.scans(), 0, "opening scans nothing");
}
