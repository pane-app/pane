//! Quicklinks as the extension keeps them: validated names and web
//! addresses, saved in its content (extension data) as one value.

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
/// The longest address, in characters.
pub const MAX_URL: usize = 2048;

/// A saved web address and the name root search finds it by.
#[derive(Clone)]
pub struct Quicklink {
    pub name: String,
    pub url: String,
}

/// The saved quicklinks, in the order they were created.
pub fn load() -> Result<Vec<Quicklink>, String> {
    let saved = content::get(KEY)?.unwrap_or_default();
    Ok(saved
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(name, url)| Quicklink {
            name: name.to_owned(),
            url: url.to_owned(),
        })
        .collect())
}

/// Saves `links`, replacing the saved ones. Names and addresses hold no
/// control characters, so a tab and a line break separate them.
pub fn save(links: &[Quicklink]) -> Result<(), String> {
    let value: String = links
        .iter()
        .map(|link| format!("{}\t{}\n", link.name, link.url))
        .collect();
    content::set(KEY, &value)
}

/// The index of the quicklink named `name`, ignoring letter case.
pub fn find(links: &[Quicklink], name: &str) -> Option<usize> {
    let name = name.to_lowercase();
    links
        .iter()
        .position(|link| link.name.to_lowercase() == name)
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

/// Why `url` (trimmed) is not an address a quicklink opens, if it is not:
/// it must be an absolute `http://` or `https://` URL with a host.
pub fn url_problem(url: &str) -> Option<String> {
    let web = url.split_once("://").is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    });
    if !web {
        return Some("Enter a web address starting with http:// or https://".into());
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Some("The address cannot contain spaces".into());
    }
    if url.chars().count() > MAX_URL {
        return Some(format!("Use at most {MAX_URL} characters"));
    }
    let (_, rest) = url.split_once("://").expect("checked above");
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() {
        return Some("The address has no host".to_string());
    }
    None
}

/// The quicklinks `query` finds, best first: those whose name holds every
/// word of it, then those whose name or address does.
pub fn matching<'a>(links: &'a [Quicklink], query: &str) -> Vec<&'a Quicklink> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut by_name = Vec::new();
    let mut by_address = Vec::new();
    for link in links {
        let name = link.name.to_lowercase();
        let url = link.url.to_lowercase();
        if words.iter().all(|word| name.contains(word)) {
            by_name.push(link);
        } else if words
            .iter()
            .all(|word| name.contains(word) || url.contains(word))
        {
            by_address.push(link);
        }
    }
    by_name.extend(by_address);
    by_name
}
