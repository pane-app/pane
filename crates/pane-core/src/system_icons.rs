//! System icons (#142): the icon the system shows for a file or an
//! application, which an extension's icon may name by path
//! (`{"file": "<path>"}`, [`crate::IconSource::File`]). The host extracts
//! it, one adapter per system, as it finds applications (ADR 0015):
//!
//! - Windows: the shell's own image of the item
//!   (`IShellItemImageFactory`, icon only, at [`ICON_SIZE`] pixels): a
//!   document's type icon, a folder's, an application's or a shortcut's
//!   own, and a packaged application's by its `shell:AppsFolder\<id>`;
//! - macOS: `NSWorkspace`'s icon for the file, its largest image up to
//!   twice [`ICON_SIZE`];
//! - Linux: the icon theme's file for a desktop entry's `Icon`, a folder or
//!   a file's kind, looked up in the `hicolor`, Adwaita and Breeze themes
//!   and `pixmaps` of the data folders.
//!
//! Extraction runs off the window's and the runtime's threads, on the
//! launcher's icon loaders (`launcher::icon_loads`), which keep each icon
//! as a PNG in Pane's own folder and draw an application's bare, without a
//! tile (ADR 0035). The "Applications done properly" specification (#124,
//! ADR 0038) shares this extraction for the applications' icons and gives
//! it a cache of its own.

use std::path::{Path, PathBuf};

/// The size, in pixels each way, Pane asks the system for an icon at.
pub const ICON_SIZE: u32 = 256;

/// A system icon as the system gave it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemIcon {
    /// A PNG image Pane made of the pixels the system drew.
    Png(Vec<u8>),
    /// An image file the system's icon theme has (Linux), drawn as it is.
    File(PathBuf),
}

/// Extracts the icon the system shows for a file or an application.
pub trait SystemIcons: Send + Sync {
    /// The icon of the file, folder or application at `path`, or why the
    /// system has none. Blocks while the system draws it.
    fn icon(&self, path: &Path) -> Result<SystemIcon, String>;
}

/// This system's icons, extracted by its adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeIcons;

impl SystemIcons for NativeIcons {
    fn icon(&self, path: &Path) -> Result<SystemIcon, String> {
        // A name of Windows' shell (`shell:AppsFolder\<id>`) is the
        // system's to find; any other path must be there.
        let shell = path
            .to_string_lossy()
            .get(..6)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("shell:"));
        if !shell && !path.exists() {
            return Err(format!("{} does not exist", path.display()));
        }
        platform::icon(path)
    }
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use self::linux as platform;
#[cfg(target_os = "macos")]
use self::macos as platform;
#[cfg(target_os = "windows")]
use self::windows as platform;

/// Systems without an adapter have no system icons: their fallbacks show.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use super::SystemIcon;
    use std::path::Path;

    pub(super) fn icon(path: &Path) -> Result<SystemIcon, String> {
        Err(format!(
            "this system gives Pane no icon for {}",
            path.display()
        ))
    }
}

/// `bgra`, rows of premultiplied BGRA pixels as Windows draws them, as
/// straight-alpha RGBA. A bitmap whose alpha is zero throughout has none,
/// and is opaque.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn straight_rgba(bgra: &[u8]) -> Vec<u8> {
    let has_alpha = bgra.chunks_exact(4).any(|pixel| pixel[3] != 0);
    let mut rgba = Vec::with_capacity(bgra.len());
    for pixel in bgra.chunks_exact(4) {
        let alpha = if has_alpha { pixel[3] } else { 255 };
        let straight = |channel: u8| -> u8 {
            match alpha {
                0 => 0,
                255 => channel,
                _ => {
                    let alpha = u32::from(alpha);
                    ((u32::from(channel) * 255 + alpha / 2) / alpha).min(255) as u8
                }
            }
        };
        rgba.extend_from_slice(&[
            straight(pixel[2]),
            straight(pixel[1]),
            straight(pixel[0]),
            alpha,
        ]);
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplied_bgra_becomes_straight_rgba() {
        // Opaque blue, half-transparent red premultiplied, transparent.
        let bgra = [255, 0, 0, 255, 0, 0, 64, 128, 9, 9, 9, 0];
        assert_eq!(
            straight_rgba(&bgra),
            [0, 0, 255, 255, 128, 0, 0, 128, 0, 0, 0, 0]
        );
        // No alpha at all: opaque.
        assert_eq!(straight_rgba(&[1, 2, 3, 0]), [3, 2, 1, 255]);
    }

    #[test]
    fn a_path_that_does_not_exist_has_no_icon() {
        let folder = tempfile::tempdir().unwrap();
        let gone = folder.path().join("gone.txt");
        // Windows' shell draws a document icon for a name it does not find
        // only when asked by type; by path it fails.
        let found = NativeIcons.icon(&gone);
        assert!(found.is_err(), "{found:?}");
    }
}
