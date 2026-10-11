//! What Manage extensions — the extension list, and each extension's page
//! in Settings — says about what an installed package needs, what it
//! provides and the cycles it is part of (#157, ADR 0041), read through
//! the launcher's public interface without running anything: the status
//! line "Enabled · Waiting for <what>", each unmet requirement with the
//! chain down to what is actually missing and the fix row beside it —
//! which does what it says: Enable and Retry bring the dependents back,
//! and Install installs what is missing again — the capabilities the
//! package provides, with whether it is the provider Pane routes calls
//! to and who uses them, and the groups of packages that require one
//! another, healthy groups included: they run, as ADR 0041 decides.
//! Optional requirements never show as unmet.
//!
//! With the operations and capabilities fixtures, the greet providers and
//! the capabilities consumer from `cargo xtask guests`.

use std::fs;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use pane_core::{
    ExtensionWait, Launcher, PackageIdentity, Runtime, SavedData, Status, Unavailable,
};
use tempfile::TempDir;

#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use guests::guest_file as guest;
use rows::{manage, titles};

/// The Rust operations sample, which greets and is greeted: the greeter
/// the others require.
const GREETER: &str = "sample-operations";

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

    fn identity(&self, name: &str) -> PackageIdentity {
        PackageIdentity::local(&self.folder(name)).unwrap()
    }

    fn extensions(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    fn launcher(&self) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.extensions())
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

    /// Writes a fixture package in source folder `name`, titled
    /// "Package <name>", whose command and `echo` operation the operations
    /// fixture serves, declaring `dependencies` (JSON array contents).
    fn echo_fixture(&self, name: &str, dependencies: &str) -> PathBuf {
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
                "commands": [
                    {{ "id": "fixture", "title": "Fixture {name}", "component": "fixture.wasm" }}
                ],
                "operations": [{{ "id": "echo", "version": 1, "component": "fixture.wasm" }}],
                "dependencies": [{dependencies}]
            }}"#
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        folder
    }

    /// Writes a capabilities fixture package in source folder `name`,
    /// titled "Package <name>", with `commands` (JSON array contents) and
    /// `members` (manifest members, each starting with a comma): its
    /// `provides`, `uses` or `dependencies`.
    fn capability_fixture(&self, name: &str, commands: &str, members: &str) -> PathBuf {
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
}

/// A required dependency on the package in sibling folder `folder`,
/// calling `echo` at version 1.
fn needs(folder: &str) -> String {
    format!(
        r#"{{ "id": "{folder}", "source": "local:../{folder}",
             "operations": [{{ "id": "echo", "version": 1 }}] }}"#
    )
}

/// A required dependency on the Rust operations sample, as `greeter`,
/// calling its `greet` 1.
fn greeter() -> String {
    r#"{ "id": "greeter", "source": "local:../sample-operations",
         "operations": [{ "id": "greet", "version": 1 }] }"#
        .to_owned()
}

/// An optional dependency on the Rust operations sample, as `helper`,
/// calling its `greet` 1.
fn uses_greeter() -> String {
    r#"{ "id": "helper", "source": "local:../sample-operations", "optional": true,
         "operations": [{ "id": "greet", "version": 1 }] }"#
        .to_owned()
}

/// The JavaScript operations sample's command and component, declaring
/// `dependencies` (JSON array contents), as the waiting suite writes it.
fn caller(dirs: &Dirs, dependencies: &str) -> PathBuf {
    let folder = dirs.folder("caller");
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
                {{ "id": "call", "title": "Call from JavaScript",
                   "component": "caller.wasm", "takesQuery": true }}
            ],
            "operations": [{{ "id": "greet", "version": 1, "component": "caller.wasm" }}],
            "dependencies": [{dependencies}]
        }}"#
    );
    fs::write(folder.join("pane.json"), manifest).unwrap();
    folder
}

/// The status the last action came to, as text.
fn result(status: &Status) -> String {
    match status {
        Status::Result(text) => text.clone(),
        other => panic!("expected a result, got {other:?}"),
    }
}

/// The one unmet requirement of the package with `identity`, as its page
/// lists it: none, or exactly one, with its fix.
fn requirement_of(launcher: &Launcher, identity: &PackageIdentity) -> pane_core::UnmetRequirement {
    let details = launcher
        .extension_details(identity)
        .unwrap_or_else(|| panic!("{identity} is not installed"));
    assert!(details.requirements.len() < 2, "{:?}", details.requirements);
    details.requirements.into_iter().next().unwrap()
}

/// The root search row titled `title`, as the waiting suite reads it.
fn row(launcher: &Launcher, title: &str) -> pane_core::Row {
    launcher
        .view()
        .rows
        .into_iter()
        .find(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)))
}

/// Installs the greeter (the Rust operations sample) and a caller that
/// requires it, leaving root search shown.
fn greeter_and_caller(dirs: &Dirs) -> Launcher {
    dirs.sample(GREETER);
    let caller = caller(dirs, &greeter());
    let launcher = dirs.launcher();
    dirs.install(&launcher, &caller);
    launcher.show_root_search();
    launcher
}

#[test]
fn the_extension_list_says_in_its_status_line_what_a_package_waits_for() {
    let dirs = Dirs::new();
    let launcher = greeter_and_caller(&dirs);
    let (caller, greeter) = (dirs.identity("caller"), dirs.identity(GREETER));

    // Its commands' rows say what they need, and so does the extension
    // list: a user finds the broken extensions without opening each.
    block_on(launcher.set_enabled(&greeter, false));
    launcher.show_root_search();
    assert_eq!(
        launcher.extension_wait(&caller),
        Some(ExtensionWait::Whole(
            "Rust operations sample, which is disabled".into()
        ))
    );
    assert_eq!(launcher.extension_wait(&greeter), None, "it is disabled");
    manage(&launcher);
    let row = launcher
        .view()
        .rows
        .into_iter()
        .find(|row| row.title == "Caller")
        .expect("the caller's row");
    let subtitle = row.subtitle.as_deref().expect("the caller's status line");
    assert!(
        subtitle.starts_with("Enabled · Waiting for Rust operations sample, which is disabled"),
        "{row:?}"
    );

    // Enabled again, the line says nothing: it is "Enabled" once more.
    block_on(launcher.set_enabled(&greeter, true));
    assert_eq!(launcher.extension_wait(&caller), None);
    manage(&launcher);
    let row = launcher
        .view()
        .rows
        .into_iter()
        .find(|row| row.title == "Caller")
        .expect("the caller's row");
    let subtitle = row.subtitle.as_deref().expect("the caller's status line");
    assert!(subtitle.starts_with("Enabled ·"), "{row:?}");
}

#[test]
fn a_package_s_page_lists_each_unmet_requirement_with_the_chain_and_fixes_it() {
    let dirs = Dirs::new();
    // a requires b, which requires c, which is disabled: a chain three
    // deep, naming what is actually missing.
    dirs.echo_fixture("c", "");
    dirs.echo_fixture("b", &needs("c"));
    let a = dirs.echo_fixture("a", &needs("b"));
    let launcher = dirs.launcher();
    dirs.install(&launcher, &a);
    block_on(launcher.set_enabled(&dirs.identity("c"), false));
    launcher.show_root_search();

    // b's requirement is its own; a's names the root cause down the chain.
    let of_b = requirement_of(&launcher, &dirs.identity("b"));
    assert_eq!(of_b.title, "Needs Package c, which is disabled");
    let of_a = requirement_of(&launcher, &dirs.identity("a"));
    assert_eq!(
        of_a.title,
        "Needs Package b, which waits for Package c: Package c is disabled"
    );
    // The fix row beside it fixes the root cause, not the direct
    // dependency.
    let fix = of_a.fix.expect("the fix row");
    assert_eq!(fix.title, "Enable Package c");
    assert_eq!(fix.action, pane_core::FixAction::Enable(dirs.identity("c")));

    // The fix applies at once: both dependents come back by themselves,
    // and the requirement rows disappear.
    block_on(launcher.run_extension_fix(&fix));
    assert_eq!(
        result(&launcher.view().status),
        "Enabled Package c".to_owned()
    );
    assert!(
        launcher
            .extension_details(&dirs.identity("a"))
            .unwrap()
            .requirements
            .is_empty()
    );
    assert_eq!(row(&launcher, "Fixture a").unavailable, None);
    assert_eq!(row(&launcher, "Fixture b").unavailable, None);

    // A fix that no longer applies — what it fixes came back — does
    // nothing: it would not disable the package again.
    block_on(launcher.run_extension_fix(&fix));
    assert!(
        launcher
            .packages()
            .iter()
            .any(|package| { package.identity == dirs.identity("c") && package.enabled })
    );
}

#[test]
fn the_fix_row_retries_a_paused_dependency_and_the_dependent_comes_back() {
    let dirs = Dirs::new();
    let launcher = greeter_and_caller(&dirs);
    let caller = dirs.identity("caller");
    drop(launcher);
    // Pane paused the greeter after it crashed, as recorded before a
    // restart.
    let registry = dirs.extensions().join("installed.json");
    let mut record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&registry).unwrap()).unwrap();
    let dir = record["packages"][0]["dir"].clone();
    record["packages"][0]["paused"] = serde_json::json!({
        "after": "crashes", "why": "it crashed", "version": "0.1.0", "code": dir
    });
    fs::write(&registry, record.to_string()).unwrap();
    let launcher = dirs.launcher();

    let requirement = requirement_of(&launcher, &caller);
    assert_eq!(
        requirement.title,
        "Needs Rust operations sample, which is paused"
    );
    let fix = requirement.fix.expect("the fix row");
    assert_eq!(fix.title, "Retry Rust operations sample");

    // Retried, the greeter starts and the dependent comes back by itself.
    block_on(launcher.run_extension_fix(&fix));
    assert_eq!(
        result(&launcher.view().status),
        "Started Rust operations sample".to_owned()
    );
    assert!(
        launcher
            .extension_details(&caller)
            .unwrap()
            .requirements
            .is_empty()
    );
    launcher.show_root_search();
    assert_eq!(row(&launcher, "Call from JavaScript").unavailable, None);
}

#[test]
fn the_fix_row_installs_a_not_installed_dependency_again() {
    let dirs = Dirs::new();
    // The greeter's settings, saved before the launcher opens, so its
    // uninstall keeps data under its title.
    dirs.sample(GREETER);
    let saved = serde_json::json!({
        "version": 1,
        "packages": { dirs.identity(GREETER).key(): { "style": "formal" } }
    });
    let settings = dirs.extensions().join("settings.json");
    fs::create_dir_all(dirs.extensions()).unwrap();
    fs::write(settings, saved.to_string()).unwrap();
    let caller = caller(&dirs, &greeter());
    let launcher = dirs.launcher();
    dirs.install(&launcher, &caller);
    let (caller, greeter) = (dirs.identity("caller"), dirs.identity(GREETER));

    // The greeter is uninstalled, its data kept: the caller waits for it,
    // naming it by the title its data was kept under.
    block_on(launcher.uninstall(&greeter, SavedData::Keep));
    launcher.show_root_search();
    let requirement = requirement_of(&launcher, &caller);
    assert_eq!(
        requirement.title,
        "Needs Rust operations sample, which is not installed"
    );
    let fix = requirement.fix.expect("the fix row");
    assert_eq!(fix.title, "Install Rust operations sample again");
    assert_eq!(fix.action, pane_core::FixAction::Install(greeter.clone()));

    // The fix installs it again, from where it came, and the dependent
    // comes back by itself.
    block_on(launcher.run_extension_fix(&fix));
    assert!(
        result(&launcher.view().status).starts_with("Installed Rust operations sample"),
        "{:?}",
        launcher.view().status
    );
    assert!(
        launcher
            .extension_details(&caller)
            .unwrap()
            .requirements
            .is_empty()
    );
    launcher.show_root_search();
    assert_eq!(row(&launcher, "Call from JavaScript").unavailable, None);
}

#[test]
fn an_optional_requirement_never_shows_as_unmet() {
    let dirs = Dirs::new();
    // An optional dependency nobody installed: nothing shows as unmet.
    let caller = caller(&dirs, &uses_greeter());
    let launcher = dirs.launcher();
    dirs.install(&launcher, &caller);
    let details = launcher
        .extension_details(&dirs.identity("caller"))
        .expect("the caller");
    assert!(details.requirements.is_empty(), "{details:?}");

    // An optional capability nobody provides, as the capabilities sample
    // declares one of: nothing shows as unmet either.
    let consumer = dirs.sample("sample-capabilities");
    dirs.install(&launcher, &consumer);
    let details = launcher
        .extension_details(&dirs.identity("sample-capabilities"))
        .expect("the consumer");
    assert!(details.requirements.is_empty(), "{details:?}");
}

#[test]
fn what_a_package_provides_shows_which_provider_serves_and_its_consumers() {
    let dirs = Dirs::new();
    // Two providers of one capability and a consumer of it: the first
    // installed is the provider Pane routes calls to — the one the
    // consumer's calls reach — and each page says who uses it.
    let greeter = dirs.sample("sample-greet");
    let javascript = dirs.sample("sample-greet-js");
    let consumer = dirs.sample("sample-capabilities");
    let launcher = dirs.launcher();
    dirs.install(&launcher, &greeter);
    dirs.install(&launcher, &javascript);
    dirs.install(&launcher, &consumer);

    let of_rust = launcher
        .extension_details(&dirs.identity("sample-greet"))
        .expect("the Rust provider");
    assert_eq!(
        of_rust.provides,
        vec![pane_core::ProvidedCapability {
            capability: "pane-samples:greet@1".into(),
            chosen: true,
            consumers: vec!["Rust capabilities sample".into()],
        }]
    );
    let of_javascript = launcher
        .extension_details(&dirs.identity("sample-greet-js"))
        .expect("the JavaScript provider");
    assert_eq!(
        of_javascript.provides,
        vec![pane_core::ProvidedCapability {
            capability: "pane-samples:greet@1".into(),
            chosen: false,
            consumers: vec!["Rust capabilities sample".into()],
        }]
    );

    // The consumer provides nothing: its page says so by listing nothing.
    let of_consumer = launcher
        .extension_details(&dirs.identity("sample-capabilities"))
        .expect("the consumer");
    assert!(of_consumer.provides.is_empty(), "{of_consumer:?}");

    // A package that is not installed has no page.
    assert!(
        launcher
            .extension_details(&PackageIdentity::npm("not-installed"))
            .is_none()
    );
}

#[test]
fn a_healthy_cycle_of_dependencies_is_shown_on_every_member_and_runs() {
    let dirs = Dirs::new();
    // x and y require each other: installed together, they run.
    dirs.echo_fixture("x", &needs("y"));
    let y = dirs.echo_fixture("y", &needs("x"));
    let launcher = dirs.launcher();
    dirs.install(&launcher, &y);
    launcher.show_root_search();

    // The group is shown on every member, whatever their state: it is
    // found from the declarations alone.
    let of_x = launcher
        .extension_details(&dirs.identity("x"))
        .expect("x is installed");
    assert_eq!(
        of_x.cycles,
        vec![pane_core::RequirementCycle {
            title: "Requires itself through Package y".into(),
        }]
    );
    let of_y = launcher
        .extension_details(&dirs.identity("y"))
        .expect("y is installed");
    assert_eq!(
        of_y.cycles,
        vec![pane_core::RequirementCycle {
            title: "Requires itself through Package x".into(),
        }]
    );
    // The healthy group runs, as ADR 0041 decides: neither waits.
    assert_eq!(launcher.extension_wait(&dirs.identity("x")), None);
    assert_eq!(launcher.extension_wait(&dirs.identity("y")), None);
    assert_eq!(row(&launcher, "Fixture x").unavailable, None);
    assert_eq!(row(&launcher, "Fixture y").unavailable, None);

    // A group one member of which cannot run waits as a whole, and is
    // still shown on every member.
    block_on(launcher.set_enabled(&dirs.identity("y"), false));
    launcher.show_root_search();
    assert_eq!(
        launcher.extension_wait(&dirs.identity("x")),
        Some(ExtensionWait::Whole("Package y, which is disabled".into()))
    );
    match row(&launcher, "Fixture x").unavailable.expect("x waits") {
        Unavailable::Waiting(reason) => assert_eq!(reason, "Needs Package y, which is disabled"),
        other => panic!("x waits, not {other:?}"),
    }
    for (name, other) in [("x", "y"), ("y", "x")] {
        let details = launcher
            .extension_details(&dirs.identity(name))
            .expect("still installed");
        assert_eq!(
            details.cycles,
            vec![pane_core::RequirementCycle {
                title: format!("Requires itself through Package {other}"),
            }],
            "{name}"
        );
    }
}

#[test]
fn a_cycle_through_capabilities_is_shown_on_every_member() {
    let dirs = Dirs::new();
    // Two packages that each provide the capability the other uses: each
    // requires the other, as the declarations say.
    let provides = r#","provides": [{ "capability": "fixture:greet@1",
             "component": "fixture.wasm", "operations": ["greet", "forward", "crash"] }]"#;
    let uses = r#","uses": [{ "capability": "fixture:greet@1",
             "operations": ["greet", "forward", "crash"] }]"#;
    let command = r#"{ "id": "fixture", "title": "Capabilities fixture",
             "component": "fixture.wasm" }"#;
    let a = dirs.capability_fixture("a", command, &format!("{provides}{uses}"));
    let b = dirs.capability_fixture("b", command, &format!("{provides}{uses}"));
    let launcher = dirs.launcher();
    dirs.install(&launcher, &a);
    dirs.install(&launcher, &b);

    // A group is found from the capabilities one uses and another
    // provides, as it is from dependencies.
    for (name, other) in [("a", "b"), ("b", "a")] {
        let details = launcher
            .extension_details(&dirs.identity(name))
            .expect("installed");
        assert_eq!(
            details.cycles,
            vec![pane_core::RequirementCycle {
                title: format!("Requires itself through Package {other}"),
            }],
            "{name}"
        );
    }
    // Each provides the capability, and each is a consumer of the other's
    // page — and of its own.
    let of_a = launcher
        .extension_details(&dirs.identity("a"))
        .expect("a is installed");
    assert_eq!(
        of_a.provides,
        vec![pane_core::ProvidedCapability {
            capability: "fixture:greet@1".into(),
            chosen: true,
            consumers: vec!["Package a".into(), "Package b".into()],
        }]
    );
    let of_b = launcher
        .extension_details(&dirs.identity("b"))
        .expect("b is installed");
    assert_eq!(
        of_b.provides,
        vec![pane_core::ProvidedCapability {
            capability: "fixture:greet@1".into(),
            chosen: false,
            consumers: vec!["Package a".into(), "Package b".into()],
        }]
    );
}
