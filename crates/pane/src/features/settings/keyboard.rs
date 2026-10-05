//! The Settings window's Keyboard page: the bounded set of in-app
//! navigation actions, each with its binding, rebindable here.
//!
//! The actions are Pane's own — previous/next result, invoke selected
//! action, back, return to root, dismiss launcher and open Settings —
//! never an extension's and never the text-editing keys a focused field
//! owns (see [`pane_core::keyboard`], the renderer-independent set and
//! its rules). Their bindings live in the host settings, so a change
//! takes effect in every window at once and survives a restart.
//!
//! The recorder is the General page's, per action: a row whose click (or
//! Enter, while it is not listening) starts listening, and whose keys,
//! while it listens, are the binding being recorded — captured without
//! acting, so neither the sidebar's navigation nor the window's
//! traversal moves, and never the action being rebound. Escape cancels;
//! Enter and Space cannot be part of a binding, being the recorder's own
//! activation, so they are the defaults' privilege and a reset's to
//! restore. A captured combination is checked — protected for a focused
//! field, and against the other actions of the set, whose contexts
//! overlap in the launcher's window — applied through the host settings
//! (which re-make the keymap before saving), and then saved; a refusal
//! is explained beside the row and keeps the recorder listening.
//!
//! Per action, a reset row appears whenever its binding is not the
//! default: pointer-only, like the General page's, so recovery from a
//! binding that does not suit the keyboard in front of the user never
//! needs the very keys being rebound. A reset goes through the same
//! checks as a recording, so it cannot land on another action's keys.

use std::collections::BTreeMap;

use gpui::{
    AnyElement, App, Context, Div, FocusHandle, KeyBinding, KeyDownEvent, MouseDownEvent, Role,
    ScrollAnchor, SharedString, Stateful, Window, actions, div, prelude::*,
};
use pane_core::{Binding, Keyboard, KeyboardAction, Launcher};

use super::{Page, SettingsWindow, search};
use crate::ui::controls::{self, status_note as note};
use crate::ui::icon::Glyph;
use crate::ui::keycap::{CapStyle, KeySequence, key_sequence};
use crate::ui::settings_shell;
use crate::ui::theme::Theme;

/// The recorder rows' key context: while a recorder holds focus, its keys
/// are the binding being recorded, not the window's navigation.
const RECORDER: &str = "KeyboardRecorder";

actions!(keyboard, [ActivateRecorder, CancelRecording]);

/// Registers the recorder rows' key bindings, in the recorder's own
/// context — deeper in the focus stack than the sidebar's and the
/// window's keys, so while a recorder holds focus they never fall
/// through. The navigation and traversal keys are bound to
/// [`gpui::NoAction`] there: the sidebar stays put while keys are
/// captured, and everything else reaches the recorder's own key handler
/// as the combination being recorded.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        // The rows are buttons: Enter and Space activate them, as a click
        // does — and while one listens, they do nothing: the keys stay
        // captured, and Enter and Space cannot be part of a binding.
        KeyBinding::new("enter", ActivateRecorder, Some(RECORDER)),
        KeyBinding::new("space", ActivateRecorder, Some(RECORDER)),
        // Escape cancels recording.
        KeyBinding::new("escape", CancelRecording, Some(RECORDER)),
        // Swallowed while a recorder holds focus (and harmless when none
        // does): the sidebar's navigation and Tab's traversal stay put,
        // so captured keys never move the page.
        KeyBinding::new("down", gpui::NoAction, Some(RECORDER)),
        KeyBinding::new("up", gpui::NoAction, Some(RECORDER)),
        KeyBinding::new("tab", gpui::NoAction, Some(RECORDER)),
        KeyBinding::new("shift-tab", gpui::NoAction, Some(RECORDER)),
        // The window's close shortcut is swallowed there too: recording
        // captures keys without executing them, so pressing the dismiss
        // binding's own default (Cmd+W / Ctrl+W) records it instead of
        // closing the window the recorder lives in.
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-w"
            } else {
                "ctrl-w"
            },
            gpui::NoAction,
            Some(RECORDER),
        ),
    ]);
}

/// The page's heading.
pub(crate) const TITLE: &str = "Keyboard";

/// What the page is, in one line: its sidebar entry's description in
/// the search, and its heading's subtitle.
pub(crate) const ABOUT: &str = "The in-app navigation bindings of Pane's own windows";

/// The note under the page's actions.
pub(crate) const NOTE: &str = "These keys move through Pane's own windows. Text editing and \
                                composition stay owned by the field you type in.";

/// The Keyboard page, registered after Shortcuts in the window's page
/// list, as the reference's sections order it.
pub(crate) fn page() -> Page {
    Page {
        title: "Keyboard",
        about: ABOUT,
        icon: Glyph::Keyboard,
        count: None,
        render,
        search: entries,
        focus,
    }
}

/// The settings the page offers the sidebar's search: each navigation
/// action of the bounded set, named as the page's row names it, in the
/// group it sits in. A binding that differs from the default is still
/// just the setting it is — the entry says the action, and the row shows
/// what it is bound to now. Read live, so a change is in the next
/// catalog as it lands.
fn entries(_launcher: &Launcher, _cx: &App) -> Vec<search::Entry> {
    KeyboardAction::ALL
        .into_iter()
        .map(|action| search::Entry {
            control: Some(action.id().into()),
            title: action.title().into(),
            group: Some("In-app navigation".into()),
            // A binding set to something the keyboard cannot use is the
            // page's own refusal to explain; the setting itself is never
            // unavailable here.
            unavailable: None,
        })
        .collect()
}

/// Each action's recorder row takes keyboard focus — the rows are tab
/// stops, and Enter on one starts recording — so a jump to an action
/// focuses its row, ready to rebind. Anything else is not this page's:
/// `false` falls back to the sidebar.
fn focus(
    this: &mut SettingsWindow,
    target: &str,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> bool {
    let Some(action) = KeyboardAction::of(target) else {
        return false;
    };
    let Some(focus) = this.keyboard.focuses.get(&action) else {
        return false;
    };
    window.focus(focus, cx);
    true
}

/// The Keyboard page's state, held by the window as a field: the
/// recorder.
pub(crate) struct State {
    /// The action whose binding is being recorded, if any.
    recording: Option<KeyboardAction>,
    /// Why the last attempt was refused, if it was: a protected key, a
    /// collision, or what a save reported. Shown as the page's status; the
    /// recorder keeps listening for another try.
    rejection: Option<String>,
    /// Each action's recorder row focus: a tab stop, so the keyboard
    /// reaches every row; the recording action's holds focus while it
    /// listens.
    focuses: BTreeMap<KeyboardAction, FocusHandle>,
}

impl State {
    /// The page's state, over the window's `cx` (its focus handles).
    pub(crate) fn new(cx: &mut Context<SettingsWindow>) -> State {
        let focuses = KeyboardAction::ALL
            .into_iter()
            .map(|action| {
                let focus = cx.focus_handle().tab_stop(true);
                (action, focus)
            })
            .collect();
        State {
            recording: None,
            rejection: None,
            focuses,
        }
    }
}

/// Which of the Keyboard page's controls an element is, for the caller of
/// [`compose`] that attaches its behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyboardControl {
    /// The action's recorder.
    Recorder(KeyboardAction),
    /// The action's Reset button, shown while its binding is not the
    /// default.
    Reset(KeyboardAction),
}

/// One action of the page, as plain values.
pub(crate) struct KeyboardRow {
    pub(crate) action: KeyboardAction,
    /// The binding's caps, from the launcher's binding adapter
    /// (`crate::keyboard::binding_keys`), and the binding as the user
    /// names it.
    pub(crate) keys: KeySequence,
    pub(crate) binding: String,
    /// The default binding, named, while the binding is not the default
    /// (Reset is offered); `None` at the default.
    pub(crate) default: Option<String>,
}

/// What the Keyboard page shows, as plain values: what [`render`] reads
/// from the host settings, and what the visual workbench's fixture
/// supplies to draw the same page (#99).
pub(crate) struct KeyboardView {
    pub(crate) rows: Vec<KeyboardRow>,
    /// The action whose recorder is listening, if any.
    pub(crate) recording: Option<KeyboardAction>,
    /// What the last attempt was refused with, and what a save reported.
    pub(crate) rejection: Option<String>,
    pub(crate) status: Option<String>,
}

/// The rows of the bounded set of actions, in the set's order, as
/// `keyboard` binds them.
pub(crate) fn rows(keyboard: &Keyboard) -> Vec<KeyboardRow> {
    let defaults = Keyboard::default_for_this_system();
    KeyboardAction::ALL
        .into_iter()
        .map(|action| {
            let binding = keyboard.binding(action);
            KeyboardRow {
                action,
                keys: crate::keyboard::binding_keys(binding),
                binding: binding.to_string(),
                default: (!keyboard.is_default(action))
                    .then(|| defaults.binding(action).to_string()),
            }
        })
        .collect()
}

/// Draws the Keyboard page: the navigation actions, each with its
/// binding, and what the last attempt or save reported.
fn render(
    this: &mut SettingsWindow,
    window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let theme = crate::settings::visuals(cx).theme;
    // Everything the page shows about the bindings comes from the host
    // settings: the choices, and what a save reported.
    let (keyboard, status) = {
        let settings = crate::settings::shared(cx).read(cx);
        (settings.keyboard(), settings.status())
    };
    let view = KeyboardView {
        rows: rows(&keyboard),
        recording: this.keyboard.recording,
        rejection: this.keyboard.rejection.clone(),
        status,
    };
    // Each row's scroll anchor, which the search's reveal scrolls to (see
    // the window's render), and each recorder's focus.
    let anchors: BTreeMap<KeyboardAction, ScrollAnchor> = KeyboardAction::ALL
        .into_iter()
        .map(|action| (action, this.search_anchor(action.id())))
        .collect();
    let focuses = this.keyboard.focuses.clone();
    let focused = focuses
        .iter()
        .find(|(_, focus)| focus.is_focused(window))
        .map(|(&action, _)| action);
    let defaults = Keyboard::default_for_this_system();
    compose(&view, focused, &theme, |control, element| match control {
        KeyboardControl::Recorder(action) => {
            let focus = focuses
                .get(&action)
                .expect("every action has a row focus")
                .clone();
            element
                .anchor_scroll(anchors.get(&action).cloned())
                .key_context(RECORDER)
                .track_focus(&focus)
                .on_action(cx.listener(move |this, _: &ActivateRecorder, window, cx| {
                    this.keyboard_activate_recorder(action, window, cx);
                }))
                .on_action(cx.listener(move |this, _: &CancelRecording, window, cx| {
                    this.keyboard_cancel_recording(window, cx);
                }))
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    this.keyboard_key_down(event, window, cx);
                }))
                // A mouse-down anywhere outside the recorder while it
                // listens cancels the recording and is consumed, as the
                // footer menu's popup does: the click underneath does not
                // act, and the recorder gives up the keys.
                .on_mouse_down_out(cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                    if this.keyboard.recording.is_some() {
                        this.keyboard_cancel_recording(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                    this.keyboard_activate_recorder(action, window, cx);
                }))
        }
        KeyboardControl::Reset(action) => {
            let default = defaults.binding(action).clone();
            element.on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                this.keyboard_apply(action, default.clone(), window, cx);
            }))
        }
    })
    .into_any_element()
}

/// What an action's row says under its name: that the keys are captured
/// while its recorder listens; what the action does otherwise, and, at its
/// default, that it is.
pub(crate) fn row_description(row: &KeyboardRow, listening: bool) -> String {
    if listening {
        controls::RECORDING_HINT.into()
    } else if row.default.is_none() {
        format!("{} — the default", row.action.does())
    } else {
        row.action.does().to_owned()
    }
}

/// The Keyboard page's composition, which the visual workbench's fixture
/// draws too: the heading block, then the "In-app navigation" field group
/// — a settings row per action, its recorder's well (and, away from the
/// default, Reset beside it) at its end — with the note under it, and what
/// the last attempt or save reported. `focused` is the action whose
/// recorder has the keyboard (its well's ring). `attach` adds each
/// control's behavior; the composition gives each its identity, its
/// accessibility and its look.
pub(crate) fn compose(
    view: &KeyboardView,
    focused: Option<KeyboardAction>,
    theme: &Theme,
    attach: impl Fn(KeyboardControl, Stateful<Div>) -> Stateful<Div>,
) -> Stateful<Div> {
    let rows = view.rows.iter().map(|row| {
        let listening = view.recording == Some(row.action);
        recorder_row(row, listening, focused == Some(row.action), theme, &attach)
    });
    let field = controls::field(theme)
        .debug_selector(|| "keyboard-field".into())
        .child(controls::field_label("In-app navigation", theme))
        .child(controls::setting_list().children(rows))
        .child(
            controls::field_description(NOTE, theme.text_muted, theme)
                .id("keyboard-note")
                .debug_selector(|| "keyboard-note".into()),
        )
        // What the last attempt was refused with, if anything.
        .children(
            view.rejection
                .as_ref()
                .map(|rejection| note("keyboard-refusal", rejection.clone(), theme.danger, theme)),
        );
    let column = controls::column(theme)
        .child(
            settings_shell::page_header(TITLE, Some(ABOUT.into()), theme)
                .id("keyboard-title")
                .debug_selector(|| "keyboard-title".into()),
        )
        .child(field)
        // What a save reported, if it failed — the same status the other
        // pages show for their own choices.
        .children(
            view.status
                .as_ref()
                .map(|status| note("keyboard-status", status.clone(), theme.danger, theme)),
        );
    div()
        .id("keyboard")
        .debug_selector(|| "keyboard".into())
        .child(column)
}

/// An action's settings row: its title and what it does at the left, and
/// at its right end its recorder — a well showing the binding in effect as
/// its caps or, while it listens, the listening mark — with Reset beside
/// it while the binding is not the default. The recorder is a button:
/// clicked or pressed with Enter it listens for the keys of the next
/// binding, holding focus, and takes the keys pressed as the binding being
/// recorded (Escape cancels); a combination that is refused keeps it
/// listening for another try. Reset is pointer-only, back through the same
/// checks a recording takes, so a reset that would land on another
/// action's binding is refused with the same explanation.
fn recorder_row(
    row: &KeyboardRow,
    listening: bool,
    focused: bool,
    theme: &Theme,
    attach: &impl Fn(KeyboardControl, Stateful<Div>) -> Stateful<Div>,
) -> Div {
    let action = row.action;
    let subtitle = row_description(row, listening);
    let label = format!(
        "{}{} with {}",
        if listening { "Recording; " } else { "" },
        action.title(),
        row.binding
    );
    let shown = if listening {
        controls::listening_mark(theme).into_any_element()
    } else {
        key_sequence(&row.keys, CapStyle::Regular, theme).into_any_element()
    };
    let recorder = controls::recorder_well(
        div()
            .id("keyboard-binding")
            .debug_selector(move || format!("keyboard-binding-{}", action.id()))
            .flex()
            .child(shown),
        focused,
        theme,
    )
    .id(action.id())
    .debug_selector(move || format!("keyboard-{}", action.id()))
    .role(Role::Button)
    .aria_label(label)
    .aria_description(subtitle.clone());
    let reset = row.default.as_ref().map(|default| {
        let reset = controls::ghost_button("Reset", true, theme)
            .id(SharedString::from(format!(
                "keyboard-reset-{}",
                action.id()
            )))
            .debug_selector(move || format!("keyboard-reset-{}", action.id()))
            .role(Role::Button)
            .aria_label(format!("Reset {} to {default}", action.title()))
            .aria_description(format!("Back to {default}, the default"));
        attach(KeyboardControl::Reset(action), reset)
    });
    let description = controls::field_description(subtitle, theme.text_muted, theme);
    controls::setting_row(action.title(), vec![description.into_any_element()], theme)
        .debug_selector(move || format!("keyboard-row-{}", action.id()))
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(theme.geometry.controls.button_gap)
                .children(reset)
                .child(attach(KeyboardControl::Recorder(action), recorder)),
        )
}

impl SettingsWindow {
    /// A recorder row's activation: Enter, Space or a click on its row.
    /// With no recording in progress it starts listening for that row's
    /// action; while one listens the keys are captured, so this does
    /// nothing — Enter and Space cannot be part of a binding, and the
    /// recorder keeps listening.
    fn keyboard_activate_recorder(
        &mut self,
        action: KeyboardAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if self.keyboard.recording.is_none() {
            self.keyboard.recording = Some(action);
            self.keyboard.rejection = None;
            let focus = self
                .keyboard
                .focuses
                .get(&action)
                .expect("every action has a row focus")
                .clone();
            window.focus(&focus, cx);
            cx.notify();
        }
    }

    /// Escape on a recorder row, or a mouse-down outside one: cancels
    /// recording, changing nothing.
    fn keyboard_cancel_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if self.keyboard.recording.take().is_some() {
            self.keyboard.rejection = None;
            let sidebar = self.focus.clone();
            window.focus(&sidebar, cx);
            cx.notify();
        }
    }

    /// A key pressed while a recorder listens: the combination it names is
    /// the binding to record. The keys the recorder handles itself —
    /// Enter, Space and Escape for its activation and cancellation — never
    /// reach here; the sidebar's and traversal keys are swallowed in the
    /// recorder's context, and a key no rule refuses records.
    fn keyboard_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(action) = self.keyboard.recording else {
            return;
        };
        cx.stop_propagation();
        match crate::keyboard::binding_of(&event.keystroke) {
            Ok(binding) => self.keyboard_apply(action, binding, window, cx),
            Err(problem) => {
                self.keyboard.rejection = Some(format!("{problem}."));
                cx.notify();
            }
        }
    }

    /// Applies `binding` as `action`'s: through the host settings, which
    /// re-make every window's key bindings before keeping and saving the
    /// choice. A refusal leaves everything as it was; the reason is the
    /// page's status, and the recorder — if one is listening — keeps
    /// listening for another try.
    fn keyboard_apply(
        &mut self,
        action: KeyboardAction,
        binding: Binding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let applied = crate::settings::shared(cx).update(cx, |settings, cx| {
            settings.set_keyboard(action, binding, cx)
        });
        match applied {
            Ok(()) => {
                // The change landed: the recorder is done, and focus
                // returns to the sidebar.
                self.keyboard.recording = None;
                self.keyboard.rejection = None;
                let sidebar = self.focus.clone();
                window.focus(&sidebar, cx);
                cx.notify();
            }
            Err(reason) => {
                self.keyboard.rejection = Some(reason);
                cx.notify();
            }
        }
    }
}
