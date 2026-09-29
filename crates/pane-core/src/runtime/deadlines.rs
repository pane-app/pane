//! Keeping Pane's extension runtime responsive when a guest stops
//! cooperating (#18).
//!
//! The runtime thread serves one guest call at a time, so a call that never
//! finishes holds every other extension's calls behind it. Pane bounds each
//! way a call can fail to finish, and never charges a guest for work that
//! is not its own:
//!
//! - **An unresponsive call**: a guest computing without waiting (a busy
//!   loop). The engine counts epochs ([`TICK`] apart) and every store yields
//!   to the runtime thread at each one, so the thread keeps checking the
//!   call's generation and injected faults however busy the guest is. A
//!   call whose guest computed for the compute limit ([`COMPUTE_LIMIT`]) in
//!   all is stopped: its instance is dropped, as a stopped call's is, and,
//!   since Wasmtime knows exactly which guest was executing, the failure is
//!   that package's own and counts towards pausing it, as a crash does.
//!   Only the guest's own computing counts ([`Meter`]): the runtime
//!   thread's CPU time while it polls the call, less the time spent in
//!   Pane's host calls ([`HostCall`]). Waiting (on a clock, a helper, an
//!   operation, a save) is not computing, nor is a slow host call, nor time
//!   the system gave to other threads. Starting an instance is never
//!   counted.
//! - **A native helper that does not exit**: its supervising thread ends it
//!   after `helpers::runner::HELPER_TIME_LIMIT`, and the run answers an
//!   error the guest handles like any other failure of its helper: an
//!   expected slow operation, not a crash.
//! - **A runtime hang**: the shared thread itself makes no progress. Its
//!   progress is a heartbeat ([`Watch`]): a count bumped at each poll of its
//!   work, at each epoch yield of a guest and as each host call starts and
//!   ends. A watchdog thread that sees the count still, while the thread is
//!   inside one poll and not inside a host call, says so after
//!   [`WARN_AFTER`] ("not responding yet") and gives up on it after
//!   [`UNRESPONSIVE_LIMIT`]. Which extension, if any, caused it is not
//!   known, so none is named or paused; every call it held is answered,
//!   its helpers are ended and a fresh thread serves calls (see
//!   `supervisor`). A thread cannot be ended from outside, so the stuck one
//!   is abandoned: its [`Fence`] closes, so whatever it still runs changes
//!   nothing, a guest it runs traps at its next epoch check, and it frees
//!   what it holds only once it returns.
//!
//! The values are explicit choices, not measurements (provisional); tests
//! and smokes may shorten them ([`Limits`], debug builds only).

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use crate::generation::Fence;

/// How often the engine's epoch advances, and so how often a computing
/// guest yields to the runtime thread.
pub(crate) const TICK: Duration = Duration::from_millis(10);

/// How long a guest call may compute, in all, without finishing: an
/// unresponsive call is stopped then. Generous for a launcher's calls, and
/// short enough that other extensions do not wait long behind a busy loop.
pub const COMPUTE_LIMIT: Duration = Duration::from_secs(5);

/// How long the runtime thread may make no progress before Pane says it is
/// not responding yet.
pub const WARN_AFTER: Duration = Duration::from_secs(10);

/// How long the runtime thread may make no progress before Pane gives up
/// on it (a runtime hang).
pub const UNRESPONSIVE_LIMIT: Duration = Duration::from_secs(30);

/// How often the watchdog looks at the runtime thread.
pub(super) const WATCH_EVERY: Duration = Duration::from_millis(100);

/// The limits a runtime applies ([`COMPUTE_LIMIT`], [`WARN_AFTER`],
/// [`UNRESPONSIVE_LIMIT`]). Tests and smokes shorten them in debug builds
/// (`Runtime::set_limits`); Pane itself uses the defaults.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub compute: Duration,
    pub warn: Duration,
    pub unresponsive: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            compute: COMPUTE_LIMIT,
            warn: WARN_AFTER,
            unresponsive: UNRESPONSIVE_LIMIT,
        }
    }
}

/// The CPU time the calling thread has used, where the system says;
/// otherwise the time since Pane started, so that time is counted only
/// while the guest runs (see [`Meter`]).
pub(crate) fn thread_time() -> Duration {
    #[cfg(unix)]
    {
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: clock_gettime writes the timespec it is given.
        if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut now) } == 0 {
            return Duration::new(
                u64::try_from(now.tv_sec).unwrap_or(0),
                u32::try_from(now.tv_nsec).unwrap_or(0),
            );
        }
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY: GetThreadTimes writes the four times it is given, for the
        // current thread's pseudo handle.
        let read = unsafe {
            GetThreadTimes(
                GetCurrentThread(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        };
        if read.is_ok() {
            let ticks = |time: FILETIME| {
                (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
            };
            // In units of 100 ns.
            return Duration::from_nanos((ticks(kernel) + ticks(user)).saturating_mul(100));
        }
    }
    static ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed()
}

/// What the watchdog knows of one runtime thread, and how the thread's
/// progress is told.
pub(crate) struct Watch {
    /// Closed once Pane gives up on the thread.
    fence: Fence,
    /// The heartbeat: bumped at each poll of the thread's work, each epoch
    /// yield of a guest and as each host call starts and ends.
    beats: AtomicU64,
    /// How many host calls the thread is inside of.
    host_depth: AtomicUsize,
    /// The thread's time spent in host calls, in nanoseconds (see
    /// [`thread_time`]), which is not charged to guests.
    host_time: AtomicU64,
    /// How many exemptions (compiling a component) are in force.
    exempt: AtomicUsize,
    /// Which poll of its work the thread is inside, if any.
    polls: Mutex<Polls>,
    /// Set once Pane gave up on the thread.
    given_up: AtomicBool,
    /// Set once the thread's end was handled: it stopped, crashed or was
    /// given up on, whichever came first.
    ended: AtomicBool,
    /// Set once the thread itself returned, even after it was given up on.
    finished: AtomicBool,
    /// What the thread is doing ([`Doing`]), for diagnostics.
    doing: AtomicU8,
    /// How long its next host call computes, for
    /// [`super::Fault::SlowHostCall`].
    #[cfg(any(test, debug_assertions))]
    slow_host: Mutex<Option<Duration>>,
}

#[derive(Default)]
struct Polls {
    current: Option<u64>,
    started: u64,
}

impl Default for Watch {
    fn default() -> Watch {
        Watch {
            fence: Fence::default(),
            beats: AtomicU64::new(0),
            host_depth: AtomicUsize::new(0),
            host_time: AtomicU64::new(0),
            exempt: AtomicUsize::new(0),
            polls: Mutex::new(Polls::default()),
            given_up: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            doing: AtomicU8::new(Doing::Waiting as u8),
            #[cfg(any(test, debug_assertions))]
            slow_host: Mutex::new(None),
        }
    }
}

/// What a runtime thread is doing, for the diagnostics of one that stops
/// responding. It names no extension: which one, if any, caused a stuck
/// thread is not known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Doing {
    Waiting,
    Handling,
    Starting,
    Running,
}

impl Doing {
    fn from(value: u8) -> Doing {
        match value {
            1 => Doing::Handling,
            2 => Doing::Starting,
            3 => Doing::Running,
            _ => Doing::Waiting,
        }
    }

    /// "a guest call", for "its last known work was …".
    pub(super) fn describe(self) -> &'static str {
        match self {
            Doing::Waiting => "waiting for a request",
            Doing::Handling => "handling a request",
            Doing::Starting => "starting a guest instance",
            Doing::Running => "running a guest call",
        }
    }
}

/// Puts back what the thread was doing before, when dropped.
pub(super) struct Done<'a> {
    watch: &'a Watch,
    before: u8,
}

impl Drop for Done<'_> {
    fn drop(&mut self) {
        self.watch.doing.store(self.before, Ordering::SeqCst);
    }
}

/// Keeps the watchdog from counting the time it lives, such as compiling a
/// component, which may take long without anything being stuck.
pub(super) struct Exempt<'a>(&'a Watch);

impl Drop for Exempt<'_> {
    fn drop(&mut self) {
        self.0.exempt.fetch_sub(1, Ordering::SeqCst);
    }
}

/// One host call in progress on the runtime thread (see [`Watch::host`]).
pub(crate) struct HostCall<'a> {
    watch: &'a Watch,
    started: Duration,
}

impl Drop for HostCall<'_> {
    fn drop(&mut self) {
        let spent = thread_time().saturating_sub(self.started);
        let nanos = u64::try_from(spent.as_nanos()).unwrap_or(u64::MAX);
        self.watch.host_time.fetch_add(nanos, Ordering::SeqCst);
        self.watch.host_depth.fetch_sub(1, Ordering::SeqCst);
        self.watch.beat();
    }
}

/// What the watchdog reads of a thread's progress at one moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Progress {
    /// The poll the thread is inside, if any.
    poll: Option<u64>,
    beats: u64,
    /// Inside a host call, or exempt: never stuck by the watchdog's count.
    sheltered: bool,
}

impl Watch {
    /// The fence closed when Pane gives up on the thread.
    pub(crate) fn fence(&self) -> &Fence {
        &self.fence
    }

    /// Tells the watchdog the thread made progress.
    pub(crate) fn beat(&self) {
        self.beats.fetch_add(1, Ordering::SeqCst);
    }

    /// Marks a host call on the runtime thread until the guard is dropped:
    /// its time is not charged to the guest, and the watchdog does not give
    /// up on the thread meanwhile (what it does must not block for long).
    pub(crate) fn host(&self) -> HostCall<'_> {
        self.host_depth.fetch_add(1, Ordering::SeqCst);
        self.beat();
        let call = HostCall {
            watch: self,
            started: thread_time(),
        };
        #[cfg(any(test, debug_assertions))]
        {
            let slow = self
                .slow_host
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take();
            if let Some(slow) = slow {
                let started = thread_time();
                while thread_time().saturating_sub(started) < slow {
                    std::hint::spin_loop();
                }
            }
        }
        call
    }

    /// Makes the thread's next host call compute for `slow` itself
    /// ([`super::Fault::SlowHostCall`]).
    #[cfg(any(test, debug_assertions))]
    pub(super) fn slow_next_host_call(&self, slow: Duration) {
        *self.slow_host.lock().unwrap_or_else(|p| p.into_inner()) = Some(slow);
    }

    /// The thread's time spent in host calls so far.
    fn host_time(&self) -> Duration {
        Duration::from_nanos(self.host_time.load(Ordering::SeqCst))
    }

    /// Notes that the thread does `doing` until the guard is dropped.
    pub(super) fn doing(&self, doing: Doing) -> Done<'_> {
        Done {
            watch: self,
            before: self.doing.swap(doing as u8, Ordering::SeqCst),
        }
    }

    /// What the thread is doing.
    pub(super) fn what(&self) -> Doing {
        Doing::from(self.doing.load(Ordering::SeqCst))
    }

    /// Exempts what the thread does while the guard lives.
    pub(super) fn exempt(&self) -> Exempt<'_> {
        self.exempt.fetch_add(1, Ordering::SeqCst);
        self.beat();
        Exempt(self)
    }

    fn polls(&self) -> std::sync::MutexGuard<'_, Polls> {
        self.polls.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The thread's progress now.
    pub(super) fn progress(&self) -> Progress {
        let poll = self.polls().current;
        Progress {
            poll,
            beats: self.beats.load(Ordering::SeqCst),
            sheltered: self.host_depth.load(Ordering::SeqCst) > 0
                || self.exempt.load(Ordering::SeqCst) > 0,
        }
    }

    /// Whether Pane gave up on the thread.
    pub(super) fn given_up(&self) -> bool {
        self.given_up.load(Ordering::SeqCst)
    }

    /// Gives up on the thread, if it is still where `seen` found it: inside
    /// the same poll, with no beat since and outside any host call. A
    /// thread that returned from that poll meanwhile (checked under the
    /// lock it returns under) or whose end was handled already is left
    /// alone. Returns whether this gave up on it. The fence closes once no
    /// change holds it open.
    pub(super) fn give_up(&self, seen: Progress) -> bool {
        {
            let polls = self.polls();
            let now = Progress {
                poll: polls.current,
                beats: self.beats.load(Ordering::SeqCst),
                sheltered: self.host_depth.load(Ordering::SeqCst) > 0
                    || self.exempt.load(Ordering::SeqCst) > 0,
            };
            if now != seen || seen.poll.is_none() || seen.sheltered {
                return false;
            }
            // The thread cannot end meanwhile: it is inside the poll, which
            // it leaves under this lock. Given up before ended, so that
            // whatever sees it ended (the epoch ticker's last tick) sees it
            // given up too.
            if self.ended.load(Ordering::SeqCst) {
                return false;
            }
            self.given_up.store(true, Ordering::SeqCst);
            self.ended.store(true, Ordering::SeqCst);
        }
        self.fence.close();
        true
    }

    /// Notes that the thread ended (it stopped or crashed); returns whether
    /// its end is this one to handle, rather than Pane having given up on
    /// it before.
    pub(super) fn end(&self) -> bool {
        !self.ended.swap(true, Ordering::SeqCst)
    }

    /// Whether the thread's end was handled.
    pub(super) fn ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }

    /// Notes that the thread itself returned.
    pub(super) fn finish(&self) {
        self.finished.store(true, Ordering::SeqCst);
    }

    /// Whether Pane gave up on the thread and it has not returned yet.
    pub(super) fn abandoned(&self) -> bool {
        self.given_up() && !self.finished.load(Ordering::SeqCst)
    }
}

/// What the watchdog makes of what it saw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    Fine,
    /// No progress for [`Limits::warn`]: say it is not responding yet.
    Slow,
    /// Progress again after [`Verdict::Slow`].
    Recovered,
    /// No progress for [`Limits::unresponsive`]: give up on it.
    GiveUp,
}

/// How long a thread has made no progress, from what the watchdog saw.
pub(super) struct Quiet {
    last: Option<Progress>,
    since: Instant,
    warned: bool,
}

impl Quiet {
    pub(super) fn new(now: Instant) -> Quiet {
        Quiet {
            last: None,
            since: now,
            warned: false,
        }
    }

    /// What `seen` at `now` means. The thread is quiet while it stays inside
    /// one poll, outside any host call, with its heartbeat still; waiting
    /// for work (outside any poll) or inside a host call is not quiet.
    pub(super) fn observe(&mut self, now: Instant, seen: Progress, limits: &Limits) -> Verdict {
        let moving =
            seen.poll.is_none() || seen.sheltered || self.last.is_none_or(|last| last != seen);
        if moving {
            self.last = Some(seen);
            self.since = now;
            return match std::mem::take(&mut self.warned) {
                true => Verdict::Recovered,
                false => Verdict::Fine,
            };
        }
        let quiet = now.saturating_duration_since(self.since);
        if quiet >= limits.unresponsive {
            Verdict::GiveUp
        } else if quiet >= limits.warn && !self.warned {
            self.warned = true;
            Verdict::Slow
        } else {
            Verdict::Fine
        }
    }

    /// What the watchdog saw last, to give up on exactly that.
    pub(super) fn last(&self) -> Option<Progress> {
        self.last
    }
}

/// Advances `engine`'s epoch every [`TICK`] for the runtime thread `watch`
/// watches, on a thread of its own, which stops once that thread's end was
/// handled or the engine is gone. When Pane gave up on the thread, a last
/// tick makes a guest it still runs reach its epoch check, where it traps.
pub(super) fn tick(engine: &wasmtime::Engine, watch: Arc<Watch>) {
    let engine = engine.weak();
    let _ = std::thread::Builder::new()
        .name("pane-runtime-epoch".into())
        .spawn(move || {
            loop {
                std::thread::sleep(TICK);
                let Some(engine) = engine.upgrade() else {
                    return;
                };
                // Read before the tick, so the last tick follows the end.
                let ended = watch.ended();
                engine.increment_epoch();
                if ended {
                    return;
                }
            }
        });
}

/// How long a guest call computed: the runtime thread's time while it
/// polls the call ([`thread_time`]), less its time in host calls. The guest
/// yields at each epoch tick, so a poll that computes returns within one,
/// and a guest that waits is not polled at all.
pub(super) struct Meter {
    watch: Arc<Watch>,
    limit: Duration,
    spent: Duration,
}

impl Meter {
    pub(super) fn new(watch: Arc<Watch>, limit: Duration) -> Meter {
        Meter {
            watch,
            limit,
            spent: Duration::ZERO,
        }
    }

    /// Polls with `poll`, counting the guest's time it takes.
    pub(super) fn measure<T>(&mut self, poll: impl FnOnce() -> Poll<T>) -> Poll<T> {
        let (started, hosted) = (thread_time(), self.watch.host_time());
        let polled = poll();
        let hosted = self.watch.host_time().saturating_sub(hosted);
        self.spent += thread_time().saturating_sub(started).saturating_sub(hosted);
        polled
    }

    /// Whether the call computed for its limit or more.
    pub(super) fn exhausted(&self) -> bool {
        self.spent >= self.limit
    }
}

/// Why a guest call was stopped as unresponsive, for its error.
pub(super) fn computed_too_long(limit: Duration) -> String {
    format!(
        "it computed for {} seconds without finishing, so Pane stopped it; other extensions' \
         calls waited meanwhile",
        limit.as_secs_f32()
    )
}

/// A guest's future, such as a call or a destructor, that ends with an
/// error once it computed for the compute limit (see [`Meter`]).
pub(super) struct Metered<F> {
    future: Pin<Box<F>>,
    meter: Meter,
    /// Where the compute limit is read at each poll, so a changed limit
    /// applies to calls already running.
    limits: Arc<Mutex<Limits>>,
}

/// Runs `future`, guest code, until it finishes or has computed for the
/// compute limit of `limits` (see [`Meter`]); the error then says so.
pub(super) fn metered<F: Future>(
    watch: Arc<Watch>,
    limits: Arc<Mutex<Limits>>,
    future: F,
) -> Metered<F> {
    let limit = limits.lock().unwrap_or_else(|p| p.into_inner()).compute;
    Metered {
        future: Box::pin(future),
        meter: Meter::new(watch, limit),
        limits,
    }
}

impl<F: Future> Future for Metered<F> {
    type Output = Result<F::Output, String>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        this.meter.limit = this
            .limits
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .compute;
        let future = &mut this.future;
        match this.meter.measure(|| future.as_mut().poll(cx)) {
            Poll::Ready(output) => Poll::Ready(Ok(output)),
            Poll::Pending if this.meter.exhausted() => {
                Poll::Ready(Err(computed_too_long(this.meter.limit)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// A host call's future (a helper's run, a web request): each poll of it is
/// a host call (see [`Watch::host`]).
pub(crate) struct Hosted<F> {
    watch: Arc<Watch>,
    future: Pin<Box<F>>,
}

/// Marks each poll of `future` as a host call on `watch`'s thread.
pub(crate) fn hosted<F: Future>(watch: Arc<Watch>, future: F) -> Hosted<F> {
    Hosted {
        watch,
        future: Box::pin(future),
    }
}

impl<F: Future> Future for Hosted<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = &mut *self;
        let _host = this.watch.host();
        this.future.as_mut().poll(cx)
    }
}

/// Notes that the thread left the poll it was inside, when dropped.
struct Leave<'a>(&'a Watch);

impl Drop for Leave<'_> {
    fn drop(&mut self) {
        self.0.polls().current = None;
    }
}

/// The runtime thread's work, noting in its [`Watch`] each poll it is
/// inside. Once Pane gave up on the thread, it ends at its next poll.
pub(super) struct Watched<'a, F> {
    pub(super) watch: &'a Watch,
    pub(super) work: Pin<Box<F>>,
}

impl<F: Future<Output = ()>> Future for Watched<'_, F> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.watch.given_up() {
            return Poll::Ready(());
        }
        {
            let mut polls = self.watch.polls();
            polls.started += 1;
            polls.current = Some(polls.started);
        }
        self.watch.beat();
        let watch = self.watch;
        let leave = Leave(watch);
        let polled = self.work.as_mut().poll(cx);
        // Returned in time (or unwinding from a panic): the watchdog, which
        // checks under the same lock, no longer gives up on this poll.
        drop(leave);
        if self.watch.given_up() {
            return Poll::Ready(());
        }
        polled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> Limits {
        Limits {
            compute: Duration::from_secs(1),
            warn: Duration::from_secs(10),
            unresponsive: Duration::from_secs(30),
        }
    }

    fn inside(poll: u64, beats: u64) -> Progress {
        Progress {
            poll: Some(poll),
            beats,
            sheltered: false,
        }
    }

    #[test]
    fn a_thread_still_inside_one_poll_warns_then_is_given_up() {
        let start = Instant::now();
        let mut quiet = Quiet::new(start);
        let s = |secs| start + Duration::from_secs(secs);
        assert_eq!(quiet.observe(s(0), inside(1, 5), &limits()), Verdict::Fine);
        assert_eq!(quiet.observe(s(9), inside(1, 5), &limits()), Verdict::Fine);
        assert_eq!(quiet.observe(s(10), inside(1, 5), &limits()), Verdict::Slow);
        assert_eq!(quiet.observe(s(20), inside(1, 5), &limits()), Verdict::Fine);
        assert_eq!(
            quiet.observe(s(30), inside(1, 5), &limits()),
            Verdict::GiveUp
        );
    }

    #[test]
    fn a_heartbeat_a_new_poll_waiting_or_a_host_call_is_progress() {
        let start = Instant::now();
        let s = |secs| start + Duration::from_secs(secs);
        let mut quiet = Quiet::new(start);
        quiet.observe(s(0), inside(1, 5), &limits());
        assert_eq!(quiet.observe(s(12), inside(1, 5), &limits()), Verdict::Slow);
        // A beat: a guest yielded, or a host call started or ended.
        assert_eq!(
            quiet.observe(s(13), inside(1, 6), &limits()),
            Verdict::Recovered
        );
        assert_eq!(
            quiet.observe(s(43), inside(1, 6), &limits()),
            Verdict::GiveUp
        );
        let mut quiet = Quiet::new(start);
        quiet.observe(s(0), inside(1, 5), &limits());
        // Inside a host call for long, such as a save waiting for its file.
        let sheltered = Progress {
            sheltered: true,
            ..inside(1, 5)
        };
        assert_eq!(quiet.observe(s(100), sheltered, &limits()), Verdict::Fine);
        // Waiting for work, outside any poll.
        let idle = Progress {
            poll: None,
            beats: 5,
            sheltered: false,
        };
        assert_eq!(quiet.observe(s(200), idle, &limits()), Verdict::Fine);
        assert_eq!(
            quiet.observe(s(229), inside(2, 5), &limits()),
            Verdict::Fine
        );
    }

    #[test]
    fn a_thread_that_returned_in_time_is_never_given_up_on() {
        let watch = Watch::default();
        let polled = Watched {
            watch: &watch,
            work: Box::pin(std::future::pending::<()>()),
        };
        let mut polled = std::pin::pin!(polled);
        let waker = std::task::Waker::noop();
        let _ = polled.as_mut().poll(&mut Context::from_waker(waker));
        // Seen inside a poll, which it has left since.
        let seen = Progress {
            poll: Some(1),
            beats: watch.progress().beats,
            sheltered: false,
        };
        assert!(!watch.give_up(seen));
        assert!(!watch.given_up());

        // Seen exactly where it still is.
        watch.polls().current = Some(1);
        let seen = watch.progress();
        watch.beat();
        assert!(!watch.give_up(seen), "a beat since is progress");
        let seen = watch.progress();
        assert!(watch.give_up(seen));
        assert!(watch.given_up() && watch.fence().closed());
        assert!(!watch.end(), "its end was handled when Pane gave up");
        assert!(watch.abandoned());
        watch.finish();
        assert!(!watch.abandoned());
    }

    #[test]
    fn a_thread_inside_a_host_call_or_that_crashed_is_not_given_up_on() {
        let watch = Watch::default();
        watch.polls().current = Some(1);
        {
            let _host = watch.host();
            assert!(!watch.give_up(watch.progress()));
        }
        let crashed = Watch::default();
        crashed.polls().current = Some(1);
        assert!(crashed.end());
        assert!(
            !crashed.give_up(crashed.progress()),
            "a crash was handled first"
        );
        assert!(!crashed.given_up() && !crashed.fence().closed());
    }

    /// A host call's CPU time is not the guest's.
    #[test]
    fn a_meter_does_not_charge_a_host_call_to_the_guest() {
        let watch = Arc::new(Watch::default());
        let mut meter = Meter::new(watch.clone(), Duration::from_millis(200));
        let spin = |for_: Duration| {
            let started = thread_time();
            while thread_time().saturating_sub(started) < for_ {}
        };
        let _ = meter.measure(|| {
            let _host = watch.host();
            spin(Duration::from_millis(400));
            Poll::<()>::Pending
        });
        assert!(!meter.exhausted(), "{:?}", meter.spent);
        let _ = meter.measure(|| {
            spin(Duration::from_millis(250));
            Poll::<()>::Pending
        });
        assert!(meter.exhausted(), "{:?}", meter.spent);
    }

    /// Time the thread does not run (it sleeps, or the system runs others)
    /// is not the guest's.
    #[cfg(any(unix, windows))]
    #[test]
    fn a_meter_does_not_charge_time_the_thread_did_not_run() {
        let watch = Arc::new(Watch::default());
        let mut meter = Meter::new(watch, Duration::from_millis(200));
        let _ = meter.measure(|| {
            std::thread::sleep(Duration::from_millis(400));
            Poll::<()>::Pending
        });
        assert!(!meter.exhausted(), "{:?}", meter.spent);
    }
}
