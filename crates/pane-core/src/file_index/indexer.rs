//! The indexer coordinator (#126 "Where the index lives", #175): opens the
//! file index in Pane's cache folder while a package that uses it
//! (`"fileIndex": true`) is enabled and not paused, catches it up at start,
//! walks what needs walking once the launcher was first shown (or a while
//! after start), keeps it current from the system's change source, and
//! answers searches for the packages that use it, naming each entry by an
//! id the host gives it, which it checks again before anything is done
//! with it.
//!
//! One thread of its own per activation ("pane-file-index"), at background
//! priority, does all the writing; queries read the index at the same time
//! and never wait for a walk. Disabling the last package that uses the
//! index stops watching at once and keeps the index on disk; uninstalling
//! it deletes the index ([`Indexer::delete`]).
//!
//! The safety valves (#126 "Priority and safety valves", #176), each
//! listed on the File search page ([`Indexer::problems`]): a folder that
//! changes constantly is taken out of the index until the user includes it
//! again (churn quarantine, [`Valves::churn_changes`]); a walk stops at a
//! ceiling of entries ([`WalkOptions::max_entries`]); indexing stops
//! writing while the disk holding the cache has too little free space, and
//! starts again once there is room ([`Valves::free_space_floor`]); and a
//! folder that does not answer is skipped for the walk
//! ([`WalkOptions::hung_after`]). Indexing also pauses while the computer
//! sleeps and resumes a few seconds after it wakes, counting only awake
//! time in its limits (`power`, [`Valves::resume_after`]).

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use super::category::Category;
use super::changes::{Caught, CaughtUpBy, ChangeSource, Changed, FolderIds, Sink, Watching};
use super::format::{EntryKind, Meta};
use super::journal::JournalCursor;
use super::power::{Awake, Pause, RESUME_AFTER, SystemAwake};
use super::reconcile::reconcile;
use super::scope::{Admitted, CACHE_TAG, Scope, ScopeRules};
use super::space::{FREE_SPACE_FLOOR, FreeSpace, free_space};
use super::store::{Change, FileIndex, Hit, IndexError, IndexRecord, Opened, Query};
use super::volume::{VolumeKinds, volume_kind};
use super::walker::{WalkOptions, WalkReport, walk, walk_folders};
use super::wording::{count_words, size_words, span_words};
use super::{Entry, lower_current_thread};
use crate::files::{canonical, is_network_path, program_named, runs_as_program};
use crate::util::lock;

/// How long after start the first full walk (or a reconciling walk the
/// catch-up asks for) waits for the launcher to be shown before it starts
/// anyway.
pub const FIRST_WALK_DELAY: Duration = Duration::from_secs(60);

/// How long changes are gathered before they are applied together.
pub const SETTLE: Duration = Duration::from_millis(100);

/// How often the folders Linux cannot watch, and the roots on a network
/// share (never watched), are reconciled.
pub const RECONCILE_UNWATCHED: Duration = Duration::from_secs(5 * 60);

/// The most entries one search answers.
pub const MAX_RESULTS: usize = 200;

/// The ids Pane keeps per package, the newest; an older one is no longer
/// known ("search again").
const IDS_KEPT: usize = 10_000;

/// The most folders whose repository (`.git`) the coordinator remembers
/// seeing; past it, it forgets them all.
const REPOSITORIES_KEPT: usize = 10_000;

/// How long before a run started a repository must have been made for the
/// run to count it as there already (file systems' clocks and times are
/// not exact).
const MADE_BEFORE: Duration = Duration::from_secs(2);

/// Folders a re-check (#186) reads before the coordinator handles the
/// changes reported meanwhile, so that re-checking a whole root never holds
/// up live changes.
const RECHECK_FOLDERS: usize = 64;

/// The record of the user's file search rules, beside `installed.json`.
pub const RULES_FILE: &str = "file-search.json";

/// The index's folder in Pane's cache folder.
pub const INDEX_DIR: &str = "file-index";

/// The safety valves' thresholds (see the module docs); the proposed
/// values by default, smaller in tests.
#[derive(Clone)]
pub struct Valves {
    /// Changes in one folder within [`Valves::churn_window`] that count
    /// as constant churn (about 1,000 a minute).
    pub churn_changes: u64,
    pub churn_window: Duration,
    /// Windows of churn in a row that take the folder out of the index
    /// (3 minutes).
    pub churn_windows: u32,
    /// Indexing stops writing while less is free ([`FREE_SPACE_FLOOR`]).
    pub free_space_floor: u64,
    /// How often a stop for space looks again.
    pub space_retry: Duration,
    /// How the free space of the volume holding a folder is read: the
    /// system's ([`free_space`]) unless a test says otherwise.
    pub free_space: FreeSpace,
    /// How long the computer has slept (see `power`): the system's clocks
    /// unless a test puts a computer of its own to sleep.
    pub awake: Arc<dyn Awake>,
    /// How long indexing waits after the computer woke before it resumes
    /// ([`RESUME_AFTER`]).
    pub resume_after: Duration,
}

impl Default for Valves {
    fn default() -> Valves {
        Valves {
            churn_changes: 1_000,
            churn_window: Duration::from_secs(60),
            churn_windows: 3,
            free_space_floor: FREE_SPACE_FLOOR,
            space_retry: Duration::from_secs(60),
            free_space: Arc::new(free_space),
            awake: Arc::new(SystemAwake),
            resume_after: RESUME_AFTER,
        }
    }
}

/// How the indexer runs: where the index is kept, what it covers, and how
/// it learns what changed.
#[derive(Clone)]
pub struct IndexerConfig {
    /// The index's own folder, in Pane's cache folder.
    pub dir: PathBuf,
    /// The roots and rules (see [`ScopeRules`]); the user's own
    /// ([`UserRules`]) are applied over them.
    pub rules: ScopeRules,
    pub source: Arc<dyn ChangeSource>,
    /// See [`FIRST_WALK_DELAY`]; a test sets none.
    pub first_walk_delay: Duration,
    /// See [`SETTLE`].
    pub settle: Duration,
    pub walk: WalkOptions,
    /// See [`RECONCILE_UNWATCHED`].
    pub reconcile_unwatched: Duration,
    /// See [`Valves`].
    pub valves: Valves,
    /// How the kind of volume holding a folder is told (a network share or
    /// a removable drive is left out unless the user includes other
    /// volumes): the system's ([`volume_kind`]) unless a test says
    /// otherwise.
    pub volumes: VolumeKinds,
}

impl IndexerConfig {
    /// This system's: the index in `cache_dir`'s `file-index`, the home
    /// folder `home` as the root, Pane's own folders (`own`, its data and
    /// cache folders) never indexed, and this system's change source.
    pub fn native(cache_dir: &Path, home: PathBuf, own: Vec<PathBuf>) -> IndexerConfig {
        let mut rules = ScopeRules::for_home(home);
        rules.always_excluded = own;
        rules.always_excluded.push(cache_dir.to_path_buf());
        IndexerConfig {
            dir: cache_dir.join(INDEX_DIR),
            rules,
            source: super::changes::native(),
            first_walk_delay: FIRST_WALK_DELAY,
            settle: SETTLE,
            walk: WalkOptions::default(),
            reconcile_unwatched: RECONCILE_UNWATCHED,
            valves: Valves::default(),
            volumes: Arc::new(volume_kind),
        }
    }

    /// `rules` ready to apply, told what kind of volume holds a folder as
    /// this configuration says.
    fn scope(&self, rules: ScopeRules) -> Scope {
        Scope::with_volumes(rules, self.volumes.clone())
    }
}

/// The rules the user changes on the File search page (#176), kept in
/// Pane's own record [`RULES_FILE`], not in any extension's data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UserRules {
    /// Folders indexed besides the home folder.
    pub added_roots: Vec<PathBuf>,
    pub excluded_folders: Vec<PathBuf>,
    /// In `.gitignore` syntax.
    pub excluded_patterns: Vec<String>,
    pub include_hidden: bool,
    pub use_ignore_files: bool,
    pub default_exclusions: bool,
    pub include_other_volumes: bool,
    /// Folders Pane took out of the index because they changed constantly
    /// (churn quarantine, #176), until the user includes them again: left
    /// out as an excluded folder is, and listed apart on the File search
    /// page with the reason.
    pub quarantined: Vec<PathBuf>,
}

impl Default for UserRules {
    fn default() -> UserRules {
        UserRules {
            added_roots: Vec::new(),
            excluded_folders: Vec::new(),
            excluded_patterns: Vec::new(),
            include_hidden: false,
            use_ignore_files: true,
            default_exclusions: true,
            include_other_volumes: false,
            quarantined: Vec::new(),
        }
    }
}

impl UserRules {
    /// `base` with these rules over it.
    pub fn applied_to(&self, base: &ScopeRules) -> ScopeRules {
        let mut rules = base.clone();
        for root in &self.added_roots {
            if !rules.roots.contains(root) {
                rules.roots.push(root.clone());
            }
        }
        rules.excluded_folders = self.excluded_folders.clone();
        for folder in &self.quarantined {
            if !rules.excluded_folders.contains(folder) {
                rules.excluded_folders.push(folder.clone());
            }
        }
        rules.excluded_patterns = self.excluded_patterns.clone();
        rules.include_hidden = self.include_hidden;
        rules.use_ignore_files = self.use_ignore_files;
        rules.default_exclusions = self.default_exclusions;
        rules.include_other_volumes = self.include_other_volumes;
        rules
    }

    /// The rules recorded in `dir`'s [`RULES_FILE`]; the defaults when
    /// there is none or it cannot be read.
    pub fn read(dir: &Path) -> UserRules {
        #[derive(Deserialize)]
        struct Record {
            rules: UserRules,
        }
        std::fs::read(dir.join(RULES_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Record>(&bytes).ok())
            .map(|record| record.rules)
            .unwrap_or_default()
    }

    /// Records these rules in `dir`'s [`RULES_FILE`].
    pub fn write(&self, dir: &Path) -> Result<(), String> {
        let record = serde_json::json!({ "version": 1, "rules": self });
        let text = serde_json::to_vec_pretty(&record).expect("rules are JSON");
        crate::atomic::write_atomically(
            &dir.join(RULES_FILE),
            &text,
            crate::atomic::Readers::Default,
        )
        .map_err(|error| format!("Pane could not record the file search rules: {error}"))
    }
}

/// Where the index is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IndexState {
    /// No enabled package uses it (or this Pane keeps none): nothing is
    /// indexed or watched.
    #[default]
    Off,
    /// Being built or caught up; what is indexed is searched meanwhile.
    Building,
    /// Current, and kept so while Pane runs.
    Current,
    /// Stopped; [`IndexStatus::reason`] says why.
    Stopped,
}

/// What the File search page, Search Files and extensions are told.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexStatus {
    pub state: IndexState,
    /// Entries in the index (an estimate while it changes).
    pub entries: u64,
    /// Entries a walk in progress has found so far.
    pub found: u64,
    /// Why it is off, waiting or stopped, or what to know about it.
    pub reason: Option<String>,
    /// Waiting for the launcher to be shown (or a while) before walking.
    pub waiting: bool,
    /// How it was last caught up, and when.
    pub caught_up: Option<(CaughtUpBy, SystemTime)>,
    /// Folders that could not be read, counted, and the first ones named.
    pub unreadable: u64,
    pub unreadable_folders: Vec<PathBuf>,
    /// Folders macOS's privacy protection did not let Pane read, told
    /// from the system's answer, counted, and the first ones named.
    pub refused: u64,
    pub refused_folders: Vec<PathBuf>,
    /// Folders not watched (Linux's watch limit), reconciled every few
    /// minutes instead.
    pub unwatched: u64,
    /// A walk stopped at the ceiling of 5 million entries.
    pub ceiling_reached: bool,
    /// Indexing stopped writing: the disk holding Pane's cache has less
    /// free space than [`Valves::free_space_floor`]. It starts again by
    /// itself once there is room.
    pub low_space: bool,
    /// Folders that did not answer within [`WalkOptions::hung_after`] in
    /// the last walk, counted, and the first ones named: skipped for that
    /// walk.
    pub hung: u64,
    pub hung_folders: Vec<PathBuf>,
    /// The computer woke a moment ago: indexing waits a few seconds
    /// ([`Valves::resume_after`]) before it resumes.
    pub resuming: bool,
}

/// What a [`Problem`] is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProblemKind {
    /// A folder that could not be read.
    Unreadable,
    /// A folder macOS did not allow Pane to read (Desktop, Documents,
    /// Downloads).
    Refused,
    /// Folders Linux cannot watch (its watch limit was reached).
    Unwatched,
    /// A folder taken out of the index because it changed constantly.
    Churned,
    /// A folder that did not answer in time.
    Hung,
    /// A walk stopped at the ceiling of entries.
    Ceiling,
    /// Indexing stopped for want of free space.
    LowSpace,
}

/// Something the File search page lists about the index (#176): what
/// happened, the folder it concerns (if one), why, and what the user can do
/// about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub kind: ProblemKind,
    pub folder: Option<PathBuf>,
    pub reason: String,
    pub remedy: String,
}

/// The remedy for a folder macOS's privacy protection refused.
const ALLOW_IN_PRIVACY_SETTINGS: &str = "Allow Pane under System Settings › Privacy & Security › Files and Folders, then rebuild      the index";

/// How to search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    pub kind: Option<EntryKind>,
    pub category: Option<Category>,
    pub sort: Sort,
    pub limit: usize,
    pub offset: usize,
}

impl Default for SearchOptions {
    fn default() -> SearchOptions {
        SearchOptions {
            kind: None,
            category: None,
            sort: Sort::Relevance,
            limit: 20,
            offset: 0,
        }
    }
}

/// How results are ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    /// Best match first; for a blank query, newest first.
    #[default]
    Relevance,
    /// Most recently modified first.
    Modified,
}

/// An entry a search found, for an extension: the id Pane gave it and what
/// the index keeps of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub id: String,
    pub path: PathBuf,
    /// Its own name, with replacement characters where the system's name
    /// is not valid Unicode.
    pub name: String,
    /// Its folder for people: below the home folder as `~/…`.
    pub folder: String,
    pub kind: EntryKind,
    /// Whether opening it would run a program, told from its name (a
    /// program by its executable bit alone is told when it is opened).
    pub program: bool,
    pub size: u64,
    pub modified: u64,
    pub volume: u64,
}

/// An entry by the id Pane gave it, as the host names it in a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownEntry {
    pub path: PathBuf,
    pub name: String,
    pub folder: String,
    pub kind: EntryKind,
    pub program: bool,
}

/// An entry checked again, ready to act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checked {
    /// Its canonical path.
    pub path: PathBuf,
    pub kind: EntryKind,
    /// Opening it would run a program (its name, or on macOS and Linux its
    /// executable bit, says so).
    pub program: bool,
}

/// What the coordinator thread is told.
pub(crate) enum Message {
    Changed(Changed),
    /// The launcher was shown: a deferred walk may start.
    Shown,
    Stop,
}

/// The file index, its coordinator and the ids it gave. Cloning shares it;
/// the default is one with no configuration, which indexes nothing.
#[derive(Clone, Default)]
pub struct Indexer(Arc<Inner>);

#[derive(Default)]
struct Inner {
    shared: Mutex<Shared>,
    /// Told whenever the status or the work in flight changes.
    changed: Condvar,
}

#[derive(Default)]
struct Shared {
    config: Option<IndexerConfig>,
    user_rules: UserRules,
    /// The folder whose [`RULES_FILE`] records the user's rules, if they
    /// are recorded: a folder taken out for churn is recorded there too.
    rules_dir: Option<PathBuf>,
    /// A folder granted under #29 is being moved into the roots.
    moving_grants: bool,
    /// The packages that use the index and may run, by identity key.
    users: BTreeSet<String>,
    /// Whether the launcher was shown since Pane started.
    shown: bool,
    run: Option<Run>,
    index: Option<Arc<FileIndex>>,
    scope: Option<Arc<Scope>>,
    status: IndexStatus,
    /// The coordinator is doing something (catching up, walking,
    /// applying).
    busy: bool,
    /// Counts the indexes opened: an id of another one is not known.
    generation: u64,
    issued: HashMap<String, Issued>,
    /// Whether each root other than the home folder was there when last
    /// looked, and when.
    roots_seen: Option<(Instant, Vec<(PathBuf, bool)>)>,
}

impl Drop for Inner {
    /// The last handle gone (Pane quits, a test's launcher is dropped):
    /// the coordinator stops, lets go of the index and saves its cursors.
    fn drop(&mut self) {
        let shared = self
            .shared
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(run) = shared.run.take() {
            run.stop();
        }
    }
}

/// The ids given one package, the newest kept.
#[derive(Default)]
struct Issued {
    next: u64,
    by_id: HashMap<String, (PathBuf, EntryKind)>,
    order: VecDeque<String>,
}

/// One activation's coordinator thread.
struct Run {
    stop: Arc<AtomicBool>,
    sender: Sender<Message>,
    /// Each activation counts only its own reports: a stopped native
    /// watcher can still be finishing a callback when its replacement runs.
    queued: Arc<Queued>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Run {
    /// Asks the thread to stop; it drops its watch at once.
    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.sender.send(Message::Stop);
    }
}

impl Indexer {
    fn shared(&self) -> MutexGuard<'_, Shared> {
        lock(&self.0.shared)
    }

    /// Runs the index as `config` says from now on, with the user's
    /// `rules`; an activation in progress starts again with them.
    pub fn configure(&self, config: IndexerConfig, rules: UserRules) {
        let restart = {
            let mut shared = self.shared();
            shared.config = Some(config);
            shared.user_rules = rules;
            shared.run.is_some()
        };
        if restart {
            self.restart();
        } else {
            self.follow();
        }
    }

    /// Whether this Pane keeps a file index at all.
    pub fn configured(&self) -> bool {
        self.shared().config.is_some()
    }

    /// The packages (identity keys) that use the index and may run now: the
    /// index is kept current while there is one, and they alone may search
    /// it. Starts or stops watching as that changes.
    pub fn set_users(&self, users: BTreeSet<String>) {
        let changed = {
            let mut shared = self.shared();
            let changed = shared.users != users;
            shared.users = users;
            changed
        };
        if changed {
            self.follow();
        }
    }

    /// Notes that the launcher was shown: a deferred walk may start.
    pub fn launcher_shown(&self) {
        let mut shared = self.shared();
        if shared.shown {
            return;
        }
        shared.shown = true;
        if let Some(run) = &shared.run {
            run.queued.sent();
            if run.sender.send(Message::Shown).is_err() {
                run.queued.unsent();
            }
        }
    }

    /// Starts the coordinator if a package uses the index and none runs, or
    /// stops it if none does.
    fn follow(&self) {
        let mut shared = self.shared();
        let wanted = !shared.users.is_empty() && shared.config.is_some();
        match (wanted, shared.run.is_some()) {
            (true, false) => self.start(&mut shared),
            (false, true) => {
                if let Some(run) = shared.run.take() {
                    run.stop();
                }
                // Let go of once the coordinator has stopped: nothing of the
                // index stays open while no package uses it.
                shared.index = None;
                shared.scope = None;
                shared.generation += 1;
                shared.issued.clear();
                shared.status = IndexStatus {
                    state: IndexState::Off,
                    entries: shared.status.entries,
                    caught_up: shared.status.caught_up,
                    reason: Some("no enabled extension uses file search".into()),
                    ..IndexStatus::default()
                };
                shared.busy = false;
                self.0.changed.notify_all();
            }
            (true, true) | (false, false) => {}
        }
    }

    fn start(&self, shared: &mut Shared) {
        let Some(config) = shared.config.clone() else {
            return;
        };
        let rules = shared.user_rules.applied_to(&config.rules);
        let (sender, receiver) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        shared.status.state = IndexState::Building;
        shared.status.reason = None;
        shared.status.found = 0;
        shared.status.low_space = false;
        shared.busy = true;
        let weak = Arc::downgrade(&self.0);
        let thread_stop = stop.clone();
        let thread_sender = sender.clone();
        let queued = Arc::new(Queued::default());
        let thread_queued = queued.clone();
        let spawned = std::thread::Builder::new()
            .name("pane-file-index".into())
            .spawn(move || {
                lower_current_thread();
                coordinate(
                    weak,
                    config,
                    rules,
                    thread_sender,
                    receiver,
                    thread_stop,
                    thread_queued,
                );
            });
        match spawned {
            Ok(thread) => {
                shared.run = Some(Run {
                    stop,
                    sender,
                    queued,
                    thread: Some(thread),
                })
            }
            Err(error) => {
                shared.busy = false;
                shared.status.state = IndexState::Stopped;
                shared.status.reason = Some(format!("Pane could not start indexing: {error}"));
            }
        }
        self.0.changed.notify_all();
    }

    /// Stops the coordinator and waits (a few seconds at most) for its
    /// thread to end, so that the index is no longer written.
    fn stop_and_wait(&self) {
        let run = self.shared().run.take();
        if let Some(mut run) = run {
            run.stop();
            if let Some(thread) = run.thread.take() {
                let deadline = Instant::now() + Duration::from_secs(10);
                while !thread.is_finished() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                if thread.is_finished() {
                    let _ = thread.join();
                }
            }
        }
    }

    /// Starts the coordinator again, as after a change of the rules.
    fn restart(&self) {
        self.stop_and_wait();
        self.close_index();
        self.follow();
    }

    /// Lets go of the open index, so that it can be opened again or
    /// deleted; the ids it gave are no longer known.
    fn close_index(&self) {
        let mut shared = self.shared();
        shared.index = None;
        shared.scope = None;
        shared.generation += 1;
        shared.issued.clear();
    }

    /// The user's rules now.
    pub fn user_rules(&self) -> UserRules {
        self.shared().user_rules.clone()
    }

    /// The roots and rules this Pane starts from, before the user's; `None`
    /// when it keeps no index.
    pub fn base_rules(&self) -> Option<ScopeRules> {
        self.shared()
            .config
            .as_ref()
            .map(|config| config.rules.clone())
    }

    /// Notes that granted folders are being moved into the roots; `false`
    /// if they already are.
    pub(crate) fn start_moving_grants(&self) -> bool {
        let mut shared = self.shared();
        !std::mem::replace(&mut shared.moving_grants, true)
    }

    /// Notes that the move [`Indexer::start_moving_grants`] began is done.
    pub(crate) fn done_moving_grants(&self) {
        self.shared().moving_grants = false;
    }

    /// The roots and rules in force now (the defaults with the user's
    /// over them); `None` when this Pane keeps no index.
    pub fn rules(&self) -> Option<ScopeRules> {
        let shared = self.shared();
        let config = shared.config.as_ref()?;
        Some(shared.user_rules.applied_to(&config.rules))
    }

    /// Applies the user's new `rules`: the index is brought to them, changing
    /// only what they affect where it can (an added root or a folder no
    /// longer excluded is walked alone; a removed root's or a newly excluded
    /// folder's entries go; any other change walks every root again), now
    /// if a package uses it, else when one next does (the index keeps the
    /// rules it was built under). Not recorded: see
    /// [`Indexer::change_rules`]. Blocking: waits for the coordinator to
    /// stop first.
    pub fn set_user_rules(&self, rules: UserRules) {
        if self.shared().user_rules == rules {
            return;
        }
        self.stop_and_wait();
        self.shared().user_rules = rules;
        // The index's roots changed: it is opened again with them.
        self.close_index();
        self.follow();
    }

    /// Records the user's rules in `dir`'s [`RULES_FILE`] from now on,
    /// whatever changes them: the File search page, or a folder taken out
    /// for churn.
    pub fn keep_rules_in(&self, dir: PathBuf) {
        self.shared().rules_dir = Some(dir);
    }

    /// Records the user's new `rules` (where [`Indexer::keep_rules_in`]
    /// says) and applies them, as [`Indexer::set_user_rules`] does. The
    /// folders taken out for churn stay out: only
    /// [`Indexer::include_again`] puts one back. Blocking.
    pub fn change_rules(&self, mut rules: UserRules) -> Result<(), String> {
        let (current, dir) = {
            let shared = self.shared();
            (shared.user_rules.clone(), shared.rules_dir.clone())
        };
        rules.quarantined = current.quarantined.clone();
        if rules == current {
            return Ok(());
        }
        if let Some(dir) = &dir {
            rules.write(dir)?;
        }
        self.set_user_rules(rules);
        Ok(())
    }

    /// Puts `folder`, taken out of the index for churn, back in: recorded,
    /// and walked again. Blocking.
    pub fn include_again(&self, folder: &Path) -> Result<(), String> {
        let (mut rules, dir) = {
            let shared = self.shared();
            (shared.user_rules.clone(), shared.rules_dir.clone())
        };
        let before = rules.quarantined.len();
        rules
            .quarantined
            .retain(|quarantined| quarantined != folder);
        if rules.quarantined.len() == before {
            return Ok(());
        }
        if let Some(dir) = &dir {
            rules.write(dir)?;
        }
        self.set_user_rules(rules);
        Ok(())
    }

    /// What the File search page lists about the index (#176): folders that
    /// could not be read (macOS's refusals apart, told by what the system
    /// answered, see `privacy`), are not watched, churned
    /// or did not answer, a walk stopped at the ceiling, and a stop for
    /// space, each with why and what to do.
    pub fn problems(&self) -> Vec<Problem> {
        let shared = self.shared();
        let status = &shared.status;
        let Some(config) = &shared.config else {
            return Vec::new();
        };
        let mut problems = Vec::new();
        if status.low_space {
            problems.push(Problem {
                kind: ProblemKind::LowSpace,
                folder: None,
                reason: format!(
                    "Less than {} is free on the disk that holds Pane's cache, so indexing \
                     stopped",
                    size_words(config.valves.free_space_floor)
                ),
                remedy: "Free some space; indexing starts again by itself".into(),
            });
        }
        if status.ceiling_reached {
            problems.push(Problem {
                kind: ProblemKind::Ceiling,
                folder: None,
                reason: format!(
                    "Indexing stopped at {} entries",
                    count_words(config.walk.max_entries)
                ),
                remedy: "Exclude folders you do not search, then rebuild the index".into(),
            });
        }
        for folder in &shared.user_rules.quarantined {
            problems.push(Problem {
                kind: ProblemKind::Churned,
                folder: Some(folder.clone()),
                reason: format!(
                    "It changed more than {} times in {}, {} times in a row, so Pane took it \
                     out of the index",
                    count_words(config.valves.churn_changes),
                    span_words(config.valves.churn_window),
                    config.valves.churn_windows
                ),
                remedy: "Include it again once it is calmer".into(),
            });
        }
        let hung_after = config.walk.hung_after.unwrap_or(super::walker::HUNG_AFTER);
        for folder in &status.hung_folders {
            problems.push(Problem {
                kind: ProblemKind::Hung,
                folder: Some(folder.clone()),
                reason: format!(
                    "It did not answer within {}, so Pane skipped it for this walk",
                    span_words(hung_after)
                ),
                remedy: "Rebuild the index once it answers".into(),
            });
        }
        let named_hung = status.hung_folders.len() as u64;
        if status.hung > named_hung {
            problems.push(Problem {
                kind: ProblemKind::Hung,
                folder: None,
                reason: format!(
                    "{} more folders did not answer in time",
                    count_words(status.hung - named_hung)
                ),
                remedy: "Rebuild the index once they answer".into(),
            });
        }
        for folder in &status.refused_folders {
            problems.push(Problem {
                kind: ProblemKind::Refused,
                folder: Some(folder.clone()),
                reason: "macOS did not allow Pane to read it".into(),
                remedy: ALLOW_IN_PRIVACY_SETTINGS.into(),
            });
        }
        let named_refused = status.refused_folders.len() as u64;
        if status.refused > named_refused {
            problems.push(Problem {
                kind: ProblemKind::Refused,
                folder: None,
                reason: format!(
                    "macOS did not allow Pane to read {} more folders",
                    count_words(status.refused - named_refused)
                ),
                remedy: ALLOW_IN_PRIVACY_SETTINGS.into(),
            });
        }
        for folder in &status.unreadable_folders {
            problems.push(Problem {
                kind: ProblemKind::Unreadable,
                folder: Some(folder.clone()),
                reason: "Pane cannot read it, so what is in it is not indexed".into(),
                remedy: "Make sure your account may open it, then rebuild the index".into(),
            });
        }
        let named = status.unreadable_folders.len() as u64;
        if status.unreadable > named {
            problems.push(Problem {
                kind: ProblemKind::Unreadable,
                folder: None,
                reason: format!(
                    "{} more folders could not be read",
                    count_words(status.unreadable - named)
                ),
                remedy: "Make sure your account may open them, then rebuild the index".into(),
            });
        }
        if status.unwatched > 0 {
            problems.push(Problem {
                kind: ProblemKind::Unwatched,
                folder: None,
                reason: format!(
                    "{} folders are not watched, since the system's limit on watched folders \
                     was reached; Pane re-checks them every {}",
                    count_words(status.unwatched),
                    span_words(config.reconcile_unwatched)
                ),
                remedy: "Raise the limit (the fs.inotify.max_user_watches setting) to watch \
                         them all"
                    .into(),
            });
        }
        problems
    }

    /// Deletes the index (the last package that used it was uninstalled):
    /// stops the coordinator, waits for it, and removes the index's folder.
    /// Blocking.
    pub fn delete(&self) -> Result<(), String> {
        self.stop_and_wait();
        self.close_index();
        let dir = self
            .shared()
            .config
            .as_ref()
            .map(|config| config.dir.clone());
        {
            let mut shared = self.shared();
            shared.status = IndexStatus {
                reason: Some("no installed extension uses file search".into()),
                ..IndexStatus::default()
            };
            self.0.changed.notify_all();
        }
        match dir {
            Some(dir) if dir.exists() => std::fs::remove_dir_all(&dir)
                .map_err(|error| format!("Pane could not delete the file index: {error}")),
            _ => Ok(()),
        }
    }

    /// Deletes the index and builds it again, if a package uses it.
    /// Blocking.
    pub fn rebuild(&self) -> Result<(), String> {
        self.delete()?;
        self.follow();
        Ok(())
    }

    /// Where the index is.
    pub fn status(&self) -> IndexStatus {
        self.shared().status.clone()
    }

    /// The index's folder, if this Pane keeps one.
    pub fn dir(&self) -> Option<PathBuf> {
        self.shared()
            .config
            .as_ref()
            .map(|config| config.dir.clone())
    }

    /// Waits until the index has settled: caught up, walked, every change
    /// reported so far applied, or stopped or off. `false` if it did not
    /// within `limit`. For tests (`Launcher::wait_for_file_index`).
    pub fn wait_until_settled(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        let mut shared = self.shared();
        loop {
            let settled = !shared.busy
                && shared
                    .run
                    .as_ref()
                    .is_none_or(|run| run.queued.0.load(Ordering::SeqCst) == 0)
                && shared.status.state != IndexState::Building;
            if settled {
                return true;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            shared = self
                .0
                .changed
                .wait_timeout(shared, left.min(Duration::from_millis(50)))
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }

    /// The entries `query` finds for the package with identity key `owner`,
    /// each with the id Pane gives it. Never waits for a walk: answers from
    /// what is indexed. A blank query lists the most recently modified.
    pub fn search(
        &self,
        owner: &str,
        query: &str,
        options: SearchOptions,
    ) -> Result<Vec<Found>, String> {
        let (index, home, generation) = {
            let shared = self.shared();
            if !shared.users.contains(owner) {
                return Err("Pane's file index is for extensions whose pane.json sets \
                            \"fileIndex\": true"
                    .into());
            }
            let Some(index) = shared.index.clone() else {
                return Ok(Vec::new());
            };
            let home = shared
                .config
                .as_ref()
                .and_then(|config| config.rules.home.clone());
            (index, home, shared.generation)
        };
        let away = self.roots_away();
        let limit = options.limit.clamp(1, MAX_RESULTS);
        let wanted = options.offset.saturating_add(limit);
        let keep = |hit: &Hit| {
            options
                .category
                .is_none_or(|category| category.holds(&hit.path, hit.meta.kind))
                && !away.iter().any(|root| hit.path.starts_with(root))
        };
        let blank = query.trim().is_empty();
        // Asked for more than wanted, as entries are left out after the
        // query (a category, a root away), up to a bound.
        let mut asked = if options.category.is_some() || options.sort == Sort::Modified {
            wanted.max(500)
        } else {
            wanted
        };
        let mut hits = loop {
            let page = if blank {
                index.recent(asked, options.kind)
            } else {
                index.search(&Query {
                    text: query,
                    limit: asked,
                    offset: 0,
                    kind: options.kind,
                })
            };
            let exhausted = page.len() < asked;
            let kept: Vec<Hit> = page.into_iter().filter(keep).collect();
            if kept.len() >= wanted || exhausted || asked >= 20_000 {
                break kept;
            }
            asked *= 4;
        };
        if options.sort == Sort::Modified {
            hits.sort_by_key(|hit| std::cmp::Reverse(hit.meta.modified));
        }
        let page: Vec<Hit> = hits.into_iter().skip(options.offset).take(limit).collect();
        let mut shared = self.shared();
        if shared.generation != generation {
            // The index was closed meanwhile: its entries are not given ids.
            return Ok(Vec::new());
        }
        let issued = shared.issued.entry(owner.to_owned()).or_default();
        Ok(page
            .into_iter()
            .map(|hit| {
                let id = issue(issued, generation, &hit.path, hit.meta.kind);
                let (name, folder) = describe(&hit.path, home.as_deref());
                Found {
                    id,
                    program: hit.meta.kind != EntryKind::Folder && program_named(&hit.path),
                    name,
                    folder,
                    kind: hit.meta.kind,
                    size: hit.meta.size,
                    modified: hit.meta.modified,
                    volume: hit.meta.volume,
                    path: hit.path,
                }
            })
            .collect())
    }

    /// The roots other than the home folder that are away now (an
    /// unplugged drive): their entries are kept but not found. Looked at
    /// at most once every two seconds.
    fn roots_away(&self) -> Vec<PathBuf> {
        let roots = {
            let shared = self.shared();
            if let Some((when, seen)) = &shared.roots_seen
                && when.elapsed() < Duration::from_secs(2)
            {
                return seen
                    .iter()
                    .filter(|(_, there)| !there)
                    .map(|(root, _)| root.clone())
                    .collect();
            }
            let Some(scope) = &shared.scope else {
                return Vec::new();
            };
            let home = scope.rules().home.clone();
            // A root left out (a network share or a removable drive) is not
            // looked at; what the index may still hold of it (one away when
            // the index opened, back since) is not found either (#184).
            let kept = scope.kept_roots();
            scope
                .rules()
                .roots
                .iter()
                .filter(|root| Some(*root) != home.as_ref())
                .map(|root| (root.clone(), kept.contains(root)))
                .collect::<Vec<_>>()
        };
        let seen: Vec<(PathBuf, bool)> = roots
            .into_iter()
            .map(|(root, kept)| {
                let there = kept && root.is_dir();
                (root, there)
            })
            .collect();
        let away = seen
            .iter()
            .filter(|(_, there)| !there)
            .map(|(root, _)| root.clone())
            .collect();
        self.shared().roots_seen = Some((Instant::now(), seen));
        away
    }

    /// The entry with id `id` that a search of the package with identity
    /// key `owner` found, as the host names it; `None` for an id Pane did
    /// not give that package from the index open now.
    pub fn known(&self, owner: &str, id: &str) -> Option<KnownEntry> {
        let shared = self.shared();
        let (path, kind) = lookup(&shared, owner, id)?;
        let home = shared
            .config
            .as_ref()
            .and_then(|config| config.rules.home.clone());
        let (name, folder) = describe(&path, home.as_deref());
        Some(KnownEntry {
            program: kind != EntryKind::Folder && program_named(&path),
            path,
            name,
            folder,
            kind,
        })
    }

    /// The entry with id `id` of the package with identity key `owner`,
    /// checked again now that Pane is to act on it: Pane gave the id from
    /// the index open now, the path is not a network path, it is still
    /// there, of the kind indexed and not a link, it is still in the index
    /// scope, and its canonical path is under a root. Blocking.
    ///
    /// The scope is told with what the coordinator keeps of the folders
    /// between batches of changes (#186), read again for a folder not
    /// watched live (a network share, a folder Linux cannot watch, any root
    /// when watching could not start), or with the rules read again when
    /// the global ignore file changed since they were.
    pub fn checked(&self, owner: &str, id: &str) -> Result<Checked, String> {
        let (path, kind, scope, volumes) = {
            let shared = self.shared();
            let (path, kind) =
                lookup(&shared, owner, id).ok_or("Pane no longer knows it; search again")?;
            let volumes = shared.config.as_ref().map(|config| config.volumes.clone());
            (path, kind, shared.scope.clone(), volumes)
        };
        if is_network_path(&path) {
            return Err("it is on a network location".into());
        }
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err("it no longer exists".into());
            }
            Err(error) => return Err(format!("Pane cannot read it: {error}")),
        };
        if metadata.file_type().is_symlink() && kind != EntryKind::Link {
            return Err("it is now a link".into());
        }
        let now = if metadata.is_dir() {
            EntryKind::Folder
        } else if metadata.file_type().is_symlink() {
            EntryKind::Link
        } else {
            EntryKind::File
        };
        match (kind, now) {
            (EntryKind::File, EntryKind::Folder) => return Err("it is now a folder".into()),
            (EntryKind::Folder, EntryKind::File) => return Err("it is now a file".into()),
            (EntryKind::Link, _) => {
                return Err("it is a link, which Pane does not follow".into());
            }
            _ => {}
        }
        if let Some(scope) = &scope {
            let is_dir = now == EntryKind::Folder;
            let admitted = if scope.global_ignore_changed() {
                let volumes = volumes.unwrap_or_else(|| Arc::new(volume_kind));
                let fresh = Scope::with_volumes(scope.rules().clone(), volumes);
                fresh.admits(&path, is_dir, &mut Admitted::default())
            } else {
                scope.admits_kept(&path, is_dir)
            };
            if !admitted {
                return Err("it is no longer in the folders file search covers".into());
            }
            let resolved =
                canonical(&path).map_err(|error| format!("Pane cannot read it: {error}"))?;
            let inside = scope
                .rules()
                .roots
                .iter()
                .any(|root| canonical(root).is_ok_and(|root| resolved.starts_with(root)));
            if !inside {
                return Err("it is no longer inside the folders file search covers".into());
            }
            let program = now == EntryKind::File && runs_as_program(&resolved, &metadata);
            return Ok(Checked {
                path: resolved,
                kind: now,
                program,
            });
        }
        Err("file search is off".into())
    }
}

/// The path and kind of the entry with id `id` of `owner`, if Pane gave it
/// from the index open now.
fn lookup(shared: &Shared, owner: &str, id: &str) -> Option<(PathBuf, EntryKind)> {
    let generation = id
        .strip_prefix('i')?
        .split_once('-')?
        .0
        .parse::<u64>()
        .ok()?;
    if generation != shared.generation || shared.index.is_none() {
        return None;
    }
    shared.issued.get(owner)?.by_id.get(id).cloned()
}

/// Gives the entry at `path` an id for one package, keeping the newest.
fn issue(issued: &mut Issued, generation: u64, path: &Path, kind: EntryKind) -> String {
    issued.next += 1;
    let id = format!("i{generation}-{}", issued.next);
    issued.by_id.insert(id.clone(), (path.to_path_buf(), kind));
    issued.order.push_back(id.clone());
    while issued.order.len() > IDS_KEPT {
        if let Some(old) = issued.order.pop_front() {
            issued.by_id.remove(&old);
        }
    }
    id
}

/// The name of the entry at `path` and its folder for people: below the
/// home folder `home` as `~/…` (`~` itself for an entry directly in it),
/// elsewhere its full path, with `/` between names below the home folder.
pub fn describe(path: &Path, home: Option<&Path>) -> (String, String) {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let parent = path.parent().unwrap_or(path);
    let folder = match home.and_then(|home| parent.strip_prefix(home).ok()) {
        Some(below) if below.as_os_str().is_empty() => "~".to_owned(),
        Some(below) => {
            let parts: Vec<String> = below
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            format!("~/{}", parts.join("/"))
        }
        None => parent.display().to_string(),
    };
    (name, folder)
}

/// The coordinator's thread (see the module docs). Holds the indexer only
/// weakly, so that dropping every handle ends it.
fn coordinate(
    inner: Weak<Inner>,
    config: IndexerConfig,
    rules: ScopeRules,
    sender: Sender<Message>,
    receiver: Receiver<Message>,
    stop: Arc<AtomicBool>,
    queued: Arc<Queued>,
) {
    let Some(mut coordinator) =
        Coordinator::open(inner.clone(), config, rules, sender, stop, queued.clone())
    else {
        // What was sent meanwhile is no longer waited for.
        let left: Vec<Message> = receiver.try_iter().collect();
        if let Some(inner) = inner.upgrade() {
            let count = counted(&left);
            if count > 0 {
                queued.0.fetch_sub(count, Ordering::SeqCst);
            }
            inner.changed.notify_all();
        }
        return;
    };
    coordinator.run(receiver);
}

struct Coordinator {
    inner: Weak<Inner>,
    config: IndexerConfig,
    scope: Arc<Scope>,
    index: Arc<FileIndex>,
    sender: Sender<Message>,
    stop: Arc<AtomicBool>,
    queued: Arc<Queued>,
    started: Instant,
    watching: Option<Box<dyn Watching>>,
    /// The cursors to save with the index next.
    cursors: Vec<JournalCursor>,
    /// Work waiting for the launcher to be shown, or the delay.
    full_walk: bool,
    walk: Vec<PathBuf>,
    reconcile: Vec<PathBuf>,
    /// Folders Linux cannot watch, and when they were last reconciled
    /// (with the roots on a network share).
    unwatched: Vec<PathBuf>,
    unwatched_reconciled: Instant,
    /// The roots on a network share the rules keep (other volumes
    /// included): never watched, and reconciled at start and every
    /// [`IndexerConfig::reconcile_unwatched`].
    network: Vec<PathBuf>,
    caught_up: Option<CaughtUpBy>,
    /// Changes counted per folder, for churn quarantine.
    churn: HashMap<PathBuf, Churn>,
    /// The disk holding the index is short of space: nothing is written
    /// until there is room again.
    low_space: bool,
    /// Changes were let go while short of space: every root is reconciled
    /// once there is room.
    missed: bool,
    /// The pause after a sleep, shared with the walker's threads.
    pause: Arc<Pause>,
    /// How long the computer had slept when this run started: the first
    /// walk's delay counts awake time only.
    asleep_at_start: Duration,
    /// When this run started, by the clock file times are told in.
    started_at: SystemTime,
    /// Folders whose ignore rules may have changed, still to re-check with
    /// everything under them (#186): read [`RECHECK_FOLDERS`] at a time
    /// between batches of changes, once walks may run, each read putting the
    /// folders under it here in turn ([`Coordinator::queue_recheck`]).
    recheck: Vec<PathBuf>,
    /// Whether each folder whose `.git` a change named held a repository
    /// then, so that what Git does inside one is told from one made or
    /// deleted.
    repositories: HashMap<PathBuf, bool>,
    /// The system's event history (macOS) is being replayed: a repository
    /// it names may have been made while Pane was not running.
    replaying: bool,
}

/// The changes counted in one folder: in the window that started `since`,
/// and how many windows in a row before it were busy.
struct Churn {
    since: Instant,
    changes: u64,
    busy: u32,
}

impl Coordinator {
    /// Opens the index and catches it up; `None` (with the status saying
    /// why) when it cannot be opened or the run was stopped.
    fn open(
        inner: Weak<Inner>,
        config: IndexerConfig,
        rules: ScopeRules,
        sender: Sender<Message>,
        stop: Arc<AtomicBool>,
        queued: Arc<Queued>,
    ) -> Option<Coordinator> {
        let scope = Arc::new(config.scope(rules));
        let existing = {
            let inner = inner.upgrade()?;
            lock(&inner.shared).index.clone()
        };
        let index = match existing {
            Some(index) => index,
            None => match open_index(&config.dir, &scope.rules().roots, &stop) {
                Ok((index, opened)) => {
                    let index = Arc::new(index);
                    let inner = inner.upgrade()?;
                    let mut shared = lock(&inner.shared);
                    if stop.load(Ordering::Relaxed) {
                        return None;
                    }
                    shared.index = Some(index.clone());
                    if let Opened::Rebuilt(why) = opened {
                        shared.status.reason =
                            Some(format!("Pane rebuilds the file index, since {why}"));
                    }
                    index
                }
                Err(why) => {
                    if let Some(inner) = inner.upgrade() {
                        let mut shared = lock(&inner.shared);
                        shared.status.state = IndexState::Stopped;
                        shared.status.reason = Some(why);
                        shared.busy = false;
                        inner.changed.notify_all();
                    }
                    return None;
                }
            },
        };
        {
            let inner = inner.upgrade()?;
            let mut shared = lock(&inner.shared);
            shared.scope = Some(scope.clone());
            shared.roots_seen = None;
        }
        // Built under other rules (the user changed them, or the home
        // folder moved): brought to these first.
        let mut record = index.record();
        let mut walk_again = Vec::new();
        if record.rules.as_ref() != Some(scope.rules()) {
            if let Some(before) = &record.rules {
                walk_again = apply_rule_change(&index, before, scope.rules());
                record = index.record();
            }
            record.rules = Some(scope.rules().clone());
            let _ = index.set_record(record);
        }
        // A root the rules leave out (on a network share or a removable
        // drive, other volumes not included) holds nothing in the index:
        // what an earlier index held of it goes.
        let left_out: Vec<Change> = (0..scope.rules().roots.len())
            .filter(|&root| scope.leaves_out_root(root))
            .map(|root| scope.rules().roots[root].clone())
            .filter(|path| index.get(path).is_some())
            .map(Change::RemoveUnder)
            .collect();
        if !left_out.is_empty() {
            let _ = index.apply(&left_out);
        }
        if let Some(inner) = inner.upgrade() {
            lock(&inner.shared).status.entries = index.stats().stored;
        }
        let pause = Arc::new(Pause::new(
            config.valves.awake.clone(),
            config.valves.resume_after,
        ));
        // A network share is never watched: no change drops what the scope
        // would keep of its folders, so it keeps nothing of them (#186).
        let network = scope.network_roots();
        scope.keep_nothing_under(&network);
        // A root whose volume the system did not say in time (a stalled
        // network mount) is taken for a network share (#184): said so.
        let silent = scope.roots_not_answering();
        if !silent.is_empty()
            && let Some(inner) = inner.upgrade()
        {
            let named: Vec<String> = silent
                .iter()
                .map(|root| root.display().to_string())
                .collect();
            lock(&inner.shared).status.reason = Some(format!(
                "{} did not answer in time, so Pane treats it as a network share",
                named.join(", ")
            ));
        }
        let mut coordinator = Coordinator {
            inner,
            asleep_at_start: config.valves.awake.asleep(),
            pause,
            started: Instant::now(),
            started_at: SystemTime::now(),
            recheck: Vec::new(),
            repositories: HashMap::new(),
            replaying: false,
            cursors: Vec::new(),
            full_walk: false,
            walk: walk_again,
            reconcile: Vec::new(),
            unwatched: Vec::new(),
            unwatched_reconciled: Instant::now(),
            network,
            watching: None,
            caught_up: None,
            churn: HashMap::new(),
            low_space: false,
            missed: false,
            config,
            scope,
            index,
            sender,
            stop,
            queued,
        };
        coordinator.catch_up();
        Some(coordinator)
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Whether the disk holding the index has room to write (at least
    /// [`Valves::free_space_floor`] free, or the system does not say). The
    /// status follows: stopped while there is none, and indexing starts
    /// again once there is (every root reconciled, if changes were let go
    /// meanwhile).
    fn room(&mut self) -> bool {
        let valves = &self.config.valves;
        let short = (valves.free_space)(&self.config.dir)
            .is_some_and(|free| free < valves.free_space_floor);
        if short && !self.low_space {
            self.low_space = true;
            let floor = size_words(valves.free_space_floor);
            self.status(|shared| {
                shared.busy = false;
                shared.status.state = IndexState::Stopped;
                shared.status.waiting = false;
                shared.status.low_space = true;
                shared.status.reason = Some(format!(
                    "Pane stopped indexing: less than {floor} is free on the disk that holds \
                     its cache"
                ));
            });
        } else if !short && self.low_space {
            self.low_space = false;
            if self.missed {
                self.missed = false;
                if !self.full_walk {
                    self.cursors = self.config.source.cursors(&self.scope);
                    for root in self.scope.kept_roots() {
                        if !self.reconcile.contains(&root) {
                            self.reconcile.push(root);
                        }
                    }
                    self.caught_up = Some(CaughtUpBy::ReconcilingWalk);
                }
            }
            self.status(|shared| {
                shared.status.low_space = false;
                shared.status.state = IndexState::Building;
                shared.status.reason = None;
            });
        }
        !short
    }

    /// Counts the changes `changed` reports, per folder they are in, at
    /// `now`: the folders that changed more than [`Valves::churn_changes`]
    /// times in each of [`Valves::churn_windows`] windows in a row, to take
    /// out of the index. Only a change of an entry the rules admit counts
    /// (one in `.git`, `node_modules`, an ignored or a hidden folder never
    /// does), told with what the scope keeps of the folders between
    /// batches (#186), which the batch is then looked at with. A root
    /// itself, and a folder out of the index already, is never taken out.
    fn count_churn(&mut self, changed: &[PathBuf], now: Instant) -> Vec<PathBuf> {
        if self.config.valves.churn_windows == 0 {
            return Vec::new();
        }
        let mut admitted: HashMap<&Path, bool> = HashMap::new();
        let mut per_folder: HashMap<&Path, u64> = HashMap::new();
        for path in changed {
            let counts = *admitted
                .entry(path.as_path())
                .or_insert_with(|| self.admits_change(path));
            if let Some(folder) = path.parent().filter(|_| counts) {
                *per_folder.entry(folder).or_default() += 1;
            }
        }
        let valves = &self.config.valves;
        let window = valves.churn_window.max(Duration::from_millis(1));
        let rules = self.scope.rules();
        let mut churned = Vec::new();
        for (folder, count) in per_folder {
            let counted = self.scope.root_of(folder).is_some()
                && !rules.roots.iter().any(|root| root == folder);
            if !counted {
                continue;
            }
            let churn = self.churn.entry(folder.to_path_buf()).or_insert(Churn {
                since: now,
                changes: 0,
                busy: 0,
            });
            let elapsed = now.saturating_duration_since(churn.since);
            if elapsed >= window {
                let passed = (elapsed.as_nanos() / window.as_nanos()).min(u128::from(u32::MAX));
                let passed = passed as u32;
                churn.busy = if passed == 1 && churn.changes > valves.churn_changes {
                    churn.busy + 1
                } else {
                    0
                };
                churn.since += window * passed;
                churn.changes = 0;
            }
            churn.changes += count;
            if churn.changes > valves.churn_changes && churn.busy + 1 >= valves.churn_windows {
                churned.push(folder.to_path_buf());
            }
        }
        for folder in &churned {
            self.churn.remove(folder);
        }
        if self.churn.len() > 4096 {
            self.churn
                .retain(|_, churn| now.saturating_duration_since(churn.since) < window * 2);
        }
        churned
    }

    /// Whether the rules admit the entry at `path` that a change reported:
    /// a folder or not as the disk says, or, once it is gone, as the index
    /// held it.
    fn admits_change(&self, path: &Path) -> bool {
        let is_dir = match path.symlink_metadata() {
            Ok(metadata) => metadata.is_dir(),
            Err(_) => self
                .index
                .get(path)
                .is_some_and(|meta| meta.kind == EntryKind::Folder),
        };
        self.scope.admits_kept(path, is_dir)
    }

    /// Drops what the scope keeps of the folders the changes `reports` name
    /// (#186), and answers the folders whose ignore rules they may have
    /// changed, to re-check with everything under them:
    ///
    /// - a folder named (made, deleted, renamed, its attributes changed):
    ///   what is kept of it and of every folder under it;
    /// - a folder's `.gitignore`, `.ignore`, `CACHEDIR.TAG` or
    ///   `.git/info/exclude`: that folder, kept no more, and re-checked;
    /// - a folder's `.git`: the same, once a repository was made or deleted
    ///   there (not for what Git does inside one).
    ///
    /// Also answers the folders a repository was made in, so that a source
    /// watching each folder watches the repository's own rules too (Linux,
    /// `.git/info`).
    fn forget_changed(&mut self, reports: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let named: BTreeSet<&Path> = reports.iter().map(PathBuf::as_path).collect();
        let mut recheck = Vec::new();
        let mut made = Vec::new();
        for path in named {
            self.scope.forget_kept(path);
            let Some((folder, repository)) = rules_folder(path) else {
                continue;
            };
            if repository {
                if !self.repository_changed(folder) {
                    continue;
                }
                if self.repositories.get(folder) == Some(&true) {
                    made.push(folder.to_path_buf());
                }
            }
            self.scope.forget_kept(folder);
            recheck.push(folder.to_path_buf());
        }
        (recheck, made)
    }

    /// Queues `folder` to re-check with everything under it (#186): not
    /// when a folder queued already holds it (that one's re-check reads it
    /// too, after this change), and in place of the folders queued under
    /// it.
    fn queue_recheck(&mut self, folder: PathBuf) {
        if self.recheck.iter().any(|queued| folder.starts_with(queued)) {
            return;
        }
        self.recheck.retain(|queued| !queued.starts_with(&folder));
        self.recheck.push(folder);
    }

    /// Whether a repository was made or deleted in `folder`, whose `.git` a
    /// change named: `.git` is there now and was not when last named, or
    /// the other way. Named for the first time, it was there before only if
    /// it was made before this run started, unless the system's history is
    /// being replayed: on Windows, what Git does inside a repository names
    /// its `.git` too.
    fn repository_changed(&mut self, folder: &Path) -> bool {
        let now = folder.join(".git").symlink_metadata();
        let there = now.is_ok();
        let before = self.repositories.get(folder).copied().or_else(|| {
            let made = now.as_ref().ok()?.created().ok()?;
            let early = made.checked_add(MADE_BEFORE)? < self.started_at;
            (early && !self.replaying).then_some(true)
        });
        if self.repositories.len() >= REPOSITORIES_KEPT {
            self.repositories.clear();
        }
        self.repositories.insert(folder.to_path_buf(), there);
        before != Some(there)
    }

    /// Re-checks a few of the folders whose ignore rules may have changed
    /// (#186, [`Coordinator::recheck`]): each read again under the rules as
    /// they are now (`reconcile::recheck`), whatever its time, and compared
    /// with what the index holds, so that what the rules leave out now goes
    /// and what they admit now comes in; a folder that does not answer
    /// keeps what the index holds of it. The changes to apply.
    fn recheck_some(&mut self) -> Vec<Change> {
        let rechecked = super::reconcile::recheck(
            &self.index,
            &self.scope,
            &mut self.recheck,
            &self.config.walk,
            &self.stop,
            RECHECK_FOLDERS,
        );
        self.note_hung(rechecked.hung_folders, false);
        rechecked.changes
    }

    /// `rules` as this run's scope from now on would apply them: keeping
    /// nothing, and nothing ever of the folders the scope before kept
    /// nothing of (not watched live).
    fn scope_with(&self, rules: ScopeRules) -> Arc<Scope> {
        let scope = Arc::new(self.config.scope(rules));
        scope.keep_nothing_under(&self.scope.kept_nothing_under());
        scope
    }

    /// Takes `folder`, which changes constantly, out of the index until the
    /// user includes it again: its entries go, the rules leave it out from
    /// now on, and Pane's own record and the File search page say so.
    fn quarantine(&mut self, folder: PathBuf) {
        let mut rules = self.scope.rules().clone();
        if !rules.excluded_folders.contains(&folder) {
            rules.excluded_folders.push(folder.clone());
        }
        let scope = self.scope_with(rules);
        self.scope = scope.clone();
        let _ = self.index.apply(&[Change::RemoveUnder(folder.clone())]);
        let mut record = self.index.record();
        record.rules = Some(scope.rules().clone());
        let _ = self.index.set_record(record);
        self.walk.retain(|walked| !walked.starts_with(&folder));
        self.reconcile.retain(|walked| !walked.starts_with(&folder));
        self.unwatched.retain(|walked| !walked.starts_with(&folder));
        self.recheck.retain(|walked| !walked.starts_with(&folder));
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let (rules, dir) = {
            let mut shared = lock(&inner.shared);
            if self.stopped() {
                return;
            }
            if !shared.user_rules.quarantined.contains(&folder) {
                shared.user_rules.quarantined.push(folder);
            }
            shared.scope = Some(scope);
            inner.changed.notify_all();
            (shared.user_rules.clone(), shared.rules_dir.clone())
        };
        if let Some(dir) = dir {
            let _ = rules.write(&dir);
        }
    }

    /// Notes the folders a walk found not answering: in place of those
    /// noted before when `replace`, else beside them.
    fn note_hung(&self, folders: Vec<PathBuf>, replace: bool) {
        if folders.is_empty() && !replace {
            return;
        }
        self.status(|shared| {
            if replace {
                shared.status.hung_folders.clear();
            }
            for folder in folders {
                if !shared.status.hung_folders.contains(&folder) {
                    shared.status.hung_folders.push(folder);
                }
            }
            shared.status.hung = shared.status.hung_folders.len() as u64;
        });
    }

    /// Changes the status, and tells whoever waits.
    fn status(&self, change: impl FnOnce(&mut Shared)) {
        if let Some(inner) = self.inner.upgrade() {
            let mut shared = lock(&inner.shared);
            if self.stopped() {
                return;
            }
            change(&mut shared);
            inner.changed.notify_all();
        }
    }

    /// Catches up with what changed while Pane was not running (or notes
    /// that a first walk is due), and starts watching.
    fn catch_up(&mut self) {
        let record = self.index.record();
        // The folder-id table, read from the index at most once for the
        // catch-up and the watch setup together (#187).
        let index = self.index.clone();
        let mut folders = FolderIds::new(&index);
        if !record.built {
            self.full_walk = true;
            self.walk.clear();
        } else if !self.room() {
            // Short of space: caught up by a reconciling walk once there is
            // room.
            self.missed = true;
        } else {
            match self.config.source.catch_up(
                &self.index,
                &self.scope,
                &record.cursors,
                &mut folders,
                &self.stop,
            ) {
                Caught::Changes {
                    changes,
                    walk,
                    reconcile,
                    recheck,
                    cursors,
                    how,
                    note,
                } => {
                    let _ = self.index.apply(&changes);
                    folders.apply(&changes);
                    // An ignore file, a repository or a cache tag that
                    // changed while Pane was not running: its folder is
                    // re-checked with the walks (#186), as is each folder
                    // the source found its rules may have changed in.
                    for change in &changes {
                        let path = match change {
                            Change::Put(entry) => &entry.path,
                            Change::Remove(path) | Change::RemoveUnder(path) => path,
                        };
                        if let Some((folder, repository)) = rules_folder(path) {
                            if repository {
                                let there = folder.join(".git").symlink_metadata().is_ok();
                                self.repositories.insert(folder.to_path_buf(), there);
                            }
                            self.queue_recheck(folder.to_path_buf());
                        }
                    }
                    for folder in recheck {
                        self.queue_recheck(folder);
                    }
                    self.walk.extend(walk);
                    self.reconcile = reconcile;
                    self.walk_added_roots();
                    self.cursors = cursors;
                    // Replayed through the live stream (macOS).
                    self.replaying = how == CaughtUpBy::EventHistory;
                    self.caught_up = Some(how);
                    if let Some(note) = note {
                        self.status(|shared| shared.status.reason = Some(note));
                    }
                }
                Caught::Reconcile(why) => {
                    self.cursors = self.config.source.cursors(&self.scope);
                    self.reconcile = self.scope.kept_roots();
                    self.caught_up = Some(CaughtUpBy::ReconcilingWalk);
                    self.status(|shared| shared.status.reason = Some(why));
                }
            }
            // A network share is not watched, and the system keeps no
            // records of it Pane reads: it is caught up by a reconciling
            // walk.
            for root in &self.network {
                if !self.reconcile.contains(root) && !self.walk.contains(root) {
                    self.reconcile.push(root.clone());
                }
            }
        }
        // Watching starts before any walk, so that what changes during it
        // is not lost (Linux adds its folders' watches once they are known).
        let watched = if self.full_walk || !self.config.source.watches_folders() {
            Vec::new()
        } else {
            self.indexed_folders(&mut folders)
        };
        let sink = Sink::new(self.sender.clone(), self.queued());
        match self
            .config
            .source
            .watch(&self.scope, &self.cursors, watched, sink)
        {
            Ok(watching) => {
                self.watching = Some(watching);
                // A root away is not watched once it is back: nothing would
                // drop what the scope kept of it (#186).
                let away: Vec<PathBuf> = self
                    .scope
                    .watched_roots()
                    .into_iter()
                    .filter(|root| !root.is_dir())
                    .collect();
                self.scope.keep_nothing_under(&away);
            }
            Err(why) => {
                // Nothing reports a change: the scope keeps nothing (#186).
                self.scope.keep_nothing_under(&self.scope.kept_roots());
                self.status(|shared| shared.status.reason = Some(why));
            }
        }
        // What the catch-up learned of folders is dropped once watching
        // started: an ignore file changed between the two is reported by
        // neither (#186).
        self.scope.forget_all_kept();
        if !self.deferred() {
            self.save();
            self.settled();
        }
    }

    /// The counter of queued messages the indexer waits on.
    fn queued(&self) -> Option<Weak<dyn Counter>> {
        Some(Arc::downgrade(&self.queued) as Weak<dyn Counter>)
    }

    /// Walks the roots the rules keep that are there but not indexed yet:
    /// added since the index was built, whatever the system's records say.
    fn walk_added_roots(&mut self) {
        for root in self.scope.kept_roots() {
            if self.index.get(&root).is_none()
                && root.is_dir()
                && !self.walk.contains(&root)
                && !self.reconcile.contains(&root)
            {
                self.walk.push(root);
            }
        }
    }

    /// Every folder indexed now that is watched (not on a network share),
    /// shallowest first, from `table`.
    fn indexed_folders(&self, table: &mut FolderIds<'_>) -> Vec<PathBuf> {
        table.folders(|folder| self.on_network(folder))
    }

    /// Whether `folder` is under a root on a network share, which is not
    /// watched.
    fn on_network(&self, folder: &Path) -> bool {
        self.network.iter().any(|root| folder.starts_with(root))
    }

    /// Whether walks wait to run.
    fn deferred(&self) -> bool {
        self.full_walk
            || !self.walk.is_empty()
            || !self.reconcile.is_empty()
            || !self.recheck.is_empty()
    }

    /// Whether a deferred walk may run now: the launcher was shown, or the
    /// delay passed, in awake time.
    fn may_walk(&self) -> bool {
        let shown = self
            .inner
            .upgrade()
            .is_some_and(|inner| lock(&inner.shared).shown);
        shown || self.awake_since_start() >= self.config.first_walk_delay
    }

    /// How long this run has been going, the computer awake.
    fn awake_since_start(&self) -> Duration {
        let elapsed = self.started.elapsed();
        if super::power::INSTANT_COUNTS_SLEEP {
            let slept = self
                .config
                .valves
                .awake
                .asleep()
                .saturating_sub(self.asleep_at_start);
            elapsed.saturating_sub(slept)
        } else {
            elapsed
        }
    }

    /// Waits out the pause after a sleep: when the computer slept since
    /// the last look (or a walker's thread noticed it), indexing waits
    /// [`Valves::resume_after`] before it goes on, the status saying so,
    /// and churn is counted afresh (a sleep breaks a run of busy minutes).
    /// At once when the computer did not sleep.
    fn wait_after_sleep(&mut self) {
        let slept = self.pause.look();
        if slept.is_none() && !self.pause.on() {
            return;
        }
        if slept.is_some() {
            self.churn.clear();
        }
        self.status(|shared| shared.status.resuming = true);
        self.pause.hold(&self.stop);
        self.status(|shared| shared.status.resuming = false);
    }

    fn run(&mut self, receiver: Receiver<Message>) {
        loop {
            if self.stopped() {
                break;
            }
            self.wait_after_sleep();
            // Short of space: looked at again, and indexing starts again
            // once there is room.
            if self.low_space && self.room() && !self.deferred() {
                self.settled();
            }
            // Folders are still to re-check after a few (#186): the changes
            // reported meanwhile are handled before the next few.
            let mut rechecking = false;
            if self.deferred() && !self.low_space {
                if self.may_walk() {
                    self.do_walks();
                    rechecking = !self.recheck.is_empty() && !self.low_space && !self.stopped();
                    if !rechecking {
                        continue;
                    }
                } else {
                    self.status(|shared| {
                        shared.status.waiting = true;
                        shared.status.state = IndexState::Building;
                        shared.busy = false;
                    });
                }
            }
            let timeout = if rechecking {
                Some(Duration::ZERO)
            } else if self.low_space {
                Some(
                    self.config
                        .valves
                        .space_retry
                        .max(Duration::from_millis(10)),
                )
            } else if self.deferred() {
                Some(
                    self.config
                        .first_walk_delay
                        .saturating_sub(self.awake_since_start())
                        .max(Duration::from_millis(10)),
                )
            } else if self.reconciles_now_and_then() {
                Some(
                    self.config
                        .reconcile_unwatched
                        .saturating_sub(self.unwatched_reconciled.elapsed())
                        .max(Duration::from_millis(10)),
                )
            } else {
                None
            };
            let first = match timeout {
                Some(timeout) => match receiver.recv_timeout(timeout) {
                    Ok(message) => Some(message),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match receiver.recv() {
                    Ok(message) => Some(message),
                    Err(_) => break,
                },
            };
            let Some(first) = first else {
                if !self.low_space && !self.deferred() && self.reconciles_now_and_then() {
                    self.reconcile_unwatched();
                }
                continue;
            };
            // What comes within the settle time is applied together.
            let mut batch = vec![first];
            let settle = Instant::now() + self.config.settle;
            while let Some(left) = settle.checked_duration_since(Instant::now()) {
                match receiver.recv_timeout(left) {
                    Ok(message) => batch.push(message),
                    Err(_) => break,
                }
            }
            // A batch that came as the computer woke waits for the pause
            // after the sleep.
            self.wait_after_sleep();
            let count = counted(&batch);
            let stop = self.handle(batch);
            self.handled(count);
            if stop {
                break;
            }
        }
        // Stopped: watching stops now; what is in the log is kept, and the
        // cursors are saved once it is a segment.
        self.watching = None;
        // What was sent and never handled is no longer waited for.
        let left: Vec<Message> = receiver.try_iter().collect();
        self.handled(counted(&left));
        if self.low_space {
            // Nothing more is written; changes let go meanwhile are caught
            // up from the cursors saved before, next time.
            return;
        }
        let _ = self.index.flush();
        let mut record = self.index.record();
        if !self.cursors.is_empty() && record.built && !self.missed {
            record.cursors = self.cursors.clone();
            let _ = self.index.set_record(record);
        }
    }

    /// Notes that `count` counted messages were handled (or let go of).
    fn handled(&self, count: usize) {
        if let Some(inner) = self.inner.upgrade() {
            if count > 0 {
                self.queued.0.fetch_sub(count, Ordering::SeqCst);
            }
            inner.changed.notify_all();
        }
    }

    /// Handles a batch of messages; `true` to stop.
    fn handle(&mut self, batch: Vec<Message>) -> bool {
        // Every path reported, as often as it was: churn counts each report.
        let mut reports: Vec<PathBuf> = Vec::new();
        let mut rescan: Vec<PathBuf> = Vec::new();
        let mut stop = false;
        let now = Instant::now();
        // The cursors the batch reports, and whether its history ended:
        // taken only once its changes are applied, so that cursors past a
        // change never are saved without it (a batch that stops, a run
        // stopped meanwhile, leaves both as they were).
        let mut reported: Option<Vec<JournalCursor>> = None;
        let mut history_done = false;
        for message in batch {
            match message {
                Message::Stop => stop = true,
                Message::Shown => {}
                Message::Changed(Changed::Paths(changed)) => reports.extend(changed),
                Message::Changed(Changed::Rescan(folder)) => rescan.push(folder),
                Message::Changed(Changed::HistoryDone(cursors)) => {
                    reported = Some(cursors);
                    history_done = true;
                }
                Message::Changed(Changed::Cursors(cursors)) => reported = Some(cursors),
                Message::Changed(Changed::Unwatched(folders)) => {
                    // No change is reported there to drop what the scope
                    // keeps (#186).
                    self.scope.keep_nothing_under(&folders);
                    self.unwatched.extend(folders);
                    self.unwatched.sort();
                    self.unwatched.dedup();
                    let count = self.unwatched.len() as u64;
                    self.status(|shared| shared.status.unwatched = count);
                }
            }
        }
        if stop || self.stopped() {
            return true;
        }
        // What the rules admit is told with what the scope keeps of the
        // folders between batches (#186), first dropped for the folders
        // these changes may have changed; churn is counted only for the
        // paths the rules admit, and the paths are looked at the same way.
        let (mut recheck, made) = self.forget_changed(&reports);
        let rescan = self.rescan_folders(rescan);
        if !rescan.is_empty() {
            // Anything may have changed there, an ignore file changed in
            // place too, which no folder's time shows: reconciled now, and
            // re-checked whole after (#186).
            self.scope.forget_all_kept();
            recheck.extend(rescan.iter().cloned());
        }
        if self.scope.global_ignore_changed() {
            // The rules are read again, and every root re-checked if the
            // global ignore file itself changed.
            let scope = self.scope_with(self.scope.rules().clone());
            if !scope.same_global_ignore(&self.scope) {
                recheck.extend(scope.kept_roots());
            }
            self.scope = scope.clone();
            self.status(|shared| shared.scope = Some(scope));
        }
        // A repository made: its own rules are watched too where the source
        // watches each folder (Linux, `.git/info`).
        let made: Vec<PathBuf> = made
            .into_iter()
            .filter(|folder| !self.on_network(folder))
            .collect();
        if let Some(watching) = &mut self.watching
            && !made.is_empty()
        {
            watching.add_folders(&made);
        }
        if self.full_walk {
            // The first walk, still to come, reads every folder under the
            // rules as they are then.
            recheck.clear();
        }
        // A folder taken out for churn builds the rules again, keeping
        // nothing.
        let churned = self.count_churn(&reports, now);
        for folder in churned {
            self.quarantine(folder);
        }
        let paths: BTreeSet<PathBuf> = reports.into_iter().collect();
        if paths.is_empty() && rescan.is_empty() && recheck.is_empty() {
            self.take_cursors(reported, history_done);
            return false;
        }
        // Re-checked a few folders at a time between batches, like the other
        // walks, never holding up the changes reported meanwhile.
        for folder in recheck {
            self.queue_recheck(folder);
        }
        if !self.room() {
            // Nothing is written: what changed is caught up once there is
            // room, and the folders to re-check are re-checked then.
            self.missed = true;
            self.take_cursors(reported, history_done);
            return false;
        }
        self.status(|shared| shared.busy = true);
        let mut changes = Vec::new();
        let mut new_folders = Vec::new();
        self.look_at(paths, &mut changes, &mut new_folders);
        if !rescan.is_empty() {
            let reconciled = reconcile(
                &self.index,
                &self.scope,
                &rescan,
                &self.config.walk,
                &self.stop,
            );
            changes.extend(reconciled.changes);
            self.note_hung(reconciled.hung_folders, false);
        }
        let walked = self.walk_new(&new_folders);
        changes.extend(walked);
        self.add_watches(&changes);
        let _ = self.index.apply(&changes);
        self.take_cursors(reported, history_done);
        self.settled();
        false
    }

    /// Takes the cursors a batch reported, its changes applied; once the
    /// replayed history ended, the index is caught up by it and the
    /// cursors are saved.
    fn take_cursors(&mut self, reported: Option<Vec<JournalCursor>>, history_done: bool) {
        if let Some(cursors) = reported {
            self.cursors = cursors;
        }
        if history_done {
            self.replaying = false;
            self.caught_up = Some(CaughtUpBy::EventHistory);
            self.save();
        }
    }

    /// The folders a rescan asks for, within the roots: a folder above a
    /// root rescans that root.
    fn rescan_folders(&self, asked: Vec<PathBuf>) -> Vec<PathBuf> {
        let mut folders: Vec<PathBuf> = Vec::new();
        for folder in asked {
            if self.scope.root_of(&folder).is_some() {
                folders.push(folder);
            } else {
                folders.extend(
                    self.scope
                        .rules()
                        .roots
                        .iter()
                        .filter(|root| root.starts_with(&folder))
                        .cloned(),
                );
            }
        }
        folders.sort();
        folders.dedup();
        // A folder under another one asked for is covered by it.
        let mut covering: Vec<PathBuf> = Vec::new();
        for folder in folders {
            if !covering.iter().any(|outer| folder.starts_with(outer)) {
                covering.push(folder);
            }
        }
        covering
    }

    /// Looks at each changed path now: indexed if it exists and the rules
    /// admit it (told with what the scope keeps of the folders between
    /// batches, #186), removed otherwise; a folder new to the index is
    /// walked.
    fn look_at(
        &self,
        paths: BTreeSet<PathBuf>,
        changes: &mut Vec<Change>,
        new_folders: &mut Vec<PathBuf>,
    ) {
        for path in paths {
            if self.scope.root_of(&path).is_none() {
                continue;
            }
            let indexed = self.index.get(&path);
            let gone = |changes: &mut Vec<Change>, meta: Option<Meta>| match meta {
                Some(meta) if meta.kind == EntryKind::Folder => {
                    changes.push(Change::RemoveUnder(path.clone()))
                }
                Some(_) => changes.push(Change::Remove(path.clone())),
                None => {}
            };
            let is_root = self.scope.rules().roots.contains(&path);
            match Entry::read(&path) {
                // A root away (an unplugged drive) keeps its entries.
                Err(_) if is_root => {}
                Ok(entry) => {
                    let is_dir = entry.meta.kind == EntryKind::Folder;
                    if !self.scope.admits_kept(&path, is_dir) {
                        gone(changes, indexed);
                        continue;
                    }
                    match indexed {
                        Some(meta) if meta.kind == EntryKind::Folder && is_dir => {
                            changes.push(Change::Put(entry));
                        }
                        Some(meta) if is_dir || meta.kind == EntryKind::Folder => {
                            gone(changes, Some(meta));
                            if is_dir {
                                new_folders.push(path);
                            } else {
                                changes.push(Change::Put(entry));
                            }
                        }
                        _ if is_dir => new_folders.push(path),
                        _ => changes.push(Change::Put(entry)),
                    }
                }
                Err(_) => gone(changes, indexed),
            }
        }
    }

    /// Walks `folders`, new to the index, whole: their entries to put.
    fn walk_new(&self, folders: &[PathBuf]) -> Vec<Change> {
        if folders.is_empty() {
            return Vec::new();
        }
        let walked = Mutex::new(Vec::new());
        walk_folders(
            &self.scope,
            folders,
            &self.config.walk,
            &self.stop,
            &|entries: Vec<Entry>| lock(&walked).extend(entries),
        );
        walked
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .into_iter()
            .map(Change::Put)
            .collect()
    }

    /// Watches the folders `changes` put (Linux watches each folder), but
    /// those on a network share.
    fn add_watches(&mut self, changes: &[Change]) {
        let folders: Vec<PathBuf> = changes
            .iter()
            .filter_map(|change| match change {
                Change::Put(entry)
                    if entry.meta.kind == EntryKind::Folder && !self.on_network(&entry.path) =>
                {
                    Some(entry.path.clone())
                }
                _ => None,
            })
            .collect();
        if let Some(watching) = &mut self.watching
            && !folders.is_empty()
        {
            watching.add_folders(&folders);
        }
    }

    /// Runs the walks that waited: the first full walk, then the folders
    /// to walk and to reconcile the catch-up asked for, then a few of the
    /// folders to re-check ([`RECHECK_FOLDERS`]); with some still to
    /// re-check, the coordinator handles the changes reported meanwhile and
    /// comes back for the next few.
    fn do_walks(&mut self) {
        if !self.room() {
            return;
        }
        self.status(|shared| {
            shared.busy = true;
            shared.status.waiting = false;
            shared.status.state = IndexState::Building;
        });
        let walked_all = self.full_walk;
        if self.full_walk {
            // Taken before the walk starts, so that what changes during it
            // is caught up next time.
            self.cursors = self.config.source.cursors(&self.scope);
            let (report, out_of_room) = self.full_walk();
            if self.stopped() {
                return;
            }
            if out_of_room {
                // The walk was given up: it starts again once there is
                // room.
                self.room();
                return;
            }
            self.full_walk = false;
            self.caught_up = Some(CaughtUpBy::FullWalk);
            // The walk read every folder under the rules as they are.
            self.recheck.clear();
            let mut record = self.index.record();
            record.built = !report.cancelled;
            let _ = self.index.flush();
            let _ = self.index.set_record(record);
            // Only a source watching each folder needs them read.
            if self.config.source.watches_folders() {
                let index = self.index.clone();
                let folders = self.indexed_folders(&mut FolderIds::new(&index));
                if let Some(watching) = &mut self.watching {
                    watching.add_folders(&folders);
                }
            }
            let ceiling = count_words(self.config.walk.max_entries);
            self.status(|shared| {
                shared.status.unreadable = report.unreadable;
                shared.status.unreadable_folders = report.unreadable_folders.clone();
                shared.status.refused = report.refused;
                shared.status.refused_folders = report.refused_folders.clone();
                shared.status.hung = report.hung;
                shared.status.hung_folders = report.hung_folders.clone();
                shared.status.ceiling_reached = report.ceiling_reached;
                if report.ceiling_reached {
                    shared.status.reason = Some(format!(
                        "Pane stopped indexing at {ceiling} entries; exclude folders to index \
                         the rest"
                    ));
                }
            });
        }
        let mut changes = Vec::new();
        if !self.reconcile.is_empty() {
            let folders = std::mem::take(&mut self.reconcile);
            let reconciled = reconcile(
                &self.index,
                &self.scope,
                &folders,
                &self.config.walk,
                &self.stop,
            );
            changes.extend(reconciled.changes);
            self.note_hung(reconciled.hung_folders, !walked_all);
            for folder in reconciled.recheck {
                self.queue_recheck(folder);
            }
        }
        if !self.walk.is_empty() {
            let folders = std::mem::take(&mut self.walk);
            changes.extend(self.walk_new(&folders));
        }
        if !self.recheck.is_empty() {
            changes.extend(self.recheck_some());
        }
        if self.stopped() {
            return;
        }
        self.add_watches(&changes);
        let _ = self.index.apply(&changes);
        if !self.recheck.is_empty() {
            // More to re-check: the changes reported meanwhile come first.
            return;
        }
        self.save();
        self.settled();
    }

    /// The first full walk, written straight into segments; queries see
    /// each segment as it is written. Given up (`true`) when the disk
    /// holding the index runs short of space meanwhile, looked at no more
    /// than once a second.
    fn full_walk(&self) -> (WalkReport, bool) {
        let index = &*self.index;
        let scope = &*self.scope;
        let options = &self.config.walk;
        let (batches, received) = std::sync::mpsc::sync_channel::<super::PreparedBatch>(64);
        let inner = self.inner.clone();
        let stop = &*self.stop;
        let cancel = &AtomicBool::new(false);
        let out_of_room = &AtomicBool::new(false);
        let looked = &Mutex::new(Instant::now());
        let valves = &self.config.valves;
        let dir = &self.config.dir;
        let pause = &*self.pause;
        std::thread::scope(|threads| {
            let walker = threads.spawn(move || {
                walk(scope, options, cancel, &|batch: Vec<Entry>| {
                    if stop.load(Ordering::Relaxed) {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    // The computer slept during the walk: each of its
                    // threads waits out the pause after the wake before it
                    // hands over its next folder.
                    if pause.look().is_some() || pause.on() {
                        let say = |resuming: bool| {
                            if let Some(inner) = inner.upgrade() {
                                lock(&inner.shared).status.resuming = resuming;
                                inner.changed.notify_all();
                            }
                        };
                        say(true);
                        pause.hold(stop);
                        say(false);
                    }
                    {
                        let mut looked = lock(looked);
                        if looked.elapsed() >= Duration::from_secs(1) {
                            *looked = Instant::now();
                            let short = (valves.free_space)(dir)
                                .is_some_and(|free| free < valves.free_space_floor);
                            if short {
                                out_of_room.store(true, Ordering::Relaxed);
                                cancel.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let count = batch.len() as u64;
                    let _ = batches.send(index.prepare(batch));
                    if let Some(inner) = inner.upgrade() {
                        let mut shared = lock(&inner.shared);
                        shared.status.found += count;
                    }
                })
            });
            let mut bulk = index.bulk().ok();
            for batch in received {
                if let Some(writer) = &mut bulk
                    && writer.add(batch).is_err()
                {
                    bulk = None;
                }
            }
            let report = walker.join().unwrap_or_default();
            let out_of_room = out_of_room.load(Ordering::Relaxed);
            if let Some(writer) = bulk
                && !out_of_room
            {
                let _ = writer.finish();
            }
            (report, out_of_room)
        })
    }

    /// Whether folders are reconciled every
    /// [`IndexerConfig::reconcile_unwatched`]: those Linux cannot watch, and
    /// the roots on a network share.
    fn reconciles_now_and_then(&self) -> bool {
        !self.unwatched.is_empty() || !self.network.is_empty()
    }

    /// Reconciles the folders Linux cannot watch, and the roots on a
    /// network share.
    fn reconcile_unwatched(&mut self) {
        self.unwatched_reconciled = Instant::now();
        let mut folders = self.unwatched.clone();
        folders.extend(self.network.iter().cloned());
        let reconciled = reconcile(
            &self.index,
            &self.scope,
            &folders,
            &self.config.walk,
            &self.stop,
        );
        if !reconciled.changes.is_empty() && self.room() {
            let _ = self.index.apply(&reconciled.changes);
        }
        self.note_hung(reconciled.hung_folders, false);
        // Read again holding an ignore file: re-checked whole with the walks
        // (#186).
        for folder in reconciled.recheck {
            self.queue_recheck(folder);
        }
        self.settled();
    }

    /// Saves the cursors with the index, once what they cover is in a
    /// segment.
    fn save(&self) {
        if self.low_space {
            return;
        }
        let _ = self.index.flush();
        let mut record: IndexRecord = self.index.record();
        if record.built && !self.cursors.is_empty() {
            record.cursors = self.cursors.clone();
            let _ = self.index.set_record(record);
        }
    }

    /// Notes that the index is current, with how it was caught up.
    fn settled(&self) {
        if self.low_space {
            // Stopped for space; the status says so.
            self.status(|shared| shared.busy = false);
            return;
        }
        if self.deferred() {
            return;
        }
        let entries = self.index.stats().stored;
        let how = self.caught_up;
        self.status(|shared| {
            shared.busy = false;
            shared.status.state = IndexState::Current;
            shared.status.waiting = false;
            shared.status.entries = entries;
            shared.status.found = 0;
            if let Some(how) = how
                && shared.status.caught_up.map(|(by, _)| by) != Some(how)
            {
                shared.status.caught_up = Some((how, SystemTime::now()));
            }
        });
    }
}

/// The folder whose ignore rules a change of `path` may change (#186), and
/// whether `path` is its repository (`.git`): `X` for `X/.gitignore`,
/// `X/.ignore`, `X/CACHEDIR.TAG`, `X/.git`, `X/.git/info` and
/// `X/.git/info/exclude`, in any letter case.
fn rules_folder(path: &Path) -> Option<(&Path, bool)> {
    let is = |name: &std::ffi::OsStr, wanted: &str| name.eq_ignore_ascii_case(wanted);
    let named = |at: &Path, wanted: &str| at.file_name().is_some_and(|name| is(name, wanted));
    let name = path.file_name()?;
    let parent = path.parent()?;
    if is(name, ".gitignore") || is(name, ".ignore") || is(name, CACHE_TAG) {
        return Some((parent, false));
    }
    if is(name, ".git") {
        return Some((parent, true));
    }
    let info = if is(name, "exclude") { parent } else { path };
    let git = info.parent()?;
    if named(info, "info") && named(git, ".git") {
        return Some((git.parent()?, false));
    }
    None
}

/// Brings the index, built under the rules `before`, to the rules `after`,
/// changing only what the change affects where it can: a removed root's
/// entries go; an added root alone is walked as new (the catch-up finds it
/// not indexed); a folder newly excluded (by the user, or for churn) takes
/// its entries out; a folder no longer excluded is walked again alone (the
/// folders answered). Any other change walks every root again.
fn apply_rule_change(index: &FileIndex, before: &ScopeRules, after: &ScopeRules) -> Vec<PathBuf> {
    let mut changes: Vec<Change> = before
        .roots
        .iter()
        .filter(|root| !after.roots.contains(root))
        .cloned()
        .map(Change::RemoveUnder)
        .collect();
    let mut walk_again = Vec::new();
    let only_roots_and_folders = ScopeRules {
        roots: after.roots.clone(),
        excluded_folders: after.excluded_folders.clone(),
        ..before.clone()
    } == *after;
    if only_roots_and_folders {
        changes.extend(
            after
                .excluded_folders
                .iter()
                .filter(|folder| !before.excluded_folders.contains(folder))
                .cloned()
                .map(Change::RemoveUnder),
        );
        walk_again.extend(
            before
                .excluded_folders
                .iter()
                .filter(|folder| !after.excluded_folders.contains(folder))
                .cloned(),
        );
    } else {
        changes.extend(after.roots.iter().cloned().map(Change::RemoveUnder));
    }
    let _ = index.apply(&changes);
    if !only_roots_and_folders {
        let mut record = index.record();
        record.built = false;
        let _ = index.flush();
        let _ = index.set_record(record);
    }
    walk_again
}

/// Opens the index in `dir`, trying again for a moment while the index of
/// this same Pane is still being let go (a restart of the coordinator).
fn open_index(
    dir: &Path,
    roots: &[PathBuf],
    stop: &AtomicBool,
) -> Result<(FileIndex, Opened), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match FileIndex::open(dir, roots) {
            Ok(opened) => return Ok(opened),
            Err(IndexError::InUse)
                if Instant::now() < deadline && !stop.load(Ordering::Relaxed) =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(IndexError::InUse) => {
                return Err("Another Pane is using file search on this computer".into());
            }
            Err(IndexError::Io(error)) => {
                return Err(format!("Pane could not open the file index: {error}"));
            }
        }
    }
}

/// How many of `messages` were counted as sent ([`Counter::sent`], or
/// [`Indexer::launcher_shown`]): all but a stop, which is never counted.
fn counted(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|message| !matches!(message, Message::Stop))
        .count()
}

/// What counts the messages a sink sends, for [`Indexer::wait_until_settled`].
pub(crate) trait Counter: Send + Sync {
    fn sent(&self);
    fn unsent(&self);
}

/// Reports belonging to one coordinator, including any native callback
/// still finishing after that coordinator was stopped.
#[derive(Default)]
struct Queued(AtomicUsize);

impl Counter for Queued {
    fn sent(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    fn unsent(&self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests;
