//! Pane's Run sample: a command that runs what Windows' Run dialog (Win+R)
//! runs through `pane_extension::run`, sharing the Run dialog's own
//! history. It answers the same in JavaScript and TypeScript
//! (guests/sample-run-js, guests/sample-run-ts), so the tests drive all
//! three alike.
//!
//! - "Run sample" is a no-view command that takes a query: the command
//!   line it is sent from root search, through its alias or as a
//!   fallback, runs; a toast says what ran.
//! - "Run elevated" runs the command line it is sent through Windows'
//!   own elevation prompt, which the user may decline.
//! - "Run in Terminal" runs the command line it is sent where console
//!   tools stay open to be read: in a new Windows Terminal tab when it is
//!   installed, otherwise in the shell's own window, with the shell the
//!   package's preference chooses.
//! - "Run with Completions" opens a screen whose search field holds the
//!   command line, completed as it is typed from the Run dialog's
//!   history, programs, applets, consoles, schemes and variables; the
//!   first row runs the text as typed, Enter on a completion runs its
//!   line.
//! - "Run history" lists the shared history, newest first: Enter runs an
//!   entry again, and Delete removes it from the history.
#![no_std]

use pane_extension::alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::programs::{self, Options};
use pane_extension::run::{self, CompletionSource, RunError};
use pane_extension::search::SearchResult;
use pane_extension::{
    Action, Command, CustomView, FieldValue, FormError, Item, LaunchRecord, LaunchType, List,
    NoCustomView, commands, preferences,
};

/// The command that runs a command line.
const RUN: &str = "run";
/// The command that runs one as an administrator.
const ELEVATED: &str = "elevated";
/// The command that runs one in a terminal.
const IN_TERMINAL: &str = "in-terminal";
/// The command whose search field holds the command line, completed as
/// it is typed.
const WITH_COMPLETIONS: &str = "with-completions";
/// The command that lists the history.
const HISTORY: &str = "history";
/// The prefix of a history entry's item id, followed by its line.
const ENTRY: &str = "entry:";
/// The prefix of a search result's item id, followed by the line Enter
/// runs: the text as typed, or a completion's line.
const LINE: &str = "line:";

struct RunSample;
pane_extension::export!(RunSample);
pane_extension::search::export!(RunSample);

/// The error's message, as the answer the launcher shows.
fn explain(error: RunError) -> String {
    error.message().to_string()
}

/// The command line `text` was sent, trimmed; the guidance when it was
/// sent none.
fn sent(text: Option<String>) -> Result<String, String> {
    let line = text.unwrap_or_default().trim().to_string();
    if line.is_empty() {
        return Err(format!(
            "Run was sent no command line: give it an alias or make it a fallback in \
             Settings, then type a command line in root search"
        ));
    }
    Ok(line)
}

/// Runs the command line `text` was sent, elevated when `elevated`,
/// showing a toast that says what ran (a launch in the background shows
/// nothing).
async fn run_line(
    text: Option<String>,
    elevated: bool,
    launch_type: LaunchType,
) -> Result<(), String> {
    let line = sent(text)?;
    run::run(&line, elevated).map_err(explain)?;
    if launch_type != LaunchType::Background {
        let title = if elevated {
            format!("Ran {line} as an administrator")
        } else {
            format!("Ran {line}")
        };
        show_toast(Toast::success(title));
    }
    Ok(())
}

/// The shells the terminal preference offers, as each runs a command
/// line: PowerShell, the preference's default, or the Command Prompt.
enum Shell {
    PowerShell,
    CommandPrompt,
}

impl Shell {
    /// The shell the package's preference chooses for the terminal; a
    /// value the preference does not offer is the default.
    fn chosen() -> Result<Shell, String> {
        #[derive(serde::Deserialize)]
        struct Preferences {
            shell: Option<String>,
        }
        let preferences: Preferences = preferences::values()
            .map_err(|why| format!("the terminal shell preference could not be read: {why}"))?;
        Ok(match preferences.shell.as_deref() {
            Some("cmd") => Shell::CommandPrompt,
            _ => Shell::PowerShell,
        })
    }

    /// The program that runs the command line, by the bare name the
    /// search path finds.
    fn program(&self) -> &'static str {
        match self {
            Shell::PowerShell => "powershell",
            Shell::CommandPrompt => "cmd",
        }
    }

    /// The arguments that run `line` and stay to be read: PowerShell
    /// keeps its window after the command runs, and the Command Prompt
    /// runs the line and stays, past the commands AutoRun would add.
    fn arguments(&self, line: &str) -> Vec<String> {
        match self {
            Shell::PowerShell => vec!["-NoExit".into(), "-Command".into(), line.into()],
            Shell::CommandPrompt => vec!["/d".into(), "/k".into(), line.into()],
        }
    }
}

/// Runs the command line `line` in a terminal, showing a toast that says
/// what ran (a launch in the background shows nothing). With Windows
/// Terminal installed, its new tab runs the command line through ADR
/// 0033's run-program host function, the shell the preference chooses:
/// the terminal owns the shell, so what the command wrote shows in the
/// tab and stays while the user reads it (a tab records nothing in the
/// Run dialog's history; it is a terminal, not a run). Without Windows
/// Terminal, the shell's own window is opened by the run host function's
/// own open: a program the run-program function starts belongs to the
/// call that started it (ADR 0033) and its streams are Pane's, so its
/// console window would show nothing and close when the run closed its
/// input — the system open gives the shell a window of its own that
/// stays, and that run records the shell's command line in the Run
/// dialog's history, as the run function does.
async fn run_in_terminal(line: &str, launch_type: LaunchType) -> Result<(), String> {
    let shell = Shell::chosen()?;
    let program = shell.program().to_string();
    let mut arguments = shell.arguments(line);
    match run::terminal().map_err(explain)? {
        // A new tab in the running Windows Terminal (`-w 0` is its most
        // recently used window, a new one when none runs) with the shell
        // the preference chooses running the command line.
        Some(terminal) => {
            let mut asked = vec!["-w".into(), "0".into(), "new-tab".into(), program];
            asked.append(&mut arguments);
            let asked: Vec<&str> = asked.iter().map(String::as_str).collect();
            programs::run(&terminal, &asked, &[], Options::default())
                .await
                .map_err(|error| error.explain())?;
        }
        // The shell's own window, as the system opens it, running the
        // command line and staying to be read.
        None => {
            let asked: Vec<&str> = arguments.iter().map(String::as_str).collect();
            let line = format!("{program} {}", asked.join(" "));
            run::run(&line, false).map_err(explain)?;
        }
    }
    if launch_type != LaunchType::Background {
        show_toast(Toast::success(format!("Ran {line} in the terminal")));
    }
    Ok(())
}

/// Removes `line` from the shared history, saying so.
async fn delete_line(line: String) -> Result<(), String> {
    run::delete_from_history(&line).map_err(explain)?;
    show_toast(Toast::success("Deleted from Run's history"));
    Ok(())
}

/// What a completion's source says, as its row's subtitle.
fn subtitle_of(source: CompletionSource) -> String {
    match source {
        CompletionSource::History => "Run history",
        CompletionSource::AppPath => "Program in App Paths",
        CompletionSource::SearchPath => "Program on the search path",
        CompletionSource::Applet => "Control Panel applet",
        CompletionSource::Console => "Management console",
        CompletionSource::Scheme => "Registered scheme",
        CompletionSource::Variable => "Environment variable",
    }
    .into()
}

impl Command for RunSample {
    type CustomView = NoCustomView;

    /// Runs a no-view command: "Run sample" runs the command line it was
    /// sent, "Run elevated" runs it elevated, "Run in Terminal" runs it
    /// where console tools stay open to be read. "Run with Completions"
    /// and "Run history" open screens, so launching them directly is an
    /// error.
    async fn run(command: String, launch: LaunchRecord) -> Result<(), String> {
        match command.as_str() {
            RUN => run_line(launch.fallback_text, false, launch.launch_type).await,
            ELEVATED => run_line(launch.fallback_text, true, launch.launch_type).await,
            IN_TERMINAL => {
                let line = sent(launch.fallback_text)?;
                run_in_terminal(&line, launch.launch_type).await
            }
            WITH_COMPLETIONS => {
                return Err(format!(
                    "`{WITH_COMPLETIONS}` opens a screen; it has no run entry point"
                ));
            }
            HISTORY => {
                return Err(format!(
                    "`{HISTORY}` opens a screen; it has no run entry point"
                ));
            }
            other => return Err(format!("unknown command: {other}")),
        }
    }

    /// The command's screen: "Run with Completions" says to type a
    /// command line (its search field holds one, completed as it is
    /// typed, which replaces this list while it holds text); "Run
    /// history" lists the shared history, newest first, or says nothing
    /// has been run yet.
    async fn render() -> Result<List, String> {
        if commands::current().command == WITH_COMPLETIONS {
            return Ok(List::new("Run with Completions").item(
                Item::new("hint", "Type a command line")
                    .subtitle("What you ran and installed completes it as you type")
                    .on_action(|| async {
                        show_toast(Toast::success("Type a command line to run it"));
                        Ok(())
                    }),
            ));
        }
        let entries = run::history().map_err(explain)?;
        if entries.is_empty() {
            return Ok(List::new("Run History").item(
                Item::new("empty", "Nothing has been run yet")
                    .subtitle("What you run in Pane or the Run dialog appears here"),
            ));
        }
        let items = entries.iter().map(|line| {
            let (again, gone) = (line.to_string(), line.to_string());
            Item::new(format!("{ENTRY}{line}"), line.clone())
                .subtitle("Enter runs it again")
                .on_action(move || run_line(Some(again), false, LaunchType::UserInitiated))
                .action(Action::new("Delete", move || delete_line(gone)).destructive())
        });
        Ok(List::new("Run History").items(items))
    }

    /// Runs the row whose item was drawn before, which is gone now: a
    /// history entry's, or a completion's line. An id no item names is an
    /// error.
    async fn run_search_result(id: String) -> Result<(), String> {
        let line = id
            .strip_prefix(ENTRY)
            .or_else(|| id.strip_prefix(LINE))
            .ok_or_else(|| format!("unknown item: {id}"))?;
        run_line(Some(line.into()), false, LaunchType::UserInitiated).await
    }

    async fn submit_form(item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: format!("unknown form: {item_id}"),
        })
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        Err(format!("unknown view: {item_id}"))
    }
}

impl pane_extension::search::Guest for RunSample {
    /// The completions for the text typed in "Run with Completions"'
    /// field, with the text's own row first: Enter runs the text as
    /// typed, then each completion's line.
    async fn search(command: String, query: String) -> Result<Vec<SearchResult>, String> {
        if command != WITH_COMPLETIONS {
            return Err(format!("unknown command: {command}"));
        }
        let text = query.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let completions = run::completions(text).map_err(explain)?;
        let mut results = vec![SearchResult {
            id: format!("{LINE}{text}"),
            title: format!("Run “{text}”"),
            subtitle: Some("Enter runs it as typed".into()),
            file: None,
        }];
        results.extend(completions.into_iter().map(|completion| {
            let title = completion.line;
            SearchResult {
                id: format!("{LINE}{}", title),
                title,
                subtitle: Some(subtitle_of(completion.source)),
                file: None,
            }
        }));
        Ok(results)
    }
}
