//! Extension packages distributed as Git repositories.
//!
//! A Git-distributed Pane package is a repository whose root holds a
//! `pane.json` and, in the revision installed, the built components it names
//! (a *release* revision, such as a release tag or branch its author commits
//! the built files to). A revision holding only the source is explained, not
//! built: Pane never runs a build, a hook or anything else from a repository.
//!
//! Pane fetches the repository itself, speaking Git's smart HTTP protocol
//! (version 2) over HTTPS through Pane's one HTTP client
//! ([`crate::http`]); no `git` program, library or configuration is used, so
//! hooks, filters, `core.fsmonitor`, `core.sshCommand`, `GIT_*` variables and
//! the user's or the repository's Git configuration have no effect. It:
//!
//! 1. asks the server for its capabilities (`info/refs`), which must be
//!    protocol version 2 with SHA-1 object ids;
//! 2. resolves the reference asked for (`ls-refs`): none is the default
//!    branch, else a branch or a tag of that name, or a full commit id;
//! 3. fetches exactly that one commit, without its history (`fetch` with
//!    `want <commit>` and `deepen 1`), as one pack;
//! 4. reads the pack (its checksum, each object's zlib stream, deltas),
//!    computing every object's SHA-1 id with collision detection, so the
//!    files written are exactly the tree of the commit asked for;
//! 5. writes that tree into a download folder of its own
//!    ([`crate::downloads`]), taking only regular files and folders whose
//!    names every system can write: a symbolic link, a submodule, a `.git`
//!    entry (in any case, as `git~1`, or with characters HFS+ ignores), two
//!    names differing only in case, or a name some system cannot write or
//!    reads as another (such as a Windows device name, or one ending in a
//!    dot) refuses the whole revision. Names that differ only in Unicode
//!    normalization are not compared. Nothing is written executable.
//!
//! The pack, the objects, the files and the folders are limited in size and
//! number ([`MAX_PACK`], [`MAX_OBJECTS`], [`MAX_UNPACKED`], [`MAX_ENTRIES`],
//! [`MAX_DEPTH`]), and what a pack takes in memory while it is read,
//! inflated entries and the objects its deltas make together, by one budget
//! ([`MAX_INFLATED`]).
//!
//! Only `https://` addresses are fetched. SSH (`ssh://`, `git@host:path`) and
//! scheme-less (`host/path`) addresses name the same repository, fetched
//! over HTTPS from the same host and path. Tests and development builds may
//! also fetch `http://` from a loopback address written as one
//! (`127.x.y.z`, `[::1]`), so that no check reaches the network.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::rc::Rc;

use sha1_checked::Sha1;
use sha1_checked::digest::Update;

use crate::downloads::{Download, check_part};
use crate::http::{Answer, GetError};
use crate::packages::capitalized;

/// The largest capability advertisement or reference listing Pane reads.
pub const MAX_REFS: u64 = 16 << 20;
/// The largest pack (the fetch's answer) Pane downloads.
pub const MAX_PACK: u64 = 64 << 20;
/// The most objects a pack may hold.
pub const MAX_OBJECTS: usize = 20_000;
/// The most the revision's files may take, all together.
pub const MAX_UNPACKED: u64 = 256 << 20;
/// The most files and folders the revision may hold.
pub const MAX_ENTRIES: usize = 10_000;
/// The deepest folders may nest in the revision.
pub const MAX_DEPTH: usize = 32;
/// The most a pack's contents may take in memory while Pane reads it, all
/// together: its entries once inflated (whole objects and the instructions
/// of deltas not yet applied) and the objects its deltas make. A delta's
/// instructions are let go, and no longer count, once it is applied.
pub const MAX_INFLATED: u64 = 256 << 20;
/// The longest chain of deltas Pane follows to one object. Deltas are
/// resolved without recursion, and [`MAX_INFLATED`] bounds what a chain
/// makes, so this only refuses what no Git writes (its default is 50).
const MAX_DELTA_CHAIN: usize = 4096;

/// The limits a fetch works within; smaller in tests.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub pack: u64,
    pub objects: usize,
    pub unpacked: u64,
    pub entries: usize,
    pub depth: usize,
    pub inflated: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            pack: MAX_PACK,
            objects: MAX_OBJECTS,
            unpacked: MAX_UNPACKED,
            entries: MAX_ENTRIES,
            depth: MAX_DEPTH,
            inflated: MAX_INFLATED,
        }
    }
}

/// What a pack's contents take in memory while it is read, within
/// [`Limits::inflated`].
struct Budget {
    used: u64,
    most: u64,
}

impl Budget {
    /// Counts `bytes` more, before they are allocated, or refuses them.
    fn take(&mut self, bytes: u64) -> Result<(), String> {
        self.used = self.used.saturating_add(bytes);
        if self.used > self.most {
            return Err(format!(
                "its objects take more than the {} MiB Pane reads",
                self.most >> 20
            ));
        }
        Ok(())
    }

    /// Counts `bytes` let go.
    fn give_back(&mut self, bytes: u64) {
        self.used = self.used.saturating_sub(bytes);
    }
}

/// A Git repository as Pane identifies and fetches it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Repository {
    /// `host[:port]/path`: the host in lowercase, the path without `.git`,
    /// the port only when it is not HTTPS's own. Its identity.
    name: String,
    /// The address Pane fetches: `https://<host>[:port]/<path>`, with `.git`
    /// when it was written with it.
    url: String,
    /// Whether it is on this computer and may be fetched over plain HTTP
    /// (tests and development builds only).
    loopback: bool,
    /// Whether it was named by an SSH address (`ssh://…`, `user@host:path`),
    /// which Pane fetches over HTTPS instead, from the same host and path.
    ssh: bool,
}

impl Repository {
    /// `host[:port]/path`, its identity.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The address Pane fetches it from.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Whether it was named by an SSH address, fetched over HTTPS instead.
    pub fn written_as_ssh(&self) -> bool {
        self.ssh
    }
}

/// The hosts that serve a repository at any case of its path, so that
/// `Owner/Repo` and `owner/repo` are one repository there: their paths are
/// compared in lowercase (the user's decision for #46). Any other host, or
/// one of these on another port, keeps the path's case.
const CASE_INSENSITIVE_HOSTS: &[&str] =
    &["github.com", "gitlab.com", "bitbucket.org", "codeberg.org"];

/// Hosts (`host:port`) tests treat as ignoring case, as they cannot serve
/// github.com from 127.0.0.1.
#[cfg(any(test, debug_assertions))]
static CASE_INSENSITIVE_FOR_TESTS: std::sync::Mutex<Vec<String>> =
    std::sync::Mutex::new(Vec::new());

/// Has Pane compare the paths of repositories on `host_port` (such as
/// `127.0.0.1:43127`) in lowercase, as it does on github.com: for tests
/// only, whose servers are on 127.0.0.1.
#[cfg(any(test, debug_assertions))]
pub fn ignore_case_on_host_for_tests(host_port: &str) {
    CASE_INSENSITIVE_FOR_TESTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(host_port.to_owned());
}

/// Whether the server at `host` and `port` (`None`: the scheme's own)
/// serves a repository at any case of its path.
fn ignores_case(host: &str, port: Option<u16>) -> bool {
    #[cfg(any(test, debug_assertions))]
    {
        let host_port = match port {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        let listed = CASE_INSENSITIVE_FOR_TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&host_port);
        if listed {
            return true;
        }
    }
    port.is_none() && CASE_INSENSITIVE_HOSTS.contains(&host)
}

/// A Git package to install, as the user or a dependency names it: a
/// repository and optionally a reference after `@` (a branch, a tag, a full
/// commit id, or `refs/heads/<branch>` or `refs/tags/<tag>`); without one,
/// the repository's default branch.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GitSpec {
    pub repository: Repository,
    pub reference: Option<String>,
}

impl fmt::Display for GitSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.reference {
            Some(reference) => write!(f, "{}@{reference}", self.repository.name),
            None => f.write_str(&self.repository.name),
        }
    }
}

impl GitSpec {
    /// Reads a repository address with an optional `@<reference>`, with or
    /// without `git:` (or `git+`) before it:
    ///
    /// - `https://host/owner/repo[.git]`
    /// - `host/owner/repo` (fetched over HTTPS)
    /// - `ssh://[user@]host[:port]/owner/repo` and `user@host:owner/repo`
    ///   (the same repository, fetched over HTTPS from the same host and
    ///   path)
    /// - in tests and development builds, `http://127.0.0.1:<port>/…` or
    ///   `http://[::1]:<port>/…`.
    ///
    /// Equivalent forms name one repository: the host is compared in
    /// lowercase, a trailing `.git` or `/` and HTTPS's port are ignored; the
    /// path keeps its case. Credentials in the address, `?`, `#`, `%` and
    /// other schemes are refused.
    pub fn parse(text: &str) -> Result<GitSpec, String> {
        let text = text.trim();
        let written = text;
        let text = text.strip_prefix("git:").unwrap_or(text);
        let text = text.strip_prefix("git+").unwrap_or(text);
        let refused = |why: &str| {
            Err(format!(
                "`{written}` is not a Git repository address: {why}"
            ))
        };
        if text.is_empty() {
            return Err(
                "a Git repository address is needed, such as https://github.com/owner/repo".into(),
            );
        }
        let (scheme, rest) = match text.split_once("://") {
            Some((scheme, rest)) => (Some(scheme.to_ascii_lowercase()), rest),
            None => (None, text),
        };
        let ssh =
            matches!(scheme.as_deref(), Some("ssh")) || (scheme.is_none() && is_scp_form(text));
        // (host with optional port, path with optional reference, fetched
        // over plain HTTP from this computer)
        let (authority, path, plain) = match scheme.as_deref() {
            Some("https") => {
                let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
                if authority.contains('@') {
                    return refused(
                        "Pane sends no credentials, so it installs only from public repositories",
                    );
                }
                (authority.to_owned(), path, false)
            }
            Some("http") => {
                let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
                if !cfg!(any(test, debug_assertions)) || !is_loopback_literal(authority) {
                    return refused(
                        "Pane fetches Git repositories only over HTTPS (https://…), so that what \
                         it installs is what the server sent",
                    );
                }
                (authority.to_owned(), path, true)
            }
            Some("ssh") => {
                let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
                let host = authority
                    .rsplit_once('@')
                    .map_or(authority, |(_, host)| host);
                // The SSH port says nothing of the HTTPS one.
                let host = strip_port(host);
                (host.to_owned(), path, false)
            }
            Some(other) => {
                return refused(&format!(
                    "Pane fetches Git repositories over HTTPS, and does not use `{other}://`"
                ));
            }
            None if ssh => {
                let at = text.find('@').expect("an scp-like address has `@`");
                let colon = text.find(':').expect("an scp-like address has `:`");
                (text[at + 1..colon].to_owned(), &text[colon + 1..], false)
            }
            None => {
                let (authority, path) = text.split_once('/').unwrap_or((text, ""));
                (authority.to_owned(), path, false)
            }
        };
        let Some((host, port)) = split_host(&authority) else {
            return refused("it names no host Pane can reach, such as github.com");
        };
        let port = match port {
            Some(443) if !plain => None,
            port => port,
        };
        let (path, reference) = match path.split_once('@') {
            Some((path, reference)) => (path, Some(reference)),
            None => (path, None),
        };
        let path = path.trim_matches('/');
        let folded = ignores_case(&host, port);
        // `.git` as written, kept in the address fetched; in any case where
        // the host ignores case. (Not yet checked to be ASCII: `get` rather
        // than slicing, which could split a character.)
        let suffix = path
            .len()
            .checked_sub(4)
            .and_then(|start| Some((start, path.get(start..)?)))
            .filter(|(_, suffix)| match folded {
                true => suffix.eq_ignore_ascii_case(".git"),
                false => *suffix == ".git",
            });
        let (path, dot_git) = match suffix {
            Some((start, suffix)) => (path[..start].trim_end_matches('/'), Some(suffix)),
            None => (path, None),
        };
        if path.is_empty() {
            return refused("it names no repository after the host, such as github.com/owner/repo");
        }
        for part in path.split('/') {
            let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~');
            if part.is_empty() || part == "." || part == ".." || !part.chars().all(allowed) {
                return refused(
                    "its repository path may hold only letters, digits, `-`, `.`, `_`, `~` and \
                     `/` between names",
                );
            }
        }
        let reference = reference.map(check_reference).transpose()?;
        let host_port = match port {
            Some(port) => format!("{host}:{port}"),
            None => host.clone(),
        };
        let scheme = if plain { "http" } else { "https" };
        // The path is ASCII (checked above), so lowercasing it is exact.
        let identity_path = match folded {
            true => path.to_ascii_lowercase(),
            false => path.to_owned(),
        };
        Ok(GitSpec {
            repository: Repository {
                name: format!("{host_port}/{identity_path}"),
                url: format!("{scheme}://{host_port}/{path}{}", dot_git.unwrap_or("")),
                loopback: plain,
                ssh,
            },
            reference,
        })
    }
}

/// Whether `text`, written without a scheme, is `user@host:path`, as scp
/// and Git write SSH addresses: an `@` before a `:` before any `/`.
fn is_scp_form(text: &str) -> bool {
    let slash = text.find('/');
    match (text.find('@'), text.find(':')) {
        (Some(at), Some(colon)) => at < colon && slash.is_none_or(|s| colon < s),
        _ => false,
    }
}

/// `host` without a `:port` after it (an IPv6 literal keeps its brackets).
fn strip_port(host: &str) -> &str {
    match host.rsplit_once(':') {
        Some((name, port)) if !name.ends_with(':') && port.chars().all(|c| c.is_ascii_digit()) => {
            name
        }
        _ => host,
    }
}

/// The host, in lowercase, and the port of `authority` (`host[:port]`, an
/// IPv6 literal in brackets), if it is one.
fn split_host(authority: &str) -> Option<(String, Option<u16>)> {
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (literal, after) = rest.split_once(']')?;
        literal.parse::<std::net::Ipv6Addr>().ok()?;
        let port = match after {
            "" => None,
            port => Some(port.strip_prefix(':')?),
        };
        (format!("[{}]", literal.to_ascii_lowercase()), port)
    } else {
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        let host = host.to_ascii_lowercase();
        let valid = !host.is_empty()
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
            && !host.starts_with(['-', '.'])
            && !host.ends_with('-');
        if !valid {
            return None;
        }
        (host.trim_end_matches('.').to_owned(), port)
    };
    let port = match port {
        None => None,
        // Digits only: `u16::from_str` would take `+80`.
        Some(port) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            match port.parse::<u16>() {
                Ok(port) if port > 0 => Some(port),
                _ => return None,
            }
        }
        Some(_) => return None,
    };
    Some((host, port))
}

/// Whether `authority` is a loopback address written as one, with a port or
/// not: `127.x.y.z` or `[::1]`, never a name such as `localhost`, which could
/// resolve elsewhere.
fn is_loopback_literal(authority: &str) -> bool {
    let host = if let Some(rest) = authority.strip_prefix('[') {
        match rest.split_once(']') {
            Some((literal, _)) => literal,
            None => return false,
        }
    } else {
        strip_port(authority)
    };
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

/// Checks a reference as written after `@`: a full commit id (40
/// hexadecimal digits, returned in lowercase), or a branch or tag name as
/// Git accepts one (`git check-ref-format`), optionally spelled
/// `refs/heads/<branch>` or `refs/tags/<tag>`.
fn check_reference(reference: &str) -> Result<String, String> {
    if is_commit_id(reference) {
        return Ok(reference.to_ascii_lowercase());
    }
    let refused = || {
        Err(format!(
            "`{reference}` is not a branch, a tag or a full commit id: name a branch or tag as \
             Git accepts one, or a commit by its 40 hexadecimal digits"
        ))
    };
    let bad_char = |c: char| {
        c.is_ascii_control() || matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\')
    };
    if reference.is_empty()
        || reference.len() > 255
        || reference == "@"
        || reference.chars().any(bad_char)
        || reference.contains("..")
        || reference.contains("@{")
        || reference.starts_with(['-', '/'])
        || reference.ends_with(['/', '.'])
        || reference.contains("//")
        || reference
            .split('/')
            .any(|part| part.starts_with('.') || part.ends_with(".lock"))
    {
        return refused();
    }
    Ok(reference.to_owned())
}

/// The most characters of a server's text (a commit subject, a refusal)
/// Pane shows.
const MAX_SHOWN: usize = 200;

/// `text`, which a server chose, as Pane shows it in a preview or an error:
/// without control characters (a terminal escape, a line break), line or
/// paragraph separators, or format characters, which show nothing
/// themselves but reorder text (bidirectional overrides, embeddings and
/// isolates, which can show `exe.txt` as `txt.exe`) or hide it (zero-width
/// spaces and joiners, soft hyphens, tags); and at most [`MAX_SHOWN`]
/// characters, `…` marking where it was cut.
pub(crate) fn shown(text: &str) -> String {
    let hidden = |c: char| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') || is_format(c);
    let mut out = String::new();
    for (count, c) in text.chars().filter(|&c| !hidden(c)).enumerate() {
        if count == MAX_SHOWN {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out
}

/// Whether `c` is a format character (Unicode's general category Cf, as
/// of Unicode 16).
fn is_format(c: char) -> bool {
    matches!(
        c,
        '\u{ad}'
            | '\u{600}'..='\u{605}'
            | '\u{61c}'
            | '\u{6dd}'
            | '\u{70f}'
            | '\u{890}'..='\u{891}'
            | '\u{8e2}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{110bd}'
            | '\u{110cd}'
            | '\u{13430}'..='\u{1343f}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0001}'
            | '\u{e0020}'..='\u{e007f}'
    )
}

/// The most bytes of a server's text Pane decodes to show it: enough for
/// [`MAX_SHOWN`] characters of four bytes each.
const MAX_DECODED: usize = 1024;

/// `bytes`, text a server chose that may be long or not UTF-8, as [`shown`]
/// shows it, decoding at most [`MAX_DECODED`] bytes of it, so that showing
/// it never takes a multiple of its size.
fn shown_bytes(bytes: &[u8]) -> String {
    let head = &bytes[..bytes.len().min(MAX_DECODED)];
    let mut out = shown(&String::from_utf8_lossy(head));
    if head.len() < bytes.len() && !out.ends_with('…') {
        out.push('…');
    }
    out
}

/// Whether `text` is a full SHA-1 commit id.
pub fn is_commit_id(text: &str) -> bool {
    text.len() == 40 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// Which reference a revision was installed from, and so whether it is
/// tracked (a branch, which a later update follows) or pinned (a tag or a
/// commit, which an update keeps).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitRef {
    /// The repository's default branch (its `HEAD`), which pointed to the
    /// branch named, if the server said: tracked.
    Default { branch: Option<String> },
    /// A branch: tracked.
    Branch(String),
    /// A tag: pinned.
    Tag(String),
    /// A commit named by its id: pinned.
    Commit,
}

/// A revision of a Git package, as Pane installed and records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitRevision {
    pub reference: GitRef,
    /// The commit's full id.
    pub commit: String,
}

impl GitRevision {
    /// Whether an update keeps this revision (a tag or a commit) rather
    /// than following its branch.
    pub fn pinned(&self) -> bool {
        matches!(self.reference, GitRef::Tag(_) | GitRef::Commit)
    }

    /// The full reference name a record keeps: `refs/heads/<branch>` or
    /// `refs/tags/<tag>`; `None` for the default branch and a commit.
    pub(crate) fn ref_name(&self) -> Option<String> {
        match &self.reference {
            GitRef::Branch(branch) => Some(format!("refs/heads/{branch}")),
            GitRef::Tag(tag) => Some(format!("refs/tags/{tag}")),
            GitRef::Default { .. } | GitRef::Commit => None,
        }
    }

    /// The revision a record keeps as `gitRef` (if any), `gitCommit` and
    /// `pinned`.
    pub(crate) fn from_record(ref_name: Option<&str>, commit: &str, pinned: bool) -> GitRevision {
        let reference = match ref_name {
            Some(name) => match (
                name.strip_prefix("refs/heads/"),
                name.strip_prefix("refs/tags/"),
            ) {
                (Some(branch), _) => GitRef::Branch(branch.to_owned()),
                (_, Some(tag)) => GitRef::Tag(tag.to_owned()),
                _ => GitRef::Branch(name.to_owned()),
            },
            None if pinned => GitRef::Commit,
            None => GitRef::Default { branch: None },
        };
        GitRevision {
            reference,
            commit: commit.to_owned(),
        }
    }

    /// The reference to ask for to fetch this revision again as it is
    /// installed: its branch or tag, its commit, or none for the default
    /// branch.
    pub(crate) fn asked_as(&self) -> Option<String> {
        match &self.reference {
            GitRef::Commit => Some(self.commit.clone()),
            _ => self.ref_name(),
        }
    }

    /// The commit's id as people read it: its first 12 digits.
    pub fn short_commit(&self) -> &str {
        &self.commit[..self.commit.len().min(12)]
    }

    /// "the default branch, main", "branch main", "tag v1.0.0" or "commit
    /// 1a2b3c4d5e6f".
    pub fn describe(&self) -> String {
        match &self.reference {
            GitRef::Default {
                branch: Some(branch),
            } => format!("the default branch, {branch}"),
            GitRef::Default { branch: None } => "the default branch".into(),
            GitRef::Branch(branch) => format!("branch {branch}"),
            GitRef::Tag(tag) => format!("tag {tag}"),
            GitRef::Commit => format!("commit {}", self.short_commit()),
        }
    }
}

/// Where a previewed or installed Git package was fetched from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitOrigin {
    pub repository: Repository,
    pub revision: GitRevision,
    /// Whether the repository's default branch, a branch or a tag points to
    /// the commit, as the server listed them (always, but for a commit named
    /// by its id). A commit id proves the contents, not where the commit came
    /// from: a host that shares storage between forks, as GitHub does,
    /// serves a fork's or a pull request's commit at the repository's
    /// address too.
    pub advertised: bool,
    /// The first line of the commit's message.
    pub subject: String,
    /// Files of the revision that are Git LFS pointers rather than their
    /// contents, which Pane does not fetch.
    pub lfs_pointers: Vec<String>,
}

impl GitOrigin {
    /// Why its commit may not be this repository's, when no branch or tag
    /// points to it (only a commit named by its id can be so), for a preview
    /// to caution with; `None` otherwise.
    pub(crate) fn caution(&self) -> Option<String> {
        (!self.advertised).then(|| {
            format!(
                "no branch or tag of {} points to commit {}. A host that shares storage between \
                 forks, as GitHub does, can serve a fork's or a pull request's commit at this \
                 address, so its id alone does not show that this repository made it",
                self.repository.name(),
                self.revision.short_commit()
            )
        })
    }
}

/// A package from Git as installed: the address it was fetched from and
/// the revision installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledGit {
    pub url: String,
    pub revision: GitRevision,
}

/// A revision fetched and written out.
pub(crate) struct Fetched {
    pub download: Download,
    pub origin: GitOrigin,
}

/// The User-Agent of Pane's Git requests: Git hosts serve the smart
/// protocol to clients naming themselves `git/…`.
const USER_AGENT: &str = concat!("git/pane-", env!("CARGO_PKG_VERSION"));

/// Fetches the revision `spec` names and writes its tree into a new folder
/// in `downloads`, or explains why it cannot. Blocks on the network.
pub(crate) fn fetch(spec: &GitSpec, downloads: &Path) -> Result<Fetched, String> {
    fetch_within(spec, downloads, Limits::default())
}

/// The revision `reference` (none: the repository's default branch) points
/// to now, resolved without fetching anything of it: the updater's check
/// of a tracked branch, which fetches only when the branch has moved. The
/// same explanation [`fetch`](fetch) gives when the reference cannot be
/// resolved.
pub(crate) fn resolve_reference(
    repository: &Repository,
    reference: Option<&str>,
) -> Result<GitRevision, String> {
    let remote = Remote::connect(repository)?;
    let (revision, _) = remote.resolve(reference)?;
    Ok(revision)
}

pub(crate) fn fetch_within(
    spec: &GitSpec,
    downloads: &Path,
    limits: Limits,
) -> Result<Fetched, String> {
    let remote = Remote::connect(&spec.repository)?;
    let (revision, advertised) = remote.resolve(spec.reference.as_deref())?;
    let pack = remote.fetch(&revision.commit, limits)?;
    let name = spec.repository.name();
    let objects = read_pack(&pack, limits)
        .map_err(|why| format!("The Git repository {name} sent a pack Pane cannot read: {why}"))?;
    drop(pack);
    let mut written = None;
    let download = Download::create(downloads, |folder| {
        written = Some(check_out(&objects, &revision.commit, folder, limits)?);
        Ok(())
    })
    .map_err(|why| {
        format!(
            "{} of the Git repository {name} cannot be installed safely: {why}",
            capitalized(&revision.describe())
        )
    })?;
    let (subject, lfs_pointers) = written.expect("written when the download was made");
    Ok(Fetched {
        download,
        origin: GitOrigin {
            repository: spec.repository.clone(),
            revision,
            advertised,
            subject,
            lfs_pointers,
        },
    })
}

/// A repository's server, which speaks protocol version 2.
struct Remote<'a> {
    repository: &'a Repository,
    /// Whether the server fetches without history (`deepen`).
    shallow: bool,
    /// Whether the server names its object format, which Pane then names
    /// back (always `sha1`).
    object_format: bool,
}

/// A pkt-line: `len` in four hexadecimal digits, counting itself.
fn pkt(line: &str) -> Vec<u8> {
    let mut out = format!("{:04x}", line.len() + 4).into_bytes();
    out.extend_from_slice(line.as_bytes());
    out
}

const FLUSH: &[u8] = b"0000";
const DELIM: &[u8] = b"0001";

/// One pkt-line read.
#[derive(Debug, PartialEq, Eq)]
enum Pkt<'a> {
    Data(&'a [u8]),
    Flush,
    Delim,
    End,
}

/// Reads pkt-lines from an answer.
struct PktReader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> PktReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        PktReader { data, at: 0 }
    }

    /// The next pkt-line; `None` at the end of the answer.
    fn next(&mut self) -> Result<Option<Pkt<'a>>, String> {
        if self.at == self.data.len() {
            return Ok(None);
        }
        let head = self
            .data
            .get(self.at..self.at + 4)
            .ok_or("the answer ends inside a pkt-line")?;
        let len = std::str::from_utf8(head)
            .ok()
            .and_then(|head| usize::from_str_radix(head, 16).ok())
            .ok_or("the answer is not made of pkt-lines")?;
        self.at += 4;
        Ok(Some(match len {
            0 => Pkt::Flush,
            1 => Pkt::Delim,
            2 => Pkt::End,
            3 => return Err("the answer has a damaged pkt-line".into()),
            len => {
                let data = self
                    .data
                    .get(self.at..self.at + len - 4)
                    .ok_or("the answer ends inside a pkt-line")?;
                self.at += len - 4;
                Pkt::Data(data)
            }
        }))
    }

    /// The next line of text (without its `\n`), or `None` at a flush or a
    /// delimiter (returned as `Err(Some(pkt))`).
    fn line(&mut self) -> Result<Result<&'a str, Option<Pkt<'a>>>, String> {
        match self.next()? {
            Some(Pkt::Data(data)) => {
                let text = std::str::from_utf8(data)
                    .map_err(|_| "the answer has a line that is not UTF-8")?;
                let text = text.strip_suffix('\n').unwrap_or(text);
                if let Some(message) = text.strip_prefix("ERR ") {
                    return Err(format!("the server refused: {}", shown(message)));
                }
                Ok(Ok(text))
            }
            other => Ok(Err(other)),
        }
    }
}

impl<'a> Remote<'a> {
    /// Asks the server of `repository` for its capabilities.
    fn connect(repository: &'a Repository) -> Result<Remote<'a>, String> {
        let name = repository.name();
        let url = format!("{}/info/refs?service=git-upload-pack", repository.url);
        let answer = get(repository, &url, &[], MAX_REFS)?;
        let mut reader = PktReader::new(&answer.body);
        let not_v2 = || {
            format!(
                "The Git repository {name} is not served with Git's protocol version 2 over \
                 HTTPS, which Pane needs (its server answered {url} with something else)"
            )
        };
        let mut first = reader.line().map_err(|_| not_v2())?;
        // Some servers announce the service first, as for version 0.
        if matches!(first, Ok(line) if line.starts_with("# service=")) {
            if reader.next().map_err(|_| not_v2())? != Some(Pkt::Flush) {
                return Err(not_v2());
            }
            first = reader.line().map_err(|_| not_v2())?;
        }
        if first != Ok("version 2") {
            return Err(not_v2());
        }
        let mut shallow = false;
        let mut object_format = false;
        let mut fetch = false;
        let mut ls_refs = false;
        loop {
            let capability = match reader.line().map_err(|_| not_v2())? {
                Ok(capability) => capability,
                Err(Some(Pkt::Flush)) | Err(None) => break,
                Err(_) => return Err(not_v2()),
            };
            let (key, value) = capability.split_once('=').unwrap_or((capability, ""));
            match key {
                "ls-refs" => ls_refs = true,
                "fetch" => {
                    fetch = true;
                    shallow = value.split(' ').any(|feature| feature == "shallow");
                }
                "object-format" => {
                    if value != "sha1" {
                        return Err(format!(
                            "The Git repository {name} uses {} object ids; Pane reads only \
                             repositories with SHA-1 ids",
                            shown(value)
                        ));
                    }
                    object_format = true;
                }
                _ => {}
            }
        }
        if !(fetch && ls_refs) {
            return Err(not_v2());
        }
        Ok(Remote {
            repository,
            shallow,
            object_format,
        })
    }

    /// Sends the protocol version 2 command `command` with `arguments`
    /// and returns the answer.
    fn command(&self, command: &str, arguments: &[String], most: u64) -> Result<Vec<u8>, String> {
        let mut body = pkt(&format!("command={command}\n"));
        if self.object_format {
            body.extend(pkt("object-format=sha1\n"));
        }
        body.extend_from_slice(DELIM);
        for argument in arguments {
            body.extend(pkt(&format!("{argument}\n")));
        }
        body.extend_from_slice(FLUSH);
        let url = format!("{}/git-upload-pack", self.repository.url);
        let answer = post(self.repository, &url, body, most)?;
        Ok(answer.body)
    }

    /// Resolves `reference` (none: the default branch) to the revision to
    /// fetch, and says whether the repository's default branch, one of its
    /// branches or one of its tags points to its commit (always, but for a
    /// commit named by its id).
    fn resolve(&self, reference: Option<&str>) -> Result<(GitRevision, bool), String> {
        if let Some(commit) = reference.filter(|r| is_commit_id(r)) {
            let commit = commit.to_ascii_lowercase();
            // A host sharing storage between forks serves a fork's commit
            // too: see whether this repository's own references name it.
            // A listing too long to read (as for a repository with very
            // many tags) is taken as naming none: cautioned about, not
            // refused, since the commit itself is still checked.
            let advertised = self
                .list_refs(&["HEAD", "refs/heads/", "refs/tags/"])?
                .is_some_and(|found| found.iter().any(|(_, id, _)| *id == commit));
            return Ok((
                GitRevision {
                    reference: GitRef::Commit,
                    commit,
                },
                advertised,
            ));
        }
        let name = self.repository.name();
        let (heads, tags) = match reference {
            None => (None, None),
            Some(reference) => match (
                reference.strip_prefix("refs/heads/"),
                reference.strip_prefix("refs/tags/"),
            ) {
                (Some(branch), _) => (Some(format!("refs/heads/{branch}")), None),
                (_, Some(tag)) => (None, Some(format!("refs/tags/{tag}"))),
                _ => (
                    Some(format!("refs/heads/{reference}")),
                    Some(format!("refs/tags/{reference}")),
                ),
            },
        };
        let prefixes: Vec<&str> = match reference {
            None => vec!["HEAD"],
            Some(_) => heads.iter().chain(&tags).map(String::as_str).collect(),
        };
        let found = self
            .list_refs(&prefixes)?
            .ok_or_else(|| format!("Could not list the references of {name}: {TOO_LARGE}"))?;
        let find = |wanted: &Option<String>| {
            wanted.as_ref().and_then(|wanted| {
                found
                    .iter()
                    .find(|(name, ..)| name == wanted)
                    .map(|(_, commit, _)| commit.clone())
            })
        };
        let Some(reference) = reference else {
            let Some((_, commit, target)) = found.iter().find(|(name, ..)| name == "HEAD") else {
                return Err(format!(
                    "The Git repository {name} has no default branch; name the branch, tag or \
                     commit to install, such as {name}@v1.0.0"
                ));
            };
            let branch = target
                .as_deref()
                .and_then(|target| target.strip_prefix("refs/heads/"))
                .map(ToOwned::to_owned);
            return Ok((
                GitRevision {
                    reference: GitRef::Default { branch },
                    commit: commit.clone(),
                },
                true,
            ));
        };
        let short = |full: &Option<String>, prefix: &str| {
            full.as_deref()
                .and_then(|full| full.strip_prefix(prefix))
                .map(ToOwned::to_owned)
        };
        let revision = match (find(&heads), find(&tags)) {
            (Some(_), Some(_)) => Err(format!(
                "The Git repository {name} has both a branch and a tag named {reference}; name \
                 the one to install as {name}@refs/heads/{reference} or {name}@refs/tags/{reference}"
            )),
            (Some(commit), None) => Ok(GitRevision {
                reference: GitRef::Branch(short(&heads, "refs/heads/").unwrap_or_default()),
                commit,
            }),
            (None, Some(commit)) => Ok(GitRevision {
                reference: GitRef::Tag(short(&tags, "refs/tags/").unwrap_or_default()),
                commit,
            }),
            (None, None) => Err(format!(
                "The Git repository {name} has no branch or tag named {reference}"
            )),
        }?;
        Ok((revision, true))
    }

    /// The references whose names start with one of `prefixes` (`ls-refs`,
    /// peeled): each one's name, the commit it points to and, for a
    /// symbolic one such as `HEAD`, the reference it points to; `None` when
    /// the listing is longer than the [`MAX_REFS`] Pane reads.
    fn list_refs(&self, prefixes: &[&str]) -> Result<Option<Vec<Listed>>, String> {
        let name = self.repository.name();
        let mut arguments = vec!["symrefs".to_owned(), "peel".to_owned()];
        for prefix in prefixes {
            arguments.push(format!("ref-prefix {prefix}"));
        }
        let answer = match self.command("ls-refs", &arguments, MAX_REFS) {
            Ok(answer) => answer,
            Err(why) if why == TOO_LARGE => return Ok(None),
            Err(why) => return Err(format!("Could not list the references of {name}: {why}")),
        };
        let damaged =
            |why: &str| format!("The Git repository {name} listed its references wrongly: {why}");
        let mut reader = PktReader::new(&answer);
        // (name, commit, what HEAD points to)
        let mut found: Vec<Listed> = Vec::new();
        loop {
            let line = match reader.line().map_err(|why| damaged(&why))? {
                Ok(line) => line,
                Err(Some(Pkt::Flush)) | Err(None) => break,
                Err(_) => return Err(damaged("an unexpected delimiter")),
            };
            let mut parts = line.split(' ');
            let (Some(id), Some(ref_name)) = (parts.next(), parts.next()) else {
                return Err(damaged(&format!("`{}`", shown(line))));
            };
            if !is_commit_id(id) {
                // An unborn HEAD ("unborn HEAD symref-target:…") has none.
                continue;
            }
            let mut commit = id.to_ascii_lowercase();
            let mut target = None;
            for attribute in parts {
                if let Some(peeled) = attribute.strip_prefix("peeled:")
                    && is_commit_id(peeled)
                {
                    commit = peeled.to_ascii_lowercase();
                }
                if let Some(symref) = attribute.strip_prefix("symref-target:") {
                    target = Some(shown(symref));
                }
            }
            found.push((ref_name.to_owned(), commit, target));
        }
        Ok(Some(found))
    }

    /// Fetches the one commit `commit`, without its history, as a pack.
    fn fetch(&self, commit: &str, limits: Limits) -> Result<Vec<u8>, String> {
        let name = self.repository.name();
        let mut arguments = vec![format!("want {commit}")];
        if self.shallow {
            arguments.push("deepen 1".into());
        }
        arguments.push("no-progress".into());
        arguments.push("done".into());
        let answer = self
            .command("fetch", &arguments, limits.pack)
            .map_err(|why| match why.as_str() {
                TOO_LARGE => format!(
                    "Commit {commit} of the Git repository {name} is larger than the {} MiB \
                     Pane downloads",
                    limits.pack >> 20
                ),
                _ => format!("Could not fetch commit {commit} of {name}: {why}"),
            })?;
        pack_in(&answer).map_err(|no_pack| match no_pack {
            NoPack::Failed(why) => format!("Could not fetch commit {commit} of {name}: {why}"),
            NoPack::Damaged(why) => {
                format!("The Git repository {name} answered the fetch wrongly: {why}")
            }
        })
    }
}

/// Why the answer to a fetch holds no pack.
#[derive(Debug, PartialEq, Eq)]
enum NoPack {
    /// The server refused (`ERR`) or failed (on band 3), saying why.
    Failed(String),
    /// The answer is not as the protocol has it.
    Damaged(String),
}

/// The pack in `answer`, the answer to a fetch: past the sections before
/// it (`shallow-info`, …), its lines on band 1 put together, in a buffer
/// the size of the answer, which holds all of it.
fn pack_in(answer: &[u8]) -> Result<Vec<u8>, NoPack> {
    let damaged = |why: &str| NoPack::Damaged(why.into());
    let mut reader = PktReader::new(answer);
    // Sections before the pack, each ended by a delimiter.
    loop {
        let section = match reader.line() {
            Ok(Ok(section)) => section,
            Ok(Err(_)) => return Err(damaged("it sent no pack")),
            Err(why) => return Err(NoPack::Failed(why)),
        };
        if section == "packfile" {
            break;
        }
        loop {
            match reader.line().map_err(|why| damaged(&why))? {
                Ok(_) => {}
                Err(Some(Pkt::Delim)) => break,
                Err(_) => return Err(damaged("it sent no pack")),
            }
        }
    }
    // Reserved at once rather than doubled as it grows, which could hold
    // twice the answer.
    let mut pack = Vec::with_capacity(answer.len());
    loop {
        match reader.next().map_err(|why| damaged(&why))? {
            Some(Pkt::Data(data)) => match data.split_first() {
                Some((1, data)) => pack.extend_from_slice(data),
                Some((2, _)) => {}
                Some((3, message)) => {
                    return Err(NoPack::Failed(format!(
                        "the server failed: {}",
                        shown_bytes(message.trim_ascii_end())
                    )));
                }
                _ => return Err(damaged("a pack line on no known band")),
            },
            Some(Pkt::Flush) | None => break,
            Some(_) => return Err(damaged("an unexpected delimiter in the pack")),
        }
    }
    Ok(pack)
}

/// A reference as `ls-refs` lists it: its name, the commit it points to
/// (peeled) and, for a symbolic one such as `HEAD`, the reference it points
/// to.
type Listed = (String, String, Option<String>);

/// The error text of an answer larger than asked for.
const TOO_LARGE: &str = "its answer is too large";

/// Explains an answer that is not `200`.
fn answered(repository: &Repository, url: &str, answer: Answer) -> Result<Answer, String> {
    let name = repository.name();
    match answer.status {
        200 => Ok(answer),
        401 | 403 => Err(format!(
            "The Git repository {name} asks to sign in (its server answered {}): Pane sends no \
             credentials, so it installs only from public repositories",
            answer.status
        )),
        404 => Err(format!("There is no Git repository at {}", repository.url)),
        status @ 300..=399 => Err(format!(
            "{url} answered {status}, sending Pane elsewhere: Pane follows no redirect, so name \
             the repository by the address it moved to"
        )),
        status => Err(format!(
            "The Git repository {name} answered {status} for {url}"
        )),
    }
}

fn unreachable(repository: &Repository, error: GetError) -> String {
    match error {
        GetError::TooLarge => TOO_LARGE.into(),
        // Its text may quote the server.
        GetError::Failed(why) => format!(
            "Could not reach the Git repository {}: {}",
            repository.name(),
            shown(&why)
        ),
    }
}

/// Whether `why` is the text of a connection failure this client met —
/// connecting, listing the references or fetching the pack — wherever it
/// wrapped one: `unreachable` builds the wording, and the fetch wraps it
/// again as "Could not fetch commit … of …". Acquiring a default
/// extension retries one of these where a revision the repository
/// refused is explained once (crate::defaults), so the classification
/// lives here, beside the wordings it reads: a reword of either changes
/// this with them, and the test below pins both forms.
pub(crate) fn is_connection_failure(why: &str) -> bool {
    why.contains("Could not reach the Git repository")
}

fn get(
    repository: &Repository,
    url: &str,
    headers: &[(&str, &str)],
    most: u64,
) -> Result<Answer, String> {
    crate::http::https_only(repository.loopback, url)?;
    let mut all = vec![("Git-Protocol", "version=2"), ("User-Agent", USER_AGENT)];
    all.extend_from_slice(headers);
    let answer = crate::http::get_blocking(url, &all, most, crate::http::OWN_LIMITS)
        .map_err(|error| unreachable(repository, error))?;
    answered(repository, url, answer)
}

fn post(repository: &Repository, url: &str, body: Vec<u8>, most: u64) -> Result<Answer, String> {
    crate::http::https_only(repository.loopback, url)?;
    let headers = [
        ("Git-Protocol", "version=2"),
        ("User-Agent", USER_AGENT),
        ("Content-Type", "application/x-git-upload-pack-request"),
        ("Accept", "application/x-git-upload-pack-result"),
    ];
    let answer = crate::http::post_blocking(url, &headers, body, most, crate::http::OWN_LIMITS)
        .map_err(|error| unreachable(repository, error))?;
    answered(repository, url, answer)
}

/// The kinds of Git objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Commit,
    Tree,
    Blob,
    Tag,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Commit => "commit",
            Kind::Tree => "tree",
            Kind::Blob => "blob",
            Kind::Tag => "tag",
        }
    }
}

/// An object id: 20 bytes.
type Id = [u8; 20];

#[cfg(test)]
use crate::integrity::hex;

fn parse_hex(text: &str) -> Option<Id> {
    if !is_commit_id(text) {
        return None;
    }
    let mut id = [0; 20];
    for (i, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(id)
}

/// The SHA-1 of `parts`, or why it cannot be trusted: a collision attack
/// detected, as Git itself detects one.
fn sha1(parts: &[&[u8]]) -> Result<Id, String> {
    let mut hasher = Sha1::new();
    for part in parts {
        hasher.update(part);
    }
    let result = hasher.try_finalize();
    if result.has_collision() {
        return Err("it holds data crafted to collide under SHA-1".into());
    }
    Ok((*result.hash()).into())
}

/// The id of an object of `kind` holding `data`.
fn object_id(kind: Kind, data: &[u8]) -> Result<Id, String> {
    let header = format!("{} {}\0", kind.name(), data.len());
    sha1(&[header.as_bytes(), data])
}

/// One entry of a pack as read, before its deltas are resolved.
enum Stored {
    Whole(Kind, Vec<u8>),
    /// A delta against the entry at this offset of the pack.
    OffsetDelta(usize, Vec<u8>),
    /// A delta against the object with this id.
    RefDelta(Id, Vec<u8>),
}

/// The objects of a pack, by id, each checked against its id.
type Objects = HashMap<Id, (Kind, Rc<Vec<u8>>)>;

/// Reads the pack `pack` (version 2 or 3): checks its checksum, inflates
/// each entry, resolves its deltas and computes each object's id.
fn read_pack(pack: &[u8], limits: Limits) -> Result<Objects, String> {
    if pack.len() < 32 || &pack[..4] != b"PACK" {
        return Err("it is not a Git pack".into());
    }
    let version = u32::from_be_bytes(pack[4..8].try_into().expect("four bytes"));
    if version != 2 && version != 3 {
        return Err(format!("it is a pack of version {version}"));
    }
    let count = u32::from_be_bytes(pack[8..12].try_into().expect("four bytes")) as usize;
    if count > limits.objects {
        return Err(format!(
            "it holds {count} objects; Pane reads at most {}",
            limits.objects
        ));
    }
    let (body, checksum) = pack.split_at(pack.len() - 20);
    if sha1(&[body])? != checksum {
        return Err("its checksum does not match its contents".into());
    }
    let mut at = 12;
    let mut stored: Vec<(usize, Stored)> = Vec::with_capacity(count);
    let mut budget = Budget {
        used: 0,
        most: limits.inflated,
    };
    for _ in 0..count {
        let offset = at;
        let byte = |at: &mut usize| -> Result<u8, String> {
            let byte = *body.get(*at).ok_or("it ends inside an entry")?;
            *at += 1;
            Ok(byte)
        };
        let mut c = byte(&mut at)?;
        let kind = (c >> 4) & 7;
        let mut size = u64::from(c & 15);
        let mut shift = 4;
        while c & 0x80 != 0 {
            c = byte(&mut at)?;
            if shift > 57 {
                return Err("it has an entry of an impossible size".into());
            }
            size |= u64::from(c & 0x7f) << shift;
            shift += 7;
        }
        let base = match kind {
            6 => {
                let mut c = byte(&mut at)?;
                let mut distance = u64::from(c & 0x7f);
                while c & 0x80 != 0 {
                    c = byte(&mut at)?;
                    distance = distance
                        .checked_add(1)
                        .and_then(|d| d.checked_mul(128))
                        .ok_or("it has a delta with an impossible base")?
                        | u64::from(c & 0x7f);
                }
                let base = (offset as u64)
                    .checked_sub(distance)
                    .filter(|_| distance > 0)
                    .ok_or("it has a delta with an impossible base")?;
                Some(Err(base as usize))
            }
            7 => {
                let id: Id = body
                    .get(at..at + 20)
                    .ok_or("it ends inside an entry")?
                    .try_into()
                    .expect("twenty bytes");
                at += 20;
                Some(Ok(id))
            }
            _ => None,
        };
        let most = limits.unpacked.max(1 << 20);
        if size > most {
            return Err(format!(
                "it has an object of {size} bytes, more than the {} MiB Pane takes",
                most >> 20
            ));
        }
        budget.take(size)?;
        let (data, used) = inflate(&body[at..], size)?;
        at += used;
        let entry = match (kind, base) {
            (1, None) => Stored::Whole(Kind::Commit, data),
            (2, None) => Stored::Whole(Kind::Tree, data),
            (3, None) => Stored::Whole(Kind::Blob, data),
            (4, None) => Stored::Whole(Kind::Tag, data),
            (6, Some(Err(base))) => Stored::OffsetDelta(base, data),
            (7, Some(Ok(id))) => Stored::RefDelta(id, data),
            (kind, _) => return Err(format!("it has an entry of unknown type {kind}")),
        };
        stored.push((offset, entry));
    }
    if at != body.len() {
        return Err("it has data after its last entry".into());
    }
    resolve(stored, limits, budget)
}

/// Inflates the zlib stream at the start of `input`, which must hold
/// exactly `size` bytes; returns them and how much of `input` it took.
/// Its buffer doubles as it fills, never past `size`, which is what the
/// budget counts, but for one byte that finds a stream longer than `size`.
fn inflate(input: &[u8], size: u64) -> Result<(Vec<u8>, usize), String> {
    let mut inflater = flate2::Decompress::new(true);
    let mut out = Vec::with_capacity(size.min(1 << 20) as usize);
    loop {
        if out.len() == out.capacity() {
            let rest = size as usize - out.len();
            out.reserve_exact(rest.min(out.len().max(1 << 20)).max(1));
        }
        let consumed = inflater.total_in() as usize;
        let status = inflater
            .decompress_vec(&input[consumed..], &mut out, flate2::FlushDecompress::None)
            .map_err(|_| "it has a damaged zlib stream")?;
        if out.len() as u64 > size {
            return Err("it has an entry larger than its header says".into());
        }
        match status {
            flate2::Status::StreamEnd => break,
            _ if inflater.total_in() as usize == consumed && out.len() < out.capacity() => {
                return Err("it ends inside an entry".into());
            }
            _ => {}
        }
    }
    if out.len() as u64 != size {
        return Err("it has an entry smaller than its header says".into());
    }
    Ok((out, inflater.total_in() as usize))
}

/// Resolves the deltas of `stored` and computes every object's id, within
/// `budget` ([`Limits::inflated`]), which already counts the entries as
/// inflated.
///
/// Each delta waits on its base: an offset delta on the entry at that
/// offset, a delta against an id on the object with that id, whichever
/// entry makes it. Starting from the whole objects, each object made is
/// taken from a work list, and the deltas waiting on it are applied and
/// added to the list: every delta is applied once, in time linear in the
/// pack, without recursion however long its chain. A delta's instructions
/// are let go once it is applied.
fn resolve(
    mut stored: Vec<(usize, Stored)>,
    limits: Limits,
    mut budget: Budget,
) -> Result<Objects, String> {
    let index: HashMap<usize, usize> = stored
        .iter()
        .enumerate()
        .map(|(i, (offset, _))| (*offset, i))
        .collect();
    // The deltas waiting on each entry (offset deltas) and on each id.
    let mut on_entry: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut on_id: HashMap<Id, Vec<usize>> = HashMap::new();
    // (entry, its kind, its contents, the deltas it took to make it)
    let mut work: Vec<(usize, Kind, Rc<Vec<u8>>, usize)> = Vec::new();
    let mut objects = Objects::new();
    let mut unresolved = 0;
    for (i, (_, entry)) in stored.iter_mut().enumerate() {
        match entry {
            // Moved rather than copied: already counted as inflated.
            Stored::Whole(kind, data) => {
                work.push((i, *kind, Rc::new(std::mem::take(data)), 0));
            }
            Stored::OffsetDelta(offset, _) => {
                let base = *index.get(offset).ok_or("it has a delta with no base")?;
                on_entry.entry(base).or_default().push(i);
                unresolved += 1;
            }
            Stored::RefDelta(id, _) => {
                on_id.entry(*id).or_default().push(i);
                unresolved += 1;
            }
        }
    }
    let most = limits.unpacked.max(1 << 20);
    while let Some((i, kind, data, chain)) = work.pop() {
        let id = object_id(kind, &data)?;
        let waiting = on_entry
            .remove(&i)
            .into_iter()
            .chain(on_id.remove(&id))
            .flatten();
        for delta_at in waiting {
            if chain + 1 > MAX_DELTA_CHAIN {
                return Err("it has a chain of deltas too long to follow".into());
            }
            let (Stored::OffsetDelta(_, delta) | Stored::RefDelta(_, delta)) =
                &mut stored[delta_at].1
            else {
                unreachable!("only deltas wait on a base");
            };
            let delta = std::mem::take(delta);
            let target = delta_target(&delta, most)?;
            budget.take(target)?;
            let made = apply_delta(&data, &delta, most)?;
            budget.give_back(delta.len() as u64);
            drop(delta);
            unresolved -= 1;
            work.push((delta_at, kind, Rc::new(made), chain + 1));
        }
        objects.insert(id, (kind, data));
    }
    if unresolved > 0 {
        return Err("it has a delta whose base it does not hold".into());
    }
    Ok(objects)
}

/// The size of what the Git delta `delta` makes, if at most `most` bytes.
fn delta_target(delta: &[u8], most: u64) -> Result<u64, String> {
    let (_, target, _) = delta_sizes(delta)?;
    if target > most {
        return Err(format!(
            "it has an object of {target} bytes, more than the {} MiB Pane takes",
            most >> 20
        ));
    }
    Ok(target)
}

/// The sizes a Git delta starts with, of its base and of what it makes,
/// and where its instructions start.
fn delta_sizes(delta: &[u8]) -> Result<(u64, u64, usize), String> {
    let damaged = || "it has a damaged delta".to_owned();
    let mut at = 0;
    let mut varint = || -> Result<u64, String> {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let byte = *delta.get(at).ok_or_else(damaged)?;
            at += 1;
            if shift > 57 {
                return Err(damaged());
            }
            value |= u64::from(byte & 0x7f) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
    };
    let source = varint()?;
    let target = varint()?;
    Ok((source, target, at))
}

/// Applies the Git delta `delta` to `base`, making at most `most` bytes.
fn apply_delta(base: &[u8], delta: &[u8], most: u64) -> Result<Vec<u8>, String> {
    let damaged = || "it has a damaged delta".to_owned();
    let (source, _, mut at) = delta_sizes(delta)?;
    let target = delta_target(delta, most)?;
    if source != base.len() as u64 {
        return Err(damaged());
    }
    let mut out = Vec::with_capacity(target as usize);
    while at < delta.len() {
        let op = delta[at];
        at += 1;
        // What the instruction adds: a part of the base, or its own bytes.
        let piece = if op & 0x80 != 0 {
            let mut offset = 0usize;
            let mut size = 0usize;
            for bit in 0..4 {
                if op & (1 << bit) != 0 {
                    offset |= usize::from(*delta.get(at).ok_or_else(damaged)?) << (8 * bit);
                    at += 1;
                }
            }
            for bit in 0..3 {
                if op & (0x10 << bit) != 0 {
                    size |= usize::from(*delta.get(at).ok_or_else(damaged)?) << (8 * bit);
                    at += 1;
                }
            }
            if size == 0 {
                size = 0x10000;
            }
            base.get(offset..offset.checked_add(size).ok_or_else(damaged)?)
                .ok_or_else(damaged)?
        } else if op != 0 {
            let inserted = delta.get(at..at + usize::from(op)).ok_or_else(damaged)?;
            at += usize::from(op);
            inserted
        } else {
            return Err(damaged());
        };
        // Before it is added: a copy may be 16 MiB past what the budget
        // counted for the object.
        if (out.len() + piece.len()) as u64 > target {
            return Err(damaged());
        }
        out.extend_from_slice(piece);
    }
    if out.len() as u64 != target {
        return Err(damaged());
    }
    Ok(out)
}

/// What checking out a tree counts.
struct Tally {
    entries: usize,
    bytes: u64,
    lfs_pointers: Vec<String>,
}

/// Writes the tree of commit `commit` from `objects` into `dest`, which
/// must not exist, as [`fetch`] describes; returns the first line of the
/// commit's message and the files that are Git LFS pointers.
fn check_out(
    objects: &Objects,
    commit: &str,
    dest: &Path,
    limits: Limits,
) -> Result<(String, Vec<String>), String> {
    let id = parse_hex(commit).ok_or("it is not a commit id")?;
    let (kind, data) = objects
        .get(&id)
        .ok_or("the server did not send that commit")?;
    if *kind != Kind::Commit {
        return Err(format!("it is a {}, not a commit", kind.name()));
    }
    // Read as bytes, not decoded whole: a message may be 256 MiB that is
    // not UTF-8, which decoded would take three times as much.
    let tree = data
        .split(|&byte| byte == b'\n')
        .next()
        .and_then(|line| line.strip_prefix(b"tree "))
        .and_then(|id| std::str::from_utf8(id).ok())
        .and_then(parse_hex)
        .ok_or("its commit names no tree")?;
    let subject = data
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .map(|end| {
            let message = &data[end + 2..];
            let first = message
                .split(|&byte| byte == b'\n')
                .next()
                .unwrap_or_default();
            shown_bytes(first.trim_ascii())
        })
        .unwrap_or_default();
    fs::create_dir(dest).map_err(|error| error.to_string())?;
    let mut tally = Tally {
        entries: 0,
        bytes: 0,
        lfs_pointers: Vec::new(),
    };
    write_tree(objects, &tree, dest, "", 0, limits, &mut tally)?;
    Ok((subject, tally.lfs_pointers))
}

/// Whether a system may read `name` as `.git`: in any case; as `git~1`,
/// its short name on Windows file systems; or on HFS+, which ignores some
/// format characters in a name, with them (as Git's own `is_hfs_dotgit`
/// checks). A trailing dot or space, which Windows drops, and `:`, which
/// names an NTFS stream, are refused of every name already.
fn is_dot_git(name: &str) -> bool {
    let ignored_by_hfs = |c: char| {
        matches!(
            c,
            '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{206a}'..='\u{206f}' | '\u{feff}'
        )
    };
    let hfs: String = name.chars().filter(|&c| !ignored_by_hfs(c)).collect();
    hfs.eq_ignore_ascii_case(".git") || name.eq_ignore_ascii_case("git~1")
}

/// The start of a Git LFS pointer file.
const LFS_POINTER: &[u8] = b"version https://git-lfs.github.com/spec/";

fn write_tree(
    objects: &Objects,
    tree: &Id,
    dest: &Path,
    prefix: &str,
    depth: usize,
    limits: Limits,
    tally: &mut Tally,
) -> Result<(), String> {
    let (kind, data) = objects
        .get(tree)
        .ok_or("the server did not send all of its tree")?;
    if *kind != Kind::Tree {
        return Err("its tree is damaged".into());
    }
    let mut seen: Vec<String> = Vec::new();
    let mut rest = &data[..];
    while !rest.is_empty() {
        let space = rest
            .iter()
            .position(|&b| b == b' ')
            .ok_or("its tree is damaged")?;
        let mode = std::str::from_utf8(&rest[..space]).map_err(|_| "its tree is damaged")?;
        rest = &rest[space + 1..];
        let nul = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or("its tree is damaged")?;
        let raw_name = &rest[..nul];
        rest = &rest[nul + 1..];
        let id: Id = rest
            .get(..20)
            .ok_or("its tree is damaged")?
            .try_into()
            .expect("twenty bytes");
        rest = &rest[20..];
        // Shown from its start only: a name is checked, and a longer one
        // refused, before any more of it is read.
        let shown = format!("{prefix}{}", shown_bytes(raw_name));
        let name = std::str::from_utf8(raw_name)
            .map_err(|_| format!("its tree contains `{shown}`, whose name is not valid UTF-8"))?;
        let refuse = |why: &str| {
            Err(format!(
                "its tree contains `{shown}`, {why}; Pane takes only files and folders every \
                 system can write"
            ))
        };
        if let Err(why) = check_part(name) {
            return refuse(why);
        }
        if is_dot_git(name) {
            return refuse("a `.git` entry, which Git itself refuses to check out");
        }
        let folded: String = name.chars().flat_map(char::to_lowercase).collect();
        if seen.contains(&folded) {
            return refuse(
                "whose name differs only in case from another in its folder, which some systems \
                 cannot hold both of",
            );
        }
        seen.push(folded);
        tally.entries += 1;
        if tally.entries > limits.entries {
            return Err(format!(
                "it holds more than {} files and folders",
                limits.entries
            ));
        }
        let path = dest.join(name);
        match mode {
            "40000" | "040000" => {
                if depth + 1 > limits.depth {
                    return Err(format!(
                        "its folders nest more than {} deep, at `{shown}`",
                        limits.depth
                    ));
                }
                fs::create_dir(&path).map_err(|error| format!("`{shown}`: {error}"))?;
                write_tree(
                    objects,
                    &id,
                    &path,
                    &format!("{shown}/"),
                    depth + 1,
                    limits,
                    tally,
                )?;
            }
            "100644" | "100755" | "100664" => {
                let (kind, contents) = objects
                    .get(&id)
                    .ok_or_else(|| format!("the server did not send `{shown}`"))?;
                if *kind != Kind::Blob {
                    return Err(format!("its tree has a damaged entry `{shown}`"));
                }
                tally.bytes += contents.len() as u64;
                if tally.bytes > limits.unpacked {
                    return Err(format!(
                        "its files take more than the {} MiB Pane allows",
                        limits.unpacked >> 20
                    ));
                }
                if contents.starts_with(LFS_POINTER) {
                    tally.lfs_pointers.push(shown.clone());
                }
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|error| format!("`{shown}`: {error}"))?;
                file.write_all(contents)
                    .map_err(|error| format!("`{shown}`: {error}"))?;
            }
            "120000" => return refuse("a symbolic link"),
            "160000" => return refuse("a submodule, which Pane does not fetch"),
            other => return refuse(&format!("an entry of mode {}", self::shown(other))),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(text: &str) -> GitSpec {
        GitSpec::parse(text).unwrap_or_else(|why| panic!("{text}: {why}"))
    }

    #[test]
    fn equivalent_addresses_name_one_repository() {
        let forms = [
            "https://github.com/Owner/Repo",
            "https://github.com/Owner/Repo.git",
            "https://GitHub.com/Owner/Repo/",
            "https://github.com:443/Owner/Repo.git/",
            "git+https://github.com/Owner/Repo.git",
            "git:github.com/Owner/Repo",
            "github.com/Owner/Repo",
            "ssh://git@github.com/Owner/Repo.git",
            "ssh://git@github.com:22/Owner/Repo",
            "git@github.com:Owner/Repo.git",
            "git+ssh://git@github.com/Owner/Repo",
        ];
        for form in forms {
            let parsed = spec(form);
            assert_eq!(parsed.repository.name(), "github.com/owner/repo", "{form}");
            assert_eq!(parsed.reference, None, "{form}");
            // Fetched as written.
            assert!(
                parsed
                    .repository
                    .url()
                    .starts_with("https://github.com/Owner/Repo")
            );
        }
        // Another port is another server.
        assert_eq!(
            spec("https://git.example.org:8443/a/b").repository.name(),
            "git.example.org:8443/a/b"
        );
        // Fetched over HTTPS, with `.git` as written.
        assert_eq!(
            spec("git@github.com:o/r.git").repository.url(),
            "https://github.com/o/r.git"
        );
        assert_eq!(
            spec("github.com/o/r").repository.url(),
            "https://github.com/o/r"
        );
    }

    #[test]
    fn hosts_that_ignore_case_name_one_repository_in_any_case_and_others_keep_it() {
        // GitHub, GitLab, Bitbucket and Codeberg serve a repository at any
        // case of its path: one package, fetched as written.
        for host in ["github.com", "gitlab.com", "bitbucket.org", "codeberg.org"] {
            let forms = [
                format!("https://{host}/Owner/Repo"),
                format!("https://{host}/owner/repo"),
                format!("https://{host}/OWNER/REPO.GIT"),
                format!("git@{}:Owner/Repo.Git", host.to_uppercase()),
            ];
            for form in &forms {
                let parsed = spec(form);
                assert_eq!(
                    parsed.repository.name(),
                    format!("{host}/owner/repo"),
                    "{form}"
                );
            }
            assert_eq!(
                spec(&forms[2]).repository.url(),
                format!("https://{host}/OWNER/REPO.GIT")
            );
            // A reference keeps its case: branches and tags are not folded.
            assert_eq!(
                spec(&format!("{host}/O/R@Release")).reference.as_deref(),
                Some("Release")
            );
        }
        // Any other host, or one of those on another port, keeps its case.
        assert_eq!(
            spec("https://git.example.org/Owner/Repo").repository.name(),
            "git.example.org/Owner/Repo"
        );
        assert_eq!(
            spec("https://github.com:8443/Owner/Repo").repository.name(),
            "github.com:8443/Owner/Repo"
        );
        assert_eq!(
            spec("https://gitlab.example.com/Owner/Repo.GIT")
                .repository
                .name(),
            "gitlab.example.com/Owner/Repo.GIT"
        );
    }

    #[test]
    fn an_ssh_address_is_known_to_be_fetched_over_https() {
        for form in [
            "git@github.com:o/r.git",
            "ssh://git@github.com/o/r",
            "git+ssh://git@github.com/o/r",
            "git:ssh://git@github.com:22/o/r",
        ] {
            assert!(spec(form).repository.written_as_ssh(), "{form}");
        }
        for form in [
            "https://github.com/o/r",
            "github.com/o/r",
            "git:github.com/o/r",
        ] {
            assert!(!spec(form).repository.written_as_ssh(), "{form}");
        }
    }

    #[test]
    fn a_reference_follows_the_repository_after_an_at_sign() {
        let parsed = spec("https://github.com/o/r.git@v1.2.3");
        assert_eq!(parsed.repository.name(), "github.com/o/r");
        assert_eq!(parsed.reference.as_deref(), Some("v1.2.3"));
        assert_eq!(parsed.to_string(), "github.com/o/r@v1.2.3");
        assert_eq!(
            spec("git@github.com:o/r@release/1.x").reference.as_deref(),
            Some("release/1.x")
        );
        assert_eq!(
            spec("github.com/o/r@ABCDEF0123456789ABCDEF0123456789ABCDEF01")
                .reference
                .as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef01")
        );
        assert_eq!(
            spec("github.com/o/r@refs/tags/v1").reference.as_deref(),
            Some("refs/tags/v1")
        );
        for bad in [
            "github.com/o/r@",
            "github.com/o/r@a..b",
            "github.com/o/r@-x",
            "github.com/o/r@x.lock",
            "github.com/o/r@a b",
            "github.com/o/r@a~1",
            "github.com/o/r@HEAD^",
            "github.com/o/r@x/",
            "github.com/o/r@@{1}",
        ] {
            let error = GitSpec::parse(bad).unwrap_err();
            assert!(error.contains("is not a branch, a tag"), "{bad}: {error}");
        }
    }

    #[test]
    fn only_https_is_fetched_and_plain_http_only_from_this_computer() {
        for refused in [
            "http://github.com/o/r",
            "http://localhost:8080/o/r",
            "http://10.0.0.1/o/r",
            "git://github.com/o/r",
            "file:///srv/repo",
            "ftp://github.com/o/r",
            "https://user:token@github.com/o/r",
            "https://github.com/o/r?x=1",
            "https://github.com/o/%72",
            "https://github.com/o/../r",
            "https://github.com/o/réé",
            "https://github.com/o/é",
            "https://example.org/o/aé",
            "https:///o/r",
            "https://github.com",
            "github.com",
            "",
        ] {
            assert!(GitSpec::parse(refused).is_err(), "{refused}");
        }
        // In tests and development builds, a loopback address written as
        // one may be fetched over plain HTTP.
        let local = spec("http://127.0.0.1:8080/repo.git@main");
        assert_eq!(local.repository.name(), "127.0.0.1:8080/repo");
        assert_eq!(local.repository.url(), "http://127.0.0.1:8080/repo.git");
        assert!(local.repository.loopback);
        assert_eq!(spec("http://[::1]:9/r").repository.name(), "[::1]:9/r");
        assert!(!spec("https://127.0.0.1/r").repository.loopback);
    }

    #[test]
    fn a_recorded_revision_is_read_back() {
        let tag = GitRevision::from_record(Some("refs/tags/v1"), "ab", true);
        assert_eq!(tag.reference, GitRef::Tag("v1".into()));
        assert!(tag.pinned());
        assert_eq!(tag.asked_as().as_deref(), Some("refs/tags/v1"));
        let branch = GitRevision::from_record(Some("refs/heads/main"), "ab", false);
        assert!(!branch.pinned());
        assert_eq!(branch.asked_as().as_deref(), Some("refs/heads/main"));
        let commit = GitRevision::from_record(None, "ab", true);
        assert_eq!(commit.reference, GitRef::Commit);
        assert_eq!(commit.asked_as().as_deref(), Some("ab"));
        let default = GitRevision::from_record(None, "ab", false);
        assert_eq!(default.reference, GitRef::Default { branch: None });
        assert_eq!(default.asked_as(), None);
    }

    #[test]
    fn pkt_lines_are_written_and_read() {
        assert_eq!(pkt("command=fetch\n"), b"0012command=fetch\n");
        let mut data = pkt("version 2\n");
        data.extend_from_slice(DELIM);
        data.extend(pkt("ERR no such thing\n"));
        data.extend_from_slice(FLUSH);
        let mut reader = PktReader::new(&data);
        assert_eq!(reader.line(), Ok(Ok("version 2")));
        assert_eq!(reader.line(), Ok(Err(Some(Pkt::Delim))));
        assert_eq!(
            reader.line(),
            Err("the server refused: no such thing".into())
        );
        assert_eq!(reader.next(), Ok(Some(Pkt::Flush)));
        assert_eq!(reader.next(), Ok(None));
        assert!(PktReader::new(b"00").next().is_err());
        assert!(PktReader::new(b"0010abc").next().is_err());
        assert!(PktReader::new(b"zzzz").next().is_err());
    }

    #[test]
    fn a_fetch_s_pack_is_read_into_no_more_than_the_answer_takes() {
        let mut answer = pkt("shallow-info\n");
        answer.extend(pkt("shallow 1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b\n"));
        answer.extend_from_slice(DELIM);
        answer.extend(pkt("packfile\n"));
        // 3 MiB on band 1, in the longest pkt-lines, with progress on band 2
        // between them.
        let chunk = vec![7u8; 65515];
        for _ in 0..48 {
            answer.extend(format!("{:04x}", chunk.len() + 5).into_bytes());
            answer.push(1);
            answer.extend(&chunk);
            answer.extend(format!("{:04x}", 10).into_bytes());
            answer.extend(b"\x02Hello");
        }
        answer.extend_from_slice(FLUSH);
        let pack = pack_in(&answer).unwrap();
        assert_eq!(pack.len(), 48 * chunk.len());
        assert!(pack.iter().all(|&byte| byte == 7));
        assert!(
            pack.capacity() <= answer.len(),
            "{} bytes held for an answer of {}",
            pack.capacity(),
            answer.len()
        );

        let mut refused = pkt("ERR upload-pack: not our ref\n");
        refused.extend_from_slice(FLUSH);
        assert_eq!(
            pack_in(&refused),
            Err(NoPack::Failed(
                "the server refused: upload-pack: not our ref".into()
            ))
        );
        let mut failed = pkt("packfile\n");
        failed.extend(pkt("\x03fatal: \u{1b}[2Jout of memory\n"));
        assert_eq!(
            pack_in(&failed),
            Err(NoPack::Failed(
                "the server failed: fatal: [2Jout of memory".into()
            ))
        );
        let mut none = pkt("acknowledgments\n");
        none.extend_from_slice(FLUSH);
        assert_eq!(
            pack_in(&none),
            Err(NoPack::Damaged("it sent no pack".into()))
        );
    }

    /// A pack of `entries`, each `(type, header extra, contents)`, as Git
    /// writes one: zlib-compressed contents after each entry's header.
    fn pack_of(entries: &[(u8, Vec<u8>, Vec<u8>)]) -> (Vec<u8>, Vec<usize>) {
        let mut pack = b"PACK".to_vec();
        pack.extend(2u32.to_be_bytes());
        pack.extend((entries.len() as u32).to_be_bytes());
        let mut offsets = Vec::new();
        for (kind, extra, contents) in entries {
            offsets.push(pack.len());
            let mut size = contents.len() as u64;
            let mut byte = (kind << 4) | (size & 15) as u8;
            size >>= 4;
            while size > 0 {
                pack.push(byte | 0x80);
                byte = (size & 0x7f) as u8;
                size >>= 7;
            }
            pack.push(byte);
            pack.extend(extra);
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            z.write_all(contents).unwrap();
            pack.extend(z.finish().unwrap());
        }
        let checksum = sha1(&[&pack]).unwrap();
        pack.extend(checksum);
        (pack, offsets)
    }

    fn tree_of(entries: &[(&str, &str, Id)]) -> Vec<u8> {
        let mut tree = Vec::new();
        for (mode, name, id) in entries {
            tree.extend(format!("{mode} {name}\0").as_bytes());
            tree.extend(id);
        }
        tree
    }

    fn commit_of(tree: &Id) -> Vec<u8> {
        format!(
            "tree {}\nauthor A <a@a> 0 +0000\ncommitter A <a@a> 0 +0000\n\nRelease 0.1.0\n\nMore.\n",
            hex(tree)
        )
        .into_bytes()
    }

    /// A pack holding a commit whose tree is built by `tree`, which is given
    /// a way to add blobs and subtrees. Returns the pack and the commit id.
    fn repository(
        tree: impl FnOnce(&mut Vec<(u8, Vec<u8>, Vec<u8>)>) -> Vec<u8>,
    ) -> (Vec<u8>, String) {
        let mut entries = Vec::new();
        let root = tree(&mut entries);
        let root_id = object_id(Kind::Tree, &root).unwrap();
        entries.push((2, Vec::new(), root));
        let commit = commit_of(&root_id);
        let commit_id = object_id(Kind::Commit, &commit).unwrap();
        entries.push((1, Vec::new(), commit));
        (pack_of(&entries).0, hex(&commit_id))
    }

    fn blob(entries: &mut Vec<(u8, Vec<u8>, Vec<u8>)>, contents: &[u8]) -> Id {
        entries.push((3, Vec::new(), contents.to_vec()));
        object_id(Kind::Blob, contents).unwrap()
    }

    fn check_out_pack(
        pack: &[u8],
        commit: &str,
        limits: Limits,
    ) -> Result<tempfile::TempDir, String> {
        let dir = tempfile::tempdir().unwrap();
        let objects = read_pack(pack, limits)?;
        check_out(&objects, commit, &dir.path().join("out"), limits)?;
        Ok(dir)
    }

    #[test]
    fn a_commit_s_tree_is_written_without_execute_permission() {
        let (pack, commit) = repository(|entries| {
            let manifest = blob(entries, b"{}");
            let component = blob(entries, b"\0asm");
            let dist = tree_of(&[("100755", "x.wasm", component)]);
            let dist_id = object_id(Kind::Tree, &dist).unwrap();
            entries.push((2, Vec::new(), dist));
            tree_of(&[
                ("40000", "dist", dist_id),
                ("100644", "pane.json", manifest),
            ])
        });
        let objects = read_pack(&pack, Limits::default()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let (subject, lfs) = check_out(&objects, &commit, &out, Limits::default()).unwrap();
        assert_eq!(subject, "Release 0.1.0");
        assert!(lfs.is_empty());
        assert_eq!(fs::read(out.join("pane.json")).unwrap(), b"{}");
        let component = out.join("dist").join("x.wasm");
        assert_eq!(fs::read(&component).unwrap(), b"\0asm");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&component).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0, "{mode:o}");
        }
    }

    #[test]
    fn deltas_are_resolved_and_every_object_checked_against_its_id() {
        let base = b"the first version of a component, long enough to share".to_vec();
        let target = b"the first version of a component, long enough to share, and more".to_vec();
        // Copy the whole base, then insert ", and more".
        let mut delta = vec![base.len() as u8, target.len() as u8];
        delta.extend([0x80 | 0x10, base.len() as u8]);
        delta.push(10);
        delta.extend(b", and more");
        let base_id = object_id(Kind::Blob, &base).unwrap();
        let target_id = object_id(Kind::Blob, &target).unwrap();
        let tree = tree_of(&[("100644", "a", base_id), ("100644", "b", target_id)]);
        let tree_id = object_id(Kind::Tree, &tree).unwrap();
        let commit = commit_of(&tree_id);
        let commit_id = object_id(Kind::Commit, &commit).unwrap();
        // Entries: base blob, ref delta (against the base's id), tree,
        // commit; then the same with an offset delta.
        let (pack, _) = pack_of(&[
            (3, Vec::new(), base.clone()),
            (7, base_id.to_vec(), delta.clone()),
            (2, Vec::new(), tree.clone()),
            (1, Vec::new(), commit.clone()),
        ]);
        let dir = check_out_pack(&pack, &hex(&commit_id), Limits::default()).unwrap();
        assert_eq!(fs::read(dir.path().join("out/b")).unwrap(), target);
        // An offset delta, the distance back to its base: the base entry's
        // length (a pack of it alone, less its checksum, from its offset).
        let (alone, offsets) = pack_of(&[(3, Vec::new(), base.clone())]);
        let distance = (alone.len() - 20 - offsets[0]) as u8;
        assert!(distance < 0x80);
        let (pack, _) = pack_of(&[
            (3, Vec::new(), base.clone()),
            (6, vec![distance], delta.clone()),
            (2, Vec::new(), tree),
            (1, Vec::new(), commit),
        ]);
        let dir = check_out_pack(&pack, &hex(&commit_id), Limits::default()).unwrap();
        assert_eq!(fs::read(dir.path().join("out/b")).unwrap(), target);
        // A delta whose base is missing.
        let (pack, _) = pack_of(&[(7, [7; 20].to_vec(), delta)]);
        assert_eq!(
            read_pack(&pack, Limits::default()).unwrap_err(),
            "it has a delta whose base it does not hold"
        );
    }

    #[test]
    fn a_damaged_pack_is_refused() {
        let (mut pack, commit) = repository(|entries| {
            let file = blob(entries, b"x");
            tree_of(&[("100644", "pane.json", file)])
        });
        let last = pack.len() - 1;
        pack[last] ^= 1;
        assert_eq!(
            read_pack(&pack, Limits::default()).unwrap_err(),
            "its checksum does not match its contents"
        );
        assert_eq!(
            read_pack(b"PACK", Limits::default()).unwrap_err(),
            "it is not a Git pack"
        );
        // Too many objects.
        let (pack, _) = repository(|entries| {
            let file = blob(entries, b"x");
            tree_of(&[("100644", "pane.json", file)])
        });
        let small = Limits {
            objects: 2,
            ..Limits::default()
        };
        assert!(
            read_pack(&pack, small)
                .unwrap_err()
                .contains("holds 3 objects")
        );
        // A commit the pack does not hold.
        let other = "0".repeat(40);
        assert_eq!(
            check_out_pack(&pack, &other, Limits::default()).unwrap_err(),
            "the server did not send that commit"
        );
        let _ = commit;
    }

    #[test]
    fn links_submodules_git_folders_and_unsafe_names_are_refused() {
        let cases: [(&str, &str, &str); 13] = [
            ("120000", "link", "`link`, a symbolic link"),
            ("160000", "vendored", "`vendored`, a submodule"),
            ("100644", ".git", "`.git`, a `.git` entry"),
            ("100644", ".GIT", "`.GIT`, a `.git` entry"),
            ("40000", "GIT~1", "`GIT~1`, a `.git` entry"),
            // `.git` to HFS+, which ignores some format characters (and
            // shown without them).
            ("100644", ".g\u{200c}it", "`.git`, a `.git` entry"),
            ("40000", "\u{feff}.GIT", "`.GIT`, a `.git` entry"),
            ("100644", ".gIt\u{206f}", "`.gIt`, a `.git` entry"),
            ("100644", "..", "`..`, which climbs out"),
            ("100644", "a:b", "`a:b`, whose name has a character"),
            (
                "100644",
                "con.txt",
                "`con.txt`, which is a Windows device name",
            ),
            ("100644", "x.", "`x.`, whose name ends with"),
            ("120001", "odd", "`odd`, an entry of mode 120001"),
        ];
        for (mode, name, why) in cases {
            let (pack, commit) = repository(|entries| {
                let file = blob(entries, b"x");
                tree_of(&[("100644", "pane.json", file), (mode, name, file)])
            });
            let error = check_out_pack(&pack, &commit, Limits::default()).unwrap_err();
            assert!(
                error.starts_with(&format!("its tree contains {why}")),
                "{name}: {error}"
            );
        }
        // Two names differing only in case, in one folder.
        let (pack, commit) = repository(|entries| {
            let file = blob(entries, b"x");
            tree_of(&[("100644", "README", file), ("100644", "readme", file)])
        });
        let error = check_out_pack(&pack, &commit, Limits::default()).unwrap_err();
        assert!(error.contains("differs only in case"), "{error}");
    }

    #[test]
    fn a_tree_beyond_the_limits_is_refused() {
        let (pack, commit) = repository(|entries| {
            let file = blob(entries, &[7; 3000]);
            tree_of(&[("100644", "a", file), ("100644", "b", file)])
        });
        let few = Limits {
            entries: 1,
            ..Limits::default()
        };
        assert_eq!(
            check_out_pack(&pack, &commit, few).unwrap_err(),
            "it holds more than 1 files and folders"
        );
        let small = Limits {
            unpacked: 4000,
            ..Limits::default()
        };
        assert!(
            check_out_pack(&pack, &commit, small)
                .unwrap_err()
                .starts_with("its files take more than")
        );
        let (pack, commit) = repository(|entries| {
            let file = blob(entries, b"x");
            let inner = tree_of(&[("100644", "f", file)]);
            let inner_id = object_id(Kind::Tree, &inner).unwrap();
            entries.push((2, Vec::new(), inner));
            let outer = tree_of(&[("40000", "b", inner_id)]);
            let outer_id = object_id(Kind::Tree, &outer).unwrap();
            entries.push((2, Vec::new(), outer));
            tree_of(&[("40000", "a", outer_id)])
        });
        let shallow = Limits {
            depth: 1,
            ..Limits::default()
        };
        assert_eq!(
            check_out_pack(&pack, &commit, shallow).unwrap_err(),
            "its folders nest more than 1 deep, at `a/b`"
        );
        assert!(check_out_pack(&pack, &commit, Limits::default()).is_ok());
    }

    #[test]
    fn git_lfs_pointers_are_noted() {
        let (pack, commit) = repository(|entries| {
            let pointer = blob(
                entries,
                b"version https://git-lfs.github.com/spec/v1\noid sha256:00\nsize 9\n",
            );
            tree_of(&[("100644", "x.wasm", pointer)])
        });
        let objects = read_pack(&pack, Limits::default()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (_, lfs) = check_out(
            &objects,
            &commit,
            &dir.path().join("out"),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(lfs, ["x.wasm"]);
    }

    #[test]
    fn text_from_the_server_is_shown_without_control_or_bidi_characters_and_short() {
        assert_eq!(shown("Release 0.1.0"), "Release 0.1.0");
        // A terminal escape, a bell, and a right-to-left override that
        // would show `exe.txt` as `txt.exe`.
        assert_eq!(
            shown("Fix\u{1b}[31m red\u{7} \u{202e}exe.txt\u{2066}x\u{2069}\u{200f}"),
            "Fix[31m red exe.txtx"
        );
        let long = "é".repeat(500);
        let short = shown(&long);
        assert_eq!(short.chars().count(), 201, "{short}");
        assert!(short.ends_with('…'));
        assert_eq!(shown(&"a".repeat(200)), "a".repeat(200));
        // Characters that show nothing but can hide or disguise what does:
        // zero-width spaces and joiners, a soft hyphen, a word joiner, a
        // byte order mark and tags (which can spell hidden text).
        assert_eq!(
            shown(
                "ma\u{200b}in\u{200c}\u{200d}\u{ad}\u{2060}\u{feff}\u{e0001}\u{e0041}\u{e007f} \
                 \u{2063}\u{600}\u{1d173}ok"
            ),
            "main ok"
        );
        // A line or paragraph separator, which would start another line.
        assert_eq!(shown("a\u{2028}b\u{2029}c"), "abc");
    }

    #[test]
    fn a_connection_failure_is_shown_safely() {
        let repository = spec("https://github.com/owner/repo").repository;
        let why = format!("reset\u{1b}[2J\u{202e}{}", "x".repeat(400));
        let text = unreachable(&repository, GetError::Failed(why));
        assert!(
            text.starts_with(
                "Could not reach the Git repository github.com/owner/repo: reset[2Jxxx"
            ),
            "{text}"
        );
        assert!(text.ends_with("x…"), "{text}");
    }

    #[test]
    fn connection_failures_are_recognized_wherever_the_client_wrapped_one() {
        // Where the client met it, the wording is the same.
        assert!(is_connection_failure(
            "Could not reach the Git repository github.com/owner/repo: reset by peer"
        ));
        // The fetch wraps it again in the commit it was fetching.
        assert!(is_connection_failure(
            "Could not fetch commit 0123456789abcdef0123456789abcdef01234567 of \
             github.com/owner/repo: Could not reach the Git repository \
             github.com/owner/repo: reset by peer"
        ));
        // A repository that answered, or a reference it refused, is not
        // one: those are explained, not retried.
        assert!(!is_connection_failure(
            "The Git repository github.com/owner/repo answered 404 for the reference"
        ));
        assert!(!is_connection_failure(
            "The Git repository github.com/owner/repo has no branch or tag named v1.0.0"
        ));
    }

    #[test]
    fn a_commit_subject_and_a_refusal_from_the_server_are_shown_safely() {
        let (pack, _) = repository(|entries| {
            let file = blob(entries, b"x");
            tree_of(&[("100644", "pane.json", file)])
        });
        let objects = read_pack(&pack, Limits::default()).unwrap();
        let tree = objects
            .iter()
            .find(|(_, (kind, _))| *kind == Kind::Tree)
            .map(|(id, _)| *id)
            .unwrap();
        let commit = format!(
            "tree {}\nauthor A <a@a> 0 +0000\ncommitter A <a@a> 0 +0000\n\n\u{202e}gnp.exe \
             \u{1b}]0;title\u{7}{}\n",
            hex(&tree),
            "x".repeat(400)
        );
        let (pack, _) = pack_of(&[
            (3, Vec::new(), b"x".to_vec()),
            (
                2,
                Vec::new(),
                tree_of(&[("100644", "pane.json", object_id(Kind::Blob, b"x").unwrap())]),
            ),
            (1, Vec::new(), commit.clone().into_bytes()),
        ]);
        let id = hex(&object_id(Kind::Commit, commit.as_bytes()).unwrap());
        let objects = read_pack(&pack, Limits::default()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (subject, _) =
            check_out(&objects, &id, &dir.path().join("out"), Limits::default()).unwrap();
        assert!(subject.starts_with("gnp.exe ]0;titlexxx"), "{subject}");
        assert_eq!(subject.chars().count(), 201);

        let mut data = pkt("ERR \u{202e}denied\u{1b}[2J\n");
        data.extend_from_slice(FLUSH);
        assert_eq!(
            PktReader::new(&data).line(),
            Err("the server refused: denied[2J".into())
        );
    }

    /// A delta against a base of `source` bytes: copy all of it, then insert
    /// `inserted`.
    fn delta_appending(source: usize, inserted: &[u8]) -> Vec<u8> {
        let varint = |mut value: usize, out: &mut Vec<u8>| loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        };
        let mut delta = Vec::new();
        varint(source, &mut delta);
        varint(source + inserted.len(), &mut delta);
        if source > 0 {
            // Copy from offset 0, a size of three bytes.
            delta.push(0x80 | 0x10 | 0x20 | 0x40);
            delta.extend(&(source as u32).to_le_bytes()[..3]);
        }
        for chunk in inserted.chunks(0x7f) {
            delta.push(chunk.len() as u8);
            delta.extend(chunk);
        }
        delta
    }

    /// A pack of blob `base` followed by a chain of `links` offset deltas,
    /// each against the entry before it and appending `appended` to it.
    fn offset_chain(base: &[u8], links: usize, appended: &[u8]) -> (Vec<u8>, Vec<u8>) {
        // Each entry's header and zlib stream, to learn the distance back.
        let mut entries = vec![(3u8, Vec::new(), base.to_vec())];
        let mut size = base.len();
        let mut last = base.to_vec();
        for _ in 0..links {
            entries.push((6, Vec::new(), delta_appending(size, appended)));
            size += appended.len();
            last.extend(appended);
        }
        // Offsets depend on each entry's length; write the pack once to
        // learn them, then fill in each distance (one to three bytes).
        let (_, offsets) = pack_of(&entries);
        let encode = |mut distance: usize| {
            let mut bytes = vec![(distance & 0x7f) as u8];
            distance >>= 7;
            while distance > 0 {
                distance -= 1;
                bytes.insert(0, 0x80 | (distance & 0x7f) as u8);
                distance >>= 7;
            }
            bytes
        };
        // The distances change the offsets they depend on: iterate until
        // they settle.
        let mut offsets = offsets;
        loop {
            for i in 1..entries.len() {
                entries[i].1 = encode(offsets[i] - offsets[i - 1]);
            }
            let (_, again) = pack_of(&entries);
            if again == offsets {
                break;
            }
            offsets = again;
        }
        (pack_of(&entries).0, last)
    }

    #[test]
    fn a_long_chain_of_deltas_is_followed_without_recursion_up_to_its_limit() {
        let (pack, last) = offset_chain(b"base", MAX_DELTA_CHAIN, b"");
        let objects = read_pack(&pack, Limits::default()).unwrap();
        assert!(objects.values().any(|(_, data)| data[..] == last[..]));
        let (pack, _) = offset_chain(b"base", MAX_DELTA_CHAIN + 1, b"");
        assert_eq!(
            read_pack(&pack, Limits::default()).unwrap_err(),
            "it has a chain of deltas too long to follow"
        );
    }

    #[test]
    fn deltas_against_ids_resolve_in_any_order() {
        // A chain of ref deltas written base last, each needing the next.
        let mut blobs = vec![b"v".to_vec()];
        for i in 0..2000 {
            let mut next = blobs.last().unwrap().clone();
            next.push(b'a' + (i % 26) as u8);
            blobs.push(next);
        }
        let mut entries = Vec::new();
        for pair in blobs.windows(2).rev() {
            let base_id = object_id(Kind::Blob, &pair[0]).unwrap();
            entries.push((
                7,
                base_id.to_vec(),
                delta_appending(pair[0].len(), &pair[1][pair[0].len()..]),
            ));
        }
        entries.push((3, Vec::new(), blobs[0].clone()));
        let (pack, _) = pack_of(&entries);
        let objects = read_pack(&pack, Limits::default()).unwrap();
        assert_eq!(objects.len(), blobs.len());
        let last = object_id(Kind::Blob, blobs.last().unwrap()).unwrap();
        assert_eq!(objects[&last].1[..], blobs.last().unwrap()[..]);
    }

    #[test]
    fn stored_entries_and_resolved_objects_share_one_budget() {
        // A base of 1000 bytes and four deltas each copying all of it: 1000
        // bytes inflated for the base, a few for each delta, and 1000 for
        // each object a delta makes.
        let base = vec![7u8; 1000];
        let base_id = object_id(Kind::Blob, &base).unwrap();
        let mut entries = vec![(3u8, Vec::new(), base.clone())];
        for i in 0..4u8 {
            entries.push((7, base_id.to_vec(), delta_appending(1000, &[i])));
        }
        let (pack, _) = pack_of(&entries);
        let within = |inflated| Limits {
            inflated,
            ..Limits::default()
        };
        assert!(read_pack(&pack, within(6000)).is_ok());
        let error = read_pack(&pack, within(4000)).unwrap_err();
        assert!(error.starts_with("its objects take more than"), "{error}");

        // A delta's instructions are let go once it is applied: four deltas
        // of 1000 inserted bytes each (about 1010 bytes of instructions)
        // make objects of 1000 bytes; kept, they would take about 8050, but
        // at most one delta's and its object's are held at once: about 5060.
        let base = b"b".to_vec();
        let base_id = object_id(Kind::Blob, &base).unwrap();
        let mut entries = vec![(3u8, Vec::new(), base.clone())];
        for i in 0..4u8 {
            let mut inserted = vec![i; 999];
            inserted[0] = b'x';
            entries.push((7, base_id.to_vec(), delta_appending(1, &inserted)));
        }
        let (pack, _) = pack_of(&entries);
        assert!(read_pack(&pack, within(5500)).is_ok());
    }

    #[test]
    fn an_entry_takes_no_more_memory_than_the_budget_counted_for_it() {
        // Larger than inflating's first allocation (1 MiB), so that it grows
        // while it is inflated; it may not grow past the size its header
        // gives, which is what the budget counts.
        let contents: Vec<u8> = (0..3 << 19).map(|i| (i % 251) as u8).collect();
        let (pack, _) = pack_of(&[(3, Vec::new(), contents.clone())]);
        let objects = read_pack(&pack, Limits::default()).unwrap();
        let (_, data) = &objects[&object_id(Kind::Blob, &contents).unwrap()];
        assert_eq!(data[..], contents[..]);
        assert_eq!(data.capacity(), contents.len());
    }

    /// A tree of `entries` (mode, name, id) written as given.
    fn raw_tree_of(entries: &[(&[u8], &[u8], Id)]) -> Vec<u8> {
        let mut tree = Vec::new();
        for (mode, name, id) in entries {
            tree.extend(*mode);
            tree.push(b' ');
            tree.extend(*name);
            tree.push(0);
            tree.extend(id);
        }
        tree
    }

    #[test]
    fn a_long_or_undecodable_name_or_mode_is_refused_without_taking_memory() {
        // 2 MiB each, which compress to a few KiB: each refused, and what is
        // held while it is refused is not a multiple of it.
        let long = vec![b'a'; 2 << 20];
        let undecodable = vec![0xff; 2 << 20];
        let cases: [(&[u8], &[u8], &str); 3] = [
            (
                b"100644",
                &long,
                "whose name is longer than the 255 bytes every system takes",
            ),
            (b"100644", &undecodable, "whose name is not valid UTF-8"),
            (&long, b"x", "an entry of mode aaaa"),
        ];
        for (mode, name, why) in cases {
            let (pack, commit) = repository(|entries| {
                let file = blob(entries, b"x");
                raw_tree_of(&[(mode, name, file)])
            });
            let objects = read_pack(&pack, Limits::default()).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let out = dir.path().join("out");
            let (result, peak) = crate::peak_memory::peak_while(|| {
                check_out(&objects, &commit, &out, Limits::default())
            });
            let error = result.unwrap_err();
            assert!(error.contains(why), "{why}: {}", shown(&error));
            assert!(error.len() < 2000, "{why}: {} bytes", error.len());
            assert!(peak < 1 << 20, "{why}: {peak} bytes held");
        }
    }

    #[test]
    fn a_commit_s_message_is_read_without_decoding_all_of_it() {
        // A message of 2 MiB that is not UTF-8, which decoded would take
        // three bytes for each: its subject is its start.
        let tree = tree_of(&[("100644", "pane.json", object_id(Kind::Blob, b"x").unwrap())]);
        let mut commit = format!(
            "tree {}\nauthor A <a@a> 0 +0000\ncommitter A <a@a> 0 +0000\n\n",
            hex(&object_id(Kind::Tree, &tree).unwrap())
        )
        .into_bytes();
        commit.extend(vec![0xff; 2 << 20]);
        let id = hex(&object_id(Kind::Commit, &commit).unwrap());
        let (pack, _) = pack_of(&[
            (3, Vec::new(), b"x".to_vec()),
            (2, Vec::new(), tree),
            (1, Vec::new(), commit),
        ]);
        let objects = read_pack(&pack, Limits::default()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let (result, peak) =
            crate::peak_memory::peak_while(|| check_out(&objects, &id, &out, Limits::default()));
        let (subject, _) = result.unwrap();
        assert_eq!(subject, format!("{}…", "\u{fffd}".repeat(200)));
        assert!(peak < 1 << 20, "{peak} bytes held");
    }

    #[test]
    fn a_delta_making_more_than_it_says_is_refused_before_it_is_held() {
        let base = vec![0u8; 2 << 20];
        // Says it makes 1 KiB of the 2 MiB base, then copies all of it.
        let mut delta = vec![0x80, 0x80, 0x80, 0x01, 0x80, 0x08];
        delta.extend([0x80 | 0x10 | 0x20 | 0x40, 0x00, 0x00, 0x20]);
        let (result, peak) =
            crate::peak_memory::peak_while(|| apply_delta(&base, &delta, MAX_UNPACKED));
        assert_eq!(result.unwrap_err(), "it has a damaged delta");
        assert!(peak < 1 << 20, "{peak} bytes held");
    }

    #[test]
    fn a_delta_is_applied_within_its_bounds() {
        let base = b"0123456789";
        // Copy 4 bytes at offset 2, insert "ab".
        let delta = [10, 6, 0x80 | 0x01 | 0x10, 2, 4, 2, b'a', b'b'];
        assert_eq!(apply_delta(base, &delta, 100).unwrap(), b"2345ab");
        // A copy beyond the base.
        let delta = [10, 6, 0x80 | 0x01 | 0x10, 8, 4];
        assert!(apply_delta(base, &delta, 100).is_err());
        // A result larger than allowed.
        let delta = [10, 0x7f, 0];
        assert!(apply_delta(base, &delta, 100).is_err());
        let delta = [10, 101, 0];
        assert!(
            apply_delta(base, &delta, 100)
                .unwrap_err()
                .contains("101 bytes")
        );
    }
}
