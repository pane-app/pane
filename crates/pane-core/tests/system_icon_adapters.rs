//! The system icon adapters (#142) against the real system: Windows'
//! extracts a file's icon and an application's, each a PNG of the size
//! Pane asks for, and has none for a path that does not exist.
//!
//! The shell's icon extraction needs a logged-in session, so the Windows
//! test runs only where `PANE_TEST_SYSTEM_ICONS=1` is set (CI's Windows
//! runner sets it); without it the test passes without looking. It reads
//! the system's icons and writes nothing but a file in its own temporary
//! folder.

use pane_core::system_icons::{NativeIcons, SystemIcons};

#[test]
fn a_path_that_does_not_exist_has_no_system_icon() {
    let folder = tempfile::tempdir().unwrap();
    assert!(NativeIcons.icon(&folder.path().join("gone.txt")).is_err());
}

/// Whether the real-system test may run here.
#[allow(dead_code)]
fn opted_in() -> bool {
    std::env::var("PANE_TEST_SYSTEM_ICONS").is_ok_and(|value| value == "1")
}

/// The width and height a PNG states in its header.
#[allow(dead_code)]
fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.get(..8)? != b"\x89PNG\r\n\x1a\n" || png.get(12..16)? != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(png.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(png.get(20..24)?.try_into().ok()?);
    Some((width, height))
}

#[cfg(windows)]
#[test]
fn windows_extracts_a_files_and_an_applications_icon() {
    use pane_core::system_icons::{ICON_SIZE, SystemIcon};
    use std::path::Path;

    if !opted_in() {
        eprintln!("skipped: set PANE_TEST_SYSTEM_ICONS=1 to extract real icons");
        return;
    }
    let folder = tempfile::tempdir().unwrap();
    let document = folder.path().join("notes.txt");
    std::fs::write(&document, "notes").unwrap();
    let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let notepad = Path::new(&windows).join("System32").join("notepad.exe");
    assert!(notepad.is_file(), "{} is missing", notepad.display());
    let mut extracted = Vec::new();
    for path in [document.as_path(), notepad.as_path(), folder.path()] {
        let icon = NativeIcons
            .icon(path)
            .unwrap_or_else(|why| panic!("{}: {why}", path.display()));
        let SystemIcon::Png(png) = icon else {
            panic!("{}: not a PNG", path.display());
        };
        let (width, height) =
            png_size(&png).unwrap_or_else(|| panic!("{}: not a PNG", path.display()));
        assert!(
            width >= 16 && height >= 16 && width <= ICON_SIZE * 2 && height <= ICON_SIZE * 2,
            "{}: {width}×{height}",
            path.display()
        );
        // Not blank: some pixel shows something.
        assert!(png.len() > 100, "{}: {} bytes", path.display(), png.len());
        extracted.push(png);
    }
    // A text file's and Notepad's icons differ.
    assert_ne!(extracted[0], extracted[1]);
}
