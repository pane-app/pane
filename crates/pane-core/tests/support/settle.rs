//! Root search's publication settling (#201): a provider's late answer
//! merges into the published list, coalesced within 16 ms with any that
//! arrives close after, and the merge's relist lands when the window
//! closes. A search a test awaits ends before that window does, so the
//! tests that read the list right after a search wait the merges out here
//! first — as the publishing tests advance the launcher's clock. Shared by
//! the test binaries that search and read the list.

#![allow(dead_code)]

use std::time::{Duration, Instant};

use pane_core::Launcher;

/// Waits until no late answer is merging into the published list (#201),
/// for at most ten seconds.
pub fn wait_for_merges(launcher: &Launcher) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while launcher.merge_pending() {
        assert!(Instant::now() < deadline, "a late answer never merged");
        std::thread::sleep(Duration::from_millis(2));
    }
}
