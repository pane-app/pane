//! Development mode: building a local package after each save and reloading
//! it (ADR 0004; #12, #13).
//!
//! The author turns it on for one installed, enabled package in Manage
//! extensions. Pane then watches the package's source folder with the
//! system's file watcher. After a save (and a short pause, so that an
//! editor's several writes are one save) it runs the package's build (see
//! `crate::develop`), which puts the package's components in a staging
//! folder of its own under Pane's data folder. The watching, the builds and
//! what follows a build are the `pane-build` crate's development session,
//! which `pane-ext` runs too; the launcher is its host (`Reloader`), which
//! reloads each build that succeeds:
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
//!
//! `pane-ext dev` develops a package from the author's terminal instead
//! (#217): it runs the same session itself, so the builds run there and print
//! there, and hands each build to Pane over the local channel
//! (`crate::local_channel`). Pane then develops the package without watching
//! or building it ([`Remote`]): each build handed over is reloaded as one of
//! Pane's own would be, and the package's rows, status, build details and
//! extension log are the same. It ends as Pane's own development does, and
//! also when `pane-ext` stops or its connection closes.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

pub use pane_build::{BuildFailure, Development};
use pane_build::{Claim, Host, MAX_OBSOLETE, Prepared};

use super::reload::Reload;
use super::{
    Changing, Entry, Launcher, LauncherView, Row, Screen, State, Status, WeakLauncher, off_thread,
};
use crate::changes::ChangeSender;
use crate::develop::{Builder, PaneManifest};
use crate::extension_log::{ExtensionLogs, LogLevel, LogLine};
use crate::packages::{InstalledPackage, PackageIdentity};

/// How many lines of a failed build's output its details show; the log
/// file has all of it.
const DETAIL_LINES: usize = 60;

/// The extension log of the development session, beside the build's.
pub(super) const EXTENSION_LOG: &str = "extension.log";

/// How long the lines a developed package writes close together are
/// gathered before the window is told its Logs screen has more: one redraw
/// for them all, however fast it writes.
const LOG_REDRAW: Duration = Duration::from_millis(50);

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
    /// What `with_development` configures, behind a lock so that configuring
    /// a launcher after it was built never replaces this Arc: the
    /// background threads the launcher starts hold it weakly, and a
    /// replacement would tell them the launcher was dropped (as
    /// `WeakLauncher::upgrade` upgrades every weakly-held part or none).
    config: Mutex<DevelopmentConfig>,
    sessions: Mutex<HashMap<PackageIdentity, Session>>,
    /// The id of the next session, so that a session's thread can tell
    /// whether its package is still developed by it.
    next: AtomicU64,
    /// Every package's extension log, which a developed package's also
    /// writes to a file.
    pub(super) logs: ExtensionLogs,
}

/// The builder local packages are built with on save, and the sender that
/// tells the window what development changed; either absent until
/// `with_development` runs, so a launcher without it explains that this
/// Pane does not build extensions.
struct DevelopmentConfig {
    builder: Option<Arc<dyn Builder>>,
    changes: Option<ChangeSender>,
}

/// One developed package.
struct Session {
    id: u64,
    driver: Driver,
}

/// Asks `pane-ext` to build a package it develops now, as a save would.
pub(crate) type BuildNow = Arc<dyn Fn() + Send + Sync>;

/// Who builds a developed package.
enum Driver {
    /// Pane, after each save.
    Own(pane_build::Session),
    /// `pane-ext dev`, which hands each build over the local channel.
    Remote {
        /// What Pane was told of its builds.
        report: Arc<Mutex<Development>>,
        build: BuildNow,
    },
}

impl Driver {
    fn report(&self) -> Development {
        match self {
            Driver::Own(session) => session.report(),
            Driver::Remote { report, .. } => lock_report(report).clone(),
        }
    }

    fn build_now(&self) {
        match self {
            Driver::Own(session) => session.build_now(),
            Driver::Remote { build, .. } => build(),
        }
    }

    /// Ends Pane's own session; `pane-ext`'s learns that it ended from the
    /// package's extension log, which stops being followed.
    fn end(&self) {
        if let Driver::Own(session) = self {
            session.end();
        }
    }
}

fn lock_report(report: &Mutex<Development>) -> MutexGuard<'_, Development> {
    report.lock().unwrap_or_else(|p| p.into_inner())
}

impl Developing {
    pub(super) fn new(
        builder: Option<Arc<dyn Builder>>,
        changes: Option<ChangeSender>,
        logs: ExtensionLogs,
    ) -> Self {
        // The configuration is held behind a lock so that
        // `with_development`, which runs after the launcher was built,
        // reconfigures this same Arc rather than replacing it: the
        // background threads hold it weakly, and a replacement would tell
        // them the launcher was dropped.
        Developing {
            config: Mutex::new(DevelopmentConfig { builder, changes }),
            sessions: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            logs,
        }
    }

    /// The configuration `with_development` last set, locked: the builder
    /// local packages are built with on save, and the sender that tells
    /// the window what development changed.
    fn config(&self) -> MutexGuard<'_, DevelopmentConfig> {
        self.config.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn sessions(&self) -> MutexGuard<'_, HashMap<PackageIdentity, Session>> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn report(&self, identity: &PackageIdentity) -> Option<Development> {
        self.sessions()
            .get(identity)
            .map(|session| session.driver.report())
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

    /// Ends the development of the package with `identity`, or of every
    /// package: the processes of a running build are killed before this
    /// returns, and each session's thread drops its watcher. Never waits
    /// for a build. Returns whether a package was developed.
    pub(super) fn end(&self, which: Option<&PackageIdentity>) -> bool {
        let ended: Vec<(PackageIdentity, Session)> = {
            let mut sessions = self.sessions();
            match which {
                Some(identity) => sessions.remove_entry(identity).into_iter().collect(),
                None => sessions.drain().collect(),
            }
        };
        for (identity, session) in &ended {
            session.driver.end();
            let owner = identity.key();
            self.logs
                .pane(&owner, 0, LogLevel::Info, "Development stopped");
            self.logs.stop_developing(&owner);
        }
        !ended.is_empty()
    }

    /// Tells the window that the launcher changed in the background, as
    /// `Launcher::changed` does, through the shared configuration so that a
    /// channel wired after the launcher was built reaches the threads that
    /// were started before it (the updater's checks; #59's family).
    pub(super) fn changed(&self) {
        if let Some(changes) = &self.config().changes {
            changes.changed();
        }
    }

    /// The builder local packages are built with on save, and Create
    /// Extension's package once (see `create`); `None` until
    /// `with_development` runs, so a launcher without it explains that
    /// this Pane does not build extensions.
    pub(super) fn builder(&self) -> Option<Arc<dyn Builder>> {
        self.config().builder.clone()
    }

    /// The sender that tells the window the launcher changed in the
    /// background, for the progress a download reports as it goes.
    /// Read through the shared configuration, so a channel wired after the
    /// launcher was built is seen here too.
    pub(super) fn changes(&self) -> Option<ChangeSender> {
        self.config().changes.clone()
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
        // The same Arc is reconfigured, not replaced: the launcher's
        // background threads hold it weakly, and a replacement would make
        // them take the launcher for dropped (the updater's checks would
        // never run again; #59).
        //
        // The changes sender is written into the shared configuration, not
        // into this launcher's fields: the threads the constructor started
        // hold launchers whose own field snapshots were taken before this
        // ran, and a field would leave them telling no window anything
        // while development's changes (read through the configuration)
        // still arrived — the smoke's #49 phase saw exactly that: the
        // update applied, and the status line never showed it.
        *self.developing.config() = DevelopmentConfig {
            builder: Some(builder),
            changes: Some(changes),
        };
        self.report_failures();
        self
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

    /// The lines Pane keeps of the extension log of the package with
    /// `identity`, oldest first: what its code wrote and Pane's messages
    /// about it. While it is developed, its most recent
    /// [`DEVELOPED_LINES`](crate::extension_log::DEVELOPED_LINES); otherwise
    /// only a small window of them, for diagnostics.
    pub fn extension_log(&self, identity: &PackageIdentity) -> Vec<LogLine> {
        self.developing.logs.lines(&identity.key())
    }

    /// Each line added to the extension log of the package with `identity`
    /// from now on, while it is developed; the receiver ends when its
    /// development does. A package not developed sends nothing.
    pub fn follow_extension_log(&self, identity: &PackageIdentity) -> Receiver<LogLine> {
        self.developing.logs.follow(&identity.key())
    }

    /// Forgets the lines kept of the package's extension log; its log file
    /// keeps them.
    pub fn clear_extension_log(&self, identity: &PackageIdentity) {
        self.developing.logs.clear(&identity.key());
    }

    /// The log file of the package's development session, while it is
    /// developed: every line of its extension log, rotated at
    /// [`FILE_LIMIT`](crate::extension_log::FILE_LIMIT).
    pub fn extension_log_file(&self, identity: &PackageIdentity) -> Option<PathBuf> {
        self.developing.logs.file(&identity.key())
    }

    /// Opens the log file of the developed package with `identity` with
    /// the system's handler for it, as its Logs screen's Open Log File
    /// does, through the launcher's opener (off the calling thread: a
    /// handler may take a moment to start). What was done, or why it
    /// could not be.
    pub fn open_extension_log_file(
        &self,
        identity: &PackageIdentity,
    ) -> impl Future<Output = Result<String, String>> + Send + 'static {
        let file = self.extension_log_file(identity);
        let title = self.title_of(identity);
        let links = self.links.clone();
        async move {
            let Some(file) = file else {
                return Err(format!(
                    "{title} has no log file: it is not being developed"
                ));
            };
            let opened = file.clone();
            off_thread(move || links.open_file(&file))
                .await
                .map(|()| format!("Opened {}", opened.display()))
                .map_err(|why| format!("Could not open {}: {why}", opened.display()))
        }
    }

    /// Shows the extension log of the package with `identity`, "Logs for
    /// <title>", in place of whatever the launcher showed (Settings opens it
    /// too, over an open command, which is left): the window reads its
    /// lines ([`Launcher::extension_log`]) and draws them as they come.
    pub(super) fn show_extension_log(&self, state: &mut State, identity: &PackageIdentity) {
        let title = state.title_of(identity);
        self.leave_command(state);
        state.entries = Vec::new();
        let screen = Screen::ExtensionLog {
            identity: identity.clone(),
        };
        state.view = LauncherView::new(screen, logs_title(&title));
    }

    /// Whether the Logs screen of the package with `identity` is on show.
    fn shows_log_of(&self, identity: &PackageIdentity) -> bool {
        matches!(
            &self.lock().view.screen,
            Screen::ExtensionLog { identity: shown } if shown == identity
        )
    }

    /// Tells the window whenever the log of the developed package with
    /// `identity` grows while its Logs screen is on show, so the screen
    /// draws the new lines: once for the lines written within
    /// [`LOG_REDRAW`] of each other. A thread of its own waits for them,
    /// until the package's development ends and lets the log's followers
    /// go.
    fn redraw_log_as_it_grows(&self, identity: &PackageIdentity) {
        let lines = self.developing.logs.follow(&identity.key());
        let weak = self.downgrade();
        let followed = identity.clone();
        let spawned = std::thread::Builder::new()
            .name("pane-extension-log".into())
            .spawn(move || {
                while lines.recv().is_ok() {
                    std::thread::sleep(LOG_REDRAW);
                    while lines.try_recv().is_ok() {}
                    let Some(launcher) = weak.upgrade() else {
                        return;
                    };
                    if launcher.shows_log_of(&followed) {
                        launcher.developing.changed();
                    }
                }
            });
        if let Err(error) = spawned {
            crate::diagnostic!(
                "pane: the Logs screen of {identity} will not follow its lines: {error}"
            );
        }
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
        let Some(builder) = self.developing.config().builder.clone() else {
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
        let prepared = {
            let work = work.clone();
            off_thread(move || Prepared::new(&*builder, Arc::new(PaneManifest), &folder, work))
                .await
        };
        let mut state = self.lock();
        let title = state.title_of(&identity);
        let prepared = match prepared {
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
        let folder = prepared.folder().to_path_buf();
        let command = prepared.command();
        let (session, worker) = prepared.begin();
        self.developing.sessions().insert(
            identity.clone(),
            Session {
                id,
                driver: Driver::Own(session),
            },
        );
        // Its log is kept beside its builds' from now on.
        let owner = identity.key();
        self.developing
            .logs
            .develop(&owner, work.join(EXTENSION_LOG));
        self.developing.logs.pane(
            &owner,
            0,
            LogLevel::Info,
            &format!(
                "Developing: each save in {} runs `{command}`, then reloads it",
                folder.display(),
            ),
        );
        self.redraw_log_as_it_grows(&identity);
        worker.start(Reloader {
            launcher: self.downgrade(),
            identity,
            id,
            executor: None,
        });
        state.view.status = Status::Result(format!(
            "Developing {title}: each save in {} runs `{command}`, then reloads it",
            folder.display(),
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

    /// The extension list's development rows: for each enabled local package,
    /// one that develops it or stops developing it, and one that shows why
    /// its last build failed, if it did.
    pub(super) fn development_rows(&self, packages: &[InstalledPackage]) -> Vec<(Row, Entry)> {
        let mut rows = Vec::new();
        // Only a local package has a source folder to build.
        let developable = |package: &&InstalledPackage| {
            package.enabled && package.identity.local_folder().is_some()
        };
        for package in packages.iter().filter(developable) {
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
                    rows.push(logs_row(id("logs"), identity, &title));
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
    /// with a row that builds it again and one that shows its log. Without
    /// a failure (it built meanwhile), the extension list.
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
        let id = format!("build-logs:{}", identity.key());
        let (logs, show_logs) = logs_row(id, identity, &title);
        state.next_screen();
        state.entries = vec![Entry::BuildAgain(identity.clone()), show_logs];
        let screen = Screen::BuildDetails {
            identity: identity.clone(),
            details,
        };
        state.view =
            LauncherView::new(screen, build_details_title(&title)).with_rows(vec![again, logs]);
    }

    /// Builds the developed package with `identity` now, as a save would.
    pub(super) fn build_again(&self, state: &mut State, identity: &PackageIdentity) {
        let sessions = self.developing.sessions();
        match sessions.get(identity) {
            Some(session) => session.driver.build_now(),
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
        // The package's log has each step of its development.
        let logged = match &status {
            Status::Progress(text) | Status::Result(text) => Some((LogLevel::Info, text)),
            Status::Error(text) => Some((LogLevel::Error, text)),
            Status::Idle | Status::Running => None,
        };
        if let Some((level, text)) = logged {
            self.developing.logs.pane(&identity.key(), 0, level, text);
        }
        let mut state = self.lock();
        self.refresh(&mut state);
        // A development event of the package whose error overlay is on
        // display ends it, when its code is being replaced: what the
        // overlay was about is over. A build failure or another failed
        // start leaves it — the failure it shows is still the state of
        // things.
        if matches!(status, Status::Progress(_) | Status::Result(_))
            && matches!(&state.view.screen, Screen::Crash { identity: shown, .. } if shown == identity)
        {
            self.leave_error_overlay(&mut state);
            self.refresh(&mut state);
        }
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
    /// replaced. Its session's report already says why.
    fn build_failed(&self, identity: &PackageIdentity, failure: &BuildFailure, command: &str) {
        let title = self.title_of(identity);
        let summary = &failure.summary;
        let whole = match &failure.log {
            Some(log) => format!("the whole output is in {}", log.display()),
            None => "Pane could not keep its output".into(),
        };
        crate::diagnostic!("pane: {title} did not build with `{command}`: {summary} ({whole})");
        self.show_development(
            identity,
            Status::Error(format!(
                "{title} did not build: {summary}. It keeps running its installed code; the \
                 diagnostics are under \"{}\" in Settings.",
                build_details_title(&title)
            )),
        );
    }

    /// Develops the installed package with `identity` with the builds
    /// `pane-ext` hands over the local channel, built with `command`: Pane
    /// neither watches nor builds it, and `build` asks `pane-ext` to build it
    /// now, as the row "Build <title> again" does. A development of the
    /// package already going on, Pane's own or another `pane-ext`'s, ends
    /// first. It lasts until the returned handle is dropped, or ends as
    /// Pane's own development does. Returned with the handle: each line of
    /// the package's extension log from the first of its development, which
    /// ends when the development does.
    pub(crate) fn develop_remotely(
        &self,
        identity: &PackageIdentity,
        command: &str,
        build: BuildNow,
    ) -> Result<(Remote, Receiver<LogLine>), String> {
        let mut state = self.lock();
        self.changeable(&state, identity, "develop")?;
        let title = state.title_of(identity);
        if let Some(busy) = state.changing.get(identity) {
            return Err(format!("{title} {}", busy.doing()));
        }
        let Some(folder) = identity.local_folder().map(Path::to_path_buf) else {
            let message = format!("Cannot develop {title}: it has no local source folder");
            return Err(message);
        };
        let installation = self
            .installation
            .as_ref()
            .expect("changeable checked there is an installation");
        let work = installation.dir.join("develop").join(slot(identity));
        self.developing.end(Some(identity));
        let id = self.developing.next.fetch_add(1, Ordering::SeqCst);
        let report = Arc::new(Mutex::new(Development {
            folder: folder.clone(),
            command: command.to_owned(),
            building: false,
            pending: false,
            waiting: false,
            finished: 0,
            obsolete: 0,
            failure: None,
        }));
        self.developing.sessions().insert(
            identity.clone(),
            Session {
                id,
                driver: Driver::Remote {
                    report: report.clone(),
                    build,
                },
            },
        );
        let owner = identity.key();
        self.developing
            .logs
            .develop(&owner, work.join(EXTENSION_LOG));
        let lines = self.developing.logs.follow(&owner);
        let developing = format!(
            "Developing {title} with pane-ext: each save in {} runs `{command}` in its terminal, \
             then reloads it",
            folder.display(),
        );
        self.developing
            .logs
            .pane(&owner, 0, LogLevel::Info, &developing);
        self.redraw_log_as_it_grows(identity);
        state.view.status = Status::Result(developing);
        self.refresh(&mut state);
        drop(state);
        self.developing.changed();
        let remote = Remote {
            reloader: Reloader {
                launcher: self.downgrade(),
                identity: identity.clone(),
                id,
                executor: None,
            },
            report,
            command: command.to_owned(),
        };
        Ok((remote, lines))
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

/// "Logs for <title>".
fn logs_title(title: &str) -> String {
    format!("Logs for {title}")
}

/// The row with `id` that shows the Logs screen of the package with
/// `identity`, titled `title`: in the extension list and a build's details.
fn logs_row(id: String, identity: &PackageIdentity, title: &str) -> (Row, Entry) {
    let row = Row {
        id,
        title: logs_title(title),
        subtitle: Some("What it writes, and Pane's messages about it".into()),
        unavailable: None,
    };
    (row, Entry::ExtensionLog(identity.clone()))
}

/// The name of the development folder of the package with `identity`: a
/// hash of its identity, so that it is short and a valid file name.
pub(in crate::launcher) fn slot(identity: &PackageIdentity) -> String {
    // FNV-1a, which is stable across Rust versions, unlike `DefaultHasher`.
    let hash = identity
        .key()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("{hash:016x}")
}

/// A package `pane-ext` develops, as the local channel holds it: what it is
/// told of `pane-ext`'s builds, and the builds it hands over (see
/// [`Launcher::develop_remotely`]). Dropping it ends the development, as
/// Stop developing does, if it has not ended otherwise.
pub(crate) struct Remote {
    /// Reloads each build handed over, as Pane's own session's host does.
    reloader: Reloader,
    report: Arc<Mutex<Development>>,
    command: String,
}

impl Remote {
    /// The developed package.
    pub(crate) fn identity(&self) -> &PackageIdentity {
        &self.reloader.identity
    }

    /// The package's title.
    pub(crate) fn title(&self) -> String {
        match self.reloader.launcher.upgrade() {
            Some(launcher) => launcher.title_of(&self.reloader.identity),
            None => self.reloader.identity.to_string(),
        }
    }

    /// Whether Pane still develops the package with this handle.
    pub(crate) fn is_current(&self) -> bool {
        self.reloader.is_current()
    }

    /// Where the installed copy is, whose components replace what an
    /// obsolete build left in the source folder.
    pub(crate) fn installed(&self) -> Option<PathBuf> {
        self.reloader.installed()
    }

    /// A build began.
    pub(crate) fn building(&self) {
        if !self.is_current() {
            return;
        }
        lock_report(&self.report).building = true;
        self.reloader.building(&self.command);
    }

    /// The build failed: nothing is replaced, and its diagnostics are shown
    /// as for Pane's own builds.
    pub(crate) fn failed(&self, failure: BuildFailure) {
        if !self.is_current() {
            return;
        }
        let failure = Arc::new(failure);
        {
            let mut report = lock_report(&self.report);
            report.building = false;
            report.failure = Some(failure.clone());
            report.finished += 1;
        }
        self.reloader.failed(&failure, &self.command);
        self.reloader.changed();
    }

    /// Reloads the package from the build staged in `staging`, once nothing
    /// else changes it (a Reload, an update). Returns whether it replaced
    /// the package, or why it could not take the build.
    pub(crate) fn deliver(&mut self, staging: &Path) -> Result<bool, String> {
        let mut claim = loop {
            match self.reloader.claim() {
                Claim::Claimed(claim) => break claim,
                Claim::Busy => {
                    lock_report(&self.report).waiting = true;
                    std::thread::sleep(CLAIM_RETRY);
                }
                Claim::Ended => {
                    return Err(format!("Pane no longer develops {}", self.title()));
                }
            }
        };
        {
            let mut report = lock_report(&self.report);
            report.building = false;
            report.waiting = false;
            report.failure = None;
        }
        self.reloader.changed();
        let replaced = self.reloader.deliver(&mut claim, staging);
        self.reloader.delivered(claim);
        lock_report(&self.report).finished += 1;
        self.reloader.changed();
        Ok(replaced)
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        if let Some(launcher) = self.reloader.current() {
            launcher.stop_developing(&self.reloader.identity);
        }
    }
}

/// How often a build handed over by `pane-ext` while the package is being
/// changed otherwise checks again whether it can be reloaded.
const CLAIM_RETRY: Duration = Duration::from_millis(100);

/// The launcher as the host of a developed package's session: it reloads
/// the package from each build that succeeds, as the Reload row reloads
/// the source folder.
struct Reloader {
    launcher: WeakLauncher,
    identity: PackageIdentity,
    /// The session this host belongs to.
    id: u64,
    /// Runs the reloads, which are futures, on the session's thread; made
    /// there, for the first.
    executor: Option<tokio::runtime::Runtime>,
}

/// A build the launcher claimed: its package is being reloaded.
struct Reloading {
    launcher: Launcher,
    /// What the reload reported.
    status: Status,
}

impl Reloader {
    /// The launcher, if the package is still developed by this session.
    fn current(&self) -> Option<Launcher> {
        let launcher = self.launcher.upgrade()?;
        launcher
            .developing
            .is_current(&self.identity, self.id)
            .then_some(launcher)
    }
}

impl Host for Reloader {
    type Claim = Reloading;

    fn is_current(&self) -> bool {
        self.current().is_some()
    }

    fn building(&self, command: &str) {
        if let Some(launcher) = self.launcher.upgrade() {
            let title = launcher.title_of(&self.identity);
            launcher.show_development(
                &self.identity,
                Status::Progress(format!("Building {title}: {command}…")),
            );
        }
    }

    fn failed(&self, failure: &BuildFailure, command: &str) {
        if let Some(launcher) = self.launcher.upgrade() {
            launcher.build_failed(&self.identity, failure, command);
        }
    }

    fn claim(&self) -> Claim<Reloading> {
        let Some(launcher) = self.current() else {
            return Claim::Ended;
        };
        // Checked under the lock that disabling, uninstalling and stopping
        // take, so that none is overtaken.
        let mut state = launcher.lock();
        if !launcher.developing.is_current(&self.identity, self.id)
            || launcher
                .changeable(&state, &self.identity, "reload")
                .is_err()
        {
            return Claim::Ended;
        }
        // A Reload or update is changing it: the session waits for it to
        // end.
        if state.changing.contains_key(&self.identity) {
            return Claim::Busy;
        }
        state.claim(&self.identity, Changing::Reloading);
        drop(state);
        Claim::Claimed(Reloading {
            launcher,
            status: Status::Idle,
        })
    }

    fn deliver(&mut self, claim: &mut Reloading, staging: &Path) -> bool {
        let launcher = &claim.launcher;
        let executor = match &mut self.executor {
            Some(executor) => executor,
            empty => match tokio::runtime::Builder::new_current_thread().build() {
                Ok(executor) => empty.insert(executor),
                Err(error) => {
                    let title = launcher.title_of(&self.identity);
                    claim.status = Status::Error(format!("Pane could not reload {title}: {error}"));
                    return false;
                }
            },
        };
        let epoch = launcher.lock().screen_epoch;
        let reload = Reload::staged(self.identity.clone(), staging.to_path_buf());
        let reloaded = executor.block_on(launcher.carry_out(epoch, reload));
        claim.status = reloaded.status;
        reloaded.replaced
    }

    fn delivered(&self, claim: Reloading) {
        let Reloading { launcher, status } = claim;
        {
            let mut state = launcher.lock();
            state.release(&self.identity);
            launcher.refresh(&mut state);
        }
        launcher.show_development(&self.identity, status);
    }

    fn installed(&self) -> Option<PathBuf> {
        self.launcher.upgrade()?.installed_location(&self.identity)
    }

    fn gave_up(&self) {
        if let Some(launcher) = self.launcher.upgrade() {
            let title = launcher.title_of(&self.identity);
            launcher.show_development(
                &self.identity,
                Status::Error(format!(
                    "{title} was not reloaded: its sources kept changing during \
                     {MAX_OBSOLETE} builds in a row. Save again to build it."
                )),
            );
        }
    }

    fn changed(&self) {
        if let Some(launcher) = self.launcher.upgrade() {
            launcher.developing.changed();
        }
    }
}
