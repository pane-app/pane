//! Pane's no-view sample: commands that run without opening a screen
//! (`"mode": "no-view"` in `pane.json`), and one view command, all served by
//! one component. Each receives its launch record: how it was launched (by
//! the user or in the background, and from where), the text sent through
//! its alias or as a fallback, and the context another command passed.
//!
//! - "Report launch" shows its launch record in a toast, and keeps it in
//!   its settings for "Last launches". Sent "fail", it answers an error,
//!   which Pane shows as a failure toast and which never pauses the
//!   extension; sent "crash", it crashes, and three crashes within five
//!   minutes pause the extension.
//! - "Tick" runs every minute on its own schedule, in the background,
//!   counting its runs and keeping its last launch record.
//! - "Last launches" shows what "Report launch" and "Tick" kept.
//! - "Launch" launches the command its text names, passing it the context
//!   `{"from":"launch"}`: `report` (a command of this package), or
//!   `<package identity>#report` (one of another package), user-initiated,
//!   or in the background when the text starts with `background `, and
//!   shows a toast saying so.
//! - "Show launch" is a view command: its list shows its launch record.
//!
//! A command launched in the background (a schedule's run, or one another
//! command launched so) does its work but shows no toast. The JavaScript
//! and TypeScript no-view samples show the same.
#![no_std]

use pane_guest::alloc::{format, string::String, vec::Vec};
use pane_guest::commands::{self, CommandRef, launch_type_name, source_name};
use pane_guest::feedback::{Toast, show_toast};
use pane_guest::{Command, Item, LaunchRecord, LaunchType, List, NoCustomView, settings};

/// The settings key holding the last launch record "Report launch" ran
/// with.
const REPORT: &str = "report";
/// The settings key holding how many times "Tick" ran.
const TICKS: &str = "ticks";
/// The settings key holding the last launch record "Tick" ran with.
const TICK: &str = "tick";
/// The context "Launch" passes the command it launches.
const CONTEXT: &str = r#"{"from":"launch"}"#;

struct NoView;
pane_guest::export!(NoView);

/// `launch` in words: its type and source, then what it was given.
fn describe(launch: &LaunchRecord) -> String {
    let arguments = if launch.arguments.is_empty() {
        String::from("none")
    } else {
        launch
            .arguments
            .iter()
            .map(|argument| format!("{}={}", argument.name, argument.value))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "{} from {}; fallback text: {}; context: {}; arguments: {arguments}",
        launch_type_name(launch.launch_type),
        source_name(launch.source),
        launch.fallback_text.as_deref().unwrap_or("none"),
        launch.context.as_deref().unwrap_or("none"),
    )
}

/// What "Report launch" shows for `launch`.
fn report(launch: &LaunchRecord) -> Result<String, String> {
    match launch.fallback_text.as_deref() {
        Some("fail") => return Err("Report launch fails on request".into()),
        Some("crash") => panic!("Report launch crashes on request"),
        _ => {}
    }
    let described = describe(launch);
    settings::set(REPORT, &described)?;
    Ok(format!("Report: {described}"))
}

/// What "Tick" would show for `launch`, counting the run.
fn tick(launch: &LaunchRecord) -> Result<String, String> {
    let ticks = settings::get(TICKS)?
        .and_then(|ticks| ticks.parse::<u64>().ok())
        .unwrap_or(0)
        + 1;
    settings::set(TICKS, &format!("{ticks}"))?;
    settings::set(TICK, &describe(launch))?;
    Ok(format!("Ticked {ticks} times"))
}

/// What "Last launches" shows: what "Report launch" and "Tick" kept.
fn last() -> Result<String, String> {
    let report = settings::get(REPORT)?.unwrap_or_else(|| "none".into());
    let ticks = settings::get(TICKS)?.unwrap_or_else(|| "0".into());
    let tick = settings::get(TICK)?.unwrap_or_else(|| "none".into());
    Ok(format!(
        "Last report: {report}. Ticks: {ticks}; last tick: {tick}"
    ))
}

/// Launches the command `text` names, as "Launch" does, answering what it
/// shows.
fn launch(text: Option<&str>) -> Result<String, String> {
    let text = text.ok_or(
        "Launch needs the command to launch: send it `report`, \
                           `background report` or `<package identity>#report`",
    )?;
    let (launch_type, named) = match text.strip_prefix("background ") {
        Some(named) => (LaunchType::Background, named.trim()),
        None => (LaunchType::UserInitiated, text),
    };
    let target = match named.rsplit_once('#') {
        Some((source, command)) => CommandRef {
            source: Some(source.into()),
            command: command.into(),
        },
        None => CommandRef {
            source: None,
            command: named.into(),
        },
    };
    commands::launch(&target, launch_type, &[], Some(CONTEXT))?;
    Ok(match launch_type {
        LaunchType::Background => format!("Launched {named} in the background"),
        LaunchType::UserInitiated => format!("Launched {named}"),
    })
}

impl Command for NoView {
    type CustomView = NoCustomView;

    /// "Show launch": its launch record, one line per part.
    async fn render() -> Result<List, String> {
        let launch = commands::current();
        let line = |id: &'static str, title: String| Item::new(id, title);
        Ok(List::new("Launch record").items([
            line(
                "type",
                format!("Launch type: {}", launch_type_name(launch.launch_type)),
            ),
            line("source", format!("Source: {}", source_name(launch.source))),
            line(
                "fallback-text",
                format!(
                    "Fallback text: {}",
                    launch.fallback_text.as_deref().unwrap_or("none")
                ),
            ),
            line(
                "context",
                format!("Context: {}", launch.context.as_deref().unwrap_or("none")),
            ),
        ]))
    }

    async fn run(command: String, launch: LaunchRecord) -> Result<(), String> {
        let done = match command.as_str() {
            "report" => report(&launch),
            "tick" => tick(&launch),
            "last" => last(),
            "launch" => self::launch(launch.fallback_text.as_deref()),
            other => Err(format!("unknown command: {other}")),
        }?;
        // Nobody is there to see a background launch's toast.
        if launch.launch_type != LaunchType::Background {
            show_toast(Toast::success(done));
        }
        Ok(())
    }
}
