//! Quicklinks as the extension keeps them: named targets (a link of any
//! scheme, or the path of a file, a folder or an application), each with an
//! optional application to open it with, saved in its content (extension
//! data) as one value, and checked before they are saved.

use pane_guest::alloc::{
    borrow::ToOwned,
    format,
    string::{String, ToString},
    vec::Vec,
};
use pane_guest::content;

/// The content key holding every quicklink.
const KEY: &str = "quicklinks";

/// The longest name, in characters.
pub const MAX_NAME: usize = 80;
/// The longest target, in characters.
pub const MAX_TARGET: usize = 2048;

/// The application a quicklink opens its target with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Application {
    /// What Pane opens the target with: an installed application's id, or
    /// an application's absolute path.
    pub id: String,
    /// Its name, as the user sees it.
    pub name: String,
}

/// A saved target and the name root search finds it by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quicklink {
    /// Identifies it for good, also once renamed: root search's indexed
    /// result and a quick slot holding it name it by this.
    pub id: String,
    pub name: String,
    pub target: String,
    /// The application it opens with; `None` for the system's handler.
    pub application: Option<Application>,
}

/// The saved quicklinks, in the order they were created.
///
/// One line per quicklink: its name, target, id, application id and
/// application name, separated by tabs (none of them holds a control
/// character). A line the first version saved has only a name and a target
/// (the upgrade keeps it): it gets the next free id, the same on every load
/// until the quicklinks are saved again.
pub fn load() -> Result<Vec<Quicklink>, String> {
    let saved = content::get(KEY)?.unwrap_or_default();
    Ok(parse(&saved))
}

/// The quicklinks the saved value `saved` holds.
fn parse(saved: &str) -> Vec<Quicklink> {
    let mut links: Vec<Quicklink> = Vec::new();
    let mut without_id = Vec::new();
    for line in saved.lines() {
        let mut fields = line.split('\t');
        let (Some(name), Some(target)) = (fields.next(), fields.next()) else {
            continue;
        };
        let id = fields.next().unwrap_or_default();
        let application = match (fields.next(), fields.next()) {
            (Some(id), Some(name)) if !id.is_empty() => Some(Application {
                id: id.to_owned(),
                name: name.to_owned(),
            }),
            _ => None,
        };
        if id.is_empty() {
            without_id.push(links.len());
        }
        links.push(Quicklink {
            id: id.to_owned(),
            name: name.to_owned(),
            target: target.to_owned(),
            application,
        });
    }
    for index in without_id {
        links[index].id = next_id(&links);
    }
    links
}

/// Saves `links`, replacing the saved ones.
pub fn save(links: &[Quicklink]) -> Result<(), String> {
    let value: String = links
        .iter()
        .map(|link| {
            let (id, name) = link
                .application
                .as_ref()
                .map_or(("", ""), |app| (app.id.as_str(), app.name.as_str()));
            format!(
                "{}\t{}\t{}\t{id}\t{name}\n",
                link.name, link.target, link.id
            )
        })
        .collect();
    content::set(KEY, &value)
}

/// An id no quicklink of `links` has: one more than the largest.
pub fn next_id(links: &[Quicklink]) -> String {
    let largest = links
        .iter()
        .filter_map(|link| link.id.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    (largest + 1).to_string()
}

/// The index of the quicklink named `name`, ignoring letter case.
pub fn find(links: &[Quicklink], name: &str) -> Option<usize> {
    let name = name.to_lowercase();
    links
        .iter()
        .position(|link| link.name.to_lowercase() == name)
}

/// The index of the quicklink with id `id`.
pub fn with_id(links: &[Quicklink], id: &str) -> Option<usize> {
    links.iter().position(|link| link.id == id)
}

/// A name for a copy of the quicklink named `name` that no quicklink of
/// `links` has: "<name> copy", then "<name> copy 2", ...
pub fn copy_name(links: &[Quicklink], name: &str) -> String {
    let mut copy = format!("{name} copy");
    let mut number = 2;
    while find(links, &copy).is_some() {
        copy = format!("{name} copy {number}");
        number += 1;
    }
    copy
}

/// Why `name` (trimmed) cannot name a quicklink, if it cannot.
pub fn name_problem(name: &str) -> Option<String> {
    if name.is_empty() {
        Some("Enter a name".into())
    } else if name.chars().any(char::is_control) {
        Some("The name cannot contain line breaks or tabs".into())
    } else if name.chars().count() > MAX_NAME {
        Some(format!("Use at most {MAX_NAME} characters"))
    } else {
        None
    }
}

/// Whether `target` is an absolute path: `/…` (macOS and Linux), or a
/// drive's `C:\…` or `C:/…` and a share's `\\server\…` (Windows).
pub fn is_path(target: &str) -> bool {
    let bytes = target.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    drive || target.starts_with('/') || target.starts_with("\\\\")
}

/// The scheme of `target` when it is a link (`https`, `mailto`,
/// `ms-settings`): a letter, then letters, digits, `+`, `-` or `.`, before
/// its first `:`. A drive letter (`C:`) is not a scheme.
pub fn scheme(target: &str) -> Option<&str> {
    let (scheme, _) = target.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    (first.is_ascii_alphabetic() && rest_ok && scheme.len() > 1).then_some(scheme)
}

/// Whether `target` is a web address, `http:` or `https:`.
pub fn is_web(target: &str) -> bool {
    scheme(target).is_some_and(|scheme| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

/// Why `target` (trimmed) is not something a quicklink opens, if it is
/// not. A quicklink opens a link of any scheme (ADR 0037) or the absolute
/// path of a file, a folder or an application; whether the path exists is
/// for the system to say when it is opened.
pub fn target_problem(target: &str) -> Option<String> {
    if target.is_empty() {
        return Some("Enter a link, or the path of a file, folder or application".into());
    }
    if target.chars().any(char::is_control) {
        return Some("The link cannot contain line breaks or tabs".into());
    }
    if target.chars().count() > MAX_TARGET {
        return Some(format!("Use at most {MAX_TARGET} characters"));
    }
    if is_path(target) {
        return None;
    }
    let Some(scheme) = scheme(target) else {
        return Some(
            "Enter a link with its scheme, such as https:// or mailto:, or the full path of \
             a file, folder or application"
                .into(),
        );
    };
    if target.chars().any(char::is_whitespace) {
        return Some("A link cannot contain spaces".into());
    }
    let rest = &target[scheme.len() + 1..];
    if rest.is_empty() {
        return Some(format!("Enter what the link opens after “{scheme}:”"));
    }
    if is_web(target) {
        let host = rest
            .strip_prefix("//")
            .map(|rest| rest.split(['/', '?', '#']).next().unwrap_or_default());
        if host.is_none_or(str::is_empty) {
            return Some("The address has no host".into());
        }
    }
    None
}

/// The application named `given` (trimmed): an installed one, by its name
/// in any letter case or its id, else an absolute path; `Ok(None)` when
/// none is given, `Err` with why when no application has that name.
pub fn application(
    given: &str,
    installed: &[pane_guest::applications::Application],
) -> Result<Option<Application>, String> {
    if given.is_empty() {
        return Ok(None);
    }
    if given.chars().any(char::is_control) {
        return Err("The application cannot contain line breaks or tabs".into());
    }
    let lower = given.to_lowercase();
    if let Some(found) = installed
        .iter()
        .find(|app| app.id == given || app.name.to_lowercase() == lower)
    {
        return Ok(Some(Application {
            id: found.id.clone(),
            name: found.name.clone(),
        }));
    }
    if is_path(given) {
        return Ok(Some(Application {
            id: given.to_owned(),
            name: file_stem(given).to_owned(),
        }));
    }
    Err(format!(
        "No installed application is named “{given}”; enter its name as Pane lists it, or \
         its full path"
    ))
}

/// The last part of `path`, without its extension: "Notepad" for
/// `C:\Windows\notepad.exe`.
fn file_stem(path: &str) -> &str {
    let name = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}
