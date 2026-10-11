//! Commands that wait while no provider can serve a required capability
//! (#156, the second slice of #151's waiting commands): while a required
//! capability of an enabled, unpaused package has no provider that is
//! installed, enabled, not paused and not waiting, the package's commands
//! stay listed saying what they need, nothing of them runs — not their
//! views, their scheduled work, their services or their results — and they
//! come back by themselves once a provider can serve again. A use narrowed
//! with `commands` gates only those commands; a use of every provider
//! (`"use": "all"`) never gates, degrading to an empty list; an optional
//! use never gates. A chain through capabilities names what is actually
//! missing, a cycle with one member unable waits as a whole, and a
//! provider that is also a consumer never serves itself. Enter on a
//! waiting command offers to install the default its use names, or an
//! extension that provides the capability, from the install forms.
//!
//! A fan-out (`call-every`) calls every provider that can serve now, in
//! the order Pane calls them, each answering with its title and result or
//! error; the providers that cannot serve are skipped, with none the list
//! is empty, and a fan-out of a use of one provider is refused.
//!
//! Through the launcher's public interface, with the capability samples in
//! all three languages (their greet use made one provider again, as it was
//! before the fan-out item's `"use": "all"`), the capabilities fixture,
//! and the manual clock.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::clipboard::{Clock, ManualClock, SystemClock};
use pane_core::{
    Launcher, PackageIdentity, Row, Runtime, Screen, SettingsTarget, Status, Unavailable,
};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/platforms.rs"]
mod platforms;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guest_file as guest;
use rows::{select_title, titles, to_root};

/// How long the scheduler, the services thread and a call may take:
/// compiling a guest once is included; a slow, busy machine is not.
const PROMPTLY: Duration = Duration::from_secs(8);

/// The capability the samples provide and use.
const GREET: &str = "pane-samples:greet@1";

/// The greet providers: (package, command title, package title, answer's
/// language).
const PROVIDERS: [(&str, &str, &str, &str); 3] = [
    (
        "sample-greet",
        "Rust greet provider",
        "Rust greet provider sample",
        "Rust",
    ),
    (
        "sample-greet-js",
        "JavaScript greet provider",
        "JavaScript greet provider sample",
        "JavaScript",
    ),
    (
        "sample-greet-ts",
        "TypeScript greet provider",
        "TypeScript greet provider sample",
        "TypeScript",
    ),
];

/// The consumers: (package, command title).
const CONSUMERS: [(&str, &str); 3] = [
    ("sample-capabilities", "Greet from Rust"),
    ("sample-capabilities-js", "Greet from JavaScript"),
    ("sample-capabilities-ts", "Greet from TypeScript"),
];

/// Source folders, Pane's data folder, the runtime the launchers share,
/// and the manual clock the scheduler and services follow.
struct Dirs {
    sources: TempDir,
    data: TempDir,
    runtime: Runtime,
    clock: Arc<ManualClock>,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            runtime: Runtime::start().unwrap(),
            clock: ManualClock::at(SystemClock.now() + 365 * 86_400_000),
        }
    }

    fn folder(&self, name: &str) -> PathBuf {
        self.sources.path().join(name)
    }

    fn identity(&self, name: &str) -> PackageIdentity {
        PackageIdentity::local(&self.folder(name)).unwrap()
    }

    fn extensions(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    fn launcher(&self) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.extensions())
            .with_clock(self.clock.clone())
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

    /// Copies the assembled sample package `package` into source folder
    /// `package` and rewrites its `pane.json` with `change`: the samples
    /// whose manifests these tests change.
    fn edited(&self, package: &str, change: impl Fn(&mut serde_json::Value)) -> PathBuf {
        let folder = self.sample(package);
        let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        change(&mut manifest);
        fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
        folder
    }

    /// Copies the assembled sample package `package` into source folder
    /// `package`, declaring `uses` (JSON array contents): a sample that
    /// needs a capability, required and of one provider, so its commands
    /// wait while no provider can serve it.
    fn using(&self, package: &str, uses: &str) -> PathBuf {
        self.edited(package, |manifest| {
            manifest["uses"] = serde_json::from_str(&format!("[{uses}]")).unwrap();
        })
    }

    /// The consumer sample `package`, its use of the greet capability made
    /// one provider again (its pane.json ships `"use": "all"` for the
    /// fan-out item): its command waits while no provider can serve it.
    fn waiting_consumer(&self, package: &str) -> PathBuf {
        self.edited(package, |manifest| {
            manifest["uses"][0].as_object_mut().unwrap().remove("use");
        })
    }

    /// Writes a fixture package in source folder `name`, titled
    /// "Package <name>", with `commands` (JSON array contents) and
    /// `members` (manifest members, each starting with a comma): its
    /// `provides`, `uses`, `operations` or `dependencies`, and the
    /// capabilities fixture component as `fixture.wasm`.
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

    /// Installs `folder`'s package, asserting it worked.
    fn install(&self, launcher: &Launcher, folder: &Path) {
        block_on(launcher.install_package(folder));
        let status = launcher.view().status;
        assert!(
            matches!(&status, Status::Result(text) if text.starts_with("Installed")),
            "installing {}: {status:?}",
            folder.display()
        );
    }

    /// How many runs the Schedule sample has counted, kept in its content,
    /// or `None` before the first.
    fn runs_counted(&self, folder: &Path) -> Option<u64> {
        counted(self.extensions().join("content.json"), folder, "count")
    }

    /// How many cycles the Service sample has run, kept in its content, or
    /// `None` before the first.
    fn cycles(&self, folder: &Path) -> Option<u64> {
        counted(self.extensions().join("content.json"), folder, "cycles")
    }

    /// Advances the clock by `seconds`.
    fn advance(&self, seconds: u64) {
        self.clock.advance(Duration::from_secs(seconds));
    }
}

/// A number the package of `folder` saved under `key` in its content, or
/// `None` before it saved one.
fn counted(content: PathBuf, folder: &Path, key: &str) -> Option<u64> {
    let text = fs::read_to_string(content).ok()?;
    let file: serde_json::Value = serde_json::from_str(&text).unwrap();
    let package = PackageIdentity::local(folder).unwrap().key();
    file["packages"][&package][key]
        .as_str()
        .and_then(|count| count.parse().ok())
}

/// The root search row titled `title`.
fn row(launcher: &Launcher, title: &str) -> Row {
    let view = launcher.view();
    view.rows
        .into_iter()
        .find(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)))
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

/// From root search, activates the row titled `title`: opening a command,
/// or, for a waiting one, showing its reason with the row that fixes it.
fn open(launcher: &Launcher, title: &str) {
    to_root(launcher);
    select_title(launcher, title);
    block_on(launcher.activate_selected());
}

/// Selects the row titled `title` on the screen shown and activates it.
fn activate(launcher: &Launcher, title: &str) -> Status {
    select_title(launcher, title);
    block_on(launcher.activate_selected());
    launcher.view().status
}

/// From root search, opens the command titled `command` and activates its
/// item titled `item`.
fn open_item(launcher: &Launcher, command: &str, item: &str) {
    open(launcher, command);
    select_title(launcher, item);
    block_on(launcher.activate_selected());
}

/// Activates the "Greet every provider" item of `command`, returning what
/// it showed.
fn every(launcher: &Launcher, command: &str) -> Status {
    open_item(launcher, command, "Greet every provider");
    shown(launcher)
}

fn result(text: &str) -> Status {
    Status::Result(text.into())
}

/// Waits until `read` returns `expected`, saying `what` it was.
fn until<T: PartialEq + std::fmt::Debug>(what: &str, expected: T, mut read: impl FnMut() -> T) {
    let began = Instant::now();
    loop {
        let found = read();
        if found == expected {
            return;
        }
        assert!(
            began.elapsed() < PROMPTLY,
            "waiting for {what}: found {found:?}, expected {expected:?}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// The required use of the greet capability the waiting samples declare:
/// one provider, so its commands wait while none can serve.
fn uses_greet() -> String {
    format!(r#"{{ "capability": "{GREET}", "operations": ["greet"] }}"#)
}

/// A required dependency on the package in sibling folder `folder`,
/// calling `echo` at version 1.
fn needs(folder: &str) -> String {
    format!(
        r#"{{ "id": "{folder}", "source": "local:../{folder}",
             "operations": [{{ "id": "echo", "version": 1 }}] }}"#
    )
}

/// The commands of the fixture packages: their command, as `a` lists it.
const COMMAND: &str =
    r#"{ "id": "fixture", "title": "Capabilities fixture", "component": "fixture.wasm" }"#;

/// A consumer of each language with no provider: its command waits, saying
/// what it needs, Enter shows the reason with the row that installs a
/// provider, and installing one — of each language — brings it back by
/// itself.
#[test]
fn a_consumer_waits_with_no_provider_and_comes_back_when_one_is_installed() {
    for (consumer, command) in CONSUMERS {
        for (provider, _, _, language) in PROVIDERS {
            let dirs = Dirs::new();
            let consumer = dirs.waiting_consumer(consumer);
            let launcher = dirs.launcher();
            dirs.install(&launcher, &consumer);

            // Installing never stops because no provider is installed
            // (#153); the command waits, and nothing of it runs.
            assert_eq!(
                waiting_reason(&launcher, command),
                format!("Needs {GREET}: no extension provides it")
            );
            open(&launcher, command);
            let view = launcher.view();
            assert!(
                matches!(view.screen, Screen::WaitingDetails { .. }),
                "{view:?}"
            );
            assert_eq!(view.title, format!("Why {command} cannot run"));
            let reason = format!("Needs {GREET}: no extension provides it.");
            assert_eq!(
                view.details().first().map(String::as_str),
                Some(reason.as_str())
            );
            assert_eq!(
                titles(&launcher),
                [format!("Install an extension that provides {GREET}")]
            );
            // The row opens the install forms, where a provider is chosen.
            assert_eq!(
                launcher.selected_settings_target(),
                Some(SettingsTarget::InstallFromNpm)
            );

            // Installing a provider brings the command back by itself,
            // with nothing done to the consumer.
            dirs.install(&launcher, &dirs.sample(provider));
            launcher.show_root_search();
            assert_eq!(row(&launcher, command).unavailable, None);
            open_item(&launcher, command, "Greet through a capability");
            assert_eq!(
                shown(&launcher),
                result(&format!("Hello, Pane, from {language}"))
            );
        }
    }
}

/// A consumer waits while its only provider is disabled, and comes back
/// when it is enabled; while the provider waits for a dependency of its
/// own, the chain names what is actually missing; and disabling one of two
/// providers, while the other can serve, leaves the consumer running.
#[test]
fn a_consumer_waits_while_its_only_provider_cannot_serve() {
    let (package, command, title) = (PROVIDERS[0].0, CONSUMERS[0].1, PROVIDERS[0].2);
    let dirs = Dirs::new();
    let consumer = dirs.waiting_consumer(CONSUMERS[0].0);
    let launcher = dirs.launcher();
    dirs.install(&launcher, &consumer);
    dirs.install(&launcher, &dirs.sample(package));
    let provider = dirs.identity(package);

    // Disabling the only provider: the consumer waits for the capability,
    // and Enter's fix row enables it again.
    block_on(launcher.set_enabled(&provider, false));
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, command),
        format!("Needs {GREET}: {title} is disabled")
    );
    open(&launcher, command);
    assert_eq!(titles(&launcher), [format!("Enable {title}")]);
    assert_eq!(
        activate(&launcher, &format!("Enable {title}")),
        Status::Result(format!("Enabled {title}"))
    );
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert_eq!(row(&launcher, command).unavailable, None);

    // A second provider installed: disabling the first changes nothing for
    // the consumer, which keeps running on the other.
    dirs.install(&launcher, &dirs.sample(PROVIDERS[1].0));
    block_on(launcher.set_enabled(&provider, false));
    launcher.show_root_search();
    assert_eq!(row(&launcher, command).unavailable, None);
    open_item(&launcher, command, "Greet through a capability");
    assert_eq!(shown(&launcher), result("Hello, Pane, from JavaScript"));
}

/// A provider waiting for what it needs leaves its consumer waiting in
/// turn, the chain names what is actually missing, and the fix row fixes
/// the root cause.
#[test]
fn a_consumer_waits_while_its_provider_waits_for_a_dependency() {
    let dirs = Dirs::new();
    let consumer = dirs.waiting_consumer(CONSUMERS[0].0);
    // The provider requires the operations sample, which its install
    // installs with it.
    let provider = dirs.edited(PROVIDERS[0].0, |manifest| {
        manifest["dependencies"] = serde_json::from_str(
            r#"[{ "id": "greeter", "source": "local:../sample-operations",
                 "operations": [{ "id": "greet", "version": 1 }] }]"#,
        )
        .unwrap();
    });
    dirs.sample("sample-operations");
    let launcher = dirs.launcher();
    dirs.install(&launcher, &consumer);
    dirs.install(&launcher, &provider);
    let greeter = dirs.identity("sample-operations");

    // The dependency disabled: the provider waits for it, no provider can
    // serve the capability, and the consumer waits in turn, naming what is
    // actually missing.
    block_on(launcher.set_enabled(&greeter, false));
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, CONSUMERS[0].1),
        format!(
            "Needs {GREET}: {}, which waits for Rust operations sample: Rust operations \
             sample is disabled",
            PROVIDERS[0].2
        )
    );
    open(&launcher, CONSUMERS[0].1);
    assert_eq!(titles(&launcher), ["Enable Rust operations sample"]);
    assert_eq!(
        activate(&launcher, "Enable Rust operations sample"),
        Status::Result("Enabled Rust operations sample".into())
    );
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert_eq!(row(&launcher, CONSUMERS[0].1).unavailable, None);
    open_item(&launcher, CONSUMERS[0].1, "Greet through a capability");
    assert_eq!(shown(&launcher), result("Hello, Pane, from Rust"));
}

/// A use narrowed with `commands` gates only those commands: the package's
/// other commands stay available.
#[test]
fn a_commands_narrowed_use_gates_only_those_commands() {
    let dirs = Dirs::new();
    // One component, two commands: the first needs the capability, the
    // second does not.
    let folder = dirs.folder("narrowed");
    fs::create_dir_all(&folder).unwrap();
    fs::copy(
        guest("sample_capabilities.wasm"),
        folder.join("narrowed.wasm"),
    )
    .unwrap();
    let manifest = format!(
        r#"{{
            "manifestVersion": 1,
            "title": "Narrowed",
            "apiVersion": "0.1",
            "commands": [
                {{ "id": "greeted", "title": "Greeted", "component": "narrowed.wasm" }},
                {{ "id": "free", "title": "Free", "component": "narrowed.wasm" }}
            ],
            "uses": [
                {{ "capability": "{GREET}", "operations": ["greet"], "commands": ["greeted"] }}
            ]
        }}"#
    );
    fs::write(folder.join("pane.json"), manifest).unwrap();
    let launcher = dirs.launcher();
    dirs.install(&launcher, &folder);

    // With no provider, only the narrowed command waits; the other opens.
    assert_eq!(
        waiting_reason(&launcher, "Greeted"),
        format!("Needs {GREET}: no extension provides it")
    );
    assert_eq!(row(&launcher, "Free").unavailable, None);
    open(&launcher, "Free");
    assert_eq!(launcher.view().screen, Screen::Command);
    launcher.back();

    // A provider installed, then disabled: the narrowed command comes back
    // and goes again alone.
    dirs.install(&launcher, &dirs.sample(PROVIDERS[0].0));
    launcher.show_root_search();
    assert_eq!(row(&launcher, "Greeted").unavailable, None);
    block_on(launcher.set_enabled(&dirs.identity(PROVIDERS[0].0), false));
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, "Greeted"),
        format!("Needs {GREET}: {} is disabled", PROVIDERS[0].2)
    );
    assert_eq!(row(&launcher, "Free").unavailable, None);
}

/// A chain through capabilities (A uses a capability only B provides, and
/// B waits for another capability nobody provides) names the root cause,
/// and Enter's fix row installs a provider of it.
#[test]
fn a_chain_through_capabilities_names_the_root_cause() {
    let dirs = Dirs::new();
    // `a` uses `fixture:greet@1`, which only `b` provides; `b` uses
    // `fixture:auth@1`, which nobody provides.
    dirs.fixture(
        "a",
        COMMAND,
        r#","uses": [
            { "capability": "fixture:greet@1", "operations": ["greet"] }
        ]"#,
    );
    dirs.fixture(
        "b",
        "",
        r#","provides": [
            { "capability": "fixture:greet@1", "component": "fixture.wasm",
              "operations": ["greet"] }
        ],
        "uses": [
            { "capability": "fixture:auth@1", "operations": ["greet"] }
        ]"#,
    );
    let launcher = dirs.launcher();
    dirs.install(&launcher, &dirs.folder("a"));
    dirs.install(&launcher, &dirs.folder("b"));

    assert_eq!(
        waiting_reason(&launcher, "Capabilities fixture"),
        "Needs fixture:greet@1: Package b, which waits for fixture:auth@1: no extension \
         provides it"
    );
    open(&launcher, "Capabilities fixture");
    assert_eq!(
        titles(&launcher),
        ["Install an extension that provides fixture:auth@1"]
    );
}

/// A cycle through capabilities runs when every member is healthy, and
/// waits as a whole when one member cannot run.
#[test]
fn a_cycle_through_capabilities_waits_as_a_whole_when_one_member_cannot_run() {
    let dirs = Dirs::new();
    // `x` provides `fixture:first@1` and uses `fixture:second@1`; `y`
    // provides `fixture:second@1`, uses `fixture:first@1` and requires
    // `z`. Installed together (with `z`), they run.
    let provides = |capability: &str| {
        format!(
            r#","provides": [
                {{ "capability": "{capability}", "component": "fixture.wasm",
                   "operations": ["greet"] }}
            ]"#
        )
    };
    let uses = |capability: &str| {
        format!(r#","uses": [{{ "capability": "{capability}", "operations": ["greet"] }}]"#)
    };
    dirs.fixture(
        "x",
        r#"{ "id": "fixture", "title": "Fixture x", "component": "fixture.wasm" }"#,
        &format!(
            "{}{}",
            provides("fixture:first@1"),
            uses("fixture:second@1")
        ),
    );
    dirs.fixture(
        "y",
        r#"{ "id": "fixture", "title": "Fixture y", "component": "fixture.wasm" }"#,
        &format!(
            "{}{}, \"dependencies\": [{}]",
            provides("fixture:second@1"),
            uses("fixture:first@1"),
            needs("z")
        ),
    );
    dirs.fixture(
        "z",
        "",
        r#","operations": [{ "id": "echo", "version": 1, "component": "fixture.wasm" }]"#,
    );
    let launcher = dirs.launcher();
    dirs.install(&launcher, &dirs.folder("x"));
    dirs.install(&launcher, &dirs.folder("y"));
    launcher.show_root_search();
    assert_eq!(row(&launcher, "Fixture x").unavailable, None);
    assert_eq!(row(&launcher, "Fixture y").unavailable, None);
    open(&launcher, "Fixture x");
    assert_eq!(launcher.view().screen, Screen::Command);

    // One member cannot run (`z`, which `y` requires, is disabled): both
    // wait as a whole, and `x`'s reason names what is actually missing.
    // Enabling `z` brings both back.
    let z = dirs.identity("z");
    block_on(launcher.set_enabled(&z, false));
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, "Fixture x"),
        "Needs fixture:second@1: Package y, which waits for Package z: Package z is disabled"
    );
    assert!(row(&launcher, "Fixture y").unavailable.is_some());

    block_on(launcher.set_enabled(&z, true));
    launcher.show_root_search();
    assert_eq!(row(&launcher, "Fixture x").unavailable, None);
    assert_eq!(row(&launcher, "Fixture y").unavailable, None);
}

/// A use of every provider never waits: with no provider at all, the
/// consumer's command runs, its fan-out answers an empty list and its
/// plain call `not-found`. An optional use never gates either, and
/// `available` answers as providers come and go.
#[test]
fn a_use_of_every_provider_never_waits_and_available_reflects_providers() {
    let dirs = Dirs::new();
    let consumer = dirs.sample(CONSUMERS[0].0);
    let launcher = dirs.launcher();
    dirs.install(&launcher, &consumer);
    let command = CONSUMERS[0].1;

    // No provider: the command is available, and the fan-out degrades to
    // an empty list.
    assert_eq!(row(&launcher, command).unavailable, None);
    open(&launcher, command);
    assert_eq!(launcher.view().screen, Screen::Command);
    assert_eq!(
        every(&launcher, command),
        result(&format!("No installed extension provides {GREET}"))
    );

    // An optional use with no provider degrades gracefully, and
    // `available` answers what there is to know.
    open_item(&launcher, command, "Is the farewell capability available");
    assert_eq!(
        shown(&launcher),
        result("pane-samples:farewell@1 has no provider now")
    );

    // A provider of the optional capability installed: `available` answers
    // it, and none of the consumer's commands wait for it (the use stays
    // optional).
    dirs.fixture(
        "farewell",
        "",
        r#","provides": [
            { "capability": "pane-samples:farewell@1", "component": "fixture.wasm",
              "operations": ["farewell"] }
        ]"#,
    );
    dirs.install(&launcher, &dirs.folder("farewell"));
    open_item(&launcher, command, "Is the farewell capability available");
    assert_eq!(
        shown(&launcher),
        result("pane-samples:farewell@1 has a provider: Package farewell")
    );
    launcher.show_root_search();
    assert_eq!(row(&launcher, command).unavailable, None);

    // Disabled again: `available` answers none.
    block_on(launcher.set_enabled(&dirs.identity("farewell"), false));
    open_item(&launcher, command, "Is the farewell capability available");
    assert_eq!(
        shown(&launcher),
        result("pane-samples:farewell@1 has no provider now")
    );
}

/// A fan-out answers each provider that can serve now, in the order Pane
/// calls them, skips the providers that cannot, and answers an empty list
/// when none can.
#[test]
fn a_fan_out_answers_each_available_provider_in_order_and_skips_the_rest() {
    let dirs = Dirs::new();
    let consumer = dirs.sample(CONSUMERS[0].0);
    let launcher = dirs.launcher();
    dirs.install(&launcher, &consumer);
    let command = CONSUMERS[0].1;
    let said = |answers: &[String]| answers.join("; ");
    let answer = |(title, language): (&str, &str)| format!("{title}: Hello, Pane, from {language}");

    // Install order is the order the providers are called in.
    for (package, ..) in [PROVIDERS[1], PROVIDERS[2], PROVIDERS[0]] {
        dirs.install(&launcher, &dirs.sample(package));
    }
    assert_eq!(
        every(&launcher, command),
        result(&said(&[
            answer((PROVIDERS[1].2, PROVIDERS[1].3)),
            answer((PROVIDERS[2].2, PROVIDERS[2].3)),
            answer((PROVIDERS[0].2, PROVIDERS[0].3)),
        ]))
    );

    // A disabled provider is skipped; the others still answer.
    block_on(launcher.set_enabled(&dirs.identity(PROVIDERS[2].0), false));
    assert_eq!(
        every(&launcher, command),
        result(&said(&[
            answer((PROVIDERS[1].2, PROVIDERS[1].3)),
            answer((PROVIDERS[0].2, PROVIDERS[0].3)),
        ]))
    );

    // With none left, the list is empty, not an error.
    block_on(launcher.set_enabled(&dirs.identity(PROVIDERS[1].0), false));
    block_on(launcher.set_enabled(&dirs.identity(PROVIDERS[0].0), false));
    assert_eq!(
        every(&launcher, command),
        result(&format!("No installed extension provides {GREET}"))
    );
}

/// The fixture drives a fan-out with providers that wait, are for other
/// systems, or answer their own errors: each provider's answer is its own,
/// the unable ones are skipped, and a fan-out of a use of one provider is
/// refused.
#[test]
fn the_fixture_drives_a_fan_out_skipping_and_refusing() {
    let dirs = Dirs::new();
    // `a` uses the every capability (`"use": "all"`) and the greet
    // capability (one provider); `b` and `d` provide the every capability;
    // `c` provides it and waits for `w`, which is disabled; `e` provides it
    // for another system.
    let uses = r#","uses": [
        { "capability": "fixture:greet@1", "operations": ["greet", "forward", "crash"],
          "optional": true },
        { "capability": "fixture:every@1", "operations": ["greet"], "use": "all" }
    ]"#;
    dirs.fixture("a", COMMAND, uses);
    let provides = r#","provides": [
        { "capability": "fixture:every@1", "component": "fixture.wasm",
          "operations": ["greet"] }
    ]"#;
    dirs.fixture("b", "", provides);
    dirs.fixture(
        "c",
        "",
        &format!("{}, \"dependencies\": [{}]", provides, needs("w")),
    );
    dirs.fixture("d", "", provides);
    dirs.fixture(
        "w",
        "",
        r#","operations": [{ "id": "echo", "version": 1, "component": "fixture.wasm" }]"#,
    );
    let [other, _] = platforms::other_systems();
    dirs.fixture(
        "e",
        "",
        &format!(
            r#","provides": [
                {{ "capability": "fixture:every@1", "component": "fixture.wasm",
                   "operations": ["greet"], "platforms": ["{}"] }}
            ]"#,
            other.id()
        ),
    );
    let launcher = dirs.launcher();
    for name in ["a", "b", "c", "d", "e"] {
        dirs.install(&launcher, &dirs.folder(name));
    }
    block_on(launcher.set_enabled(&dirs.identity("w"), false));

    // Each provider that can serve answers, in install order, with the
    // operation qualified by the capability; the waiting one and the one
    // for another system are skipped.
    let answer = |title: &str| {
        format!("{title}: {{\"greeting\":\"Hello, Ada\",\"operation\":\"fixture:every@1/greet\"}}")
    };
    open_item(
        &launcher,
        "Capabilities fixture",
        "Call every provider of the every capability",
    );
    assert_eq!(
        shown(&launcher),
        result(&[answer("Package b"), answer("Package d")].join("; "))
    );

    // Each provider's own error reaches its own answer.
    open_item(
        &launcher,
        "Capabilities fixture",
        "Call every provider with no name",
    );
    assert_eq!(
        shown(&launcher),
        result("Package b: failed: a name is needed; Package d: failed: a name is needed")
    );

    // None left: the empty list.
    block_on(launcher.set_enabled(&dirs.identity("b"), false));
    block_on(launcher.set_enabled(&dirs.identity("d"), false));
    open_item(
        &launcher,
        "Capabilities fixture",
        "Call every provider of the every capability",
    );
    assert_eq!(shown(&launcher), result("no provider answered"));

    // A fan-out of a use of one provider is refused.
    open_item(
        &launcher,
        "Capabilities fixture",
        "Call every provider of the greet capability",
    );
    assert_eq!(
        shown(&launcher),
        Status::Error(
            "The extension reported an error: refused: Package a declares that it calls \
             `fixture:greet@1` with \"use\": \"one\", so one provider serves each call; \
             declare \"use\": \"all\" in its pane.json to call every provider"
                .into()
        )
    );
}

/// The schedule of a command waiting for a capability ticks not, and comes
/// back from a full interval.
#[test]
fn waiting_for_a_capability_skips_a_schedule_s_ticks_until_it_comes_back() {
    let dirs = Dirs::new();
    let scheduled = set_every(&dirs.using("sample-schedule", &uses_greet()), 1);
    let launcher = dirs.launcher();
    dirs.install(&launcher, &dirs.sample(PROVIDERS[0].0));
    dirs.install(&launcher, &scheduled);
    assert!(launcher.wait_for_schedules(PROMPTLY));

    // It runs every second while it may run.
    dirs.advance(1);
    until("the first scheduled run", Some(1), || {
        dirs.runs_counted(&scheduled)
    });
    assert!(launcher.wait_for_schedules(PROMPTLY));

    // No provider can serve: the command waits, and the ticks are skipped,
    // not replayed.
    block_on(launcher.set_enabled(&dirs.identity(PROVIDERS[0].0), false));
    dirs.advance(5);
    assert!(launcher.wait_for_schedules(PROMPTLY));
    assert_eq!(dirs.runs_counted(&scheduled), Some(1));
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, "Counting"),
        format!("Needs {GREET}: {} is disabled", PROVIDERS[0].2)
    );

    // Enabled again: the schedule starts from a full interval — no tick
    // that fell due while it waited is replayed — and then runs.
    block_on(launcher.set_enabled(&dirs.identity(PROVIDERS[0].0), true));
    dirs.advance(1);
    until("the schedule came back", Some(2), || {
        dirs.runs_counted(&scheduled)
    });
    assert_eq!(dirs.runs_counted(&scheduled), Some(2));
}

/// The service of a command waiting for a capability does not cycle, and
/// its first cycle runs at once when it comes back.
#[test]
fn waiting_for_a_capability_stops_a_service_s_cycles_until_it_comes_back() {
    let dirs = Dirs::new();
    let serving = dirs.using("sample-service", &uses_greet());
    let launcher = dirs.launcher();
    dirs.install(&launcher, &dirs.sample(PROVIDERS[0].0));
    dirs.install(&launcher, &serving);
    assert!(launcher.wait_for_services(PROMPTLY));

    // Its first cycle runs at once while it may run.
    until("the first cycle", Some(1), || dirs.cycles(&serving));

    // No provider can serve: the command waits, and the service does not
    // cycle.
    block_on(launcher.set_enabled(&dirs.identity(PROVIDERS[0].0), false));
    dirs.advance(30);
    assert!(launcher.wait_for_services(PROMPTLY));
    assert_eq!(dirs.cycles(&serving), Some(1));
    launcher.show_root_search();
    assert_eq!(
        waiting_reason(&launcher, "Watching"),
        format!("Needs {GREET}: {} is disabled", PROVIDERS[0].2)
    );

    // Enabled again: the first cycle runs at once, in the instance the
    // service still has.
    block_on(launcher.set_enabled(&dirs.identity(PROVIDERS[0].0), true));
    until("the service came back", Some(2), || dirs.cycles(&serving));
    assert_eq!(dirs.cycles(&serving), Some(2));
}

/// Root results are not asked for while a command waits for a capability,
/// and asked for again once it is back.
#[test]
fn root_results_are_not_asked_while_waiting_for_a_capability() {
    let dirs = Dirs::new();
    let sample = dirs.using("sample-rust", &uses_greet());
    let launcher = dirs.launcher();
    dirs.install(&launcher, &dirs.sample(PROVIDERS[0].0));
    dirs.install(&launcher, &sample);
    launcher.back();

    // A query asks the sample for its root results.
    block_on(launcher.set_query("reverse Pané"));
    assert_eq!(launcher.view().rows[0].title, "énaP");
    launcher.back();

    // No provider can serve: the command waits, and its root results are
    // not asked for.
    block_on(launcher.set_enabled(&dirs.identity(PROVIDERS[0].0), false));
    block_on(launcher.set_query("reverse another"));
    assert!(
        launcher
            .view()
            .rows
            .iter()
            .all(|row| row.title != "rehtona"),
        "{:?}",
        titles(&launcher)
    );

    // Enabled again: the next query asks for its results.
    block_on(launcher.set_enabled(&dirs.identity(PROVIDERS[0].0), true));
    block_on(launcher.set_query("reverse back"));
    assert_eq!(launcher.view().rows[0].title, "kcab");
}

/// Sets the Schedule sample's command to run its item every `every`
/// seconds, and returns its folder.
fn set_every(folder: &Path, every: u64) -> PathBuf {
    let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    manifest["commands"][0]["schedule"] =
        serde_json::json!({ "everySeconds": every, "item": "count" });
    fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
    folder.to_path_buf()
}
