//! Pane's settings sample: a command whose choice Pane keeps between runs.
//! The chosen greeting style is saved with [`pane_guest::settings`], so it
//! survives restarting Pane and disabling and re-enabling the package. It also
//! keeps one value of each other kind of data: a note ([`content`]), the last
//! greeting ([`cache`]) and a sign-in token ([`credentials`]), so clearing its
//! cache in Manage extensions shows what is removed and what is kept.
//! "Save after waiting" shows a call Pane stops: it notes in its settings
//! that it started, waits ten seconds, then notes that it finished; disabling
//! or reloading the package meanwhile stops it, so it never finishes.
//! "Crash" crashes on purpose (a panic traps the guest): three crashes in a
//! row pause the package until the user retries it, keeping its data.
#![no_std]

use pane_guest::alloc::{format, string::String, vec, vec::Vec};
use pane_guest::{
    CustomView, FieldValue, FormError, Guest, Item, NoCustomView, View, cache, content,
    credentials, settings,
};

/// The settings key holding the chosen greeting style.
const STYLE: &str = "greeting-style";
/// The content key holding the user's note.
const NOTE: &str = "note";
/// The cache key holding the last greeting, which "Greet me" can make again.
const LAST_GREETING: &str = "last-greeting";
/// The credentials key holding the sign-in token.
const TOKEN: &str = "token";
/// The settings key where "Save after waiting" notes how far it got.
const SLOW_SAVE: &str = "slow-save";
/// How long "Save after waiting" waits, in nanoseconds.
const SLOW_WAIT: u64 = 10_000_000_000;

struct Greeting;
pane_guest::export!(Greeting);

impl Guest for Greeting {
    type CustomView = NoCustomView;

    async fn get_view() -> Result<View, String> {
        let title = match settings::get(STYLE)? {
            Some(style) => format!("Greeting: {style}"),
            None => "Greeting".into(),
        };
        let item = |id: &str, title: &str, subtitle: &str| Item {
            id: id.into(),
            title: title.into(),
            subtitle: Some(subtitle.into()),
            form: None,
            platforms: None,
            custom_view: None,
        };
        Ok(View {
            title,
            items: vec![
                item(
                    "formal",
                    "Use a formal greeting",
                    "Saved in Pane's settings",
                ),
                item(
                    "casual",
                    "Use a casual greeting",
                    "Saved in Pane's settings",
                ),
                item("greet", "Greet me", "Answer in the saved style"),
                item(
                    "note",
                    "Save a note",
                    "Kept in Pane as the extension's content",
                ),
                item("sign-in", "Sign in", "Keeps a token as a local credential"),
                item(
                    "kept",
                    "Show what Pane keeps",
                    "Settings, content, cache and credential",
                ),
                item(
                    "slow",
                    "Save after waiting",
                    "Waits 10 seconds, then saves; disabling or reloading stops it",
                ),
                item(
                    "crash",
                    "Crash",
                    "Crashes on purpose; three crashes within five minutes pause the extension",
                ),
            ],
        })
    }

    async fn run_action(item_id: String) -> Result<String, String> {
        match item_id.as_str() {
            "formal" | "casual" => {
                settings::set(STYLE, &item_id)?;
                Ok(format!("Saved the {item_id} greeting"))
            }
            "greet" => {
                let greeting = match settings::get(STYLE)?.as_deref() {
                    Some("formal") => "Good day to you",
                    Some("casual") => "Hi there",
                    _ => return Err("No greeting style is saved yet; choose one first".into()),
                };
                cache::set(LAST_GREETING, greeting)?;
                Ok(greeting.into())
            }
            "note" => {
                content::set(NOTE, "Water the plants")?;
                Ok("Saved a note".into())
            }
            "sign-in" => {
                credentials::set(TOKEN, "sample-token")?;
                Ok("Signed in on this computer".into())
            }
            "kept" => {
                let or_none = |value: Option<String>| value.unwrap_or_else(|| "none".into());
                let signed_in = match credentials::get(TOKEN)? {
                    Some(_) => "yes",
                    None => "no",
                };
                Ok(format!(
                    "Style: {} · Note: {} · Signed in: {signed_in} · Cached greeting: {}",
                    or_none(settings::get(STYLE)?),
                    or_none(content::get(NOTE)?),
                    or_none(cache::get(LAST_GREETING)?),
                ))
            }
            "slow" => {
                settings::set(SLOW_SAVE, "started")?;
                // The guest suspends here; if Pane stops the call meanwhile,
                // nothing after this line runs.
                wasip3::clocks::monotonic_clock::wait_for(SLOW_WAIT).await;
                settings::set(SLOW_SAVE, "finished")?;
                Ok("Saved after waiting 10 seconds".into())
            }
            // A panic traps the guest: Pane reports a crash, not an error
            // the extension answered with.
            "crash" => panic!("crashed on purpose"),
            other => Err(format!("unknown item: {other}")),
        }
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
