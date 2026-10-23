//! The installers a release carries beside each system's package (#464),
//! for a user who installs Pane as they install any other application:
//! macOS's disk image, which holds the whole `Pane.app` beside a link to
//! `/Applications` to drag it onto, and Linux's AppImage, one runnable
//! file holding the program with its desktop entry and icons. Both are
//! built from the files the package was staged from (`stage/pane`), so
//! they hold the same program, licence and notices as the package; and,
//! unlike the package, neither is the same bytes wherever it is built
//! (`hdiutil` and `mksquashfs` write times of their own), which the
//! `.sha256` beside each covers.
//!
//! Pane's updater keeps taking the package on macOS, the program a
//! `Pane.app` holds being the one the zip holds; a Pane running from an
//! AppImage takes the release's AppImage (ADR 0049).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::{VERSION, arch, read_files, run, stage_folder, write_sha256};

/// Builds `Pane-<version>[-dev].dmg` in `out` from the staged package, and
/// `Pane-<version>.dmg` in `release`, each with its `.sha256`: a
/// compressed disk image whose volume holds `Pane.app` — the program, the
/// `Info.plist` the package carries (naming `version`) and the icon it
/// names, with the licence and notices in its `Resources` — and a link to
/// `/Applications`, which Finder shows beside it to drag it onto. The
/// bundle is the one the zip's install script builds, so an update, which
/// replaces its program and rewrites its `Info.plist`, finds the layout it
/// expects wherever the user put it. Not signed, as nothing is yet.
pub(super) fn dmg(
    out: &Path,
    release: &Path,
    dev: bool,
    version: Option<&str>,
) -> Result<PathBuf, String> {
    let version = version.unwrap_or(VERSION);
    let stage = stage_folder(out);
    let volume = out.join("dmg");
    let _ = fs::remove_dir_all(&volume);
    let contents = volume.join("Pane.app/Contents");
    for (from, to) in [
        ("pane", "MacOS/pane"),
        ("Info.plist", "Info.plist"),
        ("pane.icns", "Resources/pane.icns"),
        ("LICENSE.txt", "Resources/LICENSE.txt"),
        (
            "THIRD-PARTY-NOTICES.txt",
            "Resources/THIRD-PARTY-NOTICES.txt",
        ),
    ] {
        copy(&stage.join(from), &contents.join(to))?;
    }
    symlink("/Applications", &volume.join("Applications"))?;
    let suffix = if dev { "-dev" } else { "" };
    let name = format!("Pane-{version}{suffix}.dmg");
    let image = out.join(&name);
    let _ = fs::remove_file(&image);
    // `hdiutil create` on a CI runner now and then fails at once with
    // "Resource busy" (a disk-arbitration race of its own); a second try
    // succeeds. A failure that keeps happening is a real one.
    let mut tries = 0;
    loop {
        let mut create = Command::new("hdiutil");
        create
            .args(["create", "-quiet", "-ov", "-volname", "Pane", "-fs", "HFS+"])
            .args(["-format", "UDZO", "-srcfolder"])
            .arg(&volume)
            .arg(&image);
        match run(&mut create) {
            Ok(()) => break,
            Err(why) if tries < 2 => {
                tries += 1;
                eprintln!("{why}; trying again");
                std::thread::sleep(Duration::from_secs(3));
            }
            Err(why) => return Err(why),
        }
    }
    // The volume's folder goes once the image holds it: its link to
    // /Applications would otherwise sit in `target/dist`, which the Package
    // job uploads, following links.
    let _ = fs::remove_dir_all(&volume);
    publish(out, release, &name, &format!("Pane-{version}.dmg"))?;
    Ok(image)
}

/// Builds `Pane-<version>-<arch>[-dev].AppImage` in `out` from the staged
/// package, and `Pane-<version>-<arch>.AppImage` in `release`, each with
/// its `.sha256`, when `APPIMAGETOOL` names the `appimagetool` to build it
/// with and `APPIMAGE_RUNTIME` the AppImage runtime it puts at the file's
/// head (a pinned type 2 runtime, which needs no `libfuse2` on the user's
/// system; given as a file, so `appimagetool` downloads nothing). `None`,
/// building nothing, when either is not named.
///
/// The AppDir holds what the AppImage specification asks for and what an
/// integration tool (AppImageLauncher, Gear Lever, appimaged) reads to put
/// Pane in the user's menus: `AppRun` (a link to the program, which the
/// runtime starts with `APPIMAGE` and `APPDIR` set), the desktop entry and
/// its icon at the top, and the same under `usr/share` — the entry in
/// `applications`, every icon size in `icons/hicolor` — with the licence
/// and notices in `usr/share/doc/pane`. `appimagetool` runs extracted
/// (`APPIMAGE_EXTRACT_AND_RUN`), so it needs no FUSE on the machine that
/// builds.
pub(super) fn appimage(
    out: &Path,
    release: &Path,
    dev: bool,
    version: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let (Some(tool), Some(runtime)) = (
        std::env::var_os("APPIMAGETOOL"),
        std::env::var_os("APPIMAGE_RUNTIME"),
    ) else {
        return Ok(None);
    };
    if !cfg!(target_os = "linux") {
        return Err(format!(
            "an AppImage holds a Linux program, which only a Linux checkout builds; this one \
             runs on {}",
            std::env::consts::OS
        ));
    }
    let version = version.unwrap_or(VERSION);
    let arch = arch().ok_or("the AppImage names no architecture for this system")?;
    let stage = stage_folder(out);
    let folder = out.join("appimage");
    let _ = fs::remove_dir_all(&folder);
    let appdir = folder.join("Pane.AppDir");
    let usr = appdir.join("usr");
    copy(&stage.join("pane"), &usr.join("bin/pane"))?;
    for entry in [
        appdir.join("pane.desktop"),
        usr.join("share/applications/pane.desktop"),
    ] {
        copy(&stage.join("pane.desktop"), &entry)?;
    }
    for notice in ["LICENSE.txt", "THIRD-PARTY-NOTICES.txt"] {
        copy(
            &stage.join(notice),
            &usr.join("share/doc/pane").join(notice),
        )?;
    }
    for (path, _) in read_files(&stage.join("icons"), "")? {
        copy(
            &stage.join("icons").join(&path),
            &usr.join("share/icons").join(&path),
        )?;
    }
    copy(
        &stage.join("icons/hicolor/256x256/apps/pane.png"),
        &appdir.join("pane.png"),
    )?;
    symlink("pane.png", &appdir.join(".DirIcon"))?;
    symlink("usr/bin/pane", &appdir.join("AppRun"))?;
    let suffix = if dev { "-dev" } else { "" };
    let name = format!("Pane-{version}-{arch}{suffix}.AppImage");
    let image = out.join(&name);
    let _ = fs::remove_file(&image);
    let mut build = Command::new(tool);
    build
        .arg("--no-appstream")
        .arg("--runtime-file")
        .arg(runtime)
        .arg(&appdir)
        .arg(&image)
        .env("ARCH", arch)
        .env("APPIMAGE_EXTRACT_AND_RUN", "1");
    run(&mut build)?;
    // As the disk image's volume: the AppDir and its links leave
    // `target/dist` once the AppImage holds them.
    let _ = fs::remove_dir_all(&folder);
    publish(
        out,
        release,
        &name,
        &format!("Pane-{version}-{arch}.AppImage"),
    )?;
    Ok(Some(image))
}

/// Writes the `.sha256` of the installer `name` in `out`, and copies it
/// into `release` as `released` (the name a release carries it by), with
/// its own `.sha256`. Copied, not written, so the AppImage stays runnable.
fn publish(out: &Path, release: &Path, name: &str, released: &str) -> Result<(), String> {
    let built = out.join(name);
    let bytes =
        fs::read(&built).map_err(|error| format!("read {} failed: {error}", built.display()))?;
    write_sha256(out, name, &bytes)?;
    fs::copy(&built, release.join(released))
        .map_err(|error| format!("copy {} failed: {error}", built.display()))?;
    write_sha256(release, released, &bytes)
}

/// Copies `from` to `to`, making the folders it goes in; the copy keeps
/// the file's permissions, so the program stays runnable.
fn copy(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to.parent().expect("a file in a folder"))
        .and_then(|()| fs::copy(from, to))
        .map(|_| ())
        .map_err(|error| format!("copy {} failed: {error}", from.display()))
}

/// Makes `link` a symbolic link to `target`, as the disk image's
/// Applications link and the AppDir's `AppRun` and `.DirIcon` are. Only
/// macOS and Linux build those, so elsewhere it explains so.
fn symlink(target: &str, link: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
            .map_err(|error| format!("link {} failed: {error}", link.display()))
    }
    #[cfg(not(unix))]
    {
        Err(format!(
            "{} is a symbolic link to {target}, which only a macOS or Linux checkout makes here",
            link.display()
        ))
    }
}
