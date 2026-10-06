//! The toast in the launcher's footer (#141, ADR 0037): what a command
//! says happened, drawn where the status line is, one at a time (the
//! launcher keeps it, see `pane_core::feedback`).
//!
//! The footer shows the launcher's toast while its own status line is idle
//! or running (a message of Pane's own, such as an install's outcome,
//! speaks over it): a dot in the toast's style (work in progress, success,
//! failure), its title and its message, and its actions as the footer's
//! buttons, each with the keys of its shortcut. A success or failure toast
//! leaves after `TOAST_DURATION` (3 seconds, ADR 0035), counted only while
//! the pointer is not over it and neither of its actions has the focus; an
//! animated one stays until its command updates or hides it, or the window
//! deactivates.
//!
//! **Keyboard.** The toast key (Ctrl+T, Command+T on macOS,
//! `pane_core::keyboard::toast_key`) moves the focus to the toast's first
//! action; Tab moves on to the second, Enter or Space chooses the focused
//! one and Escape gives the focus back to where it was. An action's own
//! shortcut chooses it while the toast shows, without moving the focus.
//! Choosing an action calls its command back, as an item's action does
//! (`Launcher::run_toast_action`); Pane's own "Copy Error" copies the
//! error first.
//!
//! How the toast looks is refined by "Launcher polish" (#123); its debug
//! selectors (`toast`, `toast-<style>`, `toast-title`, `toast-message`,
//! `toast-action-primary`, `toast-action-secondary`) are what tests find.

use std::time::Duration;

use gpui::{
    AnyElement, App, BoxShadow, ClickEvent, ClipboardItem, Context, Div, FocusHandle, KeyBinding,
    KeyDownEvent, Role, Stateful, Task, Window, actions, div, prelude::*, px,
};
use pane_core::feedback::TOAST_DURATION;
use pane_core::{ShownToast, Status, ToastSlot, ToastStyle};

use crate::app::{KEY_CONTEXT, LauncherWindow};
use crate::ui::footer::{self, ButtonWash};
use crate::ui::keycap::{CapStyle, KeySequence};
use crate::ui::theme::Theme;

actions!(toast, [FocusToast, PressToastAction, LeaveToast]);

/// A toast action's button's own context, so Enter and Space choose it
/// rather than run the launcher's confirm, and Escape leaves it.
const BUTTON_CONTEXT: &str = "ToastButton";

/// The dot that says a toast's style, in px.
const DOT: f32 = 8.;

/// Registers the toast key in the launcher, and a toast action's button's
/// activation keys while it is focused.
pub(crate) fn bind_keys(cx: &mut App) {
    let toast_key = pane_core::keyboard::toast_key().id();
    cx.bind_keys([
        KeyBinding::new(&toast_key, FocusToast, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", PressToastAction, Some(BUTTON_CONTEXT)),
        KeyBinding::new("space", PressToastAction, Some(BUTTON_CONTEXT)),
        KeyBinding::new("escape", LeaveToast, Some(BUTTON_CONTEXT)),
    ]);
}

/// The footer toast's controls: its actions' focus, and its time.
pub(crate) struct ToastControls {
    /// The primary action's button's focus: a tab stop while it is drawn.
    primary: FocusHandle,
    /// The secondary action's button's focus.
    secondary: FocusHandle,
    /// What had the focus when the toast key moved it to the toast,
    /// given back when the toast's action is chosen or Escape leaves it.
    restore: Option<FocusHandle>,
    /// Whether the pointer is over the toast.
    hovered: bool,
    /// The time left to the toast shown, when it hides by itself.
    countdown: Option<Countdown>,
}

/// The time left to one revision of a toast.
struct Countdown {
    id: u64,
    revision: u64,
    /// What is left of [`TOAST_DURATION`], as of `since`.
    left: Duration,
    /// When the time last started running again; `None` while it is
    /// paused (the pointer over the toast, or an action focused).
    since: Option<std::time::Instant>,
    /// Ends the toast when the time is up; dropped while paused.
    _due: Option<Task<()>>,
}

impl ToastControls {
    pub(crate) fn new(cx: &mut App) -> ToastControls {
        ToastControls {
            primary: cx.focus_handle().tab_stop(true),
            secondary: cx.focus_handle().tab_stop(true),
            restore: None,
            hovered: false,
            countdown: None,
        }
    }

    /// The focus of the button of the action in `slot`.
    fn focus(&self, slot: ToastSlot) -> &FocusHandle {
        match slot {
            ToastSlot::Primary => &self.primary,
            ToastSlot::Secondary => &self.secondary,
        }
    }

    /// Whether one of the toast's actions has the focus.
    fn focused(&self, window: &Window) -> bool {
        self.primary.is_focused(window) || self.secondary.is_focused(window)
    }
}

impl LauncherWindow {
    /// The toast the footer shows with the launcher's `status`: the
    /// launcher's, while its status line is idle or running.
    pub(crate) fn footer_toast(&self, status: &Status) -> Option<ShownToast> {
        if matches!(status, Status::Idle | Status::Running) {
            self.launcher.toast()
        } else {
            None
        }
    }

    /// Counts the time of `toast`, the toast drawn this frame: a success or
    /// failure leaves the footer once [`TOAST_DURATION`] has run, the time
    /// paused while the pointer is over it or one of its actions has the
    /// focus. An update starts the time again.
    pub(crate) fn time_toast(
        &mut self,
        toast: Option<&ShownToast>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(shown) = toast.filter(|shown| shown.toast.style.hides_by_itself()) else {
            self.toast.countdown = None;
            return;
        };
        let fresh = !self.toast.countdown.as_ref().is_some_and(|countdown| {
            countdown.id == shown.id && countdown.revision == shown.revision
        });
        if fresh {
            self.toast.countdown = Some(Countdown {
                id: shown.id,
                revision: shown.revision,
                left: TOAST_DURATION,
                since: None,
                _due: None,
            });
        }
        let held = self.toast.hovered || self.toast.focused(window);
        let now = cx.background_executor().now();
        let Some(countdown) = self.toast.countdown.as_mut() else {
            return;
        };
        match (held, countdown.since) {
            // Paused: what is left is kept for later.
            (true, Some(since)) => {
                countdown.left = countdown
                    .left
                    .saturating_sub(now.saturating_duration_since(since));
                countdown.since = None;
                countdown._due = None;
            }
            // Running again, for what is left.
            (false, None) => {
                countdown.since = Some(now);
                let (id, revision, left) = (countdown.id, countdown.revision, countdown.left);
                countdown._due = Some(cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(left).await;
                    this.update(cx, |this, cx| {
                        this.launcher.toast_left(id, revision);
                        this.toast.countdown = None;
                        cx.notify();
                    })
                    .ok();
                }));
            }
            _ => {}
        }
    }

    /// The toast key: the focus moves to the toast's first action, when
    /// the footer shows a toast with actions.
    pub(crate) fn focus_toast(
        &mut self,
        _: &FocusToast,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let status = self.launcher.view().status;
        let Some(shown) = self.footer_toast(&status) else {
            cx.propagate();
            return;
        };
        let slot = if shown.toast.primary.is_some() {
            ToastSlot::Primary
        } else if shown.toast.secondary.is_some() {
            ToastSlot::Secondary
        } else {
            cx.propagate();
            return;
        };
        if !self.toast.focused(window) {
            self.toast.restore = window.focused(cx);
        }
        let focus = self.toast.focus(slot).clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Enter or Space on a toast action's button chooses it.
    fn press_toast_action(
        &mut self,
        _: &PressToastAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let slot = if self.toast.secondary.is_focused(window) {
            ToastSlot::Secondary
        } else {
            ToastSlot::Primary
        };
        self.choose_toast_action(slot, window, cx);
    }

    /// Escape on a toast action's button gives the focus back.
    fn leave_toast(&mut self, _: &LeaveToast, window: &mut Window, cx: &mut Context<Self>) {
        self.give_focus_back(window, cx);
        cx.notify();
    }

    /// Gives the focus the toast key took back: to what had it, else to
    /// the search field or the list.
    fn give_focus_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.toast.focused(window) {
            self.toast.restore = None;
            return;
        }
        match self.toast.restore.take() {
            Some(restore) => window.focus(&restore, cx),
            None if self.launcher.view().search_field().is_some() => self.query.focus(window, cx),
            None => window.focus(&self.focus_handle, cx),
        }
    }

    /// Chooses the action in `slot` of the toast the footer shows: Pane's
    /// own copy puts its text on the clipboard first; the launcher then
    /// calls the command back, and the window shows the answer.
    pub(crate) fn choose_toast_action(
        &mut self,
        slot: ToastSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let status = self.launcher.view().status;
        let Some(shown) = self.footer_toast(&status) else {
            return;
        };
        if let Some(text) = self.launcher.toast_action_copy(shown.id, slot) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        self.give_focus_back(window, cx);
        self.motion.land_at_once();
        let pending = self.launcher.run_toast_action(shown.id, slot);
        self.show_until_done(pending, window, cx);
    }

    /// The toast's actions' own shortcuts, while the footer shows it and
    /// nothing is open over the launcher: the action runs, once per press,
    /// and the key goes no further (an item's action with the same
    /// shortcut waits for the toast to leave).
    pub(crate) fn toast_action_keys(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A confirmation over the launcher takes the keys first (#146).
        if self.actions.is_some() || self.menu.is_some() || self.launcher.confirmation().is_some() {
            return;
        }
        let status = self.launcher.view().status;
        let Some(shown) = self.footer_toast(&status) else {
            return;
        };
        let Ok(pressed) = crate::keyboard::binding_of(&event.keystroke) else {
            return;
        };
        let Some(slot) = shown.toast.bound_to(&pressed) else {
            return;
        };
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        self.choose_toast_action(slot, window, cx);
    }

    /// The toast in the footer's middle, in place of the hint: its style's
    /// dot, its title and its message. The pointer over it pauses its
    /// time.
    pub(crate) fn render_toast(
        &self,
        shown: &ShownToast,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (style, color) = match shown.toast.style {
            ToastStyle::Animated => ("toast-animated", theme.warning),
            ToastStyle::Success => ("toast-success", theme.success),
            ToastStyle::Failure => ("toast-failure", theme.danger),
        };
        let title = shown.toast.title.clone();
        let message = shown.toast.message.clone();
        div()
            .id("toast")
            .debug_selector(|| "toast".into())
            .flex_1()
            .min_w(px(0.))
            .flex()
            .items_center()
            .gap_2()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if this.toast.hovered != *hovered {
                    this.toast.hovered = *hovered;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .debug_selector(move || style.into())
                    .flex_none()
                    .size(px(DOT))
                    .rounded_full()
                    .bg(color),
            )
            .child(
                // In its style's colour, as the status line's answers
                // and errors were.
                div()
                    .debug_selector(|| "toast-title".into())
                    .flex_none()
                    .text_color(color)
                    .child(title),
            )
            .when_some(message, |toast, message| {
                toast.child(
                    div()
                        .debug_selector(|| "toast-message".into())
                        .flex_initial()
                        .min_w(px(0.))
                        .truncate()
                        .text_color(theme.text_muted)
                        .child(message),
                )
            })
    }

    /// The toast's actions as the footer's buttons, each with its
    /// shortcut's keys: empty when it has none, and the footer keeps its
    /// own buttons then.
    pub(crate) fn toast_buttons(
        &self,
        shown: &ShownToast,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut buttons = Vec::new();
        for (slot, id) in [
            (ToastSlot::Primary, "toast-action-primary"),
            (ToastSlot::Secondary, "toast-action-secondary"),
        ] {
            let Some(action) = shown.toast.action(slot) else {
                continue;
            };
            if !buttons.is_empty() {
                buttons.push(footer::divider(theme).into_any_element());
            }
            let keys = action
                .shortcut
                .as_ref()
                .map(crate::keyboard::binding_keys)
                .unwrap_or(KeySequence { keys: Vec::new() });
            let label = action.title.clone();
            let shortcut = keys.name();
            let button = footer::footer_button(
                id,
                label.clone(),
                &keys,
                CapStyle::Regular,
                ButtonWash::Hover,
                theme,
            )
            .key_context(BUTTON_CONTEXT)
            .track_focus(self.toast.focus(slot))
            .role(Role::Button)
            .aria_label(label)
            .when(!shortcut.is_empty(), |button| {
                button.aria_keyshortcuts(shortcut)
            })
            .focus(|button| {
                button.shadow(vec![
                    BoxShadow::new(px(0.), px(0.), theme.focus_ring)
                        .spread_radius(px(1.))
                        .inset(),
                ])
            })
            .on_action(cx.listener(Self::press_toast_action))
            .on_action(cx.listener(Self::leave_toast))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                // A double click's second click chooses nothing again.
                if event.click_count() > 1 {
                    return;
                }
                this.choose_toast_action(slot, window, cx);
            }))
            .cursor_pointer();
            buttons.push(button.into_any_element());
        }
        buttons
    }
}
