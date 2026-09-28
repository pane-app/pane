//! Operating systems a package or an action declares it supports, and the
//! explanation Pane gives where one is unavailable. Deliberately simple: a
//! list of systems, not a rule language.

use std::fmt;

/// An operating system Pane runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    Windows,
    Macos,
    Linux,
}

impl Platform {
    /// Every platform, in the order Pane lists them.
    pub const ALL: [Platform; 3] = [Platform::Windows, Platform::Macos, Platform::Linux];

    /// The system this Pane was built for, or `None` on a system other than
    /// Windows, macOS and Linux.
    pub fn current() -> Option<Platform> {
        if cfg!(target_os = "windows") {
            Some(Platform::Windows)
        } else if cfg!(target_os = "macos") {
            Some(Platform::Macos)
        } else if cfg!(target_os = "linux") {
            Some(Platform::Linux)
        } else {
            None
        }
    }

    /// The name used in `pane.json`: `windows`, `macos` or `linux`.
    pub fn id(self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Macos => "macos",
            Platform::Linux => "linux",
        }
    }

    pub(crate) fn from_id(id: &str) -> Option<Platform> {
        Platform::ALL
            .into_iter()
            .find(|platform| platform.id() == id)
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Platform::Windows => "Windows",
            Platform::Macos => "macOS",
            Platform::Linux => "Linux",
        })
    }
}

/// People's names for `platforms`: "Windows", "Windows and Linux",
/// "Windows, macOS and Linux".
fn names(platforms: &[Platform]) -> String {
    let names: Vec<String> = platforms.iter().map(Platform::to_string).collect();
    join(&names)
}

/// Joins `names` as a list in a sentence: "a", "a and b", "a, b and c".
pub(crate) fn join(names: &[String]) -> String {
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// Why something that `subject` names ("this action", "this package")
/// cannot be used on this system, given the platforms it supports; `None`
/// when it can, or when it declares none (it then supports every system).
pub(crate) fn unavailable(supported: Option<&[Platform]>, subject: &str) -> Option<String> {
    let supported = supported?;
    let current = Platform::current();
    if current.is_some_and(|current| supported.contains(&current)) {
        return None;
    }
    let here = match current {
        Some(current) => current.to_string(),
        None => format!("this system ({})", std::env::consts::OS),
    };
    Some(if supported.is_empty() {
        format!("Not available on {here}: {subject} supports no operating system")
    } else {
        format!(
            "Not available on {here}: {subject} supports only {}",
            names(supported)
        )
    })
}
