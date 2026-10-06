//! A confirmation a command asks for before it does something it cannot
//! undo (#146, `feedback.confirm`): drawn over the launcher's current
//! screen, one at a time, as the launcher keeps it
//! ([`pane_core::Launcher::confirmation`]).
//!
//! The confirmation is a small dialog in the middle of the launcher, over a
//! dimmer that takes every click: its title, its message, the primary
//! button (Enter; drawn in the destructive color when the command asks) and
//! the dismiss button (Escape; "Cancel" unless the command named another).
//! When the command gave a key to remember the answer by, it offers "Don't
//! ask again", which Space ticks (or a click): answered with a button while
//! it is ticked, the answer is remembered and later confirmations with the
//! same key are answered without asking.
//!
//! The dialog takes the focus while it is shown and gives it back once it
//! is answered; Tab does not leave it. A click outside it answers that the
//! user did not confirm (never remembered), and so does the window losing
//! the focus or hiding (the launcher's own rule). While the launcher is
//! hidden when a command asks (a no-view command run by its hotkey, or one
//! that closed the window first), the window shows itself first, on the
//! screen it was left on.
//!
//! Debug selectors: `confirmation`, `confirmation-title`,
//! `confirmation-message`, `confirmation-primary`,
//! `confirmation-destructive` (around a destructive primary button),
//! `confirmation-dismiss`, `confirmation-dont-ask-again` and
//! `confirmation-dont-ask-again-ticked`.

use gpui::{
    AnyElement, App, ClickEvent, Context, FocusHandle, KeyBinding, MouseDownEvent, Role,
    SharedString, Toggled, Window, actions, div, prelude::*, px,
};
use pane_core::ConfirmAnswer;
use pane_core::feedback::DONT_ASK_AGAIN;

use crate::app::LauncherWindow;
use crate::features::actions_panel::DESTRUCTIVE;
use crate::ui::footer::{self, ButtonWash};
use crate::ui::keycap::{CapStyle, Key, KeySequence, key_sequence};
use crate::ui::material::{Material, popover_shadows};
use crate::ui::theme::Theme;

actions!(
    confirmation,
    [
        ConfirmChoice,
        DismissChoice,
        ToggleDontAskAgain,
        StayInConfirmation
    ]
);

/// The dialog's key context: its keys win over the launcher's confirm,
/// back and focus traversal.
pub(crate) const CONTEXT: &str = "Confirmation";

/// The dialog's width, in px.
const WIDTH: f32 = 360.;

/// Registers the dialog's keys: Enter confirms, Escape dismisses, Space
/// ticks "Don't ask again", and Tab stays in the dialog.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmChoice, Some(CONTEXT)),
        KeyBinding::new("escape", DismissChoice, Some(CONTEXT)),
        KeyBinding::new("space", ToggleDontAskAgain, Some(CONTEXT)),
        KeyBinding::new("tab", StayInConfirmation, Some(CONTEXT)),
        KeyBinding::new("shift-tab", StayInConfirmation, Some(CONTEXT)),
    ]);
}

/// The window's side of the confirmation on display.
pub(crate) struct ConfirmationControls {
    /// The dialog's focus, while it is shown.
    focus: FocusHandle,
    /// The id of the confirmation the window shows, as it last synced.
    shown: Option<u64>,
    /// Whether "Don't ask again" is ticked.
    dont_ask_again: bool,
    /// What had the focus when the dialog took it, given back once it is
    /// answered.
    restore: Option<FocusHandle>,
    /// Whether the window showed itself for the confirmation and has not
    /// been active since: the deactivation its own hiding caused may still
    /// be on its way, and must not answer the confirmation shown since.
    awaiting_activation: bool,
}

impl ConfirmationControls {
    pub(crate) fn new(cx: &mut App) -> ConfirmationControls {
        ConfirmationControls {
            focus: cx.focus_handle(),
            shown: None,
            dont_ask_again: false,
            restore: None,
            awaiting_activation: false,
        }
    }
}

/// A key sequence of the one key `cap`, named `name`.
fn one_key(cap: &str, name: &str) -> KeySequence {
    KeySequence {
        keys: vec![Key::new(cap.to_owned(), name.to_owned())],
    }
}

impl LauncherWindow {
    /// Follows the launcher's confirmation: one asked is drawn with the
    /// focus, the window shown first if it is hidden; one answered or gone
    /// gives the focus back. The window's request for a confirmation
    /// (`WindowRequest::Confirmation`) and every screen sync come here.
    pub(crate) fn sync_confirmation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.launcher.confirmation() {
            Some(asked) => {
                if self.show_for_confirmation(window, cx) {
                    self.confirmation.awaiting_activation = true;
                }
                if self.confirmation.shown != Some(asked.id) {
                    self.confirmation.shown = Some(asked.id);
                    self.confirmation.dont_ask_again = false;
                    // Nothing else stays open over it.
                    self.close_actions(window, cx);
                    self.close_open_menu(window, cx);
                }
                if !self.confirmation.focus.is_focused(window) {
                    if self.confirmation.restore.is_none() {
                        self.confirmation.restore = window.focused(cx);
                    }
                    window.focus(&self.confirmation.focus, cx);
                }
            }
            None => {
                if self.confirmation.shown.take().is_some() {
                    self.confirmation.dont_ask_again = false;
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

    /// Gives the focus the dialog took back: to what had it, else to the
    /// search field or the list.
    fn give_confirmation_focus_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restore = self.confirmation.restore.take();
        if !self.confirmation.focus.is_focused(window) {
            return;
        }
        match restore {
            Some(restore) => window.focus(&restore, cx),
            None if self.launcher.view().search_field().is_some() => self.query.focus(window, cx),
            None => window.focus(&self.focus_handle, cx),
        }
    }

    /// Answers the confirmation on display with `answer`, "Don't ask
    /// again" as it is ticked, and gives the focus back.
    pub(crate) fn answer_confirmation(
        &mut self,
        answer: ConfirmAnswer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(asked) = self.launcher.confirmation() else {
            return;
        };
        let remember = self.confirmation.dont_ask_again && asked.rememberable;
        self.launcher
            .answer_confirmation(asked.id, answer, remember);
        self.sync_confirmation(window, cx);
    }

    fn confirm_choice(&mut self, _: &ConfirmChoice, window: &mut Window, cx: &mut Context<Self>) {
        self.answer_confirmation(ConfirmAnswer::Confirmed, window, cx);
    }

    fn dismiss_choice(&mut self, _: &DismissChoice, window: &mut Window, cx: &mut Context<Self>) {
        self.answer_confirmation(ConfirmAnswer::Dismissed, window, cx);
    }

    fn toggle_dont_ask_again(
        &mut self,
        _: &ToggleDontAskAgain,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flip_dont_ask_again(cx);
    }

    /// Ticks "Don't ask again", or unticks it, where the confirmation on
    /// display offers it.
    fn flip_dont_ask_again(&mut self, cx: &mut Context<Self>) {
        if self
            .launcher
            .confirmation()
            .is_some_and(|asked| asked.rememberable)
        {
            self.confirmation.dont_ask_again = !self.confirmation.dont_ask_again;
            cx.notify();
        }
    }

    /// Tab and Shift+Tab: the focus stays in the dialog.
    fn stay_in_confirmation(
        &mut self,
        _: &StayInConfirmation,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }

    /// Test support: the confirmation the window draws, and whether "Don't
    /// ask again" is ticked on it. Test and debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn confirmation_drawn(&self) -> Option<(u64, bool)> {
        self.confirmation
            .shown
            .map(|id| (id, self.confirmation.dont_ask_again))
    }

    /// The confirmation on display over the launcher, if one waits: the
    /// dialog in the middle over a dimmer that takes every click.
    pub(crate) fn render_confirmation_layer(
        &self,
        theme: &Theme,
        material: Material,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let asked = self.launcher.confirmation()?;
        let ticked = self.confirmation.dont_ask_again;
        let primary_keys = one_key("↵", "Enter");
        let primary = footer::footer_button(
            "confirmation-primary",
            asked.primary.clone(),
            &primary_keys,
            if asked.destructive {
                CapStyle::Regular
            } else {
                CapStyle::Accent
            },
            ButtonWash::Hover,
            theme,
        )
        .when(asked.destructive, |button| {
            button
                .text_color(theme.danger)
                .aria_description(DESTRUCTIVE)
        })
        .role(Role::Button)
        .aria_label(asked.primary.clone())
        .aria_keyshortcuts(primary_keys.name())
        .cursor_pointer()
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            this.answer_confirmation(ConfirmAnswer::Confirmed, window, cx);
        }));
        // A destructive primary button is marked around it, for the tests
        // that find it: its own selector stays the button's.
        let primary = div()
            .flex_none()
            .when(asked.destructive, |wrapper| {
                wrapper.debug_selector(|| "confirmation-destructive".into())
            })
            .child(primary);
        let dismiss_keys = one_key("Esc", "Escape");
        let dismiss = footer::footer_button(
            "confirmation-dismiss",
            asked.dismiss.clone(),
            &dismiss_keys,
            CapStyle::Regular,
            ButtonWash::Hover,
            theme,
        )
        .role(Role::Button)
        .aria_label(asked.dismiss.clone())
        .aria_keyshortcuts(dismiss_keys.name())
        .cursor_pointer()
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            this.answer_confirmation(ConfirmAnswer::Dismissed, window, cx);
        }));
        let remember = asked.rememberable.then(|| {
            let mark_size = px(16.);
            let mark = div()
                .flex_none()
                .size(mark_size)
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
                    mark.debug_selector(|| "confirmation-dont-ask-again-ticked".into())
                        .bg(theme.accent)
                        .child(div().size(px(6.)).rounded(px(2.)).bg(theme.text_title))
                });
            let space = one_key("Space", "Space");
            div()
                .id("confirmation-dont-ask-again")
                .debug_selector(|| "confirmation-dont-ask-again".into())
                .flex()
                .items_center()
                .gap_2()
                .text_size(theme.typography.footer_size)
                .text_color(theme.text_body)
                .role(Role::CheckBox)
                .aria_label(DONT_ASK_AGAIN)
                .aria_toggled(if ticked {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .aria_keyshortcuts(space.name())
                .cursor_pointer()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.flip_dont_ask_again(cx);
                }))
                .child(mark)
                .child(div().flex_1().child(DONT_ASK_AGAIN))
                .child(key_sequence(&space, CapStyle::Regular, theme))
        });
        let title = SharedString::from(asked.title.clone());
        let content = div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child(
                div()
                    .debug_selector(|| "confirmation-title".into())
                    .text_size(theme.typography.row_title_size)
                    .font_weight(theme.typography.medium)
                    .text_color(theme.text_title)
                    .child(title.clone()),
            )
            .when_some(asked.message.clone(), |content, message| {
                content.child(
                    div()
                        .debug_selector(|| "confirmation-message".into())
                        .text_size(theme.typography.row_subtitle_size)
                        .text_color(theme.text_body)
                        .child(message),
                )
            })
            .children(remember)
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(dismiss)
                    .child(primary),
            );
        let dialog = div()
            .id("confirmation")
            .debug_selector(|| "confirmation".into())
            .key_context(CONTEXT)
            .track_focus(&self.confirmation.focus)
            .on_action(cx.listener(Self::confirm_choice))
            .on_action(cx.listener(Self::dismiss_choice))
            .on_action(cx.listener(Self::toggle_dont_ask_again))
            .on_action(cx.listener(Self::stay_in_confirmation))
            // A click outside it answers that the user did not confirm,
            // and is consumed: nothing under the dimmer is clicked.
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.answer_confirmation(ConfirmAnswer::Left, window, cx);
                cx.stop_propagation();
            }))
            .w(px(WIDTH))
            .max_w(gpui::relative(1.))
            .occlude()
            .rounded(theme.geometry.popover_radius)
            .shadow(popover_shadows(theme))
            .role(Role::AlertDialog)
            .aria_modal(true)
            .aria_label(title)
            .when_some(asked.message.clone(), |dialog, message| {
                dialog.aria_description(message)
            })
            .child(material.popover(theme, content));
        Some(
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
                // Over the panel, outside the launcher's content: its
                // family and ink are set here.
                .font_family(theme.typography.family.clone())
                .font_features(theme.typography.features.clone())
                .text_color(theme.text_title)
                .child(dialog)
                .into_any_element(),
        )
    }
}
