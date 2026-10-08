//! Sleep and wake (#126 "Every system", story 43): indexing pauses while
//! the computer sleeps, resumes no sooner than a few seconds after it
//! wakes, and its time limits count awake time only.
//!
//! **How Pane learns of a sleep.** Every system keeps two clocks since it
//! started: one that runs on while the computer sleeps and one that stops
//! (Windows' interrupt time and its unbiased interrupt time; Linux's
//! `CLOCK_BOOTTIME` and `CLOCK_MONOTONIC`; macOS's `CLOCK_MONOTONIC` and
//! `CLOCK_UPTIME_RAW`). Their difference is how long the computer has
//! slept since it started ([`Awake::asleep`]): it grows by exactly each
//! sleep, never moves while awake, and, unlike the wall clock, is not moved
//! by the user or by time synchronization. Pane reads it whenever indexing
//! is about to do something — a batch of changes, a folder of a walk — so a
//! sleep is noticed at the first piece of work after the wake. The
//! system's own notifications (`WM_POWERBROADCAST` or
//! `PowerRegisterSuspendResumeNotification`, IOKit's
//! `IORegisterForSystemPower`, logind's `PrepareForSleep`) would also say
//! when a sleep begins, but each needs a window, a run loop or a D-Bus
//! connection of its own, and saying it earlier gains nothing: while the
//! computer sleeps every thread of Pane is frozen where it was, the walker
//! and the coordinator included, and the index's write-ahead log keeps
//! what was half applied. What a sleep changes is what follows the wake —
//! disks spinning up, network mounts reconnecting, a burst of changes —
//! and the time limits that would otherwise count the sleep; both are
//! handled from the first look after it ([`Pause`]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::util::lock;

/// How long after a wake indexing waits before it resumes (#126: "no
/// sooner than a few seconds after it wakes").
pub const RESUME_AFTER: Duration = Duration::from_secs(5);

/// Whether Rust's `Instant` keeps counting while the computer sleeps on
/// this system, so that a time limit measured with it has to take the
/// sleep out to count awake time only: on Windows it is the performance
/// counter, which runs on through a sleep as the interrupt time does; on
/// Linux (`CLOCK_MONOTONIC`) and macOS (`CLOCK_UPTIME_RAW`) it stops.
pub(crate) const INSTANT_COUNTS_SLEEP: bool = cfg!(windows);

/// A growth of [`Awake::asleep`] smaller than this is the clocks' own
/// drift, not a sleep.
pub(crate) const NOTICED: Duration = Duration::from_millis(500);

/// How long the computer has slept: the seam the tests drive.
pub trait Awake: Send + Sync {
    /// The time the computer has spent asleep since it started; it grows
    /// by each sleep and never moves while it is awake.
    fn asleep(&self) -> Duration;
}

/// The system's clocks (see the module docs).
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemAwake;

impl Awake for SystemAwake {
    fn asleep(&self) -> Duration {
        asleep_since_start()
    }
}

/// The time asleep since the system started: its clock that runs on while
/// it sleeps less its clock that stops. Zero where the system says
/// neither.
pub(crate) fn asleep_since_start() -> Duration {
    platform::asleep()
}

#[cfg(windows)]
mod platform {
    use std::time::Duration;

    use windows::Win32::System::WindowsProgramming::{
        QueryInterruptTime, QueryUnbiasedInterruptTime,
    };

    pub(super) fn asleep() -> Duration {
        let mut unbiased = 0u64;
        // SAFETY: plain reads of the system's clocks into a u64 the call
        // writes.
        let (biased, read) = unsafe {
            let biased = QueryInterruptTime();
            let read = QueryUnbiasedInterruptTime(&mut unbiased).as_bool();
            (biased, read)
        };
        if !read {
            return Duration::ZERO;
        }
        // Both count 100-nanosecond intervals.
        Duration::from_nanos(biased.saturating_sub(unbiased).saturating_mul(100))
    }
}

#[cfg(unix)]
mod platform {
    use std::time::Duration;

    #[cfg(target_os = "macos")]
    const WITH_SLEEP: libc::clockid_t = libc::CLOCK_MONOTONIC;
    #[cfg(target_os = "macos")]
    const WITHOUT_SLEEP: libc::clockid_t = libc::CLOCK_UPTIME_RAW;
    #[cfg(not(target_os = "macos"))]
    const WITH_SLEEP: libc::clockid_t = libc::CLOCK_BOOTTIME;
    #[cfg(not(target_os = "macos"))]
    const WITHOUT_SLEEP: libc::clockid_t = libc::CLOCK_MONOTONIC;

    fn read(clock: libc::clockid_t) -> Option<Duration> {
        // SAFETY: a plain C structure, for which all zeroes is a valid
        // value.
        let mut time: libc::timespec = unsafe { std::mem::zeroed() };
        // SAFETY: a clock the system names and a structure of the call's
        // own type.
        if unsafe { libc::clock_gettime(clock, &mut time) } != 0 {
            return None;
        }
        // The fields' widths differ between systems.
        #[allow(clippy::unnecessary_cast)]
        Some(Duration::new(
            u64::try_from(time.tv_sec).ok()?,
            u32::try_from(time.tv_nsec as i64).ok()?,
        ))
    }

    pub(super) fn asleep() -> Duration {
        // The moment between the two reads is far below what counts as a
        // sleep.
        let (Some(without), Some(with)) = (read(WITHOUT_SLEEP), read(WITH_SLEEP)) else {
            return Duration::ZERO;
        };
        with.saturating_sub(without)
    }
}

#[cfg(not(any(windows, unix)))]
mod platform {
    pub(super) fn asleep() -> std::time::Duration {
        std::time::Duration::ZERO
    }
}

/// Indexing's pause after a sleep, shared by the coordinator and the
/// walker's threads: whoever first notices a sleep starts the pause, and
/// everyone who looks meanwhile waits for it to end.
pub(crate) struct Pause {
    awake: Arc<dyn Awake>,
    resume_after: Duration,
    state: Mutex<PauseState>,
}

struct PauseState {
    /// [`Awake::asleep`] when last looked at.
    asleep: Duration,
    /// When the pause ends, while one is on.
    until: Option<Instant>,
}

/// A sleep [`Pause::look`] noticed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Slept {
    /// How long the computer slept.
    pub(crate) asleep: Duration,
}

impl Pause {
    pub(crate) fn new(awake: Arc<dyn Awake>, resume_after: Duration) -> Pause {
        let asleep = awake.asleep();
        Pause {
            awake,
            resume_after,
            state: Mutex::new(PauseState {
                asleep,
                until: None,
            }),
        }
    }

    /// Looks whether the computer slept since the last look: if it did,
    /// the pause starts (or starts again) now, and the sleep is answered.
    pub(crate) fn look(&self) -> Option<Slept> {
        let now = self.awake.asleep();
        let mut state = lock(&self.state);
        let grown = now.saturating_sub(state.asleep);
        state.asleep = now.max(state.asleep);
        if grown < NOTICED {
            return None;
        }
        state.until = Some(Instant::now() + self.resume_after);
        Some(Slept { asleep: grown })
    }

    /// Whether a pause is on now.
    pub(crate) fn on(&self) -> bool {
        lock(&self.state)
            .until
            .is_some_and(|until| Instant::now() < until)
    }

    /// Waits until the pause that is on ends (at once if none is), or
    /// until `stop` is set. Looks for a sleep first, so a caller that is
    /// first to notice one starts the pause it then waits for. The answer
    /// is the sleep this look noticed, if it noticed one.
    pub(crate) fn hold(&self, stop: &AtomicBool) -> Option<Slept> {
        let slept = self.look();
        loop {
            let left = {
                let mut state = lock(&self.state);
                match state.until {
                    Some(until) => match until.checked_duration_since(Instant::now()) {
                        Some(left) if !left.is_zero() => left,
                        _ => {
                            state.until = None;
                            return slept;
                        }
                    },
                    None => return slept,
                }
            };
            if stop.load(Ordering::Relaxed) {
                return slept;
            }
            std::thread::sleep(left.min(Duration::from_millis(20)));
            // A second sleep during the pause starts it again.
            self.look();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    /// A computer the test puts to sleep.
    #[derive(Default)]
    pub(crate) struct FakeAwake(AtomicU64);

    impl FakeAwake {
        /// The computer slept for `span`.
        pub(crate) fn sleep(&self, span: Duration) {
            self.0.fetch_add(span.as_millis() as u64, Ordering::SeqCst);
        }
    }

    impl Awake for FakeAwake {
        fn asleep(&self) -> Duration {
            Duration::from_millis(self.0.load(Ordering::SeqCst))
        }
    }

    #[test]
    fn the_systems_clocks_say_a_time_asleep_that_never_goes_back() {
        let first = SystemAwake.asleep();
        let second = SystemAwake.asleep();
        assert!(second + Duration::from_millis(1) >= first);
    }

    #[test]
    fn a_sleep_starts_a_pause_that_ends_a_while_after_the_wake() {
        let awake = Arc::new(FakeAwake::default());
        let pause = Pause::new(awake.clone(), Duration::from_millis(200));
        let stop = AtomicBool::new(false);
        // Awake: no pause.
        assert_eq!(pause.hold(&stop), None);
        assert!(!pause.on());
        // The clocks' drift is no sleep.
        awake.sleep(Duration::from_millis(100));
        assert_eq!(pause.look(), None);
        // A sleep of an hour: the pause starts at the first look after it.
        awake.sleep(Duration::from_secs(3600));
        let started = Instant::now();
        let slept = pause.hold(&stop).expect("the sleep was noticed");
        assert_eq!(slept.asleep, Duration::from_secs(3600));
        assert!(started.elapsed() >= Duration::from_millis(200));
        assert!(!pause.on());
        // Noticed once only.
        assert_eq!(pause.hold(&stop), None);
    }

    #[test]
    fn everyone_who_looks_during_a_pause_waits_for_it_and_a_stop_ends_the_wait() {
        let awake = Arc::new(FakeAwake::default());
        let pause = Arc::new(Pause::new(awake.clone(), Duration::from_millis(300)));
        awake.sleep(Duration::from_secs(60));
        assert!(pause.look().is_some());
        assert!(pause.on());
        // Another thread that looks now waits too, without noticing the
        // sleep again.
        let other = {
            let pause = pause.clone();
            std::thread::spawn(move || {
                let started = Instant::now();
                let slept = pause.hold(&AtomicBool::new(false));
                (slept, started.elapsed())
            })
        };
        let (slept, waited) = other.join().unwrap();
        assert_eq!(slept, None);
        assert!(waited >= Duration::from_millis(200), "{waited:?}");
        // A stop ends a wait at once.
        awake.sleep(Duration::from_secs(60));
        let started = Instant::now();
        assert!(pause.hold(&AtomicBool::new(true)).is_some());
        assert!(started.elapsed() < Duration::from_millis(200));
    }
}
