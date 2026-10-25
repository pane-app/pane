//! Raycast's alert, drawn by both windows: a small dialog over the view it
//! was asked from, which stays drawn under it, dimmed.
//!
//! Two kinds of question are asked with it, one at a time:
//!
//! - **A command's confirmation** (#146, `feedback.confirm`), asked before
//!   it does something it cannot undo, as the launcher keeps it
//!   ([`pane_core::Launcher::confirmation`]). When the command gave a key
//!   to remember the answer by, it offers "Don't ask again": confirmed with
//!   the primary button while it is ticked, later confirmations with the
//!   same key are confirmed without asking. A dismissal is never
//!   remembered. While the launcher is hidden when a command asks (a
//!   no-view command run by its hotkey, or one that closed the window
//!   first), the window shows itself first, on the screen it was left on.
//! - **Pane's own questions** about an extension ([`Screen::Confirm`]):
//!   "Uninstall X?", disabling or uninstalling what other extensions
//!   require, clearing a cache or a history, deleting retained data. The
//!   launcher window draws the screen the question was asked over beneath
//!   it ([`pane_core::Launcher::beneath_question`]); Settings draws its
//!   page. An uninstall's "Delete saved data" checkbox starts unticked:
//!   Uninstall keeps the data, the safe answer.
//!
//! The alert has its title, its message, the primary button (Enter; drawn
//! in the destructive color when it removes something), a second answer
//! where the question has one, the dismiss button ("Cancel", or the name a
//! command gave it; Escape) and the checkbox where there is one. It takes
//! the keyboard while it is shown and gives it back once it is answered:
//! Tab and Shift+Tab move between its checkbox and buttons and never leave
//! it; Enter presses the focused button, or the primary one; Space presses
//! the focused button, or ticks the checkbox; Escape is the dismiss
//! button, wherever the focus is in it — only the alert is dismissed,
//! never the window. A click outside it, the window losing the focus or
//! hiding (the launcher's own rule for a command's confirmation) answer
//! that the user did not confirm.
//!
//! Debug selectors: `confirmation`, `confirmation-title`,
//! `confirmation-message`, `confirmation-line-<line>`,
//! `confirmation-primary`, `confirmation-destructive` (around a destructive
//! primary button), `confirmation-secondary`, `confirmation-dismiss`, the
//! checkbox's (`confirmation-dont-ask-again`, `confirmation-option`) and the
//! checkbox's with `-ticked` while it is ticked.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Context, FocusHandle, KeyBinding, MouseDownEvent, Role,
    SharedString, Toggled, Window, actions, div, prelude::*, px,
};
use pane_core::feedback::DONT_ASK_AGAIN;
use pane_core::{ConfirmAnswer, LauncherView, QuestionAnswer, Screen};

use crate::app::{LauncherWindow, Spot};
use crate::features::actions_panel::DESTRUCTIVE;
use crate::ui::controls;
use crate::ui::footer::{self, ButtonWash};
use crate::ui::icon::{Glyph, glyph};
use crate::ui::keycap::{CapStyle, Key, KeySequence, key_sequence};
use crate::ui::material::{Material, popover_shadows};
use crate::ui::theme::Theme;

actions!(
    confirmation,
    [
        AlertEnter,
        AlertEscape,
        AlertSpace,
        AlertNext,
        AlertPrevious
    ]
);

/// The alert's key context: its keys win over the window's confirm, back,
/// Escape and focus traversal.
pub(crate) const CONTEXT: &str = "Confirmation";

/// The alert's width, in px.
const WIDTH: f32 = 400.;

/// The tallest the message grows before it scrolls, in px: a question
/// listing many extensions keeps its buttons in the window.
const MESSAGE_MAX_HEIGHT: f32 = 180.;

/// The checkbox of Pane's own questions, as its selector names it.
const OPTION: &str = "confirmation-option";

/// The checkbox of a command's confirmation, as its selector names it.
const DONT_ASK_AGAIN_SELECTOR: &str = "confirmation-dont-ask-again";

/// Registers the alert's keys: Enter, Escape, Space, and Tab and
/// Shift+Tab, which stay in the alert.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", AlertEnter, Some(CONTEXT)),
        KeyBinding::new("escape", AlertEscape, Some(CONTEXT)),
        KeyBinding::new("space", AlertSpace, Some(CONTEXT)),
        KeyBinding::new("tab", AlertNext, Some(CONTEXT)),
        KeyBinding::new("shift-tab", AlertPrevious, Some(CONTEXT)),
    ]);
}

/// A part of the alert the keyboard and the pointer press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// The checkbox.
    Option,
    /// The dismiss button: Cancel.
    Dismiss,
    /// The second answer.
    Secondary,
    /// The primary button.
    Primary,
}

impl Part {
    /// The parts in the order Tab walks them, as they are laid out: the
    /// checkbox above the buttons, then the buttons left to right.
    const ORDER: [Part; 4] = [Part::Option, Part::Dismiss, Part::Secondary, Part::Primary];

    fn index(self) -> usize {
        Part::ORDER
            .iter()
            .position(|part| *part == self)
            .unwrap_or(0)
    }
}

/// What the user did on the alert: pressed one of its parts, or pressed
/// the pointer outside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Press {
    Part(Part),
    Outside,
}

/// The alert's focus: the dialog's own, which it takes when it shows (Enter
/// presses the primary button there, Space ticks the checkbox), and each
/// part's, which Tab moves between.
#[derive(Clone)]
pub(crate) struct AlertFocus {
    dialog: FocusHandle,
    parts: [FocusHandle; 4],
}

impl AlertFocus {
    pub(crate) fn new(cx: &mut App) -> AlertFocus {
        AlertFocus {
            dialog: cx.focus_handle(),
            parts: std::array::from_fn(|_| cx.focus_handle()),
        }
    }

    /// The dialog's own focus.
    pub(crate) fn dialog(&self) -> &FocusHandle {
        &self.dialog
    }

    /// Whether the keyboard is in the alert: on the dialog or one of its
    /// parts.
    pub(crate) fn holds(&self, window: &Window) -> bool {
        self.dialog.is_focused(window) || self.parts.iter().any(|part| part.is_focused(window))
    }

    /// The part the keyboard is on, if it is on one.
    fn focused_part(&self, window: &Window) -> Option<Part> {
        Part::ORDER
            .into_iter()
            .find(|part| self.parts[part.index()].is_focused(window))
    }

    fn part(&self, part: Part) -> &FocusHandle {
        &self.parts[part.index()]
    }
}

/// The checkbox of an alert.
pub(crate) struct AlertOption {
    pub(crate) label: SharedString,
    pub(crate) ticked: bool,
    /// Its debug selector.
    pub(crate) selector: &'static str,
}

/// What an alert shows.
pub(crate) struct Alert {
    pub(crate) title: SharedString,
    pub(crate) message: Vec<SharedString>,
    pub(crate) primary: SharedString,
    pub(crate) destructive: bool,
    pub(crate) secondary: Option<SharedString>,
    pub(crate) dismiss: SharedString,
    pub(crate) option: Option<AlertOption>,
    /// The icon beside the title, if it has one.
    pub(crate) icon: Option<Glyph>,
    /// The hover wash of each part's button, by [`Part::ORDER`], as the
    /// view drawing the alert keeps it.
    pub(crate) hover: [f32; 4],
}

impl Alert {
    /// The alert with `look` as the hover wash of each part's button.
    pub(crate) fn washed(self, look: impl Fn(Part) -> f32) -> Alert {
        Alert {
            hover: Part::ORDER.map(look),
            ..self
        }
    }

    /// Whether the alert has `part`.
    fn has(&self, part: Part) -> bool {
        match part {
            Part::Option => self.option.is_some(),
            Part::Secondary => self.secondary.is_some(),
            Part::Dismiss | Part::Primary => true,
        }
    }

    /// The alert for Pane's own question `view` asks ([`Screen::Confirm`]),
    /// its checkbox `ticked`; `None` on any other screen.
    pub(crate) fn of_question(view: &LauncherView, ticked: bool) -> Option<Alert> {
        let Screen::Confirm { alert, .. } = &view.screen else {
            return None;
        };
        Some(Alert {
            title: view.title.clone().into(),
            message: alert.message.iter().cloned().map(Into::into).collect(),
            primary: alert.primary.clone().into(),
            destructive: alert.destructive,
            secondary: alert.secondary.clone().map(Into::into),
            dismiss: pane_core::feedback::DISMISS.into(),
            option: alert.option.clone().map(|label| AlertOption {
                label: label.into(),
                ticked,
                selector: OPTION,
            }),
            icon: Some(Glyph::Blocks),
            hover: [0.; 4],
        })
    }
}

/// What a view does with a press on its alert.
pub(crate) type OnPress<V> = Rc<dyn Fn(&mut V, Press, &mut Window, &mut Context<V>)>;

/// What a view does when the pointer comes over one of its alert's
/// buttons (`true`) or leaves it.
pub(crate) type OnHover<V> = Rc<dyn Fn(&mut V, Part, bool, &mut Context<V>)>;

/// What a view drawing an alert does with the user's presses on it and
/// with the pointer over its buttons.
pub(crate) struct Handlers<V> {
    pub(crate) press: OnPress<V>,
    pub(crate) hover: OnHover<V>,
}

impl<V> Clone for Handlers<V> {
    fn clone(&self) -> Self {
        Handlers {
            press: self.press.clone(),
            hover: self.hover.clone(),
        }
    }
}

/// A key sequence of the one key `cap`, named `name`.
fn one_key(cap: &str, name: &str) -> KeySequence {
    KeySequence {
        keys: vec![Key::new(cap.to_owned(), name.to_owned())],
    }
}

/// Moves the keyboard to the next part of `alert` (`forward`) or the
/// previous one, around: from the dialog itself, to the first or the last.
fn step_focus(
    alert_parts: &[Part],
    focus: &AlertFocus,
    forward: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if alert_parts.is_empty() {
        return;
    }
    let at = focus
        .focused_part(window)
        .and_then(|part| alert_parts.iter().position(|offered| *offered == part));
    let count = alert_parts.len();
    let next = match (at, forward) {
        (None, true) => 0,
        (None, false) => count - 1,
        (Some(at), true) => (at + 1) % count,
        (Some(at), false) => (at + count - 1) % count,
    };
    window.focus(focus.part(alert_parts[next]), cx);
}

/// The alert layer: `alert` in the middle of the window over a dimmer that
/// takes every click, with `focus` tracked on it and its parts, and the
/// keys and clicks given to `handlers`.
pub(crate) fn alert_layer<V: 'static>(
    alert: Alert,
    focus: &AlertFocus,
    handlers: Handlers<V>,
    theme: &Theme,
    material: Material,
    window: &Window,
    cx: &mut Context<V>,
) -> AnyElement {
    let parts: Vec<Part> = Part::ORDER
        .into_iter()
        .filter(|part| alert.has(*part))
        .collect();
    // The focus ring, for the keyboard only, as Pane's controls draw it.
    let keyboard = window.last_input_was_keyboard();
    let ringed = |part: Part| keyboard && focus.part(part).is_focused(window);
    let pressing = |part: Part| {
        let press = handlers.press.clone();
        cx.listener(move |this: &mut V, _: &ClickEvent, window, cx| {
            press(this, Press::Part(part), window, cx);
        })
    };
    let hovering = |part: Part| {
        let hover = handlers.hover.clone();
        cx.listener(move |this: &mut V, over: &bool, _, cx| hover(this, part, *over, cx))
    };
    let button =
        |part: Part, id: &'static str, label: SharedString, keys: &KeySequence, caps: CapStyle| {
            footer::footer_button(
                id,
                label.clone(),
                keys,
                caps,
                ButtonWash::Hover(alert.hover[part.index()]),
                theme,
            )
            .track_focus(focus.part(part))
            .when(ringed(part), |button| {
                button.shadow(controls::focus_ring(theme))
            })
            .role(Role::Button)
            .aria_label(label)
            .when(!keys.keys.is_empty(), |button| {
                button.aria_keyshortcuts(keys.name())
            })
            .cursor_pointer()
        };

    let primary_keys = one_key("↵", "Enter");
    let primary = button(
        Part::Primary,
        "confirmation-primary",
        alert.primary.clone(),
        &primary_keys,
        if alert.destructive {
            CapStyle::Regular
        } else {
            CapStyle::Accent
        },
    )
    .when(alert.destructive, |button| {
        button
            .text_color(theme.danger)
            .aria_description(DESTRUCTIVE)
    })
    .on_hover(hovering(Part::Primary))
    .on_click(pressing(Part::Primary));
    // A destructive primary button is marked around it, for the tests that
    // find it: its own selector stays the button's.
    let primary = div()
        .flex_none()
        .when(alert.destructive, |wrapper| {
            wrapper.debug_selector(|| "confirmation-destructive".into())
        })
        .child(primary);
    let secondary = alert.secondary.clone().map(|label| {
        button(
            Part::Secondary,
            "confirmation-secondary",
            label,
            &KeySequence { keys: Vec::new() },
            CapStyle::Regular,
        )
        .on_hover(hovering(Part::Secondary))
        .on_click(pressing(Part::Secondary))
    });
    let dismiss_keys = one_key("Esc", "Escape");
    let dismiss = button(
        Part::Dismiss,
        "confirmation-dismiss",
        alert.dismiss.clone(),
        &dismiss_keys,
        CapStyle::Regular,
    )
    .on_hover(hovering(Part::Dismiss))
    .on_click(pressing(Part::Dismiss));

    let option = alert.option.as_ref().map(|option| {
        let ticked = option.ticked;
        let selector = option.selector;
        let mark = div()
            .flex_none()
            .size(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .border_1()
            .border_color(if ticked {
                theme.accent
            } else {
                theme.text_muted
            })
            .when(ticked, |mark| {
                mark.debug_selector(move || format!("{selector}-ticked"))
                    .bg(theme.accent)
                    .child(div().size(px(6.)).rounded(px(2.)).bg(theme.text_title))
            });
        let space = one_key("Space", "Space");
        div()
            .id(selector)
            .debug_selector(move || selector.into())
            .track_focus(focus.part(Part::Option))
            .when(ringed(Part::Option), |row| {
                row.shadow(controls::focus_ring(theme))
            })
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(4.))
            .text_size(theme.typography.footer_size)
            .text_color(theme.text_body)
            .role(Role::CheckBox)
            .aria_label(option.label.clone())
            .aria_toggled(if ticked {
                Toggled::True
            } else {
                Toggled::False
            })
            .aria_keyshortcuts(space.name())
            .cursor_pointer()
            .on_click(pressing(Part::Option))
            .child(mark)
            .child(div().flex_1().child(option.label.clone()))
            .child(key_sequence(&space, CapStyle::Regular, theme))
    });

    let title = div()
        .debug_selector(|| "confirmation-title".into())
        .flex()
        .items_center()
        .gap_2()
        .text_size(theme.typography.row_title_size)
        .font_weight(theme.typography.medium)
        .text_color(theme.text_title)
        .children(alert.icon.map(|icon| {
            glyph(
                icon,
                px(16.),
                if alert.destructive {
                    theme.danger
                } else {
                    theme.accent
                },
            )
            .flex_none()
        }))
        .child(div().flex_1().min_w(px(0.)).child(alert.title.clone()));
    let description: SharedString = alert
        .message
        .iter()
        .map(SharedString::as_ref)
        .collect::<Vec<&str>>()
        .join("\n")
        .into();
    let message = (!alert.message.is_empty()).then(|| {
        div()
            .id("confirmation-message")
            .debug_selector(|| "confirmation-message".into())
            .max_h(px(MESSAGE_MAX_HEIGHT))
            // In a short window the message gives up its room first, and
            // scrolls: the buttons stay in view.
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .text_size(theme.typography.row_subtitle_size)
            .text_color(theme.text_body)
            .children(alert.message.iter().map(|line| {
                let selector = format!("confirmation-line-{line}");
                div().debug_selector(move || selector).child(line.clone())
            }))
    });
    let content = div()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .gap_3()
        .p_4()
        .child(title.flex_none())
        .children(message)
        .children(option.map(|option| option.flex_none()))
        .child(
            div()
                .flex_none()
                .flex()
                .flex_wrap()
                .justify_end()
                .gap_2()
                .child(dismiss)
                .children(secondary)
                .child(primary),
        );

    let on_key = |key: fn(Option<Part>, &[Part]) -> Option<KeyDo>| {
        let press = handlers.press.clone();
        let focus = focus.clone();
        let parts = parts.clone();
        move |this: &mut V, window: &mut Window, cx: &mut Context<V>| match key(
            focus.focused_part(window),
            &parts,
        ) {
            Some(KeyDo::Press(part)) => press(this, Press::Part(part), window, cx),
            Some(KeyDo::Step(forward)) => step_focus(&parts, &focus, forward, window, cx),
            None => {}
        }
    };
    let enter = on_key(|focused, _| {
        Some(KeyDo::Press(match focused {
            Some(part @ (Part::Dismiss | Part::Secondary | Part::Primary)) => part,
            _ => Part::Primary,
        }))
    });
    let space = on_key(|focused, parts| match focused {
        Some(part @ (Part::Dismiss | Part::Secondary | Part::Primary)) => Some(KeyDo::Press(part)),
        _ => parts
            .contains(&Part::Option)
            .then_some(KeyDo::Press(Part::Option)),
    });
    let escape = on_key(|_, _| Some(KeyDo::Press(Part::Dismiss)));
    let next = on_key(|_, _| Some(KeyDo::Step(true)));
    let previous = on_key(|_, _| Some(KeyDo::Step(false)));
    let outside = handlers.press.clone();
    let dialog = div()
        .id("confirmation")
        .debug_selector(|| "confirmation".into())
        .key_context(CONTEXT)
        .track_focus(focus.dialog())
        .on_action(cx.listener(move |this, _: &AlertEnter, window, cx| enter(this, window, cx)))
        .on_action(cx.listener(move |this, _: &AlertSpace, window, cx| space(this, window, cx)))
        .on_action(cx.listener(move |this, _: &AlertEscape, window, cx| escape(this, window, cx)))
        .on_action(cx.listener(move |this, _: &AlertNext, window, cx| next(this, window, cx)))
        .on_action(
            cx.listener(move |this, _: &AlertPrevious, window, cx| previous(this, window, cx)),
        )
        // A click outside it answers that the user did not confirm, and is
        // consumed: nothing under the dimmer is clicked.
        .on_mouse_down_out(cx.listener(move |this, _: &MouseDownEvent, window, cx| {
            outside(this, Press::Outside, window, cx);
            cx.stop_propagation();
        }))
        .w(px(WIDTH))
        .max_w(gpui::relative(1.))
        .max_h(gpui::relative(1.))
        .flex()
        .flex_col()
        .occlude()
        .rounded(theme.geometry.popover_radius)
        .shadow(popover_shadows(theme))
        .role(Role::AlertDialog)
        .aria_modal(true)
        .aria_label(alert.title.clone())
        .when(!description.is_empty(), |dialog| {
            dialog.aria_description(description)
        })
        .child(material.popover(theme, content).min_h(px(0.)));
    div()
        .id("confirmation-layer")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.actions_dimmer)
        .occlude()
        // Over the panel, outside the window's content: its family and ink
        // are set here.
        .font_family(theme.typography.family.clone())
        .font_features(theme.typography.features.clone())
        .text_color(theme.text_title)
        .child(dialog)
        .into_any_element()
}

/// Where the launcher window keeps the hover of `part`'s button, which
/// fades as the footer's buttons do (#245).
fn hover_spot(part: Part) -> Spot {
    Spot::Button(match part {
        Part::Primary => "confirmation-primary",
        Part::Secondary => "confirmation-secondary",
        Part::Dismiss | Part::Option => "confirmation-dismiss",
    })
}

/// What a key on the alert does.
enum KeyDo {
    /// Presses a part.
    Press(Part),
    /// Moves the keyboard to the next part (`true`) or the previous one.
    Step(bool),
}

/// What the launcher window's alert shows: a command's confirmation (#146)
/// by its id, or Pane's own question by its title.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Shown {
    Command(u64),
    Question(String),
}

/// The window's side of the alert on display.
pub(crate) struct ConfirmationControls {
    /// The alert's focus.
    focus: AlertFocus,
    /// What the window shows, as it last synced.
    shown: Option<Shown>,
    /// Whether the checkbox is ticked: "Don't ask again", or a question's.
    ticked: bool,
    /// What had the focus when a command's confirmation took it, given back
    /// once it is answered.
    restore: Option<FocusHandle>,
    /// Whether the window showed itself for the confirmation and has not
    /// been active since: the deactivation its own hiding caused may still
    /// be on its way, and must not answer the confirmation shown since.
    awaiting_activation: bool,
}

impl ConfirmationControls {
    pub(crate) fn new(cx: &mut App) -> ConfirmationControls {
        ConfirmationControls {
            focus: AlertFocus::new(cx),
            shown: None,
            ticked: false,
            restore: None,
            awaiting_activation: false,
        }
    }
}

impl LauncherWindow {
    /// What the alert shows now: the launcher's confirmation, else its
    /// question, if it asks one.
    fn alert_shown(&self) -> Option<Shown> {
        if let Some(asked) = self.launcher.confirmation() {
            return Some(Shown::Command(asked.id));
        }
        let view = self.launcher.view();
        matches!(view.screen, Screen::Confirm { .. }).then_some(Shown::Question(view.title))
    }

    /// Follows the launcher's confirmation and question: one asked is drawn
    /// with the focus — the window shown first for a command's confirmation
    /// if it is hidden; one answered or gone gives the focus back. The
    /// window's request for a confirmation (`WindowRequest::Confirmation`)
    /// and every screen sync come here.
    pub(crate) fn sync_confirmation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.alert_shown() {
            Some(shown) => {
                let command = matches!(shown, Shown::Command(_));
                if command && self.show_for_confirmation(window, cx) {
                    self.confirmation.awaiting_activation = true;
                }
                if self.confirmation.shown.as_ref() != Some(&shown) {
                    self.confirmation.shown = Some(shown);
                    self.confirmation.ticked = false;
                    // Nothing else stays open over it.
                    self.close_actions(window, cx);
                    self.close_open_menu(window, cx);
                }
                if !self.confirmation.focus.holds(window) {
                    // A question gives the keyboard to the screen it leaves
                    // for, not to what had it before.
                    if command && self.confirmation.restore.is_none() {
                        self.confirmation.restore = window.focused(cx);
                    }
                    window.focus(self.confirmation.focus.dialog(), cx);
                }
            }
            None => {
                if self.confirmation.shown.take().is_some() {
                    self.confirmation.ticked = false;
                    self.give_confirmation_focus_back(window, cx);
                }
            }
        }
        cx.notify();
    }

    /// The window became active (`active`) or lost the focus: whether a
    /// loss counts for the confirmation shown. One the window showed itself
    /// for counts only once the window has been active since.
    pub(crate) fn confirmation_sees_activation(&mut self, active: bool) -> bool {
        if active {
            self.confirmation.awaiting_activation = false;
            return false;
        }
        !(self.confirmation.awaiting_activation && self.launcher.confirmation().is_some())
    }

    /// Gives the focus the alert took back: to what had it, else to the
    /// search field or the list.
    fn give_confirmation_focus_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restore = self.confirmation.restore.take();
        if !self.confirmation.focus.holds(window) {
            return;
        }
        match restore {
            Some(restore) => window.focus(&restore, cx),
            None if self.launcher.screen().search_field().is_some() => self.query.focus(window, cx),
            None => window.focus(&self.focus_handle, cx),
        }
    }

    /// Answers the command's confirmation on display with `answer`, "Don't
    /// ask again" as it is ticked, and gives the focus back.
    pub(crate) fn answer_confirmation(
        &mut self,
        answer: ConfirmAnswer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(asked) = self.launcher.confirmation() else {
            return;
        };
        let remember = self.confirmation.ticked && asked.rememberable;
        self.launcher
            .answer_confirmation(asked.id, answer, remember);
        self.sync_confirmation(window, cx);
    }

    /// Answers the question on display as `answer` says, its checkbox as
    /// it is ticked, and follows what that does.
    fn answer_question(&mut self, answer: Part, window: &mut Window, cx: &mut Context<Self>) {
        let answer = match answer {
            Part::Primary => QuestionAnswer::Primary {
                option: self.confirmation.ticked,
            },
            Part::Secondary => QuestionAnswer::Secondary,
            Part::Dismiss => QuestionAnswer::Cancel,
            Part::Option => {
                self.confirmation.ticked = !self.confirmation.ticked;
                cx.notify();
                return;
            }
        };
        let pending = self.launcher.answer_question(answer);
        self.motion.land_at_once();
        self.show_until_done(pending, window, cx);
    }

    /// A press on the alert: on a command's confirmation, the primary
    /// button confirms, the dismiss button dismisses, a click outside
    /// leaves, the checkbox ticks "Don't ask again"; on a question, as
    /// [`LauncherWindow::answer_question`] answers, a click outside being
    /// Cancel.
    fn press_alert(&mut self, press: Press, window: &mut Window, cx: &mut Context<Self>) {
        if self.launcher.confirmation().is_some() {
            match press {
                Press::Part(Part::Primary) => {
                    self.answer_confirmation(ConfirmAnswer::Confirmed, window, cx)
                }
                Press::Part(Part::Dismiss | Part::Secondary) => {
                    self.answer_confirmation(ConfirmAnswer::Dismissed, window, cx)
                }
                Press::Outside => self.answer_confirmation(ConfirmAnswer::Left, window, cx),
                Press::Part(Part::Option) => {
                    if self
                        .launcher
                        .confirmation()
                        .is_some_and(|asked| asked.rememberable)
                    {
                        self.confirmation.ticked = !self.confirmation.ticked;
                        cx.notify();
                    }
                }
            }
            return;
        }
        match press {
            Press::Part(part) => self.answer_question(part, window, cx),
            Press::Outside => self.answer_question(Part::Dismiss, window, cx),
        }
    }

    /// Test support: the command's confirmation the window draws, and
    /// whether "Don't ask again" is ticked on it. Test and debug builds
    /// only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn confirmation_drawn(&self) -> Option<(u64, bool)> {
        match self.confirmation.shown {
            Some(Shown::Command(id)) => Some((id, self.confirmation.ticked)),
            _ => None,
        }
    }

    /// The alert over the launcher, if one waits: a command's confirmation,
    /// else the question `question` asks (the launcher's view, on a
    /// [`Screen::Confirm`]).
    pub(crate) fn render_confirmation_layer(
        &self,
        question: Option<&LauncherView>,
        theme: &Theme,
        material: Material,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let ticked = self.confirmation.ticked;
        let alert = match self.launcher.confirmation() {
            Some(asked) => Alert {
                title: asked.title.clone().into(),
                message: asked.message.clone().into_iter().map(Into::into).collect(),
                primary: asked.primary.clone().into(),
                destructive: asked.destructive,
                secondary: None,
                dismiss: asked.dismiss.clone().into(),
                option: asked.rememberable.then(|| AlertOption {
                    label: DONT_ASK_AGAIN.into(),
                    ticked,
                    selector: DONT_ASK_AGAIN_SELECTOR,
                }),
                icon: None,
                hover: [0.; 4],
            },
            None => Alert::of_question(question?, ticked)?,
        };
        let now = cx.background_executor().now();
        let alert = alert.washed(|part| self.motion.hover.look(hover_spot(part), now));
        let handlers = Handlers {
            press: Rc::new(|this: &mut Self, press, window, cx| {
                this.press_alert(press, window, cx)
            }),
            hover: Rc::new(|this: &mut Self, part, over, cx| {
                this.motion.hover.set(hover_spot(part), over, cx);
            }),
        };
        Some(alert_layer(
            alert,
            &self.confirmation.focus,
            handlers,
            theme,
            material,
            window,
            cx,
        ))
    }
}
