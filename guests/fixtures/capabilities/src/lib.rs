//! Test fixture: a package that provides and uses capabilities, whose items
//! drive the calls and queries Pane must serve. Tests install it several
//! times, as `a`, `b`, `c` and `p1` to `p7`, each with its own pane.json
//! saying whether it provides the capability (and which operations it
//! provides) and which it uses; the command is run from `a`, whose settings
//! key `sources` the test saved beforehand: a JSON object from those names
//! to package sources, for the chain item's identity hops. A capability's
//! operations arrive at `run-operation` qualified by the capability, so the
//! fixture answers `greet` with the operation name it received among other
//! things: the proof the qualification passes through; `forward` is reached
//! both qualified, through the capability, and as the plain name an identity
//! call uses, so the chain item can hop through it.
//!
//! Items: "Call the greet capability" calls `greet` of `fixture:greet@1`
//! with a name; "…with no name" shows its own error; "…crash" traps; "Call
//! an undeclared capability" and "Call an undeclared operation of the greet
//! capability" call what the test's manifest for `a` does not declare; the
//! "Ask who provides…" items query `providers`; "Call b's capability
//! operation by identity" calls the qualified operation as a published
//! operation, which it is not; "Call the greet capability, whose provider
//! calls back" reaches `forward`, which calls the capability its input
//! describes — routed back to `a`, in the chain already, and refused; "Call
//! a chain whose last call is a capability" hops `p1` to `p7` by their
//! sources (each serving `forward`, which calls what its input describes)
//! and has `p7` call the capability, one call past Pane's depth limit; the
//! "Call every provider…" items fan a call out to every provider of
//! `fixture:every@1`, a capability the test's manifest for `a` uses with
//! `"use": "all"` — each provider answering with its title — with no name
//! (each provider's own error), and of `fixture:greet@1`, whose use is
//! `"use": "one"`, which is refused.
#![no_std]

use pane_extension::alloc::{format, string::String, string::ToString, vec::Vec};
use pane_extension::capabilities::{call, call_every, providers};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::operations;
use pane_extension::{
    Command, CustomView, FieldValue, FormError, Item, List, NoCustomView, publish, settings,
};
use serde_json::{Value, json};

struct Fixture;
pane_extension::export!(Fixture);
pane_extension::publish::export!(Fixture);

/// Each item: (title, capability, operation, input). The input of the
/// chain and identity items is built in code, from the sources the test
/// saved.
const ITEMS: [(&str, &str, &str, &str); 13] = [
    (
        "Call the greet capability",
        "fixture:greet@1",
        "greet",
        r#"{"name":"Ada"}"#,
    ),
    (
        "Call the greet capability with no name",
        "fixture:greet@1",
        "greet",
        r#"{}"#,
    ),
    (
        "Call the greet capability's crash",
        "fixture:greet@1",
        "crash",
        r#"{}"#,
    ),
    (
        "Call an undeclared capability",
        "fixture:undeclared@1",
        "greet",
        r#"{}"#,
    ),
    (
        "Call an undeclared operation of the greet capability",
        "fixture:greet@1",
        "unknown",
        r#"{}"#,
    ),
    (
        "Ask who provides the greet capability",
        "fixture:greet@1",
        "",
        "",
    ),
    (
        "Ask who provides the farewell capability",
        "fixture:farewell@1",
        "",
        "",
    ),
    (
        "Call the greet capability, whose provider calls back",
        "fixture:greet@1",
        "forward",
        r#"{"via":"capability","to":"fixture:greet@1","operation":"greet","input":{"name":"Ada"}}"#,
    ),
    ("Call a chain whose last call is a capability", "", "", ""),
    ("Call b's capability operation by identity", "", "", ""),
    (
        "Call every provider of the every capability",
        "fixture:every@1",
        "greet",
        r#"{"name":"Ada"}"#,
    ),
    (
        "Call every provider with no name",
        "fixture:every@1",
        "greet",
        "{}",
    ),
    ("Call every provider of the greet capability", "", "", ""),
];

/// The sources the test saved, by name.
fn sources() -> Result<Value, String> {
    let saved = settings::get("sources")?.ok_or("no sources are saved")?;
    serde_json::from_str(&saved).map_err(|error| format!("{error}"))
}

/// The source `name` stands for: a saved one, or `name` itself.
fn source(sources: &Value, name: &str) -> String {
    match sources.get(name).and_then(Value::as_str) {
        Some(source) => source.into(),
        None => name.into(),
    }
}

/// `forward`'s input for a chain from `p<from>` to `p7`, each serving
/// `forward`: the last, `p7`, calls the greet capability, which Pane
/// refuses as one call past its depth limit.
fn chain(sources: &Value, from: u32) -> Value {
    if from == 7 {
        return json!({
            "via": "capability",
            "to": "fixture:greet@1",
            "operation": "greet",
            "input": { "name": "Ada" }
        });
    }
    let next = from + 1;
    json!({
        "to": source(sources, &format!("p{next}")),
        "operation": "forward",
        "input": chain(sources, next),
    })
}

/// Runs the action of the item `item_id` and shows a toast with what
/// [`outcome`] answers; each item's action is this with its id, its title.
async fn act(item_id: &str) -> Result<(), String> {
    let done = outcome(item_id).await?;
    show_toast(Toast::success(done));
    Ok(())
}

/// What the action of the item `item_id` does, answering how it went.
async fn outcome(item_id: &str) -> Result<String, String> {
    let Some(&(title, capability, operation, input)) =
        ITEMS.iter().find(|(title, ..)| *title == item_id)
    else {
        return Err(format!("unknown item: {item_id}"));
    };
    if title == "Call a chain whose last call is a capability" {
        let sources = sources()?;
        let answer = operations::call(
            source(&sources, "p1").into(),
            "forward".into(),
            1,
            chain(&sources, 1).to_string(),
        )
        .await
        .map_err(|error| error.explain())?;
        return Ok(format!("answered: {answer}"));
    }
    if title == "Call b's capability operation by identity" {
        let sources = sources()?;
        let answer = operations::call(
            source(&sources, "b").into(),
            "fixture:greet@1/greet".into(),
            1,
            r#"{"name":"Ada"}"#.into(),
        )
        .await
        .map_err(|error| error.explain())?;
        return Ok(format!("answered: {answer}"));
    }
    // The fan-out items: a use of every provider, each one answering with
    // its title, and a use of one provider, which is refused. The empty
    // fields name the one-provider item's call.
    if title.starts_with("Call every provider") {
        let capability = if capability.is_empty() {
            "fixture:greet@1"
        } else {
            capability
        };
        let operation = if operation.is_empty() {
            "greet"
        } else {
            operation
        };
        let input = if input.is_empty() {
            r#"{"name":"Ada"}"#
        } else {
            input
        };
        let answers = call_every(capability, operation, input.into())
            .await
            .map_err(|error| error.explain())?;
        let said: Vec<String> = answers
            .iter()
            .map(|answer| match &answer.answer {
                Ok(result) => format!("{}: {result}", answer.title),
                Err(error) => format!("{}: {}", answer.title, error.explain()),
            })
            .collect();
        return Ok(match said.as_slice() {
            [] => "no provider answered".into(),
            said => said.join("; "),
        });
    }
    // A query item: which providers can serve the capability now.
    if operation.is_empty() {
        let who: Vec<String> = providers(capability)
            .iter()
            .map(|provider| provider.title.clone())
            .collect();
        return Ok(match who.as_slice() {
            [] => format!("the {capability} capability has no provider"),
            titles => format!(
                "the {capability} capability is provided by {}",
                titles.join(", ")
            ),
        });
    }
    let answer = call(capability, operation, input.into())
        .await
        .map_err(|error| error.explain())?;
    Ok(format!("answered: {answer}"))
}

impl Command for Fixture {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let items = ITEMS
            .into_iter()
            .map(|(title, ..)| Item::new(title, title).on_action(move || act(title)));
        Ok(List::new("Capabilities fixture").items(items))
    }

    async fn submit_form(item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: format!("unknown form: {item_id}"),
        })
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        Err(format!("unknown view: {item_id}"))
    }
}

impl publish::Guest for Fixture {
    async fn run_operation(operation: String, input: String) -> Result<String, String> {
        // Pane qualifies each capability operation with the capability, so
        // one component can tell them from calls by identity.
        match operation.as_str() {
            "fixture:greet@1/greet" | "fixture:every@1/greet" => {
                let input: Value =
                    serde_json::from_str(&input).map_err(|error| format!("{error}"))?;
                let name = input.get("name").and_then(Value::as_str).unwrap_or("");
                if name.is_empty() {
                    return Err("a name is needed".into());
                }
                Ok(json!({
                    "greeting": format!("Hello, {name}"),
                    "operation": operation.clone(),
                })
                .to_string())
            }
            "fixture:greet@1/forward" | "forward" => {
                let request: Value =
                    serde_json::from_str(&input).map_err(|error| format!("{error}"))?;
                let field = |name: &str| request.get(name).and_then(Value::as_str).unwrap_or("");
                let input = request
                    .get("input")
                    .cloned()
                    .unwrap_or(Value::Null)
                    .to_string();
                match field("via") {
                    "capability" => call(field("to"), field("operation"), input)
                        .await
                        .map_err(|error| error.explain()),
                    // By identity, as the chain item's hops are.
                    _ => operations::call(field("to").into(), field("operation").into(), 1, input)
                        .await
                        .map_err(|error| error.explain()),
                }
            }
            "fixture:greet@1/crash" => panic!("crashing as asked"),
            other => Err(format!("unknown operation: {other}")),
        }
    }
}
