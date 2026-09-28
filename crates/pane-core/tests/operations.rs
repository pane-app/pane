//! Calling an operation another installed package publishes, through the
//! launcher's public interface: a command of one package calls an operation
//! of another, across Rust, JavaScript and TypeScript, and shows its result or
//! why the call failed. The operations samples (`guests/sample-operations*`)
//! show what authors write; the operations fixture (`guests/fixtures/
//! operations`) drives the failures, cycles and limits. Real guests from
//! `cargo xtask guests`.

use std::fs;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

/// Where `cargo xtask guests` assembles the sample packages.
fn assembled(package: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(package);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// Copies the assembled package `package` into `folder`.
fn copy_package(package: &str, folder: &Path) -> PathBuf {
    fs::create_dir_all(folder).unwrap();
    for entry in fs::read_dir(assembled(package)).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
    }
    folder.to_path_buf()
}

/// Source folders side by side, as the samples expect (`local:../<name>`),
/// and Pane's data folder.
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

    /// Copies the assembled package `package` into a source folder `name`.
    fn source(&self, package: &str, name: &str) -> PathBuf {
        copy_package(package, &self.sources.path().join(name))
    }

    fn launcher(&self) -> Launcher {
        Launcher::with_packages(
            Ok(self.runtime.clone()),
            vec![],
            self.data.path().join("extensions"),
        )
    }

    /// A launcher with the packages in source folders `names` installed.
    fn installed(&self, packages: &[(&str, &str)]) -> Launcher {
        let launcher = self.launcher();
        for (package, name) in packages {
            let folder = self.source(package, name);
            block_on(launcher.install_package(&folder));
            let status = launcher.view().status;
            assert!(
                matches!(&status, Status::Result(text) if text.starts_with("Installed")),
                "installing {package}: {status:?}"
            );
        }
        launcher
    }

    fn identity(&self, name: &str) -> PackageIdentity {
        PackageIdentity::local(&self.sources.path().join(name)).unwrap()
    }

    /// The components with a running instance, as file names.
    fn running(&self) -> Vec<String> {
        let mut running: Vec<String> = block_on(self.runtime.running())
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        running.sort();
        running
    }
}

fn titles(launcher: &Launcher) -> Vec<String> {
    launcher
        .view()
        .rows
        .into_iter()
        .map(|row| row.title)
        .collect()
}

fn select_title(launcher: &Launcher, title: &str) {
    let index = titles(launcher)
        .iter()
        .position(|row| row == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)));
    launcher.select(index);
}

/// From root search, opens the command titled `command` and runs its item
/// titled `item`, returning the outcome.
fn run(launcher: &Launcher, command: &str, item: &str) -> Status {
    launcher.back();
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().screen, Screen::Command, "{command} opened");
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    let status = launcher.view().status;
    launcher.back();
    status
}

fn result(text: &str) -> Status {
    Status::Result(text.into())
}

fn error(text: &str) -> Status {
    Status::Error(format!("The extension reported an error: {text}"))
}

const RUST: &str = "sample-operations";
const JAVASCRIPT: &str = "sample-operations-js";
const TYPESCRIPT: &str = "sample-operations-ts";

fn samples(dirs: &Dirs) -> Launcher {
    dirs.installed(&[
        (RUST, RUST),
        (JAVASCRIPT, JAVASCRIPT),
        (TYPESCRIPT, TYPESCRIPT),
    ])
}

#[test]
fn a_rust_command_calls_a_javascript_and_a_typescript_operation() {
    let dirs = Dirs::new();
    let launcher = samples(&dirs);

    assert_eq!(
        run(&launcher, "Call from Rust", "Ask JavaScript to greet"),
        result("JavaScript answered: Hello, Rust, from JavaScript")
    );
    assert_eq!(
        run(&launcher, "Call from Rust", "Ask TypeScript to greet"),
        result("TypeScript answered: Hello, Rust, from TypeScript")
    );
}

#[test]
fn javascript_and_typescript_commands_call_a_rust_operation() {
    let dirs = Dirs::new();
    let launcher = samples(&dirs);

    assert_eq!(
        run(&launcher, "Call from JavaScript", "Ask Rust to greet"),
        result("Rust answered: Hello, JavaScript, from Rust")
    );
    assert_eq!(
        run(&launcher, "Call from TypeScript", "Ask Rust to greet"),
        result("Rust answered: Hello, TypeScript, from Rust")
    );
}

#[test]
fn the_operation_s_own_error_reaches_the_caller_in_every_language() {
    let dirs = Dirs::new();
    let launcher = samples(&dirs);

    for (command, item) in [
        ("Call from Rust", "Ask JavaScript with no name"),
        ("Call from JavaScript", "Ask Rust with no name"),
        ("Call from TypeScript", "Ask Rust with no name"),
    ] {
        assert_eq!(
            run(&launcher, command, item),
            error("failed: a name is needed"),
            "{command}"
        );
    }
}

#[test]
fn a_package_that_is_not_installed_is_reported_in_every_language() {
    let dirs = Dirs::new();
    let launcher = samples(&dirs);
    let missing = dirs.sources.path().join("no-such-extension");

    for command in [
        "Call from Rust",
        "Call from JavaScript",
        "Call from TypeScript",
    ] {
        assert_eq!(
            run(&launcher, command, "Ask an extension that is not installed"),
            error(&format!(
                "not-found: no installed extension has the source local folder {}",
                missing.display()
            )),
            "{command}"
        );
    }
}

#[test]
fn a_target_starts_only_when_it_is_called() {
    let dirs = Dirs::new();
    let launcher = samples(&dirs);
    assert!(dirs.running().is_empty(), "{:?}", dirs.running());

    launcher.back();
    select_title(&launcher, "Call from Rust");
    block_on(launcher.activate_selected());
    assert_eq!(dirs.running(), ["sample_operations.wasm"]);

    select_title(&launcher, "Ask JavaScript to greet");
    block_on(launcher.activate_selected());
    assert_eq!(
        dirs.running(),
        ["sample_operations.wasm", "sample_operations_js.wasm"]
    );
}

/// The operations fixture component, built by `cargo xtask guests`.
fn fixture_component() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/operations_fixture.wasm");
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// What the fixture packages publish, unless a test says otherwise: every
/// operation of the component but `secret`.
const PUBLISHED: [&str; 5] = ["echo", "forward", "crash", "not-json", "remember"];

impl Dirs {
    /// Writes an operations fixture package in source folder `name`, titled
    /// "Package <name>", publishing `operations` at version 1, with the
    /// fixture's command if `command`.
    fn fixture(&self, name: &str, operations: &[&str], command: bool) -> PathBuf {
        let folder = self.sources.path().join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::copy(fixture_component(), folder.join("fixture.wasm")).unwrap();
        let operations: Vec<String> = operations
            .iter()
            .map(|id| format!(r#"{{ "id": "{id}", "version": 1, "component": "fixture.wasm" }}"#))
            .collect();
        let commands = if command {
            r#"{ "id": "fixture", "title": "Operations fixture", "component": "fixture.wasm" }"#
        } else {
            ""
        };
        let manifest = format!(
            r#"{{
                "manifestVersion": 1,
                "title": "Package {name}",
                "apiVersion": "0.1",
                "commands": [{commands}],
                "operations": [{}]
            }}"#,
            operations.join(", ")
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        folder
    }

    /// A launcher with fixture packages `a` (with the command) and `b`
    /// installed, both publishing [`PUBLISHED`].
    fn a_and_b(&self) -> Launcher {
        let launcher = self.launcher();
        for (name, command) in [("a", true), ("b", false)] {
            let folder = self.fixture(name, &PUBLISHED, command);
            install(&launcher, &folder);
        }
        launcher
    }
}

fn install(launcher: &Launcher, folder: &Path) {
    block_on(launcher.install_package(folder));
    let status = launcher.view().status;
    assert!(
        matches!(&status, Status::Result(text) if text.starts_with("Installed")),
        "installing {}: {status:?}",
        folder.display()
    );
}

/// Runs the fixture command's item `item` from package `a`.
fn fixture_run(launcher: &Launcher, item: &str) -> Status {
    run(launcher, "Operations fixture", item)
}

#[test]
fn only_published_operations_are_callable() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    assert_eq!(
        fixture_run(&launcher, "Call b's echo"),
        result(r#"answered: {"hello":"world"}"#)
    );
    // The component would serve `secret`, but the manifest does not publish it.
    assert_eq!(
        fixture_run(&launcher, "Call b's secret"),
        error(
            "not-found: Package b does not publish an operation `secret`; it publishes \
             `echo`, `forward`, `crash`, `not-json` and `remember`"
        )
    );
}

#[test]
fn an_operation_of_another_version_is_incompatible() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    assert_eq!(
        fixture_run(&launcher, "Call b's echo at version 2"),
        error("incompatible: Package b publishes `echo` at version 1, not version 2")
    );
}

#[test]
fn a_disabled_target_is_reported_and_stays_disabled() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();
    block_on(launcher.set_enabled(&dirs.identity("b"), false));

    assert_eq!(
        fixture_run(&launcher, "Call b's echo"),
        error(
            "disabled: Package b is disabled; Pane does not enable it for a call, enable it \
             in Manage extensions"
        )
    );
    let b = launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == dirs.identity("b"))
        .unwrap();
    assert!(!b.enabled);
    // Only the caller runs.
    assert_eq!(block_on(dirs.runtime.running()).len(), 1);

    block_on(launcher.set_enabled(&dirs.identity("b"), true));
    assert_eq!(
        fixture_run(&launcher, "Call b's echo"),
        result(r#"answered: {"hello":"world"}"#)
    );
}

#[test]
fn missing_and_unknown_sources_are_not_found() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();
    let missing = dirs.sources.path().join("missing");

    assert_eq!(
        fixture_run(&launcher, "Call a missing package"),
        error(&format!(
            "not-found: no installed extension has the source local folder {}",
            missing.display()
        ))
    );
    assert_eq!(
        fixture_run(&launcher, "Call a source that is not local"),
        error(
            "not-found: `npm:left-pad` is not a package source; use `local:` followed by the \
             package folder's path"
        )
    );
}

#[test]
fn a_crashed_target_is_reported_and_the_caller_keeps_working() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    let crashed = fixture_run(&launcher, "Call b's crash");
    assert!(
        matches!(&crashed, Status::Error(text)
            if text.starts_with("The extension reported an error: crashed: Package b crashed:")),
        "{crashed:?}"
    );
    // The caller stays usable, and the target starts afresh.
    assert_eq!(
        fixture_run(&launcher, "Call b's echo"),
        result(r#"answered: {"hello":"world"}"#)
    );
}

#[test]
fn input_and_results_that_are_not_json_are_refused() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    let refused = |status: Status, start: &str| {
        assert!(
            matches!(&status, Status::Error(text)
                if text.starts_with(&format!("The extension reported an error: refused: {start}"))),
            "{status:?}"
        );
    };
    refused(
        fixture_run(&launcher, "Call b with input that is not JSON"),
        "the input is not JSON",
    );
    refused(
        fixture_run(&launcher, "Call b's not-json"),
        "the result of Package b is not JSON",
    );
}

#[test]
fn calling_back_into_a_package_in_the_chain_is_refused_without_waiting() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    assert_eq!(
        fixture_run(&launcher, "Call myself"),
        error(
            "refused: Package a is already serving a call in this chain; an extension cannot \
             be called back while its own call waits"
        )
    );
    // a calls b, which calls a: b reports a's refusal as its own error.
    assert_eq!(
        fixture_run(&launcher, "Call b, which calls me back"),
        error(
            "failed: refused: Package a is already serving a call in this chain; an extension \
             cannot be called back while its own call waits"
        )
    );
    // Both keep working.
    assert_eq!(
        fixture_run(&launcher, "Call b's echo"),
        result(r#"answered: {"hello":"world"}"#)
    );
}

#[test]
fn a_chain_deeper_than_the_limit_is_refused() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.fixture("a", &PUBLISHED, true));
    for index in 1..=9 {
        install(
            &launcher,
            &dirs.fixture(&format!("p{index}"), &PUBLISHED, false),
        );
    }

    let status = fixture_run(&launcher, "Call a chain of nine");
    // a, p1 ... p7 hold the chain of eight; p7's call to p8 is refused.
    assert!(
        matches!(&status, Status::Error(text) if text.ends_with(
            "refused: the chain of calls is 8 deep; Pane allows at most 8"
        )),
        "{status:?}"
    );
    assert_eq!(pane_core::MAX_CALL_DEPTH, 8);
}

#[test]
fn each_package_keeps_its_own_settings() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    // b saves the value in its own settings; a has none.
    assert_eq!(
        fixture_run(&launcher, "Call b's remember"),
        result("answered: true; mine: None")
    );
    let settings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(dirs.data.path().join("extensions/settings.json")).unwrap(),
    )
    .unwrap();
    let packages = &settings["packages"];
    assert_eq!(
        packages[dirs.identity("b").key()]["last"],
        serde_json::json!(r#""from a""#)
    );
    assert!(
        packages.get(dirs.identity("a").key()).is_none(),
        "{packages}"
    );
}

#[test]
fn a_package_that_only_publishes_operations_adds_no_command() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.fixture("b", &PUBLISHED, false));

    assert_eq!(
        titles(&launcher),
        ["Install extension from folder…", "Manage extensions…"]
    );
}

#[test]
fn invalid_operations_are_explained_and_not_installed() {
    let cases = [
        (
            r#"{ "id": "echo", "version": 0, "component": "fixture.wasm" }"#,
            "Invalid pane.json: operation `echo` has version 0; versions start at 1",
        ),
        (
            r#"{ "id": "echo", "version": 1, "component": "fixture.wasm" },
               { "id": "echo", "version": 2, "component": "fixture.wasm" }"#,
            "Invalid pane.json: operation id `echo` is repeated",
        ),
        (
            r#"{ "id": "echo", "version": 1, "component": "../fixture.wasm" }"#,
            "Invalid pane.json: component `../fixture.wasm` must be a relative path inside \
             the package folder",
        ),
        (
            r#"{ "id": "echo", "version": 1, "component": "dist/echo.wasm" }"#,
            "Not ready to run: the component dist/echo.wasm of \"operation `echo`\" is \
             missing. This looks like a source-only package; build its component before \
             installing",
        ),
    ];
    for (operations, explanation) in cases {
        let dirs = Dirs::new();
        let folder = dirs.fixture("b", &[], false);
        let manifest = fs::read_to_string(folder.join("pane.json"))
            .unwrap()
            .replace(
                r#""operations": []"#,
                &format!(r#""operations": [{operations}]"#),
            );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        let launcher = dirs.launcher();

        block_on(launcher.install_package(&folder));

        assert_eq!(
            launcher.view().status,
            Status::Error(explanation.into()),
            "{operations}"
        );
        assert!(launcher.packages().is_empty());
    }
}

#[test]
fn a_component_that_serves_no_operations_cannot_publish_one() {
    let dirs = Dirs::new();
    let folder = dirs.fixture("b", &["echo"], false);
    // The Rust sample command exports `command` only.
    let command_only = fixture_component().with_file_name("sample_rust.wasm");
    fs::copy(command_only, folder.join("fixture.wasm")).unwrap();
    let launcher = dirs.launcher();

    block_on(launcher.install_package(&folder));

    let status = launcher.view().status;
    assert!(
        matches!(&status, Status::Error(text) if text.starts_with(
            "\"operation `echo`\": Incompatible extension: it does not implement Pane's \
             extension interface: its manifest publishes operations it serves, but it does \
             not export pane:extension/published-operations@0.1.0"
        )),
        "{status:?}"
    );
    assert!(launcher.packages().is_empty());
}

#[test]
fn a_package_preview_lists_its_operations() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    let folder = dirs.fixture("b", &["echo", "forward"], false);

    block_on(launcher.preview_package(&folder));

    let details = launcher.view().details().to_vec();
    assert!(
        details.contains(&"Operations: echo (version 1), forward (version 1)".to_owned()),
        "{details:?}"
    );
}
