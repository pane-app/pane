//! Building Pane's packages and the artifacts its default extensions
//! are acquired from (#53 for Linux, #51 for Windows, #52 for macOS).
//!
//! `package-linux`, `package-windows` and `package-macos` each produce,
//! under `target/dist/`:
//!
//! - `pane-<version>-<os>-<arch>[-dev].tar.gz` (Linux) or `.zip`
//!   (Windows and macOS) — the package a clean machine of that system
//!   installs from: the `pane` program, the install script and a README
//!   (and, on Linux, a desktop entry, on macOS the `Info.plist` of the
//!   `Pane.app` bundle the install script makes), and none of the default
//!   extensions' payloads (internet-first: Pane downloads them at first
//!   setup). A `.sha256` file beside it names its digest; nothing is
//!   signed, since no signing credentials exist yet.
//! - `artifacts/` — what an artifact source serves: the index document
//!   `pane-defaults.json` and one tarball per default extension's payload,
//!   built for the system this ran on. A real deployment serves this
//!   folder at Pane's published downloads; the tests and smokes serve it
//!   from this computer (`scripts/artifact_server.py`) instead, so no
//!   check reaches the network.
//!
//! `--dev` builds the package's program in the development profile: the
//! native smokes install that one, because only a development build takes
//! its artifact source from `PANE_ARTIFACTS` (a release build uses Pane's
//! published downloads, which no controlled source may replace). Without
//! it, the release profile is built.
//!
//! The index the artifacts hold also names the application package a Pane
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

use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha512};

use crate::zip;

/// The default extensions whose payloads the artifacts describe, and the
/// assembled package each is packed from: the default set (#60, the
/// user's recorded choice — the calculator, applications, quicklinks,
/// files and clipboard history), the same in every build. The helper
/// sample is no default extension (#162): it is installed by hand, with
/// `pane --install target/guests/packages/sample-helper`. The ids are the
/// ones Pane's application build acquires (`pane::default_extensions`).
const DEFAULTS: [(&str, &str); 5] = [
    ("calculator", "calculator"),
    ("applications", "applications"),
    ("quicklinks", "quicklinks"),
    ("files", "files"),
    ("clipboard-history", "clipboard-history"),
];

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
/// the default extensions' payloads, and the index naming them and the
/// application package this task also built.
pub fn linux(dev: bool, version: Option<String>) -> Result<(), String> {
    let root = root();
    let out = root.join("target/dist");
    fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    let (artifacts, entries) = assemble_payloads(&root, &out)?;
    build_program(&root, dev, version.as_deref())?;
    let (package, packed) = assemble_package(&root, &out, dev, System::Linux, version.as_deref())?;
    let target = target_id()?;
    let application = application_entry(
        version.as_deref().unwrap_or(VERSION),
        &package,
        &packed,
        &target,
    );
    write_index(&artifacts, &entries, Some(&application))?;
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
    let (artifacts, entries) = assemble_payloads(&root, &out)?;
    // The program comes last, so everything else the task builds is built
    // wherever it runs; but pane.exe can only be built by a Windows
    // checkout (no cross toolchain is set up: a Windows program needs a
    // Windows build), and packing another system's program under a
    // Windows package's name would be worse than explaining so. CI's
    // `windows-2025` runner builds the package itself.
    if !cfg!(target_os = "windows") {
        // No package was built, so the index names no application package:
        // a source that serves none still serves the default extensions.
        write_index(&artifacts, &entries, None)?;
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
    write_index(&artifacts, &entries, Some(&application))?;
    serve_package(&artifacts, &package)?;
    println!("package built into {}", package.display());
    println!("artifacts built into {}", artifacts.display());
    Ok(())
}

/// Builds the macOS package and the default extensions' artifacts.
pub fn macos(dev: bool, version: Option<String>) -> Result<(), String> {
    let root = root();
    let out = root.join("target/dist");
    fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    let (artifacts, entries) = assemble_payloads(&root, &out)?;
    // As on Windows, the program comes last, so everything else the task
    // builds is built wherever it runs; but the `pane` program for macOS
    // can only be built by a macOS checkout (no cross toolchain is set up:
    // a macOS program needs a macOS build), and packing another system's
    // program under a macOS package's name would be worse than explaining
    // so. CI's `macos-15` runner builds the package itself.
    if !cfg!(target_os = "macos") {
        // No package was built, so the index names no application package:
        // a source that serves none still serves the default extensions.
        write_index(&artifacts, &entries, None)?;
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
    write_index(&artifacts, &entries, Some(&application))?;
    serve_package(&artifacts, &package)?;
    println!("package built into {}", package.display());
    println!("artifacts built into {}", artifacts.display());
    Ok(())
}

/// Keeps the package in the artifacts an artifact source serves, under the
/// file name its index entry names: a Pane application update downloads it
/// from the same source the default extensions' payloads come from.
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

/// Assembles the payloads an artifact source serves into
/// `target/dist/artifacts`: one tarball per default extension's payload,
/// packed from the package `cargo xtask guests` assembled, and the line
/// each takes in the index (written by [`write_index`], once the
/// application package is also known). The payload's manifest names the
/// helper targets whose files it carries: the build serves the helper
/// built for the system it ran on, so the manifest is rewritten to name
/// that target alone (a real deployment builds every supported target and
/// serves one payload whose manifest names them all).
fn assemble_payloads(root: &Path, out: &Path) -> Result<(PathBuf, Vec<String>), String> {
    let artifacts = out.join("artifacts");
    let _ = fs::remove_dir_all(&artifacts);
    fs::create_dir_all(&artifacts).map_err(|error| error.to_string())?;
    let mut entries: Vec<String> = Vec::new();
    for (id, package) in DEFAULTS {
        let source = root.join("target/guests/packages").join(package);
        if !source.is_dir() {
            return Err(format!(
                "{} is missing; run `cargo xtask guests` first",
                source.display()
            ));
        }
        let files = payload_files(&source, id)?;
        let version = manifest_version(&files)?;
        let file = format!("{id}-{version}.tgz");
        let tarball = pack(&files);
        let integrity = format!("sha512-{}", base64(&Sha512::digest(&tarball)));
        fs::write(artifacts.join(&file), &tarball).map_err(|error| {
            format!("write {} failed: {error}", artifacts.join(&file).display())
        })?;
        entries.push(format!(
            "  {{ \"id\": \"{id}\", \"version\": \"{version}\", \"file\": \"{file}\", \"integrity\": \"{integrity}\", \"size\": {} }}",
            tarball.len()
        ));
    }
    Ok((artifacts, entries))
}

/// Writes the index `pane-defaults.json` into `artifacts`: the default
/// extensions' entries, and the `application` entry naming the package an
/// application update downloads (its version, file name, sha512
/// integrity, size and target) when the task built one. A source that
/// serves no application package still serves the default extensions.
fn write_index(
    artifacts: &Path,
    entries: &[String],
    application: Option<&str>,
) -> Result<(), String> {
    let mut index = format!(
        "{{\n  \"formatVersion\": 1,\n  \"defaults\": [\n{}\n  ]",
        entries.join(",\n")
    );
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

/// The files of the payload packed from `source`: the manifest, and the
/// files it names — the components of its commands, the packaged images
/// its package's and commands' icons name with the `@light` and `@dark`
/// variants the package has (#163, the default extensions' tiles), and
/// the helper file for this system — exactly what Pane installs from it. A
/// manifest that declares helpers is rewritten to name this system's helper
/// target
/// alone, since the build assembles the helper for the system it runs on;
/// nothing else in the assembled folder is packed, so a file a helper run
/// left beside its program never travels.
fn payload_files(source: &Path, _id: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let assembled = read_files(source, "")?;
    let read = |path: &str| {
        assembled
            .iter()
            .find(|(name, _)| name == path)
            .map(|(_, contents)| contents.clone())
            .ok_or_else(|| {
                format!(
                    "{path} is missing from the assembled package {}",
                    source.display()
                )
            })
    };
    let mut manifest: Value = serde_json::from_slice(&read("pane.json")?)
        .map_err(|error| format!("the payload's pane.json cannot be read: {error}"))?;
    // The files the manifest names, which Pane installs from the payload.
    // Commands can share a component (Quicklinks' four do), and Pane refuses
    // a tarball that holds a file twice, so each is named once.
    let mut named: Vec<String> = Vec::new();
    if let Some(commands) = manifest["commands"].as_array() {
        for command in commands {
            let component = command["component"].as_str().expect("a component");
            if !named.iter().any(|name| name == component) {
                named.push(component.to_owned());
            }
        }
    }
    // The images the icons name: Pane refuses a package whose icon names
    // an image it does not ship, so a payload without them would not
    // install.
    for image in manifest_images(&manifest) {
        let present = variants(&image)
            .into_iter()
            .filter(|variant| assembled.iter().any(|(name, _)| name == variant));
        for path in std::iter::once(image.clone()).chain(present) {
            if !named.contains(&path) {
                named.push(path);
            }
        }
    }
    let target = target_id()?;
    // `get_mut`, not indexing: indexing a Value for a missing key inserts
    // a null for it, and a written-out `"helpers": null` is a manifest
    // Pane refuses (its helpers are a sequence).
    if let Some(Value::Array(helpers)) = manifest.get_mut("helpers") {
        // The build assembles the helper for the system it runs on, so the
        // payload names that target alone.
        let file = format!("helpers/{target}/pane-echo{}", exe_suffix());
        if read(&file).is_err() {
            return Err(format!(
                "the payload ships no {file}; `cargo xtask guests` builds its helper for the \
                 system it runs on"
            ));
        }
        for helper in helpers {
            helper["targets"] = json!({ target.clone(): file.clone() });
        }
        named.push(file);
    }
    let manifest_text = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("the rewritten pane.json cannot be written: {error}"))?;
    let mut payload = vec![("pane.json".to_owned(), manifest_text)];
    payload.extend(
        named
            .into_iter()
            .map(|path| Ok((path.clone(), read(&path)?)))
            .collect::<Result<Vec<_>, String>>()?,
    );
    Ok(payload)
}

/// The packaged images the package's and its commands' icons in
/// `manifest` name, in order.
fn manifest_images(manifest: &Value) -> Vec<String> {
    std::iter::once(&manifest["icon"])
        .chain(
            manifest["commands"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|command| &command["icon"]),
        )
        .flat_map(icon_images)
        .collect()
}

/// The packaged images an `icon` of `pane.json` names (`docs/list-tree.md`,
/// Icons): a text ending in `.png` or `.svg`, an object's `path`, `light`
/// and `dark`, and its `fallback`'s, in that order. A built-in icon's
/// name, a URL and a system icon name none.
fn icon_images(icon: &Value) -> Vec<String> {
    match icon {
        Value::String(text) => {
            let lower = text.to_ascii_lowercase();
            if lower.ends_with(".png") || lower.ends_with(".svg") {
                vec![text.clone()]
            } else {
                Vec::new()
            }
        }
        Value::Object(fields) => {
            let mut images: Vec<String> = ["path", "light", "dark"]
                .into_iter()
                .filter_map(|field| fields.get(field)?.as_str().map(str::to_owned))
                .collect();
            if let Some(fallback) = fields.get("fallback") {
                images.extend(icon_images(fallback));
            }
            images
        }
        _ => Vec::new(),
    }
}

/// The `@light` and `@dark` variants of the image at `path`, which Pane
/// draws in place of it in those themes when the package has them:
/// `logo@light.png` and `logo@dark.png` beside `logo.png`.
fn variants(path: &str) -> [String; 2] {
    let (stem, extension) = match path.rfind('.') {
        Some(dot) if !path[dot..].contains('/') => (&path[..dot], &path[dot..]),
        _ => (path, ""),
    };
    ["light", "dark"].map(|theme| format!("{stem}@{theme}{extension}"))
}

fn exe_suffix() -> &'static str {
    if cfg!(windows) { ".exe" } else { "" }
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

/// The version a payload's `pane.json` declares.
fn manifest_version(files: &[(String, Vec<u8>)]) -> Result<String, String> {
    let manifest = files
        .iter()
        .find(|(path, _)| path == "pane.json")
        .map(|(_, contents)| contents.clone())
        .ok_or("the payload has no pane.json")?;
    let manifest: Value = serde_json::from_slice(&manifest)
        .map_err(|error| format!("the payload's pane.json cannot be read: {error}"))?;
    manifest["version"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "the payload's pane.json declares no version".to_owned())
}

/// Packs `files` (path in the package, contents) as Pane's own downloads
/// pack a payload: a gzip of a tar holding them under `package/`, every
/// file a regular file without execute permission, with the fixed time,
/// owner and mode that make the tarball the same on every system.
fn pack(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    pack_tgz(files, "package", None).expect("packing into memory")
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
  Pane downloads them itself the first time it runs, from Pane's own
  downloads (https://downloads.pane.sh/).

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

  The first run downloads Pane's default extensions (the calculator) from
  https://downloads.pane.sh/ and shows their progress; Pane stays usable
  if the download fails, and offers to try again. That location is not
  deployed yet, so today a first run on the real internet explains that
  it cannot reach it and keeps everything else working.

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
  Pane downloads them itself the first time it runs, from Pane's own
  downloads (https://downloads.pane.sh/).

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

  The first run downloads Pane's default extensions (the calculator) from
  https://downloads.pane.sh/ and shows their progress; Pane stays usable
  if the download fails, and offers to try again. That location is not
  deployed yet, so today a first run on the real internet explains that
  it cannot reach it and keeps everything else working.

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
  Pane's default extensions: Pane downloads them itself the first time
  it runs, from Pane's own downloads (https://downloads.pane.sh/).

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

  The first run downloads Pane's default extensions (the calculator)
  from https://downloads.pane.sh/ and shows their progress; Pane stays
  usable if the download fails, and offers to try again. That location
  is not deployed yet, so today a first run on the real internet
  explains that it cannot reach it and keeps everything else working.

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

#[cfg(test)]
mod tests {
    use super::*;

    /// A package with no helpers keeps a manifest with no helpers: indexing
    /// a Value for the key would write `"helpers": null` into the payload,
    /// which Pane refuses to install (CI run 36676779827 found it in the
    /// smoke, the only consumer of the assembled payloads).
    #[test]
    fn a_payload_without_helpers_writes_no_helpers_field() {
        let folder = std::env::temp_dir().join("pane-xtask-payload-test");
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("pane.json"),
            br#"{"manifestVersion": 1, "title": "T", "version": "0.1.0", "apiVersion": "0.1",
                "commands": [{"id": "c", "title": "C", "component": "c.wasm"}]}"#,
        )
        .unwrap();
        fs::write(folder.join("c.wasm"), b"the component").unwrap();
        let files = payload_files(&folder, "t").expect("the payload assembles");
        let manifest = files
            .iter()
            .find(|(path, _)| path == "pane.json")
            .map(|(_, contents)| contents.clone())
            .expect("the payload holds the manifest");
        let manifest: Value = serde_json::from_slice(&manifest).unwrap();
        assert!(
            manifest.get("helpers").is_none(),
            "the manifest gained a helpers field: {manifest}"
        );
    }

    /// The artifacts describe the default set alone: the five default
    /// extensions, and no sample (#162), whose payload a first setup
    /// would otherwise acquire.
    #[test]
    fn the_artifacts_describe_the_default_set_without_the_samples() {
        let ids: Vec<&str> = DEFAULTS.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            ids,
            [
                "calculator",
                "applications",
                "quicklinks",
                "files",
                "clipboard-history"
            ]
        );
        assert!(
            DEFAULTS
                .iter()
                .all(|(_, package)| !package.starts_with("sample-")),
            "a sample is packed as a default extension: {DEFAULTS:?}"
        );
    }

    /// Commands that share a component pack it once: Pane refuses a payload
    /// whose tarball holds a file twice, and refused Quicklinks, whose four
    /// commands share one, until this held.
    #[test]
    fn a_component_commands_share_is_packed_once() {
        let folder = std::env::temp_dir().join("pane-xtask-shared-component-test");
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("pane.json"),
            br#"{"manifestVersion": 1, "title": "T", "version": "0.1.0", "apiVersion": "0.1",
                "commands": [{"id": "a", "title": "A", "component": "c.wasm"},
                             {"id": "b", "title": "B", "component": "c.wasm"}]}"#,
        )
        .unwrap();
        fs::write(folder.join("c.wasm"), b"the component").unwrap();
        let files = payload_files(&folder, "t").expect("the payload assembles");
        let paths: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(paths, ["pane.json", "c.wasm"]);
    }

    /// The images the package's and commands' icons name travel in the
    /// payload, each once, with the `@light` and `@dark` variants the
    /// package has (#163): Pane refuses to install a package whose icon
    /// names an image it does not ship, so the default extensions' tiles
    /// must be in their payloads. Built-in icons name no file, and a file
    /// no icon names stays behind.
    #[test]
    fn the_images_the_icons_name_are_packed_with_their_variants() {
        let folder = std::env::temp_dir().join("pane-xtask-icon-images-test");
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join("assets")).unwrap();
        fs::write(
            folder.join("pane.json"),
            br#"{"manifestVersion": 1, "title": "T", "version": "0.1.0", "apiVersion": "0.1",
                "icon": "icon.svg",
                "commands": [
                    {"id": "a", "title": "A", "component": "c.wasm", "icon": "assets/a.png"},
                    {"id": "b", "title": "B", "component": "c.wasm", "icon": "icon.svg"},
                    {"id": "c", "title": "C", "component": "c.wasm",
                     "icon": {"light": "assets/sun.svg", "dark": "assets/moon.svg",
                              "fallback": {"builtin": "star"}}},
                    {"id": "d", "title": "D", "component": "c.wasm", "icon": "star"}
                ]}"#,
        )
        .unwrap();
        for file in [
            "c.wasm",
            "icon.svg",
            "assets/a.png",
            "assets/a@dark.png",
            "assets/sun.svg",
            "assets/moon.svg",
            "assets/unused.svg",
        ] {
            fs::write(folder.join(file), file.as_bytes()).unwrap();
        }
        let files = payload_files(&folder, "t").expect("the payload assembles");
        let paths: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "pane.json",
                "c.wasm",
                "icon.svg",
                "assets/a.png",
                "assets/a@dark.png",
                "assets/sun.svg",
                "assets/moon.svg",
            ]
        );
        let image = files
            .iter()
            .find(|(path, _)| path == "assets/a@dark.png")
            .map(|(_, contents)| contents.as_slice());
        assert_eq!(image, Some(b"assets/a@dark.png".as_slice()));
    }

    /// The default extensions' own manifests: every image their icons
    /// name is in the repository's package folder, so their payloads carry
    /// their tiles (#163).
    #[test]
    fn the_default_extensions_ship_the_images_their_icons_name() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for (id, package) in DEFAULTS {
            let folder = root.join("guests/packages").join(package);
            let manifest: Value =
                serde_json::from_slice(&fs::read(folder.join("pane.json")).unwrap()).unwrap();
            for image in manifest_images(&manifest) {
                assert!(
                    folder.join(&image).is_file(),
                    "{id}'s icon names {image}, which its package does not ship"
                );
            }
        }
    }
}
