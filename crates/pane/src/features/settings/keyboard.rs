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
    AnyElement, App, Context, Div, FocusHandle, Hsla, KeyBinding, KeyDownEvent, MouseDownEvent,
    Role, ScrollAnchor, Stateful, Window, actions, div, prelude::*, px,
};
use pane_core::{Binding, Keyboard, KeyboardAction, Launcher};

use super::{Page, SettingsWindow, search};
use crate::ui::icon::{Glyph, IconTone};
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

/// The Keyboard page, registered after Shortcuts in the window's page
/// list, as the reference's sections order it.
pub(crate) fn page() -> Page {
    Page {
        title: "Keyboard",
        about: "The in-app navigation bindings of Pane's own windows",
        icon: (IconTone::Command, Glyph::Keyboard),
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

/// Draws the Keyboard page: the navigation actions, each with its
/// binding, and what the last attempt or save reported.
fn render(
    this: &mut SettingsWindow,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let theme = crate::settings::visuals(cx).theme;
    let typography = &theme.typography;
    // Everything the page shows about the bindings comes from the host
    // settings: the choices, and what a save reported.
    let (keyboard, status) = {
        let settings = crate::settings::shared(cx).read(cx);
        (settings.keyboard(), settings.status())
    };
    let rejection = this.keyboard.rejection.clone();
    let recording = this.keyboard.recording;
    let defaults = Keyboard::default_for_this_system();

    let mut rows = Vec::new();
    for action in KeyboardAction::ALL {
        let binding = keyboard.binding(action).clone();
        // The row's scroll anchor, which the search's reveal scrolls to
        // (see the window's render).
        let anchor = this.search_anchor(action.id());
        rows.push(recorder_row(this, action, &binding, recording, anchor, cx));
        // The reset row appears only when there is something to reset: a
        // pointer-only recovery path back to the default, through the same
        // checks a recording takes.
        if !keyboard.is_default(action) {
            rows.push(reset_row(action, defaults.binding(action).clone(), cx));
        }
    }

    let page = div()
        .id("keyboard")
        .debug_selector(|| "keyboard".into())
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .id("keyboard-title")
                .debug_selector(|| "keyboard-title".into())
                .pb(px(8.))
                .text_size(typography.search_size)
                .font_weight(typography.medium)
                .text_color(theme.text_title)
                .child("Keyboard"),
        )
        .child(group("In-app navigation", rows, &theme))
        .child(
            div()
                .id("keyboard-note")
                .debug_selector(|| "keyboard-note".into())
                .pt(px(6.))
                .text_size(typography.row_subtitle_size)
                .text_color(theme.text_muted)
                .child(
                    "These keys move through Pane's own windows. Text editing and \
                     composition stay owned by the field you type in.",
                ),
        )
        // What the last attempt was refused with, if anything.
        .when_some(rejection, |page, rejection| {
            page.child(note("keyboard-refusal", &rejection, theme.danger, &theme))
        })
        // What a save reported, if it failed — the same status the other
        // pages show for their own choices.
        .when_some(status, |page, status| {
            page.child(note("keyboard-status", &status, theme.danger, &theme))
        });
    page.into_any_element()
}

/// One labelled group of rows, as the other pages' groups.
fn group(label: &'static str, rows: Vec<Stateful<Div>>, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .pb(px(4.))
                .text_size(theme.typography.row_kind_size)
                .font_weight(theme.typography.medium)
                .text_color(theme.text_muted)
                .child(label),
        )
        .children(rows)
}

/// `action`'s recorder row: a button that shows the binding in effect
/// and, clicked or pressed with Enter, listens for the keys of the next
/// one. While it listens it holds focus, shows what it is doing, and
/// takes the keys pressed as the binding being recorded (Escape
/// cancels); a combination that is refused keeps it listening for
/// another try.
fn recorder_row(
    this: &SettingsWindow,
    action: KeyboardAction,
    binding: &Binding,
    recording: Option<KeyboardAction>,
    anchor: ScrollAnchor,
    cx: &mut Context<SettingsWindow>,
) -> Stateful<Div> {
    let theme = crate::settings::visuals(cx).theme;
    let typography = &theme.typography;
    let geometry = &theme.geometry;
    let listening = recording == Some(action);
    let defaults = Keyboard::default_for_this_system();
    let default = defaults.binding(action);
    let subtitle: String = if listening {
        "The keys are captured here: they do not act".into()
    } else if binding == default {
        format!("{} — the default", action.does())
    } else {
        action.does().to_owned()
    };
    let label = format!(
        "{}{} with {}",
        if listening { "Recording; " } else { "" },
        action.title(),
        binding
    );
    let focus = this
        .keyboard
        .focuses
        .get(&action)
        .expect("every action has a row focus")
        .clone();
    div()
        .id(action.id())
        .debug_selector(move || format!("keyboard-{}", action.id()))
        .flex()
        .items_center()
        .gap(geometry.row_gap)
        .min_h(geometry.row_min_height)
        .px(geometry.row_padding_x)
        .rounded(geometry.row_radius)
        .cursor_pointer()
        .hover(|row| row.bg(theme.row_hover))
        // Pressed: the selected wash, one rung above the hover one.
        .active(|row| row.bg(theme.row_selected))
        .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(typography.row_title_size)
                        .font_weight(typography.medium)
                        .text_color(theme.text_title)
                        .child(action.title()),
                )
                .child(
                    div()
                        .text_size(typography.row_subtitle_size)
                        .text_color(theme.text_muted)
                        .child(subtitle.clone()),
                ),
        )
        .child(binding_chip(binding, listening, &theme))
        .anchor_scroll(Some(anchor))
        .key_context(RECORDER)
        .track_focus(&focus)
        .role(Role::Button)
        .aria_label(label)
        .aria_description(subtitle)
        .on_action(cx.listener(move |this, _: &ActivateRecorder, window, cx| {
            this.keyboard_activate_recorder(action, window, cx);
        }))
        .on_action(cx.listener(move |this, _: &CancelRecording, window, cx| {
            this.keyboard_cancel_recording(window, cx);
        }))
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            this.keyboard_key_down(event, window, cx);
        }))
        // A mouse-down anywhere outside the row while it listens cancels
        // the recording and is consumed, as the footer menu's popup does:
        // the click underneath does not act, and the recorder gives up
        // the keys.
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

/// The binding the row shows: the shortcut as the user names it, in the
/// keycap chrome — or, while recording, the listening mark.
fn binding_chip(binding: &Binding, recording: bool, theme: &Theme) -> Stateful<Div> {
    let typography = &theme.typography;
    let geometry = &theme.geometry;
    div()
        .id("keyboard-binding")
        .debug_selector(|| "keyboard-binding".into())
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .h(geometry.keycap_height)
        .px(geometry.keycap_padding_x)
        .rounded(geometry.keycap_radius)
        .bg(theme.tile_background)
        .text_size(typography.row_title_size)
        .font_weight(typography.medium)
        .text_color(if recording {
            theme.warning
        } else {
            theme.text_title
        })
        .child(if recording {
            "…".to_owned()
        } else {
            binding.to_string()
        })
}

/// `action`'s reset row, shown when its binding is not the default:
/// pointer-only, back through the same checks a recording takes, so a
/// reset that would land on another action's binding is refused with the
/// same explanation beside the row.
fn reset_row(
    action: KeyboardAction,
    default: Binding,
    cx: &mut Context<SettingsWindow>,
) -> Stateful<Div> {
    let theme = crate::settings::visuals(cx).theme;
    let typography = &theme.typography;
    let geometry = &theme.geometry;
    let row = div()
        .flex()
        .items_center()
        .gap(geometry.row_gap)
        .min_h(geometry.row_min_height)
        .px(geometry.row_padding_x)
        .rounded(geometry.row_radius)
        .cursor_pointer()
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(typography.row_title_size)
                        .font_weight(typography.medium)
                        .text_color(theme.text_title)
                        .child("Reset"),
                )
                .child(
                    div()
                        .text_size(typography.row_subtitle_size)
                        .text_color(theme.text_muted)
                        .child(format!("Back to {default}, the default")),
                ),
        );
    row.id(format!("keyboard-reset-{}", action.id()))
        // The pointer feedback, on the named row: the hover wash fades
        // over the shared pointer span, and the press takes the selected
        // wash, one rung above the hover one.
        .hover(|row| row.bg(theme.row_hover))
        .active(|row| row.bg(theme.row_selected))
        .transitions(|fades| fades.bg(crate::ui::motion::pointer_fade()))
        .debug_selector(move || format!("keyboard-reset-{}", action.id()))
        .role(Role::Button)
        .aria_label(format!("Reset {} to {default}", action.title()))
        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            this.keyboard_apply(action, default.clone(), window, cx);
        }))
}

/// One explanatory line of the page: `text` in `color`, named for
/// assistive technology and drawn as a status.
fn note(selector: &'static str, text: &str, color: Hsla, theme: &Theme) -> Stateful<Div> {
    div()
        .id(selector)
        .debug_selector(move || selector.into())
        .pt(px(6.))
        .role(Role::Status)
        .aria_label(text.to_owned())
        .text_size(theme.typography.row_subtitle_size)
        .text_color(color)
        .child(text.to_owned())
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
