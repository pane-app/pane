//! Calling a capability by name through the launcher's public interface
//! (ADR 0041): a consumer in each language calls the provider in each
//! language by the capability's name, `pane-samples:greet@1`, rather than
//! by a package identity, and Pane routes each call to the provider that
//! can serve it — the first one installed — never the caller's own
//! package. Failures name the capability, and a package whose manifest
//! declares what it does not call is refused. The capability samples
//! (`guests/sample-greet*`, `guests/sample-capabilities*`) show what
//! authors write; the capabilities fixture (`guests/fixtures/capabilities`)
//! drives the refusals, chains, error kinds and the self-use rule. Real
//! guests from `cargo xtask guests`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::executor::block_on;
use pane_core::clipboard::{Clock as _, ManualClock, SystemClock};
use pane_core::{Launcher, PackageIdentity, Runtime, SavedData, Screen, Status, Unavailable};
use tempfile::TempDir;

#[path = "support/platforms.rs"]
mod platforms;

#[path = "support/npm_registry.rs"]
mod npm_registry;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guest_file as guest;
use rows::select_title;

// The samples: each consumer's items call the capability the providers of
// all three languages provide.

const GREET: &str = "pane-samples:greet@1";
const PROVIDERS: [(&str, &str); 3] = [
    ("sample-greet", "Rust"),
    ("sample-greet-js", "JavaScript"),
    ("sample-greet-ts", "TypeScript"),
];
const CONSUMERS: [(&str, &str); 3] = [
    ("sample-capabilities", "Greet from Rust"),
    ("sample-capabilities-js", "Greet from JavaScript"),
    ("sample-capabilities-ts", "Greet from TypeScript"),
];

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

    fn launcher(&self) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.extensions())
    }

    fn extensions(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    fn identity(&self, name: &str) -> PackageIdentity {
        PackageIdentity::local(&self.folder(name)).unwrap()
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
    select_title(launcher, item);
    block_on(launcher.activate_selected());
}

/// Activates the "Greet through a capability" item of `command`, returning
/// what it showed.
fn greet(launcher: &Launcher, command: &str) -> Status {
    open_item(launcher, command, "Greet through a capability");
    shown(launcher)
}

fn result(text: &str) -> Status {
    Status::Result(text.into())
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

/// A consumer of each language with every provider of each language: the
/// consumer installs with no provider installed, and then calls whichever
/// provider is installed, by the capability's name alone.
#[test]
fn a_consumer_in_each_language_calls_the_provider_in_each_language() {
    for (consumer, command) in CONSUMERS {
        for (provider, language) in PROVIDERS {
            let dirs = Dirs::new();
            let launcher = dirs.launcher();
            // Installing never stops because no provider is installed.
            install(&launcher, &dirs.sample(consumer));
            install(&launcher, &dirs.sample(provider));

            assert_eq!(
                greet(&launcher, command),
                result(&format!("Hello, Pane, from {language}")),
                "{command} calling {provider}"
            );
        }
    }
}

/// A call goes to the first provider in install order, and a later install
/// changes nothing: the user's choice of provider (a later ticket) is the
/// only thing that would.
#[test]
fn a_call_goes_to_the_first_provider_and_a_later_install_changes_nothing() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.sample(CONSUMERS[0].0));
    install(&launcher, &dirs.sample(PROVIDERS[1].0));
    install(&launcher, &dirs.sample(PROVIDERS[0].0));

    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from JavaScript")
    );

    install(&launcher, &dirs.sample(PROVIDERS[2].0));
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from JavaScript")
    );
}

/// The optional capability nobody provides answers `not-found`, and gates
/// nothing: the package installed without it, its command runs, and asking
/// first answers what there is to know.
#[test]
fn an_optional_capability_nobody_provides_is_not_found_and_gates_nothing() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.sample(CONSUMERS[0].0));

    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        error(&format!(
            "not-found: no installed extension provides `{GREET}`"
        ))
    );
    open_item(&launcher, "Greet from Rust", "Say farewell");
    assert_eq!(
        shown(&launcher),
        result("No installed extension provides pane-samples:farewell@1; install one to use it")
    );
    open_item(&launcher, "Greet from Rust", "Who provides the greeting");
    assert_eq!(
        shown(&launcher),
        result(&format!("No installed extension provides {GREET}"))
    );
    open_item(
        &launcher,
        "Greet from Rust",
        "Is the farewell capability available",
    );
    assert_eq!(
        shown(&launcher),
        result("pane-samples:farewell@1 has no provider now")
    );

    // It comes back by itself once a provider is installed.
    install(&launcher, &dirs.sample(PROVIDERS[0].0));
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from Rust")
    );
}

/// The providers query answers the providers that can serve now, in
/// install order, and skips a disabled one.
#[test]
fn the_providers_query_answers_the_available_providers() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.sample(CONSUMERS[0].0));
    install(&launcher, &dirs.sample(PROVIDERS[1].0));
    install(&launcher, &dirs.sample(PROVIDERS[0].0));

    open_item(&launcher, "Greet from Rust", "Who provides the greeting");
    assert_eq!(
        shown(&launcher),
        result(&format!(
            "{GREET} is provided by JavaScript greet provider sample, Rust greet provider sample"
        ))
    );

    block_on(launcher.set_enabled(&dirs.sample_identity(PROVIDERS[1].0), false));
    open_item(&launcher, "Greet from Rust", "Who provides the greeting");
    assert_eq!(
        shown(&launcher),
        result(&format!(
            "{GREET} is provided by Rust greet provider sample"
        ))
    );
}

impl Dirs {
    /// The identity of the sample package copied into source folder
    /// `package`.
    fn sample_identity(&self, package: &str) -> PackageIdentity {
        self.identity(package)
    }
}

/// A disabled provider is skipped while another can serve, and when every
/// provider is disabled the call answers `disabled`, naming them.
#[test]
fn a_disabled_provider_is_skipped_until_none_is_left() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.sample(CONSUMERS[0].0));
    install(&launcher, &dirs.sample(PROVIDERS[1].0));
    install(&launcher, &dirs.sample(PROVIDERS[0].0));

    block_on(launcher.set_enabled(&dirs.sample_identity(PROVIDERS[1].0), false));
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from Rust")
    );

    block_on(launcher.set_enabled(&dirs.sample_identity(PROVIDERS[0].0), false));
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        error(&format!(
            "disabled: every installed extension providing `{GREET}` is disabled (JavaScript \
             greet provider sample and Rust greet provider sample); enable one in Settings"
        ))
    );
}

/// Uninstalling the only provider leaves the call `not-found`.
#[test]
fn an_uninstalled_provider_is_not_found() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.sample(CONSUMERS[0].0));
    install(&launcher, &dirs.sample(PROVIDERS[0].0));
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from Rust")
    );

    block_on(launcher.uninstall(&dirs.sample_identity(PROVIDERS[0].0), SavedData::Delete));
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        error(&format!(
            "not-found: no installed extension provides `{GREET}`"
        ))
    );
}

// The fixture: package `a` has the command; `b` and the rest only provide.

/// The `provides` of a fixture package providing `capability`, as
/// manifest members: `operations` (JSON array contents) and `platforms`
/// (empty for every system).
fn provides_of(capability: &str, operations: &str, platforms: &str) -> String {
    format!(
        r#","provides": [{{ "capability": "{capability}", "component": "fixture.wasm",
             "operations": [{operations}]{platforms} }}]"#
    )
}

/// The `provides` of a fixture package providing the fixture's capability,
/// as manifest members: `operations` (JSON array contents) and `platforms`
/// (empty for every system).
fn provides(operations: &str, platforms: &str) -> String {
    provides_of("fixture:greet@1", operations, platforms)
}

/// The `uses` of a fixture package using `capability`, as manifest members.
fn uses_of(capability: &str, operations: &str) -> String {
    format!(r#","uses": [{{ "capability": "{capability}", "operations": [{operations}] }}]"#)
}

/// The `uses` of a fixture package using the fixture's capability, as
/// manifest members.
fn uses(operations: &str) -> String {
    uses_of("fixture:greet@1", operations)
}

/// The operations the fixture's capability is made of.
const PROVIDED: &str = r#""greet", "forward", "crash""#;
/// The operations a using fixture package declares it calls.
const USED: &str = r#""greet", "forward", "crash""#;

/// The fixture's command, as `a` lists it.
const COMMAND: &str =
    r#"{ "id": "fixture", "title": "Capabilities fixture", "component": "fixture.wasm" }"#;

impl Dirs {
    /// Writes a fixture package in source folder `name`, titled
    /// "Package <name>", with `commands` (JSON array contents) and
    /// `members` (manifest members, each starting with a comma): its
    /// `provides`, `uses`, `operations` or `dependencies`, and the fixture
    /// component as `fixture.wasm`.
    fn fixture(&self, name: &str, commands: &str, members: &str) -> PathBuf {
        let folder = self.folder(name);
        fs::create_dir_all(&folder).unwrap();
        fs::copy(
            guest("capabilities_fixture.wasm"),
            folder.join("fixture.wasm"),
        )
        .unwrap();
        let manifest = format!(
            r#"{{
                "manifestVersion": 1,
                "title": "Package {name}",
                "apiVersion": "0.1",
                "commands": [{commands}]{members}
            }}"#
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        folder
    }

    /// Saves, as `a`'s settings, the sources the fixture calls by name:
    /// those of `names`. Before any launcher opens the data folder.
    fn save_sources(&self, names: &[&str]) {
        let mut sources = serde_json::Map::new();
        for name in names {
            sources.insert((*name).into(), self.identity(name).key().into());
        }
        let settings = serde_json::json!({
            "version": 1,
            "packages": {
                self.identity("a").key(): {
                    "sources": serde_json::Value::Object(sources).to_string()
                }
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

    /// `a` using the capability and `b` providing it, both installed.
    fn a_and_b(&self) -> Launcher {
        self.fixture("a", COMMAND, &uses(USED));
        self.fixture("b", "", &provides(PROVIDED, ""));
        self.install_fixtures(&["a", "b"])
    }
}

/// Runs the fixture command's item `item` from package `a`, returning
/// what it showed.
fn fixture_run(launcher: &Launcher, item: &str) -> Status {
    open_item(launcher, "Capabilities fixture", item);
    shown(launcher)
}

/// A package that provides and uses one capability is never routed to
/// itself: it gets another provider — and alone, it is told so.
#[test]
fn a_package_is_never_routed_to_its_own_capability() {
    let dirs = Dirs::new();
    // Both `a` and `b` provide the capability `a` uses; only `b` can serve
    // `a`'s call, and the answer carries the qualified operation the
    // provider was called with.
    dirs.fixture(
        "a",
        COMMAND,
        &format!("{}{}", provides(PROVIDED, ""), uses(USED)),
    );
    dirs.fixture("b", "", &provides(PROVIDED, ""));
    let launcher = dirs.install_fixtures(&["a", "b"]);

    assert_eq!(
        fixture_run(&launcher, "Call the greet capability"),
        result(r#"answered: {"greeting":"Hello, Ada","operation":"fixture:greet@1/greet"}"#)
    );
    // `a`'s own package would be refused as already serving the call, so
    // the answer can only have come from `b`.
    let running = block_on(dirs.runtime.running());
    assert_eq!(running.len(), 2, "{running:?}");

    // Alone, `a` provides what it uses: a package never serves its own
    // use, so it waits for another provider, and Enter offers to install
    // one.
    let alone = Dirs::new();
    alone.fixture(
        "a",
        COMMAND,
        &format!("{}{}", provides(PROVIDED, ""), uses(USED)),
    );
    let launcher = alone.install_fixtures(&["a"]);
    assert_eq!(
        waiting_reason(&launcher, "Capabilities fixture"),
        "Needs fixture:greet@1: no extension provides it"
    );
    open(&launcher, "Capabilities fixture");
    assert!(matches!(
        launcher.view().screen,
        Screen::WaitingDetails { .. }
    ));
    assert_eq!(
        rows::titles(&launcher),
        ["Install an extension that provides fixture:greet@1"]
    );
    // Installing another provider brings it back by itself, without
    // touching `a`.
    alone.fixture("b", "", &provides(PROVIDED, ""));
    install(&launcher, &alone.folder("b"));
    launcher.show_root_search();
    assert_eq!(row(&launcher, "Capabilities fixture").unavailable, None);
    assert_eq!(
        fixture_run(&launcher, "Call the greet capability"),
        result(r#"answered: {"greeting":"Hello, Ada","operation":"fixture:greet@1/greet"}"#)
    );
}

/// From root search, activates the row titled `title`: opening a command,
/// or, for a waiting one, showing its reason with the row that fixes it.
fn open(launcher: &Launcher, title: &str) {
    rows::to_root(launcher);
    rows::select_title(launcher, title);
    block_on(launcher.activate_selected());
}

/// The root search row titled `title`.
fn row(launcher: &Launcher, title: &str) -> pane_core::Row {
    let view = launcher.view();
    view.rows
        .into_iter()
        .find(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", rows::titles(launcher)))
}

/// Why the command titled `title` cannot run, as a waiting reason.
fn waiting_reason(launcher: &Launcher, title: &str) -> String {
    let reason = row(launcher, title)
        .unavailable
        .unwrap_or_else(|| panic!("{title:?} is available"));
    let Unavailable::Waiting(reason) = reason else {
        panic!("expected {title:?} to wait, got {reason:?}");
    };
    reason
}

/// A call to a capability or an operation the caller's manifest does not
/// declare is refused, with the message saying to declare it.
#[test]
fn undeclared_capabilities_and_operations_are_refused() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    assert_eq!(
        fixture_run(&launcher, "Call an undeclared capability"),
        error(
            "refused: Package a declares no use of `fixture:undeclared@1` in its pane.json; \
             declare it in `uses` to call it"
        )
    );
    assert_eq!(
        fixture_run(
            &launcher,
            "Call an undeclared operation of the greet capability"
        ),
        error(
            "refused: Package a declares that it calls `greet`, `forward` and `crash` through \
             `fixture:greet@1`, not `unknown`; declare it in its pane.json to call it"
        )
    );
}

/// A capability's operations are reached only through the capability: by
/// identity, even qualified as the capability calls them, they are not
/// published operations.
#[test]
fn a_capability_s_operations_are_not_reachable_by_identity() {
    let dirs = Dirs::new();
    dirs.fixture("a", COMMAND, &uses(USED));
    dirs.fixture("b", "", &provides(PROVIDED, ""));
    let launcher = dirs.install_fixtures(&["a", "b"]);

    assert_eq!(
        fixture_run(&launcher, "Call b's capability operation by identity"),
        error(
            "not-found: Package b does not publish an operation `fixture:greet@1/greet`; it \
             publishes no operations"
        )
    );
}

/// The operation's own error and a crash reach the caller as any call's do.
#[test]
fn the_operation_s_own_error_and_a_crash_reach_the_caller() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    assert_eq!(
        fixture_run(&launcher, "Call the greet capability with no name"),
        error("failed: a name is needed")
    );
    assert_error_starts(
        &fixture_run(&launcher, "Call the greet capability's crash"),
        "crashed: Package b crashed:",
    );
    // The caller keeps working, and the provider starts afresh.
    assert_eq!(
        fixture_run(&launcher, "Call the greet capability"),
        result(r#"answered: {"greeting":"Hello, Ada","operation":"fixture:greet@1/greet"}"#)
    );
}

/// A provider Pane paused after it failed leaves its consumer waiting for
/// the capability, and Enter offers to retry it: the chain's root cause is
/// what the fix row fixes.
#[test]
fn a_paused_provider_leaves_its_consumer_waiting_and_retry_fixes_it() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    for _ in 0..3 {
        assert_error_starts(
            &fixture_run(&launcher, "Call the greet capability's crash"),
            "crashed: Package b crashed:",
        );
    }
    // `b` is paused after its crashes: no provider can serve the
    // capability, so `a` waits for it.
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, "Capabilities fixture"),
        "Needs fixture:greet@1: Package b is paused"
    );
    open(&launcher, "Capabilities fixture");
    assert_eq!(rows::titles(&launcher), ["Retry Package b"]);

    // The fix row retries the provider; the consumer comes back by itself.
    rows::select_title(&launcher, "Retry Package b");
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Started Package b".into())
    );
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert_eq!(row(&launcher, "Capabilities fixture").unavailable, None);
    assert_eq!(
        fixture_run(&launcher, "Call the greet capability"),
        result(r#"answered: {"greeting":"Hello, Ada","operation":"fixture:greet@1/greet"}"#)
    );
}

/// A provider waiting for what it needs leaves its consumer waiting in
/// turn, and the chain names what is actually missing: the root cause the
/// fix row fixes.
#[test]
fn a_waiting_provider_leaves_its_consumer_waiting_and_the_chain_names_the_root() {
    let dirs = Dirs::new();
    // `b` provides the capability and requires `c`, whose operations it
    // never calls.
    dirs.fixture("a", COMMAND, &uses(USED));
    let dependency = r#","dependencies": [
        { "id": "dep", "source": "local:../c",
          "operations": [{ "id": "echo", "version": 1 }] }
    ]"#;
    dirs.fixture(
        "b",
        "",
        &format!("{}{}", provides(PROVIDED, ""), dependency),
    );
    dirs.fixture(
        "c",
        "",
        r#","operations": [{ "id": "echo", "version": 1, "component": "fixture.wasm" }]"#,
    );
    let launcher = dirs.install_fixtures(&["a", "b"]);

    assert_eq!(
        fixture_run(&launcher, "Call the greet capability"),
        result(r#"answered: {"greeting":"Hello, Ada","operation":"fixture:greet@1/greet"}"#)
    );

    // `c` disabled, `b` waits for it: no provider can serve the capability,
    // so `a` waits for it, naming what is actually missing.
    block_on(launcher.set_enabled(&dirs.identity("c"), false));
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, "Capabilities fixture"),
        "Needs fixture:greet@1: Package b, which waits for Package c: Package c is disabled"
    );
    // The fix is for the root cause, not the provider that waits for it.
    open(&launcher, "Capabilities fixture");
    assert_eq!(rows::titles(&launcher), ["Enable Package c"]);

    block_on(launcher.set_enabled(&dirs.identity("c"), true));
    launcher.show_root_search();
    assert_eq!(row(&launcher, "Capabilities fixture").unavailable, None);
    assert_eq!(
        fixture_run(&launcher, "Call the greet capability"),
        result(r#"answered: {"greeting":"Hello, Ada","operation":"fixture:greet@1/greet"}"#)
    );
}

/// A provider whose capability works only on other systems cannot serve
/// here, so its consumer waits for the capability, and Enter offers to
/// install one that can.
#[test]
fn a_provider_for_other_systems_leaves_its_consumer_waiting() {
    let [other, _] = platforms::other_systems();
    let dirs = Dirs::new();
    dirs.fixture("a", COMMAND, &uses(USED));
    dirs.fixture(
        "b",
        "",
        &provides(PROVIDED, &format!(r#", "platforms": ["{}"]"#, other.id())),
    );
    let launcher = dirs.install_fixtures(&["a", "b"]);

    assert_eq!(
        waiting_reason(&launcher, "Capabilities fixture"),
        format!(
            "Needs fixture:greet@1: Package b: {}",
            platforms::only("this capability", platforms::name(other))
        )
    );
    open(&launcher, "Capabilities fixture");
    assert_eq!(
        rows::titles(&launcher),
        ["Install an extension that provides fixture:greet@1"]
    );
}

/// The providers query answers the available providers and an empty list
/// for a capability no one provides.
#[test]
fn the_fixture_s_providers_query_answers_the_available_providers() {
    let dirs = Dirs::new();
    let launcher = dirs.a_and_b();

    assert_eq!(
        fixture_run(&launcher, "Ask who provides the greet capability"),
        result("the fixture:greet@1 capability is provided by Package b")
    );
    assert_eq!(
        fixture_run(&launcher, "Ask who provides the farewell capability"),
        result("the fixture:farewell@1 capability has no provider")
    );

    // `b` disabled leaves no provider that can serve: the query answers
    // none, and `a`'s command waits for the capability rather than calling
    // into an empty list — the use is one provider.
    block_on(launcher.set_enabled(&dirs.identity("b"), false));
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, "Capabilities fixture"),
        "Needs fixture:greet@1: Package b is disabled"
    );

    // Enabled again: the query answers the provider, and `a` runs.
    block_on(launcher.set_enabled(&dirs.identity("b"), true));
    launcher.show_root_search();
    assert_eq!(row(&launcher, "Capabilities fixture").unavailable, None);
    assert_eq!(
        fixture_run(&launcher, "Ask who provides the greet capability"),
        result("the fixture:greet@1 capability is provided by Package b")
    );
}

/// A provider already in the chain is refused, across capability calls.
#[test]
fn a_provider_already_in_the_chain_is_refused() {
    let dirs = Dirs::new();
    // `a` provides the capability too, so `b`'s call back through the
    // capability is routed to it: already serving the chain.
    dirs.fixture(
        "a",
        COMMAND,
        &format!("{}{}", provides(PROVIDED, ""), uses(USED)),
    );
    dirs.fixture(
        "b",
        "",
        &format!("{}{}", provides(PROVIDED, ""), uses(USED)),
    );
    let launcher = dirs.install_fixtures(&["a", "b"]);

    assert_eq!(
        fixture_run(
            &launcher,
            "Call the greet capability, whose provider calls back"
        ),
        error(
            "failed: refused: Package a is already serving a call in this chain; an extension \
             cannot be called back while its own call waits"
        )
    );
    // Both keep working.
    assert_eq!(
        fixture_run(&launcher, "Call the greet capability"),
        result(r#"answered: {"greeting":"Hello, Ada","operation":"fixture:greet@1/greet"}"#)
    );
}

/// A capability call past the depth limit is refused: the chain of
/// identity calls ends in one, and the ninth call is one too many.
#[test]
fn a_capability_call_past_the_depth_limit_is_refused() {
    let dirs = Dirs::new();
    // `a` hops `p1` to `p7` by their sources, each serving `forward`; `p7`
    // calls the capability, with the chain already eight deep.
    let members = format!(
        r#","operations": [{{ "id": "forward", "version": 1, "component": "fixture.wasm" }}]{}{}"#,
        provides(PROVIDED, ""),
        uses(USED)
    );
    dirs.fixture("a", COMMAND, &uses(USED));
    let mut names = vec!["a".to_owned()];
    for index in 1..=7 {
        let name = format!("p{index}");
        dirs.fixture(&name, "", &members);
        names.push(name);
    }
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let launcher = dirs.install_fixtures(&names);

    let status = fixture_run(&launcher, "Call a chain whose last call is a capability");
    assert!(
        matches!(&status, Status::Error(text) if text.ends_with(
            "refused: the chain of calls is 8 deep; Pane allows at most 8"
        )),
        "{status:?}"
    );
    assert_eq!(pane_core::MAX_CALL_DEPTH, 8);
}

// The manifest: what installing refuses, with the reason.

impl Dirs {
    /// Writes a one-command package in source folder `a` around the
    /// capabilities fixture component, with `members` (manifest members,
    /// each starting with a comma), previews it, and returns the error it
    /// was refused with.
    fn refused(&self, members: &str) -> String {
        let folder = self.fixture("a", COMMAND, members);
        let launcher = self.launcher();
        block_on(launcher.preview_package(&folder));
        match &launcher.view().status {
            Status::Error(text) => text.clone(),
            status => panic!("not refused: {status:?}"),
        }
    }

    /// The `provides` of a package providing `capability` — any spelling,
    /// for the malformed ones — as manifest members.
    fn providing(&self, capability: &str) -> String {
        format!(
            r#","provides": [{{ "capability": "{capability}", "component": "fixture.wasm",
                 "operations": ["greet"] }}]"#
        )
    }
}

#[test]
fn a_malformed_capability_name_is_refused() {
    for (name, reason) in [
        (
            "translate",
            "it has no `:` separating the namespace from the name",
        ),
        (
            "acme/translate@1",
            "it has no `:` separating the namespace from the name",
        ),
        ("acme:translate", "it has no `@` and a major version"),
        (
            "acme:translate@0",
            "its major version `0` is not a number from 1",
        ),
        (
            "acme:translate@x",
            "its major version `x` is not a number from 1",
        ),
        (
            "Acme:translate@1",
            "its namespace `Acme` must be lowercase letters, digits and `-`",
        ),
        (
            "acme:TransLate@1",
            "its name `TransLate` must be lowercase letters, digits and `-`",
        ),
        (
            "acme:@1",
            "its name `` must be lowercase letters, digits and `-`",
        ),
    ] {
        let dirs = Dirs::new();
        // Both `provides` and `uses` read the name.
        let uses = format!(r#","uses": [{{ "capability": "{name}", "operations": ["greet"] }}]"#);
        let members = format!("{}\n{}", dirs.providing(name), uses);
        let message = dirs.refused(&members);
        assert!(
            message.starts_with(&format!(
                "Invalid pane.json: the capability name `{name}` is malformed"
            )),
            "{name}: {message}"
        );
        assert!(message.contains(reason), "{name}: {message}");
    }
}

#[test]
fn an_operation_listed_twice_is_refused() {
    for (members, what) in [
        (
            provides(r#""greet", "greet""#, ""),
            "the operation `greet` of the capability `fixture:greet@1` is listed twice",
        ),
        (
            uses(r#""greet", "greet""#),
            "the operation `greet` of the use of `fixture:greet@1` is listed twice",
        ),
    ] {
        let dirs = Dirs::new();
        let message = dirs.refused(&members);
        assert!(
            message.starts_with(&format!("Invalid pane.json: {what}")),
            "{message}"
        );
    }
}

#[test]
fn the_same_capability_provided_twice_at_one_major_is_refused() {
    let dirs = Dirs::new();
    let members = r#","provides": [
        { "capability": "fixture:greet@1", "component": "fixture.wasm", "operations": ["greet"] },
        { "capability": "fixture:greet@1", "component": "fixture.wasm", "operations": ["greet"] }
    ]"#
    .to_string();
    let message = dirs.refused(&members);
    assert_eq!(
        message,
        "Invalid pane.json: the capability `fixture:greet@1` is provided twice; declare each \
         capability, at each major version, once"
    );
}

#[test]
fn a_missing_component_is_refused() {
    let dirs = Dirs::new();
    // A package that provides only, so the missing component is named by
    // the capability it serves.
    let folder = dirs.fixture("a", "", &provides(PROVIDED, ""));
    fs::remove_file(folder.join("fixture.wasm")).unwrap();
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&folder));

    let message = match &launcher.view().status {
        Status::Error(text) => text.clone(),
        status => panic!("not refused: {status:?}"),
    };
    assert!(
        message.starts_with(
            "Not ready to run: the component fixture.wasm of \"capability `fixture:greet@1`\" \
             is missing"
        ),
        "{message}"
    );
}

#[test]
fn a_component_without_the_operations_export_is_refused() {
    let dirs = Dirs::new();
    // The Rust sample's component exports no `published-operations`.
    let folder = dirs.folder("a");
    fs::create_dir_all(&folder).unwrap();
    fs::copy(guest("sample_rust.wasm"), folder.join("fixture.wasm")).unwrap();
    fs::write(
        folder.join("pane.json"),
        format!(
            r#"{{
                "manifestVersion": 1,
                "title": "Package a",
                "apiVersion": "0.1",
                "commands": [{COMMAND}],
                "provides": [
                    {{ "capability": "fixture:greet@1", "component": "fixture.wasm",
                       "operations": ["greet"] }}
                ]
            }}"#
        ),
    )
    .unwrap();

    let launcher = dirs.launcher();
    block_on(launcher.preview_package(&folder));

    let message = match &launcher.view().status {
        Status::Error(text) => text.clone(),
        status => panic!("not refused: {status:?}"),
    };
    assert!(
        message.contains(
            "its manifest publishes operations it serves, but it does not export \
             pane:extension/published-operations"
        ),
        "{message}"
    );
}

#[test]
fn commands_naming_an_unknown_command_are_refused() {
    let dirs = Dirs::new();
    let members = r#","uses": [{ "capability": "fixture:greet@1", "operations": ["greet"],
         "commands": ["no-such-command"] }]"#
        .to_string();
    let message = dirs.refused(&members);
    assert_eq!(
        message,
        "Invalid pane.json: the `commands` of the use of `fixture:greet@1` names \
         `no-such-command`, which the package does not declare as a command"
    );
}

#[test]
fn a_default_that_is_not_a_source_is_refused() {
    for default in ["../provider", "svn:example.org/a/b"] {
        let dirs = Dirs::new();
        let members = format!(
            r#","uses": [{{ "capability": "fixture:greet@1", "operations": ["greet"],
                 "default": "{default}" }}]"#
        );
        let message = dirs.refused(&members);
        assert!(
            message.starts_with(&format!(
                "Invalid pane.json: the default `{default}` of the use of `fixture:greet@1` \
                 must be `local:` followed by a folder path"
            )),
            "{message}"
        );
    }
}

#[test]
fn an_unknown_use_mode_is_refused() {
    let dirs = Dirs::new();
    let members = r#","uses": [ { "capability": "fixture:greet@1", "operations": ["greet"],
             "use": "both" } ]"#;
    let message = dirs.refused(members);
    assert_eq!(
        message,
        "Invalid pane.json: the use of `fixture:greet@1` has \"use\": \"both\"; `use` is \
         \"one\" (the default: call one provider) or \"all\" (call every provider)"
    );
}

/// A package published to npm cannot name a `local:` folder as a use's
/// default: its folder is on its author's computer, not the user's.
#[test]
fn a_default_a_published_package_cannot_name_is_refused() {
    use npm_registry::Registry;

    let dirs = Dirs::new();
    let registry = Registry::start();
    let name = "@pane-samples/capability-user";
    let pane_json = r#"{
        "manifestVersion": 1,
        "title": "Capability user",
        "apiVersion": "0.1",
        "commands": [
            { "id": "greet", "title": "Greet through a capability",
              "component": "sample_npm_js.wasm" }
        ],
        "uses": [
            { "capability": "pane-samples:greet@1", "operations": ["greet"],
              "default": "local:../provider" }
        ]
    }"#;
    let package_json = format!(
        r#"{{ "name": "{name}", "version": "0.1.0", "private": true,
             "license": "Apache-2.0 OR MIT" }}"#
    );
    let files = vec![
        ("package.json", package_json.into_bytes()),
        ("pane.json", pane_json.as_bytes().to_vec()),
        (
            "sample_npm_js.wasm",
            fs::read(guest("sample_npm_js.wasm")).unwrap(),
        ),
    ];
    registry.publish(name, "0.1.0", npm_registry::pack(&files));
    let launcher = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.extensions())
        .with_npm_registry(pane_core::npm::Registry::local(registry.url()).unwrap());

    block_on(launcher.preview_npm(name));

    let message = match &launcher.view().status {
        Status::Error(text) => text.clone(),
        status => panic!("not refused: {status:?}"),
    };
    assert_eq!(
        message,
        "Invalid pane.json: the default `local:../provider` of the use of \
         `pane-samples:greet@1` names a local folder, but a package from npm can name only \
         `npm:` and `git:` sources, whose packages are published, not a folder on its \
         author's computer"
    );
}

// The provider choice (#154): Pane's own record of which provider serves
// each capability, kept across restarts, reloads and updates, forgotten
// when the chosen provider is uninstalled, with the first provider
// installed serving until the user chooses and calls falling back to the
// next provider that can serve while the chosen one cannot.

impl Dirs {
    /// The record of the chosen providers, beside `installed.json`.
    fn choices_file(&self) -> PathBuf {
        self.extensions().join("capability-choices.json")
    }

    /// Chooses the sample `provider` as the provider of `GREET`, as
    /// Settings' dropdown does, waiting for the record: read back parsed,
    /// since a Windows path's backslashes are escaped in the JSON.
    fn choose(&self, launcher: &Launcher, provider: &str) {
        let key = self.sample_identity(provider).key();
        let chosen = launcher.choose_provider(GREET, &key).unwrap();
        block_on(chosen).unwrap();
        let recorded = fs::read_to_string(self.choices_file()).unwrap();
        let read: serde_json::Value = serde_json::from_str(&recorded).unwrap();
        assert_eq!(
            read["chosen"][GREET].as_str(),
            Some(key.as_str()),
            "{provider} is recorded: {recorded}"
        );
    }
}

/// A provider the user has not chosen cannot be recorded, with the reason.
#[test]
fn a_provider_that_is_not_one_is_refused() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.sample(PROVIDERS[1].0));
    let js = dirs.sample_identity(PROVIDERS[1].0).key();
    let refused = launcher
        .choose_provider(GREET, "local:/nowhere")
        .err()
        .expect("not a provider of the capability");
    assert_eq!(
        refused,
        format!(
            "`local:/nowhere` does not provide `{GREET}`: the installed extensions that provide \
             it are JavaScript greet provider sample"
        )
    );
    let refused = launcher
        .choose_provider("pane-samples:farewell@1", &js)
        .err()
        .expect("no one provides the capability");
    assert_eq!(
        refused,
        format!(
            "`{js}` does not provide `pane-samples:farewell@1`: no installed extension provides \
             `pane-samples:farewell@1`"
        )
    );
    assert!(!dirs.choices_file().exists());
}

/// Choosing a provider routes the next call to it, and a later install
/// changes nothing: the choice is Pane's own record, never extension data,
/// and nothing is reloaded or restarted for it.
#[test]
fn choosing_a_provider_routes_the_next_call_to_it() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.sample(CONSUMERS[0].0));
    install(&launcher, &dirs.sample(PROVIDERS[1].0));
    install(&launcher, &dirs.sample(PROVIDERS[2].0));
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from JavaScript")
    );

    dirs.choose(&launcher, PROVIDERS[2].0);
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from TypeScript")
    );
    // A later install changes nothing, choice or not.
    install(&launcher, &dirs.sample(PROVIDERS[0].0));
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from TypeScript")
    );
}

/// The choice is kept across a restart of the launcher on the same data
/// folder, and across a reload of the chosen provider: a replacement
/// keeps the package's identity, which the record names.
#[test]
fn the_choice_is_kept_across_a_restart_and_a_reload() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.sample(CONSUMERS[0].0));
    install(&launcher, &dirs.sample(PROVIDERS[1].0));
    install(&launcher, &dirs.sample(PROVIDERS[2].0));
    dirs.choose(&launcher, PROVIDERS[2].0);

    // A restart of the launcher, with a runtime of its own.
    let restarted =
        Launcher::with_packages(Ok(Runtime::start().unwrap()), vec![], dirs.extensions());
    assert_eq!(
        greet(&restarted, "Greet from Rust"),
        result("Hello, Pane, from TypeScript")
    );

    // A reload of the chosen provider replaces its code, not its identity.
    block_on(restarted.reload(&dirs.sample_identity(PROVIDERS[2].0)));
    assert_eq!(
        restarted.view().status,
        Status::Result("Reloaded TypeScript greet provider sample".into())
    );
    assert_eq!(
        greet(&restarted, "Greet from Rust"),
        result("Hello, Pane, from TypeScript")
    );
}

/// While the chosen provider is disabled, calls fall back to the next
/// available provider in install order, and return to the chosen one when
/// it is enabled again.
#[test]
fn the_choice_falls_back_while_the_chosen_provider_is_disabled() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.sample(CONSUMERS[0].0));
    install(&launcher, &dirs.sample(PROVIDERS[1].0));
    install(&launcher, &dirs.sample(PROVIDERS[2].0));
    install(&launcher, &dirs.sample(PROVIDERS[0].0));
    dirs.choose(&launcher, PROVIDERS[2].0);
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from TypeScript")
    );

    block_on(launcher.set_enabled(&dirs.sample_identity(PROVIDERS[2].0), false));
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from JavaScript")
    );
    block_on(launcher.set_enabled(&dirs.sample_identity(PROVIDERS[2].0), true));
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from TypeScript")
    );
}

/// The providers query answers the chosen provider first, and leaves it
/// out while it cannot serve.
#[test]
fn the_providers_query_answers_the_chosen_provider_first() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.sample(CONSUMERS[0].0));
    install(&launcher, &dirs.sample(PROVIDERS[1].0));
    install(&launcher, &dirs.sample(PROVIDERS[2].0));
    dirs.choose(&launcher, PROVIDERS[2].0);

    open_item(&launcher, "Greet from Rust", "Who provides the greeting");
    assert_eq!(
        shown(&launcher),
        result(&format!(
            "{GREET} is provided by TypeScript greet provider sample, JavaScript greet provider \
             sample"
        ))
    );

    // The chosen provider cannot serve: it is left out, and the rest stay
    // in install order.
    block_on(launcher.set_enabled(&dirs.sample_identity(PROVIDERS[2].0), false));
    open_item(&launcher, "Greet from Rust", "Who provides the greeting");
    assert_eq!(
        shown(&launcher),
        result(&format!(
            "{GREET} is provided by JavaScript greet provider sample"
        ))
    );
}

/// Uninstalling the chosen provider forgets the choice, so calls go to the
/// default order; uninstalling another provider keeps it.
#[test]
fn uninstalling_the_chosen_provider_forgets_the_choice() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.sample(CONSUMERS[0].0));
    install(&launcher, &dirs.sample(PROVIDERS[1].0));
    install(&launcher, &dirs.sample(PROVIDERS[2].0));
    install(&launcher, &dirs.sample(PROVIDERS[0].0));
    dirs.choose(&launcher, PROVIDERS[2].0);

    // Not the chosen one: the choice stays.
    block_on(launcher.uninstall(&dirs.sample_identity(PROVIDERS[1].0), SavedData::Delete));
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from TypeScript")
    );

    // The chosen one: the choice goes with it, and the first provider
    // installed among the rest serves.
    block_on(launcher.uninstall(&dirs.sample_identity(PROVIDERS[2].0), SavedData::Delete));
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from Rust")
    );
    let recorded = fs::read_to_string(dirs.choices_file()).unwrap();
    assert!(
        !recorded.contains("pane-samples:greet"),
        "the choice is forgotten: {recorded}"
    );
}

/// What Settings' Capabilities section reads: only capabilities with two
/// or more installed providers, their providers and the provider the next
/// call goes to, their consumers, and the fallback note while the chosen
/// provider cannot serve.
#[test]
fn the_capabilities_listing_shows_two_providers_consumers_and_the_fallback() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    install(&launcher, &dirs.sample(CONSUMERS[1].0));
    install(&launcher, &dirs.sample(PROVIDERS[1].0));
    assert!(launcher.capabilities().is_empty());

    install(&launcher, &dirs.sample(PROVIDERS[2].0));
    let js = dirs.sample_identity(PROVIDERS[1].0).key();
    let ts = dirs.sample_identity(PROVIDERS[2].0).key();
    let listed = launcher.capabilities();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(
        listed[0],
        pane_core::Capability {
            name: GREET.to_owned(),
            providers: vec![
                pane_core::CapabilityProvider {
                    source: js.clone(),
                    title: "JavaScript greet provider sample".into(),
                },
                pane_core::CapabilityProvider {
                    source: ts.clone(),
                    title: "TypeScript greet provider sample".into(),
                },
            ],
            selected: js,
            consumers: vec!["JavaScript capabilities sample".to_owned()],
            fallback: None,
        }
    );

    // A choice: the next call goes to it.
    dirs.choose(&launcher, PROVIDERS[2].0);
    let listed = launcher.capabilities();
    assert_eq!(listed[0].selected, ts);

    // The chosen provider cannot serve: the note says who serves instead,
    // and goes when it serves again.
    block_on(launcher.set_enabled(&dirs.sample_identity(PROVIDERS[2].0), false));
    let listed = launcher.capabilities();
    assert_eq!(
        listed[0].fallback,
        Some(
            "TypeScript greet provider sample is disabled; using JavaScript greet provider sample"
                .into()
        )
    );
    block_on(launcher.set_enabled(&dirs.sample_identity(PROVIDERS[2].0), true));
    assert_eq!(launcher.capabilities()[0].fallback, None);
}

/// While the chosen provider is paused, calls fall back to the next
/// available provider in install order, and return to the chosen one when
/// it is retried. `b` and `c` both provide the fixture's capability and
/// answer alike, so who served is told by which instance Pane started.
#[test]
fn the_choice_falls_back_while_the_chosen_provider_is_paused() {
    let dirs = Dirs::new();
    dirs.fixture("a", COMMAND, &uses(USED));
    dirs.fixture("b", "", &provides(PROVIDED, ""));
    dirs.fixture("c", "", &provides(PROVIDED, ""));
    let launcher = dirs.install_fixtures(&["a", "b", "c"]);
    let chosen = launcher
        .choose_provider("fixture:greet@1", &dirs.identity("c").key())
        .unwrap();
    block_on(chosen).unwrap();
    assert_eq!(
        fixture_run(&launcher, "Call the greet capability"),
        result(r#"answered: {"greeting":"Hello, Ada","operation":"fixture:greet@1/greet"}"#)
    );
    // The chosen provider served: `b`'s instance was never started.
    let running = block_on(dirs.runtime.running());
    assert!(
        !running.contains(&component_of(&launcher, &dirs.identity("b"))),
        "{running:?}"
    );

    // Pane pauses the chosen provider after it crashes three times.
    for _ in 0..3 {
        assert_error_starts(
            &fixture_run(&launcher, "Call the greet capability's crash"),
            "crashed: Package c crashed:",
        );
    }

    // The call falls back to `b`, the next provider in install order: its
    // instance starts and the answer comes from it.
    assert_eq!(
        fixture_run(&launcher, "Call the greet capability"),
        result(r#"answered: {"greeting":"Hello, Ada","operation":"fixture:greet@1/greet"}"#)
    );
    let running = block_on(dirs.runtime.running());
    assert!(
        running.contains(&component_of(&launcher, &dirs.identity("b"))),
        "{running:?}"
    );
    let listed = launcher.capabilities();
    assert_eq!(
        listed[0].fallback,
        Some("Package c is paused after an error; using Package b".into())
    );

    // Retried, the chosen provider serves again, first in the query.
    block_on(launcher.retry_start(&dirs.identity("c")));
    assert_eq!(
        fixture_run(&launcher, "Ask who provides the greet capability"),
        result("the fixture:greet@1 capability is provided by Package c, Package b")
    );
}

/// While the chosen provider waits for what it needs, calls fall back too,
/// with the note naming what it waits for.
#[test]
fn the_choice_falls_back_while_the_chosen_provider_waits() {
    let dirs = Dirs::new();
    // `c` provides the capability beside `b` and requires `d`.
    dirs.fixture("a", COMMAND, &uses(USED));
    dirs.fixture("b", "", &provides(PROVIDED, ""));
    let dependency = r#","dependencies": [
        { "id": "dep", "source": "local:../d",
          "operations": [{ "id": "echo", "version": 1 }] }
    ]"#;
    dirs.fixture(
        "c",
        "",
        &format!("{}{}", provides(PROVIDED, ""), dependency),
    );
    dirs.fixture(
        "d",
        "",
        r#","operations": [{ "id": "echo", "version": 1, "component": "fixture.wasm" }]"#,
    );
    let launcher = dirs.install_fixtures(&["a", "b", "c"]);
    let chosen = launcher
        .choose_provider("fixture:greet@1", &dirs.identity("c").key())
        .unwrap();
    block_on(chosen).unwrap();
    assert_eq!(
        fixture_run(&launcher, "Call the greet capability"),
        result(r#"answered: {"greeting":"Hello, Ada","operation":"fixture:greet@1/greet"}"#)
    );

    // `d` disabled: `c` waits for it, and the call falls back to `b`, with
    // the note naming what the chosen provider waits for.
    block_on(launcher.set_enabled(&dirs.identity("d"), false));
    assert_eq!(
        fixture_run(&launcher, "Call the greet capability"),
        result(r#"answered: {"greeting":"Hello, Ada","operation":"fixture:greet@1/greet"}"#)
    );
    let listed = launcher.capabilities();
    assert_eq!(
        listed[0].fallback,
        Some("Package c is waiting for Package d, which is disabled; using Package b".into())
    );
}

/// The fixture component of the package with `identity`, where its
/// instances run from.
fn component_of(launcher: &Launcher, identity: &PackageIdentity) -> PathBuf {
    launcher
        .packages()
        .into_iter()
        .find(|package| package.identity == *identity)
        .expect("the package is installed")
        .location
        .join("fixture.wasm")
}

/// The choice is kept across an update of the chosen provider: an npm
/// provider of the capability, chosen beside the TypeScript one, updates
/// by itself to a newer version, and the chosen provider — the same
/// identity — still serves.
#[test]
fn the_choice_is_kept_across_an_update_of_the_chosen_provider() {
    use npm_registry::Registry;

    let dirs = Dirs::new();
    let registry = Registry::start();
    let name = "@pane-samples/greet-provider";
    let provider = |version: &str| {
        let pane_json = format!(
            r#"{{
                "manifestVersion": 1,
                "title": "npm greet provider",
                "version": "{version}",
                "apiVersion": "0.1",
                "commands": [
                    {{ "id": "greet", "title": "npm greet provider",
                       "component": "sample_greet_js.wasm" }}
                ],
                "provides": [
                    {{ "capability": "pane-samples:greet@1", "component": "sample_greet_js.wasm",
                       "operations": ["greet"] }}
                ]
            }}"#
        );
        let package_json = format!(
            r#"{{ "name": "{name}", "version": "{version}", "private": true,
                 "license": "Apache-2.0 OR MIT" }}"#
        );
        vec![
            ("package.json", package_json.into_bytes()),
            ("pane.json", pane_json.into_bytes()),
            (
                "sample_greet_js.wasm",
                fs::read(
                    guest("packages")
                        .join("sample-greet-js")
                        .join("sample_greet_js.wasm"),
                )
                .unwrap(),
            ),
        ]
    };
    let clock = ManualClock::at(SystemClock.now());
    let launcher = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.extensions())
        .with_npm_registry(pane_core::npm::Registry::local(registry.url()).unwrap())
        .with_clock(clock.clone());
    install(&launcher, &dirs.sample(CONSUMERS[0].0));
    install(&launcher, &dirs.sample(PROVIDERS[2].0));
    registry.publish(name, "0.1.0", npm_registry::pack(&provider("0.1.0")));
    block_on(launcher.install_npm(name));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed npm greet provider".into())
    );
    // Chosen: the npm provider, beside the TypeScript one.
    let npm = launcher
        .packages()
        .into_iter()
        .find(|package| package.identity.npm_name() == Some(name))
        .map(|package| package.identity)
        .unwrap();
    let chosen = launcher.choose_provider(GREET, &npm.key()).unwrap();
    block_on(chosen).unwrap();
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from JavaScript")
    );

    // A newer version updates the package by itself, keeping its identity.
    // The command's screen is left first: the update's outcome shows on
    // root search.
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
    registry.publish(name, "0.2.0", npm_registry::pack(&provider("0.2.0")));
    clock.advance(Duration::from_secs(2));
    assert!(
        launcher.wait_for_updates(Duration::from_secs(30)),
        "the updater did not settle"
    );
    assert_eq!(
        launcher.view().status,
        Status::Result("Updated npm greet provider to 0.2.0".into())
    );
    // The choice still routes to it, not to the TypeScript provider.
    assert_eq!(
        greet(&launcher, "Greet from Rust"),
        result("Hello, Pane, from JavaScript")
    );
}
