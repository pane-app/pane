//! Test fixture: operations that fail in each way Pane must report, and a
//! command whose items call them. Tests install it several times, side by
//! side as `a`, `b` and `p1` to `p9`, each with its own pane.json saying
//! which operations it publishes; the command is run from `a`.
//!
//! Operations: `echo` answers its input; `forward` makes the call its input
//! describes (`{"to", "operation", "input"}`, version 1) and answers its
//! result; `crash` traps; `not-json` answers text that is not JSON;
//! `remember` saves its input in the package's settings under `last`;
//! `secret` answers, but tests leave it out of the manifest.
#![no_std]

use pane_guest::alloc::{format, string::String, string::ToString, vec::Vec};
use pane_guest::operations::call;
use pane_guest::{
    CustomView, FieldValue, FormError, Guest, Item, NoCustomView, View, publish, settings,
};
use serde_json::{Value, json};

struct Fixture;
pane_guest::export!(Fixture);
pane_guest::publish::export!(Fixture);

/// Each item: (title, the call it makes: source, operation, version, input).
const ITEMS: [(&str, &str, &str, u32, &str); 12] = [
    (
        "Call b's echo",
        "local:../b",
        "echo",
        1,
        r#"{"hello":"world"}"#,
    ),
    ("Call b's echo at version 2", "local:../b", "echo", 2, "{}"),
    ("Call b's secret", "local:../b", "secret", 1, "{}"),
    ("Call b's crash", "local:../b", "crash", 1, "{}"),
    ("Call b's not-json", "local:../b", "not-json", 1, "{}"),
    (
        "Call b's remember",
        "local:../b",
        "remember",
        1,
        r#""from a""#,
    ),
    (
        "Call b with input that is not JSON",
        "local:../b",
        "echo",
        1,
        "{",
    ),
    (
        "Call a missing package",
        "local:../missing",
        "echo",
        1,
        "{}",
    ),
    (
        "Call a source that is not local",
        "npm:left-pad",
        "echo",
        1,
        "{}",
    ),
    ("Call myself", "local:.", "echo", 1, "{}"),
    (
        "Call b, which calls me back",
        "local:../b",
        "forward",
        1,
        r#"{"to":"local:../a","operation":"echo","input":{}}"#,
    ),
    ("Call a chain of nine", "local:../p1", "forward", 1, ""),
];

/// `forward`'s input for a chain from `p<from>` to `p9`, which echoes.
fn chain(from: u32) -> Value {
    if from == 9 {
        return json!({ "to": "local:../p9", "operation": "echo", "input": { "end": true } });
    }
    let next = from + 1;
    json!({
        "to": format!("local:../p{next}"),
        "operation": "forward",
        "input": chain(next),
    })
}

async fn call_as_text(
    source: &str,
    operation: &str,
    version: u32,
    input: String,
) -> Result<String, String> {
    call(source.into(), operation.into(), version, input)
        .await
        .map_err(|error| error.explain())
}

impl Guest for Fixture {
    type CustomView = NoCustomView;

    async fn get_view() -> Result<View, String> {
        let items = ITEMS
            .iter()
            .map(|(title, ..)| Item {
                id: (*title).into(),
                title: (*title).into(),
                subtitle: None,
                form: None,
                platforms: None,
                custom_view: None,
            })
            .collect();
        Ok(View {
            title: "Operations fixture".into(),
            items,
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        let Some(&(title, source, operation, version, input)) =
            ITEMS.iter().find(|(title, ..)| *title == item_id)
        else {
            return Err(format!("unknown item: {item_id}"));
        };
        let input = match title {
            // p1 forwards to p2, and so on to p9.
            "Call a chain of nine" => chain(1).to_string(),
            _ => input.into(),
        };
        let answer = call_as_text(source, operation, version, input).await?;
        if operation == "remember" {
            let mine = settings::get("last")?;
            return Ok(format!("answered: {answer}; mine: {mine:?}"));
        }
        Ok(format!("answered: {answer}"))
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
        match operation.as_str() {
            "echo" => Ok(input),
            "forward" => {
                let request: Value =
                    serde_json::from_str(&input).map_err(|error| format!("{error}"))?;
                let field = |name: &str| request.get(name).and_then(Value::as_str).unwrap_or("");
                let input = request.get("input").cloned().unwrap_or(Value::Null);
                call_as_text(field("to"), field("operation"), 1, input.to_string()).await
            }
            "crash" => panic!("crashing as asked"),
            "not-json" => Ok("not JSON".into()),
            "remember" => {
                settings::set("last", &input)?;
                Ok("true".into())
            }
            "secret" => Ok(r#""the secret""#.into()),
            other => Err(format!("unknown operation: {other}")),
        }
    }
}
