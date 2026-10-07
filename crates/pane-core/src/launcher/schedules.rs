//! Scheduled work: Pane runs a command's action, or a no-view command
//! itself, on the interval its package's manifest declares, while the
//! package's code may run.
//!
//! A command's `pane.json` entry declares a schedule: an interval and, for
//! a view command, the item whose action runs. A no-view command's
//! schedule names no item: each run is the command itself, launched in the
//! background from its schedule (its `run` with a `background` launch
//! record, see `launching`), and shows nothing, as no window was shown for
//! it. The scheduler is a thread of Pane's own, which
//! looks for due work, and threads of their own run it, so neither the
//! window nor the scheduler ever waits for a guest. It is driven by the
//! launcher's clock ([`crate::clipboard::Clock`]): the system's clock, or
//! the one tests and development builds give through
//! [`Launcher::with_clock`], which tells it when the clock is set other
//! than by time passing. It wakes when a run is due, then, and whenever a
//! package's generation changes (every path that ends or begins one tells
//! the extension data's change hooks).
//!
//! The schedule belongs to the package's generation: it begins when the
//! code may run — Pane starts, the package is installed or enabled, or its
//! code is replaced — and ends when it may not: the package is disabled,
//! uninstalled, paused after it failed, or its code is replaced by a
//! reload or an update, which begins a new one. Installation alone
//! schedules nothing that is not enabled, and a schedule never runs work
//! of a package that may not run. Work that fell due while Pane was not
//! running, or while the package could not run, is not kept or replayed:
//! the interval restarts when the code may run again, and the first run is
//! one full interval after that. The declaration is the manifest, so a
//! restart schedules again whatever it declares for a package still
//! enabled. Each command's schedule is on its generation's undo list
//! ("schedule", see `generation`): the end marks it ended and wakes the
//! scheduler, which drops it, or keeps it on the next generation's list.
//!
//! Each run is the command's action of the item the schedule names, asked
//! for as the user asking for it would: through the runtime (the command's
//! tree, then the item's action's callback, see `Runtime::run_item`), with the
//! extension data of the generation current when the scheduler asked, so
//! an end of that generation stops a run still pending and discards its
//! late answer (see `generations`), and a run that traps is a crash of the
//! package like any action ([`pausing`]). At most one run of one command
//! is asked for at a time: ticks that fall due while one runs are coalesced
//! into the next run, which starts at the next tick after it answers. The
//! answer is shown on the command's screen, as an action's answer is,
//! while that screen is the one on display, and the command's list is then
//! asked for again, as after an action the user chose; the run happens
//! whether or not it is.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use super::{Launcher, Screen, Status, WeakLauncher, stopped};
use crate::clipboard::Clock;
use crate::extension_data::PackageData;
use crate::generation::EndMark;
use crate::launch::LaunchRecord;
use crate::runtime::{Answer, CallError};

/// The longest the scheduler waits before it looks again, so that a change
/// of the system's time, or a computer waking from sleep, delays a run by
/// at most this much (what is due is always run when it is looked at).
const MAX_WAIT: Duration = Duration::from_secs(3600);

/// Runs the scheduled work of the launcher's installed commands. Kept by
/// the launcher; dropping it stops the thread.
pub(super) struct Schedules {
    /// The clock the schedules follow, and what is scheduled now. Locked
    /// with the launcher's state held, never the other way round.
    state: Mutex<Scheduling>,
    /// Wakes the scheduler thread, and tells when it settled.
    wake: Arc<Wake>,
}

/// What the scheduler keeps: the clock it follows and one entry per
/// scheduled command.
struct Scheduling {
    clock: Arc<dyn Clock>,
    /// The commands whose package's code may run, by command id.
    entries: HashMap<String, Entry>,
}

/// What one command's schedule runs: the command's component, in its
/// package's managed copy; the item whose action runs, or `None` for a
/// no-view command, which runs itself; the command's manifest id; and the
/// interval, in milliseconds of the clock.
#[derive(Clone, PartialEq, Eq)]
struct Scheduled {
    component: PathBuf,
    item: Option<String>,
    command: String,
    every_ms: u64,
}

/// One scheduled command, as the scheduler runs it: what its schedule runs
/// (which a replaced code's schedule changes) and where its interval
/// stands.
struct Entry {
    /// What this run of the schedule runs.
    runs: Scheduled,
    /// When this run of the schedule began, in clock milliseconds: a tick
    /// is due every `runs.every_ms` from then.
    started: u64,
    /// The next tick, in clock milliseconds.
    next: u64,
    /// Whether a run has been asked for and has not answered: no second
    /// one is asked for meanwhile.
    in_flight: bool,
    /// Its place on its package's generation's undo list: the
    /// generation's end marks it, and the next look begins it again for
    /// the generation then current, or drops it.
    generation: EndMark,
}

impl Entry {
    /// A schedule running `runs` that begins `now`, for the generation of
    /// `data`: its first run is due one interval later.
    fn begins(runs: Scheduled, now: u64, data: Option<&PackageData>, wake: &Arc<Wake>) -> Entry {
        let every_ms = runs.every_ms;
        Entry {
            runs,
            started: now,
            next: now.saturating_add(every_ms),
            in_flight: false,
            generation: marked("schedule", data, wake),
        }
    }

    /// Restarts its interval at `now`, as if it began then.
    fn restart(&mut self, now: u64) {
        self.started = now;
        self.next = now.saturating_add(self.runs.every_ms);
    }
}

/// A mark of `what` on the undo list of `data`'s generation, whose end
/// wakes the worker `wake` is of, which then looks again (see
/// [`EndMark`]).
pub(super) fn marked(what: &'static str, data: Option<&PackageData>, wake: &Arc<Wake>) -> EndMark {
    let wake = Arc::downgrade(wake);
    EndMark::on(data.map(PackageData::generation), what, move || {
        if let Some(wake) = wake.upgrade() {
            wake.poke();
        }
    })
}

/// The next tick of a schedule that began at `started` and runs every
/// `every_ms`, after `now`: ticks that passed while no run could be
/// started are coalesced into it.
fn next_tick(started: u64, every_ms: u64, now: u64) -> u64 {
    match now.checked_sub(started) {
        Some(elapsed) => {
            let ticks = elapsed / every_ms + 1;
            started.saturating_add(ticks.saturating_mul(every_ms))
        }
        // The clock stands before the schedule began: its first run is one
        // interval after it began.
        None => started.saturating_add(every_ms),
    }
}

/// One scheduled run the scheduler starts.
struct Run {
    /// The command's component, in the package's managed copy.
    component: PathBuf,
    /// The item whose action runs; `None` for a no-view command, which
    /// runs itself.
    item: Option<String>,
    /// The command's manifest id.
    command: String,
    /// The extension data of the package's current generation: the run
    /// belongs to it.
    data: Option<PackageData>,
}

impl Schedules {
    /// The scheduler for `data`'s packages, following `clock`: told
    /// whenever a package's generation changes, so it looks again.
    /// [`Schedules::run`] starts its thread.
    pub(super) fn start(
        clock: Arc<dyn Clock>,
        data: &crate::extension_data::ExtensionData,
    ) -> Arc<Schedules> {
        let schedules = Arc::new(Schedules {
            state: Mutex::new(Scheduling {
                clock: clock.clone(),
                entries: HashMap::new(),
            }),
            wake: Arc::default(),
        });
        // The clock tells when it is set other than by time passing, and
        // the data when a generation begins or ends (installing, enabling,
        // disabling, pausing, resuming, replacing, uninstalling): look
        // again at both.
        clock.on_change(Box::new(waking(Arc::downgrade(&schedules))));
        data.set_changed(Arc::new(waking(Arc::downgrade(&schedules))));
        schedules
    }

    /// Starts the scheduler's thread, which runs until this launcher stops.
    pub(super) fn run(self: &Arc<Self>, launcher: WeakLauncher) {
        let schedules = Arc::downgrade(self);
        let started = std::thread::Builder::new()
            .name("pane-schedules".into())
            .spawn(move || schedule_until_stopped(launcher, schedules));
        if let Err(error) = started {
            eprintln!("Pane cannot run scheduled extension work in the background: {error}");
        }
    }

    /// The clock the schedules follow from now on
    /// ([`Launcher::with_clock`]), restarting every interval from its now:
    /// a clock given to a running Pane stands where it stands, and the
    /// schedules keep their phase only under one clock.
    pub(super) fn follow(self: &Arc<Self>, clock: Arc<dyn Clock>) {
        {
            let mut scheduling = self.lock();
            let now = clock.now();
            scheduling.clock = clock.clone();
            for entry in scheduling.entries.values_mut() {
                // The phase restarts under the new clock, as if every
                // schedule began now: a phase is meaningful only under the
                // clock it began under. A run still in flight stays marked.
                entry.restart(now);
            }
        }
        clock.on_change(Box::new(waking(Arc::downgrade(self))));
        self.wake.poke();
    }

    /// Waits until the scheduler looked at every change of the clock and of
    /// the packages so far, and every run it started has reported; `false`
    /// if it did not within `limit`. For tests and development builds,
    /// which so wait for scheduled work without timing it.
    pub(super) fn settled(&self, limit: Duration) -> bool {
        self.wake.settled(limit)
    }

    fn lock(&self) -> MutexGuard<'_, Scheduling> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Every scheduled run that is due now, marking each as in flight: no
    /// second run of one command is asked for before it answers. The
    /// launcher's state is locked while they are chosen, as when the user
    /// acts, so each call belongs to the generation current now, not to the
    /// one current when the run's thread first runs.
    ///
    /// A state left poisoned by a panicking thread is recovered by whoever
    /// needs it next ([`Launcher::lock`]); this only reads, so it waits for
    /// that instead, and the next look sees the state whole. Scheduling
    /// nothing for one look is safe: a poke follows the recovery.
    fn due(&self, launcher: &Launcher) -> Vec<(String, Run)> {
        // A poisoned state is not recovered here (a plain lock, not the
        // launcher's recovering one): a thread that panicked while holding
        // it is recovered by the next caller that changes something.
        let state = match launcher.state.lock() {
            Ok(state) => state,
            Err(_) => return Vec::new(),
        };
        let mut scheduling = self.lock();
        let now = scheduling.clock.now();
        // What is scheduled now: every command that declares scheduled work
        // of a package whose code may run. A command that left ends its
        // entry, so its interval restarts if it returns; one that changed
        // (its package's code was replaced) begins again.
        let mut wanted: HashMap<String, Scheduled> = HashMap::new();
        for package in &state.packages {
            if !state.runs(package) {
                continue;
            }
            for (command, schedule) in package.scheduled_commands() {
                // Its required preferences are unset: it does not run, and
                // says "Needs setup" instead (see `setup`); not a failure.
                if launcher.needs_setup(package, command.manifest_id()) {
                    continue;
                }
                let manifest_id = command.manifest_id().to_owned();
                wanted.insert(
                    command.id,
                    Scheduled {
                        component: command.component,
                        item: schedule.item,
                        command: manifest_id,
                        every_ms: schedule.every_seconds * 1000,
                    },
                );
            }
        }
        scheduling.entries.retain(|key, _| wanted.contains_key(key));
        let mut due = Vec::new();
        for (key, runs) in wanted {
            // The package's current generation, which a schedule that
            // begins now belongs to.
            let current = launcher.data_in(&state, &runs.component);
            let entry = scheduling
                .entries
                .entry(key.clone())
                .or_insert_with(|| Entry::begins(runs.clone(), now, current.as_ref(), &self.wake));
            if entry.runs != runs {
                // The schedule is another one now (its package's code was
                // replaced): the interval restarts with it, while a run
                // still in flight stays marked, so a second is not asked
                // for before it answers.
                let in_flight = entry.in_flight;
                *entry = Entry::begins(runs.clone(), now, current.as_ref(), &self.wake);
                entry.in_flight = in_flight;
            } else if entry.generation.ended() {
                // Its generation ended and the code runs in the next one
                // (a pause it came back from): the same schedule, on the
                // current generation's undo list.
                entry.generation = marked("schedule", current.as_ref(), &self.wake);
            }
            if entry.in_flight || now < entry.next {
                continue;
            }
            entry.next = next_tick(entry.started, entry.runs.every_ms, now);
            entry.in_flight = true;
            let data = launcher.data_in(&state, &entry.runs.component);
            due.push((
                key,
                Run {
                    component: entry.runs.component.clone(),
                    item: entry.runs.item.clone(),
                    command: entry.runs.command.clone(),
                    data,
                },
            ));
        }
        due
    }

    /// How long the scheduler may wait before it looks again: until the
    /// next tick of any schedule, at most [`MAX_WAIT`]. A run answering, a
    /// clock change or a generation change wakes it sooner.
    fn next_wait(&self) -> Duration {
        let scheduling = self.lock();
        let now = scheduling.clock.now();
        let soonest = scheduling
            .entries
            .values()
            .map(|entry| entry.next.saturating_sub(now))
            .min()
            .unwrap_or(u64::MAX);
        let wait = Duration::from_millis(soonest);
        wait.min(MAX_WAIT)
    }

    /// Notes that the run of the command with `key` is over — its answer
    /// was reported, or the thread running it could not be started — so its
    /// schedule may run again, and a tick that came due meanwhile is looked
    /// at now.
    fn run_ended(&self, key: &str) {
        if let Some(entry) = self.lock().entries.get_mut(key) {
            entry.in_flight = false;
        }
        self.wake.finished();
    }
}

impl Drop for Schedules {
    fn drop(&mut self) {
        self.wake.stop();
    }
}

/// A closure that pokes the scheduler `weak` names awake, holding it
/// weakly so that a clock or the extension data does not keep it running
/// past the launcher: for [`Clock::on_change`] and the data's change
/// hooks, which ask for another look.
fn waking(weak: Weak<Schedules>) -> impl Fn() + Send + Sync + 'static {
    move || {
        if let Some(schedules) = weak.upgrade() {
            schedules.wake.poke();
        }
    }
}

/// The scheduler thread: starts the runs that are due, then waits until one
/// is, something changes, or this Pane stops.
fn schedule_until_stopped(launcher: WeakLauncher, schedules: Weak<Schedules>) {
    while let Some(current) = schedules.upgrade() {
        let Some(seen) = current.wake.pokes() else {
            return;
        };
        // The launcher being gone ends the thread through the wake: this
        // Pane runs no scheduled work from here on.
        let due = launcher.upgrade().map(|launcher| current.due(&launcher));
        let (due, runs) = match due {
            Some(due) => {
                let runs = due.len();
                (due, runs)
            }
            None => (Vec::new(), 0),
        };
        current.wake.scanned(seen, runs);
        for (key, run) in due {
            start_run(&current, &launcher, key, run);
        }
        let wait = current.next_wait();
        if !current.wake.wait(seen, wait) {
            return;
        }
    }
}

/// Spawns the thread that runs `run` and reports its answer. The scheduler
/// never waits for it: a slow run delays neither the window nor the next
/// look for due work (the runtime serves one guest call at a time, and
/// stops it when its generation ends).
fn start_run(schedules: &Arc<Schedules>, launcher: &WeakLauncher, key: String, run: Run) {
    let weak = Arc::downgrade(schedules);
    let launcher = launcher.clone();
    // For the thread not starting: the command's schedule may try again.
    let retry = key.clone();
    let started = std::thread::Builder::new()
        .name("pane-scheduled-run".into())
        .spawn(move || run_once(weak, launcher, key, run));
    if let Err(error) = started {
        eprintln!("Pane could not run scheduled extension work: {error}");
        schedules.run_ended(&retry);
    }
}

/// Runs one scheduled command's action, or the no-view command itself,
/// and reports the action's answer. The run belongs to the generation the
/// scheduler took when it asked for it, so disabling, reloading, updating,
/// uninstalling or pausing the package meanwhile stops it, and its answer
/// is not shown. A no-view command's run is a background launch, which
/// shows nothing.
fn run_once(schedules: Weak<Schedules>, launcher: WeakLauncher, key: String, run: Run) {
    let alive = launcher.upgrade();
    let scheduled = LaunchRecord {
        command: Some(run.command.clone()),
        ..LaunchRecord::scheduled()
    };
    let answer = match (&alive, &run.item) {
        (Some(launcher), Some(item)) => match launcher.runtime() {
            Ok(runtime) => Some(futures::executor::block_on(runtime.run_item_launched_with(
                &run.component,
                Some(run.command.as_str()),
                item,
                &scheduled,
                run.data.clone(),
            ))),
            Err(error) => Some(Err(error.clone())),
        },
        (Some(launcher), None) => {
            if let Ok(runtime) = launcher.runtime() {
                // Its answer, and an error it answers with, are not shown:
                // no window was shown for it. A crash still counts towards
                // pausing it.
                let _ = futures::executor::block_on(runtime.run_command_with(
                    &run.component,
                    &run.command,
                    &scheduled,
                    run.data.clone(),
                ));
                // Nothing will finish a toast the run left in progress.
                launcher.clear_animated_toast(&mut launcher.lock(), &run.component);
                launcher.changed();
            }
            None
        }
        // This Pane stopped: no answer comes, and nothing remains to report
        // one to.
        (None, _) => None,
    };
    // The run's answer is shown, then the schedule may run again: a tick
    // that came due meanwhile is looked at now.
    if let (Some(launcher), Some(answer)) = (alive, answer)
        && let Some(epoch) = show(&launcher, &run.component, &run.data, answer)
    {
        futures::executor::block_on(launcher.list_again(
            epoch,
            run.component.clone(),
            run.data.clone(),
        ));
        launcher.changed();
    }
    if let Some(schedules) = schedules.upgrade() {
        schedules.run_ended(&key);
    }
}

/// Shows what `answer`, of the run of the command in `component`, calls
/// for where its screen is the one on display, as an action's answer is
/// shown: nothing for an answer (the command shows a toast or a HUD if it
/// has something to say), a failure toast for an error it answered with,
/// and Pane's own error otherwise; a generation that ended while it ran
/// means it is not shown, and the command's screen having been left means
/// it is not either. The screen's epoch, when the command handled the run
/// and its list is to be asked for again.
fn show(
    launcher: &Launcher,
    component: &Path,
    data: &Option<PackageData>,
    answer: Result<Answer, CallError>,
) -> Option<u64> {
    let handled = matches!(
        answer,
        Ok(_) | Err(CallError::Guest(_) | CallError::Unreadable(_))
    );
    let list_again = {
        let mut state = launcher.lock();
        let shown = state.open.as_ref().is_some_and(|open| open == component)
            && matches!(
                state.view.screen,
                Screen::Command | Screen::CommandSearch { .. }
            );
        if !shown {
            return None;
        }
        let state = &mut *state;
        let ended = stopped(state, component, data);
        let list_again = (handled && ended.is_none()).then_some(state.screen_epoch);
        let command = state.open_command.clone();
        match (ended, answer) {
            (Some(problem), _) => state.view.status = Status::Error(problem),
            (None, Ok(_)) => {}
            (None, Err(CallError::Guest(message))) => {
                launcher.show_failure(state, component, command.as_deref(), message);
            }
            (None, Err(error)) => state.view.status = Status::Error(error.to_string()),
        }
        list_again
    };
    launcher.changed();
    list_again
}

/// Wakes a background worker of the launcher's (the scheduler, the
/// services thread) when work may be due, and tells when it settled:
/// every change so far has been looked at, and every run it started has
/// reported.
#[derive(Default)]
pub(super) struct Wake {
    state: Mutex<WakeState>,
    condvar: std::sync::Condvar,
}

#[derive(Default)]
struct WakeState {
    stopped: bool,
    /// Counts pokes: each asks the worker to look again.
    pokes: u64,
    /// The pokes the worker's last look covered.
    looked: u64,
    /// Runs the worker started that have not reported yet.
    running: usize,
}

impl Wake {
    fn lock(&self) -> MutexGuard<'_, WakeState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Asks the worker to look again.
    pub(super) fn poke(&self) {
        self.lock().pokes += 1;
        self.condvar.notify_all();
    }

    /// Stops the worker.
    pub(super) fn stop(&self) {
        self.lock().stopped = true;
        self.condvar.notify_all();
    }

    /// The pokes to look at, or `None` once stopped.
    pub(super) fn pokes(&self) -> Option<u64> {
        let state = self.lock();
        (!state.stopped).then_some(state.pokes)
    }

    /// Notes a look that covered the first `seen` pokes and started `runs`
    /// runs.
    pub(super) fn scanned(&self, seen: u64, runs: usize) {
        let mut state = self.lock();
        state.looked = state.looked.max(seen);
        state.running += runs;
        self.condvar.notify_all();
    }

    /// Notes that one run reported (or could not be started), and asks for
    /// another look: work may have come due meanwhile.
    pub(super) fn finished(&self) {
        let mut state = self.lock();
        state.running = state.running.saturating_sub(1);
        state.pokes += 1;
        self.condvar.notify_all();
    }

    /// Waits at most `limit` for a poke after the `seen`th; returns whether
    /// the worker still runs.
    pub(super) fn wait(&self, seen: u64, limit: Duration) -> bool {
        let state = self.lock();
        let (state, _) = self
            .condvar
            .wait_timeout_while(state, limit, |state| !state.stopped && state.pokes == seen)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        !state.stopped
    }

    /// Waits until the worker settled: every poke so far was looked at and
    /// every run started has reported. `false` if it did not within
    /// `limit`, or it stopped.
    pub(super) fn settled(&self, limit: Duration) -> bool {
        let state = self.lock();
        let (state, _) = self
            .condvar
            .wait_timeout_while(state, limit, |state| {
                !state.stopped && (state.looked < state.pokes || state.running > 0)
            })
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        !state.stopped && state.looked >= state.pokes && state.running == 0
    }
}
