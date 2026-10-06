//! Package search, the online search sample: a command that searches a web
//! service as the user types into its own search field. Pane asks it only
//! once the user has opened it, never while they type in root search.
//!
//! The service is the fixture service, a made-up package registry on this
//! computer (`cargo run -p pane-core --example fixture_service`, port
//! 8740 by default; `crates/pane-core/tests/support/service.rs`). The
//! command reaches it through `wasi:http` with [`pane_guest::http::get`]
//! and reads its JSON with `serde_json`, an ordinary `no_std` library. Its
//! address is a setting the command's form changes, so the same sample
//! works against a service on another port.
//!
//! A search Pane no longer needs (the text changed, the user left) is
//! stopped where it waits for the service; an unreachable or failing
//! service is an error shown in place of results, not a crash, so it never
//! pauses the extension. Activating a result fetches that package's
//! details. Items, answers and errors match the JavaScript and TypeScript
//! samples.
#![no_std]

use pane_guest::alloc::{borrow::ToOwned, format, string::String, vec, vec::Vec};
use pane_guest::http;
use pane_guest::search::SearchResult;
use pane_guest::{
    Command, CustomView, Field, FieldKind, FieldValue, Form, FormError, Item, List, NoCustomView,
    TextField, settings,
};
use serde::Deserialize;

/// The address used until the user sets another.
const DEFAULT_SERVICE: &str = "http://127.0.0.1:8740";
/// The settings key holding the service address.
const SERVICE: &str = "service";
/// The command's id in pane.json.
const COMMAND: &str = "packages";

struct Packages;
pane_guest::export!(Packages);
pane_guest::search::export!(Packages);

#[derive(Deserialize)]
struct Found {
    results: Vec<Summary>,
}

#[derive(Deserialize)]
struct Summary {
    name: String,
    summary: String,
}

#[derive(Deserialize)]
struct Details {
    name: String,
    summary: String,
    version: String,
    license: String,
}

#[derive(Deserialize)]
struct Problem {
    error: String,
}

/// The service address: the saved one, or the default.
fn service() -> Result<String, String> {
    Ok(settings::get(SERVICE)?.unwrap_or_else(|| DEFAULT_SERVICE.into()))
}

/// Fetches `path` from the service and reads its JSON answer as `T`.
async fn fetch<T: for<'a> Deserialize<'a>>(path: &str) -> Result<T, String> {
    let service = service()?;
    let response = http::get(
        &format!("{service}{path}"),
        &[("accept", "application/json")],
    )
    .await
    .map_err(|why| format!("Could not reach the service at {service}: {why}"))?;
    if response.status != 200 {
        let why = serde_json::from_slice::<Problem>(&response.body)
            .map(|problem| problem.error)
            .unwrap_or_else(|_| response.text());
        return Err(format!("The service answered {}: {why}", response.status));
    }
    serde_json::from_slice(&response.body)
        .map_err(|error| format!("The service's answer could not be read: {error}"))
}

/// `text` with everything but unreserved URL characters percent-encoded.
fn encode(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte))
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

/// Runs the action `id`: the "about" item's, or a search result's
/// ("package:<name>"), which fetches that package's details.
async fn act(id: &str) -> Result<String, String> {
    if id == "about" {
        return Ok("Type in the search field to search the package registry".into());
    }
    let Some(name) = id.strip_prefix("package:") else {
        return Err(format!("unknown item: {id}"));
    };
    let details: Details = fetch(&format!("/packages/{}", encode(name))).await?;
    Ok(format!(
        "{} {} ({}): {}",
        details.name, details.version, details.license, details.summary
    ))
}

impl Command for Packages {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let item =
            |id: &str, title: &str, subtitle: String| Item::new(id, title).subtitle(subtitle);
        Ok(List::new("Package search").items([
            item(
                "about",
                "Type to search the package registry",
                "Results come from the service as you type; Enter shows a package's details".into(),
            )
            .on_action(|| act("about")),
            item("service", "Service address", service()?).form(Form {
                title: "Service address".into(),
                fields: vec![Field {
                    id: "address".into(),
                    label: "Address".into(),
                    kind: FieldKind::Text(TextField {
                        placeholder: Some(DEFAULT_SERVICE.into()),
                    }),
                }],
                submit_label: "Save".into(),
            }),
        ]))
    }

    /// Runs the search result the user chose, by its id
    /// ("package:<name>"): fetches that package's details.
    async fn run_search_result(id: String) -> Result<String, String> {
        act(&id).await
    }

    async fn submit_form(item_id: String, values: Vec<FieldValue>) -> Result<String, FormError> {
        if item_id != "service" {
            return Err(FormError {
                field: None,
                message: format!("unknown form: {item_id}"),
            });
        }
        let address = values
            .iter()
            .find(|value| value.id == "address")
            .map(|value| value.value.trim().trim_end_matches('/').to_owned())
            .unwrap_or_default();
        if !(address.starts_with("http://") || address.starts_with("https://")) {
            return Err(FormError {
                field: Some("address".into()),
                message: "Enter an address starting with http:// or https://".into(),
            });
        }
        settings::set(SERVICE, &address).map_err(|message| FormError {
            field: None,
            message,
        })?;
        Ok(format!("Searching {address} from now on"))
    }

    async fn open_view(_item_id: String) -> Result<CustomView, String> {
        Err("Package search has no custom views".into())
    }
}

impl pane_guest::search::Guest for Packages {
    async fn search(command: String, query: String) -> Result<Vec<SearchResult>, String> {
        if command != COMMAND {
            return Err(format!("unknown command: {command}"));
        }
        let found: Found = fetch(&format!("/search?q={}", encode(&query))).await?;
        Ok(found
            .results
            .into_iter()
            .map(|package| SearchResult {
                id: format!("package:{}", package.name),
                title: package.name,
                subtitle: Some(package.summary),
            })
            .collect())
    }
}
