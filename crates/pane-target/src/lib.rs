// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The operating systems and processors Pane names: a package's supported
//! platforms (`windows`, `macos`, `linux`) and a native helper's targets,
//! written `<os>-<arch>` such as `linux-x86_64`. One mapping of ids and
//! people's names, shared by Pane, its build tasks and the helper sample.
//! No dependencies, so a native helper can use it too.

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

    /// The system this was built for, or `None` on a system other than
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

    /// The platform whose [`Platform::id`] is `id`.
    pub fn from_id(id: &str) -> Option<Platform> {
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

/// A processor a native helper is built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arch {
    X86_64,
    Aarch64,
}

impl Arch {
    /// Every processor, in the order Pane lists them.
    pub const ALL: [Arch; 2] = [Arch::X86_64, Arch::Aarch64];

    /// The processor this was built for, or `None` for another one.
    pub fn current() -> Option<Arch> {
        Arch::from_id(std::env::consts::ARCH)
    }

    /// The name used in a helper target: `x86_64` or `aarch64`, as Rust's
    /// `std::env::consts::ARCH` names them.
    pub fn id(self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
        }
    }

    /// The processor whose [`Arch::id`] is `id`.
    pub fn from_id(id: &str) -> Option<Arch> {
        Arch::ALL.into_iter().find(|arch| arch.id() == id)
    }
}

impl fmt::Display for Arch {
    /// People's name: "x86-64" or "arm64".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Arch::X86_64 => "x86-64",
            Arch::Aarch64 => "arm64",
        })
    }
}

/// An operating system and processor: what a native helper's file is built
/// for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Target {
    pub os: Platform,
    pub arch: Arch,
}

impl Target {
    /// The target this was built for, or `None` on a system or processor
    /// Pane does not name.
    pub fn current() -> Option<Target> {
        Some(Target {
            os: Platform::current()?,
            arch: Arch::current()?,
        })
    }

    /// Reads `<os>-<arch>`, such as `linux-x86_64`.
    pub fn parse(id: &str) -> Option<Target> {
        let (os, arch) = id.split_once('-')?;
        Some(Target {
            os: Platform::from_id(os)?,
            arch: Arch::from_id(arch)?,
        })
    }

    /// `<os>-<arch>`, such as `macos-aarch64`, as `pane.json` writes it.
    pub fn id(self) -> String {
        format!("{}-{}", self.os.id(), self.arch.id())
    }

    /// The file name ending a program for this target needs: `.exe` on
    /// Windows, none elsewhere.
    pub fn exe_suffix(self) -> &'static str {
        match self.os {
            Platform::Windows => ".exe",
            Platform::Macos | Platform::Linux => "",
        }
    }
}

impl fmt::Display for Target {
    /// People's name: "Linux x86-64", "macOS arm64".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.os, self.arch)
    }
}

impl PartialOrd for Target {
    fn partial_cmp(&self, other: &Target) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Target {
    /// By id, so `pane.json`'s targets list alphabetically.
    fn cmp(&self, other: &Target) -> std::cmp::Ordering {
        self.id().cmp(&other.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_read_and_name_a_system_and_processor() {
        let mac = Target::parse("macos-aarch64").unwrap();
        assert_eq!(mac.os, Platform::Macos);
        assert_eq!(mac.arch, Arch::Aarch64);
        assert_eq!(mac.id(), "macos-aarch64");
        assert_eq!(mac.to_string(), "macOS arm64");
        assert_eq!(
            Target::parse("windows-x86_64").unwrap().to_string(),
            "Windows x86-64"
        );
        for unknown in ["linux-arm64", "freebsd-x86_64", "linux", "linux-", ""] {
            assert_eq!(Target::parse(unknown), None, "{unknown}");
        }
        if let Some(here) = Target::current() {
            assert_eq!(Target::parse(&here.id()), Some(here));
        }
        let mut targets = [mac, Target::parse("linux-x86_64").unwrap()];
        targets.sort();
        assert_eq!(targets[0].id(), "linux-x86_64");
    }
}
