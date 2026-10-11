//! Pane's registrations sample: a command that registers at run time what
//! it owns (ADR 0041, #158) — a dynamic root item under its own command,
//! updated by a timer; a folder watcher on the folder its settings name;
//! and a provision of the `pane-samples:greet@1` capability, made after a
//! "Sign in" action and held until "Sign out" or the code's replacement.
//! The package's `pane.json` declares the activation entry point, so the
//! registrations exist as soon as Pane may run the code, without the user
//! asking; each handle lives as long as the code that made it, so a
//! disable, a reload, an update, an uninstall or a pause undoes them all,
//! and activating again makes them afresh.
//!
//! The dynamic root item counts the timer's firings and the watcher's
//! changes in the command's content, so they are visible from root search
//! and survive a replacement of the code; its own action "Add one" adds
//! to the count, as an action of a registered item is repeatable. The
//! watcher's changes are coalesced by Pane; the item's title says how
//! many there have been. "Sign in" provides the capability: the
//! capabilities sample, which uses `pane-samples:greet@1` from every
//! provider, lists this package's answer while the provision is held and
//! not while it is dropped. Items, titles, results and errors match the
//! JavaScript and TypeScript registrations samples.
#![no_std]

use core::cell::RefCell;

use pane_extension::alloc::{borrow::ToOwned, format, string::String, string::ToString};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::registrations::{self, Change, Events, Item};
use pane_extension::{Command, List, NoCustomView, content, publish, settings};

/// The command's id in `pane.json`, which the dynamic root item is under.
const COMMAND: &str = "registrations";

/// The content key holding how many timer firings there have been, ever.
const TICKS: &str = "ticks";

/// The content key holding how many folder watcher changes there have
/// been, ever.
const CHANGES: &str = "changes";

/// The settings key holding the folder the watcher watches, as an
/// absolute path; a test or a user writes it. Empty for none.
const FOLDER: &str = "folder";

/// How often the timer fires, in seconds.
const EVERY: u64 = 1;

/// The dynamic root item the activation registered, and the handles of
/// the timer and the watcher that keep it current: this instance's own
/// state, dropped with it when the generation ends.
static ITEM: Cell<registrations::RootItem> = Cell(RefCell::new(None));
static TIMER: Cell<registrations::Timer> = Cell(RefCell::new(None));
static WATCHER: Cell<registrations::Watcher> = Cell(RefCell::new(None));
static PROVISION: Cell<registrations::Provision> = Cell(RefCell::new(None));

/// An owned handle this instance keeps, or none.
struct Cell<T>(RefCell<Option<T>>);

// SAFETY: a component's code runs on one thread, and no borrow is held
// across an `await`.
unsafe impl<T> Sync for Cell<T> {}

impl<T> Cell<T> {
    /// Replaces what is kept with `held`.
    fn set(&self, held: Option<T>) {
        *self.0.borrow_mut() = held;
    }
}

struct Registrations;
pane_extension::export!(Registrations);
pane_extension::publish::export!(Registrations);
pane_extension::registrations::export_events!(Events);
pane_extension::lifecycle::export!(Registrations);

impl pane_extension::lifecycle::Guest for Registrations {
    /// Registers the dynamic root item, the timer that updates it and the
    /// folder watcher: Pane calls this as the code may run, so the
    /// registrations exist without the user asking.
    async fn activate() {
        let item = registrations::root_item(COMMAND, counted_item())
            .expect("the item is within Pane's limits");
        // The timer updates the item: the handle is kept, so the item
        // stays until the code goes.
        let ticked = registrations::every(EVERY, || async {
            let count = counted(TICKS) + 1;
            content::set(TICKS, &count.to_string())?;
            update(counted_item())?;
            Ok(())
        })
        .expect("the timer is within Pane's bounds");
        let watched = settings::get(FOLDER)
            .ok()
            .flatten()
            .filter(|folder| !folder.trim().is_empty())
            .map(|folder| {
                registrations::watch_folder(&folder, false, |change| async {
                    match change {
                        Change::Paths(paths) => {
                            let count = counted(CHANGES) + paths.len() as u64;
                            content::set(CHANGES, &count.to_string())?;
                        }
                        Change::Rescan => {
                            let count = counted(CHANGES) + 1;
                            content::set(CHANGES, &count.to_string())?;
                        }
                    }
                    update(counted_item())?;
                    Ok(())
                })
            })
            .transpose()
            .expect("the folder can be watched");
        ITEM.set(Some(item));
        TIMER.set(Some(ticked));
        WATCHER.set(watched);
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

/// The dynamic root item as it stands: a row of root search under this
/// command, its subtitle saying where the counts stand.
fn counted_item() -> Item {
    Item::new("counted", "Registrations: counting")
        .subtitle(format!(
            "{} timer firings, {} watcher changes (Rust)",
            counted(TICKS),
            counted(CHANGES)
        ))
        .action(registrations::Action::new("Add one", || async {
            let count = counted(TICKS) + 1;
            content::set(TICKS, &count.to_string())?;
            update(counted_item())?;
            Ok(())
        }))
}

/// Replaces the registered item with `item`.
fn update(item: Item) -> Result<(), String> {
    match ITEM.0.borrow().as_ref() {
        Some(handle) => handle.replace(item),
        None => Err("the item was not registered".into()),
    }
}

/// The operations the capability is made of, served through the export
/// that serves published operations: `greet` arrives qualified by the
/// capability, answered only while the provision is held (a package is a
/// provider of a capability its manifest marks `atRunTime` only while it
/// holds one).
impl publish::Guest for Registrations {
    async fn run_operation(operation: String, input: String) -> Result<String, String> {
        match operation.as_str() {
            "pane-samples:greet@1/greet" => {
                let name = name_of(&input)?;
                Ok(format!(
                    "{{\"greeting\":\"Rust registrations sample greets {name}\"}}"
                ))
            }
            _ => Err(format!("this component serves no `{operation}`")),
        }
    }
}

/// The `name` of an operation's JSON input.
fn name_of(input: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(input).map_err(|error| format!("the input is not JSON: {error}"))?;
    Ok(value
        .get("name")
        .and_then(|name| name.as_str())
        .unwrap_or("everyone")
        .to_owned())
}

impl Command for Registrations {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        let signed_in = PROVISION.0.borrow().is_some();
        let watching = WATCHER.0.borrow().is_some();
        Ok(List::new(format!(
            "Registrations: {} firings, {} changes",
            counted(TICKS),
            counted(CHANGES)
        ))
        .item(
            pane_extension::Item::new(
                "sign",
                if signed_in {
                    "Sign out of the provision"
                } else {
                    "Sign in to the provision"
                },
            )
            .subtitle(
                "Provides pane-samples:greet@1 while held; the capabilities sample answers from it",
            )
            .on_action(|| async {
                if PROVISION.0.borrow().is_some() {
                    PROVISION.set(None);
                    show_toast(Toast::success("Signed out; the provision is dropped"));
                } else {
                    let provision = registrations::provide("pane-samples:greet@1")?;
                    PROVISION.set(Some(provision));
                    show_toast(Toast::success(
                        "Signed in; the provision is held until signed out",
                    ));
                }
                Ok(())
            }),
        )
        .item(
            pane_extension::Item::new("watched", "The watched folder").subtitle(if watching {
                "The folder this settings key names; changes update the row in root search"
            } else {
                "None: the settings key `folder` is empty or unset"
            }),
        ))
    }
}
