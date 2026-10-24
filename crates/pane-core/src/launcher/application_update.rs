//! Pane's own updates in the launcher (#54, ADR 0050): when Pane checks
//! for one, what it does with what it finds, and the rows and words that
//! tell the user (see `application_update`, where the check, the
//! download, the staging and the swap are, and `restart`, where the new
//! version is started).
//!
//! **When Pane checks.** At start, then every [`CHECK_EVERY`] while it
//! runs, each scheduled check a random moment of up to a sixteenth of
//! that later (fifteen minutes), so Panes sharing one address do not all
//! ask at once; and when the launcher is shown, if the last check is
//! older than [`CHECK_EVERY`]. The time of the last check is kept in
//! Pane's update record, in its cache folder, so a restart does not check
//! again before one is due. The schedule is the launcher's clock's
//! ([`Launcher::with_clock`]), so tests move it. A check the user asks for
//! — root search's Check for Pane Updates, or About's Check for updates —
//! checks at once, whatever the schedule.
//!
//! **What a check does.** A newer version it finds is downloaded at once,
//! in the background, checked against its sha256 and staged, with no row
//! and no word on the status line meanwhile (About shows the progress). A
//! background check or download that fails says nothing either: it is
//! written to Pane's log and the update record, About shows when and why,
//! and the next check that is due tries again. A check the user asked for
//! answers on the status line, even when there is nothing to tell.
//!
//! **Ready.** Once an update is staged, root search lists **Restart to
//! Update Pane** ([`RESTART_TITLE`]): first of all for a blank query, under
//! a section of its own, drawn with the accent, so it is selected when the
//! launcher opens and Enter restarts. A toast says "Pane <version> is
//! ready", with Restart as its action, once: when the update is staged
//! while the launcher is shown expanded, or else the next time it is. While
//! Pane sets up, installs, updates or removes an extension, the row is
//! unavailable and says why.
//!
//! **Restart.** Enter on the row, the toast's Restart or About's button
//! swaps the program, checks that the new one answers `--version` with its
//! version, starts it with `--updated-from` and `--wait-pid`, and waits for
//! it to report that it started (see `restart`); then Pane quits cleanly,
//! through the window ([`crate::feedback::WindowControl::quit`]). A new
//! version that does not answer, does not start or does not report in time
//! is ended, the swap is undone, the status line and About say what
//! happened, and the row stays to try again.
//!
//! **After the restart.** The new version, started with `--updated-from`
//! ([`Launcher::with_updated_from`]), says "Updated to Pane <version>" the
//! first time the launcher is shown. Once its window, hotkeys and tray are
//! up it records that it settled ([`Launcher::application_update_settled`]),
//! and only a start after that removes the old program the swap kept.
//!
//! The Settings window's About page (#82) is a second view of the same
//! flow, not a second flow: `Launcher::application_update` reads the
//! same state the rows come from, and its buttons run the same check
//! (`Launcher::check_application_update_again`) and the same restart
//! (`Launcher::restart_to_update`) the root rows run.

use std::future::Future;
use std::hash::BuildHasher;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::Duration;

use super::{Changing, Launcher, Screen, State, Status, Unavailable, WeakLauncher, off_thread};
use crate::Target;
use crate::application_update::{
    self, Found, InstallFailure, Offer, Program, Record, RecordedFailure, Staged,
};
use crate::feedback::{Toast, ToastAction, ToastDoes, ToastStyle, WindowPresence};
use crate::launcher::Entry;
use crate::launcher::Row;
use crate::releases::ReleaseSource;
use crate::restart::{self, Relaunch};

/// How often Pane checks for a newer version of itself while it runs, and
/// how old the last check may be before showing the launcher checks again
/// (ADR 0050).
const CHECK_EVERY: Duration = Duration::from_secs(4 * 3600);

/// The longest the schedule waits before it looks again, so that a change
/// of the system's time, or a computer waking from sleep, delays a check
/// by at most this much, as the extension updater's does.
const MAX_WAIT: Duration = Duration::from_secs(3600);

/// The shortest the schedule waits before it looks again.
const MIN_WAIT: Duration = Duration::from_secs(1);

/// The root row that checks for a Pane update at once, and the name
/// Raycast gives the same command, which finds it too.
const CHECK_TITLE: &str = "Check for Pane Updates";
const CHECK_ALTERNATE: &str = "Check for Updates";

/// The root row that restarts Pane into the update it staged, as Raycast
/// names its own.
pub const RESTART_TITLE: &str = "Restart to Update Pane";

/// The id of the row that restarts Pane into its update.
pub const RESTART_ROW: &str = "pane.update";

/// The label of the section the restart row is listed under, first of
/// all, for a blank query.
pub const UPDATE_SECTION: &str = "Update";

/// The title of the action of the toast that says an update is ready.
const RESTART_ACTION: &str = "Restart";

/// This launcher's application: the version of Pane it runs, where its
/// updates come from (the releases of Pane's repository on GitHub, or a
/// stand-in on this computer) and the program an update replaces. A
/// launcher without one (another system's updater is not wired yet, or
/// the program could not be named) checks for nothing and offers
/// nothing.
#[derive(Clone)]
pub(in crate::launcher) struct Application {
    version: String,
    source: ReleaseSource,
    program: Program,
}

/// What Pane knows of its own update: what the last check found, what is
/// running now, what failed, and the record kept across restarts.
#[derive(Default)]
pub(in crate::launcher) struct Updates {
    /// What the checks found, and what became of it.
    found: Notice,
    /// A check is running now.
    checking: bool,
    /// The update being downloaded now, with its progress.
    downloading: Option<Downloading>,
    /// Pane is restarting into the staged update now: its row is not
    /// listed meanwhile, and once the new version started, it stays so
    /// until Pane has quit.
    restarting: bool,
    /// Why checking failed last, if it did; cleared by one that answers.
    check_failed: Option<String>,
    /// Why downloading the update found last failed, if it did; cleared
    /// when a download begins again.
    download_failed: Option<String>,
    /// Why restarting into the staged update failed last, if it did: the
    /// update stays staged, ready to be chosen again, and the About page
    /// shows the failure beside it, as the status line does. Cleared when
    /// a new restart begins and when an update is staged anew.
    restart_failed: Option<String>,
    /// What Pane keeps of its checks across restarts, and where
    /// ([`Launcher::with_application_update_cache`]); `None` keeps it for
    /// this run only.
    record: Record,
    record_path: Option<PathBuf>,
    /// How often Pane checks: [`CHECK_EVERY`], unless a development build
    /// was given another ([`Launcher::with_application_update_interval`]).
    interval: Option<Duration>,
    /// How much later than the interval the next scheduled check is, in
    /// milliseconds: drawn anew after each check.
    jitter: u64,
    /// The schedule's thread, once started
    /// ([`Launcher::start_application_update_checks`]); it stops when this
    /// state goes with the launcher.
    schedule: Option<Arc<Schedule>>,
    /// How the new version is checked and started, and how long it has to
    /// report that it started: the system's own and
    /// [`restart::STARTED_LIMIT`], unless a test gave another
    /// ([`Launcher::with_relaunch`]).
    relaunch: Option<(Arc<dyn Relaunch>, Duration)>,
    /// The version whose toast saying it is ready was shown: once each.
    announced_ready: Option<String>,
    /// The version this run was restarted from, while the toast saying so
    /// waits for the launcher to be shown ([`Launcher::with_updated_from`]).
    updated_from: Option<String>,
}

/// What the checks found, and what became of it.
#[derive(Default)]
enum Notice {
    /// No check has answered yet.
    #[default]
    None,
    /// The last check found this Pane new enough: nothing to offer.
    Current,
    /// A newer version, found and not staged yet: it is being downloaded,
    /// or its download failed and the next check tries again. No row is
    /// listed for it (ADR 0050).
    Offered(Offer),
    /// A newer version, downloaded, checked and staged: the row restarts
    /// into it.
    Ready(Staged),
    /// The update was installed but its new version could not be started,
    /// and the swap could not be undone either: its version, which the
    /// next start of Pane runs. The status line and the About page say
    /// what happened; nothing is left to choose.
    Installed(String),
}

/// An update being downloaded: its version, and the bytes so far of the
/// size the release gives.
#[derive(Clone)]
struct Downloading {
    version: String,
    bytes: u64,
    total: u64,
}

/// What Pane knows of its own update, for a second surface of it beside
/// root search's rows (the Settings About page): one snapshot of the
/// state [`Launcher::application_update`] reads, so a page and the
/// launcher cannot disagree. Read only — nothing here checks, downloads
/// or installs anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplicationUpdate {
    /// No release source is configured: this Pane was given no release
    /// source to check (or its own program could not be named), so there
    /// is nothing to check, nothing to offer and no feed to claim. A
    /// page showing this explains the state rather than promising a
    /// check.
    Unconfigured,
    /// No check has finished yet — one may be about to run, or this
    /// Pane has not started one.
    Unchecked,
    /// A check is running now.
    Checking,
    /// The last check found this Pane new enough: no offer, no failure.
    Current,
    /// A newer version was found and is not downloaded yet: with why its
    /// download failed, if it did — the next check tries again.
    Offered {
        version: String,
        failure: Option<String>,
    },
    /// A newer version is being downloaded now: the bytes so far, of the
    /// size its release gives.
    Downloading {
        version: String,
        bytes: u64,
        total: u64,
    },
    /// A newer version is downloaded, checked and staged, ready for the
    /// user to restart into: its version, with why restarting into it
    /// last failed, if it did — it stays staged, ready to be chosen
    /// again.
    Ready {
        version: String,
        failure: Option<String>,
    },
    /// Pane is restarting into the staged update now.
    Restarting { version: String },
    /// The update was installed, but its new version could not be started
    /// and the swap could not be undone: its version, which the next start
    /// of Pane runs.
    Installed { version: String },
    /// The last check failed: why. The next check that is due tries
    /// again, and the user can ask for one.
    Failed(String),
}

/// When Pane last checked for its own update, and the last check or
/// download that failed, as its update record keeps them across
/// restarts: what the About page says beside the update's state. Times
/// are the launcher clock's, in milliseconds since the Unix epoch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateChecks {
    /// When the last check was made, whatever it came to.
    pub checked: Option<u64>,
    /// The last check or download that failed, when and why; gone once
    /// one succeeds.
    pub failure: Option<(u64, String)>,
}

impl ApplicationUpdate {
    /// How far a download has come, as the status line says it ("40% of
    /// 12.3 MiB"), while one runs.
    pub fn progress(&self) -> Option<String> {
        match self {
            ApplicationUpdate::Downloading { bytes, total, .. } => {
                Some(super::acquire::progress(*bytes, *total))
            }
            _ => None,
        }
    }
}

/// What a restart into the staged update came to.
enum Restarted {
    /// The new version started and reported so: this Pane quits.
    HandedOff,
    /// The swap could not be made; nothing changed, and the update stays
    /// staged.
    Kept(String),
    /// The staged program was gone or changed; it was removed, to be
    /// downloaded again.
    Restage(String),
    /// The new version did not start (why), and the swap was undone: the
    /// update stays staged.
    Undone(String),
    /// The new version did not start (the first), and the swap could not
    /// be undone either (the second): the new program stays in place,
    /// which the next start of Pane runs.
    Stuck(String, String),
}

impl Updates {
    /// How often Pane checks.
    fn interval(&self) -> Duration {
        self.interval.unwrap_or(CHECK_EVERY)
    }

    /// Takes the checking for one flow, so two cannot run at once.
    fn begin_check(&mut self) -> bool {
        !std::mem::replace(&mut self.checking, true)
    }

    /// When the next check is due, by the launcher's clock: the interval
    /// after the last check (and the drawn delay, for a scheduled check),
    /// and never while GitHub's rate limit is used up.
    fn due(&self, scheduled: bool) -> u64 {
        let interval = u64::try_from(self.interval().as_millis()).unwrap_or(u64::MAX);
        let after = self.record.checked.map_or(0, |checked| {
            checked
                .saturating_add(interval)
                .saturating_add(if scheduled { self.jitter } else { 0 })
        });
        after.max(self.record.limited_until.unwrap_or(0))
    }

    /// The staged update the row restarts into, when Pane is not
    /// restarting into it already and no newer one is being staged in its
    /// place.
    fn ready(&self) -> Option<Staged> {
        match (&self.found, self.restarting, &self.downloading) {
            (Notice::Ready(staged), false, None) => Some(staged.clone()),
            _ => None,
        }
    }

    /// The rows root search lists of Pane's own update, each with the
    /// other names it is found by: the staged update the user can restart
    /// into — unavailable, saying why, while Pane is `busy` with an
    /// extension — and the check the user can ask for at any time.
    pub(in crate::launcher) fn rows(&self, busy: Option<String>) -> Vec<(Row, Entry, Vec<String>)> {
        let mut rows = Vec::new();
        if let Some(staged) = self.ready() {
            rows.push((
                Row {
                    id: RESTART_ROW.into(),
                    title: RESTART_TITLE.into(),
                    subtitle: Some(format!(
                        "Pane {} is ready. Your extensions and settings are kept",
                        staged.version
                    )),
                    unavailable: busy.map(Unavailable::Busy),
                },
                Entry::RestartToUpdate,
                vec![format!("Update Pane to {}", staged.version)],
            ));
        }
        rows.push((
            Row {
                id: "pane.check-update".into(),
                title: CHECK_TITLE.into(),
                subtitle: Some("Look for a newer version of Pane now".into()),
                unavailable: None,
            },
            Entry::CheckUpdate,
            vec![CHECK_ALTERNATE.to_owned()],
        ));
        rows
    }

    /// Notes a check or download that failed in the record, for About and
    /// for the next start, and in Pane's log.
    fn note_failure(&mut self, at: u64, why: String) {
        crate::diagnostic!("Pane's own update: {why}");
        self.record.failure = Some(RecordedFailure { at, why });
    }

    /// Writes the record where it is kept, if it is.
    fn save(&self) {
        if let Some(path) = &self.record_path {
            self.record.save(path);
        }
    }

    /// How the new version is checked and started, and how long it has to
    /// report that it started.
    fn relaunch(&self) -> (Arc<dyn Relaunch>, Duration) {
        self.relaunch
            .clone()
            .unwrap_or_else(|| (Arc::new(restart::Native), restart::STARTED_LIMIT))
    }

    /// Where the new version reports that it started: Pane's cache folder,
    /// where the record is kept, as the new process names it too.
    fn report_folder(&self) -> PathBuf {
        self.record_path
            .as_deref()
            .and_then(Path::parent)
            .map_or_else(std::env::temp_dir, Path::to_path_buf)
    }
}

impl Drop for Updates {
    fn drop(&mut self) {
        if let Some(schedule) = &self.schedule {
            schedule.stop();
        }
    }
}

/// Why Pane cannot restart into its update now, if it cannot: it is
/// setting up its default extensions, or installing, updating, reloading
/// or removing an extension, which a restart would cut short.
pub(in crate::launcher) fn busy(state: &State) -> Option<String> {
    if state.acquisitions.in_flight() {
        return Some("Pane is setting up its extensions".into());
    }
    state
        .changing
        .iter()
        .find(|(_, what)| !matches!(what, Changing::Recording))
        .map(|(identity, what)| format!("{} {}", state.title_of(identity), what.doing()))
}

impl Launcher {
    /// What Pane knows of its own update, read without entering any flow
    /// and without listing any row: the same state root search's update
    /// rows come from, as a second surface of it (the Settings About page)
    /// shows. Whether a release source is configured at all, what the
    /// checks found — a newer version being downloaded or ready to
    /// restart into, that this Pane is new enough, or why the last check
    /// failed — and what is running now. Nothing here checks, downloads
    /// or installs anything.
    pub fn application_update(&self) -> ApplicationUpdate {
        if self.application.is_none() {
            return ApplicationUpdate::Unconfigured;
        }
        let state = self.lock();
        let updates = &state.updates;
        // A restart in flight comes first: the update it restarts into is
        // still the found notice, but what the page shows is the restart.
        if let Notice::Ready(staged) = &updates.found {
            if updates.restarting {
                return ApplicationUpdate::Restarting {
                    version: staged.version.clone(),
                };
            }
            if updates.downloading.is_none() {
                return ApplicationUpdate::Ready {
                    version: staged.version.clone(),
                    failure: updates.restart_failed.clone(),
                };
            }
        }
        if let Notice::Installed(version) = &updates.found {
            return ApplicationUpdate::Installed {
                version: version.clone(),
            };
        }
        if let Some(downloading) = &updates.downloading {
            return ApplicationUpdate::Downloading {
                version: downloading.version.clone(),
                bytes: downloading.bytes,
                total: downloading.total,
            };
        }
        if updates.checking {
            return ApplicationUpdate::Checking;
        }
        match (&updates.found, &updates.check_failed) {
            (Notice::Offered(offer), _) => ApplicationUpdate::Offered {
                version: offer.version.clone(),
                failure: updates.download_failed.clone(),
            },
            (_, Some(why)) => ApplicationUpdate::Failed(why.clone()),
            (Notice::None, None) => ApplicationUpdate::Unchecked,
            (_, None) => ApplicationUpdate::Current,
        }
    }

    /// When Pane last checked for its own update, and the last check or
    /// download that failed, as the update record keeps them: what the
    /// About page says beside [`Launcher::application_update`].
    pub fn application_update_checks(&self) -> UpdateChecks {
        let state = self.lock();
        let record = &state.updates.record;
        UpdateChecks {
            checked: record.checked,
            failure: record
                .failure
                .as_ref()
                .map(|failure| (failure.at, failure.why.clone())),
        }
    }

    /// This launcher checking for Pane application updates: the version
    /// `version` of Pane it runs, updated from `source`, replacing the
    /// program at `program` when the user chooses to restart into an
    /// update. What an earlier update left in the program's folder also
    /// goes, which a Pane starting does — except an update staged of a
    /// newer version, which is ready at once, and the old program an
    /// update kept, until this version has settled
    /// ([`Launcher::application_update_settled`]). A path that names no
    /// program file wires no updater at all.
    pub fn with_application_update(
        self,
        version: &str,
        source: ReleaseSource,
        program: PathBuf,
    ) -> Self {
        self.with_program_update(version, source, Program::at(program))
    }

    /// This launcher checking for Pane application updates as
    /// [`Launcher::with_application_update`] does, for a Pane that runs
    /// from the Linux AppImage at `appimage` (`pane_core::appimage`): an
    /// update installs the release's AppImage in that file's place, rather
    /// than the program the tarball holds (ADR 0049).
    pub fn with_appimage_update(
        self,
        version: &str,
        source: ReleaseSource,
        appimage: PathBuf,
    ) -> Self {
        self.with_program_update(version, source, Program::appimage(appimage))
    }

    fn with_program_update(
        self,
        version: &str,
        source: ReleaseSource,
        program: Result<Program, String>,
    ) -> Self {
        let application = program.ok().map(|program| {
            let mut state = self.lock();
            let settled = state.updates.record.settled.as_deref() == Some(version);
            if let Some(staged) = program.clean_install_folder(version, settled) {
                state.updates.found = Notice::Ready(staged);
            }
            Application {
                version: version.to_owned(),
                source,
                program,
            }
        });
        let launcher = Launcher {
            application,
            ..self
        };
        {
            let mut state = launcher.lock();
            launcher.refresh(&mut state);
        }
        launcher
    }

    /// Keeps Pane's update record in the cache folder `cache`
    /// (`application-update.json`), reading what it holds now: when Pane
    /// last checked, the listing it read and its `ETag`, when GitHub's
    /// rate limit lifts, the last failure, and the version that last
    /// settled — once it is this one, the old program an update kept goes.
    /// Without it, what a check learns lasts only while this launcher
    /// runs.
    pub fn with_application_update_cache(self, cache: &Path) -> Self {
        {
            let path = cache.join(application_update::RECORD);
            let mut state = self.lock();
            state.updates.record = Record::open(&path);
            state.updates.record_path = Some(path);
            if let Some(application) = &self.application
                && state.updates.record.settled.as_deref() == Some(application.version.as_str())
            {
                application.program.remove_old();
            }
        }
        self
    }

    /// Checks this often instead of every four hours: for development
    /// builds (`PANE_UPDATE_INTERVAL`) and tests, so a schedule can be
    /// watched without waiting hours. A release build has no such hook.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn with_application_update_interval(self, interval: Duration) -> Self {
        self.lock().updates.interval = Some(interval);
        self
    }

    /// Checks and starts the new version of a restart with `relaunch`,
    /// giving it `started_within` to report that it started, instead of
    /// the system's own processes and twenty seconds: for tests, whose
    /// programs are stand-ins that cannot run.
    #[doc(hidden)]
    pub fn with_relaunch(self, relaunch: Arc<dyn Relaunch>, started_within: Duration) -> Self {
        self.lock().updates.relaunch = Some((relaunch, started_within));
        self
    }

    /// This run of Pane was restarted from version `previous` into the one
    /// it runs, by a restart into an update (`--updated-from`): the first
    /// time the launcher is shown, a toast says "Updated to Pane
    /// <version>".
    pub fn with_updated_from(self, previous: &str) -> Self {
        self.lock().updates.updated_from = Some(previous.to_owned());
        self
    }

    /// Records that this version of Pane has settled: its window, hotkeys
    /// and tray icon are up. The next start of it removes the old program
    /// an update kept until then (ADR 0050). Called once a start has set
    /// those up.
    pub fn application_update_settled(&self) {
        let Some(application) = &self.application else {
            return;
        };
        let mut state = self.lock();
        let updates = &mut state.updates;
        if updates.record.settled.as_deref() != Some(application.version.as_str()) {
            updates.record.settled = Some(application.version.clone());
            updates.save();
        }
    }

    /// Starts checking for Pane updates by the schedule: at once if a
    /// check is due (none was made, or the last is older than the
    /// interval), then every interval with its random delay, on a thread
    /// of its own, until this launcher stops. Calling it again does
    /// nothing. A launcher that wires no updater checks for nothing.
    pub fn start_application_update_checks(&self) {
        if self.application.is_none() {
            return;
        }
        let schedule = {
            let mut state = self.lock();
            if state.updates.schedule.is_some() {
                return;
            }
            let schedule = Arc::new(Schedule::default());
            state.updates.schedule = Some(schedule.clone());
            state
                .clock
                .on_change(Box::new(poking(Arc::downgrade(&schedule))));
            schedule
        };
        let launcher = self.downgrade();
        let weak = Arc::downgrade(&schedule);
        let started = std::thread::Builder::new()
            .name("pane-application-update".into())
            .spawn(move || schedule_until_stopped(launcher, weak));
        if let Err(error) = started {
            crate::diagnostic!("Pane cannot check for its own updates by itself: {error}");
        }
    }

    /// Has the schedule look again when `clock`, given to a running
    /// launcher, is set ([`Launcher::with_clock`]).
    pub(in crate::launcher) fn follow_clock_for_application_update(
        &self,
        clock: &Arc<dyn crate::clipboard::Clock>,
    ) {
        if let Some(schedule) = &self.lock().updates.schedule {
            clock.on_change(Box::new(poking(Arc::downgrade(schedule))));
            schedule.poke();
        }
    }

    /// The launcher was shown: a check is made, in the background, if the
    /// last one is older than the interval (ADR 0050).
    pub(in crate::launcher) fn application_update_shown(&self, state: &State) {
        if self.application.is_none()
            || state.updates.checking
            || state.clock.now() < state.updates.due(false)
        {
            return;
        }
        let launcher = self.clone();
        let started = std::thread::Builder::new()
            .name("pane-application-update-check".into())
            .spawn(move || launcher.check_by_itself());
        if let Err(error) = started {
            crate::diagnostic!("Pane cannot check for its own update: {error}");
        }
    }

    /// Says what Pane's own update has to say, while the launcher is shown
    /// expanded (never as a HUD over other applications): "Updated to Pane
    /// <version>" once, after a restart into an update, and "Pane
    /// <version> is ready" once for each update staged, with Restart as
    /// its action. Called when the launcher is shown, and when an update
    /// is staged.
    pub(in crate::launcher) fn announce_application_update(&self, state: &mut State) {
        if state.feedback.presence != WindowPresence::Shown {
            return;
        }
        let Some(application) = &self.application else {
            return;
        };
        if state.updates.updated_from.take().is_some() {
            let toast = Toast::new(
                ToastStyle::Success,
                format!("Updated to Pane {}", application.version),
            );
            self.show_own_toast(state, toast);
            return;
        }
        let Some(staged) = state.updates.ready() else {
            return;
        };
        if state.updates.announced_ready.as_deref() == Some(staged.version.as_str()) {
            return;
        }
        state.updates.announced_ready = Some(staged.version.clone());
        let toast = Toast {
            style: ToastStyle::Success,
            title: format!("Pane {} is ready", staged.version),
            message: Some("Restart to update".into()),
            primary: Some(ToastAction {
                title: RESTART_ACTION.into(),
                shortcut: None,
                unbound: None,
                does: ToastDoes::RestartToUpdate,
            }),
            secondary: None,
        };
        self.show_own_toast(state, toast);
    }

    /// Checks Pane's releases for a newer version of Pane at once, as a
    /// scheduled check does: quietly, with no row and no word on the
    /// status line unless an update is staged, which lists the row that
    /// restarts into it. A newer version found is downloaded and staged
    /// before the returned future ends. Runs in the background; await the
    /// returned future to apply the result. Calling it again while a check
    /// runs does nothing.
    pub fn check_application_update(&self) -> impl Future<Output = ()> + Send + 'static {
        let launcher = self.clone();
        async move { off_thread(move || launcher.check_by_itself()).await }
    }

    /// The check for a Pane update the user asked for: root search's
    /// Check for Pane Updates runs it, and the Settings About page's
    /// "Check for updates" does too. It checks at once, whatever the
    /// schedule, and answers on the status line even when there is nothing
    /// to offer — the user asked, so up to date is an answer, not a
    /// silence; a newer version it finds is downloaded with its progress
    /// on the status line. Runs in the background; await the returned
    /// future to apply the result, as the window that offered the row
    /// does.
    pub fn check_application_update_again(&self) -> impl Future<Output = ()> + Send + 'static {
        let launcher = self.clone();
        async move { off_thread(move || launcher.check(true)).await }
    }

    /// A check Pane makes by itself: at start, by the schedule, when the
    /// launcher is shown, or after a staged update went missing.
    fn check_by_itself(&self) {
        self.check(false);
    }

    /// One check, and the download of what it finds. `asked`: the user
    /// asked for it, so it answers on the status line; otherwise it is
    /// quiet, its failures written to Pane's log and the record. Blocks
    /// on the network: run it off the window's thread.
    fn check(&self, asked: bool) {
        let Some(application) = self.application.clone() else {
            return;
        };
        // This Pane's own target: an entry for another system is
        // explained, not offered.
        let target = Target::current()
            .map(|target| target.id().to_owned())
            .unwrap_or_else(|| "a system Pane names no target for".into());
        let (mut record, now) = {
            let mut state = self.lock();
            if !state.updates.begin_check() {
                drop(state);
                if asked {
                    self.show(Status::Result(
                        "Pane is already checking for updates".into(),
                    ));
                }
                return;
            }
            if asked {
                state.view.status = Status::Progress("Checking for Pane updates…".into());
            }
            (state.updates.record.clone(), state.clock.now())
        };
        self.changed();
        let checked = application_update::check(
            &application.source,
            &application.version,
            &target,
            application.program.install(),
            &mut record,
            now,
        );
        let mut state = self.lock();
        let updates = &mut state.updates;
        updates.checking = false;
        updates.record = record;
        updates.jitter = jitter(updates.interval());
        let mut download = None;
        let status = match checked {
            Ok(Found::Offered(offer)) => {
                updates.check_failed = None;
                updates.record.failure = None;
                let version = offer.version.clone();
                let staged =
                    matches!(&updates.found, Notice::Ready(staged) if staged.version == version);
                if staged {
                    // Already downloaded: nothing to do but say so.
                    asked.then(|| Status::Result(format!("Pane {version} is ready")))
                } else if updates.downloading.is_some() || updates.restarting {
                    // Another download, or a restart, is running: the next
                    // check takes this one up.
                    asked.then(|| Status::Result(format!("Pane {version} is available")))
                } else {
                    if !matches!(updates.found, Notice::Ready(_)) {
                        updates.found = Notice::Offered(offer.clone());
                    }
                    updates.download_failed = None;
                    updates.downloading = Some(Downloading {
                        version: version.clone(),
                        bytes: 0,
                        total: offer.size,
                    });
                    download = Some(offer);
                    asked.then(|| Status::Progress(format!("Downloading Pane {version}…")))
                }
            }
            Ok(Found::UpToDate) => {
                updates.check_failed = None;
                updates.record.failure = None;
                // A staged or installed update stays what it is: a listing
                // without it does not undo a download.
                if matches!(updates.found, Notice::None | Notice::Offered(_)) {
                    updates.found = Notice::Current;
                }
                asked.then(|| Status::Result("Pane is up to date".into()))
            }
            Err(why) => {
                updates.check_failed = Some(why.clone());
                updates.note_failure(now, format!("Could not check for an update: {why}"));
                asked.then(|| Status::Error(format!("Could not check for a Pane update: {why}")))
            }
        };
        updates.save();
        if let Some(status) = status {
            state.view.status = status;
        }
        self.refresh(&mut state);
        drop(state);
        self.changed();
        if let Some(offer) = download {
            self.download(&application, offer, asked);
        }
    }

    /// Downloads, checks and stages `offer`, which a check just found,
    /// telling the About page its progress (and the status line, when the
    /// user asked for the check). Once it is staged, the toast says it is
    /// ready, if the launcher is shown. A failure is noted for About and
    /// the log, and the next check tries again. Blocks on the network.
    fn download(&self, application: &Application, offer: Offer, asked: bool) {
        let version = offer.version.clone();
        let (state, changes) = (self.state.clone(), self.developing.changes());
        let telling = version.clone();
        let telling = move |bytes: u64, total: u64| {
            let mut state = state.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(downloading) = &mut state.updates.downloading {
                downloading.bytes = bytes;
                downloading.total = total;
            }
            if asked {
                state.view.status = Status::Progress(format!(
                    "Downloading Pane {telling}: {}",
                    super::acquire::progress(bytes, total)
                ));
            }
            drop(state);
            if let Some(changes) = &changes {
                changes.changed();
            }
        };
        let staged = application_update::fetch(&application.source, &offer, &telling)
            .and_then(|package| application_update::stage(&package, &offer, &application.program));
        let mut state = self.lock();
        let now = state.clock.now();
        let updates = &mut state.updates;
        updates.downloading = None;
        let ready = staged.is_ok();
        let status = match staged {
            Ok(staged) => {
                updates.found = Notice::Ready(staged);
                updates.restart_failed = None;
                updates.record.failure = None;
                // The toast below says it, where the launcher is shown;
                // the status line goes back to rest.
                asked.then_some(Status::Idle)
            }
            Err(why) => {
                updates.download_failed = Some(why.clone());
                updates.note_failure(now, format!("Could not download Pane {version}: {why}"));
                asked.then(|| Status::Error(format!("Could not download Pane {version}: {why}")))
            }
        };
        updates.save();
        if let Some(status) = status {
            state.view.status = status;
        }
        self.refresh(&mut state);
        if ready {
            self.announce_application_update(&mut state);
        }
        drop(state);
        self.changed();
    }

    /// Restarts Pane into the staged update, as the user chose it: the
    /// root result "Restart to Update Pane" is one choice, the toast's
    /// Restart another, and the Settings About page's button a third. The
    /// staged program is checked again and replaces the running one, the
    /// new program is checked and started, and once it reports that it
    /// started, the window is asked to quit Pane. Nothing is downloaded.
    /// While Pane is busy with an extension, nothing happens but saying
    /// so. On failure the running program is the one it was, explained,
    /// and the update stays to try again — or, when the staged program
    /// changed or went, it is downloaded again in the background. Runs in
    /// the background; await the returned future to apply the result, as
    /// the window that offered the row does.
    pub fn restart_to_update(&self) -> impl Future<Output = ()> + Send + 'static {
        let launcher = self.clone();
        async move { launcher.restart_chosen_update().await }
    }

    /// The restart the user chose; see [`Launcher::restart_to_update`].
    async fn restart_chosen_update(&self) {
        let Some(application) = self.application.clone() else {
            return;
        };
        let (staged, relaunch, folder) = {
            let mut state = self.lock();
            let Some(staged) = state.updates.ready() else {
                return;
            };
            if let Some(busy) = busy(&state) {
                state.view.status = Status::Error(format!(
                    "Pane can restart to update once this is done: {busy}"
                ));
                drop(state);
                self.changed();
                return;
            }
            state.updates.restarting = true;
            // A new attempt begins: whatever the last one failed with is
            // no longer the word on it.
            state.updates.restart_failed = None;
            state.view.status =
                Status::Progress(format!("Restarting to update Pane to {}…", staged.version));
            self.refresh(&mut state);
            (
                staged,
                state.updates.relaunch(),
                state.updates.report_folder(),
            )
        };
        self.changed();
        let version = staged.version.clone();
        let restarting = application.clone();
        let restarted =
            off_thread(move || restart_into(&staged, &restarting, relaunch, &folder)).await;
        let restage = {
            let mut state = self.lock();
            let previous = &application.version;
            match restarted {
                Restarted::HandedOff => {
                    // The new version runs and waits for this one to exit:
                    // the restart stays in flight until Pane has quit.
                    state.view.status =
                        Status::Progress(format!("Restarting into Pane {version}…"));
                    self.refresh(&mut state);
                    state.feedback.window.quit();
                    false
                }
                Restarted::Kept(why) => {
                    state.updates.restarting = false;
                    state.updates.restart_failed = Some(why.clone());
                    state.view.status =
                        Status::Error(format!("Could not update Pane to {version}: {why}"));
                    self.refresh(&mut state);
                    select_offer(&mut state);
                    false
                }
                Restarted::Undone(why) => {
                    // The update stays staged; the reason is kept with it
                    // (see [`Updates::restart_failed`]), so the Settings
                    // About page shows it beside the button that tries
                    // again.
                    let said = format!(
                        "Pane {version} could not start, so Pane stays at {previous}: {why}"
                    );
                    crate::diagnostic!("Pane's own update: {said}");
                    state.updates.restarting = false;
                    state.updates.restart_failed = Some(said.clone());
                    state.view.status = Status::Error(said);
                    self.refresh(&mut state);
                    select_offer(&mut state);
                    false
                }
                Restarted::Stuck(why, undo) => {
                    let said = format!(
                        "Pane {version} could not start ({why}), and Pane could not go back to \
                         {previous}: {undo}. Pane {version} is used the next time Pane starts"
                    );
                    crate::diagnostic!("Pane's own update: {said}");
                    state.updates.restarting = false;
                    state.updates.found = Notice::Installed(version.clone());
                    state.view.status = Status::Error(said);
                    self.refresh(&mut state);
                    false
                }
                Restarted::Restage(why) => {
                    state.updates.restarting = false;
                    state.updates.found = Notice::None;
                    state.view.status =
                        Status::Error(format!("Could not update Pane to {version}: {why}"));
                    self.refresh(&mut state);
                    true
                }
            }
        };
        self.changed();
        if restage {
            off_thread({
                let launcher = self.clone();
                move || launcher.check_by_itself()
            })
            .await;
        }
    }

    /// One look of the schedule: a check when one is due, and how long
    /// until the next is.
    fn scheduled_look(&self) -> Duration {
        let due = {
            let state = self.lock();
            state.clock.now() >= state.updates.due(true) && !state.updates.checking
        };
        if due {
            self.check_by_itself();
        }
        let state = self.lock();
        let now = state.clock.now();
        Duration::from_millis(state.updates.due(true).saturating_sub(now))
    }
}

/// Restarts into `staged`: swaps `application`'s program for it, checks
/// that the new program answers with its version, starts it and waits for
/// its report in `folder` (see `restart`). A new version that does not
/// answer, start or report is ended, and the swap undone. Blocks: run it
/// off the window's thread.
fn restart_into(
    staged: &Staged,
    application: &Application,
    relaunch: (Arc<dyn Relaunch>, Duration),
    folder: &Path,
) -> Restarted {
    let (relaunch, started_within) = relaunch;
    let program = &application.program;
    match application_update::install(staged, program) {
        Ok(()) => {}
        Err(InstallFailure::Kept(why)) => return Restarted::Kept(why),
        Err(InstallFailure::Restage(why)) => return Restarted::Restage(why),
    }
    let not_started = |why: String| match application_update::undo(program, &application.version) {
        Ok(()) => Restarted::Undone(why),
        Err(undo) => Restarted::Stuck(why, undo),
    };
    let path = program.path();
    match relaunch.version(&path) {
        Ok(version) if version == staged.version => {}
        Ok(version) => {
            return not_started(format!("the new program says it is Pane {version}"));
        }
        Err(why) => return not_started(format!("the new program does not run: {why}")),
    }
    let own = std::process::id();
    restart::forget_report(folder, own);
    let arguments = restart::arguments(&application.version, own);
    let mut started = match relaunch.start(&path, &arguments) {
        Ok(started) => started,
        Err(why) => return not_started(why),
    };
    if let Err(why) = restart::wait_for_started(folder, own, started.as_mut(), started_within) {
        started.end();
        return not_started(why);
    }
    // The new version runs: the swap stays, and nothing of the staging
    // folder is needed any more.
    program.remove_staging();
    Restarted::HandedOff
}

/// A random delay of up to a sixteenth of `interval` (fifteen minutes of
/// four hours), in milliseconds, so that Panes sharing one address do not
/// all check at once.
fn jitter(interval: Duration) -> u64 {
    let most = u64::try_from(interval.as_millis() / 16).unwrap_or(u64::MAX);
    if most == 0 {
        return 0;
    }
    std::collections::hash_map::RandomState::new().hash_one(std::time::Instant::now()) % most
}

/// Wakes the schedule's thread when a check may be due sooner than it
/// thought (the launcher's clock was set), and stops it when the
/// launcher goes.
#[derive(Default)]
pub(in crate::launcher) struct Schedule {
    state: Mutex<Waking>,
    condvar: Condvar,
}

#[derive(Default)]
struct Waking {
    stopped: bool,
    /// Counts pokes: each asks the thread to look again.
    pokes: u64,
}

impl Schedule {
    fn lock(&self) -> MutexGuard<'_, Waking> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn poke(&self) {
        self.lock().pokes += 1;
        self.condvar.notify_all();
    }

    fn stop(&self) {
        self.lock().stopped = true;
        self.condvar.notify_all();
    }

    /// The pokes so far, or `None` once stopped.
    fn begin(&self) -> Option<u64> {
        let state = self.lock();
        (!state.stopped).then_some(state.pokes)
    }

    /// Waits at most `limit` for a poke after the `seen`th; returns
    /// whether the schedule still runs.
    fn wait(&self, seen: u64, limit: Duration) -> bool {
        let state = self.lock();
        let (state, _) = self
            .condvar
            .wait_timeout_while(state, limit, |state| !state.stopped && state.pokes == seen)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        !state.stopped
    }
}

/// The schedule's thread: looks, checking when a check is due, then waits
/// until the next is, the clock is set, or the launcher stops. The
/// launcher is held only while it looks, so the thread never keeps it
/// running.
fn schedule_until_stopped(launcher: WeakLauncher, schedule: Weak<Schedule>) {
    while let Some(current) = schedule.upgrade() {
        let Some(seen) = current.begin() else {
            return;
        };
        let Some(running) = launcher.upgrade() else {
            return;
        };
        let wait = running.scheduled_look();
        drop(running);
        // A check that is due while another runs (the launcher was shown)
        // is looked at again a moment later, not in a tight loop.
        if !current.wait(seen, wait.clamp(MIN_WAIT, MAX_WAIT)) {
            return;
        }
    }
}

/// A closure that pokes the schedule `weak` names awake, holding it weakly
/// so that a clock does not keep it running past the launcher.
fn poking(weak: Weak<Schedule>) -> impl Fn() + Send + Sync + 'static {
    move || {
        if let Some(schedule) = weak.upgrade() {
            schedule.poke();
        }
    }
}

/// Selects the restart row in root search, if it is listed, so Enter
/// tries a failed restart again (#424). The row left the list while the
/// restart ran, so it could not be started twice, and the selection moved
/// to another row meanwhile; without this, it would stay there when the
/// row came back, and Enter would run that row instead.
fn select_offer(state: &mut State) {
    if !matches!(state.view.screen, Screen::Root { .. }) {
        return;
    }
    if let Some(at) = state
        .entries
        .iter()
        .position(|entry| matches!(entry, Entry::RestartToUpdate))
    {
        state.view.selected = Some(at);
    }
}
