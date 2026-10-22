//! Extension packages distributed through the npm registry.
//!
//! An npm-distributed Pane package is an ordinary npm package whose tarball
//! holds a `pane.json` and the built components it names, as a local package
//! folder does. Pane downloads it itself (no Node or npm is needed): it reads
//! the package's metadata from the registry, takes the version asked for or
//! else the one tagged `latest`, downloads that version's tarball over HTTPS,
//! checks it against the sha512 `dist.integrity` the registry gives, and
//! unpacks it into a folder of its own under Pane's data folder, from which
//! the package is installed as a local folder is.
//!
//! Nothing in the package runs to install it: npm's lifecycle scripts
//! (`preinstall`, `install`, `postinstall`, `prepare`…) are never run, and
//! the package's own npm dependencies are not installed; only the components
//! `pane.json` lists are ever run, by Pane's runtime.
//!
//! Unpacking takes only regular files and folders: a symbolic or hard link,
//! a device or anything else is refused, as is a path that would leave the
//! folder (`..`, an absolute path, `\`, a drive letter) or that a system
//! reads differently (a Windows device name, a trailing dot). The metadata,
//! the tarball and what it unpacks to are limited in size.
//!
//! The registry is `https://registry.npmjs.org/`. Tests and development
//! builds can name another ([`Registry::local`]), which must be on this
//! computer (a loopback address), so that no check ever reaches the network.

use std::fmt;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::downloads::Download;
use crate::http::{Answer, GetError, Origin};
use crate::integrity::{check_integrity, sha512_values};

/// The public npm registry.
pub const NPMJS: &str = "https://registry.npmjs.org/";

/// The largest package metadata Pane reads from the registry.
pub const MAX_METADATA: u64 = 16 << 20;
/// The largest tarball Pane downloads.
pub const MAX_TARBALL: u64 = 64 << 20;
/// The most a tarball may unpack to, all files together.
pub const MAX_UNPACKED: u64 = 256 << 20;
pub use crate::archive::{MAX_ENTRIES, MAX_EXTENSION};

/// The npm lifecycle scripts npm runs when installing a package, which Pane
/// never runs.
const INSTALL_SCRIPTS: &[&str] = &[
    "preinstall",
    "install",
    "postinstall",
    "prepublish",
    "preprepare",
    "prepare",
    "postprepare",
];

/// The npm registry Pane downloads packages from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registry {
    /// Its address, and whether it is on this computer.
    origin: Origin,
}

impl Default for Registry {
    fn default() -> Registry {
        Registry::npmjs()
    }
}

impl Registry {
    /// The public npm registry, over HTTPS.
    pub fn npmjs() -> Registry {
        Registry {
            origin: Origin::public(NPMJS),
        }
    }

    /// A registry on this computer, for tests and development builds only:
    /// `url` must be `http://` or `https://` on a loopback address written
    /// as one (`127.0.0.1`, any `127.x.y.z`, or `[::1]`), with an optional
    /// port and path. Any other address is refused, `localhost` included (a
    /// name could resolve elsewhere), so that nothing but the public
    /// registry is ever reached over the network. Release builds have no
    /// way to replace the public registry.
    #[cfg(any(test, debug_assertions))]
    pub fn local(url: &str) -> Result<Registry, String> {
        let refused = || {
            format!(
                "the npm registry `{url}` is not on this computer: Pane uses \
                 https://registry.npmjs.org/, and only a registry on a loopback address such as \
                 127.0.0.1 or [::1] can replace it, for tests and development"
            )
        };
        let origin = Origin::local(url, refused)?;
        Ok(Registry { origin })
    }

    /// The registry named by `PANE_NPM_REGISTRY`, in development builds
    /// only (see [`Registry::local`]); `None` when it is not set.
    #[cfg(any(test, debug_assertions))]
    pub fn from_dev_env() -> Option<Result<Registry, String>> {
        crate::http::dev_env("PANE_NPM_REGISTRY").map(|url| Registry::local(&url))
    }

    /// Its address, ending with `/`.
    pub fn url(&self) -> &str {
        self.origin.url()
    }

    /// The address of the metadata of package `name`: a scoped name's `/`
    /// is written `%2f`, as npm does.
    fn metadata_url(&self, name: &str) -> String {
        format!("{}{}", self.url(), name.replace('/', "%2f"))
    }

    /// Asks the registry for `url`, with `headers`, a body of at most `most`
    /// bytes, through the connections guests' requests use
    /// ([`crate::http::get_blocking`]): only over HTTPS unless the registry is
    /// on this computer.
    fn get(&self, url: &str, headers: &[(&str, &str)], most: u64) -> Result<Answer, GetError> {
        self.origin.get(url, headers, most)
    }

    /// Why Pane does not download `url` for this registry, if it does not:
    /// a tarball must come from the registry's own scheme, host and port.
    fn refusal(&self, url: &str) -> Option<String> {
        let (scheme, authority, _) =
            crate::http::split_url(self.url()).expect("a registry address is a URL");
        match crate::http::split_url(url) {
            Some((s, a, _)) if s == scheme && a.eq_ignore_ascii_case(authority) => None,
            _ => Some(format!(
                "its tarball address {url} is not on the registry {}: Pane downloads a package \
                 only from the registry that describes it{}",
                self.url(),
                if scheme == "https" {
                    ", over HTTPS"
                } else {
                    ""
                }
            )),
        }
    }
}

/// An npm package to install, as the user or a dependency names it: a name,
/// scoped (`@scope/name`) or not, and optionally an exact version
/// (`name@1.2.3`); without one Pane takes the version tagged `latest`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NpmSpec {
    pub name: String,
    pub version: Option<String>,
}

impl NpmSpec {
    /// Reads `name`, `@scope/name`, `name@1.2.3` or `@scope/name@1.2.3`,
    /// with or without `npm:` before it.
    pub fn parse(text: &str) -> Result<NpmSpec, String> {
        let text = text.trim();
        let text = text.strip_prefix("npm:").unwrap_or(text);
        let (name, version) = match text.strip_prefix('@') {
            Some(scoped) => match scoped.split_once('@') {
                Some((name, version)) => (&text[..name.len() + 1], Some(version)),
                None => (text, None),
            },
            None => match text.split_once('@') {
                Some((name, version)) => (name, Some(version)),
                None => (text, None),
            },
        };
        check_name(name)?;
        if let Some(version) = version {
            check_version(version)?;
        }
        Ok(NpmSpec {
            name: name.to_owned(),
            version: version.map(ToOwned::to_owned),
        })
    }
}

impl fmt::Display for NpmSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.version {
            Some(version) => write!(f, "{}@{version}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

/// Checks an npm package name as npm accepts new ones: at most 214
/// characters, lowercase letters, digits, `-`, `.`, `_` and `~`, not
/// starting with `.` or `_`, optionally in a scope (`@scope/name`).
pub(crate) fn check_name(name: &str) -> Result<(), String> {
    let invalid = |why: &str| Err(format!("`{name}` is not an npm package name: {why}"));
    if name.is_empty() {
        return Err("an npm package name is needed, such as `@scope/name`".into());
    }
    if name.len() > 214 {
        return invalid("it is longer than 214 characters");
    }
    let parts: Vec<&str> = match name.strip_prefix('@') {
        Some(scoped) => match scoped.split_once('/') {
            Some((scope, name)) => vec![scope, name],
            None => return invalid("a scoped name is `@scope/name`"),
        },
        None => vec![name],
    };
    for part in parts {
        if part.is_empty() {
            return invalid("a scoped name is `@scope/name`");
        }
        if part.starts_with('.') || part.starts_with('_') {
            return invalid("it cannot start with `.` or `_`");
        }
        let allowed = |c: char| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.' | '_' | '~')
        };
        if !part.chars().all(allowed) {
            return invalid("use lowercase letters, digits, `-`, `.`, `_` and `~`");
        }
    }
    Ok(())
}

/// Checks that `version` is an exact version such as `1.2.3`, `1.2.3-beta.1`
/// or `1.2.3+build`: Pane installs one version, never a range or a tag.
fn check_version(version: &str) -> Result<(), String> {
    let refused = || {
        Err(format!(
            "`{version}` is not an exact version such as 1.2.3: Pane installs the version you \
             name or, without one, the latest; ranges and tags are not supported"
        ))
    };
    let (core, rest) = match version.find(['-', '+']) {
        Some(at) => version.split_at(at),
        None => (version, ""),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    let number = |part: &&str| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
    if numbers.len() != 3 || !numbers.iter().all(number) {
        return refused();
    }
    let label = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+');
    if rest.len() == 1 || !rest.chars().all(label) {
        return refused();
    }
    Ok(())
}

/// An npm package at one version, as Pane downloaded, installed and
/// records it: its name, the version, and whether it is pinned to that
/// version (the user, or the dependency that installed it, named it
/// exactly, rather than taking the latest).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NpmPackage {
    pub name: String,
    pub version: String,
    pub pinned: bool,
}

/// Where a previewed or installed npm package was downloaded from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NpmOrigin {
    /// The package and version downloaded.
    pub package: NpmPackage,
    /// The tarball's address.
    pub tarball: String,
    /// The tarball's sha512 integrity, as the registry gave it and Pane
    /// checked it (`sha512-<base64>`).
    pub integrity: String,
    /// The npm lifecycle scripts its `package.json` declares, which Pane did
    /// not run, such as `postinstall`.
    pub scripts: Vec<String>,
    /// Whether its `package.json` declares npm dependencies, which Pane does
    /// not install.
    pub has_npm_dependencies: bool,
}

/// A package downloaded and unpacked.
pub(crate) struct Fetched {
    /// The unpacked package: the tarball's top folder.
    pub download: Download,
    pub origin: NpmOrigin,
}

#[derive(Deserialize)]
struct MetadataJson {
    #[serde(default, rename = "dist-tags")]
    dist_tags: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    versions: std::collections::BTreeMap<String, VersionJson>,
}

#[derive(Deserialize)]
struct VersionJson {
    dist: Option<DistJson>,
}

#[derive(Deserialize)]
struct DistJson {
    tarball: Option<String>,
    integrity: Option<String>,
}

#[derive(Deserialize)]
struct PackageJson {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    scripts: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, serde_json::Value>,
}

/// Reads the package `name`'s metadata from `registry` — its versions and
/// `latest` tag — or why it cannot: the package is not there, the registry
/// cannot be reached, or the answer cannot be read. Blocks on the network.
fn metadata(registry: &Registry, name: &str) -> Result<MetadataJson, String> {
    let metadata_url = registry.metadata_url(name);
    let unreachable = |why: String| {
        format!(
            "Could not reach the npm registry {} for {name}: {why}",
            registry.url()
        )
    };
    let response = registry
        .get(
            &metadata_url,
            &[(
                "Accept",
                "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8",
            )],
            MAX_METADATA,
        )
        .map_err(|error| match error {
            GetError::TooLarge => format!(
                "The npm registry's description of {name} is larger than the {} MiB Pane reads",
                MAX_METADATA >> 20
            ),
            GetError::Failed(why) => unreachable(why),
        })?;
    match response.status {
        200 => {}
        404 => {
            return Err(format!(
                "npm package {name} was not found in the registry {}",
                registry.url()
            ));
        }
        status => {
            return Err(format!(
                "The npm registry {} answered {status} for {name}",
                registry.url()
            ));
        }
    }
    serde_json::from_slice(&response.body).map_err(|error| {
        format!("The npm registry's description of {name} cannot be read: {error}")
    })
}

/// The version the registry tags `latest` for package `name`, or why it
/// cannot be read: the package is not there, the registry cannot be
/// reached, or no version is tagged. Blocks on the network and downloads
/// nothing: it is the check for a newer version of an installed copy (the
/// launcher's `updates`) before anything is fetched.
pub(crate) fn latest_version(registry: &Registry, name: &str) -> Result<String, String> {
    let metadata = metadata(registry, name)?;
    metadata.dist_tags.get("latest").cloned().ok_or_else(|| {
        format!(
            "npm package {name} has no version tagged latest; name the version to install, \
             such as {name}@1.0.0"
        )
    })
}

/// Downloads the package `spec` from `registry` and unpacks it into a new
/// folder in `downloads`, or explains why it cannot. Blocks on the network.
pub(crate) fn fetch(
    registry: &Registry,
    spec: &NpmSpec,
    downloads: &Path,
) -> Result<Fetched, String> {
    let name = &spec.name;
    let metadata = metadata(registry, name)?;
    let unreachable = |why: String| {
        format!(
            "Could not reach the npm registry {} for {name}: {why}",
            registry.url()
        )
    };
    let latest = metadata.dist_tags.get("latest").cloned();
    let version = match (&spec.version, &latest) {
        (Some(version), _) => version.clone(),
        (None, Some(latest)) => latest.clone(),
        (None, None) => {
            return Err(format!(
                "npm package {name} has no version tagged latest; name the version to install, \
                 such as {name}@1.0.0"
            ));
        }
    };
    let Some(described) = metadata.versions.get(&version) else {
        let latest = match &latest {
            Some(latest) => format!("; its latest is {latest}"),
            None => String::new(),
        };
        return Err(format!(
            "npm package {name} has no version {version}{latest}"
        ));
    };
    let dist = described.dist.as_ref();
    let tarball = dist
        .and_then(|dist| dist.tarball.clone())
        .ok_or_else(|| format!("npm package {name}@{version} has no tarball in the registry"))?;
    let integrity = dist
        .and_then(|dist| dist.integrity.clone())
        .filter(|integrity| sha512_values(integrity).next().is_some())
        .ok_or_else(|| {
            format!(
                "npm package {name}@{version} has no sha512 integrity in the registry, which Pane \
                 needs to check its download"
            )
        })?;
    if let Some(refusal) = registry.refusal(&tarball) {
        return Err(format!(
            "Pane does not download {name}@{version}: {refusal}"
        ));
    }
    let response = registry
        .get(&tarball, &[], MAX_TARBALL)
        .map_err(|error| match error {
            GetError::TooLarge => format!(
                "npm package {name}@{version} is larger than the {} MiB Pane downloads",
                MAX_TARBALL >> 20
            ),
            GetError::Failed(why) => unreachable(why),
        })?;
    if response.status != 200 {
        return Err(format!(
            "The npm registry {} answered {} for the tarball of {name}@{version}",
            registry.url(),
            response.status
        ));
    }
    let bytes = response.body;
    check_integrity(&bytes, &integrity).map_err(|why| {
        format!("The download of npm package {name}@{version} {why}; nothing was installed")
    })?;
    let download = Download::create(downloads, |folder| unpack(&bytes, folder))
        .map_err(|why| format!("npm package {name}@{version} cannot be unpacked safely: {why}"))?;
    let package = read_package_json(download.folder())?;
    if package.name.as_deref() != Some(name.as_str())
        || package.version.as_deref() != Some(version.as_str())
    {
        return Err(format!(
            "The tarball of npm package {name}@{version} holds {}@{}, not the package asked for",
            package
                .name
                .as_deref()
                .unwrap_or("a package without a name"),
            package.version.as_deref().unwrap_or("?")
        ));
    }
    let scripts = INSTALL_SCRIPTS
        .iter()
        .filter(|script| package.scripts.contains_key(**script))
        .map(|script| (*script).to_owned())
        .collect();
    Ok(Fetched {
        download,
        origin: NpmOrigin {
            package: NpmPackage {
                name: name.clone(),
                version,
                pinned: spec.version.is_some(),
            },
            tarball,
            integrity,
            scripts,
            has_npm_dependencies: !package.dependencies.is_empty(),
        },
    })
}

/// Reads the `package.json` at the root of an unpacked npm package.
fn read_package_json(folder: &Path) -> Result<PackageJson, String> {
    let text = fs::read(folder.join("package.json"))
        .map_err(|error| format!("the tarball has no readable package.json: {error}"))?;
    serde_json::from_slice(&text)
        .map_err(|error| format!("the tarball's package.json cannot be read: {error}"))
}

/// Unpacks the gzipped tarball `tgz` into `dest`, which must not exist,
/// without its top folder (`package/` in npm's tarballs): only regular files
/// and folders, each inside `dest`, within [`MAX_UNPACKED`] bytes and
/// [`MAX_ENTRIES`] entries. Files are written without execute permission;
/// the mode, owner and time the tarball records are ignored. Refuses the
/// whole tarball, explaining why, at the first entry it cannot take.
///
/// The tarball is read entry by entry as written ("raw"), extension headers
/// included, so that Pane, not the tar reader, decides what they mean: a
/// GNU long name (`L`) or a PAX header's `path` names the next entry, each
/// header is at most [`MAX_EXTENSION`] bytes and counts toward the limits,
/// and a size a PAX header gives must be the one the entry's own header
/// gives, so that no two readers can see different files in one tarball.
pub(crate) fn unpack(tgz: &[u8], dest: &Path) -> Result<(), String> {
    crate::archive::unpack_within(tgz, dest, MAX_UNPACKED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::time::Duration;

    /// A gzipped tarball of `entries`: `(path, kind, contents)`, written
    /// header by header so that paths `tar::Builder` refuses can be made.
    pub(crate) fn tarball(entries: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
        let sized: Vec<_> = entries
            .iter()
            .map(|(path, kind, contents)| (*path, *kind, *contents, contents.len() as u64))
            .collect();
        tarball_declaring(&sized)
    }

    /// Like [`tarball`], each entry declaring the size given, whatever its
    /// contents.
    fn tarball_declaring(entries: &[(&str, tar::EntryType, &[u8], u64)]) -> Vec<u8> {
        let mut tar = Vec::new();
        for (path, kind, contents, size) in entries {
            let mut header = tar::Header::new_gnu();
            let name = &mut header.as_old_mut().name;
            name[..path.len()].copy_from_slice(path.as_bytes());
            header.set_entry_type(*kind);
            header.set_size(*size);
            header.set_mode(0o755);
            if *kind == tar::EntryType::Symlink {
                header.set_link_name("/etc/passwd").unwrap();
            }
            header.set_cksum();
            tar.extend_from_slice(header.as_bytes());
            tar.extend_from_slice(contents);
            tar.resize(tar.len().div_ceil(512) * 512, 0);
        }
        tar.resize(tar.len() + 1024, 0);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        io::Write::write_all(&mut gz, &tar).unwrap();
        gz.finish().unwrap()
    }

    use tar::EntryType::{Directory, Regular};

    #[test]
    fn a_spec_names_a_package_and_optionally_an_exact_version() {
        let spec = |name: &str, version: Option<&str>| NpmSpec {
            name: name.into(),
            version: version.map(Into::into),
        };
        assert_eq!(NpmSpec::parse("greeter"), Ok(spec("greeter", None)));
        assert_eq!(
            NpmSpec::parse(" npm:greeter@1.2.3 "),
            Ok(spec("greeter", Some("1.2.3")))
        );
        assert_eq!(
            NpmSpec::parse("@pane-samples/greeter"),
            Ok(spec("@pane-samples/greeter", None))
        );
        assert_eq!(
            NpmSpec::parse("@pane-samples/greeter@0.1.0-beta.2+build.5"),
            Ok(spec("@pane-samples/greeter", Some("0.1.0-beta.2+build.5")))
        );
        assert_eq!(
            NpmSpec::parse("@pane-samples/greeter@1.0.0")
                .unwrap()
                .to_string(),
            "@pane-samples/greeter@1.0.0"
        );
    }

    #[test]
    fn ranges_tags_and_invalid_names_are_refused() {
        for range in [
            "greeter@^1.0.0",
            "greeter@latest",
            "greeter@1.0",
            "greeter@1.0.0-",
        ] {
            let error = NpmSpec::parse(range).unwrap_err();
            assert!(
                error.contains("is not an exact version such as 1.2.3"),
                "{error}"
            );
        }
        for name in [
            "", "Greeter", "@scope", "@/x", ".hidden", "a b", "../x", "a/b",
        ] {
            assert!(NpmSpec::parse(name).is_err(), "{name:?} was accepted");
        }
    }

    #[test]
    fn only_a_registry_on_this_computer_replaces_npmjs() {
        for url in [
            "http://127.0.0.1:4873",
            "http://127.0.0.2:4873/npm/",
            "https://[::1]:8443/",
        ] {
            assert!(Registry::local(url).is_ok(), "{url}");
        }
        for url in [
            // A name, even this one, could resolve elsewhere.
            "http://localhost:4873/",
            "http://LOCALHOST/",
            "http://::1/",
            "http://[::ffff:127.0.0.1]/",
            "http://0.0.0.0/",
            "https://registry.npmjs.org/",
            "http://127.0.0.1.example.com/",
            "http://user@127.0.0.1/",
            "ftp://127.0.0.1/",
            "http://10.0.0.1/",
        ] {
            let error = Registry::local(url).unwrap_err();
            assert!(error.contains("is not on this computer"), "{error}");
        }
        assert_eq!(
            Registry::local("http://127.0.0.1:9").unwrap().url(),
            "http://127.0.0.1:9/"
        );
    }

    #[test]
    fn a_tarball_must_come_from_the_registry_itself() {
        let npmjs = Registry::npmjs();
        assert_eq!(
            npmjs.metadata_url("@pane-samples/greeter"),
            "https://registry.npmjs.org/@pane-samples%2fgreeter"
        );
        assert!(
            npmjs
                .refusal("https://registry.npmjs.org/greeter/-/greeter-1.0.0.tgz")
                .is_none()
        );
        for url in [
            "http://registry.npmjs.org/greeter/-/greeter-1.0.0.tgz",
            "https://example.com/greeter-1.0.0.tgz",
            "https://registry.npmjs.org.example.com/x.tgz",
            "file:///etc/passwd",
        ] {
            assert!(npmjs.refusal(url).is_some(), "{url}");
        }
    }

    #[test]
    fn each_download_has_a_folder_of_its_own_removed_when_it_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let downloads = dir.path().join("downloads");
        let tgz = tarball(&[("package/pane.json", Regular, b"{}")]);
        // The same tarball twice, as two previews of one package would.
        let first = Download::create(&downloads, |folder| unpack(&tgz, folder)).unwrap();
        let second = Download::create(&downloads, |folder| unpack(&tgz, folder)).unwrap();
        assert_ne!(first.folder(), second.folder());
        let kept = second.folder().to_path_buf();
        drop(first);
        assert_eq!(fs::read(kept.join("pane.json")).unwrap(), b"{}");
        drop(second);
        assert_eq!(fs::read_dir(&downloads).unwrap().count(), 0);
        // One refused leaves nothing either.
        let refused = tarball(&[("package/../x", Regular, b"")]);
        assert!(Download::create(&downloads, |folder| unpack(&refused, folder)).is_err());
        assert_eq!(fs::read_dir(&downloads).unwrap().count(), 0);
    }

    #[test]
    fn only_downloads_begun_long_ago_are_taken_as_abandoned() {
        let dir = tempfile::tempdir().unwrap();
        let now = std::time::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let day = crate::downloads::ABANDONED_AFTER.as_secs();
        for name in [
            format!("{}-1-0", 1_000_000 - day - 1),
            format!(".{}-1-1", 1_000_000 - day - 1),
            "not-a-download".to_owned(),
            format!("{}-2-0", 1_000_000 - day),
            format!(".{}-2-1", 1_000_000 - 5),
        ] {
            fs::create_dir(dir.path().join(name)).unwrap();
        }
        fs::write(dir.path().join("stray"), b"").unwrap();

        crate::downloads::remove_abandoned(dir.path(), now);

        let mut left: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [".999995-2-1".to_owned(), format!("{}-2-0", 1_000_000 - day)]
        );
    }

    #[test]
    fn a_package_unpacks_without_its_top_folder() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out");
        let tgz = tarball(&[
            ("package/", Directory, b""),
            ("package/pane.json", Regular, b"{}"),
            ("package/dist/c.wasm", Regular, b"wasm"),
        ]);

        unpack(&tgz, &dest).unwrap();

        assert_eq!(fs::read(dest.join("pane.json")).unwrap(), b"{}");
        assert_eq!(fs::read(dest.join("dist/c.wasm")).unwrap(), b"wasm");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dest.join("dist/c.wasm"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0, "{mode:o}");
        }
    }

    /// Unpacks `entries` into a fresh folder and returns why it was refused.
    fn refused(entries: &[(&str, tar::EntryType, &[u8])]) -> String {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out");
        let error = unpack(&tarball(entries), &dest).unwrap_err();
        // Nothing was written outside it.
        let outside: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(outside, ["out"], "{error}");
        error
    }

    #[test]
    fn paths_leaving_the_package_are_refused() {
        for path in [
            "package/../../escaped",
            "../escaped",
            "/etc/escaped",
            "package/a/../../escaped",
            "package/a\\..\\..\\escaped",
            "package/C:escaped",
            "package//x",
            "package/./x",
        ] {
            let error = refused(&[(path, Regular, b"x")]);
            assert!(
                error.contains("Pane unpacks only paths inside the package"),
                "{path}: {error}"
            );
        }
    }

    #[test]
    fn names_a_system_reads_differently_are_refused() {
        for path in [
            "package/con",
            "package/NUL.txt",
            "package/com1.wasm",
            "package/x.",
            "package/x ",
        ] {
            let error = refused(&[(path, Regular, b"x")]);
            assert!(error.contains("its tarball contains"), "{path}: {error}");
        }
        // Not device names.
        let dir = tempfile::tempdir().unwrap();
        unpack(
            &tarball(&[
                ("package/console.js", Regular, b""),
                ("package/com10", Regular, b""),
            ]),
            &dir.path().join("out"),
        )
        .unwrap();
    }

    #[test]
    fn links_devices_and_pipes_are_refused() {
        let cases = [
            (tar::EntryType::Symlink, "a symbolic link"),
            (tar::EntryType::Link, "a hard link"),
            (tar::EntryType::Char, "a device"),
            (tar::EntryType::Block, "a device"),
            (tar::EntryType::Fifo, "a named pipe"),
        ];
        for (kind, what) in cases {
            let error = refused(&[
                ("package/pane.json", Regular, b"{}"),
                ("package/link", kind, b""),
            ]);
            assert_eq!(
                error,
                format!(
                    "its tarball contains {what}, `package/link`; Pane unpacks only files and \
                     folders"
                )
            );
        }
    }

    #[test]
    fn a_file_twice_is_refused() {
        let error = refused(&[("package/a", Regular, b"1"), ("package/a", Regular, b"2")]);
        assert_eq!(error, "its tarball contains `package/a` twice");
    }

    #[test]
    fn a_tarball_unpacking_to_too_much_is_refused() {
        // The sizes declared add up beyond the limit; nothing that large is
        // read.
        let dir = tempfile::tempdir().unwrap();
        let tgz = tarball_declaring(&[
            ("package/a", Regular, b"small", 5),
            ("package/b", Regular, b"", MAX_UNPACKED),
        ]);
        let error = unpack(&tgz, &dir.path().join("out")).unwrap_err();
        assert!(
            error.contains("it unpacks to more than the 256 MiB"),
            "{error}"
        );
    }

    #[test]
    fn a_tarball_with_too_many_entries_is_refused() {
        let names: Vec<String> = (0..=MAX_ENTRIES).map(|i| format!("package/{i}")).collect();
        let entries: Vec<(&str, tar::EntryType, &[u8])> = names
            .iter()
            .map(|n| (n.as_str(), Regular, &b""[..]))
            .collect();
        let error = refused(&entries);
        assert!(error.contains("more than 10000 entries"), "{error}");
    }

    /// The data of a PAX extension header of `records` (`key`, `value`).
    fn pax(records: &[(&str, &str)]) -> Vec<u8> {
        let mut data = Vec::new();
        for (key, value) in records {
            let rest = format!(" {key}={value}\n");
            // The length counts its own digits.
            let mut length = rest.len() + 1;
            while (rest.len() + length.to_string().len()) != length {
                length += 1;
            }
            data.extend_from_slice(format!("{length}{rest}").as_bytes());
        }
        data
    }

    use tar::EntryType::{GNULongLink, GNULongName, XGlobalHeader, XHeader};

    #[test]
    fn a_size_only_a_pax_header_gives_is_never_written_empty() {
        // The header says 0 bytes, its PAX header 4: a reader taking the
        // header's size writes an empty file and reads the data as the next
        // header.
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out");
        let pax = pax(&[("size", "4")]);
        let tgz = tarball_declaring(&[
            ("package/", Directory, b"", 0),
            ("././@PaxHeader", XHeader, &pax, pax.len() as u64),
            ("package/pane.json", Regular, b"data", 0),
        ]);

        match unpack(&tgz, &dest) {
            Ok(()) => assert_eq!(fs::read(dest.join("pane.json")).unwrap(), b"data"),
            Err(error) => {
                assert!(!dest.join("pane.json").exists(), "{error}");
                assert!(error.contains("two sizes"), "{error}");
            }
        }
    }

    #[test]
    fn a_long_name_from_a_gnu_or_pax_header_is_used_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        let long = format!("package/{}/pane.json", "d".repeat(150));
        let named = format!("package/{}.wasm", "p".repeat(120));
        let pax = pax(&[("path", &named)]);
        let tgz = tarball_declaring(&[
            (
                "././@LongLink",
                GNULongName,
                long.as_bytes(),
                long.len() as u64,
            ),
            ("package/short", Regular, b"{}", 2),
            ("././@PaxHeader", XHeader, &pax, pax.len() as u64),
            ("package/other", Regular, b"wasm", 4),
        ]);
        unpack(&tgz, &dir.path().join("out")).unwrap();
        let out = dir.path().join("out");
        assert_eq!(
            fs::read(out.join(&long["package/".len()..])).unwrap(),
            b"{}"
        );
        assert_eq!(
            fs::read(out.join(&named["package/".len()..])).unwrap(),
            b"wasm"
        );
        assert!(!out.join("short").exists() && !out.join("other").exists());

        // The long name is checked as any other.
        for escaping in ["package/../../escaped", "/etc/escaped"] {
            let pax = self::pax(&[("path", escaping)]);
            let error = refused_declaring(&[
                ("././@PaxHeader", XHeader, &pax, pax.len() as u64),
                ("package/x", Regular, b"x", 1),
            ]);
            assert!(error.contains("Pane unpacks only paths inside"), "{error}");
            let error = refused_declaring(&[
                (
                    "././@LongLink",
                    GNULongName,
                    escaping.as_bytes(),
                    escaping.len() as u64,
                ),
                ("package/x", Regular, b"x", 1),
            ]);
            assert!(error.contains("Pane unpacks only paths inside"), "{error}");
        }
    }

    #[test]
    fn an_extension_header_larger_than_64_kib_is_refused() {
        let big = vec![b'a'; (MAX_EXTENSION + 1) as usize];
        for kind in [GNULongName, XHeader, XGlobalHeader] {
            let error = refused_declaring(&[
                ("././@LongLink", kind, &big, big.len() as u64),
                ("package/x", Regular, b"x", 1),
            ]);
            assert!(
                error.contains("an extension header of 65537 bytes; Pane reads at most 64 KiB"),
                "{kind:?}: {error}"
            );
        }
        // Declared that large, it is refused before anything is read.
        let error = refused_declaring(&[("././@LongLink", GNULongName, b"", u64::from(u32::MAX))]);
        assert!(error.contains("an extension header of"), "{error}");
    }

    #[test]
    fn extension_headers_count_toward_the_entry_limit() {
        let pax = pax(&[("mtime", "1")]);
        let names: Vec<String> = (0..=MAX_ENTRIES / 2)
            .map(|i| format!("package/{i}"))
            .collect();
        let mut entries: Vec<(&str, tar::EntryType, &[u8], u64)> = Vec::new();
        for name in &names {
            entries.push(("././@PaxHeader", XHeader, &pax, pax.len() as u64));
            entries.push((name, Regular, b"", 0));
        }
        let error = refused_declaring(&entries);
        assert!(error.contains("more than 10000 entries"), "{error}");
    }

    #[test]
    fn a_long_link_a_global_path_or_a_dangling_header_is_refused() {
        let error = refused_declaring(&[
            ("././@LongLink", GNULongLink, b"target", 6),
            ("package/x", Regular, b"x", 1),
        ]);
        assert!(error.contains("a long link name"), "{error}");
        let global = pax(&[("path", "package/elsewhere")]);
        let error = refused_declaring(&[
            (
                "pax_global_header",
                XGlobalHeader,
                &global,
                global.len() as u64,
            ),
            ("package/x", Regular, b"x", 1),
        ]);
        assert!(error.contains("a global header"), "{error}");
        let pax = pax(&[("path", "package/x")]);
        let error = refused_declaring(&[("././@PaxHeader", XHeader, &pax, pax.len() as u64)]);
        assert!(error.contains("describes no entry"), "{error}");
        // A global header that changes nothing (git's commit id) is fine.
        let comment = self::pax(&[("comment", "0123abcd")]);
        let dir = tempfile::tempdir().unwrap();
        unpack(
            &tarball_declaring(&[
                (
                    "pax_global_header",
                    XGlobalHeader,
                    &comment,
                    comment.len() as u64,
                ),
                ("package/x", Regular, b"x", 1),
            ]),
            &dir.path().join("out"),
        )
        .unwrap();
    }

    /// Like [`refused`], for entries declaring their sizes.
    fn refused_declaring(entries: &[(&str, tar::EntryType, &[u8], u64)]) -> String {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out");
        let error = unpack(&tarball_declaring(entries), &dest).unwrap_err();
        let outside: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(outside, ["out"], "{error}");
        error
    }

    #[test]
    fn windows_names_and_characters_are_refused_on_every_system() {
        for path in [
            "package/CONIN$",
            "package/conout$.txt",
            "package/COM\u{b9}",
            "package/com\u{b2}.js",
            "package/LPT\u{b3}",
            "package/lpt0.log",
            "package/nul .txt",
            "package/a<b",
            "package/a>b",
            "package/a\"b",
            "package/a|b",
            "package/a?b",
            "package/a*b",
            "package/a\u{7f}b",
            "package/a\u{85}b",
            "package/dir./x",
            "package/dir /x",
        ] {
            let error = refused(&[(path, Regular, b"x")]);
            assert!(error.contains("its tarball contains"), "{path}: {error}");
        }
        let dir = tempfile::tempdir().unwrap();
        unpack(
            &tarball(&[
                ("package/coninx", Regular, b""),
                ("package/com\u{b9}0", Regular, b""),
                ("package/lpt", Regular, b""),
                ("package/caf\u{e9}.txt", Regular, b""),
            ]),
            &dir.path().join("out"),
        )
        .unwrap();
    }

    #[test]
    fn a_damaged_tarball_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut tgz = tarball(&[("package/a", Regular, b"contents")]);
        tgz.truncate(tgz.len() / 2);
        let error = unpack(&tgz, &dir.path().join("out")).unwrap_err();
        assert!(error.starts_with("its tarball"), "{error}");
        let error = unpack(b"not gzip", &dir.path().join("other")).unwrap_err();
        assert!(error.starts_with("its tarball cannot be read"), "{error}");
    }
}
