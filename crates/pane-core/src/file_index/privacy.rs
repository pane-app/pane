//! macOS's privacy refusals (#126 story 55): macOS asks the user before an
//! application reads the home folder's Desktop, Documents and Downloads
//! (and removable and network volumes), and a folder the user refused is
//! listed on the File search page with how to allow it.
//!
//! A refusal is told from what the system answered when the walker opened
//! the folder, not from the folder's name: macOS's privacy protection
//! answers a read it refuses with `EPERM` ("Operation not permitted"),
//! whichever folder it protects, while a folder the account's own file
//! permissions keep out answers `EACCES` ("Permission denied") and is
//! listed as unreadable, with the remedy that fits it. Other systems have
//! no such protection: every refusal there is an unreadable folder.

use std::io;

/// `EPERM`, "Operation not permitted": the same value on every Unix (and
/// named here so that the classification is tested on every system).
const EPERM: i32 = 1;

/// Whether `error`, which opening or listing a folder answered, is macOS's
/// privacy protection refusing Pane, on this system.
pub(crate) fn refused_by_privacy(error: &io::Error) -> bool {
    privacy_refusal(error.raw_os_error(), cfg!(target_os = "macos"))
}

/// Whether the system's answer `code` is a privacy refusal, on macOS when
/// `macos`: only macOS refuses for privacy, and it answers `EPERM`.
fn privacy_refusal(code: Option<i32>, macos: bool) -> bool {
    macos && code == Some(EPERM)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `EACCES`, "Permission denied", on every Unix.
    const EACCES: i32 = 13;
    /// `ENOENT`, "No such file or directory", on every Unix.
    const ENOENT: i32 = 2;

    #[test]
    fn a_refusal_is_told_from_the_systems_answer_not_the_folders_name() {
        // macOS's privacy protection answers EPERM, whatever the folder.
        assert!(privacy_refusal(Some(EPERM), true));
        // The account's own permissions, a folder gone meanwhile and an
        // answer with no code are unreadable folders, not refusals.
        assert!(!privacy_refusal(Some(EACCES), true));
        assert!(!privacy_refusal(Some(ENOENT), true));
        assert!(!privacy_refusal(None, true));
        // Only macOS refuses for privacy.
        assert!(!privacy_refusal(Some(EPERM), false));
    }

    #[test]
    fn an_error_without_a_code_is_no_refusal() {
        let error = io::Error::other("the walker gave up");
        assert!(!refused_by_privacy(&error));
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        assert!(!refused_by_privacy(&denied));
    }
}
