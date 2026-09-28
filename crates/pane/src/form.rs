//! The form screen: an extension's form, or Pane's own alias form, rendered
//! with standard controls.
//!
//! - A text field is GPUI CE's editable text element (typing, editing keys,
//!   clipboard, undo and input-method composition) inside a focusable
//!   `TextInput` node that carries the label, value, placeholder and error.
//! - A choice field is a `RadioGroup` of `RadioButton`s. The group holds focus
//!   and the chosen option is its active descendant; arrow keys change the
//!   choice, like a native radio group.
//! - The submit button is a focusable `Button`; Enter or Space presses it.
//!
//! Tab and Shift-Tab move through the controls in order. Enter anywhere on
//! the form submits it and Escape returns to the command. After a rejected
//! submission, focus moves to the rejected field.

use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, KeyBinding, Role, Subscription,
    Toggled, Window, actions, div, prelude::*, rgb,
};
use gpui_elements::editable_text::actions::{
    DEFAULT_INPUT_CONTEXT, Enter, Escape, Tab, default_bindings,
};
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged, text_input};
use pane_core::{FieldKind, FormField, FormView, Screen, Status};

use crate::LauncherWindow;

actions!(form, [NextChoice, PreviousChoice, Press]);

const CHOICE_CONTEXT: &str = "FormChoice";
const BUTTON_CONTEXT: &str = "FormButton";

/// Proof that the editing keys of every editable text element, the form's
/// text fields and root search's query field alike, are bound: see
/// [`bind_text_editing`].
pub(crate) struct TextEditingKeys(());

/// Registers GPUI CE's editing keys for every editable text element, once,
/// except Tab, Enter and Escape: those are left to bubble to the launcher
/// (focus traversal, confirm or submit, and back) instead of being text
/// edits. The form's and root search's key bindings take the result, since
/// both rely on it: without it their fields would not edit, and with Enter
/// or Escape bound as text edits the fields would swallow them.
pub(crate) fn bind_text_editing(cx: &mut App) -> TextEditingKeys {
    let text_editing = default_bindings()
        .as_keybindings(Some(DEFAULT_INPUT_CONTEXT))
        .filter(|binding| {
            let action = binding.action();
            !(action.partial_eq(&Tab) || action.partial_eq(&Enter) || action.partial_eq(&Escape))
        })
        .collect::<Vec<_>>();
    cx.bind_keys(text_editing);
    TextEditingKeys(())
}

/// Registers the form's key bindings; its text fields edit through the
/// shared editing keys.
pub(crate) fn bind_keys(cx: &mut App, _: &TextEditingKeys) {
    cx.bind_keys([
        KeyBinding::new("down", NextChoice, Some(CHOICE_CONTEXT)),
        KeyBinding::new("right", NextChoice, Some(CHOICE_CONTEXT)),
        KeyBinding::new("up", PreviousChoice, Some(CHOICE_CONTEXT)),
        KeyBinding::new("left", PreviousChoice, Some(CHOICE_CONTEXT)),
        KeyBinding::new("space", Press, Some(BUTTON_CONTEXT)),
    ]);
}

/// The focusable controls of the open form, in field order.
pub(crate) struct FormControls {
    fields: Vec<Control>,
    submit: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

enum Control {
    Text(Entity<EditableTextState>),
    Choice(FocusHandle),
}

impl Control {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            Control::Text(input) => input.focus_handle(cx),
            Control::Choice(handle) => handle.clone(),
        }
    }
}

impl LauncherWindow {
    /// Test support: the editing state of the open form's text field
    /// `field_id`, which a platform input method talks to while composing
    /// text. GPUI CE's test platform cannot reach the window's input handler,
    /// so the window tests compose through this instead.
    #[doc(hidden)]
    pub fn text_field(&self, field_id: &str) -> Option<Entity<EditableTextState>> {
        let Screen::Form(form) = self.launcher.view().screen else {
            return None;
        };
        let index = form.fields.iter().position(|field| field.id == field_id)?;
        match &self.form.as_ref()?.fields[index] {
            Control::Text(input) => Some(input.clone()),
            Control::Choice(_) => None,
        }
    }

    /// Creates or drops the form's controls to match the launcher's screen,
    /// and moves focus accordingly: to the first field of a newly opened
    /// form, back to the list when the form closes, and to the rejected field
    /// after a rejected submission.
    pub(crate) fn sync_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = self.launcher.view();
        let form = match view.screen {
            Screen::Form(form) => Some(form),
            _ => None,
        };
        match (form, self.form.is_some()) {
            (Some(form), false) => {
                let controls = self.form_controls(&form, cx);
                if let Some(first) = controls.fields.first() {
                    window.focus(&first.focus_handle(cx), cx);
                }
                self.form = Some(controls);
            }
            (Some(form), true) => {
                let rejected = form.fields.iter().position(|field| field.error.is_some());
                if let (Some(index), Status::Error(_)) = (rejected, view.status) {
                    let handle = self.form.as_ref().unwrap().fields[index].focus_handle(cx);
                    window.focus(&handle, cx);
                }
            }
            (None, true) => {
                self.form = None;
                window.focus(&self.focus_handle, cx);
            }
            (None, false) => {}
        }
    }

    fn form_controls(&self, form: &FormView, cx: &mut Context<Self>) -> FormControls {
        let mut subscriptions = Vec::new();
        let fields = form
            .fields
            .iter()
            .map(|field| match &field.kind {
                FieldKind::Text { .. } => {
                    let input = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
                    input.focus_handle(cx).tab_stop(true);
                    // An extension's text field starts empty; Pane's own
                    // forms (an alias) start with the current value.
                    if !field.value.is_empty() {
                        input.update(cx, |input, cx| input.emplace(&field.value, cx));
                    }
                    let id = field.id.clone();
                    subscriptions.push(cx.subscribe(
                        &input,
                        move |this, input, _: &TextChanged, cx| {
                            this.launcher.set_field_value(&id, input.read(cx).as_str());
                            cx.notify();
                        },
                    ));
                    Control::Text(input)
                }
                FieldKind::Choice(_) => Control::Choice(cx.focus_handle().tab_stop(true)),
            })
            .collect();
        FormControls {
            fields,
            submit: cx.focus_handle().tab_stop(true),
            _subscriptions: subscriptions,
        }
    }

    /// Submits the form and applies the extension's reply when it arrives.
    pub(crate) fn submit_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pending = self.launcher.submit_form();
        self.show_until_done(pending, window, cx);
    }

    /// Chooses the option `delta` places from the current one in the choice
    /// field `field_id`, clamped to the options.
    fn move_choice(&mut self, field_id: &str, delta: isize, cx: &mut Context<Self>) {
        let Screen::Form(form) = self.launcher.view().screen else {
            return;
        };
        let Some(field) = form.fields.into_iter().find(|field| field.id == field_id) else {
            return;
        };
        let FieldKind::Choice(choices) = &field.kind else {
            return;
        };
        let current = choices.iter().position(|choice| choice.id == field.value);
        let next = current
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(choices.len().saturating_sub(1));
        if let Some(choice) = choices.get(next) {
            self.launcher.set_field_value(&field.id, &choice.id);
            cx.notify();
        }
    }

    /// The form's controls, for the launcher's form screen.
    pub(crate) fn render_form(
        &self,
        title: String,
        form: FormView,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(controls) = &self.form else {
            return div().into_any_element();
        };
        let fields: Vec<AnyElement> = form
            .fields
            .into_iter()
            .zip(&controls.fields)
            .enumerate()
            .map(|(index, (field, control))| self.render_field(index, field, control, cx))
            .collect();
        div()
            .id("form")
            .role(Role::Form)
            .aria_label(title)
            .flex_1()
            .flex()
            .flex_col()
            .gap_3()
            .children(fields)
            .child(
                div()
                    .id("submit")
                    .debug_selector(|| "submit".into())
                    .key_context(BUTTON_CONTEXT)
                    .track_focus(&controls.submit)
                    .role(Role::Button)
                    .aria_label(form.submit_label.clone())
                    .on_action(cx.listener(|this, _: &Press, window, cx| {
                        this.submit_form(window, cx);
                    }))
                    .on_click(cx.listener(|this, _, window, cx| this.submit_form(window, cx)))
                    .self_start()
                    .px_4()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .border_2()
                    .border_color(rgb(0x364355))
                    .bg(rgb(0x364355))
                    .focus(|button| button.border_color(rgb(0x8ab4f8)))
                    .child(form.submit_label),
            )
            .into_any_element()
    }

    fn render_field(
        &self,
        index: usize,
        field: FormField,
        control: &Control,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let error = field.error.clone();
        let control = match (control, &field.kind) {
            (Control::Text(input), FieldKind::Text { placeholder }) => {
                let placeholder = placeholder.clone().unwrap_or_default();
                div()
                    .id(("field", index))
                    .debug_selector(|| format!("field-{}", field.id))
                    // The editable text element has no accessibility node of
                    // its own; this wrapper is the field's node and tracks
                    // the element's focus handle, so it is reported as
                    // focused and is a tab stop.
                    .track_focus(&input.focus_handle(cx))
                    .role(Role::TextInput)
                    .aria_label(field.label.clone())
                    .aria_value(field.value.clone())
                    .aria_placeholder(placeholder.clone())
                    .when_some(error.clone(), |node, error| node.aria_description(error))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_2()
                    .border_color(rgb(0x364355))
                    .bg(rgb(0x1a1e24))
                    .focus(|node| node.border_color(rgb(0x8ab4f8)))
                    .child(
                        text_input(("input", index))
                            .state(input.downgrade())
                            .placeholder(placeholder)
                            .w_full()
                            .whitespace_nowrap()
                            .overflow_x_scroll(),
                    )
                    .into_any_element()
            }
            (Control::Choice(handle), FieldKind::Choice(choices)) => {
                let options: Vec<AnyElement> = choices
                    .iter()
                    .enumerate()
                    .map(|(position, choice)| {
                        let chosen = choice.id == field.value;
                        let (field_id, choice_id) = (field.id.clone(), choice.id.clone());
                        let handle = handle.clone();
                        div()
                            .id(("choice", position))
                            .debug_selector(|| format!("choice-{field_id}-{choice_id}"))
                            .role(Role::RadioButton)
                            .aria_label(choice.label.clone())
                            .aria_toggled(if chosen {
                                Toggled::True
                            } else {
                                Toggled::False
                            })
                            .aria_position_in_set(position + 1)
                            .aria_size_of_set(choices.len())
                            .when(chosen, |option| option.aria_active_descendant())
                            .flex()
                            .gap_1()
                            .px_2()
                            .rounded_md()
                            .cursor_pointer()
                            .when(chosen, |option| option.bg(rgb(0x364355)))
                            .child(if chosen { "◉" } else { "○" })
                            .child(choice.label.clone())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.launcher.set_field_value(&field_id, &choice_id);
                                window.focus(&handle, cx);
                                cx.notify();
                            }))
                            .into_any_element()
                    })
                    .collect();
                let (next, previous) = (field.id.clone(), field.id.clone());
                div()
                    .id(("field", index))
                    .debug_selector(|| format!("field-{}", field.id))
                    .key_context(CHOICE_CONTEXT)
                    .track_focus(handle)
                    .role(Role::RadioGroup)
                    .aria_label(field.label.clone())
                    .when_some(error.clone(), |node, error| node.aria_description(error))
                    .on_action(cx.listener(move |this, _: &NextChoice, _, cx| {
                        this.move_choice(&next, 1, cx)
                    }))
                    .on_action(cx.listener(move |this, _: &PreviousChoice, _, cx| {
                        this.move_choice(&previous, -1, cx)
                    }))
                    .flex()
                    .gap_2()
                    .p_1()
                    .rounded_md()
                    .border_2()
                    .border_color(rgb(0x20252d))
                    .focus(|node| node.border_color(rgb(0x8ab4f8)))
                    .children(options)
                    .into_any_element()
            }
            _ => unreachable!("controls are created from the form's fields"),
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0xaab4c0))
                    .child(field.label.clone()),
            )
            .child(control)
            .when_some(error, |element, error| {
                element.child(
                    div()
                        .debug_selector(|| format!("field-error-{}", field.id))
                        .text_sm()
                        .text_color(rgb(0xf08c8c))
                        .child(error),
                )
            })
            .into_any_element()
    }
}
