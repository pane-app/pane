//! What root search learns from what the user chooses (ADR 0030, #199)
//! through the launcher's public interface, by the launcher's clock: a
//! use of a root result earns frecency, decaying with a ten-day
//! half-life, and the query it was chosen with is remembered, ranking
//! the result above how well titles match while it counts; the gates
//! that stop the queries counting, decay over simulated days, uses that
//! earn nothing (a global hotkey, a computed answer, a fallback, an
//! unavailable row, Pane's own rows), a quick slot's use with no query,
//! forgetting on uninstall and keeping on disable, survival across a
//! restart, an unreadable record reported and never replaced, an
//! application keeping its ranking across an update into a new version
//! folder, and the blank query listing the pins, then commands and
//! applications by frecency under "Commands". The commands come from
//! packages of the no-view sample, so invoking one keeps root search on
//! screen; the applications from a fake [`Applications`], through the
//! real applications guest.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::executor::block_on;
use pane_core::applications::{Application, Applications, Key};
use pane_core::clipboard::ManualClock;
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut};
use pane_core::{
    Launcher, PackageIdentity, ResultAction, Runtime, SavedData, Section, SlotChange, Status,
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
use guests::guest;
use rows::titles;

/// When the tests' clock starts, in milliseconds since the Unix epoch.
const NOW: u64 = 1_800_000_000_000;

/// How long recording a use may take: a thread writes it.
const RECORDED: Duration = Duration::from_secs(30);

/// One test's Pane: its folders, the clock its learning decays by, and
/// the launcher.
struct Pane {
    sources: TempDir,
    data: TempDir,
    cache: TempDir,
    clock: Arc<ManualClock>,
    launcher: Launcher,
}

impl Pane {
    fn new() -> Pane {
        let clock = ManualClock::at(NOW);
        Pane::over(
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
            clock,
        )
    }

    /// A Pane over folders, a clock and a runtime of the test's own, so a
    /// record the test wrote is read as Pane starts and the runtime finds
    /// the applications a test gave it.
    fn start(
        sources: TempDir,
        data: TempDir,
        cache: TempDir,
        clock: Arc<ManualClock>,
        runtime: Runtime,
    ) -> Pane {
        let launcher =
            Launcher::with_packages(Ok(runtime), Vec::new(), data.path().join("extensions"))
                .with_clock(clock.clone())
                .with_hotkeys(Arc::new(FakeHotkeys::default()))
                .with_quick_slots(data.path());
        Pane {
            sources,
            data,
            cache,
            clock,
            launcher,
        }
    }

    /// A Pane over folders and a clock of the test's own, starting a
    /// runtime that keeps compiled code in `cache`: so a record the test
    /// wrote is read as Pane starts.
    fn over(sources: TempDir, data: TempDir, cache: TempDir, clock: Arc<ManualClock>) -> Pane {
        let runtime = Runtime::start_with_cache(cache.path().to_path_buf()).unwrap();
        Pane::start(sources, data, cache, clock, runtime)
    }

    /// The same Pane after a restart: the launcher stops and a new one,
    /// with a runtime of its own, reads the same records by the same
    /// clock.
    fn restart(self) -> Pane {
        let Pane {
            sources,
            data,
            cache,
            clock,
            launcher,
        } = self;
        drop(launcher);
        Pane::over(sources, data, cache, clock)
    }

    /// A package folder named `name` whose manifest gives one no-view
    /// command per `(manifest id, title)`, all served by the no-view
    /// sample's component: invoking one runs it, and root search stays.
    fn package(&self, name: &str, commands: &[(&str, &str)]) -> PathBuf {
        let folder = self.sources.path().join(name);
        fs::create_dir_all(&folder).unwrap();
        let commands: Vec<String> = commands
            .iter()
            .map(|(id, title)| {
                format!(
                    r#"{{
                        "id": "{id}",
                        "title": "{title}",
                        "component": "component.wasm",
                        "mode": "no-view"
                    }}"#
                )
            })
            .collect();
        let manifest = format!(
            r#"{{
                "manifestVersion": 1,
                "title": "{name}",
                "version": "0.1.0",
                "apiVersion": "0.1",
                "commands": [{}]
            }}"#,
            commands.join(", ")
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        fs::copy(guest("sample_no_view"), folder.join("component.wasm")).unwrap();
        folder
    }

    /// A package folder whose manifest gives one no-view command
    /// available only on other systems, to invoke one that cannot run.
    fn unavailable_package(&self, title: &str) -> PathBuf {
        let [first, second] = platforms::other_systems();
        let elsewhere = format!(r#"["{}", "{}"]"#, first.id(), second.id());
        let folder = self.sources.path().join("elsewhere");
        fs::create_dir_all(&folder).unwrap();
        let manifest = format!(
            r#"{{
                "manifestVersion": 1,
                "title": "Elsewhere",
                "version": "0.1.0",
                "apiVersion": "0.1",
                "commands": [{{
                    "id": "away",
                    "title": "{title}",
                    "component": "component.wasm",
                    "mode": "no-view",
                    "platforms": {elsewhere}
                }}]
            }}"#
        );
        fs::write(folder.join("pane.json"), manifest).unwrap();
        fs::copy(guest("sample_no_view"), folder.join("component.wasm")).unwrap();
        folder
    }

    /// An assembled package under `target/guests/packages`.
    fn built(&self, name: &str) -> PathBuf {
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

    fn install(&self, folder: &Path) -> PackageIdentity {
        block_on(self.launcher.install_package(folder));
        assert!(
            matches!(self.launcher.view().status, Status::Result(_)),
            "{:?}",
            self.launcher.view().status
        );
        self.launcher.back();
        PackageIdentity::local(folder).unwrap()
    }

    /// Where what root search learned is recorded.
    fn record(&self) -> PathBuf {
        self.data.path().join("extensions/learned.json")
    }
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

/// Types `query` and invokes the row whose id ends with `which`, waiting
/// for what root search learned of it to be recorded.
fn choose(launcher: &Launcher, query: &str, which: &str) {
    block_on(launcher.set_query(query));
    let view = launcher.view();
    let index = view
        .rows
        .iter()
        .position(|row| row.id.ends_with(which))
        .unwrap_or_else(|| panic!("no row ending in {which:?} in {:?}", titles(launcher)));
    launcher.select(index);
    block_on(launcher.activate_selected());
    assert!(
        launcher.wait_for_learned_recorded(RECORDED),
        "the use was recorded"
    );
}

/// The ids of the rows titled `title`, in listed order: how two
/// same-titled results are told apart.
fn ids_titled(launcher: &Launcher, title: &str) -> Vec<String> {
    launcher
        .view()
        .rows
        .iter()
        .filter(|row| row.title == title)
        .map(|row| row.id.clone())
        .collect()
}

/// Whether the row `index`'s id ends with `which`, naming it.
fn at(launcher: &Launcher, title: &str, index: usize, which: &str) -> bool {
    ids_titled(launcher, title)
        .get(index)
        .is_some_and(|id| id.ends_with(which))
}

fn day(days: u64) -> Duration {
    Duration::from_secs(days * 24 * 60 * 60)
}

/// A package of two same-titled no-view commands, to rank and to choose
/// between.
fn pythons(pane: &Pane) -> (PathBuf, PackageIdentity) {
    let folder = pane.package("pythons", &[("a", "Python"), ("b", "Python")]);
    let identity = pane.install(&folder);
    (folder, identity)
}

#[test]
fn a_use_ranks_the_result_first_for_the_blank_query_and_the_query_it_was_chosen_with() {
    let pane = Pane::new();
    pythons(&pane);
    let launcher = &pane.launcher;

    // Two same-titled commands match equally: nothing is learned, so
    // they keep the order they were listed in.
    block_on(launcher.set_query("pyt"));
    assert!(
        at(launcher, "Python", 0, "#a"),
        "{:?}",
        ids_titled(launcher, "Python")
    );
    choose(launcher, "pyt", "#b");

    // The blank query lists what root search learned first: the used
    // result's frecency is above the floor an unused result stands at.
    block_on(launcher.set_query(""));
    assert!(
        at(launcher, "Python", 0, "#b"),
        "{:?}",
        ids_titled(launcher, "Python")
    );
    // The query it was chosen with is its learned query (step 3): typing
    // it again ranks it above the equal match.
    block_on(launcher.set_query("pyt"));
    assert!(
        at(launcher, "Python", 0, "#b"),
        "{:?}",
        ids_titled(launcher, "Python")
    );
}

#[test]
fn a_learned_query_starting_with_the_query_ranks_above_an_equal_match() {
    let pane = Pane::new();
    pythons(&pane);
    let launcher = &pane.launcher;

    choose(launcher, "pyth", "#b");
    // Half the learned query is typed: the result it was chosen with
    // ranks above the equal match (step 6).
    block_on(launcher.set_query("py"));
    assert!(
        at(launcher, "Python", 0, "#b"),
        "{:?}",
        ids_titled(launcher, "Python")
    );
}

#[test]
fn a_query_starting_with_a_learned_query_ranks_above_a_better_match() {
    let pane = Pane::new();
    let folder = pane.package("letters", &[("good", "Abc Def G"), ("poor", "Axbxcxdxefg")]);
    pane.install(&folder);
    let launcher = &pane.launcher;

    // Without learning: the query starts the first result's words, and
    // only fuzzily fits the second (step 8).
    block_on(launcher.set_query("abcdef"));
    assert_eq!(titles(launcher), ["Abc Def G", "Axbxcxdxefg"]);

    // The poorly-matching result, chosen with "abc": a query that starts
    // with a learned query of at least three characters ranks it above
    // the better match (step 7), while it is at most three characters
    // longer than it.
    choose(launcher, "abc", "#poor");
    block_on(launcher.set_query("abcdef"));
    assert_eq!(
        titles(launcher),
        ["Axbxcxdxefg", "Abc Def G"],
        "the learned query wins over the better title match"
    );
    // More than three characters longer: the learned query is ignored,
    // and the better match wins again.
    block_on(launcher.set_query("abcdefg"));
    assert_eq!(
        titles(launcher),
        ["Abc Def G", "Axbxcxdxefg"],
        "the overbounds match is ignored"
    );
}

#[test]
fn the_learned_queries_stop_counting_once_the_frecency_decays_to_the_floor() {
    let pane = Pane::new();
    pythons(&pane);
    let launcher = &pane.launcher;

    choose(launcher, "pyth", "#b");
    // Twelve days: the one use's score of 2 halves to below 1, which the
    // floor holds it at — not above it, so nothing is learned any more.
    pane.clock.advance(day(12));
    block_on(launcher.set_query("py"));
    assert!(
        at(launcher, "Python", 0, "#a"),
        "the learned query no longer counts: {:?}",
        ids_titled(launcher, "Python")
    );
    block_on(launcher.set_query(""));
    assert!(
        at(launcher, "Python", 0, "#a"),
        "nor does the frecency, at the floor an unused result stands at"
    );
}

#[test]
fn the_learned_queries_stop_counting_17_days_after_the_result_was_last_opened() {
    let pane = Pane::new();
    pythons(&pane);
    let launcher = &pane.launcher;

    // Seven uses: a score of 8, still above the floor after 18 days.
    for _ in 0..7 {
        choose(launcher, "pyth", "#b");
    }
    pane.clock.advance(day(18));
    block_on(launcher.set_query(""));
    assert!(
        at(launcher, "Python", 0, "#b"),
        "the frecency still ranks the used result first: {:?}",
        ids_titled(launcher, "Python")
    );
    block_on(launcher.set_query("py"));
    assert!(
        at(launcher, "Python", 0, "#a"),
        "but its learned queries stopped counting 17 days after it was last opened"
    );
}

#[test]
fn frecency_decays_over_simulated_days_and_a_use_re_anchors_it() {
    let pane = Pane::new();
    pythons(&pane);
    let launcher = &pane.launcher;

    // Three uses: a score of 4, halved by ten days and at the floor
    // after twenty.
    for _ in 0..3 {
        choose(launcher, "pyt", "#b");
    }
    pane.clock.advance(day(10));
    block_on(launcher.set_query(""));
    assert!(
        at(launcher, "Python", 0, "#b"),
        "half of the score still ranks the result first"
    );
    pane.clock.advance(day(10));
    // The decayed score reached the floor, so nothing is learned any
    // more (searching the same query again would change nothing).
    block_on(launcher.set_query("pyt"));
    block_on(launcher.set_query(""));
    assert!(
        at(launcher, "Python", 0, "#a"),
        "the decayed score reached the floor, so nothing is learned any more"
    );
    // A use adds 1 to the decayed score, re-anchoring it.
    choose(launcher, "pyt", "#b");
    block_on(launcher.set_query(""));
    assert!(
        at(launcher, "Python", 0, "#b"),
        "the use re-anchored the score at the floor plus one"
    );
}

#[test]
fn invoking_a_quick_slot_records_a_use_with_no_query() {
    let pane = Pane::new();
    pythons(&pane);
    let launcher = &pane.launcher;

    // Pin the second Python: the selected row is what pinning pins.
    block_on(launcher.set_query("pyt"));
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.id.ends_with("#b"))
        .expect("the second Python's row");
    launcher.select(index);
    let target = launcher.view().rows[index].id.clone();
    let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::Pin);
    assert_eq!(change, SlotChange::Changed(Some(0)), "{change:?}");
    block_on(recorded);

    // Invoking the slot from the pinned home is a choice of its target,
    // with no query typed.
    block_on(launcher.set_query(""));
    block_on(launcher.activate_quick_slot(0));
    assert!(launcher.wait_for_learned_recorded(RECORDED));
    // Recording the use never re-sorts the list on screen: the next
    // search ranks with it.
    block_on(launcher.set_query("pyt"));
    block_on(launcher.set_query(""));
    assert!(
        at(launcher, "Python", 0, "#b"),
        "the slot's target earned frecency: {:?}",
        ids_titled(launcher, "Python")
    );
}

#[test]
fn a_use_is_not_recorded_when_the_row_only_explains_why_it_cannot_run() {
    let pane = Pane::new();
    let folder = pane.unavailable_package("Away");
    pane.install(&folder);
    let launcher = &pane.launcher;

    block_on(launcher.set_query("away"));
    assert!(launcher.view().rows[0].unavailable.is_some());
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().status, Status::Error(_)));
    assert!(
        !pane.record().exists(),
        "a row refused as unavailable records no use"
    );
}

#[test]
fn a_hotkey_a_computed_answer_a_fallback_and_panes_own_rows_record_nothing() {
    let pane = Pane::new();
    let query_sample = pane.install(&pane.built("sample-query"));
    pane.install(&pane.built("calculator"));
    let launcher = &pane.launcher;
    let echo = format!("{}#echo", query_sample.key());

    // A fallback row: nothing matches, so it is selected, and Enter sends
    // the text to the command — a row the user chose because nothing
    // matched, not a root result.
    block_on(launcher.set_query("zzz"));
    let fallback = ids_titled(launcher, "Echo");
    assert_eq!(fallback.len(), 1, "{:?}", titles(launcher));
    block_on(launcher.activate_selected());
    assert_eq!(shown(launcher), Status::Result("Echo heard “zzz”".into()));
    launcher.back();

    // A computed answer: Enter copies it, and nothing is learned of a
    // row without a lasting identity.
    block_on(launcher.set_query("6*7"));
    assert_eq!(titles(launcher), ["42"]);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Copied 42 to the clipboard".into())
    );

    // Pane's own row: activating it does nothing in the launcher.
    block_on(launcher.set_query(""));
    let settings = ids_titled(launcher, "Settings…");
    assert_eq!(settings.len(), 1, "{:?}", titles(launcher));
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.title == "Settings…")
        .unwrap();
    launcher.select(index);
    block_on(launcher.activate_selected());

    // A global hotkey opens the command: not a choice from root search.
    let shortcut = Shortcut::parse("ctrl+alt+e").unwrap();
    let set = launcher
        .set_hotkey(&echo, Some(shortcut.clone()))
        .expect("the hotkey is set");
    block_on(set);
    let opened = launcher
        .press_hotkey(&shortcut)
        .expect("the hotkey opens it");
    block_on(opened);

    assert!(
        !pane.record().exists(),
        "none of those uses is recorded: {:?}",
        pane.record()
    );
}

#[test]
fn uninstalling_a_package_forgets_what_was_learned_and_disabling_keeps_it() {
    let pane = Pane::new();
    let (folder, identity) = pythons(&pane);
    let launcher = &pane.launcher;
    choose(launcher, "pyt", "#b");

    // Uninstalling forgets what was learned about the package, as its
    // aliases are forgotten: installing the same source again starts
    // with nothing learned.
    block_on(launcher.uninstall(&identity, SavedData::Keep));
    assert!(matches!(launcher.view().status, Status::Result(_)));
    block_on(launcher.install_package(&folder));
    launcher.back();
    block_on(launcher.set_query(""));
    assert!(
        at(launcher, "Python", 0, "#a"),
        "the reinstalled package's rows start unlearned: {:?}",
        ids_titled(launcher, "Python")
    );
    choose(launcher, "pyt", "#b");

    // Disabling keeps what was learned: the rows leave and come back
    // with their frecency, also across a restart.
    block_on(launcher.set_enabled(&identity, false));
    let pane = pane.restart();
    let launcher = &pane.launcher;
    block_on(launcher.set_enabled(&identity, true));
    block_on(launcher.set_query(""));
    assert!(
        at(launcher, "Python", 0, "#b"),
        "what was learned survived the disable and the restart: {:?}",
        ids_titled(launcher, "Python")
    );
}

#[test]
fn what_was_learned_survives_a_restart_over_the_same_data_folder() {
    let pane = Pane::new();
    pythons(&pane);
    choose(&pane.launcher, "pyt", "#b");

    let pane = pane.restart();
    let launcher = &pane.launcher;
    block_on(launcher.set_query(""));
    assert!(
        at(launcher, "Python", 0, "#b"),
        "the frecency was read back: {:?}",
        ids_titled(launcher, "Python")
    );
    block_on(launcher.set_query("pyt"));
    assert!(
        at(launcher, "Python", 0, "#b"),
        "and the learned query was: {:?}",
        ids_titled(launcher, "Python")
    );
}

#[test]
fn an_unreadable_record_is_reported_and_never_replaced() {
    for garbage in ["{ not a record", r#"{ "version": 2, "uses": {} }"#] {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        fs::create_dir_all(data.path().join("extensions")).unwrap();
        fs::write(data.path().join("extensions/learned.json"), garbage).unwrap();
        let pane = Pane::over(sources, data, cache, ManualClock::at(NOW));
        let launcher = &pane.launcher;
        assert!(
            launcher.learned_problem().is_some(),
            "{garbage} is reported"
        );
        assert!(
            matches!(launcher.view().status, Status::Error(problem)
                if problem.starts_with("Pane could not read what root search learned")),
            "{garbage}: {:?}",
            launcher.view().status
        );

        pythons(&pane);

        // A use is recorded nowhere, and the ranking learns nothing.
        choose(launcher, "pyt", "#b");
        assert_eq!(
            fs::read_to_string(pane.record()).unwrap(),
            garbage,
            "the record stays as it was"
        );
        block_on(launcher.set_query(""));
        assert!(
            at(launcher, "Python", 0, "#a"),
            "{garbage}: root search ranks as if nothing was learned"
        );
    }
}

/// The system as the applications tests set it up: its applications and
/// what was opened.
#[derive(Default)]
struct FakeApplications {
    applications: Mutex<Vec<Application>>,
    opened: Mutex<Vec<String>>,
}

impl Applications for FakeApplications {
    fn installed(&self) -> Result<Vec<Application>, String> {
        Ok(self.applications.lock().unwrap().clone())
    }

    fn open(&self, id: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(id.to_owned());
        Ok(())
    }
}

fn app(id: &str, name: &str) -> Application {
    Application {
        id: id.into(),
        name: name.into(),
        location: "/apps".into(),
        ..Application::default()
    }
}

/// The ids of the rows whose subtitle is "Application", in listed order.
fn application_ids(launcher: &Launcher) -> Vec<String> {
    launcher
        .view()
        .rows
        .iter()
        .filter(|row| row.subtitle.as_deref() == Some("Application"))
        .map(|row| row.id.clone())
        .collect()
}

/// A pane whose runtime finds `system`'s applications, with the
/// applications package installed beside `packages` (folders the pane
/// writes with `package`).
fn applications_pane(pane: Pane, system: &Arc<FakeApplications>, packages: &[PathBuf]) -> Pane {
    let runtime = Runtime::start_with_cache(pane.cache.path().to_path_buf()).unwrap();
    runtime.set_applications(system.clone());
    let Pane {
        sources,
        data,
        cache,
        clock,
        launcher,
    } = pane;
    drop(launcher);
    let pane = Pane::start(sources, data, cache, clock, runtime);
    pane.install(&pane.built("applications"));
    for package in packages {
        pane.install(package);
    }
    pane
}

#[test]
fn the_blank_query_lists_the_pins_then_commands_and_applications_by_frecency() {
    let pane = Pane::new();
    let system = Arc::new(FakeApplications {
        applications: Mutex::new(vec![
            app("/apps/Firefox.app", "Firefox"),
            app("/apps/Terminal.app", "Terminal"),
            app("/apps/Notes.app", "Notes"),
        ]),
        opened: Mutex::new(Vec::new()),
    });
    let pythons = pane.package("pythons", &[("a", "Python"), ("b", "Python")]);
    let pane = applications_pane(pane, &system, &[pythons]);
    let launcher = &pane.launcher;

    // Pin Firefox, found by its name.
    block_on(launcher.set_query("fire"));
    let firefox = ids_titled(launcher, "Firefox")
        .into_iter()
        .find(|id| id.ends_with("/apps/Firefox.app"))
        .expect("Firefox's row");
    let (change, recorded) = launcher.change_quick_slots(&firefox, ResultAction::Pin);
    assert_eq!(change, SlotChange::Changed(Some(0)), "{change:?}");
    block_on(recorded);

    // Two uses of Terminal, one each of Firefox and the second Python:
    // what the user opens most rises to the top.
    choose(launcher, "term", "/apps/Terminal.app");
    choose(launcher, "term", "/apps/Terminal.app");
    choose(launcher, "fire", "/apps/Firefox.app");
    choose(launcher, "pyt", "#b");

    // The blank query: the pinned home first, then the commands and
    // applications by frecency — Terminal (3), then the second Python
    // and Firefox (2, the command above the application), then every
    // unused result (the commands alphabetically, then the
    // applications) — all under "Commands", with no Suggestions
    // section.
    launcher.show_root_search();
    block_on(launcher.resolve_root_home());
    let slots = launcher.quick_slots();
    assert_eq!(
        slots
            .iter()
            .map(|slot| slot.title.as_str())
            .collect::<Vec<_>>(),
        ["Firefox"],
        "the pins, above the results"
    );
    assert_eq!(
        titles(launcher),
        [
            "Terminal",
            "Python",
            "Firefox",
            "Install extension from folder…",
            "Install extension from Git…",
            "Install extension from npm…",
            "Manage Extensions",
            "Python",
            "Settings…",
            "Notes",
        ]
    );
    assert!(
        at(launcher, "Python", 1, "#a"),
        "the unused Python keeps its place: {:?}",
        ids_titled(launcher, "Python")
    );
    assert_eq!(
        launcher.presentation().sections,
        [Section {
            label: "Commands".into(),
            note: None,
            first: 0,
        }]
    );
    assert_eq!(
        *system.opened.lock().unwrap(),
        [
            "/apps/Terminal.app",
            "/apps/Terminal.app",
            "/apps/Firefox.app"
        ]
    );
}

#[test]
fn an_application_keeps_its_ranking_across_an_update_into_a_new_version_folder() {
    let pane = Pane::new();
    // The same program, its updater having moved it into a new version
    // folder: one identity, whose version segment is a wildcard.
    let program = |folder: &str| format!(r"C:\Program Files\Tools\{folder}\tools.exe");
    let identity = Key::program(&program("app-1.0.0"), "").id();
    let updated = Key::program(&program("app-1.2.0"), "").id();
    assert_eq!(
        identity, updated,
        "the version folder is wildcarded, so the update keeps the identity"
    );

    let system = Arc::new(FakeApplications {
        applications: Mutex::new(vec![app(&identity, "Tools"), app("/apps/Mail.app", "Mail")]),
        opened: Mutex::new(Vec::new()),
    });
    let pane = applications_pane(pane, &system, &[]);

    // One use of Tools: it outranks Mail on the blank query.
    choose(&pane.launcher, "tools", &identity);
    pane.launcher.show_root_search();
    block_on(pane.launcher.resolve_root_home());
    let listed = application_ids(&pane.launcher);
    assert!(
        listed[0].ends_with(&identity) && listed[1].ends_with("/apps/Mail.app"),
        "the used application ranks first: {listed:?}"
    );

    // The update: the program is found in its new version folder, the
    // same application by its identity, and the results are listed again.
    *system.applications.lock().unwrap() =
        vec![app(&updated, "Tools"), app("/apps/Mail.app", "Mail")];
    pane.launcher.show_root_search();
    block_on(pane.launcher.resolve_root_home());
    let listed = application_ids(&pane.launcher);
    assert!(
        listed[0].ends_with(&updated) && listed[1].ends_with("/apps/Mail.app"),
        "the application kept its ranking across the update: {listed:?}"
    );
    block_on(pane.launcher.activate_selected());
    assert_eq!(
        *system.opened.lock().unwrap(),
        [identity.as_str()],
        "the updated application opens by its identity"
    );
}
