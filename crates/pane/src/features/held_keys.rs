//! Keys the window holds while the current query's list is not yet
//! published (#203).
//!
//! Right after typing, the list on screen is still the previous query's
//! while the search runs ([`Launcher::list_published`], #201): a key
//! pressed then would act on rows the user cannot see yet. So the window
//! holds the keys that act on the selection — Enter, Tab and Shift+Tab,
//! Ctrl+K, Ctrl and a digit, the selected row's action chords and
//! shortcuts — and a space while the query could still turn into a
//! command's alias, until the list is published, for at most 300 ms, and
//! then applies them to the selection at that moment. The launcher only
//! reports whether the list is published: holding and replaying are the
//! window's.
//!
//! The held keys are replayed as key presses — each goes through the
//! bindings, the capture handlers and the query field exactly as the
//! pressed key would — in the order they were pressed, so what the user
//! typed meanwhile lands behind them: typing "ec", a space and "hello"
//! while a provider answers leaves the field reading "ec hello". A hold
//! is keyed by the query its first key was pressed under; a query that
//! moves on (Escape cleared it, a completion replaced it) drops the keys
//! rather than running them on another list. An action key held once is
//! held once: the system's repeats of a held Enter run nothing more.

use std::time::Duration;

use gpui::{App, KeyDownEvent, Keystroke, Task, Window, prelude::*};
use pane_core::{KeyboardAction, Screen};

use crate::app::LauncherWindow;

/// How long the window holds a key for the current query's list (#203):
/// after that it is applied to the selection as it is then, whatever the
/// providers asked have answered. Time runs on the clock the window's
/// timers run on, so the tests control it.
pub(crate) const HOLD: Duration = Duration::from_millis(300);

/// The keys the window holds for one query, and how they are replayed
/// (see the module docs). Owned by the launcher window.
#[derive(Default)]
pub(crate) struct HeldKeys {
    /// The held keys and the query they were pressed under; `None` while
    /// nothing is held.
    held: Option<Held>,
    /// Whether the held keys are being replayed now: a replayed press is
    /// never held again, whatever the list's state is then.
    replaying: bool,
}

/// One hold: the keys pressed under `query`, waiting for its list.
struct Held {
    /// The query the first held key was pressed under: a hold applies to
    /// the list for what was typed, and is dropped when the query moves
    /// on.
    query: String,
    /// The keystrokes held, in the order they were pressed.
    keys: Vec<Keystroke>,
    /// Applies the keys once the hold's time is up, whichever end comes
    /// first.
    _deadline: Task<()>,
}

/// The keystroke `action` is bound to now, as the window holds and
/// replays a key pressed under it (#203): the Keyboard page's binding,
/// parsed as the keymap parses it. `None` when the binding is not a
/// single keystroke — never, for the actions the Keyboard page records —
/// and the key then runs unheld.
pub(crate) fn keystroke_of(action: KeyboardAction, cx: &App) -> Option<Keystroke> {
    let id = crate::settings::keyboard_of(cx).binding(action).id();
    Keystroke::parse(&id).ok()
}

impl LauncherWindow {
    /// Holds `keystroke`, pressed while the current query's list is not
    /// yet published: the first key starts the hold, and everything typed
    /// after it is held behind it, in order, so it lands as it was typed.
    /// A key that types no character is never held twice — the system's
    /// repeats of a held Enter run nothing more. Whether the key was
    /// held.
    pub(crate) fn hold_key(
        &mut self,
        keystroke: Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.held.replaying || self.launcher.list_published() {
            return false;
        }
        let Screen::Root { query } = self.launcher.screen() else {
            return false;
        };
        let repeat = keystroke.key_char.is_none()
            && self
                .held
                .held
                .as_ref()
                .is_some_and(|held| held.keys.contains(&keystroke));
        match &mut self.held.held {
            Some(held) => {
                if !repeat {
                    held.keys.push(keystroke);
                }
            }
            None => {
                let deadline = cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(HOLD).await;
                    this.update_in(cx, |this, window, cx| {
                        this.replay_held_keys(window, cx);
                    })
                    .ok();
                });
                self.held.held = Some(Held {
                    query,
                    keys: vec![keystroke],
                    _deadline: deadline,
                });
            }
        }
        true
    }

    /// Applies the held keys now: the current query's list is published,
    /// or the hold's time is up. A hold whose query has moved on — Escape
    /// cleared it, a completion replaced it, root search was left — is
    /// dropped, its keys running nothing.
    pub(crate) fn replay_held_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(held) = self.held.held.take() else {
            return;
        };
        let screen = self.launcher.screen();
        if !matches!(&screen, Screen::Root { query } if query == &held.query) {
            return;
        }
        let keys = held.keys;
        // Dispatching a key can re-render (a press changes what is drawn),
        // so the replay waits for the end of this update, as GPUI's own
        // deferred action dispatch does.
        cx.defer_in(window, move |this, window, cx| {
            this.held.replaying = true;
            for keystroke in keys {
                window.dispatch_keystroke(keystroke, cx);
            }
            this.held.replaying = false;
            cx.notify();
        });
    }

    /// Applies the held keys once the current query's list is published
    /// (#203): wherever the window learns the launcher changed — the
    /// search's own end, a change that arrived in the background.
    pub(crate) fn replay_held_keys_if_published(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.held.held.is_some() && self.launcher.list_published() {
            self.replay_held_keys(window, cx);
        }
    }

    /// A key typed toward root search's query field while the current
    /// query's list is not yet published (#203): a space the query could
    /// still turn into a command's alias is held for the list, and any
    /// character typed while keys are held joins them, so everything
    /// lands in the field in the order it was typed — behind the held
    /// keys, once the list is published or the hold's time ends.
    pub(crate) fn held_typing_keys(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let typed = keystroke.key_char.is_some()
            && !keystroke.modifiers.control
            && !keystroke.modifiers.alt
            && !keystroke.modifiers.platform
            && !keystroke.modifiers.function;
        if !typed
            || !self.query_field().focus_handle(cx).is_focused(window)
            || !matches!(self.launcher.screen(), Screen::Root { .. })
        {
            return;
        }
        // Only a space could turn the query into an alias: any other
        // character is held just to keep the order.
        let alias = keystroke.key == "space" && self.launcher.could_still_be_alias();
        if (self.held.held.is_some() || alias)
            && self.hold_key(keystroke.clone(), window, cx)
        {
            cx.stop_propagation();
        }
    }

    /// Test support: the keys the window holds while the current query's
    /// list is not yet published (#203), as each keystroke is written
    /// ("enter", "space", "ctrl-k"), in the order they were pressed. Test
    /// and debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn held_keys(&self) -> Vec<String> {
        let Some(held) = self.held.held.as_ref() else {
            return Vec::new();
        };
        held.keys.iter().map(|key| key.to_string()).collect()
    }
}
