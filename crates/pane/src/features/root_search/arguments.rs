//! Root search's inline argument fields (#205): the window's controls for
//! them, their keys and how they draw, beside the query in the search
//! field. The launcher holds the values and the fields' shape
//! ([`Launcher::argument_fields`], see the core's `argument_fields`); the
//! window owns the focus, as the specification gives it:
//!
//! - **Tab** from the query focuses the first empty argument (the first,
//!   when none is empty), **Shift+Tab** the last; Tab inside an argument
//!   goes to the next, from the last back to the query, and Shift+Tab to
//!   the previous, from the first back to the query. **Left** and
//!   **Right** cross a field's edge — Left at the start goes to the field
//!   before it, Right at the end to the one after — which needs the
//!   caret's place: the fields' and the query's navigation keys are bound
//!   here over the editable text element's own, so every move the window
//!   does not make itself is one it counts. Up and Down do nothing while
//!   an argument field has the keys: the list does not move while the
//!   user types into one. **Escape** returns focus to the query with its
//!   text selected.
//! - **Enter** inside a focused blank required argument marks it (the
//!   launcher says "Enter <placeholder>" in the status line) and runs
//!   nothing; with a blank required argument anywhere in the fields,
//!   invoking the row — Enter, a click, the footer's button, Ctrl and a
//!   digit — focuses the first blank one instead of running.
//! - A **dropdown** shows its choice or placeholder on a trigger that
//!   Enter, Space or Down opens, listing a leading empty choice then its
//!   options: the arrows move the highlight, Enter picks, Escape or Tab
//!   closes.
//! - A **password** field is drawn as dots over transparent text and
//!   reads as dots to assistive technology.
//! - An **optional** argument carries an "optional" marker; a required
//!   one left blank once carries the missing mark, the danger ring, until
//!   it gets a value.
//!
//! Each field is a labelled editable node inside the search combo box's
//! group, named by its placeholder, with its required or optional state
//! as its description. The fields draw with the Settings field family
//! (`ui::controls`' wells), sized to their value or placeholder, as the
//! reference's inline arguments are.

use gpui::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, KeyBinding, NavigationDirection,
    Role, SharedString, Stateful, Window, actions, anchored, deferred, div, prelude::*, px,
};
use gpui_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT;
use gpui_elements::editable_text::{
    EditableTextState, StringStorage, TextBoundary, TextChanged, text_input,
};
use pane_core::{AliasFlow, ArgumentFields, FieldKind, FormField};

use crate::app::LauncherWindow;
use crate::ui::controls;
use crate::ui::icon::{Glyph, glyph_rotated};
use crate::ui::input::TextEditingKeys;
use crate::ui::material::Material;
use crate::ui::theme::Theme;

/// The argument fields' context: the fields' own keys are bound above the
/// editable text element's, in the nesting the query field's selection
/// keys use (`root_search::field_context`), so an argument field takes
/// the keys the query's selection does not.
pub(crate) const CONTEXT: &str = "ArgumentField";
/// A dropdown's trigger, whose Enter, Space and Down open its choices.
const TRIGGER: &str = "ArgumentTrigger";
/// A dropdown's open list of choices.
const CHOICES: &str = "ArgumentChoices";

/// What a required argument left blank says, as the argument form does
/// (`argument_form.rs` in the core); the tests keep the two in step.
pub(crate) const MISSING: &str = "Enter a value to run the command";

/// The label of the empty choice a dropdown lists before its options:
/// choosing it leaves the argument without a value.
const EMPTY_CHOICE: &str = "None";

/// The character a password field shows for each character typed, as the
/// form's password fields do.
const CONCEALED: char = '•';

actions!(
    root_search_arguments,
    [
        /// Left in the query or an argument field.
        FieldLeft,
        /// Right in the query or an argument field.
        FieldRight,
        /// Home in the query or an argument field.
        FieldHome,
        /// End in the query or an argument field.
        FieldEnd,
        /// Up or Down in an argument field: nothing.
        NothingInField,
        /// Enter, Space or Down on a dropdown's trigger: open its choices.
        OpenArgumentChoices,
        /// Down in a dropdown's open choices.
        NextArgumentChoice,
        /// Up in a dropdown's open choices.
        PreviousArgumentChoice,
        /// Enter in a dropdown's open choices: pick the highlighted one.
        ChooseArgumentChoice,
        /// Escape or Tab in a dropdown's open choices: close it.
        CloseArgumentChoices
    ]
);

/// Registers the argument fields' key bindings (#205): the caret keys in
/// the query and the argument fields, whose places the window counts to
/// cross the fields' edges, and the dropdown's own keys. They come after
/// the shared text editing keys, so they take precedence over the
/// element's own, as the query field's selection keys do.
pub(crate) fn bind_keys(cx: &mut App, _: &TextEditingKeys) {
    let query = crate::features::root_search::field_context();
    let field = format!("{CONTEXT} > {DEFAULT_INPUT_CONTEXT}");
    cx.bind_keys([
        KeyBinding::new("left", FieldLeft, Some(&query)),
        KeyBinding::new("right", FieldRight, Some(&query)),
        KeyBinding::new("home", FieldHome, Some(&query)),
        KeyBinding::new("end", FieldEnd, Some(&query)),
        KeyBinding::new("left", FieldLeft, Some(&field)),
        KeyBinding::new("right", FieldRight, Some(&field)),
        KeyBinding::new("home", FieldHome, Some(&field)),
        KeyBinding::new("end", FieldEnd, Some(&field)),
        // Up and Down do nothing in an argument field: the list does not
        // move while the user types into one.
        KeyBinding::new("up", NothingInField, Some(&field)),
        KeyBinding::new("down", NothingInField, Some(&field)),
        KeyBinding::new("enter", OpenArgumentChoices, Some(TRIGGER)),
        KeyBinding::new("space", OpenArgumentChoices, Some(TRIGGER)),
        KeyBinding::new("down", OpenArgumentChoices, Some(TRIGGER)),
        KeyBinding::new("down", NextArgumentChoice, Some(CHOICES)),
        KeyBinding::new("up", PreviousArgumentChoice, Some(CHOICES)),
        KeyBinding::new("enter", ChooseArgumentChoice, Some(CHOICES)),
        KeyBinding::new("escape", CloseArgumentChoices, Some(CHOICES)),
        KeyBinding::new("tab", CloseArgumentChoices, Some(CHOICES)),
    ]);
}

/// One argument field's control, as the window holds it.
enum Control {
    /// A text or password field's editable text, and the caret's place in
    /// it, in characters, as the window last knew it.
    Text {
        input: Entity<EditableTextState>,
        caret: usize,
    },
    /// A dropdown: its trigger's focus (a tab stop among the fields) and
    /// its open list's.
    Choice {
        trigger: FocusHandle,
        list: FocusHandle,
    },
}

/// The argument fields' controls, while root search's selected row shows
/// its fields (#205): one per argument the command declares, made for the
/// fields they were (their ids and which are dropdowns), and the open
/// dropdown's list.
pub(crate) struct ArgumentControls {
    /// The fields' controls, in the order the command declares them.
    fields: Vec<Control>,
    /// What the controls were made for: each field's id, and whether it
    /// is a dropdown. The values change; these do not, and a change in
    /// them remakes the controls.
    shape: Vec<(String, bool)>,
    /// The dropdown whose choices are open, and the choice highlighted in
    /// them: navigation only, never the value, which the launcher holds.
    open: Option<(usize, usize)>,
    _subscriptions: Vec<gpui::Subscription>,
}

/// What the controls are made for, as `fields` writes them: each field's
/// id, and whether it is a dropdown.
fn shape_of(fields: &ArgumentFields) -> Vec<(String, bool)> {
    fields
        .fields
        .iter()
        .map(|field| (field.id.clone(), matches!(field.kind, FieldKind::Choice(_))))
        .collect()
}

impl ArgumentControls {
    /// The controls for `fields`'s arguments, their values filled in and
    /// their typing reported to the launcher.
    fn new(
        fields: &ArgumentFields,
        window: &mut Window,
        cx: &mut Context<LauncherWindow>,
    ) -> ArgumentControls {
        let mut controls = Vec::new();
        let mut subscriptions = Vec::new();
        for field in &fields.fields {
            match &field.kind {
                FieldKind::Text { .. } | FieldKind::Password { .. } => {
                    let input = cx.new(|cx| EditableTextState::new(StringStorage::default(), cx));
                    input.focus_handle(cx).tab_stop(true);
                    if !field.value.is_empty() {
                        input.update(cx, |input, cx| input.emplace(&field.value, cx));
                    }
                    let name = field.id.clone();
                    subscriptions.push(cx.on_blur(
                        &input.focus_handle(cx),
                        window,
                        move |this, _, cx| {
                            // The field was left: a required one left blank
                            // is marked from now on (#205).
                            this.launcher.argument_left(&name);
                            cx.notify();
                        },
                    ));
                    subscriptions.push(cx.subscribe(
                        &input,
                        move |this, input, _: &TextChanged, cx| {
                            // The value typed is the launcher's, to invoke
                            // the command with (#205); the caret is at the
                            // text's end, where typing leaves it.
                            let value = input.read(cx).as_str();
                            this.launcher.set_argument_value(&name, value);
                            this.argument_typed(&name, value.chars().count());
                            cx.notify();
                        },
                    ));
                    controls.push(Control::Text {
                        input,
                        caret: field.value.chars().count(),
                    });
                }
                FieldKind::Choice(_) => {
                    let trigger = cx.focus_handle().tab_stop(true);
                    let list = cx.focus_handle();
                    let name = field.id.clone();
                    subscriptions.push(cx.on_blur(&trigger, window, move |this, _, cx| {
                        // The field was left: a required one left blank
                        // is marked from now on (#205).
                        this.launcher.argument_left(&name);
                        cx.notify();
                    }));
                    controls.push(Control::Choice { trigger, list });
                }
                // An argument is text, a password or a dropdown; a path is
                // a preference's.
                FieldKind::Path { .. } => unreachable!("an argument is never a path"),
            }
        }
        // A dropdown's open list closes when the window is left, as the
        // footer menu's popup does.
        subscriptions.push(cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.close_argument_choices(window, cx);
            }
        }));
        ArgumentControls {
            fields: controls,
            shape: shape_of(fields),
            open: None,
            _subscriptions: subscriptions,
        }
    }

    /// The place of the argument `name`, if it has a control.
    fn at(&self, name: &str) -> Option<usize> {
        self.shape.iter().position(|(field, _)| field == name)
    }

    /// Notes the caret of the argument `name` at `caret`, as its typing
    /// left it.
    fn typed(&mut self, name: &str, caret: usize) {
        if let Some(index) = self.at(name)
            && let Some(control) = self.fields.get_mut(index)
        {
            if let Control::Text { caret: known, .. } = control {
                *known = caret;
            }
        }
    }

    /// The argument `index`'s editable text, if it is a typed one.
    fn input(&self, index: usize) -> Option<Entity<EditableTextState>> {
        match self.fields.get(index) {
            Some(Control::Text { input, .. }) => Some(input.clone()),
            _ => None,
        }
    }

    /// The caret the window knows in the argument `index`, if it is a
    /// typed one.
    fn caret(&self, index: usize) -> Option<usize> {
        match self.fields.get(index) {
            Some(Control::Text { caret, .. }) => Some(*caret),
            _ => None,
        }
    }

    /// The focus of the argument `index`'s control: its field's, or its
    /// open list's.
    fn focus(&self, index: usize, cx: &App) -> Option<FocusHandle> {
        match self.fields.get(index) {
            Some(Control::Text { input, .. }) => Some(input.focus_handle(cx)),
            Some(Control::Choice { trigger, .. }) => Some(trigger.clone()),
            _ => None,
        }
    }
}

impl LauncherWindow {
    /// Makes the argument fields' controls follow the launcher (#205):
    /// the selected row's fields, when its command declares arguments,
    /// get controls made for them — kept while the fields are the same,
    /// their values replaced from the launcher when something other than
    /// typing changed them (another row selected with the same arguments,
    /// a remembered dropdown offered) — and the controls go when the
    /// fields do, the query taking back the focus a field had.
    pub(crate) fn sync_arguments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let fields = self.launcher.argument_fields();
        let same = self.arguments.as_ref().is_some_and(|controls| {
            fields
                .as_ref()
                .is_some_and(|fields| controls.shape == shape_of(fields))
        });
        let had_focus = self.argument_had_focus(window, cx);
        match (&fields, same) {
            // The fields stand: the values shown are the launcher's.
            (Some(fields), true) => {
                let inputs: Vec<(usize, Entity<EditableTextState>, String)> = self
                    .arguments
                    .as_ref()
                    .map(|controls| {
                        fields
                            .fields
                            .iter()
                            .enumerate()
                            .filter_map(|(index, field)| {
                                Some((index, controls.input(index)?, field.value.clone()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                for (index, input, value) in inputs {
                    if input.read(cx).as_str() != value {
                        input.update(cx, |input, cx| input.emplace(&value, cx));
                    }
                    if let Some(controls) = self.arguments.as_mut() {
                        controls.typed(&fields.fields[index].id, value.chars().count());
                    }
                }
                // A dropdown whose field is gone closes.
                if let Some((field, _)) = self.arguments.as_ref().and_then(|c| c.open)
                    && field >= fields.fields.len()
                {
                    self.close_argument_choices(window, cx);
                }
            }
            // The fields changed: a field that had the focus gives it back
            // to the query, and the controls are made anew.
            (Some(fields), false) => {
                self.close_argument_choices(window, cx);
                if had_focus {
                    self.query.focus(window, cx);
                }
                self.arguments = Some(ArgumentControls::new(fields, window, cx));
                cx.notify();
            }
            // No fields: the controls go, and the query takes back the
            // focus a field may have held.
            (None, _) => {
                self.close_argument_choices(window, cx);
                if self.arguments.take().is_some() {
                    if had_focus {
                        self.query.focus(window, cx);
                    }
                    cx.notify();
                }
            }
        }
    }

    /// Whether any of the argument fields' controls holds the keyboard —
    /// a dropdown's open list belongs to its field.
    fn argument_had_focus(&self, window: &Window, cx: &App) -> bool {
        self.focused_argument(window, cx).is_some()
            || self.arguments.as_ref().is_some_and(|controls| {
                controls.fields.iter().any(|control| match control {
                    Control::Choice { list, .. } => list.is_focused(window),
                    _ => false,
                })
            })
    }

    /// The argument field that has the keyboard, by its place among the
    /// fields (#205); `None` when the query or anything else has it.
    pub(crate) fn focused_argument(&self, window: &Window, cx: &App) -> Option<usize> {
        self.arguments
            .as_ref()?
            .fields
            .iter()
            .position(|control| match control {
                Control::Text { input, .. } => input.focus_handle(cx).is_focused(window),
                Control::Choice { trigger, .. } => trigger.is_focused(window),
            })
    }

    /// The name of the argument field that has the keyboard and is
    /// required and blank (#205): Enter inside it marks it, and runs
    /// nothing.
    pub(crate) fn focused_blank_argument(&self, window: &Window, cx: &App) -> Option<String> {
        let index = self.focused_argument(window, cx)?;
        let field = self
            .launcher
            .argument_fields()?
            .fields
            .into_iter()
            .nth(index)?;
        (field.required && field.value.trim().is_empty()).then(|| field.id)
    }

    /// The first argument field of the selected row's command that is
    /// required and blank (#205): invoking the row focuses it instead of
    /// running, never sending the command half-filled.
    pub(crate) fn blank_required_argument(&self) -> Option<usize> {
        self.launcher
            .argument_fields()?
            .fields
            .iter()
            .position(|field| field.required && field.value.trim().is_empty())
    }

    /// Notes that the argument `name` was typed into, its caret at
    /// `caret` — the fields' own typing, as their inputs report it.
    fn argument_typed(&mut self, name: &str, caret: usize) {
        if let Some(controls) = self.arguments.as_mut() {
            controls.typed(name, caret);
        }
    }

    /// Moves the keyboard to the argument field `index` (#205), when there
    /// is one; the query keeps it otherwise.
    pub(crate) fn focus_argument(&self, index: usize, window: &mut Window, cx: &mut App) {
        let handle = self
            .arguments
            .as_ref()
            .and_then(|controls| controls.focus(index, cx));
        match handle {
            Some(handle) => window.focus(&handle, cx),
            None => self.query.focus(window, cx),
        }
    }

    /// Focuses the first empty argument field of the selected row's
    /// command — the first, when none is empty — as Tab from the query
    /// and an alias followed by a space (or Tab) enter the fields (#205).
    pub(crate) fn enter_arguments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_arguments(window, cx);
        let index = self
            .launcher
            .argument_fields()
            .map(|fields| {
                fields
                    .fields
                    .iter()
                    .position(|field| field.value.trim().is_empty())
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        self.focus_argument(index, window, cx);
        cx.notify();
    }

    /// What the alias the query just became followed by a space opens
    /// (#205): the command's fields, entered at their first empty one, or
    /// the command itself, opened as Enter opens it.
    pub(crate) fn alias_opened(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.launcher.alias_after_space() {
            Some(AliasFlow::Fields) => self.enter_arguments(window, cx),
            Some(AliasFlow::Opens) => self.activate_selected(window, cx),
            None => {}
        }
    }

    /// Escape from an argument field (#205): the focus returns to the
    /// query with its text selected; the next Escape is the usual one.
    pub(crate) fn escape_argument(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query.focus(window, cx);
        self.query_field()
            .update(cx, |input, cx| input.select_document(cx));
        // The selection's caret is at its start.
        self.query_caret = 0;
        cx.notify();
    }

    /// The fields of the selected row as the window shows them — their
    /// own controls drawn, `None` while none show; one read of the
    /// launcher's, as a frame's other projections are.
    fn shown_fields(&self) -> Option<ArgumentFields> {
        let fields = self.launcher.argument_fields()?;
        (self.arguments.is_some()).then_some(fields)
    }

    /// Opens the choices of the dropdown at field `index` (#205),
    /// highlighting its chosen option, else the empty choice the list
    /// leads with.
    fn open_argument_choices(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self
            .shown_fields()
            .and_then(|fields| fields.fields.into_iter().nth(index))
        else {
            return;
        };
        let FieldKind::Choice(choices) = &field.kind else {
            return;
        };
        let highlighted = 1 + choices
            .iter()
            .position(|choice| choice.id == field.value)
            .unwrap_or(0);
        if let Some(controls) = self.arguments.as_mut() {
            controls.open = Some((index, highlighted));
            let Some(Control::Choice { list, .. }) = controls.fields.get(index) else {
                return;
            };
            let list = list.clone();
            window.focus(&list, cx);
            cx.notify();
        }
    }

    /// Closes the open dropdown's choices, returning the keyboard to its
    /// trigger (#205).
    fn close_argument_choices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(controls) = self.arguments.as_mut() else {
            return;
        };
        let Some((field, _)) = controls.open.take() else {
            return;
        };
        let Some(Control::Choice { trigger, .. }) = controls.fields.get(field) else {
            return;
        };
        let trigger = trigger.clone();
        window.focus(&trigger, cx);
        cx.notify();
    }

    /// Moves the highlight of the open dropdown's choices by `step`
    /// places (#205), stopping at its ends.
    fn move_argument_choices(&mut self, step: isize, cx: &mut Context<Self>) {
        let Some(open) = self.arguments.as_ref().and_then(|controls| controls.open) else {
            return;
        };
        let Some(FieldKind::Choice(choices)) = self
            .shown_fields()
            .and_then(|fields| fields.fields.into_iter().nth(open.0))
            .map(|field| field.kind)
        else {
            return;
        };
        // The empty choice the list leads with, then the options.
        let count = 1 + choices.len();
        if let Some(controls) = self.arguments.as_mut() {
            if let Some((_, highlighted)) = controls.open.as_mut() {
                *highlighted = (*highlighted).saturating_add_signed(step).min(count - 1);
                cx.notify();
            }
        }
    }

    /// Picks the choice at `position` of the open dropdown's list (#205):
    /// the empty choice the list leads with leaves the argument without a
    /// value.
    fn choose_argument_at(&mut self, position: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.arguments.as_ref().and_then(|controls| controls.open) else {
            return;
        };
        let Some(field) = self
            .shown_fields()
            .and_then(|fields| fields.fields.into_iter().nth(open.0))
        else {
            return;
        };
        let value = match &field.kind {
            FieldKind::Choice(choices) => position
                .checked_sub(1)
                .and_then(|option| choices.get(option))
                .map(|choice| choice.id.clone())
                .unwrap_or_default(),
            _ => return,
        };
        let name = field.id;
        self.close_argument_choices(window, cx);
        self.launcher.set_argument_value(&name, &value);
        cx.notify();
    }

    /// Left in the query field or an argument field (#205): in an
    /// argument, at its start the key goes to the field before it — the
    /// query, from the first — and moves the caret otherwise; the query's
    /// own Left never leaves it, there being nothing before the field.
    pub(crate) fn field_left(
        &mut self,
        _: &FieldLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((index, input)) = self.focused_argument_text(window, cx) {
            let at_start = self
                .arguments
                .as_ref()
                .and_then(|controls| controls.caret(index))
                .is_some_and(|caret| caret == 0);
            if at_start {
                if index == 0 {
                    self.query.focus(window, cx);
                } else {
                    self.focus_argument(index - 1, window, cx);
                }
            } else {
                input.update(cx, |input, cx| {
                    input.nav_linear(NavigationDirection::Back, TextBoundary::Graphmeme, cx)
                });
                if let Some(controls) = self.arguments.as_mut() {
                    if let Some(Control::Text { caret, .. }) = controls.fields.get_mut(index) {
                        *caret = caret.saturating_sub(1);
                    }
                }
            }
        } else {
            self.query_field().update(cx, |input, cx| {
                input.nav_linear(NavigationDirection::Back, TextBoundary::Graphmeme, cx)
            });
            self.query_caret = self.query_caret.saturating_sub(1);
        }
    }

    /// Right in the query field or an argument field (#205): at the
    /// query's end the key enters the first argument field; in an
    /// argument, at its end it goes to the field after it — the query,
    /// from the last — and moves the caret otherwise.
    pub(crate) fn field_right(
        &mut self,
        _: &FieldRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fields = self.shown_fields();
        if let Some((index, input)) = self.focused_argument_text(window, cx) {
            let at_end = self
                .arguments
                .as_ref()
                .and_then(|controls| controls.caret(index))
                .is_some_and(|caret| caret == input.read(cx).as_str().chars().count());
            let last = fields.map(|fields| fields.fields.len()).unwrap_or(0);
            if at_end {
                if index + 1 >= last {
                    self.query.focus(window, cx);
                } else {
                    self.focus_argument(index + 1, window, cx);
                }
            } else {
                input.update(cx, |input, cx| {
                    input.nav_linear(NavigationDirection::Forward, TextBoundary::Graphmeme, cx)
                });
                if let Some(controls) = self.arguments.as_mut() {
                    if let Some(Control::Text { caret, .. }) = controls.fields.get_mut(index) {
                        *caret = *caret + 1;
                    }
                }
            }
        } else {
            let at_end = self.query_field().read(cx).as_str().chars().count() == self.query_caret;
            if at_end && fields.is_some() {
                self.focus_argument(0, window, cx);
            } else {
                self.query_field().update(cx, |input, cx| {
                    input.nav_linear(NavigationDirection::Forward, TextBoundary::Graphmeme, cx)
                });
                self.query_caret += 1;
            }
        }
    }

    /// Home in the query field or an argument field (#205): the caret goes
    /// to the field's start, as the element's own key takes it — counted,
    /// so the edge keys know where it is.
    pub(crate) fn field_home(
        &mut self,
        _: &FieldHome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((index, input)) = self.focused_argument_text(window, cx) {
            input.update(cx, |input, cx| input.move_to(0, cx));
            if let Some(controls) = self.arguments.as_mut() {
                if let Some(Control::Text { caret, .. }) = controls.fields.get_mut(index) {
                    *caret = 0;
                }
            }
        } else {
            self.query_field()
                .update(cx, |input, cx| input.move_to(0, cx));
            self.query_caret = 0;
        }
    }

    /// End in the query field or an argument field (#205): the caret goes
    /// to the field's end — counted, as Home's is.
    pub(crate) fn field_end(&mut self, _: &FieldEnd, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((index, input)) = self.focused_argument_text(window, cx) {
            let end = input.read(cx).as_str().chars().count();
            input.update(cx, |input, cx| input.move_to(end, cx));
            if let Some(controls) = self.arguments.as_mut() {
                if let Some(Control::Text { caret, .. }) = controls.fields.get_mut(index) {
                    *caret = end;
                }
            }
        } else {
            let end = self.query_field().read(cx).as_str().chars().count();
            self.query_field()
                .update(cx, |input, cx| input.move_to(end, cx));
            self.query_caret = end;
        }
    }

    /// The argument field that has the keyboard, with its editable text,
    /// when it is a typed one.
    fn focused_argument_text(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<(usize, Entity<EditableTextState>)> {
        let index = self.focused_argument(window, cx)?;
        self.arguments
            .as_ref()
            .and_then(|controls| controls.input(index))
            .map(|input| (index, input))
    }
}

/// What an argument field's node tells assistive technology beyond its
/// name and value (#205): whether it is required, and — once it has been
/// left blank — what a required one waits for.
fn argument_state(field: &FormField) -> SharedString {
    match (field.required, field.error.is_some()) {
        (true, true) => SharedString::from(format!("Required. {MISSING}")),
        (true, false) => SharedString::from("Required"),
        (false, _) => SharedString::from("Optional"),
    }
}

/// The text an argument field shows when it holds nothing: its
/// placeholder, else its name — its label.
fn placeholder_of(field: &FormField) -> SharedString {
    let placeholder = match &field.kind {
        FieldKind::Text { placeholder } | FieldKind::Password { placeholder } => placeholder,
        _ => None,
    };
    placeholder
        .map(SharedString::from)
        .unwrap_or_else(|| SharedString::from(field.label.as_str()))
}

/// The optional argument's marker: "optional" in the muted small text the
/// row annotations use, inside the field at its end.
fn optional_marker(theme: &Theme) -> Div {
    div()
        .flex_none()
        .text_size(theme.typography.keycap_size)
        .text_color(theme.text_muted)
        .child("optional")
}

/// A field's width, sized to its value or placeholder, whichever is
/// longer (#205): a character of Geist at the well's 13px runs about
/// 7px, and the caret and the optional marker need room. The reference
/// sizes each inline argument this way.
fn field_width(shown: &str, optional: bool, theme: &Theme) -> gpui::Pixels {
    let controls = &theme.geometry.controls;
    let chars = shown.chars().count().max(1);
    px(7. * chars as f32 + 12. + 2. * f32::from(controls.well_padding_x))
        + if optional { px(46.) } else { px(0.) }
}

impl LauncherWindow {
    /// Test support: the editing state of the argument field `name`, which
    /// a platform input method talks to while composing text — the same
    /// reach the form's text fields give the tests. `None` while the field
    /// shows none.
    #[doc(hidden)]
    pub fn argument_field(&self, name: &str) -> Option<Entity<EditableTextState>> {
        let controls = self.arguments.as_ref()?;
        let index = controls.at(name)?;
        controls.input(index)
    }

    /// The argument fields of root search's selected row, drawn after the
    /// query in the search field (#205): one field per argument the
    /// command declares, with the open dropdown's choices under their
    /// trigger. `None` while none show.
    pub(crate) fn render_argument_fields(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let fields = self.shown_fields()?;
        let controls = self.arguments.as_ref()?;
        let visuals = crate::settings::launcher_visuals(cx);
        let theme = visuals.theme;
        let material = visuals.material;
        let shown: Vec<AnyElement> = fields
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| match controls.fields.get(index) {
                Some(Control::Text { input, .. }) => self
                    .argument_text(index, field, input, &theme, cx)
                    .into_any_element(),
                Some(Control::Choice { trigger, list }) => self
                    .argument_choice(index, field, trigger, list, &theme, &material, cx)
                    .into_any_element(),
                None => div().into_any_element(),
            })
            .collect();
        Some(
            div()
                .flex()
                .items_center()
                .gap(theme.geometry.search_gap)
                .flex_none()
                .children(shown)
                .into_any_element(),
        )
    }

    /// One text or password argument field (#205): a Settings inline well
    /// (30 high, the field family's), sized to its value or placeholder,
    /// the editable text element inside it; a password's text is drawn
    /// transparent with a dot for each character over it, and reads as
    /// dots to assistive technology; a required field left blank once
    /// carries the danger ring.
    fn argument_text(
        &self,
        index: usize,
        field: &FormField,
        input: &Entity<EditableTextState>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let selector = format!("argument-{}", field.id);
        let password = matches!(field.kind, FieldKind::Password { .. });
        let dots: String = field.value.chars().map(|_| CONCEALED).collect();
        let placeholder = placeholder_of(field);
        let width = field_width(
            if field.value.is_empty() {
                placeholder.as_ref()
            } else {
                field.value.as_str()
            },
            !field.required,
            theme,
        );
        let well = controls::well(true, theme)
            .id(("argument", index))
            .debug_selector(move || selector)
            .key_context(CONTEXT)
            .track_focus(&input.focus_handle(cx))
            .role(Role::TextInput)
            .aria_label(field.label.clone())
            .aria_value(if password {
                SharedString::from(dots)
            } else {
                SharedString::from(field.value.as_str())
            })
            .aria_placeholder(placeholder.clone())
            .aria_description(argument_state(field))
            .flex_none()
            .w(width);
        let ring = controls::well_shadows(true, theme);
        let well = match field.error {
            Some(_) => well.shadow(controls::error_ring(theme)),
            None => well.shadow(controls::well_shadows(false, theme)),
        }
        .focus(move |well| well.shadow(ring));
        let text = controls::well_input(
            text_input(("argument", index)).state(input.downgrade()),
            placeholder,
            theme,
        );
        let well = if password {
            // The password's dots are drawn over text made transparent,
            // as the form's password fields are: the element has no
            // masking of its own.
            well.child(
                div()
                    .relative()
                    .flex_1()
                    .min_w(px(0.))
                    .child(text.text_color(gpui::transparent_black()))
                    .child(
                        div()
                            .id(("concealed", index))
                            .absolute()
                            .top_0()
                            .left_0()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_color(theme.text_title)
                            .aria_hidden()
                            .child(field.value.chars().map(|_| CONCEALED).collect::<String>()),
                    ),
            )
        } else {
            well.child(text)
        };
        if !field.required {
            well.child(optional_marker(theme))
        } else {
            well
        }
    }

    /// One dropdown argument field (#205): a trigger showing its choice or
    /// placeholder with a chevron, Enter, Space or Down (or a click)
    /// opening its choices below it — a leading empty choice, then the
    /// options — on the anchored, deferred popover a select's popup is.
    fn argument_choice(
        &self,
        index: usize,
        field: &FormField,
        trigger: &FocusHandle,
        list: &FocusHandle,
        theme: &Theme,
        material: &Material,
        cx: &mut Context<Self>,
    ) -> Div {
        let FieldKind::Choice(choices) = &field.kind else {
            unreachable!("the control is made for a dropdown");
        };
        let chosen = choices.iter().find(|choice| choice.id == field.value);
        let label = chosen
            .map(|choice| SharedString::from(choice.label.as_str()))
            .unwrap_or_else(|| SharedString::from(field.label.as_str()));
        let open = self
            .arguments
            .as_ref()
            .and_then(|controls| controls.open)
            .is_some_and(|(field, _)| field == index);
        let width = field_width(
            if chosen.is_some() {
                label.as_ref()
            } else {
                field.label.as_str()
            },
            !field.required,
            theme,
        ) + px(18.);
        let selector = format!("argument-{}", field.id);
        let shown = chosen.is_some();
        let ring = controls::well_shadows(true, theme);
        let trigger_well = controls::well(true, theme)
            .id(("argument", index))
            .debug_selector(move || selector)
            .key_context(TRIGGER)
            .track_focus(trigger)
            .role(Role::ComboBox)
            .aria_label(field.label.clone())
            .aria_value(label.clone())
            .aria_description(argument_state(field))
            .aria_expanded(open)
            .flex_none()
            .w(width)
            .shadow(match field.error {
                Some(_) => controls::error_ring(theme),
                None => controls::well_shadows(false, theme),
            })
            .focus(move |well| well.shadow(ring))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_color(if shown {
                        theme.text_title
                    } else {
                        theme.text_placeholder
                    })
                    .child(label),
            )
            .child(glyph_rotated(
                Glyph::ChevronRight,
                theme.geometry.controls.chevron,
                theme.nav_icon,
                gpui::radians(std::f32::consts::FRAC_PI_2),
            ))
            .when(!field.required, |well| well.child(optional_marker(theme)))
            .on_action(
                cx.listener(move |this, _: &OpenArgumentChoices, window, cx| {
                    this.open_argument_choices(index, window, cx);
                }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_argument_choices(index, window, cx);
            }));
        // The block: the trigger, then a zero-height row that positions
        // the popup — its content-box origin is the trigger's bottom-left,
        // so the anchored popup opens below the trigger however tall it
        // grew, and the popup is deferred from there, painting above the
        // window. As the select control composes its own (#99).
        div()
            .relative()
            .flex_none()
            .flex()
            .flex_col()
            .child(trigger_well)
            .when(open, |block| {
                block.child(
                    div().flex_none().h(px(0.)).w_full().child(
                        self.argument_choices_popup(field, choices, list, theme, material, cx),
                    ),
                )
            })
    }

    /// The open dropdown's choices (#205): a leading empty choice, then
    /// the dropdown's options, as the Actions panel's entries; the arrows
    /// move the highlight, Enter picks, Escape or Tab closes, and a click
    /// picks its row.
    fn argument_choices_popup(
        &self,
        field: &FormField,
        choices: &[pane_core::Choice],
        list: &FocusHandle,
        theme: &Theme,
        material: &Material,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let highlighted = self
            .arguments
            .as_ref()
            .and_then(|controls| controls.open)
            .map(|(_, highlighted)| highlighted)
            .unwrap_or(0);
        let actions = &theme.geometry.actions;
        let rows: Vec<AnyElement> = std::iter::once(EMPTY_CHOICE)
            .chain(choices.iter().map(|choice| choice.label.as_str()))
            .enumerate()
            .map(|(position, label)| {
                let selected = field.value.is_empty() && position == 0
                    || choices
                        .get(position.wrapping_sub(1))
                        .is_some_and(|choice| choice.id == field.value);
                let row = controls::menu_row(
                    ("choice", position),
                    SharedString::from(label),
                    None,
                    (highlighted == position, selected, true),
                    theme,
                )
                .role(Role::ListBoxOption)
                .aria_label(SharedString::from(label))
                .aria_selected(selected)
                .when(highlighted == position, |row| row.aria_active_descendant())
                .on_click(cx.listener(
                    move |this, _: &gpui::ClickEvent, window, cx| {
                        this.choose_argument_at(position, window, cx);
                    },
                ));
                row.into_any_element()
            })
            .collect();
        let content = div()
            .id("argument-choices")
            .debug_selector(|| format!("argument-choices-{}", field.id))
            .key_context(CHOICES)
            .track_focus(list)
            .role(Role::ListBox)
            .aria_label(field.label.clone())
            .flex()
            .flex_col()
            .gap(actions.list_gap)
            .p(actions.list_padding)
            .min_w(px(160.))
            .children(rows)
            .on_action(cx.listener(|this, _: &NextArgumentChoice, _, cx| {
                this.move_argument_choices(1, cx);
            }))
            .on_action(cx.listener(|this, _: &PreviousArgumentChoice, _, cx| {
                this.move_argument_choices(-1, cx);
            }))
            .on_action(
                cx.listener(move |this, _: &ChooseArgumentChoice, window, cx| {
                    let highlighted = this
                        .arguments
                        .as_ref()
                        .and_then(|controls| controls.open)
                        .map(|(_, highlighted)| highlighted)
                        .unwrap_or(0);
                    this.choose_argument_at(highlighted, window, cx);
                }),
            )
            .on_action(
                cx.listener(move |this, _: &CloseArgumentChoices, window, cx| {
                    this.close_argument_choices(window, cx);
                }),
            )
            // A mouse-down outside the choices closes them and is
            // consumed, so nothing under the popup is invoked by the
            // click that dismisses it — the discipline the footer menu's
            // popup keeps.
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                this.close_argument_choices(window, cx);
                cx.stop_propagation();
            }));
        deferred(
            anchored()
                .anchor(gpui::Anchor::TopLeft)
                .offset(gpui::point(px(0.), px(4.)))
                .snap_to_window_with_margin(px(4.))
                .child(
                    div()
                        .id("argument-choices-popup")
                        .occlude()
                        .rounded(theme.geometry.popover_radius)
                        .shadow(crate::ui::material::popover_shadows(theme))
                        .child(material.popover(theme, content)),
                ),
        )
        .into_any_element()
    }
}
