//! Gives `pane.exe` its icon and version information (#454): a Windows
//! resource script, written here and compiled into the program by
//! `embed-resource` — `rc.exe` from the Windows SDK on Windows, `llvm-rc`
//! when the program is built for Windows elsewhere. Any other system's
//! program gets nothing from this script: macOS takes its icon from the
//! `Pane.app` bundle the install script builds, Linux from the desktop.
//!
//! The icon is Pane's first icon resource (ID 1): the one Explorer, the
//! Start menu's shortcut and the taskbar show, and the one GPUI loads for
//! Pane's windows. The version information is the version `pane --version`
//! prints (`APP_VERSION`): the workspace's, or the one `cargo xtask
//! package-windows --package-version` gave the build.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PANE_PACKAGE_VERSION");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo names the crate"));
    let icon = manifest.join("assets/app/pane.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let version = env::var("PANE_PACKAGE_VERSION")
        .unwrap_or_else(|_| env::var("CARGO_PKG_VERSION").expect("cargo names the version"));
    let script =
        PathBuf::from(env::var("OUT_DIR").expect("cargo gives an output folder")).join("pane.rc");
    fs::write(
        &script,
        resource_script(&icon.display().to_string(), &version),
    )
    .expect("the resource script is written");
    embed_resource::compile(&script, embed_resource::NONE)
        .manifest_optional()
        .expect("pane.exe's icon and version information compile");
}

/// The resource script: the icon at `icon`, and the version information of
/// `version`. A version's numbers are its first three (a pre-release's
/// label is dropped from them, and kept in the strings), and the fourth is
/// 0.
fn resource_script(icon: &str, version: &str) -> String {
    let numbers: Vec<u16> = version
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .chain(std::iter::repeat(0))
        .take(4)
        .collect();
    let numeric = format!(
        "{},{},{},{}",
        numbers[0], numbers[1], numbers[2], numbers[3]
    );
    let icon = icon.replace('\\', "\\\\");
    format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEFLAGSMASK 0x3F
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "The Pane contributors"
      VALUE "FileDescription", "Pane"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "pane"
      VALUE "LegalCopyright", "Copyright (C) The Pane contributors. GPL-3.0-or-later."
      VALUE "OriginalFilename", "pane.exe"
      VALUE "ProductName", "Pane"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
}
