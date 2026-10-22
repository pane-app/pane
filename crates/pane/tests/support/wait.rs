//! Waiting, in the native windows' tests on GPUI's test platform, for what
//! arrives from other threads and for the animation frames a window asks
//! for. Shared by the test binaries of `pane`; the `settle` support
//! waits for the launcher's own view instead.
#![allow(dead_code)]

use std::path::Path;
use std::time::{Duration, Instant};

use gpui::VisualTestContext;

/// Runs `cx` until `done` returns a value, so that work arriving from
/// other threads (a link opening, a save) has been drawn.
pub fn until<T>(
    cx: &mut VisualTestContext,
    mut done: impl FnMut(&mut VisualTestContext) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if let Some(value) = done(cx) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the window to draw"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs `cx` until the settings record in `data` holds `held` (a field
/// and its value, as the record writes them): the save a page started is
/// written off the window's thread.
pub fn until_record_holds(cx: &mut VisualTestContext, data: &Path, held: &str) {
    let record = data.join("settings.json");
    until(cx, |_| {
        std::fs::read_to_string(&record)
            .ok()
            .filter(|text| text.contains(held))?;
        Some(())
    });
}

/// Delivers the animation frame the window `cx` drives has asked for, as
/// the native frame loop would, with `elapsed` passing first on the test
/// platform's controlled clock. The test platform delivers no frames on
/// its own, so this is the only thing that advances a running transition;
/// one call draws at most one frame. Returns how many next-frame
/// callbacks ran — `0` means the window had asked for no frame, so
/// nothing drew.
pub fn frame(cx: &mut VisualTestContext, elapsed: Duration) -> usize {
    cx.executor().advance_clock(elapsed);
    let ran = cx.update(|window, cx| window.simulate_next_frame(cx));
    cx.run_until_parked();
    ran
}

/// Delivers frames until the window asks for none, so a transition in
/// flight completes, and returns the frames it delivered. `0` means the
/// window was already idle: no frame was pending. Bounded, so a window
/// that never stopped asking for frames fails the test instead of
/// hanging it.
pub fn settle_frames(cx: &mut VisualTestContext) -> usize {
    let mut delivered = 0;
    for _ in 0..20 {
        let ran = frame(cx, Duration::from_millis(25));
        if ran == 0 {
            return delivered;
        }
        delivered += ran;
    }
    panic!("the window never stopped asking for animation frames");
}
