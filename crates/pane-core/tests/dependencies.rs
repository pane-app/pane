//! Installing a local package with the other packages it declares under
//! `dependencies`, through the launcher's public interface: the preview
//! lists the required and optional ones before anything is installed,
//! installing adds the missing required ones with it and its code then calls
//! them by dependency id, while optional, disabled and already installed
//! (pinned) dependencies are left as they are, and anything that stops a
//! required one leaves nothing installed. A disabled or uninstalled
//! required dependency makes the dependent's command wait for it, coming
//! back by itself once it returns (#152). The operations samples
//! (`guests/sample-operations*`) and the operations fixture
//! (`guests/fixtures/operations`) from `cargo xtask guests` serve as
//! packages.

use std::fs;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use pane_core::{Launcher, PackageIdentity, Runtime, SavedData, Screen, Status, Unavailable};
use tempfile::TempDir;

#[path = "support/platforms.rs"]
mod platforms;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guest_file as guest;
use rows::{select_title, titles};

const RUST: &str = "sample-operations";
const TYPESCRIPT: &str = "sample-operations-ts";

struct Dirs {
    sources: TempDir,
    data: TempDir,
    runtime: Runtime,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            runtime: Runtime::start().unwrap(),
        }
    }

    fn folder(&self, name: &str) -> PathBuf {
        self.sources.path().join(name)
    }

    fn launcher(&self) -> Launcher {
        Launcher::with_packages(
            Ok(self.runtime.clone()),
            vec![],
            self.data.path().join("extensions"),
        )
    }

    /// Where folder `name` is (it need not exist), spelled as Pane spells
    /// an identity (macOS reports `/private/var/...` for `/var/...`).
    fn resolved(&self, name: &str) -> PathBuf {
        let sources = PackageIdentity::local(self.sources.path()).unwrap();
        sources.local_folder().unwrap().join(name)
    }

    fn identity(&self, name: &str) -> PackageIdentity {
        PackageIdentity::local(&self.folder(name)).unwrap()
    }

    /// Copies the assembled sample package `package` into source folder
    /// `package`.
    fn sample(&self, package: &str) -> PathBuf {
        self.sample_in(package, package)
    }

    /// Copies the assembled sample package `package` into source folder
    /// `name`.
    fn sample_in(&self, package: &str, name: &str) -> PathBuf {
        let assembled = guest("packages").join(package);
        let folder = self.folder(name);
        fs::create_dir_all(&folder).unwrap();
        for entry in fs::read_dir(assembled).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
        }
        folder
    }

    /// Writes package "Caller" in source folder `caller`: the JavaScript
    /// operations sample's command and component, declaring
    /// `dependencies` (JSON array contents).
    fn caller(&self, dependencies: &str) -> PathBuf {
        self.caller_in("caller", dependencies)
    }

    /// Writes package "Caller" in source folder `name`, as
    /// [`Dirs::caller`] does.
    fn caller_in(&self, name: &str, dependencies: &str) -> PathBuf {
        let folder = self.folder(name);
        fs::create_dir_all(&folder).unwrap();
        fs::copy(
            guest("sample_operations_js.wasm"),
            folder.join("caller.wasm"),
        )
        .unwrap();
        let manifest = format!(
            r#"{{
                "manifestVersion": 1,
                "title": "Caller",
                "apiVersion": "0.1",
                "commands": [
                    {{ "id": "call", "title": "Call from JavaScript", "component": "caller.wasm" }}
                ],
                "operations": [{{ "id": "greet", "version": 1, "component": "caller.wasm" }}],
                "dependencies": [{dependencies}]
            }}"#
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        folder
    }

    /// Writes a fixture package in source folder `name`, titled
    /// "Package <name>", publishing `echo` at `version` and declaring
    /// `dependencies` (JSON array contents).
    fn fixture(&self, name: &str, version: u32, dependencies: &str) -> PathBuf {
        let folder = self.folder(name);
        fs::create_dir_all(&folder).unwrap();
        fs::copy(
            guest("operations_fixture.wasm"),
            folder.join("fixture.wasm"),
        )
        .unwrap();
        let manifest = format!(
            r#"{{
                "manifestVersion": 1,
                "title": "Package {name}",
                "apiVersion": "0.1",
                "operations": [
                    {{ "id": "echo", "version": {version}, "component": "fixture.wasm" }}
                ],
                "dependencies": [{dependencies}]
            }}"#
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        folder
    }
}

/// A dependency on the package in sibling folder `folder` calling `echo`
/// at `version`.
fn needs_echo(id: &str, folder: &str, version: u32) -> String {
    format!(
        r#"{{ "id": "{id}", "source": "local:../{folder}",
             "operations": [{{ "id": "echo", "version": {version} }}] }}"#
    )
}

/// The Rust operations sample's `greet`, as `greeter`, required.
const GREETER: &str = r#"{ "id": "greeter", "source": "local:../sample-operations",
    "operations": [{ "id": "greet", "version": 1 }] }"#;

/// The TypeScript operations sample's `greet`, as `helper`, optional.
const HELPER: &str = r#"{ "id": "helper", "source": "local:../sample-operations-ts",
    "optional": true, "operations": [{ "id": "greet", "version": 1 }] }"#;

/// The titles of the installed packages, in the order they were installed.
fn installed(launcher: &Launcher) -> Vec<String> {
    launcher.packages().iter().map(|p| p.title()).collect()
}

/// Calls `greet` from Caller's command with `source` and the name "Ada".
fn greet(launcher: &Launcher, source: &str) -> Status {
    launcher.back();
    launcher.back();
    launcher.back();
    select_title(launcher, "Call from JavaScript");
    block_on(launcher.activate_selected());
    select_title(launcher, "Greet through another extension");
    block_on(launcher.activate_selected());
    assert!(launcher.view().form().is_some(), "no form");
    launcher.set_field_value("source", source);
    launcher.set_field_value("name", "Ada");
    launcher.set_field_value("times", "once");
    block_on(launcher.submit_form());
    launcher.view().status
}

fn result(text: &str) -> Status {
    Status::Result(text.into())
}

/// The error the form shows: the call's error kind and message.
fn error(text: &str) -> Status {
    Status::Error(text.into())
}

fn details(launcher: &Launcher) -> Vec<String> {
    launcher.view().details().to_vec()
}

#[test]
fn the_preview_lists_required_and_optional_dependencies_and_installs_nothing() {
    let dirs = Dirs::new();
    dirs.sample(RUST);
    dirs.sample(TYPESCRIPT);
    let caller = dirs.caller(&format!("{GREETER}, {HELPER}"));
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&caller));

    let view = launcher.view();
    assert_eq!(view.title, "Caller");
    let details = details(&launcher);
    // Sources as declared: the preview's Source line gives the folder they
    // are relative to.
    assert!(
        details.contains(
            &"Requires: Rust operations sample, installed with it from local:../sample-operations"
                .into()
        ),
        "{details:#?}"
    );
    assert!(
        details.contains(
            &"Optional: `helper` from local:../sample-operations-ts, not installed: Pane does \
              not install it; install it yourself to use it"
                .into()
        ),
        "{details:#?}"
    );
    assert_eq!(titles(&launcher), ["Install"]);
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some(
            "Copy the package into Pane and add its commands, and install Rust operations \
             sample, which it requires"
        )
    );
    // Nothing is installed by looking.
    assert!(launcher.packages().is_empty());
}

#[test]
fn installing_adds_the_missing_required_dependency_whose_operation_the_code_calls_by_id() {
    let dirs = Dirs::new();
    dirs.sample(RUST);
    dirs.sample(TYPESCRIPT);
    let caller = dirs.caller(&format!("{GREETER}, {HELPER}"));
    let launcher = dirs.launcher();
    block_on(launcher.preview_package(&caller));

    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        result("Installed Caller with Rust operations sample, which it requires")
    );
    // The dependency first; the optional one is not installed.
    assert_eq!(installed(&launcher), ["Rust operations sample", "Caller"]);
    // The JavaScript code calls the Rust package by the id its manifest
    // declares.
    assert_eq!(greet(&launcher, "greeter"), result("Hello, Ada, from Rust"));
    assert_eq!(
        greet(&launcher, "helper"),
        error(&format!(
            "not-found: Caller's optional dependency `helper` from {} is not installed; \
             install it to use it",
            dirs.identity(TYPESCRIPT)
        ))
    );
    assert_eq!(
        greet(&launcher, "nobody"),
        error(
            "not-found: Caller declares no dependency `nobody` in its pane.json, and `nobody` \
             is not a package identity; it declares `greeter` and `helper`"
        )
    );

    // The optional one, installed on its own, is used.
    block_on(launcher.install_package(&dirs.folder(TYPESCRIPT)));
    assert_eq!(
        launcher.view().status,
        result("Installed TypeScript operations sample")
    );
    assert_eq!(
        greet(&launcher, "helper"),
        result("Hello, Ada, from TypeScript")
    );

    // Where each dependency resolved is kept: after a restart, even with the
    // source folders gone, the calls still reach them.
    drop(launcher);
    fs::remove_dir_all(dirs.folder(RUST)).unwrap();
    fs::remove_dir_all(dirs.folder("caller")).unwrap();
    let launcher = dirs.launcher();
    assert_eq!(greet(&launcher, "greeter"), result("Hello, Ada, from Rust"));
}

#[test]
fn an_installed_required_dependency_is_used_as_it_is() {
    let dirs = Dirs::new();
    let rust = dirs.sample(RUST);
    let caller = dirs.caller(GREETER);
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&rust));

    block_on(launcher.preview_package(&caller));
    assert!(
        details(&launcher).contains(&"Requires: Rust operations sample, already installed".into()),
        "{:#?}",
        details(&launcher)
    );
    assert_eq!(
        launcher.view().rows[0].subtitle.as_deref(),
        Some("Copy the package into Pane and add its commands")
    );
    block_on(launcher.activate_selected());

    assert_eq!(launcher.view().status, result("Installed Caller"));
    assert_eq!(installed(&launcher), ["Rust operations sample", "Caller"]);
    assert_eq!(greet(&launcher, "greeter"), result("Hello, Ada, from Rust"));
}

#[test]
fn a_disabled_required_dependency_is_not_enabled_again() {
    let dirs = Dirs::new();
    let rust = dirs.sample(RUST);
    let caller = dirs.caller(GREETER);
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&rust));
    block_on(launcher.set_enabled(&dirs.identity(RUST), false));

    block_on(launcher.preview_package(&caller));
    assert!(
        details(&launcher).contains(
            &"Requires: Rust operations sample, which you disabled: it stays disabled, and \
              Caller cannot use it until you enable it in Settings"
                .into()
        ),
        "{:#?}",
        details(&launcher)
    );
    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        result(
            "Installed Caller; Rust operations sample stays disabled: enable it in Settings \
             for Caller to use it"
        )
    );
    let rust = launcher
        .packages()
        .into_iter()
        .find(|p| p.identity == dirs.identity(RUST))
        .unwrap();
    assert!(!rust.enabled);
    // The dependent now waits for the disabled dependency, rather than its
    // calls answering `disabled` (#152): its command stays listed, saying
    // what it needs, and running it shows the reason instead of calling
    // the guest.
    launcher.back();
    launcher.back();
    launcher.back();
    let command = launcher
        .view()
        .rows
        .iter()
        .find(|row| row.title == "Call from JavaScript")
        .unwrap()
        .clone();
    assert_eq!(
        command.unavailable,
        Some(Unavailable::Waiting(
            "Needs Rust operations sample, which is disabled".into()
        ))
    );
    select_title(&launcher, "Call from JavaScript");
    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::WaitingDetails { .. }),
        "{view:?}"
    );
    assert_eq!(view.title, "Why Call from JavaScript cannot run");
    // Enabling the dependency again brings it back by itself.
    block_on(launcher.set_enabled(&dirs.identity(RUST), true));
    launcher.show_root_search();
    let command = launcher
        .view()
        .rows
        .iter()
        .find(|row| row.title == "Call from JavaScript")
        .unwrap()
        .clone();
    assert_eq!(command.unavailable, None);
    assert_eq!(greet(&launcher, "greeter"), result("Hello, Ada, from Rust"));
}

#[test]
fn an_installed_copy_that_is_not_compatible_is_kept_and_nothing_is_installed() {
    let dirs = Dirs::new();
    let rust = dirs.sample(RUST);
    let caller = dirs.caller(
        r#"{ "id": "greeter", "source": "local:../sample-operations",
             "operations": [{ "id": "greet", "version": 2 }] }"#,
    );
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&rust));

    let expected = "Nothing was installed: Caller requires `greet` version 2 from Rust \
                    operations sample, which publishes version 1; Pane does not replace the \
                    installed copy of Rust operations sample while installing another \
                    extension: update it from its folder if a newer copy publishes it";
    block_on(launcher.preview_package(&caller));
    let view = launcher.view();
    assert_eq!(view.title, "Cannot install Caller");
    assert!(view.rows.is_empty(), "nothing to choose: {:?}", view.rows);
    assert_eq!(view.status, Status::Error(expected.into()));

    // Installing without the preview is refused the same way.
    block_on(launcher.install_package(&caller));
    assert_eq!(launcher.view().status, Status::Error(expected.into()));
    assert_eq!(installed(&launcher), ["Rust operations sample"]);
}

#[test]
fn conflicting_versions_of_one_source_are_explained_and_nothing_is_installed() {
    let dirs = Dirs::new();
    // a needs b's echo 1 and c; c needs b's echo 2. There is one b.
    dirs.fixture("b", 1, "");
    dirs.fixture("c", 1, &needs_echo("b", "b", 2));
    let a = dirs.fixture(
        "a",
        1,
        &format!("{}, {}", needs_echo("b", "b", 1), needs_echo("c", "c", 1)),
    );
    let launcher = dirs.launcher();

    block_on(launcher.install_package(&a));

    assert_eq!(
        launcher.view().status,
        Status::Error(
            "Nothing was installed: Package a and Package c need different versions of `echo` \
             from Package b (1 and 2); Pane installs one copy of each source, so they conflict"
                .into()
        )
    );
    assert!(launcher.packages().is_empty());
}

/// Writes what a case needs and returns the dependency to declare.
type Setup = Box<dyn Fn(&Dirs) -> String>;

#[test]
fn a_required_dependency_that_cannot_be_installed_leaves_nothing_installed() {
    let [other, _] = platforms::other_systems();
    let cases: [(&str, Setup); 5] = [
        (
            "a missing folder",
            Box::new(|_| needs_echo("b", "missing", 1)),
        ),
        (
            "a source-only package",
            Box::new(|dirs| {
                dirs.fixture("b", 1, "");
                fs::remove_file(dirs.folder("b").join("fixture.wasm")).unwrap();
                needs_echo("b", "b", 1)
            }),
        ),
        (
            "a package for another system",
            Box::new(move |dirs| {
                let folder = dirs.fixture("b", 1, "");
                let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
                let manifest = manifest.replacen(
                    r#""apiVersion": "0.1","#,
                    &format!(r#""apiVersion": "0.1", "platforms": ["{}"],"#, other.id()),
                    1,
                );
                fs::write(folder.join("pane.json"), manifest).unwrap();
                needs_echo("b", "b", 1)
            }),
        ),
        (
            "an operation it does not publish",
            Box::new(|dirs| {
                dirs.fixture("b", 1, "");
                r#"{ "id": "b", "source": "local:../b",
                     "operations": [{ "id": "secret", "version": 1 }] }"#
                    .into()
            }),
        ),
        ("itself", Box::new(|_| needs_echo("b", "a", 1))),
    ];
    for (case, dependency) in cases {
        let dirs = Dirs::new();
        let dependency = dependency(&dirs);
        let a = dirs.fixture("a", 1, &dependency);
        let launcher = dirs.launcher();

        block_on(launcher.preview_package(&a));
        assert_eq!(launcher.view().title, "Cannot install Package a", "{case}");
        let expected = match case {
            "a missing folder" => format!(
                "Package a requires `b` from {}, which cannot be installed: Cannot open",
                dirs.resolved("missing").display()
            ),
            "a source-only package" => format!(
                "Package a requires `b` from {}, which cannot be installed: Not ready to run",
                dirs.identity("b").local_folder().unwrap().display()
            ),
            "a package for another system" => format!(
                "Package a requires `b` from {}, which cannot be installed: {}",
                dirs.identity("b").local_folder().unwrap().display(),
                platforms::only("this package", platforms::name(other))
            ),
            "an operation it does not publish" => {
                "Package a requires Package b to publish `secret`, which it does not publish".into()
            }
            _ => "Package a names itself as its dependency `b`".into(),
        };
        block_on(launcher.install_package(&a));
        let status = launcher.view().status;
        assert!(
            matches!(&status, Status::Error(text)
                if text.starts_with(&format!("Nothing was installed: {expected}"))),
            "{case}: {status:?}"
        );
        assert!(launcher.packages().is_empty(), "{case}");
    }
}

#[test]
fn packages_requiring_each_other_are_installed_together_once() {
    let dirs = Dirs::new();
    dirs.fixture("b", 1, &needs_echo("a", "a", 1));
    let a = dirs.fixture("a", 1, &needs_echo("b", "b", 1));
    let launcher = dirs.launcher();

    block_on(launcher.install_package(&a));

    assert_eq!(
        launcher.view().status,
        result("Installed Package a with Package b, which it requires")
    );
    assert_eq!(installed(&launcher), ["Package b", "Package a"]);
}

#[test]
fn required_dependencies_of_dependencies_are_installed_first() {
    let dirs = Dirs::new();
    dirs.fixture("c", 1, "");
    dirs.fixture("b", 1, &needs_echo("c", "c", 1));
    let a = dirs.fixture("a", 1, &needs_echo("b", "b", 1));
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&a));
    assert!(
        details(&launcher).contains(
            &"Requires (for Package b): Package c, installed with it from local:../c".into()
        ),
        "{:#?}",
        details(&launcher)
    );
    assert_eq!(
        launcher.view().rows[0].subtitle.as_deref(),
        Some(
            "Copy the package into Pane and add its commands, and install the 2 extensions it \
             requires"
        )
    );
    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        result("Installed Package a with Package c and Package b, which it requires")
    );
    assert_eq!(
        installed(&launcher),
        ["Package c", "Package b", "Package a"]
    );
}

#[test]
fn a_dependency_needed_only_on_other_systems_is_not_installed() {
    let dirs = Dirs::new();
    let [other, _] = platforms::other_systems();
    dirs.fixture("b", 1, "");
    let a = dirs.fixture(
        "a",
        1,
        &format!(
            r#"{{ "id": "b", "source": "local:../b", "platforms": ["{}"],
                 "operations": [{{ "id": "echo", "version": 1 }}] }}"#,
            other.id()
        ),
    );
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&a));
    assert!(
        details(&launcher).contains(&format!(
            "Not needed on this system: `b` from local:../b (only on {})",
            platforms::name(other)
        )),
        "{:#?}",
        details(&launcher)
    );
    block_on(launcher.activate_selected());

    assert_eq!(launcher.view().status, result("Installed Package a"));
    assert_eq!(installed(&launcher), ["Package a"]);
}

#[test]
fn an_update_adding_a_required_dependency_installs_it() {
    let dirs = Dirs::new();
    dirs.sample(RUST);
    let caller = dirs.caller("");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&caller));
    assert_eq!(installed(&launcher), ["Caller"]);
    assert_eq!(
        greet(&launcher, "greeter"),
        error(
            "not-found: Caller declares no dependency `greeter` in its pane.json, and \
             `greeter` is not a package identity; it declares none"
        )
    );

    dirs.caller(GREETER);
    block_on(launcher.preview_package(&caller));
    assert_eq!(titles(&launcher), ["Update"]);
    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        result("Updated Caller with Rust operations sample, which it requires")
    );
    assert_eq!(installed(&launcher), ["Caller", "Rust operations sample"]);
    assert_eq!(greet(&launcher, "greeter"), result("Hello, Ada, from Rust"));
}

#[test]
fn a_required_dependency_uninstalled_later_leaves_its_dependent_waiting() {
    let dirs = Dirs::new();
    dirs.sample(RUST);
    let caller = dirs.caller(GREETER);
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&caller));

    block_on(launcher.uninstall(&dirs.identity(RUST), SavedData::Delete));

    // The dependent waits for it instead of failing (#152): its command
    // stays listed, naming the uninstalled dependency, and running it
    // shows the reason instead of calling the guest. Its calls by
    // dependency id are not made at all; one through an optional
    // dependency still explains the call.
    launcher.back();
    launcher.back();
    launcher.back();
    let command = launcher
        .view()
        .rows
        .iter()
        .find(|row| row.title == "Call from JavaScript")
        .unwrap()
        .clone();
    assert_eq!(
        command.unavailable,
        Some(Unavailable::Waiting(format!(
            "Needs {}, which is not installed",
            dirs.identity(RUST)
        )))
    );
}

#[test]
fn an_invalid_dependency_declaration_is_explained() {
    for (declaration, reason) in [
        (
            r#"{ "id": "Greeter", "source": "local:../b", "operations": [{ "id": "echo", "version": 1 }] }"#,
            "dependency id `Greeter` must be lowercase letters, digits and `-`",
        ),
        (
            r#"{ "id": "b", "source": "local:../b", "operations": [] }"#,
            "dependency `b` lists no `operations`; name those the package calls",
        ),
        (
            r#"{ "id": "b", "source": "local:../b", "operations": [{ "id": "echo", "version": 0 }] }"#,
            "every operation of dependency `b` needs an `id` and a `version` from 1",
        ),
    ] {
        let dirs = Dirs::new();
        let a = dirs.fixture("a", 1, declaration);
        let launcher = dirs.launcher();
        block_on(launcher.preview_package(&a));
        assert_eq!(
            launcher.view().status,
            Status::Error(format!("Invalid pane.json: {reason}"))
        );
    }
}

#[test]
fn a_source_that_is_not_a_local_folder_npm_or_git_is_not_supported_yet() {
    // (No Git source here: it would be fetched, from the network.)
    for source in ["../b", "svn:example.org/a/b", "local:"] {
        let dirs = Dirs::new();
        let a = dirs.fixture(
            "a",
            1,
            &format!(
                r#"{{ "id": "b", "source": "{source}", "operations": [{{ "id": "echo", "version": 1 }}] }}"#
            ),
        );
        let launcher = dirs.launcher();
        block_on(launcher.preview_package(&a));
        let status = launcher.view().status;
        assert!(
            matches!(&status, Status::Error(text) if text.starts_with(&format!(
                "Invalid pane.json: the source `{source}` of dependency `b` must be `local:` \
                 followed by a folder path"
            ))),
            "{source}: {status:?}"
        );
    }
}

#[test]
fn a_windows_style_source_is_refused_on_every_system() {
    // As JSON text: `\\` is one backslash.
    for source in [
        r"local:..\\b",
        "local:C:/extensions/b",
        r"local:C:\\extensions\\b",
        "local://server/share/b",
        r"local:\\\\server\\share\\b",
    ] {
        let dirs = Dirs::new();
        let a = dirs.fixture(
            "a",
            1,
            &format!(
                r#"{{ "id": "b", "source": "{source}", "operations": [{{ "id": "echo", "version": 1 }}] }}"#
            ),
        );
        let launcher = dirs.launcher();
        block_on(launcher.preview_package(&a));
        let status = launcher.view().status;
        assert!(
            matches!(&status, Status::Error(text) if text.starts_with("Invalid pane.json: the source")
            && text.ends_with(
                "of dependency `b` must separate folders with `/`, without a drive letter, \
                 `\\` or a `//server` share, so that every system reads it alike (such as \
                 `local:../greeter`)"
            )),
            "{source}: {status:?}"
        );
    }
}

#[test]
fn dot_dot_and_dot_resolve_like_the_folder_they_name() {
    let dirs = Dirs::new();
    dirs.fixture("b", 1, "");
    fs::create_dir_all(dirs.folder("group/a")).unwrap();
    // a is in group/a; b is two levels up from there.
    let a = dirs.folder("group/a");
    fs::copy(guest("operations_fixture.wasm"), a.join("fixture.wasm")).unwrap();
    fs::write(
        a.join("pane.json"),
        r#"{ "manifestVersion": 1, "title": "Package a", "apiVersion": "0.1",
             "operations": [{ "id": "echo", "version": 1, "component": "fixture.wasm" }],
             "dependencies": [{ "id": "b", "source": "local:./../x/../../b",
                                "operations": [{ "id": "echo", "version": 1 }] }] }"#,
    )
    .unwrap();
    let launcher = dirs.launcher();

    block_on(launcher.install_package(&a));

    assert_eq!(
        launcher.view().status,
        result("Installed Package a with Package b, which it requires")
    );
    let identities: Vec<PackageIdentity> = launcher
        .packages()
        .into_iter()
        .map(|p| p.identity)
        .collect();
    assert_eq!(identities, [dirs.identity("b"), dirs.identity("group/a")]);
}

#[test]
fn more_than_sixteen_packages_to_install_with_it_are_refused() {
    let dirs = Dirs::new();
    // a requires p1 to p17, none installed.
    let mut declarations = Vec::new();
    for n in 1..=17 {
        dirs.fixture(&format!("p{n}"), 1, "");
        declarations.push(needs_echo(&format!("p{n}"), &format!("p{n}"), 1));
    }
    let a = dirs.fixture("a", 1, &declarations.join(", "));
    let launcher = dirs.launcher();

    block_on(launcher.install_package(&a));

    assert_eq!(
        launcher.view().status,
        Status::Error(
            "Nothing was installed: Installing Package a would install more than 16 other \
             extensions with it; Pane installs at most 16 at once"
                .into()
        )
    );
    assert!(launcher.packages().is_empty());

    // Sixteen are installed with it.
    let a = dirs.fixture("a", 1, &declarations[..16].join(", "));
    block_on(launcher.install_package(&a));
    assert_eq!(launcher.packages().len(), 17);
}

/// From root search, opens the command titled `command` and runs its item
/// titled `item`, returning the outcome: its toast, or the status line.
fn run(launcher: &Launcher, command: &str, item: &str) -> Status {
    launcher.back();
    launcher.back();
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    shown(launcher)
}

#[test]
fn the_dependencies_sample_installs_its_required_greeter_and_uses_the_optional_one_once_installed()
{
    let dirs = Dirs::new();
    let sample = dirs.sample("sample-dependencies");
    dirs.sample("sample-operations-js");
    let rust = dirs.sample(RUST);
    let launcher = dirs.launcher();

    block_on(launcher.install_package(&sample));

    assert_eq!(
        launcher.view().status,
        result(
            "Installed Dependencies sample with JavaScript operations sample, which it requires"
        )
    );
    let command = "Greet through dependencies";
    assert_eq!(
        run(&launcher, command, "Greet through the required greeter"),
        result("Hello, Pane, from JavaScript")
    );
    assert_eq!(
        run(&launcher, command, "Greet through the optional greeter"),
        result(
            "The optional Rust greeter is not installed; install the Rust operations sample to \
             use it"
        )
    );

    block_on(launcher.install_package(&rust));
    assert_eq!(
        run(&launcher, command, "Greet through the optional greeter"),
        result("Hello, Pane, from Rust")
    );
}

/// Whether the installed package from source folder `name` is enabled;
/// `None` if it is not installed.
fn enabled(dirs: &Dirs, launcher: &Launcher, name: &str) -> Option<bool> {
    launcher
        .packages()
        .into_iter()
        .find(|p| p.identity == dirs.identity(name))
        .map(|p| p.enabled)
}

#[test]
fn a_required_dependency_cannot_be_changed_while_an_install_relies_on_it() {
    let dirs = Dirs::new();
    let rust = dirs.sample(RUST);
    let caller = dirs.caller(GREETER);
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&rust));
    block_on(launcher.preview_package(&caller));

    // Choosing Install claims what the preview relies on at once.
    let installing = launcher.activate_selected();
    let busy = Status::Error("Rust operations sample is part of an install in progress".into());
    block_on(launcher.uninstall(&dirs.identity(RUST), SavedData::Delete));
    assert_eq!(launcher.view().status, busy);
    block_on(launcher.reload(&dirs.identity(RUST)));
    assert_eq!(launcher.view().status, busy);
    block_on(launcher.set_enabled(&dirs.identity(RUST), false));
    assert_eq!(enabled(&dirs, &launcher, RUST), Some(true));

    block_on(installing);
    assert_eq!(launcher.view().status, result("Installed Caller"));
    assert_eq!(greet(&launcher, "greeter"), result("Hello, Ada, from Rust"));

    // Once the install is over, it can be uninstalled.
    block_on(launcher.uninstall(&dirs.identity(RUST), SavedData::Delete));
    assert_eq!(enabled(&dirs, &launcher, RUST), None);
}

#[test]
fn data_kept_for_a_dependency_the_install_adds_cannot_be_deleted_meanwhile() {
    let dirs = Dirs::new();
    let rust = dirs.sample(RUST);
    let caller = dirs.caller(GREETER);
    // The Rust sample kept a setting, then was uninstalled keeping it.
    fs::create_dir_all(dirs.data.path().join("extensions")).unwrap();
    fs::write(
        dirs.data.path().join("extensions/settings.json"),
        serde_json::json!({
            "version": 1,
            "packages": { dirs.identity(RUST).key(): { "style": "formal" } }
        })
        .to_string(),
    )
    .unwrap();
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&rust));
    block_on(launcher.uninstall(&dirs.identity(RUST), SavedData::Keep));
    assert_eq!(launcher.retained_data().len(), 1);
    let busy = Status::Error("Rust operations sample is part of an install in progress".into());

    block_on(launcher.preview_package(&caller));
    let installing = launcher.activate_selected();
    block_on(launcher.delete_retained_data(&dirs.identity(RUST)));
    assert_eq!(launcher.view().status, busy);
    block_on(installing);
    assert_eq!(
        launcher.view().status,
        result("Installed Caller with Rust operations sample, which it requires")
    );
    assert!(launcher.retained_data().is_empty());
}

#[test]
fn an_uninstall_begun_after_the_preview_stops_the_install_and_shows_the_new_plan() {
    let dirs = Dirs::new();
    let rust = dirs.sample(RUST);
    let caller = dirs.caller(GREETER);
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&rust));
    block_on(launcher.preview_package(&caller));
    assert!(
        details(&launcher).contains(&"Requires: Rust operations sample, already installed".into())
    );

    let uninstalling = launcher.uninstall(&dirs.identity(RUST), SavedData::Delete);
    block_on(launcher.activate_selected());

    // Nothing is installed; the preview shows what installing needs now.
    let view = launcher.view();
    assert_eq!(
        view.status,
        Status::Error(
            "What installing Caller needs changed since it was shown; check it again and \
             choose Install once more"
                .into()
        )
    );
    assert!(
        details(&launcher).contains(
            &"Requires: Rust operations sample, installed with it from local:../sample-operations"
                .into()
        ),
        "{:#?}",
        details(&launcher)
    );
    block_on(uninstalling);
    assert!(launcher.packages().is_empty());

    // Choosing Install on the new plan installs both.
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        result("Installed Caller with Rust operations sample, which it requires")
    );
}

#[test]
fn installing_after_the_preview_changed_shows_the_new_plan_instead() {
    let dirs = Dirs::new();
    let rust = dirs.sample(RUST);
    let caller = dirs.caller(GREETER);
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&caller));
    let manifest = fs::read_to_string(rust.join("pane.json")).unwrap();
    fs::write(
        rust.join("pane.json"),
        manifest.replace("Rust operations sample", "Renamed greeter"),
    )
    .unwrap();
    block_on(launcher.activate_selected());

    // Its folder changed after the preview: nothing is installed, and the
    // preview shows the new copy.
    assert_eq!(
        launcher.view().status,
        Status::Error(
            "What installing Caller needs changed since it was shown; check it again and \
             choose Install once more"
                .into()
        )
    );
    assert!(
        details(&launcher).contains(
            &"Requires: Renamed greeter, installed with it from local:../sample-operations".into()
        ),
        "{:#?}",
        details(&launcher)
    );
    assert!(launcher.packages().is_empty());
    block_on(launcher.activate_selected());
    assert_eq!(installed(&launcher), ["Renamed greeter", "Caller"]);
}

#[test]
fn a_paused_required_dependency_is_shown_as_paused_and_stays_paused() {
    let dirs = Dirs::new();
    let rust = dirs.sample(RUST);
    let caller = dirs.caller(GREETER);
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&rust));
    drop(launcher);
    // Pane paused it after it crashed, as recorded before a restart.
    let registry = dirs.data.path().join("extensions/installed.json");
    let mut record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&registry).unwrap()).unwrap();
    let dir = record["packages"][0]["dir"].clone();
    record["packages"][0]["paused"] = serde_json::json!({
        "after": "crashes", "why": "it crashed", "version": "0.1.0", "code": dir
    });
    fs::write(&registry, record.to_string()).unwrap();
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&caller));
    assert!(
        details(&launcher).contains(
            &"Requires: Rust operations sample, installed but it is paused after an error; \
              retry it in Settings"
                .into()
        ),
        "{:#?}",
        details(&launcher)
    );
    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        result(
            "Installed Caller; Rust operations sample stays paused after an error: retry it in \
             Settings for Caller to use it"
        )
    );
}

#[test]
fn a_call_by_dependency_id_reaches_only_the_operations_declared_for_it() {
    let dirs = Dirs::new();
    dirs.sample(RUST);
    let caller = dirs.caller(GREETER);
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&caller));

    // The sample's "wait" item calls `wait` version 1, which Caller does not
    // declare for `greeter`.
    launcher.back();
    launcher.back();
    select_title(&launcher, "Call from JavaScript");
    block_on(launcher.activate_selected());
    select_title(&launcher, "Wait in another extension");
    block_on(launcher.activate_selected());
    launcher.set_field_value("source", "greeter");
    block_on(launcher.submit_form());

    assert_eq!(
        launcher.view().status,
        error(
            "refused: Caller declares that it calls `greet` version 1 through its dependency \
             `greeter`, not `wait` version 1; declare it in its pane.json to call it"
        )
    );
    // The operation it declares is reached.
    assert_eq!(greet(&launcher, "greeter"), result("Hello, Ada, from Rust"));
}

/// Makes `link` a symbolic link to the folder `target`; `None` where the
/// system does not allow it (Windows without the privilege).
fn symlink_dir(target: &Path, link: &Path) -> Option<()> {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(target, link);
    made.ok()
}

#[test]
fn a_dependency_folder_made_a_link_after_installing_is_still_reached() {
    let dirs = Dirs::new();
    // helper's folder does not exist when Caller is installed.
    let caller = dirs.caller(
        r#"{ "id": "helper", "source": "local:../ts-link", "optional": true,
             "operations": [{ "id": "greet", "version": 1 }] }"#,
    );
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&caller));

    // Then it becomes a link to the TypeScript sample, installed from where
    // the link points.
    let typescript = dirs.sample(TYPESCRIPT);
    if symlink_dir(&typescript, &dirs.folder("ts-link")).is_none() {
        eprintln!("skipped: this system does not allow a symbolic link");
        return;
    }
    block_on(launcher.install_package(&typescript));

    assert_eq!(
        greet(&launcher, "helper"),
        result("Hello, Ada, from TypeScript")
    );
}

#[test]
fn a_package_installed_through_a_link_resolves_sources_from_the_folder_it_points_to() {
    let dirs = Dirs::new();
    // real/caller and real/sample-operations; the link is beside real, where
    // no sample-operations is.
    dirs.sample_in(RUST, "real/sample-operations");
    dirs.caller_in("real/caller", GREETER);
    if symlink_dir(&dirs.folder("real/caller"), &dirs.folder("caller-link")).is_none() {
        eprintln!("skipped: this system does not allow a symbolic link");
        return;
    }
    let launcher = dirs.launcher();

    block_on(launcher.install_package(&dirs.folder("caller-link")));

    assert_eq!(
        launcher.view().status,
        result("Installed Caller with Rust operations sample, which it requires")
    );
    let identities: Vec<PackageIdentity> = launcher
        .packages()
        .into_iter()
        .map(|p| p.identity)
        .collect();
    assert_eq!(
        identities,
        [
            dirs.identity("real/sample-operations"),
            dirs.identity("real/caller")
        ]
    );
    assert_eq!(greet(&launcher, "greeter"), result("Hello, Ada, from Rust"));
}
