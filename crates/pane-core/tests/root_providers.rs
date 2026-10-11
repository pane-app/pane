//! Root providers (#164) through the launcher's public interface: a command
//! whose `pane.json` entry says `"mode": "provider"` only answers root
//! search. The fixtures are copies of the Rust sample and the JavaScript
//! applications sample, their command's entry rewritten to the provider
//! mode, the shape the default extensions' manifests hold and no sample's
//! does (their sources live in their own repositories, #285, which these
//! tests cannot read): typing their names finds no row of theirs, while
//! the Rust sample's root results are still answered; a
//! provider cannot be aliased, given a hotkey or launched, and the
//! Shortcuts catalog does not list it; disabling its package stops its
//! results and enabling brings them back; a provider declaring no root
//! results is refused at install with the reason; and what was recorded
//! for a command before it became a provider (pins, aliases, fallbacks,
//! hotkeys) is dropped at start, once, with a toast naming it. The packages
//! are the ones `cargo xtask guests` assembles in `target/guests/packages`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::executor::block_on;
use pane_core::applications::{Application, Applications};
use pane_core::hotkeys::Shortcut;
use pane_core::{CommandMode, Launcher, Manifest, PackageIdentity, Runtime, Status, ToastStyle};
use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

use rows::titles;

fn built(path: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(path);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// A copy of the assembled sample package `sample` under the test's
/// sources, its first command's entry in `pane.json` rewritten to a root
/// provider's (`"mode": "provider"`): a package shaped as the default
/// extensions are, which no sample is.
fn provider(dirs: &Dirs, sample: &str) -> PathBuf {
    let folder = dirs.sources.path().join(sample);
    if folder.exists() {
        return folder;
    }
    copy_folder(&built(&format!("packages/{sample}")), &folder);
    let mut manifest = read(&folder, "pane.json");
    manifest["commands"][0]["mode"] = serde_json::json!("provider");
    fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
    folder
}

/// The Rust sample made a provider: its root results ("reverse <text>")
/// still answer.
fn rust_provider(dirs: &Dirs) -> PathBuf {
    provider(dirs, "sample-rust")
}

/// The JavaScript applications sample made a provider: the host's
/// applications still reach root search as its indexed results.
fn applications_provider(dirs: &Dirs) -> PathBuf {
    provider(dirs, "sample-applications-js")
}

/// The command id of the command `command` of the package installed from
/// `folder`: its identity's key, `#`, its id in `pane.json`.
fn command_id(folder: &Path, command: &str) -> String {
    format!(
        "{}#{command}",
        PackageIdentity::local(folder).unwrap().key()
    )
}

/// A system with Firefox installed only, so no application of the machine
/// running the tests (Windows' own Calculator) is found by name.
struct OneApplication;

impl Applications for OneApplication {
    fn installed(&self) -> Result<Vec<Application>, String> {
        Ok(vec![Application {
            id: "/apps/Firefox.app".into(),
            name: "Firefox".into(),
            location: "/apps".into(),
            ..Application::default()
        }])
    }

    fn open(&self, _id: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Copies the package folder `from` to `to`, with everything it ships.
fn copy_folder(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            copy_folder(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}

/// Pane's data folder (quick slots beside the `extensions` folder, as the
/// application keeps them) and compiled code cache for one test.
struct Dirs {
    data: TempDir,
    cache: TempDir,
    /// Where the provider package copies live, outliving restarts.
    sources: TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            data: tempfile::tempdir().unwrap(),
            cache: tempfile::tempdir().unwrap(),
            sources: tempfile::tempdir().unwrap(),
        }
    }

    fn extensions(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// A launcher over the data folder, started as the application starts
    /// one: its packages, then its quick slots.
    fn start(&self) -> Launcher {
        let runtime = Runtime::start_with_cache(self.cache.path().to_path_buf()).unwrap();
        runtime.set_applications(Arc::new(OneApplication));
        Launcher::with_packages(Ok(runtime), vec![], self.extensions())
            .with_quick_slots(self.data.path())
    }

    /// A launcher with both providers installed.
    fn with_providers(&self) -> Launcher {
        let launcher = self.start();
        install(&launcher, &rust_provider(self));
        install(&launcher, &applications_provider(self));
        launcher.back();
        launcher
    }
}

fn install(launcher: &Launcher, folder: &Path) {
    block_on(launcher.install_package(folder));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
}

fn search(launcher: &Launcher, query: &str) {
    block_on(launcher.set_query(query));
}

/// Writes `json` as the record `file` in `dir`.
fn seed(dir: &Path, file: &str, json: serde_json::Value) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(file), serde_json::to_string_pretty(&json).unwrap()).unwrap();
}

fn read(dir: &Path, file: &str) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(dir.join(file)).unwrap()).unwrap()
}

#[test]
fn the_fixture_providers_declare_the_provider_mode() {
    let dirs = Dirs::new();
    for (folder, command) in [
        (rust_provider(&dirs), "sample"),
        (applications_provider(&dirs), "launch"),
    ] {
        let manifest = Manifest::read(&folder).unwrap();
        let declared = manifest
            .commands
            .iter()
            .find(|declared| declared.id == command)
            .unwrap();
        assert_eq!(declared.mode, CommandMode::Provider, "{command}");
    }
}

#[test]
fn typing_a_providers_name_finds_no_row_while_its_results_still_answer() {
    let dirs = Dirs::new();
    let launcher = dirs.with_providers();

    for query in ["rust", "sample", "javascript", "applications"] {
        search(&launcher, query);
        assert!(
            titles(&launcher).iter().all(|title| {
                title != "Rust sample" && title != "JavaScript applications sample"
            }),
            "{query}: {:?}",
            titles(&launcher)
        );
    }
    // The blank query lists no provider either.
    search(&launcher, "");
    assert!(
        titles(&launcher)
            .iter()
            .all(|title| { title != "Rust sample" && title != "JavaScript applications sample" }),
        "{:?}",
        titles(&launcher)
    );
    // Each application is still its own root result.
    search(&launcher, "fire");
    // Pane's install row matches the four letters fuzzily below the
    // application's prefix match (#193); the calculator's arithmetic left
    // with its sources (#285), the Rust sample answering in its place.
    assert_eq!(
        titles(&launcher),
        ["Launch Firefox", "Install extension from Git…"]
    );
    // The Rust sample still answers from the query, with its answer card.
    search(&launcher, "reverse 42");
    assert_eq!(titles(&launcher), ["24"]);
    let answer = launcher.presentation().rows[0].answer.clone().unwrap();
    assert_eq!(answer.command, "Rust sample");
}

#[test]
fn a_provider_cannot_be_aliased_given_a_hotkey_or_launched_and_shortcuts_do_not_list_it() {
    let dirs = Dirs::new();
    let launcher = dirs.with_providers();
    let rust = command_id(&rust_provider(&dirs), "sample");
    let applications = command_id(&applications_provider(&dirs), "launch");

    for id in [&rust, &applications] {
        let Err(refusal) = launcher.set_alias(id, "c") else {
            panic!("{id} was given an alias");
        };
        assert!(refusal.contains("only answers root search"), "{refusal}");
        let shortcut = Shortcut::parse("ctrl+alt+c").unwrap();
        let Err(refusal) = launcher.set_hotkey(id, Some(shortcut)) else {
            panic!("{id} was given a hotkey");
        };
        assert!(refusal.contains("only answers root search"), "{refusal}");
    }

    let catalog = launcher.shortcut_catalog();
    let listed: Vec<&str> = catalog
        .groups
        .iter()
        .flat_map(|group| &group.commands)
        .map(|command| command.id.as_str())
        .collect();
    assert!(!listed.contains(&rust.as_str()), "{listed:?}");
    assert!(!listed.contains(&applications.as_str()), "{listed:?}");

    // Nothing pins it: root search has no row of it to pin, and its
    // extensions' packages still list it as their root provider.
    let packages = launcher.packages();
    let providers: Vec<String> = packages
        .iter()
        .flat_map(|package| package.providers())
        .map(|provider| provider.title)
        .collect();
    assert_eq!(providers, ["Rust sample", "JavaScript applications sample"]);
    assert!(launcher.quick_slots().is_empty());
}

#[test]
fn disabling_a_provider_stops_its_results_and_enabling_brings_them_back() {
    let dirs = Dirs::new();
    let launcher = dirs.with_providers();
    let identity = PackageIdentity::local(&rust_provider(&dirs)).unwrap();
    search(&launcher, "reverse 42");
    assert_eq!(titles(&launcher), ["24"]);

    block_on(launcher.set_enabled(&identity, false));
    search(&launcher, "reverse 42 ");
    assert!(titles(&launcher).is_empty(), "{:?}", titles(&launcher));

    block_on(launcher.set_enabled(&identity, true));
    search(&launcher, "reverse 42");
    assert_eq!(titles(&launcher), ["24"]);
}

/// Installs a copy of the Rust provider whose command's entry `change`
/// rewrites; the status line.
fn install_changed(dirs: &Dirs, change: impl FnOnce(&mut serde_json::Value)) -> Status {
    let folder = dirs.data.path().join("source");
    copy_folder(&rust_provider(dirs), &folder);
    let mut manifest = read(&folder, "pane.json");
    change(&mut manifest["commands"][0]);
    fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
    let launcher = dirs.start();
    block_on(launcher.install_package(&folder));
    let status = launcher.view().status;
    assert!(launcher.packages().is_empty(), "installed: {status:?}");
    status
}

#[test]
fn a_provider_declaring_no_root_results_is_refused_with_the_reason() {
    let dirs = Dirs::new();
    let status = install_changed(&dirs, |command| {
        command.as_object_mut().unwrap().remove("rootResults");
    });
    let Status::Error(error) = status else {
        panic!("{status:?}");
    };
    assert!(
        error.contains(
            "command `sample` is a provider (\"mode\": \"provider\") but declares neither \
             `rootResults` nor `indexedResults`"
        ),
        "{error}"
    );
}

#[test]
fn a_provider_asking_for_what_only_a_launched_command_uses_is_refused() {
    for (field, value, reason) in [
        ("search", serde_json::json!(true), "`search`"),
        ("takesQuery", serde_json::json!(true), "`takesQuery`"),
        (
            "schedule",
            serde_json::json!({ "everySeconds": 60 }),
            "`schedule`",
        ),
    ] {
        let dirs = Dirs::new();
        let status = install_changed(&dirs, |command| {
            command[field] = value;
        });
        let Status::Error(error) = status else {
            panic!("{field}: {status:?}");
        };
        assert!(
            error
                .contains("is a provider (\"mode\": \"provider\"), which only answers root search")
                && error.contains(reason),
            "{field}: {error}"
        );
    }
}

/// A data folder from before #164: Calculator and Applications were view
/// commands with rows, pinned, aliased, a fallback and given hotkeys. A
/// start drops all of it, once, with a toast naming it, and keeps a pinned
/// application, which is still its own root result.
#[test]
fn what_was_recorded_for_a_command_that_became_a_provider_is_dropped_once_with_a_toast() {
    let dirs = Dirs::new();
    drop(dirs.with_providers());
    let rust = command_id(&rust_provider(&dirs), "sample");
    let applications = command_id(&applications_provider(&dirs), "launch");
    let extensions = dirs.extensions();
    seed(
        &extensions,
        "aliases.json",
        serde_json::json!({
            "version": 1,
            "aliases": { rust.clone(): "c" },
            "fallbacks": [applications.clone()],
        }),
    );
    seed(
        &extensions,
        "hotkeys.json",
        serde_json::json!({
            "version": 1,
            "hotkeys": { rust.clone(): "ctrl+alt+c" },
        }),
    );
    seed(
        dirs.data.path(),
        "quick-slots.json",
        serde_json::json!({
            "version": 2,
            "pins": [
                { "command": rust.clone() },
                { "command": applications.clone(), "result": "/apps/Firefox.app" },
                { "command": applications.clone() },
            ],
        }),
    );

    let launcher = dirs.start();

    // The records hold nothing for either provider any more.
    let aliases = read(&extensions, "aliases.json");
    assert_eq!(aliases["aliases"], serde_json::json!({}));
    assert_eq!(aliases["fallbacks"], serde_json::json!([]));
    let hotkeys = read(&extensions, "hotkeys.json");
    assert_eq!(hotkeys["hotkeys"], serde_json::json!({}));
    // The pinned application keeps its slot.
    let slots = read(dirs.data.path(), "quick-slots.json");
    assert_eq!(
        slots["pins"],
        serde_json::json!([{ "command": applications, "result": "/apps/Firefox.app" }])
    );
    assert_eq!(launcher.quick_slots().len(), 1);
    // And a toast names what went.
    let toast = launcher
        .toast()
        .expect("a toast names what was removed")
        .toast;
    assert_eq!(toast.style, ToastStyle::Success);
    assert_eq!(
        toast.title,
        "Rust sample and JavaScript applications sample now only answer root search"
    );
    assert_eq!(
        toast.message.as_deref(),
        Some(
            "They have no row of their own now, so Pane removed the pin, alias and hotkey of \
             Rust sample and the pin and fallback of JavaScript applications sample."
        )
    );
    drop(launcher);

    // Once: the next start has nothing to drop and says nothing.
    let launcher = dirs.start();
    assert!(launcher.toast().is_none(), "{:?}", launcher.toast());
    assert_eq!(launcher.quick_slots().len(), 1);
}
