//! The error overlay (#214, ADR 0047): what Pane shows over the launcher's
//! current screen when a developed package's command crashes, traps or
//! fails to start — the message and the stack trace, with rows that open
//! the package's Logs screen, copy the message and trace, and run the
//! command again.
//!
//! Only a package Pane is developing gets the overlay: an author mid-work
//! needs the failure where it happened, not in a status line a reload's
//! report or a toast replaces; a package not being developed keeps today's
//! presentation (the status line, the failure toast and the pause flow)
//! exactly, as does every failure that is not the command's own — one Pane
//! itself caused, an unresponsive call, a tree Pane cannot read, or a
//! launch stopped by a disable, a reload or an update.
//!
//! The overlay is the screen [`Screen::Crash`] over what the launcher was
//! showing: nothing underneath is touched, and [`State::error_overlay`]
//! holds the covered view and its rows, which Back puts back. Retry
//! launches the command again as its row in root search does — a view
//! command opens again, a no-view one runs again — so what ran the old
//! code runs the new: the author fixes the crash, saves (the development
//! build reloads the package, which ends the overlay), or retries.
//!
//! What the trace holds: a trap's text is wasmtime's alternate Display,
//! which carries the wasm backtrace on its later lines; an error a
//! JavaScript or TypeScript command answered with carries only its message,
//! and its stack reaches the extension log through the adapter
//! (`logThrown`), which logs what the handler threw with its stack — the
//! overlay takes that block, already mapped to the author's sources (the
//! build keeps a source map beside the component, and the runtime maps the
//! lines as it captures them; see `crate::source_map`).

use std::path::Path;

use super::{
    Entry, Launcher, LauncherView, Opening, Pending, Row, Screen, State, Status, owner, reload,
};
use crate::extension_log::{ExtensionLogs, LogLevel, LogSource, LogStream};
use crate::packages::{CommandMode, PackageIdentity};
use crate::runtime::CallError;

/// What the overlay's Retry row does.
enum Retry {
    /// Launch the command again — open its screen, or run it if it opens
    /// none — as its row in root search does.
    Command(Opening),
    /// Start again the package whose reloaded code could not start, as the
    /// pause's Retry row does ([`Launcher::retry_start`]).
    Start(PackageIdentity),
}

/// The error overlay on show: what its Retry row does, and the view it
/// covers with that view's rows' entries, put back when it leaves. Only
/// reachable while the screen is [`Screen::Crash`], which nothing else
/// ever sets, so a snapshot another flow stranded is never restored over
/// the screen that replaced it.
pub(super) struct Shown {
    retry: Retry,
    /// The view the overlay covers.
    return_to: LauncherView,
    /// What activating each row of the covered view did.
    entries: Vec<Entry>,
}

impl Launcher {
    /// Shows the error overlay for how the command of the developed
    /// package in `component` failed with `error`, over what the launcher
    /// is showing: the message and the stack trace, with Open Logs, Copy
    /// and Retry. `retry` is the command to launch again. Whether it was
    /// shown: only a trap (a crash) or an error the command answered with
    /// — the failures that are the command's own — of a package Pane is
    /// developing show as the overlay; anything else answers as it always
    /// did (#214).
    pub(super) fn show_error_overlay(
        &self,
        state: &mut State,
        component: &Path,
        error: &CallError,
        retry: Opening,
    ) -> bool {
        if !matches!(error, CallError::Trap(_) | CallError::Guest(_)) {
            return false;
        }
        let Some(package) = owner(&state.packages, component) else {
            return false;
        };
        if !self.is_developed(&package.identity) {
            return false;
        }
        let identity = package.identity.clone();
        let title = package.title();
        let (message, trace) = message_of(error);
        // What the command threw was logged with its stack, mapped to its
        // sources: that block is the overlay's trace.
        let thrown = match error {
            CallError::Guest(text) => thrown_trace(&self.developing.logs, &identity, text),
            _ => trace,
        };
        let command = package
            .commands()
            .into_iter()
            .find(|command| command.manifest_id() == retry.command)
            .map(|command| command.title)
            .unwrap_or_else(|| title.clone());
        let (return_to, entries) = covered(state);
        self.show(
            state,
            &identity,
            match error {
                CallError::Guest(_) => format!("{title} failed"),
                _ => format!("{title} crashed"),
            },
            details(message, thrown),
            Shown {
                retry: Retry::Command(retry),
                return_to,
                entries,
            },
            format!("Run {command} again"),
        );
        true
    }

    /// Shows the error overlay for how the developed package with
    /// `identity` failed to start after a reload replaced its code: the
    /// message and the trap's backtrace, when it trapped, with a row that
    /// starts it again. The pause Pane recorded stays as it is, with its
    /// details and its Retry row in Settings; this is the overlay over
    /// what the launcher is showing, so the author sees the failure where
    /// the command they had open closed for the reload.
    pub(super) fn show_failed_start_overlay(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
        error: &CallError,
    ) {
        if !self.is_developed(identity) {
            return;
        }
        let title = state.title_of(identity);
        let (message, trace) = message_of(error);
        let (return_to, entries) = covered(state);
        self.show(
            state,
            identity,
            format!("{title} failed to start"),
            details(message, trace),
            Shown {
                retry: Retry::Start(identity.clone()),
                return_to,
                entries,
            },
            format!("Retry starting {title}"),
        );
    }

    /// Shows the overlay: `title`, `details` (the message and the trace)
    /// and its rows over the view on display, which `shown` puts back when
    /// it leaves.
    fn show(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
        title: String,
        details: Vec<String>,
        shown: Shown,
        again: String,
    ) {
        state.error_overlay = Some(shown);
        // The epoch moves, so replies for the covered screen are not shown
        // over the overlay.
        state.next_screen();
        let logs = Row {
            id: format!("crash-logs:{}", identity.key()),
            title: format!("Logs for {}", state.title_of(identity)),
            subtitle: Some("What it writes, and Pane's messages about it".into()),
            unavailable: None,
        };
        let copy = Row {
            id: "crash-copy".into(),
            title: "Copy the message and trace".into(),
            subtitle: Some("The message and the stack trace, as this screen shows them".into()),
            unavailable: None,
        };
        let retry = Row {
            id: format!("crash-retry:{}", identity.key()),
            title: again,
            subtitle: Some("Start it again".into()),
            unavailable: None,
        };
        state.entries = vec![Entry::CrashLogs(identity.clone()), Entry::CrashCopy, Entry::CrashRetry];
        let screen = Screen::Crash {
            identity: identity.clone(),
            details,
        };
        state.view = LauncherView::new(screen, title).with_rows(vec![logs, copy, retry]);
    }

    /// Puts back what the error overlay covered, as Back leaves it: the
    /// view with its rows. The command underneath was never closed — only
    /// its calls failed — so it is where it was, and a retry launches it
    /// again.
    pub(super) fn leave_error_overlay(&self, state: &mut State) {
        let Some(shown) = state.error_overlay.take() else {
            return;
        };
        state.next_screen();
        let Shown { return_to, entries, .. } = shown;
        state.view = return_to;
        state.entries = entries;
    }

    /// The error overlay's row that shows the package's Logs screen: the
    /// overlay goes, and the command it covered is left for the log.
    pub(super) fn open_crash_logs(&self, state: &mut State, identity: &PackageIdentity) {
        state.error_overlay = None;
        self.show_extension_log(state, identity);
    }

    /// The error overlay's Retry row: launches the command again as its
    /// row in root search does, or starts the package again as the pause's
    /// Retry row does. The work the row's activation returns.
    pub(super) fn retry_crash(&self, state: &mut State) -> Pending {
        let Some(shown) = state.error_overlay.take() else {
            return Pending::Nothing;
        };
        match shown.retry {
            Retry::Command(opening) => {
                if opening.no_view {
                    Launcher::begin_run(state);
                } else {
                    state.view.status = Status::Running;
                }
                Pending::Open(opening)
            }
            Retry::Start(identity) => self
                .begin_reload(state, identity, reload::Attempt::Retry)
                .map_or(Pending::Nothing, Pending::Reload),
        }
    }

    /// The command whose view is open on `component`, launched again as
    /// its row in root search does — with the record its screen was opened
    /// with — for the overlay's Retry row. `None` when no view of it is
    /// open; the caller of a no-view run, or of one that never opened,
    /// builds the opening itself.
    pub(super) fn open_again(state: &State, component: &Path) -> Option<Opening> {
        if state.open.as_deref() != Some(component) {
            return None;
        }
        let command = state.open_command.clone()?;
        let package = owner(&state.packages, component)?;
        let registration = package
            .commands()
            .into_iter()
            .find(|found| found.manifest_id() == command)?;
        let no_view = package.mode_of(&command) == CommandMode::NoView;
        Some(Opening {
            launch: state.launch.clone(),
            ..Opening::of(&registration, no_view, state.launch.source.clone())
        })
    }
}

/// The view the overlay covers, with its rows' entries, as the launcher
/// shows it now. A status line that said something was running reads idle
/// when the view comes back: whatever ran, ran to its end, and its answer
/// is the overlay's.
fn covered(state: &State) -> (LauncherView, Vec<Entry>) {
    let mut view = state.view.clone();
    if view.status == Status::Running {
        view.status = Status::Idle;
    }
    (view, state.entries.clone())
}

/// The message of the overlay, and the stack trace the error carries, when
/// it carries one: a trap's text holds the wasm backtrace after its first
/// line.
fn message_of(error: &CallError) -> (String, Option<String>) {
    match error {
        CallError::Trap(reason) => {
            let (first, rest) = reason.split_once('\n').unwrap_or((reason, ""));
            let trace = (!rest.trim().is_empty()).then(|| rest.trim_end().to_owned());
            (format!("The extension crashed: {first}"), trace)
        }
        error => (error.to_string(), None),
    }
}

/// The overlay's lines of information: the message, then the trace's lines.
fn details(message: String, trace: Option<String>) -> Vec<String> {
    let mut details = vec![message];
    if let Some(trace) = trace {
        details.extend(
            trace
                .lines()
                .map(str::trim_end)
                .filter(|line| !line.trim().is_empty())
                .map(str::to_owned),
        );
    }
    details
}

/// The stack a JavaScript or TypeScript command's handler threw, as its
/// handler logged it with the error (the adapter's `logThrown`): the
/// trailing lines the package wrote to its standard error at error level,
/// which are the error and its stack, mapped to the author's sources as
/// they were captured. `None` when the throw left no such block — thrown
/// without a stack, or not logged.
fn thrown_trace(
    logs: &ExtensionLogs,
    identity: &PackageIdentity,
    message: &str,
) -> Option<String> {
    let lines = logs.lines(&identity.key());
    let mut block: Vec<&str> = Vec::new();
    for line in lines.iter().rev() {
        if matches!(
            (line.source, line.level),
            (LogSource::Extension(LogStream::Stderr), LogLevel::Error)
        ) {
            block.push(line.text.as_str());
        } else {
            break;
        }
    }
    block.reverse();
    // The block's first line restates the error the command answered with;
    // one that does not is not this throw's.
    let first = block.first()?;
    if !first.contains(message) {
        return None;
    }
    let stack: Vec<&str> = block[1..].iter().copied().collect();
    (!stack.is_empty()).then(|| stack.join("\n"))
}
