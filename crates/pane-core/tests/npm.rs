//! Installing extension packages from npm through the launcher's public
//! interface, from a local registry on 127.0.0.1 that each test fills
//! (`support/npm_registry.rs`): nothing here reaches the network or the real
//! npm registry. The npm sample `cargo xtask guests` assembles
//! (`target/guests/npm/greeter`, from `guests/npm/greeter`) is the package:
//! a JavaScript command and a `greet` operation, whose answers name the npm
//! package ("Hello from the npm package").
//!
//! What is checked: the preview before anything is installed; installing
//! and running its command; the npm name as the identity, so that a second
//! install is refused and choosing it again offers Update, with an exact
//! version pinning it; npm dependencies of a local package, installed with
//! it, pinned or already installed; and every way a download is refused,
//! from a missing package to a tarball that would write outside its folder.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::npm::Registry as NpmRegistry;
use pane_core::{Launcher, PackageIdentity, Runtime, SavedData, Screen, Status};
use serde_json::{Value, json};
use tempfile::TempDir;

#[path = "support/npm_registry.rs"]
mod npm_registry;
#[path = "support/unreachable.rs"]
mod unreachable;

use npm_registry::{Registry, greeter_files, integrity, pack, pack_raw};

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::{guest_file as guest, guests};
use rows::{select_title, titles};

const GREETER: &str = "@pane-samples/greeter";

struct Dirs {
    sources: TempDir,
    data: TempDir,
    runtime: Runtime,
    registry: Registry,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            runtime: Runtime::start().unwrap(),
            registry: Registry::start(),
        }
    }

    fn packages_dir(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// A launcher on this data folder downloading from the local registry;
    /// a new one is a restart of Pane.
    fn launcher(&self) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.packages_dir())
            .with_npm_registry(NpmRegistry::local(self.registry.url()).unwrap())
    }

    /// Publishes the npm sample at `version`, tagged latest.
    fn publish_greeter(&self, version: &str) {
        let tarball = pack(&greeter_files(&guests(), version));
        self.registry.publish(GREETER, version, tarball);
    }

    /// Copies the assembled sample package `package` into source folder
    /// `name`.
    fn sample_in(&self, package: &str, name: &str) -> PathBuf {
        let assembled = guest("packages").join(package);
        let folder = self.sources.path().join(name);
        fs::create_dir_all(&folder).unwrap();
        for entry in fs::read_dir(assembled).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
        }
        folder
    }

    /// Writes package "Caller" in source folder `caller`, calling `greet`
    /// through `dependencies` (JSON array contents).
    fn caller(&self, dependencies: &str) -> PathBuf {
        let folder = self.sources.path().join("caller");
        fs::create_dir_all(&folder).unwrap();
        fs::copy(
            guest("sample_dependencies.wasm"),
            folder.join("caller.wasm"),
        )
        .unwrap();
        let manifest = format!(
            r#"{{ "manifestVersion": 1, "title": "Caller", "apiVersion": "0.1",
                 "commands": [{{ "id": "greet", "title": "Greet through dependencies",
                                 "component": "caller.wasm" }}],
                 "dependencies": [{dependencies}] }}"#
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        folder
    }

    /// The record of the package from npm `name` in `installed.json`.
    fn record(&self, name: &str) -> Value {
        let text = fs::read_to_string(self.packages_dir().join("installed.json")).unwrap();
        let registry: Value = serde_json::from_str(&text).unwrap();
        registry["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["npm"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("no record of {name} in {registry:#}"))
    }

    /// Waits until nothing downloaded is left in Pane's downloads folder,
    /// which is emptied in the background once an install ends.
    fn wait_for_no_downloads(&self) {
        let downloads = self.packages_dir().join("downloads");
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let left: Vec<_> = fs::read_dir(&downloads)
                .map(|entries| entries.map(|e| e.unwrap().file_name()).collect())
                .unwrap_or_default();
            if left.is_empty() {
                return;
            }
            assert!(Instant::now() < deadline, "downloads left: {left:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn details(launcher: &Launcher) -> Vec<String> {
    launcher.view().details().to_vec()
}

fn has(details: &[String], line: &str) -> bool {
    details.iter().any(|detail| detail == line)
}

/// The installed packages' titles, in installed order.
fn installed(launcher: &Launcher) -> Vec<String> {
    launcher.packages().iter().map(|p| p.title()).collect()
}

/// Opens the command titled `command` from root search and runs its item
/// titled `item`, returning what it showed: its toast, or the status line.
fn run(launcher: &Launcher, command: &str, item: &str) -> Status {
    for _ in 0..3 {
        launcher.back();
    }
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    shown(launcher)
}

fn error_of(launcher: &Launcher) -> String {
    match launcher.view().status {
        Status::Error(text) => text,
        other => panic!("not an error: {other:?}"),
    }
}

#[test]
fn a_package_from_npm_is_previewed_installed_and_its_command_runs() {
    let dirs = Dirs::new();
    dirs.publish_greeter("0.1.0");
    let launcher = dirs.launcher();

    block_on(launcher.preview_npm(GREETER));

    let view = launcher.view();
    assert_eq!(view.title, "Greeter from npm");
    let details = details(&launcher);
    let expected = [
        "Source: npm package @pane-samples/greeter".to_owned(),
        "Version: 0.1.0".into(),
        "npm version: 0.1.0, the latest".into(),
        format!(
            "Downloaded: {}, matching its sha512 integrity from the registry",
            dirs.registry.tarball_url(GREETER, "0.1.0")
        ),
        "Runs only the WebAssembly components its pane.json names, in Pane: no Node.js, npm \
         install scripts or npm dependencies"
            .into(),
        "Commands: Greeter from npm".into(),
        "Operations: greet (version 1)".into(),
    ];
    for line in &expected {
        assert!(has(&details, line), "{line:?} not in {details:#?}");
    }
    assert_eq!(titles(&launcher), ["Install"]);
    // Nothing is installed by the preview.
    assert!(launcher.packages().is_empty());
    assert!(!dirs.packages_dir().join("installed.json").exists());

    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Greeter from npm".into())
    );
    let package = &launcher.packages()[0];
    assert_eq!(package.identity, PackageIdentity::npm(GREETER));
    assert_eq!(package.identity.key(), "npm:@pane-samples/greeter");
    let record = dirs.record(GREETER);
    assert_eq!(record["npmVersion"], "0.1.0");
    assert_eq!(record.get("pinned"), None);
    assert_eq!(record.get("local"), None);
    assert_eq!(
        run(&launcher, "Greeter from npm", "Say hello"),
        Status::Result("Hello from the npm package".into())
    );
    // What was downloaded is removed once installed; the managed copy has
    // only the manifest and the components it names.
    dirs.wait_for_no_downloads();
    let copy = &package.location;
    let mut files: Vec<String> = fs::read_dir(copy)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    assert_eq!(files, ["pane.json", "sample_npm_js.wasm"]);

    // After a restart it is listed from its managed copy, with nothing
    // downloaded again.
    let asked = dirs.registry.requests().len();
    drop(launcher);
    let launcher = dirs.launcher();
    assert_eq!(installed(&launcher), ["Greeter from npm"]);
    assert_eq!(
        run(&launcher, "Greeter from npm", "Say hello"),
        Status::Result("Hello from the npm package".into())
    );
    assert_eq!(dirs.registry.requests().len(), asked);
}

#[test]
fn a_preview_keeps_nothing_it_downloaded() {
    let dirs = Dirs::new();
    dirs.publish_greeter("0.1.0");
    let folder = dirs.sample_in("sample-dependencies-npm", "sample-dependencies-npm");
    let launcher = dirs.launcher();

    // The npm package itself, then one a local package requires: once
    // shown, neither download is needed (installing downloads again).
    block_on(launcher.preview_npm(GREETER));
    assert_eq!(titles(&launcher), ["Install"]);
    dirs.wait_for_no_downloads();
    block_on(launcher.preview_package(&folder));
    assert!(has(
        &details(&launcher),
        "Requires: Greeter from npm, installed with it from npm:@pane-samples/greeter"
    ));
    dirs.wait_for_no_downloads();
    // Left for root, then installed: nothing is kept either.
    launcher.back();
    block_on(launcher.preview_package(&folder));
    block_on(launcher.activate_selected());
    assert_eq!(
        installed(&launcher),
        ["Greeter from npm", "Dependencies from npm sample"]
    );
    dirs.wait_for_no_downloads();
}

#[test]
fn a_package_whose_component_fails_its_check_keeps_nothing_downloaded() {
    let dirs = Dirs::new();
    let mut files = greeter_files(&guests(), "0.1.0");
    for (path, contents) in &mut files {
        if path.ends_with(".wasm") {
            *contents = b"not a component".to_vec();
        }
    }
    dirs.registry.publish(GREETER, "0.1.0", pack(&files));
    let launcher = dirs.launcher();

    block_on(launcher.preview_npm(GREETER));
    let error = error_of(&launcher);
    assert!(error.contains("sample_npm_js.wasm"), "{error}");
    dirs.wait_for_no_downloads();
    block_on(launcher.install_npm(GREETER));
    assert!(launcher.packages().is_empty());
    dirs.wait_for_no_downloads();
}

#[test]
fn a_start_removes_only_downloads_abandoned_long_ago() {
    let dirs = Dirs::new();
    let downloads = dirs.packages_dir().join("downloads");
    // Named by when they were begun, in seconds since 1970: one a day and
    // more ago (left by a Pane that stopped), one begun just now (another
    // Pane's install in progress on this data folder).
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let abandoned = downloads.join(format!("{}-4242-0", now - 25 * 60 * 60));
    let unfinished = downloads.join(format!(".{}-4242-1", now - 25 * 60 * 60));
    let in_progress = downloads.join(format!("{now}-4343-0"));
    let unpacking = downloads.join(format!(".{now}-4343-1"));
    for folder in [&abandoned, &unfinished, &in_progress, &unpacking] {
        fs::create_dir_all(folder).unwrap();
        fs::write(folder.join("pane.json"), "{}").unwrap();
    }

    let _launcher = dirs.launcher();

    assert!(!abandoned.exists() && !unfinished.exists());
    assert!(in_progress.join("pane.json").exists());
    assert!(unpacking.join("pane.json").exists());
}

#[test]
fn the_form_in_root_search_asks_for_the_npm_package() {
    let dirs = Dirs::new();
    dirs.publish_greeter("0.1.0");
    let launcher = dirs.launcher();

    select_title(&launcher, "Install extension from npm…");
    block_on(launcher.activate_selected());
    let form = launcher.view().form().cloned().expect("the npm form");
    assert_eq!(launcher.view().title, "Install extension from npm");
    assert_eq!(form.submit_label, "Show package");
    // Back leaves it for root search.
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));

    select_title(&launcher, "Install extension from npm…");
    block_on(launcher.activate_selected());
    launcher.set_field_value("package", &format!(" {GREETER}@0.1.0 "));
    block_on(launcher.submit_form());

    assert_eq!(launcher.view().title, "Greeter from npm");
    assert!(has(
        &details(&launcher),
        "npm version: 0.1.0, the version you named: installing pins it to that version"
    ));
    block_on(launcher.activate_selected());
    assert_eq!(dirs.record(GREETER)["pinned"], true);
    // Back from the package screen reaches root search, not the form.
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
}

#[test]
fn the_npm_name_is_the_identity_a_second_install_is_refused_and_choosing_it_again_updates() {
    let dirs = Dirs::new();
    dirs.publish_greeter("0.1.0");
    let launcher = dirs.launcher();
    block_on(launcher.install_npm(GREETER));
    assert_eq!(installed(&launcher), ["Greeter from npm"]);

    // Whatever the version asked for, the same name is the same package.
    block_on(launcher.install_npm(&format!("{GREETER}@0.1.0")));
    assert_eq!(
        error_of(&launcher),
        "Already installed from npm package @pane-samples/greeter; use Update to replace the \
         installed copy"
    );
    assert_eq!(installed(&launcher), ["Greeter from npm"]);

    // A newer version, named exactly: Update, pinning it.
    dirs.registry.publish_with(
        GREETER,
        "0.2.0",
        pack(&greeter_files(&guests(), "0.2.0")),
        None,
    );
    block_on(launcher.preview_npm(&format!("{GREETER}@0.2.0")));
    let details = details(&launcher);
    assert!(
        has(&details, "Installed: npm version 0.1.0 of this package"),
        "{details:#?}"
    );
    assert_eq!(titles(&launcher), ["Update"]);
    assert_eq!(
        launcher.view().rows[0].subtitle.as_deref(),
        Some("Replace the installed copy with npm version 0.2.0, pinned")
    );
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Updated Greeter from npm to 0.2.0".into())
    );
    let record = dirs.record(GREETER);
    assert_eq!(
        (&record["npmVersion"], &record["pinned"]),
        (&json!("0.2.0"), &json!(true))
    );

    // Without a version, a pinned package keeps its pin: the update is of
    // the version it is pinned to, not the latest (0.1.0 here).
    block_on(launcher.preview_npm(GREETER));
    let details = self::details(&launcher);
    assert!(
        has(
            &details,
            "Installed: npm version 0.2.0, pinned of this package"
        ),
        "{details:#?}"
    );
    assert!(
        has(
            &details,
            "npm version: 0.2.0, the version it is pinned to: name another version to change it"
        ),
        "{details:#?}"
    );
    assert_eq!(
        launcher.view().rows[0].subtitle.as_deref(),
        Some("Replace the installed copy with npm version 0.2.0, pinned")
    );
    block_on(launcher.activate_selected());
    let record = dirs.record(GREETER);
    assert_eq!(
        (&record["npmVersion"], &record["pinned"]),
        (&json!("0.2.0"), &json!(true))
    );

    // Naming another version changes the pin.
    block_on(launcher.preview_npm(&format!("{GREETER}@0.1.0")));
    block_on(launcher.activate_selected());
    let record = dirs.record(GREETER);
    assert_eq!(
        (&record["npmVersion"], &record["pinned"]),
        (&json!("0.1.0"), &json!(true))
    );
    assert_eq!(installed(&launcher), ["Greeter from npm"]);
}

#[test]
fn a_local_package_requiring_an_npm_package_installs_it_and_calls_it_by_id() {
    let dirs = Dirs::new();
    dirs.publish_greeter("0.1.0");
    let folder = dirs.sample_in("sample-dependencies-npm", "sample-dependencies-npm");
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&folder));

    let details = details(&launcher);
    assert!(
        has(
            &details,
            "Requires: Greeter from npm, installed with it from npm:@pane-samples/greeter"
        ),
        "{details:#?}"
    );
    assert_eq!(
        launcher.view().rows[0].subtitle.as_deref(),
        Some(
            "Copy the package into Pane and add its commands, and install Greeter from npm, \
             which it requires"
        )
    );
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result(
            "Installed Dependencies from npm sample with Greeter from npm, which it requires"
                .into()
        )
    );
    assert_eq!(
        installed(&launcher),
        ["Greeter from npm", "Dependencies from npm sample"]
    );
    assert_eq!(
        run(
            &launcher,
            "Greet through an npm dependency",
            "Greet through the required greeter"
        ),
        Status::Result("Hello, Pane, from the npm package".into())
    );
    // The dependency's record names the npm package.
    let text = fs::read_to_string(dirs.packages_dir().join("installed.json")).unwrap();
    let registry: Value = serde_json::from_str(&text).unwrap();
    let dependent = &registry["packages"][1];
    assert_eq!(
        dependent["dependencies"][0],
        json!({ "id": "greeter", "npm": GREETER })
    );
    dirs.wait_for_no_downloads();
}

#[test]
fn an_installed_npm_dependency_is_used_as_it_is_and_a_disabled_one_stays_disabled() {
    let dirs = Dirs::new();
    dirs.publish_greeter("0.1.0");
    let launcher = dirs.launcher();
    block_on(launcher.install_npm(GREETER));
    // A newer version is published since.
    dirs.publish_greeter("0.2.0");
    let caller = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "npm:{GREETER}", "operations": [{{ "id": "greet", "version": 1 }}] }}"#
    ));

    block_on(launcher.preview_package(&caller));
    assert!(has(
        &details(&launcher),
        "Requires: Greeter from npm, already installed"
    ));
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Caller".into())
    );
    // Installing another package never updates it: still 0.1.0, and the
    // newer tarball was never asked for.
    assert_eq!(dirs.record(GREETER)["npmVersion"], "0.1.0");
    assert!(
        !dirs
            .registry
            .tarball_requests()
            .iter()
            .any(|path| path.ends_with("greeter-0.2.0.tgz")),
        "{:?}",
        dirs.registry.requests()
    );
    assert_eq!(
        run(
            &launcher,
            "Greet through dependencies",
            "Greet through the required greeter"
        ),
        Status::Result("Hello, Pane, from the npm package".into())
    );

    // Disabled, it stays so, and the preview says it.
    block_on(launcher.set_enabled(&PackageIdentity::npm(GREETER), false));
    block_on(launcher.preview_package(&caller));
    let details = details(&launcher);
    assert!(
        has(
            &details,
            "Requires: Greeter from npm, which you disabled: it stays disabled, and Caller \
             cannot use it until you enable it in Manage extensions"
        ),
        "{details:#?}"
    );
}

#[test]
fn an_npm_dependency_that_can_reach_the_network_is_recorded_as_using_it() {
    // A package on npm with #30's search command, whose component imports
    // `wasi:http`, and the Rust `greet` operation, which a local package
    // requires.
    let dirs = Dirs::new();
    let package_json = json!({ "name": "net-search", "version": "1.0.0" }).to_string();
    let manifest = json!({
        "manifestVersion": 1, "title": "Search with greet", "version": "1.0.0",
        "apiVersion": "0.1",
        "commands": [{ "id": "packages", "title": "Package search",
                       "component": "sample_search.wasm", "search": true }],
        "operations": [{ "id": "greet", "version": 1, "component": "sample_operations.wasm" }]
    })
    .to_string();
    let files = vec![
        ("package.json", package_json.into_bytes()),
        ("pane.json", manifest.into_bytes()),
        (
            "sample_search.wasm",
            fs::read(guest("sample_search.wasm")).unwrap(),
        ),
        (
            "sample_operations.wasm",
            fs::read(guest("sample_operations.wasm")).unwrap(),
        ),
    ];
    dirs.registry.publish("net-search", "1.0.0", pack(&files));
    let caller = dirs.caller(
        r#"{ "id": "greeter", "source": "npm:net-search", "operations": [{ "id": "greet", "version": 1 }] }"#,
    );
    let launcher = dirs.launcher();

    block_on(launcher.install_package(&caller));

    let packages = launcher.packages();
    let search = packages
        .iter()
        .find(|package| package.identity == PackageIdentity::npm("net-search"))
        .unwrap_or_else(|| panic!("not installed: {:?}", launcher.view().status));
    assert!(search.uses_network, "{search:?}");
    let caller = packages
        .iter()
        .find(|package| package.title() == "Caller")
        .unwrap();
    assert!(!caller.uses_network);
    // As recorded, so after a restart too.
    drop(launcher);
    let launcher = dirs.launcher();
    assert!(launcher.packages().iter().any(|package| package.identity
        == PackageIdentity::npm("net-search")
        && package.uses_network));
}

#[test]
fn a_dependency_naming_a_version_installs_that_version_pinned() {
    let dirs = Dirs::new();
    dirs.publish_greeter("0.1.0");
    dirs.publish_greeter("0.2.0");
    let caller = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "npm:{GREETER}@0.1.0", "operations": [{{ "id": "greet", "version": 1 }}] }}"#
    ));
    let launcher = dirs.launcher();

    block_on(launcher.install_package(&caller));

    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Caller with Greeter from npm, which it requires".into())
    );
    let record = dirs.record(GREETER);
    assert_eq!(
        (&record["npmVersion"], &record["pinned"]),
        (&json!("0.1.0"), &json!(true))
    );
}

#[test]
fn a_dependency_pinning_another_version_than_the_installed_one_is_a_conflict() {
    let dirs = Dirs::new();
    dirs.publish_greeter("0.1.0");
    let launcher = dirs.launcher();
    block_on(launcher.install_npm(GREETER));
    dirs.registry.publish_with(
        GREETER,
        "0.2.0",
        pack(&greeter_files(&guests(), "0.2.0")),
        None,
    );
    let caller = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "npm:{GREETER}@0.2.0", "operations": [{{ "id": "greet", "version": 1 }}] }}"#
    ));

    block_on(launcher.preview_package(&caller));

    let conflict = "Nothing was installed: Caller requires Greeter from npm at npm version 0.2.0, \
                    and version 0.1.0 is installed; Pane does not replace the installed copy \
                    while installing another extension: update it to 0.2.0 (npm package \
                    @pane-samples/greeter@0.2.0) if Caller needs that version";
    assert_eq!(launcher.view().title, "Cannot install Caller");
    assert_eq!(error_of(&launcher), conflict);
    assert!(titles(&launcher).is_empty());
    block_on(launcher.install_package(&caller));
    assert_eq!(error_of(&launcher), conflict);
    assert_eq!(installed(&launcher), ["Greeter from npm"]);
    assert_eq!(dirs.record(GREETER)["npmVersion"], "0.1.0");

    // The version installed, named exactly, is no conflict.
    let caller = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "npm:{GREETER}@0.1.0", "operations": [{{ "id": "greet", "version": 1 }}] }}"#
    ));
    block_on(launcher.preview_package(&caller));
    assert!(has(
        &details(&launcher),
        "Requires: Greeter from npm, already installed"
    ));
}

#[test]
fn two_dependents_pinning_different_versions_of_one_npm_package_conflict() {
    let dirs = Dirs::new();
    dirs.publish_greeter("0.1.0");
    dirs.publish_greeter("0.2.0");
    // "Middle", on npm, pins the greeter at 0.2.0; the caller pins it at
    // 0.1.0 and requires Middle too.
    let mut files = greeter_files(&guests(), "1.0.0");
    for (path, contents) in &mut files {
        let mut json: Value = match *path {
            "package.json" | "pane.json" => serde_json::from_slice(contents).unwrap(),
            _ => continue,
        };
        if *path == "package.json" {
            json["name"] = json!("middle");
        } else {
            json["title"] = json!("Middle");
            json["dependencies"] = json!([{
                "id": "greeter", "source": format!("npm:{GREETER}@0.2.0"),
                "operations": [{ "id": "greet", "version": 1 }]
            }]);
        }
        *contents = serde_json::to_vec(&json).unwrap();
    }
    dirs.registry.publish("middle", "1.0.0", pack(&files));
    let caller = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "npm:{GREETER}@0.1.0", "operations": [{{ "id": "greet", "version": 1 }}] }},
           {{ "id": "middle", "source": "npm:middle", "operations": [{{ "id": "greet", "version": 1 }}] }}"#
    ));
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&caller));

    assert_eq!(
        error_of(&launcher),
        "Nothing was installed: Middle requires Greeter from npm at npm version 0.2.0, and \
         Caller requires version 0.1.0; Pane installs one copy of each package, so they cannot \
         both have theirs"
    );

    // Unpinned, the first to name it takes the latest; a pin of another
    // version conflicts with that.
    let caller = dirs.caller(&format!(
        r#"{{ "id": "greeter", "source": "npm:{GREETER}", "operations": [{{ "id": "greet", "version": 1 }}] }},
           {{ "id": "middle", "source": "npm:middle", "operations": [{{ "id": "greet", "version": 1 }}] }}"#
    ));
    dirs.registry.tag_latest(GREETER, "0.1.0");
    block_on(launcher.preview_package(&caller));
    assert_eq!(
        error_of(&launcher),
        "Nothing was installed: Middle requires Greeter from npm at npm version 0.2.0, and \
         Caller takes its latest, version 0.1.0; Pane installs one copy of each package, so they \
         cannot both have theirs"
    );
    assert!(launcher.packages().is_empty());
}

#[test]
fn a_required_npm_dependency_that_cannot_be_downloaded_stops_the_install() {
    let dirs = Dirs::new();
    let caller = dirs.caller(
        r#"{ "id": "greeter", "source": "npm:nobody", "operations": [{ "id": "greet", "version": 1 }] }"#,
    );
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&caller));

    assert_eq!(launcher.view().title, "Cannot install Caller");
    assert_eq!(
        error_of(&launcher),
        format!(
            "Nothing was installed: Caller requires `greeter` from npm package nobody, which \
             cannot be installed: npm package nobody was not found in the registry {}",
            dirs.registry.url()
        )
    );
    assert!(titles(&launcher).is_empty());
}

#[test]
fn an_npm_package_naming_a_local_folder_is_refused() {
    let dirs = Dirs::new();
    let mut files = greeter_files(&guests(), "0.1.0");
    let mut manifest: Value = serde_json::from_slice(&files[1].1).unwrap();
    manifest["dependencies"] = json!([{
        "id": "helper", "source": "local:../helper",
        "operations": [{ "id": "greet", "version": 1 }]
    }]);
    files[1].1 = serde_json::to_vec(&manifest).unwrap();
    dirs.registry.publish(GREETER, "0.1.0", pack(&files));
    let launcher = dirs.launcher();

    block_on(launcher.preview_npm(GREETER));

    assert_eq!(
        error_of(&launcher),
        "Nothing was installed: Greeter from npm comes from npm but names the local folder \
         `local:../helper` as its dependency `helper`; a package published to npm or Git can \
         depend only on packages from npm or Git"
    );
}

/// Previews `spec` and returns the error shown, checking that nothing was
/// installed and nothing is offered.
fn refusal(dirs: &Dirs, spec: &str) -> String {
    let launcher = dirs.launcher();
    block_on(launcher.preview_npm(spec));
    assert!(titles(&launcher).is_empty(), "{:?}", titles(&launcher));
    assert!(launcher.view().title.starts_with("Cannot install "));
    assert!(launcher.packages().is_empty());
    error_of(&launcher)
}

#[test]
fn a_missing_package_or_version_is_explained() {
    let dirs = Dirs::new();
    dirs.publish_greeter("0.1.0");
    assert_eq!(
        refusal(&dirs, "nobody"),
        format!(
            "npm package nobody was not found in the registry {}",
            dirs.registry.url()
        )
    );
    assert_eq!(
        refusal(&dirs, &format!("{GREETER}@9.9.9")),
        "npm package @pane-samples/greeter has no version 9.9.9; its latest is 0.1.0"
    );
    // A range, a tag or a name npm does not accept is refused before
    // anything is asked of the registry.
    let asked = dirs.registry.requests().len();
    assert!(refusal(&dirs, &format!("{GREETER}@^0.1.0")).contains("is not an exact version"));
    assert!(refusal(&dirs, "Greeter").contains("is not an npm package name"));
    assert_eq!(dirs.registry.requests().len(), asked);
}

#[test]
fn an_unreachable_registry_is_explained() {
    // A port that refuses connections, kept so for the whole test.
    let closed = unreachable::ClosedPort::new();
    let url = format!("{}/", closed.url());
    let error = unreachable_refusal(&url);
    assert!(
        error.starts_with(&format!(
            "Could not reach the npm registry {url} for @pane-samples/greeter:"
        )),
        "{error}"
    );
}

#[test]
fn a_registry_whose_certificate_the_system_does_not_trust_is_refused() {
    // The system's certificates decide, as for guests' requests.
    let untrusted = unreachable::UntrustedService::start();
    let url = format!("{}/", untrusted.url());
    let error = unreachable_refusal(&url);
    assert_eq!(
        error,
        format!(
            "Could not reach the npm registry {url} for @pane-samples/greeter: TLS certificate \
             error"
        )
    );
}

/// Why previewing the sample from the registry at `url` is refused.
fn unreachable_refusal(url: &str) -> String {
    let dirs = Dirs::new();
    let launcher = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir())
        .with_npm_registry(NpmRegistry::local(url).unwrap());
    block_on(launcher.preview_npm(GREETER));
    error_of(&launcher)
}

#[test]
fn a_download_that_does_not_match_its_integrity_is_refused() {
    let dirs = Dirs::new();
    let tarball = pack(&greeter_files(&guests(), "0.1.0"));
    let other = integrity(b"another tarball");
    dirs.registry.publish_with(
        GREETER,
        "0.1.0",
        tarball,
        Some(json!({ "tarball": dirs.registry.tarball_url(GREETER, "0.1.0"), "integrity": other })),
    );
    dirs.registry.tag_latest(GREETER, "0.1.0");

    let error = refusal(&dirs, GREETER);
    assert!(
        error.starts_with(
            "The download of npm package @pane-samples/greeter@0.1.0 does not match the sha512 \
             integrity the registry gives"
        ),
        "{error}"
    );
    assert!(error.ends_with("; nothing was installed"), "{error}");
}

#[test]
fn a_version_without_sha512_integrity_is_refused() {
    let dirs = Dirs::new();
    let tarball = pack(&greeter_files(&guests(), "0.1.0"));
    dirs.registry.publish_with(
        GREETER,
        "0.1.0",
        tarball,
        Some(json!({
            "tarball": dirs.registry.tarball_url(GREETER, "0.1.0"),
            "shasum": "0000000000000000000000000000000000000000"
        })),
    );
    dirs.registry.tag_latest(GREETER, "0.1.0");

    assert_eq!(
        refusal(&dirs, GREETER),
        "npm package @pane-samples/greeter@0.1.0 has no sha512 integrity in the registry, \
         which Pane needs to check its download"
    );
    // Its tarball was never asked for.
    assert!(dirs.registry.tarball_requests().is_empty());
}

#[test]
fn a_tarball_elsewhere_than_the_registry_is_not_downloaded() {
    let dirs = Dirs::new();
    let tarball = pack(&greeter_files(&guests(), "0.1.0"));
    // Another server: the same computer under another name and port.
    let elsewhere = "http://localhost:9/greeter-0.1.0.tgz";
    dirs.registry.publish_with(
        GREETER,
        "0.1.0",
        tarball.clone(),
        Some(json!({ "tarball": elsewhere, "integrity": integrity(&tarball) })),
    );
    dirs.registry.tag_latest(GREETER, "0.1.0");

    assert_eq!(
        refusal(&dirs, GREETER),
        format!(
            "Pane does not download @pane-samples/greeter@0.1.0: its tarball address {elsewhere} \
             is not on the registry {}: Pane downloads a package only from the registry that \
             describes it",
            dirs.registry.url()
        )
    );
}

#[test]
fn a_redirect_is_not_followed() {
    let dirs = Dirs::new();
    let tarball = pack(&greeter_files(&guests(), "0.1.0"));
    let real = dirs.registry.tarball_url(GREETER, "0.1.0");
    let redirecting = real.replacen(
        dirs.registry.url(),
        &format!("{}redirect/", dirs.registry.url()),
        1,
    );
    dirs.registry.publish_with(
        GREETER,
        "0.1.0",
        tarball.clone(),
        Some(json!({ "tarball": redirecting, "integrity": integrity(&tarball) })),
    );
    dirs.registry.tag_latest(GREETER, "0.1.0");

    assert_eq!(
        refusal(&dirs, GREETER),
        format!(
            "The npm registry {} answered 302 for the tarball of @pane-samples/greeter@0.1.0",
            dirs.registry.url()
        )
    );
    // Where it pointed was never asked for.
    assert!(
        !dirs
            .registry
            .requests()
            .iter()
            .any(|path| real.ends_with(path.as_str())),
        "{:?}",
        dirs.registry.requests()
    );
}

/// Publishes the raw tarball `entries` as `evil@1.0.0` and returns why
/// previewing it is refused, checking that nothing was written outside
/// Pane's downloads folder.
fn unsafe_tarball(entries: &[(&str, tar::EntryType, &[u8])]) -> String {
    let dirs = Dirs::new();
    dirs.registry.publish("evil", "1.0.0", pack_raw(entries));
    let error = refusal(&dirs, "evil");
    for escaped in [
        dirs.data.path().join("escaped"),
        dirs.packages_dir().join("escaped"),
        dirs.data.path().join("outside"),
    ] {
        assert!(!escaped.exists(), "{} was written", escaped.display());
    }
    // Nothing of it is left either.
    dirs.wait_for_no_downloads_of_this_attempt();
    error
}

impl Dirs {
    /// The downloads folder holds nothing complete from a refused attempt.
    fn wait_for_no_downloads_of_this_attempt(&self) {
        let downloads = self.packages_dir().join("downloads");
        let left: Vec<_> = fs::read_dir(&downloads)
            .map(|entries| entries.map(|e| e.unwrap().file_name()).collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "left in downloads: {left:?}");
    }
}

#[test]
fn a_tarball_with_a_path_outside_the_package_is_refused() {
    use tar::EntryType::Regular;
    for path in ["package/../../escaped", "../escaped", "/tmp/escaped"] {
        let error = unsafe_tarball(&[
            (
                "package/package.json",
                Regular,
                br#"{"name":"evil","version":"1.0.0"}"#,
            ),
            (path, Regular, b"x"),
        ]);
        assert!(
            error.starts_with(&format!(
                "npm package evil@1.0.0 cannot be unpacked safely: its tarball contains `{path}`"
            )),
            "{error}"
        );
        assert!(
            error.ends_with("; Pane unpacks only paths inside the package"),
            "{error}"
        );
    }
}

#[test]
fn a_tarball_with_a_link_is_refused() {
    use tar::EntryType::{Link, Regular, Symlink};
    for (kind, what) in [(Symlink, "a symbolic link"), (Link, "a hard link")] {
        let error = unsafe_tarball(&[
            (
                "package/package.json",
                Regular,
                br#"{"name":"evil","version":"1.0.0"}"#,
            ),
            ("package/pane.json", kind, b""),
        ]);
        assert_eq!(
            error,
            format!(
                "npm package evil@1.0.0 cannot be unpacked safely: its tarball contains {what}, \
                 `package/pane.json`; Pane unpacks only files and folders"
            )
        );
    }
}

#[test]
fn a_tarball_of_another_package_is_refused() {
    let dirs = Dirs::new();
    // The registry serves the sample's tarball under another name.
    dirs.registry.publish(
        "impostor",
        "0.1.0",
        pack(&greeter_files(&guests(), "0.1.0")),
    );
    assert_eq!(
        refusal(&dirs, "impostor"),
        "The tarball of npm package impostor@0.1.0 holds @pane-samples/greeter@0.1.0, not the \
         package asked for"
    );
}

#[test]
fn an_npm_package_that_is_not_a_pane_extension_is_explained() {
    let dirs = Dirs::new();
    let package = json!({
        "name": "left-pad", "version": "1.3.0", "main": "index.js",
        "scripts": { "postinstall": "node -e \"require('fs').writeFileSync('ran', '')\"" }
    });
    dirs.registry.publish(
        "left-pad",
        "1.3.0",
        pack(&[
            ("package.json", serde_json::to_vec(&package).unwrap()),
            ("index.js", b"module.exports = (s) => s;".to_vec()),
        ]),
    );
    assert_eq!(
        refusal(&dirs, "left-pad"),
        "npm package left-pad@1.3.0 is not a Pane extension: it has no pane.json. Pane installs \
         npm packages published as Pane extensions (a pane.json and the WebAssembly components \
         it names); it does not run other npm packages, which need Node.js and npm"
    );
}

#[test]
fn a_package_published_without_its_built_component_is_explained_and_its_scripts_never_run() {
    let dirs = Dirs::new();
    let package = json!({
        "name": "from-source", "version": "1.0.0",
        "scripts": {
            "prepare": "tsc",
            "postinstall": "node -e \"require('fs').writeFileSync('ran', '')\"",
            "test": "node test.js"
        },
        "devDependencies": { "typescript": "5" }
    });
    let manifest = json!({
        "manifestVersion": 1, "title": "From source", "apiVersion": "0.1",
        "commands": [{ "id": "c", "title": "From source", "component": "dist/command.wasm" }]
    });
    dirs.registry.publish(
        "from-source",
        "1.0.0",
        pack(&[
            ("package.json", serde_json::to_vec(&package).unwrap()),
            ("pane.json", serde_json::to_vec(&manifest).unwrap()),
            ("src/index.ts", b"export {}".to_vec()),
        ]),
    );

    assert_eq!(
        refusal(&dirs, "from-source"),
        "npm package from-source@1.0.0 was published without the built component \
         dist/command.wasm of \"From source\": its author must build it and include it in the \
         package before publishing. Pane does not build npm packages or run their install \
         scripts (its package.json has `postinstall` and `prepare`, which Pane never runs)"
    );
    assert!(no_file_named(dirs.data.path(), "ran"));
}

#[test]
fn install_scripts_and_npm_dependencies_of_a_built_package_are_listed_as_not_used() {
    let dirs = Dirs::new();
    let mut files = greeter_files(&guests(), "0.1.0");
    let mut package: Value = serde_json::from_slice(&files[0].1).unwrap();
    package["scripts"] = json!({
        "postinstall": "node -e \"require('fs').writeFileSync('ran', '')\""
    });
    package["dependencies"] = json!({ "zod": "4" });
    files[0].1 = serde_json::to_vec(&package).unwrap();
    dirs.registry.publish(GREETER, "0.1.0", pack(&files));
    let launcher = dirs.launcher();

    block_on(launcher.preview_npm(GREETER));
    assert!(has(
        &details(&launcher),
        "Not used: its `postinstall` script and its npm dependencies, which its package.json \
         declares; Pane never runs or installs them"
    ));
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Greeter from npm".into())
    );
    assert!(no_file_named(dirs.data.path(), "ran"));
}

/// Whether no file named `name` is anywhere under `dir`.
fn no_file_named(dir: &Path, name: &str) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return true;
    };
    entries.map(|e| e.unwrap()).all(|entry| {
        entry.file_name() != name
            && (!entry.file_type().unwrap().is_dir() || no_file_named(&entry.path(), name))
    })
}

#[test]
fn kept_data_belongs_to_the_npm_name_and_installing_it_again_finds_it() {
    let dirs = Dirs::new();
    dirs.publish_greeter("0.1.0");
    // A setting saved under the package's identity.
    let key = PackageIdentity::npm(GREETER).key();
    fs::create_dir_all(dirs.packages_dir()).unwrap();
    let settings: BTreeMap<&str, Value> = BTreeMap::from([
        ("version", json!(1)),
        (
            "packages",
            json!({ key.clone(): { "style": "\"formal\"" } }),
        ),
    ]);
    fs::write(
        dirs.packages_dir().join("settings.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    let launcher = dirs.launcher();
    block_on(launcher.install_npm(GREETER));

    block_on(launcher.uninstall(&PackageIdentity::npm(GREETER), SavedData::Keep));
    assert!(launcher.packages().is_empty());
    let text = fs::read_to_string(dirs.packages_dir().join("installed.json")).unwrap();
    let registry: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        registry["retained"],
        json!([{ "npm": GREETER, "title": "Greeter from npm" }])
    );

    // Installing the same name again, even at a pinned version, reclaims it.
    block_on(launcher.install_npm(&format!("{GREETER}@0.1.0")));
    let text = fs::read_to_string(dirs.packages_dir().join("installed.json")).unwrap();
    let registry: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(registry.get("retained"), None);
    let text = fs::read_to_string(dirs.packages_dir().join("settings.json")).unwrap();
    let settings: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(settings["packages"][&key]["style"], "\"formal\"");
}

#[test]
fn an_npm_package_has_no_reload_or_develop_rows() {
    let dirs = Dirs::new();
    dirs.publish_greeter("0.1.0");
    let launcher = dirs.launcher();
    block_on(launcher.install_npm(GREETER));
    launcher.back();
    select_title(&launcher, "Manage extensions…");
    block_on(launcher.activate_selected());

    let titles = titles(&launcher);
    assert!(
        titles.contains(&"Greeter from npm".to_owned()),
        "{titles:?}"
    );
    assert!(
        titles.contains(&"Uninstall Greeter from npm".to_owned()),
        "{titles:?}"
    );
    assert!(
        !titles
            .iter()
            .any(|t| t.starts_with("Reload ") || t.starts_with("Develop ")),
        "{titles:?}"
    );
    let row = &launcher.view().rows[titles
        .iter()
        .position(|t| t == "Greeter from npm")
        .expect("the package's row")];
    assert_eq!(
        row.subtitle.as_deref(),
        Some("Enabled · npm package @pane-samples/greeter")
    );
}
