//! Pane's default extensions and the artifacts they are acquired from.
//!
//! A default extension ([glossary](../CONTEXT.md): an extension Pane offers
//! by default, disableable individually) is not carried by the installer:
//! Pane acquires it over the network at first setup, from Pane's own
//! artifact source. The source holds an index document,
//! [`pane-defaults.json`](INDEX_FILE), which names each default
//! extension's payload: the tarball's file name, its version, its size and
//! its sha512 integrity. Pane reads the index, downloads the payload with
//! progress and retries, checks it against that integrity, and unpacks it
//! with the same checks an npm package's tarball gets
//! ([`crate::npm::unpack`]) — the extension runtime itself is part of
//! Pane's process (Wasmtime), so no runtime payload is ever acquired.
//!
//! A payload that passes its checks is installed as any package from a
//! folder is, into a managed copy, with the identity of its default
//! extension ([`PackageIdentity::default_extension`]), so the normal
//! mechanisms (disable, uninstall, extension data) apply to it unchanged.
//!
//! A downloaded payload is kept in a cache of its own under Pane's
//! `extensions/acquired/` folder, named by its version and the first
//! bytes of the integrity its index gives: acquiring again finds it there
//! and reuses it — but only if its bytes still match that integrity, so
//! an incomplete or damaged cache entry is downloaded again rather than
//! trusted. Acquiring another version of the same extension removes the
//! older one's entry.
//!
//! The artifact source is `https://downloads.pane.sh/`. That location is
//! not deployed yet: it is where Pane's default-extension payloads will
//! be published, so a Pane installed from today's package cannot complete
//! its first setup on the real internet, and explains so with a retry.
//! Tests and development builds can name a source on this computer
//! instead ([`ArtifactSource::local`], `PANE_ARTIFACTS`), which must be a
//! literal loopback address, so no check ever reaches the network; a
//! release build has no way to replace the published source.

use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

use serde::Deserialize;

use crate::downloads::{Download, check_part};
use crate::http::{Answer, GetError, OwnLimits};
use crate::npm;

/// Where Pane's default extensions' payloads are published: the index and
/// tarballs a Pane installed from its package downloads at first setup.
/// Not deployed yet (see the [module](self) documentation); tests and
/// development builds use a source on this computer instead.
pub const PUBLISHED: &str = "https://downloads.pane.sh/";

/// The largest index document Pane reads from an artifact source: it names
/// a handful of default extensions, so a larger one is a broken source.
pub const MAX_INDEX: u64 = 1 << 20;

/// The name of the index document at an artifact source.
const INDEX_FILE: &str = "pane-defaults.json";

/// How many times Pane tries to acquire one default extension before it
/// explains the failure and offers the row that tries again.
pub(crate) const ATTEMPTS: usize = 3;

/// How long Pane waits before trying an interrupted acquisition again.
pub(crate) const RETRY_AFTER: [Duration; 2] = [Duration::from_millis(500), Duration::from_secs(1)];

/// Where Pane's default extensions' payloads are acquired from. Release
/// builds use only [`ArtifactSource::published`]; tests and development
/// builds can use a source on this computer ([`ArtifactSource::local`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactSource {
    /// Its address, ending with `/`.
    base: String,
    /// Whether it is on this computer, reached without a proxy and over
    /// plain HTTP if its address says so.
    loopback: bool,
}

impl Default for ArtifactSource {
    fn default() -> ArtifactSource {
        ArtifactSource::published()
    }
}

impl ArtifactSource {
    /// Pane's published downloads, over HTTPS.
    pub fn published() -> ArtifactSource {
        ArtifactSource {
            base: PUBLISHED.to_owned(),
            loopback: false,
        }
    }

    /// A source on this computer, for tests and development builds only:
    /// `url` must be `http://` or `https://` on a loopback address written
    /// as one (`127.0.0.1`, any `127.x.y.z`, or `[::1]`), with an optional
    /// port and path. Any other address is refused, `localhost` included
    /// (a name could resolve elsewhere), so that nothing but Pane's
    /// published downloads is ever reached over the network. Release
    /// builds have no way to replace them.
    #[cfg(any(test, debug_assertions))]
    pub fn local(url: &str) -> Result<ArtifactSource, String> {
        let refused = || {
            format!(
                "the artifact source `{url}` is not on this computer: Pane acquires its default \
                 extensions from {PUBLISHED}, and only a source on a loopback address such as \
                 127.0.0.1 or [::1] can replace it, for tests and development"
            )
        };
        let base = crate::http::loopback_base(url, refused)?;
        Ok(ArtifactSource {
            base,
            loopback: true,
        })
    }

    /// The artifact source named by `PANE_ARTIFACTS`, in development builds
    /// only (see [`ArtifactSource::local`]); `None` when it is not set, in
    /// which case this development build installs no default extensions.
    #[cfg(any(test, debug_assertions))]
    pub fn from_dev_env() -> Option<Result<ArtifactSource, String>> {
        let url = std::env::var("PANE_ARTIFACTS").ok()?;
        (!url.is_empty()).then(|| ArtifactSource::local(&url))
    }

    /// Its address, ending with `/`.
    pub fn url(&self) -> &str {
        &self.base
    }

    /// The address of the index document.
    fn index_url(&self) -> String {
        format!("{}{INDEX_FILE}", self.base)
    }

    /// The address of the payload file `file` of an index entry: `file` is
    /// one plain name (checked when the index was read), so the address
    /// stays on this source.
    pub(crate) fn payload_url(&self, file: &str) -> String {
        format!("{}{file}", self.base)
    }

    /// Asks the source for `url`, with `headers`, a body of at most `most`
    /// bytes, telling `progress` of the bytes of the body so far, through
    /// the connections Pane's own requests use
    /// ([`crate::http::get_blocking_progressing`]): only over HTTPS unless
    /// the source is on this computer.
    pub(crate) fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        most: u64,
        progress: &(dyn Fn(u64) + Send + Sync),
    ) -> Result<Answer, GetError> {
        if !self.loopback && !url.starts_with("https://") {
            return Err(GetError::Failed(format!("`{url}` is not an HTTPS address")));
        }
        crate::http::get_blocking_progressing(
            url,
            headers,
            most,
            OwnLimits {
                connect: Duration::from_secs(30),
                between_bytes: Duration::from_secs(60),
                deadline: Duration::from_secs(300),
            },
            progress,
        )
    }
}

/// One default extension this build of Pane offers to acquire: its `id`
/// names it in the artifact source's index, and `title` is its name in
/// Pane's messages about acquiring it. A package's own manifest is what
/// its commands are finally listed by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultExtension {
    pub id: String,
    pub title: String,
}

/// A default extension's payload Pane acquired: the unpacked package, and
/// where it came from.
pub(crate) struct Fetched {
    /// The unpacked payload: the tarball's top folder, in Pane's downloads
    /// folder, removed once the package read from it is dropped.
    pub download: Download,
    pub origin: DefaultOrigin,
}

/// Where a default extension's payload was acquired from.
#[derive(Clone, Debug)]
pub(crate) struct DefaultOrigin {
    /// The default extension's id (its package identity).
    pub id: String,
    /// The version its index entry named.
    pub version: String,
    /// The sha512 integrity its index entry gave, which the payload
    /// matched.
    pub integrity: String,
}

impl DefaultOrigin {
    /// What the installed record keeps of it: the version installed (the
    /// identity is the record's source; the integrity identified the
    /// download).
    pub(crate) fn version(&self) -> &str {
        &self.version
    }
}

/// Why acquiring one default extension failed, after Pane's retries.
#[derive(Debug)]
pub(crate) struct Failed(String);

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Acquires the payload of the default extension `id` from `source`, into
/// a folder of its own in `downloads`, where
/// `acquired` is the cache folder (`extensions/acquired`), telling
/// `progress` of the payload's bytes as they arrive (of the size its index
/// entry gives). A payload whose bytes still match its index entry's
/// integrity is taken from the cache without downloading it again; an
/// incomplete or damaged entry is downloaded again and replaces it. Blocks
/// on the network and the file system, and tries an interrupted
/// acquisition again up to [`ATTEMPTS`] times before explaining the
/// failure.
pub(crate) fn fetch(
    source: &ArtifactSource,
    id: &str,
    downloads: &Path,
    acquired: &Path,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<Fetched, Failed> {
    // The failure's own message names the source; the launcher frames
    // whose set-up failed.
    with_retries(|| acquire(source, id, downloads, acquired, progress)).map_err(Failed)
}

/// Why one attempt at acquiring a payload failed, and whether trying again
/// can help (a connection that failed, not a payload that was refused).
pub(crate) struct Failure {
    pub(crate) why: String,
    pub(crate) retry: bool,
}

/// Tries `once` up to [`ATTEMPTS`] times: a failure that says to retry
/// sleeps [`RETRY_AFTER`] first, and a failure that stays is explained
/// with how many attempts were made. The three retry loops — a default
/// extension's payload, an update check, an update's download — share it.
pub(crate) fn with_retries<T>(mut once: impl FnMut() -> Result<T, Failure>) -> Result<T, String> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match once() {
            Ok(answer) => return Ok(answer),
            Err(failure) if failure.retry && attempt < ATTEMPTS => {
                thread::sleep(RETRY_AFTER[attempt.min(RETRY_AFTER.len()) - 1]);
            }
            Err(failure) => {
                let tried = if attempt > 1 {
                    format!(" (Pane tried {ATTEMPTS} times)")
                } else {
                    String::new()
                };
                return Err(format!("{}{tried}", failure.why));
            }
        }
    }
}

pub(crate) fn failed(why: impl Into<String>) -> Failure {
    Failure {
        why: why.into(),
        retry: false,
    }
}

pub(crate) fn interrupted(why: impl Into<String>) -> Failure {
    Failure {
        why: why.into(),
        retry: true,
    }
}

/// One attempt: read the index, find the entry, take the payload from the
/// cache if it matches, else download it, then unpack it.
fn acquire(
    source: &ArtifactSource,
    id: &str,
    downloads: &Path,
    acquired: &Path,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<Fetched, Failure> {
    let index = read_index(source)?;
    let entry = index.find(id).ok_or_else(|| {
        failed(format!(
            "its index names no default extension `{id}`; it names {}",
            index.described()
        ))
    })?;
    check_part(&entry.file).map_err(|why| {
        failed(format!(
            "its index names the payload file `{}`, {}",
            entry.file, why
        ))
    })?;
    let cache = acquired.join(id);
    let name = cache_name(&entry.version, &entry.integrity);
    let payload = cache.join(&name);
    let mut bytes = match fs::read(&payload) {
        Ok(cached) => match npm::check_integrity(&cached, &entry.integrity) {
            // Only a payload that still matches its integrity is reused.
            Ok(()) => Some(cached),
            Err(_) => None,
        },
        Err(_) => None,
    };
    if bytes.is_none() {
        bytes = Some(download(source, entry, &cache, &name, progress)?);
    }
    let bytes = bytes.expect("downloaded or cached");
    let origin = DefaultOrigin {
        id: id.to_owned(),
        version: entry.version.clone(),
        integrity: entry.integrity.clone(),
    };
    let download =
        Download::create(downloads, |folder| npm::unpack(&bytes, folder)).map_err(|why| {
            failed(format!(
                "its payload cannot be unpacked safely: {why}; Pane installs only the files and \
                 folders inside the package"
            ))
        })?;
    keep_payload(&cache, &name);
    Ok(Fetched { download, origin })
}

/// The index document an artifact source serves, as Pane read and checked
/// it.
pub(crate) struct Index {
    pub(crate) entries: Vec<Entry>,
    /// What the index says of Pane's own application package, whose
    /// updates Pane offers the user ([`crate::application_update`]): read
    /// but neither parsed nor validated here, so that an application
    /// entry this Pane cannot take never keeps it from acquiring its
    /// default extensions.
    pub(crate) application: Option<serde_json::Value>,
}

/// One entry of the index: what identifies a default extension's payload.
#[derive(Clone)]
pub(crate) struct Entry {
    id: String,
    version: String,
    /// The payload's file name, one plain name.
    file: String,
    /// `sha512-<base64>`, which the payload's bytes must match.
    integrity: String,
    /// The payload's size in bytes, as the index gives it, for progress.
    size: u64,
}

impl Index {
    /// The entry of the default extension `id`.
    fn find(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// How the index names the default extensions it describes, for the
    /// error that explains a missing one.
    fn described(&self) -> String {
        let names: Vec<String> = self
            .entries
            .iter()
            .map(|entry| format!("`{}`", entry.id))
            .collect();
        match names.len() {
            0 => "no default extension".to_owned(),
            _ => names.join(", "),
        }
    }
}

/// Reads and checks the index document of `source`.
pub(crate) fn read_index(source: &ArtifactSource) -> Result<Index, Failure> {
    let index_url = source.index_url();
    let read = source
        .get(&index_url, &[], MAX_INDEX, &|_| {})
        .map_err(|error| match error {
            GetError::TooLarge => failed(format!(
                "Pane's downloads at {} gave an index larger than the {} KiB Pane reads",
                source.url(),
                MAX_INDEX >> 10
            )),
            GetError::Failed(why) => interrupted(format!(
                "Pane's downloads at {} could not be reached: {why}",
                source.url()
            )),
        })?;
    if read.status != 200 {
        return Err(answer(
            read.status,
            format!(
                "Pane's downloads at {} answered {} for its index {}",
                source.url(),
                read.status,
                index_url
            ),
        ));
    }
    let index: IndexJson = serde_json::from_slice(&read.body).map_err(|error| {
        failed(format!(
            "Pane's downloads at {} gave an index that cannot be read: {error}",
            source.url()
        ))
    })?;
    if index.format_version != INDEX_FORMAT {
        return Err(failed(format!(
            "Pane's downloads at {} gave an index with format version {}, this Pane reads {}",
            source.url(),
            index.format_version,
            INDEX_FORMAT
        )));
    }
    let mut entries = Vec::new();
    for entry in index.defaults {
        if !npm::has_sha512(&entry.integrity) {
            return Err(failed(format!(
                "Pane's downloads at {} describe `{}` without a sha512 integrity, which Pane \
                 needs to check its download",
                source.url(),
                entry.id
            )));
        }
        if entries.iter().any(|other: &Entry| other.id == entry.id) {
            return Err(failed(format!(
                "Pane's downloads at {} describe the default extension `{}` twice",
                source.url(),
                entry.id
            )));
        }
        entries.push(Entry {
            id: entry.id,
            version: entry.version,
            file: entry.file,
            integrity: entry.integrity,
            size: entry.size,
        });
    }
    Ok(Index {
        entries,
        application: index.application,
    })
}

/// Downloads the payload `entry` describes into the cache folder `cache`
/// under the name `name`, checking it against the entry's integrity, and
/// returns its bytes. A previous incomplete download is replaced.
fn download(
    source: &ArtifactSource,
    entry: &Entry,
    cache: &Path,
    name: &str,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<Vec<u8>, Failure> {
    let url = source.payload_url(&entry.file);
    let (file, size) = (entry.file.clone(), entry.size);
    let told = move |bytes: u64| progress(bytes, size);
    let read = source
        .get(&url, &[], npm::MAX_TARBALL, &told)
        .map_err(|error| match error {
            GetError::TooLarge => failed(format!(
                "the payload `{}` is larger than the {} MiB Pane downloads",
                file,
                npm::MAX_TARBALL >> 20
            )),
            GetError::Failed(why) => interrupted(format!(
                "Pane's downloads at {} could not be reached: {why}",
                source.url()
            )),
        })?;
    if read.status != 200 {
        let why = match read.status {
            404 => format!("the payload `{file}` its index names is not there"),
            status => format!("it answered {status} for the payload `{file}`"),
        };
        return Err(answer(read.status, why));
    }
    let bytes = read.body;
    npm::check_integrity(&bytes, &entry.integrity)
        .map_err(|why| failed(format!("the downloaded payload `{file}` {why}")))?;
    write_payload(cache, name, &bytes)
        .map_err(|error| failed(format!("its payload cannot be kept: {error}")))?;
    Ok(bytes)
}

/// Why a status other than 200 was answered for the index or a payload:
/// trying again may fix a server that failed, never a payload that is
/// simply not there.
pub(crate) fn answer(status: u16, why: String) -> Failure {
    if matches!(status, 403 | 500 | 502 | 503 | 504) {
        interrupted(why)
    } else {
        failed(why)
    }
}

/// Writes the payload `bytes` to `cache/name`, through a `.part` file, so
/// that a Pane stopped mid-download leaves no half-written payload under
/// the name a later one looks for.
fn write_payload(cache: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(cache).map_err(|error| error.to_string())?;
    let part = cache.join(format!("{name}.part"));
    fs::write(&part, bytes).map_err(|error| error.to_string())?;
    fs::rename(&part, cache.join(name)).map_err(|error| error.to_string())?;
    Ok(())
}

/// Keeps only `name` as the payload in `cache`: a version Pane acquired
/// before, or a damaged entry, goes.
fn keep_payload(cache: &Path, name: &str) {
    let Ok(entries) = fs::read_dir(cache) else {
        return;
    };
    for entry in entries.flatten() {
        let file = entry.file_name();
        let kept = file.to_str() == Some(name);
        if !kept && file.to_string_lossy().ends_with(".tgz") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// The name of a payload in the cache: `<version>-<integrity's first 16
/// hex digits>.tgz`, so that another version, or the same version with
/// another integrity, is another name.
fn cache_name(version: &str, integrity: &str) -> String {
    let digest = npm::sha512_digest(integrity);
    format!(
        "{version}-{}.tgz",
        hex(digest.as_ref().map(|digest| &digest[..8]).unwrap_or(&[]))
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Removes what a Pane stopped mid-download left in the cache folders
/// under `acquired`: `.part` files older than
/// [`crate::downloads::ABANDONED_AFTER`]. A younger one may belong to a
/// Pane still running on the same data folder. Best effort.
pub(crate) fn remove_abandoned_parts(acquired: &Path, now: std::time::SystemTime) {
    let Ok(folders) = fs::read_dir(acquired) else {
        return;
    };
    for folder in folders.flatten() {
        let Ok(files) = fs::read_dir(folder.path()) else {
            continue;
        };
        for file in files.flatten() {
            let name = file.file_name();
            if !name.to_string_lossy().ends_with(".part") {
                continue;
            }
            let old = file
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| {
                    now.duration_since(modified)
                        .is_ok_and(|age| age > crate::downloads::ABANDONED_AFTER)
                });
            if old {
                let _ = fs::remove_file(file.path());
            }
        }
    }
}

/// The index document as it is served: `formatVersion`, `defaults` and,
/// optionally, the `application` entry naming Pane's own package.
#[derive(Deserialize)]
struct IndexJson {
    #[serde(rename = "formatVersion")]
    format_version: u64,
    #[serde(default)]
    defaults: Vec<EntryJson>,
    /// Pane's own application package (see [`crate::application_update`]),
    /// read as it is written: parsed where it is used, so a broken entry
    /// is explained there rather than making the whole index unreadable.
    /// Optional, because a source that serves none (one built before the
    /// application entry existed) still serves the default extensions.
    #[serde(default)]
    application: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct EntryJson {
    id: String,
    version: String,
    file: String,
    integrity: String,
    #[serde(default)]
    size: u64,
}

/// The index format this Pane reads.
const INDEX_FORMAT: u64 = 1;

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha512};

    #[test]
    fn a_source_on_this_computer_is_a_loopback_address_only() {
        assert_eq!(
            ArtifactSource::local("http://127.0.0.1:43127/").unwrap(),
            ArtifactSource {
                base: "http://127.0.0.1:43127/".into(),
                loopback: true
            }
        );
        for url in [
            "http://127.0.0.1:43127",
            "https://127.9.9.9/",
            "http://[::1]:8/",
        ] {
            let source = ArtifactSource::local(url).unwrap();
            assert!(source.url().ends_with('/'), "{}", source.url());
            assert!(source.loopback);
        }
        for url in [
            "http://localhost:43127/",
            "http://example.com/",
            "https://registry.npmjs.org/",
            "ftp://127.0.0.1/",
            "http://192.168.1.4/",
            " Pane",
            "http://127.0.0.1:pane/",
        ] {
            assert!(ArtifactSource::local(url).is_err(), "{url:?}");
        }
    }

    #[test]
    fn a_payload_is_named_by_its_version_and_integrity() {
        // sha512 of the empty input, in npm's integrity spelling.
        let empty = format!("sha512-{}", npm::base64(&Sha512::digest(b"")));
        assert_eq!(
            cache_name("0.1.0", &empty),
            format!("0.1.0-{}.tgz", hex(&Sha512::digest(b"")[..8]))
        );
        // Another integrity, or version, is another name.
        assert_ne!(cache_name("0.1.0", &empty), cache_name("0.1.1", &empty));
        let other = format!("sha512-{}", npm::base64(&Sha512::digest(b"other")));
        assert_ne!(cache_name("0.1.0", &empty), cache_name("0.1.0", &other));
        // An integrity without a digest names nothing: the name says the
        // version alone.
        assert_eq!(cache_name("0.1.0", "sha1-abc"), "0.1.0-.tgz");
    }

    #[test]
    fn a_source_names_its_index_and_payloads() {
        let source = ArtifactSource::published();
        assert_eq!(source.url(), PUBLISHED);
        assert_eq!(source.index_url(), format!("{PUBLISHED}{INDEX_FILE}"));
        assert_eq!(
            source.payload_url("calculator-0.1.0.tgz"),
            format!("{PUBLISHED}calculator-0.1.0.tgz")
        );
    }

    #[test]
    fn a_stopped_downloads_part_file_is_abandoned() {
        let cache = tempfile::tempdir().unwrap();
        let young = cache.path().join("calculator");
        let old = cache.path().join("helper-sample");
        for folder in [&young, &old] {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::write(folder.join("0.1.0-x.tgz.part"), b"partial").unwrap();
        }
        std::fs::write(young.join("0.1.0-x.tgz"), b"whole").unwrap();
        let now = std::time::SystemTime::now();
        let set = |folder: &Path, age: std::time::Duration| {
            let file = std::fs::File::options()
                .write(true)
                .open(folder.join("0.1.0-x.tgz.part"))
                .unwrap();
            file.set_modified(now - age).unwrap();
        };
        // A download a Pane stopped a day ago is abandoned; a younger one
        // may belong to a Pane still running on the same data folder.
        set(&young, std::time::Duration::from_secs(1));
        set(
            &old,
            crate::downloads::ABANDONED_AFTER + std::time::Duration::from_secs(1),
        );
        remove_abandoned_parts(cache.path(), now);
        assert!(young.join("0.1.0-x.tgz.part").exists());
        assert!(young.join("0.1.0-x.tgz").exists());
        assert!(!old.join("0.1.0-x.tgz.part").exists());
    }
}
