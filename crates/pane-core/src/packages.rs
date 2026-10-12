//! Extension packages: the `pane.json` manifest, source-derived package
//! identity, and Pane's managed copies of installed packages.
//!
//! A local package is a folder holding `pane.json` and the components it
//! names. Installing copies exactly those files into Pane's managed location,
//! so the user's folder is never written and the installed copy keeps working
//! if the folder changes or disappears; of a native helper (see `helpers`),
//! only its file for this system is copied. Installed packages are recorded in
//! `installed.json`, with whether the user disabled each; reading them back
//! needs only the manifests, never the guests.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component as PathPart, Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::arguments::{self, ManifestArgument};
use crate::atomic::{Readers, write_atomically};
use crate::collections::Resolved;
use crate::defaults::InstalledDefault;
use crate::git::{GitOrigin, GitRevision, GitSpec, InstalledGit, Repository, shown};
use crate::helpers::runner;
use crate::icons::{self, Icon};
use crate::launcher::CommandRegistration;
use crate::npm::{Fetched, NpmOrigin, NpmPackage, NpmSpec};
use crate::platform::{self, Platform};
use crate::preferences::{self, Preference};
use crate::runtime::{CallError, Exports};
use pane_target::Target;

/// The manifest file at the root of every package.
pub const MANIFEST_FILE: &str = pane_build::MANIFEST_FILE;

/// The manifest format version this Pane reads.
pub const MANIFEST_VERSION: u64 = 1;

/// The extension API this Pane provides, as (major, minor): the
/// `pane:extension` WIT package version.
pub const EXTENSION_API: (u64, u64) = (0, 2);

/// The extension API this Pane still runs besides [`EXTENSION_API`]: the
/// previous version, whose world lives under `wit/previous/` (ADR 0046),
/// held there by `cargo xtask ci-lints` at the version those files
/// declare. A component built for it is instantiated against that world:
/// every interface it imports is served through the host's adapter (see
/// `runtime::previous`), and every command it exports is called through
/// it. The window ends with the first Pane release after this one, and
/// then the world and the adapter are deleted together.
pub const EXTENSION_API_PREVIOUS: Option<(u64, u64)> = Some((0, 1));

const REGISTRY_FILE: &str = "installed.json";
const REGISTRY_VERSION: u64 = 1;
const PACKAGES_DIR: &str = "packages";

/// The identity of an installed package, derived from its source and
/// independent of its display title. A local package is identified by its
/// folder's resolved absolute path, as the operating system reports it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageIdentity(Source);

/// A package's source, as `installed.json` records it: `"local": "<folder>"`,
/// `"npm": "<package name>"`, `"git": "<host>/<repository path>"` or
/// `"default": "<default extension's id>"`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
enum Source {
    Local { local: String },
    Npm { npm: String },
    Git { git: String },
    Default { default: String },
}

/// The id Pane gives an installed package's command, in root search and
/// in its own records about commands (hotkeys, aliases, quick slots,
/// subtitles, remembered dropdown values): `<package identity key>#<manifest
/// command id>`. A manifest command id cannot contain `#`, but an identity
/// key can (a local package's folder may have one in its path), so the
/// package's part is everything before the last `#`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CommandId<'a> {
    /// The package's identity key ([`PackageIdentity::key`]).
    pub(crate) package: &'a str,
    /// The command's id in the package's manifest; empty when the id names
    /// no command.
    pub(crate) command: &'a str,
}

impl<'a> CommandId<'a> {
    /// The parts of the command id `id`. An id without `#` is all package
    /// and no command.
    pub(crate) fn parse(id: &'a str) -> CommandId<'a> {
        let (package, command) = id.rsplit_once('#').unwrap_or((id, ""));
        CommandId { package, command }
    }
}

impl fmt::Display for CommandId<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.package, self.command)
    }
}

impl PackageIdentity {
    /// Resolves the identity of the local package folder at `folder`.
    ///
    /// The path is made absolute and resolved by the operating system
    /// (`std::fs::canonicalize`): symbolic links and `.`/`..` are followed,
    /// and file systems that ignore case or Unicode normalization report the
    /// stored spelling. Pane applies no case folding or normalization of its
    /// own, so on a case-sensitive file system two spellings are two folders.
    pub fn local(folder: &Path) -> Result<PackageIdentity, PackageError> {
        let resolved = fs::canonicalize(folder)
            .map_err(|error| PackageError::NotAFolder(folder.to_path_buf(), error.to_string()))?;
        if !resolved.is_dir() {
            return Err(PackageError::NotAFolder(
                folder.to_path_buf(),
                "it is not a folder".into(),
            ));
        }
        let resolved = without_verbatim_prefix(resolved);
        let path = resolved
            .to_str()
            .ok_or_else(|| PackageError::NotUnicode(resolved.clone()))?;
        Ok(PackageIdentity(Source::Local {
            local: path.to_owned(),
        }))
    }

    /// The identity of the extension `id` of the collection at `folder`:
    /// the folder's resolved path with the id after `#`, as a Git
    /// repository's is with its extension's (ADR 0044). The id, not the
    /// extension's folder, names it, so moving the folder keeps the
    /// identity.
    pub fn local_extension(folder: &Path, id: &str) -> Result<PackageIdentity, PackageError> {
        Ok(local_extension_of(PackageIdentity::local(folder)?, id))
    }

    /// A stable key for this identity, for ids and records rather than for
    /// people to read: `local:` followed by the folder's resolved path. The
    /// [`Display`](fmt::Display) form is the wording shown to users.
    pub fn key(&self) -> String {
        match &self.0 {
            Source::Local { local } => format!("local:{local}"),
            Source::Npm { npm } => format!("npm:{npm}"),
            Source::Git { git } => format!("git:{git}"),
            Source::Default { default } => format!("default:{default}"),
        }
    }

    /// The id of this package's command whose manifest id is `command`
    /// (see [`CommandId`]).
    pub(crate) fn command_id(&self, command: &str) -> String {
        CommandId {
            package: &self.key(),
            command,
        }
        .to_string()
    }

    /// The identity of the Git repository `repository`, whatever the
    /// revision: `git:` and its host and path (see [`GitSpec::parse`]).
    pub fn git(repository: &Repository) -> PackageIdentity {
        PackageIdentity(Source::Git {
            git: repository.name().to_owned(),
        })
    }

    /// The identity of the extension `id` of the collection at the Git
    /// repository `repository`, whatever the revision: `git:`, its host and
    /// path, and the id after `#` (ADR 0044). The id, not the extension's
    /// folder, names it, so moving the folder keeps the identity.
    pub fn git_extension(repository: &Repository, id: &str) -> PackageIdentity {
        PackageIdentity(Source::Git {
            git: format!("{}#{id}", repository.name()),
        })
    }

    /// The id of the extension of a collection this package is, if it is
    /// one: the part of its Git repository after `#` (a repository path
    /// holds no `#`, so the first one begins the id).
    pub fn extension_id(&self) -> Option<&str> {
        match &self.0 {
            Source::Git { git } => git.split_once('#').map(|(_, id)| id),
            _ => None,
        }
    }

    /// The host and path of a Git package's repository, with the id of its
    /// extension of a collection after `#` where it is one.
    pub fn git_repository(&self) -> Option<&str> {
        match &self.0 {
            Source::Git { git } => Some(git),
            _ => None,
        }
    }

    /// Whether the package was downloaded (from npm, Git or Pane's own
    /// downloads) rather than installed from a folder on this computer.
    pub(crate) fn is_published(&self) -> bool {
        !matches!(self.0, Source::Local { .. })
    }

    /// The identity of the npm package `name` (checked by
    /// [`NpmSpec::parse`]), whatever its version.
    pub fn npm(name: &str) -> PackageIdentity {
        PackageIdentity(Source::Npm {
            npm: name.to_owned(),
        })
    }

    /// The package name of an npm package.
    pub fn npm_name(&self) -> Option<&str> {
        match &self.0 {
            Source::Npm { npm } => Some(npm),
            Source::Local { .. } | Source::Git { .. } | Source::Default { .. } => None,
        }
    }

    /// The identity of the default extension `id` (checked by
    /// [`crate::defaults::parse_pins`]), whatever its version: the
    /// extension Pane acquires for this feature, from the commit its
    /// release pins.
    pub fn default_extension(id: &str) -> PackageIdentity {
        PackageIdentity(Source::Default {
            default: id.to_owned(),
        })
    }

    /// The id of a default extension Pane acquires at first setup.
    pub fn default_id(&self) -> Option<&str> {
        match &self.0 {
            Source::Default { default } => Some(default),
            _ => None,
        }
    }

    /// The source folder of a local package.
    pub fn local_folder(&self) -> Option<&Path> {
        match &self.0 {
            Source::Local { local } => Some(Path::new(local)),
            Source::Npm { .. } | Source::Git { .. } | Source::Default { .. } => None,
        }
    }

    /// The identity of the package that the package with this identity
    /// names with the dependency source `source` (`local:` and a `/`-separated
    /// path, as the manifest checked). The path is relative to this package's
    /// source folder as Pane resolved it (a package installed through a
    /// symbolic link resolves against the folder the link points to), or
    /// absolute. An existing folder is resolved as an installed folder is.
    /// For one that does not exist (yet, or any more), `.` and `..` are
    /// removed from the spelling and its deepest existing parent is resolved
    /// by the operating system, so that it matches the identity the folder
    /// gets once it exists, unless the folder itself becomes a link, which
    /// [`PackageIdentity::resolved_again`] covers when calls are matched.
    /// Fails, with the path, only for a path that cannot be an identity (not
    /// absolute or not Unicode).
    ///
    /// An `npm:` source is the npm package it names, whatever its version,
    /// and a `git:` source the repository it names, whatever its revision.
    /// A `git:` or `local:` source may name one extension of a collection
    /// with `#<id>` (ADR 0044): where a whole path names a folder it is that
    /// folder, and the part after the last `#` is the id. A package from npm
    /// or Git cannot name a `local:` folder: its folder is on its author's
    /// computer, not the user's.
    pub(crate) fn dependency(&self, source: &str) -> Result<PackageIdentity, PathBuf> {
        self.dependency_at(self.local_folder(), source)
    }

    /// Like [`PackageIdentity::dependency`], resolving `local:` sources
    /// against `folder`: the declaring package's own source folder as Pane
    /// resolved it — for one extension of a local collection, that
    /// extension's folder, not the collection's root. `None` where the
    /// package is published, which cannot name a local folder at all.
    pub(crate) fn dependency_at(
        &self,
        folder: Option<&Path>,
        source: &str,
    ) -> Result<PackageIdentity, PathBuf> {
        let path = match SourceSpec::parse(source) {
            Ok(SourceSpec::Npm(spec)) => return Ok(PackageIdentity::npm(&spec.name)),
            Ok(SourceSpec::Git(spec)) => {
                return match spec.extension.as_deref() {
                    Some(id) => Ok(PackageIdentity::git_extension(&spec.repository, id)),
                    None => Ok(PackageIdentity::git(&spec.repository)),
                };
            }
            Ok(SourceSpec::Local(path)) => path,
            Err(_) => return Err(PathBuf::from(source)),
        };
        // A `#` names one extension of a collection at the folder the source
        // names: the part after the last `#` is the id (a folder name may
        // hold a `#` of its own).
        let (path, extension) = match path.rsplit_once('#') {
            Some((path, id)) => (path.to_owned(), Some(id.to_owned())),
            None => (path, None),
        };
        let folder = match folder {
            // A local package's own folder.
            Some(folder) if matches!(self.0, Source::Local { .. }) => folder.join(&path),
            // A package from npm or Git cannot name a local folder.
            _ => return Err(PathBuf::from(path)),
        };
        if let Ok(identity) = PackageIdentity::local(&folder) {
            return Ok(match extension {
                // The id resolves through the collection's `renamed` map
                // where the folder is one (ADR 0044, #310): an old id
                // names the extension's current one, which the identity
                // takes, as a read of the source resolves it.
                Some(id) => local_extension_of(identity, &resolved_id(&folder, &id)),
                None => identity,
            });
        }
        let mut spelled = PathBuf::new();
        for part in folder.components() {
            match part {
                PathPart::CurDir => {}
                PathPart::ParentDir => {
                    spelled.pop();
                }
                other => spelled.push(other),
            }
        }
        let mut missing = Vec::new();
        let mut existing = spelled.clone();
        let resolved = loop {
            if let Ok(resolved) = fs::canonicalize(&existing) {
                break missing
                    .iter()
                    .rev()
                    .fold(resolved, |path: PathBuf, part| path.join(part));
            }
            match (
                existing.file_name().map(ToOwned::to_owned),
                existing.parent(),
            ) {
                (Some(name), Some(parent)) => {
                    missing.push(name);
                    existing = parent.to_path_buf();
                }
                _ => break spelled.clone(),
            }
        };
        let resolved = without_verbatim_prefix(resolved);
        match resolved.to_str() {
            Some(text) if resolved.is_absolute() => Ok(match extension {
                Some(id) => local_extension_of(
                    PackageIdentity(Source::Local {
                        local: text.to_owned(),
                    }),
                    &id,
                ),
                None => PackageIdentity(Source::Local {
                    local: text.to_owned(),
                }),
            }),
            _ => Err(resolved),
        }
    }

    /// This local identity resolved by the operating system now, if its
    /// folder exists and resolves to another spelling: a dependency recorded
    /// before its folder existed, which became a symbolic link or was
    /// created with another spelling on a file system that ignores case.
    pub(crate) fn resolved_again(&self) -> Option<PackageIdentity> {
        let resolved = PackageIdentity::local(self.local_folder()?).ok()?;
        (resolved != *self).then_some(resolved)
    }
}

/// The local identity `of`, with the extension id `id` of the collection
/// at its folder after `#` (see [`PackageIdentity::local_extension`]).
fn local_extension_of(of: PackageIdentity, id: &str) -> PackageIdentity {
    let PackageIdentity(source) = of;
    let Source::Local { local } = source else {
        unreachable!("a local identity is a local folder");
    };
    PackageIdentity(Source::Local {
        local: format!("{local}#{id}"),
    })
}

/// The id a `local:` source names for the collection at `folder` (ADR
/// 0044, #310), resolved through its `renamed` map: an old id names the
/// extension's current one, which a dependency's identity takes, as a
/// read of the source resolves it. The id as written where the folder is
/// no collection, its index cannot be read, or it resolves no such id.
fn resolved_id(folder: &Path, id: &str) -> String {
    crate::collections::read(folder)
        .ok()
        .flatten()
        .and_then(|collection| collection.resolve(id))
        .map(|resolved| resolved.id)
        .unwrap_or_else(|| id.to_owned())
}

/// The installed package with `identity` among `packages`.
pub(crate) fn installed_as<'a>(
    packages: &'a [InstalledPackage],
    identity: &PackageIdentity,
) -> Option<&'a InstalledPackage> {
    packages
        .iter()
        .find(|package| package.identity == *identity)
}

impl fmt::Display for PackageIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Source::Local { local } => write!(f, "local folder {local}"),
            Source::Npm { npm } => write!(f, "npm package {npm}"),
            Source::Git { git } => write!(f, "Git repository {git}"),
            Source::Default { default } => write!(f, "Pane's default extension {default}"),
        }
    }
}

/// `text` with its first letter in uppercase.
pub(crate) fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The name people know a package folder by: its last component, or the
/// whole path when it has none.
pub(crate) fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string())
}

use pane_build::without_verbatim_prefix;

/// A package's manifest, `pane.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub title: String,
    /// What the package does, in a sentence (`"description"`): its page in
    /// Settings, the install preview and the Extensions group in Settings
    /// show it. `None` when it does not say.
    pub description: Option<String>,
    /// Who wrote the package (`"author"`, #224): parsed and held for the
    /// authoring tooling and future use; `None` when it does not say.
    pub author: Option<String>,
    /// The package's own page on the web (`"homepage"`, #224): `None` when
    /// it does not say.
    pub homepage: Option<String>,
    /// Where the package's source lives (`"repository"`, #224): also where
    /// "Report issue" opens when the manifest gives no `issues`; `None`
    /// when it does not say.
    pub repository: Option<String>,
    /// Where the package's users should report its problems (`"issues"`,
    /// #224): what "Report issue" opens with a prefilled report; `None`
    /// when it does not say.
    pub issues: Option<String>,
    /// The package's license (`"license"`, #224), as an identifier or a
    /// human-readable name; `None` when it does not say.
    pub license: Option<String>,
    /// The words a person would search for to find the package
    /// (`"keywords"`, #224), at most [`MAX_KEYWORDS`], empty entries
    /// dropped.
    pub keywords: Vec<String>,
    pub version: Option<String>,
    /// The package's own icon (`"icon"`, #139): a built-in icon or an
    /// image the package ships. `None` for none, which Pane shows as a
    /// first-letter tile.
    pub icon: Option<Icon>,
    /// The extension API the package needs, such as `0.1`.
    pub api_version: String,
    /// The operating systems the package supports; `None` when it does not
    /// say, which means every system Pane runs on, and empty for none. A
    /// package that does not support this system is explained instead of
    /// installed, and an installed copy of one lists its commands as
    /// unavailable.
    pub platforms: Option<Vec<Platform>>,
    pub commands: Vec<ManifestCommand>,
    /// The operations the package publishes for other extensions to call.
    /// Only these are callable: a command is not an operation.
    pub operations: Vec<ManifestOperation>,
    /// The native helpers the package ships, which its commands run through
    /// Pane (`pane:extension/helpers`).
    pub helpers: Vec<ManifestHelper>,
    /// The other packages whose operations this one calls, required or
    /// optional.
    pub dependencies: Vec<ManifestDependency>,
    /// The package asks for access to one folder the user chooses
    /// (`"folderAccess": true`): Pane offers its own "Choose folder" row
    /// in the package's commands, and lists only that folder for it
    /// (`pane:extension/files`).
    pub folder_access: bool,
    /// The package uses Pane's file index (`"fileIndex": true`): Pane keeps
    /// the index of the home folder open, caught up and watched while at
    /// least one enabled, unpaused package says so, and its commands may
    /// search it (`pane:extension/file-index`, #126).
    pub file_index: bool,
    /// The preferences the package declares for all its commands
    /// (`"preferences"`; see `preferences`).
    pub preferences: Vec<Preference>,
    /// The capabilities the package provides (`"provides"`): each a named,
    /// versioned set of operations that other packages call through Pane by
    /// the capability's name, without naming this package (ADR 0041).
    pub provides: Vec<ManifestProvides>,
    /// The capabilities the package uses (`"uses"`): the operations its
    /// code calls through Pane by each capability's name.
    pub uses: Vec<ManifestUse>,
    /// The package's activation entry point (`"activate"`): the component,
    /// relative to the package folder, that exports `activate` (ADR 0041),
    /// which Pane calls when the package's code may run and is not
    /// waiting, and again when the instance that ran it is dropped while
    /// the generation continues. `None` without one: a package's code
    /// first runs when the user asks for one of its commands (lazy
    /// activation, ADR 0005).
    pub activate: Option<PathBuf>,
}

/// A native helper a package ships: a prebuilt program per target (operating
/// system and processor), which its commands run by name through Pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestHelper {
    pub id: String,
    /// The helper's file for each target it is built for, such as
    /// `linux-x86_64`, relative to the package folder.
    pub targets: BTreeMap<Target, PathBuf>,
}

impl ManifestHelper {
    /// The helper's file for this system, if the package ships one.
    pub fn for_this_system(&self) -> Option<&Path> {
        self.targets.get(&Target::current()?).map(PathBuf::as_path)
    }
}

/// Another package whose operations a package calls, as its `pane.json`
/// declares it under `dependencies`. Installing the package installs its
/// missing required dependencies with it; an optional one is used only when
/// the user installed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestDependency {
    /// The name the package's code calls it by in place of its package
    /// identity: unique in the package; lowercase letters, digits and `-`.
    pub id: String,
    /// Where it is installed from, as written: `local:` and a folder path
    /// separated by `/`, relative to the declaring package's folder or
    /// absolute (`/…`), or `npm:` and a package name, optionally with an
    /// exact version (`npm:@scope/name@1.2.3`). Other sources are not
    /// supported yet.
    pub source: String,
    /// Whether the package needs it (the default) or only uses it when it is
    /// installed (`"optional": true`).
    pub required: bool,
    /// The operations the package calls, each at the version it calls: the
    /// dependency is compatible when it publishes all of them.
    pub operations: Vec<RequiredOperation>,
    /// The operating systems on which the package needs it; `None` for
    /// every system. Elsewhere it is neither installed nor checked.
    pub platforms: Option<Vec<Platform>>,
}

impl ManifestDependency {
    /// Whether the declaring package needs this dependency on this system.
    pub fn needed_here(&self) -> bool {
        self.only_on().is_none()
    }

    /// "only on Windows and Linux" when the package does not need it on
    /// this system; `None` when it does.
    pub(crate) fn only_on(&self) -> Option<String> {
        platform::unavailable(self.platforms.as_deref(), "it")?;
        let platforms = self.platforms.as_deref().unwrap_or_default();
        Some(match platforms {
            [] => "on no system".to_owned(),
            platforms => format!("only on {}", platform::names(platforms)),
        })
    }

    /// Whether the package declares that it calls `operation` at `version`
    /// here.
    pub(crate) fn calls(&self, operation: &str, version: u32) -> bool {
        self.operations
            .iter()
            .any(|declared| declared.id == operation && declared.version == version)
    }
}

/// An operation a package calls in one of its dependencies, at the version
/// it calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequiredOperation {
    pub id: String,
    pub version: u32,
}

/// An operation a package publishes: other extensions call it through
/// Pane by the package's source, the operation's id and its version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestOperation {
    pub id: String,
    /// The version of the operation's input and result: a caller names the
    /// version it was written for, and any other is refused. A change that
    /// breaks callers publishes a new version.
    pub version: u32,
    /// The component serving it, relative to the package folder; often a
    /// command's component too.
    pub component: PathBuf,
    /// The operating systems it works on; `None` for every system the
    /// package supports. Elsewhere a call to it is unavailable.
    pub platforms: Option<Vec<Platform>>,
}

/// A capability a package provides (`"provides"` in its `pane.json`): a
/// named, versioned set of operations that other packages call through Pane
/// by the capability's name, such as `acme:translate@1`, instead of by this
/// package's identity (ADR 0041). The component named there serves them
/// through the export that serves published operations, with each operation
/// qualified by its capability; a capability's operations are reached only
/// through the capability, unless the package publishes them in
/// `operations` too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestProvides {
    /// The capability's name, `<namespace>:<name>@<major>`.
    pub capability: String,
    /// The component serving it, relative to the package folder.
    pub component: PathBuf,
    /// The operations the capability is made of.
    pub operations: Vec<String>,
    /// The operating systems it works on; `None` for every system the
    /// package supports. Elsewhere the package does not provide the
    /// capability.
    pub platforms: Option<Vec<Platform>>,
    /// Whether the package provides it only at run time (`"atRunTime"`):
    /// only while its code holds a provision for it, as an owned
    /// registration (#158). The capability is still known from the
    /// manifest, so install plans, cycles and Settings work from the
    /// manifest alone; a provision the manifest does not declare and
    /// mark is refused.
    pub at_run_time: bool,
}

/// A capability a package uses (`"uses"` in its `pane.json`): the operations
/// its code calls through Pane by the capability's name, never naming the
/// package that serves them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestUse {
    pub capability: String,
    /// The operations the package calls, each reached only through the
    /// capability.
    pub operations: Vec<String>,
    /// Whether the package needs it (the default) or only uses it when some
    /// installed package provides it (`"optional": true`).
    pub required: bool,
    /// Whether the package calls one provider (`"use": "one"`, the default)
    /// or every provider (`"all"`). Read and checked here; fanning a call
    /// out to every provider comes later.
    pub use_all: bool,
    /// A provider Pane installs when no installed package provides the
    /// capability, written as a dependency's source is. Read and checked
    /// here; installing a default comes later.
    pub default: Option<String>,
    /// The manifest ids of the commands that need the capability; `None`
    /// when the whole package needs it. Read and checked here; commands
    /// waiting for a capability come later.
    pub commands: Option<Vec<String>>,
}

/// The scheduled work a command declares: every `every_seconds` seconds
/// while the package's code may run (see `launcher/schedules`), the action
/// of its component's item `item` runs (a view command), or the command
/// itself runs with a `background` launch (a no-view command, which names
/// no item). One schedule kind: a fixed interval.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestSchedule {
    /// How often the work runs, in seconds.
    pub every_seconds: u64,
    /// The id of the item whose action runs, for a view command: Pane asks
    /// for the command's tree and runs that item's action (its callback),
    /// as choosing it would. The command's list lists it, so the user can
    /// run it too. `None` for a no-view command, which Pane runs itself.
    pub item: Option<String>,
}

/// What a command does when it is launched (`"mode"` in its `pane.json`
/// entry, ADR 0037). Pane reads it from the manifest, so it knows at Enter
/// whether to open a screen without running any guest code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CommandMode {
    /// `"view"`, also the mode of a command whose entry does not say: it
    /// opens a screen, its list (`render`).
    #[default]
    View,
    /// `"no-view"`: launching it calls its run entry point (`run`) and
    /// opens no screen.
    NoView,
    /// `"provider"`: a root provider (#164), a command whose only job is
    /// to answer root search through its `rootResults` or
    /// `indexedResults`, such as the calculator. It is never launched: it
    /// has no row in root search, cannot be pinned, has no alias, fallback
    /// or hotkey, is offered by neither the Actions panel nor the Shortcuts
    /// page, and nothing opens or runs it (its component needs no `render`
    /// or `run` of its own; a reload's start check may still ask it, and
    /// takes its refusal as a start, as it does a no-view command's). Its
    /// results
    /// answer while its package is enabled, and its extension's Settings
    /// card lists it, so the user can still turn it off.
    Provider,
    /// `"designed"`: launching it opens a designed view (`open-view`, ADR
    /// 0036) — a screen the extension describes as a tree of layout nodes
    /// and UI components that Pane renders (`docs/designed-tree.md`) — and
    /// never calls its `render`. Transitional: the List and the Form move
    /// onto the tree (#240, #241), after which the `view` mode retires into
    /// this one and the mode says only that a screen opens.
    Designed,
}

/// When root search lists a command (`"when"` in its `pane.json` entry,
/// #195). The default, `Always`, is what commands did before: a row with
/// a blank query and whenever the query matches it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CommandWhen {
    /// `"always"`, also the `when` of a command whose entry does not say
    /// one: the command's row is listed with a blank query and while the
    /// user searches.
    #[default]
    Always,
    /// `"blank"`: only while nothing is typed, so the command is never
    /// found by a query, however well it matches.
    Blank,
    /// `"searching"`: only while the user types something, so the blank
    /// query's list does not hold it.
    Searching,
}

impl CommandWhen {
    /// Whether a command with this `when` is listed while the query is
    /// blank (`true`) or has text (`false`).
    pub fn listed(self, blank: bool) -> bool {
        match (self, blank) {
            (CommandWhen::Always, _) => true,
            (CommandWhen::Blank, true) => true,
            (CommandWhen::Blank, false) => false,
            (CommandWhen::Searching, true) => false,
            (CommandWhen::Searching, false) => true,
        }
    }
}

/// What root search matches a command's row by (`"matches"` in its
/// `pane.json` entry, #195). The default, `Title`, is what commands did
/// before.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CommandMatches {
    /// `"title"`, also the `matches` of a command whose entry does not say
    /// one: the row is listed when the query matches its title, subtitle
    /// or package title, as ever.
    #[default]
    Title,
    /// `"url"`: only for a query that is a typed web address, which the
    /// row is sent as its launch record's fallback text when invoked;
    /// never matched by title.
    Url,
    /// `"file-path"`: only for a path-like query, resolved and sent as
    /// its launch record's fallback text when invoked; never matched by
    /// title.
    FilePath,
}

/// The shortest interval a command's schedule may declare: 1 second.
/// Provisional (#47), pending the user's decision on scheduling intervals.
pub const MIN_SCHEDULE_SECONDS: u64 = 1;

/// The longest interval a command's schedule may declare: 30 days, so a
/// schedule is always finite. Provisional (#47), pending the user's
/// decision on scheduling intervals.
pub const MAX_SCHEDULE_SECONDS: u64 = 30 * 86_400;

/// The longest item id a command's schedule may name, in characters.
const MAX_SCHEDULE_ITEM: usize = 256;

/// The most keywords a package's manifest may list.
pub const MAX_KEYWORDS: usize = 20;

/// A command a package contributes to root search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestCommand {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    /// The command's own icon (`"icon"`, #139); `None` for its package's.
    pub icon: Option<Icon>,
    /// The command's component, relative to the package folder.
    pub component: PathBuf,
    /// The operating systems the command supports; `None` for every system
    /// the package supports. Elsewhere it is listed as unavailable.
    pub platforms: Option<Vec<Platform>>,
    /// Whether the command computes root results from root search's query
    /// (`"rootResults": true`), such as a calculator's answer: its component
    /// then also exports `pane:extension/root-results`.
    pub root_results: bool,
    /// Whether the command supplies root results ahead of the query
    /// (`"indexedResults": true`), such as the installed applications: its
    /// component then also exports `pane:extension/indexed-results`.
    pub indexed_results: bool,
    /// Whether it opens a screen, runs without one, or only answers root
    /// search (`"mode"`; `view` when the entry does not say).
    pub mode: CommandMode,
    /// The keywords the command declares (`"keywords"`, #197), which
    /// find its row in root search as its subtitle does: an author's
    /// search terms, distinct from the user's aliases.
    pub keywords: Vec<String>,
    /// When root search lists the command (`"when"`, #195; `Always` when
    /// the entry does not say one).
    pub when: CommandWhen,
    /// What root search matches the command's row by (`"matches"`, #195;
    /// `Title` when the entry does not say one): its title as usual, or
    /// only URL-like or path-like queries, which the row is then sent as
    /// its launch record's fallback text when invoked.
    pub matches: CommandMatches,
    /// Whether the command takes a query (`"takesQuery": true`): text typed
    /// into root search that Pane sends it when the user invokes it through
    /// its alias or as a fallback, as its launch record's fallback text.
    pub takes_query: bool,
    /// The scheduled work the command declares (`"schedule"`), if any:
    /// every `schedule.every_seconds` seconds while the package's code may
    /// run, Pane runs the action of `schedule.item` (a view command) or the
    /// command itself in the background (a no-view command).
    pub schedule: Option<ManifestSchedule>,
    /// Whether the command runs a continuing service (`"service": true`):
    /// while the package's code may run, Pane calls the component's
    /// `run-cycle` export in a cycle the service itself paces, with no
    /// interval the manifest declares (see `launcher/services`).
    pub service: bool,
    /// The preferences the command declares for itself (`"preferences"`),
    /// besides its package's.
    pub preferences: Vec<Preference>,
    /// The typed values the command asks for before each run
    /// (`"arguments"`, at most [`MAX_ARGUMENTS`](crate::MAX_ARGUMENTS)), in
    /// the order its fields show them; see `arguments`.
    pub arguments: Vec<ManifestArgument>,
}

impl ManifestCommand {
    /// Whether text typed into root search can be sent to the command
    /// through its alias or as a fallback: it takes a query, or its first
    /// argument is text and every other is optional (Raycast's rule). The
    /// text arrives as its launch record's fallback text and fills its
    /// first text or password argument.
    pub fn accepts_fallback_text(&self) -> bool {
        self.takes_query || arguments::accept_fallback_text(&self.arguments)
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename = "Manifest", rename_all = "camelCase")]
pub(crate) struct ManifestJson {
    /// The name the package is known by, in Title Case: root search and the
    /// extension's page in Settings show it. Not empty.
    title: String,
    /// What the package does, in a sentence: Pane shows it in the install
    /// preview and in the Extensions group in Settings. Empty when it does
    /// not say.
    #[serde(default)]
    description: Option<String>,
    /// Who wrote the package, as people write it ("Ada Lovelace"). Empty
    /// when it does not say.
    #[serde(default)]
    author: Option<String>,
    /// The package's own page on the web, which people can open to read
    /// more about it. Empty when it does not say.
    #[serde(default)]
    homepage: Option<String>,
    /// Where the package's source lives (a web page or a Git repository
    /// address); also where "Report issue" opens when the manifest gives
    /// no `issues`. Empty when it does not say.
    #[serde(default)]
    repository: Option<String>,
    /// Where the package's users should report its problems, which "Report
    /// issue" opens with a prefilled report. Empty when it does not say;
    /// Pane then falls back to `repository`.
    #[serde(default)]
    issues: Option<String>,
    /// The package's license, as an identifier ("MIT", "Apache-2.0") or a
    /// human-readable name. Empty when it does not say.
    #[serde(default)]
    license: Option<String>,
    /// The words a person would search for to find the package, one word or
    /// short phrase each: at most 20, and empty entries are ignored.
    #[serde(default)]
    #[schemars(schema_with = "crate::schema::keywords")]
    keywords: Vec<String>,
    /// The package's own version, as dotted numbers ("1.2.0"): Pane shows
    /// it in the install preview and on the extension's page. Empty when it
    /// does not say.
    #[serde(default)]
    version: Option<String>,
    /// What the package's own icon is: a built-in icon's name, or an
    /// image the package ships. Without one, Pane draws a tile from the
    /// package's first letter; a published extension's icon is a
    /// 512×512 image.
    #[serde(default)]
    #[schemars(with = "Option<crate::schema::Icon>")]
    icon: Option<serde_json::Value>,
    /// The extension API the package needs, such as "0.1": the version of
    /// Pane's `pane:extension` interface it was built against. Every
    /// breaking change to that interface gets a new version.
    api_version: String,
    /// The operating systems the package supports: `windows`, `macos` and
    /// `linux`. Without the field it supports every system; an empty list
    /// supports none, and a package that does not support this system is
    /// explained instead of installed.
    #[serde(default)]
    #[schemars(with = "Option<Vec<crate::schema::PlatformId>>")]
    platforms: Option<Vec<String>>,
    /// What the package contributes to root search. A package needs at
    /// least one command or one operation.
    #[serde(default)]
    commands: Vec<CommandJson>,
    /// The operations the package publishes for other extensions to call
    /// through Pane. Only these are callable: a command is not an
    /// operation.
    #[serde(default)]
    operations: Vec<OperationJson>,
    /// The native helpers the package ships: a prebuilt program per
    /// operating system and processor, which its commands run by name
    /// through Pane.
    #[serde(default)]
    helpers: Vec<HelperJson>,
    /// The other packages whose operations this one calls, required or
    /// optional.
    #[serde(default)]
    dependencies: Vec<DependencyJson>,
    /// The package asks for access to one folder the user chooses: Pane
    /// offers its own "Choose folder" row in the package's commands, and
    /// lists only that folder for it.
    #[serde(default)]
    folder_access: bool,
    /// The package uses Pane's file index: Pane keeps the index of the home
    /// folder open, caught up and watched while at least one enabled,
    /// unpaused package says so, and its commands may search it.
    #[serde(default)]
    file_index: bool,
    /// The preferences the package declares for all its commands: typed
    /// fields whose values the user sets in Pane and the commands read.
    #[serde(default)]
    #[schemars(with = "Vec<crate::schema::Preference>")]
    preferences: Vec<serde_json::Value>,
    /// The capabilities the package provides (`"provides"`): each a
    /// named, versioned set of operations that other packages call
    /// through Pane by the capability's name, such as `acme:translate@1`,
    /// instead of by this package's identity (ADR 0041).
    #[serde(default)]
    provides: Vec<ProvidesJson>,
    /// The capabilities the package uses (`"uses"`): the operations its
    /// code calls through Pane by the capability's name, never naming the
    /// package that serves them.
    #[serde(default)]
    uses: Vec<UsesJson>,
    /// The package's activation entry point (`"activate"`): the component,
    /// relative to the package folder, that exports `activate` (ADR 0041),
    /// called when its code may run so its registrations exist without
    /// waiting for the user.
    #[serde(default)]
    activate: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename = "Helper", deny_unknown_fields)]
pub(crate) struct HelperJson {
    /// The name the package's commands run the helper by, through Pane.
    id: String,
    /// The helper's file for each system it is built for, relative to the
    /// package folder: keyed by target, such as `linux-x86_64` or
    /// `macos-aarch64`. At least one.
    #[schemars(schema_with = "crate::schema::targets")]
    targets: BTreeMap<String, String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename = "Dependency", deny_unknown_fields)]
pub(crate) struct DependencyJson {
    /// The name the package's code calls the dependency by in place of its
    /// package identity: unique in the package; lowercase letters, digits
    /// and `-`.
    id: String,
    /// Where the dependency is installed from: `local:` and a folder path
    /// separated by `/`, relative to this package's folder or absolute;
    /// `npm:` and a package name with an optional exact version
    /// (`npm:@scope/name@1.2.3`); or `git:` and a repository with an
    /// optional reference. Other sources are not supported yet.
    source: String,
    /// Whether the package only uses the dependency when the user installed
    /// it; the default is to need it, and installing the package installs
    /// its missing required dependencies with it.
    #[serde(default)]
    optional: bool,
    /// The operations the package calls, each at the version it calls: the
    /// dependency is compatible when it publishes all of them.
    operations: Vec<RequiredOperationJson>,
    /// The operating systems on which the package needs it; without the
    /// field, every system. Elsewhere it is neither installed nor checked.
    #[serde(default)]
    #[schemars(with = "Option<Vec<crate::schema::PlatformId>>")]
    platforms: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename = "RequiredOperation", deny_unknown_fields)]
pub(crate) struct RequiredOperationJson {
    /// The id of the operation the package calls, as the dependency
    /// publishes it.
    id: String,
    /// The version of the operation's input and result the package was
    /// written for, from 1: any other is refused.
    version: u32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename = "Operation", deny_unknown_fields)]
pub(crate) struct OperationJson {
    /// The operation's id, unique in the package: other extensions call the
    /// operation by the package's source, this id and its version.
    id: String,
    /// The version of the operation's input and result: a caller names the
    /// version it was written for, and any other is refused. From 1; a
    /// change that breaks callers publishes a new version.
    version: u32,
    /// The component serving the operation, relative to the package folder;
    /// often a command's component too.
    component: String,
    /// The operating systems the operation works on; without the field,
    /// every system the package supports. Elsewhere a call to it is
    /// unavailable.
    #[serde(default)]
    #[schemars(with = "Option<Vec<crate::schema::PlatformId>>")]
    platforms: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProvidesJson {
    /// The capability's name, `<namespace>:<name>@<major>`.
    capability: String,
    /// The component serving it, relative to the package folder.
    component: String,
    /// The operations the capability is made of.
    operations: Vec<String>,
    /// The operating systems it works on; without the field, every system
    /// the package supports. Elsewhere the package does not provide the
    /// capability.
    #[serde(default)]
    platforms: Option<Vec<String>>,
    /// Whether the package provides it only at run time (`"atRunTime"`):
    /// only while its code holds a provision for it, as an owned
    /// registration (#158). The capability is still known from the
    /// manifest, so install plans, cycles and Settings work from the
    /// manifest alone; a provision the manifest does not declare and
    /// mark is refused.
    #[serde(default)]
    at_run_time: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UsesJson {
    /// The capability's name, `<namespace>:<name>@<major>`.
    capability: String,
    /// The operations the package calls, each reached only through the
    /// capability.
    operations: Vec<String>,
    /// Whether the package needs it (the default) or only uses it when some
    /// installed package provides it (`"optional": true`).
    #[serde(default)]
    optional: bool,
    /// Whether the package calls one provider (`"use": "one"`, the
    /// default) or every provider (`"all"`).
    #[serde(default)]
    r#use: Option<String>,
    /// A provider Pane installs when no installed package provides the
    /// capability, written as a dependency's source is.
    #[serde(default)]
    default: Option<String>,
    /// The manifest ids of the commands that need the capability; without
    /// the field, the whole package needs it.
    #[serde(default)]
    commands: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename = "Command", rename_all = "camelCase")]
pub(crate) struct CommandJson {
    /// The command's id in the package, unique among its commands and
    /// without `#`: Pane's own records name the command by it.
    id: String,
    /// The command's name in root search, in Title Case. Not empty.
    title: String,
    /// One line under the title in root search, saying what the command
    /// does. Empty when it does not say.
    #[serde(default)]
    subtitle: Option<String>,
    /// The command's own icon, drawn instead of the package's: a built-in
    /// icon's name, or an image the package ships.
    #[serde(default)]
    #[schemars(with = "Option<crate::schema::Icon>")]
    icon: Option<serde_json::Value>,
    /// The command's component, relative to the package folder: the
    /// WebAssembly component Pane runs for it. Not empty, and inside the
    /// package folder.
    component: String,
    /// The operating systems the command supports; without the field,
    /// every system the package supports. Elsewhere it is listed as
    /// unavailable.
    #[serde(default)]
    #[schemars(with = "Option<Vec<crate::schema::PlatformId>>")]
    platforms: Option<Vec<String>>,
    /// What the command does when it is launched: `view` (it opens a
    /// screen, the default), `no-view` (it runs without one) or `provider`
    /// (it only answers root search through its `rootResults` or
    /// `indexedResults`, and is never launched).
    #[serde(default)]
    #[schemars(with = "Option<CommandMode>")]
    mode: Option<String>,
    /// The command computes root results from root search's query, such as
    /// a calculator's answer: its component then also exports
    /// `pane:extension/root-results`.
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    when: Option<String>,
    #[serde(default)]
    matches: Option<String>,
    #[serde(default)]
    root_results: bool,
    /// The command supplies root results ahead of the query, such as the
    /// installed applications: its component then also exports
    /// `pane:extension/indexed-results`.
    #[serde(default)]
    indexed_results: bool,
    /// The command takes a query: text typed into root search that Pane
    /// sends it when the user invokes it through its alias or as a
    /// fallback, as its launch record's fallback text.
    #[serde(default)]
    takes_query: bool,
    /// The scheduled work the command declares, if any: every
    /// `everySeconds` seconds while the package's code may run, Pane runs
    /// the action of `item` (a view command) or the command itself in the
    /// background (a no-view command, which names no item).
    #[serde(default)]
    schedule: Option<ScheduleJson>,
    /// The command runs a continuing service: while the package's code may
    /// run, Pane calls the component's `run-cycle` export in a cycle the
    /// service itself paces, with no interval the manifest declares.
    #[serde(default)]
    service: bool,
    /// The preferences the command declares for itself, besides the
    /// package's: typed fields whose values the user sets in Pane and the
    /// command reads. A name the package or another command already
    /// declares is refused.
    #[serde(default)]
    #[schemars(with = "Vec<crate::schema::Preference>")]
    preferences: Vec<serde_json::Value>,
    /// The typed values the command asks for before each run, in the order
    /// its fields show them: at most three, each with a `name` and a `type`
    /// of `text`, `password` or `dropdown`.
    #[serde(default)]
    #[schemars(schema_with = "crate::schema::arguments")]
    arguments: Option<serde_json::Value>,
}

/// A command's `schedule`, as `pane.json` writes it.
#[derive(Deserialize, JsonSchema)]
#[serde(rename = "Schedule", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ScheduleJson {
    /// How often the work runs, in seconds: from 1 to 2,592,000 (30 days).
    every_seconds: u64,
    /// The id of the item whose action runs, for a view command: Pane asks
    /// for the command's tree and runs that item's action, as choosing it
    /// would. A no-view command names no `item`; Pane runs the command
    /// itself.
    #[serde(default)]
    item: Option<String>,
}

impl Manifest {
    /// Reads and validates `pane.json` in `folder`, including that the
    /// package supports this operating system and that every component it
    /// names is present. Runs no guest code.
    pub fn read(folder: &Path) -> Result<Manifest, PackageError> {
        Manifest::read_text(folder).map(|(manifest, _)| manifest)
    }

    /// Like [`Manifest::read`], also returning the text that was validated.
    fn read_text(folder: &Path) -> Result<(Manifest, String), PackageError> {
        let (manifest, text) = Manifest::read_parsed(folder)?;
        if let Some(reason) = platform::unavailable(manifest.platforms.as_deref(), "this package") {
            return Err(PackageError::UnsupportedPlatform(reason));
        }
        manifest.check_components(folder)?;
        manifest.check_icons(folder)?;
        Ok((manifest, text))
    }

    /// Checks that every image the package's and its commands' icons name
    /// is in `folder`, so that a package naming an image it does not ship
    /// is refused at install (#139). Built-in names were checked when the
    /// manifest was parsed.
    fn check_icons(&self, folder: &Path) -> Result<(), PackageError> {
        let package = self
            .icon
            .iter()
            .map(|icon| (icon, "the package".to_owned()));
        let commands = self.commands.iter().filter_map(|command| {
            let icon = command.icon.as_ref()?;
            Some((icon, format!("command `{}`", command.id)))
        });
        for (icon, what) in package.chain(commands) {
            icons::check_manifest_files(icon, folder, &what)
                .map_err(PackageError::InvalidManifest)?;
        }
        Ok(())
    }

    /// The files the package's and its commands' icons name, relative to
    /// the package folder, with the `@light` and `@dark` variants they may
    /// have: copied into the managed copy with the package.
    pub(crate) fn icon_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = Vec::new();
        let named = self.icon.iter().chain(
            self.commands
                .iter()
                .filter_map(|command| command.icon.as_ref()),
        );
        for file in named.flat_map(icons::package_files) {
            if !files.contains(&file) {
                files.push(file);
            }
        }
        files
    }

    /// Reads a managed copy as the installed listing does: like
    /// [`Manifest::read`], a copy for another system is read, so that its
    /// commands can be listed as unavailable. A manifest whose `apiVersion`
    /// names a version this Pane cannot run is refused as ever — the
    /// manifest half of what is returned is `Err`, so none of the package's
    /// code runs, exactly as for a copy that cannot be read — while the
    /// manifest, parsed beside the refusal, is returned with it: the
    /// extension stays listed with its title, description and commands'
    /// names, and its page says the refusal (#318, ADR 0046). A copy broken
    /// any other way, or one for a version this Pane runs, has nothing
    /// beside it.
    fn read_installed(folder: &Path) -> (Result<Manifest, PackageError>, Option<Unsupported>) {
        let text = match Manifest::read_file(folder) {
            Ok(text) => text,
            Err(error) => return (Err(error), None),
        };
        let (manifest, unsupported_api) = match Manifest::parse_installed(&text) {
            Ok(parsed) => parsed,
            Err(error) => return (Err(error), None),
        };
        // The version is decided before anything else of the copy is, as
        // every read of a package decides it: a copy whose components are
        // missing beside an unsupported version is refused for the version
        // — none of its code runs either way.
        match unsupported_api {
            Some(refusal) => (
                Err(PackageError::IncompatibleApi(refusal.clone())),
                Some(Unsupported { refusal, manifest }),
            ),
            None => match manifest.check_components(folder) {
                Ok(()) => (Ok(manifest), None),
                Err(error) => (Err(error), None),
            },
        }
    }

    /// Reads and parses `pane.json` in `folder`, without checking that its
    /// components exist (a development build is about to make them).
    pub(crate) fn read_parsed(folder: &Path) -> Result<(Manifest, String), PackageError> {
        let text = Manifest::read_file(folder)?;
        let manifest = Manifest::parse(&text)?;
        Ok((manifest, text))
    }

    /// Reads `pane.json` in `folder` as text: a missing file says the
    /// folder is not a package; any other read failure is an invalid
    /// manifest.
    fn read_file(folder: &Path) -> Result<String, PackageError> {
        let path = folder.join(MANIFEST_FILE);
        match fs::read_to_string(&path) {
            Ok(text) => Ok(text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(PackageError::NoManifest(folder.to_path_buf()))
            }
            Err(error) => Err(PackageError::InvalidManifest(error.to_string())),
        }
    }

    /// Checks that every component the manifest names is in `folder`, and
    /// that each helper's file for this system, where the package ships
    /// one, is a regular file inside the package and a program for this
    /// system ([`runner::check_file`]).
    fn check_components(&self, folder: &Path) -> Result<(), PackageError> {
        for (name, component) in self.components() {
            if !folder.join(component).is_file() {
                return Err(PackageError::MissingComponent {
                    command: name,
                    component: component.to_path_buf(),
                });
            }
        }
        let Some(target) = Target::current() else {
            return Ok(());
        };
        for helper in &self.helpers {
            let Some(file) = helper.for_this_system() else {
                continue;
            };
            if let Err(reason) = runner::check_file(folder, file, target) {
                return Err(PackageError::Helper {
                    helper: helper.id.clone(),
                    target,
                    reason,
                });
            }
        }
        Ok(())
    }

    /// Every component the manifest names, relative to the package folder,
    /// with what it serves as people know it: a command's title, "operation
    /// `<id>`" or "capability `<name>`". A component serving several appears
    /// once per use.
    pub(crate) fn components(&self) -> impl Iterator<Item = (String, &Path)> {
        let commands = self
            .commands
            .iter()
            .map(|command| (command.title.clone(), command.component.as_path()));
        let operations = self.operations.iter().map(|operation| {
            (
                format!("operation `{}`", operation.id),
                operation.component.as_path(),
            )
        });
        let capabilities = self.provides.iter().map(|provides| {
            (
                format!("capability `{}`", provides.capability),
                provides.component.as_path(),
            )
        });
        let activate = self
            .activate
            .as_deref()
            .map(|component| ("the package's activation entry point".to_owned(), component));
        commands
            .chain(operations)
            .chain(capabilities)
            .chain(activate)
    }

    /// What `component` exports besides `command`, as the manifest says.
    pub(crate) fn exports_of(&self, component: &Path) -> Exports {
        let commands = || {
            self.commands
                .iter()
                .filter(|command| command.component == component)
        };
        Exports {
            root_results: commands().any(|command| command.root_results),
            indexed_results: commands().any(|command| command.indexed_results),
            service: commands().any(|command| command.service),
            operations: self
                .operations
                .iter()
                .any(|operation| operation.component == component)
                || self
                    .provides
                    .iter()
                    .any(|provides| provides.component == component),
            activate: self.activate.as_deref() == Some(component),
        }
    }

    /// Parses `pane.json`, refusing a package built for a version of the
    /// extension API this Pane cannot run (ADR 0046): every read of a
    /// package being installed, previewed, checked or updated goes through
    /// this, so such a package is refused before any of its code runs.
    fn parse(text: &str) -> Result<Manifest, PackageError> {
        Manifest::parse_text(text, false).map(|(manifest, _)| manifest)
    }

    /// Parses `pane.json` as the installed listing reads it (#318, ADR
    /// 0046): like [`Manifest::parse`], but a manifest whose `apiVersion`
    /// names a version this Pane cannot run is parsed anyway — the
    /// extension stays listed — and the refusal is returned beside the
    /// manifest, so the caller runs none of it while showing all of it.
    /// Every other rule of [`Manifest::parse`] applies, with the same
    /// words.
    fn parse_installed(text: &str) -> Result<(Manifest, Option<UnsupportedApi>), PackageError> {
        Manifest::parse_text(text, true)
    }

    /// Parses `pane.json` — the one implementation behind
    /// [`Manifest::parse`] and [`Manifest::parse_installed`].
    /// `tolerate_unsupported_api` is whether a manifest whose `apiVersion`
    /// names a version this Pane cannot run is parsed anyway, the refusal
    /// returned beside the manifest (the installed listing's read, #318),
    /// rather than refused outright (every other read, ADR 0046); the
    /// version is checked where it always was, and every other rule is
    /// the same either way.
    fn parse_text(
        text: &str,
        tolerate_unsupported_api: bool,
    ) -> Result<(Manifest, Option<UnsupportedApi>), PackageError> {
        let invalid = |message: String| PackageError::InvalidManifest(message);
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|error| invalid(error.to_string()))?;
        // The format version is checked first: a newer manifest may not match
        // this version's fields at all.
        match value.get("manifestVersion").map(serde_json::Value::as_u64) {
            Some(Some(MANIFEST_VERSION)) => {}
            Some(Some(found)) if found > MANIFEST_VERSION => {
                return Err(PackageError::NewerManifest(found));
            }
            Some(_) => return Err(invalid("manifestVersion must be 1".into())),
            None => return Err(invalid("missing field `manifestVersion`".into())),
        }
        let json: ManifestJson =
            serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
        // The extension API version is decided before any field of the
        // manifest is validated: an unparseable one is an invalid manifest,
        // and a version this Pane cannot run is the refusal — which fails
        // this parse unless the listing reads it, keeping the manifest
        // beside the refusal instead (ADR 0046, #318).
        let unsupported_api = extension_api_refusal(&json.api_version)?;
        if let Some(refusal) = &unsupported_api
            && !tolerate_unsupported_api
        {
            return Err(PackageError::IncompatibleApi(refusal.clone()));
        }
        if json.title.trim().is_empty() {
            return Err(invalid("`title` is empty".into()));
        }
        let icon = json
            .icon
            .as_ref()
            .filter(|icon| !icon.is_null())
            .map(|icon| icons::parse_manifest_icon(icon, "the package"))
            .transpose()
            .map_err(invalid)?;
        let platforms = parse_platforms(json.platforms, "`platforms`")?;
        let package_preferences =
            preferences::parse(json.preferences, "the package", &[]).map_err(invalid)?;
        if json.commands.is_empty() && json.operations.is_empty() && json.provides.is_empty() {
            return Err(invalid(
                "`commands` is empty and the package provides no capability; declare a command, an \
                 operation or a capability it provides"
                    .into(),
            ));
        }
        let mut commands = Vec::new();
        for command in json.commands {
            if command.id.is_empty() || command.title.trim().is_empty() {
                return Err(invalid("every command needs an `id` and a `title`".into()));
            }
            // Pane's records name a command `<package identity>#<id>`; an id
            // without `#` keeps the package's part of that unambiguous.
            if command.id.contains('#') {
                return Err(invalid(format!(
                    "command id `{}` contains `#`, which command ids cannot",
                    command.id
                )));
            }
            if commands
                .iter()
                .any(|seen: &ManifestCommand| seen.id == command.id)
            {
                return Err(invalid(format!("command id `{}` is repeated", command.id)));
            }
            let icon = command
                .icon
                .as_ref()
                .filter(|icon| !icon.is_null())
                .map(|icon| icons::parse_manifest_icon(icon, &format!("command `{}`", command.id)))
                .transpose()
                .map_err(invalid)?;
            let mode = match command.mode.as_deref() {
                None | Some("view") => CommandMode::View,
                Some("no-view") => CommandMode::NoView,
                Some("provider") => CommandMode::Provider,
                Some("designed") => CommandMode::Designed,
                Some(other) => {
                    return Err(invalid(format!(
                        "command `{}` has the mode \"{}\"; a command's `mode` is \"view\" (it \
                         opens a screen of items, the default), \"no-view\" (it runs without one), \
                         \"provider\" (it only answers root search) or \"designed\" (it opens a \
                         screen the extension designs as a tree Pane renders; the `view` mode \
                         retires into it once the List and the Form move onto the tree)",
                        command.id,
                        other.escape_debug()
                    )));
                }
            };
            if mode == CommandMode::Provider {
                check_provider(&command)?;
            }
            let when = match command.when.as_deref() {
                None | Some("always") => CommandWhen::Always,
                Some("blank") => CommandWhen::Blank,
                Some("searching") => CommandWhen::Searching,
                Some(other) => {
                    return Err(invalid(format!(
                        "command `{}` has the when \"{}\"; a command's `when` is \"always\" \
                         (the default), \"blank\" (only while nothing is typed) or \
                         \"searching\" (only while something is)",
                        command.id,
                        other.escape_debug()
                    )));
                }
            };
            let matches = match command.matches.as_deref() {
                None | Some("title") => CommandMatches::Title,
                Some("url") => CommandMatches::Url,
                Some("file-path") => CommandMatches::FilePath,
                Some(other) => {
                    return Err(invalid(format!(
                        "command `{}` has the matches \"{}\"; a command's `matches` is \
                         \"title\" (the default), \"url\" or \"file-path\"",
                        command.id,
                        other.escape_debug()
                    )));
                }
            };
            let schedule = command
                .schedule
                .map(|schedule| parse_schedule(&command.id, mode, schedule))
                .transpose()?;
            let arguments = arguments::parse(&command.id, command.arguments).map_err(invalid)?;
            if schedule.is_some() {
                arguments::check_scheduled(&command.id, &arguments).map_err(invalid)?;
            }
            // A command may both be scheduled and run a continuing service;
            // they are separate activation models, and neither runs the
            // other's code.
            let service = command.service;
            let component = inside_package(&command.component, "component")?;
            let platforms = parse_platforms(
                command.platforms,
                &format!("`platforms` of command `{}`", command.id),
            )?;
            let own = preferences::parse(
                command.preferences,
                &format!("command `{}`", command.id),
                &package_preferences,
            )
            .map_err(invalid)?;
            commands.push(ManifestCommand {
                id: command.id,
                title: command.title,
                subtitle: command.subtitle,
                icon,
                component,
                platforms,
                keywords: command.keywords,
                mode,
                when,
                matches,
                root_results: command.root_results,
                indexed_results: command.indexed_results,
                takes_query: command.takes_query,
                schedule,
                service,
                preferences: own,
                arguments,
            });
        }
        let mut operations: Vec<ManifestOperation> = Vec::new();
        for operation in json.operations {
            if operation.id.is_empty() {
                return Err(invalid("every operation needs an `id`".into()));
            }
            if operations.iter().any(|seen| seen.id == operation.id) {
                return Err(invalid(format!(
                    "operation id `{}` is repeated",
                    operation.id
                )));
            }
            if operation.version == 0 {
                return Err(invalid(format!(
                    "operation `{}` has version 0; versions start at 1",
                    operation.id
                )));
            }
            let platforms = parse_platforms(
                operation.platforms,
                &format!("`platforms` of operation `{}`", operation.id),
            )?;
            operations.push(ManifestOperation {
                platforms,
                component: inside_package(&operation.component, "component")?,
                id: operation.id,
                version: operation.version,
            });
        }
        let mut helpers: Vec<ManifestHelper> = Vec::new();
        for helper in json.helpers {
            if let Some(problem) = runner::id_problem(&helper.id) {
                return Err(invalid(problem));
            }
            if helpers.iter().any(|seen| seen.id == helper.id) {
                return Err(invalid(format!("helper id `{}` is repeated", helper.id)));
            }
            if helper.targets.is_empty() {
                return Err(invalid(format!(
                    "helper `{}` has no `targets`; name its file for each system it is \
                     built for, such as \"linux-x86_64\"",
                    helper.id
                )));
            }
            let mut targets = BTreeMap::new();
            for (id, file) in helper.targets {
                let Some(target) = Target::parse(&id) else {
                    return Err(invalid(format!(
                        "unknown target `{}` of helper `{}`; use windows, macos or linux, \
                         a dash, and x86_64 or aarch64, such as \"linux-x86_64\"",
                        id.escape_debug(),
                        helper.id
                    )));
                };
                let file = inside_package(&file, "helper file")?;
                if let Some(problem) = runner::name_problem(&file, target) {
                    return Err(invalid(format!("helper `{}`: {problem}", helper.id)));
                }
                targets.insert(target, file);
            }
            helpers.push(ManifestHelper {
                id: helper.id,
                targets,
            });
        }
        let dependencies = parse_dependencies(json.dependencies)?;
        let keywords = parse_keywords(json.keywords)?;
        let provides = parse_provides(json.provides)?;
        let uses = parse_uses(json.uses, &commands)?;
        let activate = json
            .activate
            .as_deref()
            .filter(|component| !component.trim().is_empty())
            .map(|component| inside_package(component, "activate"))
            .transpose()?;
        Ok((
            Manifest {
                title: json.title,
                description: trimmed(json.description),
                author: trimmed(json.author),
                homepage: trimmed(json.homepage),
                repository: trimmed(json.repository),
                issues: trimmed(json.issues),
                license: trimmed(json.license),
                keywords,
                version: json.version,
                icon,
                api_version: json.api_version,
                platforms,
                commands,
                operations,
                helpers,
                dependencies,
                folder_access: json.folder_access,
                file_index: json.file_index,
                preferences: package_preferences,
                provides,
                uses,
                activate,
            },
            unsupported_api,
        ))
    }

    /// The dependency the package's code calls `id`.
    pub fn dependency(&self, id: &str) -> Option<&ManifestDependency> {
        self.dependencies
            .iter()
            .find(|dependency| dependency.id == id)
    }

    /// Checks the `default` of each use for a package read from `from` (npm,
    /// Git or Pane's own downloads): such a package cannot name a `local:`
    /// folder, whose path is on its author's computer, not the user's, as a
    /// dependency's source cannot.
    fn check_published_uses(&self, from: &str) -> Result<(), PackageError> {
        for used in &self.uses {
            let Some(source) = &used.default else {
                continue;
            };
            if matches!(SourceSpec::parse(source), Ok(SourceSpec::Local(_))) {
                return Err(PackageError::InvalidManifest(format!(
                    "the default `{source}` of the use of `{}` names a local folder, but a \
                     package from {from} can name only `npm:` and `git:` sources, whose \
                     packages are published, not a folder on its author's computer",
                    used.capability
                )));
            }
        }
        Ok(())
    }
}

/// Checks the `pane.json` entry of a command that says `"mode":
/// "provider"` (#164): a root provider answers root search and nothing
/// else, so it declares `rootResults` or `indexedResults`, and nothing that
/// only a launched command uses — a search of its own, a query, arguments
/// or a schedule. A continuing service and preferences are its own and
/// stay allowed.
fn check_provider(command: &CommandJson) -> Result<(), PackageError> {
    let id = &command.id;
    let refused = |what: &str| {
        Err(PackageError::InvalidManifest(format!(
            "command `{id}` is a provider (\"mode\": \"provider\"), which only answers root \
             search and is never launched, so it cannot {what}"
        )))
    };
    if !command.root_results && !command.indexed_results {
        return Err(PackageError::InvalidManifest(format!(
            "command `{id}` is a provider (\"mode\": \"provider\") but declares neither \
             `rootResults` nor `indexedResults`: a provider only answers root search, so it \
             needs one of them, or another mode (\"view\" or \"no-view\")"
        )));
    }
    if command.takes_query {
        return refused("take a query (`takesQuery`)");
    }
    let has_arguments = command.arguments.as_ref().is_some_and(|arguments| {
        !arguments.is_null() && arguments.as_array().is_none_or(|list| !list.is_empty())
    });
    if has_arguments {
        return refused("ask for arguments (`arguments`)");
    }
    if command.schedule.is_some() {
        return refused("run on a schedule (`schedule`)");
    }
    Ok(())
}

/// The `schedule` of the command with id `id` and `mode`, as `pane.json`
/// writes it, checked: a view command's names the item whose action runs;
/// a no-view command's names none, since Pane runs the command itself.
fn parse_schedule(
    id: &str,
    mode: CommandMode,
    schedule: ScheduleJson,
) -> Result<ManifestSchedule, PackageError> {
    let invalid = |message: String| PackageError::InvalidManifest(message);
    if schedule.every_seconds < MIN_SCHEDULE_SECONDS {
        return Err(invalid(format!(
            "the schedule of command `{id}` has `everySeconds` {}, below the \
             {MIN_SCHEDULE_SECONDS}-second minimum",
            schedule.every_seconds
        )));
    }
    if schedule.every_seconds > MAX_SCHEDULE_SECONDS {
        return Err(invalid(format!(
            "the schedule of command `{id}` has `everySeconds` {}, above the \
             {MAX_SCHEDULE_SECONDS}-second maximum",
            schedule.every_seconds
        )));
    }
    let item = match (mode, schedule.item) {
        // Refused before, by `check_provider`.
        (CommandMode::Provider, _) => None,
        (CommandMode::NoView, None) => None,
        (CommandMode::NoView, Some(_)) => {
            return Err(invalid(format!(
                "the schedule of command `{id}` names an `item`, but the command is no-view: it \
                 has no list, and Pane runs the command itself on its schedule; remove `item`"
            )));
        }
        (CommandMode::View | CommandMode::Designed, item) => {
            let item = item.unwrap_or_default();
            if item.trim().is_empty() {
                return Err(invalid(format!(
                    "the schedule of command `{id}` names no `item`; name the item whose action \
                     the schedule runs, or make the command no-view (\"mode\": \"no-view\") to \
                     have the schedule run the command itself"
                )));
            }
            if item.chars().count() > MAX_SCHEDULE_ITEM {
                return Err(invalid(format!(
                    "the `item` of the schedule of command `{id}` is longer than \
                     {MAX_SCHEDULE_ITEM} characters"
                )));
            }
            Some(item)
        }
    };
    Ok(ManifestSchedule {
        every_seconds: schedule.every_seconds,
        item,
    })
}

/// A package source as it is written: in a manifest's `dependencies`, or
/// by a caller naming a package by its identity. It is `local:` and a
/// folder path, `npm:` and a package name with an optional exact version,
/// or `git:` and a repository with an optional reference. This is the one
/// place such text is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SourceSpec {
    /// `local:<path>`: the path as written, not yet checked or resolved.
    Local(String),
    /// `npm:<name>[@<version>]`.
    Npm(NpmSpec),
    /// `git:<repository>[@<reference>]`.
    Git(GitSpec),
}

/// Why text is not a [`SourceSpec`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SourceError {
    /// It starts with none of `local:`, `npm:` and `git:`.
    UnknownKind,
    /// It is `npm:` and not a package name with an optional exact version,
    /// for this reason.
    Npm(String),
    /// It is `git:` and not a repository with an optional reference, for
    /// this reason.
    Git(String),
}

impl SourceSpec {
    pub(crate) fn parse(text: &str) -> Result<SourceSpec, SourceError> {
        match text.split_once(':') {
            Some(("local", path)) => Ok(SourceSpec::Local(path.to_owned())),
            Some(("npm", spec)) => NpmSpec::parse(spec)
                .map(SourceSpec::Npm)
                .map_err(SourceError::Npm),
            Some(("git", spec)) => GitSpec::parse(spec)
                .map(SourceSpec::Git)
                .map_err(SourceError::Git),
            _ => Err(SourceError::UnknownKind),
        }
    }
}

/// Checks that `source`, of dependency `id`, is either `npm:` and a package
/// name with an optional exact version (`npm:greeter@1.2.3`), or `local:` and
/// a folder path written the same way on every system: `/` between folders,
/// relative or absolute from `/`, never with `\`, a drive letter or a
/// `//server` share, which one system would read differently from another.
fn check_source(source: &str, id: &str) -> Result<(), PackageError> {
    match source_problem(source) {
        None => Ok(()),
        Some(reason) => Err(PackageError::InvalidManifest(format!(
            "the source `{source}` of dependency `{id}` {reason}"
        ))),
    }
}

/// Why `source` is not a source as a manifest writes it, as a dependency's
/// source and a use's `default` are: the reason for the caller to name in
/// its message; `None` when it is one.
fn source_problem(source: &str) -> Option<String> {
    let path = match SourceSpec::parse(source) {
        Ok(SourceSpec::Npm(_) | SourceSpec::Git(_)) => return None,
        Ok(SourceSpec::Local(path)) => path,
        Err(SourceError::Npm(why)) => return Some(format!("is not an npm package: {why}")),
        Err(SourceError::Git(why)) => return Some(format!("is not a Git repository: {why}")),
        Err(SourceError::UnknownKind) => {
            return Some(
                "must be `local:` followed by a folder path, `npm:` followed by a package name \
                 or `git:` followed by a repository; other sources are not supported yet"
                    .into(),
            );
        }
    };
    if path.is_empty() {
        return Some("must be `local:` followed by a folder path".into());
    }
    let drive =
        path.len() >= 2 && path.as_bytes()[1] == b':' && path.as_bytes()[0].is_ascii_alphabetic();
    if path.contains('\\') || drive || path.starts_with("//") {
        return Some(
            "must separate folders with `/`, without a drive letter, `\\` or a `//server` \
             share, so that every system reads it alike (such as `local:../greeter`)"
                .into(),
        );
    }
    // A `#` may name one extension of a collection at the folder the source
    // names (ADR 0044): the part after the last `#` is the id, as it is in
    // an install command's path and a `git:` source.
    if let Some((_, id)) = path.rsplit_once('#')
        && !crate::collections::is_id(id)
    {
        return Some(format!(
            "names the extension id `{id}` of a collection, which must be lowercase letters, \
             digits and `-`"
        ));
    }
    None
}

/// `text` as a manifest's optional text field holds it: trimmed, with an
/// empty answer read as the field's absence (as `description` always was).
fn trimmed(text: Option<String>) -> Option<String> {
    text.map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// The `keywords` of a manifest, checked: at most
/// [`MAX_KEYWORDS`], each trimmed, empty ones dropped (an empty entry reads
/// as its absence, as an empty `description` does).
fn parse_keywords(keywords: Vec<String>) -> Result<Vec<String>, PackageError> {
    let keywords: Vec<String> = keywords
        .into_iter()
        .map(|keyword| keyword.trim().to_owned())
        .filter(|keyword| !keyword.is_empty())
        .collect();
    if keywords.len() > MAX_KEYWORDS {
        return Err(PackageError::InvalidManifest(format!(
            "`keywords` lists {} entries; at most {} — keep it to the words a person would \
             search for",
            keywords.len(),
            MAX_KEYWORDS
        )));
    }
    Ok(keywords)
}

fn parse_dependencies(json: Vec<DependencyJson>) -> Result<Vec<ManifestDependency>, PackageError> {
    let invalid = |message: String| PackageError::InvalidManifest(message);
    let mut dependencies: Vec<ManifestDependency> = Vec::new();
    for dependency in json {
        let id = dependency.id;
        let valid_id = !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !valid_id {
            return Err(invalid(format!(
                "dependency id `{id}` must be lowercase letters, digits and `-`"
            )));
        }
        if dependencies.iter().any(|seen| seen.id == id) {
            return Err(invalid(format!("dependency id `{id}` is repeated")));
        }
        check_source(&dependency.source, &id)?;
        let mut operations: Vec<RequiredOperation> = Vec::new();
        for operation in dependency.operations {
            if operation.id.is_empty() || operation.version == 0 {
                return Err(invalid(format!(
                    "every operation of dependency `{id}` needs an `id` and a `version` from 1"
                )));
            }
            if operations.iter().any(|seen| seen.id == operation.id) {
                return Err(invalid(format!(
                    "operation `{}` of dependency `{id}` is repeated",
                    operation.id
                )));
            }
            operations.push(RequiredOperation {
                id: operation.id,
                version: operation.version,
            });
        }
        if operations.is_empty() {
            return Err(invalid(format!(
                "dependency `{id}` lists no `operations`; name those the package calls"
            )));
        }
        let platforms = parse_platforms(
            dependency.platforms,
            &format!("`platforms` of dependency `{id}`"),
        )?;
        dependencies.push(ManifestDependency {
            id,
            source: dependency.source,
            required: !dependency.optional,
            operations,
            platforms,
        });
    }
    Ok(dependencies)
}

/// Why `capability` is not a capability name, if it is not
/// `<namespace>:<name>@<major>` with lowercase letters, digits and `-` in the
/// namespace and the name and a positive major version: the one grammar of
/// capability names (ADR 0041). `None` when it is one.
pub(crate) fn capability_problem(capability: &str) -> Option<String> {
    let part = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    let problem = match capability.split_once(':') {
        None => "it has no `:` separating the namespace from the name".to_owned(),
        Some((namespace, rest)) => match rest.split_once('@') {
            None => "it has no `@` and a major version".to_owned(),
            Some((name, major)) => {
                if !major.bytes().all(|b| b.is_ascii_digit())
                    || major.parse::<u64>().is_err()
                    || major == "0"
                {
                    format!("its major version `{major}` is not a number from 1")
                } else if !part(namespace) {
                    format!("its namespace `{namespace}` must be lowercase letters, digits and `-`")
                } else if !part(name) {
                    format!("its name `{name}` must be lowercase letters, digits and `-`")
                } else {
                    return None;
                }
            }
        },
    };
    Some(format!(
        "the capability name `{capability}` is malformed ({problem}); a capability is \
         `<namespace>:<name>@<major>`, such as `acme:translate@1`, with lowercase letters, \
         digits and `-` and a major version from 1"
    ))
}

/// The operations `entry` lists, as its `operations` are written: unique and
/// named, or why the manifest is invalid. `what` names the entry in the
/// message, such as "the capability `acme:translate@1`".
fn parse_capability_operations(
    operations: Vec<String>,
    what: &str,
) -> Result<Vec<String>, PackageError> {
    let invalid = |message: String| PackageError::InvalidManifest(message);
    let mut named: Vec<String> = Vec::new();
    for operation in operations {
        if operation.is_empty() {
            return Err(invalid(format!(
                "{what} lists an operation with no name; every operation needs a name"
            )));
        }
        if named.contains(&operation) {
            return Err(invalid(format!(
                "the operation `{operation}` of {what} is listed twice"
            )));
        }
        named.push(operation);
    }
    if named.is_empty() {
        return Err(invalid(format!(
            "{what} lists no `operations`; name those it is made of"
        )));
    }
    Ok(named)
}

fn parse_provides(json: Vec<ProvidesJson>) -> Result<Vec<ManifestProvides>, PackageError> {
    let invalid = |message: String| PackageError::InvalidManifest(message);
    let mut provides: Vec<ManifestProvides> = Vec::new();
    for entry in json {
        if let Some(problem) = capability_problem(&entry.capability) {
            return Err(invalid(problem));
        }
        let what = format!("the capability `{}`", entry.capability);
        // A package may provide several majors of one capability, one entry
        // each; the same major twice would be two providers of one thing.
        if provides
            .iter()
            .any(|seen| seen.capability == entry.capability)
        {
            return Err(invalid(format!(
                "{what} is provided twice; declare each capability, at each major version, \
                 once"
            )));
        }
        let operations = parse_capability_operations(entry.operations, &what)?;
        let platforms = parse_platforms(entry.platforms, &format!("`platforms` of {what}"))?;
        provides.push(ManifestProvides {
            capability: entry.capability,
            component: inside_package(&entry.component, "component")?,
            operations,
            platforms,
            at_run_time: entry.at_run_time,
        });
    }
    Ok(provides)
}

fn parse_uses(
    json: Vec<UsesJson>,
    commands: &[ManifestCommand],
) -> Result<Vec<ManifestUse>, PackageError> {
    let invalid = |message: String| PackageError::InvalidManifest(message);
    let mut uses: Vec<ManifestUse> = Vec::new();
    for entry in json {
        if let Some(problem) = capability_problem(&entry.capability) {
            return Err(invalid(problem));
        }
        let what = format!("the use of `{}`", entry.capability);
        if uses.iter().any(|seen| seen.capability == entry.capability) {
            return Err(invalid(format!(
                "{what} is declared twice; declare each capability once, with every operation \
                 it calls"
            )));
        }
        let operations = parse_capability_operations(entry.operations, &what)?;
        let use_all = match entry.r#use.as_deref() {
            None | Some("one") => false,
            Some("all") => true,
            Some(other) => {
                return Err(invalid(format!(
                    "{what} has \"use\": \"{}\"; `use` is \"one\" (the default: call one \
                     provider) or \"all\" (call every provider)",
                    other.escape_debug()
                )));
            }
        };
        if let Some(default) = &entry.default
            && let Some(reason) = source_problem(default)
        {
            return Err(invalid(format!(
                "the default `{default}` of {what} {reason}"
            )));
        }
        let entry_commands = match entry.commands {
            None => None,
            Some(named) if named.is_empty() => {
                return Err(invalid(format!(
                    "the `commands` of {what} is empty; remove `commands` to need the \
                     capability for the whole package, or name the commands that need it"
                )));
            }
            Some(named) => {
                for command in &named {
                    if !commands.iter().any(|declared| declared.id == *command) {
                        return Err(invalid(format!(
                            "the `commands` of {what} names `{command}`, which the package \
                             does not declare as a command"
                        )));
                    }
                }
                Some(named)
            }
        };
        uses.push(ManifestUse {
            capability: entry.capability,
            operations,
            required: !entry.optional,
            use_all,
            default: entry.default,
            commands: entry_commands,
        });
    }
    Ok(uses)
}

/// `file` (a `what`, such as a component) as a path, if it is a relative
/// path inside the package folder.
fn inside_package(file: &str, what: &str) -> Result<PathBuf, PackageError> {
    let path = PathBuf::from(file);
    let inside = path
        .components()
        .all(|part| matches!(part, PathPart::Normal(_)));
    if !inside || file.is_empty() {
        return Err(PackageError::InvalidManifest(format!(
            "{what} `{file}` must be a relative path inside the package folder"
        )));
    }
    Ok(path)
}

/// Reads a `platforms` list (named `field` in explanations): `None` when
/// absent, and possibly empty, meaning no operating system.
fn parse_platforms(
    ids: Option<Vec<String>>,
    field: &str,
) -> Result<Option<Vec<Platform>>, PackageError> {
    ids.map(|ids| {
        ids.iter()
            .map(|id| {
                Platform::from_id(id).ok_or_else(|| {
                    PackageError::InvalidManifest(format!(
                        "unknown platform `{id}` in {field}; use windows, macos or linux"
                    ))
                })
            })
            .collect()
    })
    .transpose()
}

/// The extension API version `text` names, `MAJOR.MINOR` with an optional
/// patch: the patch is not part of the version (ADR 0046). `None` when
/// `text` names no version a manifest may.
fn api_version(text: &str) -> Option<(u64, u64)> {
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    match parts.next() {
        None => Some((major, minor)),
        Some(patch) => {
            (patch.parse::<u64>().is_ok() && parts.next().is_none()).then_some((major, minor))
        }
    }
}

/// The refusal for a package needing extension API `required`
/// (`MAJOR.MINOR`, with an optional `.PATCH`): `None` when this Pane runs
/// it — an extension built for the current version, and for the previous
/// one while its window lasts (ADR 0046) — and otherwise which version it
/// needs, in which direction it misses: newer than any version this Pane
/// runs, or older than every one it runs (#318). Before 1.0 each minor
/// version is its own API and the previous one is the previous minor; from
/// 1.0 an older minor version of the same major is served, and the previous
/// major during its window.
fn extension_api_refusal(required: &str) -> Result<Option<UnsupportedApi>, PackageError> {
    let Some(needed) = api_version(required) else {
        return Err(PackageError::InvalidManifest(format!(
            "apiVersion `{required}` is not a version such as `0.1`"
        )));
    };
    // A version this Pane cannot run is the refusal, with the direction it
    // misses in: newer than any version this Pane runs — a newer Pane runs
    // it — or older than every one it runs — a newer version of the
    // extension does (#318, ADR 0046).
    let refused = !runs_extension_api(EXTENSION_API, EXTENSION_API_PREVIOUS, needed);
    Ok(refused.then(|| UnsupportedApi {
        required: required.to_owned(),
        newer: needed > EXTENSION_API,
    }))
}

/// A package's need for an extension API version this Pane cannot run
/// (ADR 0046, #318): which version it needs, and whether that version is
/// newer than any this Pane runs — a newer Pane runs it — or older than
/// every one it runs — a newer version of the extension does. The refusal
/// is one text everywhere ([`UnsupportedApi::fmt`]): an install's, a
/// preview's, `pane-ext check`'s, an update check's skip, and the
/// installed listing's page and rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedApi {
    /// The version the package's manifest names, as it names it.
    pub required: String,
    /// Whether that version is newer than any this Pane runs.
    pub newer: bool,
}

impl UnsupportedApi {
    /// What needs updating, as the extension's row in the extension list
    /// and in root search says it: the direction of the refusal is the
    /// remedy.
    pub fn needs(&self) -> &'static str {
        if self.newer {
            "Needs a newer Pane"
        } else {
            "Needs a newer version of the extension"
        }
    }
}

impl fmt::Display for UnsupportedApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&refused_api_text(
            EXTENSION_API,
            EXTENSION_API_PREVIOUS,
            self,
        ))
    }
}

/// What a refusal of the extension API says to a Pane whose extension API
/// is `host`, still running `previous` while its window lasts: which
/// version the extension needs, which versions this Pane runs, and what
/// to update (ADR 0046, #318) — the direction of the refusal is the
/// remedy. Parameterized like [`runs_extension_api`] so a future window is
/// worded by the same code.
fn refused_api_text(
    host: (u64, u64),
    previous: Option<(u64, u64)>,
    refusal: &UnsupportedApi,
) -> String {
    let mut text = format!(
        "it needs Pane extension API {}, but this Pane provides {}.{}",
        refusal.required, host.0, host.1
    );
    if let Some((major, minor)) = previous {
        text.push_str(&format!(" and {major}.{minor}"));
    }
    text.push_str(if refusal.newer {
        ". Update Pane to run it"
    } else {
        ". Update the extension to a newer version"
    });
    text
}

/// Whether a Pane whose extension API is `host`, still running the previous
/// version `previous` while its window lasts, runs an extension built for
/// `required` (ADR 0046): the current version; an older minor of the same
/// major from 1.0, where an addition needs no new version; and the
/// previous version, the previous minor before 1.0. A version older than
/// that or newer than the host is refused and explained, never run.
fn runs_extension_api(
    host: (u64, u64),
    previous: Option<(u64, u64)>,
    required: (u64, u64),
) -> bool {
    if required == host {
        return true;
    }
    // From 1.0 an older minor of the same major runs: only a breaking
    // change gets a new major, so the host serves every minor before it.
    if host.0 >= 1 && required.0 == host.0 && required.1 <= host.1 {
        return true;
    }
    // The previous version, while its world lives under `wit/previous/`:
    // before 1.0 that is the previous minor, from 1.0 the previous major.
    previous == Some(required)
}

/// Why a package cannot be previewed, installed or updated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageError {
    /// The source is not a readable folder.
    NotAFolder(PathBuf, String),
    /// The folder's path cannot be recorded because it is not valid Unicode.
    NotUnicode(PathBuf),
    /// The folder has no `pane.json`.
    NoManifest(PathBuf),
    /// `pane.json` is not a valid manifest.
    InvalidManifest(String),
    /// `pane.json` uses a newer manifest format than this Pane reads.
    NewerManifest(u64),
    /// The package needs an extension API this Pane does not provide
    /// (ADR 0046, #318): which version it needs, and in which direction it
    /// misses — the direction is what to update.
    IncompatibleApi(UnsupportedApi),
    /// The package does not support this operating system; the reason says
    /// which ones it supports.
    UnsupportedPlatform(String),
    /// A component the manifest names is not in the folder.
    MissingComponent { command: String, component: PathBuf },
    /// The package ships a helper for this system whose file is missing or
    /// is not a program for this system.
    Helper {
        helper: String,
        target: Target,
        reason: String,
    },
    /// A component is present but Pane cannot run it.
    Component { command: String, error: CallError },
    /// A package with this identity is already installed.
    AlreadyInstalled(PackageIdentity),
    /// No package with this identity is installed.
    NotInstalled(PackageIdentity),
    /// Pane's managed location could not be read or written.
    Storage(String),
    /// A required dependency cannot be installed or used, for these
    /// reasons.
    Dependencies(Vec<String>),
    /// A package from npm cannot be downloaded, unpacked or installed; the
    /// message says why.
    Npm(String),
    /// A package from Git cannot be fetched, written out or installed; the
    /// message says why.
    Git(String),
    /// One extension of a collection whose id the revision's index no
    /// longer offers — renamed to `null` by its `renamed` map, or named
    /// by no entry at all (ADR 0044, #310); the message says which
    /// extension and what its collection says of it. An update that reads
    /// one reports it in its results as a notice, and the installed copy
    /// keeps running — Pane never uninstalls one silently; an install
    /// naming the id is refused, as any id a collection does not offer is.
    RemovedExtension(String),
    /// A collection (ADR 0044) cannot be read, its index is invalid, or the
    /// id the address names is not one of its extensions; the message says
    /// why.
    Collection(String),
    /// A default extension's pinned revision cannot be fetched or
    /// installed; the message says why.
    Defaults(String),
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageError::NotAFolder(path, reason) => {
                write!(f, "Cannot open {}: {reason}", path.display())
            }
            PackageError::NotUnicode(path) => write!(
                f,
                "Cannot install from {}: Pane needs a folder path that is valid Unicode",
                path.display()
            ),
            PackageError::NoManifest(path) => write!(
                f,
                "Not an extension package: {} has no {MANIFEST_FILE}",
                path.display()
            ),
            PackageError::InvalidManifest(reason) => {
                write!(f, "Invalid {MANIFEST_FILE}: {reason}")
            }
            PackageError::NewerManifest(found) => write!(
                f,
                "Incompatible package: its {MANIFEST_FILE} uses manifest version {found}, but this Pane reads version {MANIFEST_VERSION}; a newer Pane is needed"
            ),
            PackageError::IncompatibleApi(refusal) => {
                write!(f, "Incompatible package: {refusal}")
            }
            PackageError::UnsupportedPlatform(reason) => f.write_str(reason),
            PackageError::MissingComponent { command, component } => write!(
                f,
                "Not ready to run: the component {} of \"{command}\" is missing. This looks like a source-only package; build its component before installing",
                component.display()
            ),
            PackageError::Helper {
                helper,
                target,
                reason,
            } => write!(
                f,
                "Not ready to run: the package ships helper `{helper}` for {target}, but {reason}"
            ),
            PackageError::Component { command, error } => write!(f, "\"{command}\": {error}"),
            PackageError::AlreadyInstalled(identity) => write!(
                f,
                "Already installed from {identity}; use Update to replace the installed copy"
            ),
            PackageError::NotInstalled(identity) => {
                write!(f, "Nothing is installed from {identity}")
            }
            PackageError::Storage(reason) => {
                write!(f, "Could not update Pane's installed extensions: {reason}")
            }
            PackageError::Dependencies(problems) => {
                write!(f, "Nothing was installed: {}", problems.join("; "))
            }
            PackageError::Npm(message)
            | PackageError::Git(message)
            | PackageError::RemovedExtension(message)
            | PackageError::Collection(message)
            | PackageError::Defaults(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PackageError {}

/// A package in a source folder, read and validated but not installed.
#[derive(Clone, Debug)]
pub(crate) struct SourcePackage {
    pub identity: PackageIdentity,
    pub folder: PathBuf,
    pub manifest: Manifest,
    /// The `pane.json` text `manifest` was validated from; the managed copy
    /// gets exactly this, even if the source changes meanwhile.
    manifest_text: String,
    /// Where a package from npm was downloaded from; `None` for a local
    /// folder.
    pub npm: Option<NpmOrigin>,
    /// Where a package from Git was fetched from; `None` otherwise.
    pub git: Option<GitOrigin>,
    /// Where a default extension's pinned revision was fetched from;
    /// `None` otherwise.
    pub default: Option<crate::defaults::DefaultOrigin>,
    /// The id of the extension of a collection this package is, when it is
    /// one (ADR 0044): named by `#<id>` in the address or dependency source
    /// it was read from, and recorded beside its Git source fields.
    /// `None` otherwise. Where the source named an old id the collection
    /// renamed, this is the extension's current id (#310), except where an
    /// update keeps the installed copy's id (see
    /// [`SourcePackage::keeps_identity_of`]).
    pub extension: Option<String>,
    /// The identities this package's dependencies resolved to when it was
    /// planned, by dependency id, where a read resolved them (#310): a
    /// dependency's source naming an old id a collection renamed resolves
    /// to the extension's current id, which its record names so a call
    /// through it reaches the package that installed. `None` until the
    /// package is planned; [`PackageIdentity::dependency_at`]'s reading of
    /// the source stands in then, and for an unplanned reload.
    resolved: Option<Vec<(String, PackageIdentity)>>,
    /// For a package from npm or Git, its download, removed from the
    /// downloads folder once the last copy of this package is dropped.
    _download: Option<std::sync::Arc<crate::downloads::Download>>,
    /// Whether a component of it imports `wasi:http` (it can make web
    /// requests), as checking its components found; `false` until they are
    /// checked.
    pub network: bool,
    /// Whether a component of it imports `pane:extension/programs` (it can
    /// run system programs), as checking its components found; `false`
    /// until they are checked.
    pub programs: bool,
}

impl SourcePackage {
    /// Notes what checking its components found they import.
    pub(crate) fn note_imports(&mut self, checked: crate::runtime::Checked) {
        self.network = checked.network;
        self.programs = checked.programs;
    }

    /// Notes the identities this package's dependencies resolved to when
    /// it was planned, by dependency id (see the field): what its record
    /// names. For the launcher's planning, as the dependencies a read
    /// resolves are worked out ([`crate::dependencies::plan`]).
    pub(crate) fn planned_dependencies(&mut self, resolved: Vec<(String, PackageIdentity)>) {
        self.resolved = Some(resolved);
    }

    /// This package, read from a newer revision of the collection the
    /// installed copy with `identity` is one extension of, as the update
    /// of that copy: where the revision's index renamed the extension's
    /// id, the read followed the `renamed` map to the extension's current
    /// id, reading the current id's folder (ADR 0044, #310) — and the
    /// update replaces the installed copy, the same package, so the
    /// package takes the installed identity and the extension id its
    /// record keeps is the id it was installed with, as a moved folder's
    /// is. Nothing changes where the identities already agree.
    pub(crate) fn keeps_identity_of(mut self, identity: &PackageIdentity) -> SourcePackage {
        if self.identity.extension_id() != identity.extension_id() {
            self.identity = identity.clone();
            self.extension = identity.extension_id().map(ToOwned::to_owned);
        }
        self
    }

    /// The identity of the package this one names with the dependency
    /// source `source`: `local:` paths relative to its own folder as Pane
    /// resolved it — a local package's, or one extension of a local
    /// collection's own folder, not the collection's root.
    pub(crate) fn dependency_at(&self, source: &str) -> Result<PackageIdentity, PathBuf> {
        let folder = (!self.identity.is_published()).then_some(self.folder.as_path());
        self.identity.dependency_at(folder, source)
    }

    /// Reads the npm package that Pane downloaded and unpacked, as the
    /// package with its npm identity. Explains, rather than as for a folder,
    /// a tarball without `pane.json` (an ordinary npm package, which Pane
    /// does not run) and one without its built components.
    pub(crate) fn read_npm(fetched: Fetched) -> Result<SourcePackage, PackageError> {
        let Fetched { download, origin } = fetched;
        let folder = download.folder().to_path_buf();
        let name = origin.package.name.clone();
        let spec = format!("{name}@{}", origin.package.version);
        let (manifest, manifest_text) = match Manifest::read_text(&folder) {
            Ok(read) => read,
            Err(PackageError::NoManifest(_)) => {
                return Err(PackageError::Npm(format!(
                    "npm package {spec} is not a Pane extension: it has no {MANIFEST_FILE}. Pane \
                     installs npm packages published as Pane extensions (a {MANIFEST_FILE} and \
                     the WebAssembly components it names); it does not run other npm packages, \
                     which need Node.js and npm"
                )));
            }
            Err(PackageError::MissingComponent { command, component }) => {
                let scripts = match origin.scripts.as_slice() {
                    [] => String::new(),
                    scripts => format!(
                        " (its package.json has {}, which Pane never runs)",
                        crate::platform::join(
                            &scripts.iter().map(|s| format!("`{s}`")).collect::<Vec<_>>()
                        )
                    ),
                };
                return Err(PackageError::Npm(format!(
                    "npm package {spec} was published without the built component {} of \
                     \"{command}\": its author must build it and include it in the package \
                     before publishing. Pane does not build npm packages or run their install \
                     scripts{scripts}",
                    component.display()
                )));
            }
            Err(error) => return Err(error),
        };
        manifest.check_published_uses("npm")?;
        Ok(SourcePackage {
            identity: PackageIdentity::npm(&name),
            folder,
            manifest,
            manifest_text,
            npm: Some(origin),
            git: None,
            default: None,
            extension: None,
            _download: Some(std::sync::Arc::new(download)),
            network: false,
            programs: false,
            resolved: None,
        })
    }

    /// Reads the revision of a Git package that Pane fetched and wrote out,
    /// as the package with its Git identity. Explains, rather than as for a
    /// folder, a revision without `pane.json` at the repository's root and
    /// one without its built components (a source-only revision), and a
    /// component stored with Git LFS.
    ///
    /// A root holding `pane-collection.json` is a **collection** (ADR 0044):
    /// `extension` names the one extension of it to read, by the id the
    /// address gave after `#`; without one, the revision is explained as a
    /// collection whose extension must be named. The id resolves through
    /// the index's `renamed` map where the index lists no extension with it
    /// (#310): an old id names the extension's current one, whose folder is
    /// read and whose id the identity takes, so an address naming the old
    /// id installs the extension under its current one; one the map maps
    /// to `null`, or names no entry at all, the revision no longer offers,
    /// and is refused saying so. The extension's folder, as the index
    /// names it, is read as a local package's is: only the files under it
    /// reach the managed copy, so a component outside it is missing and
    /// the revision is explained as source-only.
    pub(crate) fn read_git(
        fetched: crate::git::Fetched,
        extension: Option<&str>,
    ) -> Result<SourcePackage, PackageError> {
        let crate::git::Fetched {
            download,
            origin,
            // The whole of the one extension this address named was
            // fetched, where the server filters (#311): nothing is left to
            // fetch, so no handle is kept.
            partial: _,
        } = fetched;
        Self::read_git_revision(download, origin, extension)
    }

    /// Reads a revision Pane fetched from Git and still holds, exactly as
    /// [`SourcePackage::read_git`] reads the one it just fetched: the
    /// choice holds the revision while it lists a collection's extensions
    /// and reads each of them from it (#308), so the revision is fetched
    /// once however many of them are installed.
    pub(crate) fn read_git_revision(
        download: std::sync::Arc<crate::downloads::Download>,
        origin: crate::git::GitOrigin,
        extension: Option<&str>,
    ) -> Result<SourcePackage, PackageError> {
        let folder = download.folder().to_path_buf();
        let revision = format!(
            "{} (commit {}) of the Git repository {}",
            origin.revision.describe(),
            origin.revision.short_commit(),
            origin.repository.name()
        );
        let revision = capitalized(&revision);
        let one_extension = folder.join(MANIFEST_FILE).is_file();
        let invalid = |why: String| {
            PackageError::Git(format!(
                "{revision} is a collection whose {} is invalid: {why}",
                crate::collections::COLLECTION_FILE
            ))
        };
        if let Some(collection) = crate::collections::read(&folder).map_err(invalid)? {
            if one_extension {
                return Err(PackageError::Git(format!(
                    "{revision} holds both {MANIFEST_FILE} and {}: a repository is one \
                     extension or a collection, never both",
                    crate::collections::COLLECTION_FILE
                )));
            }
            let name = origin.repository.name();
            let Some(id) = extension else {
                return Err(PackageError::Git(format!(
                    "{revision} is a collection, not one extension: its root holds {}, which \
                     lists the extensions it offers by id; name the one to install after `#` in \
                     the address, as {name}#<id>",
                    crate::collections::COLLECTION_FILE
                )));
            };
            // The id resolves through the index (ADR 0044, #310): where the
            // index lists no extension with it, the `renamed` map may name
            // its current id, followed through the chain — the extension's
            // package is read from its current id's folder, and the package
            // takes the current id's identity, so an address naming the old
            // id installs the extension under its current one. An id the
            // map maps to `null`, or names no entry at all, is one the
            // revision no longer offers.
            let Some(Resolved {
                extension: entry,
                id,
            }) = collection.resolve(id)
            else {
                let why = if matches!(collection.renamed().get(id), Some(None)) {
                    format!(
                        "{revision} no longer offers the extension `{id}`: its {} maps it to \
                         `null`, one that was removed",
                        crate::collections::COLLECTION_FILE
                    )
                } else {
                    format!(
                        "{revision} lists no extension `{id}` in its {}",
                        crate::collections::COLLECTION_FILE
                    )
                };
                return Err(PackageError::RemovedExtension(why));
            };
            let subfolder = folder.join(&entry.path);
            let (manifest, manifest_text) = match Manifest::read_text(&subfolder) {
                Ok(read) => read,
                Err(PackageError::NoManifest(_)) => {
                    return Err(PackageError::Git(format!(
                        "{revision} lists its extension `{id}` at `{}`, which is not a package: \
                         it has no {MANIFEST_FILE}",
                        shown(&entry.path)
                    )));
                }
                Err(PackageError::MissingComponent { command, component }) => {
                    return Err(PackageError::Git(format!(
                        "{revision} holds only the source of \"{command}\", the extension `{id}` \
                         of the collection: its built component {} is not in it. Pane does not \
                         build packages from Git or run anything in a repository; install a \
                         release revision whose commit includes the built components (its \
                         author's release tag or branch), or build it yourself and install the \
                         folder",
                        component.display()
                    )));
                }
                Err(error) => return Err(error),
            };
            for (_, component) in manifest.components() {
                let path = format!(
                    "{}/{}",
                    entry.path,
                    component.to_string_lossy().replace('\\', "/")
                );
                if origin.lfs_pointers.contains(&path) {
                    return Err(PackageError::Git(format!(
                        "{revision} stores its component {path} with Git LFS, which Pane does not \
                         fetch: its author must commit the built component itself in a release \
                         revision"
                    )));
                }
            }
            return Ok(SourcePackage {
                identity: PackageIdentity::git_extension(&origin.repository, &id),
                folder: subfolder,
                manifest,
                manifest_text,
                npm: None,
                git: Some(origin),
                default: None,
                extension: Some(id),
                _download: Some(download),
                network: false,
                programs: false,
                resolved: None,
            });
        }
        // One extension at the root, or none: `#<id>` names no extension of
        // a repository that is not a collection.
        if extension.is_some() && one_extension {
            return Err(PackageError::Git(format!(
                "{revision} is not a collection: its root holds {MANIFEST_FILE}, one extension, \
                 and no {}; `#<id>` names one extension of a collection",
                crate::collections::COLLECTION_FILE
            )));
        }
        let (manifest, manifest_text) = match Manifest::read_text(&folder) {
            Ok(read) => read,
            Err(PackageError::NoManifest(_)) => {
                return Err(PackageError::Git(format!(
                    "{revision} is not a Pane extension: it has no {MANIFEST_FILE} at the \
                     repository's root. Pane installs a repository whose root holds a \
                     {MANIFEST_FILE} and the built WebAssembly components it names"
                )));
            }
            Err(PackageError::MissingComponent { command, component }) => {
                return Err(PackageError::Git(format!(
                    "{revision} holds only the source of \"{command}\": its built component {} \
                     is not in it. Pane does not build packages from Git or run anything in a \
                     repository; install a release revision whose commit includes the built \
                     components (its author's release tag or branch), or build it yourself and \
                     install the folder",
                    component.display()
                )));
            }
            Err(error) => return Err(error),
        };
        manifest.check_published_uses("Git")?;
        for (_, component) in manifest.components() {
            let path = component.to_string_lossy().replace('\\', "/");
            if origin.lfs_pointers.contains(&path) {
                return Err(PackageError::Git(format!(
                    "{revision} stores its component {path} with Git LFS, which Pane does not \
                     fetch: its author must commit the built component itself in a release \
                     revision"
                )));
            }
        }
        Ok(SourcePackage {
            identity: PackageIdentity::git(&origin.repository),
            folder,
            manifest,
            manifest_text,
            npm: None,
            git: Some(origin),
            default: None,
            extension: None,
            _download: Some(download),
            network: false,
            programs: false,
            resolved: None,
        })
    }

    /// Reads the pinned revision of a default extension that Pane fetched
    /// and wrote out, as the package with the default extension's
    /// identity, with its Git source recorded. A revision is read and
    /// checked as a package from a folder is, so one without `pane.json`
    /// at the repository's root or without its built components (a
    /// source-only revision) is explained as the Git package's is.
    pub(crate) fn read_default(
        fetched: crate::defaults::Fetched,
    ) -> Result<SourcePackage, PackageError> {
        let crate::defaults::Fetched { download, origin } = fetched;
        let folder = download.folder().to_path_buf();
        let revision = format!(
            "{} (commit {}) of the Git repository {}",
            origin.revision.describe(),
            origin.revision.short_commit(),
            origin.repository.name()
        );
        let revision = capitalized(&revision);
        let (manifest, manifest_text) = match Manifest::read_text(&folder) {
            Ok(read) => read,
            Err(PackageError::NoManifest(_)) => {
                return Err(PackageError::Defaults(format!(
                    "{revision} is not a Pane extension: it has no {MANIFEST_FILE} at the \
                     repository's root. Pane installs a repository whose root holds a \
                     {MANIFEST_FILE} and the built WebAssembly components it names"
                )));
            }
            Err(PackageError::MissingComponent { command, component }) => {
                return Err(PackageError::Defaults(format!(
                    "{revision} holds only the source of \"{command}\": its built component {} \
                     is not in it. Pane does not build packages from Git or run anything in a \
                     repository; a release must include the built components, and until its \
                     repository releases one, Pane cannot install this default extension",
                    component.display()
                )));
            }
            Err(error) => return Err(error),
        };
        manifest.check_published_uses("Pane's own downloads")?;
        for (_, component) in manifest.components() {
            let path = component.to_string_lossy().replace('\\', "/");
            if origin.lfs_pointers.contains(&path) {
                return Err(PackageError::Defaults(format!(
                    "{revision} stores its component {path} with Git LFS, which Pane does not \
                     fetch: its repository must commit the built component itself in a \
                     release revision"
                )));
            }
        }
        Ok(SourcePackage {
            identity: PackageIdentity::default_extension(&origin.id),
            folder,
            manifest,
            manifest_text,
            npm: None,
            git: None,
            default: Some(origin),
            extension: None,
            _download: Some(download),
            network: false,
            programs: false,
            resolved: None,
        })
    }

    /// What identifies the download this package was read from, when it
    /// was downloaded: its npm tarball's integrity, its Git commit, or
    /// the default extension's pinned commit. A new download with the
    /// same `pane.json` is another plan.
    pub(crate) fn fingerprint(&self) -> Option<String> {
        match (&self.npm, &self.git, &self.default) {
            (Some(npm), _, _) => Some(npm.integrity.clone()),
            (None, Some(git), _) => Some(git.revision.commit.clone()),
            (None, None, Some(default)) => Some(default.revision.commit.clone()),
            (None, None, None) => None,
        }
    }

    /// Reads the package staged in `folder`, such as a development build,
    /// as the package with the source `identity`.
    pub fn read_staged(
        folder: &Path,
        identity: PackageIdentity,
    ) -> Result<SourcePackage, PackageError> {
        let (manifest, manifest_text) = Manifest::read_text(folder)?;
        Ok(SourcePackage {
            identity,
            folder: folder.to_path_buf(),
            manifest,
            manifest_text,
            npm: None,
            git: None,
            default: None,
            extension: None,
            _download: None,
            network: false,
            programs: false,
            resolved: None,
        })
    }

    pub fn read(folder: &Path) -> Result<SourcePackage, PackageError> {
        let identity = PackageIdentity::local(folder)?;
        let folder = identity
            .local_folder()
            .expect("a local identity has a folder")
            .to_path_buf();
        // A root holding `pane-collection.json` is a collection (ADR 0044):
        // one of its extensions must be named by its id, after `#` in the
        // address — [`SourcePackage::read_collection`] reads one. A root
        // holding both files is refused.
        let invalid = |why: String| {
            PackageError::Collection(format!(
                "The folder {} is a collection whose {} is invalid: {why}",
                folder.display(),
                crate::collections::COLLECTION_FILE
            ))
        };
        if crate::collections::read(&folder)
            .map_err(invalid)?
            .is_some()
        {
            let file = crate::collections::COLLECTION_FILE;
            if folder.join(MANIFEST_FILE).is_file() {
                return Err(PackageError::Collection(format!(
                    "The folder {} holds both {MANIFEST_FILE} and {file}: a folder is one \
                     extension or a collection, never both",
                    folder.display()
                )));
            }
            return Err(PackageError::Collection(format!(
                "The folder {} is a collection, not one extension: its root holds {file}, which \
                 lists the extensions it offers by id; name the one to install after `#`, as \
                 {}#<id>",
                folder.display(),
                folder.display()
            )));
        }
        let (manifest, manifest_text) = Manifest::read_text(&folder)?;
        Ok(SourcePackage {
            identity,
            folder,
            manifest,
            manifest_text,
            npm: None,
            git: None,
            default: None,
            extension: None,
            _download: None,
            network: false,
            programs: false,
            resolved: None,
        })
    }

    /// Reads the extension `id` of the collection at `folder`, as a local
    /// package is read: from the folder the collection's index names for
    /// it, self-contained, with the identity of the collection folder and
    /// the id (ADR 0044) — the id the index resolves for it, so an old id
    /// the collection renamed reads the extension under its current one
    /// (#310). Nothing in the folder runs.
    pub(crate) fn read_collection(folder: &Path, id: &str) -> Result<SourcePackage, PackageError> {
        let (identity, _root, subfolder) = collection_extension(folder, id)?;
        let (manifest, manifest_text) = Manifest::read_text(&subfolder)?;
        Ok(SourcePackage {
            identity,
            folder: subfolder,
            manifest,
            manifest_text,
            npm: None,
            git: None,
            default: None,
            extension: Some(id.to_owned()),
            _download: None,
            network: false,
            programs: false,
            resolved: None,
        })
    }

    /// The `pane.json` text the manifest was read from.
    pub(crate) fn manifest_text(&self) -> &str {
        &self.manifest_text
    }
}

/// An installed extension built for a version of Pane's extension API
/// this Pane cannot run (ADR 0046, #318). The package's `manifest` is
/// `Err(IncompatibleApi)` — exactly as for a copy that cannot be read, so
/// none of its code runs and nothing counts it towards pausing — while
/// the manifest, parsed with the version tolerated, is kept here beside
/// the refusal: the extension stays listed in Settings with its title,
/// description, icon and commands' names, and its page carries the
/// refusal in full. It resolves by itself when a version this Pane can run
/// arrives — the extension is updated, or Pane is updated and restarted
/// — and never survives the copy that has it.
#[derive(Clone, Debug)]
pub(crate) struct Unsupported {
    /// Which version the extension needs, and the direction of the
    /// refusal: what to update.
    pub(crate) refusal: UnsupportedApi,
    /// The manifest, parsed with the `apiVersion` tolerated: what the
    /// listing shows of the extension.
    pub(crate) manifest: Manifest,
}

impl Unsupported {
    /// The refusal as the error about the package, the one text every
    /// surface shows of it: an install's, a preview's, `pane-ext check`'s,
    /// an update check's skip, and the installed listing's page and rows
    /// (#318, ADR 0046).
    pub(crate) fn package_error(&self) -> PackageError {
        PackageError::IncompatibleApi(self.refusal.clone())
    }
}

/// One extension of a collection as the choice lists it (ADR 0044, #308):
/// what its own manifest says of it — the index holds none of that — read
/// leniently, without checking what the manifest names. An extension whose
/// manifest cannot be taken at all (it has none, or names an extension API
/// this Pane cannot run, ADR 0046) is listed anyway, its id standing for
/// its title, and what is wrong with it is explained when it is previewed
/// or installed: the strict read there refuses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ListedExtension {
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) version: Option<String>,
    /// Its manifest's own icon, resolved against its folder in the
    /// collection; `None` for none, which the choice shows as a
    /// first-letter tile.
    pub(crate) icon: Option<Icon>,
}

impl ListedExtension {
    /// The extension `entry` of the collection at `root`: its own
    /// manifest's title, description, version and icon, read leniently.
    pub(crate) fn of(root: &Path, entry: &crate::collections::Extension) -> ListedExtension {
        let subfolder = root.join(&entry.path);
        let manifest: Option<serde_json::Value> = fs::read_to_string(subfolder.join(MANIFEST_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        let field = |name: &str| {
            manifest
                .as_ref()
                .and_then(|manifest| manifest.get(name))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        ListedExtension {
            title: field("title"),
            description: field("description"),
            version: field("version"),
            icon: manifest
                .as_ref()
                .and_then(|manifest| manifest.get("icon"))
                .and_then(crate::icons::read)
                .and_then(|icon| icon.resolved(&subfolder)),
        }
    }
}

/// The extension `id` of the collection at `folder`, as an install
/// resolves one (#307): its identity — the collection's resolved folder
/// with the id (ADR 0044) — the collection's folder, and the folder
/// holding the extension's package, which is checked to be a package
/// folder but not read, so a development build of it is what is checked
/// ([`crate::develop::target`]). Refused every way an install of one
/// extension is.
///
/// The id resolves through the index's `renamed` map where the index
/// lists no extension with it (#310): an old id names the extension's
/// current one, whose folder is read and whose id the identity takes.
/// A tuple: the identity, the collection's folder, the extension's.
pub(crate) fn collection_extension(
    folder: &Path,
    id: &str,
) -> Result<(PackageIdentity, PathBuf, PathBuf), PackageError> {
    if !crate::collections::is_id(id) {
        return Err(PackageError::Collection(format!(
            "the extension id `{id}` of a collection must be lowercase letters, digits and `-`"
        )));
    }
    // The collection's folder, resolved as any local package's folder is.
    let root = PackageIdentity::local(folder)?
        .local_folder()
        .expect("a local identity has a folder")
        .to_path_buf();
    let invalid = |why: String| {
        PackageError::Collection(format!(
            "The collection at {} holds an invalid {}: {why}",
            root.display(),
            crate::collections::COLLECTION_FILE
        ))
    };
    let file = crate::collections::COLLECTION_FILE;
    let one_extension = root.join(MANIFEST_FILE).is_file();
    let Some(collection) = crate::collections::read(&root).map_err(invalid)? else {
        if one_extension {
            return Err(PackageError::Collection(format!(
                "The folder {} is not a collection: its root holds {MANIFEST_FILE}, one \
                 extension, and no {file}; `#` names one extension of a collection",
                root.display()
            )));
        }
        return Err(PackageError::Collection(format!(
            "The folder {} is not a collection: it holds no {file}; `#` names one extension \
             of a collection",
            root.display()
        )));
    };
    if one_extension {
        return Err(PackageError::Collection(format!(
            "The folder {} holds both {MANIFEST_FILE} and {file}: a folder is one extension \
             or a collection, never both",
            root.display()
        )));
    }
    // The id resolves through the index (ADR 0044, #310): an old id the
    // `renamed` map names resolves to the extension's current one,
    // which the identity takes; one mapped to `null`, or named by no
    // entry at all, the collection no longer offers.
    let Some(Resolved {
        extension: entry,
        id,
    }) = collection.resolve(id)
    else {
        let why = if matches!(collection.renamed().get(id), Some(None)) {
            format!(
                "The collection at {} no longer offers the extension `{id}`: its {file} \
                 maps it to `null`, one that was removed",
                root.display()
            )
        } else {
            format!(
                "The collection at {} lists no extension `{id}` in its {file}",
                root.display()
            )
        };
        return Err(PackageError::Collection(why));
    };
    let identity = PackageIdentity::local_extension(folder, &id)?;
    let subfolder = root.join(&entry.path);
    if !subfolder.join(MANIFEST_FILE).is_file() {
        return Err(PackageError::Collection(format!(
            "The collection at {} lists its extension `{id}` at `{}`, which is not a package: \
             it has no {MANIFEST_FILE}",
            root.display(),
            shown(&entry.path)
        )));
    }
    Ok((identity, root, subfolder))
}
/// An installed package, as read from its managed copy.
#[derive(Clone, Debug)]
pub struct InstalledPackage {
    pub identity: PackageIdentity,
    /// The manifest of the managed copy, or why it could not be read.
    pub manifest: Result<Manifest, PackageError>,
    /// The version refusal and manifest beside it, for an extension built
    /// for a version this Pane cannot run (see [`Unsupported`]): `manifest`
    /// refuses it, and this keeps it listed. `None` for every other copy.
    pub(crate) unsupported: Option<Unsupported>,
    /// Where Pane keeps this package's files.
    pub location: PathBuf,
    /// Whether the user has left the package enabled. A disabled package
    /// contributes no commands and runs nothing, but keeps its settings.
    pub enabled: bool,
    /// For a package from npm, the npm version installed and whether it is
    /// pinned to it.
    pub npm: Option<NpmPackage>,
    /// For a package from Git, the address it was fetched from and the
    /// revision installed.
    pub git: Option<InstalledGit>,
    /// For a default extension, the Git source its record keeps: the
    /// repository it was fetched from, the release tag and commit of the
    /// revision installed, and the version its manifest declared — what
    /// the updater reads to check the repository's newer release tags
    /// ([#269](https://github.com/pane-app/pane/issues/269)). `None` for
    /// a record an older Pane wrote from its own downloads, which kept
    /// no repository.
    pub default: Option<crate::defaults::InstalledDefault>,
    /// Whether a component of it imports `wasi:http`, so its code can make
    /// web requests, as found when it was installed, updated or reloaded.
    pub uses_network: bool,
    /// Whether a component of it imports `pane:extension/programs`, so its
    /// code can run system programs, as found when it was installed,
    /// updated or reloaded.
    pub uses_programs: bool,
    /// The identity each dependency the manifest declares was resolved to
    /// when the package was installed, by dependency id.
    dependencies: Vec<(String, PackageIdentity)>,
    /// The package's icon and its commands' own, resolved in the managed
    /// copy when it was read (#139).
    icons: PackageIcons,
    /// The manifest ids of the commands the user turned off on the
    /// package's page in Settings (#168): a disabled command is not
    /// offered — no root search row, no results, no alias, hotkey,
    /// schedule or service — while the rest of its package works.
    disabled_commands: BTreeSet<String>,
}

/// An installed package's icon and its commands' own, as Pane draws them.
#[derive(Clone, Debug)]
struct PackageIcons {
    /// The package's icon, or its first-letter tile.
    package: Icon,
    /// Each command with an icon of its own, by manifest id.
    commands: Vec<(String, Icon)>,
}

impl PackageIcons {
    /// The icons of the package titled `title` whose managed copy is at
    /// `location`, as `manifest` names them. An icon whose image is gone
    /// from the copy is drawn as its fallback, or the package's.
    fn of(manifest: Option<&Manifest>, title: &str, location: &Path) -> PackageIcons {
        let package = manifest
            .and_then(|manifest| manifest.icon.clone())
            .and_then(|icon| icon.resolved(location))
            .unwrap_or_else(|| Icon::letter_of(title));
        let commands = manifest
            .map(|manifest| {
                manifest
                    .commands
                    .iter()
                    .filter_map(|command| {
                        let icon = command.icon.clone()?.resolved(location)?;
                        Some((command.id.clone(), icon))
                    })
                    .collect()
            })
            .unwrap_or_default();
        PackageIcons { package, commands }
    }
}

impl InstalledPackage {
    /// The extension API version this extension was built for, when it is
    /// the previous one this Pane still runs through its adapter (ADR
    /// 0046): the version its manifest names, as it names it, for the
    /// mark its page shows. `None` for an extension built for the current
    /// version — and for one whose manifest this Pane refuses, whose page
    /// says the refusal instead.
    pub fn previous_api(&self) -> Option<String> {
        let manifest = self.manifest.as_ref().ok()?;
        let version = api_version(&manifest.api_version)?;
        (EXTENSION_API_PREVIOUS == Some(version)).then(|| manifest.api_version.clone())
    }

    /// The package `record` in `installed.json` says is installed, whose
    /// managed copy is at `location` — its source, its enabled state,
    /// what it was installed from — with the identities its dependencies
    /// were resolved to as the record says; one not recorded (installed
    /// before Pane recorded them) is resolved now.
    fn load(record: &RecordJson, location: PathBuf) -> InstalledPackage {
        let identity = PackageIdentity(record.source.clone());
        let (manifest, unsupported) = Manifest::read_installed(&location);
        let dependencies = match &manifest {
            Ok(manifest) => manifest
                .dependencies
                .iter()
                .filter_map(|dependency| {
                    let found = record.dependencies.iter().find(|r| r.id == dependency.id);
                    let resolved = match found {
                        Some(recorded) => PackageIdentity(recorded.source.clone()),
                        None => identity.dependency(&dependency.source).ok()?,
                    };
                    Some((dependency.id.clone(), resolved))
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        let npm = record.npm.as_ref().and_then(|npm| npm.package(&identity));
        let git = record.git.as_ref().and_then(|git| git.installed(&identity));
        // A default extension's record keeps its Git fields in the same
        // shape a Git package's does (see `RecordJson::git`), beside the
        // version its manifest declared.
        let default = record
            .git
            .as_ref()
            .and_then(|git| git.installed_default(record.default_version.as_deref(), &identity));
        let mut package = InstalledPackage {
            manifest,
            unsupported,
            identity,
            location,
            enabled: !record.disabled,
            npm,
            git,
            default,
            uses_network: record.network,
            uses_programs: record.programs,
            dependencies,
            icons: PackageIcons {
                package: Icon::letter_of(""),
                commands: Vec::new(),
            },
            disabled_commands: record.disabled_commands.iter().cloned().collect(),
        };
        package.icons = PackageIcons::of(package.listed(), &package.title(), &package.location);
        package
    }

    /// The manifest to read the extension's own facts from: its managed
    /// copy's when this Pane can run it, or the one parsed beside a
    /// version refusal when it cannot, so the listing shows the same
    /// title, description, icon and version either way (#318, ADR 0046).
    /// `None` only for a copy broken another way, whose facts cannot be
    /// known.
    fn listed(&self) -> Option<&Manifest> {
        match &self.unsupported {
            Some(unsupported) => Some(&unsupported.manifest),
            None => self.manifest.as_ref().ok(),
        }
    }

    /// The extension API version refusal when this Pane cannot run the
    /// extension (see [`Unsupported`]): `None` when it can.
    pub fn unsupported_api(&self) -> Option<&UnsupportedApi> {
        self.unsupported
            .as_ref()
            .map(|unsupported| &unsupported.refusal)
    }

    /// The identity of the package this one's code calls by the dependency
    /// id `id`, as resolved when it was installed; `None` if its manifest
    /// declares no such dependency, or not one from a local folder.
    pub fn dependency_identity(&self, id: &str) -> Option<&PackageIdentity> {
        self.dependencies
            .iter()
            .find(|(declared, _)| declared == id)
            .map(|(_, identity)| identity)
    }

    /// The package's display title.
    pub fn title(&self) -> String {
        match self.listed() {
            Some(manifest) => manifest.title.clone(),
            None => match (
                self.identity.local_folder(),
                self.identity.npm_name(),
                self.identity.git_repository(),
            ) {
                (Some(folder), ..) => folder_name(folder),
                (None, Some(name), _) => name.to_owned(),
                (None, None, Some(repository)) => repository.to_owned(),
                (None, None, None) => self.identity.to_string(),
            },
        }
    }

    pub fn version(&self) -> Option<String> {
        self.listed().and_then(|manifest| manifest.version.clone())
    }

    /// The package's icon as Pane draws it (#139): its manifest's, or a
    /// tile with its title's first letter when it has none.
    pub fn icon(&self) -> &Icon {
        &self.icons.package
    }

    /// The icon of this package's command with manifest id `command`: its
    /// own, else the package's.
    pub fn command_icon(&self, command: &str) -> &Icon {
        self.icons
            .commands
            .iter()
            .find(|(id, _)| id == command)
            .map_or(&self.icons.package, |(_, icon)| icon)
    }

    /// Every command this package declares, its root providers included:
    /// what its components serve. Root search lists only
    /// [`InstalledPackage::launchable_commands`].
    pub fn commands(&self) -> Vec<CommandRegistration> {
        self.available_commands()
            .into_iter()
            .map(|(command, _)| command)
            .collect()
    }

    /// Whether this package's command with manifest id `command` is a root
    /// provider (`"mode": "provider"`, #164): it answers root search but is
    /// never launched, so it has no row, pin, alias, fallback or hotkey.
    pub fn is_provider(&self, command: &str) -> bool {
        self.mode_of(command) == CommandMode::Provider
    }

    /// This package's root providers (see [`InstalledPackage::is_provider`]),
    /// in manifest order, whether the user turned them off or not: what
    /// its page in Settings lists with only their switch.
    pub fn providers(&self) -> Vec<CommandRegistration> {
        self.listed_commands()
            .into_iter()
            .filter(|command| command.mode == CommandMode::Provider)
            .map(|command| command.registration)
            .collect()
    }

    /// The commands this package offers to be launched — root search's
    /// rows, pins, aliases, fallbacks, hotkeys and launches from other
    /// commands — each with why it is unavailable on this system, if it
    /// is: every command the user left on but its root providers.
    pub(crate) fn launchable_commands(&self) -> Vec<(CommandRegistration, Option<String>)> {
        self.available_commands()
            .into_iter()
            .filter(|(command, _)| !self.is_provider(command.manifest_id()))
            .collect()
    }

    /// Every command this package offers, root providers included, each
    /// with why it is unavailable on this system, if it is: first because
    /// the package does not support this system, else because the command
    /// does not. A command the user turned off is not offered (see
    /// [`InstalledPackage::listed_commands`], which lists it).
    pub(crate) fn available_commands(&self) -> Vec<(CommandRegistration, Option<String>)> {
        self.listed_commands()
            .into_iter()
            .filter(|command| command.enabled)
            .map(|command| (command.registration, command.unavailable))
            .collect()
    }

    /// Whether the user left this package's command with manifest id
    /// `command` on (#168): every command is, until it is turned off on
    /// the package's page in Settings.
    pub fn command_enabled(&self, command: &str) -> bool {
        !self.disabled_commands.contains(command)
    }

    /// Turns this package's command with manifest id `command` on or off
    /// in Pane, without recording it (see [`Store::set_command_enabled`]).
    pub(crate) fn set_command_enabled(&mut self, command: &str, enabled: bool) {
        if enabled {
            self.disabled_commands.remove(command);
        } else {
            self.disabled_commands.insert(command.to_owned());
        }
    }

    /// The description its manifest gives (#168), if it gives one.
    pub fn description(&self) -> Option<&str> {
        self.listed()
            .and_then(|manifest| manifest.description.as_deref())
    }

    /// What the extension's page in Settings says under its title
    /// (#224): the description its manifest gives, or, for an extension of
    /// one command that gives none, that command's subtitle. Read from the
    /// manifest the listing keeps, so an extension built for a version
    /// this Pane cannot run says it still (#318) — its commands are
    /// offered nowhere, but its page stays.
    pub fn page_description(&self) -> Option<String> {
        let manifest = self.listed()?;
        if let Some(description) = &manifest.description {
            return Some(description.clone());
        }
        match manifest.commands.as_slice() {
            [only] => only
                .subtitle
                .clone()
                .filter(|subtitle| *subtitle != manifest.title),
            _ => None,
        }
    }

    /// Every command of this package's manifest, in its order, whether
    /// the user turned it off or not, as the package's page in Settings
    /// lists them: each with why it is unavailable on this system, if it
    /// is, and whether it is on. The commands Pane offers are the ones on
    /// ([`InstalledPackage::commands`]).
    pub fn listed_commands(&self) -> Vec<ListedCommand> {
        let Ok(manifest) = &self.manifest else {
            return Vec::new();
        };
        let package = platform::unavailable(manifest.platforms.as_deref(), "this package");
        manifest
            .commands
            .iter()
            .map(|command| {
                let registration = CommandRegistration {
                    id: self.identity.command_id(&command.id),
                    title: command.title.clone(),
                    subtitle: command
                        .subtitle
                        .clone()
                        .or_else(|| Some(manifest.title.clone())),
                    component: self.location.join(&command.component),
                    takes_query: command.accepts_fallback_text(),
                    keywords: command.keywords.clone(),
                    when: command.when,
                    matches: command.matches,
                };
                let unavailable = package.clone().or_else(|| {
                    platform::unavailable(command.platforms.as_deref(), "this command")
                });
                ListedCommand {
                    registration,
                    unavailable,
                    enabled: self.command_enabled(&command.id),
                    mode: command.mode,
                }
            })
            .collect()
    }
}

/// One command of an installed package as its page in Settings lists it
/// (see [`InstalledPackage::listed_commands`]).
#[derive(Clone, Debug)]
pub struct ListedCommand {
    pub registration: CommandRegistration,
    /// Why it cannot run on this system, if it cannot.
    pub unavailable: Option<String>,
    /// Whether the user left it on (#168).
    pub enabled: bool,
    /// How launching it runs it.
    pub mode: CommandMode,
}

impl InstalledPackage {
    /// The commands this package offers — the ones the user left on — each
    /// beside its manifest entry: what the providers below read their
    /// declarations from.
    fn offered_with_manifest<'a>(
        &self,
        manifest: &'a Manifest,
    ) -> Vec<((CommandRegistration, Option<String>), &'a ManifestCommand)> {
        self.listed_commands()
            .into_iter()
            .zip(&manifest.commands)
            .filter(|(listed, _)| listed.enabled)
            .map(|(listed, command)| ((listed.registration, listed.unavailable), command))
            .collect()
    }

    /// The commands of this package that compute root results and can run
    /// on this system; none if the package cannot be read.
    pub(crate) fn root_result_commands(&self) -> Vec<CommandRegistration> {
        let Ok(manifest) = &self.manifest else {
            return Vec::new();
        };
        self.offered_with_manifest(manifest)
            .into_iter()
            .filter(|((_, unavailable), command)| command.root_results && unavailable.is_none())
            .map(|((registration, _), _)| registration)
            .collect()
    }

    /// The commands of this package that supply root results ahead of the
    /// query and can run on this system; none if the package cannot be read.
    pub(crate) fn indexed_result_commands(&self) -> Vec<CommandRegistration> {
        let Ok(manifest) = &self.manifest else {
            return Vec::new();
        };
        self.offered_with_manifest(manifest)
            .into_iter()
            .filter(|((_, unavailable), command)| command.indexed_results && unavailable.is_none())
            .map(|((registration, _), _)| registration)
            .collect()
    }

    /// The commands of this package that declare scheduled work and can run
    /// on this system, with what each schedule runs; none if the package
    /// cannot be read. A command unavailable on this system is never
    /// scheduled, like an action the user cannot invoke.
    pub(crate) fn scheduled_commands(&self) -> Vec<(CommandRegistration, ManifestSchedule)> {
        let Ok(manifest) = &self.manifest else {
            return Vec::new();
        };
        self.offered_with_manifest(manifest)
            .into_iter()
            .filter(|((_, unavailable), command)| {
                command.schedule.is_some() && unavailable.is_none()
            })
            .filter_map(|((registration, _), command)| {
                command
                    .schedule
                    .clone()
                    .map(|schedule| (registration, schedule))
            })
            .collect()
    }

    /// The mode of this package's command with manifest id `command`: how
    /// launching it runs it. `view` for a command it does not have.
    pub(crate) fn mode_of(&self, command: &str) -> CommandMode {
        self.manifest
            .as_ref()
            .ok()
            .and_then(|manifest| manifest.commands.iter().find(|c| c.id == command))
            .map_or(CommandMode::View, |command| command.mode)
    }

    /// The arguments this package's command with manifest id `command`
    /// declares; none for a command it does not have.
    pub(crate) fn arguments_of(&self, command: &str) -> &[ManifestArgument] {
        self.manifest
            .as_ref()
            .ok()
            .and_then(|manifest| manifest.commands.iter().find(|c| c.id == command))
            .map(|command| command.arguments.as_slice())
            .unwrap_or_default()
    }

    /// The commands of this package that run a continuing service and can
    /// run on this system; none if the package cannot be read. A command
    /// unavailable on this system never runs its service, like an action
    /// the user cannot invoke.
    pub(crate) fn service_commands(&self) -> Vec<CommandRegistration> {
        let Ok(manifest) = &self.manifest else {
            return Vec::new();
        };
        self.offered_with_manifest(manifest)
            .into_iter()
            .filter(|((_, unavailable), command)| command.service && unavailable.is_none())
            .map(|((registration, _), _)| registration)
            .collect()
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct RegistryJson {
    version: u64,
    /// The next unused managed folder number.
    next: u64,
    packages: Vec<RecordJson>,
    /// Managed folders of replaced copies that could not be removed, such as
    /// a folder still in use on Windows; removal is tried again when Pane
    /// starts. Only folders listed here are ever removed that way: one the
    /// registry never recorded, as after it was lost, is left alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    leftovers: Vec<String>,
    /// Identities that are not installed but whose extension data Pane
    /// still keeps: the user uninstalled them keeping their saved data, or
    /// some of it could not be deleted. Installing the same source again
    /// uses that data and drops the record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    retained: Vec<RetainedJson>,
}

#[derive(Clone, Serialize, Deserialize)]
struct RetainedJson {
    /// The source, the identity the data belongs to.
    #[serde(flatten)]
    source: Source,
    /// The package's title when it was uninstalled.
    title: String,
}

/// How to record that Pane keeps a removed package's data: under the title
/// it had, at a position among the retained records, or last.
struct Retain {
    title: String,
    at: Option<usize>,
}

/// Whether uninstalling a package keeps its saved data: its extension
/// settings and content. Its cache and local credentials are removed either
/// way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedData {
    /// Keep them with the package identity, for when the same source is
    /// installed again.
    Keep,
    /// Delete them with the package.
    Delete,
}

/// Where a package identity stands in `installed.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Standing {
    Installed,
    /// Not installed, with its data kept.
    Retained,
    /// Neither installed nor with data on record.
    Neither,
}

/// Extension data Pane keeps for a package identity that is not installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedData {
    pub identity: PackageIdentity,
    /// The package's title when it was uninstalled.
    pub title: String,
}

/// What uninstalling left behind of the managed copy.
#[derive(Debug)]
pub(crate) enum Leftover {
    /// The managed copy is gone.
    None,
    /// The folder of the managed copy could not be removed, for this
    /// reason; Pane tries again when it next starts.
    Listed(PathBuf, String),
    /// The folder could not be removed, nor listed for removal at the next
    /// start.
    Unlisted(PathBuf, String),
}

#[derive(Clone, Serialize, Deserialize)]
struct RecordJson {
    /// The source, the package's identity: its local folder or npm name.
    #[serde(flatten)]
    source: Source,
    /// For a package from npm, the version installed and whether the user
    /// (or the dependency that installed it) pinned it to that version.
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    npm: Option<NpmRecordJson>,
    /// For a package from Git or a default extension, the address it was
    /// fetched from and the revision installed — the fields both kinds
    /// of record write in the same shape, so one flattened
    /// [`GitRecordJson`] keeps them (a Git package's beside the
    /// repository its source records, a default's beside the default
    /// identity and `defaultVersion`). Two structs naming the same keys
    /// cannot both be flattened here: a flattened field takes the keys
    /// it names as it deserializes, leaving none for the other (#269).
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    git: Option<GitRecordJson>,
    /// For a default extension, the version its manifest declares, when
    /// it declares one, read back with the Git fields above to check the
    /// repository's newer release tags
    /// ([#269](https://github.com/pane-app/pane/issues/269)); a manifest
    /// that declares none writes nothing, and its release tag's own
    /// version is read instead. An older Pane, which acquired the
    /// default from its own downloads, wrote this alone with no Git
    /// fields, and such a record is still read as it is.
    #[serde(
        rename = "defaultVersion",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    default_version: Option<String>,
    /// The managed folder under `packages/`.
    dir: String,
    /// Set when the user disabled the package; absent means enabled.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    disabled: bool,
    /// Set when Pane paused the package after it failed; absent means it
    /// runs. Separate from `disabled`, which is the user's choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    paused: Option<PausedJson>,
    /// The identity each `local:` dependency its manifest declares resolved
    /// to when it was installed or updated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<ResolvedJson>,
    /// Set when a component of its current code imports `wasi:http`;
    /// absent means none does (or it was installed before Pane recorded
    /// it, until it is reloaded or updated).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    network: bool,
    /// Set when a component of its current code imports
    /// `pane:extension/programs`; absent means none does.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    programs: bool,
    /// The manifest ids of the commands the user turned off on the
    /// package's page in Settings; absent means every command is on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    disabled_commands: Vec<String>,
}

/// An installed [`NpmPackage`] as its record writes it, beside the name its
/// source records: `"npm": "greeter", "npmVersion": "1.2.3", "pinned":
/// true`. (The name is the source's, so it is not written twice.)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NpmRecordJson {
    #[serde(rename = "npmVersion")]
    version: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pinned: bool,
}

impl NpmRecordJson {
    fn of(package: &NpmPackage) -> NpmRecordJson {
        NpmRecordJson {
            version: package.version.clone(),
            pinned: package.pinned,
        }
    }

    /// The package this records for the package with `identity`, an npm
    /// one.
    fn package(&self, identity: &PackageIdentity) -> Option<NpmPackage> {
        Some(NpmPackage {
            name: identity.npm_name()?.to_owned(),
            version: self.version.clone(),
            pinned: self.pinned,
        })
    }
}

/// An installed [`InstalledGit`] as its record writes it, beside the
/// repository its source records: `"git": "github.com/o/r", "gitUrl":
/// "https://github.com/o/r.git", "gitRef": "refs/tags/v1.0.0",
/// "gitCommit": "<id>", "pinned": true`. `gitRef` is absent for the default
/// branch and for a commit named by its id; `pinned` is set for a tag and a
/// commit. For one extension of a collection (ADR 0044), the repository its
/// source records carries the id after `#`, and `gitExtension` records the
/// id beside the rest. A default extension's record writes the same fields
/// beside its default identity and `defaultVersion`: where the release tag
/// it was acquired from was fetched from, and the tag and commit of the
/// revision installed, which its updates read to find the repository's
/// newer release tags (#269). For a default, `pinned` records what first
/// setup installed (the release tag this Pane release pinned), never a
/// choice of the user's: a default extension's updates do not read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct GitRecordJson {
    #[serde(rename = "gitUrl")]
    url: String,
    #[serde(rename = "gitRef", default, skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
    #[serde(rename = "gitCommit")]
    commit: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pinned: bool,
    /// The id of the extension of a collection this package is (ADR 0044),
    /// when it is one: absent for a package that is a whole repository's,
    /// for an older Pane's records, and for a default extension's.
    #[serde(
        rename = "gitExtension",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    extension: Option<String>,
}

impl GitRecordJson {
    fn of(origin: &GitOrigin, extension: Option<&str>) -> GitRecordJson {
        GitRecordJson {
            url: origin.repository.url().to_owned(),
            reference: origin.revision.ref_name(),
            commit: origin.revision.commit.clone(),
            pinned: origin.revision.pinned(),
            extension: extension.map(ToOwned::to_owned),
        }
    }

    /// The Git fields of the revision `revision`, fetched from
    /// `repository`, as a default extension's record writes them.
    fn of_revision(repository: &Repository, revision: &GitRevision) -> GitRecordJson {
        GitRecordJson {
            url: repository.url().to_owned(),
            reference: revision.ref_name(),
            commit: revision.commit.clone(),
            pinned: revision.pinned(),
            extension: None,
        }
    }

    /// The Git copy this records for the package with `identity`, a Git
    /// one.
    fn installed(&self, identity: &PackageIdentity) -> Option<InstalledGit> {
        identity.git_repository()?;
        Some(InstalledGit {
            url: self.url.clone(),
            revision: GitRevision::from_record(
                self.reference.as_deref(),
                &self.commit,
                self.pinned,
            ),
        })
    }

    /// The default extension's recorded source for the package with
    /// `identity`, a default one whose manifest declared `version`:
    /// what the updater reads to check the repository's newer release
    /// tags (#269). `None` when the record keeps no commit — an older
    /// Pane acquired the default from its own downloads and wrote
    /// `defaultVersion` alone.
    fn installed_default(
        &self,
        version: Option<&str>,
        identity: &PackageIdentity,
    ) -> Option<InstalledDefault> {
        identity.default_id()?;
        Some(InstalledDefault {
            repository: self.url.clone(),
            revision: GitRevision::from_record(
                self.reference.as_deref(),
                &self.commit,
                self.pinned,
            ),
            version: version.map(ToOwned::to_owned),
        })
    }
}

/// A dependency id and the source it resolved to.
#[derive(Clone, Serialize, Deserialize)]
struct ResolvedJson {
    id: String,
    #[serde(flatten)]
    source: Source,
}

/// The records of `package`'s dependencies, resolved from its source.
fn resolved_dependencies(package: &SourcePackage) -> Vec<ResolvedJson> {
    package
        .manifest
        .dependencies
        .iter()
        .filter_map(|dependency| {
            // Where a read resolved the dependency when the package was
            // planned, that is the identity its record names: a source
            // naming an old id a collection renamed resolves to the
            // extension's current id (ADR 0044, #310), which a call
            // through the dependency reaches. Unplanned (a reload), the
            // source's own text resolves it.
            let planned = package.resolved.as_ref().and_then(|resolved| {
                resolved
                    .iter()
                    .find(|(id, _)| id == dependency.id.as_str())
                    .map(|(_, identity)| identity.clone())
            });
            let resolved = match planned {
                Some(resolved) => resolved,
                None => package.dependency_at(&dependency.source).ok()?,
            };
            let PackageIdentity(source) = resolved;
            Some(ResolvedJson {
                id: dependency.id.clone(),
                source,
            })
        })
        .collect()
}

#[derive(Clone, Serialize, Deserialize)]
struct PausedJson {
    #[serde(flatten)]
    pause: Pause,
    /// The managed folder of the code that failed. A pause recorded for
    /// other code (the folder changed) no longer applies.
    code: String,
}

/// Why Pane paused an installed package: it runs none of its code until the
/// user retries it, reloads or updates it, or disables it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Pause {
    pub after: PauseCause,
    /// The details: what failed, and how.
    pub why: String,
    /// The version of the package that failed, if its manifest has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Why a command of the paused package titled `title` does not run, or why
/// a call to it is refused; "The extension" when the title is not known.
pub(crate) fn paused_reason(title: &str) -> String {
    format!("{title} is paused after an error; retry it in Settings")
}

/// What made Pane pause a package.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PauseCause {
    /// Its code could not start: a component could not be loaded or
    /// instantiated, or its reloaded code trapped as it started.
    FailedToStart,
    /// It crashed (trapped) too often.
    Crashes,
    /// Its calls stopped responding (computed for too long without
    /// finishing: unresponsive calls) too often (#18).
    UnresponsiveCalls,
    /// It crashed or its calls stopped responding too often, some of each.
    CrashesAndUnresponsiveCalls,
}

/// Pane's managed location for installed packages:
///
/// ```text
/// <dir>/installed.json      identities, their managed folders and whether
///                           each is disabled
/// <dir>/settings.json       each identity's extension settings
/// <dir>/packages/<n>/       one managed copy: pane.json and its components
/// ```
pub(crate) struct Store {
    dir: PathBuf,
    /// The registry, or why it could not be read. An unreadable registry is
    /// never overwritten.
    registry: Result<RegistryJson, String>,
}

impl Store {
    pub fn open(dir: PathBuf) -> Store {
        let registry = read_registry(&dir);
        let mut store = Store { dir, registry };
        store.remove_leftovers();
        store
    }

    /// Where `identity` stands in `installed.json` as it is on disk now,
    /// which another Pane on the same data folder may have changed since
    /// this one read it.
    pub fn standing_on_disk(&self, identity: &PackageIdentity) -> Result<Standing, PackageError> {
        let registry = read_registry(&self.dir).map_err(PackageError::Storage)?;
        let PackageIdentity(source) = identity;
        Ok(if registry.packages.iter().any(|r| &r.source == source) {
            Standing::Installed
        } else if registry.retained.iter().any(|r| &r.source == source) {
            Standing::Retained
        } else {
            Standing::Neither
        })
    }

    /// Tries again to remove the managed folders of replaced copies that
    /// could not be removed before, never one an installed package uses.
    /// Best effort: a folder that still cannot be removed stays listed.
    fn remove_leftovers(&mut self) {
        let Ok(registry) = &mut self.registry else {
            return;
        };
        if registry.leftovers.is_empty() {
            return;
        }
        let packages = self.dir.join(PACKAGES_DIR);
        let mut updated = registry.clone();
        updated.leftovers.retain(|dir| {
            let in_use = registry.packages.iter().any(|record| record.dir == *dir);
            if in_use {
                return false;
            }
            match fs::remove_dir_all(packages.join(dir)) {
                Ok(()) => false,
                Err(error) => error.kind() != io::ErrorKind::NotFound,
            }
        });
        if updated.leftovers.len() != registry.leftovers.len()
            && write_registry(&self.dir, &updated).is_ok()
        {
            *registry = updated;
        }
    }

    /// Why installed packages cannot be read, if they cannot.
    pub fn problem(&self) -> Option<String> {
        self.registry
            .as_ref()
            .err()
            .map(|reason| format!("Cannot read Pane's installed extensions: {reason}"))
    }

    /// Every installed package, from its managed manifest.
    pub fn installed(&self) -> Vec<InstalledPackage> {
        let Ok(registry) = &self.registry else {
            return Vec::new();
        };
        registry
            .packages
            .iter()
            .map(|record| {
                InstalledPackage::load(record, self.dir.join(PACKAGES_DIR).join(&record.dir))
            })
            .collect()
    }

    pub fn is_installed(&self, identity: &PackageIdentity) -> bool {
        self.record(identity).is_some()
    }

    fn record(&self, identity: &PackageIdentity) -> Option<&RecordJson> {
        let PackageIdentity(source) = identity;
        self.registry
            .as_ref()
            .ok()?
            .packages
            .iter()
            .find(|record| &record.source == source)
    }

    /// Installs a package whose identity is not installed yet.
    pub fn install(&mut self, package: &SourcePackage) -> Result<InstalledPackage, PackageError> {
        if self.is_installed(&package.identity) {
            return Err(PackageError::AlreadyInstalled(package.identity.clone()));
        }
        self.write_copy(package, None, |_| {})
    }

    /// Replaces the managed copy of an installed package with the current
    /// contents of its source; the identity and its record stay the same.
    /// Once the new copy is recorded, `retire` is told the old copy's folder
    /// before it is removed: the old code must stop using it first (its
    /// helpers' programs, which Windows would otherwise keep in use).
    pub fn update(
        &mut self,
        package: &SourcePackage,
        retire: impl FnOnce(&Path),
    ) -> Result<InstalledPackage, PackageError> {
        let Some(record) = self.record(&package.identity) else {
            return Err(PackageError::NotInstalled(package.identity.clone()));
        };
        let old = record.dir.clone();
        self.write_copy(package, Some(old), retire)
    }

    /// Records whether each installed package of `identities` is enabled,
    /// in one write: all of them change, or, if one is not installed or the
    /// record cannot be written, none does. Their managed copies and
    /// settings are left as they are.
    pub fn set_enabled_all(
        &mut self,
        identities: &[PackageIdentity],
        enabled: bool,
    ) -> Result<(), PackageError> {
        let registry = self
            .registry
            .as_mut()
            .map_err(|reason| PackageError::Storage(reason.clone()))?;
        let mut updated = registry.clone();
        for identity in identities {
            let PackageIdentity(source) = identity;
            let Some(record) = updated.packages.iter_mut().find(|r| &r.source == source) else {
                return Err(PackageError::NotInstalled(identity.clone()));
            };
            record.disabled = !enabled;
            // Disabling or enabling ends a pause: the package starts afresh.
            record.paused = None;
        }
        write_registry(&self.dir, &updated)
            .map_err(|error| PackageError::Storage(error.to_string()))?;
        *registry = updated;
        Ok(())
    }

    /// The installed packages Pane paused, each with why, as recorded for
    /// their current code.
    pub fn paused(&self) -> Vec<(PackageIdentity, Pause)> {
        let Ok(registry) = &self.registry else {
            return Vec::new();
        };
        registry
            .packages
            .iter()
            .filter_map(|record| {
                let paused = record.paused.as_ref()?;
                (paused.code == record.dir)
                    .then(|| (PackageIdentity(record.source.clone()), paused.pause.clone()))
            })
            .collect()
    }

    /// Records whether the command with manifest id `command` of the
    /// installed package with `identity` is on (#168), keeping the
    /// package's other records as they are. Writes nothing when the record
    /// already says so.
    pub fn set_command_enabled(
        &mut self,
        identity: &PackageIdentity,
        command: &str,
        enabled: bool,
    ) -> Result<(), PackageError> {
        let registry = self
            .registry
            .as_mut()
            .map_err(|reason| PackageError::Storage(reason.clone()))?;
        let PackageIdentity(source) = identity;
        let mut updated = registry.clone();
        let Some(record) = updated.packages.iter_mut().find(|r| &r.source == source) else {
            return Err(PackageError::NotInstalled(identity.clone()));
        };
        let listed = record.disabled_commands.iter().any(|id| id == command);
        if listed != enabled {
            return Ok(());
        }
        if enabled {
            record.disabled_commands.retain(|id| id != command);
        } else {
            record.disabled_commands.push(command.to_owned());
            record.disabled_commands.sort();
        }
        write_registry(&self.dir, &updated)
            .map_err(|error| PackageError::Storage(error.to_string()))?;
        *registry = updated;
        Ok(())
    }

    /// Records that Pane paused the installed package with `identity` for
    /// `pause`, or, with `None`, that it runs again.
    pub fn set_paused(
        &mut self,
        identity: &PackageIdentity,
        pause: Option<Pause>,
    ) -> Result<(), PackageError> {
        let registry = self
            .registry
            .as_mut()
            .map_err(|reason| PackageError::Storage(reason.clone()))?;
        let PackageIdentity(source) = identity;
        let mut updated = registry.clone();
        let Some(record) = updated.packages.iter_mut().find(|r| &r.source == source) else {
            return Err(PackageError::NotInstalled(identity.clone()));
        };
        let paused = pause.map(|pause| PausedJson {
            pause,
            code: record.dir.clone(),
        });
        if paused.is_none() && record.paused.is_none() {
            return Ok(());
        }
        record.paused = paused;
        write_registry(&self.dir, &updated)
            .map_err(|error| PackageError::Storage(error.to_string()))?;
        *registry = updated;
        Ok(())
    }

    /// The identities that are not installed but whose extension data Pane
    /// keeps, in the order they were uninstalled.
    pub fn retained(&self) -> Vec<RetainedData> {
        let Ok(registry) = &self.registry else {
            return Vec::new();
        };
        registry
            .retained
            .iter()
            .map(|record| RetainedData {
                identity: PackageIdentity(record.source.clone()),
                title: record.title.clone(),
            })
            .collect()
    }

    /// Removes again the package with `identity` that an install added,
    /// putting back, at its place among them, the record that Pane kept its
    /// data under `title` if it had one (`(title, index)`).
    pub(crate) fn undo_install(
        &mut self,
        identity: &PackageIdentity,
        retained: Option<(String, usize)>,
    ) -> Result<Leftover, PackageError> {
        let retain = retained.map(|(title, at)| Retain {
            title,
            at: Some(at),
        });
        self.remove(identity, retain)
    }

    /// Uninstalls the packages of `removals`, in one write: each record
    /// goes, and with a title, its identity is recorded as keeping extension
    /// data under that title. `installed.json` is read again first and only
    /// those records change in it, so what another Pane on the same data
    /// folder recorded since is kept. If one is not installed or the record
    /// cannot be written, nothing changes. Then each managed copy is removed; a
    /// managed folder that cannot be (one in use on Windows) is listed so
    /// that the next start removes it, as an update's replaced copy is.
    /// Returns what is left of each managed copy, in the order given.
    pub fn uninstall_all(
        &mut self,
        removals: &[(PackageIdentity, Option<String>)],
    ) -> Result<Vec<Leftover>, PackageError> {
        let removals: Vec<(PackageIdentity, Option<Retain>)> = removals
            .iter()
            .map(|(identity, title)| {
                let retain = title.clone().map(|title| Retain { title, at: None });
                (identity.clone(), retain)
            })
            .collect();
        self.remove_all(&removals)
    }

    /// Uninstalls one package as [`Store::uninstall_all`] does, recording
    /// retained data under a title, at a position among the retained records
    /// if given, else last.
    fn remove(
        &mut self,
        identity: &PackageIdentity,
        retain: Option<Retain>,
    ) -> Result<Leftover, PackageError> {
        let mut left = self.remove_all(&[(identity.clone(), retain)])?;
        Ok(left.pop().unwrap_or(Leftover::None))
    }

    /// Uninstalls as [`Store::uninstall_all`] does, each retained record
    /// under a title at a position among them if given, else last.
    fn remove_all(
        &mut self,
        removals: &[(PackageIdentity, Option<Retain>)],
    ) -> Result<Vec<Leftover>, PackageError> {
        let registry = self
            .registry
            .as_mut()
            .map_err(|reason| PackageError::Storage(reason.clone()))?;
        // The same changes to the records on disk, which another Pane may
        // have changed since, and to this Pane's.
        let mut on_disk = read_registry(&self.dir).map_err(PackageError::Storage)?;
        let mut updated = registry.clone();
        let mut dirs = Vec::new();
        for (identity, retain) in removals {
            let PackageIdentity(local) = identity;
            let missing = || PackageError::NotInstalled(identity.clone());
            take_record(&mut updated, local).ok_or_else(missing)?;
            // Its managed copy as recorded now: another Pane may have
            // updated it since.
            let record = take_record(&mut on_disk, local).ok_or_else(missing)?;
            dirs.push(record.dir);
            if let Some(Retain { title, at }) = retain {
                put_retained(&mut updated, local, title.clone(), *at);
                put_retained(&mut on_disk, local, title.clone(), *at);
            }
        }
        write_registry(&self.dir, &on_disk)
            .map_err(|error| PackageError::Storage(error.to_string()))?;
        *registry = updated;
        let failed: Vec<Option<(String, PathBuf, String)>> = dirs
            .into_iter()
            .map(|dir| {
                let location = self.dir.join(PACKAGES_DIR).join(&dir);
                match fs::remove_dir_all(&location) {
                    Ok(()) => None,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                    Err(error) => Some((dir, location, error.to_string())),
                }
            })
            .collect();
        let left: Vec<String> = failed
            .iter()
            .flatten()
            .map(|(dir, ..)| dir.clone())
            .collect();
        let recorded = left.is_empty() || {
            on_disk.leftovers.extend(left.iter().cloned());
            let written = write_registry(&self.dir, &on_disk).is_ok();
            if written {
                registry.leftovers.extend(left);
            }
            written
        };
        Ok(failed
            .into_iter()
            .map(|failed| match failed {
                None => Leftover::None,
                Some((_, location, error)) if recorded => Leftover::Listed(location, error),
                Some((_, location, error)) => Leftover::Unlisted(location, error),
            })
            .collect())
    }

    /// Drops the record that Pane keeps extension data for `identity`, once
    /// that data is deleted or no longer kept. `installed.json` is read
    /// again first and only that record is removed from it, so what another
    /// Pane on the same data folder recorded since, such as installing the
    /// same source again, is kept. A failure leaves the record.
    pub fn forget_retained(&mut self, identity: &PackageIdentity) -> Result<(), PackageError> {
        let registry = self
            .registry
            .as_mut()
            .map_err(|reason| PackageError::Storage(reason.clone()))?;
        let PackageIdentity(source) = identity;
        let mut on_disk = read_registry(&self.dir).map_err(PackageError::Storage)?;
        if on_disk
            .retained
            .iter()
            .any(|record| &record.source == source)
        {
            on_disk.retained.retain(|record| &record.source != source);
            write_registry(&self.dir, &on_disk)
                .map_err(|error| PackageError::Storage(error.to_string()))?;
        }
        registry.retained.retain(|record| &record.source != source);
        Ok(())
    }

    /// Records that Pane keeps extension data for `identity`, which is not
    /// installed, under `title`.
    pub fn retain(
        &mut self,
        identity: &PackageIdentity,
        title: String,
    ) -> Result<(), PackageError> {
        let registry = self
            .registry
            .as_mut()
            .map_err(|reason| PackageError::Storage(reason.clone()))?;
        let PackageIdentity(local) = identity;
        let mut updated = registry.clone();
        put_retained(&mut updated, local, title, None);
        write_registry(&self.dir, &updated)
            .map_err(|error| PackageError::Storage(error.to_string()))?;
        *registry = updated;
        Ok(())
    }

    /// Copies the package into a fresh managed folder, then records it,
    /// replacing `old`'s folder if given. A failure leaves the previous state.
    fn write_copy(
        &mut self,
        package: &SourcePackage,
        old: Option<String>,
        retire: impl FnOnce(&Path),
    ) -> Result<InstalledPackage, PackageError> {
        let registry = self
            .registry
            .as_mut()
            .map_err(|reason| PackageError::Storage(reason.clone()))?;
        let storage = |error: io::Error| PackageError::Storage(error.to_string());
        // A folder can exist at or beyond `next` if the registry was lost or
        // replaced; it is skipped, never deleted.
        let mut number = registry.next;
        let (dir, location) = loop {
            let dir = number.to_string();
            let location = self.dir.join(PACKAGES_DIR).join(&dir);
            match fs::symlink_metadata(&location) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => break (dir, location),
                Err(error) => return Err(storage(error)),
                Ok(_) => number += 1,
            }
        };
        let copied = copy_package(package, &location);
        if let Err(error) = copied {
            let _ = fs::remove_dir_all(&location);
            return Err(storage(error));
        }
        let PackageIdentity(local) = &package.identity;
        let mut updated = RegistryJson {
            next: number + 1,
            ..registry.clone()
        };
        let dependencies = resolved_dependencies(package);
        let npm = package
            .npm
            .as_ref()
            .map(|origin| NpmRecordJson::of(&origin.package));
        // A Git package's and a default extension's records keep the same
        // Git fields, in the one shape; a default's own part of its record
        // is the version its manifest declares.
        let git = match (&package.git, &package.default) {
            (Some(origin), _) => Some(GitRecordJson::of(origin, package.extension.as_deref())),
            (None, Some(origin)) => Some(GitRecordJson::of_revision(
                &origin.repository,
                &origin.revision,
            )),
            (None, None) => None,
        };
        let default_version = if package.default.is_some() {
            package.manifest.version.clone()
        } else {
            None
        };
        // An update keeps the record, so a disabled package stays disabled
        // and the commands the user turned off stay turned off.
        match updated.packages.iter_mut().find(|r| &r.source == local) {
            Some(record) => {
                record.dir = dir;
                // New code has not failed.
                record.paused = None;
                record.dependencies = dependencies.clone();
                record.npm = npm.clone();
                record.git = git.clone();
                record.default_version = default_version.clone();
                record.network = package.network;
                record.programs = package.programs;
            }
            None => {
                // Data kept from an earlier installation of this identity
                // is its own again.
                updated.retained.retain(|record| &record.source != local);
                updated.packages.push(RecordJson {
                    source: local.clone(),
                    npm: npm.clone(),
                    git: git.clone(),
                    default_version: default_version.clone(),
                    dir,
                    disabled: false,
                    paused: None,
                    dependencies: dependencies.clone(),
                    network: package.network,
                    programs: package.programs,
                    disabled_commands: Vec::new(),
                });
            }
        };
        if let Err(error) = write_registry(&self.dir, &updated) {
            let _ = fs::remove_dir_all(&location);
            return Err(storage(error));
        }
        *registry = updated;
        if let Some(old) = &old {
            retire(&self.dir.join(PACKAGES_DIR).join(old));
        }
        if let Some(old) = old
            && fs::remove_dir_all(self.dir.join(PACKAGES_DIR).join(&old)).is_err()
        {
            // A folder still in use (Windows) is left behind, and listed so
            // that the next start removes it. Best effort: if that cannot be
            // recorded, the folder stays.
            let mut listed = registry.clone();
            listed.leftovers.push(old);
            if write_registry(&self.dir, &listed).is_ok() {
                *registry = listed;
            }
        }
        // The record the match above wrote, in either branch, read back
        // as a restart reads it.
        let record = registry
            .packages
            .iter()
            .find(|record| &record.source == local)
            .expect("written by the match above");
        Ok(InstalledPackage::load(record, location))
    }
}

/// Records in `registry` that data is kept for the local source `local`,
/// last uninstalled as `title`: at position `at` among the records if
/// given, else last.
/// Removes and returns the record of the installed package from `local`.
fn take_record(registry: &mut RegistryJson, source: &Source) -> Option<RecordJson> {
    let index = registry.packages.iter().position(|r| r.source == *source)?;
    Some(registry.packages.remove(index))
}

fn put_retained(registry: &mut RegistryJson, source: &Source, title: String, at: Option<usize>) {
    registry.retained.retain(|record| record.source != *source);
    let record = RetainedJson {
        source: source.clone(),
        title,
    };
    match at {
        Some(at) if at <= registry.retained.len() => registry.retained.insert(at, record),
        _ => registry.retained.push(record),
    }
}

/// Writes the validated `pane.json` and copies the components it names and
/// the file of each helper for this system, keeping their relative paths.
/// Nothing else in the source folder is copied, not even helpers' files for
/// other systems.
fn copy_package(package: &SourcePackage, location: &Path) -> io::Result<()> {
    fs::create_dir_all(location)?;
    fs::write(location.join(MANIFEST_FILE), &package.manifest_text)?;
    let helper_files: Vec<&Path> = package
        .manifest
        .helpers
        .iter()
        .filter_map(ManifestHelper::for_this_system)
        .collect();
    for file in package
        .manifest
        .components()
        .map(|(_, component)| component)
        .chain(helper_files.iter().copied())
    {
        let source = package.folder.join(file);
        let target = location.join(file);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        // Checked when the package was read; a helper file replaced by a
        // link since is not followed.
        let is_helper = helper_files.contains(&file);
        if is_helper && !fs::symlink_metadata(&source)?.is_file() {
            return Err(io::Error::other(format!(
                "the helper file {} is no longer a regular file",
                file.display()
            )));
        }
        if is_helper {
            // Closed and renamed into place rather than written straight to
            // `target`: a helper run soon after this install (or a reinstall
            // rewriting it while another generation is spawning it) must
            // never see it half-written and get Linux's `ETXTBSY`.
            runner::copy_executable(&source, &target)?;
        } else {
            fs::copy(&source, &target)?;
            // A development build of JavaScript or TypeScript keeps a source
            // map beside its component; it comes with the component, so the
            // extension log's stack traces keep mapping to the sources after
            // the install or reload that copied it (#214). Most components
            // keep no map, as `pane-build`'s copying of one also knows.
            if let Some(map) = map_beside(&source)
                && map.is_file()
            {
                fs::copy(&map, map_beside(&target).expect("the target is named"))?;
            }
        }
    }
    for file in helper_files {
        make_executable(&location.join(file))?;
    }
    // The help the Setup screen shows beside a command's preferences, if
    // the package ships it: a regular file only, never a link followed.
    let help = package.folder.join(preferences::HELP_FILE);
    if fs::symlink_metadata(&help).is_ok_and(|metadata| metadata.is_file()) {
        fs::copy(&help, location.join(preferences::HELP_FILE))?;
    }
    copy_images(package, location)
}

/// The source map kept beside the component at `component`, when there is
/// one: its file name plus `.map` (#214).
pub(crate) fn map_beside(component: &Path) -> Option<PathBuf> {
    let name = component.file_name()?.to_str()?;
    Some(component.with_file_name(format!("{name}.map")))
}

/// The folder of a package whose images its lists name (#139), as
/// Raycast's `assets` folder is: copied whole into the managed copy.
pub(crate) const ASSETS_DIR: &str = "assets";

/// Copies the images a package shows into its managed copy at
/// `location`: the files its own and its commands' icons name, with
/// their `@light` and `@dark` variants where it has them, and its
/// [`ASSETS_DIR`] folder, which holds the images its lists name. Only
/// regular files and folders are copied; a link is not followed.
fn copy_images(package: &SourcePackage, location: &Path) -> io::Result<()> {
    for file in package.manifest.icon_files() {
        let source = package.folder.join(&file);
        if !fs::symlink_metadata(&source).is_ok_and(|metadata| metadata.is_file()) {
            // A variant the package does not have; the icon itself was
            // checked when the package was read.
            continue;
        }
        let target = location.join(&file);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
    }
    copy_folder(&package.folder.join(ASSETS_DIR), &location.join(ASSETS_DIR))
}

/// Copies the regular files and folders under `from` to `to`, if `from`
/// is a folder; links and other entries are left behind.
fn copy_folder(from: &Path, to: &Path) -> io::Result<()> {
    if !fs::symlink_metadata(from).is_ok_and(|metadata| metadata.is_dir()) {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Lets the system run the helper file at `path` (a package fetched as an
/// archive or through a tool may have lost the execute permission), with
/// exactly `rwxr-xr-x`: no set-user-id, set-group-id or sticky bit, and no
/// one but its owner may change it.
#[cfg(unix)]
pub(crate) fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
pub(crate) fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Reads the registry in `dir`; a missing one holds nothing.
fn read_registry(dir: &Path) -> Result<RegistryJson, String> {
    match fs::read_to_string(dir.join(REGISTRY_FILE)) {
        Ok(text) => serde_json::from_str::<RegistryJson>(&text)
            .map_err(|error| error.to_string())
            .and_then(|registry| {
                if registry.version == REGISTRY_VERSION {
                    Ok(registry)
                } else {
                    Err(format!(
                        "{REGISTRY_FILE} has version {}, this Pane reads {REGISTRY_VERSION}",
                        registry.version
                    ))
                }
            }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(RegistryJson {
            version: REGISTRY_VERSION,
            next: 1,
            packages: Vec::new(),
            leftovers: Vec::new(),
            retained: Vec::new(),
        }),
        Err(error) => Err(error.to_string()),
    }
    .map_err(|reason| format!("{}: {reason}", dir.join(REGISTRY_FILE).display()))
}

/// Replaces the registry whole (see [`write_atomically`] for what a crash or
/// a second Pane process can do to it).
fn write_registry(dir: &Path, registry: &RegistryJson) -> io::Result<()> {
    let text = serde_json::to_string_pretty(registry).map_err(io::Error::other)?;
    write_atomically(&dir.join(REGISTRY_FILE), text.as_bytes(), Readers::Default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_id_keeps_a_hash_in_its_package_part() {
        let identity = PackageIdentity(Source::Local {
            local: "/home/me/#tools".into(),
        });
        let id = identity.command_id("open");
        assert_eq!(id, "local:/home/me/#tools#open");
        assert_eq!(
            CommandId::parse(&id),
            CommandId {
                package: "local:/home/me/#tools",
                command: "open",
            }
        );
        assert_eq!(
            CommandId::parse("default:files"),
            CommandId {
                package: "default:files",
                command: "",
            }
        );
    }

    /// A package folder in `dir` with one command and, for this system, a
    /// helper `tool` whose file holds `helper`.
    fn package_with_helper(dir: &Path, helper: &[u8]) -> PathBuf {
        let target = Target::current().expect("Pane names this system's target");
        let file = format!("helpers/tool{}", target.exe_suffix());
        let folder = dir.join("source");
        fs::create_dir_all(folder.join("helpers")).unwrap();
        fs::write(folder.join("command.wasm"), b"not checked here").unwrap();
        fs::write(folder.join(&file), helper).unwrap();
        let manifest = format!(
            r#"{{ "manifestVersion": 1, "title": "Tool", "apiVersion": "0.2",
                 "commands": [{{ "id": "c", "title": "C", "component": "command.wasm" }}],
                 "helpers": [{{ "id": "tool", "targets": {{ "{}": "{file}" }} }}] }}"#,
            target.id()
        );
        fs::write(folder.join(MANIFEST_FILE), manifest).unwrap();
        folder
    }

    #[test]
    fn records_of_local_and_npm_packages_are_read_back() {
        // As #42 wrote them, and one from npm with its pinned version and a
        // dependency on another npm package.
        let text = r#"{
            "version": 1, "next": 3,
            "packages": [
                { "local": "/src/a", "dir": "1", "dependencies": [{ "id": "b", "local": "/src/b" }] },
                { "npm": "@pane-samples/greeter", "npmVersion": "0.1.0", "pinned": true, "dir": "2",
                  "dependencies": [{ "id": "c", "npm": "c" }] }
            ],
            "retained": [{ "npm": "gone", "title": "Gone" }, { "local": "/src/x", "title": "X" }]
        }"#;
        let registry: RegistryJson = serde_json::from_str(text).unwrap();
        let [a, greeter] = registry.packages.as_slice() else {
            panic!("two records")
        };
        assert_eq!(
            a.source,
            Source::Local {
                local: "/src/a".into()
            }
        );
        assert_eq!(a.npm, None);
        assert_eq!(
            a.dependencies[0].source,
            Source::Local {
                local: "/src/b".into()
            }
        );
        assert_eq!(
            greeter.source,
            Source::Npm {
                npm: "@pane-samples/greeter".into()
            }
        );
        assert_eq!(
            greeter.npm,
            Some(NpmRecordJson {
                version: "0.1.0".into(),
                pinned: true
            })
        );
        assert_eq!(
            greeter.dependencies[0].source,
            Source::Npm { npm: "c".into() }
        );
        assert_eq!(
            registry.retained[0].source,
            Source::Npm { npm: "gone".into() }
        );

        let written = serde_json::to_value(&registry).unwrap();
        assert_eq!(
            written["packages"][1],
            serde_json::json!({
                "npm": "@pane-samples/greeter", "npmVersion": "0.1.0", "pinned": true, "dir": "2",
                "dependencies": [{ "id": "c", "npm": "c" }]
            })
        );
        assert_eq!(
            written["packages"][0],
            serde_json::json!({
                "local": "/src/a", "dir": "1", "dependencies": [{ "id": "b", "local": "/src/b" }]
            })
        );
    }

    #[test]
    fn an_extension_of_a_collection_s_record_is_read_back_with_its_id() {
        // As an install by its id writes it (#307): the repository with the
        // id after `#`, and the id beside the Git source fields — the
        // repository's own address fetched as written.
        let dir = tempfile::tempdir().unwrap();
        let text = r#"{ "version": 1, "next": 2, "packages": [
            { "git": "github.com/owner/tools#clock",
              "gitUrl": "https://github.com/owner/tools.git",
              "gitRef": "refs/tags/clock/v1.2.0",
              "gitCommit": "6e07ce96ea361661f2a63eaac8bd3b135c76b012",
              "pinned": true, "gitExtension": "clock", "dir": "1" } ] }"#;
        fs::write(dir.path().join(REGISTRY_FILE), text).unwrap();
        let store = Store::open(dir.path().to_path_buf());
        let installed = store.installed();
        let [package] = installed.as_slice() else {
            panic!("one installed package")
        };
        let repository = crate::git::GitSpec::parse("https://github.com/owner/tools")
            .unwrap()
            .repository;
        assert_eq!(
            package.identity,
            PackageIdentity::git_extension(&repository, "clock")
        );
        assert_eq!(
            package.identity.extension_id(),
            Some("clock"),
            "the id, read from the repository it records"
        );
        let git = package.git.as_ref().expect("the Git source");
        assert_eq!(git.url, "https://github.com/owner/tools.git");
        assert_eq!(
            git.revision.ref_name().as_deref(),
            Some("refs/tags/clock/v1.2.0")
        );
        assert!(git.revision.pinned(), "the tag's revision, as ever");
    }

    #[test]
    fn a_default_extension_s_record_is_read_back_with_its_repository() {
        // As first setup writes it (#269): the default identity, the
        // version its manifest declared, and the Git fields of the release
        // tag it was acquired from — the same keys a Git package's record
        // writes, which the record must read as the default's own source,
        // not lose to the Git record flattened beside it.
        let dir = tempfile::tempdir().unwrap();
        let text = r#"{ "version": 1, "next": 2, "packages": [
            { "default": "calculator", "defaultVersion": "0.1.0",
              "gitUrl": "http://127.0.0.1:49152/calculator.git",
              "gitRef": "refs/tags/v0.1.0",
              "gitCommit": "6e07ce96ea361661f2a63eaac8bd3b135c76b012",
              "pinned": true, "dir": "1" } ] }"#;
        fs::write(dir.path().join(REGISTRY_FILE), text).unwrap();
        let store = Store::open(dir.path().to_path_buf());
        let installed = store.installed();
        let [package] = installed.as_slice() else {
            panic!("one installed package")
        };
        assert!(package.git.is_none(), "not a Git package's record");
        assert_eq!(
            package.identity,
            PackageIdentity::default_extension("calculator")
        );
        let default = package.default.as_ref().expect("the default's source");
        assert_eq!(default.repository, "http://127.0.0.1:49152/calculator.git");
        assert_eq!(
            default.revision.ref_name().as_deref(),
            Some("refs/tags/v0.1.0")
        );
        assert!(default.revision.pinned(), "the tag's revision, as ever");
        assert_eq!(
            default.revision.commit,
            "6e07ce96ea361661f2a63eaac8bd3b135c76b012"
        );
        assert_eq!(default.version.as_deref(), Some("0.1.0"));
    }

    /// This test binary: a program for this system's target.
    fn a_program() -> Vec<u8> {
        fs::read(std::env::current_exe().unwrap()).unwrap()
    }

    #[test]
    fn an_update_retires_the_old_copy_before_removing_it() {
        let dir = tempfile::tempdir().unwrap();
        let folder = package_with_helper(dir.path(), &a_program());
        let mut store = Store::open(dir.path().join("pane"));
        let first = store
            .install(&SourcePackage::read(&folder).unwrap())
            .unwrap();

        let mut retired = None;
        let second = store
            .update(&SourcePackage::read(&folder).unwrap(), |old| {
                // Still there: whatever runs from it is stopped first.
                assert!(old.join(MANIFEST_FILE).is_file());
                retired = Some(old.to_path_buf());
            })
            .unwrap();

        assert_eq!(retired.as_deref(), Some(first.location.as_path()));
        assert!(!first.location.exists());
        assert!(second.location.join(MANIFEST_FILE).is_file());
    }

    #[cfg(unix)]
    #[test]
    fn an_installed_helper_is_exactly_rwxr_xr_x() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let folder = package_with_helper(dir.path(), &a_program());
        let helper = folder.join("helpers/tool");
        // Set-user-id, set-group-id, sticky, and writable by anyone.
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o7777)).unwrap();
        let mut store = Store::open(dir.path().join("pane"));

        let installed = store
            .install(&SourcePackage::read(&folder).unwrap())
            .unwrap();

        let mode = fs::metadata(installed.location.join("helpers/tool"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o755, "{mode:o}");
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_file_that_is_a_link_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let folder = package_with_helper(dir.path(), &a_program());
        let real = dir.path().join("real");
        fs::rename(folder.join("helpers/tool"), &real).unwrap();
        std::os::unix::fs::symlink(&real, folder.join("helpers/tool")).unwrap();

        let error = SourcePackage::read(&folder).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "Not ready to run: the package ships helper `tool` for {}, but its file \
                 helpers/tool is a symbolic link; a helper must be a regular file in the \
                 package",
                Target::current().unwrap()
            )
        );
    }

    #[test]
    fn an_api_version_is_parsed_with_or_without_a_patch() {
        // The manifest may say `0.1` or `0.1.0`: the patch is not part of
        // the API version. Anything else is a malformed manifest. Both
        // versions this Pane runs parse: the current one, and the previous
        // one the window holds (ADR 0046).
        let runs = |required| extension_api_refusal(required).map(|refusal| refusal.is_none());
        assert_eq!(runs("0.2"), Ok(true));
        assert_eq!(runs("0.2.0"), Ok(true));
        assert_eq!(runs("0.1"), Ok(true));
        assert_eq!(runs("0.3"), Ok(false));
        assert_eq!(runs("0.0"), Ok(false));
        assert_eq!(
            extension_api_refusal("one"),
            Err(PackageError::InvalidManifest(
                "apiVersion `one` is not a version such as `0.1`".into()
            ))
        );
    }

    #[test]
    fn a_refused_api_version_names_the_direction_it_misses() {
        // A refusal names which version the extension needs, which
        // versions this Pane runs, and what to update (#318, ADR 0046):
        // a version newer than any this Pane runs needs a newer Pane, one
        // older than every version it runs a newer version of the
        // extension.
        let newer = extension_api_refusal("0.3").unwrap().expect("refused");
        assert_eq!(
            newer,
            UnsupportedApi {
                required: "0.3".into(),
                newer: true
            }
        );
        assert_eq!(newer.needs(), "Needs a newer Pane");
        assert_eq!(
            newer.to_string(),
            "it needs Pane extension API 0.3, but this Pane provides 0.2 and 0.1. \
             Update Pane to run it"
        );
        let older = extension_api_refusal("0.0").unwrap().expect("refused");
        assert_eq!(
            older,
            UnsupportedApi {
                required: "0.0".into(),
                newer: false
            }
        );
        assert_eq!(older.needs(), "Needs a newer version of the extension");
        assert_eq!(
            older.to_string(),
            "it needs Pane extension API 0.0, but this Pane provides 0.2 and 0.1. Update the \
             extension to a newer version"
        );
        // As a package error it says the package is incompatible, with the
        // same words every refusal uses — an install's, a preview's,
        // `pane-ext check`'s, an update check's skip and the installed
        // listing's page.
        assert_eq!(
            PackageError::IncompatibleApi(newer).to_string(),
            "Incompatible package: it needs Pane extension API 0.3, but this Pane provides \
             0.2 and 0.1. Update Pane to run it"
        );
        // A previous version this Pane still runs is named beside its
        // current one, so a refusal says every version it runs — the
        // wording above, and the parameterized shape below for a window
        // that is not this Pane's own.
        let host = UnsupportedApi {
            required: "0.4".into(),
            newer: true,
        };
        assert_eq!(
            refused_api_text((0, 3), Some((0, 2)), &host),
            "it needs Pane extension API 0.4, but this Pane provides 0.3 and 0.2. \
             Update Pane to run it"
        );
    }

    #[test]
    fn the_listing_reads_a_manifest_whose_api_version_is_refused() {
        // The installed listing parses a manifest naming a version this
        // Pane cannot run and keeps it beside the refusal (#318, ADR
        // 0046): a strict read refuses the same manifest, as every read
        // of a package being installed, previewed, checked or updated
        // does.
        let refused = r#"{
  "manifestVersion": 1,
  "title": "Hello",
  "version": "1.0.0",
  "apiVersion": "0.3",
  "commands": [
    { "id": "hello", "title": "Say hello", "subtitle": "Greets you", "component": "hello.wasm" }
  ]
}"#;
        assert_eq!(
            Manifest::parse(refused),
            Err(PackageError::IncompatibleApi(UnsupportedApi {
                required: "0.3".into(),
                newer: true
            }))
        );
        let (manifest, refusal) = Manifest::parse_installed(refused).unwrap();
        assert_eq!(manifest.title, "Hello");
        assert_eq!(manifest.commands[0].subtitle.as_deref(), Some("Greets you"));
        assert_eq!(
            refusal,
            Some(UnsupportedApi {
                required: "0.3".into(),
                newer: true
            })
        );
        // A manifest this Pane runs parses the same either way — the
        // current version, and the previous one the window holds — and a
        // broken one is broken for both.
        let runs = refused.replace("0.3", "0.2");
        assert_eq!(Manifest::parse(&runs).unwrap().title, "Hello");
        let (manifest, refusal) = Manifest::parse_installed(&runs).unwrap();
        assert_eq!(manifest.title, "Hello");
        assert_eq!(refusal, None);
        let previous = refused.replace("0.3", "0.1");
        assert_eq!(Manifest::parse(&previous).unwrap().title, "Hello");
        let broken = "{ not json";
        assert_eq!(
            Manifest::parse_installed(broken).unwrap_err().to_string(),
            Manifest::parse(broken).unwrap_err().to_string()
        );
    }

    #[test]
    fn while_the_previous_world_s_window_lasts_both_versions_run() {
        // The window this Pane holds: 0.2 is the current version and 0.1
        // the previous one, run through the host's adapter (ADR 0046) —
        // and every other version is refused, as it always has been.
        let (host, previous) = (EXTENSION_API, EXTENSION_API_PREVIOUS);
        assert_eq!(previous, Some((0, 1)));
        assert!(runs_extension_api(host, previous, host));
        assert!(runs_extension_api(host, previous, (0, 1)));
        for required in [(0, 0), (0, 3), (1, 0), (1, 1), (2, 0)] {
            assert!(!runs_extension_api(host, previous, required));
        }
    }

    #[test]
    fn before_1_0_the_previous_minor_alone_is_still_run() {
        // ADR 0046 before 1.0: every minor is its own API, so a host at
        // 0.3 runs 0.3 and the previous minor, 0.2, while its window
        // lasts — nothing older and nothing newer.
        let (host, previous) = ((0, 3), Some((0, 2)));
        assert!(runs_extension_api(host, previous, (0, 3)));
        assert!(runs_extension_api(host, previous, (0, 2)));
        assert!(!runs_extension_api(host, previous, (0, 1)));
        assert!(!runs_extension_api(host, previous, (0, 4)));
        assert!(!runs_extension_api(host, previous, (1, 0)));
    }

    #[test]
    fn from_1_0_every_minor_of_the_major_is_run() {
        // ADR 0046 from 1.0: an addition needs no new version, so a host
        // at 1.3 runs every minor from 1.0, and the previous major, 0.9,
        // while its window lasts — nothing older and nothing newer.
        let (host, previous) = ((1, 3), Some((0, 9)));
        for minor in 0..=3 {
            assert!(runs_extension_api(host, previous, (1, minor)));
        }
        assert!(runs_extension_api(host, previous, (0, 9)));
        assert!(!runs_extension_api(host, previous, (0, 8)));
        assert!(!runs_extension_api(host, previous, (1, 4)));
        assert!(!runs_extension_api(host, previous, (2, 0)));
        // The previous major runs only while the window lasts.
        assert!(!runs_extension_api(host, None, (0, 9)));
    }
}
