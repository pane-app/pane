//! Pane's timer sample in Rust: a clock whose screen changes by itself —
//! the view asks Pane to render it again after every second
//! (`refresh-after-ms`, #236) — with its caption arriving as pending data
//! shown first as a loading state (`Pending`), whose arrival
//! asks for a drawing itself (#243): it is shown the moment it lands, with
//! no timer to wait for. The behaviour matches the JavaScript and
//! TypeScript samples (`sample-timer-js`, `sample-timer-ts`): the same
//! texts, the same timings.
#![no_std]

use core::cell::Cell;
use core::time::Duration;

use pane_extension::alloc::{format, string::String};
use pane_extension::view::{
    Cx, IntoAnswer, IntoNode, Pending, Space, TextLevel, TextStyle, View, column, text,
};
use pane_extension::{Command, LaunchRecord};

/// What the timer says while its caption is still loading.
const LOADING: &str = "Loading…";

/// The timer the screen shows: a caption loaded before it runs, and the
/// seconds passed since, one per refresh.
struct Timer {
    caption: Pending<String>,
    elapsed: Cell<u32>,
}

impl View for Timer {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoAnswer {
        match self.caption.ready() {
            Some(caption) => {
                // The render that answers the view's own ask is where the
                // interval's work runs: one tick per second asked for —
                // the drawing the caption's arrival asked for is not one.
                if cx.refreshed() {
                    self.elapsed.set(self.elapsed.get() + 1);
                }
                column()
                    .gap(Space::M)
                    .child(text(caption.as_str()).level(TextLevel::Secondary))
                    .child(
                        text(format!("Elapsed: {}s", self.elapsed.get())).style(TextStyle::Title),
                    )
                    .refresh_after(Duration::from_secs(1))
            }
            // The loading state: shown at once; the caption's arrival
            // asks for the drawing that replaces it.
            None => text(LOADING).level(TextLevel::Secondary).into_answer(),
        }
    }
}

/// The work that fills the screen when it answers: the timer's caption,
/// held back for a moment — as a service being called would be — so the
/// loading state shows once. The wait is the guest's own clock: its
/// arrival asks for a drawing itself (#243), which Pane serves while the
/// instance runs between calls.
fn load_caption() -> impl Future<Output = String> {
    async {
        wasip3::clocks::monotonic_clock::wait_for(150_000_000).await;
        String::from("A second at a time")
    }
}

struct Sample;
pane_extension::export!(Sample);

impl Command for Sample {
    type DesignedView = Timer;

    async fn open_designed_view(_command: String, _launch: LaunchRecord) -> Result<Timer, String> {
        Ok(Timer {
            caption: Pending::loading(load_caption()),
            elapsed: Cell::new(0),
        })
    }
}
