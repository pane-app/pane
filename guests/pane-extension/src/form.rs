//! The form a designed view holds (#241, `docs/designed-tree.md`): a
//! `form` node whose children are the author's own layout — the fields
//! anywhere in it — and whose submission is an action. A submission
//! collects the form's fields' values and runs the listener
//! [`crate::view::Cx::form_listener`] names, told them all keyed by the fields' keys;
//! Pane draws the tree the listener's view answers with, so validation
//! errors are the fields' `error` in it.
//!
//! The fields are Raycast's: [`TextField`], [`PasswordField`],
//! [`TextArea`], [`Checkbox`], [`Toggle`], [`DatePicker`],
//! [`DateTimePicker`], [`Dropdown`], [`TagPicker`], [`FilePicker`],
//! [`FolderPicker`], and the static [`Description`], [`Separator`] and
//! [`Link`]. Each names itself by its key — the id its value is in a
//! submission — and carries what every field does: a `title` over its
//! control, an `info` note under it, the `error` the view's last answer
//! set, the `default` value it starts from, `auto_focus`, and `remember`
//! for the value Pane keeps as the package's settings and prefills the
//! next time the field appears.
//!
//! ```ignore
//! struct Greeting { name: RefCell<String> }
//!
//! impl View for Greeting {
//!     fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
//!         let submit = cx.form_listener(|this, values| {
//!             *this.name.borrow_mut() = values.text("name").unwrap_or_default().into();
//!         });
//!         let error = if self.name.borrow().is_empty() {
//!             Some("Enter a name")
//!         } else {
//!             None
//!         };
//!         form::Form::new()
//!             .submit_title("Greet")
//!             .on_submit(submit)
//!             .child(form::TextField::new("name").title("Name")
//!                 .error(error).remember())
//!             .into_answer()
//!     }
//! }
//! ```

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::view::{
    Border, FormListener, IntoNode, Length, Listener, Node, NodeKind, Paint, Place, Radius,
    Surface, ValueListener, divider, styled,
};

/// The values a form was submitted with (#241): each field's, keyed by
/// its key — the text a field edits, the state a checkbox or toggle is
/// in, the tags or paths a picker chose.
#[derive(Debug, Default)]
pub struct FormValues {
    values: Vec<(String, FormValue)>,
}

/// One field's value in a form's submission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormValue {
    /// A text field's, password field's, text area's, date field's,
    /// dropdown's or single-path picker's value.
    Text(String),
    /// A checkbox's or toggle's state.
    On(bool),
    /// A tag picker's chosen tags, or a picker-of-many's chosen paths.
    List(Vec<String>),
}

impl FormValues {
    /// The text of the field `key`, if it is one.
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.value(key)? {
            FormValue::Text(text) => Some(text.as_str()),
            _ => None,
        }
    }

    /// The state of the checkbox or toggle `key`, if it is one.
    pub fn on(&self, key: &str) -> Option<bool> {
        match self.value(key)? {
            FormValue::On(on) => Some(*on),
            _ => None,
        }
    }

    /// The tags or paths of the picker `key`, if it is one.
    pub fn list(&self, key: &str) -> Option<&[String]> {
        match self.value(key)? {
            FormValue::List(values) => Some(values.as_slice()),
            _ => None,
        }
    }

    /// The value of the field `key`, if the form was submitted with one.
    pub fn value(&self, key: &str) -> Option<&FormValue> {
        self.values
            .iter()
            .find(|(named, _)| named == key)
            .map(|(_, value)| value)
    }

    /// Every value the submission carries, `(key, value)` pairs in the
    /// form's field order.
    pub fn entries(&self) -> &[(String, FormValue)] {
        &self.values
    }

    /// The values a submission's payload names: the object its `values`
    /// field holds. A value that is neither a string, a boolean nor a
    /// list of strings is left out, as a field this SDK does not know
    /// would be.
    pub(crate) fn of(payload: &str) -> FormValues {
        let mut values = Vec::new();
        let Some(rest) = payload.split_once("\"values\"").map(|(_, rest)| rest) else {
            return FormValues { values };
        };
        // Walk the object the payload names, taking each key with the
        // value that follows it. The scanner is deliberately small: the
        // payload is Pane's own, its shape fixed.
        let rest = rest.trim_start().trim_start_matches(':').trim_start();
        let Some(body) = rest
            .strip_prefix('{')
            .and_then(|body| body.strip_suffix('}'))
        else {
            return FormValues { values };
        };
        let mut rest = body;
        while let Some(at) = rest.find('"') {
            let key_end = match rest[at + 1..].find('"') {
                Some(end) => at + 1 + end,
                None => break,
            };
            let key = unescape(&rest[at + 1..key_end]);
            rest = match rest[key_end + 1..].find(':') {
                Some(colon) => &rest[key_end + 1 + colon + 1..],
                None => break,
            };
            let skipped = rest.trim_start();
            if skipped.starts_with('"') {
                let end = match skipped[1..].find('"') {
                    Some(end) => end + 1,
                    None => break,
                };
                values.push((key, FormValue::Text(unescape(&skipped[1..end]))));
                rest = match skipped[end + 1..].find(',') {
                    Some(comma) => &skipped[end + 1 + comma + 1..],
                    None => break,
                };
            } else if skipped.starts_with("true") || skipped.starts_with("false") {
                let on = skipped.starts_with("true");
                values.push((key, FormValue::On(on)));
                rest = match skipped[4..].find(',') {
                    Some(comma) => &skipped[4 + comma + 1..],
                    None => break,
                };
            } else if let Some(list) = skipped.strip_prefix('[') {
                let end = match skipped.find(']') {
                    Some(end) => end,
                    None => break,
                };
                let items = list[..end]
                    .split(',')
                    .filter(|item| !item.trim().is_empty())
                    .map(|item| unescape(item.trim().trim_matches('"')))
                    .collect();
                values.push((key, FormValue::List(items)));
                rest = match skipped[end + 1..].find(',') {
                    Some(comma) => &skipped[end + 1 + comma + 1..],
                    None => break,
                };
            } else {
                break;
            }
        }
        FormValues { values }
    }
}

/// A JSON string literal's content, unescaped.
fn unescape(text: &str) -> String {
    let mut result = String::new();
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some(escaped) => result.push(escaped),
                None => {}
            },
            other => result.push(other),
        }
    }
    result
}

/// A `form` node being built: its children the author's own layout, its
/// submission an action ([`Form::on_submit`]). The children a form
/// commonly holds are its fields, each keyed by the id its value is in a
/// submission; any layout among them is the author's.
#[derive(Debug)]
pub struct Form(Node);

impl Form {
    /// A form with no fields and no submit listener yet.
    pub fn new() -> Form {
        Form(Node::of(
            NodeKind::Form(crate::view::FormPayload::default()),
        ))
    }

    /// The submit button's label; "Submit" when none is given.
    pub fn submit_title(mut self, title: impl Into<String>) -> Form {
        if let NodeKind::Form(form) = &mut self.0.kind {
            form.submit_label = Some(title.into());
        }
        self
    }

    /// A submission of the form runs `listener`, told every field's
    /// value. Enter in a single-line field of the form submits it, as
    /// Ctrl+Enter does in a text area and the submit button does.
    pub fn on_submit(mut self, listener: FormListener) -> Form {
        if let NodeKind::Form(form) = &mut self.0.kind {
            form.on_submit = Some(listener);
        }
        self
    }
}

impl Form {
    /// The navigation title of the view this form is the root of: what
    /// names the view where a screen's title is shown.
    pub fn navigation_title(mut self, title: impl Into<String>) -> Form {
        self.0.navigation_title = Some(title.into());
        self
    }
}

impl Default for Form {
    fn default() -> Form {
        Form::new()
    }
}

crate::view::builder!(Form);

/// A form's text field: a single line of text, its value in a submission
/// under its key.
pub struct TextField(pub(crate) Node);

crate::view::builder!(TextField);

/// A form's password field: a single line of text, concealed while it is
/// typed.
pub struct PasswordField(pub(crate) Node);

crate::view::builder!(PasswordField);

/// A form's text area: several lines of text, its Enter inserting a
/// newline (Ctrl+Enter submits the form).
pub struct TextArea(pub(crate) Node);

crate::view::builder!(TextArea);

/// A form's checkbox: on or off.
pub struct Checkbox(pub(crate) Node);

crate::view::builder!(Checkbox);

/// A form's toggle: on or off, drawn as the switch it is.
pub struct Toggle(pub(crate) Node);

crate::view::builder!(Toggle);

/// A form's date field: a date typed or stepped with the arrow keys, its
/// value "YYYY-MM-DD".
pub struct DatePicker(pub(crate) Node);

crate::view::builder!(DatePicker);

/// A form's date and time field: typed or stepped with the arrow keys,
/// its value "YYYY-MM-DD HH:MM".
pub struct DateTimePicker(pub(crate) Node);

crate::view::builder!(DateTimePicker);

/// A form's dropdown: exactly one of its options, searched as the user
/// types into its popup — by Pane, or by the extension through
/// [`Dropdown::on_search_text`] when it says the search is its own.
pub struct Dropdown(pub(crate) Node);

crate::view::builder!(Dropdown);

/// A form's tag picker: several of its options, chosen as chips.
pub struct TagPicker(pub(crate) Node);

crate::view::builder!(TagPicker);

/// A form's file picker: a path typed or chosen with the system's dialog.
pub struct FilePicker(pub(crate) Node);

crate::view::builder!(FilePicker);

/// A form's folder picker: a path typed or chosen with the system's
/// dialog.
pub struct FolderPicker(pub(crate) Node);

crate::view::builder!(FolderPicker);

/// One text field, keyed `key`: the id its value is in a submission.
pub fn text_field(key: impl Into<String>) -> TextField {
    TextField(field(
        NodeKind::TextInput(crate::view::TextInputPayload::default()),
        key,
    ))
}

/// One password field, keyed `key` (see [`text_field`]).
pub fn password_field(key: impl Into<String>) -> PasswordField {
    PasswordField(field(
        NodeKind::PasswordInput(crate::view::TextInputPayload::default()),
        key,
    ))
}

/// One text area, keyed `key` (see [`text_field`]).
pub fn text_area(key: impl Into<String>) -> TextArea {
    TextArea(field(
        NodeKind::TextArea(crate::view::TextInputPayload::default()),
        key,
    ))
}

/// One checkbox, keyed `key`: off until [`Checkbox::default_value`] says
/// otherwise. Its `label` is drawn beside its box.
pub fn checkbox(key: impl Into<String>) -> Checkbox {
    Checkbox(field(
        NodeKind::Checkbox(crate::view::TogglePayload::default()),
        key,
    ))
}

/// One toggle, keyed `key` (see [`checkbox`]).
pub fn toggle(key: impl Into<String>) -> Toggle {
    Toggle(field(
        NodeKind::Toggle(crate::view::TogglePayload::default()),
        key,
    ))
}

/// One date field, keyed `key` (see [`text_field`]); its value
/// "YYYY-MM-DD".
pub fn date_picker(key: impl Into<String>) -> DatePicker {
    DatePicker(field(
        NodeKind::DatePicker(crate::view::DatePayload::default()),
        key,
    ))
}

/// One date and time field, keyed `key`; its value "YYYY-MM-DD HH:MM".
pub fn date_time_picker(key: impl Into<String>) -> DateTimePicker {
    DateTimePicker(field(
        NodeKind::DateTimePicker(crate::view::DatePayload::default()),
        key,
    ))
}

/// One dropdown over `options`, keyed `key`: the first option chosen
/// until [`Dropdown::default_value`] says otherwise.
pub fn dropdown(key: impl Into<String>) -> Dropdown {
    Dropdown(field(
        NodeKind::Select(crate::view::SegmentedPayload::default()),
        key,
    ))
}

/// One tag picker over `options`, keyed `key`: none chosen until
/// [`TagPicker::default_value`] says otherwise.
pub fn tag_picker(key: impl Into<String>) -> TagPicker {
    TagPicker(field(
        NodeKind::TagPicker(crate::view::TagPickerPayload::default()),
        key,
    ))
}

/// One file picker, keyed `key` (see [`text_field`]); choose several with
/// [`FilePicker::allow_multiple`].
pub fn file_picker(key: impl Into<String>) -> FilePicker {
    FilePicker(field(
        NodeKind::FilePicker(crate::view::PathPayload::default()),
        key,
    ))
}

/// One folder picker, keyed `key` (see [`file_picker`]).
pub fn folder_picker(key: impl Into<String>) -> FolderPicker {
    FolderPicker(field(
        NodeKind::FolderPicker(crate::view::PathPayload::default()),
        key,
    ))
}

/// The description between a form's fields: `markdown`, as the `markdown`
/// component draws it.
pub fn description(markdown: impl Into<String>) -> crate::view::Markdown {
    crate::view::markdown(markdown)
}

/// The separator between a form's fields: a hairline rule, as the
/// `divider` component draws it.
pub fn separator() -> crate::view::Divider {
    divider()
}

/// The link between a form's fields: `label`, pressed running
/// `on_click` (see [`crate::view::link`]).
pub fn link(label: impl Into<String>, on_click: Listener) -> crate::view::Link {
    crate::view::link(label).on_click(on_click)
}

/// The field `kind` node, keyed `key`.
fn field(kind: NodeKind, key: impl Into<String>) -> Node {
    let mut node = Node::of(kind);
    node.key = Some(key.into());
    node
}

/// The methods every field's builder carries: what it says around its
/// control, and the keyboard it asks for. Each kind's own methods follow.
macro_rules! fielded {
    ($builder:ident, $pattern:pat_param => $payload:ident) => {
        impl $builder {
            /// The field's title, over its control; also names it to
            /// assistive technology.
            pub fn title(mut self, title: impl Into<String>) -> $builder {
                if let $pattern = &mut self.0.kind {
                    $payload.field.title = Some(title.into());
                }
                self
            }

            /// The field's note, under its control.
            pub fn info(mut self, info: impl Into<String>) -> $builder {
                if let $pattern = &mut self.0.kind {
                    $payload.field.info = Some(info.into());
                }
                self
            }

            /// The field's error, drawn under it in the danger tone: what
            /// the view's last answer set, its validation of the value the
            /// form was submitted with.
            pub fn error(mut self, error: impl Into<String>) -> $builder {
                if let $pattern = &mut self.0.kind {
                    $payload.field.error = Some(error.into());
                }
                self
            }

            /// The field's last submitted value is kept as the package's
            /// settings and prefilled the next time the field appears.
            pub fn remember(mut self) -> $builder {
                if let $pattern = &mut self.0.kind {
                    $payload.field.remember = true;
                }
                self
            }

            /// Asks for the keyboard when the form opens: the field is
            /// focused, as a view's own `focus` ask is.
            pub fn auto_focus(mut self) -> $builder {
                self.0.focus = true;
                self
            }
        }
    };
}

fielded!(TextField, NodeKind::TextInput(input) => input);
fielded!(PasswordField, NodeKind::PasswordInput(input) => input);
fielded!(TextArea, NodeKind::TextArea(input) => input);
fielded!(Checkbox, NodeKind::Checkbox(checkbox) => checkbox);
fielded!(Toggle, NodeKind::Toggle(toggle) => toggle);
fielded!(DatePicker, NodeKind::DatePicker(date) => date);
fielded!(DateTimePicker, NodeKind::DateTimePicker(date) => date);
fielded!(Dropdown, NodeKind::Select(select) => select);
fielded!(TagPicker, NodeKind::TagPicker(picker) => picker);
fielded!(FilePicker, NodeKind::FilePicker(picker) => picker);
fielded!(FolderPicker, NodeKind::FolderPicker(picker) => picker);

impl TextField {
    /// The field's placeholder, while it is empty.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> TextField {
        if let NodeKind::TextInput(input) = &mut self.0.kind {
            input.placeholder = Some(placeholder.into());
        }
        self
    }

    /// The field's default: the value it starts from.
    pub fn default_value(mut self, value: impl Into<String>) -> TextField {
        if let NodeKind::TextInput(input) = &mut self.0.kind {
            input.default = Some(value.into());
        }
        self
    }

    /// The field's value as the user types it runs `listener`, told it.
    pub fn on_input(mut self, listener: ValueListener) -> TextField {
        if let NodeKind::TextInput(input) = &mut self.0.kind {
            input.on_input = Some(listener);
        }
        self
    }

    /// A commit of the field (Enter, a blur) runs `listener`, told its
    /// value.
    pub fn on_change(mut self, listener: ValueListener) -> TextField {
        if let NodeKind::TextInput(input) = &mut self.0.kind {
            input.on_change = Some(listener);
        }
        self
    }
}

impl PasswordField {
    /// The field's placeholder, while it is empty.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> PasswordField {
        if let NodeKind::PasswordInput(input) = &mut self.0.kind {
            input.placeholder = Some(placeholder.into());
        }
        self
    }

    /// The field's default: the value it starts from.
    pub fn default_value(mut self, value: impl Into<String>) -> PasswordField {
        if let NodeKind::PasswordInput(input) = &mut self.0.kind {
            input.default = Some(value.into());
        }
        self
    }

    /// The field's value as the user types it runs `listener`, told it.
    pub fn on_input(mut self, listener: ValueListener) -> PasswordField {
        if let NodeKind::PasswordInput(input) = &mut self.0.kind {
            input.on_input = Some(listener);
        }
        self
    }

    /// A commit of the field (Enter, a blur) runs `listener`, told its
    /// value.
    pub fn on_change(mut self, listener: ValueListener) -> PasswordField {
        if let NodeKind::PasswordInput(input) = &mut self.0.kind {
            input.on_change = Some(listener);
        }
        self
    }
}

impl TextArea {
    /// The field's placeholder, while it is empty.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> TextArea {
        if let NodeKind::TextArea(input) = &mut self.0.kind {
            input.placeholder = Some(placeholder.into());
        }
        self
    }

    /// The field's default: the value it starts from.
    pub fn default_value(mut self, value: impl Into<String>) -> TextArea {
        if let NodeKind::TextArea(input) = &mut self.0.kind {
            input.default = Some(value.into());
        }
        self
    }

    /// The field's value as the user types it runs `listener`, told it.
    pub fn on_input(mut self, listener: ValueListener) -> TextArea {
        if let NodeKind::TextArea(input) = &mut self.0.kind {
            input.on_input = Some(listener);
        }
        self
    }

    /// A commit of the field (a blur; its Enter inserts a newline) runs
    /// `listener`, told its value.
    pub fn on_change(mut self, listener: ValueListener) -> TextArea {
        if let NodeKind::TextArea(input) = &mut self.0.kind {
            input.on_change = Some(listener);
        }
        self
    }
}

impl Checkbox {
    /// The checkbox's label, drawn beside its box and naming it.
    pub fn label(mut self, label: impl Into<String>) -> Checkbox {
        if let NodeKind::Checkbox(checkbox) = &mut self.0.kind {
            checkbox.label = Some(label.into());
        }
        self
    }

    /// The checkbox's default: whether it starts on.
    pub fn default_value(mut self, on: bool) -> Checkbox {
        if let NodeKind::Checkbox(checkbox) = &mut self.0.kind {
            checkbox.on = on;
        }
        self
    }

    /// A change of the checkbox runs `listener`, told the state it now
    /// is.
    pub fn on_change(mut self, listener: Listener) -> Checkbox {
        if let NodeKind::Checkbox(checkbox) = &mut self.0.kind {
            checkbox.on_click = Some(listener);
        }
        self
    }
}

impl Toggle {
    /// The toggle's label, drawn beside its switch and naming it.
    pub fn label(mut self, label: impl Into<String>) -> Toggle {
        if let NodeKind::Toggle(toggle) = &mut self.0.kind {
            toggle.label = Some(label.into());
        }
        self
    }

    /// The toggle's default: whether it starts on.
    pub fn default_value(mut self, on: bool) -> Toggle {
        if let NodeKind::Toggle(toggle) = &mut self.0.kind {
            toggle.on = on;
        }
        self
    }

    /// A change of the toggle runs `listener`, told the state it now is.
    pub fn on_change(mut self, listener: Listener) -> Toggle {
        if let NodeKind::Toggle(toggle) = &mut self.0.kind {
            toggle.on_click = Some(listener);
        }
        self
    }
}

impl DatePicker {
    /// The field's placeholder: the format its value takes.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> DatePicker {
        if let NodeKind::DatePicker(date) = &mut self.0.kind {
            date.placeholder = Some(placeholder.into());
        }
        self
    }

    /// The field's default: the date ("YYYY-MM-DD") it starts from.
    pub fn default_value(mut self, value: impl Into<String>) -> DatePicker {
        if let NodeKind::DatePicker(date) = &mut self.0.kind {
            date.default = Some(value.into());
        }
        self
    }

    /// A commit of the field (Enter, a blur) runs `listener`, told its
    /// value.
    pub fn on_change(mut self, listener: ValueListener) -> DatePicker {
        if let NodeKind::DatePicker(date) = &mut self.0.kind {
            date.on_change = Some(listener);
        }
        self
    }
}

impl DateTimePicker {
    /// The field's placeholder: the format its value takes.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> DateTimePicker {
        if let NodeKind::DateTimePicker(date) = &mut self.0.kind {
            date.placeholder = Some(placeholder.into());
        }
        self
    }

    /// The field's default: the date and time ("YYYY-MM-DD HH:MM") it
    /// starts from.
    pub fn default_value(mut self, value: impl Into<String>) -> DateTimePicker {
        if let NodeKind::DateTimePicker(date) = &mut self.0.kind {
            date.default = Some(value.into());
        }
        self
    }

    /// A commit of the field (Enter, a blur) runs `listener`, told its
    /// value.
    pub fn on_change(mut self, listener: ValueListener) -> DateTimePicker {
        if let NodeKind::DateTimePicker(date) = &mut self.0.kind {
            date.on_change = Some(listener);
        }
        self
    }
}

impl Dropdown {
    /// One option of the dropdown, named `value` and drawn as `label`.
    pub fn option(mut self, option: crate::view::Choice) -> Dropdown {
        if let NodeKind::Select(select) = &mut self.0.kind {
            select.options.push(option);
        }
        self
    }

    /// The dropdown's options, after any it already has.
    pub fn options(mut self, options: impl IntoIterator<Item = crate::view::Choice>) -> Dropdown {
        if let NodeKind::Select(select) = &mut self.0.kind {
            select.options.extend(options);
        }
        self
    }

    /// The dropdown's placeholder, while no option is chosen.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Dropdown {
        if let NodeKind::Select(select) = &mut self.0.kind {
            select.placeholder = Some(placeholder.into());
        }
        self
    }

    /// The dropdown's default: the option (its value) it starts from.
    pub fn default_value(mut self, value: impl Into<String>) -> Dropdown {
        if let NodeKind::Select(select) = &mut self.0.kind {
            select.value = Some(value.into());
        }
        self
    }

    /// A choice of the dropdown runs `listener`, told the option's
    /// value.
    pub fn on_change(mut self, listener: Listener) -> Dropdown {
        if let NodeKind::Select(select) = &mut self.0.kind {
            select.on_click = Some(listener);
        }
        self
    }

    /// The search of the dropdown is the extension's: Pane tells it the
    /// query as the user types it — `listener`, told it — and shows the
    /// options the view answers with, filtering none of its own.
    pub fn on_search_text(mut self, listener: ValueListener) -> Dropdown {
        if let NodeKind::Select(select) = &mut self.0.kind {
            select.on_input = Some(listener);
            select.search = false;
        }
        self
    }
}

impl TagPicker {
    /// One option of the tag picker, named `value` and drawn as `label`.
    pub fn option(mut self, option: crate::view::Choice) -> TagPicker {
        if let NodeKind::TagPicker(picker) = &mut self.0.kind {
            picker.options.push(option);
        }
        self
    }

    /// The tag picker's options, after any it already has.
    pub fn options(mut self, options: impl IntoIterator<Item = crate::view::Choice>) -> TagPicker {
        if let NodeKind::TagPicker(picker) = &mut self.0.kind {
            picker.options.extend(options);
        }
        self
    }

    /// The tag picker's default: the tags (their options' values) it
    /// starts from.
    pub fn default_value(
        mut self,
        values: impl IntoIterator<Item = impl Into<String>>,
    ) -> TagPicker {
        if let NodeKind::TagPicker(picker) = &mut self.0.kind {
            picker.default = values.into_iter().map(Into::into).collect();
        }
        self
    }

    /// A change of the chosen tags runs `listener`, told them all.
    pub fn on_change(mut self, listener: Listener) -> TagPicker {
        if let NodeKind::TagPicker(picker) = &mut self.0.kind {
            picker.on_change = Some(listener);
        }
        self
    }
}

impl FilePicker {
    /// The picker chooses several paths; its value in a submission is
    /// the list of them.
    pub fn allow_multiple(mut self) -> FilePicker {
        if let NodeKind::FilePicker(picker) = &mut self.0.kind {
            picker.multiple = true;
        }
        self
    }

    /// The picker's default: the path it starts from.
    pub fn default_value(mut self, value: impl Into<String>) -> FilePicker {
        if let NodeKind::FilePicker(picker) = &mut self.0.kind {
            picker.default = Some(value.into());
        }
        self
    }

    /// A commit of the field (Enter, a blur) runs `listener`, told its
    /// value.
    pub fn on_change(mut self, listener: ValueListener) -> FilePicker {
        if let NodeKind::FilePicker(picker) = &mut self.0.kind {
            picker.on_change = Some(listener);
        }
        self
    }
}

impl FolderPicker {
    /// The picker chooses several paths; its value in a submission is
    /// the list of them.
    pub fn allow_multiple(mut self) -> FolderPicker {
        if let NodeKind::FolderPicker(picker) = &mut self.0.kind {
            picker.multiple = true;
        }
        self
    }

    /// The picker's default: the path it starts from.
    pub fn default_value(mut self, value: impl Into<String>) -> FolderPicker {
        if let NodeKind::FolderPicker(picker) = &mut self.0.kind {
            picker.default = Some(value.into());
        }
        self
    }

    /// A commit of the field (Enter, a blur) runs `listener`, told its
    /// value.
    pub fn on_change(mut self, listener: ValueListener) -> FolderPicker {
        if let NodeKind::FolderPicker(picker) = &mut self.0.kind {
            picker.on_change = Some(listener);
        }
        self
    }
}
