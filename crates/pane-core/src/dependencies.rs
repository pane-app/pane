//! Dependencies between extension packages: what installing a package
//! means for the packages it declares under `dependencies` in its
//! `pane.json`, worked out before anything is installed, and installing
//! them together.
//!
//! - A **required** dependency that is not installed is installed with the
//!   package, before it, after its own required dependencies. One that is
//!   installed is used as it is: Pane never replaces an installed copy while
//!   installing another package, so an installed copy acts as pinned, and
//!   one the user disabled, or Pane paused, stays so.
//! - An **optional** dependency is never installed with the package; it is
//!   listed, and used when the user installs it.
//! - A dependency is **compatible** when its package supports this system
//!   and publishes every operation the declaring package names, at the
//!   version it names. There is one copy per source, so two packages naming
//!   different versions of one operation of the same source conflict: Pane
//!   explains it rather than solving for several versions.
//! - **Cycles** are allowed: each source is visited once, so packages that
//!   require each other are installed together. At most
//!   [`MAX_INSTALLED_WITH`] packages are installed with the requested one.
//! - Anything that stops a required dependency from being installed stops
//!   the whole install before anything changes. A failure while installing
//!   undoes what this install added, so a failed install never leaves a
//!   package installed without its required dependencies; what could not
//!   be undone is reported as installed.
//!
//! A [`Plan`] is data: the required edges it found, the optional
//! dependencies, the problems, and the [`Assumptions`] it rests on, which
//! the launcher checks again before installing. Wording is only in the
//! `Display` implementations and [`Plan::lines`].

use std::fmt;
use std::path::{Path, PathBuf};

use crate::packages::{
    InstalledPackage, Manifest, ManifestDependency, PackageError, PackageIdentity, SourcePackage,
    Store, installed_as, paused_reason,
};
use crate::platform;

/// The most packages Pane installs along with the requested one.
pub const MAX_INSTALLED_WITH: usize = 16;

/// A package as a plan names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Named {
    pub identity: PackageIdentity,
    pub title: String,
}

/// What installing a package means for its dependencies.
pub(crate) struct Plan {
    pub requested: Named,
    /// The required packages to install before the requested one, each after
    /// its own required dependencies.
    pub install: Vec<SourcePackage>,
    /// Every required dependency found: which package requires which, in the
    /// order found. A target required by several packages appears once per
    /// package.
    pub required: Vec<Required>,
    /// The requested package's optional dependencies and those it does not
    /// need on this system.
    pub optional: Vec<Optional>,
    /// Why the package cannot be installed; empty when it can.
    pub problems: Vec<Problem>,
    /// What the plan takes for granted about the installed packages.
    pub assumptions: Assumptions,
    /// The requested package's manifest, for the sources it declares.
    requested_manifest: Manifest,
}

/// One package requiring another.
#[derive(Clone, Debug)]
pub(crate) struct Required {
    pub dependent: Named,
    /// The dependency's id in the dependent's manifest.
    pub id: String,
    pub target: Named,
    pub state: RequiredState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequiredState {
    /// It is not installed, and is installed with the package (or it is the
    /// requested package itself, in a cycle).
    Install,
    /// It is installed and runs.
    Installed,
    /// It is installed and the user disabled it; it stays disabled.
    Disabled,
    /// It is installed and Pane paused it after it failed; it stays paused
    /// until the user retries it.
    Paused,
}

/// A dependency that installing the package leaves alone.
#[derive(Clone, Debug)]
pub(crate) struct Optional {
    pub id: String,
    /// Its source as declared.
    pub source: String,
    pub state: OptionalState,
}

#[derive(Clone, Debug)]
pub(crate) enum OptionalState {
    /// An optional dependency that is not installed.
    NotInstalled,
    /// An optional dependency that is installed and enabled.
    Installed(Named),
    /// An optional dependency the user disabled.
    Disabled(Named),
    /// A dependency needed only on other systems ("only on Windows").
    NotNeededHere(String),
    /// An optional dependency whose source is not a folder Pane can name.
    Unresolvable(PathBuf),
}

/// Why a required dependency stops the install.
#[derive(Clone, Debug)]
pub(crate) struct Problem {
    /// The package declaring the dependency.
    pub dependent: Named,
    /// The dependency's id, or empty for a problem of the whole install.
    pub id: String,
    pub kind: ProblemKind,
}

#[derive(Clone, Debug)]
pub(crate) enum ProblemKind {
    /// Its source is not a folder Pane can name.
    Unresolvable(PathBuf),
    /// The package names its own folder.
    Itself,
    /// It is not installed and its folder cannot be installed.
    CannotInstall {
        folder: PathBuf,
        error: PackageError,
    },
    /// More than [`MAX_INSTALLED_WITH`] packages would be installed.
    TooMany,
    /// Its installed copy cannot be read.
    CannotLoad { target: Named, error: PackageError },
    /// Its installed copy does not support this system.
    PackageUnavailable { target: Named, reason: String },
    /// It does not publish an operation the dependent calls.
    NotPublished {
        target: Named,
        operation: String,
        installed: bool,
    },
    /// It publishes the operation at another version.
    OtherVersion {
        target: Named,
        operation: String,
        wanted: u32,
        published: u32,
        installed: bool,
    },
    /// It publishes the operation, but not for this system.
    OperationUnavailable {
        target: Named,
        operation: String,
        reason: String,
    },
    /// The dependent and `other` call different versions of one operation.
    Conflict {
        other: String,
        target: Named,
        operation: String,
        versions: (u32, u32),
    },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dependent = &self.dependent.title;
        let id = &self.id;
        // Updating the installed copy may help only when the dependency's
        // own folder might now publish what is called.
        let pinned = |f: &mut fmt::Formatter<'_>, installed: bool, target: &Named| {
            if installed {
                write!(
                    f,
                    "; Pane does not replace the installed copy of {} while installing another \
                     extension: update it from its folder if a newer copy publishes it",
                    target.title
                )
            } else {
                Ok(())
            }
        };
        match &self.kind {
            ProblemKind::Unresolvable(path) => write!(
                f,
                "{dependent} requires `{id}` from {}, which is not a folder Pane can name",
                path.display()
            ),
            ProblemKind::Itself => write!(f, "{dependent} names itself as its dependency `{id}`"),
            ProblemKind::CannotInstall { folder, error } => write!(
                f,
                "{dependent} requires `{id}` from {}, which cannot be installed: {error}",
                folder.display()
            ),
            ProblemKind::TooMany => write!(
                f,
                "Installing {dependent} would install more than {MAX_INSTALLED_WITH} other \
                 extensions with it; Pane installs at most {MAX_INSTALLED_WITH} at once"
            ),
            ProblemKind::CannotLoad { target, error } => write!(
                f,
                "{dependent} requires {}, whose installed copy cannot load: {error}",
                target.title
            ),
            ProblemKind::PackageUnavailable { target, reason } => {
                write!(f, "{dependent} requires {}: {reason}", target.title)
            }
            ProblemKind::NotPublished {
                target,
                operation,
                installed,
            } => {
                write!(
                    f,
                    "{dependent} requires {} to publish `{operation}`, which it does not publish",
                    target.title
                )?;
                pinned(f, *installed, target)
            }
            ProblemKind::OtherVersion {
                target,
                operation,
                wanted,
                published,
                installed,
            } => {
                write!(
                    f,
                    "{dependent} requires `{operation}` version {wanted} from {}, which \
                     publishes version {published}",
                    target.title
                )?;
                pinned(f, *installed, target)
            }
            ProblemKind::OperationUnavailable {
                target,
                operation,
                reason,
            } => write!(
                f,
                "{dependent} requires `{operation}` from {}: {reason}",
                target.title
            ),
            ProblemKind::Conflict {
                other,
                target,
                operation,
                versions: (mine, theirs),
            } => write!(
                f,
                "{dependent} and {other} need different versions of `{operation}` from {} \
                 ({mine} and {theirs}); Pane installs one copy of each source, so they conflict",
                target.title
            ),
        }
    }
}

/// What a plan takes for granted: the requested package's manifest, and
/// for every other package it relies on, that it is still not installed
/// (with the manifest read from its folder) or still installed in the same
/// managed copy and state. Two plans with equal assumptions install the
/// same packages the same way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Assumptions {
    pub requested: PackageIdentity,
    requested_manifest: String,
    pub packages: Vec<(PackageIdentity, Assumed)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Assumed {
    /// Not installed; installed from a folder holding this `pane.json`.
    Installs(String),
    /// Installed at this managed location, enabled or not, paused or not.
    Installed {
        location: PathBuf,
        enabled: bool,
        paused: bool,
    },
}

impl Assumptions {
    /// Whether the packages the plan relies on are still as it found them,
    /// among `installed`, of which those `paused` says are paused.
    pub fn hold(
        &self,
        installed: &[InstalledPackage],
        paused: impl Fn(&PackageIdentity) -> bool,
    ) -> bool {
        self.packages.iter().all(|(identity, assumed)| {
            match (assumed, installed_as(installed, identity)) {
                (Assumed::Installs(_), None) => true,
                (
                    Assumed::Installed {
                        location,
                        enabled,
                        paused: was_paused,
                    },
                    Some(package),
                ) => {
                    package.location == *location
                        && package.enabled == *enabled
                        && paused(identity) == *was_paused
                }
                _ => false,
            }
        })
    }
}

impl Plan {
    /// The preview's lines about the dependencies: each required target once
    /// (not the requested package itself), then the optional ones.
    pub fn lines(&self) -> Vec<String> {
        let requested = &self.requested;
        let mut shown: Vec<&PackageIdentity> = vec![&requested.identity];
        let mut lines = Vec::new();
        for required in &self.required {
            if shown.contains(&&required.target.identity) {
                continue;
            }
            shown.push(&required.target.identity);
            let needs = if required.dependent.identity == requested.identity {
                "Requires".to_owned()
            } else {
                format!("Requires (for {})", required.dependent.title)
            };
            let name = &required.target.title;
            let source = self.declared_source(required);
            lines.push(match required.state {
                RequiredState::Install => {
                    format!("{needs}: {name}, installed with it from {source}")
                }
                RequiredState::Installed => format!("{needs}: {name}, already installed"),
                RequiredState::Disabled => format!(
                    "{needs}: {name}, which you disabled: it stays disabled, and {} cannot use \
                     it until you enable it in Manage extensions",
                    requested.title
                ),
                RequiredState::Paused => {
                    format!("{needs}: {name}, installed but {}", paused_reason("it"))
                }
            });
        }
        for optional in &self.optional {
            let id = &optional.id;
            let source = &optional.source;
            lines.push(match &optional.state {
                OptionalState::NotInstalled => format!(
                    "Optional: `{id}` from {source}, not installed: Pane does not install it; \
                     install it yourself to use it"
                ),
                OptionalState::Installed(target) => {
                    format!("Optional: {}, installed", target.title)
                }
                OptionalState::Disabled(target) => format!(
                    "Optional: {}, installed but disabled: it stays disabled",
                    target.title
                ),
                OptionalState::NotNeededHere(only_on) => {
                    format!("Not needed on this system: `{id}` from {source} ({only_on})")
                }
                OptionalState::Unresolvable(path) => format!(
                    "Optional: `{id}` from {source}, which is not a folder Pane can name ({})",
                    path.display()
                ),
            });
        }
        lines
    }

    /// The source `required`'s dependent declares for it, as written
    /// (`local:../greeter`).
    fn declared_source(&self, required: &Required) -> String {
        let dependent = &required.dependent.identity;
        let manifest = self
            .install
            .iter()
            .find(|package| package.identity == *dependent)
            .map_or(&self.requested_manifest, |package| &package.manifest);
        manifest.dependency(&required.id).map_or_else(
            || required.target.identity.to_string(),
            |dependency| dependency.source.clone(),
        )
    }

    /// The titles of the packages installed with the requested one.
    pub fn installed_with(&self) -> Vec<String> {
        self.install
            .iter()
            .map(|package| package.manifest.title.clone())
            .collect()
    }

    /// The titles of required dependencies found in `state`.
    pub fn titles_in(&self, state: RequiredState) -> Vec<String> {
        let mut titles: Vec<String> = Vec::new();
        for required in self.required.iter().filter(|r| r.state == state) {
            if !titles.contains(&required.target.title) {
                titles.push(required.target.title.clone());
            }
        }
        titles
    }
}

/// Works out what installing `requested` means for its dependencies, given
/// the `installed` packages, those of them Pane `paused`, and `read`, which
/// reads and validates the package in a folder. Changes nothing.
pub(crate) fn plan(
    requested: &SourcePackage,
    installed: &[InstalledPackage],
    paused: &[PackageIdentity],
    read: impl FnMut(&Path) -> Result<SourcePackage, PackageError>,
) -> Plan {
    let named = Named {
        identity: requested.identity.clone(),
        title: requested.manifest.title.clone(),
    };
    let mut planner = Planner {
        installed,
        paused,
        read,
        seen: vec![requested.identity.clone()],
        demands: Vec::new(),
        plan: Plan {
            requested: named.clone(),
            install: Vec::new(),
            required: Vec::new(),
            optional: Vec::new(),
            problems: Vec::new(),
            assumptions: Assumptions {
                requested: requested.identity.clone(),
                requested_manifest: requested.manifest_text().to_owned(),
                packages: Vec::new(),
            },
            requested_manifest: requested.manifest.clone(),
        },
        too_many: false,
    };
    planner.visit(requested);
    planner.check_demands(requested);
    planner.plan
}

/// One package's need of the operations of another.
struct Demand {
    dependent: Named,
    target: PackageIdentity,
    dependency: ManifestDependency,
}

struct Planner<'a, R> {
    installed: &'a [InstalledPackage],
    paused: &'a [PackageIdentity],
    read: R,
    /// The requested package and every required dependency visited.
    seen: Vec<PackageIdentity>,
    demands: Vec<Demand>,
    plan: Plan,
    too_many: bool,
}

impl<R: FnMut(&Path) -> Result<SourcePackage, PackageError>> Planner<'_, R> {
    fn visit(&mut self, package: &SourcePackage) {
        let dependent = Named {
            identity: package.identity.clone(),
            title: package.manifest.title.clone(),
        };
        let is_requested = package.identity == self.plan.requested.identity;
        for dependency in &package.manifest.dependencies {
            let id = dependency.id.clone();
            let problem = |kind| Problem {
                dependent: dependent.clone(),
                id: id.clone(),
                kind,
            };
            if let Some(only_on) = dependency.only_on() {
                if is_requested {
                    self.plan.optional.push(Optional {
                        id: id.clone(),
                        source: dependency.source.clone(),
                        state: OptionalState::NotNeededHere(only_on),
                    });
                }
                continue;
            }
            let target = package.identity.dependency(&dependency.source);
            if !dependency.required {
                if is_requested {
                    let state = self.optional_state(target);
                    self.plan.optional.push(Optional {
                        id: id.clone(),
                        source: dependency.source.clone(),
                        state,
                    });
                }
                continue;
            }
            let target = match target {
                Ok(target) => target,
                Err(path) => {
                    self.plan
                        .problems
                        .push(problem(ProblemKind::Unresolvable(path)));
                    continue;
                }
            };
            if target == package.identity {
                self.plan.problems.push(problem(ProblemKind::Itself));
                continue;
            }
            self.demands.push(Demand {
                dependent: dependent.clone(),
                target: target.clone(),
                dependency: dependency.clone(),
            });
            let edge = |target: Named, state| Required {
                dependent: dependent.clone(),
                id: id.clone(),
                target,
                state,
            };
            if self.seen.contains(&target) {
                // Found before, or the requested package in a cycle.
                let found = self
                    .plan
                    .required
                    .iter()
                    .find(|required| required.target.identity == target)
                    .map(|required| (required.target.clone(), required.state));
                let (named, state) =
                    found.unwrap_or_else(|| (self.plan.requested.clone(), RequiredState::Install));
                self.plan.required.push(edge(named, state));
                continue;
            }
            self.seen.push(target.clone());
            if let Some(installed) = installed_as(self.installed, &target) {
                let paused = self.paused.contains(&target);
                let state = match (installed.enabled, paused) {
                    (false, _) => RequiredState::Disabled,
                    (true, true) => RequiredState::Paused,
                    (true, false) => RequiredState::Installed,
                };
                self.plan.assumptions.packages.push((
                    target.clone(),
                    Assumed::Installed {
                        location: installed.location.clone(),
                        enabled: installed.enabled,
                        paused,
                    },
                ));
                let named = Named {
                    identity: target,
                    title: installed.title(),
                };
                self.plan.required.push(edge(named, state));
                continue;
            }
            if self.plan.install.len() + self.visiting() >= MAX_INSTALLED_WITH {
                if !self.too_many {
                    self.too_many = true;
                    self.plan.problems.push(Problem {
                        dependent: self.plan.requested.clone(),
                        id: String::new(),
                        kind: ProblemKind::TooMany,
                    });
                }
                continue;
            }
            let folder = target
                .local_folder()
                .expect("a local dependency has a folder")
                .to_path_buf();
            match (self.read)(&folder) {
                Ok(source) => {
                    let named = Named {
                        identity: target.clone(),
                        title: source.manifest.title.clone(),
                    };
                    self.plan.required.push(edge(named, RequiredState::Install));
                    self.plan
                        .assumptions
                        .packages
                        .push((target, Assumed::Installs(source.manifest_text().to_owned())));
                    self.visit(&source);
                    self.plan.install.push(source);
                }
                Err(error) => self
                    .plan
                    .problems
                    .push(problem(ProblemKind::CannotInstall { folder, error })),
            }
        }
    }

    /// How many packages to install are still being visited: read, but not
    /// yet added to the plan's install list.
    fn visiting(&self) -> usize {
        let installs = self
            .plan
            .assumptions
            .packages
            .iter()
            .filter(|(_, assumed)| matches!(assumed, Assumed::Installs(_)))
            .count();
        installs - self.plan.install.len()
    }

    fn optional_state(&self, target: Result<PackageIdentity, PathBuf>) -> OptionalState {
        let target = match target {
            Ok(target) => target,
            Err(path) => return OptionalState::Unresolvable(path),
        };
        match installed_as(self.installed, &target) {
            Some(installed) => {
                let named = Named {
                    identity: target,
                    title: installed.title(),
                };
                if installed.enabled {
                    OptionalState::Installed(named)
                } else {
                    OptionalState::Disabled(named)
                }
            }
            None => OptionalState::NotInstalled,
        }
    }

    /// Checks that every required dependency found publishes what its
    /// dependents call, with one version of each operation.
    fn check_demands(&mut self, requested: &SourcePackage) {
        let mut targets: Vec<&PackageIdentity> = Vec::new();
        for demand in &self.demands {
            if !targets.contains(&&demand.target) {
                targets.push(&demand.target);
            }
        }
        let mut problems = Vec::new();
        for target in targets {
            let demands: Vec<&Demand> = self
                .demands
                .iter()
                .filter(|demand| demand.target == *target)
                .collect();
            let installed = installed_as(self.installed, target);
            let (title, manifest) = if *target == requested.identity {
                (requested.manifest.title.clone(), Ok(&requested.manifest))
            } else if let Some(source) = self.plan.install.iter().find(|p| p.identity == *target) {
                (source.manifest.title.clone(), Ok(&source.manifest))
            } else if let Some(installed) = installed {
                (installed.title(), installed.manifest.as_ref())
            } else {
                // It could not be read, which is explained already.
                continue;
            };
            let named = Named {
                identity: target.clone(),
                title,
            };
            let is_installed = installed.is_some() && *target != requested.identity;
            let problem = |demand: &Demand, kind| Problem {
                dependent: demand.dependent.clone(),
                id: demand.dependency.id.clone(),
                kind,
            };
            let manifest = match manifest {
                Ok(manifest) => manifest,
                Err(error) => {
                    for demand in demands {
                        problems.push(problem(
                            demand,
                            ProblemKind::CannotLoad {
                                target: named.clone(),
                                error: error.clone(),
                            },
                        ));
                    }
                    continue;
                }
            };
            if is_installed
                && let Some(reason) =
                    platform::unavailable(manifest.platforms.as_deref(), "this package")
            {
                problems.push(problem(
                    demands[0],
                    ProblemKind::PackageUnavailable {
                        target: named,
                        reason,
                    },
                ));
                continue;
            }
            for (demand, kind) in unmet(&named, manifest, &demands, is_installed) {
                problems.push(problem(demand, kind));
            }
        }
        self.plan.problems.extend(problems);
    }
}

/// Why `manifest` of `target` does not serve `demands`: operations it does
/// not publish, at another version or not on this system, and two
/// dependents naming different versions of one operation.
fn unmet<'d>(
    target: &Named,
    manifest: &Manifest,
    demands: &[&'d Demand],
    installed: bool,
) -> Vec<(&'d Demand, ProblemKind)> {
    let mut problems = Vec::new();
    let mut checked: Vec<&str> = Vec::new();
    for demand in demands {
        for wanted in &demand.dependency.operations {
            if checked.contains(&wanted.id.as_str()) {
                continue;
            }
            checked.push(&wanted.id);
            let other = demands.iter().find_map(|other| {
                other
                    .dependency
                    .operations
                    .iter()
                    .find(|o| o.id == wanted.id && o.version != wanted.version)
                    .map(|o| (other.dependent.title.clone(), o.version))
            });
            let operation = wanted.id.clone();
            if let Some((other, version)) = other {
                problems.push((
                    *demand,
                    ProblemKind::Conflict {
                        other,
                        target: target.clone(),
                        operation,
                        versions: (wanted.version, version),
                    },
                ));
                continue;
            }
            match manifest.operations.iter().find(|o| o.id == wanted.id) {
                None => problems.push((
                    *demand,
                    ProblemKind::NotPublished {
                        target: target.clone(),
                        operation,
                        installed,
                    },
                )),
                Some(published) if published.version != wanted.version => problems.push((
                    *demand,
                    ProblemKind::OtherVersion {
                        target: target.clone(),
                        operation,
                        wanted: wanted.version,
                        published: published.version,
                        installed,
                    },
                )),
                Some(published) => {
                    if let Some(reason) =
                        platform::unavailable(published.platforms.as_deref(), "this operation")
                    {
                        problems.push((
                            *demand,
                            ProblemKind::OperationUnavailable {
                                target: target.clone(),
                                operation,
                                reason,
                            },
                        ));
                    }
                }
            }
        }
    }
    problems
}

/// An installed package that requires another, directly or through other
/// installed packages that do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dependent {
    pub package: Named,
    /// The package it requires that brought it into the set: the package
    /// asked about, or another dependent.
    pub requires: Named,
    /// Whether the user left it enabled.
    pub enabled: bool,
}

/// The installed package a dependency recorded as `recorded` reaches among
/// `installed`: the one with that identity, or else the one its folder
/// resolves to now (a folder that became a link after it was recorded), as
/// a call through the dependency's id finds it.
pub(crate) fn reached<'a>(
    installed: &'a [InstalledPackage],
    recorded: &PackageIdentity,
) -> Option<&'a InstalledPackage> {
    installed_as(installed, recorded).or_else(|| {
        let again = recorded.resolved_again()?;
        installed_as(installed, &again)
    })
}

/// Every installed package that requires `of` on this system, directly or
/// through another installed package that does: the **required dependent
/// closure**, each package once, nearest first and otherwise in installed
/// order. Only required dependencies needed on this system count: an
/// optional dependency, or one declared only for other systems, never
/// brings a package in. Cycles are allowed: a package requiring `of` back is
/// listed once, and `of` itself never is. Disabled dependents are listed
/// (with `enabled` false) and followed, since what requires them requires
/// `of` too. Reads only the installed records; runs nothing.
pub(crate) fn required_dependents(
    installed: &[InstalledPackage],
    of: &PackageIdentity,
) -> Vec<Dependent> {
    let named = |package: &InstalledPackage| Named {
        identity: package.identity.clone(),
        title: package.title(),
    };
    let Some(first) = installed_as(installed, of) else {
        return Vec::new();
    };
    let mut seen = vec![of.clone()];
    let mut queue = std::collections::VecDeque::from([named(first)]);
    let mut dependents = Vec::new();
    while let Some(required) = queue.pop_front() {
        for package in installed {
            if seen.contains(&package.identity) || !requires(installed, package, &required.identity)
            {
                continue;
            }
            seen.push(package.identity.clone());
            let dependent = Dependent {
                package: named(package),
                requires: required.clone(),
                enabled: package.enabled,
            };
            queue.push_back(dependent.package.clone());
            dependents.push(dependent);
        }
    }
    dependents
}

/// Whether `package` requires the installed package `target` on this
/// system, as its dependencies were recorded when it was installed.
fn requires(
    installed: &[InstalledPackage],
    package: &InstalledPackage,
    target: &PackageIdentity,
) -> bool {
    let Ok(manifest) = &package.manifest else {
        return false;
    };
    manifest
        .dependencies
        .iter()
        .filter(|dependency| dependency.required && dependency.needed_here())
        .filter_map(|dependency| package.dependency_identity(&dependency.id))
        .any(|recorded| reached(installed, recorded).is_some_and(|p| p.identity == *target))
}

/// Installing a package with its planned dependencies failed.
pub(crate) struct Failure {
    /// Why, naming the package that could not be installed.
    pub error: String,
    /// Packages this install added that could not be removed again, each
    /// with why: they stay installed.
    pub left_installed: Vec<(InstalledPackage, String)>,
}

/// Installs `dependencies` in order, then the requested package with
/// `install_requested`. If any fails, removes again those this call
/// installed, most recent first, putting back the record of data Pane kept
/// for one at its place, so that the store is as before; returns the
/// installed packages, dependencies first, or the failure.
pub(crate) fn install_all(
    store: &mut Store,
    dependencies: &[SourcePackage],
    install_requested: impl FnOnce(&mut Store) -> Result<InstalledPackage, PackageError>,
) -> Result<(Vec<InstalledPackage>, InstalledPackage), Failure> {
    let mut added: Vec<(InstalledPackage, Option<(String, usize)>)> = Vec::new();
    for dependency in dependencies {
        let retained = store
            .retained()
            .into_iter()
            .enumerate()
            .find(|(_, retained)| retained.identity == dependency.identity)
            .map(|(at, retained)| (retained.title, at));
        match store.install(dependency) {
            Ok(installed) => added.push((installed, retained)),
            Err(error) => {
                let error = format!(
                    "Could not install {}, which it requires: {error}",
                    dependency.manifest.title
                );
                return Err(undo(store, added, error));
            }
        }
    }
    match install_requested(store) {
        Ok(installed) => Ok((added.into_iter().map(|(p, _)| p).collect(), installed)),
        Err(error) => Err(undo(store, added, error.to_string())),
    }
}

fn undo(
    store: &mut Store,
    added: Vec<(InstalledPackage, Option<(String, usize)>)>,
    error: String,
) -> Failure {
    let mut left_installed = Vec::new();
    for (package, retained) in added.into_iter().rev() {
        if let Err(problem) = store.undo_install(&package.identity, retained) {
            left_installed.push((package, problem.to_string()));
        }
    }
    Failure {
        error,
        left_installed,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// A package folder `name` in `sources`, with a component file that is
    /// never run: the store copies files, it does not check them.
    fn package(sources: &Path, name: &str) -> SourcePackage {
        let folder = sources.join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("c.wasm"), b"not run").unwrap();
        fs::write(
            folder.join("pane.json"),
            format!(
                r#"{{ "manifestVersion": 1, "title": "Package {name}", "apiVersion": "0.1",
                     "operations": [{{ "id": "echo", "version": 1, "component": "c.wasm" }}] }}"#
            ),
        )
        .unwrap();
        SourcePackage::read(&folder).unwrap()
    }

    fn titles(store: &Store) -> Vec<String> {
        store
            .installed()
            .iter()
            .map(InstalledPackage::title)
            .collect()
    }

    fn retained(store: &Store) -> Vec<String> {
        store.retained().into_iter().map(|r| r.title).collect()
    }

    fn setup() -> (tempfile::TempDir, PathBuf, Store) {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let store = Store::open(data.clone());
        (dir, data, store)
    }

    #[test]
    fn a_failure_of_the_requested_package_removes_its_dependencies_again() {
        let (dir, data, mut store) = setup();
        let b = package(dir.path(), "b");
        let c = package(dir.path(), "c");
        // Pane kept the data of x, b and y from earlier installations.
        let x = package(dir.path(), "x");
        let y = package(dir.path(), "y");
        store.retain(&x.identity, "Old x".into()).unwrap();
        store.retain(&b.identity, "Old b".into()).unwrap();
        store.retain(&y.identity, "Old y".into()).unwrap();

        let failure = install_all(&mut store, &[b.clone(), c], |_| {
            Err(PackageError::Storage("the disk is full".into()))
        })
        .expect_err("the install fails");

        assert_eq!(
            failure.error,
            "Could not update Pane's installed extensions: the disk is full"
        );
        assert!(failure.left_installed.is_empty());
        assert!(titles(&store).is_empty());
        // b's kept data is on record again, at its place.
        assert_eq!(retained(&store), ["Old x", "Old b", "Old y"]);
        // So it is after a restart, and the managed copies are gone.
        let reopened = Store::open(data.clone());
        assert!(titles(&reopened).is_empty());
        assert_eq!(retained(&reopened), ["Old x", "Old b", "Old y"]);
        assert_eq!(fs::read_dir(data.join("packages")).unwrap().count(), 0);
    }

    #[test]
    fn a_failure_of_a_dependency_removes_those_installed_before_it() {
        let (dir, _data, mut store) = setup();
        let b = package(dir.path(), "b");
        let c = package(dir.path(), "c");
        // c got installed meanwhile, so installing it again fails.
        store.install(&c).unwrap();

        let failure =
            install_all(&mut store, &[b, c], |_| unreachable!()).expect_err("the install fails");

        assert!(
            failure
                .error
                .starts_with("Could not install Package c, which it requires: Already installed"),
            "{}",
            failure.error
        );
        assert_eq!(titles(&store), ["Package c"]);
    }

    /// Installs package folder `name` in `sources`, titled "Package <name>",
    /// declaring `dependencies` (JSON array contents).
    fn installed_with(
        store: &mut Store,
        sources: &Path,
        name: &str,
        dependencies: &str,
    ) -> PackageIdentity {
        let folder = sources.join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("c.wasm"), b"not run").unwrap();
        fs::write(
            folder.join("pane.json"),
            format!(
                r#"{{ "manifestVersion": 1, "title": "Package {name}", "apiVersion": "0.1",
                     "operations": [{{ "id": "echo", "version": 1, "component": "c.wasm" }}],
                     "dependencies": [{dependencies}] }}"#
            ),
        )
        .unwrap();
        store
            .install(&SourcePackage::read(&folder).unwrap())
            .unwrap()
            .identity
    }

    /// A dependency on sibling folder `folder`; `extra` adds fields.
    fn on(folder: &str, extra: &str) -> String {
        format!(
            r#"{{ "id": "{folder}", "source": "local:../{folder}", {extra}
                 "operations": [{{ "id": "echo", "version": 1 }}] }}"#
        )
    }

    fn dependent_titles(store: &Store, of: &PackageIdentity) -> Vec<(String, String)> {
        required_dependents(&store.installed(), of)
            .into_iter()
            .map(|d| (d.package.title, d.requires.title))
            .collect()
    }

    #[test]
    fn required_dependents_are_found_directly_and_through_others_nearest_first() {
        let (dir, _data, mut store) = setup();
        let sources = dir.path();
        let a = installed_with(&mut store, sources, "a", "");
        // c requires b, which is installed after it; b and d require a.
        installed_with(&mut store, sources, "c", &on("b", ""));
        installed_with(&mut store, sources, "b", &on("a", ""));
        installed_with(&mut store, sources, "d", &on("a", ""));
        // e only uses a if it is installed.
        installed_with(&mut store, sources, "e", &on("a", r#""optional": true,"#));

        assert_eq!(
            dependent_titles(&store, &a),
            [
                ("Package b".into(), "Package a".into()),
                ("Package d".into(), "Package a".into()),
                ("Package c".into(), "Package b".into()),
            ]
        );
    }

    #[test]
    fn packages_requiring_each_other_are_listed_once_without_the_package_asked_about() {
        let (dir, _data, mut store) = setup();
        let sources = dir.path();
        let a = installed_with(&mut store, sources, "a", &on("b", ""));
        installed_with(&mut store, sources, "b", &on("a", ""));
        installed_with(&mut store, sources, "c", &on("b", ""));

        assert_eq!(
            dependent_titles(&store, &a),
            [
                ("Package b".into(), "Package a".into()),
                ("Package c".into(), "Package b".into()),
            ]
        );
    }

    #[test]
    fn a_dependency_needed_only_on_other_systems_brings_no_dependent() {
        let (dir, _data, mut store) = setup();
        let sources = dir.path();
        let a = installed_with(&mut store, sources, "a", "");
        let others: Vec<String> = platform::Platform::ALL
            .into_iter()
            .filter(|p| Some(*p) != platform::Platform::current())
            .map(|p| format!("\"{}\"", p.id()))
            .collect();
        let only_elsewhere = format!(r#""platforms": [{}],"#, others.join(", "));
        installed_with(&mut store, sources, "b", &on("a", &only_elsewhere));

        assert!(dependent_titles(&store, &a).is_empty());
    }

    #[test]
    fn disabled_dependents_are_listed_and_followed() {
        let (dir, _data, mut store) = setup();
        let sources = dir.path();
        let a = installed_with(&mut store, sources, "a", "");
        let b = installed_with(&mut store, sources, "b", &on("a", ""));
        installed_with(&mut store, sources, "c", &on("b", ""));
        store.set_enabled_all(&[b], false).unwrap();

        let dependents = required_dependents(&store.installed(), &a);
        let states: Vec<(String, bool)> = dependents
            .into_iter()
            .map(|d| (d.package.title, d.enabled))
            .collect();
        assert_eq!(
            states,
            [("Package b".into(), false), ("Package c".into(), true)]
        );
    }

    #[test]
    fn a_package_not_installed_has_no_dependents() {
        let (dir, _data, mut store) = setup();
        let sources = dir.path();
        installed_with(&mut store, sources, "b", &on("a", ""));
        // Folder a does not exist: b's record spells it.
        let a = store.installed()[0]
            .dependency_identity("a")
            .unwrap()
            .clone();

        assert!(dependent_titles(&store, &a).is_empty());
    }

    #[test]
    fn a_success_installs_dependencies_first() {
        let (dir, _data, mut store) = setup();
        let a = package(dir.path(), "a");
        let b = package(dir.path(), "b");

        let (dependencies, installed) = install_all(&mut store, &[b], |store| store.install(&a))
            .ok()
            .unwrap();

        assert_eq!(dependencies.len(), 1);
        assert_eq!(installed.title(), "Package a");
        assert_eq!(titles(&store), ["Package b", "Package a"]);
    }
}
