//! What the query typed into root search is, beyond the words it holds
//! (#195): a web address or a path. The analysis is the launcher's, made
//! once per change of the query, and the rows it lists come from the
//! commands that declare [`CommandMatches`] `url` or `file-path`: they are
//! listed only for such a query, without title matching, below the results
//! found by title and above the files, in install order, and the parsed
//! address or resolved path is sent to the command as its launch record's
//! fallback text when the user invokes the row.
//!
//! The grammar (the specification's):
//!
//! - A query is **path-like** when it starts with a drive letter and a
//!   separator (`C:\` or `C:/`), with `\\` (a network path), with `~`
//!   (resolved to the user's home folder, alone or followed by a
//!   separator), with `/`, or with `file://` (an empty authority may
//!   follow, and a drive letter: `file:///etc/hosts`, `file://C:/Windows`).
//!   A path may hold spaces. Its resolved path is what the query says,
//!   `~` joined onto the home folder and `file://` taken off.
//! - A query is **URL-like** when it has no spaces, is not path-like, and
//!   either parses as an absolute URL with a scheme — at least two
//!   characters, without a dot, then `:` and something after it that is
//!   not only slashes, as `mailto:someone@example.com` does — or contains
//!   a dot that is not its last character and whose part before the first
//!   `/` is a plausible host: labels of letters, digits and `-` separated
//!   by dots, the last one alphabetic and at least two letters, with an
//!   optional `:port`, or four numbers 0 to 255 (`github.com`,
//!   `192.168.0.1`, but not `1.5`). `https://` is inferred before such a
//!   query then.
//!
//! The home folder is the one the file index is configured with, or the
//! environment's (`USERPROFILE` on Windows, `HOME` elsewhere) when this
//! launcher keeps no index; `~` is not a path while neither is known.

use std::path::Path;

use super::aliases::{Sending, Via};
use super::{Entry, Opening, RootResult, Row, State};
use crate::launch::{LaunchRecord, LaunchSource};
use crate::packages::{CommandMatches, CommandWhen};

/// A query root search understood as a typed address or path, with the
/// text sent to commands declared for it: the parsed address or the
/// resolved path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TypedQuery {
    /// A web address, `https://` inferred before a bare domain.
    Url(String),
    /// A path, `~` resolved to the home folder and `file://` taken off.
    Path(String),
}

impl TypedQuery {
    /// The text a command declared for this query receives as its launch
    /// record's fallback text.
    fn text(&self) -> &str {
        match self {
            TypedQuery::Url(address) => address,
            TypedQuery::Path(path) => path,
        }
    }

    /// The `matches` a command must declare to be listed for this query.
    fn wants(&self) -> CommandMatches {
        match self {
            TypedQuery::Url(_) => CommandMatches::Url,
            TypedQuery::Path(_) => CommandMatches::FilePath,
        }
    }
}

/// What `query` typed into root search is: an address, a path, or words
/// (`None`). The query is taken as trimmed; `home` is the folder `~`
/// resolves to, if one is known.
pub(super) fn analyze(query: &str, home: Option<&Path>) -> Option<TypedQuery> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    if let Some(path) = path_like(query, home) {
        return Some(TypedQuery::Path(path));
    }
    url_like(query).map(TypedQuery::Url)
}

/// The resolved path of a path-like `query`, if it is one: what it says
/// as typed, `~` joined onto `home`, or `file://` taken off.
fn path_like(query: &str, home: Option<&Path>) -> Option<String> {
    if let Some(rest) = query.strip_prefix("file://") {
        // An empty authority may follow the scheme, and a drive letter
        // after it: `file:///C:/Windows` is `C:/Windows`, while
        // `file:///etc/hosts` is `/etc/hosts` and `file://C:/x` is `C:/x`.
        if let Some(drive) = rest.strip_prefix('/')
            && is_drive(drive)
        {
            return Some(drive.to_owned());
        }
        return Some(if rest.is_empty() { "/".to_owned() } else { rest.to_owned() });
    }
    // `~` alone is the home folder; `~/rest` and `~\rest` are below it.
    if let Some(rest) = query.strip_prefix('~')
        && (rest.is_empty() || rest.starts_with(['/', '\\']))
    {
        let home = home?;
        let rest = rest.trim_start_matches(['/', '\\']);
        let joined = if rest.is_empty() {
            home.to_path_buf()
        } else {
            home.join(rest)
        };
        return Some(joined.to_string_lossy().into_owned());
    }
    (is_drive(query) || query.starts_with('\\') || query.starts_with('/'))
        .then(|| query.to_owned())
}

/// Whether `text` starts with a drive letter and a separator, as
/// `C:\Windows` and `C:/Windows` do — the same on every system, so a
/// Windows path is understood wherever Pane runs.
fn is_drive(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && (bytes[1] == b'/' || bytes[1] == b'\\')
}

/// The address of a URL-like `query`, if it is one: as typed when it has a
/// scheme, `https://` inferred before a bare domain.
fn url_like(query: &str) -> Option<String> {
    if query.chars().any(char::is_whitespace) {
        return None;
    }
    if has_scheme(query) {
        return Some(query.to_owned());
    }
    // A bare domain: the part before the first `/` must be a plausible
    // host, and a dot that is not the query's last character must be
    // there to hold it together.
    let before = query.split('/').next().unwrap_or(query);
    if query.contains('.') && !query.ends_with('.') && plausible_host(before) {
        return Some(format!("https://{query}"));
    }
    None
}

/// Whether `query` parses as an absolute URL with a scheme: the part
/// before its first `:` is one, and something that is not only slashes
/// follows. A scheme holds no dot, so a domain with a port is not one, and
/// it has at least two characters, so a drive letter is not one (path-like
/// queries were answered before this anyway).
fn has_scheme(query: &str) -> bool {
    let Some((scheme, rest)) = query.split_once(':') else {
        return false;
    };
    scheme.chars().next().is_some_and(|first| first.is_ascii_alphabetic())
        && scheme.len() >= 2
        && !scheme.contains('.')
        && scheme
            .chars()
            .skip(1)
            .all(|letter| letter.is_ascii_alphanumeric() || letter == '+' || letter == '-')
        && !rest.is_empty()
        && !rest.bytes().all(|byte| byte == b'/')
}

/// Whether `text` is a plausible host: labels of letters, digits and `-`
/// separated by dots, the last label alphabetic and at least two letters,
/// with an optional numeric port after `:`; or four numbers 0 to 255.
/// IPv6 is bracketed and not understood as a host.
fn plausible_host(text: &str) -> bool {
    let host = match text.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            host
        }
        _ => text,
    };
    let labels: Vec<&str> = host.split('.').collect();
    // Four numbers 0 to 255 are an IPv4 address.
    if labels.len() == 4
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 3
                && label.bytes().all(|byte| byte.is_ascii_digit())
                && label.parse::<u8>().is_ok()
        })
    {
        return true;
    }
    let Some((last, labels)) = labels.split_last() else {
        return false;
    };
    let plausible_label = |label: &str| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    };
    !labels.is_empty()
        && labels.iter().all(|label| plausible_label(label))
        && last.len() >= 2
        && last.bytes().all(|byte| byte.is_ascii_alphabetic())
}

/// The rows the query's analysis lists, in root search order (which is
/// install order): one for each command declared for what was typed, each
/// sending the parsed address or resolved path to its command as its
/// launch record's fallback text when invoked. None for a query that is
/// neither an address nor a path.
pub(super) fn rows(state: &State) -> Vec<(Row, Entry)> {
    let Some(typed) = &state.typed else {
        return Vec::new();
    };
    let text = typed.text();
    let wants = typed.wants();
    state
        .root
        .iter()
        // What was typed is never blank, so a command that never shows
        // while the user searches is not listed for it either.
        .filter(|result| result.matches == wants && result.when.listed(false))
        .filter_map(|result| address_row(result, text))
        .collect()
}

/// The row sending `text` to the command `result` opens, below the results
/// found by title. A command that cannot run now stays listed, saying why,
/// and invoking its row shows the reason instead of sending anything.
fn address_row(result: &RootResult, text: &str) -> Option<(Row, Entry)> {
    let opening = match &result.entry {
        Entry::Open(opening) => opening.clone(),
        Entry::Unavailable(_) => {
            let target = result.target.as_ref()?;
            Opening::of(&target.registration, target.no_view, LaunchSource::RootSearch)
        }
        _ => return None,
    };
    let opening = Opening {
        launch: LaunchRecord::sending(LaunchSource::RootSearch, text),
        ..opening
    };
    Some((
        result.row.clone(),
        Entry::Send(Sending {
            opening,
            via: Via::Typed,
            unavailable: result
                .row
                .unavailable
                .as_ref()
                .map(|why| why.reason().to_owned()),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The home folder `~` resolves to in these tests.
    fn home() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            r"C:\Users\v"
        } else {
            "/home/v"
        })
    }

    fn analyzed(query: &str) -> Option<TypedQuery> {
        analyze(query, Some(&home()))
    }

    #[test]
    fn an_absolute_url_with_a_scheme_is_one_as_typed() {
        assert_eq!(
            analyzed("https://github.com/pane-app/pane"),
            Some(TypedQuery::Url("https://github.com/pane-app/pane".into()))
        );
        assert_eq!(
            analyzed("mailto:someone@example.com"),
            Some(TypedQuery::Url("mailto:someone@example.com".into()))
        );
        assert_eq!(
            analyzed("ms-settings:display"),
            Some(TypedQuery::Url("ms-settings:display".into()))
        );
    }

    #[test]
    fn a_scheme_needs_two_characters_no_dot_and_something_after_it() {
        // A drive letter without a separator looks like a one-letter
        // scheme; a domain with a port like a dotted one; neither is one.
        assert_eq!(analyzed("C:"), None);
        assert_eq!(analyzed("mailto:"), None);
        assert_eq!(analyzed("https://"), None);
        assert_eq!(analyzed("3a:x"), None);
        // A port is not a scheme: the domain is understood instead.
        assert_eq!(
            analyzed("example.com:8080"),
            Some(TypedQuery::Url("https://example.com:8080".into()))
        );
    }

    #[test]
    fn a_bare_domain_infers_https() {
        assert_eq!(
            analyzed("github.com"),
            Some(TypedQuery::Url("https://github.com".into()))
        );
        assert_eq!(
            analyzed("notes.example.com/pane"),
            Some(TypedQuery::Url("https://notes.example.com/pane".into()))
        );
        assert_eq!(
            analyzed("192.168.0.1"),
            Some(TypedQuery::Url("https://192.168.0.1".into()))
        );
    }

    #[test]
    fn a_query_with_a_dot_that_is_not_a_host_is_words() {
        assert_eq!(analyzed("1.5"), None);
        assert_eq!(analyzed("3.14+2"), None);
        assert_eq!(analyzed("a.b"), None);
        assert_eq!(analyzed("256.0.0.1"), None);
        assert_eq!(analyzed("localhost"), None);
        // Spaces rule an address out.
        assert_eq!(analyzed("github.com pane"), None);
    }

    #[test]
    fn windows_paths_are_paths_with_their_spaces() {
        assert_eq!(
            analyzed(r"C:\Windows"),
            Some(TypedQuery::Path(r"C:\Windows".into()))
        );
        assert_eq!(
            analyzed(r"C:/Program Files/Zed"),
            Some(TypedQuery::Path(r"C:/Program Files/Zed".into()))
        );
        assert_eq!(
            analyzed(r"\\server\share"),
            Some(TypedQuery::Path(r"\\server\share".into()))
        );
    }

    #[test]
    fn unix_paths_are_paths() {
        assert_eq!(
            analyzed("/etc/hosts"),
            Some(TypedQuery::Path("/etc/hosts".into()))
        );
        assert_eq!(analyzed("/"), Some(TypedQuery::Path("/".into())));
        // A dot inside does not make a path an address, and a slash inside
        // does not make a domain a path.
        assert_eq!(analyzed("a/b.c"), None);
    }

    #[test]
    fn a_tilde_resolves_to_the_home_folder() {
        assert_eq!(
            analyzed("~"),
            Some(TypedQuery::Path(home().to_string_lossy().into_owned()))
        );
        let below = home().join("Documents");
        assert_eq!(
            analyzed("~/Documents"),
            Some(TypedQuery::Path(below.to_string_lossy().into_owned()))
        );
        assert_eq!(
            analyzed(r"~\Documents"),
            Some(TypedQuery::Path(below.to_string_lossy().into_owned()))
        );
        // Without a home folder known, `~` is not a path; a name beside it
        // never is.
        assert_eq!(analyze("~", None), None);
        assert_eq!(analyze("~/Documents", None), None);
        assert_eq!(analyzed("~Documents"), None);
    }

    #[test]
    fn a_file_url_is_the_path_it_names() {
        assert_eq!(
            analyzed("file:///etc/hosts"),
            Some(TypedQuery::Path("/etc/hosts".into()))
        );
        assert_eq!(
            analyzed("file://C:/Windows"),
            Some(TypedQuery::Path("C:/Windows".into()))
        );
        assert_eq!(
            analyzed("file:///C:/Windows"),
            Some(TypedQuery::Path("C:/Windows".into()))
        );
        assert_eq!(analyzed("file://"), Some(TypedQuery::Path("/".into())));
    }
}
