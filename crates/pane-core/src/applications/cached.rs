//! The host's live list of applications by identity (ADR 0038), kept in
//! front of a system adapter while a package that asked for it runs, so
//! that a guest asking for the installed applications gets them at once
//! after the first scan, the list stays current by itself, and every id,
//! current or from before identities, finds what opens it.
//!
//! The first [`Applications::installed`] scans on the calling thread and
//! starts a worker of the list's own, which watches the adapter's places
//! ([`Discovery::watch`]) and rescans:
//!
//! - once a reported change settles ([`DEBOUNCE`] after the last one, at
//!   most [`MOST_DEBOUNCE`] after the first), and again
//!   [`COMPLETING_AGAIN`] later for a change the system completes later;
//! - at once when the watcher lost changes or failed, and when the system
//!   seems to have slept (a wait that lasted far longer than it could);
//! - every reconciling period, whatever the watcher saw.
//!
//! Each rescan replaces the list by identity: an application whose last
//! source went stays listed for [`GRACE`] by the list's clock, and leaves
//! only if nothing with its identity came back meanwhile, so an update
//! that removes and reinstalls an application never makes it flicker out.
//! A failed rescan keeps the list. When the listed applications change,
//! [`Applications::on_change`]'s listeners are told.
//!
//! [`Applications::release`] (no package that asked can run any more)
//! drops the list and stops the worker and its watch; the next
//! [`Applications::installed`] starts again. Nothing is scanned or watched
//! before the first call, so a disabled Applications extension costs
//! nothing.

use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use super::identity::{Catalog, Identified, Source, is_identity};
use super::{Application, Applications, Change, Changes, Discovery, Watch};
use crate::clipboard::{Clock, SystemClock};
use crate::util::lock;

/// How long the changes a watcher reports must settle before the list is
/// rescanned.
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// The longest a burst of changes puts its rescan off, from its first
/// change.
const MOST_DEBOUNCE: Duration = Duration::from_secs(2);

/// How long an application whose last source went stays listed.
pub const GRACE: Duration = Duration::from_secs(5);

/// How long after a change the system completes later (a packaged app's
/// registration) the list is rescanned again.
const COMPLETING_AGAIN: Duration = Duration::from_secs(5);

/// The longest the worker waits before looking at the clocks again.
const TICK: Duration = Duration::from_secs(5);

/// How much longer than it was bounded a wait must last for the system to
/// be taken to have slept meanwhile (or its clock to have jumped).
const SLEPT_AFTER: Duration = Duration::from_secs(60);

/// The shortest reconciling period, so that a list is never rescanned
/// without pause.
const SHORTEST_PERIOD: Duration = Duration::from_secs(1);

/// The installed applications as `inner`'s sources make them
/// ([`Catalog`]), kept current while needed (see the module docs).
pub struct Cached {
    inner: Arc<dyn Discovery>,
    shared: Arc<Shared>,
}

/// What the list and its worker share.
struct Shared {
    /// How often the list is rescanned in full while kept.
    period: Duration,
    /// Tells the grace and the period.
    clock: Mutex<Arc<dyn Clock>>,
    state: Mutex<State>,
    /// Wakes the worker: a change, a release, the clock set.
    wake: Condvar,
    /// Told when the listed applications change.
    listeners: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
}

#[derive(Default)]
struct State {
    /// The list, while kept.
    list: Option<List>,
    /// Counts releases: the worker, a scan and a watcher's reports from an
    /// earlier session are discarded.
    session: u64,
    /// Whether this session's worker was started.
    working: bool,
    /// A reported change: when its burst began, and when to rescan.
    changed: Option<(Instant, Instant)>,
    /// When to rescan again for a change the system completes later.
    again: Option<Instant>,
    /// Whether to rescan at once: changes were lost, or the system slept.
    lost: bool,
}

/// The list as the last scan made it.
#[derive(Clone, Debug)]
struct List {
    /// The applications the last scan found.
    found: Arc<Catalog>,
    /// Their sources, as found.
    sources: Vec<Source>,
    /// The applications whose last source went, in their grace.
    leaving: Vec<Leaving>,
    /// What is listed: those found and those leaving.
    shown: Arc<Catalog>,
    /// When the list was last scanned (or a scan failed), by the clock, in
    /// milliseconds.
    looked: u64,
}

/// An application in its grace.
#[derive(Clone, Debug)]
struct Leaving {
    application: Identified,
    /// When it leaves, by the clock, in milliseconds.
    until: u64,
}

/// What the worker does next.
#[derive(Clone, Copy)]
enum Job {
    Rescan,
    /// Drops the applications whose grace ended.
    Expire,
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

impl List {
    /// The list of `sources`, found at `looked`, with `leaving` still
    /// listed.
    fn new(sources: Vec<Source>, leaving: Vec<Leaving>, looked: u64) -> List {
        let found = Arc::new(Catalog::new(sources.clone()));
        let shown = if leaving.is_empty() {
            found.clone()
        } else {
            // Found sources first, so a path found now names what it is now.
            let all = sources
                .iter()
                .cloned()
                .chain(
                    leaving
                        .iter()
                        .flat_map(|leaving| leaving.application.sources.iter().cloned()),
                )
                .collect();
            Arc::new(Catalog::new(all))
        };
        List {
            found,
            sources,
            leaving,
            shown,
            looked,
        }
    }

    /// When the next application in its grace leaves.
    fn next_leaving(&self) -> Option<u64> {
        self.leaving.iter().map(|leaving| leaving.until).min()
    }

    /// This list without the applications whose grace ended by `now`;
    /// `None` when none did.
    fn expired(&self, now: u64) -> Option<List> {
        if self.leaving.iter().all(|leaving| leaving.until > now) {
            return None;
        }
        let leaving = self
            .leaving
            .iter()
            .filter(|leaving| leaving.until > now)
            .cloned()
            .collect();
        Some(List::new(self.sources.clone(), leaving, self.looked))
    }
}

/// The list after a scan found `sources` at `now`, replacing `before` by
/// identity: an application `before` had (found, or leaving) that is not
/// found now is leaving, until its grace from when it was first missed
/// ends; one found again is listed as found. One whose every source path
/// is found now, under another identity, was replaced rather than removed,
/// and does not stay.
fn rescanned(before: Option<&List>, sources: Vec<Source>, now: u64) -> List {
    let Some(before) = before else {
        return List::new(sources, Vec::new(), now);
    };
    let found = Catalog::new(sources.clone());
    let ids: HashSet<&str> = found
        .identified()
        .iter()
        .map(|application| application.id.as_str())
        .collect();
    let paths: HashSet<&str> = sources.iter().map(|source| source.path.as_str()).collect();
    let gone = |application: &Identified| {
        !ids.contains(application.id.as_str())
            && !application
                .sources
                .iter()
                .all(|source| paths.contains(source.path.as_str()))
    };
    let mut leaving: Vec<Leaving> = before
        .leaving
        .iter()
        .filter(|leaving| gone(&leaving.application))
        .cloned()
        .collect();
    for application in before.found.identified() {
        let known = leaving
            .iter()
            .any(|leaving| leaving.application.id == application.id);
        if gone(application) && !known {
            leaving.push(Leaving {
                application: application.clone(),
                until: now.saturating_add(millis(GRACE)),
            });
        }
    }
    List::new(sources, leaving, now)
}

/// Whether `a` and `b` list the same applications, whatever their order.
fn same_applications(a: &Catalog, b: &Catalog) -> bool {
    let sorted = |catalog: &Catalog| {
        let mut applications: Vec<Application> = catalog.applications();
        applications.sort_by(|a, b| a.id.cmp(&b.id));
        applications
    };
    sorted(a) == sorted(b)
}

/// Whether a wait bounded by `limit` that lasted `real` by the monotonic
/// clock and `wall` by the list's clock means the system slept meanwhile
/// (a thread does not overrun a timed wait by a minute while it runs), or
/// the clock jumped: either way, a rescan reconciles the list. Both are
/// looked at because whether the monotonic clock counts a sleep differs by
/// system.
fn slept(limit: Duration, real: Duration, wall: Duration) -> bool {
    real.max(wall) > limit.saturating_add(SLEPT_AFTER)
}

impl Cached {
    /// The applications `inner` finds, rescanned in full every `period`
    /// (at least a second) while kept, by the system's clock.
    pub fn new(inner: Arc<dyn Discovery>, period: Duration) -> Cached {
        Cached {
            inner,
            shared: Arc::new(Shared {
                period: period.max(SHORTEST_PERIOD),
                clock: Mutex::new(Arc::new(SystemClock)),
                state: Mutex::default(),
                wake: Condvar::new(),
                listeners: Mutex::default(),
            }),
        }
    }

    /// This list telling its grace and its period by `clock` rather than
    /// the system's clock, for tests and development builds, which move it
    /// (as the launcher's, [`crate::Launcher::with_clock`]).
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn with_clock(self, clock: Arc<dyn Clock>) -> Cached {
        *lock(&self.shared.clock) = clock;
        self
    }

    /// The kept list, scanned first if there is none, which starts the
    /// worker keeping it current (see [`Cached`]).
    pub fn catalog(&self) -> Result<Arc<Catalog>, String> {
        let session = {
            let state = self.shared.lock();
            if let Some(list) = &state.list {
                return Ok(list.shown.clone());
            }
            state.session
        };
        let sources = self.inner.sources()?;
        let now = self.shared.now();
        let mut state = self.shared.lock();
        if state.session != session {
            // Released meanwhile: answered, but nothing is kept.
            return Ok(Arc::new(Catalog::new(sources)));
        }
        if let Some(list) = &state.list {
            // Another first scan was kept first.
            return Ok(list.shown.clone());
        }
        let list = rescanned(None, sources, now);
        let shown = list.shown.clone();
        state.list = Some(list);
        if !state.working {
            let (inner, shared) = (self.inner.clone(), Arc::downgrade(&self.shared));
            state.working = std::thread::Builder::new()
                .name("pane-applications".into())
                .spawn(move || work(&inner, &shared, session))
                .is_ok();
        }
        Ok(shown)
    }

    /// The kept list, if there is one; never scans.
    fn kept(&self) -> Option<Arc<Catalog>> {
        self.shared
            .lock()
            .list
            .as_ref()
            .map(|list| list.shown.clone())
    }

    /// The path of the primary source of the application `id` names, from
    /// the kept list; when none is kept, from a scan that is not kept
    /// (keeping and watching a list is for the packages that ask for it).
    fn primary_path(&self, id: &str) -> Option<String> {
        let catalog = match self.kept() {
            Some(catalog) => catalog,
            None => Arc::new(Catalog::new(self.inner.sources().ok()?)),
        };
        catalog
            .find(id)
            .map(|application| application.primary().path.clone())
    }
}

impl Drop for Cached {
    fn drop(&mut self) {
        self.release();
    }
}

impl Applications for Cached {
    fn installed(&self) -> Result<Vec<Application>, String> {
        Ok(self.catalog()?.applications())
    }

    fn open(&self, id: &str) -> Result<(), String> {
        if let Some(path) = self.primary_path(id) {
            return self.inner.open(&path);
        }
        if is_identity(id) {
            return Err("it is no longer installed".into());
        }
        // A path from before identities that no application has now: the
        // adapter explains why (it no longer exists, it is not one).
        self.inner.open(id)
    }

    fn source(&self, id: &str) -> Option<String> {
        self.primary_path(id)
    }

    fn current_id(&self, id: &str) -> Option<String> {
        self.kept()?
            .find(id)
            .map(|application| application.id.clone())
    }

    fn icon_source(&self, id: &str) -> Option<String> {
        // As `primary_path`: the kept list, else a scan that is not kept.
        let catalog = match self.kept() {
            Some(catalog) => catalog,
            None => Arc::new(Catalog::new(self.inner.sources().ok()?)),
        };
        let application = catalog.find(id)?;
        // A packaged app's own logo, with its light and dark variants, over
        // the shell's image of a shortcut to it.
        let packaged = matches!(application.primary().key, super::Key::Package { .. })
            .then(|| {
                application
                    .sources
                    .iter()
                    .find(|source| source.path.starts_with(r"shell:AppsFolder\"))
            })
            .flatten();
        Some(packaged.unwrap_or(application.primary()).path.clone())
    }

    fn on_change(&self, changed: Arc<dyn Fn() + Send + Sync>) {
        lock(&self.shared.listeners).push(changed);
    }

    fn release(&self) {
        let mut state = self.shared.lock();
        state.session += 1;
        state.list = None;
        state.working = false;
        state.changed = None;
        state.again = None;
        state.lost = false;
        drop(state);
        // The worker ends, and drops its watch, on its own thread.
        self.shared.wake.notify_all();
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Now by the list's clock, in milliseconds.
    fn now(&self) -> u64 {
        let clock = lock(&self.clock).clone();
        clock.now()
    }

    /// Takes in what `session`'s watcher saw.
    fn report(&self, session: u64, change: Change) {
        let mut state = self.lock();
        if state.session != session {
            return;
        }
        let now = Instant::now();
        match change {
            Change::Lost => state.lost = true,
            Change::Changed | Change::Completing => {
                let first = state.changed.map_or(now, |(first, _)| first);
                state.changed = Some((first, (now + DEBOUNCE).min(first + MOST_DEBOUNCE)));
                if change == Change::Completing {
                    state.again = Some(now + DEBOUNCE + COMPLETING_AGAIN);
                }
            }
        }
        drop(state);
        self.wake.notify_all();
    }

    /// Tells the listeners that the listed applications changed.
    fn tell(&self) {
        let listeners = lock(&self.listeners).clone();
        for listener in listeners {
            listener();
        }
    }

    /// Waits for `session`'s next job; `None` once the session ended.
    fn next_job(&self, session: u64) -> Option<Job> {
        let mut state = self.lock();
        loop {
            if state.session != session {
                return None;
            }
            let list = state.list.as_ref()?;
            let (instant, now) = (Instant::now(), self.now());
            let next_leaving = list.next_leaving();
            let reconcile_at = list.looked.saturating_add(millis(self.period));
            if std::mem::take(&mut state.lost) {
                state.changed = None;
                return Some(Job::Rescan);
            }
            if state.changed.is_some_and(|(_, due)| due <= instant) {
                state.changed = None;
                return Some(Job::Rescan);
            }
            if state.again.is_some_and(|again| again <= instant) {
                state.again = None;
                return Some(Job::Rescan);
            }
            if reconcile_at <= now {
                return Some(Job::Rescan);
            }
            if next_leaving.is_some_and(|until| until <= now) {
                return Some(Job::Expire);
            }
            let mut limit = TICK.min(Duration::from_millis(reconcile_at - now));
            if let Some((_, due)) = state.changed {
                limit = limit.min(due - instant);
            }
            if let Some(again) = state.again {
                limit = limit.min(again - instant);
            }
            if let Some(until) = next_leaving {
                limit = limit.min(Duration::from_millis(until - now));
            }
            state = self
                .wake
                .wait_timeout(state, limit)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
            let real = instant.elapsed();
            let wall = Duration::from_millis(self.now().saturating_sub(now));
            if slept(limit, real, wall) {
                state.lost = true;
            }
        }
    }
}

/// Starts watching `inner`'s places for `session`; `None` when the adapter
/// cannot, and the list relies on its period.
fn start_watch(inner: &Arc<dyn Discovery>, shared: &Weak<Shared>, session: u64) -> Option<Watch> {
    let reports = shared.clone();
    let changes: Changes = Arc::new(move |change| {
        if let Some(shared) = reports.upgrade() {
            shared.report(session, change);
        }
    });
    inner.watch(changes).ok()
}

/// `session`'s worker: watches `inner`'s places, and rescans and expires
/// the list as they and the clock require, until the session ends or the
/// list is dropped.
fn work(inner: &Arc<dyn Discovery>, shared: &Weak<Shared>, session: u64) {
    let mut watch = start_watch(inner, shared, session);
    {
        let Some(strong) = shared.upgrade() else {
            return;
        };
        // A clock set other than by time passing (a test's) wakes the
        // worker to look at the grace and the period again.
        let woken = shared.clone();
        let clock = lock(&strong.clock).clone();
        clock.on_change(Box::new(move || {
            if let Some(shared) = woken.upgrade() {
                let _state = shared.lock();
                shared.wake.notify_all();
            }
        }));
    }
    loop {
        let Some(shared) = shared.upgrade() else {
            return;
        };
        let Some(job) = shared.next_job(session) else {
            return;
        };
        let changed = match job {
            Job::Rescan => {
                let found = inner.sources();
                let now = shared.now();
                let mut state = shared.lock();
                if state.session != session {
                    return;
                }
                let Some(list) = state.list.as_mut() else {
                    return;
                };
                match found {
                    Ok(sources) => {
                        let after = rescanned(Some(list), sources, now);
                        let changed = !same_applications(&list.shown, &after.shown);
                        *list = after;
                        changed
                    }
                    // A failed rescan keeps the list, until the next one.
                    Err(_) => {
                        list.looked = now;
                        false
                    }
                }
            }
            Job::Expire => {
                let now = shared.now();
                let mut state = shared.lock();
                if state.session != session {
                    return;
                }
                let Some(list) = state.list.as_mut() else {
                    return;
                };
                match list.expired(now) {
                    Some(after) => {
                        let changed = !same_applications(&list.shown, &after.shown);
                        *list = after;
                        changed
                    }
                    None => false,
                }
            }
        };
        if changed {
            shared.tell();
        }
        // A watch missing a place is made again, the new one before the old
        // one goes, so nothing is missed between them.
        if matches!(job, Job::Rescan)
            && watch.as_ref().is_none_or(|watch| !watch.complete())
            && let Some(again) = start_watch(inner, &Arc::downgrade(&shared), session)
        {
            watch = Some(again);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::applications::Key;

    fn source(name: &str, target: &str) -> Source {
        Source::new(
            Key::program(target, ""),
            format!(r"C:\Menu\{name}.lnk"),
            name,
            r"C:\Menu",
            2,
        )
    }

    fn names(list: &List) -> Vec<String> {
        let mut names: Vec<String> = list
            .shown
            .applications()
            .into_iter()
            .map(|application| application.name)
            .collect();
        names.sort();
        names
    }

    const SECOND: u64 = 1000;

    #[test]
    fn an_application_whose_last_source_went_stays_for_its_grace() {
        let editor = source("Editor", r"C:\Editor\editor.exe");
        let mail = source("Mail", r"C:\Mail\mail.exe");
        let first = rescanned(None, vec![editor.clone(), mail], 0);

        let missed = rescanned(Some(&first), vec![editor.clone()], 10 * SECOND);
        assert_eq!(names(&missed), ["Editor", "Mail"]);
        // Its grace counts from when it was first missed.
        let again = rescanned(Some(&missed), vec![editor], 12 * SECOND);
        assert_eq!(names(&again), ["Editor", "Mail"]);
        assert!(again.expired(14 * SECOND).is_none());
        let expired = again.expired(15 * SECOND).unwrap();
        assert_eq!(names(&expired), ["Editor"]);
    }

    #[test]
    fn an_application_found_again_in_its_grace_never_leaves() {
        let editor = source("Editor", r"C:\Editor\editor.exe");
        let mail = source("Mail", r"C:\Mail\mail.exe");
        let first = rescanned(None, vec![editor.clone(), mail.clone()], 0);
        let missed = rescanned(Some(&first), vec![editor.clone()], SECOND);
        // Reinstalled, its shortcut now in another folder.
        let back = Source {
            path: r"C:\Other\Mail.lnk".into(),
            ..mail
        };
        let found = rescanned(Some(&missed), vec![editor, back], 2 * SECOND);
        assert!(found.leaving.is_empty());
        assert!(found.expired(60 * SECOND).is_none());
        assert_eq!(names(&found), ["Editor", "Mail"]);
    }

    #[test]
    fn a_shortcut_that_now_opens_another_program_does_not_stay_as_the_old_one() {
        let editor = source("Editor", r"C:\Editor\editor.exe");
        let first = rescanned(None, vec![editor], 0);
        let retargeted = source("Editor", r"C:\Writer\writer.exe");
        let after = rescanned(Some(&first), vec![retargeted.clone()], SECOND);
        assert!(after.leaving.is_empty());
        assert_eq!(after.shown.applications().len(), 1);
        assert_eq!(after.shown.applications()[0].id, retargeted.key.id());
    }

    #[test]
    fn a_renamed_shortcut_keeps_the_application_and_changes_its_title() {
        let editor = source("Editor", r"C:\Editor\editor.exe");
        let first = rescanned(None, vec![editor], 0);
        let renamed = source("Code Editor", r"C:\Editor\editor.exe");
        let after = rescanned(Some(&first), vec![renamed], SECOND);
        assert!(after.leaving.is_empty());
        assert_eq!(names(&after), ["Code Editor"]);
        assert!(!same_applications(&first.shown, &after.shown));
        assert_eq!(
            first.shown.applications()[0].id,
            after.shown.applications()[0].id
        );
    }

    #[test]
    fn the_same_applications_in_another_order_are_no_change() {
        let editor = source("Editor", r"C:\Editor\editor.exe");
        let mail = source("Mail", r"C:\Mail\mail.exe");
        let a = rescanned(None, vec![editor.clone(), mail.clone()], 0);
        let b = rescanned(None, vec![mail, editor], 0);
        assert!(same_applications(&a.shown, &b.shown));
    }

    #[test]
    fn a_wait_far_longer_than_its_bound_is_a_sleep() {
        let limit = Duration::from_secs(5);
        let second = Duration::from_secs(1);
        assert!(!slept(limit, second, second));
        assert!(!slept(limit, limit, limit + second));
        // The monotonic clock counted the sleep (Windows), or only the
        // wall clock did (Linux, macOS).
        assert!(slept(limit, Duration::from_secs(3600), second));
        assert!(slept(limit, second, Duration::from_secs(3600)));
    }
}
