//! Applications' names through the launcher's public interface, with the
//! real Applications guest (`target/guests/packages/applications`) and the
//! host's list of applications ([`Cached`]) over a fake system whose
//! sources the tests decide: a localized title is listed and the
//! untranslated name still finds it; a program's name finds its
//! application unless it is generic, shared or the shortcut passes
//! arguments; keywords find an application; a result found by another
//! name shows its real title; applications of one name are told apart by
//! their subtitles and their pins; and an extension gives its own indexed
//! results alternate titles and keywords. The pure rules are unit tests of
//! `pane_core::applications::names`; the systems' names are checked in
//! `application_adapters.rs`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::executor::block_on;
use pane_core::applications::{Cached, Discovery, Key, Source};
use pane_core::{Launcher, ResultAction, Runtime, SlotChange, Status};
use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

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

/// A Start menu shortcut named `name` (as Explorer shows it) in `folder`,
/// to `target` with `arguments`.
fn shortcut(folder: &str, name: &str, target: &str, arguments: &str) -> Source {
    let folder = format!(r"C:\Menu\{folder}");
    Source {
        program: Some(target.to_owned()),
        arguments: !arguments.trim().is_empty(),
        ..Source::new(
            Key::program(target, arguments),
            format!(r"{folder}\{name}.lnk"),
            name,
            folder,
            2,
        )
    }
}

/// A launcher, with its own data folder, whose host lists `system`'s
/// applications by identity, with the Applications package and `others`
/// installed.
struct Fixture {
    data: TempDir,
    _cache: TempDir,
    launcher: Launcher,
}

fn launcher(system: &Arc<FakeSystem>, others: &[&str]) -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let runtime = Runtime::start_with_cache(cache.path().to_path_buf()).unwrap();
    runtime.set_applications(Arc::new(Cached::new(
        system.clone(),
        Duration::from_secs(3600),
    )));
    let launcher = Launcher::with_packages(Ok(runtime), vec![], data.path().join("extensions"))
        .with_quick_slots(data.path());
    install(&launcher, &built("packages/applications"));
    for other in others {
        install(&launcher, &built(&format!("packages/{other}")));
    }
    Fixture {
        data,
        _cache: cache,
        launcher,
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

/// Root search's rows for `query`, each as (title, subtitle).
fn rows(launcher: &Launcher, query: &str) -> Vec<(String, Option<String>)> {
    search(launcher, query);
    launcher
        .view()
        .rows
        .into_iter()
        .map(|row| (row.title, row.subtitle))
        .collect()
}

/// (title, subtitle) as root search lists an application.
fn row(title: &str, subtitle: &str) -> (String, Option<String>) {
    (title.to_owned(), Some(subtitle.to_owned()))
}

#[test]
fn a_localized_title_is_listed_and_the_untranslated_name_still_finds_it() {
    let paint = Source {
        untranslated: Some("Paint".into()),
        ..shortcut(
            "Phụ kiện",
            "Ứng dụng Vẽ",
            r"C:\Windows\System32\mspaint.exe",
            "",
        )
    };
    let system = FakeSystem::with(vec![paint.clone()]);
    let fixture = launcher(&system, &[]);
    let launcher = &fixture.launcher;

    assert_eq!(
        rows(launcher, "ứng dụng"),
        [row("Ứng dụng Vẽ", "Application")]
    );
    // The English name a tutorial gives, and the program's: the row shows
    // the title the user sees in their Start menu.
    assert_eq!(rows(launcher, "paint"), [row("Ứng dụng Vẽ", "Application")]);
    assert_eq!(
        rows(launcher, "mspaint"),
        [row("Ứng dụng Vẽ", "Application")]
    );

    block_on(launcher.activate_selected());
    assert_eq!(system.opened(), [paint.path]);
    assert_eq!(
        launcher.view().status,
        Status::Result("Opened Ứng dụng Vẽ".into())
    );
}

#[test]
fn a_program_s_name_finds_its_application_unless_generic_shared_or_given_arguments() {
    let system = FakeSystem::with(vec![
        shortcut("Code", "Visual Studio Code", r"C:\VS Code\Code.exe", ""),
        shortcut("Terminal", "Windows Terminal", r"C:\Terminal\wt.exe", ""),
        // Generic program names.
        shortcut("Game", "Space Game", r"C:\Games\Space\launcher.exe", ""),
        shortcut("Tool", "Cleaner", r"C:\Cleaner\setup.exe", ""),
        // One program name, two applications.
        shortcut("Writer", "Writer", r"C:\Writer\editor.exe", ""),
        shortcut(
            "Writer Beta",
            "Writer Beta",
            r"C:\Writer Beta\editor.exe",
            "",
        ),
        // A browser and a web app it hosts.
        shortcut("Chrome", "Google Chrome", r"C:\Chrome\chrome.exe", ""),
        shortcut(
            "Chrome Apps",
            "Gmail",
            r"C:\Chrome\chrome.exe",
            "--profile-directory=Default --app-id=mail",
        ),
    ]);
    let fixture = launcher(&system, &[]);
    let launcher = &fixture.launcher;

    assert_eq!(
        titles_for(launcher, "code"),
        ["Manage Extensions", "Visual Studio Code"]
    );
    assert_eq!(
        titles_for(launcher, "wt"),
        ["Windows Terminal", "Writer", "Writer Beta"]
    );
    // Typing a role finds nothing by it.
    assert!(titles_for(launcher, "launcher").is_empty());
    assert!(titles_for(launcher, "setup").is_empty());
    // A name two applications share picks neither, though their titles
    // still find them.
    assert!(titles_for(launcher, "editor").is_empty());
    assert_eq!(titles_for(launcher, "writer"), ["Writer", "Writer Beta"]);
    // The browser's name finds the browser, not the web app it hosts.
    assert_eq!(titles_for(launcher, "chrome"), ["Google Chrome"]);

    // The one found by its program's name opens.
    search(launcher, "wt");
    block_on(launcher.activate_selected());
    assert_eq!(system.opened(), [r"C:\Menu\Terminal\Windows Terminal.lnk"]);
}

/// The titles root search lists for `query`.
fn titles_for(launcher: &Launcher, query: &str) -> Vec<String> {
    search(launcher, query);
    let mut found = titles(launcher);
    // Equally good matches keep root search's order, which is not what
    // these tests are about.
    found.sort();
    found
}

#[test]
fn a_desktop_entry_s_program_and_keywords_find_it() {
    let terminal = Source {
        program: Some("gnome-terminal".into()),
        keywords: vec!["shell".into(), "prompt".into(), "command line".into()],
        ..Source::new(
            Key::DesktopFile("org.gnome.Terminal.desktop".into()),
            "/usr/share/applications/org.gnome.Terminal.desktop",
            "Terminal",
            "/usr/share/applications",
            1,
        )
    };
    let system = FakeSystem::with(vec![terminal]);
    let fixture = launcher(&system, &[]);
    let launcher = &fixture.launcher;

    assert_eq!(
        rows(launcher, "gnome-terminal"),
        [row("Terminal", "Application")]
    );
    assert_eq!(rows(launcher, "prompt"), [row("Terminal", "Application")]);
    assert_eq!(
        rows(launcher, "command line"),
        [row("Terminal", "Application")]
    );
    assert!(rows(launcher, "browser").is_empty());
}

#[test]
fn applications_of_one_name_are_told_apart_by_their_subtitles() {
    let system = FakeSystem::with(vec![
        shortcut("Python 3.11", "Python", r"C:\Python311\python.exe", ""),
        shortcut("Python 3.12", "Python", r"C:\Python312\python.exe", ""),
        shortcut("Editor", "Editor", r"C:\Editor\editor.exe", ""),
        shortcut(
            "Editor Preview",
            "Editor",
            r"C:\Editor\editor-preview.exe",
            "",
        ),
        shortcut("Notes", "Notes", r"C:\Notes\notes.exe", ""),
    ]);
    let fixture = launcher(&system, &[]);
    let launcher = &fixture.launcher;

    // The shortest thing unique among them: the program's name, else its
    // folder.
    let mut pythons = rows(launcher, "python");
    pythons.sort();
    assert_eq!(
        pythons,
        [row("Python", "Python311"), row("Python", "Python312")]
    );
    let mut editors = rows(launcher, "editor");
    editors.sort();
    assert_eq!(
        editors,
        [row("Editor", "editor"), row("Editor", "editor-preview")]
    );
    // Alone with its name: as plain as ever.
    assert_eq!(rows(launcher, "notes"), [row("Notes", "Application")]);
}

#[test]
fn a_pinned_application_sharing_its_name_says_what_tells_it_apart() {
    let system = FakeSystem::with(vec![
        shortcut("Python 3.11", "Python", r"C:\Python311\python.exe", ""),
        shortcut("Python 3.12", "Python", r"C:\Python312\python.exe", ""),
        shortcut("Notes", "Notes", r"C:\Notes\notes.exe", ""),
    ]);
    let fixture = launcher(&system, &[]);
    let launcher = &fixture.launcher;
    pin_row(launcher, "python", "Python312");
    pin_row(launcher, "notes", "Application");
    search(launcher, "");

    let slots = launcher.quick_slots();
    let shown: Vec<(&str, Option<&str>)> = slots
        .iter()
        .map(|slot| (slot.title.as_str(), slot.detail.as_deref()))
        .collect();
    assert_eq!(shown, [("Python", Some("Python312")), ("Notes", None)]);
    assert!(fixture.data.path().join("quick-slots.json").exists());

    block_on(launcher.activate_quick_slot(0));
    assert_eq!(system.opened(), [r"C:\Menu\Python 3.12\Python.lnk"]);
}

/// Pins root search's row found by `query` whose subtitle is `subtitle`.
fn pin_row(launcher: &Launcher, query: &str, subtitle: &str) {
    search(launcher, query);
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.subtitle.as_deref() == Some(subtitle))
        .unwrap_or_else(|| panic!("no row subtitled {subtitle:?}"));
    launcher.select(index);
    let target = launcher.view().rows[index].id.clone();
    let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::Pin);
    assert!(matches!(change, SlotChange::Changed(_)), "{change:?}");
    block_on(recorded);
}

/// The JavaScript and TypeScript author examples give their own indexed
/// results the application's alternate titles and keywords, which find
/// them as Pane's own results are found.
fn a_js_command_gives_its_results_alternate_titles_and_keywords(package: &str) {
    let terminal = Source {
        keywords: vec!["console".into()],
        ..shortcut("Terminal", "Windows Terminal", r"C:\Terminal\wt.exe", "")
    };
    let system = FakeSystem::with(vec![terminal]);
    let fixture = launcher(&system, &[package]);
    let launcher = &fixture.launcher;

    // `Launch wt`, the sample's alternate title from the program's name:
    // the row shows its real title.
    assert_eq!(
        titles_for(launcher, "launch wt"),
        ["Launch Windows Terminal"]
    );
    // The keyword finds the sample's result and Pane's own.
    assert_eq!(
        titles_for(launcher, "console"),
        ["Launch Windows Terminal", "Windows Terminal"]
    );

    search(launcher, "launch wt");
    select_title(launcher, "Launch Windows Terminal");
    block_on(launcher.activate_selected());
    assert_eq!(system.opened(), [r"C:\Menu\Terminal\Windows Terminal.lnk"]);
}

#[test]
fn a_javascript_command_gives_its_results_alternate_titles_and_keywords() {
    a_js_command_gives_its_results_alternate_titles_and_keywords("sample-applications-js");
}

#[test]
fn a_typescript_command_gives_its_results_alternate_titles_and_keywords() {
    a_js_command_gives_its_results_alternate_titles_and_keywords("sample-applications-ts");
}
