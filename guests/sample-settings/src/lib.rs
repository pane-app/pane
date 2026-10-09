//! Pane's settings sample: a command whose choice Pane keeps between runs.
//! The chosen greeting style is saved with [`pane_extension::settings`], so it
//! survives restarting Pane and disabling and re-enabling the package. It also
//! keeps one value of each other kind of data: a note ([`content`]), the last
//! greeting ([`cache`]) and a sign-in token ([`credentials`]), so clearing its
//! cache on its page in Settings shows what is removed and what is kept.
//! "Save after waiting" shows a call Pane stops: it notes in its settings
//! that it started, waits ten seconds, then notes that it finished; disabling
//! or reloading the package meanwhile stops it, so it never finishes.
//! "Crash" crashes on purpose (a panic traps the guest): three crashes in a
//! row pause the package until the user retries it, keeping its data.
//! "Count" adds one to a count kept in its content and shows the new count
//! in a toast: an action whose effect is done once it has run. If its answer
//! is lost (Pane's runtime crashed before it answered), Pane does not run it
//! again by itself, so the count never grows without the user asking.
//! "Stop responding" notes in its settings that it started, then computes
//! without waiting for anything for up to a minute before noting that it
//! finished: Pane stops a call that computes for 5 seconds without waiting,
//! so it never finishes, and it counts towards pausing the package as a
//! crash does.
//! "Write to the log" writes a line at each level and a printed line to
//! each output, which Pane keeps in the package's extension log; "Flood the
//! log" writes a line longer than Pane keeps, then more lines in a second
//! than Pane keeps. "Fail" fails with an error the extension answers
//! with.
#![no_std]

use pane_extension::alloc::{format, string::String, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::{
    Command, CustomView, FieldValue, FormError, Item, List, NoCustomView, cache, content,
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
/// The content key holding the count "Count" adds to.
const COUNT: &str = "count";
/// How long "Save after waiting" waits, in nanoseconds.
const SLOW_WAIT: u64 = 10_000_000_000;
/// The settings key where "Stop responding" notes how far it got.
const BUSY: &str = "busy";
/// How long "Stop responding" computes at most, in nanoseconds: bounded, so
/// that even without Pane stopping it, it ends.
const BUSY_FOR: u64 = 60_000_000_000;
/// How long a line "Flood the log" writes first, in bytes: more than Pane
/// keeps of a line.
const FLOOD_LINE: usize = 5_000;
/// How many lines "Flood the log" writes then: more than Pane keeps of a
/// second's.
const FLOOD_LINES: u32 = 1_500;

struct Greeting;
pane_extension::export!(Greeting);

/// Runs the action of the item `id` and shows a toast with what [`outcome`]
/// answers; each item's action is this with its id.
async fn act(id: &str) -> Result<(), String> {
    let done = outcome(id).await?;
    show_toast(Toast::success(done));
    Ok(())
}

/// What the action of the item `id` does, answering what it did or found.
async fn outcome(id: &str) -> Result<String, String> {
    match id {
        "formal" | "casual" => {
            settings::set(STYLE, id)?;
            Ok(format!("Saved the {id} greeting"))
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
        "count" => {
            let count = match content::get(COUNT)? {
                Some(count) => count
                    .parse::<u64>()
                    .map_err(|_| "the count is not a number")?,
                None => 0,
            } + 1;
            content::set(COUNT, &format!("{count}"))?;
            Ok(format!("Counted {count}"))
        }
        "busy" => {
            settings::set(BUSY, "started")?;
            // Computes without awaiting anything: the guest never
            // yields to Pane by itself.
            let now = wasip3::clocks::monotonic_clock::now;
            let end = now() + BUSY_FOR;
            while now() < end {}
            settings::set(BUSY, "finished")?;
            Ok("Finished computing after a minute".into())
        }
        "log" => {
            pane_extension::debug!("a debug line");
            pane_extension::info!("an info line");
            pane_extension::warn!("a warning line");
            pane_extension::error!("an error line");
            pane_extension::println!("a printed line");
            pane_extension::eprintln!("a printed error");
            Ok("Wrote to the log".into())
        }
        "flood" => {
            pane_extension::println!("{}", "x".repeat(FLOOD_LINE));
            for n in 0..FLOOD_LINES {
                pane_extension::println!("line {n}");
            }
            Ok("Flooded the log".into())
        }
        "fail" => Err("failed on purpose".into()),
        // A panic traps the guest: Pane reports a crash, not an error
        // the extension answered with.
        "crash" => panic!("crashed on purpose"),
        other => Err(format!("unknown item: {other}")),
    }
}

impl Command for Greeting {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let title = match settings::get(STYLE)? {
            Some(style) => format!("Greeting: {style}"),
            None => "Greeting".into(),
        };
        let item = |id: &'static str, title: &str, subtitle: &str| {
            Item::new(id, title)
                .subtitle(subtitle)
                .on_action(move || act(id))
        };
        Ok(List::new(title).items([
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
            item("count", "Count", "Adds one to a count kept in its content"),
            // The smokes reach "Stop responding" by its place: items added
            // later go after it.
            item(
                "busy",
                "Stop responding",
                "Computes without waiting for up to a minute; Pane stops it after 5 seconds",
            ),
            item(
                "log",
                "Write to the log",
                "A line at each level, and one printed to each output",
            ),
            item(
                "flood",
                "Flood the log",
                "A line too long to keep, then too many lines in a second",
            ),
            item(
                "fail",
                "Fail",
                "Fails with an error, which the extension answers with",
            ),
        ]))
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
