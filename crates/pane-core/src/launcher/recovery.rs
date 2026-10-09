//! What the launcher shows when Pane's extension runtime thread crashes
//! (#17) or stops responding (#18), and restarting it.
//!
//! A failure of the shared runtime thread is not attributed to any
//! package: it is a fault in Pane or Wasmtime, not a guest trap or a guest
//! computing for too long (which are its package's own), and whichever
//! extension ran last did not necessarily cause it. So the launcher pauses
//! nothing and names no extension. It says what happened in the status line
//! and in the extension list, where the details and, when Pane did not
//! restart the runtime by itself, **Restart the extension runtime** are.
//! Nothing that was running is run again by itself: the runtime answers
//! every call it held that it stopped, and the launcher never sends one
//! again. Navigation, the extension list and every management action (which
//! run no extension) keep working meanwhile.

use super::{Entry, Launcher, LauncherView, Row, Screen, State, Status};
use crate::runtime::{CRASH_WINDOW, RuntimeStatus};

/// The id of the extension list's row restarting the runtime.
const RESTART_ROW: &str = "pane.runtime.restart";
/// The id of the extension list's row showing why the runtime stopped.
const DETAILS_ROW: &str = "pane.runtime.details";

impl Launcher {
    /// Has the runtime tell this launcher of each crash of its thread.
    pub(super) fn report_runtime_crashes(&self) {
        let Ok(runtime) = &self.runtime else {
            return;
        };
        let launcher = self.downgrade();
        runtime.set_crash_report(std::sync::Arc::new({
            let launcher = self.downgrade();
            move |thread, status| {
                if let Some(launcher) = launcher.upgrade() {
                    launcher.note_runtime_crash(thread, status);
                }
            }
        }));
        runtime.set_slow_report(std::sync::Arc::new(move |_thread, slow| {
            if let Some(launcher) = launcher.upgrade() {
                launcher.note_runtime_slow(slow);
            }
        }));
    }

    /// The runtime thread is not responding yet (`slow`), or carries on
    /// after that: says so in the status line, and puts back what it said
    /// before once the thread carries on. Pane gives up on a thread that
    /// stays stuck, which [`Launcher::note_runtime_crash`] says.
    pub(super) fn note_runtime_slow(&self, slow: bool) {
        let mut state = self.lock();
        let note = Status::Progress(slow_note());
        if slow {
            let before = std::mem::replace(&mut state.view.status, note);
            state.runtime_slow.get_or_insert(before);
        } else if let Some(before) = state.runtime_slow.take()
            && state.view.status == note
        {
            state.view.status = before;
        }
        drop(state);
        self.changed();
    }

    /// Runtime thread number `thread` crashed and the runtime was
    /// restarted or not (`status`): says so, closes the custom view on
    /// screen if that thread held it (one a restarted thread opened since
    /// stays), updates the screens about it, and has the window redraw.
    /// Called on the crashed thread, once the helpers it ran were ended.
    pub(super) fn note_runtime_crash(&self, thread: u64, status: &RuntimeStatus) {
        let mut state = self.lock();
        state.runtime_slow = None;
        let toast = Status::Error(toast(status));
        let held = state
            .custom_view
            .as_ref()
            .is_some_and(|open| open.id.thread() == thread);
        if held {
            // Its guest instance, and the view with it, is gone.
            self.return_from_custom_view(&mut state, toast.clone());
        }
        match &state.view.screen {
            Screen::Extensions { .. } => self.refresh_extensions(&mut state),
            Screen::RuntimeDetails { .. } => self.keep_runtime_details(&mut state),
            _ => {}
        }
        state.view.status = toast;
        drop(state);
        self.changed();
    }

    /// Puts `state` back into a known state after a thread panicked while
    /// holding it (the runtime thread may, while it notes a failure). What
    /// that thread was doing may be half done, so:
    ///
    /// - every claim of a change in progress (enabling, reloading,
    ///   installing, uninstalling, ...) is dropped: the panicked thread's
    ///   would never be released, leaving its package "busy" for good. A
    ///   change still running elsewhere finishes as before; releasing a
    ///   claim that is gone is harmless, but another change to the same
    ///   package is no longer refused meanwhile;
    /// - the pauses listed are made to agree with the packages whose code
    ///   is stopped (see [`Launcher::reconcile_pauses`]);
    /// - root search is shown afresh, built from the installed packages,
    ///   closing any command, form or custom view, with a status line saying
    ///   what happened.
    ///
    /// The packages, their records and extension data are not rebuilt:
    /// they are changed only after the change is written.
    pub(super) fn recover_state(&self, state: &mut State) {
        state.changing.clear();
        self.reconcile_pauses(state);
        self.show_root(state, None);
        state.view.status = Status::Error(
            "Pane recovered from an internal error; what was in progress may not have finished. \
             Saved data is kept."
                .into(),
        );
    }

    /// What the runtime does, when it failed; `None` while it runs as it
    /// started, or had no runtime to begin with.
    fn runtime_status(&self) -> Option<RuntimeStatus> {
        let status = self.runtime.as_ref().ok()?.status();
        (status != RuntimeStatus::Running).then_some(status)
    }

    /// The extension list's first rows after a crash of the runtime:
    /// restarting it, when Pane did not, and why it stopped.
    pub(super) fn runtime_rows(&self) -> Vec<(Row, Entry)> {
        let Some(status) = self.runtime_status() else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        let how = status.failure().map_or("crashing", |failure| failure.how());
        let state = match status {
            RuntimeStatus::Stopped { .. } => {
                rows.push((restart_row(), Entry::RestartRuntime));
                format!("Stopped after {how}")
            }
            _ => format!("Restarted after {how}"),
        };
        let details = Row {
            id: DETAILS_ROW.into(),
            title: "Why the extension runtime stopped".into(),
            subtitle: Some(format!("{state} · The error and its diagnostics")),
            unavailable: None,
        };
        rows.push((details, Entry::RuntimeDetails));
        rows
    }

    /// Shows why the runtime stopped, and what Pane did, with a row that
    /// restarts it if Pane did not. A runtime running as it started (the
    /// user restarted it meanwhile) shows the extension list instead.
    pub(super) fn show_runtime_details(&self, state: &mut State) {
        let Some(status) = self.runtime_status() else {
            self.show_extensions(state);
            return;
        };
        let (why, what) = match &status {
            RuntimeStatus::Restarted { why, .. } => {
                (why, "Pane started it again by itself.".into())
            }
            RuntimeStatus::Stopped {
                why, not_restarted, ..
            } => (
                why,
                format!(
                    "Pane did not start it again: {not_restarted}. Extensions run nothing until \
                     you restart it."
                ),
            ),
            RuntimeStatus::Running => unreachable!("checked above"),
        };
        let limits = self
            .runtime
            .as_ref()
            .map(crate::runtime::Runtime::limits)
            .unwrap_or_default();
        let failure = status.failure().expect("checked above");
        let happened = failure.happened(&limits);
        let mut details = vec![
            happened,
            what,
            "Pane cannot tell which extension, if any, caused it, so none is named or paused."
                .into(),
            "Calls that were running or waiting were stopped and are not run again by \
             themselves. An action may have done its work, such as saving, before its answer \
             was lost; run it again only if you want it done again."
                .into(),
            "The native helpers it ran were ended. Extensions' settings and saved data are kept."
                .into(),
        ];
        if let Some(left) = failure.left_behind() {
            details.push(left.into());
        }
        details.push(format!(
            "Diagnostics (also written to standard error): {why}"
        ));
        let rows = match status {
            RuntimeStatus::Stopped { .. } => vec![restart_row()],
            _ => Vec::new(),
        };
        state.entries = match rows.is_empty() {
            true => Vec::new(),
            false => vec![Entry::RestartRuntime],
        };
        self.leave_command(state);
        state.view = LauncherView::new(
            Screen::RuntimeDetails { details },
            "Why the extension runtime stopped",
        )
        .with_rows(rows);
    }

    /// Shows the runtime details again after they changed, keeping the
    /// screen epoch as refreshing does.
    pub(super) fn keep_runtime_details(&self, state: &mut State) {
        let epoch = state.screen_epoch;
        self.show_runtime_details(state);
        state.screen_epoch = epoch;
    }

    /// Restarts the runtime at the user's request and shows the extension
    /// list, at its first row.
    pub(super) fn restart_runtime(&self, state: &mut State) {
        let restarted = match &self.runtime {
            Ok(runtime) => runtime.restart(),
            Err(error) => Err(error.clone()),
        };
        self.show_extensions(state);
        state.view.status = match restarted {
            Ok(()) => Status::Result("Restarted the extension runtime".into()),
            Err(error) => {
                Status::Error(format!("Could not restart the extension runtime: {error}"))
            }
        };
    }
}

fn restart_row() -> Row {
    Row {
        id: RESTART_ROW.into(),
        title: "Restart the extension runtime".into(),
        subtitle: Some(
            "Start it again; nothing that was running when it stopped is run again".into(),
        ),
        unavailable: None,
    }
}

/// The status line while the runtime thread is not responding yet.
fn slow_note() -> String {
    "Pane's extension runtime is not responding yet. Pane starts it again if it stays stuck; \
     saved data is kept."
        .into()
}

/// The status line when the runtime thread crashed or stopped responding,
/// naming no extension.
fn toast(status: &RuntimeStatus) -> String {
    let stopped = status
        .failure()
        .map_or("stopped unexpectedly", |failure| failure.stopped());
    match status {
        RuntimeStatus::Stopped { .. } => format!(
            "Pane's extension runtime {stopped} again within {} minutes and was not restarted; \
             saved data is kept. Restart it in Settings.",
            CRASH_WINDOW.as_secs() / 60
        ),
        _ => format!(
            "Pane's extension runtime {stopped} and was started again; what was running was \
             stopped and is not run again. Saved data is kept; details are in Settings."
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use futures::executor::block_on;

    use super::*;
    use crate::launcher::{Changing, CommandRegistration};
    use crate::packages::{CommandMatches, CommandWhen, PackageIdentity, Store};
    use crate::runtime::{Runtime, RuntimeFailure};

    /// A launcher whose one command is the Rust sample, which draws a
    /// custom view, with it open on its color view.
    fn with_view_open() -> (Runtime, Launcher) {
        let component =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/sample_rust.wasm");
        assert!(component.exists(), "run `cargo xtask guests`");
        let runtime = Runtime::start().unwrap();
        let command = CommandRegistration {
            id: "rust".into(),
            title: "Rust sample".into(),
            subtitle: None,
            component,
            takes_query: false,
            search: false,
            when: CommandWhen::default(),
            matches: CommandMatches::default(),
        };
        let launcher = Launcher::new(Ok(runtime.clone()), vec![command]);
        block_on(launcher.activate_selected());
        let items = launcher.view().rows;
        let color = items
            .iter()
            .position(|row| row.title == "Choose a color")
            .unwrap();
        launcher.select(color);
        block_on(launcher.activate_selected());
        assert!(matches!(launcher.view().screen, Screen::CustomView(_)));
        (runtime, launcher)
    }

    fn crashed() -> RuntimeStatus {
        RuntimeStatus::Restarted {
            failure: RuntimeFailure::Crashed,
            why: "a test".into(),
        }
    }

    #[test]
    fn a_crash_closes_the_custom_view_of_the_thread_that_crashed() {
        let (_runtime, launcher) = with_view_open();
        let thread = launcher.lock().custom_view.as_ref().unwrap().id.thread();

        launcher.note_runtime_crash(thread, &crashed());

        assert_eq!(launcher.view().screen, Screen::Command);
    }

    #[test]
    fn a_crash_leaves_a_custom_view_another_thread_opened() {
        let (_runtime, launcher) = with_view_open();
        let thread = launcher.lock().custom_view.as_ref().unwrap().id.thread();

        // Reported late, after a restarted thread opened this view.
        launcher.note_runtime_crash(thread - 1, &crashed());

        assert!(matches!(launcher.view().screen, Screen::CustomView(_)));
        assert!(matches!(launcher.view().status, Status::Error(_)));
    }

    #[test]
    fn a_state_taken_over_from_a_panicked_thread_is_made_consistent() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("pane.json"),
            r#"{ "manifestVersion": 1, "title": "Broken", "apiVersion": "0.1",
                "commands": [{ "id": "open", "title": "Open", "component": "c.wasm" }] }"#,
        )
        .unwrap();
        std::fs::write(source.join("c.wasm"), b"").unwrap();
        let packages = dir.path().join("extensions");
        let package = crate::packages::SourcePackage::read(&source).unwrap();
        let identity: PackageIdentity = Store::open(packages.clone())
            .install(&package)
            .unwrap()
            .identity;
        let unavailable = crate::runtime::CallError::RuntimeUnavailable("none".into());
        let launcher = Launcher::with_packages(Err(unavailable), vec![], packages);
        let data = launcher.installation.clone().unwrap().data;

        // A thread panics holding the state, halfway through pausing the
        // package (its code stopped, the pause not listed yet) and with a
        // change claimed that it will never release.
        let panicked = {
            let launcher = launcher.clone();
            let identity = identity.clone();
            std::thread::spawn(move || {
                let mut state = launcher.state.lock().unwrap();
                // Held while it panics.
                state.changing.insert(identity.clone(), Changing::Reloading);
                launcher
                    .installation
                    .as_ref()
                    .unwrap()
                    .data
                    .pause(&identity);
                panic!("halfway");
            })
            .join()
        };
        assert!(panicked.is_err());
        assert!(launcher.state.is_poisoned());

        let state = launcher.lock();

        assert!(!launcher.state.is_poisoned());
        assert!(state.changing.is_empty());
        assert!(state.paused.is_paused(&identity));
        assert_eq!(
            data.owned_by(&identity).stopped(),
            Some(crate::generation::End::Paused)
        );
        assert!(matches!(state.view.screen, Screen::Root { .. }));
        assert!(matches!(&state.view.status, Status::Error(text) if text.contains("recovered")));
    }
}
