//! What the head of a command line names, as the run adapter acts on it.
//! Plain text work, compiled and tested on every system.
//!
//! The kinds, in the order they are told apart:
//!
//! - an address with a scheme — two or more letters, digits, `+`, `-` or
//!   `.` before a `:` — such as `shell:windows`, `ms-settings:display`,
//!   `https://example.com` or another registered scheme, opened by the
//!   system's own open (a single letter before the `:` is a drive, not a
//!   scheme);
//! - a rooted path — a drive (`C:\…`) or a network path (`\\server\…`) —
//!   a Control Panel applet when it ends in `.cpl`, else a program,
//!   document, folder or network path, opened by the system's own open,
//!   which also opens a folder in File Explorer;
//! - a head with a separator but no root (a relative path, which the
//!   system opens as it opens any other);
//! - otherwise a bare name, a program resolved on the search path and in
//!   App Paths — `mmc`, `notepad`, a Control Panel applet's own name.

use super::parse;

/// What the head of a command line names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A Control Panel applet (`.cpl`), run through the Control Panel
    /// program.
    Applet {
        /// The applet's path.
        path: String,
    },
    /// A program by its bare name, resolved on the search path and in App
    /// Paths.
    Program {
        /// The name as typed.
        name: String,
    },
    /// A path: a program, a document, a folder or a network path, opened
    /// by the system's own open.
    Path {
        /// The path as given (rooted, unless a relative path was typed).
        path: String,
    },
    /// An address with a registered scheme (`shell:`, `ms-settings:`,
    /// `https:`, …), opened by the system's own open.
    Address {
        /// The address as given.
        address: String,
    },
}

/// What `head`, the expanded head of a command line, names.
pub fn classify(head: &str) -> Target {
    // Two or more scheme characters before a colon name a scheme; one
    // letter before it names a drive.
    if let Some((scheme, _)) = head.split_once(':')
        && scheme.len() >= 2
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return Target::Address {
            address: head.to_owned(),
        };
    }
    if parse::rooted(head) {
        if extension(head).is_some_and(|extension| extension.eq_ignore_ascii_case("cpl")) {
            return Target::Applet {
                path: head.to_owned(),
            };
        }
        return Target::Path {
            path: head.to_owned(),
        };
    }
    if head.contains(['\\', '/']) {
        return Target::Path {
            path: head.to_owned(),
        };
    }
    Target::Program {
        name: head.to_owned(),
    }
}

/// `head`'s file extension, if it has one.
fn extension(head: &str) -> Option<&str> {
    head.rsplit_once('.').map(|(_, extension)| extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemes_are_addresses_and_drives_are_paths() {
        assert_eq!(
            classify("shell:windows"),
            Target::Address {
                address: "shell:windows".into()
            }
        );
        assert_eq!(
            classify("ms-settings:display"),
            Target::Address {
                address: "ms-settings:display".into()
            }
        );
        assert_eq!(
            classify("https://example.com"),
            Target::Address {
                address: "https://example.com".into()
            }
        );
        assert_eq!(
            classify("mailto:someone@example.com"),
            Target::Address {
                address: "mailto:someone@example.com".into()
            }
        );
        // One letter before the colon is a drive.
        assert_eq!(
            classify(r"C:\Windows\System32"),
            Target::Path {
                path: r"C:\Windows\System32".into()
            }
        );
        assert_eq!(classify(r"C:"), Target::Path { path: r"C:".into() });
    }

    #[test]
    fn a_cpl_is_an_applet_and_other_rooted_paths_are_paths() {
        assert_eq!(
            classify(r"C:\Windows\System32\desk.cpl"),
            Target::Applet {
                path: r"C:\Windows\System32\desk.cpl".into()
            }
        );
        // The extension matches ignoring case.
        assert_eq!(
            classify(r"C:\Windows\System32\DESK.CPL"),
            Target::Applet {
                path: r"C:\Windows\System32\DESK.CPL".into()
            }
        );
        assert_eq!(
            classify(r"C:\Program Files\Tool\tool.exe"),
            Target::Path {
                path: r"C:\Program Files\Tool\tool.exe".into()
            }
        );
        assert_eq!(
            classify(r"C:\Users\v\readme.txt"),
            Target::Path {
                path: r"C:\Users\v\readme.txt".into()
            }
        );
        assert_eq!(
            classify(r"C:\Users\v"),
            Target::Path {
                path: r"C:\Users\v".into()
            }
        );
        // A network path is a rooted one, kept as it is.
        assert_eq!(
            classify(r"\\server\share\folder"),
            Target::Path {
                path: r"\\server\share\folder".into()
            }
        );
    }

    #[test]
    fn a_bare_name_is_a_program_and_a_relative_path_a_path() {
        assert_eq!(
            classify("notepad"),
            Target::Program {
                name: "notepad".into()
            }
        );
        assert_eq!(
            classify("desk.cpl"),
            Target::Program {
                name: "desk.cpl".into()
            },
            "an applet by its bare name is found on the search path and run \
             through the Control Panel program"
        );
        assert_eq!(
            classify("folder/tool.exe"),
            Target::Path {
                path: "folder/tool.exe".into()
            }
        );
    }
}
