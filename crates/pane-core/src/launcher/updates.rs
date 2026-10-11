//! Automatic updates of installed packages from npm and Git: Pane
//! replaces the managed copy of an eligible npm package with a compatible
//! newer version from its registry, and of an eligible Git package with
//! the newer commit of the tracked branch it was installed from, without
//! the user asking (US72–US75, T15).
//!
//! The updater is a thread of Pane's own, shaped like the scheduler's and
//! the services thread's, driven by the launcher's clock: it checks a
//! minute after Pane starts — never competing with Pane's own start — and
//! then every
//! [`CHECK_EVERY`](CHECK_EVERY) hours, in the background, so the window
//! never waits for the registry or the repository. A check reads only the
//! metadata — the registry's for an npm package, the repository's
//! reference listing for a Git one; nothing is downloaded while the
//! latest version is the version installed, or the tracked branch points
//! at the commit installed. A newer version or commit is fetched and
//! checked exactly as an install checks a package (its manifest and API,
//! its components, its platforms and helpers, and its dependencies as a
//! plan), and what was downloaded stays staged until it is applied.
//!
//! Applying a staged update waits for the safe activation boundary: no
//! screen of the package is on display (a command, its search, a form or
//! a custom view — a call the user is waiting on runs with one open), and
//! no call of it the user asked for is running (see
//! [`Runtime::busy`](crate::Runtime::busy)). The user is never
//! interrupted mid-command; the update is tried again every
//! [`RETRY_EVERY`](RETRY_EVERY) seconds until the package is quiet.
//! Managed background work (a scheduled run, a service cycle) is not
//! waited for: a replacement ends it with the package's generation and
//! the new code starts it again, exactly as a reload does. Applying the
//! update is an update as the preview's Update row is: the identity, the
//! saved data, the disabled state, the hotkeys and the aliases are kept,
//! and the old code's generation ends, stopping what is still pending
//! with its late answer discarded.
//!
//! Which packages are eligible: installed from npm and not pinned (a
//! pinned version is kept, whatever the latest is), or installed from Git
//! and tracked (a tag or commit named to install it is pinned and kept,
//! and an update follows the branch), and in either case enabled, not
//! paused after a failure, and not turned off — the user's controls are a
//! global choice and a per-package one (rows in the extension list,
//! recorded in `updates.json` beside `installed.json`). A local folder's
//! or a development copy's code is never replaced here: only npm and Git
//! packages update by themselves. A check or an update that fails is
//! recorded, and the installed copy is left as it is.
//!
//! **The results.** Each pass records what it came to per package, as
//! Pane's own record beside `updates.json` (`update-results.json`, see
//! `update_results`): a package the pass does not look at is skipped with
//! why, a check or an update that fails is recorded as failed — and a
//! failure is announced once, the next time the launcher is shown —
//! while a successful update is quiet, its outcome the record itself,
//! which the results screen shows. The status-line messages a single
//! background update once set are replaced by that. An update staged
//! whose package is in use waits, listed in the record as waiting until
//! the package is not in use.
//!
//! **The pass the user asks for** ([`Launcher::check_extension_updates`])
//! checks at once, whatever the cadence, and looks wider than the
//! automatic pass: every installed package from a source Pane updates,
//! default extensions included, turned off, disabled and paused ones
//! included — the user asked, so "update all" means all (an update keeps
//! a disabled package disabled, and unpauses a paused one, as the
//! preview's Update row does). A toast
//! follows it — "Checking for extension updates…", "Updating N of M…", a
//! summary ending with View Details — and it answers even when everything
//! is up to date. Root search's "Check for Extension Updates" row, the
//! Settings Extensions group's "Check for updates" button, and the
//! results view's Retry and Update Now all run it, the last two over one
//! extension (ADR 0043: two entry points, one flow). Its checks run a
//! few packages at a time, each bounded by Pane's HTTP limits as ever.
//!
//! **Provisional, pending the user's decision:** the cadence (a check a
//! minute after Pane starts, then every 24 hours, by the launcher's
//! clock), the retry
//! every second while a package is in use, and that a disabled or paused
//! package is not updated (updating a paused one would unpause it, which
//! is the user's choice to make).

use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use super::install::{self, Stopped};
use super::update_results::Pass;
use super::{
    ApplicationUpdate, Changing, Entry, Launcher, Mode, PackageIdentity, Row, State, Status,
    WeakLauncher, off_thread,
};
use crate::atomic::{Readers, write_atomically};
use crate::clipboard::Clock;
use crate::defaults::{DefaultExtension, InstalledDefault};
use crate::dependencies;
use crate::git::{self, GitRevision};
use crate::npm;
use crate::packages::{InstalledPackage, PackageError, SourcePackage};

/// How long Pane waits after it starts before the first check, so that
/// the check never competes with Pane's own start, as Raycast's does.
/// Provisional, pending the user's decision on update timing; the
/// launcher's clock keeps it (see [`Launcher::with_clock`]), so tests
/// stay fast.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(60);

/// How often Pane checks for a newer version of each eligible package:
/// the launcher's clock tells when. Provisional, like scheduled work's
/// intervals, pending the user's decision on update timing.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 3600);

/// How often a staged update is tried again while the package is in use.
/// Provisional; a look is a lock and a comparison, so this is cheap.
const RETRY_EVERY: Duration = Duration::from_secs(1);

/// The longest the updater waits before it looks again, so that a change
/// of the system's time, or a computer waking from sleep, delays a check
/// by at most this much. As the scheduler's and the services thread's.
const MAX_WAIT: Duration = Duration::from_secs(3600);

/// How many packages a pass checks at once: a few, so a pass over many
/// finishes sooner without the checks of one holding up the rest — each
/// check is bounded by Pane's HTTP limits as ever. The pass's outcomes
/// merge per few in the order the packages were checked, so the record
/// stays in the installed list's order however they completed.
const AT_ONCE: usize = 4;

/// How long a pass the user asked for is given to end before the future
/// its entry point returns resolves anyway: bounded, so the window that
/// started it never waits for ever, while a pass that outlives it goes
/// on — its toast still reaches the window on its own.
const ASKED_PASS_WAIT: Duration = Duration::from_secs(600);

/// A pass the user asked for (see [`Launcher::check_extension_updates`]),
/// and which packages it looks at.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Ask {
    /// Every installed package from a source Pane updates, whatever the
    /// user's controls and the package's state: the Check for Extension
    /// Updates command, and the Settings Extensions group's Check for
    /// updates button.
    All,
    /// The one extension a result row's action names: Retry on a Failed
    /// row, Update Now on a Skipped one. The pass says nothing of any
    /// other extension.
    One(PackageIdentity),
}

/// The record of the user's automatic-update choices, `updates.json`
/// beside `installed.json`: whether every eligible package updates in the
/// background, and which packages the user turned it off for. Absent
/// fields mean the defaults: on, and none turned off — as no record at
/// all does, or one this Pane cannot read (the next choice the user makes
/// writes it anew).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct UpdateControls {
    /// Whether eligible packages update automatically. On unless the user
    /// turned it off.
    pub(super) automatic: bool,
    /// The identity keys of the packages the user turned updates off for.
    pub(super) off: HashSet<String>,
}

impl Default for UpdateControls {
    fn default() -> UpdateControls {
        UpdateControls {
            automatic: true,
            off: HashSet::new(),
        }
    }
}

impl UpdateControls {
    /// Reads the controls recorded in `dir`; `None` when there is no
    /// record, or one this Pane cannot read (which it never replaces).
    pub(super) fn open(dir: &std::path::Path) -> Option<UpdateControls> {
        let text = std::fs::read_to_string(dir.join(FILE)).ok()?;
        let read: Result<Recorded, _> = serde_json::from_str(&text);
        let read = read.ok()?;
        (read.version == 1).then(|| UpdateControls {
            automatic: read.automatic,
            off: read.off.unwrap_or_default().into_iter().collect(),
        })
    }

    /// The record's text for these controls, and where it goes.
    fn text(&self, dir: &std::path::Path) -> (PathBuf, String) {
        let recorded = Recorded {
            version: 1,
            automatic: self.automatic,
            off: (!self.off.is_empty()).then(|| self.off.iter().cloned().collect()),
        };
        (
            dir.join(FILE),
            serde_json::to_string_pretty(&recorded).unwrap_or_default(),
        )
    }
}

/// The file the controls are recorded in, beside `installed.json`.
const FILE: &str = "updates.json";

/// The controls as `updates.json` records them. `automatic` is written
/// only when off and `off` only when any is, so the file stays small and
/// a record from before a field existed still reads.
#[derive(serde::Serialize, serde::Deserialize)]
struct Recorded {
    version: u64,
    #[serde(default = "default_true", skip_serializing_if = "std::ops::Not::not")]
    automatic: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    off: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
}

/// Runs the automatic updates of the launcher's installed npm packages.
/// Kept by the launcher; dropping it stops the thread.
pub(super) struct Updates {
    /// Where packages are read from: the registry a development build is
    /// given replaces the public one here too, once
    /// [`Launcher::with_npm_registry`] has run.
    sources: Mutex<install::Sources>,
    /// What the updater knows: the clock it follows, when it next checks,
    /// and what it has staged to apply.
    state: Mutex<Checking>,
    /// Wakes the updater thread, and tells when it settled.
    settled: Settled,
    /// Test support: holds an update between its claim and its
    /// replacement while a test asks (see [`Launcher::hold_update_applies`]).
    hold: Arc<Hold>,
}

/// Test support: whether an update the updater applies waits, once it has
/// claimed the package and before its replacement is written, and what
/// wakes it when that ends.
#[derive(Default)]
struct Hold {
    held: Mutex<bool>,
    condvar: std::sync::Condvar,
}

impl Hold {
    fn lock(&self) -> MutexGuard<'_, bool> {
        self.held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Waits while held.
    fn wait(&self) {
        let held = self.lock();
        let _released = self
            .condvar
            .wait_while(held, |held| *held)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

/// Test support: while this is kept, every update Pane applies by itself
/// stops once it has claimed its package, before the replacement is
/// written; dropping it lets them go on (see
/// [`Launcher::hold_update_applies`]).
#[doc(hidden)]
#[must_use = "the updates go on as soon as the hold is dropped"]
pub struct UpdateHold(Arc<Hold>);

impl UpdateHold {
    /// The hold of a launcher that applies no updates.
    pub(super) fn of_nothing() -> UpdateHold {
        UpdateHold(Arc::default())
    }
}

impl Drop for UpdateHold {
    fn drop(&mut self) {
        *self.0.lock() = false;
        self.0.condvar.notify_all();
    }
}

/// What the updater keeps: the clock it follows, when it next checks,
/// the pass the user asked for that has not begun, one staged update per
/// package, the pass it is running, and the toast that pass shows.
struct Checking {
    clock: Arc<dyn Clock>,
    /// When the next check is, in clock milliseconds.
    next_check: u64,
    /// The pass the user asked for that has not begun yet (set by
    /// [`Updates::check_now_asked`]); the check that runs it takes it,
    /// widening what the pass looks at.
    asked: Option<Ask>,
    /// The updates Pane has downloaded and checked and not applied yet,
    /// each holding its download alive until then.
    staged: Vec<Staged>,
    /// The pass whose outcomes collect: begun by each check, amended by
    /// what applies afterwards (an update the boundary deferred applies
    /// in a later look, still this pass), until the next check begins
    /// another. What it came to is Pane's record (see `update_results`).
    pass: Option<Pass>,
    /// The toast a pass the user asked for shows, by its id, while that
    /// pass runs: updated through it and ended as its summary (see
    /// `update_results`), then taken. Only the look that ran the asked
    /// pass's checks updates it — what that look deferred lands in the
    /// record without a toast, the summary having ended the pass's say.
    toast: Option<u64>,
}

/// A new version of one package, downloaded and checked, waiting for the
/// package to be quiet so that it can replace the installed copy.
struct Staged {
    identity: PackageIdentity,
    /// Whether the pass the user asked for staged this: applying it does
    /// not re-ask the user's controls — the user asked for the update —
    /// but still drops it if the package was pinned, uninstalled or
    /// replaced from another source since.
    asked: bool,
    /// What marks the revision the installed copy had when this was
    /// staged — the version of an npm package, the commit of a Git one —
    /// so an installed copy at another since means this is not for it.
    installed: String,
    /// The package to install, at the version or commit it would be.
    package: SourcePackage,
    /// What installing it means for its dependencies, as checked.
    plan: dependencies::Plan,
}

impl Updates {
    /// The updater that reads packages from `sources`, checking
    /// [`FIRST_CHECK_AFTER`] after it starts and then every
    /// [`CHECK_EVERY`]. [`Updates::run`] starts its thread.
    pub(super) fn start(sources: install::Sources) -> Arc<Updates> {
        let clock: Arc<dyn Clock> = Arc::new(crate::clipboard::SystemClock);
        let next_check = clock.now() + FIRST_CHECK_AFTER.as_millis() as u64;
        Arc::new(Updates {
            sources: Mutex::new(sources),
            state: Mutex::new(Checking {
                clock,
                next_check,
                asked: None,
                staged: Vec::new(),
                pass: None,
                toast: None,
            }),
            settled: Settled::default(),
            hold: Arc::default(),
        })
    }

    /// Holds every update applied from now on between its claim and its
    /// replacement, until the hold returned is dropped.
    pub(super) fn hold_applies(&self) -> UpdateHold {
        *self.hold.lock() = true;
        UpdateHold(self.hold.clone())
    }

    /// Starts the updater's thread, which runs until this launcher stops.
    pub(super) fn run(self: &Arc<Self>, launcher: WeakLauncher) {
        let updates = Arc::downgrade(self);
        let started = std::thread::Builder::new()
            .name("pane-updates".into())
            .spawn(move || update_until_stopped(launcher, updates));
        if let Err(error) = started {
            crate::diagnostic!(
                "Pane cannot check for extension updates in the background: {error}"
            );
        }
    }

    /// The registry and downloads folder packages are read from now on
    /// ([`Launcher::with_npm_registry`], in development builds), and a
    /// look at once.
    pub(super) fn follow_registry(&self, sources: install::Sources) {
        *self.lock_sources() = sources;
        self.settled.poke();
    }

    /// The clock the updater follows from now on
    /// ([`Launcher::with_clock`]), checking again one
    /// [`FIRST_CHECK_AFTER`] after its now: the first check a clock given
    /// to a running Pane stands where the clock stands.
    pub(super) fn follow(self: &Arc<Self>, clock: Arc<dyn Clock>) {
        {
            let mut state = self.lock();
            state.clock = clock.clone();
            state.next_check = clock.now() + FIRST_CHECK_AFTER.as_millis() as u64;
        }
        clock.on_change(Box::new(poking(Arc::downgrade(self))));
        self.settled.poke();
    }

    /// Checks at once, whatever the cadence: the user's controls changed,
    /// so the packages they govern are looked at again.
    pub(super) fn check_now(&self) {
        self.lock().next_check = 0;
        self.settled.poke();
    }

    /// Checks at once, whatever the cadence, for a pass the user asked
    /// for over `ask`: it looks wider than the automatic pass — every
    /// installed package from a source Pane updates, turned off, disabled
    /// and paused ones included — answers even when everything is up to
    /// date, and shows the toast that follows it. The next check that
    /// begins runs it (see [`Ask`]).
    fn check_now_asked(&self, ask: Ask) {
        {
            let mut state = self.lock();
            state.next_check = 0;
            state.asked = Some(ask);
        }
        self.settled.poke();
    }

    /// Waits until a check pass completed and the updater settled — every
    /// poke so far was looked at, and every update that pass staged was
    /// applied or deferred; `false` if it did not within `limit`, or it
    /// stopped. For tests and development builds, which so wait for the
    /// check a start, a cadence or a control change brings.
    pub(super) fn checked(&self, limit: Duration) -> bool {
        self.settled.checked(limit)
    }

    fn lock(&self) -> MutexGuard<'_, Checking> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_sources(&self) -> MutexGuard<'_, install::Sources> {
        self.sources
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// How long the updater may wait before it looks again: until the
    /// next check, or, while an update is deferred, no longer than
    /// [`RETRY_EVERY`]. A poke (the clock, the controls) wakes it sooner.
    fn next_wait(&self) -> Duration {
        let state = self.lock();
        let now = state.clock.now();
        let wait = Duration::from_millis(state.next_check.saturating_sub(now));
        if state.staged.is_empty() {
            wait.min(MAX_WAIT)
        } else {
            wait.min(RETRY_EVERY)
        }
    }

    /// One look: a check when one is due, then applying what is staged at
    /// the boundary each package allows. Blocks on the network, on the
    /// package checks and on the install's writes, so it runs on the
    /// updater's own thread, never the window's. A look that ran a pass
    /// the user asked for ends that pass once it has applied what it can:
    /// what it deferred waits for the package to grow quiet, landing in
    /// the record without a toast.
    fn pass(&self, launcher: &Launcher) {
        let (now, next) = {
            let state = self.lock();
            (state.clock.now(), state.next_check)
        };
        let asked = if now >= next {
            self.check(launcher)
        } else {
            None
        };
        self.apply(launcher);
        if asked.is_some() {
            self.end_asked_pass(launcher);
        }
    }

    /// Ends the pass the user asked for that this look ran: its toast —
    /// "Checking for extension updates…", "Updating N of M…" — becomes its
    /// summary of what it updated and failed, with View Details (see
    /// `update_results`). What this look deferred (a package in use) is
    /// not counted: the toast ends when the pass has nothing more it can
    /// do now, and the row becomes an Updated one once the package grows
    /// quiet.
    fn end_asked_pass(&self, launcher: &Launcher) {
        let (pass, toast) = {
            let mut state = self.lock();
            (state.pass.clone(), state.toast.take())
        };
        let Some(id) = toast else { return };
        let Some(pass) = pass else { return };
        let mut state = launcher.lock();
        launcher.end_update_pass_toast(&mut state, id, &pass);
    }

    /// Checks every eligible installed package for a newer version and
    /// stages what it finds, collecting what the pass came to per package
    /// as its results record (see `update_results`): a package whose
    /// metadata cannot be read fails it, a newer version that cannot be
    /// downloaded or does not pass the checks an install makes fails it
    /// too — except one that needs a newer Pane or is not available on
    /// this system, which is skipped with that reason — and every
    /// package the pass does not look at is skipped with why. The
    /// installed copy is left as it is either way. Returns the pass the
    /// user asked for this check ran, if it ran one (see [`Ask`]): a
    /// check the user asked for looks wider than the automatic one, and
    /// answers even when everything is up to date.
    fn check(&self, launcher: &Launcher) -> Option<Ask> {
        // The pass the user asked for, which this check runs: taken here,
        // so the next check begins a fresh (automatic) pass whatever
        // happens to this one.
        let asked = self.lock().asked.take();
        // What to check: the eligible installed npm, Git and default
        // packages, skipping any something is already happening to (an
        // install, an update, a change the user asked for), which the next
        // check catches. An asked pass looks wider — turned-off, disabled
        // and paused packages too, the user asking for them — and, asked
        // of one extension, that one alone. The pass the outcomes collect
        // for, and the rows for every package the pass looks at but does
        // not check, with why it does not: a pass that records says what
        // became of every extension it considered.
        let (candidates, mut pass, toast) = {
            let mut state = launcher.lock();
            let in_scope = |package: &InstalledPackage| match &asked {
                Some(Ask::One(identity)) => package.identity == *identity,
                _ => true,
            };
            let updatable = |package: &InstalledPackage| match &asked {
                Some(_) => eligible_when_asked(package),
                None => eligible(&state, package),
            };
            let candidates: Vec<InstalledPackage> = state
                .packages
                .iter()
                .filter(|package| {
                    in_scope(package)
                        && updatable(package)
                        && !state.changing.contains_key(&package.identity)
                })
                .cloned()
                .collect();
            let mut pass = Pass::after(&state.update_results, asked.is_some());
            for package in &state.packages {
                if candidates
                    .iter()
                    .any(|candidate| candidate.identity == package.identity)
                {
                    continue;
                }
                // A pass asked of one extension says nothing of the
                // others: they were not its business.
                if !in_scope(package) {
                    continue;
                }
                if let Some((identity, title, why)) =
                    skipped_row(launcher, &state, package, asked.is_some())
                {
                    pass.skipped(identity, title, &why);
                }
            }
            // A pass the user asked for says it is working: one toast,
            // updated through the pass (see `update_results`).
            let toast = asked
                .is_some()
                .then(|| launcher.begin_update_pass_toast(&mut state));
            (candidates, pass, toast)
        };
        // A package that is no longer one a pass could look at, or whose
        // installed copy is not at the version or commit its staged update
        // was staged against, keeps nothing staged: the next check plans
        // again. A staged update of a pass the user asked for survives a
        // wider set: the user asked for it, so only the package itself
        // changing ends it.
        let keep: Vec<(PackageIdentity, String, bool)> = {
            let state = launcher.lock();
            state
                .packages
                .iter()
                .filter(|package| !state.changing.contains_key(&package.identity))
                .filter_map(|package| {
                    Some((
                        package.identity.clone(),
                        installed_at(package)?,
                        eligible(&state, package) || eligible_when_asked(package),
                    ))
                })
                .collect()
        };
        self.lock().staged.retain(|staged| {
            keep.iter().any(|(identity, at, looked_at)| {
                identity == &staged.identity && at == &staged.installed && *looked_at
            })
        });
        // The checks run a few packages at a time ([`AT_ONCE`]), each on
        // its own thread over the updater's shared state, the outcomes
        // merging per few in the order the packages were checked — so the
        // record stays in the installed list's order however they
        // completed ([`Pass::in_order_of`] settles it anyway).
        let of_an_asked_pass = asked.is_some();
        for few in candidates.chunks(AT_ONCE) {
            if few.len() == 1 {
                let mut outcome = Pass::part(of_an_asked_pass);
                self.check_one(launcher, &few[0], &mut outcome);
                pass.merge(outcome);
                continue;
            }
            let outcomes = std::thread::scope(|scope| {
                let running: Vec<_> = few
                    .iter()
                    .map(|package| {
                        scope.spawn(move || {
                            let mut outcome = Pass::part(of_an_asked_pass);
                            self.check_one(launcher, package, &mut outcome);
                            outcome
                        })
                    })
                    .collect();
                running
                    .into_iter()
                    .map(|run| run.join().expect("a check panicked"))
                    .collect::<Vec<Pass>>()
            });
            for outcome in outcomes {
                pass.merge(outcome);
            }
        }
        let checked_at;
        {
            let mut state = self.lock();
            checked_at = state.clock.now();
            state.next_check = checked_at.saturating_add(CHECK_EVERY.as_millis() as u64);
            // A pass asked for while this check ran (a second press): it
            // is still waiting to begin, so the next look runs it rather
            // than a day away.
            if state.asked.is_some() {
                state.next_check = 0;
            }
            state.toast = toast;
        }
        // The pass it is from now on: what applies afterwards (now, or
        // once a deferred update's package is quiet) lands in it.
        self.lock().pass = Some(pass.clone());
        launcher.note_update_pass(&pass);
        // When this check ran: what "Last checked …" says, whatever the
        // pass came to — the check itself is done.
        launcher.note_extension_check(checked_at);
        self.settled.checked_pass();
        asked
    }

    /// Checks one installed package for a newer version and stages what it
    /// finds, collecting the outcome in `pass`: the metadata alone says
    /// what the latest is (the registry's for an npm package, the
    /// repository's reference listing for a Git one, and its release tags
    /// for a default extension), and what is newer is downloaded and
    /// checked as an install checks a package. May run on any of the
    /// pass's few check threads (see [`Updates::check`]); each request is
    /// bounded by Pane's HTTP limits as ever.
    fn check_one(&self, launcher: &Launcher, package: &InstalledPackage, pass: &mut Pass) {
        if let Some(npm) = package.npm.clone() {
            self.check_npm(launcher, package, npm, pass);
        } else if let Some(git) = package.git.clone() {
            self.check_git(launcher, package, git, pass);
        } else if let Some(default) = package.default.clone() {
            self.check_default(launcher, package, default, pass);
        }
    }

    /// Checks one installed npm package for a newer version and stages
    /// what it finds, collecting the outcome in `pass`. The registry's
    /// metadata alone says what the latest version is: nothing is fetched
    /// while that is the version installed.
    fn check_npm(
        &self,
        launcher: &Launcher,
        package: &InstalledPackage,
        npm: npm::NpmPackage,
        pass: &mut Pass,
    ) {
        let registry = self.lock_sources().registry.clone();
        match npm::latest_version(&registry, &npm.name) {
            Err(reason) => {
                pass.failed(
                    package.identity.clone(),
                    package.title(),
                    &format!("It was not checked for a newer version: {reason}"),
                );
            }
            Ok(latest) => {
                // Up to date, or staged for that version already:
                // nothing is fetched.
                let staged_at_latest = self.lock().staged.iter().any(|staged| {
                    staged.identity == package.identity
                        && staged
                            .package
                            .npm
                            .as_ref()
                            .is_some_and(|origin| origin.package.version == latest)
                });
                if latest == npm.version || staged_at_latest {
                    return;
                }
                let spec = crate::npm::NpmSpec {
                    name: npm.name.clone(),
                    // No version: the latest, and not pinned by the update.
                    version: None,
                };
                let request = install::Request::Npm(spec);
                if let Some(staged) = self.stage(launcher, request, package, pass) {
                    self.lock().staged.push(staged);
                }
            }
        }
    }

    /// Checks one installed Git package for the newer commit of its
    /// tracked branch and stages what it finds, collecting the outcome in
    /// `pass`. The repository's reference listing alone says what its
    /// branch points to now: nothing is fetched while that is the commit
    /// installed.
    fn check_git(
        &self,
        launcher: &Launcher,
        package: &InstalledPackage,
        git: git::InstalledGit,
        pass: &mut Pass,
    ) {
        // The repository as the installed copy's record names it, fetched
        // from where it was fetched before; a record Pane cannot read
        // names no repository.
        let spec = match git::GitSpec::parse(&git.url) {
            Ok(spec) => spec,
            Err(reason) => {
                pass.failed(
                    package.identity.clone(),
                    package.title(),
                    &format!("It was not checked for a newer version: {reason}"),
                );
                return;
            }
        };
        // The tracked reference the copy was installed from (a pinned
        // revision is never a candidate: `eligible` filters it out).
        let asked_as = git.revision.asked_as();
        match git::resolve_reference(&spec.repository, asked_as.as_deref()) {
            Err(reason) => {
                pass.failed(
                    package.identity.clone(),
                    package.title(),
                    &format!("It was not checked for a newer version: {reason}"),
                );
            }
            Ok(revision) => {
                // The branch has not moved, or that commit is staged
                // already: nothing is fetched.
                let already_staged = self.lock().staged.iter().any(|staged| {
                    staged.identity == package.identity
                        && staged
                            .package
                            .git
                            .as_ref()
                            .is_some_and(|origin| origin.revision.commit == revision.commit)
                });
                if revision.commit == git.revision.commit || already_staged {
                    return;
                }
                // The tracked branch again, at whatever commit it has
                // moved to: fetched and checked as an install checks a
                // package.
                let spec = git::GitSpec {
                    reference: asked_as,
                    ..spec
                };
                let request = install::Request::Git(spec);
                if let Some(staged) = self.stage(launcher, request, package, pass) {
                    self.lock().staged.push(staged);
                }
            }
        }
    }

    /// Checks one installed default extension for a newer release of its
    /// repository and stages what it finds, collecting the outcome in
    /// `pass`. The repository's reference listing alone says what its
    /// release tags are: nothing is fetched while the newest names the
    /// version installed. A newer tag's commit is fetched, checked and
    /// staged exactly as first setup acquires one — the same retries, the
    /// default identity and the tag's own origin recorded — so the
    /// record's Git fields name where the update came from
    /// ([`crate::defaults`]).
    fn check_default(
        &self,
        launcher: &Launcher,
        package: &InstalledPackage,
        default: InstalledDefault,
        pass: &mut Pass,
    ) {
        let Some(id) = package.identity.default_id() else {
            return;
        };
        // The repository as the installed copy's record names it, fetched
        // from where the revision it was installed from was fetched; a
        // record Pane cannot read names no repository (the package is not
        // a candidate then).
        let spec = match git::GitSpec::parse(&default.repository) {
            Ok(spec) => spec,
            Err(reason) => {
                pass.failed(
                    package.identity.clone(),
                    package.title(),
                    &format!("It was not checked for a newer version: {reason}"),
                );
                return;
            }
        };
        let tags = match git::release_tags(&spec.repository) {
            Ok(Some(tags)) => tags,
            // A listing longer than Pane reads: the newest release cannot
            // be told, so the check fails rather than guess.
            Ok(None) => {
                pass.failed(
                    package.identity.clone(),
                    package.title(),
                    &format!(
                        "It was not checked for a newer version: Could not list the \
                         references of {}: {}",
                        spec.repository.name(),
                        git::TOO_LARGE
                    ),
                );
                return;
            }
            Err(reason) => {
                pass.failed(
                    package.identity.clone(),
                    package.title(),
                    &format!("It was not checked for a newer version: {reason}"),
                );
                return;
            }
        };
        // The version installed, as the record keeps it, and the newest
        // release tag above it — tags newest first. Nothing is fetched
        // while the newest tag names that version (or an older one: a
        // repository that moved its tags backwards is not followed down).
        // A record that keeps no version to compare names none newer.
        let installed = installed_version(&default).unwrap_or_default();
        let Some(found) = tags
            .iter()
            .find(|tag| git::is_newer_release(tag.version(), &installed))
        else {
            return;
        };
        // That tag's commit is the one installed, or is staged already:
        // nothing is fetched.
        let already_staged = self.lock().staged.iter().any(|staged| {
            staged.identity == package.identity
                && staged
                    .package
                    .default
                    .as_ref()
                    .is_some_and(|origin| origin.revision.commit == found.commit)
        });
        if found.commit == default.revision.commit || already_staged {
            return;
        }
        // The revision to fetch: the default's identity and title, its
        // repository, and the newer tag with the commit it points to —
        // a pin in the update's shape, staged as first setup acquires one.
        // No platform is named: the update fetches a default this system
        // already set up, so the pins file's platform gate is not its
        // business (a default of another system is never installed, so
        // never updated).
        let request = install::Request::Default(DefaultExtension {
            id: id.to_owned(),
            title: package.title(),
            repository: default.repository.clone(),
            tag: found.tag.clone(),
            commit: found.commit.clone(),
            platform: None,
        });
        if let Some(staged) = self.stage(launcher, request, package, pass) {
            self.lock().staged.push(staged);
        }
    }

    /// Downloads and checks what `request` names — the latest version of
    /// an npm package, the moved commit of a Git package's tracked
    /// branch, or a default extension's newer release tag — as an install
    /// checks a package, working out what it means for its dependencies,
    /// against the installed copy `installed`, and collects the outcome
    /// in `pass`: the update to stage, or `None` with the pass recording
    /// why the installed copy stays — a newer version that needs a newer
    /// Pane or is not available on this system skipped with that reason,
    /// anything else failed. Blocks on the network and the checks; runs
    /// no code of the package.
    fn stage(
        &self,
        launcher: &Launcher,
        request: install::Request,
        installed: &InstalledPackage,
        pass: &mut Pass,
    ) -> Option<Staged> {
        let title = installed.title();
        // The updater's own sources: a WeakLauncher keeps the launcher's
        // as they were when it was made, before a development build's
        // registry was given.
        let sources = self.lock_sources().clone();
        let checked = futures::executor::block_on(launcher.read_and_check_from(sources, request));
        let package = match checked {
            Ok(package) => package,
            Err(reason) => {
                // A newer version this Pane cannot run, or that does not
                // work on this system, is skipped with that reason:
                // skipping never looks like a fault. Anything else — the
                // source could not be reached, the download did not match
                // its integrity, the revision holds only the source —
                // failed, and the installed copy keeps running.
                match &reason {
                    PackageError::IncompatibleApi(_) | PackageError::NewerManifest(_) => {
                        // The reason points the user at Pane's own update
                        // when one exists, so they know what to do.
                        pass.refused(
                            installed.identity.clone(),
                            title,
                            &format!("{reason}{}", application_update_note(launcher)),
                        );
                    }
                    PackageError::UnsupportedPlatform(_) => {
                        pass.refused(installed.identity.clone(), title, &reason.to_string());
                    }
                    _ => {
                        pass.failed(
                            installed.identity.clone(),
                            title,
                            &format!("It was not updated: {reason}"),
                        );
                    }
                }
                return None;
            }
        };
        // What the update stages the copy at: the version of an npm
        // package, the revision a Git one fetched, the release a default
        // extension takes.
        let default_release = package
            .default
            .as_ref()
            .map(|origin| default_says(package.manifest.version.clone(), &origin.revision));
        let staged_revision = package
            .npm
            .as_ref()
            .map(|origin| origin.package.version.clone())
            .or_else(|| {
                package
                    .git
                    .as_ref()
                    .map(|origin| origin.revision.describe())
            })
            .or(default_release)
            .unwrap_or_default();
        let sources = self.lock_sources().clone();
        let (package, plan) =
            futures::executor::block_on(launcher.plan_dependencies_from(sources, package));
        if !plan.problems.is_empty() {
            // The same checks an install makes, with the same words: what
            // the newer version needs is not there.
            let problems: Vec<String> = plan.problems.iter().map(ToString::to_string).collect();
            pass.failed(
                installed.identity.clone(),
                title,
                &format!(
                    "It was not updated to {staged_revision}: {}",
                    problems.join("; ")
                ),
            );
            return None;
        }
        Some(Staged {
            identity: installed.identity.clone(),
            asked: pass.is_asked(),
            installed: installed_at(installed).unwrap_or_default(),
            package,
            plan,
        })
    }

    /// Applies every staged update whose package is at the safe boundary,
    /// deferring the others (see the module documentation). A pass the
    /// user asked for says how far its applies have got, through the
    /// toast that pass shows ("Updating N of M…", see `update_results`).
    fn apply(&self, launcher: &Launcher) {
        let (staged, toast) = {
            let mut state = self.lock();
            (std::mem::take(&mut state.staged), state.toast)
        };
        let total = staged.len();
        for (done, update) in staged.into_iter().enumerate() {
            if let Some(id) = toast
                && total > 0
            {
                let mut state = launcher.lock();
                launcher.update_pass_progress(&mut state, id, done + 1, total);
            }
            match self.apply_one(launcher, update) {
                Applied::Yes => {}
                Applied::Deferred(update) => {
                    // A package in use: the update waits, and its row in
                    // the pass's record says so until it applies (see
                    // `Pass::waiting`).
                    self.note_waiting(launcher, &update.identity);
                    self.lock().staged.push(*update);
                }
                Applied::Dropped => {}
            }
        }
    }

    /// Notes that the staged update of `identity` waits for the package
    /// to grow quiet, in the pass now running (a later look retries the
    /// apply without re-noting, `Pass::waiting` saying nothing new), and
    /// records the pass: the waiting row is the deferred apply's say,
    /// where it was silent before.
    fn note_waiting(&self, launcher: &Launcher, identity: &PackageIdentity) {
        let pass = {
            let title = {
                let state = launcher.lock();
                state.title_of(identity)
            };
            let mut checking = self.lock();
            let Some(pass) = checking.pass.as_mut() else {
                return;
            };
            pass.waiting(identity.clone(), title).then(|| pass.clone())
        };
        if let Some(pass) = pass {
            launcher.note_update_pass(&pass);
            launcher.changed();
        }
    }

    /// Notes what applying one staged update came to — `came` — in the
    /// pass now running, and records the pass (see
    /// [`Launcher::note_update_pass`]): the pass's checks ended, but what
    /// it staged lands in it as it applies, also once a deferred update's
    /// package grows quiet in a later look.
    fn note(&self, launcher: &Launcher, came: Came) {
        let pass = {
            let mut checking = self.lock();
            let Some(pass) = checking.pass.as_mut() else {
                return;
            };
            came.add(pass);
            pass.clone()
        };
        launcher.note_update_pass(&pass);
    }

    /// Applies one staged update: `Yes` when it replaced the installed
    /// copy (or was applied and its failure recorded), `Deferred` when
    /// the package is in use or busy, so it is tried again, and `Dropped`
    /// when it is not for the package as it is now.
    fn apply_one(&self, launcher: &Launcher, update: Staged) -> Applied {
        let identity = update.identity.clone();
        // What the update installs, checked as it was staged: the version
        // of an npm package, the commit of a Git one or of a default
        // extension's release.
        let target = staged_at(&update.package).unwrap_or_default();
        let from = update.installed.clone();
        // What an Updated row says of the old and the new revision: the
        // version of an npm package, a Git commit id as people read it,
        // and a default extension's old and new version — what its
        // record declared and its release tags name.
        let mut says = (revision(&from), revision(&target));
        let mut claimed;
        {
            let mut state = launcher.lock();
            let Some(installed) = state.package(&identity) else {
                return Applied::Dropped;
            };
            if let (Some(old), Some(new)) = (&installed.default, update.package.default.as_ref()) {
                says = (
                    default_says(old.version.clone(), &old.revision),
                    default_says(update.package.manifest.version.clone(), &new.revision),
                );
            }
            // Not the copy this was staged for, or already there: the next
            // check plans again.
            let Some(at) = installed_at(installed) else {
                return Applied::Dropped;
            };
            if at != update.installed || at == target {
                return Applied::Dropped;
            }
            // The user turned updates off for it, pinned, disabled or
            // paused it, or something else is happening to it: not now —
            // unless this was staged by a pass the user asked for, where
            // turning updates off since does not undo the ask; only the
            // package no longer being one a pass could look at does (it
            // was pinned, replaced from another source, uninstalled).
            if state.changing.contains_key(&identity) {
                return Applied::Dropped;
            }
            let updatable = if update.asked {
                eligible_when_asked(installed)
            } else {
                eligible(&state, installed)
            };
            if !updatable {
                return Applied::Dropped;
            }
            if !quiet(launcher, &state, installed) {
                return Applied::Deferred(Box::new(update));
            }
            // Claimed as an update, at the boundary: from here the
            // replacement is on its way, and no call the user makes into
            // the package is started (see `Launcher::open_command`).
            match install::claim(
                &mut state,
                &Mode::Update(identity.clone()),
                &update.plan.assumptions,
                Changing::BackgroundUpdating,
            ) {
                Ok(claims) => claimed = claims,
                Err(install::Refusal::Busy(_)) => return Applied::Deferred(Box::new(update)),
                Err(install::Refusal::Changed) => return Applied::Dropped,
            }
        }
        // Claimed, with the package not yet replaced: where a test that
        // holds the updates asks things of it (never held otherwise).
        self.hold.wait();
        let store = launcher
            .installation
            .as_ref()
            .expect("an eligible package is installed")
            .store
            .clone();
        let result = futures::executor::block_on(launcher.install_plan(
            store,
            update.package,
            update.plan,
            &Mode::Update(identity.clone()),
            None,
            &mut claimed,
        ));
        let mut state = launcher.lock();
        for identity in &claimed {
            state.release(identity);
        }
        // What applying came to, for the pass's record: said once the
        // state is unlocked (the record replaces it). Nothing is said on
        // the status line — the record and, for a failure, the
        // announcement are the outcome.
        let came = match result {
            Ok(outcome) => {
                for dependency in outcome.dependencies {
                    launcher.put_installed(&mut state, dependency);
                }
                let package = outcome.package;
                let first = package.commands().first().map(|c| c.component.clone());
                let replaced_is_open = launcher.put_installed(&mut state, package);
                if replaced_is_open {
                    // A command of the replaced copy opened in the moment
                    // between the boundary check and the replacement.
                    launcher.show_root(&mut state, first);
                } else {
                    // The list and root search show the new version's
                    // commands; another screen keeps its own status.
                    launcher.refresh(&mut state);
                }
                Some(Came::Updated {
                    identity: identity.clone(),
                    title: state.title_of(&identity),
                    from: says.0,
                    to: says.1,
                })
            }
            // What the update needs changed while it was being applied:
            // the next check plans again.
            Err(Stopped::Changed(_)) => return Applied::Yes,
            Err(Stopped::Failed(failure)) => {
                let message = launcher.install_left_behind(&mut state, &failure);
                launcher.refresh(&mut state);
                Some(Came::Failed {
                    identity: identity.clone(),
                    title: state.title_of(&identity),
                    why: format!("It was not updated: {message}"),
                })
            }
        };
        launcher.changed();
        drop(state);
        if let Some(came) = came {
            self.note(launcher, came);
        }
        Applied::Yes
    }
}

/// What applying one staged update came to, for the pass's record: added
/// to the pass ([`Updates::note`]) once the launcher's state is unlocked.
enum Came {
    /// The package was updated, from `from` to `to`.
    Updated {
        identity: PackageIdentity,
        title: String,
        from: String,
        to: String,
    },
    /// It failed, `why` saying what failed.
    Failed {
        identity: PackageIdentity,
        title: String,
        why: String,
    },
}

impl Came {
    /// Adds this outcome to the pass now running.
    fn add(self, pass: &mut Pass) {
        match self {
            Came::Updated {
                identity,
                title,
                from,
                to,
            } => pass.updated(identity, title, &from, &to),
            Came::Failed {
                identity,
                title,
                why,
            } => pass.failed(identity, title, &why),
        }
    }
}

/// Whether `package` is one Pane updates by itself: installed from npm
/// and not pinned (a pinned version is kept, whatever the latest is),
/// installed from Git and tracked (a tag or commit named to install it is
/// pinned and kept; an update follows the branch), or a default extension
/// whose record keeps its repository — and in every case enabled, not
/// paused after a failure, and not turned off by the user's controls (the
/// global one first).
fn eligible(state: &State, package: &InstalledPackage) -> bool {
    from_a_source_pane_updates(package)
        && package.enabled
        && !state.paused.is_paused(&package.identity)
        && state.update_controls.automatic
        && !state.update_controls.off.contains(&package.identity.key())
}

/// Whether `package` is one a pass the user asked for looks at: from a
/// source Pane updates, whatever the user's controls and the package's
/// state — the user asked, so "update all" means all. An update keeps a
/// disabled package disabled, and unpauses a paused one, as the
/// preview's Update row does; only a pinned version or revision, a local
/// folder's or a development copy's code stays untouched, as ever.
pub(in crate::launcher) fn eligible_when_asked(package: &InstalledPackage) -> bool {
    from_a_source_pane_updates(package)
}

/// Whether the source `package` was installed from is one Pane updates:
/// npm without a pin, Git with a tracked reference, or a default
/// extension whose record keeps its repository. A default's `pinned`
/// records what first setup installed — the release tag this Pane release
/// pinned — never a choice of the user's, so it never keeps a default
/// from updating; a default whose record keeps no repository (an older
/// Pane acquired it from Pane's own downloads) is not one Pane updates.
fn from_a_source_pane_updates(package: &InstalledPackage) -> bool {
    match (&package.npm, &package.git, &package.default) {
        (Some(npm), _, _) => !npm.pinned,
        (None, Some(git), _) => !git.revision.pinned(),
        (None, None, Some(_)) => true,
        (None, None, None) => false,
    }
}

/// Whether the only reason a pass did not look at `package` is the
/// user's switch: its automatic updates are turned off (globally or for
/// it) and everything else would let it update. What
/// [`crate::Launcher::update_results_row_actions`] offers Update Now on
/// turns on as the controls do.
pub(in crate::launcher) fn only_the_switch(state: &State, package: &InstalledPackage) -> bool {
    eligible_when_asked(package)
        && package.enabled
        && !state.paused.is_paused(&package.identity)
        && !state.changing.contains_key(&package.identity)
        && (!state.update_controls.automatic
            || state.update_controls.off.contains(&package.identity.key()))
}

/// The Skipped row for the installed `package`, which the pass does not
/// look at (the reverse of [`eligible`] — for a pass the user asked for,
/// of [`eligible_when_asked`] — with the words the record keeps): its
/// identity, its title and why the pass did not look. `None` when the
/// pass looks at it, or has nothing to say of it. A pass the user asked
/// for looks at turned-off, disabled and paused packages too, so those
/// are not its skip reasons.
fn skipped_row(
    launcher: &Launcher,
    state: &State,
    package: &InstalledPackage,
    asked: bool,
) -> Option<(PackageIdentity, String, String)> {
    // Something else is being done to it right now: the next check
    // catches it.
    if let Some(what) = state.changing.get(&package.identity) {
        return Some((
            package.identity.clone(),
            package.title(),
            format!("It {}", what.doing()),
        ));
    }
    // From a source this pass does not update: a local folder's or a
    // development copy's code is never replaced here, and neither is a
    // default extension whose record keeps no repository — an older Pane
    // acquired it from Pane's own downloads, which recorded none. (A
    // default whose record keeps its repository falls through to the
    // pass's own reasons, as an unpinned npm package does.)
    if package.npm.is_none() && package.git.is_none() && package.default.is_none() {
        if package.identity.default_id().is_some() {
            return Some((
                package.identity.clone(),
                package.title(),
                "It was installed by an older Pane, which kept no repository for it".into(),
            ));
        }
        return Some((
            package.identity.clone(),
            package.title(),
            if launcher.is_developed(&package.identity) {
                "It is a development copy".into()
            } else {
                "It is a local copy, from a folder".into()
            },
        ));
    }
    if package.npm.as_ref().is_some_and(|npm| npm.pinned) {
        return Some((
            package.identity.clone(),
            package.title(),
            "Its version is pinned".into(),
        ));
    }
    if package
        .git
        .as_ref()
        .is_some_and(|git| git.revision.pinned())
    {
        return Some((
            package.identity.clone(),
            package.title(),
            "Its revision is pinned".into(),
        ));
    }
    if asked {
        // The pass the user asked for looks at everything else: a
        // turned-off, disabled or paused package is updated by it, not
        // skipped.
        return None;
    }
    if !package.enabled {
        return Some((
            package.identity.clone(),
            package.title(),
            "It is disabled".into(),
        ));
    }
    if state.paused.is_paused(&package.identity) {
        return Some((
            package.identity.clone(),
            package.title(),
            "It is paused after an error".into(),
        ));
    }
    if !state.update_controls.automatic {
        return Some((
            package.identity.clone(),
            package.title(),
            "Automatic updates of extensions are off".into(),
        ));
    }
    if state.update_controls.off.contains(&package.identity.key()) {
        return Some((
            package.identity.clone(),
            package.title(),
            "Automatic updates of it are off".into(),
        ));
    }
    None
}

/// What marks the revision `package` is installed at: the version of an
/// npm package, the commit of a Git one, the commit of a default
/// extension's revision (as its download's identity does); `None` for a
/// package from a local folder, which the updater never replaces.
fn installed_at(package: &InstalledPackage) -> Option<String> {
    package
        .npm
        .as_ref()
        .map(|npm| npm.version.clone())
        .or_else(|| package.git.as_ref().map(|git| git.revision.commit.clone()))
        .or_else(|| {
            package
                .default
                .as_ref()
                .map(|default| default.revision.commit.clone())
        })
}

/// What marks the revision a staged `package` would install: the version
/// of an npm package, the commit of a Git one or of a default extension's
/// release.
fn staged_at(package: &SourcePackage) -> Option<String> {
    package
        .npm
        .as_ref()
        .map(|origin| origin.package.version.clone())
        .or_else(|| {
            package
                .git
                .as_ref()
                .map(|origin| origin.revision.commit.clone())
        })
        .or_else(|| {
            package
                .default
                .as_ref()
                .map(|origin| origin.revision.commit.clone())
        })
}

/// The version a default extension is installed at, as its record keeps
/// it: the version its manifest declared, or — when the manifest declared
/// none — its release tag's own. `None` when the record keeps neither:
/// no version to compare, so no release is newer.
fn installed_version(default: &InstalledDefault) -> Option<String> {
    default
        .version
        .clone()
        .or_else(|| tag_version(&default.revision))
}

/// The version a default extension's release tag names: `refs/tags/v0.2.0`
/// is `0.2.0`; `None` for a revision that is not a tagged release.
fn tag_version(of: &GitRevision) -> Option<String> {
    of.ref_name()
        .and_then(|name| name.strip_prefix("refs/tags/v").map(ToOwned::to_owned))
}

/// What a failure's message and an Updated row say of a default
/// extension's revision: the version its manifest declares or its release
/// tag names — the commit, as people read it, when its tag names no
/// version.
fn default_says(version: Option<String>, of: &GitRevision) -> String {
    version
        .or_else(|| tag_version(of))
        .unwrap_or_else(|| revision(&of.commit))
}

/// What the refusal of a newer version that needs a newer Pane appends
/// when Pane's own application update exists: pointing the user at it,
/// the reason's own help. Nothing when no offer exists or no release
/// source is configured.
fn application_update_note(launcher: &Launcher) -> String {
    match launcher.application_update() {
        ApplicationUpdate::Offered { version, .. } | ApplicationUpdate::Installing { version } => {
            format!(" Pane {version} is available: update Pane itself.")
        }
        ApplicationUpdate::Installed { version } => {
            format!(" Pane {version} runs from the next start.")
        }
        _ => String::new(),
    }
}

/// Whether `package` is at the safe boundary for replacing its code: no
/// screen of one of its commands is on display, and no call of it the
/// user asked for is running. Managed background work is not waited for
/// (a replacement ends it with the generation and the new code starts it
/// again); nor are the calls other packages make into it, which answer
/// that the package was updated and may be made again.
fn quiet(launcher: &Launcher, state: &State, package: &InstalledPackage) -> bool {
    // A command of the package on display: its items, searches, forms and
    // views run while it is open, and the user is in it.
    if state
        .open
        .as_ref()
        .is_some_and(|open| open.starts_with(&package.location))
    {
        return false;
    }
    // A call the user asked for that has not answered: opening a command,
    // running an item, a query sent from root, a search, a form.
    match launcher.runtime() {
        Ok(runtime) => {
            let busy = runtime.busy();
            !package
                .commands()
                .iter()
                .any(|command| busy.contains(&command.component))
        }
        Err(_) => true,
    }
}

/// What an Updated row says for the revision `at`: an npm version as it
/// is, a Git commit id as people read it, its first 12 digits.
fn revision(at: &str) -> String {
    if at.len() == 40 && at.chars().all(|c| c.is_ascii_hexdigit()) {
        at[..12].to_owned()
    } else {
        at.to_owned()
    }
}

/// What applying one staged update came to.
enum Applied {
    /// The installed copy was replaced (or the update was applied and its
    /// failure explained).
    Yes,
    /// The package is in use; the update is tried again. Boxed, so the
    /// small outcomes carry no staged update's size.
    Deferred(Box<Staged>),
    /// The update is not for the package as it is now; it is dropped.
    Dropped,
}

/// The updater thread: looks when work may be due, then waits until
/// something is, this Pane stops, or the clock or the user's controls
/// change.
fn update_until_stopped(launcher: WeakLauncher, updates: Weak<Updates>) {
    while let Some(current) = updates.upgrade() {
        let Some(seen) = current.settled.begin_look() else {
            return;
        };
        if let Some(launcher) = launcher.upgrade() {
            current.pass(&launcher);
        }
        current.settled.ended(seen);
        let wait = current.next_wait();
        if !current.settled.wait(seen, wait) {
            return;
        }
    }
}

/// A closure that pokes the updater `weak` names awake, holding it weakly
/// so that a clock does not keep it running past the launcher. For
/// [`Clock::on_change`], which asks for another look when the clock is
/// set.
fn poking(weak: Weak<Updates>) -> impl Fn() + Send + Sync + 'static {
    move || {
        if let Some(updates) = weak.upgrade() {
            updates.settled.poke();
        }
    }
}

/// Wakes the updater thread when work may be due, and tells when it
/// settled: every poke so far was looked at, and a check pass after a
/// given one completed, applying or deferring everything it staged.
#[derive(Default)]
struct Settled {
    state: Mutex<Settling>,
    condvar: std::sync::Condvar,
}

#[derive(Default)]
struct Settling {
    stopped: bool,
    /// Counts pokes: each asks the updater to look again.
    pokes: u64,
    /// Looks the updater has begun. A look in flight is one of these the
    /// updater has not ended yet, so waiting for it to end is what tells
    /// the updater is quiet, not only that the pokes so far were begun
    /// before some earlier look.
    looks: u64,
    /// Looks the updater has ended.
    done: u64,
    /// The most pokes any ended look covered.
    covered: u64,
    /// Checks the updater completed.
    checks: u64,
}

impl Settled {
    fn lock(&self) -> MutexGuard<'_, Settling> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Asks the updater to look again.
    fn poke(&self) {
        self.lock().pokes += 1;
        self.condvar.notify_all();
    }

    /// Stops the updater.
    fn stop(&self) {
        self.lock().stopped = true;
        self.condvar.notify_all();
    }

    /// Begins a look, returning the pokes it is to cover, or `None` once
    /// stopped.
    fn begin_look(&self) -> Option<u64> {
        let mut state = self.lock();
        if state.stopped {
            return None;
        }
        state.looks += 1;
        Some(state.pokes)
    }

    /// Notes that the look covering the first `seen` pokes ended.
    fn ended(&self, seen: u64) {
        let mut state = self.lock();
        state.done += 1;
        state.covered = state.covered.max(seen);
        self.condvar.notify_all();
    }

    /// Notes that a check pass completed.
    fn checked_pass(&self) {
        self.lock().checks += 1;
        self.condvar.notify_all();
    }

    /// Waits at most `limit` for a poke after the `seen`th; returns
    /// whether the updater still runs.
    fn wait(&self, seen: u64, limit: Duration) -> bool {
        let state = self.lock();
        let (state, _) = self
            .condvar
            .wait_timeout_while(state, limit, |state| !state.stopped && state.pokes == seen)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        !state.stopped
    }

    /// Waits until a check pass completed and the updater settled — every
    /// poke so far was covered by a look that ended, and no look is in
    /// flight; `false` if it did not within `limit`, or it stopped.
    fn checked(&self, limit: Duration) -> bool {
        let state = self.lock();
        let (state, _) = self
            .condvar
            .wait_timeout_while(state, limit, |state| {
                !state.stopped
                    && (state.checks < 1 || state.done < state.looks || state.covered < state.pokes)
            })
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        !state.stopped
            && state.checks >= 1
            && state.done >= state.looks
            && state.covered >= state.pokes
    }
}

impl Drop for Updates {
    fn drop(&mut self) {
        self.settled.stop();
    }
}

impl Launcher {
    /// Checks every updatable extension at once and updates what the pass
    /// finds, whatever the cadence: the pass the user asked for, which
    /// root search's "Check for Extension Updates" row and the Settings
    /// Extensions group's "Check for updates" button both start (ADR 0043:
    /// two entry points, one flow). Unlike the automatic pass it includes
    /// extensions whose automatic updates are turned off, and disabled and
    /// paused ones — the user asked — and it answers even when everything
    /// is up to date: a toast follows the pass, ending as its summary with
    /// View Details, and the record holds what it came to. The pass begins
    /// at once, on the updater's own thread; await the returned future for
    /// it to end, as the window that offered it does, so both redraw with
    /// its outcome. A Pane with no installation checks nothing.
    pub fn check_extension_updates(&self) -> impl Future<Output = ()> + Send + 'static {
        self.asked_pass_of(Some(Ask::All))
    }

    /// Checks the extension whose row in the update results has the
    /// identity key `target` for an update, and updates what it finds: a
    /// pass the user asked for whose scope is that one extension, as the
    /// results view's Retry (a Failed row) and Update Now (a Skipped row
    /// skipped only for the user's switch) run it, its outcome landing in
    /// the record and its toast as any asked pass's. Nothing happens when
    /// no row of the record is that key's. See
    /// [`Launcher::check_extension_updates`].
    pub fn check_extension_update_of(
        &self,
        target: &str,
    ) -> impl Future<Output = ()> + Send + 'static {
        let ask = self
            .lock()
            .update_results
            .results
            .row_of(target)
            .map(|row| Ask::One(row.identity.clone()));
        self.asked_pass_of(ask)
    }

    /// Starts the pass the user asked for over `ask` at once — `None`
    /// starts nothing — and returns the future that ends when it has: the
    /// look that ran it ended and the updater settled (bounded by
    /// [`ASKED_PASS_WAIT`], so a window never waits for ever; a pass that
    /// outlives the bound goes on, its toast reaching the window on its
    /// own).
    fn asked_pass_of(&self, ask: Option<Ask>) -> impl Future<Output = ()> + Send + 'static {
        let updates = ask.and_then(|ask| {
            self.updates.as_ref().map(|updates| {
                updates.check_now_asked(ask);
                updates.clone()
            })
        });
        async move {
            if let Some(updates) = updates {
                off_thread(move || updates.checked(ASKED_PASS_WAIT)).await;
            }
        }
    }
}

/// One change of the user's automatic-update controls, as the extension
/// list's rows make it: the global choice, or one package's.
pub(super) struct UpdateToggle {
    /// The controls as they were, to put back if the record cannot be
    /// written.
    before: UpdateControls,
    /// The message for the status line once the choice holds.
    done: String,
}

impl Launcher {
    /// Begins turning automatic updates of every eligible package (with
    /// `None`), or of the package with `identity`, on or off: the choice
    /// is made in Pane now, and [`Launcher::finish_update_toggle`]
    /// records it. No package is updated by this; the updater looks again
    /// once the choice is recorded.
    pub(super) fn begin_update_toggle(
        &self,
        state: &mut State,
        which: Option<PackageIdentity>,
    ) -> Option<UpdateToggle> {
        let before = state.update_controls.clone();
        let done = match &which {
            None => {
                state.update_controls.automatic = !state.update_controls.automatic;
                if state.update_controls.automatic {
                    "Automatic updates of extensions are on".into()
                } else {
                    "Automatic updates of extensions are off".into()
                }
            }
            Some(identity) => {
                let key = identity.key();
                let off = !state.update_controls.off.contains(&key);
                if off {
                    state.update_controls.off.insert(key);
                } else {
                    state.update_controls.off.remove(&key);
                }
                let title = state.title_of(identity);
                if off {
                    format!("Automatic updates of {title} are off")
                } else {
                    format!("Automatic updates of {title} are on")
                }
            }
        };
        // The row's subtitle says the new choice; the choice itself holds
        // once it is recorded.
        self.refresh(state);
        Some(UpdateToggle { before, done })
    }

    /// Records the choice [`Launcher::begin_update_toggle`] began, writing
    /// the record off the calling thread; if it cannot be written, the
    /// choice goes back to what it was and the reason is shown. Turning
    /// updates on asks the updater to check at once.
    pub(super) async fn finish_update_toggle(&self, toggle: UpdateToggle) {
        let UpdateToggle { before, done } = toggle;
        let saved = {
            let (file, text) = {
                let state = self.lock();
                match self.installation.as_ref() {
                    Some(installation) => state.update_controls.text(&installation.dir),
                    None => return,
                }
            };
            let written =
                off_thread(move || write_atomically(&file, text.as_bytes(), Readers::Default));
            written.await.is_ok()
        };
        let mut state = self.lock();
        if !saved {
            state.update_controls = before;
            self.refresh(&mut state);
            state.view.status = Status::Error(
                "Could not keep the choice: Pane could not write the extension list's update \
                 record"
                    .into(),
            );
        } else {
            state.view.status = Status::Result(done);
            // On: the packages it governs are looked at at once. Off: the
            // staged updates of a package turned off are dropped at that
            // look.
            if let Some(updates) = &self.updates {
                updates.check_now();
            }
        }
        self.changed();
    }
}

/// The extension list's rows for the update controls: one per installed
/// npm package that is not pinned, per Git package that is tracked and
/// per default extension whose record keeps its repository (after the
/// reload rows a local package has; an npm or Git package has none),
/// saying whether it updates by itself.
pub(super) fn package_rows(
    packages: &[InstalledPackage],
    off: &HashSet<String>,
) -> Vec<(Row, Entry)> {
    packages
        .iter()
        .filter(|package| {
            package.npm.as_ref().is_some_and(|npm| !npm.pinned)
                || package
                    .git
                    .as_ref()
                    .is_some_and(|git| !git.revision.pinned())
                || package.default.is_some()
        })
        .map(|package| {
            let identity = package.identity.clone();
            let title = package.title();
            let off = off.contains(&identity.key());
            // What the row says is replaced: the newer npm version of an
            // npm package, the newer commit of the tracked branch of a
            // Git one, the newer release of a default extension's
            // repository.
            let newer = if package.default.is_some() {
                "a newer release of its repository"
            } else if package.git.is_some() {
                "a newer commit of its tracked branch"
            } else {
                "a compatible newer npm version"
            };
            let row = Row {
                id: format!("updates:{}", identity.key()),
                title: format!("Update {title} automatically"),
                subtitle: Some(if off {
                    format!(
                        "Off · replace it yourself with Update on its preview · {}",
                        identity
                    )
                } else {
                    format!(
                        "On · {newer} replaces it once no command of it runs · {}",
                        identity
                    )
                }),
                unavailable: None,
            };
            (row, Entry::ToggleUpdates(Some(identity)))
        })
        .collect()
}

/// The extension list's row for the global choice, last of all: after
/// every package's rows, as `extension_rows` places it.
pub(super) fn global_row(automatic: bool) -> (Row, Entry) {
    let row = Row {
        id: "updates".into(),
        title: "Update extensions automatically".into(),
        subtitle: Some(if automatic {
            "On · every eligible extension updates by itself, at its source's newer version".into()
        } else {
            "Off · no extension updates by itself; choose Update on a package's preview".into()
        }),
        unavailable: None,
    };
    (row, Entry::ToggleUpdates(None))
}
