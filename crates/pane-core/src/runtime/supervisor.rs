//! Keeping Pane usable when its extension runtime thread crashes (#17).
//!
//! The runtime thread serves every guest call. A panic on it (a fault in
//! Pane's host code or in Wasmtime, not a guest trap, which is a crash of
//! one package) unwinds it: its instances, views and pending calls are
//! dropped, so every call that was running or queued answers
//! [`CallError::RuntimeUnavailable`] and none is sent again. The thread
//! catches its own unwinding, ends the native helpers still running (they
//! were all started by its guests), and then:
//!
//! - starts a fresh runtime thread, so the next call the user asks for runs
//!   there; or,
//! - when it crashed within [`CRASH_WINDOW`] of the previous crash, starts
//!   none: repeated automatic restarts are suppressed, and the runtime stays
//!   stopped until the user restarts it ([`Runtime::restart`]).
//!
//! Either way the launcher is told ([`CrashReport`]), without naming any
//! package: which one, if any, caused a crash of the shared thread is not
//! known, so nothing is paused. Extension data is untouched.
//!
//! This needs the panic to unwind (checked at compile time below). A panic
//! in a destructor while the thread unwinds from a first panic aborts the
//! whole process, as does any other abort: neither is recovered.
//!
//! A thread that **stops responding** (#18, a runtime hang) is handled the
//! same way. A watchdog thread follows its heartbeat (see `deadlines`): a
//! thread inside one poll of its work, outside any host call, whose
//! heartbeat stays still for [`deadlines::WARN_AFTER`] is said to be not
//! responding yet ([`SlowReport`]), and after
//! [`deadlines::UNRESPONSIVE_LIMIT`] Pane gives up on it. A guest computing
//! beats at every tick (and is stopped by its own limit), a host call is
//! never given up on, and a thread waiting for work or for a guest's host
//! work is not stuck, so neither a busy extension nor a slow host call is
//! mistaken for a hang. Every call it held answers that the runtime
//! stopped, its helpers are ended, it is restarted or not as after a crash
//! (the two share the restart window), and the launcher is told without
//! any package named. The stuck thread cannot be ended: it is abandoned,
//! its fence closes so nothing it still runs changes anything, and it
//! frees what it holds only once it returns.
//!
//! A fresh thread must not wait on something an abandoned one holds, or it
//! would fail in turn, using up the restart window for the first failure.
//! So what runtime threads share is held only briefly and never across
//! blocking work: the extension data lock while a value is read or staged
//! (files are written by their own thread), the helpers' lock while a run
//! is registered (processes start off the runtime thread), and the
//! runtime's own state here. A thread stuck elsewhere holds none of them.
//! What remains: the launcher's health report, which the runtime thread
//! calls with a failure it reports, takes the launcher's lock briefly; a
//! thread stuck inside it would have stopped the launcher too.

use std::any::Any;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};

use super::deadlines::{self, Limits, Quiet, Verdict, Watch, Watched};
#[cfg(any(test, debug_assertions))]
use super::faults::Fault;
use super::faults::Faults;

use super::{
    CallError, Code, HealthReport, Host, Request, SharedApplications, SharedClipboard,
    SharedDirectory, lock, unavailable,
};
use crate::helpers::runner::Helpers;

// Recovering from a crash of the runtime thread relies on its panic
// unwinding to a `catch_unwind` on that thread: with `panic = "abort"`, any
// panic there ends Pane's whole process.
#[cfg(not(panic = "unwind"))]
compile_error!(
    "Pane must be built with `panic = \"unwind\"`: it recovers from a crash of its extension runtime thread by catching its panic"
);

/// How close together crashes count together: three crashes of a package
/// within it pause the package (see the launcher's `pausing`), and a second
/// crash of the runtime thread within it after the previous one stops the
/// runtime instead of restarting it. An explicit choice (provisional).
pub(crate) const CRASH_WINDOW: Duration = Duration::from_secs(5 * 60);

/// How the runtime thread failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeFailure {
    /// It panicked.
    Crashed,
    /// It stopped responding (a runtime hang): it made no progress for
    /// [`deadlines::UNRESPONSIVE_LIMIT`], so Pane gave up on it.
    Unresponsive,
}

impl RuntimeFailure {
    /// "after crashing", "after not responding": how it stopped, for a
    /// state such as "Restarted after …".
    pub fn how(self) -> &'static str {
        match self {
            RuntimeFailure::Crashed => "crashing",
            RuntimeFailure::Unresponsive => "not responding",
        }
    }

    /// "stopped unexpectedly", "stopped responding": what the runtime did.
    pub(crate) fn stopped(self) -> &'static str {
        match self {
            RuntimeFailure::Crashed => "stopped unexpectedly",
            RuntimeFailure::Unresponsive => "stopped responding",
        }
    }

    /// What happened, for the details of the failure, under `limits`. It
    /// says only what is known: never which code held a stuck thread.
    pub(crate) fn happened(self, limits: &Limits) -> String {
        match self {
            RuntimeFailure::Crashed => {
                "Pane's extension runtime, which runs every extension, stopped unexpectedly.".into()
            }
            RuntimeFailure::Unresponsive => format!(
                "Pane's extension runtime, which runs every extension, made no progress for {}, \
                 so Pane gave up on it. It was not running an extension's code (an \
                 extension computing for too long is stopped by itself, and named) nor inside \
                 one of Pane's host calls; which code held it is not known.",
                deadlines::seconds(limits.unresponsive)
            ),
        }
    }

    /// What the failure leaves behind, if anything.
    pub(crate) fn left_behind(self) -> Option<&'static str> {
        match self {
            RuntimeFailure::Crashed => None,
            RuntimeFailure::Unresponsive => Some(
                "A stuck thread cannot be ended: it keeps the memory it holds until it returns, \
                 and then runs nothing more; nothing it still does changes anything.",
            ),
        }
    }

    /// How a call whose answer it lost starts to say so.
    fn stopped_before_answering(self) -> &'static str {
        match self {
            RuntimeFailure::Crashed => "it stopped before answering",
            RuntimeFailure::Unresponsive => "it stopped responding before answering",
        }
    }
}

/// What the runtime is doing, as far as failures of its thread go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeStatus {
    /// It runs, and has not failed since it started or was last restarted
    /// by the user.
    Running,
    /// It failed, and Pane started it again by itself. `why` is what its
    /// thread reported as it stopped, or why Pane gave up on it.
    Restarted {
        failure: RuntimeFailure,
        why: String,
    },
    /// It failed and was not started again: it runs nothing until the user
    /// restarts it. `not_restarted` says why Pane did not.
    Stopped {
        failure: RuntimeFailure,
        why: String,
        not_restarted: String,
    },
}

impl RuntimeStatus {
    /// How it last failed, unless it runs as it started.
    pub fn failure(&self) -> Option<RuntimeFailure> {
        match self {
            RuntimeStatus::Running => None,
            RuntimeStatus::Restarted { failure, .. } | RuntimeStatus::Stopped { failure, .. } => {
                Some(*failure)
            }
        }
    }
}

/// Told on the crashed runtime thread, once its helpers were ended and it
/// was restarted or not, with the crashed thread's number (see
/// [`super::ViewId::thread`]) and what the runtime does now.
pub(crate) type CrashReport = Arc<dyn Fn(u64, &RuntimeStatus) + Send + Sync>;

/// Told on the watchdog's thread when the runtime thread `.0` has made no
/// progress for [`deadlines::WARN_AFTER`] (`true`: it is not responding
/// yet, and Pane gives up on it unless it carries on), and when it carries
/// on after that (`false`).
pub(crate) type SlowReport = Arc<dyn Fn(u64, bool) + Send + Sync>;

/// What every [`super::Runtime`] handle shares: the runtime thread now
/// serving calls, if one is, and what a new one needs.
pub(super) struct Shared {
    /// The native helper processes guests started, which end once every
    /// handle is dropped.
    pub(super) helpers: Helpers,
    pub(super) applications: SharedApplications,
    pub(super) files: crate::files::FileAccess,
    pub(super) clipboard: SharedClipboard,
    pub(super) directory: SharedDirectory,
    pub(super) health: Arc<Mutex<Option<HealthReport>>>,
    /// Custom view ids, never reused, even by a restarted thread: a view
    /// the window still shows from a crashed one must not name a new view.
    pub(super) next_view: Arc<AtomicU64>,
    /// Guests' web requests: their limits, and what each package did this
    /// session, which a restarted thread carries on.
    pub(super) network: Arc<crate::http::Network>,
    /// What a search waits on before it starts, if a test replaced the
    /// clock's [`super::SEARCH_DEBOUNCE`]; a restarted thread keeps it.
    /// A release build has none.
    #[cfg(any(test, debug_assertions))]
    pub(super) search_timer: Arc<Mutex<Option<super::SearchTimer>>>,
    crashes: Mutex<Option<CrashReport>>,
    slow: Mutex<Option<SlowReport>>,
    /// The limits guest calls and the watchdog apply.
    pub(super) limits: Arc<Mutex<Limits>>,
    /// The number of the last thread Pane is done with (it failed and was
    /// restarted or not, the launcher told), for a call whose answer a
    /// failure lost: threads serve one after another, so every thread up to
    /// it is done.
    handled: watch::Sender<u64>,
    /// The threads Pane gave up on, until each returns.
    abandoned: Mutex<Vec<Arc<Watch>>>,
    /// The components whose calls the user asked for and that have not
    /// answered yet, with how many: sent, queued or running (see
    /// [`Runtime::busy`]). The replacement of a package's code waits for
    /// them, so that an update never interrupts a command mid-run.
    pub(super) busy: Mutex<HashMap<PathBuf, Busy>>,
    /// The threads made to hang, for releasing them.
    #[cfg(any(test, debug_assertions))]
    hung: Mutex<Vec<Arc<Faults>>>,
    cache_dir: Option<PathBuf>,
    current: Mutex<Current>,
}

/// The calls of one generation of a component that have not answered yet,
/// and which generation they belong to: `Runtime::forget` ends a
/// generation, so a call of an ended generation whose counting ends late
/// (its future is dropped after a newer generation began counting) cannot
/// touch the newer generation's count (see [`super::Runtime::busy`]).
#[derive(Default)]
pub(super) struct Busy {
    /// How many calls of this generation have not answered yet.
    pub(super) calls: usize,
    /// Which generation of the component the count is of; `forget` moves it
    /// on, never reusing a number.
    pub(super) generation: u64,
}

/// Why a request was not sent to the runtime thread.
pub(super) enum NotSent {
    /// The runtime is stopped after failing.
    Stopped,
    /// Thread number `.0` has just crashed; Pane is handling it.
    Lost(u64),
}

impl NotSent {
    /// How a request that answers nothing says it was not sent.
    pub(super) fn error(self) -> CallError {
        match self {
            NotSent::Stopped => stopped(),
            NotSent::Lost(_) => lost_in(&RuntimeStatus::Running),
        }
    }
}

/// Counts thread number `.1` as handled when it ends, however its handling
/// ends.
struct Handled(Weak<Shared>, u64);

impl Drop for Handled {
    fn drop(&mut self) {
        if let Some(shared) = self.0.upgrade() {
            shared.mark_handled(self.1);
        }
    }
}

impl Drop for Shared {
    /// The last handle is gone (Pane is quitting): so are the helpers,
    /// which would otherwise outlive it. The thread stops with its requests.
    fn drop(&mut self) {
        self.helpers.stop_all();
        // A thread made to hang ends too.
        #[cfg(any(test, debug_assertions))]
        self.inject(Fault::Release);
    }
}

struct Current {
    /// The thread serving calls; `None` while the runtime is stopped.
    thread: Option<Thread>,
    /// Counts the threads started, to tell a report of an old one.
    started: u64,
    /// When the runtime last failed, since the user last restarted it.
    last_failure: Option<Instant>,
    status: RuntimeStatus,
}

/// A runtime thread's end of its requests.
#[derive(Clone)]
struct Thread {
    requests: mpsc::UnboundedSender<Request>,
    /// Its number among the threads started.
    number: u64,
    /// Where faults are injected into it.
    #[cfg(any(test, debug_assertions))]
    faults: Arc<Faults>,
    /// Where a slow host call is injected into it.
    #[cfg(any(test, debug_assertions))]
    watch: Arc<Watch>,
}

impl Shared {
    /// Starts the first runtime thread with `code`.
    pub(super) fn start(
        code: Arc<Code>,
        applications: SharedApplications,
        helpers: Helpers,
        cache_dir: Option<PathBuf>,
    ) -> Result<Arc<Shared>, CallError> {
        let shared = Arc::new(Shared {
            helpers,
            applications,
            files: crate::files::FileAccess::default(),
            clipboard: SharedClipboard::default(),
            directory: SharedDirectory::default(),
            health: Arc::default(),
            next_view: Arc::default(),
            network: Arc::default(),
            #[cfg(any(test, debug_assertions))]
            search_timer: Arc::default(),
            crashes: Mutex::new(None),
            slow: Mutex::new(None),
            limits: Arc::default(),
            handled: watch::Sender::new(0),
            abandoned: Mutex::default(),
            busy: Mutex::default(),
            #[cfg(any(test, debug_assertions))]
            hung: Mutex::default(),
            cache_dir,
            current: Mutex::new(Current {
                thread: None,
                started: 0,
                last_failure: None,
                status: RuntimeStatus::Running,
            }),
        });
        {
            let mut current = lock(&shared.current);
            let thread = shared.spawn(code, 1)?;
            current.thread = Some(thread);
            current.started = 1;
        }
        Ok(shared)
    }

    /// Sends `request` to the runtime thread, returning its number.
    pub(super) fn send(&self, request: Request) -> Result<u64, NotSent> {
        let (requests, number) = match &lock(&self.current).thread {
            Some(thread) => (thread.requests.clone(), thread.number),
            None => return Err(NotSent::Stopped),
        };
        // A thread that just crashed has dropped its requests: this one is
        // not sent, nor sent again to the next thread.
        requests
            .send(request)
            .map(|()| number)
            .map_err(|_| NotSent::Lost(number))
    }

    /// Tells, through [`lost`], when a thread that fails after this call
    /// has been handled.
    pub(super) fn handled(&self) -> watch::Receiver<u64> {
        self.handled.subscribe()
    }

    /// Notes that Pane is done with thread number `number`.
    fn mark_handled(&self, number: u64) {
        self.handled.send_if_modified(|handled| {
            let later = number > *handled;
            *handled = (*handled).max(number);
            later
        });
    }

    /// How many runtime threads Pane gave up on, as they stopped
    /// responding, are still stuck.
    pub(super) fn abandoned(&self) -> usize {
        let mut abandoned = lock(&self.abandoned);
        abandoned.retain(|watch| watch.abandoned());
        abandoned.len()
    }

    pub(super) fn limits(&self) -> Limits {
        *lock(&self.limits)
    }

    #[cfg(any(test, debug_assertions))]
    pub(super) fn set_limits(&self, limits: Limits) {
        *lock(&self.limits) = limits;
    }

    pub(super) fn set_slow_report(&self, report: SlowReport) {
        *lock(&self.slow) = Some(report);
    }

    pub(super) fn status(&self) -> RuntimeStatus {
        lock(&self.current).status.clone()
    }

    pub(super) fn set_crash_report(&self, report: CrashReport) {
        *lock(&self.crashes) = Some(report);
    }

    #[cfg(any(test, debug_assertions))]
    pub(super) fn inject(&self, fault: Fault) {
        // Released wherever they hang, even once another thread serves.
        if fault == Fault::Release {
            for hung in lock(&self.hung).drain(..) {
                hung.inject(Fault::Release);
            }
            return;
        }
        let Some(thread) = lock(&self.current).thread.clone() else {
            return;
        };
        if let Fault::SlowHostCall(slow) = fault {
            thread.watch.slow_next_host_call(slow);
            return;
        }
        if fault == Fault::Hang {
            lock(&self.hung).push(thread.faults.clone());
        }
        thread.faults.inject(fault);
    }

    /// Starts the runtime again after it stopped, at the user's request:
    /// failures before this no longer count against restarting it by
    /// itself. Nothing that ran before is run again. A runtime that runs is
    /// left as it is.
    pub(super) fn restart(self: &Arc<Shared>) -> Result<(), CallError> {
        let mut current = lock(&self.current);
        if current.thread.is_some() {
            return Ok(());
        }
        if self.helpers.quitting() {
            return Err(CallError::RuntimeUnavailable("Pane is quitting".into()));
        }
        let number = current.started + 1;
        let thread = self.spawn(Arc::new(Code::new(self.engine()?)), number)?;
        current.thread = Some(thread);
        current.started = number;
        current.last_failure = None;
        current.status = RuntimeStatus::Running;
        Ok(())
    }

    fn engine(&self) -> Result<wasmtime::Engine, CallError> {
        super::engine(self.cache_dir.clone())
    }

    /// Starts runtime thread number `number`, serving calls with `code`.
    fn spawn(self: &Arc<Shared>, code: Arc<Code>, number: u64) -> Result<Thread, CallError> {
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(unavailable)?;
        let (requests, receiver) = mpsc::unbounded_channel();
        let faults = Arc::new(Faults::default());
        let watch = Arc::new(Watch::default());
        // Its epoch ticks, which end with it (see `deadlines::tick`).
        deadlines::tick(&code.engine, watch.clone());
        let host = Host::new(
            code,
            self,
            number,
            Arc::clone(&faults),
            watch.clone(),
            requests.downgrade(),
        );
        let shared = Arc::downgrade(self);
        {
            let (watch, shared) = (watch.clone(), shared.clone());
            std::thread::Builder::new()
                .name("pane-extension-runtime".into())
                .spawn(move || {
                    // Dropped last, once the failure (if any) was handled.
                    let _handled = Handled(shared.clone(), number);
                    let served = std::panic::catch_unwind(AssertUnwindSafe(|| {
                        executor.block_on(Watched {
                            watch: &watch,
                            work: Box::pin(host.serve(receiver)),
                        })
                    }));
                    drop(executor);
                    // What it held is freed now, even if Pane gave up on it.
                    watch.finish();
                    if !watch.end() {
                        // Pane gave up on it while it was stuck, and handled
                        // that then.
                        return;
                    }
                    if let Err(panic) = served {
                        failed(
                            &shared,
                            number,
                            RuntimeFailure::Crashed,
                            panic_message(&*panic),
                        );
                    }
                })
                .map_err(unavailable)?;
        }
        watchdog(shared, watch.clone(), number);
        Ok(Thread {
            requests,
            number,
            #[cfg(any(test, debug_assertions))]
            faults,
            #[cfg(any(test, debug_assertions))]
            watch,
        })
    }
}

/// Watches runtime thread `number` on a thread of its own, following its
/// heartbeat (see [`Watch`] and [`Quiet`]): says when it is not responding
/// yet, and when it carries on, and gives up on it once it made no progress
/// for its limit. It stops once the thread's end was handled, or every
/// runtime handle is gone.
fn watchdog(shared: Weak<Shared>, watch: Arc<Watch>, number: u64) {
    let _ = std::thread::Builder::new()
        .name("pane-runtime-watchdog".into())
        .spawn(move || {
            let mut quiet = Quiet::new(Instant::now());
            loop {
                std::thread::sleep(deadlines::WATCH_EVERY);
                if watch.ended() {
                    return;
                }
                let Some(runtime) = shared.upgrade() else {
                    return;
                };
                let limits = runtime.limits();
                let verdict = quiet.observe(Instant::now(), watch.progress(), &limits);
                let slow = |slow: bool| {
                    let report = lock(&runtime.slow).clone();
                    if let Some(report) = report {
                        report(number, slow);
                    }
                };
                match verdict {
                    Verdict::Fine => {}
                    Verdict::Slow => slow(true),
                    Verdict::Recovered => slow(false),
                    Verdict::GiveUp => {
                        // Only exactly as last seen: a thread that moved on
                        // meanwhile is watched on.
                        let seen = quiet.last().expect("seen before giving up");
                        if !watch.give_up(seen) {
                            continue;
                        }
                        lock(&runtime.abandoned).push(watch.clone());
                        drop(runtime);
                        failed(
                            &shared,
                            number,
                            RuntimeFailure::Unresponsive,
                            format!(
                                "its thread made no progress for {}; its last known \
                                 work was {}. It was not running an extension's code, which \
                                 yields to Pane at every tick, nor inside a host call Pane \
                                 marks; which code held it is not known",
                                deadlines::seconds(limits.unresponsive),
                                watch.what().describe()
                            ),
                        );
                        return;
                    }
                }
            }
        });
}

/// Runtime thread `number` failed (`failure`) with `why`: it crashed, and
/// its unwinding dropped everything it held, or it stopped responding and
/// Pane gave up on it. Ends the helpers its guests left running, restarts
/// it unless it failed within [`CRASH_WINDOW`] of its previous failure,
/// tells the launcher, and counts the thread as handled.
fn failed(shared: &Weak<Shared>, number: u64, failure: RuntimeFailure, why: String) {
    let Some(shared) = shared.upgrade() else {
        // Pane is quitting.
        return;
    };
    // Every helper still running was started by the failed thread's
    // guests: no other thread runs until this one has been handled.
    shared.helpers.stop_running();
    let status = {
        let mut current = lock(&shared.current);
        // Only the thread serving calls runs guests and so can fail: a
        // restart replaces a thread only once it has stopped or Pane gave
        // up on it.
        current.thread = None;
        let now = Instant::now();
        let again = restarts_automatically(current.last_failure, now);
        current.last_failure = Some(now);
        let restarted = if shared.helpers.quitting() {
            Err("Pane is quitting".to_owned())
        } else if !again {
            Err(format!(
                "it stopped twice within {} minutes; repeated automatic restarts are suppressed",
                CRASH_WINDOW.as_secs() / 60
            ))
        } else {
            let next = number + 1;
            shared
                .engine()
                .and_then(|engine| shared.spawn(Arc::new(Code::new(engine)), next))
                .map(|thread| {
                    current.thread = Some(thread);
                    current.started = next;
                })
                .map_err(|error| format!("starting it again failed: {error}"))
        };
        current.status = match restarted {
            Ok(()) => RuntimeStatus::Restarted { failure, why },
            Err(not_restarted) => RuntimeStatus::Stopped {
                failure,
                why,
                not_restarted,
            },
        };
        current.status.clone()
    };
    eprintln!("Pane's extension runtime stopped unexpectedly: {status:?}");
    let report = lock(&shared.crashes).clone();
    if let Some(report) = report {
        report(number, &status);
    }
    shared.mark_handled(number);
}

/// Whether a failure at `now` restarts the runtime by itself: not when it
/// already failed within [`CRASH_WINDOW`] before (`previous`, since the
/// user last restarted it).
pub(crate) fn restarts_automatically(previous: Option<Instant>, now: Instant) -> bool {
    previous.is_none_or(|previous| now.saturating_duration_since(previous) >= CRASH_WINDOW)
}

/// The message of a panic, for diagnostics.
fn panic_message(panic: &(dyn Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "the runtime thread panicked".to_owned()
    }
}

/// How a call answers when the runtime is stopped: it is not sent.
pub(super) fn stopped() -> CallError {
    CallError::RuntimeUnavailable(
        "it stopped after failing and runs nothing until you restart it in Manage extensions"
            .into(),
    )
}

/// Resolves once Pane is done with thread number `number`, which failed;
/// never while it runs.
pub(super) async fn handled_after(mut handled: watch::Receiver<u64>, number: u64) {
    // The sender goes only with the last runtime handle.
    if handled.wait_for(|done| *done >= number).await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// How a call answers when runtime thread `number` failed before answering
/// it, once Pane has handled the failure: whether it restarted the
/// runtime. It is not sent again.
pub(super) async fn lost(
    shared: Weak<Shared>,
    mut handled: watch::Receiver<u64>,
    number: u64,
) -> CallError {
    // The thread counts itself as handled however its handling ends.
    let _ = handled.wait_for(|done| *done >= number).await;
    let status = match shared.upgrade() {
        Some(shared) => shared.status(),
        None => RuntimeStatus::Running,
    };
    lost_in(&status)
}

/// How a call whose answer was lost answers, while the runtime does
/// `status`.
fn lost_in(status: &RuntimeStatus) -> CallError {
    let stopped = status.failure().map_or(
        "it stopped before answering",
        RuntimeFailure::stopped_before_answering,
    );
    CallError::RuntimeUnavailable(match status {
        RuntimeStatus::Restarted { .. } => {
            format!("{stopped} and was started again; Pane does not run this again by itself")
        }
        RuntimeStatus::Stopped { not_restarted, .. } => format!(
            "{stopped} and was not restarted ({not_restarted}); Pane does not run this again by \
             itself. Restart it in Manage extensions"
        ),
        RuntimeStatus::Running => format!("{stopped}; Pane does not run this again by itself"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_crash_restarts_the_runtime() {
        assert!(restarts_automatically(None, Instant::now()));
    }

    #[test]
    fn a_second_crash_within_the_window_does_not() {
        let first = Instant::now();
        let soon = first + CRASH_WINDOW - Duration::from_secs(1);
        assert!(!restarts_automatically(Some(first), soon));
    }

    #[test]
    fn a_crash_after_the_window_restarts_it_again() {
        let first = Instant::now();
        assert!(restarts_automatically(Some(first), first + CRASH_WINDOW));
    }

    #[test]
    fn a_panic_message_is_kept() {
        let panic = std::panic::catch_unwind(|| panic!("broke {}", 1)).unwrap_err();
        assert_eq!(panic_message(&*panic), "broke 1");
        let panic = std::panic::catch_unwind(|| panic!("static")).unwrap_err();
        assert_eq!(panic_message(&*panic), "static");
    }
}
