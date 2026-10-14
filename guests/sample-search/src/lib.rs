//! Package search, the online search sample: a command whose screen is a
//! designed List that handles its search itself (#240) — the search field
//! Pane draws in the header, its text told to the view through the
//! List's search-text event, throttled, and the service's answers listed
//! as the items. Pane asks the service nothing while the user types in
//! root search: the field is the view's own, not root search's.
//!
//! The service is the fixture service, a made-up package registry on this
//! computer (`cargo run -p pane-core --example fixture_service`, port
//! 8740 by default; `crates/pane-core/tests/support/service.rs`). The
//! command reaches it through `wasi:http` with [`pane_extension::http::get`]
//! and reads its JSON with `serde_json`, an ordinary `no_std` library. Its
//! address is a setting the "Service address" item's pushed view changes
//! (a text field and a Save button, the modern replacement for the typed
//! form the List document carried), so the same sample works against a
//! service on another port.
//!
//! A search starts in the render that answers the text (the `Pending`
//! pattern, #243): the loading state shows while it runs, and the results
//! are drawn the moment they land. An unreachable or failing service is an
//! error shown in place of results, not a crash, so it never pauses the
//! extension. Activating a result fetches that package's details and shows
//! them in a toast. Items, toasts and errors match the JavaScript and
//! TypeScript samples.
#![no_std]

use pane_extension::alloc::{borrow::ToOwned, format, string::String, vec, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::http;
use pane_extension::view::{
    Cx, IntoAnswer, Pending, Space, View, button, column, empty_state, item, list, text_input,
};
use pane_extension::{Command, LaunchRecord, preferences, settings};
use serde::Deserialize;

/// The address used until the user sets another.
const DEFAULT_SERVICE: &str = "http://127.0.0.1:8740";
/// The settings key holding the service address.
const SERVICE: &str = "service";
/// The command's id in pane.json.
const COMMAND: &str = "packages";

struct Packages;
pane_extension::export!(Packages);

#[derive(Deserialize)]
struct Found {
    results: Vec<Summary>,
}

#[derive(Clone, Deserialize)]
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

/// The package's `service` preference, as the user set it (Setup, or the
/// extension's card in Settings › Extensions) or else its declared default.
#[derive(Deserialize)]
struct ServicePreference {
    #[serde(default, rename = "service")]
    address: Option<String>,
}

/// The service address: the one the user set as the command's `service`
/// preference, the one this command saved on its Service address view, or
/// the default.
fn service() -> Result<String, String> {
    let user = preferences::values::<ServicePreference>()?.address;
    let saved = settings::get(SERVICE)?;
    Ok(user.or(saved).unwrap_or_else(|| DEFAULT_SERVICE.into()))
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

/// What the action `id` answers: the "about" item's, or a search result's
/// ("package:<name>"), which fetches that package's details.
async fn outcome(id: &str) -> Result<String, String> {
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
    type DesignedView = Search;

    async fn open_designed_view(command: String, _launch: LaunchRecord) -> Result<Search, String> {
        if command != COMMAND {
            return Err(format!("unknown command: {command}"));
        }
        Ok(Search::Rows {
            query: String::new(),
            searched: String::new(),
            results: Vec::new(),
            failed: None,
            pending: Pending::loading(async {
                SearchOutcome {
                    results: Vec::new(),
                    failed: None,
                }
            }),
            opening: None,
        })
    }
}

/// The search view: the List that handles its search itself, or the
/// "Service address" view it pushed.
enum Search {
    /// The List, with what its search has answered.
    Rows {
        /// The text in the search field, as the view last heard it.
        query: String,
        /// The text the pending search runs for.
        searched: String,
        /// The results the service answered for `searched`.
        results: Vec<Summary>,
        /// Why the last search failed, if it did.
        failed: Option<String>,
        /// The search in flight, started by the render that answers a new
        /// text (`Pending`, #243): the loading state shows while it runs,
        /// and the results are drawn the moment they land.
        pending: Pending<SearchOutcome>,
        /// The details a result's press fetches, on their way.
        /// The details a result's press fetches, on their way, or why
        /// they failed.
        opening: Option<Pending<Result<String, String>>>,
    },
    /// The service address, pushed above the list: a text field and a
    /// Save button (the modern replacement for the typed form the List
    /// document carried).
    Address {
        /// The field's value, as the user typed it.
        address: String,
    },
}

/// What a search answers: the results, or why it failed.
struct SearchOutcome {
    results: Vec<Summary>,
    failed: Option<String>,
}

/// Searches `query` with the service, as the view's List asks for it.
async fn search(query: String) -> SearchOutcome {
    let found: Result<Found, String> = fetch(&format!("/search?q={}", encode(&query))).await;
    match found {
        Ok(found) => SearchOutcome {
            results: found.results,
            failed: None,
        },
        Err(why) => SearchOutcome {
            results: Vec::new(),
            failed: Some(why),
        },
    }
}

impl View for Search {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
        match self {
            Search::Rows {
                query,
                searched,
                results,
                failed,
                pending,
                opening,
            } => {
                // A result's details arrived: shown in a toast, the state
                // cleared.
                let opened = opening
                    .as_mut()
                    .and_then(|opening| opening.ready().cloned());
                if let Some(done) = opened {
                    *opening = None;
                    match done {
                        Ok(details) => show_toast(Toast::success(details)),
                        Err(why) => show_toast(Toast::failure(why)),
                    };
                }
                // A blank text: nothing is searched, nothing listed.
                if query.trim().is_empty() {
                    *searched = String::new();
                    results.clear();
                    *failed = None;
                }
                // A new text: its search starts here, drawn as the loading
                // state until it answers.
                if query != searched && !query.trim().is_empty() {
                    *searched = query.clone();
                    results.clear();
                    *failed = None;
                    *pending = Pending::loading(search(query.clone()));
                }
                if let Some(outcome) = pending.ready() {
                    results.clear();
                    results.extend(outcome.results.iter().cloned());
                    *failed = outcome.failed.clone();
                }
                // The loading state: while a search runs for a text, and
                // before the first one answers.
                let running = !query.trim().is_empty() && results.is_empty() && failed.is_none();
                let mut list = list()
                    .navigation_title("Package search")
                    .search_placeholder("Search the registry…")
                    .is_loading(running)
                    .on_search_text(cx.value_listener(|this: &mut Self, text: &str| {
                        if let Search::Rows { query, .. } = this {
                            *query = text.to_owned();
                        }
                    }));
                for result in results.iter() {
                    let id = format!("package:{}", result.name);
                    list = list.child(
                        item(id.clone())
                            .title(result.name.clone())
                            .subtitle(result.summary.clone())
                            .on_press(cx.listener(move |this: &mut Self| {
                                if let Search::Rows { opening, .. } = this {
                                    // The details fetch in the render that
                                    // answers this press.
                                    let opened = id.clone();
                                    *opening =
                                        Some(Pending::loading(
                                            async move { outcome(&opened).await },
                                        ));
                                }
                            })),
                    );
                }
                if results.is_empty() {
                    list = list.child(
                        empty_state(match failed {
                            Some(why) => why.clone(),
                            None => "Type to search the package registry".into(),
                        })
                        .description(
                            "Results come from the service as you type; Enter shows a \
                             package's details",
                        )
                        .child(button("Service address").on_click(cx.push(|this| {
                            let held = match this {
                                Search::Rows { query, .. } => query.clone(),
                                _ => String::new(),
                            };
                            let _ = held;
                            Search::Address {
                                address: service().unwrap_or_default(),
                            }
                        }))),
                    );
                }
                list.into_answer()
            }
            Search::Address { address } => column()
                .navigation_title("Service address")
                .gap(Space::M)
                .child(
                    text_input(address.clone())
                        .key("address")
                        .label("Address")
                        .placeholder(DEFAULT_SERVICE)
                        .on_input(cx.value_listener(|this: &mut Self, value: &str| {
                            if let Search::Address { address, .. } = this {
                                *address = value.to_owned();
                            }
                        })),
                )
                .child(button("Save").on_click(cx.pop_with(|this| match this {
                    Search::Address { address } => {
                        let address = address.trim().trim_end_matches('/').to_owned();
                        let _ = settings::set(SERVICE, &address);
                        format!("Searching {address} from now on")
                    }
                    _ => String::new(),
                })))
                .into_answer(),
        }
    }
}
