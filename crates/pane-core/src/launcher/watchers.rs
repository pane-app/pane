//! Folder watchers extensions registered at run time (#158): a path,
//! recursive or not, watched through the native watcher development mode
//! uses (inotify, FSEvents, ReadDirectoryChangesW), each change delivered
//! to the component's `events` export with the watcher's tag and the
//! paths that changed, relative to the watched folder. An overflow — too
//! many changes were lost — is one "rescan" event.
//!
//! A watcher is an owned registration the guest holds: its handle
//! dropped, its instance going or its generation ending ends it (see
//! `registrations`), and the native watcher stops with the next look the
//! registry's hooks wake this thread to. A package that waits for what it
//! needs runs none of its code, so its watchers' changes are held and
//! merged, and delivered when it can run again.
//!
//! Changes are coalesced: those arriving within half a second of the
//! first are one event, whose paths are the union of them, sorted and
//! without repetition. The thread takes what the native watchers report
//! on their own thread — which only sends here — and the registry's
//! changes through its hooks, and delivers each event on a thread of its
//! own, so a slow guest delays neither the window nor the next event.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use notify::event::{EventKind, MetadataKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use super::schedules::Wake;
use super::{Launcher, WeakLauncher};
use crate::extension_data::PackageData;
use crate::registrations::{GuestEvent, WatcherChanges, WatcherOf};

/// How long changes arriving after the first are coalesced into one
/// event.
const COALESCE: Duration = Duration::from_millis(500);

/// Runs the watchers the extensions registered. Kept by the launcher;
/// dropping it stops the thread, which ends the native watchers.
pub(super) struct Watchers {
    /// The native watchers, by registration id, the changes each is
    /// coalescing, and the changes held for packages that cannot run.
    /// Locked with the launcher's state held, never the other way round.
    state: Mutex<Watching>,
    wake: Arc<Wake>,
    /// Where the native watchers' events go, and what the thread reads.
    reports: (Sender<Report>, Mutex<Receiver<Report>>),
}

/// What the watchers thread keeps.
struct Watching {
    /// One native watcher per registration still held.
    watchers: HashMap<u64, Held>,
    /// The coalesced changes of watchers whose packages cannot run,
    /// merged into one each and delivered when the package can again.
    held: HashMap<u64, WatcherChanges>,
}

/// One native watcher, for one registration.
struct Held {
    /// Held for its drop: it is what watches, and the field is never read.
    _watcher: RecommendedWatcher,
    /// The registration it watches for, as the registry holds it.
    registered: WatcherOf,
    /// The changes arriving since the first, and when that was: delivered
    /// once half a second passes without another.
    coalescing: Option<(Instant, Vec<PathBuf>, bool)>,
}

/// What a native watcher's own thread reports.
struct Report {
    /// The registration the event is of.
    id: u64,
    /// The paths that changed, or an error (an overflow).
    event: notify::Result<notify::Event>,
}

/// One event to deliver, as the thread starts a run for.
struct Delivery {
    component: PathBuf,
    data: Option<PackageData>,
    event: GuestEvent,
}

impl Watchers {
    /// The watchers thread for the packages the registry serves. Told
    /// whenever the registry or a package's generation changes, so it
    /// looks again. [`Watchers::run`] starts its thread.
    pub(super) fn start() -> Arc<Watchers> {
        let (sender, receiver) = mpsc::channel();
        Arc::new(Watchers {
            state: Mutex::new(Watching {
                watchers: HashMap::new(),
                held: HashMap::new(),
            }),
            wake: Arc::default(),
            reports: (sender, Mutex::new(receiver)),
        })
    }

    /// Starts the watchers thread, which runs until this launcher stops.
    pub(super) fn run(self: &Arc<Self>, launcher: WeakLauncher) {
        let watchers = Arc::downgrade(self);
        let started = std::thread::Builder::new()
            .name("pane-watchers".into())
            .spawn(move || until_stopped(launcher, watchers));
        if let Err(error) = started {
            eprintln!("Pane cannot run extension folder watchers in the background: {error}");
        }
    }

    /// Waits until the watchers thread looked at every change of the
    /// registry so far and every event it started to deliver has
    /// reported; `false` if it did not within `limit`. For tests and
    /// development builds, which so wait for watchers without timing
    /// them.
    pub(super) fn settled(&self, limit: Duration) -> bool {
        self.wake.settled(limit)
    }

    /// Wakes the watchers thread: something it looks at changed.
    pub(super) fn poke(&self) {
        self.wake.poke();
    }
}

/// The watchers thread: takes the native watchers' reports and the
/// registry's changes, and delivers what the packages' code may be told
/// of, one coalesced event at a time.
fn until_stopped(launcher: WeakLauncher, watchers: Weak<Watchers>) {
    while let Some(current) = watchers.upgrade() {
        let Some(seen) = current.wake.pokes() else {
            return;
        };
        // One look at everything the watchers thread watches: what the
        // registry holds, what the native watchers reported, and what the
        // packages' code may be told of.
        let deliveries = match launcher.upgrade() {
            Some(launcher) => look(&current, &launcher),
            None => Vec::new(),
        };
        current.wake.scanned(seen, deliveries.len());
        for delivery in deliveries {
            start_delivery(&current, &launcher, delivery);
        }
        let wait = next_wait(&current);
        // A report arriving within the wait wakes the thread at once.
        let reported = {
            let receiver = current
                .reports
                .1
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            receiver.recv_timeout(wait).ok()
        };
        if reported.is_some() {
            current.wake.poke();
        }
    }
}

/// One look: brings the native watchers in line with the registry, takes
/// the reports that arrived, coalesces them, closes the windows that
/// closed, and answers the events to deliver — those of packages whose
/// code may run, including the changes held while one could not.
fn look(watchers: &Watchers, launcher: &Launcher) -> Vec<Delivery> {
    // The launcher's state first, then the thread's: the order every
    // other path that takes both uses. Which packages' code may run, and
    // the data each event's call belongs to.
    let (wanted, runs, data) = {
        let state = launcher.lock();
        let wanted: Vec<WatcherOf> = state
            .registrations
            .as_ref()
            .map(|registrations| registrations.watchers())
            .unwrap_or_default();
        let runs = |registered: &WatcherOf| {
            state
                .packages
                .iter()
                .find(|package| package.identity.key() == registered.owner)
                .is_some_and(|package| state.runs(package))
        };
        let data = |registered: &WatcherOf| {
            state
                .packages
                .iter()
                .find(|package| package.identity.key() == registered.owner)
                .and_then(|package| {
                    launcher
                        .installation
                        .as_ref()
                        .map(|installation| installation.data.owned_by(&package.identity))
                })
        };
        (
            wanted.clone(),
            wanted
                .iter()
                .map(|registered| (registered.id, runs(registered)))
                .collect::<Vec<_>>(),
            wanted
                .iter()
                .map(|registered| (registered.id, data(registered)))
                .collect::<Vec<_>>(),
        )
    };
    let runs: HashMap<u64, bool> = runs.into_iter().collect();
    let data: HashMap<u64, Option<PackageData>> = data.into_iter().collect();
    // The reports that arrived, and the watchers as they now stand.
    let reports = {
        let receiver = watchers
            .reports
            .1
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut reports = Vec::new();
        while let Ok(report) = receiver.try_recv() {
            reports.push(report);
        }
        reports
    };
    let mut watching = watchers.state.lock().unwrap_or_else(|p| p.into_inner());
    // One native watcher per registration still held: one registered
    // since begins, one dropped or undone goes.
    watching
        .watchers
        .retain(|id, _| wanted.iter().any(|watcher| watcher.id == *id));
    watching
        .held
        .retain(|id, _| wanted.iter().any(|watcher| watcher.id == *id));
    for wanted in wanted {
        if watching.watchers.contains_key(&wanted.id) {
            continue;
        }
        if let Some(held) = make(watchers, &wanted) {
            watching.watchers.insert(wanted.id, held);
        }
    }
    // The reports join their watchers' coalescing.
    for report in reports {
        let Some(held) = watching.watchers.get_mut(&report.id) else {
            continue;
        };
        let entry = held
            .coalescing
            .get_or_insert_with(|| (Instant::now(), Vec::new(), false));
        match report.event {
            Ok(event) => {
                // Reading a file is not a change of it, as the build
                // watcher judges it.
                let read = matches!(
                    event.kind,
                    EventKind::Access(_)
                        | EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))
                );
                let rescan = event.need_rescan();
                if !read {
                    entry.1.extend(event.paths);
                }
                entry.2 |= rescan;
            }
            Err(_) => entry.2 = true,
        }
    }
    let mut deliveries = Vec::new();
    // The windows that closed, as one event each; those of packages whose
    // code may not run are held, merged with what is held already.
    let closed: Vec<(u64, WatcherChanges)> = {
        let mut closed = Vec::new();
        for (id, held) in watching.watchers.iter_mut() {
            let Some((since, paths, rescan)) = held.coalescing.take() else {
                continue;
            };
            if since.elapsed() < COALESCE {
                held.coalescing = Some((since, paths, rescan));
                continue;
            }
            closed.push((*id, coalesced(&held.registered.path, paths, rescan)));
        }
        closed
    };
    for (id, changes) in closed {
        let Some(held) = watching.watchers.get(&id) else {
            continue;
        };
        if runs.get(&id).copied().unwrap_or(false) {
            deliveries.push(Delivery {
                component: held.registered.component.clone(),
                data: data.get(&id).cloned().flatten(),
                event: GuestEvent::Watcher {
                    tag: held.registered.tag.clone(),
                    changes,
                },
            });
        } else {
            // None of its code runs: held, merged with what is held
            // already, and delivered as one when it can run again.
            merge(watching.held.entry(id).or_insert(changes.clone()), changes);
        }
    }
    // The held changes of packages whose code may run again are
    // delivered, one merged event each.
    let deliverable: Vec<u64> = watching
        .held
        .keys()
        .copied()
        .filter(|id| runs.get(id).copied().unwrap_or(false))
        .collect();
    for id in deliverable {
        let changes = watching.held.remove(&id);
        let Some(held) = watching.watchers.get(&id) else {
            continue;
        };
        if let Some(changes) = changes {
            deliveries.push(Delivery {
                component: held.registered.component.clone(),
                data: data.get(&id).cloned().flatten(),
                event: GuestEvent::Watcher {
                    tag: held.registered.tag.clone(),
                    changes,
                },
            });
        }
    }
    deliveries
}

/// The native watcher for `registered`, or `None` if none could be made.
fn make(watchers: &Watchers, registered: &WatcherOf) -> Option<Held> {
    let sender = watchers.reports.0.clone();
    let id = registered.id;
    let path = registered.path.clone();
    let recursive = registered.recursive;
    let mut native = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        // The native watcher's own thread: sent on, never worked on
        // here.
        let _ = sender.send(Report { id, event });
    })
    .ok()?;
    let mode = if recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };
    native.watch(&path, mode).ok()?;
    Some(Held {
        _watcher: native,
        registered: registered.clone(),
        coalescing: None,
    })
}

/// The coalesced changes of one closed window: the paths that changed,
/// relative to the watched folder, sorted and without repetition, or a
/// rescan when the watcher overflowed.
fn coalesced(root: &Path, paths: Vec<PathBuf>, rescan: bool) -> WatcherChanges {
    if rescan {
        return WatcherChanges::Rescan;
    }
    // The paths inside the watched folder, relative to it, come first,
    // sorted; the rest — renamed into or out of the folder, say — keep
    // their full paths, after them.
    let mut inside: Vec<PathBuf> = Vec::new();
    let mut outside: Vec<PathBuf> = Vec::new();
    for path in paths {
        match path.strip_prefix(root).map(Path::to_path_buf) {
            Ok(relative) => {
                if !inside.contains(&relative) {
                    inside.push(relative);
                }
            }
            Err(_) => {
                if !outside.contains(&path) {
                    outside.push(path);
                }
            }
        }
    }
    inside.sort();
    outside.sort();
    WatcherChanges::Paths(
        inside
            .into_iter()
            .chain(outside)
            .map(|path| path.display().to_string())
            .collect(),
    )
}

/// Merges `changes` into `held`: the union of the paths, or a rescan.
fn merge(held: &mut WatcherChanges, changes: WatcherChanges) {
    match (&mut *held, changes) {
        (_, WatcherChanges::Rescan) => *held = WatcherChanges::Rescan,
        (WatcherChanges::Rescan, _) => {}
        (WatcherChanges::Paths(held), WatcherChanges::Paths(paths)) => {
            for path in paths {
                if !held.contains(&path) {
                    held.push(path);
                }
            }
        }
    }
}

/// Spawns the thread that delivers `delivery`, and reports it: its call
/// belongs to the generation the look took it in, so disabling, reloading,
/// updating, uninstalling or pausing the package meanwhile stops it.
fn start_delivery(watchers: &Watchers, launcher: &WeakLauncher, delivery: Delivery) {
    let wake = watchers.wake.clone();
    let launcher = launcher.clone();
    let started = std::thread::Builder::new()
        .name("pane-watcher-event".into())
        .spawn(move || {
            if let Some(launcher) = launcher.upgrade()
                && let Ok(runtime) = launcher.runtime()
            {
                let runtime = runtime.clone();
                let _ = futures::executor::block_on(runtime.event_with(
                    &delivery.component,
                    delivery.event,
                    delivery.data,
                ));
            }
            wake.finished();
        });
    if let Err(error) = started {
        eprintln!("Pane could not deliver a folder watcher's changes: {error}");
        watchers.wake.finished();
    }
}

/// How long the watchers thread may wait before it looks again: until the
/// next coalescing window closes, at most the window itself.
fn next_wait(watchers: &Watchers) -> Duration {
    let watching = watchers.state.lock().unwrap_or_else(|p| p.into_inner());
    let soonest = watching
        .watchers
        .values()
        .filter_map(|held| held.coalescing.as_ref().map(|(since, ..)| since.elapsed()))
        .min();
    match soonest {
        Some(elapsed) if elapsed < COALESCE => COALESCE - elapsed,
        _ => COALESCE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The changes of a closed window are the paths that changed,
    /// relative to the watched folder, sorted and without repetition; a
    /// path outside it is said as it is.
    #[test]
    fn a_closed_window_lists_its_paths() {
        let changes = coalesced(
            &PathBuf::from("/somewhere/watched"),
            vec![
                PathBuf::from("/somewhere/watched/b.txt"),
                PathBuf::from("/somewhere/watched/a.txt"),
                PathBuf::from("/somewhere/watched/b.txt"),
                PathBuf::from("/elsewhere/c.txt"),
            ],
            false,
        );
        assert_eq!(
            changes,
            WatcherChanges::Paths(vec![
                "a.txt".to_owned(),
                "b.txt".to_owned(),
                "/elsewhere/c.txt".to_owned(),
            ])
        );
    }

    /// An overflow is one "rescan" event, whatever paths arrived with it.
    #[test]
    fn an_overflow_is_one_rescan_event() {
        let mut held: HashMap<u64, WatcherChanges> = HashMap::new();
        let changes = WatcherChanges::Paths(vec!["a.txt".into()]);
        merge(held.entry(1).or_insert(changes.clone()), changes);
        merge(held.get_mut(&1).unwrap(), WatcherChanges::Rescan);
        merge(
            held.get_mut(&1).unwrap(),
            WatcherChanges::Paths(vec!["b.txt".into()]),
        );
        assert_eq!(held.get(&1), Some(&WatcherChanges::Rescan));
    }
}
