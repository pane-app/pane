//! Pane's operations sample in Rust. Its package publishes the operation
//! `greet` (under `operations` in its pane.json), served by
//! [`publish::Guest::run_operation`], and its command calls the `greet`
//! operation of another package, with [`pane_extension::operations::call`]:
//! a form asks for that package's source (its identity, as Pane shows it:
//! `local:` and the folder it was installed from), a name, and whether to
//! ask once or twice at once. Items, titles, results and errors match the
//! JavaScript and TypeScript samples.
//!
//! `greet` version 1 takes `{"name": "<name>"}` and answers
//! `{"greeting": "Hello, <name>, from Rust"}`, or the error "a name is
//! needed".
//!
//! `wait` version 1 shows a call Pane stops: it saves `waiting` as
//! "started" in its settings, waits ten seconds, saves "finished" and
//! answers `{"waited": true}`. Disabling or reloading either package
//! meanwhile stops it; the "wait" item calls it.
#![no_std]

use core::cell::RefCell;

use pane_extension::alloc::{
    borrow::ToOwned, format, string::String, string::ToString, vec, vec::Vec,
};
use pane_extension::form::{self, FormValues};
use pane_extension::operations::call;
use pane_extension::view::{Cx, IntoAnswer, Pending, View};
use pane_extension::{Command, Item, LaunchRecord, List, publish, settings};
use serde_json::{Value, json};

struct Operations;
pane_extension::export!(Operations);
pane_extension::publish::export!(Operations);

/// Calls `greet` version 1 of the package with `source` for `name`, and
/// returns its greeting, or why there is none ("<kind>: <message>").
async fn greet(source: &str, name: &str) -> Result<String, String> {
    let input = json!({ "name": name }).to_string();
    let result = call(source.into(), "greet".into(), 1, input)
        .await
        .map_err(|error| error.explain())?;
    let result: Value = serde_json::from_str(&result).map_err(|error| format!("{error}"))?;
    match result.get("greeting").and_then(Value::as_str) {
        Some(greeting) => Ok(greeting.into()),
        None => Err("the answer has no greeting".into()),
    }
}

impl Command for Operations {
    type DesignedView = Calling;

    async fn render() -> Result<List, String> {
        Ok(List::new("Call from Rust").items([
            Item::new("greet", "Greet through another extension")
                .subtitle("Calls its greet operation through Pane")
                .on_action(|| async { open_form("greet") }),
            Item::new("wait", "Wait in another extension")
                .subtitle("Calls its wait operation, which takes ten seconds")
                .on_action(|| async { open_form("wait") }),
        ]))
    }

    async fn open_designed_view(command: String, _launch: LaunchRecord) -> Result<Calling, String> {
        match command.as_str() {
            "greet" | "wait" => Ok(Calling::new(command == "wait")),
            _ => Err("this command opens no designed view".into()),
        }
    }
}

/// The operations the package publishes: `greet` and `wait`.
impl publish::Guest for Operations {
    async fn run_operation(operation: String, input: String) -> Result<String, String> {
        if operation == "wait" {
            settings::set("waiting", "started")?;
            // If Pane stops the call meanwhile, nothing after this runs.
            wasip3::clocks::monotonic_clock::wait_for(10_000_000_000).await;
            settings::set("waiting", "finished")?;
            return Ok(json!({ "waited": true }).to_string());
        }
        if operation != "greet" {
            return Err(format!("unknown operation: {operation}"));
        }
        let input: Value = serde_json::from_str(&input).map_err(|error| format!("{error}"))?;
        let name = input.get("name").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() {
            return Err("a name is needed".into());
        }
        Ok(json!({ "greeting": format!("Hello, {name}, from Rust") }).to_string())
    }
}

/// Opens the form command `which`, as the user would.
fn open_form(which: &'static str) -> Result<(), String> {
    pane_extension::commands::launch(
        &pane_extension::commands::CommandRef {
            source: None,
            command: which.into(),
        },
        pane_extension::commands::LaunchType::UserInitiated,
        &[],
        None,
    )
}

/// The form command's view (#241): the fields the operation's arguments
/// ask for, whose submission calls it. The call runs as the view's own
/// work ([`Pending`]): its answer, or why there is none, draws over the
/// form.
struct Calling {
    /// Whether the view is the `wait` form; the `greet` one otherwise.
    waiting: bool,
    /// The fields' values, as the view last drew them.
    source: RefCell<String>,
    name: RefCell<String>,
    times: RefCell<String>,
    /// The call a submission started, its answer on its way.
    pending: Option<Pending<String>>,
    /// Why the last submission was refused: its field and message.
    error: RefCell<(String, String)>,
}

impl Calling {
    fn new(waiting: bool) -> Calling {
        Calling {
            waiting,
            source: RefCell::new(String::new()),
            name: RefCell::new(String::new()),
            times: RefCell::new("once".into()),
            pending: None,
            error: RefCell::new((String::new(), String::new())),
        }
    }
}

impl View for Calling {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
        let waiting = self.waiting;
        let submit = cx.form_listener(move |this, values| {
            let source = values.text("source").unwrap_or_default().trim().to_owned();
            let name = values.text("name").unwrap_or_default().to_owned();
            let times = values.text("times").unwrap_or("once").to_owned();
            if source.is_empty() {
                *this.error.borrow_mut() = ("source".into(), "Enter the package's source".into());
                return;
            }
            *this.error.borrow_mut() = (String::new(), String::new());
            *this.source.borrow_mut() = source.clone();
            *this.name.borrow_mut() = name.clone();
            *this.times.borrow_mut() = times.clone();
            // The call runs as the view's own work: its answer draws the
            // moment it arrives (#243).
            let pending = if waiting {
                Pending::loading(async move {
                    match call(source.into(), "wait".into(), 1, "{}".into()).await {
                        Ok(_) => "Waited in the other extension".to_owned(),
                        Err(error) => error.explain(),
                    }
                })
            } else {
                Pending::loading(async move {
                    let input = json!({ "name": name }).to_string();
                    let times = if times == "twice" { 2 } else { 1 };
                    match call(source.into(), "greet".into(), times, input).await {
                        Ok(answer) => match serde_json::from_str::<Value>(&answer) {
                            Ok(answer) => match answer.get("greeting").and_then(Value::as_str) {
                                Some(greeting) => greeting.to_owned(),
                                None => "the answer has no greeting".to_owned(),
                            },
                            Err(error) => format!("{error}"),
                        },
                        Err(error) => error.explain(),
                    }
                })
            };
            this.pending = Some(pending);
        });
        // What the call answered, when it has: drawn over the form.
        if let Some(pending) = &mut self.pending
            && let Some(answer) = pending.ready()
        {
            let answer = answer.clone();
            self.pending = None;
            let mut view = form_of(self, submit, answer.clone());
            let _ = &mut view;
            return pane_extension::view::column()
                .gap(pane_extension::view::Space::M)
                .child(
                    pane_extension::view::text(answer.as_str())
                        .style(pane_extension::view::TextStyle::Title),
                )
                .child(view)
                .into_answer();
        }
        if self.pending.is_some() {
            return pane_extension::view::text("Calling the other extension…").into_answer();
        }
        form_of(self, submit, String::new()).into_answer()
    }
}

/// The form the view draws: the fields the operation's arguments ask
/// for, its answer over them.
fn form_of(
    view: &Calling,
    submit: pane_extension::view::FormListener,
    answer: String,
) -> form::Form {
    let _ = answer;
    let error = |field: &str| {
        let error = view.error.borrow();
        (error.0 == field)
            .then(|| error.1.clone())
            .unwrap_or_default()
    };
    let mut form = form::Form::new()
        .key("form")
        .submit_title("Call")
        .on_submit(submit)
        .child(
            form::text_field("source")
                .title("Package source")
                .placeholder("local:/path/to/sample-operations-js")
                .default_value(view.source.borrow().clone())
                .error(error("source"))
                .auto_focus(),
        );
    if !view.waiting {
        form = form
            .child(
                form::text_field("name")
                    .title("Name")
                    .default_value(view.name.borrow().clone()),
            )
            .child(
                form::dropdown("times")
                    .title("Ask")
                    .options([
                        pane_extension::view::choice("once").label("Once"),
                        pane_extension::view::choice("twice").label("Twice at once"),
                    ])
                    .default_value("once"),
            );
    }
    form
}
