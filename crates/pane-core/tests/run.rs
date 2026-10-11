//! Run (#254, ADR 0040) through the launcher's public interface, with the
//! run samples in Rust, JavaScript and TypeScript (`guests/sample-run`,
//! `guests/sample-run-js`, `guests/sample-run-ts`), which answer the
//! same: a command line typed in root search runs through its alias or
//! as a fallback, an elevated run through Windows' own prompt is declined
//! honestly, and the history the Run dialog and Pane share lists what
//! ran, runs an entry again and deletes from it. The adapter the host
//! functions act on is a fake that answers as scripted and records what
//! ran with the pure half's own record and remove.
//!
//! The decision logic — how a command line splits, how a rooted path is
//! spelled, what the head names and how Explorer's RunMRU format reads
//! and writes — is pure functions tested here on every system, as they
//! are in their modules.
//!
//! On Windows, the real adapter is checked against the real system with
//! state of the tests' own: the RunMRU key it is given (never the user's),
//! the search path it is asked for at each call, App Paths values named
//! after the tests, and a marker-writing `.cmd` program the tests write
//! and run with arguments and unquoted spaced paths. Windows' elevation
//! prompt itself needs a person: it is a manual smoke, as the programs
//! module's is. The real default extension is acquired from an artifact
//! source on 127.0.0.1 and driven with a fake adapter; its package
//! declares `windows` alone, so those tests run on Windows only.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use pane_core::run::{
    Candidates, Completion, Run, RunError, Source, Sources, classify, complete, decode, encode,
    normalize, record, remove, split,
};
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/defaults.rs"]
mod defaults;
#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/repo_server.rs"]
mod repo_server;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::{manage, select_title, titles, to_root};

/// A run sample in one language.
struct Sample {
    /// Its assembled package, under `target/guests/packages`.
    package: &'static str,
    component: &'static str,
    /// Its package and command title; the copies these tests install are
    /// retitled "Run sample", so that every language reads the same.
    title: &'static str,
}

const SAMPLES: [Sample; 3] = [
    Sample {
        package: "sample-run",
        component: "sample_run.wasm",
        title: "Run sample",
    },
    Sample {
        package: "sample-run-js",
        component: "sample_run_js.wasm",
        title: "JavaScript run sample",
    },
    Sample {
        package: "sample-run-ts",
        component: "sample_run_ts.wasm",
        title: "TypeScript run sample",
    },
];

/// What every language's sample is retitled to.
const RETITLED: &str = "Run sample";

/// The packages `cargo xtask guests` assembled.
fn packages() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/packages")
}

/// Copies the assembled sample `sample` into `folder`, retitled
/// [`RETITLED`], and returns `folder`.
fn package(sample: &Sample, folder: &Path) -> PathBuf {
    let assembled = packages().join(sample.package);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    let manifest = fs::read_to_string(assembled.join("pane.json"))
        .unwrap()
        .replace(sample.title, RETITLED);
    fs::write(folder.join("pane.json"), manifest).unwrap();
    fs::copy(
        assembled.join(sample.component),
        folder.join(sample.component),
    )
    .unwrap();
    folder.to_path_buf()
}

/// "The extension reported an error: `message`", as the launcher shows an
/// error a command answered with.
fn error(message: &str) -> Status {
    Status::Error(format!("The extension reported an error: {message}"))
}

/// A marker program named `name` in `folder`, which records the arguments
/// it is given in `<folder>/<name>.txt` — each on a line of its own where
/// the shell splits them, the whole tail where Windows' command
/// interpreter does not: a shell script on the Unix systems, a `.cmd`
/// file on Windows, the file Pane runs either way. Its path, for a test
/// to answer as Windows Terminal.
fn marker_program(folder: &Path, name: &str) -> PathBuf {
    #[cfg(unix)]
    let program = folder.join(name);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(&program, "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$0.txt\"\n").unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(windows)]
    let program = folder.join(format!("{name}.cmd"));
    #[cfg(windows)]
    fs::write(
        &program,
        format!("@echo off\r\necho %*>> \"%~dp0{name}.txt\"\r\n"),
    )
    .unwrap();
    program
}

/// Asserts the marker program `name` in `folder` was given `args`: each
/// on a line of its own where the shell splits them, each somewhere in
/// the recorded tail where Windows' command interpreter does not.
fn asked_for(folder: &Path, name: &str, args: &[&str]) {
    let recorded = fs::read_to_string(folder.join(format!("{name}.txt"))).unwrap_or_default();
    #[cfg(unix)]
    assert_eq!(
        recorded.lines().collect::<Vec<_>>(),
        args,
        "the marker recorded {recorded:?}"
    );
    #[cfg(windows)]
    for arg in args {
        assert!(recorded.contains(arg), "{arg:?} is not in {recorded:?}");
    }
}

/// A fake of the Run dialog's work: it answers as scripted, records what
/// ran in a history of its own with the pure half's [`record`], and
/// remembers what it was asked. An elevated run is declined when told to
/// be, as the user declining Windows' prompt is, and records nothing.
#[derive(Clone, Default)]
struct FakeRun {
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    /// Every run it was asked for, with whether it was elevated.
    asked: Vec<(String, bool)>,
    /// The history, newest first.
    history: Vec<String>,
    /// Whether an elevated run is declined instead of run.
    declines: bool,
    /// The sources its completions are drawn from, as the pure half
    /// matches them.
    candidates: Candidates,
    /// What `terminal` answers: Windows Terminal's `wt.exe`, or none.
    terminal: Option<String>,
}

impl FakeRun {
    /// What it was asked to run, with whether it was elevated.
    fn asked(&self) -> Vec<(String, bool)> {
        self.state.lock().unwrap().asked.clone()
    }

    /// The history, newest first.
    fn history(&self) -> Vec<String> {
        self.state.lock().unwrap().history.clone()
    }

    /// Every elevated run is declined from now on.
    fn decline_elevated(&self) {
        self.state.lock().unwrap().declines = true;
    }

    /// Answers completions drawn from `candidates`, as the pure half
    /// matches them.
    fn complete_from(&self, candidates: Candidates) {
        self.state.lock().unwrap().candidates = candidates;
    }

    /// Answers that Windows Terminal is `wt`, the path it answers.
    fn terminal_at(&self, wt: &Path) {
        self.state.lock().unwrap().terminal = Some(wt.to_string_lossy().into_owned());
    }

    /// Answers that Windows Terminal is not installed.
    fn without_terminal(&self) {
        self.state.lock().unwrap().terminal = None;
    }
}

impl Run for FakeRun {
    fn run(&self, line: &str, elevated: bool) -> Result<(), RunError> {
        let line = line.trim().to_owned();
        let mut state = self.state.lock().unwrap();
        state.asked.push((line.clone(), elevated));
        if elevated && state.declines {
            return Err(RunError::Declined(format!(
                "the user declined to run {line} as an administrator"
            )));
        }
        state.history = record(std::mem::take(&mut state.history), &line);
        Ok(())
    }

    fn history(&self) -> Result<Vec<String>, RunError> {
        Ok(self.history())
    }

    fn delete_from_history(&self, line: &str) -> Result<(), RunError> {
        let mut state = self.state.lock().unwrap();
        match remove(&state.history, line.trim()) {
            Some(without) => {
                state.history = without;
                Ok(())
            }
            None => Err(RunError::Failed(format!(
                "“{}” is not in the Run dialog’s history",
                line.trim()
            ))),
        }
    }

    fn completions(&self, text: &str) -> Result<Vec<Completion>, RunError> {
        let state = self.state.lock().unwrap();
        Ok(complete(&state.candidates, text))
    }

    fn terminal(&self) -> Result<Option<String>, RunError> {
        Ok(self.state.lock().unwrap().terminal.clone())
    }
}

/// A sample installed with the fake adapter, on its own data folder.
struct Installed {
    _sources: TempDir,
    data: TempDir,
    fake: FakeRun,
    launcher: Launcher,
    /// The folder the package was installed from, for its identity.
    folder: PathBuf,
}

impl Installed {
    fn new(sample: &Sample) -> Installed {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let fake = FakeRun::default();
        let launcher =
            Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
                .with_run(Arc::new(fake.clone()));
        let folder = package(sample, &sources.path().join("run"));
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {RETITLED}"))
        );
        Installed {
            _sources: sources,
            data,
            fake,
            launcher,
            folder,
        }
    }

    /// Sets the package's preference kept as `key` to `value`, as its
    /// card in Settings does.
    fn set_preference(&self, key: &str, value: &str) {
        let identity = PackageIdentity::local(&self.folder).unwrap();
        block_on(self.launcher.set_preference(&identity, key, Some(value)))
            .unwrap_or_else(|why| panic!("{key} = {value}: {why}"));
    }

    /// The same launcher started again on the same data folder, with the
    /// same fake adapter.
    fn restart(&self) -> Launcher {
        Launcher::with_packages(
            Runtime::start(),
            vec![],
            self.data.path().join("extensions"),
        )
        .with_run(Arc::new(self.fake.clone()))
    }
}

/// Gives the installed sample's command titled `command` the alias
/// `alias` in Manage extensions, returning to root search.
fn set_alias(launcher: &Launcher, command: &str, alias: &str) {
    manage(launcher);
    select_title(launcher, &format!("Alias for {command}"));
    block_on(launcher.activate_selected());
    let form = launcher.view().form().cloned().expect("the alias form");
    launcher.set_field_value(&form.fields[0].id, alias);
    block_on(launcher.submit_form());
    to_root(launcher);
}

/// Makes the installed sample's command titled `command` a fallback for
/// any text typed in root search.
fn make_fallback(launcher: &Launcher, command: &str) {
    manage(launcher);
    select_title(launcher, &format!("Fallback: {command}"));
    block_on(launcher.activate_selected());
    to_root(launcher);
}

/// Searches for `query` in root search.
fn search(launcher: &Launcher, query: &str) {
    to_root(launcher);
    block_on(launcher.set_query(query));
}

/// Runs the command line `line` through the alias `alias` of the command
/// titled `command`, and answers what the launcher showed.
fn run_through_alias(launcher: &Launcher, command: &str, alias: &str, line: &str) -> Status {
    search(launcher, &format!("{alias} {line}"));
    assert_eq!(titles(launcher), [command], "the alias lists the command");
    block_on(launcher.activate_selected());
    shown(launcher)
}

#[test]
fn a_command_line_splits_as_the_run_dialog_reads_it() {
    /// The ground a parser stands on: the files that exist, and the
    /// environment's variables.
    struct Ground {
        files: Vec<String>,
        variables: Vec<(&'static str, &'static str)>,
    }

    impl Sources for Ground {
        fn exists(&self, path: &str) -> bool {
            self.files.iter().any(|file| file == path)
        }

        fn expand(&self, text: &str) -> String {
            let mut expanded = text.to_owned();
            for (name, value) in &self.variables {
                expanded = expanded
                    .replace(&format!("%{name}%"), value)
                    .replace(&format!("%{}%", name.to_lowercase()), value);
            }
            expanded
        }
    }

    let ground = Ground {
        files: vec![
            r"C:\Program Files\Tool\tool.exe".into(),
            r"C:\My Documents\readme.txt".into(),
        ],
        variables: vec![("PROGRAMFILES", r"C:\Program Files")],
    };
    // A quoted head is taken as written.
    assert_eq!(
        split(r#""C:\Program Files\Tool\tool.exe" -a"#, &ground).unwrap(),
        pane_core::run::Split {
            head: r"C:\Program Files\Tool\tool.exe".into(),
            arguments: "-a".into()
        }
    );
    // A head ending in a program extension splits there, and a rooted
    // path with spaces takes its longest existing prefix: both run
    // without quotes, as `C:\Program Files\Tool\tool.exe -a` does.
    assert_eq!(split("tool.exe -a b", &ground).unwrap().head, "tool.exe");
    assert_eq!(
        split(r"C:\Program Files\Tool\tool.exe -a", &ground)
            .unwrap()
            .arguments,
        "-a"
    );
    assert_eq!(
        split(r"C:\My Documents\readme.txt extra", &ground)
            .unwrap()
            .head,
        r"C:\My Documents\readme.txt"
    );
    // Otherwise the first space splits.
    assert_eq!(
        split("mmc devmgmt.msc", &ground).unwrap(),
        pane_core::run::Split {
            head: "mmc".into(),
            arguments: "devmgmt.msc".into()
        }
    );
    // Environment variables expand in both parts.
    assert_eq!(
        split(
            r"%PROGRAMFILES%\Tool\tool.exe %PROGRAMFILES%\notes",
            &ground
        )
        .unwrap(),
        pane_core::run::Split {
            head: r"C:\Program Files\Tool\tool.exe".into(),
            arguments: r"C:\Program Files\notes".into()
        }
    );
    // A blank line is refused.
    assert!(split("  ", &ground).is_err());
}

#[test]
fn a_rooted_path_is_spelled_as_the_system_spells_it() {
    let real = |path: &str| match path {
        r"c:\my folder\tool.exe" => r"C:\My Folder\Tool.exe".to_owned(),
        other => other.to_owned(),
    };
    assert_eq!(
        normalize(r"c:/my folder/tool.exe", &real),
        r"C:\My Folder\Tool.exe"
    );
    // A path the file system does not hold keeps its casing, and a
    // network path is kept one.
    assert_eq!(
        normalize(r"\\server\share\file.txt", &real),
        r"\\server\share\file.txt"
    );
}

#[test]
fn the_head_names_a_program_path_or_address() {
    use pane_core::run::Target;
    assert_eq!(
        classify("shell:windows"),
        Target::Address {
            address: "shell:windows".into()
        }
    );
    assert_eq!(
        classify(r"C:\Windows\System32\desk.cpl"),
        Target::Applet {
            path: r"C:\Windows\System32\desk.cpl".into()
        }
    );
    assert_eq!(
        classify(r"\\server\share\folder"),
        Target::Path {
            path: r"\\server\share\folder".into()
        }
    );
    assert_eq!(
        classify("notepad"),
        Target::Program {
            name: "notepad".into()
        }
    );
}

#[test]
fn explorers_history_format_decodes_encodes_and_records() {
    // The lettered values in the order MRUList names, Explorer's marker
    // stripped, duplicates removed ignoring case.
    let values = vec![
        ("a".to_owned(), "notepad\u{1}".to_owned()),
        ("b".to_owned(), "cmd.exe\u{1}".to_owned()),
        ("c".to_owned(), "NOTEPAD\u{1}".to_owned()),
    ];
    assert_eq!(decode(&values, "cba"), ["NOTEPAD", "cmd.exe"]);
    // The entries take the letters in order, newest first.
    let (written, list) = encode(&["fresh".to_owned(), "cmd.exe".to_owned()]);
    assert_eq!(list, "ab");
    assert_eq!(
        written,
        vec![
            ("a".to_owned(), "fresh\u{1}".to_owned()),
            ("b".to_owned(), "cmd.exe\u{1}".to_owned()),
        ]
    );
    // Recording puts the line first without its duplicates, and removing
    // takes its case-insensitive matches or nothing.
    let history = vec!["notepad".to_owned(), "cmd.exe".to_owned()];
    assert_eq!(record(history, "CMD.EXE"), ["CMD.EXE", "notepad"]);
    let history = vec!["notepad".to_owned(), "cmd.exe".to_owned()];
    assert_eq!(
        remove(&history, "NOTEPAD"),
        Some(vec!["cmd.exe".to_owned()])
    );
    assert_eq!(remove(&history, "calc"), None);
}

#[test]
fn the_completions_match_the_typed_text_from_every_source() {
    let candidates = Candidates {
        history: vec!["notepad -a".into(), "cmd".into()],
        app_paths: vec!["winget".into()],
        programs: vec!["notepad".into(), "mspaint".into()],
        applets: vec!["desk.cpl".into()],
        consoles: vec!["devmgmt.msc".into()],
        schemes: vec!["ms-settings:".into()],
        variables: vec!["%TEMP%".into()],
    };
    let completion = |line: &str, source: Source| Completion {
        line: line.into(),
        source,
    };
    // The history's entries come first (newest first, as they are given),
    // then each source in its order, each matching the typed text,
    // ignoring case: a history entry matches its head, as typed with
    // arguments, and a program's name is offered without the extension
    // the search path resolves.
    assert_eq!(
        complete(&candidates, "not"),
        [
            completion("notepad -a", Source::History),
            completion("notepad", Source::SearchPath),
        ]
    );
    assert_eq!(
        complete(&candidates, "CM"),
        [completion("cmd", Source::History)]
    );
    assert_eq!(
        complete(&candidates, "win"),
        [completion("winget", Source::AppPath)]
    );
    assert_eq!(
        complete(&candidates, "des"),
        [completion("desk.cpl", Source::Applet)]
    );
    assert_eq!(
        complete(&candidates, "dev"),
        [completion("devmgmt.msc", Source::Console)]
    );
    assert_eq!(
        complete(&candidates, "%te"),
        [completion("%TEMP%", Source::Variable)]
    );
    assert_eq!(
        complete(&candidates, "ms"),
        [
            completion("mspaint", Source::SearchPath),
            completion("ms-settings:", Source::Scheme),
        ]
    );
    // A text no line starts with is answered with nothing, as blank text
    // is: a blank field shows the command's own list.
    assert!(complete(&candidates, "nothing").is_empty());
    assert!(complete(&candidates, "").is_empty());
}

#[test]
fn a_command_line_sent_through_an_alias_runs_and_is_recorded() {
    for sample in SAMPLES {
        let pane = Installed::new(&sample);
        set_alias(&pane.launcher, RETITLED, "r");

        assert_eq!(
            run_through_alias(&pane.launcher, RETITLED, "r", "notepad -a"),
            Status::Result("Ran notepad -a".into()),
            "{}",
            sample.title
        );
        assert_eq!(pane.fake.asked(), [("notepad -a".to_owned(), false)]);
        assert_eq!(pane.fake.history(), ["notepad -a"]);

        // A second run records newest first, without its duplicate.
        run_through_alias(&pane.launcher, RETITLED, "r", "cmd");
        assert_eq!(pane.fake.history(), ["cmd", "notepad -a"]);
    }
}

#[test]
fn a_fallback_sends_the_whole_query_to_run() {
    for sample in SAMPLES {
        let pane = Installed::new(&sample);
        make_fallback(&pane.launcher, RETITLED);

        // Nothing else matches: the fallback is listed, not selected, so
        // Enter sends nothing.
        search(&pane.launcher, "zqx command line");
        assert_eq!(titles(&pane.launcher), [RETITLED]);
        assert_eq!(pane.launcher.view().selected, None);

        // Choosing it sends the whole query.
        pane.launcher.move_selection(1);
        block_on(pane.launcher.activate_selected());
        assert_eq!(
            shown(&pane.launcher),
            Status::Result("Ran zqx command line".into()),
            "{}",
            sample.title
        );
        assert_eq!(pane.fake.history(), ["zqx command line"]);
    }
}

#[test]
fn run_with_no_text_says_how_to_send_it_some() {
    for sample in SAMPLES {
        let pane = Installed::new(&sample);
        search(&pane.launcher, RETITLED);
        select_title(&pane.launcher, RETITLED);
        block_on(pane.launcher.activate_selected());
        assert_eq!(
            shown(&pane.launcher),
            error(
                "Run was sent no command line: give it an alias or make it a fallback in \
                 Settings, then type a command line in root search"
            ),
            "{}",
            sample.title
        );
        assert!(pane.fake.asked().is_empty(), "nothing was asked to run");
    }
}

#[test]
fn an_elevated_run_runs_through_the_prompt_and_a_declined_one_says_why() {
    for sample in SAMPLES {
        let pane = Installed::new(&sample);
        set_alias(&pane.launcher, "Run elevated", "ra");

        // The elevated command sends the line elevated, and records it.
        assert_eq!(
            run_through_alias(&pane.launcher, "Run elevated", "ra", "cmd"),
            Status::Result("Ran cmd as an administrator".into()),
            "{}",
            sample.title
        );
        assert_eq!(
            pane.fake.asked(),
            [("cmd".to_owned(), true)],
            "{}",
            sample.title
        );
        assert_eq!(pane.fake.history(), ["cmd"]);

        // A declined elevation says why and records nothing.
        pane.fake.decline_elevated();
        assert_eq!(
            run_through_alias(&pane.launcher, "Run elevated", "ra", "regedit"),
            error("the user declined to run regedit as an administrator"),
            "{}",
            sample.title
        );
        assert_eq!(pane.fake.history(), ["cmd"], "the decline recorded nothing");
        assert_eq!(pane.fake.asked().len(), 2);
    }
}

#[test]
fn the_history_lists_what_ran_and_runs_an_entry_again() {
    for sample in SAMPLES {
        let pane = Installed::new(&sample);
        set_alias(&pane.launcher, RETITLED, "r");
        run_through_alias(&pane.launcher, RETITLED, "r", "notepad");
        run_through_alias(&pane.launcher, RETITLED, "r", "cmd");

        // The history lists what ran, newest first.
        search(&pane.launcher, "Run history");
        select_title(&pane.launcher, "Run history");
        block_on(pane.launcher.activate_selected());
        assert_eq!(pane.launcher.view().screen, Screen::Command);
        assert_eq!(titles(&pane.launcher), ["cmd", "notepad"]);

        // Enter runs the entry again.
        select_title(&pane.launcher, "cmd");
        block_on(pane.launcher.activate_selected());
        assert_eq!(
            shown(&pane.launcher),
            Status::Result("Ran cmd".into()),
            "{}",
            sample.title
        );
        assert_eq!(
            pane.fake.history(),
            ["cmd", "notepad"],
            "still newest first"
        );
    }
}

#[test]
fn an_empty_history_says_so() {
    for sample in SAMPLES {
        let pane = Installed::new(&sample);
        search(&pane.launcher, "Run history");
        select_title(&pane.launcher, "Run history");
        block_on(pane.launcher.activate_selected());
        assert_eq!(pane.launcher.view().screen, Screen::Command);
        assert_eq!(titles(&pane.launcher), ["Nothing has been run yet"]);
    }
}

#[test]
fn deleting_an_entry_removes_it_from_the_history() {
    for sample in SAMPLES {
        let pane = Installed::new(&sample);
        set_alias(&pane.launcher, RETITLED, "r");
        run_through_alias(&pane.launcher, RETITLED, "r", "notepad");
        run_through_alias(&pane.launcher, RETITLED, "r", "cmd");

        search(&pane.launcher, "Run history");
        select_title(&pane.launcher, "Run history");
        block_on(pane.launcher.activate_selected());
        select_title(&pane.launcher, "notepad");
        // The entry's Delete action, after its primary one.
        block_on(pane.launcher.run_selected_action(1));
        assert_eq!(
            shown(&pane.launcher),
            Status::Result("Deleted from Run's history".into()),
            "{}",
            sample.title
        );
        assert_eq!(pane.fake.history(), ["cmd"], "the entry is gone");

        // The list is drawn again without it.
        assert_eq!(titles(&pane.launcher), ["cmd"]);
    }
}

#[test]
fn completions_appear_while_typing_and_run_what_they_offer() {
    for sample in SAMPLES {
        let pane = Installed::new(&sample);
        pane.fake.complete_from(Candidates {
            history: vec!["notepad -a".into()],
            app_paths: vec!["registered".into()],
            programs: vec!["notepad".into()],
            applets: vec!["desk.cpl".into()],
            consoles: vec!["devmgmt.msc".into()],
            schemes: vec!["ms-settings:".into()],
            variables: vec!["%TEMP%".into()],
        });

        // Opening "Run with Completions" says to type a command line: its
        // search field holds one, and what the field matches replaces the
        // list while it holds text.
        search(&pane.launcher, "Run with completions");
        select_title(&pane.launcher, "Run with Completions");
        block_on(pane.launcher.activate_selected());
        assert_eq!(
            pane.launcher.view().screen,
            Screen::CommandSearch {
                query: String::new()
            }
        );
        assert_eq!(titles(&pane.launcher), ["Type a command line"]);

        // Typing completes from every source, the typed text's own row
        // first.
        block_on(pane.launcher.set_query("note"));
        assert_eq!(
            titles(&pane.launcher),
            ["Run “note”", "notepad -a", "notepad"],
            "{}",
            sample.title
        );
        block_on(pane.launcher.set_query("ms-s"));
        assert_eq!(titles(&pane.launcher), ["Run “ms-s”", "ms-settings:"]);
        block_on(pane.launcher.set_query("%te"));
        assert_eq!(titles(&pane.launcher), ["Run “%te”", "%TEMP%"]);
        // A text nothing completes still offers its own row: Enter runs
        // the text as typed.
        block_on(pane.launcher.set_query("zqx"));
        assert_eq!(titles(&pane.launcher), ["Run “zqx”"]);

        // Enter on a completion runs its line.
        block_on(pane.launcher.set_query("note"));
        select_title(&pane.launcher, "notepad");
        block_on(pane.launcher.activate_selected());
        assert_eq!(
            shown(&pane.launcher),
            Status::Result("Ran notepad".into()),
            "{}",
            sample.title
        );
        assert_eq!(pane.fake.history(), ["notepad"]);

        // Enter on the typed text's own row runs the text as typed.
        block_on(pane.launcher.set_query("not"));
        select_title(&pane.launcher, "Run “not”");
        block_on(pane.launcher.activate_selected());
        assert_eq!(
            shown(&pane.launcher),
            Status::Result("Ran not".into()),
            "{}",
            sample.title
        );
        assert_eq!(
            pane.fake.asked(),
            [("notepad".to_owned(), false), ("not".to_owned(), false)]
        );
        assert_eq!(pane.fake.history(), ["not", "notepad"]);
    }
}

#[test]
fn run_in_terminal_opens_a_new_windows_terminal_tab() {
    for sample in SAMPLES {
        let pane = Installed::new(&sample);
        let bin = tempfile::tempdir().unwrap();
        // The marker program stands for Windows Terminal: what the guest
        // asks the run-program host function to run, it records.
        pane.fake.terminal_at(&marker_program(bin.path(), "wt"));
        set_alias(&pane.launcher, "Run in Terminal", "rt");

        assert_eq!(
            run_through_alias(&pane.launcher, "Run in Terminal", "rt", "ipconfig /all"),
            Status::Result("Ran ipconfig /all in the terminal".into()),
            "{}",
            sample.title
        );
        // Windows Terminal was asked for a new tab running the command
        // line in the shell the preference chooses (its default,
        // PowerShell), through the run-program host function: nothing was
        // asked of the run host function, and a tab records nothing in
        // the Run dialog's history.
        asked_for(
            bin.path(),
            "wt",
            &[
                "-w",
                "0",
                "new-tab",
                "powershell",
                "-NoExit",
                "-Command",
                "ipconfig /all",
            ],
        );
        assert!(pane.fake.asked().is_empty());
        assert!(pane.fake.history().is_empty());
    }
}

#[test]
fn run_in_terminal_without_windows_terminal_opens_the_shells_own_window() {
    for sample in SAMPLES {
        let pane = Installed::new(&sample);
        set_alias(&pane.launcher, "Run in Terminal", "rt");

        assert_eq!(
            run_through_alias(&pane.launcher, "Run in Terminal", "rt", "ipconfig"),
            Status::Result("Ran ipconfig in the terminal".into()),
            "{}",
            sample.title
        );
        // The shell's own window is opened by the run host function's own
        // open (a program the run-program function starts belongs to the
        // call that started it and its streams are Pane's, so its console
        // window would show nothing): the shell runs the command line and
        // stays to be read, and the run records it in the Run dialog's
        // history.
        assert_eq!(
            pane.fake.asked(),
            [("powershell -NoExit -Command ipconfig".to_owned(), false)]
        );
        assert_eq!(
            pane.fake.history(),
            ["powershell -NoExit -Command ipconfig"]
        );
    }
}

#[test]
fn the_shell_preference_chooses_the_terminal_s_shell() {
    for sample in SAMPLES {
        let pane = Installed::new(&sample);
        pane.set_preference("shell", "cmd");
        let bin = tempfile::tempdir().unwrap();
        pane.fake.terminal_at(&marker_program(bin.path(), "wt"));
        set_alias(&pane.launcher, "Run in Terminal", "rt");

        // Windows Terminal's tab runs the Command Prompt.
        run_through_alias(&pane.launcher, "Run in Terminal", "rt", "ipconfig");
        asked_for(
            bin.path(),
            "wt",
            &["-w", "0", "new-tab", "cmd", "/d", "/k", "ipconfig"],
        );

        // Without Windows Terminal, the Command Prompt's own window is
        // opened instead.
        pane.fake.without_terminal();
        run_through_alias(&pane.launcher, "Run in Terminal", "rt", "ipconfig");
        assert_eq!(
            pane.fake.asked().last(),
            Some(&("cmd /d /k ipconfig".to_owned(), false))
        );
    }
}

#[test]
fn a_terminal_run_that_failed_says_why() {
    for sample in SAMPLES {
        let pane = Installed::new(&sample);
        // Windows Terminal is answered as a program that is not there, so
        // the run-program host function's error is shown.
        let nowhere = tempfile::tempdir().unwrap();
        pane.fake
            .terminal_at(&nowhere.path().join("no-such-terminal"));
        set_alias(&pane.launcher, "Run in Terminal", "rt");

        run_through_alias(&pane.launcher, "Run in Terminal", "rt", "ipconfig");
        match shown(&pane.launcher) {
            Status::Error(why) => assert!(why.contains("no program at"), "{why}"),
            other => panic!("expected an error, got {other:?}"),
        }
        assert!(pane.fake.asked().is_empty());
    }
}

#[test]
fn the_run_sample_lists_the_history_after_a_restart() {
    let pane = Installed::new(&SAMPLES[0]);
    set_alias(&pane.launcher, RETITLED, "r");
    run_through_alias(&pane.launcher, RETITLED, "r", "notepad");
    let launcher = pane.restart();
    search(&launcher, "Run history");
    select_title(&launcher, "Run history");
    block_on(launcher.activate_selected());
    assert_eq!(titles(&launcher), ["notepad"]);
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;

    use std::ffi::OsString;
    use std::time::{Duration, Instant};

    use pane_core::run::WindowsRun;
    use pane_core::{DefaultExtension, PackageIdentity, SearchPath};

    use super::defaults;
    use super::feedback::RecordingWindow;
    use super::repo_server;

    /// A number making this test's registry names its own.
    static NAMES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// A registry name of this test run's own, for the RunMRU key and the
    /// App Paths value, never the user's.
    fn own(name: &str) -> String {
        let number = NAMES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        format!("Pane-Run-{name}-{}-{number}", std::process::id())
    }

    /// The RunMRU key this test writes, under `HKEY_CURRENT_USER`, of this
    /// test's own.
    fn own_key() -> String {
        format!(r"Software\Pane\Tests\{}", own("RunMRU"))
    }

    /// The search path of a test: the folder `bin` joined with Pane's own
    /// `PATH`, so the system's shells stay reachable.
    fn search_path(bin: &Path) -> OsString {
        let mut folders = vec![bin.to_path_buf()];
        if let Some(path) = std::env::var_os("PATH") {
            folders.extend(std::env::split_paths(&path));
        }
        std::env::join_paths(folders).unwrap()
    }

    /// A marker-writing program named `<name>.cmd` in `folder`, which
    /// appends the arguments it was given to `folder/marker.txt`, and that
    /// marker file.
    fn marker_program(folder: &Path, name: &str) -> (PathBuf, PathBuf) {
        let program = folder.join(format!("{name}.cmd"));
        fs::write(&program, "@echo off\r\necho %*>> \"%~dp0marker.txt\"\r\n").unwrap();
        (program, folder.join("marker.txt"))
    }

    /// Waits until the marker file holds `expected` and answers what it
    /// holds: the shell starts the program before the program writes.
    fn waited(marker: &Path, expected: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(text) = fs::read_to_string(marker)
                && text.contains(expected)
            {
                return text;
            }
            if Instant::now() > deadline {
                panic!(
                    "{} never wrote {:?} to its marker",
                    marker.display(),
                    expected
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// The Run dialog's work over this test's own key and search path.
    fn adapter(mru: &str, search_path: SearchPath) -> WindowsRun {
        WindowsRun::with(mru.to_owned(), search_path)
    }

    #[test]
    fn explorers_format_is_read_written_and_deleted_in_the_key_it_is_given() {
        let bin = tempfile::tempdir().unwrap();
        let (_, marker) = marker_program(bin.path(), "panemarker");
        let folder = bin.path().to_path_buf();
        let source: SearchPath = Arc::new(move || search_path(&folder));
        let mru = own_key();
        seed(&mru, &[("b", "cmd.exe"), ("a", "notepad")], "ab");

        let run = adapter(&mru, source);
        // Win+R's own history, as Explorer wrote it, is what Pane reads:
        // the lettered values in the order MRUList names, the marker
        // stripped.
        assert_eq!(run.history().unwrap(), ["notepad", "cmd.exe"]);

        // A run records its line, newest first, and the key keeps
        // Explorer's format: lettered values with the marker, and MRUList
        // naming them in order.
        run.run("panemarker -a arguments", false).unwrap();
        waited(&marker, "-a arguments");
        assert_eq!(
            run.history().unwrap(),
            ["panemarker -a arguments", "notepad", "cmd.exe"]
        );
        assert_eq!(
            values_of(&mru),
            vec![
                ("MRUList".to_owned(), "abc".to_owned()),
                ("a".to_owned(), "panemarker -a arguments\u{1}".to_owned()),
                ("b".to_owned(), "notepad\u{1}".to_owned()),
                ("c".to_owned(), "cmd.exe\u{1}".to_owned()),
            ]
        );

        // Deleting an entry rewrites the key without it, matched ignoring
        // case, letter and all.
        run.delete_from_history("NOTEPAD").unwrap();
        assert_eq!(
            run.history().unwrap(),
            ["panemarker -a arguments", "cmd.exe"]
        );
        assert_eq!(
            values_of(&mru),
            vec![
                ("MRUList".to_owned(), "ab".to_owned()),
                ("a".to_owned(), "panemarker -a arguments\u{1}".to_owned()),
                ("b".to_owned(), "cmd.exe\u{1}".to_owned()),
            ]
        );
        // Deleting one that is not there says so, and a key that does not
        // exist is empty: the Run dialog on a fresh Windows.
        assert!(run.delete_from_history("gone").is_err());
        let fresh = adapter(&own_key(), Arc::new(OsString::new));
        assert_eq!(fresh.history().unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_marker_program_runs_with_arguments_and_unquoted_spaced_paths() {
        let bin = tempfile::tempdir().unwrap();
        let (spaced, marker) = marker_program(bin.path(), "pane run marker");
        marker_program(bin.path(), "panemarker");
        let folder = bin.path().to_path_buf();
        let source: SearchPath = Arc::new(move || search_path(&folder));
        let mru = own_key();
        let run = adapter(&mru, source);

        // An unquoted spaced path, resolved by its longest existing
        // prefix, runs with its arguments.
        let line = format!("{} -a spaced argument", spaced.display());
        run.run(&line, false).unwrap();
        assert_eq!(
            waited(&marker, "-a spaced argument").trim(),
            "-a spaced argument"
        );
        assert_eq!(run.history().unwrap(), [line.trim()]);

        // A bare name is resolved on the search path the test gives, run
        // the same way, and recorded as typed.
        run.run("panemarker -a from the path", false).unwrap();
        waited(&marker, "-a from the path");
        assert_eq!(
            run.history().unwrap(),
            ["panemarker -a from the path", line.trim()]
        );

        // A missing program says so, and records nothing.
        let missing = run.run("nothing-by-this-name", false).unwrap_err();
        assert!(
            missing.message().contains("no program"),
            "{}",
            missing.message()
        );
        assert_eq!(run.history().unwrap().len(), 2);
    }

    #[test]
    fn a_program_that_appeared_after_the_first_run_is_found() {
        // The search path is asked for at each call, as the registry's is
        // read each time, so a tool installed after Pane started is found
        // without a restart.
        let bin = tempfile::tempdir().unwrap();
        let (_, marker) = marker_program(bin.path(), "late");
        let on_path = Arc::new(Mutex::new(false));
        let folder = bin.path().to_path_buf();
        let source = {
            let (on_path, folder) = (on_path.clone(), folder);
            Arc::new(move || {
                let mut folders = Vec::new();
                if *on_path.lock().unwrap() {
                    folders.push(folder.clone());
                }
                if let Some(path) = std::env::var_os("PATH") {
                    folders.extend(std::env::split_paths(&path));
                }
                std::env::join_paths(folders).unwrap()
            }) as SearchPath
        };
        let run = adapter(&own_key(), source);

        // Not on the search path yet: not found.
        let missing = run.run("late -a first", false).unwrap_err();
        assert!(
            missing.message().contains("no program"),
            "{}",
            missing.message()
        );

        // The folder joins the search path the next call asks for, as a
        // registry value a new tool's installer wrote would.
        *on_path.lock().unwrap() = true;
        run.run("late -a then", false).unwrap();
        waited(&marker, "-a then");
    }

    #[test]
    fn a_program_registered_in_app_paths_is_found_by_its_name() {
        // App Paths is the Run dialog's other source: an installer
        // registers a program's name there, quoted or not, in the user's
        // software.
        let bin = tempfile::tempdir().unwrap();
        let (program, marker) = marker_program(bin.path(), "registered");
        let name = format!("{}.exe", own("app"));
        register_app_path(&name, &format!("\"{}\"", program.display()));

        let run = adapter(&own_key(), Arc::new(OsString::new));
        run.run(&name, false).unwrap();
        assert!(!waited(&marker, "ECHO").trim().is_empty(), "it ran");

        // The value is this test's own; it is removed again.
        forget_app_path(&name);
    }

    #[test]
    fn the_completions_come_from_every_source_it_is_given() {
        // Every source, with values of this test's own: the RunMRU key, a
        // program App Paths registers, a program, an applet and a console
        // the search path's folder holds, a scheme registered in the
        // classes root, and an environment variable.
        let bin = tempfile::tempdir().unwrap();
        let (program, _) = marker_program(bin.path(), "pane-run-tool");
        fs::write(bin.path().join("pane-run-applet.cpl"), b"").unwrap();
        fs::write(bin.path().join("pane-run-console.msc"), b"").unwrap();
        let folder = bin.path().to_path_buf();
        let source: SearchPath = Arc::new(move || search_path(&folder));
        let mru = own_key();
        seed(&mru, &[("a", "pane-run history")], "a");
        let registered = own("app");
        register_app_path(&format!("{registered}.exe"), &program.display().to_string());
        let scheme = own("scheme");
        register_scheme(&scheme);
        let variable = own("var");
        // SAFETY: this test's own process (one a test), whose environment
        // nothing else reads while it runs.
        unsafe {
            std::env::set_var(&variable, "1");
        }

        let run = adapter(&mru, source);
        let offered = |text: &str| {
            run.completions(text)
                .unwrap()
                .into_iter()
                .map(|completion| (completion.line, completion.source))
                .collect::<Vec<_>>()
        };
        // The history first, then each source in its order, each matching
        // the typed text ignoring case: a program is named without the
        // extension the search path resolves, an applet and a console by
        // their file names, a scheme with the `:` the Run dialog runs it
        // with.
        assert_eq!(
            offered("pane-run"),
            vec![
                ("pane-run history".into(), Source::History),
                (registered.clone(), Source::AppPath),
                ("pane-run-tool".into(), Source::SearchPath),
                ("pane-run-applet.cpl".into(), Source::Applet),
                ("pane-run-console.msc".into(), Source::Console),
                (format!("{scheme}:"), Source::Scheme),
            ]
        );
        // An environment variable's line is its `%NAME%`, so it completes
        // the text once the `%` is typed.
        assert_eq!(
            offered("%pane-run"),
            vec![(format!("%{variable}%"), Source::Variable)]
        );
        // Typing more of the text narrows the same sources, and a text no
        // line starts with is answered with nothing.
        assert_eq!(
            offered("pane-run-tool"),
            vec![("pane-run-tool".into(), Source::SearchPath)]
        );
        assert!(offered("nothing-by-this-name").is_empty());

        // The values are this test's own; they are removed again.
        forget_app_path(&format!("{registered}.exe"));
        forget_scheme(&scheme);
        // SAFETY: as above.
        unsafe {
            std::env::remove_var(&variable);
        }
    }

    #[test]
    fn windows_terminal_is_found_by_its_name() {
        // Windows Terminal is found as the Run dialog finds programs: App
        // Paths, where its installer registers it, then the search path.
        // A machine that has it is answered its own wherever it is; this
        // test puts one on the search path for a machine that has none.
        let bin = tempfile::tempdir().unwrap();
        fs::write(bin.path().join("wt.exe"), b"").unwrap();
        let folder = bin.path().to_path_buf();
        let source: SearchPath = Arc::new(move || search_path(&folder));
        let run = adapter(&own_key(), source);
        let found = run.terminal().unwrap().expect("Windows Terminal was found");
        assert!(found.to_lowercase().ends_with("wt.exe"), "{found}");
        assert!(Path::new(&found).is_file(), "{found}");
    }

    /// Writes `values` (letter names) and `list` to the RunMRU key `mru`
    /// as Explorer writes them: REG_SZ strings, the command lines with
    /// the marker byte.
    fn seed(mru: &str, entries: &[(&str, &str)], list: &str) {
        use ::windows::Win32::Foundation::ERROR_SUCCESS;
        use ::windows::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegSetValueExW,
        };
        use ::windows::core::{HSTRING, PCWSTR};

        let subkey = wide(mru);
        let mut key = HKEY::default();
        // SAFETY: `key` is a handle the call fills in when it succeeds.
        let created = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        };
        assert_eq!(created, ERROR_SUCCESS, "the test's key could not be made");
        let write = |name: &str, data: &str| {
            let name = HSTRING::from(name);
            let mut bytes: Vec<u8> = data.encode_utf16().flat_map(u16::to_le_bytes).collect();
            bytes.extend_from_slice(&[0, 0]);
            // SAFETY: `key` is open for writing, and `bytes` is a plain
            // byte buffer the call copies before returning.
            let result =
                unsafe { RegSetValueExW(key, &name, None, REG_SZ, Some(bytes.as_slice())) };
            assert_eq!(result, ERROR_SUCCESS);
        };
        for (name, data) in entries {
            write(name, &format!("{data}\u{1}"));
        }
        write("MRUList", list);
        // SAFETY: the handle the create returned, closed once.
        unsafe {
            let _ = RegCloseKey(key);
        }
    }

    /// Every value of the RunMRU key `mru`, MRUList and each letter, as
    /// the registry holds them.
    fn values_of(mru: &str) -> Vec<(String, String)> {
        use ::windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
        use ::windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
        use ::windows::core::HSTRING;

        let read = |name: &str| {
            let (subkey, name) = (HSTRING::from(mru), HSTRING::from(name));
            let flags = RRF_RT_REG_SZ;
            let mut bytes: u32 = 0;
            // SAFETY: asks for the size only; every pointer is valid.
            let sized = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    &subkey,
                    &name,
                    flags,
                    None,
                    None,
                    Some(&raw mut bytes),
                )
            };
            if sized == ERROR_FILE_NOT_FOUND {
                return None;
            }
            assert_eq!(sized, ERROR_SUCCESS);
            let mut buffer = vec![0u16; (bytes as usize).div_ceil(2) + 1];
            let mut size = (buffer.len() * 2) as u32;
            // SAFETY: `buffer` holds `size` bytes and outlives the call.
            let read = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    &subkey,
                    &name,
                    flags,
                    None,
                    Some(buffer.as_mut_ptr().cast::<core::ffi::c_void>()),
                    Some(&raw mut size),
                )
            };
            assert_eq!(read, ERROR_SUCCESS);
            let length = buffer
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(buffer.len());
            Some(String::from_utf16_lossy(&buffer[..length]))
        };

        let mut values = Vec::new();
        if let Some(list) = read("MRUList") {
            values.push(("MRUList".to_owned(), list));
        }
        for letter in 'a'..='z' {
            if let Some(data) = read(&letter.to_string()) {
                values.push((letter.to_string(), data));
            }
        }
        values
    }

    /// Registers `program`'s path as App Paths' value for `name`, in the
    /// user's software, as an installer would.
    fn register_app_path(name: &str, program: &str) {
        use ::windows::Win32::Foundation::ERROR_SUCCESS;
        use ::windows::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegSetValueExW,
        };
        use ::windows::core::PCWSTR;

        let subkey = wide(format!(
            r"Software\Microsoft\Windows\CurrentVersion\App Paths\{name}"
        ));
        let mut key = HKEY::default();
        // SAFETY: `key` is a handle the call fills in when it succeeds.
        let created = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        };
        assert_eq!(created, ERROR_SUCCESS, "the App Paths key could be made");
        let mut bytes: Vec<u8> = program.encode_utf16().flat_map(u16::to_le_bytes).collect();
        bytes.extend_from_slice(&[0, 0]);
        // SAFETY: `key` is open for writing, and `bytes` is a plain byte
        // buffer the call copies before returning. The default value has
        // no name: a null one.
        let written =
            unsafe { RegSetValueExW(key, PCWSTR::null(), None, REG_SZ, Some(bytes.as_slice())) };
        assert_eq!(written, ERROR_SUCCESS);
        // SAFETY: the handle the create returned, closed once.
        unsafe {
            let _ = RegCloseKey(key);
        }
    }

    /// Removes the App Paths key `name` names, this test's own.
    fn forget_app_path(name: &str) {
        use ::windows::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};
        use ::windows::core::PCWSTR;

        let subkey = wide(format!(
            r"Software\Microsoft\Windows\CurrentVersion\App Paths\{name}"
        ));
        // SAFETY: a NUL-terminated subkey path that outlives the call,
        // deleting this test's own key.
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(subkey.as_ptr()));
        }
    }

    /// Registers the scheme `name` in the classes root, with the `URL
    /// Protocol` marker a registered scheme carries; this test's own key,
    /// which it removes again.
    fn register_scheme(name: &str) {
        use ::windows::Win32::Foundation::ERROR_SUCCESS;
        use ::windows::Win32::System::Registry::{
            HKEY, HKEY_CLASSES_ROOT, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegSetValueExW,
        };
        use ::windows::core::{HSTRING, PCWSTR};

        let subkey = wide(name);
        let mut key = HKEY::default();
        // SAFETY: `key` is a handle the call fills in when it succeeds,
        // creating this test's own key.
        let created = unsafe {
            RegCreateKeyExW(
                HKEY_CLASSES_ROOT,
                PCWSTR(subkey.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        };
        assert_eq!(
            created, ERROR_SUCCESS,
            "the test's scheme could not be made"
        );
        let marker = HSTRING::from("URL Protocol");
        // An empty string: the value's being there is what marks the
        // scheme.
        let bytes: Vec<u8> = vec![0, 0];
        // SAFETY: `key` is open for writing, and `bytes` is a plain byte
        // buffer the call copies before returning.
        let written = unsafe { RegSetValueExW(key, &marker, None, REG_SZ, Some(bytes.as_slice())) };
        assert_eq!(written, ERROR_SUCCESS);
        // SAFETY: the handle the create returned, closed once.
        unsafe {
            let _ = RegCloseKey(key);
        }
    }

    /// Removes the scheme key `name` from the classes root, this test's
    /// own.
    fn forget_scheme(name: &str) {
        use ::windows::Win32::System::Registry::{HKEY_CLASSES_ROOT, RegDeleteTreeW};
        use ::windows::core::PCWSTR;

        let subkey = wide(name);
        // SAFETY: a NUL-terminated subkey path that outlives the call,
        // deleting this test's own key.
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CLASSES_ROOT, PCWSTR(subkey.as_ptr()));
        }
    }

    /// `text` as Windows takes it, NUL-terminated.
    fn wide(text: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        text.as_ref().encode_wide().chain([0]).collect()
    }

    /// Pane's data location, and the served repository holding Run's
    /// package as a release revision, for one test of the real default
    /// extension: a stand-in for the repository a release pins it to.
    struct Pane {
        data: TempDir,
        _repos: TempDir,
        _server: repo_server::Server,
        pin: DefaultExtension,
        fake: FakeRun,
    }

    impl Pane {
        /// Pane with Run's package served as its repository's release.
        fn new() -> Pane {
            let server = repo_server::Server::start();
            let repos = tempfile::tempdir().unwrap();
            let files = package_files();
            let tag = format!("v{}", defaults::version_of(&files));
            let pin = defaults::pinned(&server, repos.path(), "run", "Run", &tag, &files);
            Pane {
                data: tempfile::tempdir().unwrap(),
                _repos: repos,
                _server: server,
                pin,
                fake: FakeRun::default(),
            }
        }

        /// Pane started with `run` as the run host functions' adapter, Run
        /// acquired as its default extension.
        fn start(&self, run: Arc<dyn Run>) -> Launcher {
            let launcher = Launcher::with_packages(
                Runtime::start(),
                vec![],
                self.data.path().join("extensions"),
            )
            .with_defaults(vec![self.pin.clone()])
            .with_run(run);
            block_on(launcher.acquire_defaults());
            assert!(
                matches!(launcher.view().status, Status::Result(_)),
                "{:?}",
                launcher.view().status
            );
            launcher
        }

        /// Pane started with the fake adapter.
        fn started(&self) -> Launcher {
            self.start(Arc::new(self.fake.clone()))
        }
    }

    /// The files of the assembled Run package.
    fn package_files() -> Vec<(String, Vec<u8>)> {
        let folder = packages().join("run");
        assert!(
            folder.is_dir(),
            "{} is missing; run `cargo xtask guests`",
            folder.display()
        );
        let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file())
            .map(|path| {
                let name = path.file_name().unwrap().to_str().unwrap().to_owned();
                (name, fs::read(&path).unwrap())
            })
            .collect();
        files.sort();
        files
    }

    /// The titles of the Run commands in root search, sorted, as the
    /// package's three commands list for a query that matches them all.
    fn command_titles(launcher: &Launcher) -> Vec<String> {
        let mut titles = titles(launcher);
        titles.sort();
        titles
    }

    #[test]
    fn the_run_default_extension_runs_records_and_deletes_from_the_history() {
        let pane = Pane::new();
        let launcher = pane.started();
        let window = RecordingWindow::attach(&launcher);

        // Acquired as the default extension, with its identity, and
        // enabled by default.
        let identity = PackageIdentity::default_extension("run");
        let installed = launcher
            .packages()
            .into_iter()
            .find(|package| package.identity == identity)
            .expect("Run was installed");
        assert!(installed.enabled, "enabled by default");

        // Its commands are in root search.
        search(&launcher, "Run");
        assert_eq!(
            command_titles(&launcher),
            [
                "Run",
                "Run History",
                "Run as Administrator",
                "Run in Terminal",
                "Run with Completions",
            ]
        );

        // A command line typed in root search runs through an alias and is
        // recorded: the window is asked to show the HUD, closing the
        // launcher as the Run dialog does.
        set_alias(&launcher, "Run", "r");
        run_through_alias(&launcher, "Run", "r", "notepad -a");
        assert_eq!(
            window
                .huds()
                .iter()
                .map(|hud| hud.title.clone())
                .collect::<Vec<_>>(),
            ["Ran notepad -a".to_owned()]
        );
        assert_eq!(pane.fake.asked(), [("notepad -a".to_owned(), false)]);

        // The run's HUD closed the launcher, as the Run dialog's does;
        // the user opens it again for the history.
        launcher.set_window_presence(pane_core::WindowPresence::Shown);

        // The history the fake holds lists through the real extension,
        // and an entry can be deleted from it.
        search(&launcher, "Run history");
        select_title(&launcher, "Run History");
        block_on(launcher.activate_selected());
        assert_eq!(titles(&launcher), ["notepad -a"]);
        select_title(&launcher, "notepad -a");
        // The entry's actions: Run as Administrator, then Delete.
        block_on(launcher.run_selected_action(2));
        assert_eq!(
            shown(&launcher),
            Status::Result("Deleted from Run's history".into())
        );
        assert!(pane.fake.history().is_empty(), "the entry is gone");

        // A declined elevation says why and records nothing.
        pane.fake.decline_elevated();
        set_alias(&launcher, "Run as Administrator", "ra");
        assert_eq!(
            run_through_alias(&launcher, "Run as Administrator", "ra", "regedit"),
            error("the user declined to run regedit as an administrator")
        );
        assert!(pane.fake.history().is_empty());
    }

    #[test]
    fn the_run_default_extension_is_disableable_on_its_own() {
        let pane = Pane::new();
        let launcher = pane.started();

        search(&launcher, "Run");
        assert_eq!(
            command_titles(&launcher),
            [
                "Run",
                "Run History",
                "Run as Administrator",
                "Run in Terminal",
                "Run with Completions",
            ]
        );
        // Disabled on its own: its commands contribute nothing.
        let identity = PackageIdentity::default_extension("run");
        block_on(launcher.set_enabled(&identity, false));
        search(&launcher, "Run");
        assert_eq!(titles(&launcher), Vec::<String>::new());

        // Enabled again, its commands come back.
        block_on(launcher.set_enabled(&identity, true));
        search(&launcher, "Run");
        assert_eq!(
            command_titles(&launcher),
            [
                "Run",
                "Run History",
                "Run as Administrator",
                "Run in Terminal",
                "Run with Completions",
            ]
        );
    }

    #[test]
    fn the_run_default_extension_completes_as_the_user_types() {
        let pane = Pane::new();
        pane.fake.complete_from(Candidates {
            history: vec!["notepad -a".into()],
            app_paths: vec!["registered".into()],
            programs: vec!["notepad".into()],
            applets: vec!["desk.cpl".into()],
            consoles: vec!["devmgmt.msc".into()],
            schemes: vec!["ms-settings:".into()],
            variables: vec!["%TEMP%".into()],
        });
        let launcher = pane.started();
        let window = RecordingWindow::attach(&launcher);

        // Opening "Run with Completions" says to type a command line; the
        // field completes from every source, the typed text's own row
        // first.
        search(&launcher, "Run with completions");
        select_title(&launcher, "Run with Completions");
        block_on(launcher.activate_selected());
        assert_eq!(
            launcher.view().screen,
            Screen::CommandSearch {
                query: String::new()
            }
        );
        assert_eq!(titles(&launcher), ["Type a command line"]);
        block_on(launcher.set_query("note"));
        assert_eq!(titles(&launcher), ["Run “note”", "notepad -a", "notepad"]);

        // Enter on a completion runs its line, the HUD closing the
        // launcher as the Run dialog does.
        select_title(&launcher, "notepad");
        block_on(launcher.activate_selected());
        assert_eq!(
            window
                .huds()
                .iter()
                .map(|hud| hud.title.clone())
                .collect::<Vec<_>>(),
            ["Ran notepad".to_owned()]
        );
        assert_eq!(
            pane.fake.asked(),
            [("notepad".to_owned(), false)],
            "the completion's line ran through the host"
        );
    }

    #[test]
    fn the_run_default_extension_runs_in_the_terminal() {
        let pane = Pane::new();
        let launcher = pane.started();
        let window = RecordingWindow::attach(&launcher);
        // The marker program stands for Windows Terminal: what the
        // extension asks the run-program host function to run, it records.
        let bin = tempfile::tempdir().unwrap();
        let (terminal, marker) = marker_program(bin.path(), "terminal");
        pane.fake.terminal_at(&terminal);
        set_alias(&launcher, "Run in Terminal", "rt");

        run_through_alias(&launcher, "Run in Terminal", "rt", "ipconfig");
        // The HUD says what ran, closing the launcher, and Windows
        // Terminal was asked for a new tab running the command line in
        // the shell the preference chooses (its default, PowerShell):
        // nothing was asked of the run host function, and a tab records
        // nothing in the Run dialog's history.
        assert_eq!(
            window
                .huds()
                .iter()
                .map(|hud| hud.title.clone())
                .collect::<Vec<_>>(),
            ["Ran ipconfig in the terminal".to_owned()]
        );
        let asked = fs::read_to_string(&marker).unwrap_or_default();
        for part in ["new-tab", "powershell", "ipconfig"] {
            assert!(asked.contains(part), "Windows Terminal was asked {asked:?}");
        }
        assert!(pane.fake.asked().is_empty());

        // The preference chooses the Command Prompt.
        let identity = PackageIdentity::default_extension("run");
        block_on(launcher.set_preference(&identity, "shell", Some("cmd"))).unwrap();
        fs::remove_file(&marker).unwrap();
        run_through_alias(&launcher, "Run in Terminal", "rt", "ipconfig");
        let asked = fs::read_to_string(&marker).unwrap_or_default();
        for part in ["new-tab", "cmd", "ipconfig"] {
            assert!(asked.contains(part), "Windows Terminal was asked {asked:?}");
        }
    }

    #[test]
    fn a_marker_program_runs_through_the_run_extension_sharing_the_history() {
        // The real adapter, over a key of this test's own, behind the real
        // default extension: a command line typed in root search runs a
        // marker-writing program with an unquoted spaced path, and what
        // the Run dialog's history then holds lists through the
        // extension, from where it is deleted.
        let bin = tempfile::tempdir().unwrap();
        let (program, marker) = marker_program(bin.path(), "pane run marker");
        let mru = own_key();
        let source: SearchPath = {
            let folder = bin.path().to_path_buf();
            Arc::new(move || search_path(&folder))
        };
        let pane = Pane::new();
        let launcher = pane.start(Arc::new(adapter(&mru, source)));

        make_fallback(&launcher, "Run");
        let line = format!("{} -a through the extension", program.display());
        search(&launcher, &line);
        assert_eq!(titles(&launcher), ["Run"]);
        launcher.move_selection(1);
        block_on(launcher.activate_selected());

        assert_eq!(
            waited(&marker, "-a through the extension").trim(),
            "-a through the extension"
        );
        // The run's HUD closed the launcher; it opens again for the
        // history.
        launcher.set_window_presence(pane_core::WindowPresence::Shown);
        // The Run dialog's history, read through the extension, holds the
        // command line as typed, and deleting it empties the key.
        search(&launcher, "Run history");
        select_title(&launcher, "Run History");
        block_on(launcher.activate_selected());
        assert_eq!(titles(&launcher).len(), 1, "the command line lists");
        launcher.select(0);
        block_on(launcher.run_selected_action(2));
        assert_eq!(
            shown(&launcher),
            Status::Result("Deleted from Run's history".into())
        );
        assert_eq!(values_of(&mru).len(), 1, "only MRUList, empty, remains");
    }
}
