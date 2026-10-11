//! Operations: calls from one extension to an operation another installed
//! package publishes, routed by Pane.
//!
//! A package publishes operations under `operations` in its `pane.json`;
//! nothing else is callable, so a command is never an operation by accident.
//! A guest calls one with `pane:extension/operations.call`, naming the
//! target by its package identity as Pane shows it (`local:` and the
//! absolute folder it was installed from), the operation and the version it
//! was written for. Pane finds that identity among the installed packages,
//! never a title or a path relative to anything, refuses a missing, disabled or
//! incompatible target without enabling anything, starts the target's
//! instance only if it is not running, and passes the target's JSON result
//! or its error back.
//!
//! A capability (ADR 0041) is a second way to address the same calls:
//! `<namespace>:<name>@<major>`, such as `acme:translate@1`, a named,
//! versioned set of operations that any installed package may provide under
//! `provides` in its `pane.json` and another calls by that name with
//! `operations.call-capability`, declaring it under `uses`. Pane resolves
//! the call when it is made, against the packages as they are then: the
//! providers in the order Pane calls them — the user's chosen provider
//! first, as the launcher's record holds the choice (see
//! `capability_choices`), then the rest in install order — and the first
//! that can serve the call serves it: enabled, not paused, not waiting for
//! what it needs, built for this system, never the caller's own package,
//! which never serves its own use. While the chosen provider cannot serve,
//! the call falls back to the next provider that can, and returns to the
//! chosen one when it can serve again. The provider serves the
//! call as a published operation is served, with the operation qualified by
//! its capability, and the call obeys the same rules a call by identity
//! does. Until no provider can serve it: `not-found` when none is installed,
//! `disabled` when every provider is disabled, `unavailable` when every
//! provider is paused, waiting or for another system — each message naming
//! the capability. `operations.providers` answers which providers can serve
//! a capability now, for asking before calling. A use declared
//! `"use": "all"` may also call every provider at once with
//! `operations.call-every`, which answers each provider's identity, title
//! and result or error, skipping the providers that cannot serve and
//! answering an empty list when none can: a use of every provider never
//! makes a command wait for it.
//!
//! The runtime serves every call on one thread. A guest waiting for an
//! operation is suspended inside its own call, and the runtime serves the
//! operation in the meantime (see `Host::run_guest` in the runtime), so a
//! call chain never waits on itself. Each package in a chain is busy until
//! its call returns, whichever of its components serves it: a call that
//! would reach one again is refused, as is a chain deeper than
//! [`MAX_CALL_DEPTH`]. A call a guest makes while Pane is not running a call
//! of it has no frame to serve it, and is refused.
//!
//! The runtime serves other chains meanwhile (#136), and one instance runs
//! one call at a time: an operation call whose target is busy with another
//! chain's call waits for its turn. One that could only get it once its own
//! chain's calls returned (the chain holding the target waits, perhaps
//! through others, for a turn this chain holds) is refused instead of
//! waiting for ever.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};
use wasmtime::component::{Access, Accessor, HasData};

use crate::extension_data::{ExtensionData, PackageData};
use crate::launcher::capability_choices::CapabilityChoices;
use crate::packages::{
    InstalledPackage, ManifestOperation, ManifestProvides, ManifestUse, PackageIdentity,
    SourceSpec, installed_as, paused_reason,
};
use crate::platform;
use crate::runtime::{CallError, GuestState, bindings};
use crate::waiting::Waiting;

use bindings::pane::extension::operations;

/// The most calls a chain may hold at once, counting the first: a command
/// calling an operation that calls another is a chain of two.
pub const MAX_CALL_DEPTH: usize = 8;

/// The largest input or result an operation call carries, in bytes.
pub const MAX_OPERATION_JSON: usize = 1 << 20;

/// Why an operation call did not produce the operation's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OperationError {
    pub kind: OperationErrorKind,
    pub message: String,
}

/// The WIT `call-error-kind`: lifecycle failures apart from the operation's
/// own error ([`OperationErrorKind::Failed`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OperationErrorKind {
    NotFound,
    Disabled,
    Incompatible,
    Unavailable,
    Failed,
    Crashed,
    Refused,
}

impl OperationError {
    fn new(kind: OperationErrorKind, message: impl Into<String>) -> OperationError {
        OperationError {
            kind,
            message: message.into(),
        }
    }

    /// Why the target's component could not serve the call, from the
    /// runtime's error when starting or calling it.
    pub(crate) fn from_call(title: &str, error: CallError) -> OperationError {
        use OperationErrorKind::*;
        let (kind, message) = match error {
            CallError::Guest(message) => (Failed, message),
            CallError::Disabled => (Disabled, disabled(title)),
            CallError::Replaced => (
                Unavailable,
                format!("{title} was reloaded or updated while serving the call; call it again"),
            ),
            CallError::Uninstalled => (
                Unavailable,
                format!("{title} was uninstalled while serving the call"),
            ),
            CallError::Paused => (Unavailable, paused_reason(title)),
            CallError::Trap(reason) => (Crashed, format!("{title} crashed: {reason}")),
            // The operation kinds have no kind of its own: the target failed
            // while running, as a crash does (provisional).
            CallError::Unresponsive(reason) => {
                (Crashed, format!("{title} stopped responding: {reason}"))
            }
            CallError::RuntimeUnavailable(_) => (Unavailable, format!("{title}: {error}")),
            CallError::Load(_)
            | CallError::Incompatible(_)
            | CallError::Interface(_)
            | CallError::OlderApiShape(_) => (Incompatible, format!("{title}: {error}")),
            // A form's rejection answers only `submit-form`, a live
            // instance (started just before the call) is never missing, only
            // root search's own calls are cancelled, and only a command's
            // tree and event answers are read.
            CallError::Form(_)
            | CallError::ViewClosed
            | CallError::Cancelled
            | CallError::Unreadable(_) => {
                unreachable!("an operation call cannot end with {error:?}")
            }
        };
        OperationError { kind, message }
    }

    pub(crate) fn refused(message: impl Into<String>) -> OperationError {
        OperationError::new(OperationErrorKind::Refused, message)
    }
}

impl fmt::Display for OperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

fn disabled(title: &str) -> String {
    format!("{title} is disabled; Pane does not enable it for a call, enable it in Settings")
}

/// `reason` up to its first ";": a fallback note keeps the state ("is
/// paused after an error") and leaves the advice that follows ("retry it
/// in Settings") to the page that fixes the provider.
fn cut_advice(reason: &str) -> String {
    reason
        .split_once(';')
        .map_or_else(|| reason.to_owned(), |(kept, _)| kept.to_owned())
}

/// A call a guest made, waiting for the runtime to serve it.
pub(crate) struct OperationCall {
    /// The component of the calling guest.
    pub caller: PathBuf,
    /// How the call names the package that serves it.
    pub addressed: Addressed,
    /// The operation called; for a capability call, unqualified: the
    /// provider sees it qualified by its capability
    /// ([`Addressed::operation_name`]).
    pub operation: String,
    pub input: String,
    pub reply: oneshot::Sender<OperationAnswer>,
}

/// How a call names the package that serves it.
pub(crate) enum Addressed {
    /// The package's source as the caller wrote it (its identity, or the id
    /// of a dependency the caller's `pane.json` declares), and the version
    /// of the operation it calls.
    Package { source: String, version: u32 },
    /// A capability's name, which any installed package may provide.
    Capability { capability: String },
    /// A capability's name called on every provider that can serve it, as
    /// `call-every` addresses it: a use declared `"use": "all"`.
    Every { capability: String },
}

impl Addressed {
    /// The operation name the serving component is called with: the
    /// operation as the caller wrote it, or, for a capability call, the
    /// operation qualified by its capability (`acme:translate@1/translate`),
    /// so one component can tell the two apart.
    pub(crate) fn operation_name(&self, operation: &str) -> String {
        match self {
            Addressed::Package { .. } => operation.to_owned(),
            Addressed::Capability { capability } | Addressed::Every { capability } => {
                format!("{capability}/{operation}")
            }
        }
    }

    /// Whether this call is answered by every provider, each with its own
    /// answer (`call-every`), rather than by one target.
    pub(crate) fn is_every(&self) -> bool {
        matches!(self, Addressed::Every { .. })
    }
}

/// The answer to a call a guest made: one target's, or every provider's.
pub(crate) enum OperationAnswer {
    /// The answer of a call to one target.
    One(Result<String, OperationError>),
    /// The answers of a call to every provider of a capability, or why the
    /// call as a whole could not be made.
    Every(Result<Vec<ProviderAnswer>, OperationError>),
}

impl OperationCall {
    /// The answer saying the call could not be made at all, of this call's
    /// own shape: a stranded call, one whose runtime stopped, answers with
    /// it.
    pub(crate) fn unanswered(&self, error: OperationError) -> OperationAnswer {
        if self.addressed.is_every() {
            OperationAnswer::Every(Err(error))
        } else {
            OperationAnswer::One(Err(error))
        }
    }
}

/// One provider's answer to a fan-out call, as `call-every` answers it:
/// the provider's identity, its title, and its result or error.
#[derive(Clone, Debug)]
pub(crate) struct ProviderAnswer {
    pub identity: PackageIdentity,
    pub title: String,
    pub answer: Result<String, OperationError>,
}

/// The installed package serving a call, as resolved by a [`Directory`].
pub(crate) struct Target {
    pub identity: PackageIdentity,
    /// The package's display title, for explanations.
    pub title: String,
    pub component: PathBuf,
    /// The package's own extension data: the target's, never the caller's.
    pub data: Option<PackageData>,
}

/// The installed packages as the launcher currently has them, which the
/// runtime resolves calls against. The launcher's latest choices (enabled
/// or disabled, installed or updated) apply at once.
pub(crate) type Directory = Arc<dyn Fn() -> Installed + Send + Sync>;

/// Why a provider cannot serve a capability call now, when it cannot (see
/// [`Installed::cannot_serve`]).
enum Cannot {
    /// The user disabled the package.
    Disabled,
    /// It is paused, waiting, broken or for another system; the string says
    /// which, as the call's message shows it.
    Unavailable(String),
}

/// A snapshot of the installed packages and their extension data, with
/// which of their capability providers the user chose (see
/// `capability_choices`).
#[derive(Default)]
pub(crate) struct Installed {
    pub packages: Vec<InstalledPackage>,
    /// Which of them wait for a required dependency that cannot serve
    /// them, with why (see `waiting`): a package waiting as a whole answers
    /// its operations `unavailable`.
    pub waiting: Waiting,
    /// Which capabilities each package provides at run time, as its code
    /// holds provisions for them (`(owner key, capability)`, #158): a
    /// `provides` entry marked `atRunTime` makes the package a provider
    /// only while it holds one. From the runtime's registry of what the
    /// packages' guests registered.
    pub provisions: Vec<(String, String)>,
    /// Which of them Pane paused after they failed, by identity: a paused
    /// provider cannot serve a call now.
    pub paused: Vec<PackageIdentity>,
    /// Which provider the user chose for each capability, by identity key.
    pub chosen: CapabilityChoices,
    pub data: Option<ExtensionData>,
}

impl Installed {
    /// Resolves a call from `caller`'s component to the package with
    /// identity `source`, or to the dependency its package declares with id
    /// `source`, and its `operation` at `version`.
    pub fn resolve(
        &self,
        caller: &Path,
        source: &str,
        operation: &str,
        version: u32,
    ) -> Result<Target, OperationError> {
        use OperationErrorKind::*;
        let dependency;
        let source = if source.contains(':') {
            source
        } else {
            dependency = self.dependency(caller, source, operation, version)?;
            dependency.as_str()
        };
        let Some(key) = identity_key(source) else {
            return Err(OperationError::new(
                NotFound,
                format!(
                    "`{source}` is not a package identity; use `local:` followed by the \
                     absolute folder path Pane shows for the package, `npm:` followed by its \
                     npm package name, or `git:` followed by its repository"
                ),
            ));
        };
        let Some(package) = self
            .packages
            .iter()
            .find(|package| package.identity.key() == key)
        else {
            return Err(OperationError::new(
                NotFound,
                format!("no installed extension has the source {source}"),
            ));
        };
        let title = package.title();
        if !package.enabled {
            return Err(OperationError::new(Disabled, disabled(&title)));
        }
        // A package waiting for what its package requires answers what it
        // waits for (see `waiting`): waiting ends no generation, but its
        // code runs nothing a call would start.
        if let Some(reason) = self.waiting.reason(&package.identity) {
            return Err(OperationError::new(Unavailable, reason.calling.clone()));
        }
        let manifest = match &package.manifest {
            Ok(manifest) => manifest,
            Err(error) => {
                return Err(OperationError::new(
                    Incompatible,
                    format!("{title} cannot load: {error}"),
                ));
            }
        };
        let Some(published) = manifest.operations.iter().find(|o| o.id == operation) else {
            let published: Vec<String> = manifest
                .operations
                .iter()
                .map(|o| format!("`{}`", o.id))
                .collect();
            let offers = match published.as_slice() {
                [] => "it publishes no operations".to_owned(),
                names => format!("it publishes {}", platform::join(names)),
            };
            return Err(OperationError::new(
                NotFound,
                format!("{title} does not publish an operation `{operation}`; {offers}"),
            ));
        };
        if published.version != version {
            return Err(OperationError::new(
                Incompatible,
                format!(
                    "{title} publishes `{operation}` at version {}, not version {version}",
                    published.version
                ),
            ));
        }
        let unavailable = platform::unavailable(manifest.platforms.as_deref(), "this package")
            .or_else(|| platform::unavailable(published.platforms.as_deref(), "this operation"));
        if let Some(reason) = unavailable {
            return Err(OperationError::new(
                Unavailable,
                format!("{title}: {reason}"),
            ));
        }
        Ok(Target {
            component: package.location.join(&published.component),
            data: self
                .data
                .as_ref()
                .map(|data| data.owned_by(&package.identity)),
            identity: package.identity.clone(),
            title,
        })
    }

    /// The identity, as a source, of the dependency that the package of
    /// `caller`'s component declares with id `id`, if it is installed; else
    /// why the call cannot reach it.
    fn dependency(
        &self,
        caller: &Path,
        id: &str,
        operation: &str,
        version: u32,
    ) -> Result<String, OperationError> {
        use OperationErrorKind::*;
        let not_identity = || {
            OperationError::new(
                NotFound,
                format!(
                    "`{id}` is not a package identity; use `local:` followed by the absolute \
                     folder path Pane shows for the package, `npm:` followed by its npm package \
                     name, `git:` followed by its repository, or the id of a dependency the \
                     caller's pane.json declares"
                ),
            )
        };
        let caller = self
            .packages
            .iter()
            .find(|package| caller.starts_with(&package.location))
            .ok_or_else(not_identity)?;
        let manifest = caller.manifest.as_ref().map_err(|_| not_identity())?;
        let title = caller.title();
        let Some(declared) = manifest.dependency(id) else {
            let ids: Vec<String> = manifest
                .dependencies
                .iter()
                .map(|dependency| format!("`{}`", dependency.id))
                .collect();
            let declares = match ids.as_slice() {
                [] => "it declares none".to_owned(),
                ids => format!("it declares {}", platform::join(ids)),
            };
            return Err(OperationError::new(
                NotFound,
                format!(
                    "{title} declares no dependency `{id}` in its pane.json, and `{id}` is not \
                     a package identity; {declares}"
                ),
            ));
        };
        // A dependency id reaches only what the caller declared it calls
        // there, so the declaration is what installing checked.
        if !declared.calls(operation, version) {
            let declared_calls: Vec<String> = declared
                .operations
                .iter()
                .map(|o| format!("`{}` version {}", o.id, o.version))
                .collect();
            return Err(OperationError::refused(format!(
                "{title} declares that it calls {} through its dependency `{id}`, not \
                 `{operation}` version {version}; declare it in its pane.json to call it",
                platform::join(&declared_calls)
            )));
        }
        let Some(recorded) = caller.dependency_identity(id) else {
            return Err(OperationError::new(
                Unavailable,
                format!(
                    "{title}'s dependency `{id}` from {} is not a folder Pane can name",
                    declared.source
                ),
            ));
        };
        // Recorded before its folder existed, it may since resolve to
        // another spelling, such as through a link.
        let identity = std::iter::once(recorded.clone())
            .chain(recorded.resolved_again())
            .find(|identity| installed_as(&self.packages, identity).is_some());
        if let Some(identity) = identity {
            return Ok(identity.key());
        }
        let identity = recorded;
        let message = if let Some(only_on) = declared.only_on() {
            return Err(OperationError::new(
                Unavailable,
                format!("{title} uses its dependency `{id}` {only_on}, and it is not installed"),
            ));
        } else if declared.required {
            format!(
                "{title} requires `{id}` from {identity}, which is not installed; install \
                 {title} again to install it"
            )
        } else {
            format!(
                "{title}'s optional dependency `{id}` from {identity} is not installed; \
                 install it to use it"
            )
        };
        Err(OperationError::new(NotFound, message))
    }

    /// Resolves a call from `caller`'s component to `operation` of the
    /// capability `capability`, as the packages are now: the first provider
    /// in the order Pane calls them that can serve the call — the user's
    /// chosen provider first, then install order — enabled, not paused, not
    /// waiting for what it needs, and built for this system, never the
    /// caller's own package, which never serves its own use: while the
    /// chosen provider cannot serve, the call falls back to the next
    /// provider that can. Refused when the caller's `pane.json` declares
    /// no use of the capability, or not this operation; `not-found` when no
    /// other installed package provides it; `disabled` when every provider
    /// is disabled; `unavailable`, naming the capability and each
    /// provider's reason, when none can serve it.
    pub fn resolve_capability(
        &self,
        caller: &Path,
        capability: &str,
        operation: &str,
    ) -> Result<Target, OperationError> {
        use OperationErrorKind::*;
        // The caller's own manifest declares the use, and the package it
        // must never be routed to (a package never serves its own use).
        self.declared_use(caller, capability, operation)?;
        let candidates = self.call_order(Some(caller), capability, Some(operation));
        // The first provider in the order Pane calls them that can serve
        // the call does; the others are remembered for the message when
        // none can.
        let mut unserving: Vec<String> = Vec::new();
        let mut disabled: Vec<String> = Vec::new();
        for (package, entry) in &candidates {
            let title = package.title();
            match self.cannot_serve(package) {
                None => {
                    match platform::unavailable(entry.platforms.as_deref(), "this capability") {
                        None => {
                            return Ok(Target {
                                component: package.location.join(&entry.component),
                                data: self
                                    .data
                                    .as_ref()
                                    .map(|data| data.owned_by(&package.identity)),
                                identity: package.identity.clone(),
                                title,
                            });
                        }
                        Some(reason) => unserving.push(format!("{title}: {reason}")),
                    }
                }
                // Disabled is a class of its own: every provider disabled
                // is a `disabled` error, naming what to enable.
                Some(Cannot::Disabled) => disabled.push(title),
                Some(Cannot::Unavailable(reason)) => unserving.push(reason),
            }
        }
        if candidates.is_empty() {
            return Err(OperationError::new(
                NotFound,
                self.missing_provider(caller, capability, operation),
            ));
        }
        if unserving.is_empty() {
            return Err(OperationError::new(
                Disabled,
                format!(
                    "every installed extension providing `{capability}` is disabled ({}); \
                     enable one in Settings",
                    platform::join(&disabled)
                ),
            ));
        }
        let mut all: Vec<String> = disabled
            .iter()
            .map(|title| format!("{title} is disabled"))
            .collect();
        all.extend(unserving);
        Err(OperationError::new(
            Unavailable,
            format!(
                "no installed extension providing `{capability}` can serve it now: {}",
                platform::join(&all)
            ),
        ))
    }

    /// Resolves a fan-out call from `caller`'s component to `operation` of
    /// the capability `capability`, as the packages are now: every provider
    /// that can serve the call — enabled, not paused, not waiting for what
    /// it needs, and built for this system — never the caller's own package,
    /// which never serves its own use, each with the operation it calls.
    /// Refused when the caller's `pane.json` declares no use of the
    /// capability, or the use is not `"use": "all"`; with no provider that
    /// can serve, the answer is an empty list, not an error: a use of every
    /// provider never makes a command wait for it.
    ///
    /// The chosen provider comes first, then the others in install order
    /// (#154).
    pub fn resolve_every(
        &self,
        caller: &Path,
        capability: &str,
        operation: &str,
    ) -> Result<Vec<Target>, OperationError> {
        let declared = self.declared_use(caller, capability, operation)?;
        if !declared.use_all {
            return Err(OperationError::refused(format!(
                "{} declares that it calls `{capability}` with \"use\": \"one\", so one provider \
                 serves each call; declare \"use\": \"all\" in its pane.json to call every \
                 provider",
                self.caller_of(caller).title()
            )));
        }
        Ok(self
            .call_order(Some(caller), capability, Some(operation))
            .into_iter()
            .filter(|(package, entry)| {
                self.cannot_serve(package).is_none()
                    && platform::unavailable(entry.platforms.as_deref(), "this capability")
                        .is_none()
            })
            .map(|(package, entry)| Target {
                identity: package.identity.clone(),
                title: package.title(),
                component: package.location.join(&entry.component),
                data: self
                    .data
                    .as_ref()
                    .map(|data| data.owned_by(&package.identity)),
            })
            .collect())
    }

    /// The use of `capability` the manifest of `caller`'s package declares,
    /// checked for `operation`: the caller's own package, whose use Pane
    /// checks, and whose own provision never serves it.
    fn declared_use<'a>(
        &'a self,
        caller: &Path,
        capability: &str,
        operation: &str,
    ) -> Result<&'a ManifestUse, OperationError> {
        use OperationErrorKind::*;
        let own = self
            .packages
            .iter()
            .find(|package| caller.starts_with(&package.location))
            .ok_or_else(|| {
                OperationError::refused(
                    "capabilities are called by an installed extension's code, and this \
                     component belongs to no installed extension",
                )
            })?;
        let title = own.title();
        let manifest = match &own.manifest {
            Ok(manifest) => manifest,
            Err(error) => {
                return Err(OperationError::new(
                    Incompatible,
                    format!("{title} cannot load: {error}"),
                ));
            }
        };
        let Some(declared) = manifest
            .uses
            .iter()
            .find(|used| used.capability == capability)
        else {
            return Err(OperationError::refused(format!(
                "{title} declares no use of `{capability}` in its pane.json; declare it in \
                 `uses` to call it"
            )));
        };
        // A use reaches only the operations it declares, as a dependency's
        // id does, so what installing checked is what the code calls.
        if !declared.operations.iter().any(|called| called == operation) {
            let called: Vec<String> = declared
                .operations
                .iter()
                .map(|called| format!("`{called}`"))
                .collect();
            return Err(OperationError::refused(format!(
                "{title} declares that it calls {} through `{capability}`, not `{operation}`; \
                 declare it in its pane.json to call it",
                platform::join(&called)
            )));
        }
        Ok(declared)
    }

    /// The installed package whose managed copy holds `caller`'s component.
    fn caller_of(&self, caller: &Path) -> &InstalledPackage {
        self.packages
            .iter()
            .find(|package| caller.starts_with(&package.location))
            .expect("the caller was found above")
    }

    /// Why no provider serves: what a call to a capability nobody provides
    /// is answered with. The caller's own package providing it is a case of
    /// its own, since a package never serves its own use.
    fn missing_provider(&self, caller: &Path, capability: &str, operation: &str) -> String {
        let provides = |package: &InstalledPackage| {
            package.manifest.as_ref().is_ok_and(|manifest| {
                manifest
                    .provides
                    .iter()
                    .any(|provides| provides.capability == capability)
            })
        };
        let others: Vec<String> = self
            .packages
            .iter()
            .filter(|package| !caller.starts_with(&package.location) && provides(package))
            .map(InstalledPackage::title)
            .collect();
        let own = self
            .packages
            .iter()
            .find(|package| caller.starts_with(&package.location))
            .filter(|package| provides(package))
            .map(InstalledPackage::title);
        if others.is_empty() {
            return match own {
                Some(title) => format!(
                    "no installed extension other than {title} provides `{capability}`, and a \
                     package never serves its own use"
                ),
                None => format!("no installed extension provides `{capability}`"),
            };
        }
        // Others provide the capability, but not this operation.
        format!(
            "no installed extension provides `{capability}` with a `{operation}` operation \
             ({} provide the capability)",
            platform::join(&others)
        )
    }

    /// The installed packages providing `capability`, in install order,
    /// with the manifest entry that serves it, excluding the package of
    /// `caller`'s component: a package never serves its own use. With
    /// `operation`, only entries naming that operation answer. A `None`
    /// `caller` lists every provider, as Settings' Capabilities section
    /// does; a `None` `operation` lists every provider of the capability.
    fn providers_of<'a>(
        &'a self,
        caller: Option<&Path>,
        capability: &str,
        operation: Option<&str>,
    ) -> Vec<(&'a InstalledPackage, &'a ManifestProvides)> {
        let own = caller.and_then(|caller| {
            self.packages
                .iter()
                .find(|package| caller.starts_with(&package.location))
        });
        self.packages
            .iter()
            .filter(|package| own.is_none_or(|own| package.identity != own.identity))
            .filter_map(|package| {
                let manifest = package.manifest.as_ref().ok()?;
                let entry = manifest
                    .provides
                    .iter()
                    .find(|entry| entry.capability == capability)?;
                let serves = match operation {
                    Some(operation) => entry.operations.iter().any(|served| served == operation),
                    None => true,
                };
                // An entry marked `atRunTime` makes the package a provider
                // only while its code holds a provision for it.
                let provisioned = !entry.at_run_time
                    || self.provisions.iter().any(|(owner, provided)| {
                        *provided == capability && *owner == package.identity.key()
                    });
                (serves && provisioned).then_some((package, entry))
            })
            .collect()
    }

    /// The providers of `capability` in the order Pane calls them: the
    /// user's chosen provider first, as this snapshot holds the choice,
    /// then the rest in install order. [`providers_of`]'s caller, exclusion
    /// and operation apply; a chosen provider that is not among them (its
    /// package uninstalled, or the caller's own) changes nothing.
    pub(crate) fn call_order<'a>(
        &'a self,
        caller: Option<&Path>,
        capability: &str,
        operation: Option<&str>,
    ) -> Vec<(&'a InstalledPackage, &'a ManifestProvides)> {
        let mut providers = self.providers_of(caller, capability, operation);
        if let Some(chosen) = self.chosen.provider_of(capability)
            && let Some(at) = providers
                .iter()
                .position(|(package, _)| package.identity.key() == chosen)
            && at > 0
        {
            let chosen = providers.remove(at);
            providers.insert(0, chosen);
        }
        providers
    }

    /// The providers of `capability` that can serve a call now — enabled,
    /// not paused, not waiting for what they need, and built for this system
    /// — in the order Pane calls them (the chosen provider first, then
    /// install order), with their source and title, as
    /// `operations.providers` answers. The package of `caller`'s component
    /// is never among them.
    pub fn capable_providers(&self, caller: &Path, capability: &str) -> Vec<(String, String)> {
        self.call_order(Some(caller), capability, None)
            .into_iter()
            .filter(|(package, entry)| {
                self.cannot_serve(package).is_none()
                    && platform::unavailable(entry.platforms.as_deref(), "this capability")
                        .is_none()
            })
            .map(|(package, _)| (package.identity.key(), package.title()))
            .collect()
    }

    /// Why `package` cannot serve a call now, or `None` when it can: it is
    /// disabled, or paused, or waiting for what it needs, or its copy
    /// cannot load. A package for another system is a provider that cannot
    /// serve here, but only its manifest's `provides` entry knows that, so
    /// the caller checks it beside this.
    fn cannot_serve(&self, package: &InstalledPackage) -> Option<Cannot> {
        let title = package.title();
        if !package.enabled {
            return Some(Cannot::Disabled);
        }
        if let Err(error) = &package.manifest {
            return Some(Cannot::Unavailable(format!("{title} cannot load: {error}")));
        }
        if self.paused.contains(&package.identity) {
            return Some(Cannot::Unavailable(paused_reason(&title)));
        }
        if let Some(reason) = self.waiting.reason(&package.identity) {
            return Some(Cannot::Unavailable(reason.calling.clone()));
        }
        if let Some(reason) = platform::unavailable(
            package
                .manifest
                .as_ref()
                .ok()
                .and_then(|manifest| manifest.platforms.as_deref()),
            "this package",
        ) {
            return Some(Cannot::Unavailable(format!("{title}: {reason}")));
        }
        None
    }

    /// Why `package`, providing a capability through `entry`, cannot serve
    /// a call now, in the Settings Capabilities section's fallback note's
    /// words ("<title> is disabled", "<title> is paused", "<title> is
    /// waiting for <what>"); `None` when it can. The advice the full
    /// reasons carry ("retry it in Settings") stays with the page that
    /// fixes the provider.
    pub(crate) fn cannot_serve_note(
        &self,
        package: &InstalledPackage,
        entry: &ManifestProvides,
    ) -> Option<String> {
        let title = package.title();
        match self.cannot_serve(package) {
            // The note's wording: "<chosen> is disabled; using <other>".
            Some(Cannot::Disabled) => Some(format!("{title} is disabled")),
            Some(Cannot::Unavailable(reason)) => Some(cut_advice(&reason)),
            None => platform::unavailable(entry.platforms.as_deref(), "this capability")
                .map(|reason| format!("{title}: {reason}")),
        }
    }

    /// The identity of the installed package whose managed copy holds
    /// `component`.
    pub fn package_of(&self, component: &Path) -> Option<&PackageIdentity> {
        self.packages
            .iter()
            .find(|package| component.starts_with(&package.location))
            .map(|package| &package.identity)
    }
}

/// The key of the package identity `source` names, as an operation call or
/// a launch writes it: `local:` and an absolute folder, `npm:` and a
/// package name, or `git:` and a repository; `None` for anything else. An
/// identity names a package, not a version of it, and a repository may be
/// written in any of its equivalent forms.
pub(crate) fn identity_key(source: &str) -> Option<String> {
    match SourceSpec::parse(source) {
        Ok(SourceSpec::Local(path)) if Path::new(&path).is_absolute() => Some(source.to_owned()),
        Ok(SourceSpec::Npm(spec)) if spec.version.is_none() => Some(source.to_owned()),
        Ok(SourceSpec::Git(spec)) if spec.reference.is_none() => {
            Some(crate::packages::PackageIdentity::git(&spec.repository).key())
        }
        _ => None,
    }
}

/// Checks an operation's input or result: JSON text within Pane's limit.
pub(crate) fn check_json(text: &str, what: &str) -> Result<(), OperationError> {
    if text.len() > MAX_OPERATION_JSON {
        return Err(OperationError::refused(format!(
            "the {what} is {} bytes; at most {MAX_OPERATION_JSON} are passed",
            text.len()
        )));
    }
    serde_json::from_str::<serde::de::IgnoredAny>(text)
        .map(|_| ())
        .map_err(|error| OperationError::refused(format!("the {what} is not JSON: {error}")))
}

/// The host side of `pane:extension/operations`: hands the call to the
/// runtime, which serves it while the calling guest waits.
pub(crate) struct Calls;

impl HasData for Calls {
    type Data<'a> = &'a mut GuestState;
}

impl operations::Host for GuestState {}

impl<T> operations::HostWithStore<T> for Calls {
    async fn call(
        accessor: &Accessor<T, Self>,
        source: String,
        operation: String,
        version: u32,
        input: String,
    ) -> Result<String, operations::CallError> {
        let answer = send(
            accessor,
            Addressed::Package { source, version },
            operation,
            input,
        )
        .await
        .map_err(operations::CallError::from)?;
        match answer {
            OperationAnswer::One(result) => result.map_err(operations::CallError::from),
            // Only a call addressed to every provider is answered with
            // every provider's answers.
            OperationAnswer::Every(_) => unreachable!("a call to one target answered with many"),
        }
    }

    async fn call_capability(
        accessor: &Accessor<T, Self>,
        capability: String,
        operation: String,
        input: String,
    ) -> Result<String, operations::CallError> {
        let answer = send(
            accessor,
            Addressed::Capability { capability },
            operation,
            input,
        )
        .await
        .map_err(operations::CallError::from)?;
        match answer {
            OperationAnswer::One(result) => result.map_err(operations::CallError::from),
            // Only a call addressed to every provider is answered with
            // every provider's answers.
            OperationAnswer::Every(_) => unreachable!("a call to one target answered with many"),
        }
    }

    async fn call_every(
        accessor: &Accessor<T, Self>,
        capability: String,
        operation: String,
        input: String,
    ) -> Result<Vec<operations::ProviderAnswer>, operations::CallError> {
        let answer = send(accessor, Addressed::Every { capability }, operation, input)
            .await
            .map_err(operations::CallError::from)?;
        match answer {
            OperationAnswer::Every(result) => result
                .map(|answers| {
                    answers
                        .into_iter()
                        .map(|answer| operations::ProviderAnswer {
                            provider: answer.identity.key(),
                            title: answer.title,
                            answer: answer.answer.map_err(operations::CallError::from),
                        })
                        .collect()
                })
                .map_err(operations::CallError::from),
            // Only a call addressed to one target is answered with one
            // target's answer.
            OperationAnswer::One(_) => unreachable!("a fan-out answered with one answer"),
        }
    }

    fn providers(mut host: Access<'_, T, Self>, capability: String) -> Vec<operations::Provider> {
        let state = host.get();
        let installed = state.installed();
        let caller = state.component.clone();
        installed
            .capable_providers(&caller, &capability)
            .into_iter()
            .map(|(source, title)| operations::Provider { source, title })
            .collect()
    }
}

/// Sends `call` — by identity, by capability, or to every provider of one —
/// to the runtime, which serves it while the calling guest waits, and
/// awaits its answer, in the shape the call was sent with.
async fn send<T>(
    accessor: &Accessor<T, Calls>,
    addressed: Addressed,
    operation: String,
    input: String,
) -> Result<OperationAnswer, OperationError> {
    let every = addressed.is_every();
    let (reply, response) = oneshot::channel();
    let sent = accessor.with(|mut view| {
        let state = view.get();
        if !state.serving {
            return Err(outside_a_call());
        }
        // Stopped code starts no more work.
        if let Some(end) = state.stopped() {
            return Err(OperationError::refused(crate::runtime::stopped_code(end)));
        }
        // The operation is served by the runtime's loop while this
        // waits, not inside this host call.
        state
            .calls
            .send(OperationCall {
                caller: state.component.clone(),
                addressed,
                operation,
                input,
                reply,
            })
            .map_err(|_| stopped())
    });
    match sent {
        // The runtime answers in the shape the call was sent with; one
        // that stopped without answering leaves the reply dropped.
        Ok(()) => Ok(response.await.unwrap_or_else(|_| {
            if every {
                OperationAnswer::Every(Err(stopped()))
            } else {
                OperationAnswer::One(Err(stopped()))
            }
        })),
        Err(error) => Err(error),
    }
}

/// Why a call made while Pane is not running a call of the guest, such as
/// while its component starts or after its call returned, is refused.
pub(crate) fn outside_a_call() -> OperationError {
    OperationError::refused("operations can only be called while serving a Pane call")
}

fn stopped() -> OperationError {
    OperationError::new(
        OperationErrorKind::Unavailable,
        "the extension runtime has stopped",
    )
}

/// Where guests send their calls, and where the runtime receives them.
pub(crate) fn channel() -> (
    mpsc::UnboundedSender<OperationCall>,
    mpsc::UnboundedReceiver<OperationCall>,
) {
    mpsc::unbounded_channel()
}

impl From<OperationError> for operations::CallError {
    fn from(error: OperationError) -> operations::CallError {
        use operations::CallErrorKind as Wit;
        let kind = match error.kind {
            OperationErrorKind::NotFound => Wit::NotFound,
            OperationErrorKind::Disabled => Wit::Disabled,
            OperationErrorKind::Incompatible => Wit::Incompatible,
            OperationErrorKind::Unavailable => Wit::Unavailable,
            OperationErrorKind::Failed => Wit::Failed,
            OperationErrorKind::Crashed => Wit::Crashed,
            OperationErrorKind::Refused => Wit::Refused,
        };
        operations::CallError {
            kind,
            message: error.message,
        }
    }
}

/// "Operations: <id> (version <n>), …" for a package preview; `None` when
/// the package publishes none.
pub(crate) fn describe(operations: &[ManifestOperation]) -> Option<String> {
    if operations.is_empty() {
        return None;
    }
    let names: Vec<String> = operations
        .iter()
        .map(|operation| format!("{} (version {})", operation.id, operation.version))
        .collect();
    Some(format!("Operations: {}", names.join(", ")))
}
