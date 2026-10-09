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

use serde::{Deserialize, Serialize};

use crate::arguments::{self, ManifestArgument};
use crate::atomic::{Readers, write_atomically};
use crate::git::{GitOrigin, GitRevision, GitSpec, InstalledGit, Repository};
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
pub const EXTENSION_API: (u64, u64) = (0, 1);

const REGISTRY_FILE: &str = "installed.json";
const REGISTRY_VERSION: u64 = 1;
const PACKAGES_DIR: &str = "packages";

/// The identity of an installed package, derived from its source and
/// independent of its display title. A local package is identified by its
/// folder's resolved absolute path, as the operating system reports it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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

    /// The host and path of a Git package's repository.
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
    /// [`crate::defaults::fetch`]), whatever its version: the extension Pane
    /// acquired for this feature, from Pane's own downloads.
    pub fn default_extension(id: &str) -> PackageIdentity {
        PackageIdentity(Source::Default {
            default: id.to_owned(),
        })
    }

    /// The id of a default extension acquired from Pane's own downloads.
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
    /// A package from npm or Git cannot name a `local:` folder: its folder is
    /// on its author's computer, not the user's.
    pub(crate) fn dependency(&self, source: &str) -> Result<PackageIdentity, PathBuf> {
        let path = match SourceSpec::parse(source) {
            Ok(SourceSpec::Npm(spec)) => return Ok(PackageIdentity::npm(&spec.name)),
            Ok(SourceSpec::Git(spec)) => return Ok(PackageIdentity::git(&spec.repository)),
            Ok(SourceSpec::Local(path)) => path,
            Err(_) => return Err(PathBuf::from(source)),
        };
        let folder = match &self.0 {
            Source::Local { local } => Path::new(local).join(&path),
            Source::Npm { .. } | Source::Git { .. } | Source::Default { .. } => {
                return Err(PathBuf::from(path));
            }
        };
        if let Ok(identity) = PackageIdentity::local(&folder) {
            return Ok(identity);
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
            Some(text) if resolved.is_absolute() => Ok(PackageIdentity(Source::Local {
                local: text.to_owned(),
            })),
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
    /// Settings shows it under the title. `None` when it does not say.
    pub description: Option<String>,
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    /// Whether the command searches as the user types into its own search
    /// field once it is open (`"search": true`), such as a command searching
    /// an online service; root search never asks it. Its component then
    /// also exports `pane:extension/command-search`.
    pub search: bool,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestJson {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    version: Option<String>,
    /// Checked by [`icons::parse_manifest_icon`].
    #[serde(default)]
    icon: Option<serde_json::Value>,
    api_version: String,
    #[serde(default)]
    platforms: Option<Vec<String>>,
    #[serde(default)]
    commands: Vec<CommandJson>,
    #[serde(default)]
    operations: Vec<OperationJson>,
    #[serde(default)]
    helpers: Vec<HelperJson>,
    #[serde(default)]
    dependencies: Vec<DependencyJson>,
    #[serde(default)]
    folder_access: bool,
    #[serde(default)]
    file_index: bool,
    #[serde(default)]
    preferences: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HelperJson {
    id: String,
    targets: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DependencyJson {
    id: String,
    source: String,
    #[serde(default)]
    optional: bool,
    operations: Vec<RequiredOperationJson>,
    #[serde(default)]
    platforms: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequiredOperationJson {
    id: String,
    version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationJson {
    id: String,
    version: u32,
    component: String,
    #[serde(default)]
    platforms: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandJson {
    id: String,
    title: String,
    #[serde(default)]
    subtitle: Option<String>,
    /// Checked by [`icons::parse_manifest_icon`].
    #[serde(default)]
    icon: Option<serde_json::Value>,
    component: String,
    #[serde(default)]
    platforms: Option<Vec<String>>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    when: Option<String>,
    #[serde(default)]
    matches: Option<String>,
    #[serde(default)]
    root_results: bool,
    #[serde(default)]
    indexed_results: bool,
    #[serde(default)]
    takes_query: bool,
    #[serde(default)]
    search: bool,
    #[serde(default)]
    schedule: Option<ScheduleJson>,
    #[serde(default)]
    service: bool,
    #[serde(default)]
    preferences: Vec<serde_json::Value>,
    /// Checked by `arguments::parse`, which says what is wrong in Pane's
    /// words.
    #[serde(default)]
    arguments: Option<serde_json::Value>,
}

/// A command's `schedule`, as `pane.json` writes it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScheduleJson {
    every_seconds: u64,
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

    /// Reads a managed copy: like [`Manifest::read`], but a copy for other
    /// systems is read, so that its commands can be listed as unavailable.
    fn read_installed(folder: &Path) -> Result<Manifest, PackageError> {
        let (manifest, _) = Manifest::read_parsed(folder)?;
        manifest.check_components(folder)?;
        Ok(manifest)
    }

    /// Reads and parses `pane.json` in `folder`, without checking that its
    /// components exist (a development build is about to make them).
    pub(crate) fn read_parsed(folder: &Path) -> Result<(Manifest, String), PackageError> {
        let path = folder.join(MANIFEST_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(PackageError::NoManifest(folder.to_path_buf()));
            }
            Err(error) => return Err(PackageError::InvalidManifest(error.to_string())),
        };
        let manifest = Manifest::parse(&text)?;
        Ok((manifest, text))
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
    /// with what it serves as people know it: a command's title, or
    /// "operation `<id>`". A component serving several appears once per use.
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
        commands.chain(operations)
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
            search: commands().any(|command| command.search),
            service: commands().any(|command| command.service),
            operations: self
                .operations
                .iter()
                .any(|operation| operation.component == component),
        }
    }

    fn parse(text: &str) -> Result<Manifest, PackageError> {
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
        if !api_compatible(&json.api_version)? {
            return Err(PackageError::IncompatibleApi(json.api_version));
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
        if json.commands.is_empty() && json.operations.is_empty() {
            return Err(invalid("`commands` is empty".into()));
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
            // A command may both search inside itself and answer root
            // search (Search Files, #150): root search still never asks one
            // that searches unless its manifest says `rootResults` too, so
            // what is typed there reaches only a command that asks for it.
            let mode = match command.mode.as_deref() {
                None | Some("view") => CommandMode::View,
                Some("no-view") => CommandMode::NoView,
                Some("provider") => CommandMode::Provider,
                Some(other) => {
                    return Err(invalid(format!(
                        "command `{}` has the mode \"{}\"; a command's `mode` is \"view\" (it \
                         opens a screen, the default), \"no-view\" (it runs without one) or \
                         \"provider\" (it only answers root search)",
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
                mode,
                when,
                matches,
                root_results: command.root_results,
                indexed_results: command.indexed_results,
                takes_query: command.takes_query,
                search: command.search,
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
        Ok(Manifest {
            title: json.title,
            description: json
                .description
                .map(|description| description.trim().to_owned())
                .filter(|description| !description.is_empty()),
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
        })
    }

    /// The dependency the package's code calls `id`.
    pub fn dependency(&self, id: &str) -> Option<&ManifestDependency> {
        self.dependencies
            .iter()
            .find(|dependency| dependency.id == id)
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
    if command.search {
        return refused("search as the user types into its own field (`search`)");
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
        (CommandMode::View, item) => {
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
    let invalid = |reason: &str| {
        Err(PackageError::InvalidManifest(format!(
            "the source `{source}` of dependency `{id}` {reason}"
        )))
    };
    let path = match SourceSpec::parse(source) {
        Ok(SourceSpec::Npm(_) | SourceSpec::Git(_)) => return Ok(()),
        Ok(SourceSpec::Local(path)) => path,
        Err(SourceError::Npm(why)) => return invalid(&format!("is not an npm package: {why}")),
        Err(SourceError::Git(why)) => return invalid(&format!("is not a Git repository: {why}")),
        Err(SourceError::UnknownKind) => {
            return invalid(
                "must be `local:` followed by a folder path, `npm:` followed by a package name \
                 or `git:` followed by a repository; other sources are not supported yet",
            );
        }
    };
    if path.is_empty() {
        return invalid("must be `local:` followed by a folder path");
    }
    let drive =
        path.len() >= 2 && path.as_bytes()[1] == b':' && path.as_bytes()[0].is_ascii_alphabetic();
    if path.contains('\\') || drive || path.starts_with("//") {
        return invalid(
            "must separate folders with `/`, without a drive letter, `\\` or a `//server` \
             share, so that every system reads it alike (such as `local:../greeter`)",
        );
    }
    Ok(())
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

/// Whether a package needing extension API `required` (`MAJOR.MINOR`, with
/// an optional `.PATCH`) runs on this Pane. Before 1.0 each minor version is
/// its own API; from 1.0 an older minor version of the same major is served.
fn api_compatible(required: &str) -> Result<bool, PackageError> {
    let parts: Vec<Option<u64>> = required.split('.').map(|part| part.parse().ok()).collect();
    let (major, minor) = match parts.as_slice() {
        [Some(major), Some(minor)] | [Some(major), Some(minor), Some(_)] => (*major, *minor),
        _ => {
            return Err(PackageError::InvalidManifest(format!(
                "apiVersion `{required}` is not a version such as `0.1`"
            )));
        }
    };
    let (host_major, host_minor) = EXTENSION_API;
    Ok(if major == 0 {
        host_major == 0 && minor == host_minor
    } else {
        major == host_major && minor <= host_minor
    })
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
    /// The package needs an extension API this Pane does not provide.
    IncompatibleApi(String),
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
    /// A default extension's payload cannot be acquired or installed; the
    /// message says why.
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
            PackageError::IncompatibleApi(required) => write!(
                f,
                "Incompatible package: it needs Pane extension API {required}, but this Pane provides {}.{}",
                EXTENSION_API.0, EXTENSION_API.1
            ),
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
    /// Where a default extension's payload was acquired from; `None`
    /// otherwise.
    pub default: Option<crate::defaults::DefaultOrigin>,
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
        Ok(SourcePackage {
            identity: PackageIdentity::npm(&name),
            folder,
            manifest,
            manifest_text,
            npm: Some(origin),
            git: None,
            default: None,
            _download: Some(std::sync::Arc::new(download)),
            network: false,
            programs: false,
        })
    }

    /// Reads the revision of a Git package that Pane fetched and wrote out,
    /// as the package with its Git identity. Explains, rather than as for a
    /// folder, a revision without `pane.json` at the repository's root and
    /// one without its built components (a source-only revision), and a
    /// component stored with Git LFS.
    pub(crate) fn read_git(fetched: crate::git::Fetched) -> Result<SourcePackage, PackageError> {
        let crate::git::Fetched { download, origin } = fetched;
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
            _download: Some(std::sync::Arc::new(download)),
            network: false,
            programs: false,
        })
    }

    /// Reads the payload of the default extension Pane acquired from its
    /// own downloads, as the package with the default extension's
    /// identity. A payload is unpacked and checked as an npm package's
    /// tarball is, so a payload without `pane.json` or without its built
    /// components is explained like one.
    pub(crate) fn read_default(
        fetched: crate::defaults::Fetched,
    ) -> Result<SourcePackage, PackageError> {
        let crate::defaults::Fetched { download, origin } = fetched;
        let folder = download.folder().to_path_buf();
        let id = origin.id.clone();
        let (manifest, manifest_text) = match Manifest::read_text(&folder) {
            Ok(read) => read,
            Err(PackageError::NoManifest(_)) => {
                return Err(PackageError::Defaults(format!(
                    "the payload of Pane's default extension {id} is not a Pane extension: it \
                     has no {MANIFEST_FILE}, and Pane does not install what does not hold one"
                )));
            }
            Err(error) => return Err(error),
        };
        Ok(SourcePackage {
            identity: PackageIdentity::default_extension(&id),
            folder,
            manifest,
            manifest_text,
            npm: None,
            git: None,
            default: Some(origin),
            _download: Some(std::sync::Arc::new(download)),
            network: false,
            programs: false,
        })
    }

    /// What identifies the download this package was read from, when it
    /// was downloaded: its npm tarball's integrity, its Git commit, or its
    /// default payload's integrity. A new download with the same
    /// `pane.json` is another plan.
    pub(crate) fn fingerprint(&self) -> Option<String> {
        match (&self.npm, &self.git, &self.default) {
            (Some(npm), _, _) => Some(npm.integrity.clone()),
            (None, Some(git), _) => Some(git.revision.commit.clone()),
            (None, None, Some(default)) => Some(default.integrity.clone()),
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
            _download: None,
            network: false,
            programs: false,
        })
    }

    pub fn read(folder: &Path) -> Result<SourcePackage, PackageError> {
        let identity = PackageIdentity::local(folder)?;
        let folder = identity
            .local_folder()
            .expect("a local identity has a folder")
            .to_path_buf();
        let (manifest, manifest_text) = Manifest::read_text(&folder)?;
        Ok(SourcePackage {
            identity,
            folder,
            manifest,
            manifest_text,
            npm: None,
            git: None,
            default: None,
            _download: None,
            network: false,
            programs: false,
        })
    }

    /// The `pane.json` text the manifest was read from.
    pub(crate) fn manifest_text(&self) -> &str {
        &self.manifest_text
    }
}

/// An installed package, as read from its managed copy.
#[derive(Clone, Debug)]
pub struct InstalledPackage {
    pub identity: PackageIdentity,
    /// The manifest of the managed copy, or why it could not be read.
    pub manifest: Result<Manifest, PackageError>,
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
    /// The package whose managed copy is at `location`, with the identities
    /// its dependencies were resolved to as `recorded`; one not recorded
    /// (installed before Pane recorded them) is resolved now.
    fn load(
        identity: PackageIdentity,
        location: PathBuf,
        enabled: bool,
        uses_network: bool,
        recorded: &[ResolvedJson],
        npm: Option<&NpmRecordJson>,
        git: Option<&GitRecordJson>,
    ) -> InstalledPackage {
        let manifest = Manifest::read_installed(&location);
        let dependencies = match &manifest {
            Ok(manifest) => manifest
                .dependencies
                .iter()
                .filter_map(|dependency| {
                    let resolved = match recorded.iter().find(|r| r.id == dependency.id) {
                        Some(record) => PackageIdentity(record.source.clone()),
                        None => identity.dependency(&dependency.source).ok()?,
                    };
                    Some((dependency.id.clone(), resolved))
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        let npm = npm.and_then(|npm| npm.package(&identity));
        let git = git.and_then(|git| git.installed(&identity));
        let mut package = InstalledPackage {
            manifest,
            identity,
            location,
            enabled,
            npm,
            git,
            uses_network,
            // Its record says, once loaded (see `Store::installed`).
            uses_programs: false,
            dependencies,
            icons: PackageIcons {
                package: Icon::letter_of(""),
                commands: Vec::new(),
            },
            // Its record says, once loaded (see `Store::installed`).
            disabled_commands: BTreeSet::new(),
        };
        package.icons = PackageIcons::of(
            package.manifest.as_ref().ok(),
            &package.title(),
            &package.location,
        );
        package
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
        match &self.manifest {
            Ok(manifest) => manifest.title.clone(),
            Err(_) => match (
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
        self.manifest.as_ref().ok()?.version.clone()
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
        self.manifest.as_ref().ok()?.description.as_deref()
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
                    search: command.search,
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
    /// For a package from Git, the address fetched and the revision
    /// installed.
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    git: Option<GitRecordJson>,
    /// For a default extension Pane acquired, the version installed.
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    default: Option<DefaultRecordJson>,
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
/// commit.
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
}

impl GitRecordJson {
    fn of(origin: &GitOrigin) -> GitRecordJson {
        GitRecordJson {
            url: origin.repository.url().to_owned(),
            reference: origin.revision.ref_name(),
            commit: origin.revision.commit.clone(),
            pinned: origin.revision.pinned(),
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
}

/// The version of an acquired default extension as its record writes it,
/// beside the default extension its source records: `"default":
/// "calculator", "defaultVersion": "0.1.0"`. The payload's integrity
/// identified the download and is not kept: what is kept is what was
/// installed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct DefaultRecordJson {
    #[serde(rename = "defaultVersion")]
    version: String,
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
            let PackageIdentity(source) = package.identity.dependency(&dependency.source).ok()?;
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
                let mut package = InstalledPackage::load(
                    PackageIdentity(record.source.clone()),
                    self.dir.join(PACKAGES_DIR).join(&record.dir),
                    !record.disabled,
                    record.network,
                    &record.dependencies,
                    record.npm.as_ref(),
                    record.git.as_ref(),
                );
                package.uses_programs = record.programs;
                package.disabled_commands = record.disabled_commands.iter().cloned().collect();
                package
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
        let git = package.git.as_ref().map(GitRecordJson::of);
        let default = package.default.as_ref().map(|origin| DefaultRecordJson {
            version: origin.version().to_owned(),
        });
        // An update keeps the record, so a disabled package stays disabled.
        let enabled = match updated.packages.iter_mut().find(|r| &r.source == local) {
            Some(record) => {
                record.dir = dir;
                // New code has not failed.
                record.paused = None;
                record.dependencies = dependencies.clone();
                record.npm = npm.clone();
                record.git = git.clone();
                record.default = default.clone();
                record.network = package.network;
                record.programs = package.programs;
                !record.disabled
            }
            None => {
                // Data kept from an earlier installation of this identity
                // is its own again.
                updated.retained.retain(|record| &record.source != local);
                updated.packages.push(RecordJson {
                    source: local.clone(),
                    npm: npm.clone(),
                    git: git.clone(),
                    default: default.clone(),
                    dir,
                    disabled: false,
                    paused: None,
                    dependencies: dependencies.clone(),
                    network: package.network,
                    programs: package.programs,
                    disabled_commands: Vec::new(),
                });
                true
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
        let mut installed = InstalledPackage::load(
            package.identity.clone(),
            location,
            enabled,
            package.network,
            &dependencies,
            npm.as_ref(),
            git.as_ref(),
        );
        installed.uses_programs = package.programs;
        // An update keeps the commands the user turned off.
        installed.disabled_commands = registry
            .packages
            .iter()
            .find(|record| &record.source == local)
            .map(|record| record.disabled_commands.iter().cloned().collect())
            .unwrap_or_default();
        Ok(installed)
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
            fs::copy(source, target)?;
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
            r#"{{ "manifestVersion": 1, "title": "Tool", "apiVersion": "0.1",
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
}
