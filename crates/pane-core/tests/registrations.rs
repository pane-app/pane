//! Owned registrations through the launcher's public interface (#158,
//! ADR 0041): a dynamic root item, a timer, a folder watcher and a
//! run-time provision, each an owned registration the guest holds as a
//! resource, with the registrations sample in Rust, JavaScript and
//! TypeScript (real guests `cargo xtask guests` assembles) and the
//! registrations fixture (also Rust) for the edges. What is registered is
//! undone when the handle is dropped, when the instance holding it goes
//! and when the generation ends — a disable, a pause, a reload, an update
//! or an uninstall — leaving the generation's undo list empty (#136's
//! test hook); nothing an extension registered outlives its code, and the
//! activation entry point makes the registrations again without waiting
//! for the user. The consumers of a run-time capability see the provider
//! only while it holds its provision: they wait for it and come back, or
//! fall back to another provider. Timers fire by the launcher's clock,
//! coalescing while the package waits; a watcher's changes coalesce.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::clipboard::{Clock, ManualClock, SystemClock};
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{Launcher, PackageIdentity, ResultAction, Runtime, Status, Unavailable};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{manage, select_title, to_root};

/// The text the status or toast shows, for asserting on.
fn said(status: Status) -> String {
    match status {
        Status::Result(text) | Status::Error(text) | Status::Progress(text) => text,
        Status::Idle | Status::Running { .. } => String::new(),
    }
}

/// One language's registrations sample.
struct Sample {
    /// Its assembled package under `target/guests/packages`.
    package: &'static str,
    /// Its package's title, as the install reports it.
    title: &'static str,
    /// Its command's title in root search.
    command: &'static str,
    /// The word its dynamic root item's subtitle ends with, saying which
    /// language answered.
    language: &'static str,
}

const SAMPLES: [Sample; 3] = [
    Sample {
        package: "sample-registrations",
        title: "Rust registrations sample",
        command: "Registrations",
        language: "Rust",
    },
    Sample {
        package: "sample-registrations-js",
        title: "JavaScript registrations sample",
        command: "Registrations (JavaScript)",
        language: "JavaScript",
    },
    Sample {
        package: "sample-registrations-ts",
        title: "TypeScript registrations sample",
        command: "Registrations (TypeScript)",
        language: "TypeScript",
    },
];

/// The capability the sample provides at run time, as the capabilities
/// sample uses it.
const GREET: &str = "pane-samples:greet@1";

/// How long the timers and watchers threads, an activation and a guest
/// call may take: compiling the guest once is included; a slow, busy
/// machine is not.
const PROMPTLY: Duration = Duration::from_secs(20);

const SECOND: Duration = Duration::from_secs(1);

/// One test's Pane: its data folder (outliving restarts), its source
/// folders, its runtime cache, the clock its timers follow and the hotkey
/// system its global hotkeys go through.
struct Pane {
    sources: TempDir,
    data: TempDir,
    cache: TempDir,
    /// Pane's clock for timers, scheduled work and services, which moves
    /// only when a test advances it.
    clock: Arc<ManualClock>,
    /// A hotkey system that accepts anything, so a test can record and
    /// press a global hotkey.
    hotkeys: Arc<AnyHotkeys>,
}

/// A hotkey system that accepts every shortcut, remembering what it
/// registered.
struct AnyHotkeys {
    registered: Mutex<Vec<Shortcut>>,
}

impl Hotkeys for AnyHotkeys {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
        self.registered.lock().unwrap().push(shortcut.clone());
        Ok(())
    }

    fn unregister(&self, shortcut: &Shortcut) {
        self.registered
            .lock()
            .unwrap()
            .retain(|held| held != shortcut);
    }
}

impl Pane {
    fn new() -> Pane {
        Pane {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            cache: tempfile::tempdir().unwrap(),
            clock: ManualClock::at(SystemClock.now() + 365 * 86_400_000),
            hotkeys: Arc::new(AnyHotkeys {
                registered: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Starts Pane on this data folder, as after a restart.
    fn start(&self) -> Launcher {
        let runtime = Runtime::start_with_cache(self.cache.path().to_path_buf()).unwrap();
        Launcher::with_packages(
            Ok(runtime.clone()),
            vec![],
            self.data.path().join("extensions"),
        )
        .with_clock(self.clock.clone())
        .with_hotkeys(self.hotkeys.clone())
    }

    /// Starts Pane with `sample`'s package installed and activated.
    fn installed(&self, sample: &Sample) -> (Launcher, PackageIdentity, PathBuf) {
        let started = self.start();
        let folder = self.package(sample);
        block_on(started.install_package(&folder));
        assert_eq!(
            started.view().status,
            Status::Result(format!("Installed {}", sample.title))
        );
        let identity = PackageIdentity::local(&folder).unwrap();
        (started, identity, folder)
    }

    /// The assembled sample of `sample` copied into a source folder of
    /// this test.
    fn package(&self, sample: &Sample) -> PathBuf {
        let folder = self.sources.path().join(sample.package);
        let assembled = assembled(sample.package);
        fs::create_dir_all(&folder).unwrap();
        for file in ["pane.json", component_of(sample.package)] {
            fs::copy(assembled.join(file), folder.join(file)).unwrap();
        }
        folder
    }

    /// How many firings or changes the package of `folder` has counted in
    /// its content, or `None` before the first.
    fn counted(&self, folder: &Path, key: &str) -> Option<u64> {
        let text = fs::read_to_string(self.data.path().join("extensions/content.json")).ok()?;
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        file["packages"][&PackageIdentity::local(folder).unwrap().key()][key]
            .as_str()
            .and_then(|count| count.parse().ok())
    }

    /// Waits until the timers thread looked at every change of the clock
    /// and the registry so far, and every firing it started has reported.
    fn timers_settled(&self, launcher: &Launcher) {
        assert!(
            launcher.wait_for_timers(PROMPTLY),
            "the timers did not settle"
        );
    }

    /// Waits until `read` returns `expected`, saying `what` it was.
    fn until<T: PartialEq + std::fmt::Debug>(
        &self,
        what: &str,
        expected: T,
        mut read: impl FnMut() -> T,
    ) {
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

    /// The capabilities sample, as the consumer of the run-time
    /// capability.
    fn capabilities_package(&self) -> PathBuf {
        let folder = self.sources.path().join("sample-capabilities");
        let assembled = assembled("sample-capabilities");
        fs::create_dir_all(&folder).unwrap();
        for file in ["pane.json", "sample_capabilities.wasm"] {
            fs::copy(assembled.join(file), folder.join(file)).unwrap();
        }
        folder
    }

    /// The registrations fixture as a package of this test, with
    /// `commands` and the manifest members `members` beside the fixture's
    /// component, with the settings `settings` saved for it.
    fn fixture_package(
        &self,
        name: &str,
        commands: &str,
        members: &str,
        settings: &[(&str, &str)],
    ) -> PathBuf {
        let folder = self.sources.path().join(name);
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/registrations_fixture.wasm");
        assert!(
            fixture.exists(),
            "{} is missing; run `cargo xtask guests`",
            fixture.display()
        );
        fs::create_dir_all(&folder).unwrap();
        fs::copy(&fixture, folder.join("fixture.wasm")).unwrap();
        let manifest = format!(
            "{{ \"manifestVersion\": 1, \"title\": \"Registrations fixture\", \
             \"version\": \"0.1.0\", \"apiVersion\": \"0.1\", \
             \"commands\": [{commands}]{members} }}"
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        for (key, value) in settings {
            let file = self.data.path().join("extensions/settings.json");
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            let mut saved: serde_json::Value = match fs::read_to_string(&file) {
                Ok(text) => serde_json::from_str(&text).unwrap(),
                Err(_) => serde_json::json!({ "version": 1, "packages": {} }),
            };
            let identity = PackageIdentity::local(&folder).unwrap();
            let entry = saved["packages"]
                .as_object_mut()
                .unwrap()
                .entry(identity.key())
                .or_insert_with(|| serde_json::json!({}));
            entry[key] = serde_json::Value::String((*value).to_owned());
            fs::write(&file, serde_json::to_string(&saved).unwrap()).unwrap();
        }
        folder
    }
}

/// The assembled package `name` under `target/guests/packages`.
fn assembled(name: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(name);
    assert!(
        folder.exists(),
        "{} is missing; run `cargo xtask guests`",
        folder.display()
    );
    folder
}

/// The component file `package`'s manifest names.
fn component_of(package: &str) -> &'static str {
    match package {
        "sample-registrations" => "sample_registrations.wasm",
        "sample-registrations-js" => "sample_registrations_js.wasm",
        "sample-registrations-ts" => "sample_registrations_ts.wasm",
        _ => "fixture.wasm",
    }
}

/// The subtitle the sample's dynamic root item shows, saying where its
/// counts stand.
fn item_subtitle(sample: &Sample, ticks: u64, changes: u64) -> String {
    format!(
        "{ticks} timer firings, {changes} watcher changes ({})",
        sample.language
    )
}

/// The row of the sample's dynamic root item in root search.
fn dynamic_row(launcher: &Launcher) -> pane_core::Row {
    launcher
        .view()
        .rows
        .iter()
        .find(|row| row.id.ends_with(":counted"))
        .expect("the dynamic root item's row")
        .clone()
}

/// The row of the fixture's command in root search.
fn fixture_row(launcher: &Launcher) -> pane_core::Row {
    launcher
        .view()
        .rows
        .iter()
        .find(|row| row.id.ends_with("#fixture"))
        .expect("the fixture's row")
        .clone()
}

/// The dynamic root item appears, updates and disappears in root search,
/// in all three languages: the activation entry point registers it as
/// Pane may run the code, the timer updates it, and the generation's end
/// removes it, leaving the undo list empty. Enabling the package again
/// activates it afresh, and a restart of Pane does too.
#[test]
fn a_dynamic_root_item_appears_updates_and_disappears_in_root_search() {
    for sample in SAMPLES {
        let pane = Pane::new();
        let (launcher, identity, _folder) = pane.installed(&sample);
        // The activation entry point registered the item: the row is
        // listed with nothing counted yet.
        pane.until("the dynamic root item's row", true, || {
            launcher
                .view()
                .rows
                .iter()
                .any(|row| row.id.ends_with(":counted"))
        });
        let row = dynamic_row(&launcher);
        assert_eq!(row.title, "Registrations: counting");
        assert_eq!(row.subtitle, Some(item_subtitle(&sample, 0, 0)));
        // The undo list holds what the activation registered: the item
        // and the timer (no watcher: no folder is named). The row appears
        // as the item is registered, before the activation has come to the
        // timer, so the timer is waited for.
        assert!(launcher.undo_list(&identity).contains(&"dynamic root item"));
        pane.until("the activation's timer", true, || {
            launcher.undo_list(&identity).contains(&"timer")
        });
        // The timer fires and updates the row: the thread has begun the
        // timer's interval where the clock stands.
        pane.timers_settled(&launcher);
        pane.clock.advance(SECOND);
        pane.timers_settled(&launcher);
        pane.until("the row's count", item_subtitle(&sample, 1, 0), || {
            dynamic_row(&launcher).subtitle.unwrap()
        });
        // Disabling ends the generation: the row goes, and the undo list
        // is empty.
        block_on(launcher.set_enabled(&identity, false));
        pane.until("the row's removal", false, || {
            launcher
                .view()
                .rows
                .iter()
                .any(|row| row.id.ends_with(":counted"))
        });
        assert_eq!(launcher.undo_list(&identity), Vec::<&str>::new());
        // Enabling activates again: the item is registered afresh, on a
        // new generation.
        block_on(launcher.set_enabled(&identity, true));
        pane.until("the dynamic root item's row again", true, || {
            launcher
                .view()
                .rows
                .iter()
                .any(|row| row.id.ends_with(":counted"))
        });
        // A restart of Pane starts the activation again where the package
        // is enabled, and the row is back.
        let restarted = pane.start();
        pane.until("the dynamic root item's row after a restart", true, || {
            restarted
                .view()
                .rows
                .iter()
                .any(|row| row.id.ends_with(":counted"))
        });
    }
}

/// A dynamic root item's actions run its command's event entry point, as
/// often as the user chooses: the sample's "Add one" counts in its
/// content, and each invocation of the row runs it again.
#[test]
fn a_dynamic_root_items_action_runs_as_often_as_chosen() {
    let pane = Pane::new();
    let (launcher, _identity, folder) = pane.installed(&SAMPLES[0]);
    pane.until("the dynamic root item's row", true, || {
        launcher
            .view()
            .rows
            .iter()
            .any(|row| row.id.ends_with(":counted"))
    });
    let ticks = pane.counted(&folder, "ticks").unwrap_or(0);
    for added in 1..=2 {
        to_root(&launcher);
        select_title(&launcher, "Registrations: counting");
        block_on(launcher.activate_selected());
        pane.until("the action's count", ticks + added, || {
            pane.counted(&folder, "ticks").unwrap_or(0)
        });
    }
}

/// The folder watcher delivers coalesced changes: those arriving within
/// half a second of the first are one event, whose paths all arrive, and
/// the item's row says how many changes there have been. The watcher goes
/// with the generation, leaving the undo list empty.
#[test]
fn a_folder_watcher_delivers_coalesced_changes() {
    let pane = Pane::new();
    let watched = tempfile::tempdir().unwrap();
    // The sample watches the folder its settings name: saved before the
    // package is installed, so its activation reads it.
    let folder = pane.package(&SAMPLES[0]);
    let identity = PackageIdentity::local(&folder).unwrap();
    let file = pane.data.path().join("extensions/settings.json");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let settings = serde_json::json!({
        "version": 1,
        "packages": { identity.key(): { "folder": watched.path().display().to_string() } }
    });
    fs::write(&file, serde_json::to_string(&settings).unwrap()).unwrap();
    let (launcher, identity, _folder) = pane.installed(&SAMPLES[0]);
    pane.until("the dynamic root item's row", true, || {
        launcher
            .view()
            .rows
            .iter()
            .any(|row| row.id.ends_with(":counted"))
    });
    assert!(launcher.undo_list(&identity).contains(&"folder watcher"));
    // Two files written at once: one coalesced event, both paths in it.
    fs::write(watched.path().join("one.txt"), "one").unwrap();
    fs::write(watched.path().join("two.txt"), "two").unwrap();
    let folder = pane.sources.path().join("sample-registrations");
    pane.until("the watcher's changes", 2, || {
        pane.counted(&folder, "changes").unwrap_or(0)
    });
    // A third file, after the coalescing window: another event.
    thread::sleep(Duration::from_millis(700));
    fs::write(watched.path().join("three.txt"), "three").unwrap();
    pane.until("the watcher's changes again", 3, || {
        pane.counted(&folder, "changes").unwrap_or(0)
    });
    assert!(
        launcher.wait_for_watchers(PROMPTLY),
        "the watchers did not settle"
    );
    // The row says where the counts stand.
    pane.until(
        "the row's changes",
        item_subtitle(&SAMPLES[0], 0, 3),
        || dynamic_row(&launcher).subtitle.unwrap(),
    );
    // Disabling ends the generation: the watcher goes with it.
    block_on(launcher.set_enabled(&identity, false));
    assert_eq!(launcher.undo_list(&identity), Vec::<&str>::new());
}

/// A run-time provision makes the package a provider only while it holds
/// one: the capabilities sample, which calls every provider of the greet
/// capability, answers from the registrations sample while the provision
/// is held and not after it is dropped.
#[test]
fn a_run_time_provision_makes_the_package_a_provider_only_while_held() {
    let pane = Pane::new();
    let (launcher, identity, _folder) = pane.installed(&SAMPLES[0]);
    let consumer = pane.capabilities_package();
    block_on(launcher.install_package(&consumer));
    // With no provision held, the fan-out answers without the provider.
    assert!(
        !every_provider(&launcher).contains("registrations sample"),
        "the provider answered with no provision held"
    );
    // Signing in holds the provision: the provider is one of the
    // fan-out's answers.
    open_and_run(&launcher, SAMPLES[0].command, "Sign in to the provision");
    pane.until("the provision's undo entry", true, || {
        launcher
            .undo_list(&identity)
            .contains(&"run-time provision")
    });
    assert!(
        every_provider(&launcher).contains("Rust registrations sample"),
        "the provider did not answer with the provision held"
    );
    // Signing out drops it: the provider is no longer among them, and its
    // entry on the undo list is gone.
    open_and_run(&launcher, SAMPLES[0].command, "Sign out of the provision");
    assert!(
        !every_provider(&launcher).contains("registrations sample"),
        "the provider answered after the provision was dropped"
    );
    assert!(
        !launcher
            .undo_list(&identity)
            .contains(&"run-time provision")
    );
}

/// A consumer that requires the capability waits while no provider holds
/// a provision, and comes back when one does: its command's row says why
/// it waits, and its timer runs none of its code meanwhile — firings that
/// fall due while it waits are one, delivered when it can run again.
#[test]
fn a_waiting_package_runs_no_timer_code_until_the_provision_comes() {
    let pane = Pane::new();
    let (launcher, _provider_identity, _folder) = pane.installed(&SAMPLES[0]);
    // The fixture uses the capability the sample provides at run time,
    // required, so it waits until the provision is held.
    let fixture = pane.fixture_package(
        "waiting",
        r#"{ "id": "fixture", "title": "Registrations fixture", "component": "fixture.wasm" }"#,
        &format!(
            r#","uses": [{{ "capability": "{GREET}", "operations": ["greet"] }}],
             "activate": "fixture.wasm""#
        ),
        &[],
    );
    block_on(launcher.install_package(&fixture));
    let identity = PackageIdentity::local(&fixture).unwrap();
    // Nothing provides the capability: the fixture's command waits,
    // saying what it needs, and its activation has not run.
    pane.until(
        "the waiting row",
        "Needs pane-samples:greet@1: no extension provides it".to_owned(),
        || {
            to_root(&launcher);
            match fixture_row(&launcher).unavailable {
                Some(Unavailable::Waiting(reason)) => reason,
                _ => String::new(),
            }
        },
    );
    assert_eq!(launcher.undo_list(&identity), Vec::<&str>::new());
    // Signing in holds the provision: the fixture comes back by itself,
    // its activation runs, and its timer counts firings.
    open_and_run(&launcher, SAMPLES[0].command, "Sign in to the provision");
    pane.until("the activation's timer", true, || {
        launcher.undo_list(&identity).contains(&"timer")
    });
    pane.timers_settled(&launcher);
    pane.clock.advance(3 * SECOND);
    pane.timers_settled(&launcher);
    assert_eq!(pane.counted(&fixture, "fired"), Some(3));
    // Signing out drops the provision: the fixture waits again, and its
    // timer runs none of its code while it does.
    open_and_run(&launcher, SAMPLES[0].command, "Sign out of the provision");
    pane.until(
        "the waiting row again",
        "Needs pane-samples:greet@1: no extension provides it".to_owned(),
        || {
            to_root(&launcher);
            match fixture_row(&launcher).unavailable {
                Some(Unavailable::Waiting(reason)) => reason,
                _ => String::new(),
            }
        },
    );
    pane.clock.advance(5 * SECOND);
    pane.timers_settled(&launcher);
    assert_eq!(
        pane.counted(&fixture, "fired"),
        Some(3),
        "a firing ran while the package waited"
    );
    // The provision comes back: one coalesced firing is delivered, not
    // five.
    open_and_run(&launcher, SAMPLES[0].command, "Sign in to the provision");
    pane.timers_settled(&launcher);
    pane.until("the coalesced firing", 4, || {
        pane.counted(&fixture, "fired").unwrap_or(0)
    });
    assert_eq!(pane.counted(&fixture, "fired"), Some(4));
}

/// The limits are refused with the limit named, the timer bounds with the
/// bounds, and a provision the manifest does not declare, or does not
/// mark `atRunTime`, with the reason: the fixture's items answer each
/// refusal as a toast, and nothing is registered by the refused call.
#[test]
fn beyond_the_limits_bounds_and_declarations_is_refused() {
    let pane = Pane::new();
    let launcher = pane.start();
    // The fixture declares the capability unmarked, so providing it at
    // run time is refused, and its activation entry point is declared.
    let fixture = pane.fixture_package(
        "edges",
        r#"{ "id": "fixture", "title": "Registrations fixture", "component": "fixture.wasm" }"#,
        r#","provides": [{{ "capability": "fixture:held@1", "component": "fixture.wasm",
             "operations": ["greet"] }}], "activate": "fixture.wasm""#,
        &[("many", "1000")],
    );
    block_on(launcher.install_package(&fixture));
    let identity = PackageIdentity::local(&fixture).unwrap();
    pane.until("the activation's timer", true, || {
        launcher.undo_list(&identity).contains(&"timer")
    });
    open_fixture(&launcher);
    run_fixture_item(&launcher, "Register many items");
    assert!(
        said(shown(&launcher)).contains("1000 of Pane's limit of 1000 dynamic root items"),
        "the items limit is not named: {:?}",
        shown(&launcher)
    );
    pane.save_setting(&identity, "many", "64");
    open_fixture(&launcher);
    run_fixture_item(&launcher, "Register many timers");
    assert!(
        said(shown(&launcher)).contains("64 of Pane's limit of 64 timers"),
        "the timers limit is not named: {:?}",
        shown(&launcher)
    );
    run_fixture_item(&launcher, "Register a 0-second timer");
    assert!(
        said(shown(&launcher)).contains("1 second to 30 days"),
        "the bounds are not named: {:?}",
        shown(&launcher)
    );
    run_fixture_item(&launcher, "Register a 31-day timer");
    assert!(
        said(shown(&launcher)).contains("1 second to 30 days"),
        "the bounds are not named: {:?}",
        shown(&launcher)
    );
    run_fixture_item(&launcher, "Provide the capability");
    assert!(
        said(shown(&launcher)).contains("provides `fixture:held@1` whenever it can run"),
        "the unmarked provision is not refused: {:?}",
        shown(&launcher)
    );
    run_fixture_item(&launcher, "Provide the undeclared capability");
    assert!(
        said(shown(&launcher)).contains("declares no `provides` entry"),
        "the undeclared provision is not refused: {:?}",
        shown(&launcher)
    );
    // Watching a path that is not there is refused with the reason.
    pane.save_setting(&identity, "folder", "/not/there");
    run_fixture_item(&launcher, "Watch the folder");
    assert!(
        said(shown(&launcher)).contains("there is nothing there"),
        "the unwatchable path is not refused: {:?}",
        shown(&launcher)
    );
    // Only the activation's timer is registered: nothing the refused
    // calls would have made, beside the instance's own undo entry.
    let undo = launcher.undo_list(&identity);
    assert!(undo.contains(&"timer"), "the activation's timer: {undo:?}");
    assert!(
        !undo.contains(&"dynamic root item") && !undo.contains(&"folder watcher"),
        "the refused calls registered something: {undo:?}"
    );
}

impl Pane {
    /// Saves `value` under `key` in `identity`'s settings, where its code
    /// reads it.
    fn save_setting(&self, identity: &PackageIdentity, key: &str, value: &str) {
        let file = self.data.path().join("extensions/settings.json");
        let mut saved: serde_json::Value = match fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text).unwrap(),
            Err(_) => serde_json::json!({ "version": 1, "packages": {} }),
        };
        let entry = saved["packages"]
            .as_object_mut()
            .unwrap()
            .entry(identity.key())
            .or_insert_with(|| serde_json::json!({}));
        entry[key] = serde_json::Value::String(value.to_owned());
        fs::write(&file, serde_json::to_string(&saved).unwrap()).unwrap();
    }
}

/// An activation entry point that traps is a crash of the package,
/// counted towards pausing it: three within the window pause it. Each
/// trapped instance is dropped while the generation continues, and the
/// activation starts again — that is what counts them.
#[test]
fn a_trapping_activation_counts_towards_pausing() {
    let pane = Pane::new();
    let launcher = pane.start();
    let fixture = pane.fixture_package(
        "trapping",
        r#"{ "id": "fixture", "title": "Registrations fixture", "component": "fixture.wasm" }"#,
        r#","activate": "fixture.wasm""#,
        &[("trap", "yes")],
    );
    block_on(launcher.install_package(&fixture));
    // The activation traps, is started again, traps again: three within
    // the window pause the package.
    pane.until("the pause", true, || {
        matches!(
            fixture_row(&launcher).unavailable,
            Some(Unavailable::Paused(_))
        )
    });
    assert!(
        matches!(
            fixture_row(&launcher).unavailable,
            Some(Unavailable::Paused(_))
        ),
        "the trapping activation did not pause the package"
    );
}

/// A quick slot and a global hotkey hold a dynamic root item by its
/// command and item id, and while it is not registered they say why: the
/// slot keeps its place, and the hotkey explains instead of launching.
#[test]
fn a_quick_slot_and_a_hotkey_of_a_dynamic_item_say_why() {
    let pane = Pane::new();
    let (launcher, identity, _folder) = pane.installed(&SAMPLES[0]);
    pane.until("the dynamic root item's row", true, || {
        launcher
            .view()
            .rows
            .iter()
            .any(|row| row.id.ends_with(":counted"))
    });
    let row = dynamic_row(&launcher);
    // A quick slot holds it: the slot runs while the item is registered.
    to_root(&launcher);
    select_title(&launcher, "Registrations: counting");
    let (change, recorded) = launcher.change_quick_slots(&row.id, ResultAction::Pin);
    assert!(matches!(change, pane_core::SlotChange::Changed(Some(0))));
    block_on(recorded);
    let slots = launcher.quick_slots();
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].title, "Registrations: counting");
    assert!(slots[0].ready());
    // A global hotkey holds it too: pressing it launches the command.
    let shortcut = Shortcut::parse("ctrl+alt+r").unwrap();
    block_on(
        launcher
            .set_hotkey(&row.id, Some(shortcut.clone()))
            .unwrap(),
    );
    assert!(
        launcher.press_hotkey(&shortcut).is_some(),
        "the hotkey launches the dynamic item's command"
    );
    // The generation ends: the item is not registered, and each says why.
    block_on(launcher.set_enabled(&identity, false));
    let slots = launcher.quick_slots();
    assert_eq!(slots.len(), 1, "the slot keeps its place");
    assert_eq!(
        slots[0].unavailable.as_deref(),
        Some("Registrations no longer lists it")
    );
    if let Some(future) = launcher.press_hotkey(&shortcut) {
        block_on(future);
        assert_eq!(
            launcher.view().status,
            Status::Error("Registrations no longer lists it".into()),
            "the hotkey says why"
        );
    } else {
        panic!("the hotkey pressed nothing");
    }
}

/// An alias holds a dynamic root item by its row id: typing it finds the
/// row, and while the item is not registered the choice shows as not
/// active in the extension list, saying why.
#[test]
fn an_alias_of_a_dynamic_item_says_why_while_it_is_not_registered() {
    let pane = Pane::new();
    let (launcher, identity, _folder) = pane.installed(&SAMPLES[0]);
    pane.until("the dynamic root item's row", true, || {
        launcher
            .view()
            .rows
            .iter()
            .any(|row| row.id.ends_with(":counted"))
    });
    let row = dynamic_row(&launcher);
    // Record the alias from the Actions panel, as the user would.
    to_root(&launcher);
    select_title(&launcher, "Registrations: counting");
    assert!(
        launcher.open_result_action(&row.id, pane_core::ResultAction::Alias),
        "the alias form opened for the dynamic item's row"
    );
    let form = launcher.view().form().cloned().expect("the alias form");
    launcher.set_field_value(&form.fields[0].id, "reg");
    block_on(launcher.submit_form());
    // Typing the alias finds the row.
    to_root(&launcher);
    block_on(launcher.set_query("reg"));
    assert!(
        launcher
            .view()
            .rows
            .iter()
            .any(|row| row.id.ends_with(":counted")),
        "the alias finds the dynamic item's row"
    );
    // The generation ends: the item is not registered, and the alias
    // shows as not active in the extension list, saying why.
    block_on(launcher.set_enabled(&identity, false));
    manage(&launcher);
    let view = launcher.view();
    let unlisted = view
        .rows
        .iter()
        .find(|row| row.id.starts_with("unlisted-setting:"))
        .expect("the alias's row in the extension list");
    assert!(
        unlisted
            .subtitle
            .as_deref()
            .unwrap_or("")
            .contains("no longer lists it"),
        "the alias says why it is not active: {:?}",
        unlisted.subtitle
    );
    let _ = identity;
}

/// A reload or an update ends the generation, undoing every registration:
/// the sample's item, timer and provision go, and activating again makes
/// them afresh on the new code.
#[test]
fn a_reload_undoes_the_registrations_and_activates_again() {
    let pane = Pane::new();
    let (launcher, identity, folder) = pane.installed(&SAMPLES[0]);
    open_and_run(&launcher, SAMPLES[0].command, "Sign in to the provision");
    pane.until("the registrations", 3, || {
        launcher
            .undo_list(&identity)
            .iter()
            .filter(|what| **what != "extension instance")
            .count()
    });
    // The package's code is replaced from its source folder: a reload,
    // with the command's screen on display, so it opens again on the new
    // code (ADR 0041).
    block_on(launcher.reload(&identity));
    // The undo list ran with the old generation and is empty; activating
    // again registered the item and the timer afresh, listed in root
    // search.
    to_root(&launcher);
    pane.until("the item again", true, || {
        launcher
            .view()
            .rows
            .iter()
            .any(|row| row.id.ends_with(":counted"))
    });
    assert!(
        launcher.undo_list(&identity).contains(&"dynamic root item"),
        "the activation did not register the item again: {:?}",
        launcher.undo_list(&identity)
    );
    assert!(
        !launcher
            .undo_list(&identity)
            .contains(&"run-time provision"),
        "the provision was not undone by the reload"
    );
    let _ = folder;
}

/// Activates the "Greet every provider" item of the capabilities sample
/// and answers what it showed.
fn every_provider(launcher: &Launcher) -> String {
    to_root(launcher);
    select_title(launcher, "Greet from Rust");
    block_on(launcher.activate_selected());
    select_title(launcher, "Greet every provider");
    block_on(launcher.activate_selected());
    said(shown(launcher))
}

/// Opens `command`'s screen in `launcher` and runs the action of its item
/// titled `item`.
fn open_and_run(launcher: &Launcher, command: &str, item: &str) {
    to_root(launcher);
    select_title(launcher, command);
    block_on(launcher.activate_selected());
    select_title(launcher, item);
    block_on(launcher.activate_selected());
}

/// Opens the fixture command's screen.
fn open_fixture(launcher: &Launcher) {
    to_root(launcher);
    select_title(launcher, "Registrations fixture");
    block_on(launcher.activate_selected());
}

/// Runs the action of the fixture's item titled `title`, from its
/// command's screen.
fn run_fixture_item(launcher: &Launcher, title: &str) {
    select_title(launcher, title);
    block_on(launcher.activate_selected());
}
