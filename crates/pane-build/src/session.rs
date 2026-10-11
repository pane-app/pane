//! A development session: building a local package after each save and
//! handing each build that succeeds to its host (ADR 0004; #12, #13).
//!
//! [`Prepared::new`] finds how the package builds and watches its source
//! folder with the system's file watcher; [`Worker::start`] then runs the
//! session on a thread of its own. After a save (and a short pause, so that
//! an editor's several writes are one save) it runs the package's build,
//! which puts the package's components in a staging folder of its own in
//! the session's work folder:
//!
//! - A build that fails is recorded as a [`BuildFailure`], with the whole
//!   output in a log file in the work folder, and told to the host.
//! - A build that succeeds is handed to the host once the host can take it
//!   ([`Host::claim`]); Pane's launcher reloads the package from it. If the
//!   host replaced the package with it, its components are then also copied
//!   to the source folder, so that what the folder holds is what runs.
//! - A save while a build runs makes that build obsolete: it runs to its end
//!   but is never handed over, and the folder is built again. Builds run one
//!   at a time, and each hand-over ends before the next build starts, so an
//!   older build never replaces a newer one. The components an obsolete
//!   build left in the source folder (a Rust build's `target`) are replaced
//!   with the installed ones, so a Reload never picks it up. After
//!   [`MAX_OBSOLETE`] obsolete builds in a row, the session waits for the
//!   next save.
//! - A build that ends while the host cannot take it (Pane's package being
//!   changed otherwise: a Reload, an update) waits for that to end; a save
//!   meanwhile makes it obsolete.
//!
//! [`Session::end`] ends it: a running build is stopped with every process
//! it started before the call returns, and the session's thread drops its
//! watcher. A build that ends after that is dropped without a word.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use notify::event::{EventKind, MetadataKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::build::{
    Build, BuildJob, BuildOutcome, BuildOutput, BuildStop, Builder, Echo, first_error, is_save,
    stage_package,
};
use crate::sources::Sources;
use crate::{ManifestFiles, canonical};

/// How long the folder must stay unchanged after a save before it is built,
/// so that an editor's several writes are one save.
const SETTLE: Duration = Duration::from_millis(150);

/// How often a build that waits for its host to take it checks again.
const CLAIM_RETRY: Duration = Duration::from_millis(100);

/// How many builds in a row may be obsolete before the session stops
/// building until the next save.
pub const MAX_OBSOLETE: u64 = 3;

/// The log of the running build, and of the last one that failed, in the
/// session's work folder.
const BUILD_LOG: &str = "build.log";
const FAILED_LOG: &str = "failed-build.log";

/// A package being developed, as its session reports it.
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

/// The failure of a build that printed `output` and failed for `reason`;
/// `log` keeps the whole output once `output` is closed, and says where.
pub(crate) fn failure(
    reason: String,
    output: BuildOutput,
    log: impl FnOnce() -> Option<PathBuf>,
) -> BuildFailure {
    let (output, earlier) = {
        // Closed before `log` moves its file.
        let output = output;
        output.line(&reason);
        output.tail()
    };
    let log = log();
    let summary = first_error(output.iter().map(String::as_str))
        .unwrap_or(&reason)
        .trim_end_matches('.')
        .to_owned();
    BuildFailure {
        summary,
        output,
        earlier,
        log,
    }
}

/// What a session hands its builds to, and tells what happens: Pane's
/// launcher, which reloads the package from each build that succeeds.
/// Its methods are called on the session's thread.
pub trait Host: Send + 'static {
    /// What the host keeps while it takes a build.
    type Claim;

    /// Whether this session still develops its package; a session whose
    /// host says not ends without another word.
    fn is_current(&self) -> bool;

    /// A build began, running `command`.
    fn building(&self, command: &str);

    /// The build that ran `command` failed; the session's report already
    /// says so.
    fn failed(&self, failure: &BuildFailure, command: &str);

    /// Whether the host can take a build now.
    fn claim(&self) -> Claim<Self::Claim>;

    /// Takes the build staged in `staging`, which the host claimed. Returns
    /// whether it replaced the package with it: its components are then
    /// copied to the source folder.
    fn deliver(&mut self, claim: &mut Self::Claim, staging: &Path) -> bool;

    /// The build taken with `claim` was handled, and its components copied.
    fn delivered(&self, claim: Self::Claim);

    /// Where the package's installed copy is, whose components replace what
    /// an obsolete build left in the source folder.
    fn installed(&self) -> Option<PathBuf>;

    /// The sources kept changing during [`MAX_OBSOLETE`] builds in a row:
    /// nothing is built until the next save.
    fn gave_up(&self);

    /// The session's report changed.
    fn changed(&self);
}

/// Whether a [`Host`] can take a build.
pub enum Claim<C> {
    /// It can, and keeps this meanwhile.
    Claimed(C),
    /// Not yet: something else changes the package.
    Busy,
    /// Never: the session has ended.
    Ended,
}

/// What a session's thread is told.
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

/// A package ready to be developed: how it builds, and its folder watched.
pub struct Prepared {
    /// The source folder, canonical.
    folder: PathBuf,
    build: Arc<dyn Build>,
    manifests: Arc<dyn ManifestFiles>,
    watcher: RecommendedWatcher,
    sources: Arc<Mutex<Sources>>,
    signals: Sender<Signal>,
    received: Receiver<Signal>,
    work: PathBuf,
    echo: Option<Echo>,
}

impl Prepared {
    /// Finds how `builder` builds the package in `folder`, whose manifest
    /// `manifests` reads, and watches the folder; `work` is the session's
    /// own folder, for its builds' staging folders and logs, and what an
    /// earlier session left there is removed. Blocks on the file system.
    pub fn new(
        builder: &dyn Builder,
        manifests: Arc<dyn ManifestFiles>,
        folder: &Path,
        work: PathBuf,
    ) -> Result<Prepared, String> {
        let (signals, received) = mpsc::channel();
        // FSEvents reports canonical paths.
        let folder = canonical(folder).unwrap_or_else(|_| folder.to_path_buf());
        let build = builder.build_for(&folder)?;
        let (watcher, sources) = watch(&folder, build.clone(), signals.clone())?;
        // What an earlier session left, such as after a crash.
        let _ = std::fs::remove_dir_all(&work);
        Ok(Prepared {
            folder,
            build,
            manifests,
            watcher,
            sources,
            signals,
            received,
            work,
            echo: None,
        })
    }

    /// This package's session, showing each line its builds print with
    /// `echo` as they print it, such as in `pane-ext`'s terminal.
    pub fn echo(self, echo: Echo) -> Prepared {
        Prepared {
            echo: Some(echo),
            ..self
        }
    }

    /// The source folder watched, canonical.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The build command run after each save.
    pub fn command(&self) -> String {
        self.build.command()
    }

    /// The session, and its thread to start.
    pub fn begin(self) -> (Session, Worker) {
        let stop = BuildStop::default();
        let report = Arc::new(Mutex::new(Development {
            folder: self.folder.clone(),
            command: self.build.command(),
            building: false,
            pending: false,
            waiting: false,
            finished: 0,
            obsolete: 0,
            failure: None,
        }));
        let session = Session {
            report: report.clone(),
            signals: self.signals.clone(),
            stop: stop.clone(),
        };
        let worker = Worker {
            report,
            folder: self.folder,
            build: self.build,
            manifests: self.manifests,
            watcher: self.watcher,
            sources: self.sources,
            signals: self.signals,
            received: self.received,
            stop,
            work: self.work,
            echo: self.echo,
        };
        (session, worker)
    }
}

/// A developed package's session, as its owner holds it.
pub struct Session {
    report: Arc<Mutex<Development>>,
    /// Reaches the session's thread.
    signals: Sender<Signal>,
    /// Stops the session's build.
    stop: BuildStop,
}

impl Session {
    /// What the session is doing, and how its last build went.
    pub fn report(&self) -> Development {
        lock(&self.report).clone()
    }

    /// Builds the package now, as a save would.
    pub fn build_now(&self) {
        let _ = self.signals.send(Signal::Saved);
    }

    /// Ends the session: the processes of a running build are killed before
    /// this returns, and its thread drops its watcher. Never waits for the
    /// build.
    pub fn end(&self) {
        self.stop.stop();
        let _ = self.signals.send(Signal::Stop);
    }
}

/// A session's thread, not started yet.
pub struct Worker {
    report: Arc<Mutex<Development>>,
    folder: PathBuf,
    build: Arc<dyn Build>,
    manifests: Arc<dyn ManifestFiles>,
    watcher: RecommendedWatcher,
    sources: Arc<Mutex<Sources>>,
    signals: Sender<Signal>,
    received: Receiver<Signal>,
    stop: BuildStop,
    work: PathBuf,
    echo: Option<Echo>,
}

impl Worker {
    /// Starts the session's thread, handing its builds to `host`.
    pub fn start(self, host: impl Host) {
        let running = Running {
            host,
            worker: self,
            builds: 0,
        };
        std::thread::Builder::new()
            .name("pane-develop".into())
            .spawn(move || running.run())
            .expect("the development thread could not start");
    }
}

fn lock(report: &Mutex<Development>) -> MutexGuard<'_, Development> {
    report.lock().unwrap_or_else(|p| p.into_inner())
}

/// Copies the components named by `from`'s `pane.json` from `from` to the
/// same paths in `to`, replacing each file rather than writing through it
/// (a Rust build's component is a hard link into `target`). Returns their
/// paths, relative to both. The source map a development build of
/// JavaScript or TypeScript keeps beside its component comes with it, so a
/// later Reload of the same build keeps it (#214).
pub fn copy_components(manifests: &dyn ManifestFiles, from: &Path, to: &Path) -> Vec<PathBuf> {
    let Ok(components) = manifests.components(from) else {
        return Vec::new();
    };
    for component in &components {
        let target = to.join(component);
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::copy(from.join(component), &target);
        // The component's source map, when the build kept one.
        let map = map_beside(&from.join(component));
        if map.is_file() {
            let beside = map_beside(&target);
            let _ = std::fs::remove_file(&beside);
            let _ = std::fs::copy(&map, &beside);
        }
    }
    components
}

/// The path of the source map a development build may keep beside the
/// component at `component`: its file name plus `.map` (#214).
fn map_beside(component: &Path) -> PathBuf {
    let name = component.file_name().unwrap_or_default().to_string_lossy();
    component.with_file_name(format!("{name}.map"))
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

/// A session's thread: waits for saves, builds, and hands builds over.
struct Running<H> {
    host: H,
    worker: Worker,
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
    /// It was handed over, or found failed.
    Done,
    /// The folder was saved before it could be handed over: it is obsolete.
    Saved,
    /// Development ended.
    Stopped,
}

impl<H: Host> Running<H> {
    fn run(mut self) {
        while self.wait_for_save() {
            if !self.develop() {
                break;
            }
        }
        let _ = std::fs::remove_dir_all(self.worker.work.join("staging"));
    }

    /// Changes this session's report.
    fn update(&self, change: impl FnOnce(&mut Development)) {
        change(&mut lock(&self.worker.report));
    }

    /// Builds after a save until a build ends with no newer save, and acts
    /// on it. Returns false once development ended.
    fn develop(&mut self) -> bool {
        let mut obsolete = 0;
        loop {
            if let Waited::Stopped = self.settle() {
                return false;
            }
            if !self.host.is_current() {
                return false;
            }
            self.update(|d| {
                d.building = true;
                d.pending = false;
            });
            self.host.building(&self.worker.build.command());
            self.builds += 1;
            let staging = self
                .worker
                .work
                .join("staging")
                .join(format!("build-{}", self.builds));
            let output = BuildOutput::new(Some(&self.worker.work.join(BUILD_LOG)))
                .echoing(self.worker.echo.clone());
            let built = match stage_package(&*self.worker.manifests, &self.worker.folder, &staging)
            {
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
                    if self.host.is_current() {
                        self.build_failed(reason, output);
                    }
                    Concluded::Done
                }
                Some((BuildOutcome::Built, false)) => self.reload(&staging),
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
                    if !self.host.is_current() {
                        return false;
                    }
                    if let Some(installed) = self.host.installed() {
                        self.write_components(&installed);
                    }
                    if obsolete >= MAX_OBSOLETE {
                        self.update(|d| d.building = false);
                        self.host.gave_up();
                        self.finished();
                        return true;
                    }
                }
            }
        }
    }

    /// Records that the build failed: the build's log is kept as the failed
    /// build's.
    fn build_failed(&self, reason: String, output: BuildOutput) {
        let work = &self.worker.work;
        let failure = Arc::new(failure(reason, output, || {
            std::fs::rename(work.join(BUILD_LOG), work.join(FAILED_LOG))
                .ok()
                .map(|()| work.join(FAILED_LOG))
        }));
        self.update(|d| {
            d.failure = Some(failure.clone());
            d.building = false;
        });
        self.host.failed(&failure, &self.worker.build.command());
    }

    /// Notes that a save was acted on.
    fn finished(&self) {
        self.update(|d| {
            d.building = false;
            d.waiting = false;
            d.finished += 1;
        });
        self.host.changed();
    }

    /// Hands the build staged in `staging` to the host once it can take it,
    /// and copies the components to the source folder if it replaced the
    /// package with them.
    fn reload(&mut self, staging: &Path) -> Concluded {
        let mut claim = loop {
            match self.host.claim() {
                Claim::Ended => return Concluded::Stopped,
                Claim::Claimed(claim) => break claim,
                Claim::Busy => {}
            }
            // A Reload or update is changing it: wait for it to end.
            self.update(|d| d.waiting = true);
            match self.worker.received.recv_timeout(CLAIM_RETRY) {
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
        self.host.changed();
        if self.host.deliver(&mut claim, staging) {
            self.write_components(staging);
        }
        self.host.delivered(claim);
        Concluded::Done
    }

    /// Waits for a save; returns false if development ends first.
    fn wait_for_save(&mut self) -> bool {
        loop {
            match self.worker.received.recv() {
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
            match self.worker.received.recv_timeout(SETTLE) {
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
        let build = self.worker.build.clone();
        let job = BuildJob::with(
            self.worker.stop.clone(),
            staging.to_path_buf(),
            output.clone(),
        );
        let signals = self.worker.signals.clone();
        let running = std::thread::Builder::new()
            .name("pane-build".into())
            .spawn(move || {
                let _ = signals.send(Signal::Built(build.run(&job)));
            })
            .expect("the build thread could not start");
        let mut saved = false;
        loop {
            match self.worker.received.recv() {
                Ok(Signal::Saved) => {
                    if !saved {
                        saved = true;
                        self.update(|d| d.pending = true);
                    }
                }
                Ok(Signal::Folder(folder)) => self.watch_folder(&folder),
                Ok(Signal::Built(outcome)) => {
                    let _ = running.join();
                    if self.worker.stop.is_stopped() {
                        return None;
                    }
                    return Some((outcome, saved));
                }
                Ok(Signal::Stop) | Err(_) => {
                    self.worker.stop.stop();
                    return None;
                }
            }
        }
    }

    /// Copies the components in `from` to the source folder; the copies
    /// are not saves.
    fn write_components(&self, from: &Path) {
        // Held while writing, so that the watcher sees them as written.
        let mut sources = self
            .worker
            .sources
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        for component in copy_components(&*self.worker.manifests, from, &self.worker.folder) {
            sources.wrote(&component);
        }
    }

    fn watch_folder(&mut self, folder: &Path) {
        let _ = self.worker.watcher.watch(folder, RecursiveMode::Recursive);
    }
}
