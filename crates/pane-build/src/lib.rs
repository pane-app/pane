//! How a local package is built from its source folder: once, or after each
//! save while it is developed (ADR 0004, ADR 0047; #12, #13, #216).
//!
//! Pane's core and `pane-ext` build packages with this crate, so that the
//! two never build a package differently:
//!
//! - [`Build`] and [`Builder`] say what a build is; [`Toolchains`] is Pane's,
//!   one adapter per language.
//! - [`build_package`] builds a folder once and returns its staged
//!   components, or why it did not build; [`build_js_command`] is the
//!   JavaScript/TypeScript build of one command, which `cargo xtask
//!   js-guests` runs as well.
//! - [`Prepared`] and [`Session`] build a folder after each save, handing each
//!   build that succeeds to a [`Host`]: Pane's launcher reloads the package
//!   from it.
//! - [`process_tree`] ends a build with every process it started; Pane's
//!   core also runs an extension's system programs with it.
//!
//! It knows nothing of the window or the launcher. What a package's
//! `pane.json` names is read by the caller's [`ManifestFiles`], so that the
//! manifest's rules stay in one place, Pane's core.

use std::path::{Path, PathBuf};

mod build;
mod js;
mod js_assets;
pub mod process_tree;
mod session;
mod sources;

pub use build::{
    Build, BuildJob, BuildOutcome, Builder, Componentizer, Echo, Toolchains, build_package,
    is_save, stage_package,
};
pub use js::build_js_command;
pub use session::{
    BuildFailure, Claim, Development, Host, MAX_OBSOLETE, Prepared, Session, Worker,
    copy_components,
};

/// The name of a package's manifest, in its folder.
pub const MANIFEST_FILE: &str = "pane.json";

/// What a package's manifest names, as its caller reads it: Pane's core
/// knows the manifest's rules, and this crate does not.
pub trait ManifestFiles: Send + Sync + 'static {
    /// The distinct components the [`MANIFEST_FILE`] in `folder` names,
    /// relative to `folder`, or why it cannot be read.
    fn components(&self, folder: &Path) -> Result<Vec<PathBuf>, String>;

    /// The files of the helpers the [`MANIFEST_FILE`] in `folder` ships for
    /// this system, relative to `folder`, or why it cannot be read.
    fn helper_files(&self, folder: &Path) -> Result<Vec<PathBuf>, String>;
}

/// `path` resolved as a package identity's folder is: canonical, in the
/// ordinary spelling on Windows.
pub fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path).map(without_verbatim_prefix)
}

/// Windows' canonical paths carry a `\\?\` prefix; the identity uses the
/// ordinary spelling (`C:\…`, `\\server\share\…`) that users recognise.
pub fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\")
        && rest.as_bytes().get(1) == Some(&b':')
    {
        PathBuf::from(rest)
    } else {
        path
    }
}
