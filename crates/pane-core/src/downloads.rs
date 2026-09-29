//! Packages Pane downloads (from npm or Git) before installing them, each
//! written into a folder of its own under the data folder's
//! `extensions/downloads/`, and the names Pane lets such a package hold.
//!
//! A download is removed when the last package read from it is dropped:
//! once a preview is shown, once an install ends, and on every failure. No
//! two downloads share a folder, so removing one never removes what another
//! preview or install is reading. A starting Pane removes only downloads
//! begun more than [`ABANDONED_AFTER`] ago.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// A downloaded package written into a folder of its own in Pane's
/// downloads folder, which is removed when this is dropped: once whatever
/// read it (a preview, an install, a plan's dependency) is done with it,
/// whether it was installed or refused.
#[derive(Debug)]
pub(crate) struct Download {
    folder: PathBuf,
}

impl Download {
    /// Makes a new folder in `downloads`, named by when it was begun
    /// (seconds since 1970), this process and a count
    /// (`<seconds>-<process>-<count>`), has `fill` write the package into it
    /// while it is named `.<seconds>-<process>-<count>`, and renames it once
    /// `fill` succeeded. On a failure nothing is left.
    pub(crate) fn create(
        downloads: &Path,
        fill: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<Download, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let begun = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let name = format!(
            "{begun}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        fs::create_dir_all(downloads).map_err(|error| error.to_string())?;
        let partial = downloads.join(format!(".{name}"));
        let filled = fill(&partial);
        let folder = downloads.join(&name);
        let filled =
            filled.and_then(|()| fs::rename(&partial, &folder).map_err(|error| error.to_string()));
        if let Err(why) = filled {
            let _ = fs::remove_dir_all(&partial);
            return Err(why);
        }
        Ok(Download { folder })
    }

    /// The downloaded package.
    pub(crate) fn folder(&self) -> &Path {
        &self.folder
    }
}

impl Drop for Download {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.folder);
    }
}

/// How long a download may stay in the downloads folder before a starting
/// Pane takes it as abandoned (left by a Pane that stopped) and removes it;
/// a younger one may be another Pane's preview or install in progress.
pub const ABANDONED_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// Removes what `downloads` holds that was begun more than
/// [`ABANDONED_AFTER`] before `now`, or whose name does not say when it was
/// begun, when Pane starts: downloads a Pane that stopped left behind. A
/// younger one is kept, since another Pane on this data folder may be
/// previewing or installing from it.
pub(crate) fn remove_abandoned(downloads: &Path, now: std::time::SystemTime) {
    let Ok(entries) = fs::read_dir(downloads) else {
        return;
    };
    let now = now
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    for entry in entries.flatten() {
        let name = entry.file_name();
        let begun = name
            .to_str()
            .map(|name| name.trim_start_matches('.'))
            .and_then(|name| name.split('-').next())
            .and_then(|seconds| seconds.parse::<u64>().ok());
        let abandoned =
            begun.is_none_or(|begun| begun.saturating_add(ABANDONED_AFTER.as_secs()) < now);
        if abandoned {
            let path = entry.path();
            let _ = fs::remove_dir_all(&path).or_else(|_| fs::remove_file(&path));
        }
    }
}

/// The longest name, in bytes, every system takes: ext4's and APFS's limit
/// (NTFS takes 255 UTF-16 units, which 255 bytes of UTF-8 never exceed).
const MAX_NAME: usize = 255;

/// Checks one part of a path in a downloaded package, the same on every
/// system, so that a package Pane unpacks on one unpacks on all: at most
/// [`MAX_NAME`] bytes (checked first, so that a longer one is not read),
/// exactly one plain name (no `/`, `\`, `.` or `..`, so that joining it to
/// a folder names something inside that folder), no `:`, `<`, `>`, `"`,
/// `|`, `?`, `*` or control character, no trailing `.` or space, and no
/// Windows device name (`con`, `conin$`, `nul`, `com1`, `lpt³`…, with or
/// without an extension), compared by character.
pub(crate) fn check_part(part: &str) -> Result<(), &'static str> {
    if part.len() > MAX_NAME {
        return Err("whose name is longer than the 255 bytes every system takes");
    }
    match part {
        "" | "." => return Err("which has an empty or `.` part"),
        ".." => return Err("which climbs out with `..`"),
        _ => {}
    }
    if part.contains(['/', '\\']) {
        return Err("whose name holds `/` or `\\`, which would put it in another folder");
    }
    // Whatever this system reads as more than one plain name (a prefix, a
    // root, `.` or `..`) is refused too, not only what the checks above know.
    let mut components = Path::new(part).components();
    if !matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    ) {
        return Err("whose name is not one plain name");
    }
    if part
        .chars()
        .any(|c| matches!(c, '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*') || c.is_control())
    {
        return Err("whose name has a character Windows does not allow, such as `\\`, `:` or `?`");
    }
    if part.ends_with('.') || part.ends_with(' ') {
        return Err("whose name ends with `.` or a space");
    }
    // Windows reads a device name whatever follows its first dot, and
    // ignores spaces before it.
    let stem: Vec<char> = part
        .split('.')
        .next()
        .unwrap_or(part)
        .trim_end_matches(' ')
        .chars()
        .flat_map(char::to_lowercase)
        // The longest device name has 7 characters: 8 tell it from them.
        .take(8)
        .collect();
    let is = |name: &str| stem.iter().copied().eq(name.chars());
    let numbered = |prefix: &str| {
        stem.len() == 4
            && stem[..3].iter().copied().eq(prefix.chars())
            && matches!(stem[3], '0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}')
    };
    let device = ["con", "prn", "aux", "nul", "conin$", "conout$"]
        .iter()
        .any(|name| is(name))
        || numbered("com")
        || numbered("lpt");
    if device {
        return Err("which is a Windows device name");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_is_one_plain_name() {
        for name in ["pane.json", "dist", "x.wasm", ".gitattributes", "a b", "é"] {
            assert_eq!(check_part(name), Ok(()), "{name}");
        }
        // Each of these would put a file elsewhere than under one name inside
        // the folder it is joined to: in a subfolder, above it, or anywhere
        // (an absolute path replaces the folder it is joined to).
        for name in [
            "a/b",
            "../x",
            "/abs",
            "/abs/path",
            "sub/.git",
            "..",
            ".",
            "",
            "a\\b",
            "..\\x",
            "\\abs",
            "a/",
            "/",
            "./a",
        ] {
            assert!(check_part(name).is_err(), "{name:?} was taken");
        }
    }

    #[test]
    fn a_part_is_at_most_255_bytes() {
        assert_eq!(check_part(&"a".repeat(255)), Ok(()));
        assert_eq!(check_part(&"é".repeat(127)), Ok(()));
        for name in ["a".repeat(256), "é".repeat(128), "a".repeat(1 << 20)] {
            assert_eq!(
                check_part(&name),
                Err("whose name is longer than the 255 bytes every system takes"),
                "{}",
                name.len()
            );
        }
    }
}
