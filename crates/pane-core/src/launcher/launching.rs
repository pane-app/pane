//! Launching commands: every way into a command hands it its launch record
//! (see `crate::launch`), and a no-view command runs instead of opening a
//! screen (ADR 0037).
//!
//! The launcher reads a command's mode from its manifest, so it knows at
//! Enter whether to open a screen without running guest code. A view
//! command opens as it always did, its `render` receiving the record (and
//! the same record each time its screen is drawn again while it is open). A
//! no-view command's `run` is called once per launch and no screen opens:
//! root search, or whatever is shown, stays as it is, and the answer is
//! shown in the status line while that screen is still the one shown (from
//! root search, while the query it was launched from is still typed), as a
//! query sent through an alias was. Its global hotkey runs it without
//! showing Pane's window ([`Launcher::hotkey_shows_window`], amending ADR
//! 0016). An error it answers is the extension's operation error and never
//! counts towards pausing; a crash still does.
//!
//! Every launch passes the argument form's step on its way
//! ([`Launcher::launch_with_arguments`], see `argument_form`): a launch the
//! user started that leaves a required argument without a value shows the
//! form instead, and runs once it is submitted. The setup gate of the
//! preferences ticket (#143) comes before it, in
//! [`Launcher::launch_opening`].
//!
//! A command launches another with `pane:extension/commands.launch`
//! ([`Launcher::launch_from_guest`]): one of its own package's by manifest
//! id, or another installed package's by package identity, passing JSON
//! context and asking nothing, since extensions are trusted. A
//! user-initiated launch opens the target as if the user had invoked it; a
//! background launch runs a no-view command without a window and shows
//! nothing, and is refused for a view command and for one with a required
//! argument the launch gives no value. The arguments it passes must be the
//! target's, a dropdown's among its options. The launch starts on a
//! thread of its own, so the caller's call never waits for the target (the
//! target may need the caller's own instance, which the caller holds until
//! it answers).

use std::sync::{Condvar, Mutex, MutexGuard};

use super::{Launcher, Opening, Screen, State, Status, argument_form, owner, stopped};
use crate::arguments;
use crate::extension_data::PackageData;
use crate::launch::{LaunchRecord, LaunchRequest, LaunchSource, LaunchType};
use crate::operations::{MAX_OPERATION_JSON, identity_key};
use crate::packages::{CommandMode, paused_reason};

/// Counts the launches guests asked for that are still running, so tests
/// and development builds can wait for them ([`Launcher::wait_for_launches`]).
#[derive(Default)]
pub(super) struct InFlight {
    running: Mutex<usize>,
    done: Condvar,
}

impl InFlight {
    fn lock(&self) -> MutexGuard<'_, usize> {
        self.running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn begin(&self) {
        *self.lock() += 1;
    }

    fn end(&self) {
        let mut running = self.lock();
        *running = running.saturating_sub(1);
        self.done.notify_all();
    }

    /// Waits until none runs; `false` if one still does after `limit`.
    #[cfg(any(test, debug_assertions))]
    fn settled(&self, limit: std::time::Duration) -> bool {
        let running = self.lock();
        let (running, _) = self
            .done
            .wait_timeout_while(running, limit, |running| *running > 0)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *running == 0
    }
}

impl Launcher {
    /// Launches the command `opening` names, as its launch record says,
    /// every way in: root search, an alias, a fallback, a hotkey, a quick
    /// slot or another command. The setup gate (#143) goes here, before
    /// the argument form's step.
    pub(super) async fn launch_opening(
        &self,
        epoch: u64,
        opening: Opening,
        data: Option<PackageData>,
    ) {
        self.launch_with_arguments(epoch, opening, data).await
    }

    /// The argument form's step of a launch, then the launch: when a
    /// required argument is still without a value, the argument form is
    /// shown and the command runs once it is submitted
    /// ([`Launcher::ask_for_arguments`]); otherwise it launches now with
    /// its arguments filled in. What a setup gate lets through continues
    /// here.
    pub(super) async fn launch_with_arguments(
        &self,
        epoch: u64,
        opening: Opening,
        data: Option<PackageData>,
    ) {
        if let Some(opening) = self.ask_for_arguments(epoch, opening) {
            self.launch_ready(epoch, opening, data).await
        }
    }

    /// Launches `opening` with nothing more to ask: a no-view command runs
    /// ([`Launcher::run_no_view`]), a view command opens its screen
    /// ([`Launcher::open_command`]).
    pub(super) async fn launch_ready(
        &self,
        epoch: u64,
        opening: Opening,
        data: Option<PackageData>,
    ) {
        if opening.no_view {
            self.run_no_view(epoch, opening, data).await
        } else {
            self.open_command(epoch, opening, data).await
        }
    }

    /// Notes, while the launcher is locked, that the no-view command a
    /// launch runs is running: the status line is about it from now on,
    /// and from root search its answer is shown only while the query it
    /// was launched from stays typed.
    pub(super) fn begin_run(state: &mut State) {
        state.sent_from = state.view.query().map(str::to_owned);
        state.view.status = Status::Running;
    }

    /// Runs the no-view command of `opening` (`run`) with its launch
    /// record, and shows its answer while the screen it was launched from
    /// is still shown (from root search, while the query it was launched
    /// from is still typed), which stays as it was. A background launch
    /// shows nothing: no window was shown for it.
    pub(super) async fn run_no_view(
        &self,
        epoch: u64,
        opening: Opening,
        data: Option<PackageData>,
    ) {
        let Opening {
            component,
            command,
            launch,
            ..
        } = opening;
        let background = launch.is_background();
        if let Some(problem) = self.updating(&component) {
            // As opening a command: its package's code is being replaced.
            let mut state = self.lock();
            if state.screen_epoch == epoch && !background {
                state.view.status = Status::Error(problem);
            }
            return;
        }
        let result = match self.runtime() {
            Ok(runtime) => {
                runtime
                    .run_command_with(&component, &command, &launch, data.clone())
                    .await
            }
            Err(error) => Err(error),
        };
        if background {
            return;
        }
        let Some(mut state) = self.lock_if_current(epoch) else {
            return;
        };
        if matches!(state.view.screen, Screen::Root { .. }) {
            let Some(sent) = state.sent_from.clone() else {
                // The query changed meanwhile, which cleared the status.
                return;
            };
            if state.view.query() != Some(sent.as_str()) {
                return;
            }
        }
        state.view.status = match (stopped(&state, &component, &data), result) {
            // Stopped while it was running: its answer is not shown.
            (Some(problem), _) => Status::Error(problem),
            (None, Ok(answer)) => answer.status.map_or(Status::Idle, Status::Result),
            (None, Err(error)) => Status::Error(error.to_string()),
        };
    }

    /// Whether pressing the global hotkey `shortcut` shows Pane's window:
    /// not for a no-view command, which runs without it (ADR 0037), unless
    /// it asks for its arguments first (the argument form). The window
    /// asks before [`Launcher::press_hotkey`].
    pub fn hotkey_shows_window(&self, shortcut: &crate::hotkeys::Shortcut) -> bool {
        let state = self.lock();
        self.hotkey_opening(&state, shortcut)
            .is_none_or(|opening| !opening.no_view || argument_form::asks_first(&state, &opening))
    }

    /// Whether a command a guest launched asked for Pane's window since
    /// this was last asked: a user-initiated launch of a view command
    /// opens its screen, which the user must see, also when a no-view run
    /// started from a hotkey launched it while the window was hidden. The
    /// window asks each time the launcher tells it that it changed.
    pub fn take_window_request(&self) -> bool {
        std::mem::take(&mut self.lock().window_wanted)
    }

    /// Waits until every command a guest launched has run, or opened;
    /// `false` if one has not within `limit`. For tests and development
    /// builds, which so wait for launches without timing them.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn wait_for_launches(&self, limit: std::time::Duration) -> bool {
        let launches = self.lock().launches.clone();
        launches.settled(limit)
    }

    /// Starts the launch a guest asked for (`pane:extension/commands`), or
    /// says why it will not: the target is not installed, has no such
    /// command, is disabled, paused or unavailable on this system, a
    /// background launch names a view command or leaves a required
    /// argument without a value, an argument passed is not the target's (or
    /// a dropdown's value not among its options), or the context is not
    /// JSON within Pane's limit. A user-initiated launch that leaves a
    /// required argument without a value shows the argument form. Called on
    /// the runtime thread, inside the caller's call: it never waits for the
    /// target, which runs on a thread of its own.
    pub(super) fn launch_from_guest(&self, request: LaunchRequest) -> Result<(), String> {
        let mut guard = self.lock();
        let state = &mut *guard;
        let caller = owner(&state.packages, &request.caller);
        let package = match &request.source {
            None => caller.ok_or(
                "only an installed extension's command can launch a command of its own package",
            )?,
            Some(source) => {
                let key = identity_key(source).ok_or_else(|| {
                    format!(
                        "`{source}` is not a package identity; use `local:` followed by the \
                         absolute folder path Pane shows for the package, `npm:` followed by \
                         its npm package name, or `git:` followed by its repository, or name \
                         no package for a command of the caller's own"
                    )
                })?;
                state
                    .packages
                    .iter()
                    .find(|package| package.identity.key() == key)
                    .ok_or_else(|| format!("no installed extension has the source {source}"))?
            }
        };
        let title = package.title();
        if !package.enabled {
            return Err(format!(
                "{title} is disabled; Pane does not enable it to launch its command, enable it \
                 in Manage extensions"
            ));
        }
        if state.paused.is_paused(&package.identity) {
            return Err(paused_reason(&title));
        }
        if let Err(error) = &package.manifest {
            return Err(format!("{title} cannot load: {error}"));
        }
        let command = request.command.as_str();
        let Some((registration, unavailable)) = package
            .available_commands()
            .into_iter()
            .find(|(registration, _)| registration.manifest_id() == command)
        else {
            return Err(format!("{title} has no command `{command}`"));
        };
        if let Some(reason) = unavailable {
            return Err(format!("{} of {title}: {reason}", registration.title));
        }
        let no_view = package.mode_of(command) == CommandMode::NoView;
        if request.launch_type == LaunchType::Background && !no_view {
            return Err(format!(
                "{} of {title} opens a view, so it cannot be launched in the background; \
                 launch it user-initiated",
                registration.title
            ));
        }
        let declared = package.arguments_of(command);
        arguments::check_given(declared, &request.arguments)
            .map_err(|why| format!("{} of {title}: {why}", registration.title))?;
        if request.launch_type == LaunchType::Background
            && let Some(missing) = arguments::first_missing(declared, &request.arguments)
        {
            return Err(format!(
                "{} of {title} needs a value for its argument `{}`, which a background launch \
                 cannot ask for; pass it, or launch it user-initiated",
                registration.title, missing.name
            ));
        }
        if let Some(context) = &request.context {
            check_context(context)?;
        }
        let mut opening = Opening::of(&registration, no_view, LaunchSource::Command);
        opening.launch = LaunchRecord {
            launch_type: request.launch_type,
            source: LaunchSource::Command,
            arguments: request.arguments,
            fallback_text: None,
            context: request.context,
        };
        if request.launch_type == LaunchType::UserInitiated {
            if no_view {
                // As the user invoking it where they are: the screen stays.
                Launcher::begin_run(state);
            } else {
                // As its hotkey opens it: whatever Pane shows makes way,
                // and the window is shown for it.
                self.show_root(state, Some(opening.component.clone()));
                state.view.status = Status::Running;
                state.window_wanted = true;
            }
        }
        // Its data as the package is now, so a disable or reload meanwhile
        // stops the launch.
        let data = self.data_in(state, &opening.component);
        let epoch = state.screen_epoch;
        let launches = state.launches.clone();
        drop(guard);
        launches.begin();
        let launcher = self.clone();
        let ended = launches.clone();
        let started = std::thread::Builder::new()
            .name("pane-launch".into())
            .spawn(move || {
                futures::executor::block_on(launcher.launch_opening(epoch, opening, data));
                launcher.changed();
                ended.end();
            });
        if let Err(error) = started {
            launches.end();
            return Err(format!("Pane could not start the launch: {error}"));
        }
        self.changed();
        Ok(())
    }
}

/// Checks the context a command passes another: JSON text within the
/// limit an operation's input has.
fn check_context(context: &str) -> Result<(), String> {
    if context.len() > MAX_OPERATION_JSON {
        return Err(format!(
            "the context is {} bytes; at most {MAX_OPERATION_JSON} are passed",
            context.len()
        ));
    }
    serde_json::from_str::<serde::de::IgnoredAny>(context)
        .map(|_| ())
        .map_err(|error| format!("the context is not JSON: {error}"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_context_must_be_json_within_the_limit() {
        assert!(check_context(r#"{"from": "launch"}"#).is_ok());
        assert!(check_context("42").is_ok());
        assert!(
            check_context("{not json")
                .unwrap_err()
                .starts_with("the context is not JSON")
        );
        let long = format!("\"{}\"", "x".repeat(MAX_OPERATION_JSON));
        assert!(check_context(&long).unwrap_err().contains("bytes; at most"));
    }

    #[test]
    fn waiting_for_launches_ends_when_none_runs() {
        let launches = InFlight::default();
        assert!(launches.settled(Duration::ZERO));
        launches.begin();
        assert!(!launches.settled(Duration::from_millis(10)));
        launches.end();
        assert!(launches.settled(Duration::ZERO));
    }
}
