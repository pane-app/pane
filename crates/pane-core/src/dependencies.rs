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
//! - A dependency on an npm package may **pin** an exact npm version
//!   (`npm:greeter@1.2.3`). The one copy must have it: an installed copy of
//!   another version, another dependent pinning another version, or the
//!   latest version taken for a dependent that pins none is a conflict Pane
//!   explains, rather than installing a version the dependent did not ask
//!   for.
//! - **Cycles** are allowed: each source is visited once, so packages that
//!   require each other are installed together. At most
//!   [`MAX_INSTALLED_WITH`] packages are installed with the requested one.
//! - Anything that stops a required dependency from being installed stops
//!   the whole install before anything changes. A failure while installing
//!   undoes what this install added, so a failed install never leaves a
//!   package installed without its required dependencies; what could not
//!   be undone is reported as installed.
//!
//! A package that **uses capabilities** (ADR 0041) has them planned too
//! (see [`Use`]): who provides each — a package installed already, whatever
//! state it is in, or one this install installs — or, with no provider and
//! no default, the package waits for one, which never stops an install. A
//! required use's `default` provider is installed with the package as a
//! missing required dependency is: planned, claimed, installed before the
//! package that names it, and rolled back with the rest. An optional use
//! is listed, and its default is never installed.
//!
//! A [`Plan`] is data: the required edges it found, the optional
//! dependencies, the capability uses, the problems, and the [`Assumptions`]
//! it rests on, which the launcher checks again before installing. Wording
//! is only in the `Display` implementations and [`Plan::lines`].

use std::collections::VecDeque;
use std::fmt;
use std::path::PathBuf;

use crate::packages::{
    InstalledPackage, Manifest, ManifestDependency, ManifestProvides, ManifestUse, PackageError,
    PackageIdentity, SourcePackage, SourceSpec, Store, installed_as, paused_reason,
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
    /// The capabilities the requested package and the default providers
    /// installed with it use, in the order their manifests declare them
    /// (see `Use`).
    pub uses: Vec<Use>,
    /// The capabilities the requested package provides that other packages
    /// provide too (see `Provided`).
    pub provides: Vec<Provided>,
    /// The default providers installed with the requested one, each named
    /// by the use it answers (see `DefaultProvider`).
    pub defaults: Vec<DefaultProvider>,
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

/// A capability a package the plan installs uses, as the preview lists it
/// (see [`Plan::lines`]). A required use's default provider is installed
/// with the package when no package provides the capability; an optional
/// use's never is; with no provider and no default, the package waits for
/// one.
#[derive(Clone, Debug)]
pub(crate) struct Use {
    /// The package whose `pane.json` declares the use.
    pub consumer: Named,
    /// The capability's name, such as `acme:translate@1`.
    pub capability: String,
    /// Whether the package needs the capability or only calls it when some
    /// provider is installed.
    pub required: bool,
    /// Whether only some of the package's commands need it, rather than the
    /// whole package.
    pub some_commands: bool,
    /// The default provider its `pane.json` names, as written, if any.
    pub default: Option<String>,
    pub kind: UseKind,
}

/// What the plan does for one use.
#[derive(Clone, Debug)]
pub(crate) enum UseKind {
    /// A package provides it, in the state the plan found that package in.
    Provided {
        provider: Named,
        state: ProvidedState,
    },
    /// No package provides it; the default the consumer names is installed
    /// with this one, as a missing required dependency is.
    InstallsDefault { provider: Named },
    /// No package provides it, and none is installed with this one.
    NoProvider,
}

/// The state a provider the plan found is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProvidedState {
    /// Installed already, and enabled.
    Installed,
    /// Installed already, and the user disabled it: it stays disabled.
    Disabled,
    /// Installed already, and Pane paused it after it failed.
    Paused,
    /// Installed with this one, as a required dependency or a default
    /// provider.
    WithThis,
}

/// A capability the requested package provides that another package
/// provides too, as the preview lists it: a choice the user has in
/// Settings.
#[derive(Clone, Debug)]
pub(crate) struct Provided {
    /// The capability's name.
    pub capability: String,
    /// The other packages providing it, in install order: installed
    /// already, or installed with this one.
    pub others: Vec<(Named, ProvidedState)>,
}

/// A default provider a use names, planned to be installed with the
/// package: as a missing required dependency is, it is claimed, installed
/// before the package that names it, and rolled back with the rest.
#[derive(Clone, Debug)]
pub(crate) struct DefaultProvider {
    /// The package whose use names it.
    pub consumer: Named,
    /// The capability the use names it for.
    pub capability: String,
    /// Its source as the consumer wrote it.
    pub source: String,
    /// The package itself.
    pub target: Named,
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
    /// It is not installed and its source cannot be installed: `from` names
    /// that source (a folder, "npm package <name>" or "Git repository
    /// <repository>").
    CannotInstall { from: String, error: PackageError },
    /// The dependent comes from npm or Git (`from`) and names a local
    /// folder, which is on its author's computer, not the user's.
    LocalFromPublished { from: &'static str, source: String },
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
    /// It is a Git package the dependent names at reference `wanted`, and
    /// the plan's copy is another revision, `have` (its reference and
    /// commit): the installed one (`by` is `None`), or the one fetched for
    /// the dependent named by `by`.
    GitRevision {
        target: Named,
        wanted: String,
        have: String,
        by: Option<String>,
    },
    /// It is an npm package the dependent pins to npm version `wanted`, and
    /// the plan's copy has version `have`: the installed one (`by` is
    /// `None`), or the one read for the dependent named by `by`, which
    /// pinned it too or took its latest.
    NpmVersion {
        target: Named,
        wanted: String,
        have: String,
        by: Option<(String, bool)>,
    },
    /// The dependent and `other` call different versions of one operation.
    Conflict {
        other: String,
        target: Named,
        operation: String,
        versions: (u32, u32),
    },
    /// The default provider a use of a capability names is not a source
    /// Pane can name (`path` is what it resolved to, if anything).
    DefaultUnresolvable { source: String, path: PathBuf },
    /// The default provider a use names is the package naming it.
    DefaultItself,
    /// The default provider a use names cannot be read or installed:
    /// `from` names that source, as [`ProblemKind::CannotInstall`] does.
    DefaultCannotInstall {
        source: String,
        from: String,
        error: PackageError,
    },
    /// The default provider a use names does not provide the capability it
    /// is named for (not at all, not on this system, or without an
    /// operation the use calls): `why` says which. `found` says where the
    /// plan found it, for the advice it gives.
    DefaultDoesNotProvide {
        source: String,
        target: Named,
        found: DefaultFound,
        why: String,
    },
}

/// Where the plan found a default provider that does not provide its
/// capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DefaultFound {
    /// Read from the source the use names.
    Source,
    /// Already installed with this one, as a required dependency.
    WithThis,
    /// Installed already, and Pane does not replace it while installing
    /// another package.
    Installed,
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
            ProblemKind::CannotInstall { from, error } => write!(
                f,
                "{dependent} requires `{id}` from {from}, which cannot be installed: {error}"
            ),
            ProblemKind::LocalFromPublished { from, source } => write!(
                f,
                "{dependent} comes from {from} but names the local folder `{source}` as its \
                 dependency `{id}`; a package published to npm or Git can depend only on \
                 packages from npm or Git"
            ),
            ProblemKind::GitRevision {
                target,
                wanted,
                have,
                by,
            } => {
                let title = &target.title;
                write!(f, "{dependent} requires {title} at {wanted}, and ")?;
                match by {
                    None => write!(
                        f,
                        "{have} is installed; Pane does not replace the installed copy while \
                         installing another extension: update it to {wanted} (Git repository \
                         {}@{wanted}) if {dependent} needs that revision",
                        target.identity.git_repository().unwrap_or(title)
                    ),
                    Some(other) => write!(
                        f,
                        "{other} requires {have}; Pane installs one copy of each package, so \
                         they cannot both have theirs"
                    ),
                }
            }
            ProblemKind::NpmVersion {
                target,
                wanted,
                have,
                by,
            } => {
                let title = &target.title;
                write!(
                    f,
                    "{dependent} requires {title} at npm version {wanted}, and "
                )?;
                match by {
                    None => write!(
                        f,
                        "version {have} is installed; Pane does not replace the installed copy \
                         while installing another extension: update it to {wanted} (npm \
                         package {}@{wanted}) if {dependent} needs that version",
                        target.identity.npm_name().unwrap_or(title)
                    ),
                    Some((other, pinned)) => write!(
                        f,
                        "{other} {} {have}; Pane installs one copy of each package, so they \
                         cannot both have theirs",
                        if *pinned {
                            "requires version"
                        } else {
                            "takes its latest, version"
                        }
                    ),
                }
            }
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
            ProblemKind::DefaultUnresolvable { source, path } => write!(
                f,
                "{dependent} names `{source}` as the default provider of `{id}`, which is not \
                 a folder Pane can name ({})",
                path.display()
            ),
            ProblemKind::DefaultItself => write!(
                f,
                "{dependent} names itself as the default provider of `{id}`"
            ),
            ProblemKind::DefaultCannotInstall {
                source,
                from,
                error,
            } => write!(
                f,
                "{dependent} names `{source}` as the default provider of `{id}`, from {from}, \
                 which cannot be installed: {error}"
            ),
            ProblemKind::DefaultDoesNotProvide {
                source,
                target,
                found,
                why,
            } => {
                let title = &target.title;
                match found {
                    DefaultFound::Source => write!(
                        f,
                        "{dependent} names `{source}` as the default provider of `{id}`, which \
                         {why}"
                    ),
                    DefaultFound::WithThis => write!(
                        f,
                        "{dependent} names `{source}` as the default provider of `{id}`, which \
                         is installed with it and {why}"
                    ),
                    DefaultFound::Installed => write!(
                        f,
                        "{dependent} names `{source}` as the default provider of `{id}`, and \
                         {title} is installed already and {why}; Pane does not replace the \
                         installed copy of {title} while installing another extension: update \
                         it from its folder if a newer copy provides it"
                    ),
                }
            }
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
    /// The integrity of the requested package's npm tarball, or its Git
    /// commit, if it was downloaded: a new download with the same
    /// `pane.json` is another plan.
    requested_tarball: Option<String>,
    pub packages: Vec<(PackageIdentity, Assumed)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Assumed {
    /// Not installed; installed from a source holding this `pane.json`, and,
    /// from npm, the tarball with this integrity, or from Git, this commit.
    Installs(String, Option<String>),
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
                (Assumed::Installs(..), None) => true,
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

/// What one use says about the provider the plan found: its title with the
/// state the plan found it in, for [`Plan::lines`].
fn provided_line(provider: &Named, state: ProvidedState, consumer: &str, required: bool) -> String {
    let provider = &provider.title;
    match state {
        ProvidedState::Installed => format!("provided by {provider} (installed)"),
        ProvidedState::Disabled if required => format!(
            "provided by {provider} (installed, but you disabled it: it stays disabled, \
             and {consumer} waits until you enable it in Settings)"
        ),
        ProvidedState::Disabled => {
            format!(
                "provided by {provider} (installed, but you disabled it: it stays \
                     disabled)"
            )
        }
        ProvidedState::Paused => {
            format!(
                "provided by {provider} (installed, but {})",
                paused_reason("it")
            )
        }
        ProvidedState::WithThis => format!("provided by {provider} (installed with it)"),
    }
}

impl Plan {
    /// The preview's lines about the dependencies: each required target once
    /// (not the requested package itself), followed by a caution for one
    /// from Git whose commit no branch or tag points to, then the optional
    /// ones, then the capabilities the packages installed use and the ones
    /// the requested package provides that others provide too.
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
                     it until you enable it in Settings",
                    requested.title
                ),
                RequiredState::Paused => {
                    format!("{needs}: {name}, installed but {}", paused_reason("it"))
                }
            });
            // A Git dependency named by a commit no branch or tag points to
            // is cautioned about as the requested package would be.
            let caution = self
                .install
                .iter()
                .find(|package| package.identity == required.target.identity)
                .and_then(|package| package.git.as_ref())
                .and_then(crate::git::GitOrigin::caution);
            if let Some(caution) = caution {
                lines.push(format!("Caution ({name}): {caution}"));
            }
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
        for used in &self.uses {
            lines.push(self.use_line(used));
        }
        for provided in &self.provides {
            let others: Vec<String> = provided
                .others
                .iter()
                .map(|(named, state)| match state {
                    ProvidedState::WithThis => format!("{}, installed with it", named.title),
                    _ => named.title.clone(),
                })
                .collect();
            let verb = if others.len() == 1 {
                "provides"
            } else {
                "provide"
            };
            lines.push(format!(
                "Provides {} ({} {verb} it too; choose in Settings)",
                provided.capability,
                platform::join(&others)
            ));
        }
        lines
    }

    /// The preview's line about one use: who provides it, or the default
    /// Pane installs, or that the package waits for a provider. An optional
    /// use is listed and never has anything installed for it.
    fn use_line(&self, used: &Use) -> String {
        let capability = &used.capability;
        let consumer = &used.consumer.title;
        let needs = if used.consumer.identity == self.requested.identity {
            String::new()
        } else {
            format!(" (for {})", consumer)
        };
        if !used.required {
            // An optional use gates nothing, and its default is never
            // installed.
            return match (&used.kind, &used.default) {
                (UseKind::Provided { provider, state }, _) => format!(
                    "Optional{needs}: {capability}, {}",
                    provided_line(provider, *state, consumer, used.required)
                ),
                (_, Some(default)) => format!(
                    "Optional{needs}: {capability}, no installed extension provides it; Pane \
                     does not install `{default}`, which {consumer} names; install it yourself \
                     to use it"
                ),
                (_, None) => format!(
                    "Optional{needs}: {capability}, no installed extension provides it; \
                     install one to use it"
                ),
            };
        }
        let what = match &used.kind {
            UseKind::Provided { provider, state } => {
                provided_line(provider, *state, consumer, used.required)
            }
            UseKind::InstallsDefault { provider } => format!(
                "no installed extension provides it; Pane installs {}, which {consumer} names",
                provider.title
            ),
            UseKind::NoProvider if used.some_commands => {
                format!(
                    "no installed extension provides it; the commands of {consumer} that need \
                     it wait until one does"
                )
            }
            UseKind::NoProvider => {
                format!("no installed extension provides it; {consumer} waits until one does")
            }
        };
        format!("Uses{needs} {capability}: {what}")
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

    /// The titles of the required dependencies installed with the requested
    /// one, and not the default providers its uses name (see
    /// [`Plan::named_defaults`]).
    pub fn installed_with(&self) -> Vec<String> {
        self.install
            .iter()
            .filter(|package| {
                !self
                    .defaults
                    .iter()
                    .any(|default| default.target.identity == package.identity)
            })
            .map(|package| package.manifest.title.clone())
            .collect()
    }

    /// The titles of the default providers of uses installed with the
    /// requested one.
    pub fn named_defaults(&self) -> Vec<String> {
        self.defaults
            .iter()
            .map(|default| default.target.title.clone())
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
/// reads and validates the package with an identity from the source a
/// dependency declares (a folder, or a download from npm). Changes nothing.
pub(crate) fn plan(
    requested: &SourcePackage,
    installed: &[InstalledPackage],
    paused: &[PackageIdentity],
    read: impl FnMut(&PackageIdentity, &str) -> Result<SourcePackage, PackageError>,
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
            uses: Vec::new(),
            provides: Vec::new(),
            defaults: Vec::new(),
            problems: Vec::new(),
            assumptions: Assumptions {
                requested: requested.identity.clone(),
                requested_manifest: requested.manifest_text().to_owned(),
                requested_tarball: requested.fingerprint(),
                packages: Vec::new(),
            },
            requested_manifest: requested.manifest.clone(),
        },
        too_many: false,
    };
    planner.visit(requested);
    planner.plan_uses(requested);
    planner.check_demands(requested);
    planner.plan
}

/// The source of the package with `identity` as a problem names it: its
/// folder, or "npm package <name>".
pub(crate) fn source_name(identity: &PackageIdentity) -> String {
    match identity.local_folder() {
        Some(folder) => folder.display().to_string(),
        None => identity.to_string(),
    }
}

/// The installed package `package` as a provider of `capability`, if its
/// manifest provides the capability on this system: its name and title.
/// Whatever state the package is in — the user disabled it, or Pane paused
/// it — it is a provider: an installed provider is used rather than a
/// consumer's default being installed beside it.
fn provides_here(package: &InstalledPackage, capability: &str) -> Option<Named> {
    let manifest = package.manifest.as_ref().ok()?;
    entry_of(manifest, capability)?;
    Some(Named {
        identity: package.identity.clone(),
        title: package.title(),
    })
}

/// The manifest entry providing `capability`, when the manifest provides it
/// on this system: `None` when it does not.
fn entry_of<'a>(manifest: &'a Manifest, capability: &str) -> Option<&'a ManifestProvides> {
    let entry = manifest
        .provides
        .iter()
        .find(|entry| entry.capability == capability)?;
    platform::unavailable(entry.platforms.as_deref(), "this capability")
        .is_none()
        .then_some(entry)
}

/// Why `manifest` does not serve as the default provider of `capability`
/// for the operations `operations`, as [`ProblemKind::DefaultDoesNotProvide`]
/// says it: `None` when it does. It must provide the capability, on this
/// system, with every operation the use calls.
fn default_lacks(manifest: &Manifest, capability: &str, operations: &[String]) -> Option<String> {
    let Some(entry) = manifest
        .provides
        .iter()
        .find(|entry| entry.capability == capability)
    else {
        return Some("does not provide it".into());
    };
    if let Some(reason) = platform::unavailable(entry.platforms.as_deref(), "this capability") {
        return Some(format!("does not provide it here: {reason}"));
    }
    for called in operations {
        if !entry.operations.contains(called) {
            return Some(format!("does not provide its `{called}` operation"));
        }
    }
    None
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

impl<R: FnMut(&PackageIdentity, &str) -> Result<SourcePackage, PackageError>> Planner<'_, R> {
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
            let local_from_published = package.identity.is_published()
                && matches!(
                    SourceSpec::parse(&dependency.source),
                    Ok(SourceSpec::Local(_))
                );
            if local_from_published && dependency.required {
                let from = if package.identity.npm_name().is_some() {
                    "npm"
                } else {
                    "Git"
                };
                self.plan
                    .problems
                    .push(problem(ProblemKind::LocalFromPublished {
                        from,
                        source: dependency.source.clone(),
                    }));
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
            match (self.read)(&target, &dependency.source) {
                Ok(source) => {
                    let named = Named {
                        identity: target.clone(),
                        title: source.manifest.title.clone(),
                    };
                    self.plan.required.push(edge(named, RequiredState::Install));
                    let tarball = source.fingerprint();
                    self.plan.assumptions.packages.push((
                        target,
                        Assumed::Installs(source.manifest_text().to_owned(), tarball),
                    ));
                    self.visit(&source);
                    self.plan.install.push(source);
                }
                Err(error) => self.plan.problems.push(problem(ProblemKind::CannotInstall {
                    from: source_name(&target),
                    error,
                })),
            }
        }
    }

    /// Plans the capabilities the requested package and the default
    /// providers installed with it use (see `Use`), and the capabilities
    /// the requested package provides that others provide too (see
    /// `Provided`). Every default the plan installs brings its own uses in,
    /// planned the same way.
    fn plan_uses(&mut self, requested: &SourcePackage) {
        // Each package whose uses are planned, with the use declarations
        // its manifest makes: the requested one, and every default
        // provider the plan installs as it discovers them.
        let mut consumers = VecDeque::from([(
            requested.identity.clone(),
            requested.manifest.title.clone(),
            requested.manifest.uses.clone(),
        )]);
        while let Some((identity, title, uses)) = consumers.pop_front() {
            let consumer = Named { identity, title };
            for used in uses {
                let planned = self.plan_use(&consumer, &used, &mut consumers);
                self.plan.uses.push(planned);
            }
        }
        self.plan_provides(requested);
    }

    /// Plans one use of `consumer`: who provides it — a package installed
    /// already, whatever state it is in, or one this install installs, never
    /// the consumer's own package — or, for a required use with no
    /// provider, the default it names, installed as a missing required
    /// dependency is, queued in `consumers` for its own uses. An optional
    /// use is listed and its default is never installed; a use with no
    /// provider and no default leaves the package waiting for one, which
    /// stops nothing.
    fn plan_use(
        &mut self,
        consumer: &Named,
        used: &ManifestUse,
        consumers: &mut VecDeque<(PackageIdentity, String, Vec<ManifestUse>)>,
    ) -> Use {
        let planned = |kind| Use {
            consumer: consumer.clone(),
            capability: used.capability.clone(),
            required: used.required,
            some_commands: used.commands.is_some(),
            default: used.default.clone(),
            kind,
        };
        if let Some((provider, state)) = self.provider_of(consumer, &used.capability) {
            return planned(UseKind::Provided { provider, state });
        }
        let no_provider = || planned(UseKind::NoProvider);
        let Some(source) = used.default.as_ref() else {
            return no_provider();
        };
        // Only a required use has its default installed: an optional one is
        // listed and left alone.
        if !used.required {
            return no_provider();
        }
        let problem = |kind| Problem {
            dependent: consumer.clone(),
            id: used.capability.clone(),
            kind,
        };
        let target = match consumer.identity.dependency(source) {
            Ok(target) => target,
            Err(path) => {
                self.plan
                    .problems
                    .push(problem(ProblemKind::DefaultUnresolvable {
                        source: source.clone(),
                        path,
                    }));
                return no_provider();
            }
        };
        if target == consumer.identity {
            self.plan.problems.push(problem(ProblemKind::DefaultItself));
            return no_provider();
        }
        // One copy per source: a default that is installed already, or
        // installed with this one — as a required dependency — cannot be
        // installed again. It did not turn up among the providers, so it
        // does not provide the capability; without it, the package waits.
        if let Some(installed) = installed_as(self.installed, &target) {
            self.plan
                .problems
                .push(problem(ProblemKind::DefaultDoesNotProvide {
                    source: source.clone(),
                    target: Named {
                        identity: target,
                        title: installed.title(),
                    },
                    found: DefaultFound::Installed,
                    why: "does not provide it".into(),
                }));
            return no_provider();
        }
        if self.seen.contains(&target) {
            self.plan
                .problems
                .push(problem(ProblemKind::DefaultDoesNotProvide {
                    source: source.clone(),
                    target: self.planned_named(&target),
                    found: DefaultFound::WithThis,
                    why: "does not provide it".into(),
                }));
            return no_provider();
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
            return no_provider();
        }
        self.seen.push(target.clone());
        match (self.read)(&target, source) {
            Ok(package) => {
                // It must provide the capability it is named for, on this
                // system, with every operation the use calls.
                let lacks = default_lacks(&package.manifest, &used.capability, &used.operations);
                if let Some(why) = lacks {
                    self.plan
                        .problems
                        .push(problem(ProblemKind::DefaultDoesNotProvide {
                            source: source.clone(),
                            target: Named {
                                identity: target,
                                title: package.manifest.title.clone(),
                            },
                            found: DefaultFound::Source,
                            why,
                        }));
                    return no_provider();
                }
                let named = Named {
                    identity: target.clone(),
                    title: package.manifest.title.clone(),
                };
                self.plan.defaults.push(DefaultProvider {
                    consumer: consumer.clone(),
                    capability: used.capability.clone(),
                    source: source.clone(),
                    target: named.clone(),
                });
                self.plan.assumptions.packages.push((
                    target,
                    Assumed::Installs(package.manifest_text().to_owned(), package.fingerprint()),
                ));
                consumers.push_back((
                    named.identity.clone(),
                    named.title.clone(),
                    package.manifest.uses.clone(),
                ));
                self.visit(&package);
                self.plan.install.push(package);
                planned(UseKind::InstallsDefault { provider: named })
            }
            Err(error) => {
                self.plan
                    .problems
                    .push(problem(ProblemKind::DefaultCannotInstall {
                        source: source.clone(),
                        from: source_name(&target),
                        error,
                    }));
                no_provider()
            }
        }
    }

    /// The package with `identity` as the plan names it: one this install
    /// installs — the requested package or one installed with it — or the
    /// identity itself, when the plan knows no title of it.
    fn planned_named(&self, identity: &PackageIdentity) -> Named {
        if *identity == self.plan.requested.identity {
            return self.plan.requested.clone();
        }
        self.plan
            .install
            .iter()
            .find(|package| package.identity == *identity)
            .map(|package| Named {
                identity: identity.clone(),
                title: package.manifest.title.clone(),
            })
            .unwrap_or_else(|| Named {
                identity: identity.clone(),
                title: identity.to_string(),
            })
    }

    /// Plans the capabilities the requested package provides that other
    /// packages provide too: a choice the user has in Settings.
    fn plan_provides(&mut self, requested: &SourcePackage) {
        for entry in &requested.manifest.provides {
            // Elsewhere the package is not a provider of the capability.
            let elsewhere = platform::unavailable(entry.platforms.as_deref(), "this capability");
            if elsewhere.is_some() {
                continue;
            }
            let others = self.providers_of(&requested.identity, &entry.capability);
            if others.is_empty() {
                continue;
            }
            self.plan.provides.push(Provided {
                capability: entry.capability.clone(),
                others,
            });
        }
    }

    /// The first package providing `capability` for `consumer`, in install
    /// order, never the consumer's own package (see `Planner::providers_of`).
    fn provider_of(&self, consumer: &Named, capability: &str) -> Option<(Named, ProvidedState)> {
        self.providers_of(&consumer.identity, capability)
            .into_iter()
            .next()
    }

    /// The packages providing `capability`, in install order, excluding the
    /// package `not`: those installed already — whatever state each is in,
    /// for an installed provider is used rather than a default being
    /// installed beside it — then those this install installs, deduplicated
    /// by identity (a package installed already that is also replaced by
    /// this install).
    fn providers_of(&self, not: &PackageIdentity, capability: &str) -> Vec<(Named, ProvidedState)> {
        let mut found: Vec<PackageIdentity> = Vec::new();
        let mut providers = Vec::new();
        for package in self.installed {
            if package.identity == *not || found.contains(&package.identity) {
                continue;
            }
            if let Some(named) = provides_here(package, capability) {
                let state = match (package.enabled, self.paused.contains(&package.identity)) {
                    (false, _) => ProvidedState::Disabled,
                    (true, true) => ProvidedState::Paused,
                    (true, false) => ProvidedState::Installed,
                };
                found.push(package.identity.clone());
                providers.push((named, state));
            }
        }
        // The packages this install installs provide it too: a default is
        // never installed beside a provider coming with it.
        for (identity, title, manifest) in self.planned_packages() {
            if *identity == *not || found.contains(identity) {
                continue;
            }
            if entry_of(manifest, capability).is_some() {
                found.push(identity.clone());
                providers.push((
                    Named {
                        identity: identity.clone(),
                        title: title.clone(),
                    },
                    ProvidedState::WithThis,
                ));
            }
        }
        providers
    }

    /// The requested package and those installed with it, each with the
    /// identity, title and manifest the plan knows of it.
    fn planned_packages(&self) -> Vec<(&PackageIdentity, &String, &Manifest)> {
        let mut planned = vec![(
            &self.plan.requested.identity,
            &self.plan.requested.title,
            &self.plan.requested_manifest,
        )];
        for package in &self.plan.install {
            planned.push((
                &package.identity,
                &package.manifest.title,
                &package.manifest,
            ));
        }
        planned
    }

    /// How many packages to install are still being visited: read, but not
    /// yet added to the plan's install list.
    fn visiting(&self) -> usize {
        let installs = self
            .plan
            .assumptions
            .packages
            .iter()
            .filter(|(_, assumed)| matches!(assumed, Assumed::Installs(..)))
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
            // The npm version each dependent pins must be the one of the
            // copy: the one installed, or the one read for the first
            // dependent found to need it.
            let have = if *target == requested.identity {
                requested.npm.as_ref().map(|npm| {
                    let by = (requested.manifest.title.clone(), npm.package.pinned);
                    (npm.package.version.clone(), Some(by))
                })
            } else if let Some(source) = self.plan.install.iter().find(|p| p.identity == *target) {
                source.npm.as_ref().map(|npm| {
                    let by = (demands[0].dependent.title.clone(), npm.package.pinned);
                    (npm.package.version.clone(), Some(by))
                })
            } else {
                installed
                    .and_then(|installed| installed.npm.as_ref())
                    .map(|npm| (npm.version.clone(), None))
            };
            self.check_git_revisions(
                requested,
                target,
                &named,
                installed,
                &demands,
                &mut problems,
            );
            if let Some((have, by)) = have {
                for demand in &demands {
                    let wanted = match SourceSpec::parse(&demand.dependency.source) {
                        Ok(SourceSpec::Npm(spec)) => spec.version,
                        _ => None,
                    };
                    if let Some(wanted) = wanted.filter(|wanted| *wanted != have) {
                        problems.push(problem(
                            demand,
                            ProblemKind::NpmVersion {
                                target: named.clone(),
                                wanted,
                                have: have.clone(),
                                by: by.clone(),
                            },
                        ));
                    }
                }
            }
        }
        self.plan.problems.extend(problems);
    }
}

impl<R> Planner<'_, R> {
    /// Checks that the Git revision each dependent names for `target` (a
    /// branch, a tag or a commit) is the one of the plan's copy: the one
    /// installed, or the one fetched for the first dependent found to need
    /// it.
    fn check_git_revisions(
        &self,
        requested: &SourcePackage,
        target: &PackageIdentity,
        named: &Named,
        installed: Option<&InstalledPackage>,
        demands: &[&Demand],
        problems: &mut Vec<Problem>,
    ) {
        let (revision, by) = if *target == requested.identity {
            let Some(git) = &requested.git else { return };
            (git.revision.clone(), Some(requested.manifest.title.clone()))
        } else if let Some(source) = self.plan.install.iter().find(|p| p.identity == *target) {
            let Some(git) = &source.git else { return };
            (
                git.revision.clone(),
                Some(demands[0].dependent.title.clone()),
            )
        } else if let Some(git) = installed.and_then(|installed| installed.git.as_ref()) {
            (git.revision.clone(), None)
        } else {
            return;
        };
        let have = format!(
            "{} (commit {})",
            revision.describe(),
            revision.short_commit()
        );
        for demand in demands {
            let Ok(SourceSpec::Git(spec)) = SourceSpec::parse(&demand.dependency.source) else {
                continue;
            };
            let Some(wanted) = spec.reference else {
                continue;
            };
            if git_matches(&wanted, &revision) {
                continue;
            }
            problems.push(Problem {
                dependent: demand.dependent.clone(),
                id: demand.dependency.id.clone(),
                kind: ProblemKind::GitRevision {
                    target: named.clone(),
                    wanted,
                    have: have.clone(),
                    by: by.clone(),
                },
            });
        }
    }
}

/// Whether the reference `wanted` (as a dependency names it) is `revision`:
/// its commit, its branch or its tag.
fn git_matches(wanted: &str, revision: &crate::git::GitRevision) -> bool {
    use crate::git::GitRef;
    if wanted.eq_ignore_ascii_case(&revision.commit) {
        return true;
    }
    match &revision.reference {
        GitRef::Branch(branch) => {
            wanted == branch || wanted.strip_prefix("refs/heads/") == Some(branch.as_str())
        }
        GitRef::Tag(tag) => {
            wanted == tag || wanted.strip_prefix("refs/tags/") == Some(tag.as_str())
        }
        GitRef::Default {
            branch: Some(branch),
        } => wanted == branch || wanted.strip_prefix("refs/heads/") == Some(branch.as_str()),
        GitRef::Default { branch: None } | GitRef::Commit => false,
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

/// A capability that disabling or uninstalling a package leaves without
/// an installed provider, as the Disable all and Uninstall all questions
/// list it: the package is the last provider, and these enabled packages
/// require the capability.
#[derive(Clone, Debug)]
pub(crate) struct LastProvider {
    /// The capability's name.
    pub capability: String,
    /// The enabled packages that require it, in installed order; the
    /// package asked about is never among them.
    pub consumers: Vec<Named>,
}

/// The capabilities `of` is the last installed provider of that enabled
/// installed packages require, each with those packages: with another
/// provider left there is nothing to say. Capabilities add no package to
/// the questions' dependent closures — another provider may serve — so
/// the question says instead who will wait for a provider.
pub(crate) fn last_provided(
    installed: &[InstalledPackage],
    of: &PackageIdentity,
) -> Vec<LastProvider> {
    let Some(package) = installed_as(installed, of) else {
        return Vec::new();
    };
    let Ok(manifest) = &package.manifest else {
        return Vec::new();
    };
    let mut last = Vec::new();
    for entry in &manifest.provides {
        // Elsewhere the package is not a provider of the capability, so
        // nothing changes for its consumers.
        let elsewhere = platform::unavailable(entry.platforms.as_deref(), "this capability");
        if elsewhere.is_some() {
            continue;
        }
        // Another installed provider remains: nothing waits.
        let another = installed.iter().any(|other| {
            other.identity != *of && provides_here(other, &entry.capability).is_some()
        });
        if another {
            continue;
        }
        let consumers: Vec<Named> = installed
            .iter()
            .filter(|other| other.enabled && other.identity != *of)
            .filter(|other| requires_capability(other, &entry.capability))
            .map(|other| Named {
                identity: other.identity.clone(),
                title: other.title(),
            })
            .collect();
        if consumers.is_empty() {
            continue;
        }
        last.push(LastProvider {
            capability: entry.capability.clone(),
            consumers,
        });
    }
    last
}

/// Whether `package` requires the capability `capability`, as its manifest
/// declares: an optional use never waits.
fn requires_capability(package: &InstalledPackage, capability: &str) -> bool {
    package.manifest.as_ref().is_ok_and(|manifest| {
        manifest
            .uses
            .iter()
            .any(|used| used.capability == capability && used.required)
    })
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

/// The requirement groups among `installed`: each a set of two or more
/// packages that require one another, directly or through others, as
/// their `pane.json` files declare — a required dependency, or a required
/// capability one uses and another provides (a package's own provision
/// never serves its own use, so it is never its own requirement; a use
/// narrowed to commands or every provider never gates the package, so it
/// makes no requirement either). The groups are found from the
/// declarations alone, whatever the packages' states: a group of healthy
/// packages runs, as ADR 0041 decides, and a group one member of which
/// cannot run waits as a whole (see `waiting`). Each group's members in
/// installed order, and the groups in installed order of their first
/// member; a package is in one group at most. Reads only the installed
/// records; runs nothing. Manage extensions shows the group on every
/// member's page (#157).
pub(crate) fn requirement_groups(installed: &[InstalledPackage]) -> Vec<Vec<PackageIdentity>> {
    let requires = requirements_among(installed);
    // Tarjan's strongly connected components, over the packages' indices.
    let mut found = Groups {
        requires: &requires,
        next: 0,
        seen: vec![None; installed.len()],
        low: vec![usize::MAX; installed.len()],
        stack: Vec::new(),
        stacked: vec![false; installed.len()],
        groups: Vec::new(),
    };
    for package in 0..installed.len() {
        if found.seen[package].is_none() {
            found.visit(package);
        }
    }
    let mut groups = found.groups;
    groups.sort_unstable();
    groups
        .into_iter()
        .map(|group| {
            group
                .into_iter()
                .map(|package| installed[package].identity.clone())
                .collect()
        })
        .collect()
}

/// Which of `installed` each package requires, by index, as its `pane.json`
/// declares: its required dependencies needed on this system, and the
/// providers of each capability it requires (a package's own provision
/// never serving its own use).
fn requirements_among(installed: &[InstalledPackage]) -> Vec<Vec<usize>> {
    installed
        .iter()
        .map(|package| {
            let Ok(manifest) = &package.manifest else {
                return Vec::new();
            };
            let mut required: Vec<usize> = manifest
                .dependencies
                .iter()
                .filter(|dependency| dependency.required && dependency.needed_here())
                .filter_map(|dependency| package.dependency_identity(&dependency.id))
                .filter_map(|recorded| index_of(installed, package, recorded))
                .collect();
            // A capability requirement reaches every provider of the
            // capability installed besides the package itself; a use of
            // every provider, or one narrowed to commands, never gates the
            // package and makes no requirement.
            for used in manifest
                .uses
                .iter()
                .filter(|used| used.required && !used.use_all && used.commands.is_none())
            {
                required.extend(
                    installed
                        .iter()
                        .enumerate()
                        .filter(|(_, other)| {
                            other.identity != package.identity
                                && provides_capability(other, &used.capability)
                        })
                        .map(|(index, _)| index),
                );
            }
            required.sort_unstable();
            required.dedup();
            required
        })
        .collect()
}

/// The index among `installed` of the package the recorded `identity` was
/// resolved to, if it is installed; a package requiring itself is no
/// requirement of anything.
fn index_of(
    installed: &[InstalledPackage],
    of: &InstalledPackage,
    identity: &PackageIdentity,
) -> Option<usize> {
    let found = reached(installed, identity)?;
    let index = installed
        .iter()
        .position(|other| other.identity == found.identity)?;
    (found.identity != of.identity).then_some(index)
}

/// Whether `package` provides `capability` on this system, as its manifest
/// declares: it names the capability among `provides` and the entry is for
/// this system.
pub(crate) fn provides_capability(package: &InstalledPackage, capability: &str) -> bool {
    let Ok(manifest) = &package.manifest else {
        return false;
    };
    manifest.provides.iter().any(|provides| {
        provides.capability == capability
            && platform::unavailable(provides.platforms.as_deref(), "it").is_none()
    })
}

/// Tarjan's traversal over the packages' indices, closing each strongly
/// connected group it finds (see [`requirement_groups`]).
struct Groups<'a> {
    /// What each package requires, by index.
    requires: &'a [Vec<usize>],
    /// The index the next visited package takes.
    next: usize,
    /// Each package's visit index, once visited.
    seen: Vec<Option<usize>>,
    /// Each package's low link: the least index it reaches back to.
    low: Vec<usize>,
    /// The packages whose groups are not closed yet.
    stack: Vec<usize>,
    /// Which packages `stack` holds.
    stacked: Vec<bool>,
    /// The groups of two or more packages found, each in index order.
    groups: Vec<Vec<usize>>,
}

impl Groups<'_> {
    /// Visits `package`: notes its index and low link, follows what it
    /// requires, and closes the group it roots, if the group has a member
    /// besides it.
    fn visit(&mut self, package: usize) {
        self.seen[package] = Some(self.next);
        self.low[package] = self.next;
        self.next += 1;
        self.stack.push(package);
        self.stacked[package] = true;
        let requires = self.requires[package].clone();
        for &required in &requires {
            match self.seen[required] {
                None => {
                    self.visit(required);
                    self.low[package] = self.low[package].min(self.low[required]);
                }
                Some(index) if self.stacked[required] => {
                    self.low[package] = self.low[package].min(index);
                }
                Some(_) => {}
            }
        }
        if self.seen[package] == Some(self.low[package]) {
            let mut group = Vec::new();
            while let Some(member) = self.stack.pop() {
                self.stacked[member] = false;
                group.push(member);
                if member == package {
                    break;
                }
            }
            // A package alone requires itself through no one: groups of
            // one are not cycles.
            if group.len() > 1 {
                group.sort_unstable();
                self.groups.push(group);
            }
        }
    }
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
    use std::path::Path;

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

    /// Reads the package with `identity` from its local folder, as the
    /// launcher's read closure does for a default.
    fn read_local(
        identity: &PackageIdentity,
        _source: &str,
    ) -> Result<SourcePackage, PackageError> {
        let folder = identity.local_folder().expect("a local package");
        SourcePackage::read(folder)
    }

    /// A package folder `name` in `sources`, as `package` writes one, whose
    /// manifest declares `uses` (JSON array contents).
    fn using(sources: &Path, name: &str, uses: &str) -> SourcePackage {
        let folder = sources.join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("c.wasm"), b"not run").unwrap();
        fs::write(
            folder.join("pane.json"),
            format!(
                r#"{{ "manifestVersion": 1, "title": "Package {name}", "apiVersion": "0.1",
                     "operations": [{{ "id": "echo", "version": 1, "component": "c.wasm" }}],
                     "uses": [{uses}] }}"#
            ),
        )
        .unwrap();
        SourcePackage::read(&folder).unwrap()
    }

    /// A package folder `name` in `sources` providing the capability the
    /// `using` ones use.
    fn providing(sources: &Path, name: &str) -> SourcePackage {
        let folder = sources.join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("c.wasm"), b"not run").unwrap();
        fs::write(
            folder.join("pane.json"),
            format!(
                r#"{{ "manifestVersion": 1, "title": "Package {name}", "apiVersion": "0.1",
                     "provides": [{{ "capability": "test:greet@1", "component": "c.wasm",
                                    "operations": ["greet"] }}] }}"#
            ),
        )
        .unwrap();
        SourcePackage::read(&folder).unwrap()
    }

    #[test]
    fn a_use_s_default_is_planned_installed_and_rolled_back_as_a_dependency_is() {
        let (dir, _data, mut store) = setup();
        // a uses a capability no one provides, naming b as its default
        // provider; b provides it.
        let a = using(
            dir.path(),
            "a",
            r#"{ "capability": "test:greet@1", "operations": ["greet"],
                 "default": "local:../b" }"#,
        );
        let b = providing(dir.path(), "b");
        let plan = plan(&a, &[], &[], read_local);

        // The default is planned as a missing required dependency is: in
        // the install list, claimed by the assumptions, and named by the
        // plan apart from the required dependencies.
        assert_eq!(plan.install.len(), 1);
        assert_eq!(plan.install[0].identity, b.identity);
        assert_eq!(plan.defaults.len(), 1);
        assert_eq!(plan.defaults[0].consumer.identity, a.identity);
        assert_eq!(plan.defaults[0].target.identity, b.identity);
        assert_eq!(plan.named_defaults(), ["Package b".to_owned()]);
        assert_eq!(plan.installed_with(), Vec::<String>::new());
        assert_eq!(
            plan.uses
                .iter()
                .map(|used| used.capability.clone())
                .collect::<Vec<_>>(),
            ["test:greet@1".to_owned()]
        );
        assert!(
            matches!(
                plan.assumptions.packages.as_slice(),
                [(identity, Assumed::Installs(..))] if *identity == b.identity
            ),
            "{:?}",
            plan.assumptions.packages
        );

        // A failure after the default was installed removes it again.
        let failure = install_all(&mut store, &plan.install, |_| {
            Err(PackageError::Storage("the disk is full".into()))
        })
        .expect_err("the install fails");
        assert_eq!(
            failure.error,
            "Could not update Pane's installed extensions: the disk is full"
        );
        assert!(titles(&store).is_empty());
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
