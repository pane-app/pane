//! The forms Pane itself asks the user, as designed trees (#241): the
//! argument form (#144), the Setup screen (#143), the alias form, and the
//! npm and Git install forms. Every form an extension asks is a designed
//! tree too now — its fields are the tree's field components and its
//! submission an event — so Pane's own forms are what they always were,
//! drawn by the same renderer through the same keyed reconciler, with one
//! difference: no extension answers their submission. The tree's `form`
//! node names no `onSubmit`; the window collects the fields' values (as
//! it collects an extension form's) and hands them to
//! [`Launcher::submit_pane_form`], which does what the open form's
//! purpose does: the argument form launches its command, the Setup screen
//! saves its preferences and launches, the alias form applies at once, the
//! npm and Git forms preview the package they name.
//!
//! What each purpose shows is built here as a tree: one `form` node whose
//! children are the fields — a text field, a password field, a dropdown,
//! a path field with the system's picker, a checkbox — each with its
//! `title`, `placeholder` and `info`, and the form's `submitLabel`. A
//! required argument or preference that is empty asks for the keyboard
//! (`focus`), so the form opens with it; a submission that leaves one
//! empty sets its `error` and asks for it again, moving the keyboard
//! there. What a discrete control chooses (a dropdown's option, a
//! checkbox's state) is told to [`Launcher::pane_form_changed`], which
//! moves it into the tree the screen holds, exactly as an extension's
//! change event moves it into the tree its answer holds.

use super::{FormPurpose, Launcher, LauncherView, OpenForm, Screen, State, Status};
use crate::runtime::{
    Checkbox, DesignedTree, FieldProps, FilePicker, FormNode, Layout, Node, NodeKind, Segment,
    Select, TextInput,
};

/// A form Pane itself asks the user, as its tree says: the argument form,
/// the Setup screen, the alias form, or the npm or Git install form
/// (`Screen::PaneForm`). The tree's `form` node names no `onSubmit` —
/// Pane itself answers its submission, through
/// [`Launcher::submit_pane_form`].
#[derive(Clone, Debug, PartialEq)]
pub struct PaneForm {
    /// Which open form this is: a number given as each form opens, so the
    /// window's keyed state follows one whose tree changes (a field's
    /// error set) and leaves with the form.
    pub id: u64,
    /// The form's tree: a `form` node with its fields, and the header the
    /// screen shows above them.
    pub tree: DesignedTree,
    /// The form's submit button, from its tree's `form` node: the label
    /// the window draws on the button Enter runs.
    pub submit: String,
    /// The identity key of the package whose form this is, whose icons
    /// load; `None` for a form about none.
    pub owner: Option<String>,
    /// Whether the tree holds an icon still loading, so the window draws
    /// it when it arrives.
    pub loading: bool,
}

/// One field of a form Pane itself asks, before it is a tree node: what
/// the field is, what it says around it, and what it starts from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneFormField {
    /// The field's key: where its value is in the submission.
    pub key: String,
    /// What the field is.
    pub kind: PaneFieldKind,
    /// Its title, over its control.
    pub title: String,
    /// Its placeholder.
    pub placeholder: Option<String>,
    /// Its note, under its control.
    pub info: Option<String>,
    /// What the field starts from: a text field's text, or the id of the
    /// option chosen first.
    pub value: String,
    /// Whether the form needs a value in it: a required argument or
    /// preference, whose first empty one the form opens with and a
    /// rejected submission takes the keyboard to.
    pub required: bool,
}

/// What one field of a Pane form is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaneFieldKind {
    /// A single-line text field.
    Text,
    /// A single-line text field whose text is concealed while it is typed.
    Password,
    /// A dropdown: exactly one of these options, `(value, label)` pairs.
    Choice(Vec<(String, String)>),
    /// A checkbox, drawn beside its label.
    Check(String),
    /// A path, typed or chosen with the system's picker for `pick`.
    Path(PathPick),
}

/// What a path field's picker chooses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathPick {
    File,
    Folder,
    /// An application: a program's file, or on macOS its bundle (a
    /// folder).
    Application,
}

impl PaneForm {
    /// The form of `fields`, titled by its root's `navigationTitle`, its
    /// submit button saying `submit`: a `form` node of the fields, with
    /// the first required empty one asking for the keyboard. `header`, if
    /// given, is drawn over the fields.
    pub(super) fn of(
        id: u64,
        title: &str,
        submit: &str,
        fields: Vec<PaneFormField>,
        header: Option<Node>,
    ) -> PaneForm {
        let mut children: Vec<Node> = Vec::with_capacity(fields.len() + 1);
        if let Some(header) = header {
            children.push(header);
        }
        let mut focus_next_required = true;
        for field in &fields {
            let mut node = field.node();
            if field.required && field.value.trim().is_empty() && focus_next_required {
                // The form opens with the keyboard on the first required
                // field that is empty, as it always has.
                node.focus = true;
                focus_next_required = false;
            }
            children.push(node);
        }
        let form = Node {
            kind: NodeKind::Form(FormNode {
                on_submit: None,
                submit_label: Some(submit.to_owned()),
            }),
            key: Some("form".into()),
            children,
            ..Node::plain()
        };
        let tree = DesignedTree {
            root: Node {
                kind: NodeKind::Column(Layout::default()),
                navigation_title: Some(title.to_owned()),
                key: Some("screen".into()),
                children: vec![form],
                ..Node::plain()
            },
        };
        PaneForm {
            id,
            tree,
            submit: submit.to_owned(),
            owner: None,
            loading: false,
        }
    }

    /// Resolves the tree's icons in the package `identity`'s folder and
    /// starts their loads, as a designed view's tree landing does: the
    /// form never waits for them, drawing what arrives.
    pub(super) fn land(&mut self, state: &State, identity: &crate::packages::PackageIdentity) {
        self.owner = Some(identity.key());
        self.loading = super::designed_views::land_pane_form(
            &state.packages,
            &state.icon_loads,
            identity,
            &mut self.tree,
        );
    }
}

impl PaneFormField {
    /// The field as a tree node: its key, title, placeholder, note and
    /// starting value.
    pub(super) fn node(&self) -> Node {
        let field = FieldProps {
            title: Some(self.title.clone()),
            info: self.info.clone(),
            error: None,
            remember: false,
        };
        let label = Some(self.title.clone());
        let kind = match &self.kind {
            PaneFieldKind::Text => NodeKind::TextInput(TextInput {
                value: self.value.clone(),
                placeholder: self.placeholder.clone(),
                label,
                field,
                ..TextInput::default()
            }),
            PaneFieldKind::Password => NodeKind::PasswordInput(TextInput {
                value: self.value.clone(),
                placeholder: self.placeholder.clone(),
                label,
                field,
                ..TextInput::default()
            }),
            PaneFieldKind::Choice(options) => NodeKind::Select(Select {
                options: options
                    .iter()
                    .map(|(value, label)| Segment {
                        value: value.clone(),
                        label: Some(label.clone()),
                        section: None,
                    })
                    .collect(),
                value: (!self.value.is_empty()).then(|| self.value.clone()),
                label,
                field,
                ..Select::default()
            }),
            PaneFieldKind::Check(label) => NodeKind::Checkbox(Checkbox {
                checked: self.value == "true",
                label: Some(label.clone()),
                field,
                ..Checkbox::default()
            }),
            PaneFieldKind::Path(pick) => path_node(*pick, self, field),
        };
        Node {
            kind,
            key: Some(self.key.clone()),
            ..Node::plain()
        }
    }
}

/// A path field for `pick`, as a `file-picker` or `folder-picker` node: a
/// program's file or a macOS bundle is chosen by the file picker, as the
/// Settings card's "Choose…" does (a folder there chooses a bundle).
fn path_node(pick: PathPick, field: &PaneFormField, props: FieldProps) -> NodeKind {
    let picker = FilePicker {
        paths: if field.value.is_empty() {
            Vec::new()
        } else {
            vec![field.value.clone()]
        },
        multiple: false,
        field: props,
    };
    match pick {
        PathPick::Folder => NodeKind::FolderPicker(picker),
        PathPick::File | PathPick::Application => NodeKind::FilePicker(picker),
    }
}

impl Node {
    /// A node with nothing but its kind: the fields' own properties fill
    /// the rest.
    pub(super) fn plain() -> Node {
        Node {
            kind: NodeKind::Spacer,
            style: crate::runtime::Style::default(),
            place: None,
            offset: None,
            key: None,
            name: None,
            navigation_title: None,
            requires: None,
            fallback: None,
            children: Vec::new(),
            focus: false,
            on_focus: None,
            on_blur: None,
            on_key: None,
        }
    }
}

impl Launcher {
    /// Notes what the discrete control of the open Pane form keyed `key`
    /// now holds — a dropdown's choice, a checkbox's state — moving it
    /// into the tree the screen holds, as an extension's change event
    /// moves it into the tree its answer holds. Text fields tell nothing
    /// per keystroke: their values are collected at submission. A field's
    /// error goes with its change. Unknown fields and choices are ignored.
    pub fn pane_form_changed(&self, key: &str, value: &str) {
        let mut state = self.lock();
        let super::Screen::PaneForm(form) = &mut state.view.screen else {
            return;
        };
        if !set_field(&mut form.tree, key, value) {
            return;
        }
        self.changed();
    }

    /// Submits the open Pane form with the values `values` collected from
    /// its fields (keyed by their keys). Await the returned future to
    /// apply what its purpose does: the argument form validates and
    /// launches, the Setup screen saves and launches, the alias form
    /// applies, the npm and Git forms preview. While a submission is
    /// waiting for its reply, submitting again does nothing.
    pub fn submit_pane_form(
        &self,
        values: Vec<(String, String)>,
    ) -> impl Future<Output = ()> + Send + 'static {
        let mut state = self.lock();
        let state = &mut *state;
        // The Setup screen saves the preferences it asks for, then
        // launches the command it held back (see `setup`).
        let setup = self.begin_setup_submit(state, &values);
        let arguments = self.submit_arguments(state, &values);
        let alias_change = match (&state.view.screen, &state.form) {
            (
                super::Screen::PaneForm(_),
                Some(super::OpenForm {
                    purpose: super::FormPurpose::Alias(command),
                    ..
                }),
            ) => {
                let command = command.clone();
                let alias = alias_of(&values).unwrap_or_default();
                self.submit_alias(state, &command, alias.trim())
            }
            _ => None,
        };
        // Pane's own Create Extension form: the package is written from
        // the templates, built once and handed to the install preview the
        // author confirms, after which Pane develops it (see `create`).
        let creation = super::Launcher::begun_creation(state, &values);
        // Pane's own npm and Git forms preview the package they name; the
        // form stays until the preview replaces it, and Back meanwhile
        // discards it.
        let named = match (&state.view.screen, &mut state.form) {
            (
                super::Screen::PaneForm(_),
                Some(
                    open @ super::OpenForm {
                        purpose: super::FormPurpose::Npm | super::FormPurpose::Git,
                        submitting: false,
                        ..
                    },
                ),
            ) => {
                open.submitting = true;
                state.view.status = Status::Running {
                    since: std::time::Instant::now(),
                };
                let git = matches!(open.purpose, super::FormPurpose::Git);
                let field = if git {
                    super::GIT_REPOSITORY_FIELD
                } else {
                    super::NPM_PACKAGE_FIELD
                };
                values
                    .iter()
                    .find(|(key, _)| key == field)
                    .map(|(_, spec)| (git, spec.clone()))
            }
            _ => None,
        };
        let epoch = state.screen_epoch;
        let launcher = self.clone();
        async move {
            if let Some(setup) = setup {
                launcher.finish_setup(epoch, setup).await;
            }
            if let Some(submitted) = arguments {
                launcher.launch_submitted(submitted).await;
            }
            if let Some(change) = alias_change {
                launcher.finish_choice_change(change).await;
            }
            match named {
                Some((false, spec)) => launcher.preview_npm(&spec).await,
                Some((true, spec)) => launcher.preview_git(&spec).await,
                None => {}
            }
            if let Some(creation) = creation {
                launcher.finish_creation(epoch, creation).await;
            }
        }
    }
}

/// Shows `form`, titled `title`, over what is on screen, its submission
/// `purpose`: Back returns to what the form was shown over. The form takes
/// a fresh identity, so the window's keyed state starts afresh with it.
pub(super) fn open_pane_form(
    state: &mut State,
    purpose: FormPurpose,
    form: PaneForm,
    title: String,
) {
    let view = LauncherView::new(Screen::PaneForm(form), title);
    let return_to = std::mem::replace(&mut state.view, view);
    state.form = Some(OpenForm {
        purpose,
        return_to,
        submitting: false,
    });
    state.next_screen();
}

/// The alias of an alias form's values: its field's value.
fn alias_of(values: &[(String, String)]) -> Option<String> {
    values
        .iter()
        .find(|(key, _)| key == super::aliases::ALIAS_FIELD)
        .map(|(_, value)| value.clone())
}

/// Moves `value` into the field keyed `key` of `tree`'s form: a
/// dropdown's chosen option (an unknown one is ignored, as the typed
/// form's reading always did), or a checkbox's state. Whether the field
/// was found and moved; a field's error goes with its change.
fn set_field(tree: &mut DesignedTree, key: &str, value: &str) -> bool {
    fn at(node: &mut Node, key: &str, value: &str) -> bool {
        if node.key.as_deref() == Some(key) {
            let marked = match &mut node.kind {
                NodeKind::Select(select) => {
                    if !select.options.iter().any(|option| option.value == value) {
                        // A choice the form does not offer is ignored, as
                        // the typed form's reading always did.
                        return false;
                    }
                    select.value = Some(value.to_owned());
                    select.field.error = None;
                    true
                }
                NodeKind::Checkbox(checkbox) => {
                    checkbox.checked = value == "true";
                    checkbox.field.error = None;
                    true
                }
                _ => false,
            };
            return marked;
        }
        if let Some(fallback) = &mut node.fallback
            && at(fallback, key, value)
        {
            return true;
        }
        node.children.iter_mut().any(|child| at(child, key, value))
    }
    at(&mut tree.root, key, value)
}

/// Why a field of a Pane form is marked, after a submission that left it
/// empty or unusable: `message`, shown as the field's error with the
/// keyboard moved to it (`focus`, an ask the tree did not name before).
/// Whether the field was found.
pub(super) fn mark_error(tree: &mut DesignedTree, key: &str, message: &str) -> bool {
    fn at(node: &mut Node, key: &str, message: &str) -> bool {
        if node.key.as_deref() == Some(key) {
            let marked = match &mut node.kind {
                NodeKind::TextInput(input) | NodeKind::PasswordInput(input) => {
                    input.field.error = Some(message.to_owned());
                    true
                }
                NodeKind::Select(select) => {
                    select.field.error = Some(message.to_owned());
                    true
                }
                NodeKind::Checkbox(checkbox) => {
                    checkbox.field.error = Some(message.to_owned());
                    true
                }
                NodeKind::DatePicker(date) | NodeKind::DateTimePicker(date) => {
                    date.field.error = Some(message.to_owned());
                    true
                }
                NodeKind::TagPicker(picker) => {
                    picker.field.error = Some(message.to_owned());
                    true
                }
                NodeKind::FilePicker(picker) | NodeKind::FolderPicker(picker) => {
                    picker.field.error = Some(message.to_owned());
                    true
                }
                _ => false,
            };
            if marked {
                node.focus = true;
            }
            return marked;
        }
        if let Some(fallback) = &mut node.fallback
            && at(fallback, key, message)
        {
            return true;
        }
        node.children
            .iter_mut()
            .any(|child| at(child, key, message))
    }
    at(&mut tree.root, key, message)
}

/// The label of the first form node `tree` holds — what its submit
/// button says, "Submit" when the tree names none — or `None` when the
/// tree holds no form: what a screen with a form shows as its primary
/// action, as the footer's button does.
pub(super) fn form_submit_of(tree: &DesignedTree) -> Option<String> {
    fn at(node: &Node) -> Option<String> {
        if let NodeKind::Form(form) = &node.kind {
            return Some(form.submit_label.clone().unwrap_or_else(|| "Submit".into()));
        }
        if let Some(fallback) = &node.fallback
            && let Some(label) = at(fallback)
        {
            return Some(label);
        }
        node.children.iter().find_map(at)
    }
    at(&tree.root)
}
