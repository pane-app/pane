//! Which installed packages wait for a required dependency or a required
//! capability that cannot serve them, and why: the waiting commands of
//! #151 (ADR 0041), on the dependencies `pane.json` already declares
//! (#152) and the capabilities it declares under `uses` (#156).
//!
//! A command of an enabled, unpaused package waits while one of its
//! package's required dependencies is missing, disabled, paused or waiting
//! itself, or while a required capability of its package has no provider
//! that is installed, enabled, not paused and not waiting. An optional
//! dependency or capability never makes a command wait, nor does a
//! dependency the package needs only on other systems, nor a use that
//! declares `"use": "all"`: a call to every provider degrades to an empty
//! list instead. A use narrowed with `commands` gates only those commands:
//! the package's other commands stay available, and only the narrowed
//! commands wait.
//!
//! Who waits is computed as a greatest fixed point over the manifests and
//! the packages' states ([`Waiting::of`]): every enabled, unpaused package
//! starts as able to run, and any package with an unmet requirement is
//! removed, repeating until nothing changes. A cycle of healthy packages
//! runs; a cycle with one member missing waits as a whole. A capability a
//! waiting package provides does not count as provided, so its own
//! consumers wait in turn, and a provider that is also a consumer of the
//! same capability never serves itself: it waits until another provider
//! can serve.
//!
//! What a waiting command does not do is the launcher's, told by
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
//! installed, or its package is reloaded or updated, or a provider of the
//! capability can serve again — the waiting map is recomputed the same way
//! and the command comes back by itself.
//!
//! The wording lives here, as the dependency plan's does (see
//! `dependencies`): the map is data.

use std::collections::HashMap;

use crate::packages::{InstalledPackage, ManifestUse, PackageIdentity};
use crate::platform;

/// The packages that wait, each with why (see the module documentation).
/// [`Waiting::of`] is the only constructor.
#[derive(Clone, Default, Debug)]
pub(crate) struct Waiting {
    /// Why each package that waits as a whole does: every one of its
    /// commands waits with this reason, and nothing of the package runs.
    reasons: HashMap<PackageIdentity, Reason>,
    /// Why commands of packages that run wait: a use narrowed to them with
    /// `commands`, keyed by the command's manifest id. The package's other
    /// commands stay available, and the package itself still provides.
    commands: HashMap<PackageIdentity, HashMap<String, Reason>>,
}

/// Why one package waits, as its commands' rows, the calls its package
/// would serve and the fix row beside the reason say it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reason {
    /// "Needs <title>, which is <state>", or "Needs <capability>:
    /// <provider> is <state>", the reason under each of its commands'
    /// rows; a chain names what is actually missing ("Needs Notes Sync,
    /// which waits for Auth: Auth is disabled", or "Needs
    /// acme:translate@1: DeepL Translate, which waits for acme:auth@1: no
    /// extension provides it").
    pub(crate) row: String,
    /// "<title> is waiting for <what>", what a call to its operations is
    /// answered with.
    pub(crate) calling: String,
    /// Each requirement that is not met, in the order the manifest
    /// declares them, each with the chain down to what is actually
    /// missing and the fix for it: what a package's page in Settings
    /// lists, one requirement row beside one fix row (#157). The row above
    /// joins the chains; the fix below is the first's.
    pub(crate) requirements: Vec<Requirement>,
    /// What the fix row beside the reason offers: the first requirement's
    /// fix.
    pub(crate) fix: Fix,
}

impl Reason {
    /// What the package waits for, as the extension list's status line
    /// says it (#157): its requirements' chains joined, "Greeter, which
    /// is disabled".
    pub(crate) fn what(&self) -> String {
        let whats: Vec<String> = self
            .requirements
            .iter()
            .map(|requirement| requirement.what.clone())
            .collect();
        platform::join(&whats)
    }
}

/// One requirement of a waiting package that is not met (see `Reason`):
/// the chain down to what is actually missing, and what fixes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Requirement {
    /// "Greeter, which is disabled", or, down a chain, "Notes Sync, which
    /// waits for Auth: Auth is disabled": the chain down to what is
    /// actually missing, not the package that waits for it.
    pub(crate) what: String,
    /// What fixes it: the chain's root cause, as the fix row beside the
    /// requirement shows it (see `Fix`).
    pub(crate) fix: Fix,
}

/// What fixes a wait, as the fix row beside the reason shows it: "Enable
/// <title>", "Retry <title>", "Install <title> again", an install row
/// for a capability's provider, a choice of provider in Settings, or
/// "Open Manage extensions".
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Fix {
    /// Enable the disabled package the chain of waits ends at.
    Enable(PackageIdentity),
    /// Retry the paused package the chain of waits ends at.
    Retry(PackageIdentity),
    /// Install again the package the chain of waits ends at, which is not
    /// installed. From root search the row beside the reason offers
    /// Manage extensions, whose pages install it; the page of the
    /// package that waits installs it directly.
    Install(PackageIdentity),
    /// Install a provider of the capability the chain of waits ends at,
    /// from the install forms: the default its use names, or any extension
    /// that provides the capability.
    InstallProvider {
        capability: String,
        /// The default the use names, written as a dependency's source is,
        /// for the row to name.
        default: Option<String>,
    },
    /// Choose a provider in Settings' Capabilities section: the capability
    /// has two or more installed providers, and none can serve it now.
    Choose,
    /// Nothing Pane can do directly: the chain of waits cannot be named.
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
        provisions: &[(String, String)],
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
        // wait, and a capability it provides no longer counts as provided,
        // which is how a cycle with one member missing waits as a whole
        // while a cycle of healthy packages runs.
        loop {
            let mut removed = false;
            for (index, package) in packages.iter().enumerate() {
                if !able[index]
                    || unmet_of(packages, &able, provisions, paused, title_of, package).is_empty()
                {
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
                    reason(packages, &able, provisions, paused, title_of, package),
                )
            })
            .collect();
        // The commands of packages that run, waiting for a use narrowed to
        // them: the fixed point leaves their packages able, so nothing
        // else waits for them through what they provide.
        let commands = packages
            .iter()
            .enumerate()
            .filter(|(index, _)| able[*index])
            .filter_map(|(_, package)| {
                let Ok(manifest) = &package.manifest else {
                    return None;
                };
                let waiting: HashMap<String, Reason> = manifest
                    .uses
                    .iter()
                    .filter(|used| used.required && !used.use_all && used.commands.is_some())
                    .filter_map(|used| {
                        unmet_capability(packages, &able, provisions, paused, package, used)
                    })
                    .flat_map(|unmet| {
                        let why = capability_reason(
                            packages, &able, provisions, paused, title_of, package, &unmet,
                        );
                        unmet
                            .commands
                            .iter()
                            .flatten()
                            .map(|command| (command.clone(), why.clone()))
                            .collect::<Vec<_>>()
                    })
                    .collect();
                (!waiting.is_empty()).then(|| (package.identity.clone(), waiting))
            })
            .collect();
        Waiting { reasons, commands }
    }

    /// Why the package with `identity` waits, if it waits as a whole: every
    /// one of its commands waits with this reason.
    pub(crate) fn reason(&self, identity: &PackageIdentity) -> Option<&Reason> {
        self.reasons.get(identity)
    }

    /// Why the command with manifest id `command` of the package with
    /// `identity` waits, if it does: the package's reason when the whole
    /// package waits, else its own, when a use is narrowed to it.
    pub(crate) fn reason_for(&self, identity: &PackageIdentity, command: &str) -> Option<&Reason> {
        self.reason(identity)
            .or_else(|| self.commands.get(identity).and_then(|why| why.get(command)))
    }

    /// The reasons of the commands of the package with `identity` that
    /// wait alone, for a use narrowed with `commands` while the package
    /// itself runs: what its row and the extension list's status line say
    /// of it. Empty when the whole package waits or none does.
    pub(crate) fn command_reasons(&self, identity: &PackageIdentity) -> Vec<&Reason> {
        self.commands
            .get(identity)
            .map(|why| why.values().collect())
            .unwrap_or_default()
    }
}

/// One requirement of a waiting package that is not met, of either kind:
/// the chain and fix for it are a [`Requirement`].
enum Missing {
    /// A required dependency: missing, disabled, paused or waiting itself.
    Dependency(UnmetDependency),
    /// A required capability with no provider that can serve it.
    Capability(UnmetCapability),
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

/// One required capability of a waiting package that no provider can
/// serve: the capability, the default its use names for the fix row, and
/// the providers that cannot serve it, with why. The package's own
/// provision is never among them: a package never serves its own use.
struct UnmetCapability {
    capability: String,
    default: Option<String>,
    providers: Vec<Provider>,
    /// The manifest ids of the commands the use is narrowed to.
    commands: Option<Vec<String>>,
}

/// One provider of an unmet capability that cannot serve it now.
struct Provider {
    identity: PackageIdentity,
    title: String,
    cannot: Cannot,
}

/// Why a provider of a capability cannot serve it now, when it cannot (see
/// [`UnmetCapability`]). A provider waiting for what it needs is not a
/// kind of its own: the chain of waits names what it actually waits for.
enum Cannot {
    /// The user disabled the package.
    Disabled,
    /// Pane paused the package after it failed.
    Paused,
    /// It is built for another system, or its copy cannot be read; the
    /// string says which, as the reason shows it after its title.
    Here(String),
    /// It waits for what it needs; the chain of waits says for what.
    Waiting,
}

/// The requirements of `package` that are not met, in the order its
/// manifest declares them: each required dependency that is missing,
/// disabled, paused or waiting itself, and each required capability of the
/// whole package (a use not narrowed with `commands`, and not one of every
/// provider) that no provider can serve.
fn unmet_of(
    packages: &[InstalledPackage],
    able: &[bool],
    provisions: &[(String, String)],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    title_of: &dyn Fn(&PackageIdentity) -> String,
    package: &InstalledPackage,
) -> Vec<Missing> {
    let Ok(manifest) = &package.manifest else {
        // A package whose manifest cannot be read is broken its own way;
        // it declares nothing Pane can check here.
        return Vec::new();
    };
    let mut requirements = manifest
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
                    return Some(Missing::Dependency(UnmetDependency {
                        title: title_of(&identity),
                        state: Some(Unmet::NotInstalled),
                        identity,
                    }));
                }
            };
            if !dependency.enabled {
                return Some(Missing::Dependency(UnmetDependency {
                    title: dependency.title(),
                    state: Some(Unmet::Disabled),
                    identity,
                }));
            }
            if paused(&dependency.identity) {
                return Some(Missing::Dependency(UnmetDependency {
                    title: dependency.title(),
                    state: Some(Unmet::Paused),
                    identity,
                }));
            }
            // It is installed, enabled and unpaused: it either waits for
            // another, or serves and the requirement is met.
            (!able[index]).then(|| {
                Missing::Dependency(UnmetDependency {
                    title: dependency.title(),
                    state: None,
                    identity,
                })
            })
        })
        .collect::<Vec<Missing>>();
    // A use of every provider never makes a command wait: a call to it
    // degrades to an empty list. A use narrowed with `commands` waits in
    // `Waiting`'s commands, not here: the package still runs, and only
    // those commands wait.
    requirements.extend(
        manifest
            .uses
            .iter()
            .filter(|used| used.required && !used.use_all && used.commands.is_none())
            .filter_map(|used| {
                unmet_capability(packages, able, provisions, paused, package, used)
                    .map(Missing::Capability)
            }),
    );
    requirements
}

/// The unmet capability use `used` of `package`, or `None` when some
/// provider can serve it: a package other than `package` itself that
/// provides the capability, is enabled, unpaused, not waiting (per `able`)
/// and built for this system. A package never serves its own use, so its
/// own provision never meets it; its providers that cannot serve are
/// gathered for the reason.
fn unmet_capability(
    packages: &[InstalledPackage],
    able: &[bool],
    provisions: &[(String, String)],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    package: &InstalledPackage,
    used: &ManifestUse,
) -> Option<UnmetCapability> {
    let mut providers = Vec::new();
    for (index, other) in packages.iter().enumerate() {
        // A package never serves its own use.
        if other.identity == package.identity {
            continue;
        }
        // What it provides is what its manifest says: a package whose
        // manifest cannot be read provides nothing Pane can check here.
        let Ok(manifest) = &other.manifest else {
            continue;
        };
        let Some(entry) = manifest
            .provides
            .iter()
            .find(|entry| entry.capability == used.capability)
        else {
            continue;
        };
        // A package provides a capability its manifest marks `atRunTime`
        // only while its code holds a provision for it (#158): while it
        // does not, it is no provider at all, not one that cannot serve.
        if entry.at_run_time
            && !provisions.iter().any(|(owner, provided)| {
                *provided == used.capability && *owner == other.identity.key()
            })
        {
            continue;
        }
        let cannot = if !other.enabled {
            Cannot::Disabled
        } else if paused(&other.identity) {
            Cannot::Paused
        } else if !able[index] {
            Cannot::Waiting
        } else {
            // The package can run here; the capability's own declaration
            // may still be for another system. None: it serves, and the
            // requirement is met.
            let reason = platform::unavailable(manifest.platforms.as_deref(), "this package")
                .or_else(|| platform::unavailable(entry.platforms.as_deref(), "this capability"))?;
            Cannot::Here(reason)
        };
        providers.push(Provider {
            identity: other.identity.clone(),
            title: other.title(),
            cannot,
        });
    }
    Some(UnmetCapability {
        capability: used.capability.clone(),
        default: used.default.clone(),
        providers,
        commands: used.commands.clone(),
    })
}

/// What is actually missing at the end of the chain of waits that starts
/// with the last package of `path` — the first requirement, from that
/// package on, that is itself missing, disabled, paused or providerless
/// rather than waiting for another. `None` when the chain cannot be named
/// (a cycle, which the fixed point does not make: a package is removed
/// only through one that is missing, disabled, paused or removed before
/// it).
fn root_of(
    packages: &[InstalledPackage],
    able: &[bool],
    provisions: &[(String, String)],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    title_of: &dyn Fn(&PackageIdentity) -> String,
    path: &[PackageIdentity],
) -> Option<Root> {
    let last = path.last()?;
    let package = packages.iter().find(|package| &package.identity == last)?;
    let first = unmet_of(packages, able, provisions, paused, title_of, package)
        .into_iter()
        .next()?;
    match first {
        // The dependency is what is actually missing.
        Missing::Dependency(unmet) => match unmet.state {
            Some(state) => Some(Root::Package(unmet.identity, state)),
            // It waits for another: follow the chain on, guarding a cycle.
            None => {
                let mut path = path.to_vec();
                if path.contains(&unmet.identity) {
                    return None;
                }
                path.push(unmet.identity);
                root_of(packages, able, provisions, paused, title_of, &path)
            }
        },
        // The capability is what is actually missing: a provider the user
        // can enable or retry, or the capability itself, whose fix row
        // installs a provider.
        Missing::Capability(unmet) => Some(root_of_capability(
            packages, able, provisions, paused, title_of, path, &unmet,
        )),
    }
}

/// The root of the chain of waits behind an unmet capability: a provider
/// that is disabled or paused (the first one, in install order), a
/// provider that waits for another (followed on, guarding a cycle), or
/// the capability itself, with no provider that can serve it.
fn root_of_capability(
    packages: &[InstalledPackage],
    able: &[bool],
    provisions: &[(String, String)],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    title_of: &dyn Fn(&PackageIdentity) -> String,
    path: &[PackageIdentity],
    unmet: &UnmetCapability,
) -> Root {
    for provider in &unmet.providers {
        match provider.cannot {
            Cannot::Disabled => return Root::Package(provider.identity.clone(), Unmet::Disabled),
            Cannot::Paused => return Root::Package(provider.identity.clone(), Unmet::Paused),
            Cannot::Here(_) => {}
            // It waits for another: follow the chain on, guarding a cycle.
            Cannot::Waiting => {
                let mut path = path.to_vec();
                if !path.contains(&provider.identity) {
                    path.push(provider.identity.clone());
                    if let Some(root) = root_of(packages, able, provisions, paused, title_of, &path)
                    {
                        return root;
                    }
                }
            }
        }
    }
    Root::Capability(
        unmet.capability.clone(),
        providers_say(packages, able, provisions, paused, title_of, path, unmet),
        unmet.default.clone(),
    )
}

/// What is actually missing at the end of a chain of waits: the package the
/// fix row offers to enable or retry, or the capability it offers to
/// install a provider of.
enum Root {
    /// A package the user can enable or retry, and why it cannot serve.
    Package(PackageIdentity, Unmet),
    /// A capability with no provider that can serve it: its name, what its
    /// providers that cannot serve say, and the default its use names, for
    /// the install row.
    Capability(String, String, Option<String>),
}

impl Root {
    /// What the fix row beside the reason offers.
    fn fix(&self) -> Fix {
        match self {
            Root::Package(identity, Unmet::Disabled) => Fix::Enable(identity.clone()),
            Root::Package(identity, Unmet::Paused) => Fix::Retry(identity.clone()),
            Root::Package(identity, Unmet::NotInstalled) => Fix::Install(identity.clone()),
            Root::Capability(capability, _, default) => Fix::InstallProvider {
                capability: capability.clone(),
                default: default.clone(),
            },
        }
    }

    /// How the chain of waits that ends here names what is actually
    /// missing, after "waits for": "Auth: Auth is disabled", or
    /// "acme:auth@1: no extension provides it".
    fn what(&self, title_of: &dyn Fn(&PackageIdentity) -> String) -> String {
        match self {
            Root::Package(identity, state) => {
                let title = title_of(identity);
                format!("{title}: {}", state.says(&title))
            }
            Root::Capability(capability, says, _) => format!("{capability}: {says}"),
        }
    }
}

/// Why `package` waits, from its unmet required dependencies and
/// capabilities: what its commands' rows say, what a call to its
/// operations is answered with, each requirement that is not met, and
/// what fixes the first of them.
fn reason(
    packages: &[InstalledPackage],
    able: &[bool],
    provisions: &[(String, String)],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    title_of: &dyn Fn(&PackageIdentity) -> String,
    package: &InstalledPackage,
) -> Reason {
    let unmets = unmet_of(packages, able, provisions, paused, title_of, package);
    let path = [package.identity.clone()];
    let requirements: Vec<Requirement> = unmets
        .iter()
        .map(|unmet| {
            let what = what_of(packages, able, provisions, paused, title_of, &path, unmet);
            Requirement {
                what,
                fix: fix_of(packages, able, provisions, paused, title_of, package, unmet),
            }
        })
        .collect();
    let whats: Vec<String> = requirements
        .iter()
        .map(|requirement| requirement.what.clone())
        .collect();
    let what = platform::join(&whats);
    Reason {
        row: format!("Needs {what}"),
        calling: format!("{} is waiting for {what}", package.title()),
        fix: requirements
            .first()
            .map(|requirement| requirement.fix.clone())
            .unwrap_or(Fix::Manage),
        requirements,
    }
}

/// What fixes one unmet requirement of `package`: the chain's root cause,
/// as the fix row beside the requirement shows it. The requirement itself
/// being what is actually missing, the fix is for it; waiting for another,
/// it is for what that one waits for.
fn fix_of(
    packages: &[InstalledPackage],
    able: &[bool],
    provisions: &[(String, String)],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    title_of: &dyn Fn(&PackageIdentity) -> String,
    package: &InstalledPackage,
    missing: &Missing,
) -> Fix {
    match missing {
        Missing::Dependency(unmet) => match unmet.state {
            // The dependency is what is actually missing.
            Some(Unmet::Disabled) => Fix::Enable(unmet.identity.clone()),
            Some(Unmet::Paused) => Fix::Retry(unmet.identity.clone()),
            Some(Unmet::NotInstalled) => Fix::Install(unmet.identity.clone()),
            // It waits for another: the chain's root is what to fix.
            None => {
                let path = [package.identity.clone(), unmet.identity.clone()];
                root_of(packages, able, provisions, paused, title_of, &path)
                    .map(|root| root.fix())
                    .unwrap_or(Fix::Manage)
            }
        },
        // The capability is what is actually missing: a provider of it.
        Missing::Capability(unmet) => {
            let path = [package.identity.clone()];
            capability_fix(
                root_of_capability(packages, able, provisions, paused, title_of, &path, unmet),
                unmet,
            )
        }
    }
}

/// What fixes an unmet capability requirement from its chain's root: the
/// root's fix, with the choice among two or more installed providers,
/// which the Settings Capabilities section offers, before installing one
/// nobody has.
fn capability_fix(root: Root, unmet: &UnmetCapability) -> Fix {
    match &root {
        Root::Capability(_, _, Some(_)) => root.fix(),
        Root::Capability(..) if unmet.providers.len() >= 2 => Fix::Choose,
        _ => root.fix(),
    }
}

/// Why one command of a package that runs waits, from the capability use
/// narrowed to it: what the command's row says, and what fixes it. The
/// package still serves its own operations and capabilities, so nothing
/// answers this reason but the command's row.
fn capability_reason(
    packages: &[InstalledPackage],
    able: &[bool],
    provisions: &[(String, String)],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    title_of: &dyn Fn(&PackageIdentity) -> String,
    package: &InstalledPackage,
    unmet: &UnmetCapability,
) -> Reason {
    let path = [package.identity.clone()];
    let what = capability_what(packages, able, provisions, paused, title_of, &path, unmet);
    let fix = capability_fix(
        root_of_capability(packages, able, provisions, paused, title_of, &path, unmet),
        unmet,
    );
    Reason {
        row: format!("Needs {what}"),
        calling: format!("{} is waiting for {what}", package.title()),
        requirements: vec![Requirement {
            what,
            fix: fix.clone(),
        }],
        fix,
    }
}

/// What one unmet requirement of a package is named by: "Greeter, which is
/// disabled", or, down a chain, "Notes Sync, which waits for Auth: Auth is
/// disabled" (naming what is actually missing, not the package that waits
/// for it); or, for a capability, "acme:translate@1: DeepL Translate is
/// disabled" or "acme:translate@1: no extension provides it".
fn what_of(
    packages: &[InstalledPackage],
    able: &[bool],
    provisions: &[(String, String)],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    title_of: &dyn Fn(&PackageIdentity) -> String,
    path: &[PackageIdentity],
    requirement: &Missing,
) -> String {
    match requirement {
        Missing::Dependency(unmet) => {
            let Some(state) = unmet.state else {
                // It waits for another: what is actually missing is at the
                // end of its chain, or beyond naming if the chain is a
                // cycle.
                let mut path = path.to_vec();
                path.push(unmet.identity.clone());
                return match root_of(packages, able, provisions, paused, title_of, &path) {
                    Some(root) => {
                        format!("{}, which waits for {}", unmet.title, root.what(title_of))
                    }
                    None => format!("{}, which is waiting for something else", unmet.title),
                };
            };
            format!("{}, which is {}", unmet.title, state.state())
        }
        Missing::Capability(unmet) => {
            capability_what(packages, able, provisions, paused, title_of, path, unmet)
        }
    }
}

/// What an unmet capability use is named by: the capability, then why no
/// provider can serve it.
fn capability_what(
    packages: &[InstalledPackage],
    able: &[bool],
    provisions: &[(String, String)],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    title_of: &dyn Fn(&PackageIdentity) -> String,
    path: &[PackageIdentity],
    unmet: &UnmetCapability,
) -> String {
    let says = providers_say(packages, able, provisions, paused, title_of, path, unmet);
    format!("{}: {says}", unmet.capability)
}

/// Why no provider of an unmet capability can serve it: each provider that
/// cannot, with why, or that none provides it at all. A provider waiting
/// for another names what is actually missing at the end of its chain.
fn providers_say(
    packages: &[InstalledPackage],
    able: &[bool],
    provisions: &[(String, String)],
    paused: &dyn Fn(&PackageIdentity) -> bool,
    title_of: &dyn Fn(&PackageIdentity) -> String,
    path: &[PackageIdentity],
    unmet: &UnmetCapability,
) -> String {
    if unmet.providers.is_empty() {
        return "no extension provides it".to_owned();
    }
    let says: Vec<String> = unmet
        .providers
        .iter()
        .map(|provider| {
            let mut path = path.to_vec();
            path.push(provider.identity.clone());
            match provider.cannot {
                Cannot::Disabled => format!("{} is disabled", provider.title),
                Cannot::Paused => format!("{} is paused", provider.title),
                Cannot::Here(ref reason) => format!("{}: {reason}", provider.title),
                // It waits for another: what is actually missing is at the
                // end of its chain, or beyond naming if the chain is a
                // cycle.
                Cannot::Waiting => {
                    match root_of(packages, able, provisions, paused, title_of, &path) {
                        Some(root) => {
                            format!(
                                "{}, which waits for {}",
                                provider.title,
                                root.what(title_of)
                            )
                        }
                        None => format!("{}, which is waiting for something else", provider.title),
                    }
                }
            }
        })
        .collect();
    platform::join(&says)
}
