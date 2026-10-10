//! Switch Windows (#263, ADR 0040) through the launcher's public
//! interface, with the windows samples in Rust, JavaScript and
//! TypeScript (`guests/sample-switch-windows`, `-js`, `-ts`), which
//! answer the same: the windows Alt+Tab would show are listed with their
//! titles and their applications' names, in the order the adapter gave
//! (the front application's first), typing filters them by title and
//! application name, and Enter switches to the chosen one, which says
//! what it answered or why it could not. The adapter the host functions
//! act on is a fake that answers the windows the test sets and records
//! what was activated.
//!
//! The window-to-application resolution — the window's own
//! AppUserModelID first, then its program's path, then its process's
//! package identity, matched against the installed applications — is a
//! pure function over fake applications, tested here on every system,
//! including a web app under its own identity. The rule that says which
//! windows count is tested in its module.
//!
//! On Windows, the real adapter is asked for its list against a window
//! of the test's own (a form a script of the test's runs in a process of
//! its own, since a window of the test's own process is never listed),
//! which touches nothing but that window and its process; and the real
//! default extension is acquired from an artifact source on 127.0.0.1
//! and driven with the fake adapter. Its package declares `windows`
//! alone, so those tests run on Windows only. Switching with real input
//! is the GUI smoke's phase, not a test's.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use pane_core::applications::{Catalog, Key, Source};
use pane_core::switch_windows::{Facts, SwitchWindows, Window, WindowsError, window as record};
use pane_core::system_icons::{SystemIcon, SystemIcons};
use pane_core::{Launcher, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/defaults.rs"]
mod defaults;
#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/repo_server.rs"]
mod repo_server;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guests;
use rows::{select_title, titles, to_root};

/// One window of the fake's list, as the tests say it.
fn window(id: &str, title: &str, application: &str) -> Window {
    Window {
        id: id.into(),
        title: title.into(),
        application: application.into(),
        icon: None,
        minimized: false,
        maximized: false,
        elsewhere: false,
        elevated: false,
    }
}

/// A window of the fake's list, with every field the tests say.
#[allow(clippy::too_many_arguments)]
fn listed(
    id: &str,
    title: &str,
    application: &str,
    icon: Option<&str>,
    minimized: bool,
    maximized: bool,
    elsewhere: bool,
    elevated: bool,
) -> Window {
    Window {
        id: id.into(),
        title: title.into(),
        application: application.into(),
        icon: icon.map(str::to_owned),
        minimized,
        maximized,
        elsewhere,
        elevated,
    }
}

/// The host's icon extraction, stood in for: it answers a small PNG for
/// a path that exists and for a Windows `shell:` name — the system's to
/// find, as Windows' own extraction reads it — recording the paths it
/// was asked for, which are what the windows' records named (#263).
#[derive(Clone, Default)]
struct FakeIcons {
    asked: Arc<Mutex<Vec<String>>>,
}

impl FakeIcons {
    /// The paths it was asked for, in order.
    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl SystemIcons for FakeIcons {
    fn icon(&self, path: &Path) -> Result<SystemIcon, String> {
        self.asked.lock().unwrap().push(path.display().to_string());
        let shell = path
            .to_string_lossy()
            .get(..6)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("shell:"));
        if shell || path.exists() {
            Ok(SystemIcon::Png(small_png()))
        } else {
            Err(format!("{} does not exist", path.display()))
        }
    }
}

/// A small PNG, as the system's icon extraction answers.
fn small_png() -> Vec<u8> {
    let pixels = [48, 164, 108, 255].repeat(64);
    pane_core::icons::encode_png(8, 8, &pixels).expect("a PNG")
}

/// A fake of the open windows for the tests (#263): it answers the
/// windows the test sets, in the order it set them (the front
/// application's first, as the real adapter lists), and records which it
/// was asked to activate; an activation can be told to fail, as a window
/// that closed would.
#[derive(Clone, Default)]
struct FakeWindows {
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    /// The windows, as the fake lists them.
    listed: Vec<Window>,
    /// What the listing answers from now on.
    listing: Option<WindowsError>,
    /// The ids it was asked to activate, in order.
    activated: Vec<String>,
    /// What every activation answers from now on.
    answer: Option<WindowsError>,
}

impl FakeWindows {
    /// The windows the fake lists from now on.
    fn set(&self, listed: Vec<Window>) {
        self.state.lock().unwrap().listed = listed;
    }

    /// What the listing answers from now on.
    fn fail_list(&self, error: WindowsError) {
        self.state.lock().unwrap().listing = Some(error);
    }

    /// What every activation answers from now on.
    fn fail(&self, error: WindowsError) {
        self.state.lock().unwrap().answer = Some(error);
    }

    /// The ids the fake was asked to activate, in order, forgotten once
    /// read.
    fn activated(&self) -> Vec<String> {
        std::mem::take(&mut self.state.lock().unwrap().activated)
    }
}

impl SwitchWindows for FakeWindows {
    fn list(&self) -> Result<Vec<Window>, WindowsError> {
        let state = self.state.lock().unwrap();
        match state.listing.clone() {
            Some(error) => Err(error),
            None => Ok(state.listed.clone()),
        }
    }

    fn activate(&self, id: &str) -> Result<(), WindowsError> {
        let mut state = self.state.lock().unwrap();
        state.activated.push(id.to_owned());
        match state.answer.clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// A shortcut to a Start menu application in a fake catalog.
fn shortcut(name: &str, program: &str) -> Source {
    let key = Key::program(program, "");
    let path = format!(r"C:\Menu\{name}.lnk");
    Source {
        program: Some(program.into()),
        ..Source::new(key, path, name, "the Start menu", 2)
    }
}

/// The facts of one window, as the adapter read it: its handle, its
/// title, and what resolves its application.
fn facts(
    handle: usize,
    title: &str,
    aumid: Option<&str>,
    program: Option<&str>,
    family: Option<&str>,
) -> Facts {
    Facts {
        window: handle,
        title: title.into(),
        aumid: aumid.map(str::to_owned),
        program: program.map(str::to_owned),
        family: family.map(str::to_owned),
        minimized: false,
        maximized: false,
        elsewhere: false,
        elevated: false,
    }
}

#[test]
fn a_window_s_application_comes_from_its_own_app_user_model_id() {
    // A web app (a progressive web app) installed from the browser has
    // its own identity in the Apps folder: a window carrying its
    // AppUserModelID resolves to the web app's application, while a
    // window of the same browser with no AppUserModelID of its own
    // resolves to the browser.
    let aumid = "Chrome.PWA_abc";
    let pwa = Source::new(
        Key::Path(format!("shell:AppsFolder\\{aumid}")),
        format!("shell:AppsFolder\\{aumid}"),
        "Contoso Notes",
        "the Apps folder",
        5,
    );
    let browser = shortcut("Google Chrome", r"C:\Chrome\chrome.exe");
    let installed = Catalog::new(vec![pwa, browser]);
    let web_app = facts(
        101,
        "Contoso Notes",
        Some(aumid),
        Some(r"C:\Chrome\chrome.exe"),
        None,
    );
    assert_eq!(
        record(&web_app, &installed).application,
        "Contoso Notes",
        "the web app, not the browser"
    );
    let browsing = facts(
        102,
        "Contoso — search",
        None,
        Some(r"C:\Chrome\chrome.exe"),
        None,
    );
    assert_eq!(
        record(&browsing, &installed).application,
        "Google Chrome",
        "the browser's own window"
    );
}

#[test]
fn a_store_app_hosted_by_the_frame_process_is_found_by_its_identity() {
    // The frame host shows a Store app's window: the window's own
    // AppUserModelID names the app, whose icon is its Apps Folder name.
    let aumid = "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App";
    let source = Source::new(
        Key::Package {
            family: "microsoft.windowscalculator_8wekyb3d8bbwe".into(),
            app: None,
        },
        format!("shell:AppsFolder\\{aumid}"),
        "Calculator",
        "the Apps folder",
        5,
    );
    let installed = Catalog::new(vec![source]);
    let hosted = facts(
        7,
        "Calculator",
        Some(aumid),
        Some(r"C:\Windows\System32\ApplicationFrameHost.exe"),
        None,
    );
    assert_eq!(
        record(&hosted, &installed),
        listed(
            "7",
            "Calculator",
            "Calculator",
            Some("shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
            false,
            false,
            false,
            false,
        )
    );
}

#[test]
fn a_window_s_program_names_its_application() {
    let code = shortcut("Visual Studio Code", r"C:\VS Code\Code.exe");
    let installed = Catalog::new(vec![code]);
    let editing = facts(
        9,
        "notes.rs - Visual Studio Code",
        None,
        Some(r"C:\VS Code\Code.exe"),
        None,
    );
    let answered = record(&editing, &installed);
    assert_eq!(answered.application, "Visual Studio Code");
    assert_eq!(answered.icon.as_deref(), Some(r"C:\VS Code\Code.exe"));
    // A program nothing installed names: the window's title stands in,
    // with the program's own icon.
    let plain = facts(
        10,
        "Untitled - Paint",
        None,
        Some(r"C:\Windows\System32\mspaint.exe"),
        None,
    );
    let answered = record(&plain, &installed);
    assert_eq!(answered.application, "Untitled - Paint");
    assert_eq!(
        answered.icon.as_deref(),
        Some(r"C:\Windows\System32\mspaint.exe")
    );
}

#[test]
fn a_window_s_process_names_its_packaged_application() {
    let key = Key::Package {
        family: "contoso.suite_abc".into(),
        app: None,
    };
    let source = Source::new(
        key,
        r"shell:AppsFolder\Contoso.Suite_abc!Writer",
        "Contoso Suite",
        "the Apps folder",
        5,
    );
    let installed = Catalog::new(vec![source]);
    let writing = facts(11, "Writer", None, None, Some("Contoso.Suite_abc"));
    let answered = record(&writing, &installed);
    assert_eq!(answered.application, "Contoso Suite");
    assert_eq!(
        answered.icon.as_deref(),
        Some(r"shell:AppsFolder\Contoso.Suite_abc!Writer")
    );
}

#[test]
fn a_window_with_nothing_known_of_it_is_named_by_its_title() {
    let nothing = facts(12, "Some window", None, None, None);
    let answered = record(&nothing, &Catalog::default());
    assert_eq!(answered.application, "Some window");
    assert_eq!(answered.icon, None);
    // An empty title falls back to the program's name.
    let untitled = facts(13, " ", None, Some(r"C:\Tools\wt.exe"), None);
    let answered = record(&untitled, &Catalog::default());
    assert_eq!(answered.application, "wt");
    assert_eq!(answered.icon.as_deref(), Some(r"C:\Tools\wt.exe"));
}

#[test]
fn a_window_s_record_carries_what_it_is() {
    let fact = Facts {
        window: 42,
        title: "Calculator".into(),
        aumid: None,
        program: None,
        family: None,
        minimized: true,
        maximized: false,
        elsewhere: true,
        elevated: true,
    };
    assert_eq!(
        record(&fact, &Catalog::default()),
        listed(
            "42",
            "Calculator",
            "Calculator",
            None,
            true,
            false,
            true,
            true,
        )
    );
}

/// One language's windows sample package.
struct Sample {
    /// Its assembled package, under `target/guests/packages`.
    package: &'static str,
    /// Its command's title in root search.
    title: &'static str,
}

const RUST: Sample = Sample {
    package: "sample-switch-windows",
    title: "Switch Windows sample",
};
const JAVASCRIPT: Sample = Sample {
    package: "sample-switch-windows-js",
    title: "JavaScript Switch Windows sample",
};
const TYPESCRIPT: Sample = Sample {
    package: "sample-switch-windows-ts",
    title: "TypeScript Switch Windows sample",
};

const SAMPLES: [Sample; 3] = [RUST, JAVASCRIPT, TYPESCRIPT];

/// Copies the assembled package `name` under `target/guests/packages` to
/// `folder`.
fn copy(name: &str, folder: &Path) -> PathBuf {
    let assembled = guests().join("packages").join(name);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    for entry in fs::read_dir(&assembled).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
    }
    folder.to_path_buf()
}

/// One test's Pane: its folders, the fake of the windows its commands
/// reach, the stand-in for the host's icon extraction, and the launcher.
struct Pane {
    _sources: TempDir,
    _data: TempDir,
    fake: FakeWindows,
    icons: FakeIcons,
    launcher: Launcher,
}

impl Pane {
    fn with(sample: &Sample) -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let fake = FakeWindows::default();
        let icons = FakeIcons::default();
        let launcher =
            Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
                .with_switch_windows(Arc::new(fake.clone()))
                .with_system_icons(Arc::new(icons.clone()));
        let folder = copy(sample.package, &sources.path().join(sample.package));
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {}", sample.title))
        );
        Pane {
            _sources: sources,
            _data: data,
            fake,
            icons,
            launcher,
        }
    }

    /// Root search, with `query` typed.
    fn search(&self, query: &str) {
        to_root(&self.launcher);
        block_on(self.launcher.set_query(query));
    }

    /// Opens the sample's command from root search.
    fn open(&self, sample: &Sample) {
        self.search(sample.title);
        select_title(&self.launcher, sample.title);
        block_on(self.launcher.activate_selected());
        assert!(matches!(
            self.launcher.view().screen,
            Screen::CommandSearch { .. }
        ));
    }

    /// Runs the item titled `item` of the open command and says what it
    /// showed: its toast, or the status line.
    fn run(&self, item: &str) -> Status {
        select_title(&self.launcher, item);
        block_on(self.launcher.activate_selected());
        shown(&self.launcher)
    }

    /// The rows on screen as (title, subtitle) pairs.
    fn rows(&self) -> Vec<(String, Option<String>)> {
        self.launcher
            .view()
            .rows
            .iter()
            .map(|row| (row.title.clone(), row.subtitle.clone()))
            .collect()
    }
}

fn the_windows_are_listed_with_their_applications_and_icons(sample: &Sample) {
    let pane = Pane::with(sample);
    // The front application's window first, as the adapter listed them.
    pane.fake.set(vec![
        listed(
            "1",
            "notes.txt - Notepad",
            "Notepad",
            Some(r"C:\Windows\System32\notepad.exe"),
            false,
            false,
            false,
            false,
        ),
        listed(
            "2",
            "Contoso Notes",
            "Contoso Notes",
            Some(r"shell:AppsFolder\Chrome.PWA_abc"),
            false,
            false,
            true,
            false,
        ),
        listed(
            "3",
            "Terminal",
            "Windows Terminal",
            None,
            true,
            false,
            false,
            true,
        ),
    ]);
    pane.open(sample);
    assert_eq!(
        pane.rows(),
        vec![
            ("notes.txt - Notepad".into(), Some("Notepad".into())),
            (
                "Contoso Notes".into(),
                Some("Contoso Notes — on another desktop".into())
            ),
            ("Terminal".into(), Some("Windows Terminal".into())),
        ],
        "{}",
        sample.title
    );
    // Each row's icon is what its window's record named: the host's icon
    // extraction is asked for the record's path or `shell:` name — a
    // program's path where the file is (Windows only, for a Windows
    // path), a shell name on every system, as Windows' own extraction
    // reads it, and nothing for a window whose record knows no icon —
    // and the rows that named one draw the image it answered, the one
    // that named none shows the row's own fallback.
    assert!(
        pane.launcher
            .wait_for_icons(std::time::Duration::from_secs(30)),
        "{}: the icons kept loading",
        sample.title
    );
    let presentation = pane.launcher.presentation();
    let drawn: Vec<bool> = presentation
        .rows
        .iter()
        .map(|row| {
            row.icon
                .as_ref()
                .is_some_and(|icon| matches!(icon.source, pane_core::IconSource::Image { .. }))
        })
        .collect();
    assert_eq!(
        drawn,
        [cfg!(windows), true, false],
        "{}: the rows draw the icons their records named",
        sample.title
    );
    let mut asked = pane.icons.asked();
    asked.sort();
    let mut expected = vec![r"shell:AppsFolder\Chrome.PWA_abc".to_owned()];
    if cfg!(windows) {
        expected.push(r"C:\Windows\System32\notepad.exe".to_owned());
    }
    expected.sort();
    assert_eq!(
        asked, expected,
        "{}: the extraction was asked for what the records named",
        sample.title
    );
}

fn a_window_is_switched_to_by_choosing_it(sample: &Sample) {
    let pane = Pane::with(sample);
    pane.fake.set(vec![
        window("1", "notes.txt - Notepad", "Notepad"),
        window("2", "Slack", "Slack"),
    ]);
    pane.open(sample);

    // Enter switches to the selected window, which says so in a toast.
    assert_eq!(
        pane.run("Slack"),
        Status::Result("Switched to Slack".into()),
        "{}",
        sample.title
    );
    assert_eq!(pane.fake.activated(), ["2"], "{}", sample.title);

    // The list is drawn again, and another window can be switched to.
    assert_eq!(titles(&pane.launcher), ["notes.txt - Notepad", "Slack"]);
    assert_eq!(
        pane.run("notes.txt - Notepad"),
        Status::Result("Switched to notes.txt - Notepad".into())
    );
    assert_eq!(pane.fake.activated(), ["1"]);
}

fn a_window_that_closed_says_why(sample: &Sample) {
    let pane = Pane::with(sample);
    pane.fake.set(vec![window("1", "Gone", "Editor")]);
    pane.fake.fail(WindowsError::Failed(
        "that window closed since it was listed".into(),
    ));
    pane.open(sample);
    assert_eq!(
        pane.run("Gone"),
        Status::Error("Switch to Gone: that window closed since it was listed".into()),
        "{}",
        sample.title
    );
    assert_eq!(pane.fake.activated(), ["1"], "{}", sample.title);
}

fn typing_filters_the_windows_by_title_and_application(sample: &Sample) {
    let pane = Pane::with(sample);
    pane.fake.set(vec![
        window("1", "sales.csv - Excel", "Excel"),
        window("2", "Standup — Slack", "Slack"),
        window("3", "roadmap.md - VS Code", "VS Code"),
    ]);
    pane.open(sample);
    assert_eq!(titles(&pane.launcher).len(), 3, "{}", sample.title);

    // Typing a title's part filters by title...
    block_on(pane.launcher.set_query("roadmap"));
    assert_eq!(
        titles(&pane.launcher),
        ["roadmap.md - VS Code"],
        "{}",
        sample.title
    );
    // ...and typing an application's name by application.
    block_on(pane.launcher.set_query("slack"));
    assert_eq!(
        titles(&pane.launcher),
        ["Standup — Slack"],
        "{}",
        sample.title
    );
    // Nothing matches: no rows.
    block_on(pane.launcher.set_query("zzz"));
    assert_eq!(
        titles(&pane.launcher),
        Vec::<String>::new(),
        "{}",
        sample.title
    );
    // Choosing a search result switches to its window.
    block_on(pane.launcher.set_query("slack"));
    select_title(&pane.launcher, "Standup — Slack");
    block_on(pane.launcher.activate_selected());
    assert_eq!(
        shown(&pane.launcher),
        Status::Result("Switched to Standup — Slack".into()),
        "{}",
        sample.title
    );
    assert_eq!(pane.fake.activated(), ["2"], "{}", sample.title);
}

#[test]
fn the_samples_list_the_windows_with_their_applications_and_icons() {
    for sample in SAMPLES {
        the_windows_are_listed_with_their_applications_and_icons(&sample);
    }
}

#[test]
fn the_samples_switch_to_a_window_and_say_what_it_answered() {
    for sample in SAMPLES {
        a_window_is_switched_to_by_choosing_it(&sample);
        a_window_that_closed_says_why(&sample);
    }
}

#[test]
fn the_samples_filter_the_windows_as_the_user_types() {
    for sample in SAMPLES {
        typing_filters_the_windows_by_title_and_application(&sample);
    }
}

#[test]
fn a_listing_with_no_windows_says_so() {
    for sample in SAMPLES {
        let pane = Pane::with(&sample);
        pane.open(&sample);
        assert_eq!(
            pane.rows(),
            [(
                "No windows are open".into(),
                Some("Nothing Alt+Tab would show is listed".into())
            )],
            "{}",
            sample.title
        );
    }
}

#[test]
fn a_listing_that_could_not_be_read_is_an_error() {
    for sample in SAMPLES {
        let pane = Pane::with(&sample);
        // The listing answers `not-available`, which the sample shows as
        // the error it is given.
        pane.fake.fail_list(WindowsError::NotAvailable(
            "The open windows are not available on this system yet".into(),
        ));
        pane.search(sample.title);
        select_title(&pane.launcher, sample.title);
        block_on(pane.launcher.activate_selected());
        assert_eq!(
            shown(&pane.launcher),
            Status::Error(
                "The extension reported an error: The open windows are not available on this \
                 system yet"
                    .into()
            ),
            "{}",
            sample.title
        );
    }
}

#[cfg(target_os = "windows")]
mod real {
    use super::*;

    use std::time::{Duration, Instant};

    use pane_core::switch_windows::native;

    /// How long the test waits for anything it waits for, and how often
    /// it looks while it waits.
    const WAIT: Duration = Duration::from_secs(30);
    const LOOK: Duration = Duration::from_millis(100);

    /// The title of the form the test runs in a process of its own, which
    /// it is found by in the listing.
    const OWN: &str = "Pane switch windows test window";

    /// A form the test runs in a process of its own: a window of the
    /// test's own process is never listed, since it is Pane's own. It is
    /// ended with its process at the end.
    struct Form {
        _folder: tempfile::TempDir,
        child: std::process::Child,
    }

    impl Form {
        /// Starts the form, which shows itself and stays up for as long
        /// as the test needs it.
        fn start() -> Form {
            let folder = tempfile::tempdir().unwrap();
            let script = "\n\
Add-Type -AssemblyName System.Windows.Forms\n\
$form = New-Object System.Windows.Forms.Form\n\
$form.Text = 'Pane switch windows test window'\n\
$form.Show()\n\
$still = [DateTime]::UtcNow.AddSeconds(30)\n\
while ([DateTime]::UtcNow -lt $still) {\n\
  [System.Windows.Forms.Application]::DoEvents()\n\
  Start-Sleep -Milliseconds 100\n\
}\n\
Start-Sleep -Seconds 300\n";
            let file = folder.path().join("form.ps1");
            fs::write(&file, script).unwrap();
            use std::os::windows::process::CommandExt;
            let mut powershell = std::process::Command::new("powershell.exe");
            powershell
                .arg("-NoProfile")
                .arg("-STA")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-File")
                .arg(&file);
            // CREATE_NO_WINDOW: no console of its own.
            powershell.creation_flags(0x0800_0000);
            let child = powershell
                .spawn()
                .expect("the test's form process could start");
            Form {
                _folder: folder,
                child,
            }
        }
    }

    impl Drop for Form {
        /// Ends the form's process, which would otherwise never answer
        /// again.
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// Waits up to [`WAIT`] for `check` to answer something, answering
    /// what it said; panics with `what` if it never did.
    fn wait_for<T>(what: &str, check: impl Fn() -> Option<T>) -> T {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(answer) = check() {
                return answer;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(LOOK);
        }
    }

    #[test]
    fn the_real_adapter_lists_a_window_of_the_test_s_own() {
        // Enumeration alone: it touches nothing but the windows it reads.
        // Switching with real input is the GUI smoke's phase.
        let form = Form::start();
        let adapter = native();

        // The form's window is listed, by its title, with the icon of the
        // application its process is (Windows PowerShell).
        let found = wait_for("the test's window to be listed", || {
            adapter
                .list()
                .expect("the open windows could be listed")
                .into_iter()
                .find(|window| window.title == OWN)
        });
        assert!(
            found
                .icon
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains("powershell"),
            "its application's icon names its program: {:?}",
            found.icon
        );
        // The ids are opaque numbers of the session: the same listing
        // answers the same one.
        assert!(found.id.chars().all(|digit| digit.is_ascii_digit()));
        let again = adapter.list().expect("the open windows could be listed");
        assert_eq!(
            again
                .iter()
                .find(|window| window.title == OWN)
                .map(|window| window.id.clone()),
            Some(found.id)
        );

        // An id no window of this session has is refused; no real window
        // is activated here.
        let refused = adapter
            .activate("not-a-window")
            .expect_err("an id no window has");
        assert!(
            refused.message().contains("is no window of this session"),
            "{}",
            refused.message()
        );
        drop(form);
    }
}

#[cfg(target_os = "windows")]
mod extension {
    use std::thread;
    use std::time::{Duration, Instant};

    use futures::executor::block_on;
    use pane_core::PackageIdentity;

    use super::defaults;
    use super::feedback::RecordingWindow;
    use super::repo_server;
    use super::rows::{select_title, titles, to_root};
    use super::{FakeWindows, WindowsError, listed, window};
    use pane_core::{Screen, Status};

    /// How long a launch or a guest call may take: compiling the guest
    /// once is included; a slow, busy machine is not.
    const PROMPTLY: Duration = Duration::from_secs(60);

    /// One test's Pane: its data folder, the artifact source the default
    /// extension is acquired from, the fake of the windows its command
    /// reaches, and the launcher.
    struct Pane {
        _data: tempfile::TempDir,
        _repos: tempfile::TempDir,
        _server: repo_server::Server,
        fake: FakeWindows,
        launcher: pane_core::Launcher,
        identity: PackageIdentity,
    }

    impl Pane {
        fn new() -> Pane {
            let data = tempfile::tempdir().unwrap();
            // The package's own repository, served as a release revision:
            // a stand-in for the one a release pins the default to.
            let server = repo_server::Server::start();
            let repos = tempfile::tempdir().unwrap();
            let pin = defaults::from_sample(
                &server,
                repos.path(),
                "switch-windows",
                "Switch Windows",
                "switch-windows",
            );
            let fake = FakeWindows::default();
            let launcher = pane_core::Launcher::with_packages(
                pane_core::Runtime::start(),
                vec![],
                data.path().join("extensions"),
            )
            .with_defaults(vec![pin])
            .with_switch_windows(std::sync::Arc::new(fake.clone()));
            block_on(launcher.acquire_defaults());
            assert!(
                matches!(launcher.view().status, Status::Result(_)),
                "{:?}",
                launcher.view().status
            );
            Pane {
                _data: data,
                _repos: repos,
                _server: server,
                fake,
                launcher,
                identity: PackageIdentity::default_extension("switch-windows"),
            }
        }

        /// Root search, with `query` typed.
        fn search(&self, query: &str) {
            to_root(&self.launcher);
            block_on(self.launcher.set_query(query));
        }

        /// Opens the Switch Windows command from root search.
        fn open(&self) {
            self.search("Switch Windows");
            select_title(&self.launcher, "Switch Windows");
            block_on(self.launcher.activate_selected());
            assert!(matches!(
                self.launcher.view().screen,
                Screen::CommandSearch { .. }
            ));
        }
    }

    /// Waits up to [`PROMPTLY`] for `check`, panicking with `what` if it
    /// never holds.
    fn wait(what: &str, check: impl Fn() -> bool) {
        let deadline = Instant::now() + PROMPTLY;
        while !check() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn the_switch_windows_default_extension_lists_and_switches() {
        let pane = Pane::new();
        let window = RecordingWindow::attach(&pane.launcher);

        // Acquired as the default extension, with its identity, and
        // enabled by default.
        let installed = pane
            .launcher
            .packages()
            .into_iter()
            .find(|package| package.identity == pane.identity)
            .expect("Switch Windows was installed");
        assert!(installed.enabled, "enabled by default");

        // Its command lists the windows the fake adapter answers, in the
        // order it gave them (the front application's first), with their
        // applications' names and the icons their records named.
        pane.fake.set(vec![
            listed(
                "1",
                "notes.txt - Notepad",
                "Notepad",
                Some(r"C:\Windows\System32\notepad.exe"),
                false,
                false,
                false,
                false,
            ),
            listed(
                "2",
                "Terminal",
                "Windows Terminal",
                None,
                false,
                true,
                false,
                false,
            ),
        ]);
        pane.open();
        wait("the windows to be listed", || {
            titles(&pane.launcher) == ["notes.txt - Notepad", "Terminal"]
        });
        let rows: Vec<(String, Option<String>)> = pane
            .launcher
            .view()
            .rows
            .iter()
            .map(|row| (row.title.clone(), row.subtitle.clone()))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("notes.txt - Notepad".into(), Some("Notepad".into())),
                ("Terminal".into(), Some("Windows Terminal".into())),
            ]
        );

        // Enter switches to the selected window: the window the
        // extension chose is activated, and the launcher's window is
        // asked to hide (closing it, as the window takes the
        // foreground).
        select_title(&pane.launcher, "Terminal");
        block_on(pane.launcher.activate_selected());
        assert_eq!(pane.fake.activated(), ["2"]);
        assert_eq!(window.hides(), 1, "the launcher closed");
        assert_eq!(titles(&pane.launcher), ["notes.txt - Notepad", "Terminal"]);

        // A window that closed says why in a HUD: the launcher is hidden,
        // so the toast the sample answered with is shown as one.
        pane.fake.fail(WindowsError::Failed(
            "that window closed since it was listed".into(),
        ));
        select_title(&pane.launcher, "notes.txt - Notepad");
        block_on(pane.launcher.activate_selected());
        let huds: Vec<String> = window.huds().iter().map(|hud| hud.title.clone()).collect();
        assert!(
            huds.iter()
                .any(|hud| hud.contains("closed since it was listed")),
            "{huds:?}"
        );
    }

    #[test]
    fn the_switch_windows_default_extension_filters_as_the_user_types() {
        let pane = Pane::new();
        pane.fake.set(vec![
            window("1", "sales.csv - Excel", "Excel"),
            window("2", "Standup — Slack", "Slack"),
            window("3", "roadmap.md - VS Code", "VS Code"),
        ]);
        pane.open();
        wait("the windows to be listed", || {
            titles(&pane.launcher).len() == 3
        });

        // Typing filters by title and by application name, and choosing
        // a result switches to its window.
        block_on(pane.launcher.set_query("slack"));
        wait("the filtered list", || {
            titles(&pane.launcher) == ["Standup — Slack"]
        });
        select_title(&pane.launcher, "Standup — Slack");
        block_on(pane.launcher.activate_selected());
        assert_eq!(pane.fake.activated(), ["2"]);
    }

    #[test]
    fn the_switch_windows_default_extension_is_disableable_on_its_own() {
        let pane = Pane::new();
        pane.search("Switch Windows");
        assert_eq!(titles(&pane.launcher), ["Switch Windows"]);
        // Disabled on its own: its command contributes nothing.
        block_on(pane.launcher.set_enabled(&pane.identity, false));
        pane.search("Switch Windows");
        assert_eq!(titles(&pane.launcher), Vec::<String>::new());

        // Enabled again, its command comes back.
        block_on(pane.launcher.set_enabled(&pane.identity, true));
        pane.search("Switch Windows");
        assert_eq!(titles(&pane.launcher), ["Switch Windows"]);
    }
}
