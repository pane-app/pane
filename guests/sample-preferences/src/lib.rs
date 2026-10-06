//! Pane's preferences sample: a package that declares preferences of every
//! type in `pane.json`, for the whole extension and for single commands,
//! and commands that read their effective values through
//! `pane_guest::preferences` as types of their own (serde).
//!
//! - The package declares an API key (a password, required, no default),
//!   units (a dropdown, required, with a default), a greeting (text,
//!   optional) and "Verbose" (a checkbox). Until the API key is set every
//!   command needs setup: Pane shows its Setup screen, with the package's
//!   `HELP.md`, before the first run.
//! - "Show preferences" is a view command that also declares a notes
//!   folder (a folder, required), a notes file (a file) and an editor (an
//!   application): its list shows every value it received.
//! - "Report preferences" is a no-view command that also declares "Loud"
//!   (a checkbox): it answers where it was launched from and its values,
//!   shouted when Loud is on.
//! - "Tick" runs every minute on its own schedule, in the background, once
//!   the package is set up, counting its runs; "Last tick" answers the
//!   count.
//!
//! The JavaScript and TypeScript preferences samples answer the same.
#![no_std]

use pane_guest::alloc::{format, string::String};
use pane_guest::commands::source_name;
use pane_guest::{Command, Item, LaunchRecord, List, NoCustomView, preferences, settings};
use serde::Deserialize;

/// The settings key holding how many times "Tick" ran.
const TICKS: &str = "ticks";

/// What "Show preferences" receives: its package's preferences and its
/// own.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShowPreferences {
    api_key: String,
    units: String,
    greeting: Option<String>,
    verbose: bool,
    folder: String,
    notes: Option<String>,
    editor: Option<String>,
}

/// What "Report preferences" receives: its package's preferences and its
/// own.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportPreferences {
    api_key: String,
    units: String,
    greeting: Option<String>,
    verbose: bool,
    loud: bool,
}

struct Preferences;
pane_guest::export!(Preferences);

/// "none" for a value that is absent.
fn or_none(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("none")
}

/// What "Report preferences" answers, launched as `launch` says.
fn report(launch: &LaunchRecord) -> Result<String, String> {
    let values: ReportPreferences = preferences::values()?;
    let report = format!(
        "Report from {}: API key of {} characters; units: {}; greeting: {}; verbose: {}",
        source_name(launch.source),
        values.api_key.chars().count(),
        values.units,
        or_none(&values.greeting),
        values.verbose,
    );
    Ok(if values.loud {
        report.to_uppercase()
    } else {
        report
    })
}

/// What "Tick" answers, counting the run.
fn tick() -> Result<String, String> {
    let ticks = settings::get(TICKS)?
        .and_then(|ticks| ticks.parse::<u64>().ok())
        .unwrap_or(0)
        + 1;
    settings::set(TICKS, &format!("{ticks}"))?;
    Ok(format!("Ticked {ticks} times"))
}

/// What "Last tick" answers: how many times "Tick" ran.
fn last() -> Result<String, String> {
    let ticks = settings::get(TICKS)?.unwrap_or_else(|| "0".into());
    Ok(format!("Ticks: {ticks}"))
}

impl Command for Preferences {
    type CustomView = NoCustomView;

    /// "Show preferences": every value it received, one per item.
    async fn render() -> Result<List, String> {
        let values: ShowPreferences = preferences::values()?;
        Ok(List::new("Preferences").items([
            Item::new(
                "api-key",
                format!("API key: {} characters", values.api_key.chars().count()),
            ),
            Item::new("units", format!("Units: {}", values.units)),
            Item::new(
                "greeting",
                format!("Greeting: {}", or_none(&values.greeting)),
            ),
            Item::new("verbose", format!("Verbose: {}", values.verbose)),
            Item::new("folder", format!("Notes folder: {}", values.folder)),
            Item::new("notes", format!("Notes file: {}", or_none(&values.notes))),
            Item::new("editor", format!("Editor: {}", or_none(&values.editor))),
        ]))
    }

    async fn run(command: String, launch: LaunchRecord) -> Result<String, String> {
        match command.as_str() {
            "report" => report(&launch),
            "tick" => tick(),
            "last" => last(),
            other => Err(format!("unknown command: {other}")),
        }
    }
}
