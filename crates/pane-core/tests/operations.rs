//! Calling an operation another installed package publishes, through the
//! launcher's public interface: a command of one package calls an operation
//! of another by that package's identity, across Rust, JavaScript and
//! TypeScript, and shows its result or why the call failed. The operations
//! samples (`guests/sample-operations*`) show what authors write; the
//! operations fixture (`guests/fixtures/operations`) drives the failures,
//! cycles, concurrency and limits. Real guests from `cargo xtask guests`.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::{Launcher, PackageIdentity, Runtime, SavedData, Screen, Status};
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

/// Source folders, Pane's data folder and the runtime the launchers share.
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

    /// Where folder `name` is (it need not exist), spelled as Pane spells a
    /// package identity: resolved by the operating system (macOS reports
    /// `/private/var/...` for a temporary `/var/...` folder, Windows the long
    /// form of a short `RUNNER~1` name), without Windows' `\\?\` prefix.
    fn resolved(&self, name: &str) -> PathBuf {
        let sources = PackageIdentity::local(self.sources.path()).unwrap();
        sources.local_folder().unwrap().join(name)
    }

    fn launcher(&self) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.extensions())
    }

    fn extensions(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    fn identity(&self, name: &str) -> PackageIdentity {
        PackageIdentity::local(&self.folder(name)).unwrap()
    }

    /// The source other packages call the package in folder `name` by: its
    /// identity, as Pane shows it.
    fn source(&self, name: &str) -> String {
        self.identity(name).key()
    }

    /// Copies the assembled sample package `package` into source folder
    /// `package`.
    fn sample(&self, package: &str) -> PathBuf {
        let assembled = guest("packages").join(package);
        let folder = self.folder(package);
        fs::create_dir_all(&folder).unwrap();
        for entry in fs::read_dir(assembled).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
        }
        folder
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

fn install(launcher: &Launcher, folder: &Path) {
    block_on(launcher.install_package(folder));
    let status = launcher.view().status;
    assert!(
        matches!(&status, Status::Result(text) if text.starts_with("Installed")),
        "installing {}: {status:?}",
        folder.display()
    );
}

/// From root search, opens the command titled `command` and activates its
/// item titled `item`.
fn open_item(launcher: &Launcher, command: &str, item: &str) {
    launcher.back();
    launcher.back();
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().screen, Screen::Command, "{command} opened");
    select_title(launcher, item);
    block_on(launcher.activate_selected());
}

// The samples: each command's form calls `greet` of the package whose
// source is typed in.

const RUST: &str = "sample-operations";
const JAVASCRIPT: &str = "sample-operations-js";
const TYPESCRIPT: &str = "sample-operations-ts";

fn samples(dirs: &Dirs) -> Launcher {
    let launcher = dirs.launcher();
    for package in [RUST, JAVASCRIPT, TYPESCRIPT] {
        install(&launcher, &dirs.sample(package));
    }
    launcher
}

/// Submits the greet form of `command` with `source`, `name` and `times`
/// ("once" or "twice"), returning the outcome.
fn greet(launcher: &Launcher, command: &str, source: &str, name: &str, times: &str) -> Status {
    open_item(launcher, command, "Greet through another extension");
    assert!(launcher.view().form().is_some(), "{command}: no form");
    launcher.set_field_value("source", source);
    launcher.set_field_value("name", name);
    launcher.set_field_value("times", times);
    block_on(launcher.submit_form());
    launcher.view().status
}

fn result(text: &str) -> Status {
    Status::Result(text.into())
}

#[test]
fn a_rust_command_calls_a_javascript_and_a_typescript_operation() {
    let dirs = Dirs::new();
    let launcher = samples(&dirs);

    assert_eq!(
        greet(
            &launcher,
            "Call from Rust",
            &dirs.source(JAVASCRIPT),
            "Rust",
            "once"
        ),
        result("Hello, Rust, from JavaScript")
    );
    assert_eq!(
        greet(
            &launcher,
            "Call from Rust",
            &dirs.source(TYPESCRIPT),
            "Rust",
            "once"
        ),
        result("Hello, Rust, from TypeScript")
    );
}

#[test]
fn javascript_and_typescript_commands_call_a_rust_operation() {
    let dirs = Dirs::new();
    let launcher = samples(&dirs);

    for (command, name) in [
        ("Call from JavaScript", "JavaScript"),
        ("Call from TypeScript", "TypeScript"),
    ] {
        assert_eq!(
            greet(&launcher, command, &dirs.source(RUST), name, "once"),
            result(&format!("Hello, {name}, from Rust"))
        );
    }
}

#[test]
fn two_calls_made_at_once_are_both_served_in_every_language() {
    let dirs = Dirs::new();
    let launcher = samples(&dirs);

    // Rust joins two calls; JavaScript and TypeScript use Promise.all.
    for (command, target, answer) in [
        ("Call from Rust", JAVASCRIPT, "Hello, Ada, from JavaScript"),
        ("Call from JavaScript", RUST, "Hello, Ada, from Rust"),
        ("Call from TypeScript", RUST, "Hello, Ada, from Rust"),
    ] {
        assert_eq!(
            greet(&launcher, command, &dirs.source(target), "Ada", "twice"),
            result(&format!("{answer} / {answer}")),
            "{command}"
        );
    }
}

#[test]
fn the_operation_s_own_error_reaches_the_caller_in_every_language() {
    let dirs = Dirs::new();
    let launcher = samples(&dirs);

    for (command, target) in [
        ("Call from Rust", JAVASCRIPT),
        ("Call from JavaScript", RUST),
        ("Call from TypeScript", RUST),
    ] {
        assert_eq!(
            greet(&launcher, command, &dirs.source(target), "", "once"),
            Status::Error("failed: a name is needed".into()),
            "{command}"
        );
    }
}

#[test]
fn a_package_that_is_not_installed_is_reported_in_every_language() {
    let dirs = Dirs::new();
    let launcher = samples(&dirs);
    let missing = format!("local:{}", dirs.resolved("no-such-extension").display());

    for command in [
        "Call from Rust",
        "Call from JavaScript",
        "Call from TypeScript",
    ] {
        assert_eq!(
            greet(&launcher, command, &missing, "Ada", "once"),
            Status::Error(format!(
                "not-found: no installed extension has the source {missing}"
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

    open_item(
        &launcher,
        "Call from Rust",
        "Greet through another extension",
    );
    assert_eq!(dirs.running(), ["sample_operations.wasm"]);

    launcher.set_field_value("source", &dirs.source(JAVASCRIPT));
    launcher.set_field_value("name", "Rust");
    block_on(launcher.submit_form());
    assert_eq!(
        dirs.running(),
        ["sample_operations.wasm", "sample_operations_js.wasm"]
    );
}

// The fixture: package `a` has the command; the others only publish.

/// What the fixture packages publish, unless a test says otherwise: every
/// operation of the component but `secret`.
const PUBLISHED: &str = r#"
    { "id": "echo", "version": 1, "component": "fixture.wasm" },
    { "id": "forward", "version": 1, "component": "fixture.wasm" },
    { "id": "crash", "version": 1, "component": "fixture.wasm" },
    { "id": "not-json", "version": 1, "component": "fixture.wasm" },
    { "id": "remember", "version": 1, "component": "fixture.wasm" }
"#;

/// The fixture's command, as `a` lists it.
const COMMAND: &str =
    r#"{ "id": "fixture", "title": "Operations fixture", "component": "fixture.wasm" }"#;

impl Dirs {
    /// Writes a fixture package in source folder `name`, titled
    /// "Package <name>", with `commands` and `operations` (JSON array
    /// contents) and the fixture component as `fixture.wasm`.
    fn fixture(&self, name: &str, commands: &str, operations: &str) -> PathBuf {
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
                "commands": [{commands}],
                "operations": [{operations}]
            }}"#
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        folder
    }

    /// Saves, as `a`'s settings, the sources the fixture calls by name:
    /// those of `names` and of a missing folder. Before any launcher opens
    /// the data folder.
    fn save_sources(&self, names: &[&str]) {
        let mut sources = serde_json::Map::new();
        for name in names {
            sources.insert((*name).into(), self.source(name).into());
        }
        let missing = format!("local:{}", self.resolved("missing").display());
        sources.insert("missing".into(), missing.into());
        let settings = serde_json::json!({
            "version": 1,
            "packages": {
                self.source("a"): { "sources": serde_json::Value::Object(sources).to_string() }
            }
        });
        fs::create_dir_all(self.extensions()).unwrap();
        fs::write(
            self.extensions().join("settings.json"),
            settings.to_string(),
        )
        .unwrap();
    }

    /// Installs the fixture packages written in folders `names`, `a` first,
    /// after saving their sources for `a`.
    fn install_fixtures(&self, names: &[&str]) -> Launcher {
        self.save_sources(names);
        let launcher = self.launcher();
        for name in names {
            install(&launcher, &self.folder(name));
        }
        launcher
    }

    /// `a` with the command and `b`, both publishing [`PUBLISHED`], installed.
    fn a_and_b(&self) -> Launcher {
        self.fixture("a", COMMAND, PUBLISHED);
        self.fixture("b", "", PUBLISHED);
        self.install_fixtures(&["a", "b"])
    }
}

/// Runs the fixture command's item `item` from package `a`, returning
/// what it showed: its toast, or the status line.
fn fixture_run(launcher: &Launcher, item: &str) -> Status {
    open_item(launcher, "Operations fixture", item);
    shown(launcher)
}

fn error(text: &str) -> Status {
    Status::Error(format!("The extension reported an error: {text}"))
}

fn assert_error_starts(status: &Status, start: &str) {
    assert!(
        matches!(status, Status::Error(text)
            if text.starts_with(&format!("The extension reported an error: {start}"))),
        "{status:?} does not start with {start:?}"
    );
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
fn a_target_whose_component_pane_cannot_run_is_incompatible() {
    for (replacement, start) in [
        (
            "mixed_p2.wasm",
            "incompatible: Package c: Incompatible extension: Pane supports only WASI 0.3, but it \
             imports wasi:",
        ),
        (
            "old_api.wasm",
            "incompatible: Package c: Incompatible extension: it was built for an older \
             extension API shape",
        ),
    ] {
        let dirs = Dirs::new();
        dirs.fixture("a", COMMAND, PUBLISHED);
        dirs.fixture("c", "", PUBLISHED);
        let launcher = dirs.install_fixtures(&["a", "c"]);
        // Its installed copy is replaced behind Pane's back: installing
        // would have refused it.
        let c = launcher
            .packages()
            .into_iter()
            .find(|package| package.identity == dirs.identity("c"))
            .unwrap();
        fs::copy(guest(replacement), c.location.join("fixture.wasm")).unwrap();

        assert_error_starts(&fixture_run(&launcher, "Call c's echo"), start);
        // The caller keeps working.
        assert_eq!(
            fixture_run(&launcher, "Call myself"),
            error(
                "refused: Package a is already serving a call in this chain; an extension \
                 cannot be called back while its own call waits"
            )
        );
    }
}

#[test]
fn an_operation_for_other_systems_is_unavailable() {
    let dirs = Dirs::new();
    let [other, _] = platforms::other_systems();
    dirs.fixture("a", COMMAND, PUBLISHED);
    dirs.fixture(
        "c",
        "",
        &format!(
            r#"{{ "id": "echo", "version": 1, "component": "fixture.wasm", "platforms": ["{}"] }}"#,
            other.id()
        ),
    );
    let launcher = dirs.install_fixtures(&["a", "c"]);

    assert_eq!(
        fixture_run(&launcher, "Call c's echo"),
        error(&format!(
            "unavailable: Package c: {}",
            platforms::only("this operation", platforms::name(other))
        ))
    );
    // Only the caller runs.
    assert_eq!(dirs.running(), ["fixture.wasm"]);
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
             in Settings"
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
fn an_uninstalled_target_is_not_found_and_its_instance_stops() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();
    assert_eq!(
        fixture_run(&launcher, "Call b's echo"),
        result(r#"answered: {"hello":"world"}"#)
    );
    assert_eq!(block_on(dirs.runtime.running()).len(), 2);

    block_on(launcher.uninstall(&dirs.identity("b"), SavedData::Delete));
    assert_eq!(
        launcher.view().status,
        result("Uninstalled Package b and deleted its saved data")
    );
    // Only the caller's instance is left.
    assert_eq!(block_on(dirs.runtime.running()).len(), 1);
    assert_eq!(
        fixture_run(&launcher, "Call b's echo"),
        error(&format!(
            "not-found: no installed extension has the source {}",
            dirs.source("b")
        ))
    );
}

#[test]
fn a_package_is_called_by_its_identity_only() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    assert_eq!(
        fixture_run(&launcher, "Call a missing package"),
        error(&format!(
            "not-found: no installed extension has the source local:{}",
            dirs.resolved("missing").display()
        ))
    );
    // A path relative to anything is not an identity.
    assert_eq!(
        fixture_run(&launcher, "Call a relative source"),
        error(
            "not-found: `local:../b` is not a package identity; use `local:` followed by \
             the absolute folder path Pane shows for the package, `npm:` followed by its npm \
             package name, or `git:` followed by its repository"
        )
    );
    assert_eq!(
        fixture_run(&launcher, "Call a source that is not local"),
        // An npm package is named by its name, as installed.
        error("not-found: no installed extension has the source npm:left-pad")
    );
}

#[test]
fn a_crashed_target_is_reported_and_the_caller_keeps_working() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    assert_error_starts(
        &fixture_run(&launcher, "Call b's crash"),
        "crashed: Package b crashed:",
    );
    // The caller stays usable, and the target starts afresh.
    assert_eq!(
        fixture_run(&launcher, "Call b's echo"),
        result(r#"answered: {"hello":"world"}"#)
    );
}

#[test]
fn a_target_that_keeps_crashing_is_paused_and_its_caller_is_not() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    for _ in 0..3 {
        assert_error_starts(
            &fixture_run(&launcher, "Call b's crash"),
            "crashed: Package b crashed:",
        );
    }
    // The crashes were `b`'s: `b` is paused, while its caller runs on (its
    // command opens and answers).
    assert_eq!(
        fixture_run(&launcher, "Call b's echo"),
        error("unavailable: Package b is paused after an error; retry it in Settings")
    );

    block_on(launcher.retry_start(&dirs.identity("b")));
    assert_eq!(launcher.view().status, result("Started Package b"));
    assert_eq!(
        fixture_run(&launcher, "Call b's echo"),
        result(r#"answered: {"hello":"world"}"#)
    );
}

#[test]
fn input_and_results_that_are_not_json_are_refused() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    assert_error_starts(
        &fixture_run(&launcher, "Call b with input that is not JSON"),
        "refused: the input is not JSON",
    );
    assert_error_starts(
        &fixture_run(&launcher, "Call b's not-json"),
        "refused: the result of Package b is not JSON",
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
fn a_package_in_the_chain_is_refused_whichever_component_serves_it() {
    let dirs = Dirs::new();
    // a's operations are served by a second copy of the component, not by
    // its command's.
    let folder = dirs.fixture(
        "a",
        COMMAND,
        &PUBLISHED.replace("fixture.wasm", "serve.wasm"),
    );
    fs::copy(guest("operations_fixture.wasm"), folder.join("serve.wasm")).unwrap();
    let launcher = dirs.install_fixtures(&["a"]);

    assert_eq!(
        fixture_run(&launcher, "Call my own package's other component"),
        error(
            "refused: Package a is already serving a call in this chain; an extension cannot \
             be called back while its own call waits"
        )
    );
}

#[test]
fn calls_a_guest_makes_at_once_are_served_one_after_another() {
    let dirs = Dirs::new();
    dirs.fixture("a", COMMAND, PUBLISHED);
    dirs.fixture("b", "", PUBLISHED);
    dirs.fixture("c", "", PUBLISHED);
    let launcher = dirs.install_fixtures(&["a", "b", "c"]);

    // While b waits on c for the first, the second is not taken for a call
    // of b's (which would find b in the chain): a's frame serves it next.
    assert_eq!(
        fixture_run(&launcher, "Call b twice at once, which calls c"),
        result(r#"answered: "first" and "second""#)
    );
}

#[test]
fn a_call_its_caller_gives_up_on_before_it_starts_never_runs() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    assert_eq!(
        fixture_run(&launcher, "Call b's remember and give up at once"),
        result("gave up: true")
    );
    // b never runs when the give-up lands first: its host call is not
    // started, or its operation is stopped before it saves, and no
    // instance of it is left. But the give-up is a race the design
    // documents (operations.md: the caller's guest is not polled while
    // its operation runs, so it cannot abandon one already running): on a
    // machine whose threads stall — CI's shared runners — the call can be
    // served after the caller gave up, and b then runs to its end, saving
    // its input and keeping its instance. Both outcomes are checked for
    // what each guarantees: either nothing of b ran, or it ran to its end
    // — never a half-run.
    let settings = fs::read_to_string(dirs.extensions().join("settings.json"));
    if settings
        .as_deref()
        .is_ok_and(|text| text.contains("given up"))
    {
        // The raced outcome: b ran to its end, so its instance is kept.
        assert_eq!(
            dirs.running(),
            ["fixture.wasm", "fixture.wasm"],
            "b saved but is not running"
        );
    } else {
        let settled = Instant::now() + Duration::from_secs(10);
        while dirs.running() != ["fixture.wasm".to_owned()] {
            assert!(
                Instant::now() < settled,
                "b is still running: {:?}",
                dirs.running()
            );
            thread::sleep(Duration::from_millis(5));
        }
        let settings = settings.unwrap();
        assert!(!settings.contains("given up"), "{settings}");
    }
}

#[test]
fn a_chain_deeper_than_the_limit_is_refused() {
    let dirs = Dirs::new();
    dirs.fixture("a", COMMAND, PUBLISHED);
    let mut names = vec!["a".to_owned()];
    for index in 1..=9 {
        let name = format!("p{index}");
        dirs.fixture(&name, "", PUBLISHED);
        names.push(name);
    }
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let launcher = dirs.install_fixtures(&names);

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
    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dirs.extensions().join("settings.json")).unwrap())
            .unwrap();
    let packages = &settings["packages"];
    assert_eq!(
        packages[dirs.source("b")]["last"],
        serde_json::json!(r#""from a""#)
    );
    assert!(
        packages[dirs.source("a")].get("last").is_none(),
        "{packages}"
    );
}

#[test]
fn a_package_that_only_publishes_operations_adds_no_command() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.fixture("b", "", PUBLISHED));

    assert_eq!(
        titles(&launcher),
        [
            "Install extension from folder…",
            "Install extension from Git…",
            "Install extension from npm…",
            "Manage Extensions",
            // Pane's own row, ranked with the commands by title (#199).
            "Settings…"
        ]
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
        (
            r#"{ "id": "echo", "version": 1, "component": "fixture.wasm", "platforms": ["amiga"] }"#,
            "Invalid pane.json: unknown platform `amiga` in `platforms` of operation `echo`; \
             use windows, macos or linux",
        ),
    ];
    for (operations, explanation) in cases {
        let dirs = Dirs::new();
        let folder = dirs.fixture("b", "", operations);
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
    let folder = dirs.fixture(
        "b",
        "",
        r#"{ "id": "echo", "version": 1, "component": "fixture.wasm" }"#,
    );
    // The Rust sample command exports `command` only.
    fs::copy(guest("sample_rust.wasm"), folder.join("fixture.wasm")).unwrap();
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
    let folder = dirs.fixture(
        "b",
        "",
        r#"{ "id": "echo", "version": 1, "component": "fixture.wasm" },
           { "id": "forward", "version": 1, "component": "fixture.wasm" }"#,
    );

    block_on(launcher.preview_package(&folder));

    let details = launcher.view().details().to_vec();
    assert!(
        details.contains(&"Operations: echo (version 1), forward (version 1)".to_owned()),
        "{details:?}"
    );
}

// Stopping calls in a chain: `a`'s item "Call b's wait" waits in `b`'s
// `wait`, which saves "started", waits ten seconds and saves "finished".

/// Well under the ten seconds `wait` waits.
// How long a stop may take while proving it did not wait for the guest's
// own wait (about a minute): generous for a runner whose CPUs other tests
// in the same binary are sharing, still far from the wait itself.
const STOPPED_WITHIN: Duration = Duration::from_secs(15);

impl Dirs {
    /// `a` with the command and `b` publishing `wait` too, installed.
    fn a_and_waiting_b(&self) -> Launcher {
        let waiting = format!(
            r#"{PUBLISHED}, {{ "id": "wait", "version": 1, "component": "fixture.wasm" }}"#
        );
        self.fixture("a", COMMAND, PUBLISHED);
        self.fixture("b", "", &waiting);
        self.install_fixtures(&["a", "b"])
    }

    /// What `b`'s `wait` saved: "started", "finished" or nothing.
    fn waiting(&self) -> Option<String> {
        let text = fs::read_to_string(self.extensions().join("settings.json")).unwrap_or_default();
        ["finished", "started"]
            .into_iter()
            .find(|progress| text.contains(&format!("\"waiting\": \"{progress}\"")))
            .map(str::to_owned)
    }

    /// Whether a component of the installed package `name` is running.
    fn is_running(&self, launcher: &Launcher, name: &str) -> bool {
        let location = launcher
            .packages()
            .into_iter()
            .find(|package| package.identity == self.identity(name))
            .unwrap()
            .location;
        block_on(self.runtime.running())
            .iter()
            .any(|component| component.starts_with(&location))
    }
}

/// Runs `a`'s "Call b's wait" on another thread, returning once `b` waits.
fn start_waiting(dirs: &Dirs, launcher: &Launcher) -> (thread::JoinHandle<()>, Instant) {
    launcher.back();
    launcher.back();
    select_title(launcher, "Operations fixture");
    block_on(launcher.activate_selected());
    select_title(launcher, "Call b's wait");
    let calling = launcher.activate_selected();
    let started = Instant::now();
    let thread = thread::spawn(move || block_on(calling));
    while dirs.waiting().as_deref() != Some("started") {
        assert!(
            started.elapsed() < STOPPED_WITHIN,
            "b did not start waiting"
        );
        thread::sleep(Duration::from_millis(10));
    }
    (thread, started)
}

#[test]
fn disabling_a_target_stops_the_call_it_serves_and_its_caller_is_told() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_waiting_b();
    let (calling, started) = start_waiting(&dirs, &launcher);

    // Disabling applies at once; recording it (and reporting that) waits,
    // so the caller's answer is seen first.
    let disabling = launcher.set_enabled(&dirs.identity("b"), false);
    calling.join().unwrap();

    assert!(
        started.elapsed() < STOPPED_WITHIN,
        "{:?}",
        started.elapsed()
    );
    // The caller carries on with the error and answers at once.
    assert_eq!(
        shown(&launcher),
        error(
            "disabled: Package b is disabled; Pane does not enable it for a call, enable it \
             in Settings"
        )
    );
    block_on(disabling);
    assert_eq!(dirs.waiting().as_deref(), Some("started"));
    assert!(!dirs.is_running(&launcher, "b"));
    assert!(dirs.is_running(&launcher, "a"));
}

#[test]
fn disabling_a_caller_stops_the_operation_it_waits_for() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_waiting_b();
    let (calling, started) = start_waiting(&dirs, &launcher);

    block_on(launcher.set_enabled(&dirs.identity("a"), false));
    calling.join().unwrap();

    assert!(
        started.elapsed() < STOPPED_WITHIN,
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        launcher.view().status,
        Status::Result("Disabled Package a".into())
    );
    // `b`'s abandoned call never finishes: its instance went with it.
    assert_eq!(dirs.waiting().as_deref(), Some("started"));
    assert!(!dirs.is_running(&launcher, "a"));
    assert!(!dirs.is_running(&launcher, "b"));
    // `b` itself is not disabled, and serves the next call afresh.
    block_on(launcher.set_enabled(&dirs.identity("a"), true));
    assert_eq!(
        fixture_run(&launcher, "Call b's echo"),
        result(r#"answered: {"hello":"world"}"#)
    );
    assert_eq!(dirs.waiting().as_deref(), Some("started"));
}

#[test]
fn a_target_stopped_with_its_caller_again_and_again_is_not_paused() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_waiting_b();
    // Each time, `b`'s instance goes with the stopped call and restarts
    // afresh on the next call, as after a crash; that is not a crash of
    // `b`'s.
    for _ in 0..3 {
        // `b` saves "started" as it begins waiting; Pane writes the file
        // afresh from its own copy then, so removing it makes the next
        // "started" a sign of this iteration's wait.
        let _ = fs::remove_file(dirs.extensions().join("settings.json"));
        let (calling, _) = start_waiting(&dirs, &launcher);
        block_on(launcher.set_enabled(&dirs.identity("a"), false));
        calling.join().unwrap();
        block_on(launcher.set_enabled(&dirs.identity("a"), true));
    }
    assert_eq!(
        fixture_run(&launcher, "Call b's echo"),
        result(r#"answered: {"hello":"world"}"#)
    );
}

#[test]
fn reloading_a_target_stops_the_call_it_serves_and_its_caller_is_told() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_waiting_b();
    let (calling, started) = start_waiting(&dirs, &launcher);

    block_on(launcher.reload(&dirs.identity("b")));
    calling.join().unwrap();

    assert!(
        started.elapsed() < STOPPED_WITHIN,
        "{:?}",
        started.elapsed()
    );
    // The caller's answer (a failure toast) and the reload's own (in the
    // status line) are both shown, in whichever order they finish; either
    // is the last one shown.
    let status = shown(&launcher);
    assert!(
        status
            == error(
                "unavailable: Package b was reloaded or updated while serving the call; call it \
                 again"
            )
            || status == result("Reloaded Package b"),
        "{status:?}"
    );
    assert_eq!(dirs.waiting().as_deref(), Some("started"));
}

// Stopped code that is still running: `a` computes without yielding, so it
// is not interrupted, but what it tries once its generation has ended is
// refused and its answer is discarded.

impl Dirs {
    /// What `a`'s spinning items saved: "started", "finished" or nothing.
    fn spin(&self) -> Option<String> {
        let text = fs::read_to_string(self.extensions().join("settings.json")).unwrap_or_default();
        ["finished", "started"]
            .into_iter()
            .find(|progress| text.contains(&format!("\"spin\": \"{progress}\"")))
            .map(str::to_owned)
    }
}

/// Runs `a`'s item `item`, which spins, on another thread, returning once
/// it spins.
fn start_spinning(dirs: &Dirs, launcher: &Launcher, item: &str) -> thread::JoinHandle<()> {
    launcher.back();
    launcher.back();
    select_title(launcher, "Operations fixture");
    block_on(launcher.activate_selected());
    select_title(launcher, item);
    let running = launcher.activate_selected();
    let started = Instant::now();
    let thread = thread::spawn(move || block_on(running));
    while dirs.spin().as_deref() != Some("started") {
        assert!(
            started.elapsed() < STOPPED_WITHIN,
            "a did not start spinning"
        );
        thread::sleep(Duration::from_millis(10));
    }
    thread
}

/// `a` saves nothing and calls nothing once it is stopped, and neither its
/// answer nor its error is shown.
fn stopped_code_that_keeps_running_is_refused(stop: impl Fn(&Dirs, &Launcher), shown: Status) {
    for item in ["Spin, then save and call b's remember", "Spin, then fail"] {
        let dirs = Dirs::new();
        let launcher = dirs.a_and_b();
        let spinning = start_spinning(&dirs, &launcher, item);

        stop(&dirs, &launcher);
        spinning.join().unwrap();

        assert_eq!(launcher.view().status, shown, "{item}");
        assert_eq!(dirs.spin().as_deref(), Some("started"), "{item}");
        let saved = fs::read_to_string(dirs.extensions().join("settings.json")).unwrap();
        assert!(!saved.contains("from a spinning caller"), "{saved}");
        assert!(!dirs.is_running(&launcher, "b"), "{item}");
    }
}

#[test]
fn disabled_code_that_keeps_running_saves_and_calls_nothing() {
    stopped_code_that_keeps_running_is_refused(
        |dirs, launcher| block_on(launcher.set_enabled(&dirs.identity("a"), false)),
        Status::Result("Disabled Package a".into()),
    );
}

#[test]
fn replaced_code_that_keeps_running_saves_and_calls_nothing() {
    stopped_code_that_keeps_running_is_refused(
        |dirs, launcher| block_on(launcher.reload(&dirs.identity("a"))),
        Status::Result("Reloaded Package a".into()),
    );
}

// Stopping a chain across languages with the samples' `wait`, which saves
// `waiting` as "started", waits ten seconds and saves "finished".

/// The samples' package titles and commands, by package folder.
fn sample_titles(package: &str) -> (&'static str, &'static str) {
    match package {
        RUST => ("Rust operations sample", "Call from Rust"),
        JAVASCRIPT => ("JavaScript operations sample", "Call from JavaScript"),
        TYPESCRIPT => ("TypeScript operations sample", "Call from TypeScript"),
        other => panic!("no sample {other}"),
    }
}

/// Submits `caller`'s "Wait in another extension" form for `target` on
/// another thread, returning once the target waits.
fn start_sample_wait(dirs: &Dirs, launcher: &Launcher, caller: &str, target: &str) -> Pending {
    open_item(
        launcher,
        sample_titles(caller).1,
        "Wait in another extension",
    );
    launcher.set_field_value("source", &dirs.source(target));
    let submitting = launcher.submit_form();
    let started = Instant::now();
    let thread = thread::spawn(move || block_on(submitting));
    while dirs.waiting().as_deref() != Some("started") {
        assert!(
            started.elapsed() < STOPPED_WITHIN,
            "{target} did not start waiting"
        );
        thread::sleep(Duration::from_millis(10));
    }
    Pending { thread, started }
}

struct Pending {
    thread: thread::JoinHandle<()>,
    started: Instant,
}

impl Pending {
    fn assert_stopped(self) {
        self.thread.join().unwrap();
        let took = self.started.elapsed();
        assert!(took < STOPPED_WITHIN, "the call ran for {took:?}");
    }
}

/// (caller, target): each language calls another.
const SAMPLE_CHAINS: [(&str, &str); 3] = [
    (RUST, JAVASCRIPT),
    (JAVASCRIPT, TYPESCRIPT),
    (TYPESCRIPT, RUST),
];

#[test]
fn disabling_a_sample_target_stops_its_wait_in_every_language() {
    for (caller, target) in SAMPLE_CHAINS {
        let dirs = Dirs::new();
        let launcher = samples(&dirs);
        let pending = start_sample_wait(&dirs, &launcher, caller, target);

        // Recorded after the caller answered, so its answer is seen first.
        let disabling = launcher.set_enabled(&dirs.identity(target), false);
        pending.assert_stopped();

        assert_eq!(
            launcher.view().status,
            Status::Error(format!(
                "disabled: {} is disabled; Pane does not enable it for a call, enable it in \
                 Settings",
                sample_titles(target).0
            )),
            "{caller} calling {target}"
        );
        block_on(disabling);
        assert_eq!(dirs.waiting().as_deref(), Some("started"));
    }
}

#[test]
fn disabling_a_sample_caller_stops_the_wait_it_asked_for_in_every_language() {
    for (caller, target) in SAMPLE_CHAINS {
        let dirs = Dirs::new();
        let launcher = samples(&dirs);
        let pending = start_sample_wait(&dirs, &launcher, caller, target);

        block_on(launcher.set_enabled(&dirs.identity(caller), false));
        pending.assert_stopped();

        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Disabled {}", sample_titles(caller).0)),
            "{caller} calling {target}"
        );
        assert_eq!(dirs.waiting().as_deref(), Some("started"));
        assert!(dirs.running().is_empty(), "{:?}", dirs.running());
    }
}
