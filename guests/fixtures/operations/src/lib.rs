//! Test fixture: operations that fail in each way Pane must report, and a
//! command whose items call them. Tests install it several times, as `a`,
//! `b`, `c` and `p1` to `p9`, each with its own pane.json saying which
//! operations it publishes; the command is run from `a`. Pane addresses a
//! package by its identity, the absolute source path known only once a test
//! has made its folders, so the test saves `a`'s settings key `sources`
//! beforehand: a JSON object from those names (and `missing`) to sources.
//!
//! Operations: `echo` answers its input; `forward` makes the call its input
//! describes (`{"to", "operation", "input"}`, version 1) and answers its
//! result; `crash` traps; `not-json` answers text that is not JSON;
//! `remember` saves its input in the package's settings under `last`;
//! Two items spin after saving `spin` as "started": they compute without
//! yielding until Pane refuses them their settings (they were stopped), or
//! for 20 seconds. One then tries to save `spin` as "finished" and call
//! `b`'s `remember`, and answers; the other fails. Tests stop `a` while it
//! spins: what it tries afterwards must be refused and its answer discarded.
//!
//! `wait` saves `waiting` as "started", waits ten seconds, then saves it as
//! "finished" (tests stop it before); `secret` answers, but tests leave it
//! and `wait` out of the manifest unless they need them.
#![no_std]

use futures::FutureExt;
use pane_guest::alloc::{format, string::String, string::ToString, vec::Vec};
use pane_guest::operations::call;
use pane_guest::{
    Command, CustomView, FieldValue, FormError, Item, List, NoCustomView, publish, settings,
};
use serde_json::{Value, json};

struct Fixture;
pane_guest::export!(Fixture);
pane_guest::publish::export!(Fixture);

/// Each item: (title, the package it calls: a name in `sources` or a source
/// as written, operation, version, input).
const ITEMS: [(&str, &str, &str, u32, &str); 20] = [
    ("Call b's echo", "b", "echo", 1, r#"{"hello":"world"}"#),
    ("Call b's echo at version 2", "b", "echo", 2, "{}"),
    ("Call b's secret", "b", "secret", 1, "{}"),
    ("Call b's crash", "b", "crash", 1, "{}"),
    ("Call b's not-json", "b", "not-json", 1, "{}"),
    ("Call b's remember", "b", "remember", 1, r#""from a""#),
    ("Call b with input that is not JSON", "b", "echo", 1, "{"),
    ("Call b twice at once, which calls c", "b", "forward", 1, ""),
    (
        "Call b's remember and give up at once",
        "b",
        "remember",
        1,
        r#""given up""#,
    ),
    ("Call c's echo", "c", "echo", 1, "{}"),
    ("Call b's wait", "b", "wait", 1, "{}"),
    (
        "Spin, then save and call b's remember",
        "b",
        "remember",
        1,
        r#""from a spinning caller""#,
    ),
    ("Spin, then fail", "b", "echo", 1, "{}"),
    ("Call a missing package", "missing", "echo", 1, "{}"),
    (
        "Call a source that is not local",
        "npm:left-pad",
        "echo",
        1,
        "{}",
    ),
    ("Call a relative source", "local:../b", "echo", 1, "{}"),
    ("Call myself", "a", "echo", 1, "{}"),
    ("Call b, which calls me back", "b", "forward", 1, ""),
    ("Call a chain of nine", "p1", "forward", 1, ""),
    (
        "Call my own package's other component",
        "a",
        "echo",
        1,
        "{}",
    ),
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

/// `forward`'s input for a chain from `p<from>` to `p9`, which echoes.
fn chain(sources: &Value, from: u32) -> Value {
    if from == 9 {
        return json!({ "to": source(sources, "p9"), "operation": "echo", "input": { "end": true } });
    }
    let next = from + 1;
    json!({
        "to": source(sources, &format!("p{next}")),
        "operation": "forward",
        "input": chain(sources, next),
    })
}

/// Saves `spin` as "started", then computes without yielding until reading
/// its settings is refused, which happens once Pane has stopped it, or for
/// 20 seconds.
fn spin() -> Result<(), String> {
    use wasip3::clocks::monotonic_clock::now;
    settings::set("spin", "started")?;
    let end = now() + 20_000_000_000;
    while now() < end && settings::get("spin").is_ok() {
        let next = now() + 1_000_000;
        while now() < next {}
    }
    Ok(())
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

/// Runs the action of the item `item_id`; each item's action is this with
/// its id, its title.
async fn act(item_id: &str) -> Result<String, String> {
    let Some(&(title, target, operation, version, input)) =
        ITEMS.iter().find(|(title, ..)| *title == item_id)
    else {
        return Err(format!("unknown item: {item_id}"));
    };
    let sources = sources()?;
    let target = source(&sources, target);
    let input = match title {
        "Call b, which calls me back" => {
            json!({ "to": source(&sources, "a"), "operation": "echo", "input": {} }).to_string()
        }
        // p1 forwards to p2, and so on to p9.
        "Call a chain of nine" => chain(&sources, 1).to_string(),
        _ => input.into(),
    };
    match title {
        "Spin, then save and call b's remember" => {
            spin()?;
            let saved = settings::set("spin", "finished");
            let called = call_as_text(&target, operation, version, input).await;
            return Ok(format!("spun: saved {saved:?}, called {called:?}"));
        }
        "Spin, then fail" => {
            spin()?;
            return Err("failed after spinning".into());
        }
        "Call b twice at once, which calls c" => {
            // b forwards each to c's echo, so it is still waiting on c
            // when Pane takes the second call.
            let forward = |word: &str| {
                json!({ "to": source(&sources, "c"), "operation": "echo", "input": word })
                    .to_string()
            };
            let (first, second) = futures::join!(
                call_as_text(&target, operation, version, forward("first")),
                call_as_text(&target, operation, version, forward("second")),
            );
            return Ok(format!("answered: {} and {}", first?, second?));
        }
        "Call b's remember and give up at once" => {
            // Polled once, so the call is sent, then dropped.
            let dropped = call_as_text(&target, operation, version, input).now_or_never();
            return Ok(format!("gave up: {}", dropped.is_none()));
        }
        _ => {}
    }
    let answer = call_as_text(&target, operation, version, input).await?;
    if operation == "remember" {
        let mine = settings::get("last")?;
        return Ok(format!("answered: {answer}; mine: {mine:?}"));
    }
    Ok(format!("answered: {answer}"))
}

impl Command for Fixture {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let items = ITEMS
            .into_iter()
            .map(|(title, ..)| Item::new(title, title).on_action(move || act(title)));
        Ok(List::new("Operations fixture").items(items))
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
            "wait" => {
                settings::set("waiting", "started")?;
                wasip3::clocks::monotonic_clock::wait_for(10_000_000_000).await;
                settings::set("waiting", "finished")?;
                Ok("true".into())
            }
            "secret" => Ok(r#""the secret""#.into()),
            other => Err(format!("unknown operation: {other}")),
        }
    }
}
