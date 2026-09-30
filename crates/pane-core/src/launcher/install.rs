//! Installing or updating a package from a folder with the required
//! dependencies it is missing (see `dependencies`), exactly as the preview
//! showed them.
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

use std::path::PathBuf;

use super::{Changing, Launcher, Mode, State, Status, off_thread};
use crate::dependencies::{self, Assumptions, Plan, RequiredState};
use crate::git::{self as git_source, GitSpec};
use crate::npm::{self, NpmSpec, Registry};
use crate::packages::{InstalledPackage, PackageError, PackageIdentity, SourcePackage, SourceSpec};
use crate::platform;

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
enum Refusal {
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

/// Claims for an install in `mode` the requested package and every package
/// the plan with `assumptions` relies on, if they are as it assumed and
/// nothing else is happening to them.
fn claim(
    state: &mut State,
    mode: &Mode,
    assumptions: &Assumptions,
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
                Mode::Update(updated) if updated == identity => Changing::Updating,
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

/// The message when the plan changed since the preview.
fn changed(title: &str) -> String {
    format!(
        "What installing {title} needs changed since it was shown; check it again and choose \
         Install once more"
    )
}

impl Launcher {
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
        let claimed = match claim(state, &mode, &assumptions) {
            Ok(claimed) => claimed,
            Err(Refusal::Busy(message)) => {
                state.view.status = Status::Error(message);
                return None;
            }
            // Planned again, the change is shown.
            Err(Refusal::Changed) => Vec::new(),
        };
        state.view.status = Status::Running;
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
        let mut state = self.lock();
        for identity in &claimed {
            state.release(identity);
        }
        let current = state.screen_epoch == epoch;
        match result {
            Ok(outcome) => {
                let title = outcome.package.title();
                let mut message = match (&mode, outcome.package.version()) {
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
                        "; {} stays disabled: enable it in Manage extensions for {title} to use it",
                        platform::join(&outcome.disabled)
                    ));
                }
                if !outcome.paused.is_empty() {
                    message.push_str(&format!(
                        "; {} stays paused after an error: retry it in Manage extensions for \
                         {title} to use it",
                        platform::join(&outcome.paused)
                    ));
                }
                for dependency in outcome.dependencies {
                    self.put_installed(&mut state, dependency);
                }
                let package = outcome.package;
                let first = package.commands().first().map(|c| c.component.clone());
                let replaced_is_open = self.put_installed(&mut state, package);
                if current || replaced_is_open {
                    self.show_root(&mut state, first);
                    state.view.status = Status::Result(message);
                } else {
                    self.refresh(&mut state);
                }
            }
            Err(Stopped::Changed(changed_plan)) => {
                let (package, plan) = *changed_plan;
                if current {
                    let title = package.manifest.title.clone();
                    self.show_preview(&mut state, &request, Ok((package, plan)));
                    state.view.status = Status::Error(changed(&title));
                }
            }
            Err(Stopped::Failed(failure)) => {
                let message = self.install_left_behind(&mut state, &failure);
                if current {
                    state.view.status = Status::Error(message);
                }
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
            match claim(&mut state, mode, &plan.assumptions) {
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
        let sources = self.sources.clone();
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
                // requests.
                Ok(network) => dependency.network = network,
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
