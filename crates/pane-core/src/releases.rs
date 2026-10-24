//! Where Pane's own application updates are published: the releases of
//! Pane's repository on GitHub, `pane-app/pane` (ADR 0048).
//!
//! One release serves every system: its assets are the packages
//! `cargo xtask package-*` builds, one per system, each named
//! `pane-<version>-<os>-<arch>.zip` (Windows, macOS) or `.tar.gz`
//! (Linux), with a `<package>.sha256` file beside it naming its digest
//! as `sha256sum` writes one. Pane lists the newest releases through
//! GitHub's API — prereleases included, since every release of Pane is a
//! preview for now — and [`crate::application_update`] takes the newest
//! one that carries this system's package.
//!
//! Reading the list is one request to `api.github.com` when a check is
//! due — at most every few hours, and each time the user asks (ADR
//! 0050) — well within the 60 requests an hour GitHub allows an address
//! that does not sign in. The request names the listing it read last
//! (`If-None-Match` with its `ETag`), so a listing that has not changed is
//! answered `304 Not Modified` and not read again; GitHub still counts
//! such an answer against the limit for an address that does not sign
//! in, so the schedule is what keeps Pane inside it. A limit that was
//! used up anyway is explained, with when it lifts, and not tried again
//! until then. Downloading an asset asks `github.com`, which
//! redirects to the host it serves release assets from; Pane follows a
//! redirect only to GitHub's own hosts ([`ASSET_HOSTS`]), only over
//! HTTPS, and only a few times, so a release's download can lead nowhere
//! else.
//!
//! Release builds read only the published releases. Tests and
//! development builds can name a stand-in on this computer instead
//! (`PANE_RELEASES`, [`ReleaseSource::local`]): it answers the same API
//! path and serves the downloads under the same paths, and a redirect
//! from it is followed only to itself, so no check ever reaches the
//! network.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::defaults::{Failure, answer, failed, interrupted};
use crate::http::{Answer, GetError, OWN_LIMITS};

/// The repository whose releases carry Pane's packages.
pub const REPOSITORY: &str = "pane-app/pane";

/// GitHub's API, which lists a repository's releases.
const API: &str = "https://api.github.com/";

/// GitHub itself, where a release's assets are downloaded from.
const GITHUB: &str = "https://github.com/";

/// The hosts a download of a release's asset may be redirected to:
/// GitHub itself, and the hosts it serves release assets from
/// (`release-assets.githubusercontent.com` today,
/// `objects.githubusercontent.com` before it).
pub(crate) const ASSET_HOSTS: [&str; 3] = [
    "github.com",
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
];

/// How many releases one listing asks for, newest first: enough to find
/// the newest release that carries this system's package when a few
/// releases after it carry none.
const LISTED: u32 = 10;

/// The largest listing Pane reads: ten releases, their assets and their
/// notes, which GitHub keeps far smaller than this.
const MAX_LISTING: u64 = 4 << 20;

/// How many redirects one download follows: GitHub's own is one.
const MAX_REDIRECTS: usize = 5;

/// The version of GitHub's REST API Pane reads.
const API_VERSION: &str = "2022-11-28";

/// Where Pane reads its own releases. Release builds use only
/// [`ReleaseSource::published`]; tests and development builds can use a
/// stand-in on this computer ([`ReleaseSource::local`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseSource {
    /// The API's address, ending with `/`.
    api: String,
    /// The address the assets are downloaded from, ending with `/`.
    downloads: String,
    /// Whether it is on this computer: reached over plain HTTP if its
    /// address says so, and redirected only to itself.
    loopback: bool,
}

impl Default for ReleaseSource {
    fn default() -> ReleaseSource {
        ReleaseSource::published()
    }
}

/// One release of Pane, as the listing gives it: its version (its tag,
/// `v<version>`, without the `v`), its assets, and its notes and page,
/// which What's New shows after an update. Pane's update record keeps
/// what it needs of the listing in this shape too.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Release {
    pub(crate) version: String,
    pub(crate) assets: Vec<Asset>,
    /// The release's notes, as Markdown (GitHub's `body`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) notes: Option<String>,
    /// The release's page on GitHub (`html_url`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) page: Option<String>,
}

/// What one request for the listing answered.
#[derive(Debug)]
pub(crate) enum Listed {
    /// The listing, with the `ETag` that names it, if GitHub gave one.
    Releases {
        releases: Vec<Release>,
        etag: Option<String>,
    },
    /// `304 Not Modified`: the listing the `ETag` sent names is still the
    /// listing.
    Unchanged,
    /// GitHub's rate limit refused the request: why, and how many seconds
    /// from now it lifts.
    Limited { why: String, wait: u64 },
}

/// One asset of a release: its file name, its size in bytes and the
/// address it is downloaded from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Asset {
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) url: String,
}

impl ReleaseSource {
    /// Pane's releases on GitHub.
    pub fn published() -> ReleaseSource {
        ReleaseSource {
            api: API.to_owned(),
            downloads: GITHUB.to_owned(),
            loopback: false,
        }
    }

    /// A stand-in on this computer, for tests and development builds
    /// only: `url` must be `http://` or `https://` on a loopback address
    /// written as one (`127.0.0.1`, any `127.x.y.z`, or `[::1]`), with an
    /// optional port and path, and answers both the API and the downloads.
    /// Any other address is refused, `localhost` included (a name could
    /// resolve elsewhere), so that nothing but Pane's published releases
    /// is ever reached over the network. Release builds have no way to
    /// replace them.
    #[cfg(any(test, debug_assertions))]
    pub fn local(url: &str) -> Result<ReleaseSource, String> {
        let refused = || {
            format!(
                "the release source `{url}` is not on this computer: Pane reads its own \
                 application updates from the releases of {GITHUB}{REPOSITORY}, and only a \
                 stand-in on a loopback address such as 127.0.0.1 or [::1] can replace them, for \
                 tests and development"
            )
        };
        let base = crate::http::loopback_base(url, refused)?;
        Ok(ReleaseSource {
            api: base.clone(),
            downloads: base,
            loopback: true,
        })
    }

    /// The release source named by `PANE_RELEASES`, in development builds
    /// only (see [`ReleaseSource::local`]); `None` when it is not set, in
    /// which case this development build checks for no application update.
    #[cfg(any(test, debug_assertions))]
    pub fn from_dev_env() -> Option<Result<ReleaseSource, String>> {
        crate::http::dev_env("PANE_RELEASES").map(|url| ReleaseSource::local(&url))
    }

    /// The address of the listing of Pane's releases, newest first.
    pub fn listing_url(&self) -> String {
        format!("{}repos/{REPOSITORY}/releases?per_page={LISTED}", self.api)
    }

    /// Where every asset of Pane's releases is downloaded from: an asset
    /// whose address the listing gives elsewhere is refused.
    fn downloads_prefix(&self) -> String {
        format!("{}{REPOSITORY}/releases/download/", self.downloads)
    }

    /// Whether `url` is the download of an asset of Pane's releases.
    pub(crate) fn is_download(&self, url: &str) -> bool {
        url.strip_prefix(&self.downloads_prefix())
            .is_some_and(|rest| !rest.is_empty() && !rest.contains(['?', '#']))
    }

    /// Whether a download may be redirected to `url`: one of GitHub's own
    /// hosts over HTTPS, or, for a stand-in on this computer, the
    /// stand-in itself.
    pub(crate) fn may_follow(&self, url: &str) -> bool {
        let Some((scheme, authority, _)) = crate::http::split_url(url) else {
            return false;
        };
        if self.loopback {
            return crate::http::split_url(&self.downloads)
                .is_some_and(|(own_scheme, own, _)| own_scheme == scheme && own == authority);
        }
        scheme == "https"
            && ASSET_HOSTS
                .iter()
                .any(|host| authority.eq_ignore_ascii_case(host))
    }

    /// Lists Pane's newest releases: those whose tag is a release tag
    /// (`v` and dotted numbers), drafts left out (which GitHub shows no
    /// one who does not sign in anyway), prereleases kept. `etag` names the
    /// listing read last, if one was kept: GitHub answers `304 Not
    /// Modified` while it is still the listing ([`Listed::Unchanged`]).
    pub(crate) fn list(&self, etag: Option<&str>) -> Result<Listed, Failure> {
        let url = self.listing_url();
        let mut headers = vec![
            ("Accept", "application/vnd.github+json"),
            ("X-GitHub-Api-Version", API_VERSION),
        ];
        if let Some(etag) = etag {
            headers.push(("If-None-Match", etag));
        }
        crate::http::https_only(self.loopback, &url).map_err(failed)?;
        let read = crate::http::get_blocking(&url, &headers, MAX_LISTING, OWN_LIMITS).map_err(
            |error| match error {
                GetError::TooLarge => failed(format!(
                    "Pane's releases at {url} gave a listing larger than the {} MiB Pane reads",
                    MAX_LISTING >> 20
                )),
                GetError::Failed(why) => interrupted(format!(
                    "Pane's releases at {url} could not be reached: {why}"
                )),
            },
        )?;
        if let Some((why, wait)) = rate_limited(&read) {
            return Ok(Listed::Limited { why, wait });
        }
        // Only a request that named a listing can be told it is unchanged.
        if read.status == 304 && etag.is_some() {
            return Ok(Listed::Unchanged);
        }
        if read.status != 200 {
            return Err(answer(
                read.status,
                format!("Pane's releases at {url} answered {}", read.status),
            ));
        }
        let releases = parse_listing(&read.body).map_err(|why| {
            failed(format!(
                "Pane's releases at {url} gave a listing that {why}"
            ))
        })?;
        let etag = read
            .headers
            .get(http::header::ETAG)
            .and_then(|etag| etag.to_str().ok())
            .map(str::to_owned);
        Ok(Listed::Releases { releases, etag })
    }

    /// Downloads `url`, an asset of Pane's releases (`what` names it in
    /// an explanation), whose body may be at most `most` bytes, telling
    /// `progress` of its bytes so far. A redirect is followed only where
    /// [`ReleaseSource::may_follow`] allows, at most [`MAX_REDIRECTS`]
    /// times; the answer is the first that is not a redirect, whatever its
    /// status.
    pub(crate) fn download(
        &self,
        url: &str,
        what: &str,
        most: u64,
        progress: &(dyn Fn(u64) + Send + Sync),
    ) -> Result<Answer, Failure> {
        if !self.is_download(url) {
            return Err(failed(format!(
                "{what} is not downloaded from Pane's releases (its address is `{url}`)"
            )));
        }
        let mut url = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            crate::http::https_only(self.loopback, &url).map_err(failed)?;
            let read = crate::http::get_blocking_progressing(&url, &[], most, OWN_LIMITS, progress)
                .map_err(|error| match error {
                    GetError::TooLarge => failed(format!(
                        "{what} is larger than the {} MiB Pane downloads",
                        most >> 20
                    )),
                    GetError::Failed(why) => interrupted(format!(
                        "{what} could not be downloaded from {}: {why}",
                        host_of(&url)
                    )),
                })?;
            if !matches!(read.status, 301 | 302 | 303 | 307 | 308) {
                return Ok(read);
            }
            let location = read
                .headers
                .get(http::header::LOCATION)
                .and_then(|location| location.to_str().ok())
                .ok_or_else(|| {
                    failed(format!(
                        "{} redirected the download of {what} without saying where",
                        host_of(&url)
                    ))
                })?;
            let next = absolute(&url, location);
            if !self.may_follow(&next) {
                return Err(failed(format!(
                    "{} redirected the download of {what} to {}, which is not one of GitHub's \
                     own hosts; Pane downloads its releases from GitHub alone",
                    host_of(&url),
                    host_of(&next)
                )));
            }
            url = next;
        }
        Err(failed(format!(
            "the download of {what} was redirected more than {MAX_REDIRECTS} times"
        )))
    }
}

/// Why GitHub refused to answer, when it was its rate limit: the 60
/// requests an hour it allows an address that does not sign in were used
/// up (`403` or `429` with no requests remaining), or it asked for a
/// pause (`429`, or `Retry-After`). Trying again at once cannot help, so
/// the explanation says when it can, and the answer gives how many
/// seconds from now that is (an hour when GitHub did not say), so a check
/// is not made again before it.
fn rate_limited(read: &Answer) -> Option<(String, u64)> {
    if !matches!(read.status, 403 | 429) {
        return None;
    }
    let header = |name: &str| {
        read.headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
    };
    let used_up = header("x-ratelimit-remaining") == Some("0");
    let retry_after = header("retry-after").and_then(|value| value.parse::<u64>().ok());
    if !used_up && retry_after.is_none() && read.status != 429 {
        return None;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let wait = retry_after.or_else(|| {
        header("x-ratelimit-reset")
            .and_then(|value| value.parse::<u64>().ok())
            .map(|reset| reset.saturating_sub(now))
    });
    Some((limit_reached(wait), wait.unwrap_or(3600)))
}

/// Why a check cannot be made while GitHub's rate limit is used up, with
/// when it lifts, `wait` seconds from now (if that is known).
pub(crate) fn limit_reached(wait: Option<u64>) -> String {
    let when = match wait {
        Some(seconds) => {
            let minutes = seconds.div_ceil(60).max(1);
            let unit = if minutes == 1 { "minute" } else { "minutes" };
            format!("try again in about {minutes} {unit}")
        }
        None => "try again later".to_owned(),
    };
    format!(
        "GitHub limits how often Pane's releases can be read from this address, and the limit \
         is reached; {when}"
    )
}

/// `location`, as an absolute address: a redirect to a path stays on the
/// host of `url`.
fn absolute(url: &str, location: &str) -> String {
    match crate::http::split_url(url) {
        Some((scheme, authority, _))
            if location.starts_with('/') && !location.starts_with("//") =>
        {
            format!("{scheme}://{authority}{location}")
        }
        _ => location.to_owned(),
    }
}

/// The host `url` names, for an explanation: a download's address can be
/// long and signed, and the host is what the user needs.
fn host_of(url: &str) -> String {
    crate::http::split_url(url).map_or_else(|| url.to_owned(), |(_, host, _)| host.to_owned())
}

/// The releases a listing names, as GitHub's API writes them: a JSON array
/// of releases, each with its `tag_name`, whether it is a `draft` and its
/// `assets`, each asset with its `name`, `size` and
/// `browser_download_url`. A release whose tag is not a release tag
/// (`v` and dotted numbers) is not one of Pane's versions, and is left
/// out, as a draft is.
pub(crate) fn parse_listing(body: &[u8]) -> Result<Vec<Release>, String> {
    let releases: Vec<ReleaseJson> =
        serde_json::from_slice(body).map_err(|error| format!("cannot be read: {error}"))?;
    Ok(releases
        .into_iter()
        .filter(|release| !release.draft && crate::defaults::is_release_tag(&release.tag_name))
        .map(|release| Release {
            version: release.tag_name[1..].to_owned(),
            assets: release
                .assets
                .into_iter()
                .map(|asset| Asset {
                    name: asset.name,
                    size: asset.size,
                    url: asset.browser_download_url,
                })
                .collect(),
            notes: release.body.filter(|body| !body.trim().is_empty()),
            page: release.html_url,
        })
        .collect())
}

/// One release of the listing, as GitHub's API writes it: the fields Pane
/// reads, of the many it gives.
#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<AssetJson>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
}

/// One asset of a release, as GitHub's API writes it.
#[derive(Deserialize)]
struct AssetJson {
    name: String,
    #[serde(default)]
    size: u64,
    browser_download_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_published_source_is_pane_s_repository_on_github() {
        let source = ReleaseSource::published();
        assert_eq!(
            source.listing_url(),
            "https://api.github.com/repos/pane-app/pane/releases?per_page=10"
        );
        assert!(source.is_download(
            "https://github.com/pane-app/pane/releases/download/v0.1.0/pane-0.1.0-windows-x86_64.zip"
        ));
        // Another repository's release, another host, or a query: not a
        // download of Pane's releases.
        for url in [
            "https://github.com/someone/pane/releases/download/v0.1.0/pane.zip",
            "https://example.com/pane-app/pane/releases/download/v0.1.0/pane.zip",
            "http://github.com/pane-app/pane/releases/download/v0.1.0/pane.zip",
            "https://github.com/pane-app/pane/releases/download/",
            "https://github.com/pane-app/pane/releases/download/v0.1.0/pane.zip?x=1",
        ] {
            assert!(!source.is_download(url), "{url}");
        }
    }

    #[test]
    fn a_download_follows_a_redirect_only_to_github_s_own_hosts() {
        let source = ReleaseSource::published();
        for url in [
            "https://github.com/pane-app/pane/releases/download/v0.1.0/pane.zip",
            "https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sig=x",
            "https://objects.githubusercontent.com/github-production-release-asset-2e65be/1/2",
            "https://Release-Assets.GitHubUserContent.com/x",
        ] {
            assert!(source.may_follow(url), "{url}");
        }
        for url in [
            // Not over HTTPS.
            "http://release-assets.githubusercontent.com/x",
            // Not one of GitHub's asset hosts, however close its name.
            "https://example.com/x",
            "https://github.com.example.com/x",
            "https://evil-githubusercontent.com/x",
            "https://raw.githubusercontent.com/x",
            "https://api.github.com/x",
            "https://github.com:8443/x",
            // User information, or no address at all.
            "https://someone@github.com/x",
            "ftp://github.com/x",
            "/relative",
        ] {
            assert!(!source.may_follow(url), "{url}");
        }
    }

    #[test]
    fn a_stand_in_is_on_this_computer_and_redirects_only_to_itself() {
        let source = ReleaseSource::local("http://127.0.0.1:43127").unwrap();
        assert_eq!(
            source.listing_url(),
            "http://127.0.0.1:43127/repos/pane-app/pane/releases?per_page=10"
        );
        assert!(source.is_download("http://127.0.0.1:43127/pane-app/pane/releases/download/v1/x"));
        assert!(source.may_follow("http://127.0.0.1:43127/assets/x"));
        for url in [
            "http://127.0.0.1:43128/assets/x",
            "https://127.0.0.1:43127/assets/x",
            "https://release-assets.githubusercontent.com/x",
        ] {
            assert!(!source.may_follow(url), "{url}");
        }
        for url in [
            "http://localhost:43127/",
            "http://example.com/",
            "https://api.github.com/",
            "ftp://127.0.0.1/",
            "http://192.168.1.4/",
        ] {
            assert!(ReleaseSource::local(url).is_err(), "{url:?}");
        }
    }

    #[test]
    fn a_listing_names_pane_s_versions_and_their_assets() {
        let listing = serde_json::json!([
            { "tag_name": "v0.2.0", "draft": false, "prerelease": true,
              "body": "## What's new\n\nThings.",
              "html_url": "https://github.com/pane-app/pane/releases/tag/v0.2.0", "assets": [
                { "name": "pane-0.2.0-windows-x86_64.zip", "size": 12,
                  "browser_download_url": "https://github.com/pane-app/pane/releases/download/v0.2.0/pane-0.2.0-windows-x86_64.zip",
                  "state": "uploaded" }
            ] },
            // A draft, and a tag that is not a version: left out.
            { "tag_name": "v0.3.0", "draft": true, "assets": [] },
            { "tag_name": "nightly", "assets": [] },
            { "tag_name": "v0.3.0-rc.1", "assets": [] },
            { "tag_name": "v0.1.0", "body": "  ", "assets": [] }
        ]);
        let releases = parse_listing(listing.to_string().as_bytes()).unwrap();
        assert_eq!(
            releases,
            [
                Release {
                    version: "0.2.0".into(),
                    assets: vec![Asset {
                        name: "pane-0.2.0-windows-x86_64.zip".into(),
                        size: 12,
                        url: "https://github.com/pane-app/pane/releases/download/v0.2.0/pane-0.2.0-windows-x86_64.zip".into(),
                    }],
                    notes: Some("## What's new\n\nThings.".into()),
                    page: Some("https://github.com/pane-app/pane/releases/tag/v0.2.0".into()),
                },
                // Notes that are only blank are none.
                Release {
                    version: "0.1.0".into(),
                    assets: vec![],
                    notes: None,
                    page: None,
                },
            ]
        );
        // No release yet: nothing listed, which is not an error.
        assert_eq!(parse_listing(b"[]").unwrap(), []);
        // Something that is not a listing is explained.
        assert!(parse_listing(br#"{ "message": "Not Found" }"#).is_err());
    }

    #[test]
    fn a_rate_limit_that_is_used_up_says_when_it_lifts() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let answer = |status: u16, headers: &[(&'static str, String)]| {
            let mut map = http::HeaderMap::new();
            for (name, value) in headers {
                map.insert(*name, value.parse().unwrap());
            }
            Answer {
                status,
                headers: map,
                body: Vec::new(),
            }
        };
        let limited = answer(
            403,
            &[
                ("x-ratelimit-remaining", "0".into()),
                ("x-ratelimit-reset", (now + 600).to_string()),
            ],
        );
        let (why, until) = rate_limited(&limited).unwrap();
        assert!(why.contains("limit is reached"), "{why}");
        assert!(why.contains("try again in about 10 minutes"), "{why}");
        // When it lifts is kept, so no check is made before it.
        assert!((590..=600).contains(&until), "{until}");
        let paused = answer(429, &[("retry-after", "30".into())]);
        let (why, until) = rate_limited(&paused).unwrap();
        assert!(why.contains("try again in about 1 minute"), "{why}");
        assert_eq!(until, 30);
        // A limit that does not say when it lifts is waited out an hour.
        let unsaid = answer(429, &[]);
        let (why, until) = rate_limited(&unsaid).unwrap();
        assert!(why.contains("try again later"), "{why}");
        assert_eq!(until, 3600);
        // A 403 that is not the rate limit, and a 200, are not.
        assert_eq!(
            rate_limited(&answer(403, &[("x-ratelimit-remaining", "12".into())])),
            None
        );
        assert_eq!(rate_limited(&answer(200, &[])), None);
    }
}
