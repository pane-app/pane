//! Extension packages: the `pane.json` manifest, source-derived package
//! identity, and Pane's managed copies of installed packages.
//!
//! A local package is a folder holding `pane.json` and the components it
//! names. Installing copies exactly those files into Pane's managed location,
//! so the user's folder is never written and the installed copy keeps working
//! if the folder changes or disappears. Installed packages are recorded in
//! `installed.json`, with whether the user disabled each; reading them back
//! needs only the manifests, never the guests.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Component as PathPart, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atomic::{Readers, write_atomically};
use crate::launcher::CommandRegistration;
use crate::platform::{self, Platform};
use crate::runtime::{CallError, Exports};

/// The manifest file at the root of every package.
pub const MANIFEST_FILE: &str = "pane.json";

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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Source {
    Local(String),
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
        Ok(PackageIdentity(Source::Local(path.to_owned())))
    }

    /// A stable key for this identity, for ids and records rather than for
    /// people to read: `local:` followed by the folder's resolved path. The
    /// [`Display`](fmt::Display) form is the wording shown to users.
    pub fn key(&self) -> String {
        match &self.0 {
            Source::Local(path) => format!("local:{path}"),
        }
    }

    /// The source folder of a local package.
    pub fn local_folder(&self) -> Option<&Path> {
        match &self.0 {
            Source::Local(path) => Some(Path::new(path)),
        }
    }
}

impl fmt::Display for PackageIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Source::Local(path) => write!(f, "local folder {path}"),
        }
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

/// Windows' canonical paths carry a `\\?\` prefix; the identity uses the
/// ordinary spelling (`C:\…`, `\\server\share\…`) that users recognise.
fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
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

/// A package's manifest, `pane.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub title: String,
    pub version: Option<String>,
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

/// A command a package contributes to root search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestCommand {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
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
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestJson {
    title: String,
    #[serde(default)]
    version: Option<String>,
    api_version: String,
    #[serde(default)]
    platforms: Option<Vec<String>>,
    #[serde(default)]
    commands: Vec<CommandJson>,
    #[serde(default)]
    operations: Vec<OperationJson>,
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
    component: String,
    #[serde(default)]
    platforms: Option<Vec<String>>,
    #[serde(default)]
    root_results: bool,
    #[serde(default)]
    indexed_results: bool,
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
        Ok((manifest, text))
    }

    /// Reads a managed copy: like [`Manifest::read`], but a copy for other
    /// systems is read, so that its commands can be listed as unavailable.
    fn read_installed(folder: &Path) -> Result<Manifest, PackageError> {
        let (manifest, _) = Manifest::read_parsed(folder)?;
        manifest.check_components(folder)?;
        Ok(manifest)
    }

    /// Reads and parses `pane.json` in `folder`.
    fn read_parsed(folder: &Path) -> Result<(Manifest, String), PackageError> {
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

    /// Checks that every component the manifest names is in `folder`.
    fn check_components(&self, folder: &Path) -> Result<(), PackageError> {
        for (name, component) in self.components() {
            if !folder.join(component).is_file() {
                return Err(PackageError::MissingComponent {
                    command: name,
                    component: component.to_path_buf(),
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
        let platforms = parse_platforms(json.platforms, "`platforms`")?;
        if json.commands.is_empty() && json.operations.is_empty() {
            return Err(invalid("`commands` is empty".into()));
        }
        let mut commands = Vec::new();
        for command in json.commands {
            if command.id.is_empty() || command.title.trim().is_empty() {
                return Err(invalid("every command needs an `id` and a `title`".into()));
            }
            if commands
                .iter()
                .any(|seen: &ManifestCommand| seen.id == command.id)
            {
                return Err(invalid(format!("command id `{}` is repeated", command.id)));
            }
            let component = inside_package(&command.component)?;
            let platforms = parse_platforms(
                command.platforms,
                &format!("`platforms` of command `{}`", command.id),
            )?;
            commands.push(ManifestCommand {
                id: command.id,
                title: command.title,
                subtitle: command.subtitle,
                component,
                platforms,
                root_results: command.root_results,
                indexed_results: command.indexed_results,
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
                component: inside_package(&operation.component)?,
                id: operation.id,
                version: operation.version,
            });
        }
        Ok(Manifest {
            title: json.title,
            version: json.version,
            api_version: json.api_version,
            platforms,
            commands,
            operations,
        })
    }
}

/// `component` as a path, if it is a relative path inside the package folder.
fn inside_package(component: &str) -> Result<PathBuf, PackageError> {
    let path = PathBuf::from(component);
    let inside = path
        .components()
        .all(|part| matches!(part, PathPart::Normal(_)));
    if !inside || component.is_empty() {
        return Err(PackageError::InvalidManifest(format!(
            "component `{component}` must be a relative path inside the package folder"
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
    /// A component is present but Pane cannot run it.
    Component { command: String, error: CallError },
    /// A package with this identity is already installed.
    AlreadyInstalled(PackageIdentity),
    /// No package with this identity is installed.
    NotInstalled(PackageIdentity),
    /// Pane's managed location could not be read or written.
    Storage(String),
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
}

impl SourcePackage {
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
        })
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
}

impl InstalledPackage {
    fn load(identity: PackageIdentity, location: PathBuf, enabled: bool) -> InstalledPackage {
        InstalledPackage {
            manifest: Manifest::read_installed(&location),
            identity,
            location,
            enabled,
        }
    }

    /// The package's display title.
    pub fn title(&self) -> String {
        match &self.manifest {
            Ok(manifest) => manifest.title.clone(),
            Err(_) => match self.identity.local_folder() {
                Some(folder) => folder_name(folder),
                None => self.identity.to_string(),
            },
        }
    }

    pub fn version(&self) -> Option<String> {
        self.manifest.as_ref().ok()?.version.clone()
    }

    /// The commands this package offers in root search.
    pub fn commands(&self) -> Vec<CommandRegistration> {
        self.available_commands()
            .into_iter()
            .map(|(command, _)| command)
            .collect()
    }

    /// The commands this package offers in root search, each with why it is
    /// unavailable on this system, if it is: first because the package does
    /// not support this system, else because the command does not.
    pub(crate) fn available_commands(&self) -> Vec<(CommandRegistration, Option<String>)> {
        let Ok(manifest) = &self.manifest else {
            return Vec::new();
        };
        let package = platform::unavailable(manifest.platforms.as_deref(), "this package");
        manifest
            .commands
            .iter()
            .map(|command| {
                let registration = CommandRegistration {
                    id: format!("{}#{}", self.identity.key(), command.id),
                    title: command.title.clone(),
                    subtitle: command
                        .subtitle
                        .clone()
                        .or_else(|| Some(manifest.title.clone())),
                    component: self.location.join(&command.component),
                };
                let unavailable = package.clone().or_else(|| {
                    platform::unavailable(command.platforms.as_deref(), "this command")
                });
                (registration, unavailable)
            })
            .collect()
    }
}

impl InstalledPackage {
    /// The commands of this package that compute root results and can run
    /// on this system; none if the package cannot be read.
    pub(crate) fn root_result_commands(&self) -> Vec<CommandRegistration> {
        let Ok(manifest) = &self.manifest else {
            return Vec::new();
        };
        self.available_commands()
            .into_iter()
            .zip(&manifest.commands)
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
        self.available_commands()
            .into_iter()
            .zip(&manifest.commands)
            .filter(|((_, unavailable), command)| command.indexed_results && unavailable.is_none())
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
    /// The local source folder, the identity the data belongs to.
    local: String,
    /// The package's title when it was uninstalled.
    title: String,
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
    /// The local source folder, the package's identity.
    local: String,
    /// The managed folder under `packages/`.
    dir: String,
    /// Set when the user disabled the package; absent means enabled.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    disabled: bool,
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
        let registry = match fs::read_to_string(dir.join(REGISTRY_FILE)) {
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
        .map_err(|reason| format!("{}: {reason}", dir.join(REGISTRY_FILE).display()));
        let mut store = Store { dir, registry };
        store.remove_leftovers();
        store
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
                InstalledPackage::load(
                    PackageIdentity(Source::Local(record.local.clone())),
                    self.dir.join(PACKAGES_DIR).join(&record.dir),
                    !record.disabled,
                )
            })
            .collect()
    }

    pub fn is_installed(&self, identity: &PackageIdentity) -> bool {
        self.record(identity).is_some()
    }

    fn record(&self, identity: &PackageIdentity) -> Option<&RecordJson> {
        let PackageIdentity(Source::Local(path)) = identity;
        self.registry
            .as_ref()
            .ok()?
            .packages
            .iter()
            .find(|record| &record.local == path)
    }

    /// Installs a package whose identity is not installed yet.
    pub fn install(&mut self, package: &SourcePackage) -> Result<InstalledPackage, PackageError> {
        if self.is_installed(&package.identity) {
            return Err(PackageError::AlreadyInstalled(package.identity.clone()));
        }
        self.write_copy(package, None)
    }

    /// Replaces the managed copy of an installed package with the current
    /// contents of its source; the identity and its record stay the same.
    pub fn update(&mut self, package: &SourcePackage) -> Result<InstalledPackage, PackageError> {
        let Some(record) = self.record(&package.identity) else {
            return Err(PackageError::NotInstalled(package.identity.clone()));
        };
        let old = record.dir.clone();
        self.write_copy(package, Some(old))
    }

    /// Records whether the installed package with `identity` is enabled.
    /// Its managed copy and settings are left as they are.
    pub fn set_enabled(
        &mut self,
        identity: &PackageIdentity,
        enabled: bool,
    ) -> Result<(), PackageError> {
        let registry = self
            .registry
            .as_mut()
            .map_err(|reason| PackageError::Storage(reason.clone()))?;
        let PackageIdentity(Source::Local(local)) = identity;
        let mut updated = registry.clone();
        let Some(record) = updated.packages.iter_mut().find(|r| &r.local == local) else {
            return Err(PackageError::NotInstalled(identity.clone()));
        };
        record.disabled = !enabled;
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
                identity: PackageIdentity(Source::Local(record.local.clone())),
                title: record.title.clone(),
            })
            .collect()
    }

    /// Uninstalls the package with `identity`: its record goes, then its
    /// managed copy. With `retain`, the identity is recorded as keeping
    /// extension data under that title, in the same write. A failure to
    /// record it leaves everything as it was. A managed folder that cannot
    /// be removed (one in use on Windows) is listed so that the next start
    /// removes it, as an update's replaced copy is.
    pub fn uninstall(
        &mut self,
        identity: &PackageIdentity,
        retain: Option<String>,
    ) -> Result<Leftover, PackageError> {
        let registry = self
            .registry
            .as_mut()
            .map_err(|reason| PackageError::Storage(reason.clone()))?;
        let PackageIdentity(Source::Local(local)) = identity;
        let mut updated = registry.clone();
        let Some(index) = updated.packages.iter().position(|r| &r.local == local) else {
            return Err(PackageError::NotInstalled(identity.clone()));
        };
        let record = updated.packages.remove(index);
        if let Some(title) = retain {
            put_retained(&mut updated, local, title);
        }
        write_registry(&self.dir, &updated)
            .map_err(|error| PackageError::Storage(error.to_string()))?;
        *registry = updated;
        let location = self.dir.join(PACKAGES_DIR).join(&record.dir);
        let Err(error) = fs::remove_dir_all(&location) else {
            return Ok(Leftover::None);
        };
        if error.kind() == io::ErrorKind::NotFound {
            return Ok(Leftover::None);
        }
        let mut listed = registry.clone();
        listed.leftovers.push(record.dir);
        Ok(match write_registry(&self.dir, &listed) {
            Ok(()) => {
                *registry = listed;
                Leftover::Listed(location, error.to_string())
            }
            Err(_) => Leftover::Unlisted(location, error.to_string()),
        })
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
        let PackageIdentity(Source::Local(local)) = identity;
        let mut updated = registry.clone();
        put_retained(&mut updated, local, title);
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
        let PackageIdentity(Source::Local(local)) = &package.identity;
        let mut updated = RegistryJson {
            next: number + 1,
            ..registry.clone()
        };
        // An update keeps the record, so a disabled package stays disabled.
        let enabled = match updated.packages.iter_mut().find(|r| &r.local == local) {
            Some(record) => {
                record.dir = dir;
                !record.disabled
            }
            None => {
                // Data kept from an earlier installation of this identity
                // is its own again.
                updated.retained.retain(|record| &record.local != local);
                updated.packages.push(RecordJson {
                    local: local.clone(),
                    dir,
                    disabled: false,
                });
                true
            }
        };
        if let Err(error) = write_registry(&self.dir, &updated) {
            let _ = fs::remove_dir_all(&location);
            return Err(storage(error));
        }
        *registry = updated;
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
        Ok(InstalledPackage::load(
            package.identity.clone(),
            location,
            enabled,
        ))
    }
}

/// Records in `registry` that data is kept for the local source `local`,
/// last uninstalled as `title`.
fn put_retained(registry: &mut RegistryJson, local: &str, title: String) {
    registry.retained.retain(|record| record.local != local);
    registry.retained.push(RetainedJson {
        local: local.to_owned(),
        title,
    });
}

/// Writes the validated `pane.json` and copies the components it names,
/// keeping their relative paths. Nothing else in the source folder is copied.
fn copy_package(package: &SourcePackage, location: &Path) -> io::Result<()> {
    fs::create_dir_all(location)?;
    fs::write(location.join(MANIFEST_FILE), &package.manifest_text)?;
    for (_, component) in package.manifest.components() {
        let source = package.folder.join(component);
        let target = location.join(component);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
    }
    Ok(())
}

/// Replaces the registry whole (see [`write_atomically`] for what a crash or
/// a second Pane process can do to it).
fn write_registry(dir: &Path, registry: &RegistryJson) -> io::Result<()> {
    let text = serde_json::to_string_pretty(registry).map_err(io::Error::other)?;
    write_atomically(&dir.join(REGISTRY_FILE), text.as_bytes(), Readers::Default)
}
