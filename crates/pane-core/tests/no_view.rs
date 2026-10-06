//! No-view commands and the launch record through the launcher's public
//! interface (ADR 0037), with the no-view sample in Rust, JavaScript and
//! TypeScript, real guests `cargo xtask guests` assembles: a command whose
//! `pane.json` entry says `"mode": "no-view"` runs its `run` once on every
//! way in (Enter, its alias, a fallback, its global hotkey, its quick slot,
//! another command, its schedule) with the launch record of that way in,
//! and opens no screen; a view command receives its launch record too; an
//! error a no-view command answers is shown and never pauses it, while a
//! crash counts as before; a schedule without an item runs the command in
//! the background every interval; and a command launches another of its
//! own package or of another, passing JSON context, with a background
//! launch of a view command and an unknown or disabled target refused.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::clipboard::{Clock, ManualClock, SystemClock};
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{
    CommandMode, Launcher, Manifest, PackageIdentity, ResultAction, Runtime, Screen, Status,
};
use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

use rows::{manage, select_title, titles};

/// One language's no-view sample package.
struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    /// Its package's title.
    title: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-no-view",
    title: "No-view sample",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-no-view-js",
    title: "JavaScript no-view sample",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-no-view-ts",
    title: "TypeScript no-view sample",
};

/// How long a launch, the scheduler and a run may take: compiling the
/// guest once is included; a slow, busy machine is not.
const PROMPTLY: Duration = Duration::from_secs(30);

const MINUTE: Duration = Duration::from_secs(60);

/// What the sample's commands answer with no launch context.
const NO_CONTEXT: &str = "none";

/// The context the sample's "Launch" passes.
const CONTEXT: &str = r#"{"from":"launch"}"#;

/// What "Report launch" answers when launched as described.
fn report(launch_type: &str, source: &str, text: &str, context: &str) -> Status {
    Status::Result(format!(
        "Report: {}",
        described(launch_type, source, text, context)
    ))
}

/// A launch record as the sample describes it.
fn described(launch_type: &str, source: &str, text: &str, context: &str) -> String {
    format!(
        "{launch_type} from {source}; fallback text: {text}; context: {context}; arguments: none"
    )
}

/// An error the sample answered with, as Pane shows it.
fn answered_error(message: &str) -> Status {
    Status::Error(format!("The extension reported an error: {message}"))
}

/// A system whose global hotkeys always register.
#[derive(Default)]
struct FakeHotkeys {
    registered: Mutex<Vec<Shortcut>>,
}

impl Hotkeys for FakeHotkeys {
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
            .retain(|kept| kept != shortcut);
    }
}

/// One test's Pane: its folders, the clock its schedules follow, and the
/// launcher.
struct Pane {
    sources: TempDir,
    _data: TempDir,
    clock: Arc<ManualClock>,
    launcher: Launcher,
}

impl Pane {
    fn new() -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let clock = ManualClock::at(SystemClock.now() + 365 * 86_400_000);
        let launcher =
            Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
                .with_clock(clock.clone())
                .with_hotkeys(Arc::new(FakeHotkeys::default()))
                .with_quick_slots(data.path());
        Pane {
            sources,
            _data: data,
            clock,
            launcher,
        }
    }

    /// The assembled package `name` copied into the source folder
    /// `folder`.
    fn source(&self, name: &str, folder: &str) -> PathBuf {
        let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages")
            .join(name);
        assert!(
            assembled.exists(),
            "{} is missing; run `cargo xtask guests`",
            assembled.display()
        );
        let folder = self.sources.path().join(folder);
        fs::create_dir_all(&folder).unwrap();
        for entry in fs::read_dir(&assembled).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
        }
        folder
    }

    /// Installs `fixture`'s sample from the source folder `folder`.
    fn install(&self, fixture: &Fixture, folder: &str) -> PathBuf {
        let folder = self.source(fixture.package, folder);
        block_on(self.launcher.install_package(&folder));
        assert_eq!(
            self.launcher.view().status,
            Status::Result(format!("Installed {}", fixture.title))
        );
        folder
    }

    /// Root search, with `query` typed.
    fn search(&self, query: &str) {
        while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.launcher.back();
        }
        block_on(self.launcher.set_query(query));
    }

    /// Types `query` in root search, chooses the row titled `title` and
    /// waits for what it does; the status line.
    fn run(&self, query: &str, title: &str) -> Status {
        self.search(query);
        select_title(&self.launcher, title);
        block_on(self.launcher.activate_selected());
        self.launcher.view().status
    }

    /// Types `query` in root search, which the alias it starts with makes
    /// select its command's row, and presses Enter; the status line.
    fn send(&self, query: &str) -> Status {
        self.search(query);
        assert_eq!(self.launcher.view().selected, Some(0), "{query}");
        block_on(self.launcher.activate_selected());
        self.launcher.view().status
    }

    /// Gives the command with manifest id `command` of the package from
    /// `folder` the alias `alias`.
    fn alias(&self, folder: &Path, command: &str, alias: &str) {
        let outcome = self.launcher.set_alias(&id(folder, command), alias);
        block_on(outcome.expect("the alias is accepted"));
    }

    /// What "Last launches" of the package from `folder` answers.
    fn last(&self, folder: &Path) -> String {
        self.search("last launches");
        let wanted = id(folder, "last");
        let index = self
            .launcher
            .view()
            .rows
            .iter()
            .position(|row| row.id == wanted)
            .unwrap_or_else(|| panic!("no row {wanted} in {:?}", titles(&self.launcher)));
        self.launcher.select(index);
        block_on(self.launcher.activate_selected());
        match self.launcher.view().status {
            Status::Result(text) => text,
            other => panic!("Last launches answered {other:?}"),
        }
    }

    /// Waits until the commands launched by other commands have run.
    fn launched(&self) {
        assert!(self.launcher.wait_for_launches(PROMPTLY));
    }
}

/// The command id of the command `command` of the package from `folder`.
fn id(folder: &Path, command: &str) -> String {
    format!(
        "{}#{command}",
        PackageIdentity::local(folder).unwrap().key()
    )
}

fn each_way_in_runs_the_command_once_with_its_record_and_opens_no_screen(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture, "no-view");
    let launcher = &pane.launcher;

    // Enter on its row runs it, and root search stays as it was.
    pane.search("report launch");
    select_title(launcher, "Report launch");
    assert_eq!(launcher.selected_action().label, "Run command");
    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert_eq!(
        view.status,
        report("user-initiated", "root-search", "none", NO_CONTEXT)
    );
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "report launch".into()
        }
    );
    // Each Enter runs it once.
    assert_eq!(
        pane.run("tick", "Tick"),
        Status::Result("Ticked 1 times".into())
    );
    assert_eq!(
        pane.run("tick", "Tick"),
        Status::Result("Ticked 2 times".into())
    );

    // Its alias alone, and followed by text, which arrives trimmed.
    pane.alias(&folder, "report", "rl");
    assert_eq!(
        pane.send("rl"),
        report("user-initiated", "alias", "none", NO_CONTEXT)
    );
    pane.search("rl  hello  world ");
    assert_eq!(
        launcher.view().rows[0].subtitle.as_deref(),
        Some("Send “hello  world” · alias rl")
    );
    assert_eq!(
        pane.send("rl  hello  world "),
        report("user-initiated", "alias", "hello  world", NO_CONTEXT)
    );
    assert_eq!(launcher.view().query(), Some("rl  hello  world "));

    // As a fallback, which the user chooses.
    manage(launcher);
    select_title(launcher, "Fallback: Report launch");
    block_on(launcher.activate_selected());
    pane.search("zqx  words ");
    assert_eq!(titles(launcher), ["Report launch"]);
    assert_eq!(launcher.view().selected, None);
    launcher.move_selection(1);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        report("user-initiated", "fallback", "zqx  words", NO_CONTEXT)
    );

    // Its global hotkey runs it where Pane is, without its window.
    let shortcut = Shortcut::parse("ctrl+alt+r").unwrap();
    let set = launcher.set_hotkey(&id(&folder, "report"), Some(shortcut.clone()));
    block_on(set.expect("the hotkey is accepted"));
    pane.search("abc");
    assert!(!launcher.hotkey_shows_window(&shortcut));
    block_on(
        launcher
            .press_hotkey(&shortcut)
            .expect("the hotkey is registered"),
    );
    let view = launcher.view();
    assert_eq!(
        view.status,
        report("user-initiated", "hotkey", "none", NO_CONTEXT)
    );
    assert_eq!(
        view.screen,
        Screen::Root {
            query: "abc".into()
        }
    );

    // Its quick slot.
    pane.search("");
    select_title(launcher, "Report launch");
    let (_, recorded) = launcher.change_quick_slots(&id(&folder, "report"), ResultAction::Pin);
    block_on(recorded);
    pane.search("");
    block_on(launcher.activate_quick_slot(0));
    let view = launcher.view();
    assert_eq!(
        view.status,
        report("user-initiated", "quick-slot", "none", NO_CONTEXT)
    );
    assert!(matches!(view.screen, Screen::Root { .. }));
}

fn an_error_it_answers_never_pauses_it_and_a_crash_counts_as_before(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture, "no-view");
    pane.alias(&folder, "report", "rl");

    // More errors than would pause a crashing package: each is shown, and
    // the command still runs.
    for _ in 0..4 {
        assert_eq!(
            pane.send("rl fail"),
            answered_error("Report launch fails on request")
        );
    }
    assert_eq!(
        pane.send("rl"),
        report("user-initiated", "alias", "none", NO_CONTEXT)
    );

    // Three crashes pause it.
    for _ in 0..3 {
        let status = pane.send("rl crash");
        assert!(matches!(status, Status::Error(_)), "{status:?}");
    }
    let Status::Error(error) = pane.launcher.view().status else {
        unreachable!()
    };
    assert!(error.contains("is paused"), "{error}");
    pane.search("rl");
    assert!(pane.launcher.view().rows[0].unavailable.is_some());
}

fn a_schedule_without_an_item_runs_the_command_in_the_background(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture, "no-view");
    // The scheduler sees the package before the clock moves: the
    // interval begins then.
    assert!(pane.launcher.wait_for_schedules(PROMPTLY));
    assert_eq!(
        pane.last(&folder),
        "Last report: none. Ticks: 0; last tick: none"
    );

    pane.clock.advance(MINUTE);
    assert!(pane.launcher.wait_for_schedules(PROMPTLY));
    let tick = described("background", "schedule", "none", NO_CONTEXT);
    // The run showed nothing: the status line is still what the user saw.
    assert_eq!(
        pane.launcher.view().status,
        Status::Result("Last report: none. Ticks: 0; last tick: none".into())
    );
    assert_eq!(
        pane.last(&folder),
        format!("Last report: none. Ticks: 1; last tick: {tick}")
    );

    pane.clock.advance(MINUTE);
    assert!(pane.launcher.wait_for_schedules(PROMPTLY));
    assert_eq!(
        pane.last(&folder),
        format!("Last report: none. Ticks: 2; last tick: {tick}")
    );
}

fn a_view_command_receives_its_launch_record(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture, "no-view");
    let launcher = &pane.launcher;

    pane.search("show launch");
    select_title(launcher, "Show launch");
    assert_eq!(launcher.selected_action().label, "Open command");
    block_on(launcher.activate_selected());
    let view = launcher.view();
    assert_eq!(view.screen, Screen::Command);
    assert_eq!(view.title, "Launch record");
    assert_eq!(
        titles(launcher),
        [
            "Launch type: user-initiated",
            "Source: root-search",
            "Fallback text: none",
            "Context: none",
        ]
    );

    // Text sent through its alias opens it with the text.
    pane.alias(&folder, "show", "sl");
    pane.send("sl  hi there ");
    assert_eq!(launcher.view().screen, Screen::Command);
    assert_eq!(
        titles(launcher),
        [
            "Launch type: user-initiated",
            "Source: alias",
            "Fallback text: hi there",
            "Context: none",
        ]
    );
}

fn a_command_launches_another_of_its_own_package_with_context(fixture: &Fixture) {
    let pane = Pane::new();
    let folder = pane.install(fixture, "no-view");
    let launcher = &pane.launcher;
    pane.alias(&folder, "launch", "ln");

    // User-initiated: it runs as if the user had invoked it.
    pane.send("ln report");
    pane.launched();
    assert_eq!(
        pane.last(&folder),
        format!(
            "Last report: {}. Ticks: 0; last tick: none",
            described("user-initiated", "command", "none", CONTEXT)
        )
    );

    // In the background.
    assert_eq!(
        pane.send("ln background report"),
        Status::Result("Launched report in the background".into())
    );
    pane.launched();
    assert_eq!(
        pane.last(&folder),
        format!(
            "Last report: {}. Ticks: 0; last tick: none",
            described("background", "command", "none", CONTEXT)
        )
    );

    // A view command opens, and asks for the window.
    assert!(!launcher.take_window_request());
    pane.send("ln show");
    pane.launched();
    let view = launcher.view();
    assert_eq!(view.screen, Screen::Command);
    assert_eq!(view.title, "Launch record");
    assert_eq!(
        titles(launcher),
        [
            "Launch type: user-initiated",
            "Source: command",
            "Fallback text: none",
            format!("Context: {CONTEXT}").as_str(),
        ]
    );
    assert!(launcher.take_window_request());
    assert!(!launcher.take_window_request());

    // Refused, with the reason, and never a failure of the caller.
    assert_eq!(
        pane.send("ln background show"),
        answered_error(&format!(
            "Show launch of {} opens a view, so it cannot be launched in the background; \
             launch it user-initiated",
            fixture.title
        ))
    );
    assert_eq!(
        pane.send("ln nothing"),
        answered_error(&format!("{} has no command `nothing`", fixture.title))
    );
    let nowhere = format!("local:{}", pane.sources.path().join("nowhere").display());
    assert_eq!(
        pane.send(&format!("ln {nowhere}#report")),
        answered_error(&format!("no installed extension has the source {nowhere}"))
    );
    assert_eq!(
        pane.send("ln report"),
        Status::Result("Launched report".into()),
        "the caller still runs"
    );
    pane.launched();
}

fn a_command_launches_one_of_another_package_but_not_a_disabled_one(fixture: &Fixture) {
    let pane = Pane::new();
    let first = pane.install(fixture, "first");
    let second = pane.install(fixture, "second");
    pane.alias(&first, "launch", "ln");
    let other = PackageIdentity::local(&second).unwrap();

    pane.send(&format!("ln {}#report", other.key()));
    pane.launched();
    assert_eq!(
        pane.last(&second),
        format!(
            "Last report: {}. Ticks: 0; last tick: none",
            described("user-initiated", "command", "none", CONTEXT)
        )
    );
    assert_eq!(
        pane.last(&first),
        "Last report: none. Ticks: 0; last tick: none",
        "only the other package's command ran"
    );

    block_on(pane.launcher.set_enabled(&other, false));
    assert_eq!(
        pane.send(&format!("ln {}#report", other.key())),
        answered_error(&format!(
            "{} is disabled; Pane does not enable it to launch its command, enable it in \
             Manage extensions",
            fixture.title
        ))
    );
    pane.launched();
    assert!(
        !pane
            .launcher
            .packages()
            .iter()
            .any(|package| package.identity == other && package.enabled),
        "the launch did not enable it"
    );
}

macro_rules! contract {
    ($($check:ident),* $(,)?) => {
        mod rust {
            $(#[test] fn $check() { super::$check(&super::RUST) })*
        }
        mod javascript {
            $(#[test] fn $check() { super::$check(&super::JAVASCRIPT) })*
        }
        mod typescript {
            $(#[test] fn $check() { super::$check(&super::TYPESCRIPT) })*
        }
    };
}

contract!(
    each_way_in_runs_the_command_once_with_its_record_and_opens_no_screen,
    an_error_it_answers_never_pauses_it_and_a_crash_counts_as_before,
    a_schedule_without_an_item_runs_the_command_in_the_background,
    a_view_command_receives_its_launch_record,
    a_command_launches_another_of_its_own_package_with_context,
    a_command_launches_one_of_another_package_but_not_a_disabled_one,
);

/// Installs the Rust sample with its `pane.json` changed by `change`;
/// the status line.
fn install_changed(change: impl FnOnce(&mut serde_json::Value)) -> (Pane, Status) {
    let pane = Pane::new();
    let folder = pane.source(RUST.package, "changed");
    let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    change(&mut manifest);
    fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
    block_on(pane.launcher.install_package(&folder));
    let status = pane.launcher.view().status;
    (pane, status)
}

#[test]
fn the_mode_is_read_from_the_manifest_and_omitted_means_view() {
    let pane = Pane::new();
    let folder = pane.source(RUST.package, "read");
    let manifest = Manifest::read(&folder).unwrap();
    let modes: Vec<(&str, CommandMode)> = manifest
        .commands
        .iter()
        .map(|command| (command.id.as_str(), command.mode))
        .collect();
    assert_eq!(
        modes,
        [
            ("report", CommandMode::NoView),
            ("tick", CommandMode::NoView),
            ("last", CommandMode::NoView),
            ("launch", CommandMode::NoView),
            ("show", CommandMode::View),
        ]
    );
    let tick = &manifest.commands[1];
    assert_eq!(tick.schedule.as_ref().unwrap().item, None);
}

#[test]
fn an_invalid_mode_is_refused_at_install_with_the_reason() {
    let (pane, status) = install_changed(|manifest| {
        manifest["commands"][0]["mode"] = "menu-bar".into();
    });
    let Status::Error(error) = status else {
        panic!("installed: {status:?}");
    };
    assert!(
        error.contains(
            "command `report` has the mode \"menu-bar\"; a command's `mode` is \"view\" (it \
             opens a screen, the default) or \"no-view\" (it runs without one)"
        ),
        "{error}"
    );
    assert!(pane.launcher.packages().is_empty());
}

#[test]
fn a_no_view_schedule_naming_an_item_is_refused_at_install() {
    let (pane, status) = install_changed(|manifest| {
        manifest["commands"][1]["schedule"]["item"] = "count".into();
    });
    let Status::Error(error) = status else {
        panic!("installed: {status:?}");
    };
    assert!(
        error.contains(
            "the schedule of command `tick` names an `item`, but the command is no-view: it \
             has no list, and Pane runs the command itself on its schedule; remove `item`"
        ),
        "{error}"
    );
    assert!(pane.launcher.packages().is_empty());
}

#[test]
fn a_launch_that_waits_for_nothing_settles_at_once() {
    let pane = Pane::new();
    let started = Instant::now();
    assert!(pane.launcher.wait_for_launches(PROMPTLY));
    assert!(started.elapsed() < PROMPTLY);
}
