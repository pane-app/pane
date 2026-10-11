//! Timers extensions registered at run time (#158): `after` (one firing)
//! and `every` (one every interval), from 1 second to 30 days, each firing
//! a call into the component's `events` export with the timer's tag.
//!
//! A timer is an owned registration the guest holds: its handle dropped,
//! its instance going or its generation ending ends it (see
//! `registrations`), and this thread stops firing it with the next look
//! the registry's hooks wake it to. The timer belongs to the generation
//! of the code that registered it: each firing is a guest call of it,
//! stopped by a disable, a reload, an update, an uninstall or a pause,
//! and a trap in it is a crash of the package like any call's, counted
//! towards pausing it.
//!
//! Firings are coalesced, never replayed: firings that fall due while one
//! is pending, or while the package waits for what it needs (none of its
//! code runs then), are one firing, delivered when the pending one
//! answers or the package can run again. An `every` timer's interval then
//! begins again at that firing; an `after` timer fires once.
//!
//! The thread follows the launcher's clock (the system's, or the one a
//! test or a development build gives), as the scheduler does: it wakes
//! when a firing is due, when the clock is set other than by time
//! passing, when a package's generation changes and when the registry
//! changes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use super::schedules::Wake;
use super::{Launcher, WeakLauncher};
use crate::clipboard::Clock;
use crate::extension_data::PackageData;
use crate::registrations::{GuestEvent, TimerOf};

/// The longest the timers thread waits before it looks again, so that a
/// change of the system's time, or a computer waking from sleep, delays a
/// firing by at most this much (what is due is always fired when it is
/// looked at).
const MAX_WAIT: Duration = Duration::from_secs(3600);

/// Runs the timers the extensions registered. Kept by the launcher;
/// dropping it stops the thread.
pub(super) struct Timers {
    /// The clock the timers follow, and where each timer stands. Locked
    /// with the launcher's state held, never the other way round.
    state: Mutex<Timing>,
    wake: Arc<Wake>,
}

/// What the timers thread keeps: the clock it follows and one entry per
/// timer still held.
struct Timing {
    clock: Arc<dyn Clock>,
    entries: HashMap<u64, Entry>,
}

/// One timer, as the thread runs it: the registration it fires for, and
/// where its interval stands.
struct Entry {
    /// The timer's registration, as the registry holds it.
    timer: TimerOf,
    /// When this run of the timer began, in clock milliseconds: a tick is
    /// due every `seconds` from then.
    started: u64,
    /// The next tick, in clock milliseconds.
    next: u64,
    /// Whether a firing has been asked for and has not answered: no
    /// second one is asked for meanwhile.
    in_flight: bool,
    /// Whether a firing fell due while one was pending or the package
    /// waited, to be delivered as one when it can: firings are coalesced,
    /// not replayed.
    owed: bool,
    /// Whether an `after` timer has fired: it fires once, and its
    /// registration lives on until it is dropped.
    spent: bool,
}

/// One firing the timers thread starts.
struct Run {
    /// The registration the firing is of, so its entry is freed when the
    /// call answers.
    timer: TimerOf,
    component: PathBuf,
    /// The extension data of the package's current generation: the firing
    /// belongs to it.
    data: Option<PackageData>,
    event: GuestEvent,
}

impl Timers {
    /// The timers thread for the packages the registry serves, following
    /// `clock`: told whenever the registry or a package's generation
    /// changes, so it looks again. [`Timers::run`] starts its thread.
    pub(super) fn start(clock: Arc<dyn Clock>) -> Arc<Timers> {
        let timers = Arc::new(Timers {
            state: Mutex::new(Timing {
                clock: clock.clone(),
                entries: HashMap::new(),
            }),
            wake: Arc::default(),
        });
        clock.on_change(Box::new(waking(Arc::downgrade(&timers))));
        timers
    }

    /// Starts the timers thread, which runs until this launcher stops.
    pub(super) fn run(self: &Arc<Self>, launcher: WeakLauncher) {
        let timers = Arc::downgrade(self);
        let started = std::thread::Builder::new()
            .name("pane-timers".into())
            .spawn(move || until_stopped(launcher, timers));
        if let Err(error) = started {
            eprintln!("Pane cannot run extension timers in the background: {error}");
        }
    }

    /// The clock the timers follow from now on ([`Launcher::with_clock`]),
    /// standing where it stands: each timer's interval begins again from
    /// now, as a service's phase does. A firing still in flight stays
    /// marked.
    pub(super) fn follow(self: &Arc<Self>, clock: Arc<dyn Clock>) {
        {
            let mut timing = self.lock();
            let now = clock.now();
            timing.clock = clock.clone();
            for entry in timing.entries.values_mut() {
                let every_ms = entry.timer.seconds * 1000;
                entry.started = now;
                entry.next = now.saturating_add(every_ms);
            }
        }
        clock.on_change(Box::new(waking(Arc::downgrade(self))));
        self.wake.poke();
    }

    /// Waits until the timers thread looked at every change of the clock
    /// and the registry so far, and every firing it started has reported;
    /// `false` if it did not within `limit`. For tests and development
    /// builds, which so wait for timers without timing them.
    pub(super) fn settled(&self, limit: Duration) -> bool {
        self.wake.settled(limit)
    }

    /// Wakes the timers thread: the registry or a package's generation
    /// changed, so it looks again.
    pub(super) fn poke(&self) {
        self.wake.poke();
    }

    fn lock(&self) -> MutexGuard<'_, Timing> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Every firing that is due now, as the registry and the packages
    /// stand: a timer registered since begins here, one dropped or undone
    /// goes, and one whose code was replaced begins again. The launcher's
    /// state is locked while they are chosen, as when the user acts, so
    /// each firing belongs to the generation current now. A timer of a
    /// package whose code may not run (it waits for what it needs) owes
    /// one firing, delivered when it can run again; so does one whose
    /// firing is still pending.
    fn due(&self, launcher: &Launcher) -> Vec<Run> {
        // A poisoned state is not recovered here (a plain lock, not the
        // launcher's recovering one): a thread that panicked while holding
        // it is recovered by the next caller that changes something.
        let state = match launcher.state.lock() {
            Ok(state) => state,
            Err(_) => return Vec::new(),
        };
        let timers: Vec<TimerOf> = state
            .registrations
            .as_ref()
            .map(|registrations| registrations.timers())
            .unwrap_or_default();
        let mut timing = self.lock();
        let now = timing.clock.now();
        timing
            .entries
            .retain(|id, _| timers.iter().any(|timer| timer.id == *id));
        let mut due = Vec::new();
        for timer in timers {
            let entry = timing
                .entries
                .entry(timer.id)
                .or_insert_with(|| Entry::begins(timer.clone(), now));
            if entry.timer != timer {
                // The same registration is another timer now (its code
                // was replaced): it begins again.
                *entry = Entry::begins(timer.clone(), now);
            }
            let every_ms = entry.timer.seconds * 1000;
            if entry.spent {
                continue;
            }
            if entry.in_flight {
                // A tick that came due meanwhile is owed as one firing,
                // not replayed.
                entry.owed |= now >= entry.next;
                continue;
            }
            // None of a waiting package's code runs: the firing it owes is
            // delivered when it can run again.
            let runs = state
                .packages
                .iter()
                .find(|package| package.identity.key() == timer.owner)
                .is_some_and(|package| state.runs(package));
            if !runs {
                entry.owed |= now >= entry.next;
                continue;
            }
            if entry.owed || now >= entry.next {
                let event = GuestEvent::Timer {
                    tag: entry.timer.tag.clone(),
                };
                if entry.timer.after {
                    // An `after` timer fires once; its registration lives
                    // on until it is dropped.
                    entry.spent = true;
                } else if entry.owed {
                    // The firing it owed while it could not run: one
                    // firing, and the interval begins again from now, so
                    // the intervals it missed are not replayed.
                    entry.next = now.saturating_add(every_ms);
                } else {
                    // From the firing before, not from now: a clock that
                    // jumped ahead fires every interval it covered, one
                    // firing at a time.
                    entry.next = entry.next.saturating_add(every_ms);
                }
                entry.owed = false;
                entry.in_flight = true;
                due.push(Run {
                    timer: entry.timer.clone(),
                    component: entry.timer.component.clone(),
                    data: launcher.data_in(&state, &entry.timer.component),
                    event,
                });
            }
        }
        due
    }

    /// Notes that the firing of the timer with registration id `id` is
    /// over — its call answered, or the thread running it could not be
    /// started — so it may fire again, and a firing that fell due
    /// meanwhile is looked at now.
    fn fired(&self, id: u64) {
        if let Some(entry) = self.lock().entries.get_mut(&id) {
            entry.in_flight = false;
        }
        self.wake.finished();
    }

    /// How long the timers thread may wait before it looks again: until
    /// the next tick of any timer, at most [`MAX_WAIT`].
    fn next_wait(&self) -> Duration {
        let timing = self.lock();
        let now = timing.clock.now();
        let soonest = timing
            .entries
            .values()
            .filter(|entry| !entry.spent)
            .map(|entry| entry.next.saturating_sub(now))
            .min()
            .unwrap_or(u64::MAX);
        Duration::from_millis(soonest).min(MAX_WAIT)
    }
}

impl Entry {
    /// A timer running `timer` that begins `now`: its first firing is due
    /// one interval later, an `every` one's owed firing at once.
    fn begins(timer: TimerOf, now: u64) -> Entry {
        let next = now.saturating_add(timer.seconds * 1000);
        Entry {
            timer,
            started: now,
            next,
            in_flight: false,
            owed: false,
            spent: false,
        }
    }
}

impl Drop for Timers {
    fn drop(&mut self) {
        self.wake.stop();
    }
}

/// A closure that pokes the timers thread `weak` names awake, holding it
/// weakly so that a clock or the registry does not keep it running past
/// the launcher.
fn waking(weak: Weak<Timers>) -> impl Fn() + Send + Sync + 'static {
    move || {
        if let Some(timers) = weak.upgrade() {
            timers.wake.poke();
        }
    }
}

/// The timers thread: starts the firings that are due, then waits until
/// one is, something changes, or this Pane stops.
fn until_stopped(launcher: WeakLauncher, timers: Weak<Timers>) {
    while let Some(current) = timers.upgrade() {
        let Some(seen) = current.wake.pokes() else {
            return;
        };
        let due = launcher.upgrade().map(|launcher| current.due(&launcher));
        let due = due.unwrap_or_default();
        current.wake.scanned(seen, due.len());
        for run in due {
            start_firing(&current, &launcher, run);
        }
        let wait = current.next_wait();
        if !current.wake.wait(seen, wait) {
            return;
        }
    }
}

/// Spawns the thread that runs `run` and reports it. The timers thread
/// never waits for it: a slow firing delays neither the window nor the
/// next look for due work (the runtime serves one guest call at a time,
/// and stops it when its generation ends).
fn start_firing(timers: &Arc<Timers>, launcher: &WeakLauncher, run: Run) {
    let weak = Arc::downgrade(timers);
    let launcher = launcher.clone();
    let retry = run.timer.id;
    let started = std::thread::Builder::new()
        .name("pane-timer-firing".into())
        .spawn(move || fire_once(weak, launcher, run));
    if let Err(error) = started {
        eprintln!("Pane could not fire an extension timer: {error}");
        timers.fired(retry);
    }
}

/// Fires one timer's event, and reports the firing: its call belongs to
/// the generation the timers thread took when it asked for it, so
/// disabling, reloading, updating, uninstalling or pausing the package
/// meanwhile stops it.
fn fire_once(timers: Weak<Timers>, launcher: WeakLauncher, run: Run) {
    let id = run.timer.id;
    if let Some(launcher) = launcher.upgrade()
        && let Ok(runtime) = launcher.runtime()
    {
        let runtime = runtime.clone();
        let _ =
            futures::executor::block_on(runtime.event_with(&run.component, run.event, run.data));
    }
    if let Some(timers) = timers.upgrade() {
        timers.fired(id);
    }
}
