//! Generations: who owns a guest call, and so when it stops.
//!
//! A [`Generation`] is one run of an installed package's code, from when the
//! package is enabled (or installed, or Pane starts) until it is disabled,
//! reloaded, updated or paused after it failed. Every call into the package, and every guest
//! instance serving one, belongs to the generation that was current when the
//! user or another extension asked for it; so does an operation call it
//! serves for another package's call, which also belongs to its caller's
//! generation through the call chain.
//!
//! When a generation ends, the runtime stops its work at once: a call not
//! started yet is not started; a call waiting inside the guest (on a timer,
//! an operation, a helper, a web request, any async import) is abandoned and
//! its instance, with everything the store holds (views, streams, futures,
//! host tasks), is dropped; a result that completes anyway is discarded; and
//! host imports refuse the stopped code (saving data, calling operations).
//! Since #18 a guest computing without awaiting yields at each epoch tick, so
//! it is stopped there too.
//!
//! Code of a runtime thread Pane gave up on (a runtime hang, #18) is stopped
//! the same way, through that thread's [`Fence`]: its generation has not
//! ended, but nothing it still runs may change anything.
//!
//! **One undo list per generation** (ADR 0041, #136). Every host subsystem
//! that sets something up for a generation records how to undo it on the
//! generation's list ([`Generation::on_end`]): a guest instance, a native
//! helper's process, a web request. When the generation ends, Pane runs the
//! list once, newest first, whatever the reason ([`Generation::end`]); a
//! teardown that fails is logged and the others still run. A subsystem that
//! undoes its work by itself first (a helper that exited, an instance that
//! was dropped) drops its [`Registration`], which takes it off the list, so
//! the list holds only what is still set up. The guest never sees it.
//!
//! A subsystem that keeps what it set up in state of its own, behind a lock
//! the thread ending the generation may hold (the launcher's schedules,
//! services and hotkeys), registers an [`EndMark`] instead: the end marks
//! the entry ended and wakes the subsystem, which drops it (or sets it up
//! again for the generation that follows) when it next looks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, Weak};

use tokio::sync::watch;

/// Why a generation ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// The user disabled the package.
    Disabled,
    /// The package's code was replaced by a reload or an update; the new
    /// code runs in a new generation.
    Replaced,
    /// The user uninstalled the package.
    Uninstalled,
    /// Pane paused the package after it failed: it could not start, or it
    /// crashed too often. Retry, a reload or an update runs
    /// it in a new generation.
    Paused,
    /// Not an end of the generation: the runtime thread running this code
    /// stopped responding and Pane gave up on it (a runtime hang). A fresh
    /// thread runs the package's next calls; this code changes nothing more.
    Abandoned,
}

/// Closed once Pane gives up on a runtime thread: code that thread runs is
/// then stopped, as if its generation had ended ([`End::Abandoned`]).
/// Cloning shares it.
///
/// A host call that changes something checks it and makes its change while
/// holding it ([`Fence::hold`]); closing waits for such a change to finish,
/// so none lands after the thread was given up on. What is held is never
/// blocking work, so closing never waits long.
#[derive(Clone, Debug, Default)]
pub(crate) struct Fence(Arc<RwLock<bool>>);

impl Fence {
    /// Whether the fence was closed.
    pub fn closed(&self) -> bool {
        *self.hold()
    }

    /// Holds the fence open, if it is, until the guard is dropped: a change
    /// made meanwhile lands before any close. The guard says whether it was
    /// closed already.
    pub fn hold(&self) -> RwLockReadGuard<'_, bool> {
        self.0
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Closes the fence, once no change held it open.
    pub fn close(&self) {
        *self
            .0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
    }
}

/// One run of an installed package's code. Cloning shares it.
#[derive(Clone, Debug)]
pub(crate) struct Generation(Arc<Shared>);

/// What every clone of one generation shares.
struct Shared {
    end: watch::Sender<Option<End>>,
    undo: Mutex<UndoList>,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Generation")
            .field("ended", &*self.end.borrow())
            .finish_non_exhaustive()
    }
}

/// A teardown on a generation's undo list.
type Teardown = Box<dyn FnOnce() -> Result<(), String> + Send>;

/// One entry of an undo list: its number, what it undoes (for the log and
/// diagnostics) and how.
type Entry = (u64, &'static str, Teardown);

/// What a generation will undo when it ends, oldest first.
#[derive(Default)]
struct UndoList {
    /// The next entry's number, never reused.
    next: u64,
    entries: Vec<Entry>,
    /// Set once the generation ended and its list was taken: a teardown
    /// registered afterwards runs at once.
    done: bool,
}

impl Generation {
    /// A generation that has not ended.
    pub fn new() -> Generation {
        Generation(Arc::new(Shared {
            end: watch::channel(None).0,
            undo: Mutex::default(),
        }))
    }

    /// Ends the generation for `why`, waking the work waiting on it. A
    /// generation ends once; ending it again keeps the first reason and
    /// undoes nothing more.
    ///
    /// The first end takes the generation's undo list: the returned
    /// [`Undo`] runs it, newest first, when it is dropped (or run), so a
    /// caller holding a lock can let it go first.
    pub fn end(&self, why: End) -> Undo {
        let first = self.0.end.send_if_modified(|end| {
            let first = end.is_none();
            if first {
                *end = Some(why);
            }
            first
        });
        if !first {
            return Undo(Vec::new());
        }
        let mut list = self.list();
        list.done = true;
        Undo(std::mem::take(&mut list.entries))
    }

    /// Why it ended, if it has.
    pub fn ended(&self) -> Option<End> {
        *self.0.end.borrow()
    }

    /// Resolves when the generation ends, with why.
    pub fn wait_end(&self) -> impl Future<Output = End> + Send + use<> {
        let mut ended = self.0.end.subscribe();
        async move {
            let end = ended.wait_for(Option::is_some).await.map(|end| *end);
            match end {
                Ok(end) => end.expect("waited for an end"),
                // The sender lives as long as this generation's clones, one
                // of which made this future, so it cannot close first.
                Err(_) => std::future::pending().await,
            }
        }
    }

    /// Records `undo`, which undoes `what` this generation set up, on its
    /// undo list: the generation's end runs it, unless the returned
    /// registration is dropped first (the subsystem undid its work itself).
    /// On a generation that has ended already it runs at once. It must not
    /// block: it runs on whichever thread ends the generation.
    pub fn on_end(
        &self,
        what: &'static str,
        undo: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) -> Registration {
        let mut list = self.list();
        if list.done {
            drop(list);
            undo_one(what, Box::new(undo));
            return Registration {
                list: Weak::new(),
                entry: 0,
            };
        }
        let entry = list.next;
        list.next += 1;
        list.entries.push((entry, what, Box::new(undo)));
        Registration {
            list: Arc::downgrade(&self.0),
            entry,
        }
    }

    /// What is on the undo list now, oldest first: what this generation
    /// still has set up. A diagnostic for tests.
    pub fn undo_list(&self) -> Vec<&'static str> {
        self.list()
            .entries
            .iter()
            .map(|(_, what, _)| *what)
            .collect()
    }

    fn list(&self) -> MutexGuard<'_, UndoList> {
        self.0
            .undo
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// One entry on a generation's undo list. Dropping it takes the entry off
/// the list without running it: the subsystem undid its work itself. Once
/// the generation ended, dropping it changes nothing.
#[derive(Debug)]
pub(crate) struct Registration {
    list: Weak<Shared>,
    entry: u64,
}

impl Drop for Registration {
    fn drop(&mut self) {
        let Some(shared) = self.list.upgrade() else {
            return;
        };
        let removed = {
            let mut list = shared
                .undo
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            list.entries
                .iter()
                .position(|(entry, _, _)| *entry == self.entry)
                .map(|at| list.entries.remove(at))
        };
        // Dropped with the list unlocked: what the teardown holds may
        // register or drop entries of its own.
        drop(removed);
    }
}

/// An entry of a subsystem's own state on its generation's undo list, for
/// a subsystem that cannot undo the entry from whichever thread ends the
/// generation (see the module docs): the end marks it ended and runs
/// `then`, which must not block (a worker woken to look again). The
/// subsystem drops an ended entry, or sets it up again for the current
/// generation, when it next looks. Dropping the mark takes it off the list.
#[derive(Debug)]
pub(crate) struct EndMark {
    ended: Arc<AtomicBool>,
    _registration: Option<Registration>,
}

impl EndMark {
    /// A mark of `what` on `generation`'s undo list; with no generation
    /// (a launcher that installs nothing), one that never ends.
    pub(crate) fn on(
        generation: Option<&Generation>,
        what: &'static str,
        then: impl FnOnce() + Send + 'static,
    ) -> EndMark {
        let ended = Arc::new(AtomicBool::new(false));
        let registration = generation.map(|generation| {
            let ended = ended.clone();
            generation.on_end(what, move || {
                ended.store(true, Ordering::SeqCst);
                then();
                Ok(())
            })
        });
        EndMark {
            ended,
            _registration: registration,
        }
    }

    /// Whether the generation it was marked on has ended.
    pub(crate) fn ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }
}

/// The undo list of a generation that just ended, run newest first when
/// dropped (see [`Generation::end`]).
#[must_use = "dropping it runs the ended generation's undo list at once: drop it once any lock \
              held is let go"]
pub(crate) struct Undo(Vec<Entry>);

impl Undo {
    /// Runs the list now, newest first, and returns why each teardown that
    /// failed did; each failure is logged too, and never stops the others.
    /// Dropping it does the same, for code that needs no answer.
    #[cfg(test)]
    pub fn run(mut self) -> Vec<String> {
        self.run_all()
    }

    fn run_all(&mut self) -> Vec<String> {
        let mut failures = Vec::new();
        while let Some((_, what, teardown)) = self.0.pop() {
            if let Some(failure) = undo_one(what, teardown) {
                failures.push(failure);
            }
        }
        failures
    }
}

impl Drop for Undo {
    fn drop(&mut self) {
        self.run_all();
    }
}

/// Runs one teardown, logging its failure (an error or a panic), which it
/// returns.
fn undo_one(what: &'static str, teardown: Teardown) -> Option<String> {
    let failure = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(teardown)) {
        Ok(Ok(())) => return None,
        Ok(Err(why)) => format!("undoing {what} at the end of its generation failed: {why}"),
        Err(_) => format!("undoing {what} at the end of its generation panicked"),
    };
    eprintln!("pane: {failure}");
    Some(failure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_end_mark_is_marked_and_woken_at_the_end_and_leaves_the_list_when_dropped() {
        let generation = Generation::new();
        let woken = Arc::new(AtomicBool::new(false));
        let wake = woken.clone();
        let mark = EndMark::on(Some(&generation), "schedule", move || {
            wake.store(true, Ordering::SeqCst)
        });
        let dropped = EndMark::on(Some(&generation), "service", || {});
        assert_eq!(generation.undo_list(), ["schedule", "service"]);
        drop(dropped);
        assert_eq!(generation.undo_list(), ["schedule"]);
        assert!(!mark.ended());
        drop(generation.end(End::Disabled));
        assert!(mark.ended() && woken.load(Ordering::SeqCst));
        assert!(generation.undo_list().is_empty());
        // Marked on an ended generation: ended at once.
        assert!(EndMark::on(Some(&generation), "hotkey", || {}).ended());
        assert!(!EndMark::on(None, "hotkey", || {}).ended());
    }

    /// A teardown that notes `name` in `ran` when it runs.
    fn noting(
        ran: &Arc<Mutex<Vec<&'static str>>>,
        name: &'static str,
    ) -> impl FnOnce() -> Result<(), String> + Send + 'static {
        let ran = ran.clone();
        move || {
            ran.lock().unwrap().push(name);
            Ok(())
        }
    }

    /// Whatever ends it, a generation runs every teardown registered on it,
    /// newest first, once, and its list is empty afterwards.
    #[test]
    fn an_end_runs_every_teardown_newest_first_once_whatever_the_reason() {
        for why in [
            End::Disabled,
            End::Replaced,
            End::Uninstalled,
            End::Paused,
            End::Abandoned,
        ] {
            let generation = Generation::new();
            let ran = Arc::new(Mutex::new(Vec::new()));
            let _instance = generation.on_end("instance", noting(&ran, "instance"));
            let _helper = generation.on_end("helper", noting(&ran, "helper"));
            let _request = generation.on_end("request", noting(&ran, "request"));
            assert_eq!(generation.undo_list(), ["instance", "helper", "request"]);

            let failures = generation.end(why).run();

            assert!(failures.is_empty(), "{failures:?}");
            assert_eq!(*ran.lock().unwrap(), ["request", "helper", "instance"]);
            assert_eq!(generation.undo_list(), Vec::<&str>::new(), "{why:?}");
            // Ending again keeps the first reason and undoes nothing more.
            drop(generation.end(End::Disabled));
            assert_eq!(generation.ended(), Some(why));
            assert_eq!(ran.lock().unwrap().len(), 3, "{why:?}");
        }
    }

    /// A teardown that fails, by an error or a panic, is logged and does
    /// not stop the others.
    #[test]
    fn a_failing_teardown_is_logged_and_the_others_still_run() {
        let generation = Generation::new();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let _first = generation.on_end("first", noting(&ran, "first"));
        let _failing = generation.on_end("a failing one", || Err("it broke".into()));
        let _panicking = generation.on_end("a panicking one", || panic!("it panicked"));
        let _last = generation.on_end("last", noting(&ran, "last"));

        let failures = generation.end(End::Disabled).run();

        assert_eq!(*ran.lock().unwrap(), ["last", "first"]);
        assert_eq!(
            failures,
            [
                "undoing a panicking one at the end of its generation panicked",
                "undoing a failing one at the end of its generation failed: it broke",
            ]
        );
        assert!(generation.undo_list().is_empty());
    }

    /// A registration dropped before the end (its subsystem undid the work
    /// itself) leaves the list and is not run; one made after the end runs
    /// at once.
    #[test]
    fn a_dropped_registration_is_not_run_and_a_late_one_runs_at_once() {
        let generation = Generation::new();
        let ran = Arc::new(Mutex::new(Vec::new()));
        let finished = generation.on_end("finished", noting(&ran, "finished"));
        let _kept = generation.on_end("kept", noting(&ran, "kept"));
        drop(finished);
        assert_eq!(generation.undo_list(), ["kept"]);

        drop(generation.end(End::Replaced));
        assert_eq!(*ran.lock().unwrap(), ["kept"]);

        let late = generation.on_end("late", noting(&ran, "late"));
        assert_eq!(*ran.lock().unwrap(), ["kept", "late"]);
        drop(late);
        assert!(generation.undo_list().is_empty());
    }
}
