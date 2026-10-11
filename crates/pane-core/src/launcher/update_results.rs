//! The update results of the updater's passes (#256): what each pass
//! updated, skipped and failed, per package, kept as Pane's own record
//! beside `updates.json` (`update-results.json`) so the latest pass that
//! found something new survives a restart, and shown as a screen of the
//! launcher's ([`crate::Launcher::update_results`], the same record).
//!
//! The record is written by the updater's own thread as a pass collects
//! its outcomes (see `updates`): a pass that updated, failed or skipped
//! something it found replaces what the record held, and a pass that
//! found nothing new — every package up to date, or skipped before the
//! pass looked at it — keeps it. A pass the user asked for records
//! whatever it came to (the user asked; its rows are its answer). Each
//! row names the package, its title and what came of it: an update says
//! the old and new version or commit, a skip says why (a pinned version
//! or revision, a local or development copy, automatic updates off,
//! disabled, paused, a newer version that needs a newer Pane or is not
//! available on this system), an update still waiting for its package to
//! grow quiet says so, and a failure says why, ending that the extension
//! keeps running its installed code. The record also keeps when the
//! updater last checked, which the Settings Extensions group's page
//! says under its "Check for updates" button.
//!
//! **The announcement.** A background pass that failed something is
//! announced once, the next time the launcher is shown: a failure toast
//! ("1 extension update failed") carrying View Details, which opens the
//! results screen. Successes and skips stay quiet. The announcement is
//! keyed on the record's pass, so a new failing pass re-arms it; it is
//! not repeated for the same one. A pass the user asked for announces
//! its own failures — its ending toast (see
//! [`Launcher::begin_update_pass_toast`]) is the announcement, so the
//! background one stays silent.
//!
//! **The view.** The screen [`crate::Screen::UpdateResults`] lists the
//! groups in the order Updated, Waiting, Skipped, Failed, hiding empty
//! ones, each row the extension's icon, title, detail and a status tag,
//! searched by what the user types (see [`crate::Launcher::set_query`])
//! and driven by the launcher's own list keys. Each row's entry opens
//! its extension's page in Settings; the Actions panel offers that,
//! copying the details, Retry on a Failed row (which checks that
//! extension alone) and Update Now on a Skipped row whose only reason is
//! the user's switch. The window draws it (see `pane`'s
//! `features::update_results`); Pane's Settings window draws it in place
//! of the Extensions page when its operation opens it (ADR 0043).

use std::path::PathBuf;

use super::{Entry, Launcher, LauncherView, Row, Screen, State, Status, first_index, updates};
use crate::atomic::{Readers, write_atomically};
use crate::feedback::{Toast, ToastAction, ToastDoes, ToastStyle, WindowPresence};
use crate::packages::{PackageIdentity, Pause};

/// The file the record is kept in, beside `updates.json`.
const FILE: &str = "update-results.json";

/// What every failure's detail ends with: the extension keeps running the
/// code it has installed, so a failure never looks like a loss.
pub(in crate::launcher) const KEEPS_RUNNING: &str = "It keeps running its installed code.";

/// The title of the failure that announces a pass's failures, for `failed`
/// of them.
fn failure_title(failed: usize) -> String {
    match failed {
        1 => "1 extension update failed".into(),
        failed => format!("{failed} extension updates failed"),
    }
}

/// The asked pass's ending toast, for the `updated` extensions it updated
/// and the `failed` it failed: a success that says what was updated, or
/// that everything is up to date — the user asked, so up to date is an
/// answer, not a silence — or a failure that says how many failed. It
/// carries View Details, which opens the results view. What the pass
/// deferred (a package in use) is in the record, waiting, and not counted
/// here: the toast ends when the pass has nothing more it can do now.
fn ending_toast(updated: usize, failed: usize) -> Toast {
    let extensions = |count: usize| {
        if count == 1 {
            "extension"
        } else {
            "extensions"
        }
    };
    let (style, title) = match (updated, failed) {
        (0, 0) => (ToastStyle::Success, "Extensions are up to date".into()),
        (updated, 0) => (
            ToastStyle::Success,
            format!("Updated {updated} {}", extensions(updated)),
        ),
        (0, failed) => (ToastStyle::Failure, failure_title(failed)),
        (updated, failed) => (
            ToastStyle::Failure,
            format!("Updated {updated} {}, {failed} failed", extensions(updated)),
        ),
    };
    Toast {
        style,
        title,
        message: None,
        primary: Some(ToastAction {
            title: VIEW_DETAILS.into(),
            shortcut: None,
            unbound: None,
            does: ToastDoes::ShowUpdateResults,
        }),
        secondary: None,
    }
}

/// The toast action that opens the results screen.
const VIEW_DETAILS: &str = "View Details";

/// What the toast a pass the user asked for shows while it checks says.
const CHECKING: &str = "Checking for extension updates…";

/// One package's outcome in one update pass, as the record keeps it and
/// the results view shows it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    /// The package the outcome is about: what its row's icon and its
    /// Show Extension action resolve by.
    pub identity: PackageIdentity,
    /// Its title, as the pass found it.
    pub title: String,
    /// What the outcome says: the old and new version or commit of an
    /// update, the reason of a skip, the explanation of a failure.
    pub detail: String,
    /// What an update installed, the new version or commit: what a pause
    /// of that code records as the update's failure (see `Record::paused`).
    /// `None` on every other row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_to: Option<String>,
}

/// The update results of one pass, grouped as the view lists them.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UpdateResults {
    /// The packages the pass updated, with what it updated them from and
    /// to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updated: Vec<UpdateResult>,
    /// The packages whose newer version the pass downloaded and staged,
    /// waiting for the package to grow quiet so it can be applied: each
    /// with the title it waits for, which the row becomes an Updated one
    /// once it applies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waiting: Vec<UpdateResult>,
    /// The packages the pass considered and did not update, each with why.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<UpdateResult>,
    /// The packages the pass failed to update, each with the explanation,
    /// ending [`KEEPS_RUNNING`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<UpdateResult>,
}

impl UpdateResults {
    /// Whether any group holds a row.
    pub fn is_empty(&self) -> bool {
        self.updated.is_empty()
            && self.waiting.is_empty()
            && self.skipped.is_empty()
            && self.failed.is_empty()
    }

    /// The row of the package with the identity key `target`, whichever
    /// group holds it; `None` when no row of the record is that key's.
    pub(in crate::launcher) fn row_of(&self, target: &str) -> Option<&UpdateResult> {
        self.updated
            .iter()
            .chain(&self.waiting)
            .chain(&self.skipped)
            .chain(&self.failed)
            .find(|row| row.identity.key() == target)
    }
}

/// One action the Actions panel offers in the update results view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateResultsAction {
    /// Opens the extension's page in Settings: what Enter does.
    ShowExtension,
    /// Copies the row's details, to report them to the extension's author.
    CopyDetails,
    /// Checks the row's extension alone for an update, and updates what
    /// that finds: offered on a Failed row, so a passing network problem
    /// is easy to recover from.
    Retry,
    /// Updates the row's extension now: offered on a Skipped row whose
    /// only reason is the user's switch — its automatic updates turned
    /// off, globally or for it.
    UpdateNow,
}

/// What Pane keeps of the latest pass that recorded: its results, which
/// pass they are from, whether that pass's failure was announced, and
/// when the updater last checked. Read at Pane's start; written by the
/// updater's thread.
#[derive(Default)]
pub(in crate::launcher) struct Record {
    /// The pass the results are from: what the one-time announcement of a
    /// failure is keyed on, so a new failing pass re-arms it.
    pass: u64,
    /// Whether that pass's failure was announced already. Kept for this
    /// run only: a start of Pane announces a failure the record still
    /// holds once more, the first time it is shown.
    announced: bool,
    /// When the updater last checked, in clock milliseconds: what "Last
    /// checked …" says, which survives restarts with the record. `None`
    /// until a check has completed.
    last_checked: Option<u64>,
    /// The results of that pass, as the view and the tests read them.
    pub(in crate::launcher) results: UpdateResults,
}

/// The record as `update-results.json` writes it. Each group is written
/// only when it holds anything, so the file stays small and a record from
/// before a group existed still reads. The fields are named as
/// `installed.json` names its multi-word ones.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Recorded {
    version: u64,
    pass: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    updated: Vec<UpdateResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    waiting: Vec<UpdateResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    skipped: Vec<UpdateResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    failed: Vec<UpdateResult>,
    /// When the updater last checked, in clock milliseconds; absent in a
    /// record from before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_checked: Option<u64>,
}

impl Record {
    /// Reads the record kept in `dir`: none there, or one this Pane cannot
    /// read, is no record at all (a pass that records writes it anew).
    pub(in crate::launcher) fn open(dir: &std::path::Path) -> Record {
        let Ok(text) = std::fs::read_to_string(dir.join(FILE)) else {
            return Record::default();
        };
        let Ok(read) = serde_json::from_str::<Recorded>(&text) else {
            return Record::default();
        };
        // Only this record layout's version reads; anything else is
        // yesterday's record, not this Pane's (a pass that records writes
        // it anew).
        if read.version != 1 {
            return Record::default();
        }
        Record {
            pass: read.pass,
            announced: false,
            last_checked: read.last_checked,
            results: UpdateResults {
                updated: read.updated,
                waiting: read.waiting,
                skipped: read.skipped,
                failed: read.failed,
            },
        }
    }

    /// The record's text, and where it goes.
    fn text(&self, dir: &std::path::Path) -> (PathBuf, String) {
        let recorded = Recorded {
            version: 1,
            pass: self.pass,
            updated: self.results.updated.clone(),
            waiting: self.results.waiting.clone(),
            skipped: self.results.skipped.clone(),
            failed: self.results.failed.clone(),
            last_checked: self.last_checked,
        };
        (
            dir.join(FILE),
            serde_json::to_string_pretty(&recorded).unwrap_or_default(),
        )
    }

    /// The identity of the pass after the one recorded.
    fn next_pass(&self) -> u64 {
        self.pass + 1
    }

    /// Records `pass`: whether it replaces what this record held. A pass
    /// that found something new does (see [`Pass`]); one that found
    /// nothing new does not, so what the last such pass recorded stays —
    /// unless it is a pass the user asked for, whose rows are its answer
    /// and record whatever they came to.
    fn record(&mut self, pass: &Pass) -> bool {
        if !pass.found_new && !(pass.asked && !pass.results.is_empty()) {
            return false;
        }
        self.pass = pass.id;
        self.announced = false;
        self.results = pass.results.clone();
        true
    }

    /// The failures this record holds that were not announced yet, as a
    /// count: `None` when there are none, or they were.
    fn unannounced(&self) -> Option<usize> {
        let failed = self.results.failed.len();
        (!self.announced && failed > 0).then_some(failed)
    }

    /// Notes that the record's failure was announced.
    fn announce(&mut self) {
        self.announced = true;
    }

    /// Moves the record's Updated row for the package with `identity` at
    /// `pause`'s version to Failed, with the failure that paused it: the
    /// update's replacement passed its checks, but the code Pane paused
    /// after it could not run, and no older version is restored (Q31, ADR
    /// 0004). Whether a row moved.
    fn paused(&mut self, identity: &PackageIdentity, pause: &Pause, title: &str) -> bool {
        let version = pause.version.as_deref();
        let found = self
            .results
            .updated
            .iter()
            .position(|row| row.identity == *identity && row.updated_to.as_deref() == version);
        let Some(at) = found else {
            return false;
        };
        let mut row = self.results.updated.remove(at);
        row.detail = format!(
            "{} and is paused: Pane runs none of its code until you retry it. {}",
            pause.after.failure(title),
            pause.why
        );
        row.updated_to = None;
        self.results.failed.push(row);
        true
    }
}

/// One pass of the updater, as its outcomes collect (see `updates`): the
/// pass's identity, whether the user asked for it, and what it has come
/// to for each package it considered. The updater keeps the pass it is
/// running; the outcomes of what it applies land in it even after the
/// pass's checks ended, until the next pass begins.
#[derive(Clone)]
pub(in crate::launcher) struct Pass {
    /// The pass's identity: the recorded pass's number, which tells a new
    /// failing pass from one already announced.
    id: u64,
    /// Whether the user asked for this pass: root search's "Check for
    /// Extension Updates" row, the Settings Extensions group's "Check for
    /// updates" button, or a result row's Retry or Update Now. An asked
    /// pass answers even when everything is up to date, records whatever
    /// it came to, and announces its own failures (its ending toast is
    /// the announcement).
    asked: bool,
    /// Whether the pass found something new: updated, failed, skipped a
    /// newer version it found, or is waiting to apply one. A pass that
    /// found nothing new records nothing (see [`Record::record`]).
    found_new: bool,
    /// What the pass has come to, per package.
    results: UpdateResults,
}

impl Pass {
    /// The pass after the one `recorded`.
    pub(in crate::launcher) fn after(recorded: &Record, asked: bool) -> Pass {
        Pass {
            id: recorded.next_pass(),
            asked,
            found_new: false,
            results: UpdateResults::default(),
        }
    }

    /// One package's outcome, as a check of it collects while the pass's
    /// checks run a few packages at a time (see `Updates::check`):
    /// merged into the pass in the order the packages were checked, so
    /// the pass's rows keep that order however the checks completed. The
    /// part carries the pass's `asked` — what it stages is an asked
    /// pass's staging, which applying does not re-ask the user's
    /// controls for (see the `Staged` the updater keeps).
    pub(in crate::launcher) fn part(asked: bool) -> Pass {
        Pass {
            id: 0,
            asked,
            found_new: false,
            results: UpdateResults::default(),
        }
    }

    /// Merges `part`, one checked package's outcome, into this pass.
    pub(in crate::launcher) fn merge(&mut self, part: Pass) {
        self.found_new |= part.found_new;
        let Pass { results, .. } = part;
        self.results.updated.extend(results.updated);
        self.results.waiting.extend(results.waiting);
        self.results.skipped.extend(results.skipped);
        self.results.failed.extend(results.failed);
    }

    /// Whether the user asked for this pass: what it stages applies
    /// without re-asking the user's controls (see the `Staged` the
    /// updater keeps), and its failures are announced by its own ending
    /// toast rather than the background announcement.
    pub(in crate::launcher) fn is_asked(&self) -> bool {
        self.asked
    }

    /// Puts each group's rows in the installed list's order — the
    /// identities `installed`, in that order — whatever order the pass's
    /// checks and applies completed in: the record and the view list them
    /// as the extensions are installed. Rows of packages no longer
    /// installed keep their relative order, last.
    pub(in crate::launcher) fn in_order_of(&mut self, installed: &[&PackageIdentity]) {
        let at = |identity: &PackageIdentity| {
            installed
                .iter()
                .position(|installed| *installed == identity)
                .unwrap_or(usize::MAX)
        };
        for group in [
            &mut self.results.updated,
            &mut self.results.waiting,
            &mut self.results.skipped,
            &mut self.results.failed,
        ] {
            group.sort_by_cached_key(|row| at(&row.identity));
        }
    }

    /// The pass updated the package `identity`, titled `title`, from
    /// `from` to `to`. Any row waiting for the package to grow quiet is
    /// settled: it became this one.
    pub(in crate::launcher) fn updated(
        &mut self,
        identity: PackageIdentity,
        title: String,
        from: &str,
        to: &str,
    ) {
        self.found_new = true;
        self.settle(&identity);
        self.results.updated.push(UpdateResult {
            identity,
            title,
            detail: format!("{from} → {to}"),
            updated_to: Some(to.to_owned()),
        });
    }

    /// The pass staged a newer version of the package `identity`, titled
    /// `title`, and its apply waits for the package to grow quiet: a
    /// waiting row, which an update or a failure of the same package
    /// settles. Whether a row was added — a later look that retries the
    /// apply records nothing new while the row is already there.
    pub(in crate::launcher) fn waiting(
        &mut self,
        identity: PackageIdentity,
        title: String,
    ) -> bool {
        if self
            .results
            .waiting
            .iter()
            .any(|row| row.identity == identity)
        {
            return false;
        }
        self.found_new = true;
        self.results.waiting.push(UpdateResult {
            identity,
            detail: format!("Waiting until {title} is not in use"),
            title,
            updated_to: None,
        });
        true
    }

    /// The pass considered the package `identity`, titled `title`, and did
    /// not look at it, for `why`: its version or revision is pinned, it is
    /// a local or development copy, its automatic updates are off, it is
    /// disabled, paused, or something else is being done to it. Skipping
    /// never looks like a fault, and says why.
    pub(in crate::launcher) fn skipped(
        &mut self,
        identity: PackageIdentity,
        title: String,
        why: &str,
    ) {
        self.results.skipped.push(UpdateResult {
            identity,
            title,
            detail: why.to_owned(),
            updated_to: None,
        });
    }

    /// The pass found a newer version of the package `identity`, titled
    /// `title`, and did not take it, because `why`: it needs a newer Pane,
    /// or is not available on this system. Unlike the other skips, this
    /// one records the pass: the pass found something new.
    pub(in crate::launcher) fn refused(
        &mut self,
        identity: PackageIdentity,
        title: String,
        why: &str,
    ) {
        self.found_new = true;
        self.results.skipped.push(UpdateResult {
            identity,
            title,
            detail: format!("{why}. {KEEPS_RUNNING}"),
            updated_to: None,
        });
    }

    /// The pass failed the package `identity`, titled `title`: `why` says
    /// what failed, and the detail ends that it keeps running its
    /// installed code. Any row waiting for the package to grow quiet is
    /// settled: the update it waited for failed.
    pub(in crate::launcher) fn failed(
        &mut self,
        identity: PackageIdentity,
        title: String,
        why: &str,
    ) {
        self.found_new = true;
        self.settle(&identity);
        self.results.failed.push(UpdateResult {
            identity,
            title,
            detail: format!("{why}. {KEEPS_RUNNING}"),
            updated_to: None,
        });
    }

    /// Takes the waiting row of the package `identity` away, its wait
    /// settled by an update or a failure of the same package.
    fn settle(&mut self, identity: &PackageIdentity) {
        self.results.waiting.retain(|row| row.identity != *identity);
    }
}

impl Launcher {
    /// The update results of the latest pass that found something new,
    /// read without entering any flow: the same record the results screen
    /// and the failure announcement come from, for a second surface of it
    /// beside the launcher's (Pane's Settings window) and for tests.
    /// Nothing here records, shows or announces anything.
    pub fn update_results(&self) -> UpdateResults {
        self.lock().update_results.results.clone()
    }

    /// When the updater last checked for extension updates, in clock
    /// milliseconds (see [`crate::clipboard::Clock`]): what "Last checked
    /// …" says in the Settings Extensions group, which survives restarts
    /// with the record. `None` until a check has completed.
    pub fn last_extension_check(&self) -> Option<u64> {
        self.lock().update_results.last_checked
    }

    /// The actions the update results view's Actions panel offers for the
    /// row with the identity key `target`, beside showing its extension's
    /// page in Settings and copying its details: Retry on a Failed row,
    /// which checks that extension alone, and Update Now on a Skipped row
    /// whose only reason is the user's switch — its automatic updates
    /// turned off, globally or for it — which updates it. Read from the
    /// record and the state as they are now: a row's package that is no
    /// longer installed, or no longer one a pass could look at, offers
    /// neither.
    pub fn update_results_row_actions(&self, target: &str) -> Vec<UpdateResultsAction> {
        let state = self.lock();
        let Some(row) = state.update_results.results.row_of(target) else {
            return Vec::new();
        };
        let Some(package) = state.package(&row.identity) else {
            return Vec::new();
        };
        let failed = state
            .update_results
            .results
            .failed
            .iter()
            .any(|failed| failed.identity == row.identity);
        let mut actions = Vec::new();
        if failed
            && updates::eligible_when_asked(package)
            && !state.changing.contains_key(&package.identity)
        {
            actions.push(UpdateResultsAction::Retry);
        }
        if !failed && updates::only_the_switch(&state, package) {
            actions.push(UpdateResultsAction::UpdateNow);
        }
        actions
    }

    /// Records `pass`, the outcomes the updater collected for one pass,
    /// as Pane's own record beside `updates.json`: the latest pass that
    /// found something new replaces what the record held (a pass that
    /// found nothing new keeps it; an asked one records whatever it came
    /// to), a failure it holds is announced while the launcher is shown
    /// — unless the pass was asked for, whose ending toast is its own
    /// announcement — and the file is written on the updater's own
    /// thread. The pass's rows are put in the installed list's order,
    /// whatever order its checks and applies completed in.
    pub(in crate::launcher) fn note_update_pass(&self, pass: &Pass) {
        let written = {
            let mut state = self.lock();
            let mut pass = pass.clone();
            let installed: Vec<&PackageIdentity> = state
                .packages
                .iter()
                .map(|package| &package.identity)
                .collect();
            pass.in_order_of(&installed);
            let replaced = state.update_results.record(&pass);
            // The results view lists the record the pass just wrote, its
            // query and selection kept: a pass that records while the
            // view is open shows its own rows, not the ones it replaced.
            if replaced && matches!(state.view.screen, Screen::UpdateResults { .. }) {
                self.refresh_update_results(&mut state);
            }
            // A pass the user asked for announces its own failures: its
            // ending toast (see [`Launcher::end_update_pass_toast`]) is
            // the announcement, so the background one — here, or on the
            // next showing — stays silent.
            if replaced && pass.asked {
                state.update_results.announce();
            }
            let written = replaced
                .then(|| {
                    let installation = self.installation.as_ref()?;
                    Some(state.update_results.text(&installation.dir))
                })
                .flatten();
            // A failure recorded while the launcher is shown is announced
            // at once; one recorded while it is hidden waits for the next
            // time it is shown (see `set_window_presence`) — unless the
            // pass was asked for, whose ending toast is its own
            // announcement, so the background one stays silent also
            // mid-pass, before its record replaces what the record held
            // (which would otherwise take the progress toast's place).
            if !pass.asked && state.feedback.presence == WindowPresence::Shown {
                self.announce_update_failures(&mut state);
            }
            written
        };
        if let Some((file, text)) = written {
            let _ = write_atomically(&file, text.as_bytes(), Readers::Default);
        }
    }

    /// Notes that the updater checked for extension updates at `at` (the
    /// clock's milliseconds): the record's "Last checked", written with
    /// the record as it now stands — whatever the pass that checked came
    /// to, the check itself is done. For the updater's own thread, as
    /// each check pass completes.
    pub(in crate::launcher) fn note_extension_check(&self, at: u64) {
        let written = {
            let mut state = self.lock();
            if state.update_results.last_checked == Some(at) {
                return;
            }
            state.update_results.last_checked = Some(at);
            self.installation
                .as_ref()
                .map(|installation| state.update_results.text(&installation.dir))
        };
        if let Some((file, text)) = written {
            let _ = write_atomically(&file, text.as_bytes(), Readers::Default);
        }
    }

    /// Shows the toast a pass the user asked for updates through its
    /// life: "Checking for extension updates…" while it checks, "Updating
    /// N of M…" as it applies what it found, ending as its summary with
    /// View Details (see [`Launcher::end_update_pass_toast`]). Returns
    /// the toast's id, to update it by. Nothing here checks, records or
    /// announces anything; the updater's own thread calls this as it
    /// begins an asked pass, with the launcher locked.
    pub(in crate::launcher) fn begin_update_pass_toast(&self, state: &mut State) -> u64 {
        let id = self.put_own_toast(state, Toast::new(ToastStyle::Animated, CHECKING));
        self.changed();
        id
    }

    /// Updates the asked pass's progress toast `id`, which
    /// [`Launcher::begin_update_pass_toast`] showed, with what applying
    /// its staged updates has come to: "Updating `done` of `total`…". The
    /// updater's own thread calls this, with the launcher locked.
    pub(in crate::launcher) fn update_pass_progress(
        &self,
        state: &mut State,
        id: u64,
        done: usize,
        total: usize,
    ) {
        self.update_own_toast(
            state,
            id,
            Toast::new(ToastStyle::Animated, format!("Updating {done} of {total}…")),
        );
        self.changed();
    }

    /// Ends the asked pass's progress toast `id` as the pass's summary:
    /// what it updated and what failed, with View Details, which opens
    /// the results view; a pass that updated nothing and failed nothing
    /// says the extensions are up to date — the user asked, so up to date
    /// is an answer, not a silence. What the pass deferred (a package in
    /// use) is in the record, waiting, and not counted here: the toast
    /// ends when the pass has nothing more it can do now, and the row
    /// becomes an Updated one once the package grows quiet. The status
    /// line goes back to rest, the row the pass was started by having
    /// said it was running. The updater's own thread calls this, with
    /// the launcher locked.
    pub(in crate::launcher) fn end_update_pass_toast(
        &self,
        state: &mut State,
        id: u64,
        pass: &Pass,
    ) {
        let updated = pass.results.updated.len();
        let failed = pass.results.failed.len();
        let ending = ending_toast(updated, failed);
        // The progress toast this ends — unless another toast replaced it
        // meanwhile (an announcement of the record's earlier failure,
        // say), in which case the summary is put anew: it is the pass's
        // say, and it is not lost to whatever took the toast's place.
        if !self.update_own_toast(state, id, ending.clone()) {
            self.put_own_toast(state, ending);
        }
        if matches!(state.view.status, Status::Running { .. }) {
            state.view.status = Status::Idle;
        }
        self.changed();
    }

    /// Announces a failure the record holds and has not announced yet, as
    /// a failure toast carrying View Details, which opens the results
    /// screen: once for the pass the failure is from, so showing the
    /// launcher again says nothing more. The announcement re-arms when a
    /// new pass records.
    pub(in crate::launcher) fn announce_update_failures(&self, state: &mut State) {
        let Some(failed) = state.update_results.unannounced() else {
            return;
        };
        state.update_results.announce();
        let toast = Toast {
            style: ToastStyle::Failure,
            title: failure_title(failed),
            message: None,
            primary: Some(ToastAction {
                title: VIEW_DETAILS.into(),
                shortcut: None,
                unbound: None,
                does: ToastDoes::ShowUpdateResults,
            }),
            secondary: None,
        };
        self.show_own_toast(state, toast);
        self.changed();
    }

    /// Notes that Pane paused the installed package with `identity`: the
    /// record's Updated row for the version that failed becomes a Failed
    /// row with the failure that paused it, and the record is written
    /// again off the calling thread (the runtime's).
    pub(in crate::launcher) fn update_results_paused(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
        pause: &Pause,
    ) {
        let title = state.title_of(identity);
        let written = state
            .update_results
            .paused(identity, pause, &title)
            .then(|| {
                let installation = self.installation.as_ref()?;
                Some(state.update_results.text(&installation.dir))
            })
            .flatten();
        if let Some((file, text)) = written {
            std::thread::spawn(move || {
                let _ = write_atomically(&file, text.as_bytes(), Readers::Default);
            });
        }
    }

    /// Shows the update results of the latest pass that recorded, "Update
    /// Results", in place of whatever the launcher showed (Settings opens
    /// it too, over an open command, which is left): each group in the
    /// order Updated, Waiting, Skipped, Failed, the empty ones hidden, its
    /// rows searched by what the user types.
    pub(in crate::launcher) fn show_update_results(&self, state: &mut State) {
        self.show_update_results_at(state, "");
    }

    /// [`Launcher::show_update_results`], under `query`.
    fn show_update_results_at(&self, state: &mut State, query: &str) {
        let results = state.update_results.results.clone();
        let (rows, entries) = result_rows(&results, query);
        self.leave_command(state);
        state.entries = entries;
        let screen = Screen::UpdateResults {
            query: query.to_owned(),
        };
        state.view = LauncherView::new(screen, "Update Results").with_rows(rows);
    }

    /// Searches the update results the screen lists by `query`, the text
    /// typed in its search field: the rows whose title or detail holds the
    /// trimmed text, ignoring case, keep the groups' order, and the row
    /// selected stays selected while it is still listed, else the first
    /// is. The screen is not left, and no reply is discarded.
    pub(in crate::launcher) fn search_update_results(&self, state: &mut State, query: &str) {
        let selected = state
            .view
            .selected
            .and_then(|index| state.view.rows.get(index))
            .map(|row| row.id.clone());
        let results = state.update_results.results.clone();
        let (rows, entries) = result_rows(&results, query);
        state.entries = entries;
        state.view.screen = Screen::UpdateResults {
            query: query.to_owned(),
        };
        state.view.rows = rows;
        state.view.selected = selected
            .and_then(|id| state.view.rows.iter().position(|row| row.id == id))
            .or_else(|| first_index(&state.view.rows));
    }

    /// Refreshes the update results the screen lists, after a later pass
    /// recorded or a package changed in the background, keeping the query
    /// and the screen's epoch.
    pub(in crate::launcher) fn refresh_update_results(&self, state: &mut State) {
        let query = match &state.view.screen {
            Screen::UpdateResults { query } => query.clone(),
            _ => return,
        };
        let selected = state
            .view
            .selected
            .and_then(|index| state.view.rows.get(index))
            .map(|row| row.id.clone());
        let epoch = state.screen_epoch;
        self.show_update_results_at(state, &query);
        state.screen_epoch = epoch;
        if let Some(id) = selected
            && let Some(index) = state.view.rows.iter().position(|row| row.id == id)
        {
            state.view.selected = Some(index);
        }
    }
}

/// The results screen's rows and entries for `results` under `query`:
/// each group in the order Updated, Waiting, Skipped, Failed, the empty
/// ones hidden, the rows whose title or detail holds the trimmed `query`
/// (ignoring case), each opening its extension's page in Settings.
fn result_rows(results: &UpdateResults, query: &str) -> (Vec<Row>, Vec<Entry>) {
    // What the search holds: the rows whose title or detail holds the
    // trimmed query, ignoring case; all of them for a blank one.
    let needle = query.trim().to_lowercase();
    let mut rows = Vec::new();
    let mut entries = Vec::new();
    for group in [
        &results.updated,
        &results.waiting,
        &results.skipped,
        &results.failed,
    ] {
        for row in group {
            if !needle.is_empty()
                && !row.title.to_lowercase().contains(&needle)
                && !row.detail.to_lowercase().contains(&needle)
            {
                continue;
            }
            rows.push(Row {
                id: row.identity.key(),
                title: row.title.clone(),
                subtitle: Some(row.detail.clone()),
                unavailable: None,
            });
            entries.push(Entry::ShowExtension(row.identity.clone()));
        }
    }
    (rows, entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pass_that_found_nothing_new_keeps_what_the_record_held() {
        let mut record = Record::default();
        let mut pass = Pass::after(&record, false);
        // A pinned package, a disabled one and an up-to-date one: nothing
        // new found, so nothing is recorded.
        pass.skipped(
            PackageIdentity::npm("@pane-tests/settings"),
            "Settings from npm".into(),
            "Its version is pinned",
        );
        assert!(!record.record(&pass));
        assert!(record.results.is_empty());

        // A pass that updated something records, with its skips.
        let mut pass = Pass::after(&record, false);
        pass.skipped(
            PackageIdentity::npm("@pane-tests/settings"),
            "Settings from npm".into(),
            "Its version is pinned",
        );
        pass.updated(
            PackageIdentity::npm("@pane-tests/greeter"),
            "Greeter from npm".into(),
            "0.1.0",
            "0.2.0",
        );
        assert!(record.record(&pass));
        assert_eq!(record.results.updated.len(), 1);
        assert_eq!(record.results.skipped.len(), 1);
        assert_eq!(record.results.updated[0].detail, "0.1.0 → 0.2.0");
        assert_eq!(
            record.results.updated[0].updated_to.as_deref(),
            Some("0.2.0")
        );

        // The next pass finds nothing new: the record stays.
        let mut quiet = Pass::after(&record, false);
        quiet.skipped(
            PackageIdentity::npm("@pane-tests/greeter"),
            "Greeter from npm".into(),
            "It is disabled",
        );
        assert!(!record.record(&quiet));
        assert_eq!(record.results.updated.len(), 1, "the record stays");
    }

    #[test]
    fn a_pass_the_user_asked_for_records_what_it_came_to() {
        let mut record = Record::default();
        // A record to keep: an earlier pass updated something.
        let mut pass = Pass::after(&record, false);
        pass.updated(
            PackageIdentity::npm("@pane-tests/greeter"),
            "Greeter from npm".into(),
            "0.1.0",
            "0.2.0",
        );
        assert!(record.record(&pass));

        // An asked pass that found nothing new records its skips, unlike
        // a background one: its rows are its answer.
        let mut asked = Pass::after(&record, true);
        asked.skipped(
            PackageIdentity::npm("@pane-tests/settings"),
            "Settings from npm".into(),
            "Its version is pinned",
        );
        assert!(record.record(&asked));
        assert_eq!(record.results.skipped.len(), 1);
        assert_eq!(record.results.updated.len(), 0);

        // An asked pass that came to nothing at all keeps the record, as
        // a background one does: there is nothing to say.
        let empty = Pass::after(&record, true);
        assert!(!record.record(&empty));
        assert_eq!(record.results.skipped.len(), 1, "the record stays");
    }

    #[test]
    fn a_pass_that_refused_a_newer_version_records_but_does_not_fail() {
        let mut record = Record::default();
        let mut pass = Pass::after(&record, false);
        pass.refused(
            PackageIdentity::npm("@pane-tests/settings"),
            "Settings from npm".into(),
            "Incompatible package: it needs Pane extension API 0.2, but this Pane provides 0.1",
        );
        assert!(pass.found_new, "the pass found something new");
        assert!(record.record(&pass));
        assert!(record.results.failed.is_empty(), "skipping is no fault");
        assert_eq!(record.unannounced(), None, "nothing to announce");
        assert!(record.results.skipped[0].detail.ends_with(KEEPS_RUNNING));
    }

    #[test]
    fn a_failure_is_announced_once_until_a_new_pass_records() {
        let mut record = Record::default();
        let mut pass = Pass::after(&record, false);
        pass.failed(
            PackageIdentity::npm("@pane-tests/settings"),
            "Settings from npm".into(),
            "It was not checked for a newer version: no route to the registry",
        );
        assert!(record.record(&pass));
        assert_eq!(record.unannounced(), Some(1));
        record.announce();
        assert_eq!(record.unannounced(), None, "announced once");

        // A new failing pass re-arms it.
        let mut again = Pass::after(&record, false);
        again.failed(
            PackageIdentity::npm("@pane-tests/settings"),
            "Settings from npm".into(),
            "It was not checked for a newer version: no route to the registry",
        );
        assert!(record.record(&again));
        assert_eq!(record.unannounced(), Some(1));
        assert_eq!(failure_title(1), "1 extension update failed");
        assert_eq!(failure_title(2), "2 extension updates failed");
    }

    #[test]
    fn an_asked_pass_marks_its_failures_announced() {
        // The pass's ending toast is the announcement of its failures, so
        // the background one stays silent: note_update_pass marks it.
        let mut record = Record::default();
        let mut pass = Pass::after(&record, true);
        pass.failed(
            PackageIdentity::npm("@pane-tests/settings"),
            "Settings from npm".into(),
            "It was not checked for a newer version: no route to the registry",
        );
        assert!(record.record(&pass));
        record.announce();
        assert_eq!(record.unannounced(), None, "announced by its own toast");
    }

    #[test]
    fn a_waiting_row_is_added_once_and_settled_by_its_outcome() {
        let mut record = Record::default();
        let mut pass = Pass::after(&record, false);
        let identity = PackageIdentity::npm("@pane-tests/settings");
        assert!(pass.waiting(identity.clone(), "Settings from npm".into()));
        assert!(!pass.waiting(identity.clone(), "Settings from npm".into()));
        assert!(pass.found_new, "a pending update is something new");
        assert!(record.record(&pass));
        assert_eq!(
            record.results.waiting[0].detail,
            "Waiting until Settings from npm is not in use"
        );

        // The apply lands: the waiting row becomes the Updated one.
        let mut pass = Pass::after(&record, false);
        pass.waiting(identity.clone(), "Settings from npm".into());
        pass.updated(
            identity.clone(),
            "Settings from npm".into(),
            "0.1.0",
            "0.2.0",
        );
        assert!(record.record(&pass));
        assert!(record.results.waiting.is_empty());
        assert_eq!(record.results.updated.len(), 1);

        // An apply that fails settles it the same way.
        let mut pass = Pass::after(&record, false);
        pass.waiting(identity.clone(), "Settings from npm".into());
        pass.failed(
            identity,
            "Settings from npm".into(),
            "It was not updated: gone",
        );
        assert!(record.record(&pass));
        assert!(record.results.waiting.is_empty());
        assert_eq!(record.results.failed.len(), 1);
    }

    #[test]
    fn the_passs_rows_take_the_installed_lists_order() {
        let mut pass = Pass::after(&Record::default(), false);
        // The order the outcomes completed in, backwards from the
        // installed list's.
        let greeter = PackageIdentity::npm("@pane-tests/greeter");
        let settings = PackageIdentity::npm("@pane-tests/settings");
        pass.failed(settings.clone(), "Settings from npm".into(), "gone");
        pass.updated(greeter.clone(), "Greeter from npm".into(), "0.1.0", "0.2.0");
        pass.in_order_of(&[&greeter, &settings]);
        assert_eq!(
            pass.results.updated[0].identity, greeter,
            "the installed list's order"
        );
        // A package no longer installed keeps its relative order, last.
        pass.failed(
            PackageIdentity::npm("@pane-tests/gone"),
            "Gone".into(),
            "uninstalled meanwhile",
        );
        pass.in_order_of(&[&greeter, &settings]);
        assert_eq!(pass.results.failed[1].title, "Gone");
    }

    #[test]
    fn the_ending_toast_says_what_the_pass_came_to() {
        let shown = |updated: usize, failed: usize| {
            let toast = ending_toast(updated, failed);
            (toast.style, toast.title)
        };
        assert_eq!(
            shown(0, 0),
            (ToastStyle::Success, "Extensions are up to date".into())
        );
        assert_eq!(
            shown(1, 0),
            (ToastStyle::Success, "Updated 1 extension".into())
        );
        assert_eq!(
            shown(3, 0),
            (ToastStyle::Success, "Updated 3 extensions".into())
        );
        assert_eq!(
            shown(0, 2),
            (ToastStyle::Failure, "2 extension updates failed".into())
        );
        assert_eq!(
            shown(2, 1),
            (ToastStyle::Failure, "Updated 2 extensions, 1 failed".into())
        );
        assert_eq!(
            shown(1, 1),
            (ToastStyle::Failure, "Updated 1 extension, 1 failed".into())
        );
        for toast in [ending_toast(0, 0), ending_toast(2, 1)] {
            assert_eq!(
                toast.primary.as_ref().map(|action| action.title.as_str()),
                Some(VIEW_DETAILS)
            );
        }
    }

    #[test]
    fn the_record_keeps_when_the_updater_last_checked() {
        let mut record = Record::default();
        assert_eq!(record.last_checked, None);
        record.last_checked = Some(1700000000000);
        let (_, text) = record.text(std::path::Path::new("/none"));
        assert!(text.contains("\"lastChecked\": 1700000000000"), "{text}");
        // A record from before the field existed still reads, as none.
        let read = r#"{"version":1,"pass":3}"#;
        let read: Recorded = serde_json::from_str(read).unwrap();
        assert_eq!(read.last_checked, None, "a record from before the field");
    }

    #[test]
    fn pausing_the_version_an_update_installed_records_the_failure() {
        let mut record = Record::default();
        let mut pass = Pass::after(&record, false);
        pass.updated(
            PackageIdentity::npm("@pane-tests/settings"),
            "Settings from npm".into(),
            "0.1.0",
            "0.2.0",
        );
        assert!(record.record(&pass));
        let pause = Pause {
            after: crate::packages::PauseCause::FailedToStart,
            why: "the component could not be instantiated".into(),
            version: Some("0.2.0".into()),
        };
        assert!(record.paused(
            &PackageIdentity::npm("@pane-tests/settings"),
            &pause,
            "Settings from npm"
        ));
        assert!(record.results.updated.is_empty());
        let [failed] = &record.results.failed[..] else {
            panic!("the update's row moved to Failed");
        };
        assert_eq!(failed.updated_to, None);
        assert_eq!(
            failed.detail,
            "Settings from npm could not start and is paused: Pane runs none of its code until \
             you retry it. the component could not be instantiated"
        );

        // Another version pausing, or a package the record did not
        // update, records nothing.
        let other = Pause {
            version: Some("0.3.0".into()),
            ..pause.clone()
        };
        assert!(!record.paused(
            &PackageIdentity::npm("@pane-tests/settings"),
            &other,
            "Settings from npm"
        ));
        assert!(!record.paused(
            &PackageIdentity::npm("@pane-tests/greeter"),
            &pause,
            "Greeter from npm"
        ));
    }
}
