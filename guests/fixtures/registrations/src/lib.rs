//! Test fixture: a package that registers at run time what Pane must undo
//! (#158), whose items drive the edges the tests ask for. Tests install
//! it as many packages, each with its own pane.json naming what it
//! provides (a capability, marked `atRunTime` or not) and its activation
//! entry point or none. The fixture's own settings say what its items do:
//! the key `many` how many items or timers to register beyond the limit,
//! `folder` the folder to watch, `capability` the capability to provide,
//! and `trap` whether the activation entry point traps — a trap in it is
//! a crash of the package, counted towards pausing it.
//!
//! Items: "Register the held item" registers a dynamic root item under
//! the command and keeps its handle, so later items can update and drop
//! it; "Update the held item" replaces it through the handle, answering
//! the refusal when this code of the extension was replaced; "Drop the
//! held item" drops the handle; "Register many items" and "Register many
//! timers" register as many as the settings key `many` names plus one,
//! so the last is refused with the limit named; "Register a 0-second
//! timer" and "Register a 31-day timer" are refused with the bounds;
//! "Watch the folder" watches the settings key `folder`'s path;
//! "Provide the capability" provides the settings key `capability`'s
//! name, and "Provide the undeclared capability" one no pane.json
//! declares; the timer the fixture registers fires with the tag
//! "fixture", counting into its content, so a firing is visible.
#![no_std]

use core::cell::RefCell;

use pane_extension::alloc::{format, string::String, string::ToString};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::registrations::{self, Change, Events, Item};
use pane_extension::{Command, List, NoCustomView, content, publish, settings};

/// The command's id in the pane.json the test writes.
const COMMAND: &str = "fixture";

/// The settings key holding how many items or timers to register, plus
/// one, so the last is beyond Pane's limit.
const MANY: &str = "many";

/// The settings key holding the folder to watch.
const FOLDER: &str = "folder";

/// The settings key holding the capability to provide.
const CAPABILITY: &str = "capability";

/// The settings key holding whether the activation entry point traps.
const TRAP: &str = "trap";

/// The content key counting the timer's firings.
const FIRED: &str = "fired";

/// The dynamic root item's handle, kept by this instance.
static HELD: Held = Held(RefCell::new(None));

/// The activation's timer's handle, kept with it: dropping the handle
/// stops the timer, and the tests read its firings.
static TICKING: Ticking = Ticking(RefCell::new(None));

struct Held(RefCell<Option<registrations::RootItem>>);

struct Ticking(RefCell<Option<registrations::Timer>>);

// SAFETY: a component's code runs on one thread, and no borrow is held
// across an `await`.
unsafe impl Sync for Held {}

// SAFETY: as `Held`.
unsafe impl Sync for Ticking {}

struct Fixture;
pane_extension::export!(Fixture);
pane_extension::publish::export!(Fixture);
pane_extension::registrations::export_events!(Events);
pane_extension::lifecycle::export!(Fixture);

impl pane_extension::lifecycle::Guest for Fixture {
    /// The activation entry point: traps when the settings say so (a trap
    /// in it is a crash, counted towards pausing the package), and
    /// registers a timer otherwise, so activating is visible in the
    /// fixture's content.
    async fn activate() {
        if settings::get(TRAP).ok().flatten().as_deref() == Some("yes") {
            panic!("the activation entry point traps, as the test asked");
        }
        // The handle is kept with the instance, so the timer fires until
        // its generation ends. A refusal, if any, drops it and leaves the
        // content empty: the activation itself still answered.
        if let Ok(ticking) = registrations::every(1, || async {
            let fired = counted(FIRED) + 1;
            content::set(FIRED, &fired.to_string())?;
            Ok(())
        }) {
            *TICKING.0.borrow_mut() = Some(ticking);
        }
    }
}

/// The count kept in the command's content, zero before the first.
fn counted(key: &str) -> u64 {
    content::get(key)
        .ok()
        .flatten()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

/// What the item `item_id`'s action does, answered as a toast's text.
async fn act(item_id: &str) -> Result<(), String> {
    let done = outcome(item_id)?;
    show_toast(Toast::success(done));
    Ok(())
}

/// What the action of the item `item_id` does, answering what it did or
/// the refusal it met: the refusals are the edges the tests assert.
fn outcome(item_id: &str) -> Result<String, String> {
    let count = |name: &str| {
        settings::get(name)
            .ok()
            .flatten()
            .and_then(|many| many.parse().ok())
            .unwrap_or(0)
    };
    match item_id {
        "held" => {
            let item = registrations::root_item(COMMAND, held_item("held"))?;
            *HELD.0.borrow_mut() = Some(item);
            Ok("Registered the held item".into())
        }
        "update" => {
            let item = held_item("replaced");
            match HELD.0.borrow().as_ref() {
                Some(handle) => handle
                    .replace(item)
                    .map(|()| "Updated the held item".into()),
                None => Err("no item is held".into()),
            }
        }
        "drop" => {
            *HELD.0.borrow_mut() = None;
            Ok("Dropped the held item".into())
        }
        "items" => {
            let many = count(MANY);
            let mut registered = 0;
            for index in 0..=many {
                match registrations::root_item(COMMAND, held_item(&format!("many{index}"))) {
                    Ok(_) => registered += 1,
                    Err(refusal) => return Ok(format!("{registered} then refused: {refusal}")),
                }
            }
            Ok(format!("Registered {registered} items"))
        }
        "timers" => {
            let many = count(MANY);
            let mut registered = 0;
            for _ in 0..=many {
                // The component exports the events entry point, so the
                // refusal, when one comes, is the limit's.
                match registrations::every(60, || async { Ok(()) }) {
                    Ok(_) => registered += 1,
                    Err(refusal) => return Ok(format!("{registered} then refused: {refusal}")),
                }
            }
            Ok(format!("Registered {registered} timers"))
        }
        "fast" => match registrations::after(0, || async { Ok(()) }) {
            Ok(_) => Ok("Registered a 0-second timer".into()),
            Err(refusal) => Ok(format!("refused: {refusal}")),
        },
        "slow" => match registrations::after(31 * 86_400, || async { Ok(()) }) {
            Ok(_) => Ok("Registered a 31-day timer".into()),
            Err(refusal) => Ok(format!("refused: {refusal}")),
        },
        "watch" => {
            let folder = settings::get(FOLDER).ok().flatten().unwrap_or_default();
            let watched = registrations::watch_folder(&folder, false, |change| async {
                match change {
                    Change::Paths(paths) => {
                        let fired = counted(FIRED) + paths.len() as u64;
                        content::set(FIRED, &fired.to_string())?;
                    }
                    Change::Rescan => {
                        let fired = counted(FIRED) + 1;
                        content::set(FIRED, &fired.to_string())?;
                    }
                }
                Ok(())
            });
            match watched {
                Ok(_) => Ok(format!("Watched {folder}")),
                Err(refusal) => Ok(format!("refused: {refusal}")),
            }
        }
        "provide" => {
            let capability = settings::get(CAPABILITY)
                .ok()
                .flatten()
                .unwrap_or_else(|| "fixture:held@1".into());
            match registrations::provide(&capability) {
                Ok(_) => Ok(format!("Provided {capability}")),
                Err(refusal) => Ok(format!("refused: {refusal}")),
            }
        }
        "undeclared" => match registrations::provide("fixture:nobody@1") {
            Ok(_) => Ok("Provided fixture:nobody@1".into()),
            Err(refusal) => Ok(format!("refused: {refusal}")),
        },
        _ => Err(format!("unknown item: {item_id}")),
    }
}

/// The dynamic root item with id `id`, titled as the fixture's.
fn held_item(id: &str) -> Item {
    Item::new(id, "Fixture's item").subtitle(format!(
        "the fixture registered this; {} firings",
        counted(FIRED)
    ))
}

/// The operations the manifests' `provides` entries name, served through
/// the export that serves published operations: the fixture answers the
/// one operation its test manifests declare, `greet` of the fixture's
/// capability, qualified by the capability as a capability call arrives.
impl publish::Guest for Fixture {
    async fn run_operation(operation: String, _input: String) -> Result<String, String> {
        match operation.as_str() {
            "fixture:held@1/greet" => Ok("{\"greeting\":\"the fixture greets\"}".into()),
            _ => Err(format!("this component serves no `{operation}`")),
        }
    }
}

impl Command for Fixture {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        // An item whose action is `act` with its id.
        let item = |id: &'static str, title: &str, subtitle: &str| {
            pane_extension::Item::new(id, title)
                .subtitle(subtitle)
                .on_action(move || act(id))
        };
        Ok(
            List::new(format!("Registrations fixture: {} firings", counted(FIRED)))
                .item(item(
                    "held",
                    "Register the held item",
                    "A dynamic root item whose handle this instance keeps",
                ))
                .item(item(
                    "update",
                    "Update the held item",
                    "Replaces it through the handle; the refusal names what replaced this code",
                ))
                .item(item(
                    "drop",
                    "Drop the held item",
                    "Drops the handle, which removes the row",
                ))
                .item(item(
                    "items",
                    "Register many items",
                    "The settings key `many` plus one, so the last is refused with the limit",
                ))
                .item(item(
                    "timers",
                    "Register many timers",
                    "The settings key `many` plus one, so the last is refused with the limit",
                ))
                .item(item(
                    "fast",
                    "Register a 0-second timer",
                    "Refused: below Pane's 1-second minimum",
                ))
                .item(item(
                    "slow",
                    "Register a 31-day timer",
                    "Refused: beyond Pane's 30-day maximum",
                ))
                .item(item(
                    "watch",
                    "Watch the folder",
                    "The settings key `folder`'s path; its changes count in the content",
                ))
                .item(item(
                    "provide",
                    "Provide the capability",
                    "The settings key `capability`'s name, declared and marked in pane.json",
                ))
                .item(item(
                    "undeclared",
                    "Provide the undeclared capability",
                    "fixture:nobody@1, which no pane.json declares",
                )),
        )
    }
}
