//! A designed view that changes by itself (#236): its render's answer
//! asks Pane to draw it again after some milliseconds
//! (`refresh-after-ms`), so a timer or a poll runs with no change to the
//! extension runtime. These checks drive the launcher with its clock
//! under the test's hand (`ManualClock`, as the schedules' and services'
//! tests do): a view refreshes at the time it asked for, its ask clamped
//! to the 100 ms floor and the 24 h ceiling (driven through the
//! hand-written `designed` fixture, whose answers no SDK writes, so the
//! clamp is Pane's reading alone), refreshes pause while the launcher's
//! window is hidden, one that fell due then catching up once — from the
//! clock's current time, not the ticks it missed — and leaving the view
//! cancelling its refresh. The timer samples in Rust, JavaScript and
//! TypeScript load their data as a loading state first and then advance
//! by themselves, held to the tri-lingual parity the other samples are.
//!
//! Components come from `cargo xtask guests`; the JavaScript and
//! TypeScript ones are the prebuilt components in `guests/prebuilt/`.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::clipboard::{Clock, ManualClock, SystemClock};
use pane_core::{Launcher, Node, NodeKind, Runtime, Screen, Status, TextContent, WindowPresence};

use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

use rows::select_title;

/// One language's timer sample package.
struct Sample {
    package: &'static str,
    language: &'static str,
    title: &'static str,
}

const SAMPLES: [Sample; 3] = [
    Sample {
        package: "sample-timer",
        language: "Rust",
        title: "Timer sample",
    },
    Sample {
        package: "sample-timer-js",
        language: "JavaScript",
        title: "JavaScript timer sample",
    },
    Sample {
        package: "sample-timer-ts",
        language: "TypeScript",
        title: "TypeScript timer sample",
    },
];

/// How long a refresh may take: compiling the guest once is included; a
/// slow, busy machine is not.
const PROMPTLY: Duration = Duration::from_secs(8);

/// `ms` as a duration.
fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

/// The day the refresh ceiling is: 24 hours, in milliseconds.
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// One test's Pane: its data folder, the launcher and the clock the
/// launcher's view refreshes follow.
struct Pane {
    data: TempDir,
    launcher: Launcher,
    /// Pane's clock, which moves only when a test advances it. It starts a
    /// year ahead of the system's, which the launcher uses before it is
    /// given this one.
    clock: Arc<ManualClock>,
}

impl Pane {
    /// A launcher with the assembled packages it is given installed, from
    /// `target/guests/packages`, following this test's clock.
    fn new(packages: &[&str]) -> Pane {
        let data = tempfile::tempdir().unwrap();
        let clock = ManualClock::at(SystemClock.now() + 365 * DAY_MS);
        let launcher =
            Launcher::with_packages(Runtime::start(), Vec::new(), data.path().join("extensions"))
                .with_clock(clock.clone());
        let pane = Pane {
            data,
            launcher,
            clock,
        };
        for package in packages {
            pane.install_assembled(package);
        }
        pane
    }

    /// Installs the assembled package `name`.
    fn install_assembled(&self, name: &str) {
        let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages")
            .join(name);
        assert!(
            folder.exists(),
            "{} is missing; run `cargo xtask guests`",
            folder.display()
        );
        let source = self.data.path().join(name);
        fs::create_dir_all(&source).unwrap();
        for entry in fs::read_dir(&folder).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), source.join(entry.file_name())).unwrap();
        }
        block_on(self.launcher.install_package(&source));
        assert!(matches!(self.launcher.view().status, Status::Result(_)));
    }

    /// Types `query` in root search and opens the command titled `title`.
    fn open(&self, query: &str, title: &str) {
        while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.launcher.back();
        }
        block_on(self.launcher.set_query(query));
        select_title(&self.launcher, title);
        block_on(self.launcher.activate_selected());
    }

    /// The screen's tree, when a designed view is open.
    fn tree(&self) -> Node {
        match &self.launcher.view().screen {
            Screen::DesignedView(view) => view.tree.root.clone(),
            other => panic!("a designed view is open, not {other:?}"),
        }
    }

    /// Every text the screen's tree draws, in order.
    fn texts(&self) -> Vec<String> {
        let mut texts = Vec::new();
        collect_texts(&self.tree(), &mut texts);
        texts
    }

    /// Presses the button with `label` and waits for the tree it answers.
    fn press(&self, label: &str) {
        let (callback, key) = button_of(&self.tree(), label)
            .unwrap_or_else(|| panic!("the tree has a {label} button"));
        block_on(self.launcher.send_designed_event(callback, key.as_deref()));
    }

    /// Waits for the view's refresh to have looked at the clock as it
    /// stands, and every refresh it sent to have answered.
    fn settled(&self) {
        assert!(
            self.launcher.wait_for_view_refresh(PROMPTLY),
            "the view's refresh settled"
        );
    }
}

/// Waits until the open view's texts are `expected`: the samples load
/// their caption through a WASI monotonic wait, which sleeps real time
/// even under the manual clock the rest of the test drives.
fn until_texts(pane: &Pane, expected: &[&str], because: &str) {
    let expected: Vec<String> = expected.iter().map(|text| (*text).to_owned()).collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let shown = pane.texts();
        if shown == expected {
            return;
        }
        assert!(Instant::now() < deadline, "{because}: {shown:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Every text `node`'s tree draws, appended to `into`, in order.
fn collect_texts(node: &Node, into: &mut Vec<String>) {
    let plain = |text: &pane_core::Text| match &text.content {
        TextContent::Plain(content) => Some(content.clone()),
        TextContent::Spans(spans) => (!spans.is_empty()).then(|| {
            spans
                .iter()
                .map(|span| span.text.clone())
                .collect::<String>()
        }),
    };
    match &node.kind {
        NodeKind::Text(text) => {
            if let Some(content) = plain(text) {
                into.push(content);
            }
        }
        // An unknown node draws its fallback, else its children.
        NodeKind::Unknown(_) => {
            if let Some(fallback) = node.fallback.as_deref() {
                collect_texts(fallback, into);
            }
            for child in &node.children {
                collect_texts(child, into);
            }
        }
        _ => {
            for child in &node.children {
                collect_texts(child, into);
            }
        }
    }
}

/// The button with `label` in `node`'s tree: its callback id and key.
fn button_of(node: &Node, label: &str) -> Option<(u32, Option<String>)> {
    match &node.kind {
        NodeKind::Button(button) if button.label == label => {
            button.on_press.map(|id| (id, node.key.clone()))
        }
        NodeKind::Unknown(_) => node
            .fallback
            .as_deref()
            .and_then(|fallback| button_of(fallback, label))
            .or_else(|| {
                node.children
                    .iter()
                    .find_map(|child| button_of(child, label))
            }),
        _ => node
            .children
            .iter()
            .find_map(|child| button_of(child, label)),
    }
}

/// How many times the `designed` fixture's view was drawn, as its tree
/// says: every render, a refresh's included.
fn renders(pane: &Pane) -> u32 {
    pane.texts()
        .iter()
        .find_map(|text| text.strip_prefix("Renders: "))
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("the tree says how often it was drawn"))
}

#[test]
fn the_timer_sample_shows_its_loading_state_then_its_data_then_advances() {
    for Sample {
        package,
        language,
        title,
    } in SAMPLES
    {
        let pane = Pane::new(&[package]);
        pane.open("", title);
        // The loading state is drawn at once; the data is awaited in the
        // prompt refresh it asks for.
        assert_eq!(
            pane.texts(),
            ["Loading…"],
            "the {language} sample's loading state"
        );

        // The caption is a WASI monotonic wait the guest holds: it sleeps
        // real time, which the manual clock does not move, so its arrival
        // is waited for rather than advanced to.
        until_texts(
            &pane,
            &["A second at a time", "Elapsed: 0s"],
            &format!("the {language} sample's data"),
        );
        pane.settled();

        // A second less than the interval: nothing yet.
        pane.clock.advance(ms(999));
        pane.settled();
        assert_eq!(pane.texts(), ["A second at a time", "Elapsed: 0s"]);

        pane.clock.advance(ms(1));
        pane.settled();
        assert_eq!(
            pane.texts(),
            ["A second at a time", "Elapsed: 1s"],
            "the {language} sample ticks at its interval"
        );

        pane.clock.advance(ms(1000));
        pane.settled();
        assert_eq!(pane.texts(), ["A second at a time", "Elapsed: 2s"]);
    }
}

#[test]
fn a_hidden_timer_catches_up_with_one_refresh_not_the_ticks_it_missed() {
    for Sample {
        package,
        language,
        title,
    } in SAMPLES
    {
        let pane = Pane::new(&[package]);
        pane.open("", title);
        // The caption's arrival, as above: a real wait the manual clock
        // does not move.
        until_texts(
            &pane,
            &["A second at a time", "Elapsed: 0s"],
            &format!("the {language} sample's data"),
        );
        pane.settled();

        // The window hides: the view refreshes nothing while it is not
        // drawn, however much time passes.
        pane.launcher.set_window_presence(WindowPresence::Hidden);
        pane.clock.advance(ms(5000));
        pane.settled();
        assert_eq!(
            pane.texts(),
            ["A second at a time", "Elapsed: 0s"],
            "no {language} refresh while hidden"
        );

        // Shown again: the refresh that fell due runs at once, once, and
        // the interval starts from then.
        pane.launcher.set_window_presence(WindowPresence::Shown);
        pane.settled();
        assert_eq!(
            pane.texts(),
            ["A second at a time", "Elapsed: 1s"],
            "the {language} sample caught up once"
        );
        pane.clock.advance(ms(1000));
        pane.settled();
        assert_eq!(pane.texts(), ["A second at a time", "Elapsed: 2s"]);
    }
}

/// The `designed` fixture: a view whose answers are written by hand.
fn fixture() -> Pane {
    let pane = Pane::new(&["designed"]);
    pane.open("", "Designed fixture");
    pane
}

#[test]
fn a_refresh_below_the_floor_runs_at_the_floor() {
    let pane = fixture();
    pane.press("Answer refresh 10ms");
    assert_eq!(renders(&pane), 2, "the press's own drawing");

    // 10 ms is below the floor: the refresh runs at 100 ms, not before.
    pane.clock.advance(ms(99));
    pane.settled();
    assert_eq!(renders(&pane), 2, "nothing below the floor");

    pane.clock.advance(ms(1));
    pane.settled();
    assert_eq!(renders(&pane), 3, "the refresh at the floor");

    // The ask stands: the next refresh is a floor later.
    pane.clock.advance(ms(100));
    pane.settled();
    assert_eq!(renders(&pane), 4);

    // An event's answer re-asks it, as every answer does.
    pane.press("Increment");
    assert_eq!(renders(&pane), 5);
    pane.clock.advance(ms(100));
    pane.settled();
    assert_eq!(renders(&pane), 6);
}

#[test]
fn a_refresh_above_the_ceiling_runs_at_the_ceiling() {
    let pane = fixture();
    pane.press("Answer refresh 25h");
    assert_eq!(renders(&pane), 2);

    // 25 hours is above the ceiling: the refresh runs at 24 hours, not
    // before.
    pane.clock.advance(ms(DAY_MS - 1));
    pane.settled();
    assert_eq!(renders(&pane), 2, "nothing above the ceiling");

    pane.clock.advance(ms(1));
    pane.settled();
    assert_eq!(renders(&pane), 3, "the refresh at the ceiling");
}

#[test]
fn a_view_hides_without_refreshing_and_the_one_that_fell_due_catches_up() {
    let pane = fixture();
    pane.press("Answer refresh 1s");
    assert_eq!(renders(&pane), 2);

    // The window hides, or collapses to its search field: the view is not
    // drawn, so it refreshes nothing.
    let mut caught_up = 2;
    for presence in [WindowPresence::Hidden, WindowPresence::Compact] {
        pane.launcher.set_window_presence(presence);
        pane.clock.advance(ms(10_000));
        pane.settled();
        assert_eq!(renders(&pane), caught_up, "no refresh while {presence:?}");

        // Shown again: the refresh that fell due runs at once — one
        // refresh from the clock's current time, not the ten it missed.
        pane.launcher.set_window_presence(WindowPresence::Shown);
        pane.settled();
        caught_up += 1;
        assert_eq!(
            renders(&pane),
            caught_up,
            "one catch-up refresh after {presence:?}"
        );
    }

    // And the interval continues from the catch-up.
    pane.clock.advance(ms(1000));
    pane.settled();
    assert_eq!(renders(&pane), caught_up + 1);
}

#[test]
fn leaving_the_view_cancels_its_refresh_and_a_new_view_asks_anew() {
    let pane = fixture();
    pane.press("Answer refresh 1s");
    assert_eq!(renders(&pane), 2);

    // Leaving the view: its refresh goes with it.
    assert!(pane.launcher.back());
    assert!(matches!(pane.launcher.view().screen, Screen::Root { .. }));
    pane.clock.advance(ms(10_000));
    pane.settled();
    assert_eq!(block_on(pane.launcher.designed_view_count()), 0);

    // A view opened afresh starts afresh: no refresh of its own until one
    // of its answers asks for one.
    pane.open("", "Designed fixture");
    assert_eq!(renders(&pane), 1);
    pane.clock.advance(ms(10_000));
    pane.settled();
    assert_eq!(renders(&pane), 1, "the new view asks for no refresh");
}

#[test]
fn an_answer_that_asks_for_no_refresh_ends_one_that_did() {
    let pane = fixture();
    pane.press("Answer refresh 1s");
    assert_eq!(renders(&pane), 2);

    // The answer that asks for no drawing again: the refresh stops.
    pane.press("Stop refreshing");
    assert_eq!(renders(&pane), 3);
    pane.clock.advance(ms(10_000));
    pane.settled();
    assert_eq!(renders(&pane), 3, "no refresh after the answer ended it");

    // Asking again starts it again.
    pane.press("Answer refresh 1s");
    pane.clock.advance(ms(1000));
    pane.settled();
    assert_eq!(renders(&pane), 5, "the press's drawing and the refresh");
}
