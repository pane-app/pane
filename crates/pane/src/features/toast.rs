//! The toast in the launcher's footer (#141, ADR 0037): what a command
//! says happened, drawn where the status line is, one at a time (the
//! launcher keeps it, see `pane_core::feedback`), and — since "Launcher
//! polish" (#249) — an outcome of the status line, a result or a failure,
//! drawn and timed through these same controls: the core keeps the
//! status, the window owns the timing and clears an expired outcome
//! through the launcher (`Launcher::outcome_left`), so a later screen
//! never shows a stale one. Work in progress in words keeps its words.
//!
//! The footer shows one toast: a dot in the toast's style (work in
//! progress, success, failure), its title and its message, its details
//! affordance when the text does not fit on one line, and its close
//! button, which appears on hover or focus. A success or failure leaves
//! after `TOAST_DURATION` (3 seconds, ADR 0035), counted only while the
//! pointer is not over it, its details are not open and none of its
//! controls has the focus; when that ends, the full time starts again.
//! An animated one stays until its command updates or hides it, or the
//! window deactivates.
//!
//! **Leaving.** The close button, and Escape while the toast has the
//! focus, dismiss it at once: the launcher's toast leaves the footer
//! (`Launcher::toast_left`), an outcome status goes back to rest. Tab
//! reaches the close button and the details affordance; both are named.
//!
//! **Details.** A toast whose text does not fit on one line shows its
//! first line, truncated, with a details affordance; the toast key
//! (Ctrl+T, Command+T on macOS, `pane_core::keyboard::toast_key`) or a
//! click on the affordance opens the full text, wrapped and scrollable,
//! in a popover above the footer, which also lists the toast's actions
//! as its rows. The toast key supersedes the binding that moved the
//! focus to the toast's action: the actions are reached by keys inside
//! the popover, Down and Up between them, Enter choosing, Escape
//! closing; an action's own shortcut still runs it while the toast
//! shows, wherever the focus is. Choosing an action calls its command
//! back (`Launcher::run_toast_action`); Pane's own "Copy Error" copies
//! the error first.
//!
//! How the toast looks is refined by "Launcher polish" (#123); its debug
//! selectors (`toast`, `toast-<style>`, `toast-title`, `toast-message`,
//! `toast-details-button`, `toast-close`, `toast-details`,
//! `toast-details-text`, `toast-details-title`, `toast-details-close`,
//! `toast-details-action-<slot>`) are what tests find.

use gpui::{
    AnyElement, App, BoxShadow, ClickEvent, ClipboardItem, Context, Div, FocusHandle, Hsla,
    KeyBinding, KeyDownEvent, MouseDownEvent, Pixels, Role, Size, Stateful, Subscription, Task,
    TextStyle, Window, actions, div, prelude::*, px, relative,
};
use pane_core::feedback::TOAST_DURATION;
use pane_core::{ShownToast, Status, Toast, ToastSlot, ToastStyle};

use crate::app::{KEY_CONTEXT, LauncherWindow};
use crate::features::announcer::{Listing, Opening, Selected, Target};
use crate::ui::icon::{Glyph, glyph, glyph_rotated};
use crate::ui::keycap::{CapStyle, key_sequence};
use crate::ui::material::Material;
use crate::ui::theme::Theme;

actions!(
    toast,
    [
        OpenToastDetails,
        CloseToastDetails,
        NextToastAction,
        PreviousToastAction,
        ChooseToastAction,
        DismissToast
    ]
);

/// The toast's own context, on its row: Escape dismisses the toast while
/// one of its controls has the focus.
const TOAST_CONTEXT: &str = "Toast";
/// The details popover's context: the keys its list takes while it holds
/// the focus.
const DETAILS_CONTEXT: &str = "ToastDetails";
/// The details affordance's own context, so Enter and Space open the
/// details rather than run the launcher's confirm.
const DETAILS_BUTTON_CONTEXT: &str = "ToastDetailsButton";
/// The toast's close button's own context, so Enter and Space dismiss
/// the toast rather than run the launcher's confirm.
const CLOSE_CONTEXT: &str = "ToastClose";
/// The popover's close button's own context, so Enter and Space close
/// the details.
const DETAILS_CLOSE_CONTEXT: &str = "ToastDetailsClose";

/// The popover's name, its content's and the affordance's label.
const DETAILS_NAME: &str = "Toast details";
/// The toast's close button's label.
const DISMISS_NAME: &str = "Dismiss";
/// The toast's close button's and details affordance's box, in px.
const CONTROL: f32 = 24.;
/// A control's glyph, in px.
const GLYPH: f32 = 12.;
/// The dot that says a toast's style, in px.
const DOT: f32 = 8.;
/// The popover's least and most width, in px: wide enough to read the
/// full text in, never wider than the Actions panel is.
const POPOVER_MIN: f32 = 280.;
const POPOVER_MAX: f32 = 460.;
/// How tall the popover's full text may be, in px, before it scrolls.
const TEXT_MAX: f32 = 200.;
/// What the footer's buttons are taken to take, when deciding whether a
/// toast's text fits on one line: while an outcome shows, the primary
/// action steps aside and the column is Actions and its keys; while the
/// launcher is idle, the selected result's action is there too.
const BUTTONS_BUSY: f32 = 120.;
const BUTTONS_IDLE: f32 = 300.;
/// The toast's own parts: the dot, the gaps around its controls, the
/// details affordance and the close button.
const TOAST_CHROME: f32 = 8. + 16. + CONTROL + 8. + CONTROL;

/// Registers the toast key in the launcher, the toast's controls'
/// activation keys while they are focused, and the details popover's
/// keys while it holds the focus.
pub(crate) fn bind_keys(cx: &mut App) {
    let toast_key = pane_core::keyboard::toast_key().id();
    cx.bind_keys([
        KeyBinding::new(&toast_key, OpenToastDetails, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", DismissToast, Some(TOAST_CONTEXT)),
        KeyBinding::new("enter", OpenToastDetails, Some(DETAILS_BUTTON_CONTEXT)),
        KeyBinding::new("space", OpenToastDetails, Some(DETAILS_BUTTON_CONTEXT)),
        KeyBinding::new("enter", DismissToast, Some(CLOSE_CONTEXT)),
        KeyBinding::new("space", DismissToast, Some(CLOSE_CONTEXT)),
        KeyBinding::new("down", NextToastAction, Some(DETAILS_CONTEXT)),
        KeyBinding::new("up", PreviousToastAction, Some(DETAILS_CONTEXT)),
        KeyBinding::new("enter", ChooseToastAction, Some(DETAILS_CONTEXT)),
        KeyBinding::new("escape", CloseToastDetails, Some(DETAILS_CONTEXT)),
        KeyBinding::new("enter", CloseToastDetails, Some(DETAILS_CLOSE_CONTEXT)),
        KeyBinding::new("space", CloseToastDetails, Some(DETAILS_CLOSE_CONTEXT)),
    ]);
}

/// The footer toast's controls: its focus, its details and its time.
pub(crate) struct ToastControls {
    /// The close button's focus: a tab stop while a toast is drawn.
    close: FocusHandle,
    /// The details affordance's focus: a tab stop while a long toast is
    /// drawn.
    details: FocusHandle,
    /// The popover's close button's focus: a tab stop while the details
    /// are open.
    details_close: FocusHandle,
    /// The open details popover, while one is.
    open: Option<ToastDetails>,
    /// What had the focus when the toast's controls took it, given back
    /// when the toast is dismissed or its details close.
    restore: Option<FocusHandle>,
    /// Whether the pointer is over the toast.
    hovered: bool,
    /// The toast's time, while it hides by itself.
    countdown: Option<Countdown>,
}

/// The time of the toast shown: what it dismisses, and the task that
/// ends it. Pausing drops the task; resuming starts the full time again,
/// as the toast controls do (ADR 0035).
struct Countdown {
    /// What this time dismisses.
    timed: Timed,
    /// Ends the toast when the time is up; `None` while the time is
    /// paused.
    due: Option<Task<()>>,
}

/// What a toast shown in the footer is, for its time: the launcher's
/// toast (an extension's), or an outcome of the status line.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Timed {
    /// The launcher's toast, by id and revision: an update starts its
    /// time again, and the window clears it through
    /// [`Launcher::toast_left`].
    Toast { id: u64, revision: u64 },
    /// An outcome the status line set, as the status itself: the window
    /// clears it through [`Launcher::outcome_left`]. Two different
    /// outcomes are two toasts; the status text is the identity.
    Outcome(Status),
}

/// The toast's open details: the popover above the footer strip, with
/// the toast's full text and its actions.
pub(crate) struct ToastDetails {
    /// The toast whose details these are: a toast that is replaced or
    /// updated closes them.
    toast: ShownToast,
    /// The popover's focus, not a tab stop: the details are opened by
    /// the toast key or the affordance, not reached through traversal.
    focus: FocusHandle,
    /// The selected action, an index into the toast's actions.
    selected: usize,
    /// Closes the details when the window loses activation, as native
    /// popovers do; ends with them.
    _deactivation: Subscription,
}

impl ToastControls {
    pub(crate) fn new(cx: &mut App) -> ToastControls {
        ToastControls {
            close: cx.focus_handle().tab_stop(true),
            details: cx.focus_handle().tab_stop(true),
            details_close: cx.focus_handle().tab_stop(true),
            open: None,
            restore: None,
            hovered: false,
            countdown: None,
        }
    }

    /// Whether the toast has the focus: one of its controls — the
    /// close button, the details affordance, the popover's close button —
    /// or its open details.
    fn focused(&self, window: &Window) -> bool {
        self.close.is_focused(window)
            || self.details.is_focused(window)
            || self.details_close.is_focused(window)
            || self
                .open
                .as_ref()
                .is_some_and(|details| details.focus.is_focused(window))
    }
}

/// How a toast or HUD of `style` is drawn: its debug selector
/// (`toast-animated`, `toast-success`, `toast-failure`) and the colour of
/// its dot. The footer's toast, Clipboard History's footer lead and the
/// HUD all draw a style so.
pub(crate) fn style_look(style: ToastStyle, theme: &Theme) -> (&'static str, Hsla) {
    match style {
        ToastStyle::Animated => ("toast-animated", theme.warning),
        ToastStyle::Success => ("toast-success", theme.success),
        ToastStyle::Failure => ("toast-failure", theme.danger),
    }
}

/// The slots of the toast's actions, in the order the details list them.
fn toast_slots(toast: &Toast) -> Vec<ToastSlot> {
    [ToastSlot::Primary, ToastSlot::Secondary]
        .into_iter()
        .filter(|slot| toast.action(*slot).is_some())
        .collect()
}

/// A toast slot's name, as its debug selector says it.
fn slot_name(slot: ToastSlot) -> &'static str {
    match slot {
        ToastSlot::Primary => "primary",
        ToastSlot::Secondary => "secondary",
    }
}

/// The first line of `text`, as the toast's one line shows it.
fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_owned()
}

/// The text style the footer's toast draws its text in, as the layout
/// that decides whether it fits on one line measures it.
fn footer_text_style(theme: &Theme) -> TextStyle {
    let typography = &theme.typography;
    TextStyle {
        font_family: typography.family.clone(),
        font_features: typography.features.clone(),
        font_size: typography.footer_size.into(),
        ..TextStyle::default()
    }
}

/// The room the footer leaves the toast's one line, as an estimate: the
/// window's width less the strip's paddings, the mark, the buttons'
/// column and the toast's own parts. An estimate errs toward calling a
/// toast long, which only draws a details affordance that is harmless,
/// rather than hiding a text that does not fit with no way to read the
/// rest of it. `busy` says whether the primary action is beside the
/// toast, as an outcome makes it step aside.
fn one_line_room(viewport: Size<Pixels>, theme: &Theme, busy: bool) -> Pixels {
    let geometry = &theme.geometry;
    let buttons = if busy { BUTTONS_BUSY } else { BUTTONS_IDLE };
    viewport.width
        - geometry.footer_padding_left
        - geometry.footer_padding_right
        - geometry.footer_mark_size
        - geometry.footer_lead_gap
        - px(buttons + TOAST_CHROME)
}

/// Whether the toast's text does not fit on one line in the footer: the
/// text has more than one line, or its one line is wider than the room
/// the footer's other parts leave it. Such a toast shows its first line,
/// truncated, with a details affordance that opens the full text.
fn text_is_long(
    shown: &ShownToast,
    theme: &Theme,
    viewport: Size<Pixels>,
    busy: bool,
    window: &Window,
) -> bool {
    let text = shown.toast.text();
    if text.lines().count() > 1 {
        return true;
    }
    let style = footer_text_style(theme);
    let run = style.to_run(text.len());
    let font_size = style.font_size.to_pixels(window.rem_size());
    let width = window
        .text_system()
        .layout_line(&text, font_size, &[run], None)
        .width;
    width > one_line_room(viewport, theme, busy)
}

/// The popover's width, at most [`POPOVER_MAX`] and never past the
/// window's edges.
fn popover_width(viewport: Size<Pixels>, theme: &Theme) -> Pixels {
    let inset = theme.geometry.actions.inset;
    (viewport.width - inset * 2.).min(px(POPOVER_MAX))
}

impl LauncherWindow {
    /// The toast the footer shows with the launcher's `status`: the
    /// launcher's, while its status line is idle or running, and an
    /// outcome of the status line — a result or a failure — as a toast
    /// of its own (#249), which the window draws and times through these
    /// controls. Work in progress in words keeps its words in the footer.
    pub(crate) fn footer_toast(&self, status: &Status) -> Option<ShownToast> {
        match status {
            Status::Idle | Status::Running => self.launcher.toast(),
            Status::Result(text) => Some(ShownToast {
                id: 0,
                revision: 0,
                toast: Toast::new(ToastStyle::Success, text.clone()),
            }),
            Status::Error(text) => Some(ShownToast {
                id: 0,
                revision: 0,
                toast: Toast::new(ToastStyle::Failure, text.clone()),
            }),
            Status::Progress(_) => None,
        }
    }

    /// Counts the time of `toast`, the toast drawn this frame with the
    /// launcher's `status`: a success or failure leaves the footer once
    /// [`TOAST_DURATION`] has run, an outcome clearing through the
    /// launcher, the launcher's toast through it. The time is paused
    /// while the pointer is over the toast, its details are open or one
    /// of its controls has the focus, and the full time starts again when
    /// that ends; an update or a new toast starts it over too.
    pub(crate) fn time_toast(
        &mut self,
        toast: Option<&ShownToast>,
        status: &Status,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The details of a toast that is gone, or was replaced by
        // another, close with it.
        if self
            .toast
            .open
            .as_ref()
            .is_some_and(|details| toast != Some(&details.toast))
        {
            self.close_toast_details(window, cx);
        }
        let timed = match status {
            Status::Result(_) | Status::Error(_) => Some(Timed::Outcome(status.clone())),
            _ => toast
                .filter(|shown| shown.toast.style.hides_by_itself())
                .map(|shown| Timed::Toast {
                    id: shown.id,
                    revision: shown.revision,
                }),
        };
        let Some(timed) = timed else {
            self.toast.countdown = None;
            return;
        };
        let fresh = self
            .toast
            .countdown
            .as_ref()
            .is_none_or(|countdown| countdown.timed != timed);
        if fresh {
            self.toast.countdown = Some(Countdown { timed, due: None });
        }
        // The toast's time is held while the pointer is over it, its
        // details are open, or one of its controls has the focus.
        let held = self.toast.hovered || self.toast.open.is_some() || self.toast.focused(window);
        let Some(countdown) = self.toast.countdown.as_mut() else {
            return;
        };
        if held {
            // Paused: the task that would end the toast is dropped, and
            // the full time starts again once it is released.
            countdown.due = None;
        } else if countdown.due.is_none() {
            let timed = countdown.timed.clone();
            countdown.due = Some(cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(TOAST_DURATION).await;
                this.update(cx, |this, cx| {
                    this.leave_timed_toast(&timed);
                    this.toast.countdown = None;
                    cx.notify();
                })
                .ok();
            }));
        }
    }

    /// The toast `timed` has been shown for its time: it leaves the
    /// footer, an outcome status going back to rest so a later screen
    /// never shows a stale one, the launcher's toast leaving it.
    fn leave_timed_toast(&mut self, timed: &Timed) {
        match timed {
            Timed::Toast { id, revision } => self.launcher.toast_left(*id, *revision),
            Timed::Outcome(outcome) => self.launcher.outcome_left(outcome),
        }
    }

    /// Dismisses the toast the footer shows at once, as the close button
    /// and Escape while the toast has the focus do: the launcher's toast
    /// leaves the footer, an outcome status goes back to rest.
    fn leave_the_shown_toast(&mut self) {
        let status = self.launcher.status();
        match &status {
            Status::Result(_) | Status::Error(_) => self.launcher.outcome_left(&status),
            _ => {
                if let Some(shown) = self.launcher.toast() {
                    self.launcher.toast_left(shown.id, shown.revision);
                }
            }
        }
        self.toast.countdown = None;
    }

    /// The toast key and the details affordance: opens the toast's full
    /// text and its actions in a popover above the footer, moving the
    /// focus into it. With the details already open, it closes them, so
    /// the key toggles.
    pub(crate) fn open_toast_details(
        &mut self,
        _: &OpenToastDetails,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.toast.open.is_some() {
            self.close_toast_details(window, cx);
            return;
        }
        // The compact bar has no footer to anchor a popover over.
        if self.is_collapsed() {
            cx.propagate();
            return;
        }
        let status = self.launcher.status();
        let Some(shown) = self.footer_toast(&status) else {
            cx.propagate();
            return;
        };
        // The Actions panel and the details are two popovers over the
        // same strip: one at a time.
        self.close_actions(window, cx);
        let focus = cx.focus_handle().tab_stop(false);
        if !self.toast.focused(window) {
            self.toast.restore = window.focused(cx);
        }
        window.focus(&focus, cx);
        let deactivation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.close_toast_details(window, cx);
            }
        });
        self.toast.open = Some(ToastDetails {
            toast: shown,
            focus,
            selected: 0,
            _deactivation: deactivation,
        });
        cx.notify();
    }

    /// Closes the toast's details, if they are open, giving the focus
    /// they took back — unless the focus moved elsewhere while they were
    /// open, which stays where the user put it. The popover's Escape and
    /// close button, its toast being replaced or gone, the Actions panel
    /// opening, and the window deactivating all end up here.
    pub(crate) fn close_toast_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Whether the popover (or a toast control) holds the focus is
        // read while it is still in the tree: after, its handle is gone,
        // and the focus must be given back rather than left on nothing.
        let held = self.toast.focused(window);
        if self.toast.open.take().is_some() {
            if held {
                self.give_focus_back(window, cx);
            } else {
                self.toast.restore = None;
            }
            cx.notify();
        }
    }

    /// The popover's Escape and close button: the details close, the
    /// toast itself stays.
    fn toast_close_details(
        &mut self,
        _: &CloseToastDetails,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_toast_details(window, cx);
    }

    /// Down in the details popover: the next action is selected.
    fn toast_next_action(&mut self, _: &NextToastAction, _: &mut Window, cx: &mut Context<Self>) {
        let count = self
            .toast
            .open
            .as_ref()
            .map(|details| toast_slots(&details.toast.toast).len())
            .unwrap_or(0);
        if let Some(details) = self.toast.open.as_mut()
            && details.selected + 1 < count
        {
            details.selected += 1;
            self.announcer.user_moved();
            cx.notify();
        }
    }

    /// Up in the details popover: the previous action is selected.
    fn toast_previous_action(
        &mut self,
        _: &PreviousToastAction,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(details) = self.toast.open.as_mut()
            && details.selected > 0
        {
            details.selected -= 1;
            self.announcer.user_moved();
            cx.notify();
        }
    }

    /// Enter in the details popover: the selected action is chosen.
    fn toast_choose_action(
        &mut self,
        _: &ChooseToastAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let slot = self.toast.open.as_ref().and_then(|details| {
            toast_slots(&details.toast.toast)
                .into_iter()
                .nth(details.selected)
        });
        if let Some(slot) = slot {
            self.choose_toast_action(slot, window, cx);
        }
    }

    /// The toast's close button, and Escape while the toast has the
    /// focus: the toast is dismissed at once, its details with it.
    fn dismiss_toast(&mut self, _: &DismissToast, window: &mut Window, cx: &mut Context<Self>) {
        let held = self.toast.focused(window);
        self.leave_the_shown_toast();
        self.toast.open = None;
        if held {
            self.give_focus_back(window, cx);
        }
        cx.notify();
    }

    /// Gives the focus the toast's controls took back: to what had it,
    /// else to the search field or the list. Only a caller that knows the
    /// toast's controls have the focus now calls this.
    fn give_focus_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.toast.restore.take() {
            Some(restore) => window.focus(&restore, cx),
            None if self.launcher.screen().search_field().is_some() => self.query.focus(window, cx),
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
        let status = self.launcher.status();
        let Some(shown) = self.footer_toast(&status) else {
            return;
        };
        if let Some(text) = self.launcher.toast_action_copy(shown.id, slot) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        // The details close with the choice, giving the focus they took
        // back; chosen by its shortcut, nothing had the focus to give
        // back.
        self.close_toast_details(window, cx);
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
        let status = self.launcher.status();
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
    /// dot, its title and its message — the first line of each, truncated
    /// — its details affordance when the text does not fit on one line,
    /// and its close button, which appears on hover or focus. The pointer
    /// over it pauses its time.
    pub(crate) fn render_toast(
        &self,
        shown: &ShownToast,
        theme: &Theme,
        viewport: Size<Pixels>,
        busy: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (style, color) = style_look(shown.toast.style, theme);
        let title = first_line(&shown.toast.title);
        let message = shown.toast.message.as_deref().map(first_line);
        let long = text_is_long(shown, theme, viewport, busy, window);
        let close_shown = self.toast.hovered || self.toast.focused(window);
        let details_button = long.then(|| self.toast_details_button(theme, cx));
        let close_button = self.toast_close_button(close_shown, theme, cx);
        div()
            .id("toast")
            .debug_selector(|| "toast".into())
            .flex_1()
            .min_w(px(0.))
            .flex()
            .items_center()
            .gap_2()
            .key_context(TOAST_CONTEXT)
            .on_action(cx.listener(Self::dismiss_toast))
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
                // and errors were, truncated to its first line.
                div()
                    .debug_selector(|| "toast-title".into())
                    .flex_initial()
                    .min_w(px(0.))
                    .truncate()
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
            .when_some(details_button, |toast, button| toast.child(button))
            .child(close_button)
    }

    /// The toast's details affordance, after its text: shown when the
    /// toast's text does not fit on one line, opening the details. The
    /// chevron points up, to where the popover opens.
    fn toast_details_button(&self, theme: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let keys = crate::keyboard::binding_keys(&pane_core::keyboard::toast_key());
        let shortcut = keys.name();
        div()
            .id("toast-details-button")
            .debug_selector(|| "toast-details-button".into())
            .flex_none()
            .size(px(CONTROL))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.geometry.actions.row_radius)
            .key_context(DETAILS_BUTTON_CONTEXT)
            .track_focus(&self.toast.details)
            .role(Role::Button)
            .aria_label(DETAILS_NAME)
            .aria_keyshortcuts(shortcut)
            .aria_expanded(self.toast.open.is_some())
            .hover(|button| button.bg(theme.control_hover))
            .focus(|button| {
                button.shadow(vec![
                    BoxShadow::new(px(0.), px(0.), theme.focus_ring)
                        .spread_radius(px(1.))
                        .inset(),
                ])
            })
            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                // A double click's second click opens nothing again.
                if event.click_count() > 1 {
                    return;
                }
                this.open_toast_details(&OpenToastDetails, window, cx);
            }))
            .cursor_pointer()
            .child(glyph_rotated(
                Glyph::ChevronRight,
                px(GLYPH),
                theme.text_muted,
                gpui::radians(-std::f32::consts::FRAC_PI_2),
            ))
    }

    /// The toast's close button, last in its row: it appears on hover or
    /// focus (as `close_shown` says) and dismisses the toast at once. Its
    /// slot is always in the row and it is always a tab stop, so the
    /// keyboard reaches it; while it is not shown it is drawn transparent
    /// and takes no clicks.
    fn toast_close_button(
        &self,
        close_shown: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id("toast-close")
            .debug_selector(|| "toast-close".into())
            .flex_none()
            .size(px(CONTROL))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.geometry.actions.row_radius)
            .key_context(CLOSE_CONTEXT)
            .track_focus(&self.toast.close)
            .role(Role::Button)
            .aria_label(DISMISS_NAME)
            .focus(|button| {
                button.shadow(vec![
                    BoxShadow::new(px(0.), px(0.), theme.focus_ring)
                        .spread_radius(px(1.))
                        .inset(),
                ])
            })
            .when(close_shown, |button| {
                button
                    .opacity(1.)
                    .cursor_pointer()
                    .hover(|button| button.bg(theme.control_hover))
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        // A double click's second click dismisses nothing
                        // again.
                        if event.click_count() > 1 {
                            return;
                        }
                        this.dismiss_toast(&DismissToast, window, cx);
                    }))
            })
            .when(!close_shown, |button| button.opacity(0.))
            .child(glyph(Glyph::Delete, px(GLYPH), theme.text_muted))
    }

    /// The popover's close button, at its top right: closes the details,
    /// leaving the toast.
    fn details_close_button(&self, theme: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("toast-details-close")
            .debug_selector(|| "toast-details-close".into())
            .flex_none()
            .size(px(CONTROL))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.geometry.actions.row_radius)
            .key_context(DETAILS_CLOSE_CONTEXT)
            .track_focus(&self.toast.details_close)
            .role(Role::Button)
            .aria_label(DISMISS_NAME)
            .hover(|button| button.bg(theme.control_hover))
            .focus(|button| {
                button.shadow(vec![
                    BoxShadow::new(px(0.), px(0.), theme.focus_ring)
                        .spread_radius(px(1.))
                        .inset(),
                ])
            })
            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                if event.click_count() > 1 {
                    return;
                }
                this.close_toast_details(window, cx);
            }))
            .cursor_pointer()
            .child(glyph(Glyph::Delete, px(GLYPH), theme.text_muted))
    }

    /// The open toast's details, the popover above the footer strip: the
    /// toast's full text, wrapped and scrollable, its actions as the
    /// Actions panel's rows, and its own close button. `None` while no
    /// details are open.
    pub(crate) fn render_toast_details_layer(
        &self,
        theme: &Theme,
        material: Material,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let details = self.toast.open.as_ref()?;
        let geometry = &theme.geometry.actions;
        let (_, color) = style_look(details.toast.toast.style, theme);
        let title = details.toast.toast.title.clone();
        let message = details.toast.toast.message.clone();
        let focus = details.focus.clone();
        let selected = details.selected;
        let slots = toast_slots(&details.toast.toast);
        let rows: Vec<_> = slots
            .iter()
            .enumerate()
            .map(|(index, slot)| {
                // A listed slot has its action; the slots came from it.
                // The slot is copied for the row's click, which owns it.
                let slot = *slot;
                let action = details
                    .toast
                    .toast
                    .action(slot)
                    .expect("the action of a listed slot");
                let label = action.title.clone();
                let shortcut = action.shortcut.as_ref().map(crate::keyboard::binding_keys);
                let row = slot_name(slot);
                div()
                    .id(("toast-action", index))
                    .debug_selector(move || format!("toast-details-action-{row}"))
                    .flex()
                    .items_center()
                    .gap(geometry.row_gap)
                    .h(geometry.row_height)
                    .px(geometry.row_padding_x)
                    .rounded(geometry.row_radius)
                    .text_size(theme.typography.action_size)
                    .font_weight(theme.typography.action_weight)
                    .text_color(theme.action_text)
                    .when(index == selected, |row| {
                        row.bg(theme.action_selected).aria_selected(true)
                    })
                    .when(index != selected, |row| {
                        row.hover(|row| row.bg(theme.control_hover))
                    })
                    .role(Role::MenuItem)
                    .aria_label(label.clone())
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.choose_toast_action(slot, window, cx);
                    }))
                    .cursor_pointer()
                    .child(label)
                    .child(div().flex_1().min_w(px(0.)))
                    .when_some(shortcut, |row, keys| {
                        row.child(key_sequence(&keys, CapStyle::Regular, theme))
                    })
            })
            .collect();
        // The toast's full text, wrapped, scrolling past the cap.
        let text = div()
            .id("toast-details-text")
            .debug_selector(|| "toast-details-text".into())
            .flex_initial()
            .min_w(px(0.))
            .w_full()
            .max_h(px(TEXT_MAX))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(theme.geometry.footer_hint_gap)
            .text_size(theme.typography.footer_size)
            .child(
                div()
                    .debug_selector(|| "toast-details-title".into())
                    .w_full()
                    .min_w(px(0.))
                    .flex_none()
                    .text_color(color)
                    .child(title),
            )
            .when_some(message, |text, message| {
                text.child(
                    div()
                        .debug_selector(|| "toast-details-message".into())
                        .w_full()
                        .min_w(px(0.))
                        .flex_none()
                        .text_color(theme.text_muted)
                        .child(message),
                )
            });
        // The popover's content: its close button, the full text, then
        // the toast's actions.
        let content = div()
            .id("toast-details")
            .key_context(DETAILS_CONTEXT)
            .track_focus(&focus)
            .role(Role::Dialog)
            .aria_label(DETAILS_NAME)
            .on_action(cx.listener(Self::toast_next_action))
            .on_action(cx.listener(Self::toast_previous_action))
            .on_action(cx.listener(Self::toast_choose_action))
            .on_action(cx.listener(Self::toast_close_details))
            // A mouse-down anywhere outside the popover dismisses it and
            // is consumed: nothing underneath is activated, and the
            // affordance's click, which would reopen it, does not run.
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.close_toast_details(window, cx);
                cx.stop_propagation();
            }))
            .flex()
            .flex_col()
            .gap(geometry.list_gap)
            .p(geometry.list_padding)
            .min_w(px(POPOVER_MIN))
            .max_w(popover_width(viewport, theme))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(self.details_close_button(theme, cx)),
            )
            .child(text)
            .children(rows);
        Some(
            div()
                .id("toast-details-popup")
                .debug_selector(|| "toast-details".into())
                .absolute()
                // The Actions panel's insets, mirrored: 10px in from the
                // window's left edge, 8px above the strip, however tall
                // the strip is.
                .left(geometry.inset)
                .bottom(relative(1.))
                .pb(geometry.above_footer)
                .flex_none()
                .occlude()
                .child(
                    div()
                        .relative()
                        .shadow(crate::ui::material::popover_shadows(theme))
                        .child(material.popover(theme, content)),
                )
                .into_any_element(),
        )
    }

    /// The open toast details as the announcer follows them (#132): over
    /// the screen, saying its selected action as it opens, since the
    /// screen reader reads the dialog's name as it takes the focus. With
    /// no actions, nothing is said beyond the dialog itself.
    pub(crate) fn toast_details_listing(&self) -> Option<Listing> {
        let details = self.toast.open.as_ref()?;
        let slots = toast_slots(&details.toast.toast);
        let slot = slots.get(details.selected)?;
        let action = details.toast.toast.action(*slot)?;
        let title = action.title.clone();
        Some(Listing {
            over: true,
            key: DETAILS_NAME.to_owned(),
            opening: Opening::Selection,
            count: slots.len(),
            target: Target::Row(Selected {
                id: title.clone(),
                title,
                position: details.selected + 1,
                unavailable: false,
                section: None,
            }),
            query: None,
            settled: true,
        })
    }
}
