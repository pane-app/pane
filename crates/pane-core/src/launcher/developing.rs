//! Development mode: building a local package after each save and reloading
//! it (ADR 0004; #12, #13).
//!
//! The author turns it on for one installed, enabled package in Manage
//! extensions. Pane then watches the package's source folder with the
//! system's file watcher. After a save (and a short pause, so that an
//! editor's several writes are one save) it runs the package's build (see
//! `crate::develop`), which puts the package's components in a staging
//! folder of its own under Pane's data folder:
//!
//! - A build that fails replaces nothing: the package keeps running its
//!   installed code, and the diagnostics are shown ("Why <title> did not
//!   build"), with the whole output in a log file under Pane's data folder.
//! - A build that succeeds is reloaded from its staging folder exactly as
//!   the Reload row reloads the source folder (`reload`): checked as an
//!   install, then replacing the managed copy and starting; a start that
//!   fails pauses the package with Retry, and the earlier code is not
//!   restored. Its components are then also copied to the source folder, so
//!   a later Reload reloads the same build.
//! - A save while a build runs makes that build obsolete: it runs to its end
//!   but is never reloaded, and the folder is built again. Builds of one
//!   package run one at a time, and each reload ends before the next build
//!   starts, so an older build never replaces a newer one. The components
//!   an obsolete build left in the source folder (a Rust build's `target`)
//!   are replaced with the installed ones, so a Reload never picks it up.
//!   After [`MAX_OBSOLETE`] obsolete builds in a row, Pane waits for the
//!   next save.
//! - A build that ends while the package is being changed otherwise (a
//!   Reload, an update) waits for that change to end; a save meanwhile
//!   makes it obsolete.
//!
//! Development ends when the author stops it, when the package is disabled
//! or uninstalled, and when the launcher goes: its watcher is dropped and a
//! running build is stopped with every process it started, before the call
//! that ends it returns. A build that ends after that is dropped without a
//! word. Development is not recorded, so it also ends when Pane quits.
//! Nothing else changes: another installed copy of the same package
//! (another source) is never built, reloaded, disabled or swapped for this
//! one.
//!
//! Once on, any write to the folder (an editor's autosave, `git pull`, a
//! sync client) runs the build, including a Rust package's `build.rs` and
//! the tools its build runs, with the author's own rights and Pane's
//! environment. That is what the author asked for by developing the
//! package, for this session only.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use notify::event::{EventKind, MetadataKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use super::reload::Reload;
use super::{
    Changing, Entry, Launcher, LauncherView, Row, Screen, State, Status, WeakLauncher, off_thread,
};
use crate::changes::ChangeSender;
use crate::develop::{
    Build, BuildJob, BuildOutcome, BuildOutput, BuildStop, Builder, components, first_error,
    is_save, stage_package,
};
use crate::packages::{InstalledPackage, Manifest, PackageIdentity, canonical};

mod sources;

use sources::Sources;

/// How long the folder must stay unchanged after a save before it is built,
/// so that an editor's several writes are one save.
const SETTLE: Duration = Duration::from_millis(150);

/// How often a build that waits for another change of its package to end
/// checks again.
const CLAIM_RETRY: Duration = Duration::from_millis(100);

/// How many builds in a row may be obsolete before Pane stops building
/// until the next save.
pub(super) const MAX_OBSOLETE: u64 = 3;

/// How many lines of a failed build's output its details show; the log
/// file has all of it.
const DETAIL_LINES: usize = 60;

/// The log of the running build, and of the last one that failed, in the
/// package's development folder.
const BUILD_LOG: &str = "build.log";
const FAILED_LOG: &str = "failed-build.log";

/// A package being developed, as [`Launcher::development`] reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Development {
    /// The source folder watched.
    pub folder: PathBuf,
    /// The build command run after each save.
    pub command: String,
    /// Whether a build is running.
    pub building: bool,
    /// Whether a save arrived while the running build ran, which makes it
    /// obsolete.
    pub pending: bool,
    /// Whether a finished build waits for another change of the package,
    /// such as a Reload, to end before it reloads it.
    pub waiting: bool,
    /// How many saves have been acted on: their build reloaded, reported as
    /// failed, or given up after too many obsolete builds.
    pub finished: u64,
    /// How many builds were obsolete when they ended, because the source
    /// was saved again meanwhile; they were not reloaded.
    pub obsolete: u64,
    /// Why the last build failed, if it did; `None` after one succeeds.
    pub failure: Option<Arc<BuildFailure>>,
}

/// Why a development build failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildFailure {
    /// Its first error, or why it failed.
    pub summary: String,
    /// The end of what it printed, then why it failed.
    pub output: Vec<String>,
    /// How many earlier lines it printed; they are only in `log`.
    pub earlier: usize,
    /// The file holding everything it printed, if Pane could write one.
    pub log: Option<PathBuf>,
}

/// What [`Launcher::begin_developing`] found: how to build the package, and
/// where.
pub(super) struct DevelopStart {
    builder: Arc<dyn Builder>,
    /// The source folder.
    folder: PathBuf,
    /// The package's development folder under Pane's data folder: its
    /// builds' staging folders and logs.
    work: PathBuf,
}

/// The launcher's development: which packages are developed, and how they
/// are built.
pub(super) struct Developing {
    builder: Option<Arc<dyn Builder>>,
    changes: Option<ChangeSender>,
    sessions: Mutex<HashMap<PackageIdentity, Session>>,
    /// The id of the next session, so that a session's thread can tell
    /// whether its package is still developed by it.
    next: AtomicU64,
}

/// One developed package.
struct Session {
    id: u64,
    report: Development,
    /// Reaches the session's thread.
    signals: Sender<Signal>,
    /// Stops the session's build.
    stop: BuildStop,
}

/// What the session's thread is told.
enum Signal {
    /// A file in the folder was saved.
    Saved,
    /// A folder was created at the top of the source folder, or moved
    /// there; it is watched too.
    Folder(PathBuf),
    /// The running build ended.
    Built(BuildOutcome),
    /// Development ends.
    Stop,
}

impl Developing {
    pub(super) fn new(builder: Option<Arc<dyn Builder>>, changes: Option<ChangeSender>) -> Self {
        Developing {
            builder,
            changes,
            sessions: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
        }
    }

    fn sessions(&self) -> MutexGuard<'_, HashMap<PackageIdentity, Session>> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn report(&self, identity: &PackageIdentity) -> Option<Development> {
        self.sessions()
            .get(identity)
            .map(|session| session.report.clone())
    }

    fn is_developed(&self, identity: &PackageIdentity) -> bool {
        self.sessions().contains_key(identity)
    }

    /// Whether the package with `identity` is still developed by session
    /// `id`.
    fn is_current(&self, identity: &PackageIdentity, id: u64) -> bool {
        self.sessions()
            .get(identity)
            .is_some_and(|session| session.id == id)
    }

    /// Changes the report of session `id`, if it still develops its
    /// package.
    fn update(&self, identity: &PackageIdentity, id: u64, change: impl FnOnce(&mut Development)) {
        if let Some(session) = self.sessions().get_mut(identity)
            && session.id == id
        {
            change(&mut session.report);
        }
    }

    /// Ends the development of the package with `identity`, or of every
    /// package: the processes of a running build are killed before this
    /// returns, and each session's thread drops its watcher. Never waits
    /// for a build. Returns whether a package was developed.
    pub(super) fn end(&self, which: Option<&PackageIdentity>) -> bool {
        let ended: Vec<Session> = {
            let mut sessions = self.sessions();
            match which {
                Some(identity) => sessions.remove(identity).into_iter().collect(),
                None => sessions.drain().map(|(_, session)| session).collect(),
            }
        };
        for session in &ended {
            session.stop.stop();
            let _ = session.signals.send(Signal::Stop);
        }
        !ended.is_empty()
    }

    fn changed(&self) {
        if let Some(changes) = &self.changes {
            changes.changed();
        }
    }
}

impl Drop for Developing {
    fn drop(&mut self) {
        self.end(None);
    }
}

impl Launcher {
    /// This launcher building local packages on save with `builder`
    /// (normally [`crate::develop::Toolchains`]), and telling the window
    /// through `changes` when that changed what it shows. Without it,
    /// developing a package explains that this Pane cannot.
    pub fn with_development(self, builder: Arc<dyn Builder>, changes: ChangeSender) -> Self {
        self.developing.end(None);
        let launcher = Launcher {
            developing: Arc::new(Developing::new(Some(builder), Some(changes.clone()))),
            changes: Some(changes),
            ..self
        };
        // The runtime reports failures to this launcher, not the one it
        // replaces, whose development is gone.
        launcher.report_failures();
        launcher
    }

    /// Develops the installed package with `identity`: from now on, each
    /// save in its source folder builds it and, if the build succeeds,
    /// reloads it (see the module documentation). Await the returned future
    /// for the outcome; the builds happen in the background. A package
    /// already developed is left as it is.
    pub fn start_developing(
        &self,
        identity: &PackageIdentity,
    ) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let start = self.begin_developing(&mut state, identity);
        drop(state);
        let launcher = self.clone();
        let identity = identity.clone();
        async move {
            if let Some(start) = start {
                launcher.finish_developing(identity, start).await;
            }
        }
    }

    /// Ends the development of the package with `identity`: its folder is
    /// no longer watched, and a build that is running is stopped with the
    /// processes it started.
    pub fn stop_developing(&self, identity: &PackageIdentity) {
        let mut state = self.lock();
        self.end_developing(&mut state, identity);
        self.refresh(&mut state);
    }

    /// Ends every package's development, as when Pane quits: the processes
    /// of running builds are killed before this returns.
    pub fn stop_all_development(&self) {
        self.developing.end(None);
    }

    /// The development of the package with `identity`, if it is developed.
    pub fn development(&self, identity: &PackageIdentity) -> Option<Development> {
        self.developing.report(identity)
    }

    /// Checks that the package can be developed now, explaining why not.
    pub(super) fn begin_developing(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
    ) -> Option<DevelopStart> {
        if let Err(problem) = self.changeable(state, identity, "develop") {
            state.view.status = Status::Error(problem);
            return None;
        }
        if self.is_developed(identity) {
            return None;
        }
        let title = state.title_of(identity);
        // Being reloaded, updated, installed with another package or relied
        // on by an install: it is developed once that ends.
        match state.changing.get(identity) {
            None => {}
            Some(Changing::Recording) => return None,
            Some(busy) => {
                state.view.status = Status::Error(format!("{title} {}", busy.doing()));
                return None;
            }
        }
        let Some(builder) = self.developing.builder.clone() else {
            state.view.status = Status::Error(format!(
                "Cannot develop {title}: this Pane does not build extensions"
            ));
            return None;
        };
        let Some(folder) = identity.local_folder() else {
            state.view.status = Status::Error(format!(
                "Cannot develop {title}: it has no local source folder"
            ));
            return None;
        };
        let installation = self
            .installation
            .as_ref()
            .expect("changeable checked there is an installation");
        state.view.status = Status::Running;
        Some(DevelopStart {
            builder,
            folder: folder.to_path_buf(),
            work: installation.dir.join("develop").join(slot(identity)),
        })
    }

    /// Finds how the package builds and watches its folder, off the
    /// caller's thread, then starts its session.
    pub(super) async fn finish_developing(&self, identity: PackageIdentity, start: DevelopStart) {
        let DevelopStart {
            builder,
            folder,
            work,
        } = start;
        let (signals, received) = mpsc::channel();
        let prepared = {
            let signals = signals.clone();
            let work = work.clone();
            off_thread(move || {
                // FSEvents reports canonical paths.
                let folder = canonical(&folder).unwrap_or(folder);
                let build = builder.build_for(&folder)?;
                let (watcher, sources) = watch(&folder, build.clone(), signals)?;
                // What an earlier session left, such as after a crash.
                let _ = std::fs::remove_dir_all(&work);
                Ok::<_, String>((folder, build, watcher, sources))
            })
            .await
        };
        let mut state = self.lock();
        let title = state.title_of(&identity);
        let (folder, build, watcher, sources) = match prepared {
            Ok(prepared) => prepared,
            Err(reason) => {
                state.view.status = Status::Error(format!("Cannot develop {title}: {reason}"));
                return;
            }
        };
        // Disabled, uninstalled or developed meanwhile: the new watcher goes.
        if self.changeable(&state, &identity, "develop").is_err() || self.is_developed(&identity) {
            return;
        }
        let id = self.developing.next.fetch_add(1, Ordering::SeqCst);
        let stop = BuildStop::default();
        let report = Development {
            folder: folder.clone(),
            command: build.command(),
            building: false,
            pending: false,
            waiting: false,
            finished: 0,
            obsolete: 0,
            failure: None,
        };
        self.developing.sessions().insert(
            identity.clone(),
            Session {
                id,
                report,
                signals: signals.clone(),
                stop: stop.clone(),
            },
        );
        let worker = Worker {
            launcher: self.downgrade(),
            identity,
            id,
            folder: folder.clone(),
            build: build.clone(),
            watcher,
            sources,
            signals,
            received,
            stop,
            work,
            builds: 0,
        };
        std::thread::Builder::new()
            .name("pane-develop".into())
            .spawn(move || worker.run())
            .expect("the development thread could not start");
        state.view.status = Status::Result(format!(
            "Developing {title}: each save in {} runs `{}`, then reloads it",
            folder.display(),
            build.command()
        ));
        self.refresh(&mut state);
    }

    /// Ends the package's development, if it is developed.
    pub(super) fn end_developing(&self, state: &mut State, identity: &PackageIdentity) {
        if self.developing.end(Some(identity)) {
            state.view.status =
                Status::Result(format!("Stopped developing {}", state.title_of(identity)));
        }
    }

    /// Whether the package with `identity` is developed.
    pub(super) fn is_developed(&self, identity: &PackageIdentity) -> bool {
        self.developing.is_developed(identity)
    }

    /// The extension list's development rows: for each enabled package,
    /// one that develops it or stops developing it, and one that shows why
    /// its last build failed, if it did.
    pub(super) fn development_rows(&self, packages: &[InstalledPackage]) -> Vec<(Row, Entry)> {
        let mut rows = Vec::new();
        for package in packages.iter().filter(|package| package.enabled) {
            let identity = &package.identity;
            let title = package.title();
            let source = match identity.local_folder() {
                Some(folder) => folder.display().to_string(),
                None => identity.to_string(),
            };
            let id = |kind: &str| format!("{kind}:{}", identity.key());
            match self.developing.report(identity) {
                None => rows.push((
                    Row {
                        id: id("develop"),
                        title: format!("Develop {title}"),
                        subtitle: Some(format!("Build and reload it after each save in {source}")),
                        unavailable: None,
                    },
                    Entry::Develop(identity.clone()),
                )),
                Some(development) => {
                    rows.push((
                        // The same id as the row that started it, so it
                        // stays selected.
                        Row {
                            id: id("develop"),
                            title: format!("Stop developing {title}"),
                            subtitle: Some(format!(
                                "Each save in {source} runs `{}`",
                                development.command
                            )),
                            unavailable: None,
                        },
                        Entry::StopDeveloping(identity.clone()),
                    ));
                    if development.failure.is_some() {
                        rows.push((
                            Row {
                                id: id("build-failed"),
                                title: build_details_title(&title),
                                subtitle: Some("The build's diagnostics".into()),
                                unavailable: None,
                            },
                            Entry::BuildDetails(identity.clone()),
                        ));
                    }
                }
            }
        }
        rows
    }

    /// Shows why the last build of the package with `identity` failed: the
    /// command, the folder, where the whole output is and the end of it,
    /// with a row that builds it again. Without a failure (it built
    /// meanwhile), the extension list.
    pub(super) fn show_build_details(&self, state: &mut State, identity: &PackageIdentity) {
        let report = self.developing.report(identity);
        let Some((development, failure)) =
            report.and_then(|d| d.failure.clone().map(|failure| (d, failure)))
        else {
            self.show_extensions(state);
            return;
        };
        let title = state.title_of(identity);
        let mut details = vec![
            format!("{title} did not build, so it keeps running its installed code."),
            format!("Folder: {}", development.folder.display()),
            format!("Build command: {}", development.command),
        ];
        if let Some(log) = &failure.log {
            details.push(format!("The whole output is in {}", log.display()));
        }
        let lines: Vec<&str> = failure
            .output
            .iter()
            .map(|line| line.trim_end())
            .filter(|line| !line.trim().is_empty())
            .collect();
        let skipped = lines.len().saturating_sub(DETAIL_LINES);
        let hidden = skipped + failure.earlier;
        if hidden > 0 {
            details.push(match &failure.log {
                Some(_) => format!("({hidden} earlier lines are only in that file.)"),
                None => format!("({hidden} earlier lines are not shown.)"),
            });
        }
        details.extend(lines[skipped..].iter().map(|line| line.to_string()));
        let again = Row {
            id: format!("build-again:{}", identity.key()),
            title: format!("Build {title} again"),
            subtitle: Some(format!("Run `{}` now", development.command)),
            unavailable: None,
        };
        state.next_screen();
        state.entries = vec![Entry::BuildAgain(identity.clone())];
        let screen = Screen::BuildDetails {
            identity: identity.clone(),
            details,
        };
        state.view = LauncherView::new(screen, build_details_title(&title)).with_rows(vec![again]);
    }

    /// Builds the developed package with `identity` now, as a save would.
    pub(super) fn build_again(&self, state: &mut State, identity: &PackageIdentity) {
        let sessions = self.developing.sessions();
        match sessions.get(identity) {
            Some(session) => {
                let _ = session.signals.send(Signal::Saved);
            }
            None => {
                drop(sessions);
                state.view.status = Status::Error(format!(
                    "{} is not being developed",
                    state.title_of(identity)
                ));
            }
        }
    }

    /// Shows `status` of the development of the package with `identity`,
    /// and redraws: on the extension list, a build's details, or root search
    /// with nothing typed. On another screen, such as an open command, it is
    /// kept for when the user returns to one of those, so it does not
    /// replace what that screen says.
    fn show_development(&self, identity: &PackageIdentity, status: Status) {
        let mut state = self.lock();
        self.refresh(&mut state);
        let shown = match &state.view.screen {
            Screen::Extensions { .. } | Screen::BuildDetails { .. } => true,
            Screen::Root { query } => query.is_empty(),
            _ => false,
        };
        if shown {
            state.view.status = status;
            state.development_status = None;
        } else {
            state.development_status = Some((identity.clone(), status));
        }
        drop(state);
        self.developing.changed();
    }

    /// Shows the development status kept by [`Launcher::show_development`],
    /// if its package is still developed; called when the extension list or
    /// root search is shown.
    pub(super) fn show_kept_development_status(&self, state: &mut State) {
        if let Some((identity, status)) = state.development_status.take()
            && self.is_developed(&identity)
        {
            state.view.status = status;
        }
    }

    /// Reports that the developed package's build failed: its code is not
    /// replaced. The build's log is kept as the failed build's.
    fn build_failed(
        &self,
        identity: &PackageIdentity,
        id: u64,
        build: &dyn Build,
        reason: String,
        output: BuildOutput,
        work: &Path,
    ) {
        let title = self.title_of(identity);
        output.line(&reason);
        let (output, earlier) = {
            let output = output;
            output.tail()
        };
        let log = std::fs::rename(work.join(BUILD_LOG), work.join(FAILED_LOG))
            .ok()
            .map(|()| work.join(FAILED_LOG));
        let summary = first_error(output.iter().map(String::as_str))
            .unwrap_or(&reason)
            .trim_end_matches('.')
            .to_owned();
        let whole = match &log {
            Some(log) => format!("the whole output is in {}", log.display()),
            None => "Pane could not keep its output".into(),
        };
        eprintln!(
            "pane: {title} did not build with `{}`: {summary} ({whole})",
            build.command()
        );
        let failure = BuildFailure {
            summary: summary.clone(),
            output,
            earlier,
            log,
        };
        self.developing.update(identity, id, |d| {
            d.failure = Some(Arc::new(failure));
            d.building = false;
        });
        self.show_development(
            identity,
            Status::Error(format!(
                "{title} did not build: {summary}. It keeps running its installed code; the \
                 diagnostics are under \"{}\" in Manage extensions.",
                build_details_title(&title)
            )),
        );
    }

    /// Where the installed copy of the package with `identity` is.
    fn installed_location(&self, identity: &PackageIdentity) -> Option<PathBuf> {
        let state = self.lock();
        state
            .package(identity)
            .map(|package| package.location.clone())
    }
}

/// "Why <title> did not build".
pub(super) fn build_details_title(title: &str) -> String {
    format!("Why {title} did not build")
}

/// The name of the development folder of the package with `identity`: a
/// hash of its identity, so that it is short and a valid file name.
fn slot(identity: &PackageIdentity) -> String {
    // FNV-1a, which is stable across Rust versions, unlike `DefaultHasher`.
    let hash = identity
        .key()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("{hash:016x}")
}

/// Copies the components named by `from`'s `pane.json` from `from` to the
/// same paths in `to`, replacing each file rather than writing through it
/// (a Rust build's component is a hard link into `target`). Returns their
/// paths, relative to both.
fn copy_components(from: &Path, to: &Path) -> Vec<PathBuf> {
    let Ok((manifest, _)) = Manifest::read_parsed(from) else {
        return Vec::new();
    };
    let components = components(&manifest);
    for component in &components {
        let target = to.join(component);
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::copy(from.join(component), &target);
    }
    components
}

/// `path`, from an event of a watcher of `root` (canonical), relative to
/// `root`; FSEvents may report it through another path to the same place.
fn relative_to(root: &Path, path: &Path) -> Option<PathBuf> {
    if let Ok(relative) = path.strip_prefix(root) {
        return Some(relative.to_path_buf());
    }
    let resolved = canonical(path).ok().or_else(|| {
        // Removed: its folder still exists.
        let parent = canonical(path.parent()?).ok()?;
        Some(parent.join(path.file_name()?))
    })?;
    resolved.strip_prefix(root).ok().map(Path::to_path_buf)
}

/// Watches `root` (canonical) for saves, as `build` tells them from its
/// output: the folder itself and every top-level folder that is not the
/// build's (not `target`, `node_modules`, `dist` or hidden ones), so the
/// build's own writes are mostly not watched; saves deeper in the tree are
/// still told apart by [`is_save`], and an event is a save only if its path
/// changed since last seen (see [`Sources`]), which is returned with the
/// watcher. Each save is sent to `signals`, and each folder that appeared at
/// the top, to be watched too.
fn watch(
    root: &Path,
    build: Arc<dyn Build>,
    signals: Sender<Signal>,
) -> Result<(RecommendedWatcher, Arc<Mutex<Sources>>), String> {
    let watched = root.to_path_buf();
    let filter = build.clone();
    // Seen before watching, so that FSEvents telling of earlier writes is
    // not a save.
    let sources = Arc::new(Mutex::new(Sources::new(root, build.clone())));
    let seen = sources.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        // Reading a file (as the build does) is not a save.
        let read = matches!(
            event.kind,
            EventKind::Access(_)
                | EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))
        );
        if read {
            return;
        }
        let mut sources = seen.lock().unwrap_or_else(|p| p.into_inner());
        let saved = if event.need_rescan() {
            sources.rescan()
        } else {
            let mut saved = false;
            for relative in event
                .paths
                .iter()
                .filter_map(|path| relative_to(&watched, path))
                .filter(|relative| is_save(relative, &*filter))
            {
                saved |= sources.changed(&relative);
            }
            saved
        };
        for folder in sources.take_new_folders() {
            let _ = signals.send(Signal::Folder(folder));
        }
        drop(sources);
        if saved {
            let _ = signals.send(Signal::Saved);
        }
    })
    .map_err(|error| format!("Pane could not watch {}: {error}", root.display()))?;
    let watch = |watcher: &mut RecommendedWatcher, path: &Path, mode| {
        watcher
            .watch(path, mode)
            .map_err(|error| format!("Pane could not watch {}: {error}", path.display()))
    };
    watch(&mut watcher, root, RecursiveMode::NonRecursive)?;
    let entries = std::fs::read_dir(root)
        .map_err(|error| format!("Pane could not read {}: {error}", root.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = PathBuf::from(entry.file_name());
        if path.is_dir() && is_save(&name, &*build) {
            watch(&mut watcher, &path, RecursiveMode::Recursive)?;
        }
    }
    Ok((watcher, sources))
}

/// A developed package's thread: waits for saves, builds, and reloads.
struct Worker {
    launcher: WeakLauncher,
    identity: PackageIdentity,
    /// The session this thread belongs to.
    id: u64,
    /// The source folder, canonical.
    folder: PathBuf,
    build: Arc<dyn Build>,
    watcher: RecommendedWatcher,
    /// What the watcher last saw of the folder.
    sources: Arc<Mutex<Sources>>,
    signals: Sender<Signal>,
    received: Receiver<Signal>,
    stop: BuildStop,
    /// The package's development folder: staging folders and logs.
    work: PathBuf,
    /// How many builds this session ran.
    builds: u64,
}

/// How a wait for the folder to settle ended.
enum Waited {
    Settled,
    Stopped,
}

/// How acting on a successful build ended.
enum Concluded {
    /// It was reloaded, or found failed.
    Done,
    /// The folder was saved before it could be reloaded: it is obsolete.
    Saved,
    /// Development ended.
    Stopped,
}

impl Worker {
    fn run(mut self) {
        // Reloads are futures; this thread waits for them.
        let Ok(executor) = tokio::runtime::Builder::new_current_thread().build() else {
            return;
        };
        while self.wait_for_save() {
            if !self.develop(&executor) {
                break;
            }
        }
        let _ = std::fs::remove_dir_all(self.work.join("staging"));
    }

    /// The launcher, if the package is still developed by this session.
    fn current(&self) -> Option<Launcher> {
        let launcher = self.launcher.upgrade()?;
        launcher
            .developing
            .is_current(&self.identity, self.id)
            .then_some(launcher)
    }

    /// Changes this session's report, if the package is still developed
    /// by it.
    fn update(&self, change: impl FnOnce(&mut Development)) {
        if let Some(launcher) = self.launcher.upgrade() {
            launcher.developing.update(&self.identity, self.id, change);
        }
    }

    /// Builds after a save until a build ends with no newer save, and acts
    /// on it. Returns false once development ended.
    fn develop(&mut self, executor: &tokio::runtime::Runtime) -> bool {
        let mut obsolete = 0;
        loop {
            if let Waited::Stopped = self.settle() {
                return false;
            }
            let Some(launcher) = self.current() else {
                return false;
            };
            let title = launcher.title_of(&self.identity);
            self.update(|d| {
                d.building = true;
                d.pending = false;
            });
            launcher.show_development(
                &self.identity,
                Status::Progress(format!("Building {title}: {}…", self.build.command())),
            );
            drop(launcher);
            self.builds += 1;
            let staging = self
                .work
                .join("staging")
                .join(format!("build-{}", self.builds));
            let output = BuildOutput::new(Some(&self.work.join(BUILD_LOG)));
            let built = match stage_package(&self.folder, &staging) {
                Ok(()) => self.build_once(&staging, &output),
                Err(error) => Some((
                    BuildOutcome::Failed(format!(
                        "Pane could not stage the package in {}: {error}",
                        staging.display()
                    )),
                    false,
                )),
            };
            let concluded = match built {
                None => Concluded::Stopped,
                Some((_, true)) => Concluded::Saved,
                Some((BuildOutcome::Stopped, false)) => Concluded::Stopped,
                Some((BuildOutcome::Failed(reason), false)) => {
                    if let Some(launcher) = self.current() {
                        launcher.build_failed(
                            &self.identity,
                            self.id,
                            &*self.build,
                            reason,
                            output,
                            &self.work,
                        );
                    }
                    Concluded::Done
                }
                Some((BuildOutcome::Built, false)) => self.reload(executor, &staging),
            };
            let _ = std::fs::remove_dir_all(&staging);
            match concluded {
                Concluded::Stopped => return false,
                Concluded::Done => {
                    self.finished();
                    return true;
                }
                Concluded::Saved => {
                    obsolete += 1;
                    self.update(|d| d.obsolete += 1);
                    let Some(launcher) = self.current() else {
                        return false;
                    };
                    if let Some(installed) = launcher.installed_location(&self.identity) {
                        self.write_components(&installed);
                    }
                    if obsolete >= MAX_OBSOLETE {
                        let title = launcher.title_of(&self.identity);
                        self.update(|d| d.building = false);
                        launcher.show_development(
                            &self.identity,
                            Status::Error(format!(
                                "{title} was not reloaded: its sources kept changing during \
                                 {MAX_OBSOLETE} builds in a row. Save again to build it."
                            )),
                        );
                        self.finished();
                        return true;
                    }
                }
            }
        }
    }

    /// Notes that a save was acted on.
    fn finished(&self) {
        self.update(|d| {
            d.building = false;
            d.waiting = false;
            d.finished += 1;
        });
        if let Some(launcher) = self.launcher.upgrade() {
            launcher.developing.changed();
        }
    }

    /// Reloads the package from the build staged in `staging`, once nothing
    /// else changes it, and copies the components to the source folder.
    fn reload(&mut self, executor: &tokio::runtime::Runtime, staging: &Path) -> Concluded {
        let launcher = loop {
            let Some(launcher) = self.current() else {
                return Concluded::Stopped;
            };
            {
                // Checked under the lock that disabling, uninstalling and
                // stopping take, so that none is overtaken.
                let mut state = launcher.lock();
                if !launcher.developing.is_current(&self.identity, self.id)
                    || launcher
                        .changeable(&state, &self.identity, "reload")
                        .is_err()
                {
                    return Concluded::Stopped;
                }
                if !state.changing.contains_key(&self.identity) {
                    state.claim(&self.identity, Changing::Reloading);
                    break launcher.clone();
                }
            }
            drop(launcher);
            // A Reload or update is changing it: wait for it to end.
            self.update(|d| d.waiting = true);
            match self.received.recv_timeout(CLAIM_RETRY) {
                Ok(Signal::Saved) => return Concluded::Saved,
                Ok(Signal::Folder(folder)) => self.watch_folder(&folder),
                Ok(Signal::Built(_)) | Err(RecvTimeoutError::Timeout) => {}
                Ok(Signal::Stop) | Err(RecvTimeoutError::Disconnected) => {
                    return Concluded::Stopped;
                }
            }
        };
        self.update(|d| {
            d.building = false;
            d.waiting = false;
            d.failure = None;
        });
        launcher.developing.changed();
        let epoch = launcher.lock().screen_epoch;
        let reload = Reload::staged(self.identity.clone(), staging.to_path_buf());
        let reloaded = executor.block_on(launcher.carry_out(epoch, reload));
        if reloaded.replaced {
            self.write_components(staging);
        }
        {
            let mut state = launcher.lock();
            state.release(&self.identity);
            launcher.refresh(&mut state);
        }
        launcher.show_development(&self.identity, reloaded.status);
        Concluded::Done
    }

    /// Waits for a save; returns false if development ends first.
    fn wait_for_save(&mut self) -> bool {
        loop {
            match self.received.recv() {
                Ok(Signal::Saved) => return true,
                Ok(Signal::Folder(folder)) => self.watch_folder(&folder),
                Ok(Signal::Built(_)) => {}
                Ok(Signal::Stop) | Err(_) => return false,
            }
        }
    }

    /// Waits until no save arrived for [`SETTLE`].
    fn settle(&mut self) -> Waited {
        loop {
            match self.received.recv_timeout(SETTLE) {
                Ok(Signal::Saved | Signal::Built(_)) => {}
                Ok(Signal::Folder(folder)) => self.watch_folder(&folder),
                Err(RecvTimeoutError::Timeout) => return Waited::Settled,
                Ok(Signal::Stop) | Err(RecvTimeoutError::Disconnected) => return Waited::Stopped,
            }
        }
    }

    /// Runs the build, staging into `staging`, on a thread of its own while
    /// listening for saves. Returns its outcome and whether a save arrived
    /// meanwhile, or `None` if development ended; its processes are then
    /// already killed, and the build is not waited for.
    fn build_once(&mut self, staging: &Path, output: &BuildOutput) -> Option<(BuildOutcome, bool)> {
        let build = self.build.clone();
        let job = BuildJob::with(self.stop.clone(), staging.to_path_buf(), output.clone());
        let signals = self.signals.clone();
        let running = std::thread::Builder::new()
            .name("pane-build".into())
            .spawn(move || {
                let _ = signals.send(Signal::Built(build.run(&job)));
            })
            .expect("the build thread could not start");
        let mut saved = false;
        loop {
            match self.received.recv() {
                Ok(Signal::Saved) => {
                    if !saved {
                        saved = true;
                        self.update(|d| d.pending = true);
                    }
                }
                Ok(Signal::Folder(folder)) => self.watch_folder(&folder),
                Ok(Signal::Built(outcome)) => {
                    let _ = running.join();
                    if self.stop.is_stopped() {
                        return None;
                    }
                    return Some((outcome, saved));
                }
                Ok(Signal::Stop) | Err(_) => {
                    self.stop.stop();
                    return None;
                }
            }
        }
    }

    /// Copies the components in `from` to the source folder; the copies
    /// are not saves.
    fn write_components(&self, from: &Path) {
        // Held while writing, so that the watcher sees them as written.
        let mut sources = self.sources.lock().unwrap_or_else(|p| p.into_inner());
        for component in copy_components(from, &self.folder) {
            sources.wrote(&component);
        }
    }

    fn watch_folder(&mut self, folder: &Path) {
        let _ = self.watcher.watch(folder, RecursiveMode::Recursive);
    }
}
