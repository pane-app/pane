//! Installing or updating a package from a folder with the required
//! dependencies it is missing (see `dependencies`), exactly as the preview
//! showed them.
//!
//! The preview comes first: Pane's own forms asking which npm package or
//! Git repository to install, reading the package a folder, npm or Git
//! names without running any of it, and the package screen that shows what
//! it is, where it came from and what installing it needs.
//!
//! The preview's plan travels with its Install row as the plan's
//! [`Assumptions`]. Choosing the row claims, at once, the requested package
//! and every package the plan relies on ([`Changing::Installing`]), after
//! checking that they are still as the plan found them: until the install
//! ends, uninstalling, deleting the retained data of, reloading, updating,
//! enabling, disabling or starting to develop any of them is refused, and a
//! development build of one waits to reload it. The install then reads the
//! folders and works the plan out again; if it differs from the preview's
//! (a folder changed, or a package was installed or changed meanwhile),
//! nothing is installed and the new plan is shown. An install without a
//! preview ([`Launcher::install_package`]) plans, then claims the same way
//! before installing anything.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{
    Changing, Entry, FormField, FormPurpose, FormView, GIT_REPOSITORY_FIELD, Launcher,
    LauncherView, Mode, NPM_PACKAGE_FIELD, OpenForm, Row, Screen, State, Status, off_thread,
};
use crate::defaults::DefaultExtension;
use crate::dependencies::{self, Assumptions, Plan, RequiredState};
use crate::git::{self as git_source, GitSpec};
use crate::npm::{self, NpmSpec, Registry};
use crate::packages::{InstalledPackage, PackageError, PackageIdentity, SourcePackage, SourceSpec};
use crate::platform::{self, Platform};
use crate::runtime::FieldKind;
use crate::{helpers, operations};

/// Where a package to preview or install comes from.
#[derive(Clone, Debug)]
pub(in crate::launcher) enum Request {
    /// A local package folder.
    Folder(PathBuf),
    /// A package from npm, at the version named or else the latest.
    Npm(NpmSpec),
    /// A package from a Git repository, at the reference named or else its
    /// default branch.
    Git(GitSpec),
    /// A default extension's revision, from its repository at the commit
    /// its release tag points to — first setup's acquisition of a pin
    /// ([`crate::defaults`]) and the updater's of a newer release tag
    /// (#269), which read it with the default extension's identity. Never
    /// previewed: only those two flows make one.
    Default(DefaultExtension),
}

impl Request {
    /// How the preview names what was asked for when it cannot be read:
    /// "Folder: …" or "npm package: …", and the title's name for it.
    pub(in crate::launcher) fn describe(&self) -> (String, String) {
        match self {
            Request::Folder(folder) => (
                format!("Folder: {}", folder.display()),
                crate::packages::folder_name(folder),
            ),
            Request::Npm(spec) => (format!("npm package: {spec}"), spec.name.clone()),
            Request::Git(spec) => (
                format!("Git repository: {spec}"),
                spec.repository.name().to_owned(),
            ),
            Request::Default(pin) => (
                format!("Default extension: default:{}", pin.id),
                pin.title.clone(),
            ),
        }
    }
}

/// Reads packages from their sources: local folders, npm packages, which it
/// downloads from its registry, and Git repositories, which it fetches, into
/// its downloads folder.
#[derive(Clone)]
pub(in crate::launcher) struct Sources {
    pub registry: Registry,
    /// Where downloaded packages are unpacked; `None` when this launcher
    /// installs nothing, and so downloads nothing.
    pub downloads: Option<PathBuf>,
}

impl Sources {
    /// Reads and validates the package `request` names. Blocks on the file
    /// system, and for npm and Git on the network.
    pub fn read(&self, request: &Request) -> Result<SourcePackage, PackageError> {
        match request {
            Request::Folder(folder) => SourcePackage::read(folder),
            Request::Npm(spec) => self.fetch(spec),
            Request::Git(spec) => self.fetch_git(spec),
            Request::Default(pin) => self.fetch_default(pin),
        }
    }

    /// Reads the package with `identity`, a dependency declared with the
    /// source `source` (for npm, possibly naming a version; for Git, a
    /// reference).
    pub fn read_dependency(
        &self,
        identity: &PackageIdentity,
        source: &str,
    ) -> Result<SourcePackage, PackageError> {
        match (identity.local_folder(), SourceSpec::parse(source)) {
            (Some(folder), _) => SourcePackage::read(folder),
            (None, Ok(SourceSpec::Npm(spec))) => self.fetch(&spec),
            (None, Ok(SourceSpec::Git(spec))) => self.fetch_git(&spec),
            (None, _) => Err(PackageError::Npm(format!(
                "{identity} is not a source Pane can install from"
            ))),
        }
    }

    fn fetch(&self, spec: &NpmSpec) -> Result<SourcePackage, PackageError> {
        let Some(downloads) = &self.downloads else {
            return Err(PackageError::Storage(
                "this launcher does not install packages".into(),
            ));
        };
        // Its download is removed with the package read from it, or at once
        // if it cannot be read.
        let fetched = npm::fetch(&self.registry, spec, downloads).map_err(PackageError::Npm)?;
        SourcePackage::read_npm(fetched)
    }

    fn fetch_git(&self, spec: &GitSpec) -> Result<SourcePackage, PackageError> {
        let Some(downloads) = &self.downloads else {
            return Err(PackageError::Storage(
                "this launcher does not install packages".into(),
            ));
        };
        // As for npm, its download goes with the package read from it.
        let fetched = git_source::fetch(spec, downloads).map_err(PackageError::Git)?;
        SourcePackage::read_git(fetched)
    }

    /// Fetches the release tag's commit `pin` names and reads it as the
    /// default extension's package — with the default extension's
    /// identity and its Git source recorded — exactly as first setup
    /// acquires one, retries included ([`crate::defaults::fetch`]).
    fn fetch_default(&self, pin: &DefaultExtension) -> Result<SourcePackage, PackageError> {
        let Some(downloads) = &self.downloads else {
            return Err(PackageError::Storage(
                "this launcher does not install packages".into(),
            ));
        };
        let fetched = crate::defaults::fetch(pin, downloads)
            .map_err(|why| PackageError::Defaults(why.to_string()))?;
        SourcePackage::read_default(fetched)
    }
}

/// An install begun by choosing Install or Update on a preview, or asked
/// for without one.
pub(in crate::launcher) struct Begun {
    request: Request,
    mode: Mode,
    /// The assumptions of the plan the preview showed, if there was one.
    shown: Option<Assumptions>,
    /// The packages claimed for this install, released when it ends.
    claimed: Vec<PackageIdentity>,
}

impl Begun {
    /// An install of the package `request` names without a preview.
    pub(in crate::launcher) fn unplanned(request: Request) -> Begun {
        Begun {
            request,
            mode: Mode::Install,
            shown: None,
            claimed: Vec::new(),
        }
    }
}

/// What an install added.
pub(in crate::launcher) struct Outcome {
    /// The required dependencies installed with it, first installed first.
    pub(in crate::launcher) dependencies: Vec<InstalledPackage>,
    pub(in crate::launcher) package: InstalledPackage,
    /// Titles of required dependencies the user disabled, which stay so.
    pub(in crate::launcher) disabled: Vec<String>,
    /// Titles of required dependencies Pane paused, which stay so.
    pub(in crate::launcher) paused: Vec<String>,
}

/// Why an install installed nothing, or not all of it.
pub(in crate::launcher) enum Stopped {
    Failed(dependencies::Failure),
    /// The plan is not the one the preview showed: this is the new one.
    Changed(Box<(SourcePackage, Plan)>),
}

/// Why packages cannot be claimed for an install.
pub(in crate::launcher) enum Refusal {
    /// One of them is busy; the message says which and how.
    Busy(String),
    /// They are not as the plan assumed.
    Changed,
}

/// "Nothing was installed: <each problem>", for the plan's problems.
pub(in crate::launcher) fn problems(plan: &Plan) -> PackageError {
    PackageError::Dependencies(plan.problems.iter().map(ToString::to_string).collect())
}

fn failed(error: impl ToString) -> Stopped {
    Stopped::Failed(dependencies::Failure {
        error: error.to_string(),
        left_installed: Vec::new(),
    })
}

/// The message for an install or update that installed `outcome` as `mode`
/// asked: what was installed or updated, with which required dependencies,
/// and which of them the user left disabled or Pane left paused.
pub(in crate::launcher) fn outcome_message(mode: &Mode, outcome: &Outcome) -> String {
    let title = outcome.package.title();
    let mut message = match (mode, outcome.package.version()) {
        (Mode::Install, _) => format!("Installed {title}"),
        (Mode::Update(_), Some(version)) => format!("Updated {title} to {version}"),
        (Mode::Update(_), None) => format!("Updated {title}"),
    };
    if !outcome.dependencies.is_empty() {
        let titles: Vec<String> = outcome
            .dependencies
            .iter()
            .map(InstalledPackage::title)
            .collect();
        message.push_str(&format!(
            " with {}, which it requires",
            platform::join(&titles)
        ));
    }
    if !outcome.disabled.is_empty() {
        message.push_str(&format!(
            "; {} stays disabled: enable it in Settings for {title} to use it",
            platform::join(&outcome.disabled)
        ));
    }
    if !outcome.paused.is_empty() {
        message.push_str(&format!(
            "; {} stays paused after an error: retry it in Settings for {title} to \
             use it",
            platform::join(&outcome.paused)
        ));
    }
    message
}

/// Claims for an install in `mode` the requested package and every package
/// the plan with `assumptions` relies on, if they are as it assumed and
/// nothing else is happening to them. The updated package is claimed as
/// `updating`: [`Changing::Updating`] for an update the user chose, and
/// [`Changing::BackgroundUpdating`] for the one the updater applies by
/// itself — which is what tells the two apart to a call the user makes into
/// the package in the moment between the boundary check and the
/// replacement (see [`Launcher::open_command`]).
pub(in crate::launcher) fn claim(
    state: &mut State,
    mode: &Mode,
    assumptions: &Assumptions,
    updating: Changing,
) -> Result<Vec<PackageIdentity>, Refusal> {
    if !assumptions.hold(&state.packages, |identity| state.paused.is_paused(identity)) {
        return Err(Refusal::Changed);
    }
    let identities: Vec<PackageIdentity> = std::iter::once(&assumptions.requested)
        .chain(assumptions.packages.iter().map(|(identity, _)| identity))
        .cloned()
        .collect();
    let claims: Vec<(PackageIdentity, Changing)> = identities
        .iter()
        .map(|identity| {
            let what = match mode {
                Mode::Update(updated) if updated == identity => updating,
                _ => Changing::Installing,
            };
            (identity.clone(), what)
        })
        .collect();
    if let Err((identity, busy)) = state.claim_all(&claims) {
        let mut message = format!("{} {}", state.title_of(&identity), busy.doing());
        // Its data is being removed: installing it later finds none.
        if matches!(busy, Changing::Uninstalling | Changing::DeletingRetained) {
            message.push_str("; install it again once that is done");
        }
        return Err(Refusal::Busy(message));
    }
    Ok(identities)
}

/// Where the install preview of a local folder stands, for the local
/// channel (`crate::local_channel`), which shows it when `pane-ext dev`
/// first develops a folder Pane has not installed (#217).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InstallPreview {
    /// The package is installed.
    Installed,
    /// Its preview is on show, offering Install, or installing it.
    Shown,
    /// Its preview explains why it cannot be installed.
    Refused(String),
    /// Something else is on show.
    Elsewhere,
}

/// The message when the plan changed since the preview.
fn changed(title: &str) -> String {
    format!(
        "What installing {title} needs changed since it was shown; check it again and choose \
         Install once more"
    )
}

impl Launcher {
    /// Reads the package in `folder` and shows its identity, version,
    /// commands and compatibility, offering Install, or Update when a
    /// package with the same identity is installed. No guest code runs. An
    /// invalid or incompatible package is explained instead.
    pub fn preview_package(&self, folder: &Path) -> impl Future<Output = ()> + Send + 'static {
        self.preview(Ok(Request::Folder(folder.to_path_buf())))
    }

    /// Downloads the npm package `spec` names (`name`, `@scope/name`, with
    /// an optional exact version: `name@1.2.3`) and shows it as
    /// [`Launcher::preview_package`] shows a folder, with the npm version it
    /// would install. Without a version it is the latest; with one,
    /// installing pins the package to it. Nothing in the package runs.
    pub fn preview_npm(&self, spec: &str) -> impl Future<Output = ()> + Send + 'static {
        let asked = spec.trim().to_owned();
        let request = crate::npm::NpmSpec::parse(spec)
            .map(Request::Npm)
            .map_err(|why| (format!("npm package: {asked}"), asked, why));
        self.preview(request)
    }

    /// Fetches the revision of the Git repository `spec` names (an address
    /// such as `https://github.com/owner/repo`, `github.com/owner/repo` or
    /// `git@github.com:owner/repo`, with an optional `@<branch, tag or
    /// commit>`) and shows it as [`Launcher::preview_package`] shows a
    /// folder, with the revision it would install. Without a reference it is
    /// the default branch, tracked; a branch is tracked, a tag or a commit
    /// pinned. For an installed repository named without one, the installed
    /// reference is kept. Nothing in the repository runs.
    pub fn preview_git(&self, spec: &str) -> impl Future<Output = ()> + Send + 'static {
        let asked = spec.trim().to_owned();
        let request = crate::git::GitSpec::parse(spec)
            .map(Request::Git)
            .map_err(|why| (format!("Git repository: {asked}"), asked, why));
        self.preview(request)
    }

    /// Previews the package `request` names, or explains why the text asked
    /// for (its detail line, the text and the reason) names none.
    fn preview(
        &self,
        request: Result<Request, (String, String, String)>,
    ) -> impl Future<Output = ()> + Send + 'static {
        let epoch = self.start_running();
        let launcher = self.clone();
        async move {
            let request = match request {
                Ok(request) => request,
                Err((detail, asked, why)) => {
                    let mut state = launcher.lock();
                    if state.screen_epoch == epoch {
                        launcher.leave_command(&mut state);
                        state.entries = Vec::new();
                        state.view = LauncherView {
                            status: Status::Error(why),
                            ..LauncherView::new(
                                Screen::Package {
                                    details: vec![detail],
                                },
                                format!("Cannot install {asked}"),
                            )
                        };
                    }
                    return;
                }
            };
            let request = launcher.keeping_pin(request);
            let checked = match launcher.read_and_check(request.clone()).await {
                Ok(package) => Ok(launcher.plan_dependencies(package).await),
                Err(error) => Err(error),
            };
            let mut state = launcher.lock();
            if state.screen_epoch != epoch {
                return;
            }
            launcher.show_preview(&mut state, &request, checked);
        }
    }

    /// `request`, or for an npm package without a version that is installed
    /// pinned to one, that version: updating it keeps its pin, which only
    /// naming another version changes. Likewise a Git repository without a
    /// reference keeps the branch, tag or commit it is installed from.
    fn keeping_pin(&self, request: Request) -> Request {
        match request {
            Request::Npm(spec) if spec.version.is_none() => {
                let state = self.lock();
                let pinned = state
                    .package(&PackageIdentity::npm(&spec.name))
                    .and_then(|package| package.npm.as_ref())
                    .filter(|npm| npm.pinned)
                    .map(|npm| npm.version.clone());
                Request::Npm(crate::npm::NpmSpec {
                    version: pinned,
                    ..spec
                })
            }
            // A repository named without a reference keeps the one it is
            // installed from: its branch, tag or commit.
            Request::Git(spec) if spec.reference.is_none() => {
                let state = self.lock();
                let installed = state
                    .package(&PackageIdentity::git(&spec.repository))
                    .and_then(|package| package.git.as_ref())
                    .and_then(|git| git.revision.asked_as());
                Request::Git(crate::git::GitSpec {
                    reference: installed,
                    ..spec
                })
            }
            other => other,
        }
    }

    /// Shows the package screen for `request`, from its package and plan or
    /// why it cannot be read.
    fn show_preview(
        &self,
        state: &mut State,
        request: &Request,
        checked: Result<(SourcePackage, dependencies::Plan), PackageError>,
    ) {
        let installed = checked
            .as_ref()
            .ok()
            .and_then(|(package, _)| state.package(&package.identity).cloned());
        let (view, entries) = preview_view(request, checked, installed);
        self.leave_command(state);
        state.view = view;
        state.entries = entries;
    }

    /// Where the install preview of the local package with `identity`,
    /// shown by [`Launcher::preview_package`] for its folder, stands.
    pub(crate) fn previewing(&self, identity: &PackageIdentity) -> InstallPreview {
        let state = self.lock();
        if state.package(identity).is_some() {
            return InstallPreview::Installed;
        }
        let (Screen::Package { details }, Some(folder)) =
            (&state.view.screen, identity.local_folder())
        else {
            return InstallPreview::Elsewhere;
        };
        let offered = state.entries.iter().any(|entry| {
            matches!(entry, Entry::Install(Request::Folder(shown), _, _) if shown == folder)
        });
        if offered {
            return InstallPreview::Shown;
        }
        // A package that cannot be installed is explained, with nothing
        // offered: its source, or the folder if it could not be read.
        let named = [
            format!("Source: {identity}"),
            format!("Folder: {}", folder.display()),
        ];
        match &state.view.status {
            Status::Error(why) if details.iter().any(|line| named.contains(line)) => {
                InstallPreview::Refused(why.clone())
            }
            _ => InstallPreview::Elsewhere,
        }
    }

    /// Shows Pane's own form asking which npm package to install.
    pub(in crate::launcher) fn show_npm_form(&self, state: &mut State) {
        let form = FormView {
            fields: vec![FormField {
                id: NPM_PACKAGE_FIELD.into(),
                label: "npm package: its name, and a version to install that one".into(),
                kind: FieldKind::Text {
                    placeholder: Some("such as @scope/name or name@1.2.3".into()),
                },
                value: String::new(),
                error: None,
                description: None,
                required: false,
            }],
            submit_label: "Show package".into(),
            setup: None,
        };
        let view = LauncherView::new(Screen::Form(form), "Install extension from npm");
        let return_to = std::mem::replace(&mut state.view, view);
        state.form = Some(OpenForm {
            purpose: FormPurpose::Npm,
            return_to,
            submitting: false,
        });
        state.screen_epoch += 1;
    }

    /// Shows Pane's own form asking which Git repository to install from.
    pub(in crate::launcher) fn show_git_form(&self, state: &mut State) {
        let form = FormView {
            fields: vec![FormField {
                id: GIT_REPOSITORY_FIELD.into(),
                label: "Git repository: its address, and @ a branch, tag or commit to install \
                        that one"
                    .into(),
                kind: FieldKind::Text {
                    placeholder: Some("such as https://github.com/owner/repo@v1.0.0".into()),
                },
                value: String::new(),
                error: None,
                description: None,
                required: false,
            }],
            submit_label: "Show package".into(),
            setup: None,
        };
        let view = LauncherView::new(Screen::Form(form), "Install extension from Git");
        let return_to = std::mem::replace(&mut state.view, view);
        state.form = Some(OpenForm {
            purpose: FormPurpose::Git,
            return_to,
            submitting: false,
        });
        state.screen_epoch += 1;
    }

    /// Installs the package in `folder` as an explicit install request, with
    /// the required dependencies it is missing, as the preview would show
    /// them. A package whose identity is already installed is rejected:
    /// replacing it is an update.
    pub fn install_package(&self, folder: &Path) -> impl Future<Output = ()> + Send + 'static {
        self.install_unplanned(Ok(Request::Folder(folder.to_path_buf())))
    }

    /// Installs the npm package `spec` names as an explicit install request,
    /// as [`Launcher::install_package`] installs a folder: a package whose
    /// npm name is already installed is rejected, whatever its version.
    pub fn install_npm(&self, spec: &str) -> impl Future<Output = ()> + Send + 'static {
        self.install_unplanned(crate::npm::NpmSpec::parse(spec).map(Request::Npm))
    }

    /// Installs the revision of the Git repository `spec` names as an
    /// explicit install request, as [`Launcher::install_package`] installs a
    /// folder: a repository already installed, in any of its equivalent
    /// forms, is rejected, whatever the reference.
    pub fn install_git(&self, spec: &str) -> impl Future<Output = ()> + Send + 'static {
        self.install_unplanned(crate::git::GitSpec::parse(spec).map(Request::Git))
    }

    fn install_unplanned(
        &self,
        request: Result<Request, String>,
    ) -> impl Future<Output = ()> + Send + 'static {
        let epoch = self.start_running();
        let launcher = self.clone();
        async move {
            match request {
                Ok(request) => {
                    let install = Begun::unplanned(request);
                    launcher.finish_install(epoch, install).await
                }
                Err(why) => {
                    if let Some(mut state) = launcher.lock_if_current(epoch) {
                        state.view.status = Status::Error(why);
                    }
                }
            }
        }
    }

    /// Begins installing the package `request` names as the preview's plan
    /// with `assumptions` showed it: claims what it relies on, or explains
    /// why not and returns `None`.
    pub(in crate::launcher) fn begin_install(
        &self,
        state: &mut State,
        request: Request,
        mode: Mode,
        assumptions: Assumptions,
    ) -> Option<Begun> {
        if self.installation.is_none() {
            let error = PackageError::Storage("this launcher does not install packages".into());
            state.view.status = Status::Error(error.to_string());
            return None;
        }
        let claimed = match claim(state, &mode, &assumptions, Changing::Updating) {
            Ok(claimed) => claimed,
            Err(Refusal::Busy(message)) => {
                state.view.status = Status::Error(message);
                return None;
            }
            // Planned again, the change is shown.
            Err(Refusal::Changed) => Vec::new(),
        };
        state.view.status = Status::Running {
            since: Instant::now(),
        };
        Some(Begun {
            request,
            mode,
            shown: Some(assumptions),
            claimed,
        })
    }

    /// Installs or updates the package with the required dependencies it
    /// is missing, or explains why not, or shows how its plan changed.
    pub(in crate::launcher) async fn finish_install(&self, epoch: u64, begun: Begun) {
        let Begun {
            request,
            mode,
            shown,
            mut claimed,
        } = begun;
        let result = self
            .install_planned(request.clone(), &mode, shown.as_ref(), &mut claimed)
            .await;
        // The install's own work, in a block so the state's lock is not
        // held across the development that may follow: what it answers is
        // the package Create Extension or Import Extension previewed, to
        // develop once it is installed (see `create`).
        let developed = {
            let mut state = self.lock();
            for identity in &claimed {
                state.release(identity);
            }
            let current = state.screen_epoch == epoch;
            match result {
                Ok(outcome) => {
                    let message = outcome_message(&mode, &outcome);
                    for dependency in outcome.dependencies {
                        self.put_installed(&mut state, dependency);
                    }
                    let package = outcome.package;
                    // What Create Extension or Import Extension asked for:
                    // the package they previewed is developed once it is
                    // installed (see `create`).
                    let develop_after = state
                        .develop_after
                        .take_if(|identity| *identity == package.identity);
                    let first = package.commands().first().map(|c| c.component.clone());
                    let replaced_is_open = self.put_installed(&mut state, package);
                    if current || replaced_is_open {
                        self.show_root(&mut state, first);
                        state.view.status = Status::Result(message);
                    } else {
                        self.refresh(&mut state);
                    }
                    develop_after
                }
                Err(Stopped::Changed(changed_plan)) => {
                    let (package, plan) = *changed_plan;
                    if current {
                        let title = package.manifest.title.clone();
                        self.show_preview(&mut state, &request, Ok((package, plan)));
                        state.view.status = Status::Error(changed(&title));
                    }
                    None
                }
                Err(Stopped::Failed(failure)) => {
                    let message = self.install_left_behind(&mut state, &failure);
                    if current {
                        state.view.status = Status::Error(message);
                    }
                    None
                }
            }
        };
        // The package Create Extension or Import Extension previewed is
        // developed as its own row would develop it: its folder is watched
        // from now on, each save building and reloading it.
        if let Some(identity) = developed {
            let start = {
                let mut state = self.lock();
                self.begin_developing(&mut state, &identity)
            };
            if let Some(start) = start {
                self.finish_developing(identity, start).await;
            }
        }
    }

    /// Reads and checks the package `request` names, plans its dependencies
    /// and, if the plan is the one `shown` (when a preview showed one) and
    /// can be installed, claims what it relies on (unless `claimed` already
    /// holds it), then installs or updates it with those it is missing: all
    /// of them or, removing again what it installed when one fails, none.
    /// What it downloaded from npm or Git is removed with the packages read
    /// from it, once they are installed or not.
    async fn install_planned(
        &self,
        request: Request,
        mode: &Mode,
        shown: Option<&Assumptions>,
        claimed: &mut Vec<PackageIdentity>,
    ) -> Result<Outcome, Stopped> {
        let Some(store) = self.installation.as_ref().map(|i| i.store.clone()) else {
            return Err(failed(PackageError::Storage(
                "this launcher does not install packages".into(),
            )));
        };
        let package = self.read_and_check(request).await.map_err(failed)?;
        let (package, plan) = self.plan_dependencies(package).await;
        self.install_plan(store, package, plan, mode, shown, claimed)
            .await
    }

    /// Installs `package` with `plan`, as [`Launcher::install_planned`]
    /// describes, once it has been read and planned.
    pub(in crate::launcher) async fn install_plan(
        &self,
        store: std::sync::Arc<std::sync::Mutex<crate::packages::Store>>,
        package: SourcePackage,
        plan: Plan,
        mode: &Mode,
        shown: Option<&Assumptions>,
        claimed: &mut Vec<PackageIdentity>,
    ) -> Result<Outcome, Stopped> {
        if shown.is_some_and(|shown| *shown != plan.assumptions) {
            return Err(Stopped::Changed(Box::new((package, plan))));
        }
        if !plan.problems.is_empty() {
            return Err(failed(problems(&plan)));
        }
        if claimed.is_empty() {
            let mut state = self.lock();
            match claim(&mut state, mode, &plan.assumptions, Changing::Updating) {
                Ok(identities) => *claimed = identities,
                Err(Refusal::Busy(message)) => return Err(failed(message)),
                Err(Refusal::Changed) => return Err(failed(changed(&package.manifest.title))),
            }
        }
        let disabled = plan.titles_in(RequiredState::Disabled);
        let paused = plan.titles_in(RequiredState::Paused);
        let mode = mode.clone();
        let retire = self.retire(&package.identity);
        let (dependencies, package) = off_thread(move || {
            let mut store = store.lock().unwrap_or_else(|p| p.into_inner());
            dependencies::install_all(&mut store, &plan.install, |store| match mode {
                Mode::Install => store.install(&package),
                Mode::Update(_) => store.update(&package, retire),
            })
        })
        .await
        .map_err(Stopped::Failed)?;
        Ok(Outcome {
            dependencies,
            package,
            disabled,
            paused,
        })
    }

    /// Works out what installing `package` means for its dependencies (see
    /// `dependencies`) against the installed packages, reading the folders
    /// of those it would install off the calling thread and having the
    /// runtime check their components without running them (noting whether
    /// they use the network, as for the package itself).
    pub(in crate::launcher) async fn plan_dependencies(
        &self,
        package: SourcePackage,
    ) -> (SourcePackage, Plan) {
        self.plan_dependencies_from(self.sources.clone(), package)
            .await
    }

    /// Like [`Launcher::plan_dependencies`], reading the dependencies the
    /// plan installs from `sources` rather than this launcher's own: the
    /// updater's thread (see [`Launcher::read_and_check_from`]).
    pub(in crate::launcher) async fn plan_dependencies_from(
        &self,
        sources: Sources,
        package: SourcePackage,
    ) -> (SourcePackage, Plan) {
        let (installed, paused) = {
            let state = self.lock();
            let paused: Vec<PackageIdentity> = state
                .packages
                .iter()
                .filter(|p| state.paused.is_paused(&p.identity))
                .map(|p| p.identity.clone())
                .collect();
            (state.packages.clone(), paused)
        };
        let (package, mut plan) = off_thread(move || {
            let read = |identity: &PackageIdentity, source: &str| {
                sources.read_dependency(identity, source)
            };
            let plan = dependencies::plan(&package, &installed, &paused, read);
            (package, plan)
        })
        .await;
        let mut problems = Vec::new();
        for dependency in &mut plan.install {
            match self.check_components(dependency).await {
                // Recorded as the package's own is: whether it can make web
                // requests and run system programs.
                Ok(checked) => dependency.note_imports(checked),
                Err(error) => {
                    let required = plan
                        .required
                        .iter()
                        .find(|required| required.target.identity == dependency.identity)
                        .expect("a package to install is a required dependency");
                    problems.push(dependencies::Problem {
                        dependent: required.dependent.clone(),
                        id: required.id.clone(),
                        kind: dependencies::ProblemKind::CannotInstall {
                            from: dependencies::source_name(&dependency.identity),
                            error,
                        },
                    });
                }
            }
        }
        plan.problems.extend(problems);
        (package, plan)
    }
    /// What an install that stopped partway leaves: the message for the
    /// status line, with the packages it had already installed put back
    /// and the state refreshed, so the caller shows the message as its
    /// screen allows (an install the user chose, a default extension's
    /// acquisition).
    pub(in crate::launcher) fn install_left_behind(
        &self,
        state: &mut State,
        failure: &dependencies::Failure,
    ) -> String {
        let mut message = failure.error.clone();
        if !failure.left_installed.is_empty() {
            let left: Vec<String> = failure
                .left_installed
                .iter()
                .map(|(package, why)| format!("{} ({why})", package.title()))
                .collect();
            message.push_str(&format!(
                ". Pane could not remove again what it had installed, which stays \
                 installed: {}",
                platform::join(&left)
            ));
            for (package, _) in failure.left_installed.clone() {
                self.put_installed(state, package);
            }
            self.refresh(state);
        }
        message
    }
}

/// The package screen for `request`: what the package is and whether it
/// can be installed, or why it cannot.
fn preview_view(
    request: &Request,
    checked: Result<(SourcePackage, dependencies::Plan), PackageError>,
    installed: Option<InstalledPackage>,
) -> (LauncherView, Vec<Entry>) {
    let (package, plan) = match checked {
        Ok(checked) => checked,
        Err(error) => {
            let (asked, name) = request.describe();
            let view = LauncherView {
                status: Status::Error(error.to_string()),
                ..LauncherView::new(
                    Screen::Package {
                        details: vec![asked],
                    },
                    format!("Cannot install {name}"),
                )
            };
            return (view, Vec::new());
        }
    };
    let manifest = &package.manifest;
    let mut details = vec![format!("Source: {}", package.identity)];
    // What the package does, in a sentence: the preview shows it beside
    // the source, as the extension's page in Settings does under the
    // title (#224).
    if let Some(description) = &manifest.description {
        details.push(description.clone());
    }
    if let Some(version) = &manifest.version {
        details.push(format!("Version: {version}"));
    }
    if let Some(npm) = &package.npm {
        let pinned_to = installed
            .as_ref()
            .and_then(|installed| installed.npm.as_ref())
            .filter(|installed| installed.pinned)
            .map(|installed| installed.version.as_str());
        details.extend(npm_lines(npm, pinned_to));
    }
    if let Some(git) = &package.git {
        let installed = installed
            .as_ref()
            .and_then(|installed| installed.git.as_ref());
        details.extend(git_lines(git, installed));
    }
    // A published package has its own 512×512 icon (#139); one from npm or
    // Git without it is installed anyway, with a caution.
    if (package.npm.is_some() || package.git.is_some())
        && let Some(caution) = crate::icons::caution(&package.folder, manifest.icon.as_ref())
    {
        details.push(format!("Caution: {caution}"));
    }
    let titles: Vec<&str> = manifest.commands.iter().map(|c| c.title.as_str()).collect();
    if !titles.is_empty() {
        details.push(format!("Commands: {}", titles.join(", ")));
    }
    if let Some(operations) = operations::describe(&manifest.operations) {
        details.push(operations);
    }
    if let Some(helpers) = helpers::describe(&manifest.helpers) {
        details.push(helpers);
    }
    if package.programs {
        details.push(super::programs::PREVIEW_NOTE.into());
    }
    details.push(format!(
        "Compatible: needs extension API {}, and its components import only WASI 0.3",
        manifest.api_version
    ));
    if let Some(platforms) = &manifest.platforms {
        // A package that does not support this system is explained instead.
        let names: Vec<String> = platforms
            .iter()
            .map(|&platform| {
                if Some(platform) == Platform::current() {
                    format!("{platform} (this system)")
                } else {
                    platform.to_string()
                }
            })
            .collect();
        details.push(format!("Supported systems: {}", platform::join(&names)));
    }
    details.extend(plan.lines());
    if !plan.problems.is_empty() {
        // Nothing is offered: a required dependency cannot be installed.
        let view = LauncherView {
            status: Status::Error(problems(&plan).to_string()),
            ..LauncherView::new(
                Screen::Package { details },
                format!("Cannot install {}", manifest.title),
            )
        };
        return (view, Vec::new());
    }
    let with = match plan.installed_with().as_slice() {
        [] => String::new(),
        [one] => format!(", and install {one}, which it requires"),
        titles => format!(", and install the {} extensions it requires", titles.len()),
    };
    let (row, entry) = match installed {
        Some(installed) => {
            details.push(
                match (&installed.npm, &installed.git, installed.version()) {
                    (Some(npm), _, _) => format!(
                        "Installed: npm version {}{} of this package",
                        npm.version,
                        if npm.pinned { ", pinned" } else { "" }
                    ),
                    (None, Some(git), _) => format!(
                        "Installed: {} (commit {}) of this repository",
                        git.revision.describe(),
                        git.revision.short_commit()
                    ),
                    (None, None, Some(version)) => {
                        format!("Installed: version {version} from this folder")
                    }
                    (None, None, None) => "Installed from this folder".into(),
                },
            );
            if !installed.enabled {
                details.push("Disabled: enable it in Settings".into());
            }
            let replace = match (package.npm.as_ref().map(|npm| &npm.package), &package.git) {
                (Some(npm), _) if npm.pinned => format!("npm version {}, pinned", npm.version),
                (Some(npm), _) => format!("npm version {}, the latest", npm.version),
                (None, Some(git)) => format!(
                    "{} (commit {}){}",
                    git.revision.describe(),
                    git.revision.short_commit(),
                    if git.revision.pinned() {
                        ", pinned"
                    } else {
                        ", tracked"
                    }
                ),
                (None, None) => "this folder's contents".into(),
            };
            let row = Row {
                id: "update".into(),
                title: "Update".into(),
                subtitle: Some(format!("Replace the installed copy with {replace}{with}")),
                unavailable: None,
            };
            let mode = Mode::Update(installed.identity.clone());
            (
                row,
                Entry::Install(request.clone(), mode, plan.assumptions.clone()),
            )
        }
        None => {
            let row = Row {
                id: "install".into(),
                title: "Install".into(),
                subtitle: Some(format!(
                    "Copy the package into Pane and add its commands{with}"
                )),
                unavailable: None,
            };
            (
                row,
                Entry::Install(request.clone(), Mode::Install, plan.assumptions.clone()),
            )
        }
    };
    let view =
        LauncherView::new(Screen::Package { details }, manifest.title.clone()).with_rows(vec![row]);
    (view, vec![entry])
}

/// The preview's lines about where a package from npm was downloaded from,
/// and what Pane does not do with it; `pinned_to` is the version the
/// installed copy is pinned to, if it is.
fn npm_lines(npm: &crate::npm::NpmOrigin, pinned_to: Option<&str>) -> Vec<String> {
    let version = &npm.package.version;
    let mut lines = vec![
        if npm.package.pinned && pinned_to == Some(version) {
            format!(
                "npm version: {version}, the version it is pinned to: name another version to \
                 change it"
            )
        } else if npm.package.pinned {
            format!(
                "npm version: {}, the version you named: installing pins it to that version",
                npm.package.version
            )
        } else {
            format!("npm version: {}, the latest", npm.package.version)
        },
        format!(
            "Downloaded: {}, matching its sha512 integrity from the registry",
            npm.tarball
        ),
        "Runs only the WebAssembly components its pane.json names, in Pane: no Node.js, npm \
         install scripts or npm dependencies"
            .into(),
    ];
    if !npm.scripts.is_empty() || npm.has_npm_dependencies {
        let mut ignored = Vec::new();
        if !npm.scripts.is_empty() {
            let scripts: Vec<String> = npm.scripts.iter().map(|s| format!("`{s}`")).collect();
            ignored.push(format!("its {} script", platform::join(&scripts)));
        }
        if npm.has_npm_dependencies {
            ignored.push("its npm dependencies".into());
        }
        lines.push(format!(
            "Not used: {}, which its package.json declares; Pane never runs or installs them",
            platform::join(&ignored)
        ));
    }
    lines
}

/// The preview's lines about the Git revision a package was fetched from,
/// whether it is tracked or pinned, and what Pane does not do with it;
/// `installed` is the installed copy's, if the repository is installed.
fn git_lines(
    git: &crate::git::GitOrigin,
    installed: Option<&crate::git::InstalledGit>,
) -> Vec<String> {
    use crate::git::GitRef;
    let revision = &git.revision;
    let kept = installed.is_some_and(|installed| {
        installed.revision.reference == revision.reference && revision.pinned()
    });
    let what = match &revision.reference {
        GitRef::Default { .. } => format!(
            "Revision: {}, tracked: an update fetches that branch again",
            revision.describe()
        ),
        GitRef::Branch(_) => format!(
            "Revision: {}, tracked: an update fetches that branch again",
            revision.describe()
        ),
        GitRef::Tag(_) | GitRef::Commit if kept => format!(
            "Revision: {}, which it is pinned to: name another branch, tag or commit to change it",
            revision.describe()
        ),
        GitRef::Tag(_) | GitRef::Commit => format!(
            "Revision: {}, which you named: installing pins it to that revision",
            revision.describe()
        ),
    };
    let subject = match git.subject.as_str() {
        "" => String::new(),
        subject => format!(" “{subject}”"),
    };
    // Where it was served, not who made it: a commit's id proves its
    // contents, while a host sharing storage between forks serves a fork's
    // commits at this address too.
    let served = match git.repository.written_as_ssh() {
        true => format!(
            "SSH address fetched over HTTPS from {}",
            git.repository.url()
        ),
        false => format!("served at {}", git.repository.url()),
    };
    let mut lines = vec![
        what,
        format!(
            "Fetched: commit {}{subject}, {served}; each object checked against its id",
            revision.commit
        ),
    ];
    if let Some(caution) = git.caution() {
        lines.push(format!("Caution: {caution}"));
    }
    // One short line, so that the preview's Git lines fit above Install.
    // What the package runs, its components and any helpers, is listed as
    // its Commands, Operations and Helpers: only what Pane itself never
    // does is said here.
    lines.push("Pane builds nothing and runs no repository hooks, scripts or submodules".into());
    lines
}

#[cfg(test)]
mod git_lines_tests;
