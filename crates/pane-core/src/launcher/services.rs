//! Continuing services: Pane runs a command's service in a cycle while the
//! package's code may run, at no interval the manifest declares.
//!
//! A command's `pane.json` entry declares a service (`"service": true`):
//! while the package's code may run — it is enabled and not paused (see
//! `pausing`) — Pane calls the component's `run-cycle` export,
//! which runs one slice of the service's work and answers the status to
//! show and how long to wait before the next cycle. The cadence is the
//! service's own, chosen cycle by cycle in its code; that, not an interval,
//! is what tells a continuing service from scheduled work (see
//! `schedules`), which the manifest paces. The instance (and whatever the
//! service keeps in it, its task's state) lives for the whole generation:
//! each cycle starts it if it has none and finds its state where the last
//! left it.
//!
//! The services thread is a thread of Pane's own, which looks for work due
//! and starts each cycle on a thread of its own, so neither the window nor
//! the services thread ever waits for a guest — the same shape as the
//! scheduler's, and driven by the same seams: the launcher's clock
//! ([`crate::clipboard::Clock`]), which tells it when the clock is set
//! other than by time passing, and the extension data's change hooks,
//! which tell it whenever a package's generation changes (installing,
//! enabling, disabling, pausing, resuming, replacing, uninstalling).
//!
//! The service belongs to the package's generation, exactly as scheduled
//! work and any call do: it begins when the code may run — Pane starts, the
//! package is installed or enabled, or its code is replaced — and its first
//! cycle runs at once, since a continuing service's declaration is a
//! request to run, not a request to be woken later. It ends when the code
//! may not: the package is disabled, uninstalled, paused after it failed,
//! or its code is replaced by a reload or an update, which begins a new
//! one. A cycle still running is stopped with the generation (the runtime
//! stops the call, its instance and task go), and its late answer is
//! discarded, never shown and never paced from; work is not replayed for
//! the time the code could not run. At most one cycle of a service is
//! asked for at a time: time that passes while one runs is served by the
//! next cycle, at the cadence the answer gives from when it lands.
//!
//! Each cycle is an ordinary guest call, so its failures follow the
//! established policy for free: a trap, or a cycle that computes without
//! finishing, is a crash of the package (the third within five minutes
//! pauses it, which ends the service with the generation), and an error
//! the service answers with is an expected error, however often: Pane
//! shows it and runs the next cycle after
//! [`RETRY_AFTER_SECONDS`](RETRY_AFTER_SECONDS), the same wait as after a
//! crash, so one broken cycle never ends a service the user enabled. The
//! status of a cycle shows on the command's screen while that screen is on
//! display, as an action's answer is; the cycle runs whether or not it is.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use super::schedules::Wake;
use super::{Launcher, Screen, Status, WeakLauncher, stopped};
use crate::clipboard::Clock;
use crate::extension_data::PackageData;
use crate::runtime::{CallError, Cycle};

/// The longest the services thread waits before it looks again, so that a
/// change of the system's time, or a computer waking from sleep, delays a
/// cycle by at most this much (what is due is always run when it is looked
/// at). A service's own wait is never stretched past its cadence by more
/// than this.
const MAX_WAIT: Duration = Duration::from_secs(3600);

/// The shortest cadence a service may ask Pane for: 1 second, as the
/// shortest a schedule may declare. A service that answers 0, or less,
/// runs at this cadence; one that answers more runs at what it asked.
/// Provisional, like scheduled work's bounds, pending the user's decision
/// on background activation intervals.
const MIN_SERVICE_SECONDS: u64 = 1;

/// The longest cadence a service may ask Pane for: 30 days, as the longest
/// a schedule may declare, so a service always runs again. A service that
/// answers more runs at this. Provisional, like scheduled work's bounds.
const MAX_SERVICE_SECONDS: u64 = 30 * 86_400;

/// How long Pane waits before the next cycle after one failed: an error
/// the service answered with, a trap, or a cycle Pane stopped as
/// unresponsive. The service keeps running, as the user asked by enabling
/// it; only pausing the package after repeated crashes ends it. A
/// provisional default for a cadence the failing cycle did not give.
const RETRY_AFTER_SECONDS: u64 = 1;

/// Runs the continuing services of the launcher's installed commands. Kept
/// by the launcher; dropping it stops the thread.
pub(super) struct Services {
    /// The clock the services follow, and what runs now. Locked with the
    /// launcher's state held, never the other way round.
    state: Mutex<Serving>,
    /// Wakes the services thread, and tells when it settled.
    wake: Wake,
}

/// What the services thread keeps: the clock it follows and one entry per
/// running service.
struct Serving {
    clock: Arc<dyn Clock>,
    /// The services running now, by command id.
    entries: HashMap<String, Entry>,
    /// Counts the cycles started, so that a report of one whose entry went
    /// with its generation (a replaced service's old code) cannot clear a
    /// newer cycle's mark or pace it.
    dispatched: u64,
}

/// One running service: the command whose service runs, and where its
/// cycle stands.
struct Entry {
    /// The command's component, in its package's managed copy.
    component: PathBuf,
    /// The command's id in its package manifest, sent with each cycle.
    command: String,
    /// The cadence the service last asked for, in milliseconds: what the
    /// next cycle waits after this one, and what its phase restarts from
    /// under a clock the launcher is given.
    every_ms: u64,
    /// When the next cycle is due, in clock milliseconds.
    next: u64,
    /// The dispatch token of the cycle asked for and not answered, if any:
    /// no second cycle of this service is asked for before it answers. A
    /// cycle whose generation ended answers soon and is ignored then, so
    /// a service that begins again (its code was replaced) is not held up
    /// by the dead code's cycle.
    in_flight: Option<u64>,
}

impl Entry {
    /// A service of the command in `component` that begins `now`: its
    /// first cycle is due at once, at the cadence every service begins
    /// with until its own first answer says another.
    fn begins(component: PathBuf, command: String, now: u64) -> Entry {
        Entry {
            component,
            command,
            every_ms: MIN_SERVICE_SECONDS * 1000,
            next: now,
            in_flight: None,
        }
    }
}

/// One service cycle the services thread starts.
struct Run {
    /// The command's component, in the package's managed copy.
    component: PathBuf,
    /// The command's id in its package manifest.
    command: String,
    /// The extension data of the package's current generation: the cycle
    /// belongs to it.
    data: Option<PackageData>,
    /// The cycle's dispatch token, so its report cannot be mistaken for
    /// another cycle's of the same service.
    token: u64,
}

impl Services {
    /// The services thread for `data`'s packages, following `clock`: told
    /// whenever a package's generation changes, so it looks again.
    /// [`Services::run`] starts its thread.
    pub(super) fn start(
        clock: Arc<dyn Clock>,
        data: &crate::extension_data::ExtensionData,
    ) -> Arc<Services> {
        let services = Arc::new(Services {
            state: Mutex::new(Serving {
                clock: clock.clone(),
                entries: HashMap::new(),
                dispatched: 0,
            }),
            wake: Wake::default(),
        });
        // The clock tells when it is set other than by time passing, and
        // the data when a generation begins or ends (installing, enabling,
        // disabling, pausing, resuming, replacing, uninstalling): look
        // again at both.
        clock.on_change(Box::new(waking(Arc::downgrade(&services))));
        data.set_changed(Arc::new(waking(Arc::downgrade(&services))));
        services
    }

    /// Starts the services thread, which runs until this launcher stops.
    pub(super) fn run(self: &Arc<Self>, launcher: WeakLauncher) {
        let services = Arc::downgrade(self);
        let started = std::thread::Builder::new()
            .name("pane-services".into())
            .spawn(move || serve_until_stopped(launcher, services));
        if let Err(error) = started {
            eprintln!("Pane cannot run continuing extension services in the background: {error}");
        }
    }

    /// The clock the services follow from now on
    /// ([`Launcher::with_clock`]), restarting every service's phase from
    /// its now, at the cadence each last asked for: a clock given to a
    /// running Pane stands where it stands, and a service's phase is
    /// meaningful only under the clock it began under. A cycle still in
    /// flight stays marked.
    pub(super) fn follow(self: &Arc<Self>, clock: Arc<dyn Clock>) {
        {
            let mut serving = self.lock();
            let now = clock.now();
            serving.clock = clock.clone();
            for entry in serving.entries.values_mut() {
                // The phase restarts under the new clock, as if the last
                // cycle had just answered now: no cycle is replayed for
                // the time the clock jumped over. A cycle still in flight
                // stays marked.
                let every_ms = entry.every_ms;
                entry.next = now.saturating_add(every_ms);
            }
        }
        clock.on_change(Box::new(waking(Arc::downgrade(self))));
        self.wake.poke();
    }

    /// Waits until the services thread looked at every change of the clock
    /// and of the packages so far, and every cycle it started has
    /// reported; `false` if it did not within `limit`. For tests and
    /// development builds, which so wait for services without timing them.
    pub(super) fn settled(&self, limit: Duration) -> bool {
        self.wake.settled(limit)
    }

    fn lock(&self) -> MutexGuard<'_, Serving> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Every service cycle that is due now, marking each as in flight: no
    /// second cycle of one service is asked for before it answers. The
    /// launcher's state is locked while they are chosen, as when the user
    /// acts, so each call belongs to the generation current now, not to the
    /// one current when the cycle's thread first runs.
    ///
    /// A state left poisoned by a panicking thread is recovered by whoever
    /// needs it next ([`Launcher::lock`]); this only reads, so it waits for
    /// that instead, and the next look sees the state whole. Serving
    /// nothing for one look is safe: a poke follows the recovery.
    fn due(&self, launcher: &Launcher) -> Vec<(String, Run)> {
        // A poisoned state is not recovered here (a plain lock, not the
        // launcher's recovering one): a thread that panicked while holding
        // it is recovered by the next caller that changes something.
        let state = match launcher.state.lock() {
            Ok(state) => state,
            Err(_) => return Vec::new(),
        };
        let mut serving = self.lock();
        let now = serving.clock.now();
        // What runs now: every command that declares a service of a
        // package whose code may run. A command that left ends its entry,
        // so its service begins again if it returns; one that changed (its
        // package's code was replaced) begins again.
        let mut wanted: HashMap<String, (PathBuf, String)> = HashMap::new();
        for package in &state.packages {
            if !state.runs(package) {
                continue;
            }
            for command in package.service_commands() {
                // Its required preferences are unset: its service does not
                // run, and it says "Needs setup" instead (see `setup`); not
                // a failure.
                if launcher.needs_setup(package, command.manifest_id()) {
                    continue;
                }
                let manifest_id = command.manifest_id().to_owned();
                let id = command.id;
                let component = command.component;
                wanted.insert(id, (component, manifest_id));
            }
        }
        serving.entries.retain(|key, _| wanted.contains_key(key));
        let mut due = Vec::new();
        for (key, (component, command)) in wanted {
            // The dispatch token of the cycle this look may start, counted
            // before the entry is borrowed below.
            let token = serving.dispatched + 1;
            serving.dispatched = token;
            let entry = serving
                .entries
                .entry(key.clone())
                .or_insert_with(|| Entry::begins(component.clone(), command.clone(), now));
            if entry.component != component {
                // The service is another one now (its package's code was
                // replaced): it begins again, at once. A cycle of the old
                // code still running answers soon (its generation ended)
                // and its report is ignored, so it holds nothing up: the
                // new code need not wait for it.
                *entry = Entry::begins(component.clone(), command.clone(), now);
            }
            // While a cycle runs, its entry's `next` may lie in the past
            // (it is set when the answer lands, from when the next cycle
            // waits): probe one cadence later rather than spinning on a
            // past time — the answer pokes when it arrives, and so does
            // every change that ends the cycle's generation.
            if entry.in_flight.is_some() {
                if now >= entry.next {
                    entry.next = now.saturating_add(entry.every_ms);
                }
                continue;
            }
            if now < entry.next {
                continue;
            }
            if !entry.component.is_file() {
                // The component is not there — its package's code is being
                // replaced: the old managed copy goes before the new one is
                // recorded in the launcher's state, and recording it pokes.
                // Run when the replacement is in place, at that poke (a
                // replaced entry begins again at once); a component that is
                // missing for good is probed at the cadence, and the
                // command's own load failure is what reports it.
                entry.next = now.saturating_add(entry.every_ms);
                continue;
            }
            // The next cycle waits a cadence from now even before the
            // answer says so, so the thread never spins on a past `next`
            // while one runs; the answer moves it to when the service
            // asked to run again.
            entry.next = now.saturating_add(entry.every_ms);
            entry.in_flight = Some(token);
            let data = launcher.data_in(&state, &entry.component);
            due.push((
                key,
                Run {
                    component: entry.component.clone(),
                    command: entry.command.clone(),
                    data,
                    token,
                },
            ));
        }
        due
    }

    /// How long the services thread may wait before it looks again: until
    /// the next cycle of any service, at most [`MAX_WAIT`]. A cycle
    /// answering, a clock change or a generation change wakes it sooner.
    fn next_wait(&self) -> Duration {
        let serving = self.lock();
        let now = serving.clock.now();
        let soonest = serving
            .entries
            .values()
            .map(|entry| entry.next.saturating_sub(now))
            .min()
            .unwrap_or(u64::MAX);
        let wait = Duration::from_millis(soonest);
        wait.min(MAX_WAIT)
    }

    /// Notes how long the service of the command with `key` waits before
    /// its next cycle, from now: the cadence its cycle dispatched as
    /// `token` asked for, or the wait after one that failed. Nothing, and
    /// no second cycle, if the entry has gone (the service ended with its
    /// generation), or a newer cycle holds the mark: the answer of a
    /// stopped generation paces nothing.
    fn cycle_ended(&self, key: &str, token: u64, every_ms: u64) {
        let mut serving = self.lock();
        let now = serving.clock.now();
        if let Some(entry) = serving.entries.get_mut(key)
            && entry.in_flight == Some(token)
        {
            entry.every_ms = every_ms;
            entry.next = now.saturating_add(every_ms);
        }
    }

    /// Notes that the cycle of the command with `key`, dispatched as
    /// `token`, reported — its answer was shown, or the thread running it
    /// could not be started — so its service may cycle again, and work
    /// that came due meanwhile is looked at now. A report of a cycle whose
    /// entry went with its generation (its service began again after its
    /// code was replaced) clears nothing: a newer cycle holds the mark.
    fn run_ended(&self, key: &str, token: u64) {
        // The entry may be gone (the service ended) or a new one (the
        // package was disabled and enabled again): a stale report clears
        // nothing, and a poke follows either way.
        if let Some(entry) = self.lock().entries.get_mut(key)
            && entry.in_flight == Some(token)
        {
            entry.in_flight = None;
        }
        self.wake.finished();
    }
}

impl Drop for Services {
    fn drop(&mut self) {
        self.wake.stop();
    }
}

/// A closure that pokes the services thread `weak` names awake, holding it
/// weakly so that a clock or the extension data does not keep it running
/// past the launcher: for [`Clock::on_change`] and the data's change
/// hooks, which ask for another look.
fn waking(weak: Weak<Services>) -> impl Fn() + Send + Sync + 'static {
    move || {
        if let Some(services) = weak.upgrade() {
            services.wake.poke();
        }
    }
}

/// The services thread: starts the cycles that are due, then waits until
/// one is, something changes, or this Pane stops.
fn serve_until_stopped(launcher: WeakLauncher, services: Weak<Services>) {
    while let Some(current) = services.upgrade() {
        let Some(seen) = current.wake.pokes() else {
            return;
        };
        // The launcher being gone ends the thread through the wake: this
        // Pane runs no continuing services from here on.
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

/// Spawns the thread that runs `run` and reports its answer. The services
/// thread never waits for it: a slow cycle delays neither the window nor
/// the next look for due work (the runtime serves one guest call at a
/// time, and stops the cycle when its generation ends).
fn start_run(services: &Arc<Services>, launcher: &WeakLauncher, key: String, run: Run) {
    let weak = Arc::downgrade(services);
    let launcher = launcher.clone();
    // For the thread not starting: the service may try again.
    let (retry, token) = (key.clone(), run.token);
    let started = std::thread::Builder::new()
        .name("pane-service-cycle".into())
        .spawn(move || run_once(weak, launcher, key, run));
    if let Err(error) = started {
        eprintln!("Pane could not run a continuing extension service: {error}");
        services.run_ended(&retry, token);
    }
}

/// Runs one cycle of a service and reports its answer. The cycle belongs
/// to the generation the services thread took when it asked for it, so
/// disabling, reloading, updating, uninstalling or pausing the package
/// meanwhile stops it (with the instance and the task's state in it), and
/// its answer is not shown and paces nothing.
fn run_once(services: Weak<Services>, launcher: WeakLauncher, key: String, run: Run) {
    let alive = launcher.upgrade();
    let answer = match &alive {
        Some(launcher) => match launcher.runtime() {
            Ok(runtime) => Some(futures::executor::block_on(runtime.run_cycle_with(
                &run.component,
                &run.command,
                run.data.clone(),
            ))),
            Err(error) => Some(Err(error.clone())),
        },
        // This Pane stopped: no answer comes, and nothing remains to report
        // one to.
        None => None,
    };
    // The answer paces the next cycle — the cadence the service asked for,
    // or the wait after a failure — but only while the code it ran may
    // still run: a stopped generation's answer is discarded.
    let still_running = run
        .data
        .as_ref()
        .is_none_or(|data| data.stopped().is_none());
    if let (Some(launcher), Some(answer)) = (&alive, &answer) {
        if still_running {
            // Every failure paces the next cycle at the same wait: an error
            // the service answered with carries no cadence, and a crash
            // must be able to repeat, so that the policy can pause the
            // package after three.
            let every_ms = match answer {
                Ok(cycle) => {
                    cycle
                        .next_seconds
                        .clamp(MIN_SERVICE_SECONDS, MAX_SERVICE_SECONDS)
                        * 1000
                }
                Err(_) => RETRY_AFTER_SECONDS * 1000,
            };
            if let Some(services) = services.upgrade() {
                services.cycle_ended(&key, run.token, every_ms);
            }
        }
        // The cycle's answer is shown, then the service may cycle again: a
        // cadence that came due meanwhile is looked at now.
        show(launcher, &run.component, &run.data, answer);
    }
    if let Some(services) = services.upgrade() {
        services.run_ended(&key, run.token);
    }
}

/// Shows `answer`, of the cycle of the service in `component`, where its
/// command's screen is the one on display, as an action's answer is shown;
/// a generation that ended while it ran means it is not shown, and the
/// command's screen having been left means it is not either. The status of
/// a failing cycle (an error it answered with, a trap) is the error, the
/// same as any call's.
fn show(
    launcher: &Launcher,
    component: &Path,
    data: &Option<PackageData>,
    answer: &Result<Cycle, CallError>,
) {
    let shown = {
        let mut state = launcher.lock();
        let shown = state.open.as_ref().is_some_and(|open| open == component)
            && matches!(
                state.view.screen,
                Screen::Command | Screen::CommandSearch { .. }
            );
        if shown {
            state.view.status = match (stopped(&state, component, data), answer) {
                (Some(problem), _) => Status::Error(problem),
                (None, Ok(cycle)) => Status::Result(cycle.status.clone()),
                (None, Err(error)) => Status::Error(error.to_string()),
            };
        }
        shown
    };
    if shown {
        launcher.changed();
    }
}
