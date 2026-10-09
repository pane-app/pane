//! Redaction at the log's writer, before anything is written: the home
//! folder's path becomes `~`, the user's name `<user>` and the computer's
//! name `<computer>`. Each is matched without regard to case (and with `/`
//! and `\` taken as one), only as a whole word or path component (its
//! neighbours are not letters or digits), and only when it is at least
//! [`MIN_CHARS`] characters long, so a name does not eat ordinary words: the
//! user "admin" leaves "administrator" alone. A name that identifies nobody
//! ([`IDENTIFY_NOBODY`], such as "root" or "localhost") is not redacted, so
//! "root search" and Pane's own name stay readable. Other paths are kept:
//! they are what a diagnosis needs.

use std::path::{Path, PathBuf};

/// The fewest characters a name must have to be redacted.
pub(crate) const MIN_CHARS: usize = 3;

/// User and computer names that many people share and that are also words
/// Pane's messages use, in lower case: redacting them would hide nothing
/// and mangle every message that says them. A home folder named after one
/// is still redacted, as a path.
const IDENTIFY_NOBODY: [&str; 6] = [
    "admin",
    "administrator",
    "localhost",
    "pane",
    "root",
    "user",
];

/// What the log replaces before it writes.
#[derive(Clone, Debug, Default)]
pub(crate) struct Redactor {
    /// Each text and what replaces it, longest first, so the home folder's
    /// path wins over the user's name inside it.
    needles: Vec<(String, &'static str)>,
}

impl Redactor {
    /// One that replaces nothing (tests of other things).
    #[cfg(test)]
    pub(crate) fn none() -> Redactor {
        Redactor::default()
    }

    /// One replacing `home`, the `users`' names and the `computers`'.
    pub(crate) fn new(home: Option<&Path>, users: &[String], computers: &[String]) -> Redactor {
        let mut needles: Vec<(String, &'static str)> = Vec::new();
        let mut add = |text: &str, replacement: &'static str| {
            let text = text.trim();
            let known = needles
                .iter()
                .any(|(needle, _)| needle.to_lowercase() == text.to_lowercase());
            if text.chars().count() >= MIN_CHARS && !known {
                needles.push((text.to_owned(), replacement));
            }
        };
        if let Some(home) = home {
            let home = home.to_string_lossy();
            add(home.trim_end_matches(['/', '\\']), "~");
        }
        let identifies =
            |name: &&String| !IDENTIFY_NOBODY.contains(&name.trim().to_lowercase().as_str());
        for user in users.iter().filter(identifies) {
            add(user, "<user>");
        }
        for computer in computers.iter().filter(identifies) {
            add(computer, "<computer>");
        }
        needles.sort_by_key(|(needle, _)| std::cmp::Reverse(needle.chars().count()));
        Redactor { needles }
    }

    /// One for this computer: its user's home folder, the user's name and
    /// the computer's, as the system reports them.
    pub(crate) fn of_this_system() -> Redactor {
        let home = home_folder();
        let mut users = user_names();
        // The home folder's own name is usually the user's.
        if let Some(name) = home
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
        {
            users.push(name.to_owned());
        }
        Redactor::new(home.as_deref(), &users, &computer_names())
    }

    /// `text` with every needle replaced where it stands as a whole word or
    /// path component.
    pub(crate) fn redact(&self, text: &str) -> String {
        if self.needles.is_empty() {
            return text.to_owned();
        }
        let mut redacted = String::with_capacity(text.len());
        let mut rest = text;
        // The character before `rest`.
        let mut before = None;
        'text: while let Some(next) = rest.chars().next() {
            for (needle, replacement) in &self.needles {
                if let Some(length) = matches_at(before, rest, needle) {
                    redacted.push_str(replacement);
                    before = rest[..length].chars().next_back();
                    rest = &rest[length..];
                    continue 'text;
                }
            }
            redacted.push(next);
            before = Some(next);
            rest = &rest[next.len_utf8()..];
        }
        redacted
    }
}

/// How many bytes of `text` its start matches `needle` with, if it does:
/// letters in any case, and either slash for either, and only as a whole
/// word or path component. A needle that starts with a letter or digit
/// matches only after a character that is neither (`before`, or the start
/// of the text), and one that ends with one only before such a character
/// (or the end), so the user "admin" leaves "administrator" alone and a
/// computer called "pane" leaves "panel" and "panes" as they are.
fn matches_at(before: Option<char>, text: &str, needle: &str) -> Option<usize> {
    let word = |c: char| c.is_alphanumeric();
    let starts_word = needle.chars().next().is_some_and(word);
    if starts_word && before.is_some_and(word) {
        return None;
    }
    let mut length = 0;
    let mut chars = text.chars();
    for wanted in needle.chars() {
        let found = chars.next()?;
        if !same(found, wanted) {
            return None;
        }
        length += found.len_utf8();
    }
    let ends_word = needle.chars().next_back().is_some_and(word);
    if ends_word && chars.next().is_some_and(word) {
        return None;
    }
    Some(length)
}

fn same(found: char, wanted: char) -> bool {
    let slash = |c: char| c == '/' || c == '\\';
    found == wanted
        || (slash(found) && slash(wanted))
        || found.to_lowercase().eq(wanted.to_lowercase())
}

/// The user's home folder.
fn home_folder() -> Option<PathBuf> {
    let variable = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(variable)
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

/// The user's name, as the system and the environment say it.
fn user_names() -> Vec<String> {
    let mut names = Vec::new();
    for variable in ["USERNAME", "USER", "LOGNAME"] {
        if let Some(name) = std::env::var(variable).ok().filter(|name| !name.is_empty()) {
            names.push(name);
        }
    }
    #[cfg(windows)]
    names.extend(windows_names::user());
    names
}

/// The computer's name, as the system and the environment say it.
fn computer_names() -> Vec<String> {
    let mut names = Vec::new();
    if let Some(name) = std::env::var("COMPUTERNAME")
        .ok()
        .filter(|name| !name.is_empty())
    {
        names.push(name);
    }
    #[cfg(windows)]
    names.extend(windows_names::computer());
    #[cfg(unix)]
    if let Some(host) = unix_names::host() {
        // `Alices-MacBook.local` is also written `Alices-MacBook`.
        if let Some((first, _)) = host.split_once('.') {
            names.push(first.to_owned());
        }
        names.push(host);
    }
    names
}

/// The names Windows reports.
#[cfg(windows)]
mod windows_names {
    use ::windows::Win32::System::WindowsProgramming::{GetComputerNameW, GetUserNameW};
    use ::windows::core::PWSTR;

    /// The text in `buffer` up to its first NUL.
    fn text(buffer: &[u16]) -> Option<String> {
        let length = buffer
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(buffer.len());
        let name = String::from_utf16_lossy(&buffer[..length]);
        (!name.trim().is_empty()).then_some(name)
    }

    /// The name of the user this process runs as.
    pub(super) fn user() -> Option<String> {
        // UNLEN (256) and its NUL.
        let mut buffer = [0u16; 257];
        let mut size = buffer.len() as u32;
        // SAFETY: `buffer` is writable for `size` units.
        unsafe { GetUserNameW(Some(PWSTR(buffer.as_mut_ptr())), &mut size) }.ok()?;
        text(&buffer)
    }

    /// The computer's NetBIOS name.
    pub(super) fn computer() -> Option<String> {
        // MAX_COMPUTERNAME_LENGTH (15) and its NUL, with room to spare.
        let mut buffer = [0u16; 64];
        let mut size = buffer.len() as u32;
        // SAFETY: `buffer` is writable for `size` units.
        unsafe { GetComputerNameW(Some(PWSTR(buffer.as_mut_ptr())), &mut size) }.ok()?;
        text(&buffer)
    }
}

/// The names a Unix system reports.
#[cfg(unix)]
mod unix_names {
    /// The computer's host name.
    pub(super) fn host() -> Option<String> {
        let mut buffer = [0u8; 256];
        // SAFETY: `buffer` is writable for its length; the last byte stays
        // NUL whatever the call writes.
        let answered = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len() - 1) };
        if answered != 0 {
            return None;
        }
        let name = std::ffi::CStr::from_bytes_until_nul(&buffer).ok()?;
        let name = name.to_string_lossy().trim().to_owned();
        (!name.is_empty()).then_some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor() -> Redactor {
        Redactor::new(
            Some(Path::new(r"C:\Users\Alice")),
            &["alice".to_owned()],
            &["DESK-42".to_owned()],
        )
    }

    #[test]
    fn the_home_folder_becomes_a_tilde_in_either_slash_and_any_case() {
        let redactor = redactor();
        assert_eq!(
            redactor.redact(r"could not read C:\Users\Alice\AppData\Local\Pane\x.json"),
            r"could not read ~\AppData\Local\Pane\x.json"
        );
        assert_eq!(
            redactor.redact("could not read c:/users/ALICE/notes"),
            "could not read ~/notes"
        );
        // Another path is kept: it is what a diagnosis needs.
        assert_eq!(
            redactor.redact(r"D:\Projects\pane\pane.json"),
            r"D:\Projects\pane\pane.json"
        );
    }

    #[test]
    fn the_users_and_the_computers_names_go_in_any_case() {
        let redactor = redactor();
        assert_eq!(
            redactor.redact("ALICE asked desk-42 and Desk-42 for Alice's file"),
            "<user> asked <computer> and <computer> for <user>'s file"
        );
        assert_eq!(
            redactor.redact(r"\\DESK-42\share\alice"),
            r"\\<computer>\share\<user>"
        );
    }

    #[test]
    fn a_name_shorter_than_three_characters_is_left_alone() {
        let redactor = Redactor::new(Some(Path::new("/")), &["al".to_owned()], &["pc".to_owned()]);
        assert_eq!(
            redactor.redact("al on PC wrote / and /al"),
            "al on PC wrote / and /al"
        );
        // Three characters are enough.
        let redactor = Redactor::new(None, &["ali".to_owned()], &["box".to_owned()]);
        assert_eq!(redactor.redact("Ali's BOX"), "<user>'s <computer>");
    }

    #[test]
    fn a_name_is_redacted_only_as_a_whole_word_or_path_component() {
        let redactor = Redactor::new(None, &["alice".to_owned()], &["desk".to_owned()]);
        assert_eq!(
            redactor.redact("Alice's desk; malice, desktop, alice2, /home/ALICE/x"),
            "<user>'s <computer>; malice, desktop, alice2, /home/<user>/x"
        );
        // The home folder's path is a whole component too.
        let redactor = Redactor::new(Some(Path::new("/home/al")), &[], &[]);
        assert_eq!(
            redactor.redact("/home/al/notes and /home/alan/notes"),
            "~/notes and /home/alan/notes"
        );
    }

    #[test]
    fn ordinary_words_are_not_mangled_by_a_user_called_pane_or_admin() {
        let redactor = Redactor::new(
            Some(Path::new(r"C:\Users\Admin")),
            &["pane".to_owned(), "Admin".to_owned(), "root".to_owned()],
            &["localhost".to_owned()],
        );
        let message = "Pane could not open the panel: the Administrator of root search on \
                       localhost said no";
        assert_eq!(redactor.redact(message), message);
        // The home folder is still redacted.
        assert_eq!(
            redactor.redact(r"C:\Users\Admin\AppData\Local\Pane"),
            r"~\AppData\Local\Pane"
        );
        // A name that identifies someone still goes where it is a word.
        let redactor = Redactor::new(None, &["Paneer".to_owned()], &[]);
        assert_eq!(
            redactor.redact("Pane ran for paneer"),
            "Pane ran for <user>"
        );
    }

    #[test]
    fn names_with_letters_beyond_ascii_match_in_any_case() {
        let redactor = Redactor::new(None, &["Jürgen".to_owned()], &[]);
        assert_eq!(redactor.redact("JÜRGEN and jürgen"), "<user> and <user>");
    }

    #[test]
    fn this_systems_names_are_found() {
        // Whatever this computer is called, its home folder is redacted.
        let redactor = Redactor::of_this_system();
        let Some(home) = home_folder() else {
            return;
        };
        if home.to_string_lossy().len() >= MIN_CHARS {
            let path = home.join("notes.txt");
            let redacted = redactor.redact(&path.to_string_lossy());
            assert!(redacted.starts_with('~'), "{redacted}");
        }
    }
}
