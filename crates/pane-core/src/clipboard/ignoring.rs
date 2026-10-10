//! The ignore window a simulated copy arms while it reads a selection
//! (#262): the target's copy is not Pane's and carries no marker Pane
//! could give it, so no change of the window is reported to any history
//! while one is armed, and the restore that follows is tagged but covered
//! the same. The selected-text read arms it (`system::selected`) around
//! the copy and the restore; the Windows listener asks it before reporting
//! a change. The rules are compiled and tested on every system, as the
//! pause and the waking of the selected-text read are; only the armer and
//! the asker are Windows'.
//!
//! The arm and the disarm are plain atomics, holding no lock the listener
//! takes, so the read that arms the window and the listener that asks it
//! can never wait on each other.

// The Windows read and listener are this module's only users; the window
// itself is tested on every system.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// The ignore window itself.
pub(crate) struct Ignoring {
    /// Whether changes are being ignored.
    armed: AtomicBool,
    /// When this arm ends even if it is never disarmed, in milliseconds
    /// since the Unix epoch: a read stuck between the arming and the
    /// disarming must not silence history for good.
    until: AtomicU64,
}

/// How long an ignore window lasts at most, even if it is never disarmed:
/// far past the bounds of a simulated copy, so a healthy one is always
/// disarmed long before, and a read stuck in a call that never returns
/// cannot keep history silent for good.
const IGNORED_FOR: Duration = Duration::from_secs(30);

impl Ignoring {
    /// An ignore window that is not armed.
    pub(crate) const fn off() -> Ignoring {
        Ignoring {
            armed: AtomicBool::new(false),
            until: AtomicU64::new(0),
        }
    }

    /// Arms the window at `now` (milliseconds since the Unix epoch),
    /// answering the disarm that ends it when dropped.
    pub(crate) fn arm(&self, now: u64) -> Disarm<'_> {
        self.armed.store(true, Ordering::SeqCst);
        self.until.store(
            now.saturating_add(IGNORED_FOR.as_millis() as u64),
            Ordering::SeqCst,
        );
        Disarm(self)
    }

    /// Runs `work` with the window armed, disarming it however `work`
    /// ends: the copy, and the restore that follows, happen inside.
    pub(crate) fn around<T>(&self, work: impl FnOnce() -> T) -> T {
        let disarm = self.arm(crate::util::now_ms());
        let answer = work();
        drop(disarm);
        answer
    }

    /// Whether a change of the clipboard at `now` (milliseconds since
    /// the Unix epoch) is ignored.
    pub(crate) fn is_ignoring(&self, now: u64) -> bool {
        self.armed.load(Ordering::SeqCst) && now < self.until.load(Ordering::SeqCst)
    }
}

/// The disarm of an armed [`Ignoring`] window, run when dropped: however
/// the work it was armed for ends.
pub(crate) struct Disarm<'a>(&'a Ignoring);

impl Drop for Disarm<'_> {
    fn drop(&mut self) {
        self.0.armed.store(false, Ordering::SeqCst);
    }
}

/// The ignore window of the selected-text read (#262): the one the
/// Windows listener asks before reporting a change.
pub(crate) static IGNORED: Ignoring = Ignoring::off();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ignored_window_covers_only_its_arm() {
        let ignored = Ignoring::off();
        assert!(!ignored.is_ignoring(0));
        {
            let _armed = ignored.arm(1_000);
            assert!(ignored.is_ignoring(2_000));
            // The window ends by itself if its disarm never comes, far
            // past the bounds of a simulated copy.
            let at_most = IGNORED_FOR.as_millis() as u64;
            assert!(ignored.is_ignoring(1_000 + at_most - 1));
            assert!(!ignored.is_ignoring(1_000 + at_most));
        }
        // The disarm ran when the arm was dropped, however the work it
        // was armed for ended.
        assert!(!ignored.is_ignoring(2_000));
        // A later arm is a window of its own.
        let _again = ignored.arm(2_000);
        assert!(ignored.is_ignoring(2_001));
    }

    #[test]
    fn work_runs_inside_an_ignored_window_and_disarms_it_after() {
        let ignored = Ignoring::off();
        let answer = ignored.around(|| {
            assert!(ignored.is_ignoring(crate::util::now_ms()));
            "the selection"
        });
        assert_eq!(answer, "the selection");
        assert!(!ignored.is_ignoring(crate::util::now_ms()));
    }
}
