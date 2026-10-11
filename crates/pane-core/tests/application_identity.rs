//! Applications' stable identity through the launcher's public interface,
//! with the JavaScript applications sample (`target/guests/packages/
//! sample-applications-js`), which supplies the host's applications to root
//! search as indexed results, and the host's list of applications
//! ([`Cached`]) over a fake system whose sources the tests decide: an
//! application keeps its id, and so its pin,
//! when its program moves to a new version folder; shortcuts to one program
//! are one result, opened by the preferred one; arguments keep two programs
//! apart; and a pin made before applications had stable identities, which
//! holds a shortcut's path, resolves and is rewritten. The pure rules are
//! unit tests of `pane_core::applications::identity`; the real systems'
//! sources are checked in `application_adapters.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::executor::block_on;
use pane_core::applications::icons::{Extracted, IconExtractor};
use pane_core::applications::{Application, Applications, Cached, Discovery, Key, Source};
use pane_core::{Launcher, PinTarget, ResultAction, Runtime, SlotChange, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{select_title, titles};

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

/// A system as the tests set it up: the sources of its applications, and
/// what was opened.
#[derive(Default)]
struct FakeSystem {
    sources: Mutex<Vec<Source>>,
    opened: Mutex<Vec<String>>,
}

impl FakeSystem {
    fn with(sources: Vec<Source>) -> Arc<FakeSystem> {
        Arc::new(FakeSystem {
            sources: Mutex::new(sources),
            ..FakeSystem::default()
        })
    }

    fn opened(&self) -> Vec<String> {
        self.opened.lock().unwrap().clone()
    }
}

impl Discovery for FakeSystem {
    fn sources(&self) -> Result<Vec<Source>, String> {
        Ok(self.sources.lock().unwrap().clone())
    }

    fn open(&self, path: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(path.to_owned());
        Ok(())
    }
}

/// Where a shortcut is: the user's Desktop is preferred to their Start
/// menu, which is preferred to every user's (`Place` on Windows).
const DESKTOP: usize = 0;
const START_MENU: usize = 2;
const ALL_USERS_START_MENU: usize = 3;

/// The shortcut `name` in the folder of `place`, to `target` with
/// `arguments`.
fn shortcut(name: &str, place: usize, target: &str, arguments: &str) -> Source {
    let folder = format!(r"C:\Places\{place}");
    Source {
        program: Some(target.to_owned()),
        arguments: !arguments.trim().is_empty(),
        ..Source::new(
            Key::program(target, arguments),
            format!(r"{folder}\{name}.lnk"),
            name,
            folder,
            place,
        )
    }
}

/// Discord's program in the version folder `version`.
fn discord(version: &str) -> String {
    format!(r"C:\Users\Ann\AppData\Local\Discord\{version}\Discord.exe")
}

/// No application has an icon: these applications are made up, and the
/// system's extraction (the shell, on Windows) is not asked about them. A
/// row shows its placeholder; icons are `application_icons.rs`'s.
struct NoIcons;

impl IconExtractor for NoIcons {
    fn extract(&self, source: &str) -> Result<Extracted, String> {
        Err(format!("no icon for {source} in these tests"))
    }
}

/// Pane's data location and compiled code cache, kept across restarts.
struct Dirs {
    data: TempDir,
    cache: TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            data: tempfile::tempdir().unwrap(),
            cache: tempfile::tempdir().unwrap(),
        }
    }

    /// A launcher, started afresh, keeping its packages and quick slots in
    /// this data folder, whose runtime finds applications through
    /// `applications`.
    fn launcher(&self, applications: Arc<dyn Applications>) -> Launcher {
        let runtime = Runtime::start_with_cache(self.cache.path().to_path_buf()).unwrap();
        runtime.set_applications(applications);
        Launcher::with_packages(Ok(runtime), vec![], self.data.path().join("extensions"))
            .with_quick_slots(self.data.path())
            .with_application_icons(
                self.cache.path().join("application-icons"),
                Arc::new(NoIcons),
            )
    }

    /// A launcher, started afresh, whose host lists `system`'s
    /// applications by identity.
    fn hosting(&self, system: &Arc<FakeSystem>) -> Launcher {
        self.launcher(Arc::new(Cached::new(
            system.clone(),
            Duration::from_secs(3600),
        )))
    }

    fn record(&self) -> String {
        fs::read_to_string(self.data.path().join("quick-slots.json")).unwrap()
    }
}

fn install(launcher: &Launcher, folder: &Path) {
    block_on(launcher.install_package(folder));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
    launcher.back();
}

fn search(launcher: &Launcher, query: &str) {
    block_on(launcher.set_query(query));
}

/// Pins root search's row titled `title`, found by `query`.
fn pin(launcher: &Launcher, query: &str, title: &str) {
    search(launcher, query);
    select_title(launcher, title);
    let index = launcher.view().selected.unwrap();
    let target = launcher.view().rows[index].id.clone();
    let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::Pin);
    assert!(matches!(change, SlotChange::Changed(_)), "{change:?}");
    block_on(recorded);
}

/// The result id the only quick slot holds.
fn pinned_result(launcher: &Launcher) -> String {
    let slots = launcher.quick_slots();
    assert_eq!(slots.len(), 1, "{slots:?}");
    match &slots[0].target {
        PinTarget::Indexed { result, .. } => result.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn two_shortcuts_to_one_program_are_one_result_opened_by_the_preferred_one() {
    let dirs = Dirs::new();
    let editor = r"C:\Program Files\Editor\editor.exe";
    let system = FakeSystem::with(vec![
        shortcut("Editor", ALL_USERS_START_MENU, editor, ""),
        shortcut("Editor", START_MENU, editor, ""),
        shortcut("My Editor", DESKTOP, editor, ""),
    ]);
    let launcher = dirs.hosting(&system);
    install(&launcher, &built("packages/sample-applications-js"));

    search(&launcher, "editor");

    // One result, named and opened by the user's own Desktop shortcut.
    assert_eq!(titles(&launcher), ["Launch My Editor"]);
    block_on(launcher.activate_selected());
    assert_eq!(system.opened(), [r"C:\Places\0\My Editor.lnk"]);
    assert_eq!(
        launcher.view().status,
        Status::Result("Opened Launch My Editor".into())
    );
}

#[test]
fn shortcuts_to_one_program_with_different_arguments_stay_two_results() {
    let dirs = Dirs::new();
    let browser = r"C:\Program Files\Browser\browser.exe";
    let system = FakeSystem::with(vec![
        shortcut("Browser", START_MENU, browser, ""),
        shortcut("Browser Mail", START_MENU, browser, "--app-id=mail"),
        shortcut(
            "Browser Work",
            START_MENU,
            browser,
            "--profile-directory=Work",
        ),
    ]);
    let launcher = dirs.hosting(&system);
    install(&launcher, &built("packages/sample-applications-js"));

    search(&launcher, "browser");

    assert_eq!(
        titles(&launcher),
        [
            "Launch Browser",
            "Launch Browser Mail",
            "Launch Browser Work"
        ]
    );
    select_title(&launcher, "Launch Browser Work");
    block_on(launcher.activate_selected());
    assert_eq!(system.opened(), [r"C:\Places\2\Browser Work.lnk"]);
}

#[test]
fn an_application_updated_into_a_new_version_folder_keeps_its_id_and_its_pin() {
    let dirs = Dirs::new();
    let system = FakeSystem::with(vec![shortcut(
        "Discord",
        START_MENU,
        &discord("app-1.0.9003"),
        "",
    )]);
    let pinned = {
        let launcher = dirs.hosting(&system);
        install(&launcher, &built("packages/sample-applications-js"));
        pin(&launcher, "discord", "Launch Discord");
        pinned_result(&launcher)
    };
    assert_eq!(pinned, Key::program(&discord("app-1.0.9003"), "").id());

    // Discord updates itself into a new version folder and rewrites its
    // shortcut; Pane starts again.
    *system.sources.lock().unwrap() = vec![shortcut(
        "Discord",
        START_MENU,
        &discord("app-1.0.9004"),
        "",
    )];
    let launcher = dirs.hosting(&system);
    block_on(launcher.resolve_root_home());

    let slot = &launcher.quick_slots()[0];
    assert!(slot.ready(), "{slot:?}");
    assert_eq!(slot.title, "Launch Discord");
    assert_eq!(pinned_result(&launcher), pinned);
    block_on(launcher.activate_quick_slot(0));
    assert_eq!(system.opened(), [r"C:\Places\2\Discord.lnk"]);
    // The same result is found by typing, with the same id.
    search(&launcher, "disc");
    assert_eq!(titles(&launcher), ["Launch Discord"]);
}

/// The installed applications as Pane listed them before applications had
/// stable identities: each identified by its shortcut's path.
struct BeforeIdentities(Vec<Source>);

impl Applications for BeforeIdentities {
    fn installed(&self) -> Result<Vec<Application>, String> {
        Ok(self
            .0
            .iter()
            .map(|source| Application {
                id: source.path.clone(),
                name: source.name.clone(),
                location: source.location.clone(),
                ..Application::default()
            })
            .collect())
    }

    fn open(&self, _id: &str) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn pins_made_before_identities_resolve_keep_their_slots_and_are_rewritten() {
    let dirs = Dirs::new();
    let firefox = shortcut("Firefox", START_MENU, r"C:\Firefox\firefox.exe", "");
    let mail = shortcut("Mail", START_MENU, r"C:\Mail\mail.exe", "");
    let notes = shortcut("Notes", START_MENU, r"C:\Notes\notes.exe", "");
    // The data folder of an earlier Pane: three applications pinned by
    // their shortcuts' paths.
    {
        let before = BeforeIdentities(vec![firefox.clone(), mail.clone(), notes.clone()]);
        let launcher = dirs.launcher(Arc::new(before));
        install(&launcher, &built("packages/sample-applications-js"));
        pin(&launcher, "fire", "Launch Firefox");
        pin(&launcher, "mail", "Launch Mail");
        pin(&launcher, "notes", "Launch Notes");
    }
    let record = dirs.record();
    assert!(record.contains(r"C:\\Places\\2\\Firefox.lnk"), "{record}");

    // This Pane: Firefox is the same; Mail's shortcut now also has a
    // Desktop copy, which is preferred; Notes was uninstalled.
    let mine = shortcut("Mail", DESKTOP, r"C:\Mail\mail.exe", "");
    let system = FakeSystem::with(vec![firefox.clone(), mail.clone(), mine.clone()]);
    let launcher = dirs.hosting(&system);
    block_on(launcher.resolve_root_home());

    let slots = launcher.quick_slots();
    let titles: Vec<&str> = slots.iter().map(|slot| slot.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Launch Firefox",
            "Launch Mail",
            "JavaScript applications sample"
        ]
    );
    assert!(slots[0].ready() && slots[1].ready(), "{slots:?}");
    let results: Vec<String> = slots
        .iter()
        .map(|slot| match &slot.target {
            PinTarget::Indexed { result, .. } => result.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(results[0], firefox.key.id());
    assert_eq!(results[1], mail.key.id());
    // A pin whose application is gone keeps its slot and says why.
    assert_eq!(results[2], notes.path);
    assert_eq!(
        slots[2].unavailable.as_deref(),
        Some("JavaScript applications sample no longer lists it")
    );
    // The record holds the new ids, so the next start needs no carrying.
    let record = dirs.record();
    assert!(record.contains(&firefox.key.id()), "{record}");
    assert!(record.contains(&mail.key.id()), "{record}");
    assert!(!record.contains(r"C:\\Places\\2\\Firefox.lnk"), "{record}");

    // Invoked, the carried pin opens the application's preferred source.
    block_on(launcher.activate_quick_slot(1));
    assert_eq!(system.opened(), std::slice::from_ref(&mine.path));
    assert_eq!(
        shown(&launcher),
        Status::Result("Opened Launch Mail".into())
    );
}

#[test]
fn the_host_opens_an_application_by_the_path_it_had_before_identities() {
    let system = FakeSystem::with(vec![
        shortcut("Tool", START_MENU, r"C:\Tool\app-1.2\tool.exe", ""),
        shortcut("Tool", DESKTOP, r"C:\Tool\app-1.3\tool.exe", ""),
    ]);
    let host = Cached::new(system.clone(), Duration::from_secs(3600));

    host.open(r"C:\Places\2\Tool.lnk").unwrap();

    assert_eq!(system.opened(), [r"C:\Places\0\Tool.lnk"]);
}

/// The JavaScript and TypeScript author examples receive the stable ids
/// through the same import, and opening one by it opens the application's
/// preferred source.
fn a_js_command_opens_an_application_by_its_stable_id(package: &str, language: &str) {
    let dirs = Dirs::new();
    let firefox = r"C:\Firefox\firefox.exe";
    let system = FakeSystem::with(vec![
        shortcut("Firefox", ALL_USERS_START_MENU, firefox, ""),
        shortcut("Firefox", START_MENU, firefox, ""),
    ]);
    let launcher = dirs.hosting(&system);
    install(&launcher, &built(&format!("packages/{package}")));

    pin(&launcher, "fire", "Launch Firefox");
    assert_eq!(
        pinned_result(&launcher),
        Key::program(firefox, "").id(),
        "the sample's result id is the application's id"
    );
    search(&launcher, "launch fire");
    block_on(launcher.activate_selected());
    assert_eq!(system.opened(), [r"C:\Places\2\Firefox.lnk"]);

    // Its command lists the application once and opens it by its id.
    let sample = format!("{language} applications sample");
    search(&launcher, &sample.to_lowercase());
    select_title(&launcher, &sample);
    block_on(launcher.activate_selected());
    assert_eq!(titles(&launcher), ["Firefox"]);
    block_on(launcher.activate_selected());
    assert_eq!(shown(&launcher), Status::Result("Opened Firefox".into()));
    assert_eq!(
        system.opened(),
        [r"C:\Places\2\Firefox.lnk", r"C:\Places\2\Firefox.lnk"]
    );
}

#[test]
fn a_javascript_command_opens_an_application_by_its_stable_id() {
    a_js_command_opens_an_application_by_its_stable_id("sample-applications-js", "JavaScript");
}

#[test]
fn a_typescript_command_opens_an_application_by_its_stable_id() {
    a_js_command_opens_an_application_by_its_stable_id("sample-applications-ts", "TypeScript");
}
