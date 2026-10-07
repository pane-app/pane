//! The form screen: an extension's form, or Pane's own alias form, rendered
//! with standard controls.
//!
//! - A text field is GPUI CE's editable text element (typing, editing keys,
//!   clipboard, undo and input-method composition) inside a focusable
//!   `TextInput` node that carries the label, value, placeholder and error.
//!   A password field (a password argument in Pane's argument form, a
//!   password preference on the Setup screen) is the
//!   same field with its text concealed: drawn as dots, and reported to
//!   assistive technology as dots too.
//! - A choice field is a `RadioGroup` of `RadioButton`s. The group holds focus
//!   and the chosen option is its active descendant; arrow keys change the
//!   choice, like a native radio group.
//! - The submit button is a focusable `Button`; Enter or Space presses it.
//!
//! Tab and Shift-Tab move through the controls in order. Enter anywhere on
//! the form submits it and Escape returns to the command. A form opens with
//! focus on its first required field that is empty (Pane's argument form),
//! else on its first field. After a rejected submission (a required field
//! left empty, too), focus moves to the rejected field.
//!
//! The controls are drawn with the Settings board's families (#99,
//! `ui::controls`), the launcher's own copies of their styling gone: each
//! field a field group — its label over its control, its error under it —
//! a text field a field's well, a choice field the segmented choice, and
//! the submit control a button.
//!
//! The Setup screen (#143) is this form too: Pane's own, asking for a
//! command's required, unset preferences before it runs. Over its fields
//! it shows the extension's tile and title and "Set these up before using
//! <command>"; each field's description is under it, a password's text is
//! hidden as it is typed, and the package's `HELP.md` is beside the
//! fields, as plain paragraphs.

use gpui::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, KeyBinding, Role, Stateful,
    Subscription, Toggled, Window, actions, div, prelude::*, px, transparent_black,
};
use gpui_elements::editable_text::{EditableTextState, StringStorage, TextChanged, text_input};
use pane_core::{FieldKind, FormField, FormView, Screen, SetupHeader, Status};

use crate::app::LauncherWindow;
use crate::ui::controls;
use crate::ui::extension_icon::{RowIcon, row_icon_at};
use crate::ui::icon::TileSize;
use crate::ui::input::TextEditingKeys;
use crate::ui::theme::Theme;

actions!(form, [NextChoice, PreviousChoice, Press]);

const CHOICE_CONTEXT: &str = "FormChoice";
const BUTTON_CONTEXT: &str = "FormButton";

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
    /// The fields they were made for (see [`shape`]): a form that replaced
    /// another without a screen between them (an argument form a hotkey
    /// opened over a form, a Setup screen shown over a form) gets controls
    /// of its own.
    shape: Vec<(String, bool)>,
}

/// What a form's controls depend on: each field's id, and whether it is a
/// choice (else a text field).
fn shape(form: &FormView) -> Vec<(String, bool)> {
    form.fields
        .iter()
        .map(|field| (field.id.clone(), matches!(field.kind, FieldKind::Choice(_))))
        .collect()
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

impl FormControls {
    /// The form's text fields' editing states, which a platform input
    /// method talks to while composing text — for the launcher's back
    /// key, which cancels an active composition before it acts.
    pub(crate) fn text_fields(&self) -> Vec<Entity<EditableTextState>> {
        self.fields
            .iter()
            .filter_map(|control| match control {
                Control::Text(input) => Some(input.clone()),
                Control::Choice(_) => None,
            })
            .collect()
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
    /// and moves focus accordingly: to the first required field that is
    /// empty of a newly opened form, else its first field; back to the list
    /// when the form closes; and to the rejected field after a rejected
    /// submission.
    pub(crate) fn sync_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = self.launcher.view();
        let form = match view.screen {
            Screen::Form(form) => Some(form),
            _ => None,
        };
        let new = match (&form, &self.form) {
            (Some(form), Some(controls)) => controls.shape != shape(form),
            (Some(_), None) => true,
            (None, _) => false,
        };
        match (form, self.form.is_some()) {
            (Some(form), true) if !new => {
                let rejected = form.fields.iter().position(|field| field.error.is_some());
                if let (Some(index), Status::Error(_)) = (rejected, view.status) {
                    let handle = self.form.as_ref().unwrap().fields[index].focus_handle(cx);
                    window.focus(&handle, cx);
                }
            }
            (Some(form), _) => {
                let controls = self.form_controls(&form, cx);
                let empty_required = form
                    .fields
                    .iter()
                    .position(|field| field.required && field.value.trim().is_empty());
                if let Some(first) = controls.fields.get(empty_required.unwrap_or(0)) {
                    window.focus(&first.focus_handle(cx), cx);
                }
                self.form = Some(controls);
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
                FieldKind::Text { .. } | FieldKind::Password { .. } => {
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
            shape: shape(form),
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

    /// The form's controls, for the launcher's form screen: each field a
    /// Settings field group (#99) — its label over its control, its error
    /// under it — and the submit button, composed by [`compose`].
    pub(crate) fn render_form(
        &self,
        title: String,
        form: FormView,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(form_controls) = &self.form else {
            return div().into_any_element();
        };
        // The form keeps its own behavior; only its paint comes from the
        // shared theme, so it stays legible in either appearance.
        let visuals = crate::settings::launcher_visuals(cx);
        let theme = &visuals.theme;
        let fields: Vec<AnyElement> = form
            .fields
            .into_iter()
            .zip(&form_controls.fields)
            .enumerate()
            .map(|(index, (field, control))| self.render_field(index, field, control, cx))
            .collect();
        let submit = submit_button(form.submit_label, theme)
            .track_focus(&form_controls.submit)
            .on_action(cx.listener(|this, _: &Press, window, cx| {
                this.submit_form(window, cx);
            }))
            .on_click(cx.listener(|this, _, window, cx| this.submit_form(window, cx)));
        match (&form.setup, self.launcher.arguments_asked_for()) {
            // The extension's icon (#139) over the Setup screen (#143).
            (Some(setup), _) => {
                let icon =
                    crate::features::icons::row_icon_of(&self.launcher, &setup.package, theme);
                compose_setup(title, setup, &icon, fields, submit, theme).into_any_element()
            }
            // Pane's argument form (#144): the command's icon (#139) and
            // title over its fields.
            (None, Some(command)) => {
                let icon = crate::features::icons::row_icon_of(&self.launcher, &command, theme);
                compose_arguments(title, &icon, fields, submit, theme).into_any_element()
            }
            (None, None) => compose(title, fields, submit, theme).into_any_element(),
        }
    }

    fn render_field(
        &self,
        index: usize,
        field: FormField,
        control: &Control,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let error = field.error.clone();
        let description = field.description.clone();
        let visuals = crate::settings::launcher_visuals(cx);
        let theme = &visuals.theme;
        let control = match (control, &field.kind) {
            (Control::Text(input), FieldKind::Text { placeholder }) => text_control(
                TextControl {
                    index,
                    id: &field.id,
                    label: &field.label,
                    value: &field.value,
                    placeholder: placeholder.as_deref().unwrap_or_default(),
                    error: error.as_deref(),
                },
                input,
                &input.focus_handle(cx),
                theme,
            )
            .into_any_element(),
            (Control::Text(input), FieldKind::Password { placeholder }) => password_control(
                TextControl {
                    index,
                    id: &field.id,
                    label: &field.label,
                    value: &field.value,
                    placeholder: placeholder.as_deref().unwrap_or_default(),
                    error: error.as_deref(),
                },
                input,
                &input.focus_handle(cx),
                theme,
            )
            .into_any_element(),
            (Control::Choice(handle), FieldKind::Choice(choices)) => {
                let segments = choices
                    .iter()
                    .enumerate()
                    .map(|(position, choice)| {
                        let chosen = choice.id == field.value;
                        let (field_id, choice_id) = (field.id.clone(), choice.id.clone());
                        let handle = handle.clone();
                        choice_segment(
                            (field.id.as_str(), choice.id.as_str(), choice.label.as_str()),
                            (position, choices.len()),
                            chosen,
                            theme,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.launcher.set_field_value(&field_id, &choice_id);
                                window.focus(&handle, cx);
                                cx.notify();
                            },
                        ))
                    })
                    .collect();
                let (next, previous) = (field.id.clone(), field.id.clone());
                choice_track(
                    (index, field.id.as_str(), field.label.as_str()),
                    error.as_deref(),
                    segments,
                    theme,
                )
                .key_context(CHOICE_CONTEXT)
                .track_focus(handle)
                .on_action(
                    cx.listener(move |this, _: &NextChoice, _, cx| this.move_choice(&next, 1, cx)),
                )
                .on_action(cx.listener(move |this, _: &PreviousChoice, _, cx| {
                    this.move_choice(&previous, -1, cx)
                }))
                .into_any_element()
            }
            _ => unreachable!("controls are created from the form's fields"),
        };
        field_group(&field.id, field.label.clone(), control, error, theme)
            .when_some(description, |group, description| {
                let selector = format!("field-description-{}", field.id);
                group.child(
                    controls::field_description(description, theme.text_muted, theme)
                        .debug_selector(move || selector),
                )
            })
            .into_any_element()
    }
}

/// The Setup screen's composition (#143): over the form, the extension's
/// icon (`icon`, as its row draws it) and title and the sentence naming the command; then its fields and
/// `submit` in a column, with the package's help beside them as plain
/// paragraphs when it ships one.
pub(crate) fn compose_setup(
    title: String,
    setup: &SetupHeader,
    icon: &RowIcon,
    fields: Vec<AnyElement>,
    submit: Stateful<Div>,
    theme: &Theme,
) -> Stateful<Div> {
    let header = div()
        .flex()
        .items_center()
        .gap(theme.geometry.controls.row_gap)
        .child(row_icon_at(
            icon,
            TileSize::Row,
            "setup-icon",
            "setup",
            theme,
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.))
                .child(controls::field_label(setup.title.clone(), theme))
                .child(
                    controls::field_description(setup.sentence.clone(), theme.text_muted, theme)
                        .id("setup-sentence")
                        .debug_selector(|| "setup-sentence".into())
                        .role(Role::Heading)
                        .aria_label(setup.sentence.clone()),
                ),
        );
    let column = div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap(theme.geometry.controls.group_gap)
        .children(fields)
        .child(div().flex().child(submit));
    let help = (!setup.help.is_empty()).then(|| {
        let paragraphs = setup.help.iter().enumerate().map(|(index, paragraph)| {
            controls::field_description(paragraph.clone(), theme.text_body, theme)
                .id(("setup-help", index))
                .debug_selector(move || format!("setup-help-{index}"))
        });
        div()
            .id("setup-help")
            .debug_selector(|| "setup-help".into())
            .role(Role::Note)
            .aria_label("Help")
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .gap(theme.geometry.controls.field_gap)
            .children(paragraphs)
    });
    div()
        .id("form")
        .debug_selector(|| "setup".into())
        .role(Role::Form)
        .aria_label(title)
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .gap(theme.geometry.controls.group_gap)
        .px(theme.geometry.search_padding_x)
        .py(theme.geometry.screen_padding_y)
        .overflow_y_scroll()
        .child(header)
        .child(
            div()
                .flex()
                .gap(theme.geometry.controls.group_gap)
                .child(column)
                .children(help),
        )
}

/// The argument form's composition (#144): over the form's fields, the
/// command's icon as its row in root search draws it (#139) beside its
/// title, so the user sees which command asks; then the fields and
/// `submit`, laid out as [`compose`] lays them out.
pub(crate) fn compose_arguments(
    title: String,
    icon: &RowIcon,
    fields: Vec<AnyElement>,
    submit: Stateful<Div>,
    theme: &Theme,
) -> Stateful<Div> {
    let header = div()
        .flex()
        .items_center()
        .gap(theme.geometry.controls.row_gap)
        .child(row_icon_at(
            icon,
            TileSize::Row,
            "arguments-icon",
            "arguments",
            theme,
        ))
        .child(
            controls::field_label(title.clone(), theme).debug_selector(|| "arguments-title".into()),
        );
    div()
        .id("form")
        .debug_selector(|| "arguments".into())
        .role(Role::Form)
        .aria_label(title)
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .gap(theme.geometry.controls.group_gap)
        .px(theme.geometry.search_padding_x)
        .py(theme.geometry.screen_padding_y)
        .overflow_y_scroll()
        .child(header)
        .children(fields)
        .child(div().flex().child(submit))
}

/// A form screen's composition (#99): the form's field groups, 18px apart in a column that
/// keeps its own edge padding and scrolls when its fields outgrow the
/// window — so they never meet the panel's edges — then `submit`.
pub(crate) fn compose(
    title: String,
    fields: Vec<AnyElement>,
    submit: Stateful<Div>,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id("form")
        .role(Role::Form)
        .aria_label(title)
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .gap(theme.geometry.controls.group_gap)
        .px(theme.geometry.search_padding_x)
        .py(theme.geometry.screen_padding_y)
        .overflow_y_scroll()
        .children(fields)
        .child(div().flex().child(submit))
}

/// One field's group (`ui::controls::field`): its label (13.5/500 over its
/// control, 8px apart, as the Settings board's fields), its control, and —
/// after a rejected submission — its error in the danger tone under it.
pub(crate) fn field_group(
    id: &str,
    label: String,
    control: impl IntoElement,
    error: Option<String>,
    theme: &Theme,
) -> Div {
    let label_selector = format!("field-label-{id}");
    let error_selector = format!("field-error-{id}");
    controls::field(theme)
        .child(controls::field_label(label, theme).debug_selector(move || label_selector))
        .child(control)
        .when_some(error, |group, error| {
            group.child(
                controls::field_description(error, theme.danger, theme)
                    .debug_selector(move || error_selector),
            )
        })
}

/// What a text field shows: its place in the form, its identity, label,
/// value, placeholder and error.
pub(crate) struct TextControl<'a> {
    pub(crate) index: usize,
    pub(crate) id: &'a str,
    pub(crate) label: &'a str,
    pub(crate) value: &'a str,
    pub(crate) placeholder: &'a str,
    pub(crate) error: Option<&'a str>,
}

/// A text field: GPUI CE's editable text element in a field's well (34px,
/// black 24% under its ring, the focus color while it has the keyboard).
/// The editable text element has no accessibility node of its own; the
/// well is the field's node and tracks the element's `focus`, so it is
/// reported as focused and is a tab stop.
pub(crate) fn text_control(
    field: TextControl<'_>,
    input: &Entity<EditableTextState>,
    focus: &FocusHandle,
    theme: &Theme,
) -> Stateful<Div> {
    let selector = format!("field-{}", field.id);
    let ring = controls::well_shadows(true, theme);
    controls::well(false, theme)
        .id(("field", field.index))
        .debug_selector(move || selector)
        .track_focus(focus)
        .role(Role::TextInput)
        .aria_label(field.label.to_owned())
        .aria_value(field.value.to_owned())
        .aria_placeholder(field.placeholder.to_owned())
        .when_some(field.error, |node, error| {
            node.aria_description(error.to_owned())
        })
        .focus(move |node| node.shadow(ring))
        .child(controls::well_input(
            text_input(("input", field.index)).state(input.downgrade()),
            field.placeholder.to_owned(),
            theme,
        ))
}

/// The character a password field shows for each character typed.
const CONCEALED: char = '\u{2022}';

/// A password field: a text field ([`text_control`]) whose text is drawn
/// transparent, with a dot for each character over it, and whose value
/// assistive technology reads as those dots. GPUI CE's editable text has
/// no masking of its own yet (it is on its backlog), so the dots are drawn
/// over the field and do not follow the caret's glyph widths exactly.
pub(crate) fn password_control(
    field: TextControl<'_>,
    input: &Entity<EditableTextState>,
    focus: &FocusHandle,
    theme: &Theme,
) -> Stateful<Div> {
    let selector = format!("field-{}", field.id);
    let ring = controls::well_shadows(true, theme);
    let dots: String = field.value.chars().map(|_| CONCEALED).collect();
    controls::well(false, theme)
        .id(("field", field.index))
        .debug_selector(move || selector)
        .track_focus(focus)
        .role(Role::TextInput)
        .aria_label(field.label.to_owned())
        .aria_value(dots.clone())
        .aria_placeholder(field.placeholder.to_owned())
        .when_some(field.error, |node, error| {
            node.aria_description(error.to_owned())
        })
        .focus(move |node| node.shadow(ring))
        .child(
            div()
                .relative()
                .flex_1()
                .min_w(px(0.))
                .child(
                    controls::well_input(
                        text_input(("input", field.index)).state(input.downgrade()),
                        field.placeholder.to_owned(),
                        theme,
                    )
                    .text_color(transparent_black()),
                )
                .child(
                    div()
                        .id(("concealed", field.index))
                        .absolute()
                        .top_0()
                        .left_0()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_color(theme.text_title)
                        .aria_hidden()
                        .child(dots),
                ),
        )
}

/// One choice of a choice field: a segment of its track, a radio button
/// (`(field, choice, label)` name it; `(position, count)` place it in its
/// set), the chosen one the group's active descendant. The caller attaches
/// its click.
pub(crate) fn choice_segment(
    (field, choice, label): (&str, &str, &str),
    (position, count): (usize, usize),
    chosen: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let selector = format!("choice-{field}-{choice}");
    controls::segment(label.to_owned(), chosen, true, theme)
        .id(("choice", position))
        .debug_selector(move || selector)
        .role(Role::RadioButton)
        .aria_label(label.to_owned())
        .aria_toggled(if chosen {
            Toggled::True
        } else {
            Toggled::False
        })
        .aria_position_in_set(position + 1)
        .aria_size_of_set(count)
        .when(chosen, |segment| segment.aria_active_descendant())
}

/// A choice field: the segmented choice's track (`(index, field, label)`
/// name it), a radio group holding `segments` and the keyboard — whose
/// arrows change the choice, like a native radio group — under Pane's
/// focus ring while the keyboard is on it. The caller attaches its focus
/// and keys.
pub(crate) fn choice_track(
    (index, field, label): (usize, &str, &str),
    error: Option<&str>,
    segments: Vec<Stateful<Div>>,
    theme: &Theme,
) -> Stateful<Div> {
    let selector = format!("field-{field}");
    let ring = controls::focus_ring(theme);
    controls::segment_track(theme)
        .id(("field", index))
        .debug_selector(move || selector)
        .role(Role::RadioGroup)
        .aria_label(label.to_owned())
        .when_some(error, |node, error| node.aria_description(error.to_owned()))
        .focus(move |node| node.shadow(ring))
        .children(segments)
}

/// The submit button: a Settings button (`.pill`, 30px, white 8%) under
/// Pane's focus ring while the keyboard is on it; Enter on the form and
/// Space on the button press it. The caller attaches its focus and
/// presses.
pub(crate) fn submit_button(label: String, theme: &Theme) -> Stateful<Div> {
    let ring = controls::focus_ring(theme);
    controls::button("submit", label.clone(), true, theme)
        .debug_selector(|| "submit".into())
        .key_context(BUTTON_CONTEXT)
        .role(Role::Button)
        .aria_label(label)
        .focus(move |button| button.shadow(ring))
}
