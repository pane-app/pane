//! The text selected in the application that was in front before Pane
//! (#262, the host function #120 declared and the Windows power features
//! implement, #125): the pure rules the read decides with, compiled and
//! tested on every system. They say how the read is asked for and
//! bounded — the worker process's timeout, the failure pause, the waking
//! of a Chromium-based application's accessibility tree and the retry
//! that follows — and how the worker is spoken to (the one-line JSON
//! request and answer the process protocol carries). The Windows half —
//! the worker process of Pane's own program, the UI Automation read, the
//! simulated copy and the clipboard's ignore window
//! (`crate::clipboard::ignoring`) — is in `windows` beside this, as the
//! front application's watcher is in `front`.

// The Windows adapter is this module's only user; the rules are tested on
// every system, but nothing else reaches them.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use windows::serve;

#[cfg(target_os = "windows")]
pub(in crate::system) use windows::Selected;

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The argument that starts Pane's program as the selected-text worker
/// (#262): Pane's own, unlisted in its interface and never parsed as the
/// user's, checked in `main` before anything else so the worker starts no
/// window, no runtime and no settings — only the reads it is asked for.
pub const WORKER_ARGUMENT: &str = "--pane-selected-text";

/// How long a read waits for the worker's answer before giving up on it
/// (proposed 2.5 s, #125): the worker is replaced with a fresh one, and
/// the read falls back to a simulated copy. The wait bounds everything
/// the worker does, however slow the application in front is.
pub(crate) const WORKER_WAIT: Duration = Duration::from_millis(2500);

/// How many failures within [`FAILURES_WITHIN`] pause the UI Automation
/// reads: while paused, each read goes straight to the simulated copy.
pub(crate) const FAILURES_TO_PAUSE: usize = 3;

/// The window within which [`FAILURES_TO_PAUSE`] failures pause the reads
/// (proposed ten minutes, #125).
pub(crate) const FAILURES_WITHIN: Duration = Duration::from_secs(10 * 60);

/// How long that pause lasts (proposed ten minutes, #125): an application
/// whose accessibility never answers stops being asked for a while,
/// rather than costing every read its timeout.
pub(crate) const PAUSED_FOR: Duration = Duration::from_secs(10 * 60);

/// How long a read that found nothing waits before it is tried again once
/// (proposed "shortly after", #125): a tree just woken needs the time to
/// build.
pub(crate) const RETRY_AFTER: Duration = Duration::from_millis(200);

/// The failure state of the UI Automation reads (#125): three failures
/// within [`FAILURES_WITHIN`] pause the reads for [`PAUSED_FOR`], so an
/// application whose accessibility never answers stops costing every read
/// a timeout while the simulated copy carries them. A failure is the read
/// itself failing (a worker that timed out, ended or answered an error);
/// "nothing is selected" and "UI Automation gives nothing" are answers,
/// not failures. The record is in Pane's memory, for as long as it runs.
#[derive(Debug, Default)]
pub(crate) struct Pause {
    /// When each failure was noted, in milliseconds since the Unix epoch,
    /// the ones within [`FAILURES_WITHIN`] of the latest kept.
    failures: Vec<u64>,
    /// When the pause ends, in the same counting: 0 before one starts.
    until: u64,
}

impl Pause {
    /// Notes a failure at `now` (milliseconds since the Unix epoch),
    /// pausing the reads if this is the third within ten minutes.
    pub(crate) fn noted(&mut self, now: u64) {
        let within = ms(FAILURES_WITHIN);
        self.failures.retain(|at| at.saturating_add(within) > now);
        self.failures.push(now);
        let trailing = self
            .failures
            .iter()
            .filter(|at| **at > now.saturating_sub(within))
            .count();
        if trailing >= FAILURES_TO_PAUSE {
            self.until = now.saturating_add(ms(PAUSED_FOR));
        }
    }

    /// Whether the reads are paused at `now` (milliseconds since the Unix
    /// epoch).
    pub(crate) fn paused(&self, now: u64) -> bool {
        now < self.until
    }
}

/// The windows whose accessibility trees this worker has already been
/// given first contact with: a Chromium-based application builds its tree
/// only once something asks the window for its accessibility object
/// (`WM_GETOBJECT`), so the first read of a window wakes the tree before
/// it reads, and the read that then finds nothing is tried again shortly
/// after, once the tree has had the time to build.
#[derive(Debug, Default)]
pub(crate) struct Waking {
    /// The windows already woken, as addresses.
    woken: std::collections::HashSet<usize>,
}

impl Waking {
    /// Whether `window` is met for the first time, and so its tree is to
    /// be woken before it is read.
    pub(crate) fn first_contact(&mut self, window: usize) -> bool {
        self.woken.insert(window)
    }
}

/// How long a read that found nothing waits before it is tried again once
/// more: a tree just woken needs the time to build. `tried` is how many
/// reads have run (the first is 1); `None` ends the trying, so there is
/// one retry, shortly after the first.
pub(crate) fn retry_after(tried: u32) -> Option<Duration> {
    (tried == 1).then_some(RETRY_AFTER)
}

/// What the worker answered about the selection it was asked to read: one
/// line of JSON, so a selection's newlines and quotes travel intact. The
/// kinds the read distinguishes: text, nothing selected (an answer, not a
/// failure), UI Automation giving nothing (the fallback, a simulated
/// copy, is next), and a failure of the read itself, which counts toward
/// the pause.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "kebab-case")]
pub(crate) enum Answer {
    /// The text selected.
    Text {
        /// The selection; an empty one is no selection, and the reader
        /// treats it as [`Answer::Nothing`].
        text: String,
    },
    /// Nothing is selected, which is not a failure: a command is told so,
    /// rather than given an empty string silently.
    Nothing,
    /// UI Automation gives nothing of the selection — no text pattern on
    /// the focused element, even after its tree was woken and the read
    /// tried again. A simulated copy is the fallback.
    GivesNothing,
    /// The read itself failed, which counts toward the pause: why.
    Failed { why: String },
}

impl Answer {
    /// The answer as the one line the worker writes, with no newline of
    /// its own (a selection's newlines are escaped by the JSON).
    pub(crate) fn line(&self) -> String {
        serde_json::to_string(self).expect("an answer is always one line of JSON")
    }

    /// The answer a worker's line carries, or why the line is not one.
    pub(crate) fn parse(line: &str) -> Result<Answer, String> {
        serde_json::from_str(line.trim())
            .map_err(|error| format!("the worker's answer could not be read: {error}"))
    }
}

/// The request line asking for the selection of `window`, as an address:
/// one line of JSON, with no newline of its own.
pub(crate) fn request_line(window: usize) -> String {
    #[derive(Serialize)]
    struct Request {
        window: usize,
    }
    serde_json::to_string(&Request { window }).expect("a request is always one line of JSON")
}

/// The window a request line names, or why the line is not a request.
pub(crate) fn requested(line: &str) -> Result<usize, String> {
    #[derive(Deserialize)]
    struct Request {
        window: usize,
    }
    serde_json::from_str(line.trim())
        .map_err(|error| format!("the worker's request could not be read: {error}"))
        .map(|request: Request| request.window)
}

/// Why asking the worker for a read did not answer: both replace the
/// worker (a fresh one is started by the next ask) and both count toward
/// the pause.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AskError {
    /// The worker did not answer within [`WORKER_WAIT`]: the application
    /// in front is slow, and the worker is ended and replaced.
    TimedOut,
    /// The worker ended, or could not be started or read: its process, or
    /// the line it wrote, is gone.
    Ended,
}

/// How a UI Automation read is asked for, always bounded by
/// [`WORKER_WAIT`]: through Pane's worker process (the default, `windows`
/// beside this), or in this process on a thread of its own — the
/// real-input adapter test's, which cannot start Pane's program as its
/// worker.
pub(crate) trait Asking: Send + Sync + 'static {
    /// Asks for the selection of `window`, as an address.
    fn ask(&self, window: usize) -> Result<Answer, AskError>;
}

/// An asker that runs `read` on a thread of this process, waiting `wait`
/// at most: one that takes longer is abandoned where it is, and the next
/// ask runs on a fresh thread — the worker is replaced without a process.
/// The real-input adapter test reads through this (the UI Automation read
/// itself); the pure tests hang one deliberately, to check the bounding
/// and the replacement.
pub(crate) struct Reading {
    read: Arc<dyn Fn(usize) -> Answer + Send + Sync>,
    wait: Duration,
}

impl Reading {
    /// Reads through `read`, waiting [`WORKER_WAIT`] for each answer.
    pub(crate) fn new(read: Arc<dyn Fn(usize) -> Answer + Send + Sync>) -> Reading {
        Reading {
            read,
            wait: WORKER_WAIT,
        }
    }
}

impl Asking for Reading {
    fn ask(&self, window: usize) -> Result<Answer, AskError> {
        let read = self.read.clone();
        let (answer, answered) = std::sync::mpsc::channel();
        let asking = std::thread::Builder::new()
            .name("pane-selected-worker".into())
            .spawn(move || {
                let _ = answer.send(read(window));
            });
        if asking.is_err() {
            return Err(AskError::Ended);
        }
        answered
            .recv_timeout(self.wait)
            .map_err(|_| AskError::TimedOut)
    }
}

/// `duration` as whole milliseconds, saturating at `u64::MAX`.
fn ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A minute, in the milliseconds the pause counts.
    const MINUTE: u64 = 60 * 1000;

    #[test]
    fn three_failures_within_ten_minutes_pause_the_reads_for_ten() {
        let mut pause = Pause::default();
        pause.noted(100);
        assert!(!pause.paused(100), "one failure is not three");
        pause.noted(200);
        assert!(!pause.paused(200), "two failures are not three");
        pause.noted(300);
        // The pause starts at the third failure and lasts ten minutes.
        assert!(pause.paused(300));
        assert!(pause.paused(600_299));
        assert!(!pause.paused(600_300));
        // A failure that is the third within the window restarts the
        // pause, from itself.
        pause.noted(700);
        assert!(pause.paused(700));
        assert!(pause.paused(600_699));
        assert!(!pause.paused(600_700));
    }

    #[test]
    fn failures_ten_minutes_apart_never_pause_the_reads() {
        let mut pause = Pause::default();
        for at in [0, 11 * MINUTE, 22 * MINUTE, 33 * MINUTE] {
            pause.noted(at);
            assert!(!pause.paused(at), "a failure at {at}");
        }
        // The third within the window is what counts, not the third ever:
        // two close together and one eleven minutes later do not pause.
        let mut spaced = Pause::default();
        spaced.noted(0);
        spaced.noted(1);
        spaced.noted(11 * MINUTE);
        assert!(!spaced.paused(11 * MINUTE));
    }

    #[test]
    fn a_pause_ends_and_old_failures_do_not_reach_the_next_one() {
        let mut pause = Pause::default();
        pause.noted(0);
        pause.noted(1);
        pause.noted(2);
        // The pause ends; the failures that made it are ten minutes old
        // by then, so a new one needs two more failures beside it.
        assert!(!pause.paused(30 * MINUTE));
        pause.noted(30 * MINUTE);
        pause.noted(30 * MINUTE + 1);
        assert!(!pause.paused(30 * MINUTE + 1));
        pause.noted(30 * MINUTE + 2);
        assert!(pause.paused(30 * MINUTE + 2));
    }

    #[test]
    fn the_first_contact_with_a_window_wakes_it_once() {
        let mut waking = Waking::default();
        assert!(waking.first_contact(197_236));
        // However often the worker reads the same window, it is woken
        // once: the tree, once built, stays built.
        assert!(!waking.first_contact(197_236));
        assert!(waking.first_contact(197_238));
        // A window whose address is reused counts as a new one: only the
        // wake is skipped, never the read.
        let mut other = Waking::default();
        assert!(other.first_contact(197_236));
    }

    #[test]
    fn a_read_that_finds_nothing_is_tried_again_once_shortly_after() {
        assert_eq!(retry_after(1), Some(RETRY_AFTER));
        assert_eq!(retry_after(2), None);
        assert_eq!(retry_after(3), None);
    }

    #[test]
    fn the_worker_s_protocol_carries_every_answer_as_one_line() {
        for answer in [
            Answer::Text {
                text: "a selection\nwith newlines and “quotes”".into(),
            },
            Answer::Text {
                text: String::new(),
            },
            Answer::Nothing,
            Answer::GivesNothing,
            Answer::Failed {
                why: "COM could not be started".into(),
            },
        ] {
            let line = answer.line();
            assert!(!line.contains('\n'), "{line}");
            assert_eq!(Answer::parse(&line), Ok(answer.clone()));
            // A line with a newline around it still carries it.
            assert_eq!(Answer::parse(&format!("{line}\r\n")), Ok(answer));
        }
        // What is not an answer says so, rather than guessing.
        assert!(Answer::parse("").is_err());
        assert!(Answer::parse("text: hello").is_err());
        assert!(Answer::parse("{\"answer\":\"other\"}").is_err());
    }

    #[test]
    fn the_worker_s_protocol_carries_the_window_it_is_asked_for() {
        let line = request_line(197_236);
        assert!(!line.contains('\n'), "{line}");
        assert_eq!(requested(&line), Ok(197_236));
        assert!(requested("197236").is_err());
        assert!(requested("").is_err());
        assert!(requested("{\"window\": \"an address\"}").is_err());
    }

    #[test]
    fn a_worker_that_takes_too_long_is_replaced_by_the_next_ask() {
        // The first read is slow past the wait; the ones after answer at
        // once, so the replaced worker's successor answers.
        let reads = AtomicUsize::new(0);
        let read = Arc::new(move |_window: usize| {
            if reads.fetch_add(1, Ordering::SeqCst) == 0 {
                std::thread::sleep(Duration::from_millis(300));
            }
            Answer::Nothing
        });
        let asking = Reading {
            read,
            wait: Duration::from_millis(50),
        };
        let started = std::time::Instant::now();
        assert!(matches!(asking.ask(197_236), Err(AskError::TimedOut)));
        assert!(started.elapsed() < Duration::from_millis(300));
        // The next ask does not wait for the abandoned read: it runs on a
        // thread of its own, at once.
        assert_eq!(asking.ask(197_236), Ok(Answer::Nothing));
    }

    #[test]
    fn a_read_that_answers_in_time_is_answered() {
        let asking = Reading {
            read: Arc::new(|_window: usize| Answer::Nothing),
            wait: Duration::from_secs(1),
        };
        assert_eq!(asking.ask(197_236), Ok(Answer::Nothing));
        // A read past the wait is timed out, however small the wait.
        let immediate = Reading {
            read: Arc::new(|_window: usize| {
                std::thread::sleep(Duration::from_millis(10));
                Answer::Nothing
            }),
            wait: Duration::from_nanos(1),
        };
        assert!(matches!(immediate.ask(197_236), Err(AskError::TimedOut)));
    }
}
