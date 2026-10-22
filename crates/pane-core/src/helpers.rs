//! Native helpers: prebuilt programs a package ships for each operating
//! system and processor it supports, which Pane runs for its commands
//! through `pane:extension/helpers` (a WASI 0.3 guest cannot start a
//! program; [ADR 0014](../../../docs/adr/0014-optional-native-extension-helpers.md)).
//!
//! A package declares each helper under `helpers` in its `pane.json`, with
//! one file per target (`linux-x86_64`, `macos-aarch64`, `windows-x86_64`,
//! ...). Pane runs the file for the system it runs on, never compiles
//! anything, and explains a helper with no file for this target, a missing
//! file, a link, a file outside the package, a script or a program for
//! another system (read from its header: ELF, Mach-O or PE).
//!
//! Pane owns the helper's process ([`runner`]): it passes the input and
//! output, and ends the process (its supervising thread reaps it) when the
//! guest drops the call, when the Pane call that started it returns, when
//! the instance goes, when the package's generation ends or when Pane
//! quits, whichever comes first. Processes the helper starts itself are not
//! tracked.

use std::path::{Path, PathBuf};

use pane_target::Target;
use wasmtime::component::{Accessor, HasData};

use crate::operations::Installed;
use crate::packages::ManifestHelper;
use crate::platform;
use crate::runtime::{GuestState, bindings};
use runner::{HelperError, HelperErrorKind};

use bindings::pane::extension::helpers as wit;

pub(crate) mod runner;

/// The host side of `pane:extension/helpers`: starts the helper for the
/// calling guest and waits for it, ending its process if the guest drops
/// the call.
pub(crate) struct Runs;

impl HasData for Runs {
    type Data<'a> = &'a mut GuestState;
}

impl wit::Host for GuestState {}

impl<T> wit::HostWithStore<T> for Runs {
    async fn run(
        accessor: &Accessor<T, Self>,
        helper: String,
        args: Vec<String>,
        input: String,
    ) -> Result<String, wit::HelperError> {
        let (started, watch) = accessor.with(|mut view| {
            let state = view.get();
            (state.start_helper(helper, args, input), state.watch())
        });
        // Waiting for the helper is not the guest's computing, nor a hang:
        // the runtime thread only awaits it. Dropped here if the guest
        // cancels the call: the process ends.
        let result =
            crate::runtime::deadlines::hosted(watch, async move { started.await?.finish().await })
                .await;
        result.map_err(wit::HelperError::from)
    }
}

/// The one mapping of Pane's error kinds to the WIT's.
impl From<HelperError> for wit::HelperError {
    fn from(error: HelperError) -> wit::HelperError {
        use wit::HelperErrorKind as Wit;
        let kind = match error.kind {
            HelperErrorKind::NotFound => Wit::NotFound,
            HelperErrorKind::Unavailable => Wit::Unavailable,
            HelperErrorKind::Failed => Wit::Failed,
            HelperErrorKind::Refused => Wit::Refused,
        };
        wit::HelperError {
            kind,
            message: error.message,
        }
    }
}

/// The checked file to run for the helper `name` of the installed package
/// whose managed copy holds `component`: the package must declare it, ship
/// a file for this system, and that file must pass [`runner::check_file`].
pub(crate) fn find(
    installed: &Installed,
    component: &Path,
    name: &str,
) -> Result<PathBuf, HelperError> {
    use HelperErrorKind::*;
    let package = installed
        .packages
        .iter()
        .find(|package| component.starts_with(&package.location))
        .ok_or_else(|| {
            HelperError::new(
                Refused,
                "Pane does not know the package of this command; only installed packages \
                 ship helpers",
            )
        })?;
    let manifest = package.manifest.as_ref().map_err(|error| {
        HelperError::new(Unavailable, format!("the package cannot load: {error}"))
    })?;
    let Some(helper) = manifest.helpers.iter().find(|helper| helper.id == name) else {
        let declared: Vec<String> = manifest
            .helpers
            .iter()
            .map(|helper| format!("`{}`", helper.id))
            .collect();
        let declared = match declared.as_slice() {
            [] => "it declares none".to_owned(),
            names => format!("it declares {}", platform::join(names)),
        };
        return Err(HelperError::new(
            NotFound,
            format!(
                "{} declares no helper `{}` in its pane.json; {declared}",
                package.title(),
                name.escape_debug()
            ),
        ));
    };
    let Some(target) = Target::current() else {
        return Err(HelperError::new(
            Unavailable,
            format!(
                "Not available on this system ({} {}): Pane names no helper target for it",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        ));
    };
    let Some(file) = helper.for_this_system() else {
        let targets: Vec<String> = helper.targets.keys().map(Target::to_string).collect();
        return Err(HelperError::new(
            Unavailable,
            format!(
                "Not available on {target}: helper `{name}` is built only for {}",
                platform::join(&targets)
            ),
        ));
    };
    runner::check_file(&package.location, file, target).map_err(|reason| {
        HelperError::new(Unavailable, format!("helper `{name}` cannot run: {reason}"))
    })
}

/// "Helpers: echo for Linux x86-64 (this system) and Windows x86-64", or
/// "... (none for this system)", for a package preview; `None` when the
/// package ships none.
pub(crate) fn describe(helpers: &[ManifestHelper]) -> Option<String> {
    if helpers.is_empty() {
        return None;
    }
    let here = Target::current();
    let described: Vec<String> = helpers
        .iter()
        .map(|helper| {
            let targets: Vec<String> = helper
                .targets
                .keys()
                .map(|target| {
                    if Some(*target) == here {
                        format!("{target} (this system)")
                    } else {
                        target.to_string()
                    }
                })
                .collect();
            let none_here = if helper.for_this_system().is_some() {
                ""
            } else {
                " (none for this system)"
            };
            format!("{} for {}{none_here}", helper.id, platform::join(&targets))
        })
        .collect();
    Some(format!("Helpers: {}", described.join("; ")))
}
