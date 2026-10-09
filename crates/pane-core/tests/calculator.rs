//! The calculator, a default extension, through the launcher's public
//! interface: an expression typed into root search lists its answer first,
//! computed by the calculator's guest from the query, and invoking the
//! answer copies it. The calculator package is the one `cargo xtask guests`
//! assembles in `target/guests/packages/calculator`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::{ComputedAnswer, Launcher, Limits, PackageIdentity, Runtime, Section, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;
#[path = "support/system.rs"]
mod system;

use feedback::RecordingWindow;
use rows::titles;
use system::{Done, RecordingSystem};

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

/// The calculator package's folder.
fn calculator() -> PathBuf {
    built("packages/calculator")
}

/// Pane's data location and compiled code cache for one test.
struct Dirs {
    sources: TempDir,
    data: TempDir,
    cache: TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
            cache: tempfile::tempdir().unwrap(),
        }
    }

    fn runtime(&self) -> Runtime {
        Runtime::start_with_cache(self.cache.path().to_path_buf()).unwrap()
    }

    /// A launcher with the calculator installed.
    fn launcher(&self, runtime: Runtime) -> Launcher {
        let launcher =
            Launcher::with_packages(Ok(runtime), vec![], self.data.path().join("extensions"));
        install(&launcher, &calculator());
        launcher
    }

    /// A package folder named `name` holding `manifest` and `component` as
    /// `command.wasm`.
    fn package(&self, name: &str, manifest: &str, component: &Path) -> PathBuf {
        let folder = self.sources.path().join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("pane.json"), manifest).unwrap();
        fs::copy(component, folder.join("command.wasm")).unwrap();
        folder
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

fn selected_title(launcher: &Launcher) -> Option<String> {
    let view = launcher.view();
    view.selected.map(|index| view.rows[index].title.clone())
}

fn search(launcher: &Launcher, query: &str) {
    block_on(launcher.set_query(query));
}

#[test]
fn an_expression_lists_the_calculators_answer_first_and_selects_it() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher(dirs.runtime());

    search(&launcher, "2 + 3 × 4");

    let view = launcher.view();
    assert_eq!(view.query(), Some("2 + 3 × 4"));
    assert_eq!(titles(&launcher), ["14"]);
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("2 + 3 × 4 = 14 · Enter copies the answer")
    );
    assert_eq!(selected_title(&launcher).as_deref(), Some("14"));
}

#[test]
fn incomplete_and_invalid_expressions_and_ordinary_words_list_no_answer() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher(dirs.runtime());
    let status = launcher.view().status;

    // Incomplete, invalid, undefined and operation-free queries have no
    // answer; that is not a failure of the extension.
    for query in [
        "2 +", "(1 + 2", "2 + * 3", "1 / 0", "2 ^ 0.5", "2 3", "hello", "42",
    ] {
        search(&launcher, query);
        assert_eq!(titles(&launcher), Vec::<String>::new(), "{query}");
        assert_eq!(launcher.view().selected, None, "{query}");
        assert_eq!(launcher.view().status, status, "{query}");
    }
    // The calculator is a root provider (#164): typing its name finds no
    // row of its own.
    search(&launcher, "calc");
    assert_eq!(titles(&launcher), Vec::<String>::new());
    // Completing the expression answers it.
    search(&launcher, "(1 + 2");
    search(&launcher, "(1 + 2)");
    assert_eq!(titles(&launcher), ["3"]);
}

#[test]
fn deeply_nested_and_very_long_queries_list_no_answer_and_do_not_crash_the_calculator() {
    let dirs = Dirs::new();
    let runtime = dirs.runtime();
    let launcher = dirs.launcher(runtime.clone());
    let status = launcher.view().status;
    search(&launcher, "1 + 1");
    assert_eq!(block_on(runtime.running()).len(), 1);

    // Deeper than 64 parentheses, longer than 256 characters: no answer,
    // which is not a failure.
    let nested = format!("{}1 + 1{}", "(".repeat(65), ")".repeat(65));
    let long_sum = vec!["1"; 200].join(" + ");
    let queries = [
        nested,
        long_sum,
        "(".repeat(100_000),
        format!("{}1 + 1", "-".repeat(100_000)),
        format!("{}1 + 1", "(-".repeat(50_000)),
    ];
    for query in &queries {
        search(&launcher, query);
        let shown = &query[..query.len().min(20)];
        assert_eq!(titles(&launcher), Vec::<String>::new(), "{shown}…");
        assert_eq!(launcher.view().status, status, "{shown}…");
        assert_eq!(
            block_on(runtime.running()).len(),
            1,
            "{shown}…: the calculator's instance is not restarted"
        );
    }

    // Within the limits, nesting and signs are answered as usual.
    for (query, answer) in [
        (format!("{}1 + 1{}", "(".repeat(64), ")".repeat(64)), "2"),
        (format!("{}1 + 1", "-".repeat(200)), "2"),
        (format!("{}1 * 3", "-".repeat(201)), "-3"),
    ] {
        search(&launcher, &query);
        assert_eq!(titles(&launcher), [answer], "{query}");
    }
}

#[test]
fn answers_follow_the_documented_precedence_and_number_format() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher(dirs.runtime());
    let answers = [
        ("2 + 3 * 4", "14"),
        ("(2 + 3) * 4", "20"),
        ("2 ^ 3 ^ 2", "512"),
        ("-2 ^ 2", "-4"),
        ("10 / 4", "2.5"),
        ("7 ÷ 2 − 1", "2.5"),
        ("0.1 + 0.2", "0.3"),
        ("1 / 3", "0.3333333333"),
        (".5 * 2^-1", "0.25"),
        ("2 * -(1 + 1)", "-4"),
        ("10 ^ 15 + 10", "1.00000000000001e15"),
        ("10 ^ 15 + 1", "1e15"),
        ("1 / 10 ^ 7", "1e-7"),
    ];
    for (query, answer) in answers {
        search(&launcher, query);
        assert_eq!(titles(&launcher), [answer], "{query}");
    }
}

#[test]
fn the_answer_is_listed_above_the_results_found_by_title() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher(dirs.runtime());
    let sums = dirs.package(
        "sums",
        &manifest("Sums", "Sum 1 + 1"),
        &built("sample_rust.wasm"),
    );
    install(&launcher, &sums);
    launcher.back();

    search(&launcher, "1 + 1");

    assert_eq!(titles(&launcher), ["2", "Sum 1 + 1"]);
    assert_eq!(selected_title(&launcher).as_deref(), Some("2"));
}

#[test]
fn enter_on_the_answer_reports_the_copy_of_its_text() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher(dirs.runtime());
    search(&launcher, "6 * 7");
    assert_eq!(launcher.selected_copy().as_deref(), Some("42"));

    block_on(launcher.activate_selected());

    let view = launcher.view();
    assert_eq!(
        view.status,
        Status::Result("Copied 42 to the clipboard".into())
    );
    assert_eq!(view.query(), Some("6 * 7"), "root search stays as it was");
    // Other rows copy nothing.
    search(&launcher, "install");
    assert!(!titles(&launcher).is_empty());
    assert_eq!(launcher.selected_copy(), None);
}

/// The answer's actions (#150): Copy, Enter's, and Paste, Ctrl+Enter's,
/// which pastes it into the application in front, closing the window, or
/// copies it with a HUD saying so where Pane cannot paste yet.
#[test]
fn the_answer_offers_copy_then_paste_which_copies_where_it_cannot_paste() {
    let dirs = Dirs::new();
    let system = Arc::new(RecordingSystem::default());
    let launcher = dirs.launcher(dirs.runtime()).with_system(system.clone());
    let window = RecordingWindow::attach(&launcher);
    search(&launcher, "6 * 7");
    let actions: Vec<String> = launcher
        .item_actions()
        .expect("the answer has actions")
        .actions
        .into_iter()
        .map(|action| action.title)
        .collect();
    assert_eq!(actions, ["Copy answer", "Paste answer"]);
    assert_eq!(launcher.selected_action().label, "Copy answer");
    // Copy answer also runs with the platform's copy chord (#251): a
    // focused field's own copy of its selection takes the chord first, so
    // it reaches the answer only with nothing selected in the field.
    let copy_chord = if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    };
    let shortcuts: Vec<Option<String>> = launcher
        .item_actions()
        .expect("the answer has actions")
        .actions
        .into_iter()
        .map(|action| action.shortcut.map(|binding| binding.id()))
        .collect();
    assert_eq!(
        shortcuts,
        [Some(copy_chord.to_owned()), None],
        "the copy chord is bound, paste stays a chord-less action"
    );

    // Not available here yet: it copies the answer and says so.
    block_on(launcher.run_selected_action(1));
    assert_eq!(
        system.take(),
        [Done::Copied {
            clip: pane_core::system::Clip::Text("42".into()),
            concealed: false,
        }]
    );
    let huds: Vec<String> = window.huds().into_iter().map(|hud| hud.title).collect();
    assert_eq!(huds, ["Copied — paste is not available here yet"]);
    window.take();

    // Where Pane can paste, it closes the window and pastes the answer
    // (the clipboard emptied first, so there is nothing to put back).
    system.support_paste();
    system.set_clipboard(None);
    launcher.set_window_presence(pane_core::WindowPresence::Shown);
    block_on(launcher.run_selected_action(1));
    assert_eq!(
        system.take(),
        [
            Done::Copied {
                clip: pane_core::system::Clip::Text("42".into()),
                concealed: true,
            },
            Done::Pasted(Some(pane_core::system::Clip::Text("42".into()))),
        ]
    );
    assert_eq!(window.hides(), 1, "the window closed first");
    assert!(window.huds().is_empty(), "a paste says nothing more");
    assert_eq!(launcher.view().query(), Some("6 * 7"), "root search stays");
}

/// The answer's presentation (#96): what the launcher holds of it — the
/// query it answers, without its surrounding spaces, the text Enter copies
/// and the command that computed it — under that command's title, the
/// title matches after it under "Results" with their own count. A title
/// match is no answer, and a query with no answer presents none: its
/// title matches are root search's results again.
#[test]
fn an_answer_is_presented_with_its_query_under_its_commands_title() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher(dirs.runtime());
    let sums = dirs.package(
        "sums",
        &manifest("Sums", "Sum 1 + 1"),
        &built("sample_rust.wasm"),
    );
    install(&launcher, &sums);
    launcher.back();

    search(&launcher, "1 + 1");
    let (view, presentation) = launcher.presented_view();
    assert_eq!(titles(&launcher), ["2", "Sum 1 + 1"]);
    assert_eq!(
        presentation.rows[0].answer,
        Some(ComputedAnswer {
            query: "1 + 1".into(),
            answer: "2".into(),
            command: "Calculator".into(),
        })
    );
    assert_eq!(presentation.rows[1].answer, None);
    assert_eq!(
        presentation.sections,
        [
            Section {
                label: "Calculator".into(),
                note: None,
                first: 0,
            },
            Section {
                label: "Results".into(),
                note: Some("1 match".into()),
                first: 1,
            },
        ]
    );
    // It keeps its own id and its copy action.
    assert!(view.rows[0].id.ends_with(":answer"), "{}", view.rows[0].id);
    assert_eq!(launcher.selected_copy().as_deref(), Some("2"));

    // The query as typed, without the spaces around it.
    search(&launcher, " 6 * 7 ");
    let answer = launcher.presentation().rows[0].answer.clone();
    assert_eq!(answer.map(|answer| answer.query).as_deref(), Some("6 * 7"));

    // No answer: the title matches alone, under "Results".
    search(&launcher, "1 +");
    let presentation = launcher.presentation();
    assert_eq!(titles(&launcher), ["Sum 1 + 1"]);
    assert_eq!(presentation.rows[0].answer, None);
    assert_eq!(presentation.sections[0].label, "Results");
}

#[test]
fn an_answer_arriving_after_the_query_changed_is_discarded() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher(dirs.runtime());

    let earlier = launcher.set_query("1 + 1");
    search(&launcher, "2 + 2");
    block_on(earlier);

    assert_eq!(titles(&launcher), ["4"]);
    // Until its answer arrives, a new query lists no answer, never an
    // earlier query's.
    let pending = launcher.set_query("3 + 3");
    assert_eq!(titles(&launcher), Vec::<String>::new());
    block_on(pending);
    assert_eq!(titles(&launcher), ["6"]);

    // Nor is one for an earlier search of the same query.
    let earlier = launcher.set_query("1 + 1");
    search(&launcher, "2 + 2");
    let later = launcher.set_query("1 + 1");
    block_on(earlier);
    block_on(later);
    assert_eq!(titles(&launcher), ["2"]);
}

#[test]
fn a_row_the_user_moved_to_stays_selected_when_the_answers_arrive() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher(dirs.runtime());

    let pending = launcher.set_query("install");
    launcher.move_selection(1);
    block_on(pending);

    assert_eq!(
        selected_title(&launcher).as_deref(),
        Some("Install extension from npm…")
    );
}

#[test]
fn the_calculator_starts_only_once_something_is_typed() {
    let dirs = Dirs::new();
    dirs.launcher(dirs.runtime());

    // A restart: nothing runs until the user types.
    let runtime = dirs.runtime();
    let launcher = Launcher::with_packages(
        Ok(runtime.clone()),
        vec![],
        dirs.data.path().join("extensions"),
    );
    assert_eq!(block_on(runtime.running()), Vec::<PathBuf>::new());
    search(&launcher, "   ");
    assert_eq!(block_on(runtime.running()), Vec::<PathBuf>::new());

    search(&launcher, "1 + 1");
    assert_eq!(titles(&launcher), ["2"]);
    assert_eq!(block_on(runtime.running()).len(), 1);
}

#[test]
fn disabling_the_calculator_removes_its_answer_and_leaves_other_results() {
    let dirs = Dirs::new();
    let runtime = dirs.runtime();
    let launcher = dirs.launcher(runtime.clone());
    let sums = dirs.package(
        "sums",
        &manifest("Sums", "Sum 2 * 3"),
        &built("sample_rust.wasm"),
    );
    install(&launcher, &sums);
    launcher.back();
    search(&launcher, "2 * 3");
    assert_eq!(titles(&launcher), ["6", "Sum 2 * 3"]);

    let calculator = PackageIdentity::local(&calculator()).unwrap();
    let disabling = launcher.set_enabled(&calculator, false);
    assert_eq!(
        titles(&launcher),
        ["Sum 2 * 3"],
        "the answer leaves before the choice is recorded"
    );
    block_on(disabling);
    search(&launcher, "2 * 3 ");
    assert_eq!(titles(&launcher), ["Sum 2 * 3"]);
    assert_eq!(block_on(runtime.running()), Vec::<PathBuf>::new());

    block_on(launcher.set_enabled(&calculator, true));
    search(&launcher, "2 * 3");
    assert_eq!(titles(&launcher), ["6", "Sum 2 * 3"]);
}

#[test]
fn a_command_that_fails_to_answer_is_explained_and_other_results_stay() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher(dirs.runtime());
    let faulty = dirs.package(
        "faulty",
        &manifest_computing("Faulty", "Faulty answers"),
        &built("faulty.wasm"),
    );
    install(&launcher, &faulty);
    launcher.back();

    search(&launcher, "error");
    let view = launcher.view();
    assert_eq!(titles(&launcher), ["Faulty answers"]);
    assert_eq!(
        launcher.presentation().rows[0].answer,
        None,
        "a failure is explained, never presented as an answer"
    );
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("Could not answer: The extension reported an error: the guest refused the query")
    );
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Error(
            "Faulty answers could not answer “error”: The extension reported an error: \
             the guest refused the query"
                .into()
        )
    );

    // A crash is explained too, and the next query starts a fresh instance.
    search(&launcher, "trap");
    let subtitle = launcher.view().rows[0].subtitle.clone().unwrap();
    assert!(
        subtitle.starts_with("Could not answer: The extension crashed"),
        "{subtitle}"
    );
    search(&launcher, "1 + 1");
    assert_eq!(titles(&launcher), ["2"]);
    search(&launcher, "faulty");
    assert_eq!(titles(&launcher), ["Faulty answers"]);
}

#[test]
fn the_answer_is_listed_while_a_command_asked_after_it_is_still_answering() {
    let dirs = Dirs::new();
    let runtime = dirs.runtime();
    // The slow fixture computes for about a second, which a loaded
    // machine can stretch past the default 5-second computing limit, and
    // Pane stopping it as unresponsive would answer the query twice
    // ("Slow answers"). Waiting longer is this test's own business; the
    // calculator's answer is not affected.
    runtime.set_limits(Limits {
        compute: Duration::from_secs(180),
        ..Limits::default()
    });
    let launcher = dirs.launcher(runtime);
    // Installed after the calculator, so asked after it.
    let slow = dirs.package(
        "slow",
        &manifest_computing("Slow", "Slow answers"),
        &built("faulty.wasm"),
    );
    install(&launcher, &slow);
    launcher.back();

    // The faulty fixture answers "0 + 0" after about a second of work.
    let pending = launcher.set_query("0 + 0");
    let (done, answered) = mpsc::channel();
    thread::spawn(move || {
        block_on(pending);
        let _ = done.send(());
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    while titles(&launcher).is_empty() {
        assert!(Instant::now() < deadline, "the calculator never answered");
        thread::sleep(Duration::from_millis(1));
    }

    assert_eq!(titles(&launcher), ["0"]);
    assert_eq!(selected_title(&launcher).as_deref(), Some("0"));
    assert!(
        answered.try_recv().is_err(),
        "the slow command is still answering"
    );
    // Generous: alone the slow command answers in seconds, but beside the
    // whole suite its instance and its busy second can take over a minute.
    answered.recv_timeout(Duration::from_secs(240)).unwrap();
    assert_eq!(titles(&launcher), ["0", "Slow answer"]);
    assert_eq!(selected_title(&launcher).as_deref(), Some("0"));
}

#[test]
fn a_command_declaring_root_results_it_does_not_export_is_not_installed() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher(dirs.runtime());
    let folder = dirs.package(
        "pretend",
        &manifest_computing("Pretend", "Pretend answers"),
        &built("sample_settings.wasm"),
    );

    block_on(launcher.install_package(&folder));

    let Status::Error(error) = launcher.view().status else {
        panic!("{:?}", launcher.view().status)
    };
    assert!(
        error.starts_with(
            "\"Pretend answers\": Incompatible extension: it does not implement Pane's \
             extension interface: its manifest says it computes root results, but it does not \
             export pane:extension/root-results@0.1.0"
        ),
        "{error}"
    );
    assert_eq!(launcher.packages().len(), 1, "only the calculator");
}

/// A manifest for a package titled `title` with one command titled
/// `command`.
fn manifest(title: &str, command: &str) -> String {
    format!(
        r#"{{ "manifestVersion": 1, "title": "{title}", "apiVersion": "0.1",
  "commands": [{{ "id": "command", "title": "{command}", "component": "command.wasm" }}] }}"#
    )
}

/// Like [`manifest`], for a command that computes root results.
fn manifest_computing(title: &str, command: &str) -> String {
    format!(
        r#"{{ "manifestVersion": 1, "title": "{title}", "apiVersion": "0.1",
  "commands": [{{ "id": "command", "title": "{command}", "component": "command.wasm",
    "rootResults": true }}] }}"#
    )
}
