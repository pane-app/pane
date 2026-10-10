//! `@pane-app/create` (`packages/create`), the package `npm create
//! @pane-app` runs (#221, ADR 0047): its bin starts `pane-ext new` with
//! the arguments npm passed, so an author starts a Pane extension with
//! Node.js and npm alone. What is testable before the packages are
//! published (#281, a person's step): the package packs (`npm pack`, the
//! same tarball npm would publish), and its bin, run from where npm
//! installs it with the `pane-ext` binary in the `@pane-app/cli` platform
//! package's folder — the layout `@pane-app/cli`'s dependency puts it in,
//! the same one Pane's own build looks for the componentizer in —
//! scaffolds the project. The one-step `npm create @pane-app` itself
//! needs the published packages, so it is the README's, not CI's, story.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use pane_core::Target;

/// The npm pack tarball of packages/create, packed into `destination` and
/// read whole.
fn pack(destination: &Path) -> PathBuf {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let output = Command::new(npm)
        .current_dir(repository().join("packages/create"))
        .args(["pack", "--pack-destination"])
        .arg(destination)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "npm pack failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let tarball = destination.join("pane-app-create-0.1.0.tgz");
    assert!(tarball.is_file(), "npm packed no {}", tarball.display());
    tarball
}

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// The files `tarball` holds, under the `package/` root npm packs.
fn files(tarball: &Path) -> Vec<String> {
    let bytes = fs::read(tarball).unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&bytes[..]));
    archive
        .entries()
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert_eq!(
                entry.header().entry_type(),
                tar::EntryType::Regular,
                "the package holds only files"
            );
            let path = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
            path
        })
        .collect()
}

/// Unpacks `tarball` into `folder`, as npm install of it would put it at
/// `folder/@pane-app/create`.
fn unpack(tarball: &Path, create: &Path) {
    let bytes = fs::read(tarball).unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&bytes[..]));
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        let Some(file) = path.strip_prefix("package/") else {
            continue;
        };
        let to = create.join(file);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents).unwrap();
        fs::write(&to, contents).unwrap();
    }
}

/// Puts the pane-ext binary this test's own build made where
/// `@pane-app/cli`'s platform package holds it, for this system's target.
fn install_cli(node_modules: &Path) {
    let Some(target) = Target::current() else {
        panic!("Pane names no target for this system");
    };
    let package = node_modules.join(format!("@pane-app/cli-{}", target.id()));
    fs::create_dir_all(&package).unwrap();
    let program = package.join(if cfg!(windows) {
        "pane-ext.exe"
    } else {
        "pane-ext"
    });
    fs::copy(env!("CARGO_BIN_EXE_pane-ext"), &program).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Runs the create package's bin with `args`, returning whether it
/// succeeded and what it printed.
fn create(create_js: &Path, args: &[&str]) -> (bool, String) {
    let output = Command::new("node")
        .arg(create_js)
        .args(args)
        .output()
        .unwrap();
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), printed)
}

#[test]
fn the_create_package_packs_and_its_bin_scaffolds_a_project() {
    let work = tempfile::tempdir().unwrap();
    let tarball = pack(work.path());
    assert_eq!(
        files(&tarball),
        vec![
            "package/LICENSE-APACHE".to_owned(),
            "package/LICENSE-MIT".to_owned(),
            "package/README.md".to_owned(),
            "package/create.js".to_owned(),
            "package/package.json".to_owned(),
        ]
    );
    // Where npm install puts the published package, with the pane-ext
    // binary beside it in the platform package its dependency installs.
    let node_modules = work.path().join("node_modules");
    let create = node_modules.join("@pane-app/create");
    unpack(&tarball, &create);
    install_cli(&node_modules);

    let project = work.path().join("notes");
    let (passed, printed) = create(
        &create.join("create.js"),
        &[
            project.to_str().unwrap(),
            "--name",
            "Word Count",
            "--language",
            "typescript",
            "--template",
            "form",
        ],
    );
    assert!(passed, "{printed}");
    assert!(
        printed.contains("wrote Word Count (the form template, in TypeScript) into"),
        "{printed}"
    );
    let manifest = fs::read_to_string(project.join("pane.json")).unwrap();
    assert!(manifest.contains("\"title\": \"Word Count\""), "{manifest}");
    assert!(project.join("package.json").is_file());
    assert!(project.join("src/index.ts").is_file());
}

#[test]
fn the_create_bin_says_how_to_get_pane_ext_when_it_finds_none() {
    let work = tempfile::tempdir().unwrap();
    let tarball = pack(work.path());
    let create = work.path().join("create");
    unpack(&tarball, &create);
    // No @pane-app/cli platform package beside it, and no pane-ext on the
    // PATH the test runs with.
    let (passed, printed) = create(
        &create.join("create.js"),
        &[work.path().join("nowhere").to_str().unwrap()],
    );
    assert!(!passed, "{printed}");
    assert!(printed.contains("found no pane-ext to run"), "{printed}");
    assert!(printed.contains("@pane-app/cli"), "{printed}");
}
