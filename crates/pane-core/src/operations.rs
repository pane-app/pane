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
//! The runtime serves every call on one thread. A guest waiting for an
//! operation is suspended inside its own call, and the runtime serves the
//! operation in the meantime (see `Host::run_guest` in the runtime), so a
//! call chain never waits on itself. Each package in a chain is busy until
//! its call returns, whichever of its components serves it: a call that
//! would reach one again is refused, as is a chain deeper than
//! [`MAX_CALL_DEPTH`]. A call a guest makes while Pane is not running a call
//! of it has no frame to serve it, and is refused.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};
use wasmtime::component::{Accessor, HasData};

use crate::extension_data::{ExtensionData, PackageData};
use crate::packages::{
    InstalledPackage, ManifestOperation, PackageIdentity, SourceSpec, installed_as, paused_reason,
};
use crate::platform;
use crate::runtime::{CallError, GuestState, bindings};

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
    format!(
        "{title} is disabled; Pane does not enable it for a call, enable it in Manage extensions"
    )
}

/// A call a guest made, waiting for the runtime to serve it.
pub(crate) struct OperationCall {
    /// The component of the calling guest.
    pub caller: PathBuf,
    pub source: String,
    pub operation: String,
    pub version: u32,
    pub input: String,
    pub reply: oneshot::Sender<Result<String, OperationError>>,
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

/// A snapshot of the installed packages and their extension data.
#[derive(Default)]
pub(crate) struct Installed {
    pub packages: Vec<InstalledPackage>,
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
        // An identity names a package, not a version of it. A repository
        // may be written in any of its equivalent forms.
        let key = match SourceSpec::parse(source) {
            Ok(SourceSpec::Local(path)) if Path::new(&path).is_absolute() => {
                Some(source.to_owned())
            }
            Ok(SourceSpec::Npm(spec)) if spec.version.is_none() => Some(source.to_owned()),
            Ok(SourceSpec::Git(spec)) if spec.reference.is_none() => {
                Some(crate::packages::PackageIdentity::git(&spec.repository).key())
            }
            _ => None,
        };
        let Some(key) = key else {
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

    /// The identity of the installed package whose managed copy holds
    /// `component`.
    pub fn package_of(&self, component: &Path) -> Option<&PackageIdentity> {
        self.packages
            .iter()
            .find(|package| component.starts_with(&package.location))
            .map(|package| &package.identity)
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
                    source,
                    operation,
                    version,
                    input,
                    reply,
                })
                .map_err(|_| stopped())
        });
        let result = match sent {
            Ok(()) => response.await.unwrap_or_else(|_| Err(stopped())),
            Err(error) => Err(error),
        };
        result.map_err(operations::CallError::from)
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
