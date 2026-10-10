//! Pane's default extensions and the pins this release sets them up from.
//!
//! A default extension ([glossary](../CONTEXT.md): an extension Pane offers
//! by default, disableable individually) is not carried by the installer:
//! Pane acquires it over the network at first setup, fetching exactly the
//! commit of the release tag this Pane release was tested with, from the
//! extension's own public repository, with Pane's own Git client
//! ([`crate::git`], ADR 0021: a commit id pins the bytes, so no index,
//! integrity file or download host is involved). Each Pane release names
//! those pins, and a newer Pane release moves them forward (ADR 0045);
//! between releases a default extension updates from its repository's
//! newer release tags ([#269](https://github.com/pane-app/pane/issues/269)),
//! from the Git source its installed record keeps.
//!
//! The pins are committed to the build as a JSON array
//! ([`pane::default_extensions`] reads it): each entry names the default
//! extension's id, its title, its repository, its release tag and that
//! tag's commit, and, optionally, the only system the default is set up
//! on (`platform`, as `pane.json` names one: absent, every system — a
//! Windows-only default's pin names `windows`, and the gate
//! [`DefaultExtension::runs_here`] drops it on every other system
//! before any repository is fetched). Tests and development builds can
//! replace the pins with a file of their own naming the same
//! (`PANE_DEFAULTS`, whose
//! repositories must be reachable as a Git address is: HTTPS, or a
//! loopback address in these builds alone), so the tests and smokes can
//! serve the repositories on this computer and no check ever reaches a
//! real Git host; without it, the committed pins are used, in development
//! and release builds alike, and a release build has no way to replace
//! them.
//!
//! A fetched revision is installed as any package from a folder is, into a
//! managed copy, with the identity of its default extension
//! ([`PackageIdentity::default_extension`]), so the normal mechanisms
//! (disable, uninstall, extension data) apply to it unchanged, and with
//! its Git source recorded (repository, tag, commit, pinned) beside the
//! version its manifest declares — the record a later release's updater
//! reads. An install that acquired its defaults another way (an older
//! Pane, from the artifact source) keeps them: their identity and saved
//! data are unchanged, and they are not acquired again.
//!
//! What remains of Pane's own artifact source (`https://downloads.pane.sh/`)
//! serves the index of Pane's *application* updates alone
//! ([`crate::application_update`]); it no longer serves any default
//! extension, and a development build can name one on this computer for
//! those updates alone (`PANE_ARTIFACTS`).

use std::path::Path;
use std::thread;
use std::time::Duration;

use serde::Deserialize;

use crate::downloads::Download;
use crate::git::{GitRef, GitRevision};
use crate::http::{Answer, GetError, Origin};
use crate::platform::Platform;

/// Where Pane's own application updates are published: the index a Pane
/// installed from its package reads at start for a newer version of
/// itself. Not deployed yet (see the [module](self) documentation); tests
/// and development builds use a source on this computer instead.
pub const PUBLISHED: &str = "https://downloads.pane.sh/";

/// The largest index document Pane reads from an artifact source: it
/// names Pane's own application package, so a larger one is a broken
/// source.
pub const MAX_INDEX: u64 = 1 << 20;

/// The name of the index document at an artifact source.
const INDEX_FILE: &str = "pane-defaults.json";

/// The largest pins file Pane reads: it names a handful of default
/// extensions, so a larger one is a broken file.
pub(crate) const MAX_PINS: u64 = 1 << 20;

/// How many times Pane tries to acquire one default extension before it
/// explains the failure and offers the row that tries again.
pub(crate) const ATTEMPTS: usize = 3;

/// How long Pane waits before trying an interrupted acquisition again.
pub(crate) const RETRY_AFTER: [Duration; 2] = [Duration::from_millis(500), Duration::from_secs(1)];

/// Where Pane reads the index of its own application updates. Release
/// builds use only [`ArtifactSource::published`]; tests and development
/// builds can use a source on this computer ([`ArtifactSource::local`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactSource {
    /// Its address, and whether it is on this computer.
    origin: Origin,
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
            origin: Origin::public(PUBLISHED),
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
                "the artifact source `{url}` is not on this computer: Pane reads its own \
                 application updates from {PUBLISHED}, and only a source on a loopback address \
                 such as 127.0.0.1 or [::1] can replace it, for tests and development"
            )
        };
        let origin = Origin::local(url, refused)?;
        Ok(ArtifactSource { origin })
    }

    /// The artifact source named by `PANE_ARTIFACTS`, in development builds
    /// only (see [`ArtifactSource::local`]); `None` when it is not set, in
    /// which case this development build checks for no application update.
    #[cfg(any(test, debug_assertions))]
    pub fn from_dev_env() -> Option<Result<ArtifactSource, String>> {
        crate::http::dev_env("PANE_ARTIFACTS").map(|url| ArtifactSource::local(&url))
    }

    /// Its address, ending with `/`.
    pub fn url(&self) -> &str {
        self.origin.url()
    }

    /// The address of the index document.
    fn index_url(&self) -> String {
        format!("{}{INDEX_FILE}", self.url())
    }

    /// The address of the package file `file` of an index entry: `file` is
    /// one plain name (checked when the index was read), so the address
    /// stays on this source.
    pub(crate) fn payload_url(&self, file: &str) -> String {
        format!("{}{file}", self.url())
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
        self.origin.get_progressing(url, headers, most, progress)
    }
}

/// One default extension this build of Pane sets up at first setup: its
/// `id` names it everywhere (the identity of the package installed,
/// `default:<id>`), `title` is its name in Pane's messages about setting
/// it up, and the rest is its pin — the repository it is fetched from,
/// the release tag this Pane release was tested with, and that tag's
/// commit, which Pane fetches exactly ([`crate::git`]: a commit id pins
/// the bytes). A package's own manifest is what its commands are finally
/// listed by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultExtension {
    pub id: String,
    pub title: String,
    /// The repository the pinned revision is fetched from, as a Git
    /// address is written (`https://github.com/pane-app/calculator`).
    pub repository: String,
    /// The release tag the pin names, `v<semver>` (ADR 0044).
    pub tag: String,
    /// The full commit id of that tag, as this Pane release was tested
    /// with.
    pub commit: String,
    /// The only system this pin's default is set up on (`windows`), as
    /// the pins file names it; absent, every system. The field lives in
    /// the pins file because that file is read before any repository is
    /// fetched, so a default of another system is never fetched — the
    /// launcher's platform gate, [`DefaultExtension::runs_here`].
    pub platform: Option<Platform>,
}

impl DefaultExtension {
    /// The fetch this pin names: the repository, at the pinned commit (a
    /// commit id pins the bytes, so the tag is recorded but never asked
    /// for: a tag the repository moved does not move what this release
    /// installs).
    pub(crate) fn spec(&self) -> Result<crate::git::GitSpec, String> {
        let mut spec = crate::git::GitSpec::parse(&self.repository)?;
        spec.reference = Some(self.commit.clone());
        Ok(spec)
    }

    /// Whether this pin's platform is the system this runs on: a pin
    /// that names none is set up on every system, and one that names
    /// another system is never listed, never fetched. A pin that names a
    /// platform and a system [`Platform::current`] does not know counts
    /// as another system's.
    pub fn runs_here(&self) -> bool {
        self.platform
            .is_none_or(|platform| Platform::current() == Some(platform))
    }
}

/// The pins this build sets its default extensions up from, as the pins
/// file holds them: a JSON array of `{ "id", "title", "repository",
/// "tag", "commit", "platform" }` — the `platform` optional, naming the
/// only system the default is set up on (absent, every system). Each id
/// appears once, each repository is a Git address Pane fetches, each tag
/// is a `v…` release tag, each commit a full id and each platform one
/// Pane names; the text is at most [`MAX_PINS`] long. An empty array names
/// no default extension — a pins file a development build takes as its
/// own when it sets up none (the smokes' phases that install samples by
/// hand, where first setup must add nothing).
pub fn parse_pins(text: &str) -> Result<Vec<DefaultExtension>, String> {
    if text.len() as u64 > MAX_PINS {
        return Err(format!(
            "the pins file is larger than the {} KiB Pane reads",
            MAX_PINS >> 10
        ));
    }
    let pins: Vec<PinJson> = serde_json::from_str(text)
        .map_err(|error| format!("the pins file is not a list of pins: {error}"))?;
    pins.into_iter()
        .enumerate()
        .map(|(at, pin)| {
            let named = |field: &str| format!("pin {}: its {field}", at + 1);
            let id = pin.id.ok_or_else(|| named("id is missing"))?;
            if !is_extension_id(&id) {
                return Err(format!("{} is `{id}`, not an extension id", named("id")));
            }
            let title = pin.title.ok_or_else(|| named("title is missing"))?;
            if title.trim().is_empty() {
                return Err(format!("{} is empty", named("title")));
            }
            let repository = pin
                .repository
                .ok_or_else(|| named("repository is missing"))?;
            if let Err(why) = crate::git::GitSpec::parse(&repository) {
                return Err(format!(
                    "{} is not a Git repository address: {why}",
                    named("repository")
                ));
            }
            let tag = pin.tag.ok_or_else(|| named("tag is missing"))?;
            if !is_release_tag(&tag) {
                return Err(format!(
                    "{} is `{tag}`, not a release tag such as v1.0.0",
                    named("tag")
                ));
            }
            let commit = pin.commit.ok_or_else(|| named("commit is missing"))?;
            if !crate::git::is_commit_id(&commit) {
                return Err(format!(
                    "{} is `{commit}`, not a full commit id",
                    named("commit")
                ));
            }
            let platform = match pin.platform.as_deref() {
                None => None,
                Some(id) => Some(Platform::from_id(id).ok_or_else(|| {
                    format!(
                        "{} is `{id}`, not `windows`, `macos` or `linux`",
                        named("platform")
                    )
                })?),
            };
            Ok(DefaultExtension {
                id,
                title,
                repository,
                tag,
                commit,
                platform,
            })
        })
        .collect::<Result<Vec<_>, String>>()
        .and_then(|pins| {
            for (at, pin) in pins.iter().enumerate() {
                if pins[..at].iter().any(|other| other.id == pin.id) {
                    return Err(format!("the pins file names `{}` twice", pin.id));
                }
            }
            Ok(pins)
        })
}

/// The pins named by `PANE_DEFAULTS`, in development builds only: a file
/// of pins that replaces the committed ones, so a development build can
/// point its default extensions at repositories served on this computer
/// (the tests' and smokes' own). `None` when it is not set, in which case
/// the committed pins are used.
#[cfg(any(test, debug_assertions))]
pub fn pins_from_dev_env() -> Option<Result<Vec<DefaultExtension>, String>> {
    let read = |path: String| -> Result<Vec<DefaultExtension>, String> {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("the pins file {path} cannot be read: {error}"))?;
        parse_pins(&text)
    };
    crate::http::dev_env("PANE_DEFAULTS").map(read)
}

/// `true` for an id of an extension: lowercase letters, digits and
/// hyphens, one or more.
fn is_extension_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-')
        && !id.ends_with('-')
}

/// `true` for a release tag as ADR 0044 writes one: `v` followed by
/// dotted numbers, such as `v0.5.0`.
fn is_release_tag(tag: &str) -> bool {
    let Some(version) = tag.strip_prefix('v') else {
        return false;
    };
    !version.is_empty()
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// A default extension's pinned revision, fetched and written out.
pub(crate) struct Fetched {
    /// The revision's files, in Pane's downloads folder, removed once the
    /// package read from it is dropped.
    pub download: Download,
    pub origin: DefaultOrigin,
}

/// Where a default extension's pinned revision was fetched from.
#[derive(Clone, Debug)]
pub(crate) struct DefaultOrigin {
    /// The default extension's id (its package identity).
    pub id: String,
    /// The repository the pin named, as Pane fetched it.
    pub repository: crate::git::Repository,
    /// The revision installed: the pin's release tag, at the commit that
    /// was fetched (exactly the pinned one).
    pub revision: GitRevision,
    /// Files of the revision that are Git LFS pointers rather than their
    /// contents, which Pane does not fetch.
    pub lfs_pointers: Vec<String>,
}

/// A default extension's recorded source, as `installed.json` keeps it
/// beside the default identity: the repository the revision was fetched
/// from, the release tag and commit of the revision installed, and the
/// version its manifest declared — what the updater reads to check the
/// repository's newer release tags
/// ([#269](https://github.com/pane-app/pane/issues/269)). A record an
/// older Pane wrote from its own downloads keeps none of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledDefault {
    /// The repository, as its record names it: a Git address, fetched
    /// from where the revision was fetched before.
    pub repository: String,
    /// The revision installed: the release tag it was installed from
    /// (the pin's, or an update's), at that tag's commit.
    pub revision: GitRevision,
    /// The version the installed manifest declared, when it declared one.
    pub version: Option<String>,
}

/// Why acquiring one default extension failed, after Pane's retries.
#[derive(Debug)]
pub(crate) struct Failed(String);

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Acquires the default extension `pin` names, fetching its pinned commit
/// from its repository into a folder of its own in `downloads`. Blocks on
/// the network and the file system, and tries an interrupted acquisition
/// again up to [`ATTEMPTS`] times before explaining the failure.
pub(crate) fn fetch(pin: &DefaultExtension, downloads: &Path) -> Result<Fetched, Failed> {
    // The failure's own message names the repository; the launcher frames
    // whose set-up failed.
    with_retries(|| acquire(pin, downloads)).map_err(Failed)
}

/// Why one attempt at acquiring a default extension failed, and whether
/// trying again can help (a connection that failed, not a revision that
/// was refused).
pub(crate) struct Failure {
    pub(crate) why: String,
    pub(crate) retry: bool,
}

/// Tries `once` up to [`ATTEMPTS`] times: a failure that says to retry
/// sleeps [`RETRY_AFTER`] first, and a failure that stays is explained
/// with how many attempts were made. The two retry loops — a default
/// extension's pinned revision, an update check, an update's download —
/// share it.
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

/// One attempt: fetch the pinned revision and write it out.
fn acquire(pin: &DefaultExtension, downloads: &Path) -> Result<Fetched, Failure> {
    let spec = pin.spec().map_err(failed)?;
    let fetched = crate::git::fetch(&spec, downloads).map_err(|why| {
        // A connection that failed, or a server that failed, may work on
        // another try; a revision the repository refused is explained
        // once. The Git client says which it was wherever it met one
        // (crate::git::is_connection_failure).
        if crate::git::is_connection_failure(&why) {
            interrupted(why)
        } else {
            failed(why)
        }
    })?;
    let crate::git::Fetched { download, origin } = fetched;
    let revision = GitRevision {
        // The pin names the tag whose commit was fetched; the commit that
        // was fetched is exactly the pinned one (a commit id pins the
        // bytes), so the record keeps the tag — what a later release's
        // updater compares its repository's newer release tags with.
        reference: GitRef::Tag(pin.tag.clone()),
        commit: origin.revision.commit.clone(),
    };
    Ok(Fetched {
        download,
        origin: DefaultOrigin {
            id: pin.id.clone(),
            repository: origin.repository,
            revision,
            lfs_pointers: origin.lfs_pointers,
        },
    })
}

/// The index document an artifact source serves, as Pane read and checked
/// it.
pub(crate) struct Index {
    /// What the index says of Pane's own application package, whose
    /// updates Pane offers the user ([`crate::application_update`]): read
    /// but neither parsed nor validated here, so that an application entry
    /// this Pane cannot take never keeps it from doing anything else.
    pub(crate) application: Option<serde_json::Value>,
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
    Ok(Index {
        application: index.application,
    })
}

/// Why a status other than 200 was answered for the index or a package:
/// trying again may fix a server that failed, never a package that is
/// simply not there.
pub(crate) fn answer(status: u16, why: String) -> Failure {
    if matches!(status, 403 | 500 | 502 | 503 | 504) {
        interrupted(why)
    } else {
        failed(why)
    }
}

/// The index document as it is served: `formatVersion` and, optionally,
/// the `application` entry naming Pane's own package.
#[derive(Deserialize)]
struct IndexJson {
    #[serde(rename = "formatVersion")]
    format_version: u64,
    /// Pane's own application package (see [`crate::application_update`]),
    /// read as it is written: parsed where it is used, so a broken entry
    /// is explained there rather than making the whole index unreadable.
    /// Optional, because a source that serves none (one built before the
    /// application entry existed) still serves the index.
    #[serde(default)]
    application: Option<serde_json::Value>,
}

/// One pin of the pins file, as it is written.
#[derive(Deserialize)]
struct PinJson {
    id: Option<String>,
    title: Option<String>,
    repository: Option<String>,
    tag: Option<String>,
    commit: Option<String>,
    platform: Option<String>,
}

/// The index format this Pane reads.
const INDEX_FORMAT: u64 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_on_this_computer_is_a_loopback_address_only() {
        let source = ArtifactSource::local("http://127.0.0.1:43127/").unwrap();
        assert_eq!(source.url(), "http://127.0.0.1:43127/");
        assert!(source.origin.is_loopback());
        for url in [
            "http://127.0.0.1:43127",
            "https://127.9.9.9/",
            "http://[::1]:8/",
        ] {
            let source = ArtifactSource::local(url).unwrap();
            assert!(source.url().ends_with('/'), "{}", source.url());
            assert!(source.origin.is_loopback());
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
    fn a_source_names_its_index() {
        let source = ArtifactSource::published();
        assert_eq!(source.url(), PUBLISHED);
        assert_eq!(source.index_url(), format!("{PUBLISHED}{INDEX_FILE}"));
    }

    #[test]
    fn a_pin_names_everything_a_fetch_needs() {
        let pins = parse_pins(
            r#"[
                { "id": "calculator", "title": "Calculator",
                  "repository": "https://github.com/pane-app/calculator",
                  "tag": "v0.5.0",
                  "commit": "0123456789012345678901234567890123456789" },
                { "id": "run", "title": "Run",
                  "repository": "https://github.com/pane-app/run",
                  "tag": "v0.2.0",
                  "commit": "0123456789012345678901234567890123456789",
                  "platform": "windows" }
            ]"#,
        )
        .unwrap();
        assert_eq!(
            pins,
            vec![
                DefaultExtension {
                    id: "calculator".into(),
                    title: "Calculator".into(),
                    repository: "https://github.com/pane-app/calculator".into(),
                    tag: "v0.5.0".into(),
                    commit: "0123456789012345678901234567890123456789".into(),
                    platform: None,
                },
                DefaultExtension {
                    id: "run".into(),
                    title: "Run".into(),
                    repository: "https://github.com/pane-app/run".into(),
                    tag: "v0.2.0".into(),
                    commit: "0123456789012345678901234567890123456789".into(),
                    platform: Some(Platform::Windows),
                }
            ]
        );
        // The platform gate: a pin that names none is set up on every
        // system, and one that names a platform on that system alone —
        // and on a system `Platform::current` does not know, not even
        // there.
        assert!(pins[0].runs_here());
        assert_eq!(
            pins[1].runs_here(),
            Platform::current() == Some(Platform::Windows)
        );
        // The fetch names the repository at the pinned commit: a commit id
        // pins the bytes, so the tag is recorded, never asked for.
        let spec = pins[0].spec().unwrap();
        assert_eq!(
            spec.reference.as_deref(),
            Some("0123456789012345678901234567890123456789")
        );
        assert_eq!(spec.repository.name(), "github.com/pane-app/calculator");
        // An empty file names no default extension: what a development
        // build sets up when it sets up none.
        assert_eq!(parse_pins("[]").unwrap(), Vec::new());
    }

    #[test]
    fn a_pins_file_that_cannot_be_used_is_explained() {
        // Not a list, an empty list, a pin missing a field, an id that is
        // no extension id, a repository that is no Git address, a tag that
        // is no release tag, a commit that is no full id, a platform that
        // names no system, and the same id twice: each explained, naming
        // the pin.
        for (text, expected) in [
            (
                "{ \"formatVersion\": 1 }",
                "the pins file is not a list of pins",
            ),
            (
                "[{ \"id\": \"calculator\", \"title\": \"Calculator\" }]",
                "pin 1: its repository is missing",
            ),
            (
                "[{ \"id\": \"Calc\", \"title\": \"\", \"repository\": \
                  \"https://github.com/pane-app/calculator\", \"tag\": \"v0.5.0\", \
                  \"commit\": \"0123456789012345678901234567890123456789\" }]",
                "pin 1: its id is `Calc`, not an extension id",
            ),
            (
                "[{ \"id\": \"calculator\", \"title\": \"Calculator\", \"repository\": \
                  \"not a repository\", \"tag\": \"v0.5.0\", \
                  \"commit\": \"0123456789012345678901234567890123456789\" }]",
                "pin 1: its repository is not a Git repository address",
            ),
            (
                "[{ \"id\": \"calculator\", \"title\": \"Calculator\", \"repository\": \
                  \"https://github.com/pane-app/calculator\", \"tag\": \"0.5.0\", \
                  \"commit\": \"0123456789012345678901234567890123456789\" }]",
                "pin 1: its tag is `0.5.0`, not a release tag such as v1.0.0",
            ),
            (
                "[{ \"id\": \"calculator\", \"title\": \"Calculator\", \"repository\": \
                  \"https://github.com/pane-app/calculator\", \"tag\": \"v0.5.0\", \
                  \"commit\": \"01234567890123456\" }]",
                "pin 1: its commit is `01234567890123456`, not a full commit id",
            ),
            (
                "[{ \"id\": \"run\", \"title\": \"Run\", \"repository\": \
                  \"https://github.com/pane-app/run\", \"tag\": \"v0.2.0\", \
                  \"commit\": \"0123456789012345678901234567890123456789\", \
                  \"platform\": \"haiku\" }]",
                "pin 1: its platform is `haiku`, not `windows`, `macos` or `linux`",
            ),
            (
                r#"[
                    { "id": "calculator", "title": "Calculator",
                      "repository": "https://github.com/pane-app/calculator",
                      "tag": "v0.5.0",
                      "commit": "0123456789012345678901234567890123456789" },
                    { "id": "calculator", "title": "Calculator",
                      "repository": "https://github.com/pane-app/calculator",
                      "tag": "v0.5.0",
                      "commit": "0123456789012345678901234567890123456789" }
                ]"#,
                "the pins file names `calculator` twice",
            ),
        ] {
            let why = parse_pins(text).unwrap_err();
            assert!(why.contains(expected), "{why} does not contain {expected}");
        }
    }
}
