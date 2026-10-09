//! `pane-ext pack` as a process (#225), against a copy of the Rust
//! development sample (`guests/hello-rust`) with the path to
//! `pane-extension` rewritten, as the `dev` tests use: the release
//! components are built by the same `pane-build` code development mode
//! uses; an npm package (one with a `package.json`) is packed into the
//! deterministic tarball `npm pack` makes, which Pane then installs from
//! the loopback registry the npm tests serve, and its command runs; and
//! each package Pane would refuse — a link, a name no system takes, a
//! missing component, an oversized entry, a `files` list that does not
//! cover `pane.json`, no 512×512 icon — is refused with Pane's message. A
//! package without a `package.json` is checked as the folder its release
//! revision will hold, and writes no tarball.
//!
//! The builds run `cargo build --release --target wasm32-wasip2` (the
//! pinned toolchain and its `wasm32-wasip2` target, as `cargo xtask
//! guests` needs).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use futures::executor::block_on;
use pane_core::npm::Registry as NpmRegistry;
use pane_core::pack::PACKED_MTIME;
use pane_core::{Launcher, Runtime, Status, ToastStyle};

#[path = "support/npm_registry.rs"]
mod npm_registry;

use npm_registry::Registry;

/// The npm name of the packed sample, and the version it packs.
const NAME: &str = "@pane-samples/hello-rust";
const VERSION: &str = "0.1.0";

/// The tarball `npm pack` names for it, in the package's `dist` folder.
const TARBALL: &str = "dist/pane-samples-hello-rust-0.1.0.tgz";

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// The sample's pane.json: one command whose component is the cargo-built
/// `hello_rust.wasm`, with its own 512×512 icon when `icon`, and any extra
/// manifest fields (spliced after the commands, with their own comma).
fn manifest(icon: bool, extra: &str) -> String {
    let icon = if icon { r#""icon": "icon.png","# } else { "" };
    format!(
        r#"{{
  "manifestVersion": 1,
  "title": "Hello Rust",
  "version": "{VERSION}",
  "apiVersion": "0.1",
  {icon}
  "commands": [
    {{
      "id": "hello",
      "title": "Hello Rust",
      "subtitle": "Packed from the development sample",
      "component": "hello_rust.wasm"
    }}
  ]{extra}
}}"#
    )
}

/// The sample's package.json: a private package whose `files` covers
/// everything its pane.json names.
const PACKAGE_JSON: &str = r#"{
  "name": "@pane-samples/hello-rust",
  "version": "0.1.0",
  "private": true,
  "files": ["pane.json", "hello_rust.wasm", "icon.png"]
}"#;

/// A fresh copy of the sample's sources in Cargo's test folder's `name`,
/// keeping what earlier runs built there, with the path to
/// `pane-extension` rewritten, the repository's toolchain file, `manifest`
/// as its pane.json and, when `package` is given, that as its
/// package.json.
fn sample(name: &str, manifest: &str, package: Option<&str>) -> PathBuf {
    let from = repository().join("guests/hello-rust");
    let to = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(to.join("src"));
    fs::create_dir_all(to.join("src")).unwrap();
    for file in ["Cargo.toml", "Cargo.lock"] {
        fs::copy(from.join(file), to.join(file)).unwrap();
    }
    fs::copy(from.join("src/lib.rs"), to.join("src/lib.rs")).unwrap();
    let guest = repository().join("guests/pane-extension");
    let cargo = fs::read_to_string(to.join("Cargo.toml")).unwrap().replace(
        r#"path = "../pane-extension""#,
        &format!("path = {:?}", guest.to_str().unwrap()),
    );
    fs::write(to.join("Cargo.toml"), cargo).unwrap();
    fs::copy(
        repository().join("rust-toolchain.toml"),
        to.join("rust-toolchain.toml"),
    )
    .unwrap();
    fs::write(to.join("pane.json"), manifest).unwrap();
    if let Some(package) = package {
        fs::write(to.join("package.json"), package).unwrap();
    }
    to.canonicalize().unwrap()
}

/// A 512×512 PNG, as a published extension's icon is.
fn icon_png() -> Vec<u8> {
    let rgba = vec![64u8; 512 * 512 * 4];
    pane_core::icons::encode_png(512, 512, &rgba).unwrap()
}

/// Runs `pane-ext pack` on `folder`, returning whether it succeeded and
/// what it printed, standard output and error together.
fn pack(folder: &Path) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_pane-ext"))
        .arg("pack")
        .arg(folder)
        .output()
        .unwrap();
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), printed)
}

/// Creates a symbolic link to a file, or `None` where the OS does not
/// allow one (Windows without Developer Mode or administrator rights).
fn symlink(target: &Path, link: &Path) -> Option<()> {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(target, link);
    made.ok()
}

/// A helper target that is not this system's, so that a package declaring
/// one stays a package this system installs.
fn other_target() -> String {
    let current = pane_core::Target::current().expect("Pane names this system's target");
    [
        "windows-x86_64",
        "macos-aarch64",
        "linux-x86_64",
        "windows-aarch64",
        "macos-x86_64",
        "linux-aarch64",
    ]
    .into_iter()
    .find(|id| pane_core::Target::parse(id) != Some(current))
    .expect("Pane names more than one target")
    .to_owned()
}

/// What the launcher shows of the last outcome: the status line, else the
/// toast in the footer.
fn shown(launcher: &Launcher) -> Status {
    let status = launcher.view().status;
    if status != Status::Idle {
        return status;
    }
    match launcher.toast() {
        None => Status::Idle,
        Some(shown) => match shown.toast.style {
            ToastStyle::Success => Status::Result(shown.toast.text()),
            ToastStyle::Failure => Status::Error(shown.toast.text()),
            ToastStyle::Animated => Status::Progress(shown.toast.text()),
        },
    }
}

/// Opens the sample's command from root search and runs its "Say hello",
/// returning what it showed.
fn say_hello(launcher: &Launcher) -> Status {
    for _ in 0..3 {
        launcher.back();
    }
    let rows = launcher.view().rows;
    let Some(index) = rows.iter().position(|row| row.title == "Hello Rust") else {
        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        panic!("no row \"Hello Rust\": {titles:?}");
    };
    launcher.select(index);
    block_on(launcher.activate_selected());
    let rows = launcher.view().rows;
    let Some(index) = rows.iter().position(|row| row.title == "Say hello") else {
        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        panic!("no row \"Say hello\": {titles:?}");
    };
    launcher.select(index);
    block_on(launcher.activate_selected());
    shown(launcher)
}

#[test]
fn a_rust_sample_packs_into_a_tarball_that_pane_installs_from_the_registry() {
    let folder = sample("pane-ext-pack", &manifest(true, ""), Some(PACKAGE_JSON));
    fs::write(folder.join("icon.png"), icon_png()).unwrap();
    let (passed, printed) = pack(&folder);
    assert!(passed, "{printed}");
    let tarball = folder.join(TARBALL);
    assert!(tarball.is_file(), "{printed}");
    assert!(
        printed.contains(&format!("packed {}", tarball.display())),
        "{printed}"
    );
    assert!(printed.contains("4 files"), "{printed}");
    for file in ["hello_rust.wasm", "icon.png", "package.json", "pane.json"] {
        assert!(printed.contains(file), "missing {file}:\n{printed}");
    }

    // The tarball is written the same on every system: every file under
    // `package/`, in name order, with npm's fixed time and mode.
    let bytes = fs::read(&tarball).unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&bytes[..]));
    let mut names = Vec::new();
    for entry in archive.entries().unwrap() {
        let entry = entry.unwrap();
        let name = String::from_utf8(entry.path_bytes().to_vec()).unwrap();
        assert_eq!(
            entry.header().entry_type(),
            tar::EntryType::Regular,
            "{name}"
        );
        assert_eq!(entry.header().mode().unwrap(), 0o644, "{name}");
        assert_eq!(entry.header().mtime().unwrap(), PACKED_MTIME, "{name}");
        names.push(name);
    }
    assert_eq!(names.len(), 4);
    assert_eq!(names[0], "package/hello_rust.wasm");
    assert_eq!(names[1], "package/icon.png");
    assert_eq!(names[2], "package/package.json");
    assert_eq!(names[3], "package/pane.json");

    // Pane installs the packed tarball as an npm package, and its command
    // runs: nothing publishes anything, so the test serves the tarball
    // itself, from the loopback registry the npm tests use.
    let registry = Registry::start();
    registry.publish(NAME, VERSION, bytes);
    let data = tempfile::tempdir().unwrap();
    let launcher = Launcher::with_packages(
        Ok(Runtime::start().unwrap()),
        vec![],
        data.path().join("extensions"),
    )
    .with_npm_registry(NpmRegistry::local(registry.url()).unwrap());
    block_on(launcher.install_npm(NAME));
    assert_eq!(launcher.packages().len(), 1, "{printed}");
    assert_eq!(launcher.packages()[0].title(), "Hello Rust");
    assert_eq!(
        say_hello(&launcher),
        Status::Result("Hello from Rust".into())
    );
}

#[test]
fn what_pane_refuses_is_refused_by_pack_with_panes_messages() {
    let folder = sample(
        "pane-ext-pack-refused",
        &manifest(true, ""),
        Some(PACKAGE_JSON),
    );
    fs::write(folder.join("icon.png"), icon_png()).unwrap();
    // A crashed earlier run may have left a folder where the component
    // should be; pack's copy-back restores the file.
    let _ = fs::remove_dir_all(folder.join("hello_rust.wasm"));

    // A link, as a tarball Pane unpacks refuses: npm would pack it, and
    // Pane's unpacker takes only files and folders.
    let link = folder.join("assets/link.png");
    fs::create_dir_all(folder.join("assets")).unwrap();
    if let Some(()) = symlink(&folder.join("icon.png"), &link) {
        let (passed, printed) = pack(&folder);
        assert!(!passed, "{printed}");
        assert!(printed.contains("a symbolic link"), "{printed}");
        assert!(
            printed.contains("Pane unpacks only files and folders"),
            "{printed}"
        );
    } else {
        eprintln!("skipped: this system does not allow symbolic links");
    }
    let _ = fs::remove_file(&link);

    // A name no system takes, declared for a helper of another target:
    // Pane's unpacker refuses the name wherever the file is.
    let helper = format!(
        r#",
  "helpers": [
    {{ "id": "tool", "targets": {{ "{}": "helpers/pane-echo " }} }}
  ]"#,
        other_target()
    );
    fs::write(folder.join("pane.json"), manifest(true, &helper)).unwrap();
    let (passed, printed) = pack(&folder);
    assert!(!passed, "{printed}");
    assert!(
        printed.contains("whose name ends in `.` or a space"),
        "{printed}"
    );
    assert!(
        printed.contains("Pane unpacks only paths inside the package"),
        "{printed}"
    );
    fs::write(folder.join("pane.json"), manifest(true, "")).unwrap();

    // A missing component: the build succeeds, but the folder holds a
    // folder where the component should be, which Pane's install refuses.
    let component = folder.join("hello_rust.wasm");
    let _ = fs::remove_file(&component);
    let _ = fs::remove_dir_all(&component);
    fs::create_dir(&component).unwrap();
    let (passed, printed) = pack(&folder);
    assert!(!passed, "{printed}");
    assert!(
        printed.contains(
            "Not ready to run: the component hello_rust.wasm of \"Hello Rust\" is missing. \
             This looks like a source-only package"
        ),
        "{printed}"
    );
    let _ = fs::remove_dir(&component);

    // An oversized entry: more than Pane ever unpacks.
    let huge = folder.join("assets/huge.bin");
    fs::File::create(&huge)
        .unwrap()
        .set_len((256 << 20) + 1)
        .unwrap();
    let (passed, printed) = pack(&folder);
    assert!(!passed, "{printed}");
    assert!(
        printed.contains("the package unpacks to more than the 256 MiB Pane allows"),
        "{printed}"
    );
    let _ = fs::remove_file(&huge);

    // A `files` list that does not cover what the tarball needs: npm would
    // pack the package without its pane.json.
    fs::write(
        folder.join("package.json"),
        r#"{ "name": "@pane-samples/hello-rust", "version": "0.1.0", "files": ["hello_rust.wasm"] }"#,
    )
    .unwrap();
    let (passed, printed) = pack(&folder);
    assert!(!passed, "{printed}");
    assert!(printed.contains("`files` does not cover"), "{printed}");
    assert!(printed.contains("`pane.json`"), "{printed}");
    assert!(
        printed.contains("npm would pack the package without"),
        "{printed}"
    );
    fs::write(folder.join("package.json"), PACKAGE_JSON).unwrap();

    // No icon, or one that is not 512×512: an error when packing, where
    // check only warns.
    fs::write(folder.join("pane.json"), manifest(false, "")).unwrap();
    let (passed, printed) = pack(&folder);
    assert!(!passed, "{printed}");
    assert!(
        printed.contains("the package has no icon of its own"),
        "{printed}"
    );
    assert!(
        printed.contains("a published extension's icon is a 512×512 image"),
        "{printed}"
    );
    fs::write(folder.join("pane.json"), manifest(true, "")).unwrap();

    // Once every problem is fixed, the package packs again.
    let (passed, printed) = pack(&folder);
    assert!(passed, "{printed}");
}

#[test]
fn a_git_distributed_package_is_checked_as_the_folder_of_its_release_revision() {
    // No package.json: the release revision is the repository's tree, and
    // pack writes no tarball.
    let folder = sample("pane-ext-pack-git", &manifest(true, ""), None);
    fs::write(folder.join("icon.png"), icon_png()).unwrap();
    let (passed, printed) = pack(&folder);
    assert!(passed, "{printed}");
    assert!(!folder.join("dist").exists(), "{printed}");
    assert!(folder.join("hello_rust.wasm").is_file(), "{printed}");
    assert!(printed.contains("the release revision"), "{printed}");
    assert!(printed.contains("files and folders"), "{printed}");

    // A link in the tree, as Pane's write-out of a revision refuses it.
    let link = folder.join("src/link.rs");
    let Some(()) = symlink(&folder.join("pane.json"), &link) else {
        eprintln!("skipped: this system does not allow symbolic links");
        return;
    };
    let (passed, printed) = pack(&folder);
    assert!(!passed, "{printed}");
    assert!(printed.contains("a symbolic link"), "{printed}");
    assert!(
        printed.contains("Pane takes only files and folders every system can write"),
        "{printed}"
    );
}

#[test]
fn a_folder_without_a_manifest_is_not_a_package() {
    let folder = tempfile::tempdir().unwrap();
    let (passed, printed) = pack(folder.path());
    assert!(!passed, "{printed}");
    assert!(printed.contains("has no pane.json"), "{printed}");
}

#[test]
fn an_argument_pack_does_not_take_is_a_usage_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_pane-ext"))
        .arg("pack")
        .arg("--nope")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        printed.contains("is not an argument pack takes"),
        "{printed}"
    );
    assert!(printed.contains("Usage: pane-ext"), "{printed}");
}
