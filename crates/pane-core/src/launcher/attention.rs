//! Attention launches (ADR 0053, #557): a user-initiated `launch` of one of
//! the calling package's own view commands, asked from a call no window was
//! shown for — a timer's firing, a schedule, a service — so an alarm can
//! put its "Snooze / Dismiss" screen in front of the user. It is Raycast's
//! `launchCommand` with the user-initiated type from a background command.
//!
//! [`Launcher::launch_from_guest`] checks it and queues it here; the window
//! then asks [`Launcher::attend`] while one waits, telling it how long the
//! keyboard and the pointer have been still, and the launcher shows the
//! first waiting launch once nothing holds it back:
//!
//! - **The user typing**, in another application or in the launcher: it
//!   waits for [`STILL`] of no input, at most [`LONGEST_WAIT`], so keys
//!   meant for a document never land in the launcher and Enter never
//!   dismisses an alarm by accident.
//! - **An Alert pending** on the launcher's screen: it waits until the
//!   Alert is answered. Any other screen makes way, as for any
//!   user-initiated launch.
//! - **Game mode holding Pane's hotkeys** (a game in front): it waits until
//!   the game leaves the front, as the hotkeys do.
//! - **Another attention launch shown:** one at a time, in the order asked;
//!   the next waits until the user leaves the screen the shown one opened
//!   (root search again, or the window hidden).
//!
//! Each package holds at most one waiting launch: a newer one takes its
//! older one's place in the queue, so the queue is bounded by the packages
//! installed. A waiting launch whose package's generation ends (disabled,
//! paused, reloaded, updated, uninstalled) is dropped, never shown.
//!
//! Shown, it opens as the command's hotkey opens it — whatever the launcher
//! showed makes way, and Settings stays open — and the window takes the
//! keyboard focus for it ([`Launcher::take_focus_request`]), so Enter and
//! Escape act on the opened view at once. Only an attention launch takes the
//! focus: nothing else a call with no window asks for does.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::{Launcher, Opening, Screen, State, Status};
use crate::extension_data::PackageData;
use crate::feedback::WindowPresence;

/// How long the keyboard and the pointer must have been still before an
/// attention launch is shown.
pub const STILL: Duration = Duration::from_secs(1);

/// The longest an attention launch waits for the user to stop typing.
pub const LONGEST_WAIT: Duration = Duration::from_secs(10);

/// The attention launches waiting, and whether one is shown.
#[derive(Default)]
pub(super) struct Attention {
    /// In the order asked, at most one per package.
    waiting: VecDeque<Waiting>,
    /// Whether the screen the last shown attention launch opened is still
    /// the one the user is on: the next waits until they leave it.
    shown: bool,
    /// Whether the window should take the keyboard focus for the launch
    /// just shown ([`Launcher::take_focus_request`]).
    focus_wanted: bool,
}

/// One attention launch waiting.
struct Waiting {
    /// Its package's identity key.
    package: String,
    opening: Opening,
    /// Its package's data as it was asked: the launch is dropped once that
    /// generation ends.
    data: Option<PackageData>,
    /// When the user's typing first held it back (the launcher's clock, in
    /// milliseconds), for [`LONGEST_WAIT`].
    held_since: Option<u64>,
}

impl Attention {
    /// Queues the launch of `opening` the package with key `package` asked
    /// for: in place of the one it already has waiting, or last.
    pub(super) fn ask(&mut self, package: String, opening: Opening, data: Option<PackageData>) {
        let waiting = Waiting {
            package,
            opening,
            data,
            held_since: None,
        };
        match self
            .waiting
            .iter_mut()
            .find(|older| older.package == waiting.package)
        {
            Some(older) => *older = waiting,
            None => self.waiting.push_back(waiting),
        }
    }

    /// Drops the waiting launches whose package's generation has ended.
    fn drop_ended(&mut self) {
        self.waiting.retain(|waiting| {
            waiting
                .data
                .as_ref()
                .and_then(PackageData::stopped)
                .is_none()
        });
    }

    /// The user left the screen the shown attention launch opened: the
    /// next may be shown.
    pub(super) fn left(&mut self) {
        self.shown = false;
    }
}

/// Whether an Alert is pending on the launcher's screen: a command's
/// confirmation, or Pane's own question, while the window is shown.
fn alert_pending(state: &State) -> bool {
    state.feedback.confirming.is_some()
        || (state.feedback.presence != WindowPresence::Hidden
            && matches!(state.view.screen, Screen::Confirm { .. }))
}

impl Launcher {
    /// Whether an attention launch waits to be shown: the window then asks
    /// [`Launcher::attend`] until none does.
    pub fn attention_waiting(&self) -> bool {
        let mut state = self.lock();
        state.attention.drop_ended();
        !state.attention.waiting.is_empty()
    }

    /// Shows the first waiting attention launch if nothing holds it back,
    /// the keyboard and the pointer having been still for `input_idle` —
    /// `None` where the system cannot tell, which holds nothing back.
    /// Whether one was shown: the window is then asked for
    /// ([`Launcher::take_window_request`]), with the keyboard focus
    /// ([`Launcher::take_focus_request`]).
    pub fn attend(&self, input_idle: Option<Duration>) -> bool {
        let mut guard = self.lock();
        let state = &mut *guard;
        state.attention.drop_ended();
        if state.attention.waiting.is_empty()
            || state.attention.shown
            || state.game.paused
            || alert_pending(state)
        {
            return false;
        }
        let now = state.clock.now();
        let first = state.attention.waiting.front_mut().expect("one waits");
        if input_idle.is_some_and(|idle| idle < STILL) {
            let since = *first.held_since.get_or_insert(now);
            if now.saturating_sub(since) < LONGEST_WAIT.as_millis() as u64 {
                return false;
            }
        }
        let Waiting { opening, data, .. } = state.attention.waiting.pop_front().expect("one waits");
        // As its hotkey opens it: whatever Pane shows makes way, and the
        // window is shown for it, with the focus.
        self.show_root(
            state,
            Some((opening.component.clone(), opening.command.clone())),
        );
        state.view.status = Status::Running {
            since: Instant::now(),
        };
        state.window_wanted = true;
        state.attention.shown = true;
        state.attention.focus_wanted = true;
        let epoch = state.screen_epoch;
        drop(guard);
        if let Err(problem) = self.start_launch(epoch, opening, data, true) {
            let mut state = self.lock();
            state.attention.shown = false;
            state.view.status = Status::Error(problem);
        }
        self.changed();
        true
    }

    /// Whether the window should take the keyboard focus for the attention
    /// launch it was just asked to show, since this was last asked: the
    /// window asks when it shows itself for a launch.
    pub fn take_focus_request(&self) -> bool {
        std::mem::take(&mut self.lock().attention.focus_wanted)
    }

    /// An attention launch's opening has ended: if it opened nothing (it
    /// was refused, or the user left meanwhile), root search is on screen
    /// and the next may be shown.
    pub(super) fn attention_opened(&self) {
        let mut state = self.lock();
        if matches!(state.view.screen, Screen::Root { .. }) {
            state.attention.left();
        }
    }

    /// Test support: launches as `launch` of `pane:extension/commands`
    /// does when the package with `caller` calls it, from a call with a
    /// window or without one (`windowed`): the target `source` (`None` for
    /// the caller's own package) and `command`, of `launch_type`, with
    /// `context`. Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn launch_as(
        &self,
        caller: &crate::PackageIdentity,
        source: Option<&str>,
        command: &str,
        launch_type: crate::LaunchType,
        context: Option<&str>,
        windowed: bool,
    ) -> Result<(), String> {
        let caller = self
            .lock()
            .packages
            .iter()
            .find(|package| package.identity == *caller)
            .map(|package| package.location.clone())
            .ok_or("the caller is not installed")?;
        self.launch_from_guest(crate::launch::LaunchRequest {
            caller,
            source: source.map(str::to_owned),
            command: command.to_owned(),
            launch_type,
            arguments: Vec::new(),
            context: context.map(str::to_owned),
            windowed,
        })
    }
}
