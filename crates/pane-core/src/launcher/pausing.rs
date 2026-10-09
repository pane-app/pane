//! Pausing an installed package that keeps failing (#16), so one broken
//! extension does not fail again and again while the rest of Pane runs.
//!
//! Pane pauses a package when a failure is attributable to it: its code ran
//! in this package's current generation and failed on its own.
//!
//! - **It could not start**: a component could not be loaded or
//!   instantiated, or its reloaded code trapped as it started (a startup
//!   failure). Paused at once: starting it again would fail the same way.
//! - **It crashed [`CRASHES_BEFORE_PAUSE`] times within
//!   [`CRASH_WINDOW`]**: a guest call trapped, whether the user ran it
//!   (opening a command, an action, a form, a view event or closing a view),
//!   root search asked it for results, or another package called its
//!   operation. Calls that answer in between do not start the count again
//!   (opening a command before each crashing action answers); crashes
//!   further apart than the window, and those before Pane started or the
//!   package's generation began, are not counted together.
//! - **Its call stopped responding** (#18, an unresponsive call): a guest
//!   call computed for the runtime's compute limit without finishing and
//!   was stopped. Only the guest's own computing counts, never Pane's host
//!   calls or waiting, and Wasmtime was running that package's code, so
//!   the failure is its own; it counts as a crash does, in the same window
//!   (provisional). Starting a package is never counted: a slow start is
//!   not a failure to start.
//!
//! What is not a failure of the package: an error the extension answers
//! with ("sign in first"), which is an ordinary outcome; a call stopped
//! because its generation, or a caller's in its chain, ended (disable,
//! reload, update, uninstall), even though its instance restarts afresh
//! afterwards as after a crash; and Pane's own runtime being unavailable. A
//! crash of a package serving an operation is its own, not its caller's. An
//! unattributed failure of the runtime itself (#17) pauses nothing.
//!
//! A paused package's generation ends, which stops its pending calls and
//! drops its instances; its commands stay listed in root search, saying why
//! they do not run, and it computes no root results and serves no
//! operations. Its settings and saved data are kept. The pause is recorded
//! with the package's version and managed copy, so it holds after a restart
//! for that code. It ends with Retry (which starts the same code again),
//! a reload or an update (new code), disabling or enabling the package (it
//! starts afresh) or uninstalling it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use super::{Launcher, State, Status, owner};
use crate::extension_data::PackageData;
use crate::extension_log::LogLevel;
use crate::generation::End;
use crate::packages::{PackageError, PackageIdentity, Pause, PauseCause, Store};
use crate::runtime::{CallError, Health};

/// How many crashes within [`CRASH_WINDOW`] pause a package. Small, so
/// that a broken command stops failing soon, and more than one, so that one
/// bad input does not stop an extension that otherwise works.
const CRASHES_BEFORE_PAUSE: usize = 3;

// How close together crashes count towards pausing a package: long enough
// to catch a user trying a broken command again, or root search asking a
// broken provider on each key, and short enough that rare crashes of a
// long-running Pane never add up to a pause. Shared with the runtime's
// restart policy.
use crate::runtime::CRASH_WINDOW;

/// The installed packages Pane paused, each with why, and when and how
/// each package's current generation failed within [`CRASH_WINDOW`].
#[derive(Default)]
pub(super) struct Pauses {
    pauses: HashMap<PackageIdentity, Pause>,
    failures: HashMap<PackageIdentity, Vec<(Instant, Failing)>>,
}

/// How a package's call failed, towards pausing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failing {
    /// It trapped.
    Crash,
    /// It computed for too long without finishing (an unresponsive call).
    UnresponsiveCall,
}

impl Failing {
    /// How a call into an installed package failed, towards pausing it,
    /// and its error; `Err` for a failure to start, which pauses at once.
    fn of(health: Health) -> (Failing, Result<CallError, CallError>) {
        match health {
            Health::Crashed(error) => (Failing::Crash, Ok(error)),
            Health::Unresponsive(error) => (Failing::UnresponsiveCall, Ok(error)),
            Health::FailedToStart(error) => (Failing::Crash, Err(error)),
        }
    }
}

impl PauseCause {
    /// Why a package that failed as `failures` did is paused.
    fn of(failures: &[(Instant, Failing)]) -> PauseCause {
        let unresponsive = failures
            .iter()
            .filter(|(_, how)| *how == Failing::UnresponsiveCall)
            .count();
        match unresponsive {
            0 => PauseCause::Crashes,
            all if all == failures.len() => PauseCause::UnresponsiveCalls,
            _ => PauseCause::CrashesAndUnresponsiveCalls,
        }
    }

    /// How the paused package titled `title` failed, in a sentence:
    /// "<title> crashed 3 times within 5 minutes", or "<title> could not
    /// start".
    pub(super) fn failure(self, title: &str) -> String {
        match self {
            PauseCause::FailedToStart => format!("{title} could not start"),
            _ => format!("{title} {} {}", self.failed_how().to_lowercase(), within()),
        }
    }

    /// "Crashed", "Stopped responding" or "Crashed or stopped responding":
    /// how a package paused after failing too often failed.
    fn failed_how(self) -> &'static str {
        match self {
            PauseCause::UnresponsiveCalls => "Stopped responding",
            PauseCause::CrashesAndUnresponsiveCalls => "Crashed or stopped responding",
            PauseCause::Crashes | PauseCause::FailedToStart => "Crashed",
        }
    }

    /// The state of an enabled package paused for this, in the extension
    /// list: "Enabled · Paused after crashing".
    pub(super) fn state(self) -> &'static str {
        match self {
            PauseCause::FailedToStart => "Enabled · Failed to start",
            PauseCause::Crashes => "Enabled · Paused after crashing",
            PauseCause::UnresponsiveCalls => "Enabled · Paused after not responding",
            PauseCause::CrashesAndUnresponsiveCalls => {
                "Enabled · Paused after crashing or not responding"
            }
        }
    }

    /// The title of the row that retries the paused package titled
    /// `title`.
    pub(super) fn retry_title(self, title: &str) -> String {
        match self {
            PauseCause::FailedToStart => format!("Retry starting {title}"),
            _ => format!("Retry {title}"),
        }
    }
}

impl Pauses {
    /// Why the package with `identity` is paused, if it is.
    pub(super) fn of(&self, identity: &PackageIdentity) -> Option<&Pause> {
        self.pauses.get(identity)
    }

    pub(super) fn is_paused(&self, identity: &PackageIdentity) -> bool {
        self.pauses.contains_key(identity)
    }

    /// Forgets the pause and failures of the package with `identity`,
    /// returning its pause: it runs in a new generation, or not at all.
    pub(super) fn forget(&mut self, identity: &PackageIdentity) -> Option<Pause> {
        self.failures.remove(identity);
        self.pauses.remove(identity)
    }

    /// Notes that the package with `identity` is paused for `pause`, as
    /// recorded before Pane last stopped or before an uninstall that could
    /// not be recorded.
    pub(super) fn restore(&mut self, identity: PackageIdentity, pause: Pause) {
        self.pauses.insert(identity, pause);
    }

    /// Notes that the package with `identity` failed (`how`) at `now`, and
    /// returns why it is paused if that is its [`CRASHES_BEFORE_PAUSE`]th
    /// failure within [`CRASH_WINDOW`]: after crashes, after not
    /// responding, or after both.
    fn failed(
        &mut self,
        identity: &PackageIdentity,
        now: Instant,
        how: Failing,
    ) -> Option<PauseCause> {
        let failures = self.failures.entry(identity.clone()).or_default();
        failures.retain(|(at, _)| now.saturating_duration_since(*at) < CRASH_WINDOW);
        failures.push((now, how));
        (failures.len() >= CRASHES_BEFORE_PAUSE).then(|| PauseCause::of(failures))
    }

    fn pause(&mut self, identity: PackageIdentity, pause: Pause) {
        self.failures.remove(&identity);
        self.pauses.insert(identity, pause);
    }
}

/// Writes whether packages are paused to `installed.json`, one after
/// another on a thread of its own, so neither the runtime thread nor the
/// window waits for the file. Each record carries the package's state when
/// it was queued, and they are written in that order, so the last one
/// written is the latest.
#[derive(Clone)]
pub(super) struct Recorder(mpsc::Sender<Record>);

enum Record {
    Pause(PackageIdentity, Option<Pause>),
    /// Answered once the records queued before it are written.
    Written(tokio::sync::oneshot::Sender<()>),
}

impl Recorder {
    /// Starts the thread writing to `store`. It stops once every recorder
    /// is dropped.
    pub(super) fn start(store: Arc<Mutex<Store>>) -> Recorder {
        let (records, queue) = mpsc::channel();
        std::thread::Builder::new()
            .name("pane-pause-records".into())
            .spawn(move || {
                for record in queue {
                    match record {
                        Record::Pause(identity, pause) => {
                            let mut store = store.lock().unwrap_or_else(|p| p.into_inner());
                            match store.set_paused(&identity, pause) {
                                // Uninstalled meanwhile: nothing to record.
                                Ok(()) | Err(PackageError::NotInstalled(_)) => {}
                                Err(error) => eprintln!(
                                    "pane: could not record whether {identity} is paused: {error}"
                                ),
                            }
                        }
                        Record::Written(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })
            .expect("starting the thread recording paused extensions");
        Recorder(records)
    }

    fn record(&self, identity: &PackageIdentity, pause: Option<Pause>) {
        // The thread lives as long as this sender.
        let _ = self.0.send(Record::Pause(identity.clone(), pause));
    }

    /// Resolves once the records queued so far are written.
    pub(super) fn written(&self) -> impl Future<Output = ()> + Send + 'static {
        let (done, written) = tokio::sync::oneshot::channel();
        let _ = self.0.send(Record::Written(done));
        async move {
            let _ = written.await;
        }
    }
}

/// The title of the row, and of the screen, that shows why the package
/// titled `title` is paused.
pub(super) fn details_title(title: &str) -> String {
    format!("Why {title} is paused")
}

/// "3 times within 5 minutes": how often a package crashes before Pane
/// pauses it.
pub(super) fn within() -> String {
    format!(
        "{CRASHES_BEFORE_PAUSE} times within {} minutes",
        CRASH_WINDOW.as_secs() / 60
    )
}

impl Launcher {
    /// Notes how a call into `component`, made with `data`, failed, pausing
    /// its package if the failure is its [`CRASHES_BEFORE_PAUSE`]th crash
    /// within [`CRASH_WINDOW`] or a failure to start (see the module
    /// documentation). Called on the runtime thread before the call's answer
    /// is sent, so whoever awaits the answer sees the pause.
    pub(super) fn note_health(&self, component: &Path, data: &PackageData, health: Health) {
        let mut state = self.lock();
        // Checked with the state locked: disabling, reloading, updating or
        // uninstalling the package ends its generation with the state
        // locked, so a failure of code stopped since is never counted.
        if data.stopped().is_some() {
            return;
        }
        let Some(package) = owner(&state.packages, component).filter(|p| p.enabled) else {
            return;
        };
        let identity = package.identity.clone();
        let version = package.version();
        let title = package.title();
        let (how, error) = Failing::of(health);
        let pause = match error {
            Ok(error) => {
                let Some(after) = state.paused.failed(&identity, Instant::now(), how) else {
                    return;
                };
                Pause {
                    after,
                    why: format!(
                        "{} {}; the last time: {error}",
                        after.failed_how(),
                        within()
                    ),
                    version,
                }
            }
            Err(error) => Pause {
                after: PauseCause::FailedToStart,
                why: error.to_string(),
                version,
            },
        };
        let what = pause.after.failure(&title);
        self.pause(&mut state, &identity, pause);
        state.view.status = Status::Error(format!(
            "{what} and is paused: Pane runs none of its code until you retry it, and keeps its \
             saved data. Retry it, or see why, in Settings."
        ));
    }

    /// Pauses the package with `identity` for `pause` and queues the record
    /// of it: its generation ends, which stops its calls and drops its
    /// instances, and a command of it that is open closes.
    pub(super) fn pause(&self, state: &mut State, identity: &PackageIdentity, pause: Pause) {
        self.developing.logs.pane(
            &identity.key(),
            0,
            LogLevel::Warn,
            &format!("Pane paused the extension: {}", pause.why),
        );
        if let Some(installation) = &self.installation {
            installation.data.pause(identity);
            installation.records.record(identity, Some(pause.clone()));
        }
        state.paused.pause(identity.clone(), pause);
        // Its dependents now wait for it, rather than fail (see `waiting`).
        state.recheck_waiting();
        // Its results kept for root search go.
        Launcher::forget_indexes(state);
        let components: Vec<PathBuf> = state
            .package(identity)
            .map(|package| {
                package
                    .commands()
                    .into_iter()
                    .map(|command| command.component)
                    .collect()
            })
            .unwrap_or_default();
        if state
            .open
            .as_ref()
            .is_some_and(|open| components.contains(open))
        {
            self.show_root(state, None);
        } else {
            self.refresh(state);
        }
    }

    /// Makes the pauses the launcher lists agree with the packages whose
    /// code is stopped as paused, after a thread panicked while holding the
    /// state, perhaps halfway through [`Launcher::pause`]: a package whose
    /// code was stopped but not yet listed is listed as paused (with Retry),
    /// and one listed whose code still runs is stopped. Its record is
    /// written again either way.
    pub(super) fn reconcile_pauses(&self, state: &mut State) {
        let Some(installation) = &self.installation else {
            return;
        };
        for package in state.packages.clone() {
            let identity = &package.identity;
            let stopped = installation.data.owned_by(identity).stopped() == Some(End::Paused);
            match (stopped, state.paused.of(identity).cloned()) {
                (true, None) => {
                    let pause = Pause {
                        after: PauseCause::Crashes,
                        why: "Pane was interrupted while it paused this extension after an \
                              error; retry it to start it again"
                            .into(),
                        version: package.version(),
                    };
                    installation.records.record(identity, Some(pause.clone()));
                    state.paused.pause(identity.clone(), pause);
                }
                (false, Some(pause)) if package.enabled => {
                    installation.data.pause(identity);
                    installation.records.record(identity, Some(pause));
                }
                _ => {}
            }
        }
    }

    /// Runs the package with `identity` again, which Pane may have paused,
    /// and queues the record of it: a new generation, with no crashes
    /// counted. Returns the pause it ends, if any.
    pub(super) fn unpause(&self, state: &mut State, identity: &PackageIdentity) -> Option<Pause> {
        let pause = state.paused.forget(identity);
        if let Some(installation) = &self.installation {
            installation.data.resume(identity);
            installation.records.record(identity, None);
        }
        // Its dependents come back from waiting for it (see `waiting`).
        state.recheck_waiting();
        pause
    }

    /// Resolves once Pane has written its records of which extensions are
    /// paused, as they are now, so that a restart finds them. They are
    /// written in the background as packages are paused and retried.
    pub fn records_written(&self) -> impl Future<Output = ()> + Send + 'static {
        let written = self
            .installation
            .as_ref()
            .map(|installation| installation.records.written());
        async move {
            if let Some(written) = written {
                written.await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn identity(dir: &tempfile::TempDir) -> PackageIdentity {
        PackageIdentity::local(dir.path()).unwrap()
    }

    /// Whether a crash of the package with `identity` at `now` pauses it.
    fn crashes(pauses: &mut Pauses, identity: &PackageIdentity, now: Instant) -> bool {
        pauses.failed(identity, now, Failing::Crash).is_some()
    }

    #[test]
    fn three_crashes_within_the_window_pause() {
        let dir = tempfile::tempdir().unwrap();
        let identity = identity(&dir);
        let mut pauses = Pauses::default();
        let start = Instant::now();
        let minute = Duration::from_secs(60);
        assert!(!crashes(&mut pauses, &identity, start));
        assert!(!crashes(&mut pauses, &identity, start + minute));
        assert!(crashes(&mut pauses, &identity, start + 4 * minute));
    }

    #[test]
    fn not_responding_counts_as_a_crash_does_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let identity = identity(&dir);
        let now = Instant::now();
        let mut pauses = Pauses::default();
        assert_eq!(
            pauses.failed(&identity, now, Failing::UnresponsiveCall),
            None
        );
        assert_eq!(
            pauses.failed(&identity, now, Failing::UnresponsiveCall),
            None
        );
        assert_eq!(
            pauses.failed(&identity, now, Failing::UnresponsiveCall),
            Some(PauseCause::UnresponsiveCalls)
        );

        let mut pauses = Pauses::default();
        assert_eq!(pauses.failed(&identity, now, Failing::Crash), None);
        assert_eq!(
            pauses.failed(&identity, now, Failing::UnresponsiveCall),
            None
        );
        assert_eq!(
            pauses.failed(&identity, now, Failing::Crash),
            Some(PauseCause::CrashesAndUnresponsiveCalls)
        );
        assert_eq!(
            PauseCause::CrashesAndUnresponsiveCalls.failure("Sample"),
            "Sample crashed or stopped responding 3 times within 5 minutes"
        );
        assert_eq!(
            PauseCause::Crashes.failure("Sample"),
            "Sample crashed 3 times within 5 minutes"
        );
        assert_eq!(
            PauseCause::FailedToStart.failure("Sample"),
            "Sample could not start"
        );
    }

    #[test]
    fn crashes_spread_beyond_the_window_never_pause() {
        let dir = tempfile::tempdir().unwrap();
        let identity = identity(&dir);
        let mut pauses = Pauses::default();
        let start = Instant::now();
        let minute = Duration::from_secs(60);
        // Never three within five minutes of one another.
        for crash in 0..10 {
            assert!(
                !crashes(&mut pauses, &identity, start + crash * 3 * minute),
                "crash {crash}"
            );
        }
        // Exactly the window apart does not count together either.
        let later = start + 60 * minute;
        assert!(!crashes(&mut pauses, &identity, later));
        assert!(!crashes(
            &mut pauses,
            &identity,
            later + CRASH_WINDOW - minute
        ));
        assert!(!crashes(&mut pauses, &identity, later + CRASH_WINDOW));
    }

    /// A launcher without a runtime with one package installed, its
    /// identity and its command's component.
    fn installed() -> (tempfile::TempDir, Launcher, PackageIdentity, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("pane.json"),
            r#"{ "manifestVersion": 1, "title": "Broken", "apiVersion": "0.1",
                "commands": [{ "id": "open", "title": "Open", "component": "c.wasm" }] }"#,
        )
        .unwrap();
        std::fs::write(source.join("c.wasm"), b"").unwrap();
        let packages = dir.path().join("extensions");
        let package = crate::packages::SourcePackage::read(&source).unwrap();
        let installed = Store::open(packages.clone()).install(&package).unwrap();
        let unavailable = crate::runtime::CallError::RuntimeUnavailable("none".into());
        let launcher = Launcher::with_packages(Err(unavailable), vec![], packages);
        let component = installed.commands()[0].component.clone();
        (dir, launcher, installed.identity, component)
    }

    fn crash(launcher: &Launcher, component: &Path, data: &PackageData) {
        let trap = crate::runtime::CallError::Trap("trapped".into());
        launcher.note_health(component, data, Health::Crashed(trap));
    }

    #[test]
    fn crashes_of_code_disabled_meanwhile_never_pause_it() {
        let (_dir, launcher, identity, component) = installed();
        let installation = launcher.installation.clone().unwrap();
        // The calls were asked for before the package was disabled, and
        // report their crashes after.
        let data = installation.data.owned_by(&identity);
        futures::executor::block_on(launcher.set_enabled(&identity, false));
        for _ in 0..CRASHES_BEFORE_PAUSE {
            crash(&launcher, &component, &data);
        }
        futures::executor::block_on(launcher.set_enabled(&identity, true));
        assert!(!launcher.lock().paused.is_paused(&identity));

        // The same crashes of its current code pause it.
        let data = installation.data.owned_by(&identity);
        for _ in 0..CRASHES_BEFORE_PAUSE {
            crash(&launcher, &component, &data);
        }
        assert!(launcher.lock().paused.is_paused(&identity));
    }

    #[test]
    fn crashes_of_one_package_do_not_count_for_another() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut pauses = Pauses::default();
        let now = Instant::now();
        assert!(!crashes(&mut pauses, &identity(&a), now));
        assert!(!crashes(&mut pauses, &identity(&a), now));
        assert!(!crashes(&mut pauses, &identity(&b), now));
        assert!(crashes(&mut pauses, &identity(&a), now));
    }
}
