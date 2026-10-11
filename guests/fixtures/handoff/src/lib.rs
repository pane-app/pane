//! Test fixture: a package that opts in to the state handoff (ADR 0041,
//! #159), whose item drives the edges the tests ask for. Tests install it
//! as many packages, each with its own pane.json; the fixture's own
//! settings say what its handoff does: the key `snapshot` whether the old
//! code answers late (`wait`) or beyond the size limit (`oversized`) —
//! `no` answers the counter — and the key `restore` whether the new code
//! rejects (`reject`) or traps on (`trap`) the state it is given — `no`
//! restores it.
//!
//! The state itself is a counter this instance keeps in memory: "Add one"
//! counts, and the command's title says where it stands, so a test sees
//! what a replacement kept and what it lost.
#![no_std]

use core::cell::RefCell;

use pane_extension::alloc::{format, string::String, vec, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::{
    Command, CustomView, Item, List, NoCustomView, content, lifecycle, settings, state,
};

/// The settings key saying what the old code's `snapshot` answers.
const SNAPSHOT: &str = "snapshot";

/// The settings key saying what the new code's `restore` does with the
/// state it is given.
const RESTORE: &str = "restore";

/// How long the fixture's snapshot waits when the settings say so: well
/// past the deadline Pane gives a snapshot to answer.
const LONGER_THAN_THE_DEADLINE: u64 = 10_000_000_000;

/// The content key where a holding item notes how far it got.
const HELD: &str = "held";

/// The counter this instance keeps in memory: handed to the new code by
/// the handoff, and lost with the instance without one.
static COUNTER: CounterCell = CounterCell(RefCell::new(0));

// SAFETY: a component's code runs on one thread, and no borrow of the
// counter is held across an `await`.
/// [`RefCell`] is not `Sync`; the wrapper is.
struct CounterCell(RefCell<u64>);

// SAFETY: a component's code runs on one thread, and no borrow is held
// across an `await`.
unsafe impl Sync for CounterCell {}

struct Fixture;
pane_extension::export!(Fixture);
pane_extension::lifecycle::export!(Fixture);

impl lifecycle::Guest for Fixture {
    /// The fixture declares no activation entry point; the interface has
    /// this, so it is here.
    async fn activate() {}

    /// The state handed to the new code: the counter, unless the settings
    /// ask for the edges — a snapshot that answers too late, or one beyond
    /// the size limit.
    async fn snapshot() -> Option<Vec<u8>> {
        match asked(SNAPSHOT).as_deref() {
            Some("wait") => {
                wasip3::clocks::monotonic_clock::wait_for(LONGER_THAN_THE_DEADLINE).await;
                None
            }
            Some("oversized") => Some(vec![0; (1 << 20) + 1]),
            _ => state::save(&*COUNTER.0.borrow()),
        }
    }

    /// Restores what a replaced instance handed over, unless the settings
    /// ask for the edges — a restore that answers an error, or one that
    /// traps.
    async fn restore(bytes: Vec<u8>) -> Result<(), String> {
        match asked(RESTORE).as_deref() {
            Some("reject") => Err("the new code rejects the state, as the test asked".into()),
            Some("trap") => panic!("restoring the state traps, as the test asked"),
            _ => {
                *COUNTER.0.borrow_mut() = state::load(&bytes)?;
                Ok(())
            }
        }
    }
}

impl Command for Fixture {
    type CustomView = NoCustomView;

    /// The counter as it stands, with the items that count and hold.
    async fn render() -> Result<List, String> {
        Ok(List::new(title())
            .item(
                Item::new("add", format!("Add one ({} so far)", *COUNTER.0.borrow())).on_action(
                    || async {
                        *COUNTER.0.borrow_mut() += 1;
                        show_toast(Toast::success("Counted one more"));
                        Ok(())
                    },
                ),
            )
            .item(
                Item::new("hold", "Hold for ten seconds").on_action(|| async {
                    content::set(HELD, "started")?;
                    // The command suspends here; if Pane replaces the code
                    // meanwhile, nothing after this line runs.
                    wasip3::clocks::monotonic_clock::wait_for(LONGER_THAN_THE_DEADLINE).await;
                    content::set(HELD, "finished")?;
                    show_toast(Toast::success("Held for ten seconds"));
                    Ok(())
                }),
            )
            .item(Item::new("crash", "Crash the command").on_action(|| async {
                // Traps, so Pane drops this instance: a test can then
                // replace the code with no instance left to snapshot.
                panic!("crashed on purpose")
            })))
    }
}

/// The command's title, saying where the counter stands.
fn title() -> String {
    format!("Handoff fixture: {} counted", *COUNTER.0.borrow())
}

/// What the fixture's settings say for `key`, if anything.
fn asked(key: &str) -> Option<String> {
    settings::get(key).ok().flatten()
}
