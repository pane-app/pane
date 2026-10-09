//! Which installed packages wait for a required dependency that cannot
//! serve them, and why: the first slice of the waiting commands of #151
//! (ADR 0041), with no manifest fields beyond the dependencies
//! `pane.json` already declares.
//!
//! A command of an enabled, unpaused package waits while one of its
//! package's required dependencies is missing, disabled, paused or waiting
//! itself. An optional dependency never makes a command wait, nor does one
//! the package needs only on other systems. Who waits is computed as a
//! greatest fixed point over the manifests and the packages' states
//! ([`Waiting::of`]): every enabled, unpaused package starts as able to
//! run, and any package with an unmet requirement is removed, repeating
//! until nothing changes. A cycle of healthy packages runs; a cycle with
//! one member missing waits as a whole.
//!
//! What a package's commands do while it waits is the launcher's, told by
//! `State::recheck_waiting` recomputing this whenever the packages or their
//! pauses change: a waiting command's view, run entry point, actions,
//! arguments and setup screen do not run, its schedule's ticks are skipped
//! and not replayed, its service does not cycle, and root search does not
//! ask for its root or indexed results. Waiting ends no generation and
//! stops no instance: a call or cycle already running finishes, and an
//! open screen stays. It never counts towards pausing. A package waiting
//! as a whole answers its published operations `unavailable`, naming what
//! it waits for.
//!
//! When the requirement is met again — the dependency is enabled, retried,
//! installed, or its package is reloaded or updated — the waiting map is
//! recomputed the same way and the command comes back by itself.
//!
//! The wording lives here, as the dependency plan's does (see
//! `dependencies`): the map is data.

use std::collections::HashMap;

use crate::packages::{InstalledPackage, PackageIdentity};
use crate::platform;

/// The packages that wait, each with why (see the module documentation).
/// [`Waiting::of`] is the only constructor.
#[derive(Clone, Default, Debug)]
pub(crate) struct Waiting {
    reasons: HashMap<PackageIdentity, Reason>,
}

/// Why one package waits, as its commands' rows, the calls its package
/// would serve and the fix row beside the reason say it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reason {
    /// "Needs <title>, which is <state>", the reason under each of its
    /// commands' rows; a chain names what is actually missing ("Needs
    /// Notes Sync, which waits for Auth: Auth is disabled").
    pub(crate) row: String,
    /// "<title> is waiting for <what>", what a call to its operations is
    /// answered with.
    pub(crate) calling: String,
    /// What the fix row beside the reason offers.
    pub(crate) fix: Fix,
}

/// What fixes a wait, as the fix row beside the reason shows it: "Enable
/// <title>", "Retry <title>" or "Open Manage extensions".
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Fix {
    /// Enable the disabled package the chain of waits ends at.
    Enable(PackageIdentity),
    /// Retry the paused package the chain of waits ends at.
    Retry(PackageIdentity),
    /// Nothing Pane can do directly: what it waits for is not installed.
    Manage,
}

/// What makes a dependency unmet, when the dependency is what is actually
/// missing rather than waiting for another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unmet {
    /// It is not installed.
    NotInstalled,
    /// The user disabled it.
    Disabled,
    /// Pane paused it after it failed.
    Paused,
}

impl Unmet {
    /// "not installed", "disabled" or "paused".
    fn state(self) -> &'static str {
        match self {
            Unmet::NotInstalled => "not installed",
            Unmet::Disabled => "disabled",
            Unmet::Paused => "paused",
        }
    }

    /// "<title> is <state>".
    fn says(self, title: &str) -> String {
        format!("{title} is {}", self.state())
    }
}

impl Waiting {
    /// Computes who waits now: the greatest fixed point over `packages`
    /// and their states. `paused` tells whether a package is paused after
    /// it failed, and `title_of` names a package, as well as it is known
    /// (an installed one's title, the title its retained data was kept
    /// under, or its identity).
    pub(crate) fn of(
        packages: &[InstalledPackage],
        paused: &dyn Fn(&PackageIdentity) -> bool,
        title_of: &dyn Fn(&PackageIdentity) -> String,
    ) -> Waiting {
        // Every enabled, unpaused package starts as able to run.
        let mut able: Vec<bool> = packages
            .iter()
            .map(|package| package.enabled && !paused(&package.identity))
            .collect();
        // Any package with an unmet requirement is removed, repeating until
        // nothing changes: one that was removed makes its own dependents
        // wait, which is how a cycle with one member missing waits as a
        // whole while a cycle of healthy packages runs.
        loop {
            let mut removed = false;
            for (index, package) in packages.iter().enumerate() {
                if !able[index] || unmet_of(packages, &able, paused, title_of, package).is_empty() {
                    continue;
                }
                able[index] = false;
                removed = true;
            }
            if !removed {
                break;
            }
        }
        let reasons = packages
            .iter()
            .enumerate()
            // A disabled or paused package does not wait: it says its own
            // state, as before.
            .filter(|(index, package)| {
                !able[*index] && package.enabled && !paused(&package.identity)
            })
            .map(|(_, package)| {
                (
                    package.identity.clone(),
                    reason(packages, &able, paused, title_of, package),
                )
            })
            .collect();
        Waiting { reasons }
    }

    /// Why the package with `identity` waits, if it does.
    pub(crate) fn reason(&self, identity: &PackageIdentity) -> Option<&Reason> {
        self.reasons.get(identity)
    }
}

/// One required dependency of a waiting package that is not met: the
/// dependency, by the identity it resolved to when the package was
/// installed, its title as well as it is known, and its state — or `None`
/// when it waits for another, which the chain of waits names.
struct UnmetDependency {
    identity: PackageIdentity,
    title: String,
    state: Option<Unmet>,
}

/// The required dependencies of `package` that are not met, in the order
/// its manifest declares them: each is missing, disabled, paused or
/// waiting itself.
fn unmet_of(
    packages: &[InstalledPackage],
    able: &[bool],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    title_of: &dyn Fn(&PackageIdentity) -> String,
    package: &InstalledPackage,
) -> Vec<UnmetDependency> {
    let Ok(manifest) = &package.manifest else {
        // A package whose manifest cannot be read is broken its own way;
        // it declares nothing Pane can check here.
        return Vec::new();
    };
    manifest
        .dependencies
        .iter()
        .filter(|dependency| dependency.required && dependency.needed_here())
        .filter_map(|dependency| {
            let identity = package.dependency_identity(&dependency.id)?.clone();
            let (index, dependency) = match packages
                .iter()
                .enumerate()
                .find(|(_, other)| other.identity == identity)
            {
                Some((index, dependency)) => (index, dependency),
                None => {
                    return Some(UnmetDependency {
                        title: title_of(&identity),
                        state: Some(Unmet::NotInstalled),
                        identity,
                    });
                }
            };
            if !dependency.enabled {
                return Some(UnmetDependency {
                    title: dependency.title(),
                    state: Some(Unmet::Disabled),
                    identity,
                });
            }
            if paused(&dependency.identity) {
                return Some(UnmetDependency {
                    title: dependency.title(),
                    state: Some(Unmet::Paused),
                    identity,
                });
            }
            // It is installed, enabled and unpaused: it either waits for
            // another, or serves and the requirement is met.
            (!able[index]).then(|| UnmetDependency {
                title: dependency.title(),
                state: None,
                identity,
            })
        })
        .collect()
}

/// What is actually missing at the end of the chain of waits that starts
/// with the last package of `path` — the first dependency, from that
/// package on, that is itself missing, disabled or paused rather than
/// waiting for another. `None` when the chain cannot be named (a cycle,
/// which the fixed point does not make: a package is removed only through
/// one that is missing, disabled, paused or removed before it).
fn root_of(
    packages: &[InstalledPackage],
    able: &[bool],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    title_of: &dyn Fn(&PackageIdentity) -> String,
    path: &[PackageIdentity],
) -> Option<(PackageIdentity, String, Unmet)> {
    let last = path.last()?;
    let package = packages.iter().find(|package| &package.identity == last)?;
    let first = unmet_of(packages, able, paused, title_of, package)
        .into_iter()
        .next()?;
    match first.state {
        // The dependency is what is actually missing.
        Some(state) => Some((first.identity, first.title, state)),
        // It waits for another: follow the chain on, guarding a cycle.
        None => {
            let mut path = path.to_vec();
            if path.contains(&first.identity) {
                return None;
            }
            path.push(first.identity);
            root_of(packages, able, paused, title_of, &path)
        }
    }
}

/// Why `package` waits, from its unmet required dependencies: what its
/// commands' rows say, what a call to its operations is answered with, and
/// what fixes it.
fn reason(
    packages: &[InstalledPackage],
    able: &[bool],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    title_of: &dyn Fn(&PackageIdentity) -> String,
    package: &InstalledPackage,
) -> Reason {
    let unmets = unmet_of(packages, able, paused, title_of, package);
    let whats: Vec<String> = unmets
        .iter()
        .map(|unmet| what_of(packages, able, paused, title_of, package, unmet))
        .collect();
    let what = platform::join(&whats);
    // The first requirement's root cause is what the fix row fixes.
    let root = unmets.first().and_then(|first| match first.state {
        // The dependency itself is what is actually missing.
        Some(state) => Some((first.identity.clone(), state)),
        // It waits for another: the chain's root is what is missing.
        None => {
            let path = [package.identity.clone(), first.identity.clone()];
            root_of(packages, able, paused, title_of, &path)
                .map(|(identity, _, state)| (identity, state))
        }
    });
    Reason {
        row: format!("Needs {what}"),
        calling: format!("{} is waiting for {what}", package.title()),
        fix: match root {
            Some((identity, Unmet::Disabled)) => Fix::Enable(identity),
            Some((identity, Unmet::Paused)) => Fix::Retry(identity),
            _ => Fix::Manage,
        },
    }
}

/// What one unmet requirement of `package` is named by: "Greeter, which is
/// disabled", or, down a chain, "Notes Sync, which waits for Auth: Auth is
/// disabled" (naming what is actually missing, not the package that waits
/// for it).
fn what_of(
    packages: &[InstalledPackage],
    able: &[bool],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    title_of: &dyn Fn(&PackageIdentity) -> String,
    package: &InstalledPackage,
    unmet: &UnmetDependency,
) -> String {
    let Some(state) = unmet.state else {
        // It waits for another: what is actually missing is at the end of
        // its chain, or beyond naming if the chain is a cycle.
        let path = [package.identity.clone(), unmet.identity.clone()];
        return match root_of(packages, able, paused, title_of, &path) {
            Some((_, title, state)) => {
                let says = state.says(&title);
                format!("{}, which waits for {title}: {says}", unmet.title)
            }
            None => format!("{}, which is waiting for something else", unmet.title),
        };
    };
    format!("{}, which is {}", unmet.title, state.state())
}
