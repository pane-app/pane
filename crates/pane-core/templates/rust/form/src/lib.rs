//! __TITLE__, started from `pane-ext`'s `form` template: a command whose
//! screen is a form — a name to type and a greeting to choose, with a
//! `Greet` button — which the command checks and answers as its view
//! renders it again, the problem with a field shown next to it (#241,
//! the designed tree's form). `pane-ext dev` builds it with `cargo build
//! --release --target wasm32-wasip2`, hands each build to the running
//! Pane, and builds it again after each save, so an edit shows at once;
//! see the README beside this file and Pane's development-mode
//! documentation.
//!
//! One component (the WebAssembly file `pane.json` names) serves every
//! command the package declares: Pane passes the command's id with each
//! call, so `render` and `run` below match on it. `pane-ext new
//! command .` adds one.
#![no_std]

// pane-ext new command adds a command's module here.

use core::cell::RefCell;

use pane_extension::alloc::{borrow::ToOwned, format, string::String};
use pane_extension::commands::LaunchRecord;
use pane_extension::form;
use pane_extension::view::{Answer, Cx, IntoAnswer, IntoNode};
use pane_extension::{Command, List, choice};

/// The package's own type, whose designed view the SDK exports.
struct __STRUCT__;

pane_extension::export!(__STRUCT__);

/// The greeting form's choices, by id.
const GREETINGS: [(&str, &str); 3] = [
    ("hello", "Hello"),
    ("morning", "Good morning"),
    ("welcome", "Welcome"),
];

/// The form's state: the answer the last submission gave, and why it was
/// refused, on the name's field.
struct Greeting {
    /// The answer the last submission gave, drawn over the form.
    answered: RefCell<String>,
    /// Why the last submission was refused, on the name's field.
    error: RefCell<Option<&'static str>>,
}

impl Greeting {
    /// The form's tree, as the command's view renders it: the listener
    /// the tree names runs with the command's view, whichever command
    /// opened it.
    fn tree(&self, cx: &mut Cx<Greeting>) -> Answer {
        let submit = cx.form_listener(|view, values| {
            let name = values.text("name").unwrap_or_default().trim().to_owned();
            let greeting = values
                .text("greeting")
                .and_then(|value| {
                    GREETINGS
                        .iter()
                        .find(|&&(id, _)| id == value)
                        .map(|&(_, label)| label)
                })
                .unwrap_or("Hello");
            if name.is_empty() {
                *view.error.borrow_mut() = Some("Enter a name");
                return;
            }
            if name.chars().count() > 40 {
                *view.error.borrow_mut() = Some("Use at most 40 characters");
                return;
            }
            *view.error.borrow_mut() = None;
            *view.answered.borrow_mut() = format!("{greeting}, {name}");
        });
        let error = self.error.borrow().clone();
        let answered = self.answered.borrow().clone();
        let mut view = form::Form::new()
            .key("form")
            .navigation_title("__TITLE__")
            .submit_title("Greet")
            .on_submit(submit);
        // The answer draws over the form as its first child, the form
        // staying the tree's root: the window keeps a field's state by
        // the path of its key, and wrapping the form in a column would
        // move every field's path — losing the text, the caret and the
        // focus the user left in them (#238's rule).
        if !answered.is_empty() {
            view = view.child(
                pane_extension::view::text(answered.as_str())
                    .style(pane_extension::view::TextStyle::Title)
                    .into_node(),
            );
        }
        view.child(
            form::text_field("name")
                .title("Name")
                .placeholder("Ada Lovelace")
                .error(error.map(str::to_owned).unwrap_or_default())
                .auto_focus()
                .into_node(),
        )
        .child(
            form::dropdown("greeting")
                .title("Greeting")
                .options(
                    GREETINGS
                        .iter()
                        .map(|&(value, label)| choice(value).label(label)),
                )
                .default_value("hello")
                .into_node(),
        )
        .into_answer()
    }
}

impl pane_extension::view::View for Greeting {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
        self.tree(cx)
    }
}

impl Command for __STRUCT__ {
    /// The form the command opens: a designed view (its pane.json says
    /// `"mode": "designed"`).
    type DesignedView = Greeting;

    /// Opens it: a fresh form, empty and unrefused. Each call opens a
    /// new view with its own state.
    async fn open_designed_view(
        command: String,
        _launch: LaunchRecord,
    ) -> Result<Greeting, String> {
        if command != "__NAME__" {
            return Err(format!("unknown command: {command}"));
        }
        Ok(Greeting {
            answered: RefCell::new(String::new()),
            error: RefCell::new(None),
        })
    }

    async fn render() -> Result<List, String> {
        match pane_extension::commands::current().command.as_str() {
            // pane-ext new command adds a view command's arm here.
            other => Err(format!("`{other}` opens a designed view; it has no list")),
        }
    }

    /// A command that opens no screen would run here instead; the
    /// `no-view` template starts from one.
    async fn run(command: String, _launch: LaunchRecord) -> Result<(), String> {
        match command.as_str() {
            // pane-ext new command adds a no-view command's arm here.
            other => Err(format!(
                "`{other}` opens a screen; it has no run entry point"
            )),
        }
    }
}
