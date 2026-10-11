//! Pane's capabilities sample in Rust: a package that uses a capability by
//! its name, under `uses` in its pane.json, instead of naming the package
//! that serves it (ADR 0041). It uses `pane-samples:greet@1`, which the
//! greet provider samples provide in Rust, JavaScript and TypeScript
//! (guests/sample-greet*), each answering with its own language's name:
//! whoever is installed serves the call, and the code names none of them.
//! Its use of it says `"use": "all"`, so beside the call that reaches one
//! provider, an item fans a call out to every provider, answering each
//! one's greeting labelled with its title. It also uses
//! `pane-samples:farewell@1` optionally, which no sample provides: a call
//! to it answers `not-found`, and nothing is gated on it. Items, titles,
//! results and errors match the JavaScript and TypeScript samples.
//!
//! Each capability's `greet`/`farewell` operation takes `{"name": "<name>"}`
//! and answers `{"greeting": "..."}`. Before calling, "Who provides it"
//! asks for the providers that can serve the capability now, with their
//! titles.
#![no_std]

use pane_extension::alloc::{format, string::String, string::ToString, vec::Vec};
use pane_extension::capabilities::{available, call, call_every, providers};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::operations::CallErrorKind;
use pane_extension::{Command, CustomView, Item, List, NoCustomView};
use serde_json::{Value, json};

/// The capabilities this package uses, as its pane.json declares them.
const GREET: &str = "pane-samples:greet@1";
const FAREWELL: &str = "pane-samples:farewell@1";

struct Capabilities;
pane_extension::export!(Capabilities);

/// Calls `operation` of `capability` for "Pane", and returns what its
/// provider answers, or the call's error.
async fn call_name(
    capability: &str,
    operation: &str,
) -> Result<String, pane_extension::operations::CallError> {
    let input = json!({ "name": "Pane" }).to_string();
    let result = call(capability, operation, input).await?;
    let result: Value = serde_json::from_str(&result).unwrap_or(Value::Null);
    Ok(result
        .get("greeting")
        .and_then(Value::as_str)
        .unwrap_or("the answer has no greeting")
        .into())
}

/// Runs the action of the item `item_id` and shows a toast with what it
/// found.
async fn act(item_id: &str) -> Result<(), String> {
    let said = outcome(item_id).await?;
    show_toast(Toast::success(said));
    Ok(())
}

/// What the action of the item `item_id` answers: the greeting a capability
/// serves, why there is none, or every provider's greeting.
async fn outcome(item_id: &str) -> Result<String, String> {
    match item_id {
        // A required use that no installed package provides is the user's
        // to fix; Pane's reason is the answer.
        "greet" => call_name(GREET, "greet")
            .await
            .map_err(|error| error.explain()),
        // A use of every provider: each provider's answer, labelled with
        // its title, and an empty list when none can serve.
        "every" => {
            let input = json!({ "name": "Pane" }).to_string();
            let answers = call_every(GREET, "greet", input)
                .await
                .map_err(|error| error.explain())?;
            let said: Vec<String> = answers
                .iter()
                .map(|answer| match &answer.answer {
                    Ok(result) => {
                        let result: Value = serde_json::from_str(result).unwrap_or(Value::Null);
                        let greeting = result
                            .get("greeting")
                            .and_then(Value::as_str)
                            .unwrap_or("the answer has no greeting");
                        format!("{}: {greeting}", answer.title)
                    }
                    Err(error) => format!("{}: {}", answer.title, error.explain()),
                })
                .collect();
            Ok(match said.as_slice() {
                [] => format!("No installed extension provides {GREET}"),
                said => said.join("; "),
            })
        }
        "farewell" => match call_name(FAREWELL, "farewell").await {
            Ok(farewell) => Ok(farewell),
            // An optional use with no provider degrades gracefully: say so
            // rather than fail.
            Err(error) if error.kind == CallErrorKind::NotFound => Ok(format!(
                "No installed extension provides {FAREWELL}; install one to use it"
            )),
            Err(error) => Err(error.explain()),
        },
        "who" => {
            let who: Vec<String> = providers(GREET)
                .iter()
                .map(|provider| provider.title.clone())
                .collect();
            Ok(match who.as_slice() {
                [] => format!("No installed extension provides {GREET}"),
                titles => format!("{GREET} is provided by {}", titles.join(", ")),
            })
        }
        "available" => Ok(match available(FAREWELL) {
            Some(provider) => format!("{FAREWELL} has a provider: {}", provider.title),
            None => format!("{FAREWELL} has no provider now"),
        }),
        other => Err(format!("unknown item: {other}")),
    }
}

/// An item whose action is [`act`] with its id.
fn item(id: &'static str, title: &str, subtitle: &str) -> Item {
    Item::new(id, title)
        .subtitle(subtitle)
        .on_action(move || act(id))
}

impl Command for Capabilities {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        Ok(List::new("Greet from Rust").items([
            item(
                "greet",
                "Greet through a capability",
                "Calls pane-samples:greet@1, whichever extension provides it",
            ),
            item(
                "every",
                "Greet every provider",
                "Calls pane-samples:greet@1 on every extension that provides it",
            ),
            item(
                "farewell",
                "Say farewell",
                "Calls the optional pane-samples:farewell@1, which no sample provides",
            ),
            item(
                "who",
                "Who provides the greeting",
                "Asks which extensions can serve pane-samples:greet@1 now",
            ),
            item(
                "available",
                "Is the farewell capability available",
                "Asks whether an optional capability has a provider",
            ),
        ]))
    }

    async fn open_view(item_id: String) -> Result<CustomView, String> {
        Err(format!("unknown view: {item_id}"))
    }
}
