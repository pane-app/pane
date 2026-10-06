//! Pane's arguments sample: no-view commands that ask for typed values
//! before they run (`"arguments"` in `pane.json`), all served by one
//! component. Each receives the values in its launch record, by name; an
//! optional argument left empty is absent.
//!
//! - "Greet" has three arguments: a required text (`name`), an optional
//!   password (`secret`) and a dropdown (`tone`). It answers which values it
//!   was given (only the length of the secret, which it never shows or
//!   keeps), where it was launched from and the fallback text, counting its
//!   runs. Its first argument is text and the others are optional, so it
//!   may be a fallback: text sent to it fills `name`.
//! - "Stamp" has one required text argument (`label`), for launches with
//!   no fields of their own (a global hotkey, a quick slot). It answers the
//!   label and keeps it, with how it was launched, for "Relay last".
//! - "Relay" launches the command of this package its text names, with the
//!   arguments it lists: `stamp label=x`, or `background stamp label=x` in
//!   the background. Sent `last`, it answers what "Stamp" last kept.
//!
//! The JavaScript and TypeScript arguments samples answer the same.
#![no_std]

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::commands::{self, ArgumentValue, CommandRef, launch_type_name, source_name};
use pane_guest::{Command, LaunchRecord, LaunchType, NoCustomView, settings};

/// The settings key holding how many times "Greet" ran.
const RUNS: &str = "greet-runs";
/// The settings key holding the label "Stamp" last stamped, and how.
const STAMP: &str = "stamp";

struct Arguments;
pane_guest::export!(Arguments);

/// What "Greet" answers for `launch`, counting the run.
fn greet(launch: &LaunchRecord) -> Result<String, String> {
    let runs = settings::get(RUNS)?
        .and_then(|runs| runs.parse::<u64>().ok())
        .unwrap_or(0)
        + 1;
    settings::set(RUNS, &format!("{runs}"))?;
    let given: Vec<String> = launch
        .arguments
        .iter()
        .map(|argument| {
            if argument.name == "secret" {
                format!("secret ({} characters)", argument.value.chars().count())
            } else {
                format!("{}={}", argument.name, argument.value)
            }
        })
        .collect();
    let given = if given.is_empty() {
        String::from("nothing")
    } else {
        given.join(", ")
    };
    Ok(format!(
        "Greet run {runs} from {}: {given}; fallback text: {}",
        source_name(launch.source),
        launch.fallback_text.as_deref().unwrap_or("none")
    ))
}

/// What "Stamp" answers for `launch`, keeping what it stamped.
fn stamp(launch: &LaunchRecord) -> Result<String, String> {
    let label = launch.argument("label").ok_or("Stamp has no label")?;
    let source = source_name(launch.source);
    let how = launch_type_name(launch.launch_type);
    settings::set(STAMP, &format!("{label} from {source}, {how}"))?;
    Ok(format!("Stamped {label} from {source}"))
}

/// Launches the command `text` names with the arguments it lists, as
/// "Relay" does, or answers what "Stamp" last kept.
fn relay(text: Option<&str>) -> Result<String, String> {
    let text = text.ok_or(
        "Relay needs what to do: `last`, or a command and its arguments, such as \
         `background stamp label=x`",
    )?;
    if text == "last" {
        let stamp = settings::get(STAMP)?.unwrap_or_else(|| "none".into());
        return Ok(format!("Last stamp: {stamp}"));
    }
    let mut words = text.split_whitespace();
    let mut command = words.next().unwrap_or_default();
    let launch_type = if command == "background" {
        command = words.next().unwrap_or_default();
        LaunchType::Background
    } else {
        LaunchType::UserInitiated
    };
    let arguments: Vec<ArgumentValue> = words
        .map(|word| {
            let (name, value) = word.split_once('=').unwrap_or((word, ""));
            ArgumentValue {
                name: name.into(),
                value: value.into(),
            }
        })
        .collect();
    let target = CommandRef {
        source: None,
        command: command.into(),
    };
    commands::launch(&target, launch_type, &arguments, None)?;
    Ok(match launch_type {
        LaunchType::Background => format!("Relayed {command} in the background"),
        LaunchType::UserInitiated => format!("Relayed {command}"),
    })
}

impl Command for Arguments {
    type CustomView = NoCustomView;

    async fn run(command: String, launch: LaunchRecord) -> Result<String, String> {
        match command.as_str() {
            "greet" => greet(&launch),
            "stamp" => stamp(&launch),
            "relay" => relay(launch.fallback_text.as_deref()),
            other => Err(format!("unknown command: {other}")),
        }
    }
}
