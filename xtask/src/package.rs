//! Building Pane's packages and the artifacts its own updates are
//! downloaded from (#53 for Linux, #51 for Windows, #52 for macOS).
//!
//! `package-linux`, `package-windows` and `package-macos` each produce,
//! under `target/dist/`:
//!
//! - `pane-<version>-<os>-<arch>[-dev].tar.gz` (Linux) or `.zip`
//!   (Windows and macOS) — the package a clean machine of that system
//!   installs from: the `pane` program, the install script and a README
//!   (and, on Linux, a desktop entry, on macOS the `Info.plist` of the
//!   `Pane.app` bundle the install script makes), and none of the default
//!   extensions (internet-first: Pane fetches them at first setup, from
//!   the commits this release pins). A `.sha256` file beside it names its
//!   digest; nothing is signed, since no signing credentials exist yet.
//! - `artifacts/` — what an artifact source serves: the index document
//!   `pane-defaults.json`, naming the application package a Pane
//!   application update downloads, and the package itself. A real
//!   deployment serves this folder at Pane's published downloads; the
//!   tests and smokes serve it from this computer
//!   (`scripts/artifact_server.py`) instead, so no check reaches the
//!   network.
//!
//! `--dev` builds the package's program in the development profile: the
//! native smokes install that one, because only a development build takes
//! its artifact source from `PANE_ARTIFACTS` and its default extensions'
//! pins from `PANE_DEFAULTS` (a release build uses Pane's published
//! downloads and the committed pins, which no controlled source may
//! replace). Without it, the release profile is built.
//!
//! The index the artifacts hold names the application package a Pane
//! application update downloads (#54): its `application` entry, with the
//! package's version, file name, sha512 integrity, size and target.
//! `--package-version <version>` builds the program reporting that
//! version and names the package and its entry by it — for the smokes,
//! which need a newer version to offer than the one installed (a real
//! release builds the workspace's own version, and no override is
//! given).
//!
//! Each task builds the package for the system it runs on, so a release
//! for several systems builds one package per system (this machine builds
//! the Linux one, CI's `windows-2025`, `macos-15` and `ubuntu-24.04`
//! runners theirs). `package-windows` and `package-macos` run everywhere
//! far enough to assemble the artifacts, then refuse anywhere but their
//! own system: `pane.exe` needs a Windows build and the `pane` program a
//! macOS one (no cross toolchain is set up), and packing another system's
//! program under that system's package name would be worse than
//! explaining so.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::PACKED_MTIME;

use sha2::{Digest, Sha256, Sha512};

use crate::zip;

/// The pane program's version, as the package names it: this workspace's
/// version, which every crate of it shares.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Which system one of the package tasks builds for.
#[derive(Clone, Copy)]
enum System {
    Linux,
    Windows,
    Macos,
}

impl System {
    /// The system's id, as the package's name and the docs say it.
    fn id(self) -> &'static str {
        match self {
            System::Linux => "linux",
            System::Windows => "windows",
            System::Macos => "macos",
        }
    }

    /// The install script the package holds: where the repository keeps it,
    /// and the name it is packed under.
    fn install_script(self) -> (&'static str, &'static str) {
        match self {
            System::Linux => ("scripts/install-linux.sh", "install.sh"),
            System::Windows => ("scripts/install-windows.ps1", "install.ps1"),
            System::Macos => ("scripts/install-macos.sh", "install.sh"),
        }
    }

    /// How the package is packed: the tarball Linux unpacks with `tar`, or
    /// the zip a Windows user unzips with whatever is at hand (Windows has
    /// no tar a user can rely on) and a macOS user opens with one
    /// double-click in Finder — both written by the same fixed-bytes zip
    /// writer, whose bytes a test pins.
    fn archive(self) -> &'static str {
        match self {
            System::Linux => "tar.gz",
            System::Windows | System::Macos => "zip",
        }
    }

    /// The one file this system's package holds besides the program, the
    /// install script and the README: Linux's desktop entry, which Windows'
    /// install script replaces with a Start-menu shortcut and macOS's
    /// `Info.plist`, which its script builds the `Pane.app` bundle around.
    fn extra_file(self) -> Option<(&'static str, String)> {
        match self {
            System::Linux => Some(("pane.desktop", desktop_entry())),
            System::Windows => None,
            System::Macos => Some(("Info.plist", app_plist())),
        }
    }
}

/// Builds the Linux package and the artifacts an artifact source serves:
/// the index naming the application package this task also built, and
/// the package itself.
pub fn linux(dev: bool, version: Option<String>) -> Result<(), String> {
    let root = root();
    let out = root.join("target/dist");
    fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    let artifacts = artifacts_folder(&out)?;
    build_program(&root, dev, version.as_deref())?;
    let (package, packed) = assemble_package(&root, &out, dev, System::Linux, version.as_deref())?;
    let target = target_id()?;
    let application = application_entry(
        version.as_deref().unwrap_or(VERSION),
        &package,
        &packed,
        &target,
    );
    write_index(&artifacts, Some(&application))?;
    serve_package(&artifacts, &package)?;
    println!("package built into {}", package.display());
    println!("artifacts built into {}", artifacts.display());
    Ok(())
}

/// Builds the Windows package and the same artifacts.
pub fn windows(dev: bool, version: Option<String>) -> Result<(), String> {
    let root = root();
    let out = root.join("target/dist");
    fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    let artifacts = artifacts_folder(&out)?;
    // The program comes last, so everything else the task builds is built
    // wherever it runs; but pane.exe can only be built by a Windows
    // checkout (no cross toolchain is set up: a Windows program needs a
    // Windows build), and packing another system's program under a
    // Windows package's name would be worse than explaining so. CI's
    // `windows-2025` runner builds the package itself.
    if !cfg!(target_os = "windows") {
        // No package was built, so the index names no application package.
        write_index(&artifacts, None)?;
        return Err(format!(
            "package-windows builds the pane program for Windows, which only a Windows checkout \
             can build; this one runs on {}. The artifacts under {} are assembled for this \
             system, and a Windows run re-assembles them for windows-x86_64",
            std::env::consts::OS,
            artifacts.display()
        ));
    }
    build_program(&root, dev, version.as_deref())?;
    let (package, packed) =
        assemble_package(&root, &out, dev, System::Windows, version.as_deref())?;
    let target = target_id()?;
    let application = application_entry(
        version.as_deref().unwrap_or(VERSION),
        &package,
        &packed,
        &target,
    );
    write_index(&artifacts, Some(&application))?;
    serve_package(&artifacts, &package)?;
    println!("package built into {}", package.display());
    println!("artifacts built into {}", artifacts.display());
    Ok(())
}

/// Builds the macOS package and the same artifacts.
pub fn macos(dev: bool, version: Option<String>) -> Result<(), String> {
    let root = root();
    let out = root.join("target/dist");
    fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    let artifacts = artifacts_folder(&out)?;
    // As on Windows, the program comes last, so everything else the task
    // builds is built wherever it runs; but the `pane` program for macOS
    // can only be built by a macOS checkout (no cross toolchain is set
    // up: a macOS program needs a macOS build), and packing another
    // system's program under a macOS package's name would be worse than
    // explaining so. CI's `macos-15` runner builds the package itself.
    if !cfg!(target_os = "macos") {
        // No package was built, so the index names no application package.
        write_index(&artifacts, None)?;
        return Err(format!(
            "package-macos builds the pane program for macOS, which only a macOS checkout can \
             build; this one runs on {}. The artifacts under {} are assembled for this system, \
             and a macOS run re-assembles them for macos-aarch64",
            std::env::consts::OS,
            artifacts.display()
        ));
    }
    build_program(&root, dev, version.as_deref())?;
    let (package, packed) = assemble_package(&root, &out, dev, System::Macos, version.as_deref())?;
    let target = target_id()?;
    let application = application_entry(
        version.as_deref().unwrap_or(VERSION),
        &package,
        &packed,
        &target,
    );
    write_index(&artifacts, Some(&application))?;
    serve_package(&artifacts, &package)?;
    println!("package built into {}", package.display());
    println!("artifacts built into {}", artifacts.display());
    Ok(())
}

/// Keeps the package in the artifacts an artifact source serves, under the
/// file name its index entry names: a Pane application update downloads
/// it from that source.
fn serve_package(artifacts: &Path, package: &Path) -> Result<(), String> {
    let served = artifacts.join(package.file_name().expect("the package is named"));
    fs::copy(package, &served)
        .map_err(|error| format!("copy {} failed: {error}", served.display()))?;
    Ok(())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

fn run(command: &mut Command) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("failed to start {command:?}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command:?} failed with {status}"))
    }
}

/// Builds the `pane` program for the system this runs on. With `version`,
/// the program reports that version (`pane --version`, and the version an
/// application update compares itself with) instead of the workspace's.
fn build_program(root: &Path, dev: bool, version: Option<&str>) -> Result<(), String> {
    let mut build = cargo();
    build
        .current_dir(root)
        .args(["build", "--locked", "-p", "pane"]);
    if !dev {
        build.arg("--release");
    }
    if let Some(version) = version {
        build.env("PANE_PACKAGE_VERSION", version);
    }
    run(&mut build)
}

/// Where `cargo build` put the `pane` program.
fn program(root: &Path, dev: bool) -> PathBuf {
    let profile = if dev { "debug" } else { "release" };
    root.join("target").join(profile).join(program_name())
}

fn program_name() -> String {
    format!("pane{}", std::env::consts::EXE_SUFFIX)
}

/// The system this runs on, as Pane names a helper target (`linux-x86_64`).
fn target_id() -> Result<String, String> {
    pane_target::Target::current()
        .map(|target| target.id())
        .ok_or_else(|| "Pane names no target for this system".to_owned())
}

/// The empty `artifacts` folder under `out`, replacing whatever an
/// earlier build left there: the index of Pane's own application updates
/// and the package it names.
fn artifacts_folder(out: &Path) -> Result<PathBuf, String> {
    let artifacts = out.join("artifacts");
    let _ = fs::remove_dir_all(&artifacts);
    fs::create_dir_all(&artifacts).map_err(|error| error.to_string())?;
    Ok(artifacts)
}

/// Writes the index `pane-defaults.json` into `artifacts`: the
/// `application` entry naming the package a Pane application update
/// downloads (its version, file name, sha512 integrity, size and target)
/// when the task built one. A source that serves no application package
/// serves the index alone.
fn write_index(artifacts: &Path, application: Option<&str>) -> Result<(), String> {
    let mut index = "{\n  \"formatVersion\": 1".to_owned();
    if let Some(application) = application {
        index.push_str(",\n");
        index.push_str(application);
    }
    index.push_str("\n}\n");
    let index_file = artifacts.join("pane-defaults.json");
    fs::write(&index_file, index)
        .map_err(|error| format!("write {} failed: {error}", index_file.display()))?;
    Ok(())
}

/// The `application` line of the index: what a Pane application update
/// reads of the package `packed` built as `file`, of `version`, for
/// `target`.
fn application_entry(version: &str, package: &Path, packed: &[u8], target: &str) -> String {
    let file = package
        .file_name()
        .and_then(|name| name.to_str())
        .expect("the package is named");
    let integrity = format!("sha512-{}", base64(&Sha512::digest(packed)));
    format!(
        "  \"application\": {{ \"version\": \"{version}\", \"file\": \"{file}\", \
         \"integrity\": \"{integrity}\", \"size\": {}, \"target\": \"{target}\" }}",
        packed.len()
    )
}

/// Every file under `folder`, by its path relative to it, in name order.
fn read_files(folder: &Path, prefix: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut files = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(folder)
        .map_err(|error| format!("read {} failed: {error}", folder.display()))?
        .map(|entry| entry.map_err(|error| error.to_string()))
        .collect::<Result<_, String>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| format!("{} holds a file name that is not UTF-8", folder.display()))?;
        let in_package = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if path.is_dir() {
            files.extend(read_files(&path, &in_package)?);
        } else {
            files.push((
                in_package,
                fs::read(&path).map_err(|error| error.to_string())?,
            ));
        }
    }
    Ok(files)
}

/// Packs `files` (path in the archive, contents) as the tarballs this
/// repository packs: a gzip of a tar holding them under `prefix/`, every
/// file a regular file, `program` (when given) executable, with the fixed
/// time, owner and mode that make the bytes the same on every system.
fn pack_tgz(
    files: &[(String, Vec<u8>)],
    prefix: &str,
    program: Option<&str>,
) -> Result<Vec<u8>, String> {
    let mut tar = tar::Builder::new(Vec::new());
    for (path, contents) in files {
        let mode = if Some(path.as_str()) == program {
            0o755
        } else {
            0o644
        };
        let mut header = tar::Header::new_ustar();
        header.set_size(contents.len() as u64);
        header.set_mode(mode);
        header.set_mtime(PACKED_MTIME);
        header.set_uid(0);
        header.set_gid(0);
        header.set_entry_type(tar::EntryType::Regular);
        tar.append_data(&mut header, format!("{prefix}/{path}"), contents.as_slice())
            .map_err(|error| format!("packing {path} failed: {error}"))?;
    }
    let tar = tar.into_inner().map_err(|error| error.to_string())?;
    let mut gz = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), flate2::Compression::best());
    std::io::Write::write_all(&mut gz, &tar).map_err(|error| error.to_string())?;
    gz.finish().map_err(|error| error.to_string())
}

/// Assembles the package: the `pane` program, the install script, a
/// README (and, on Linux, a desktop entry) in `pane/`, packed as
/// `pane-<version>-<os>-<arch>[-dev].tar.gz` on Linux and
/// `pane-<version>-<os>-<arch>[-dev].zip` on Windows, with a `.sha256`
/// file beside it. The package is the same bytes wherever it is built: the
/// tarball with fixed time, owner and mode, the zip the same way
/// ([`zip::pack`]).
fn assemble_package(
    root: &Path,
    out: &Path,
    dev: bool,
    system: System,
    version: Option<&str>,
) -> Result<(PathBuf, Vec<u8>), String> {
    let stage = out.join("stage/pane");
    let _ = fs::remove_dir_all(stage.parent().expect("the stage folder"));
    fs::create_dir_all(&stage).map_err(|error| error.to_string())?;
    let suffix = if dev { "-dev" } else { "" };
    let name = format!(
        "pane-{}-{}-{}{suffix}.{}",
        version.unwrap_or(VERSION),
        system.id(),
        arch().ok_or("the package names no architecture for this system")?,
        system.archive(),
    );
    let (script, script_name) = system.install_script();
    let copies = [
        (program(root, dev), stage.join(program_name())),
        (root.join(script), stage.join(script_name)),
    ];
    for (from, to) in copies {
        fs::copy(&from, &to).map_err(|error| format!("copy {} failed: {error}", from.display()))?;
    }
    fs::write(stage.join("README.txt"), readme(system, dev, version))
        .map_err(|error| error.to_string())?;
    if let Some((file, contents)) = system.extra_file() {
        fs::write(stage.join(file), contents).map_err(|error| error.to_string())?;
    }
    let files = read_files(&stage, "")?;
    let packed = match system {
        System::Linux => pack_tgz(&files, "pane", Some(&program_name()))?,
        System::Windows | System::Macos => zip::pack(&files, "pane")?,
    };
    let package = out.join(&name);
    fs::write(&package, &packed)
        .map_err(|error| format!("write {} failed: {error}", package.display()))?;
    // As `sha256sum` writes it, so the file can be checked with
    // `sha256sum -c` on Linux; on Windows, `Get-FileHash` prints the digest
    // to compare with.
    let digest: String = Sha256::digest(&packed)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let sha256 = out.join(format!("{name}.sha256"));
    fs::write(&sha256, format!("{digest}  {name}\n"))
        .map_err(|error| format!("write {} failed: {error}", sha256.display()))?;
    Ok((package, packed))
}

/// The architecture of the system this ran on, as the package name says
/// it.
fn arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("x86_64"),
        "aarch64" => Some("aarch64"),
        _ => None,
    }
}

/// The README in the package, naming the `version` the packaged program
/// reports.
fn readme(system: System, dev: bool, version: Option<&str>) -> String {
    let version = version.unwrap_or(VERSION);
    match system {
        System::Linux => readme_linux(dev, version),
        System::Windows => readme_windows(dev, version),
        System::Macos => readme_macos(dev, version),
    }
}

fn readme_linux(dev: bool, version: &str) -> String {
    let profile = if dev { "development" } else { "release" };
    format!(
        "Pane {version} for Linux (this package is the {profile} profile)

WHAT THIS IS

  Pane, a desktop launcher. This package holds the pane program and
  installs it for one user; it holds none of Pane's default extensions:
  Pane fetches them itself the first time it runs, from the commits of
  their own repositories' release tags that this release pins, with its
  own Git client.

PREREQUISITES

  A 64-bit Linux with X11, and these libraries (Ubuntu 24.04 package
  names; the install script checks what is missing):

    libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libxcb1
    libfontconfig1 libfreetype6 libvulkan1

  plus a Vulkan driver; Mesa's mesa-vulkan-drivers works without a GPU.

INSTALL

  bash install.sh            # installs to ~/.local
  bash install.sh --prefix X # installs to X instead

  It copies the pane program to <prefix>/bin and a desktop entry to
  <prefix>/share/applications, and runs `pane --version` to check what it
  installed. No root is needed.

UNINSTALL

  Remove <prefix>/bin/pane and <prefix>/share/applications/pane.desktop.
  Pane keeps its own data in ~/.local/share/pane (its installed
  extensions and their settings); remove that folder to remove them too.

FIRST RUN

  The first run fetches Pane's default extensions (the calculator,
  applications, quicklinks, files and clipboard history) from the
  commits of their repositories' release tags that this release pins,
  and says what it is setting up; Pane stays usable if a fetch fails,
  and offers to try again. All five are installed, the first-setup
  choice screen being not built yet; each can be disabled in Settings.

  NOTHING IS SIGNED

  The package is not signed: no signing credentials exist. Its sha256 is
  in the pane-<version>-linux-<arch>.tar.gz.sha256 file beside it, which
  says only what was packed.
"
    )
}

fn readme_windows(dev: bool, version: &str) -> String {
    let profile = if dev { "development" } else { "release" };
    format!(
        r#"Pane {version} for Windows (this package is the {profile} profile)

WHAT THIS IS

  Pane, a desktop launcher. This package holds the pane.exe program and
  installs it for one user; it holds none of Pane's default extensions:
  Pane fetches them itself the first time it runs, from the commits of
  their own repositories' release tags that this release pins, with its
  own Git client.

PREREQUISITES

  A 64-bit x86_64 Windows. The system this package was built and checked
  on is Windows Server 2025 (CI's windows-2025 runner); no other Windows
  has been tried. Nothing else is needed: no Node, Rust, npm, Git or
  compiler, and no administrator rights.

INSTALL

  Unzip this package (Explorer unzips it, or Expand-Archive in
  PowerShell), then, in the pane folder it unpacked:

    powershell -ExecutionPolicy Bypass -File install.ps1

  It copies the pane.exe program to %LOCALAPPDATA%\Pane, puts a Pane
  shortcut in your Start menu, and runs `pane --version` to check what it
  installed. No administrator rights are needed. To install somewhere
  else, add -InstallDir <folder>; to install from another folder, add
  -PackageFolder <folder>.

  PowerShell may refuse install.ps1 as a script that came from the
  internet (nothing here is signed): the -ExecutionPolicy Bypass above
  answers that, or run Unblock-File install.ps1 once. Check the package's
  digest against the pane-<version>-windows-<arch>.zip.sha256 file beside
  it if it reached you over the internet.

UNINSTALL

  Remove %LOCALAPPDATA%\Pane\pane.exe and the Pane shortcut in your Start
  menu (close Pane first). Pane keeps its own data in
  %LOCALAPPDATA%\Pane\data (its installed extensions and their settings);
  remove that folder to remove them too.

FIRST RUN

  The first run fetches Pane's default extensions (the calculator,
  applications, quicklinks, files and clipboard history) from the
  commits of their repositories' release tags that this release pins,
  and says what it is setting up; Pane stays usable if a fetch fails,
  and offers to try again. All five are installed, the first-setup
  choice screen being not built yet; each can be disabled in Settings.

  NOTHING IS SIGNED

  The package is not signed: no signing credentials exist (no
  Authenticode certificate), so Windows may warn about an unknown
  publisher when the program or the install script runs. Its sha256 is in
  the pane-<version>-windows-<arch>.zip.sha256 file beside it, which says
  only what was packed."#
    )
}

fn readme_macos(dev: bool, version: &str) -> String {
    let profile = if dev { "development" } else { "release" };
    format!(
        r#"Pane {version} for macOS (this package is the {profile} profile)

WHAT THIS IS

  Pane, a desktop launcher. This package holds the pane program and
  installs it, as a Pane.app bundle, for one user; it holds none of
  Pane's default extensions: Pane fetches them itself the first time
  it runs, from the commits of their own repositories' release tags
  that this release pins, with its own Git client.

PREREQUISITES

  macOS 15 on Apple silicon (arm64): the system this package was built
  and checked on (CI's macos-15 runner); no other macOS or Mac has been
  tried. Nothing else is needed: no Node, Rust, npm, Git or compiler,
  and no administrator rights.

INSTALL

  Unzip this package (Finder opens a zip with a double-click, or unzip
  in the terminal), then, in the pane folder it unpacked:

    bash install.sh            # installs to ~/Applications
    bash install.sh --app-dir X # installs Pane.app into X instead

  It builds the Pane.app bundle in ~/Applications — a folder of your
  own, so no administrator rights are needed — around the pane program
  and this package's Info.plist, and runs `pane --version` to check
  what it installed. Open Pane with a double-click in Finder, or:

    open ~/Applications/Pane.app

  Nothing is signed (no Apple Developer credentials exist). macOS only
  checks Gatekeeper on files that carry its quarantine mark, which the
  web browser or mail program that downloaded this package set: its
  first Pane.app will be blocked as an app macOS cannot check, and you
  allow it in System Settings (Privacy & Security). A package built on
  your own machine, like a CI runner's, carries no mark and runs at
  once. Check the package's digest against the
  pane-<version>-macos-<arch>.zip.sha256 file beside it if it reached
  you over the internet.

UNINSTALL

  Remove ~/Applications/Pane.app (close Pane first). Pane keeps its own
  data in ~/Library/Application Support/Pane (its installed extensions
  and their settings) and its caches in ~/Library/Caches/Pane; remove
  those folders to remove them too.

FIRST RUN

  The first run fetches Pane's default extensions (the calculator,
  applications, quicklinks, files and clipboard history) from the
  commits of their repositories' release tags that this release pins,
  and says what it is setting up; Pane stays usable if a fetch fails,
  and offers to try again. All five are installed, the first-setup
  choice screen being not built yet; each can be disabled in Settings.

  NOTHING IS SIGNED

  The package is not signed: no signing credentials exist (no Apple
  Developer ID certificate, and nothing is notarized). Its sha256 is in
  the pane-<version>-macos-<arch>.zip.sha256 file beside it, which says
  only what was packed."#
    )
}

/// The desktop entry in the package.
fn desktop_entry() -> String {
    "[Desktop Entry]\n\
Type=Application\n\
Name=Pane\n\
GenericName=Launcher\n\
Comment=Launch applications, calculate and run extension commands\n\
Exec=pane\n\
Terminal=false\n\
Categories=Utility;\n"
        .to_owned()
}

/// The `Info.plist` of the `Pane.app` bundle the install script builds:
/// the minimum Launch Services reads — the executable to run, the bundle's
/// identity and name, and its version. Nothing more is declared, because
/// nothing more is honestly known: the bundle declares no document types,
/// no services and no minimum system version this package has been checked
/// against, and it is not signed.
fn app_plist() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
\t<key>CFBundleExecutable</key><string>pane</string>\n\
\t<key>CFBundleIdentifier</key><string>dev.pane.launcher</string>\n\
\t<key>CFBundleName</key><string>Pane</string>\n\
\t<key>CFBundlePackageType</key><string>APPL</string>\n\
\t<key>CFBundleShortVersionString</key><string>{VERSION}</string>\n\
\t<key>CFBundleVersion</key><string>{VERSION}</string>\n\
</dict>\n\
</plist>\n"
    )
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                text.push(ALPHABET[(n >> shift) as usize & 63] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}
