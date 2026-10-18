//! A designed view pushing a re-render the moment its data arrives (#243,
//! the real-push slice of #121): the view's instance runs between Pane's
//! calls — the runtime keeps a guest that still has work going while one
//! of its designed views is open — so work a view's render started lands
//! in the background, and the view asks Pane to draw it again through
//! `pane:extension/view`. The drawing it asks for is sent numbered with
//! the view's events, at most one at a time, and a late answer never
//! replaces a newer tree: these checks hold the pushes against the
//! event answers, drop one that arrives after its view left, and draw a
//! view that pushes while another extension waits on a slow service —
//! drawn without waiting for it. The hand-written `designed` fixture asks
//! for pushes by hand (no SDK writes them), and the loading samples in
//! Rust, JavaScript and TypeScript show the loading helper's arrival
//! asking for a drawing itself: no timer, no clock to advance.
//!
//! Components come from `cargo xtask guests`; the JavaScript and
//! TypeScript ones are the prebuilt components in `guests/prebuilt/`.

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status, WindowPresence};

use tempfile::TempDir;

#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use rows::select_title;

/// One language's designed view sample package, whose `loading` command
/// answers a view that loads something on open.
struct Sample {
    package: &'static str,
    language: &'static str,
    title: &'static str,
}

const SAMPLES: [Sample; 3] = [
    Sample {
        package: "sample-view",
        language: "Rust",
        title: "loading sample",
    },
    Sample {
        package: "sample-view-js",
        language: "JavaScript",
        title: "JavaScript loading sample",
    },
    Sample {
        package: "sample-view-ts",
        language: "TypeScript",
        title: "TypeScript loading sample",
    },
];

/// How long a drawing may take: compiling the guest once is included; a
/// slow, busy machine is not.
const PROMPTLY: Duration = Duration::from_secs(8);

/// How long the operations fixture's `wait` makes a call wait: ten
/// seconds, far beyond anything these checks wait for.
const SLOW: Duration = Duration::from_secs(10);

/// One test's Pane: its data folder and the launcher it drives.
struct Pane {
    data: TempDir,
    launcher: Launcher,
}

impl Pane {
    /// A launcher with the assembled packages it is given installed, from
    /// `target/guests/packages`.
    fn new(packages: &[&str]) -> Pane {
        let data = tempfile::tempdir().unwrap();
        let launcher =
            Launcher::with_packages(Runtime::start(), Vec::new(), data.path().join("extensions"));
        let pane = Pane { data, launcher };
        for package in packages {
            pane.install_assembled(package);
        }
        pane
    }

    /// Installs the assembled package `name`.
    fn install_assembled(&self, name: &str) {
        let folder = packages().join(name);
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
    fn tree(&self) -> pane_core::Node {
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

    /// Waits for the view's refreshes and pushes to have been looked at,
    /// and every drawing they sent to have answered.
    fn settled(&self) {
        assert!(
            self.launcher.wait_for_view_refresh(PROMPTLY),
            "the view's refresh settled"
        );
    }
}

/// Where the assembled packages `cargo xtask guests` makes are.
fn packages() -> PathBuf {
    guests::guests().join("packages")
}

/// Every text `node`'s tree draws, appended to `into`, in order.
fn collect_texts(node: &pane_core::Node, into: &mut Vec<String>) {
    let plain = |text: &pane_core::Text| match &text.content {
        pane_core::TextContent::Plain(content) => Some(content.clone()),
        pane_core::TextContent::Spans(spans) => (!spans.is_empty()).then(|| {
            spans
                .iter()
                .map(|span| span.text.clone())
                .collect::<String>()
        }),
    };
    match &node.kind {
        pane_core::NodeKind::Text(text) => {
            if let Some(content) = plain(text) {
                into.push(content);
            }
        }
        // An unknown node draws its fallback, else its children.
        pane_core::NodeKind::Unknown(_) => {
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
fn button_of(node: &pane_core::Node, label: &str) -> Option<(u32, Option<String>)> {
    match &node.kind {
        pane_core::NodeKind::Button(button) if button.label == label => {
            button.on_press.map(|id| (id, node.key.clone()))
        }
        pane_core::NodeKind::Unknown(_) => node
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
/// says: every render, a pushed drawing's included.
fn renders(pane: &Pane) -> u32 {
    pane.texts()
        .iter()
        .find_map(|text| text.strip_prefix("Renders: "))
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("the tree says how often it was drawn"))
}

/// A Pane with the `designed` fixture's view open.
fn fixture() -> Pane {
    let pane = Pane::new(&["designed"]);
    pane.open("", "Designed fixture");
    pane
}

#[test]
fn the_loading_sample_shows_its_loading_state_then_its_data_without_a_timer() {
    for Sample {
        package,
        language,
        title,
    } in SAMPLES
    {
        let pane = Pane::new(&[package]);
        pane.open("", title);
        // The loading state is drawn at once; the load is started by the
        // view's opening render.
        assert_eq!(
            pane.texts(),
            ["Loading…"],
            "the {language} sample's loading state"
        );

        // What the load answered is drawn the moment it arrives: the
        // arrival asks for the drawing itself. No clock is advanced and
        // no key pressed for it — the pane's clock is the system's, which
        // this test never moves. The load takes its own time (a moment,
        // as work from a service would), so the drawing is waited for as
        // every designed answer is, not read at once.
        let deadline = Instant::now() + PROMPTLY;
        while pane.texts() != ["Loaded", "Pane drew this the moment it arrived"] {
            assert!(
                Instant::now() < deadline,
                "the {language} sample's data never arrived: {:?}",
                pane.texts()
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        // And the view asks for nothing further: the arrival is the whole
        // story.
        pane.settled();
        assert_eq!(
            pane.texts(),
            ["Loaded", "Pane drew this the moment it arrived"],
            "the {language} sample asks for no drawing again"
        );
    }
}

#[test]
fn a_push_the_view_asked_for_draws_it_again() {
    let pane = fixture();
    assert_eq!(renders(&pane), 1);

    // The press's own drawing; the ask fires in it, by hand. The drawing
    // the ask asked for can already have landed by the time this reads:
    // it serves the ask the press's own render made, so what is asserted
    // is that the press drew at all.
    pane.press("Push a render");
    assert!(renders(&pane) >= 2, "the press's own drawing");

    // The drawing the ask asked for follows — no event, no clock — and
    // one drawing serves the ask.
    pane.settled();
    assert_eq!(renders(&pane), 3, "the pushed drawing");
    pane.settled();
    assert_eq!(renders(&pane), 3, "one drawing for the ask");

    // A second ask draws again: the push is the view's to repeat.
    pane.press("Push a render");
    pane.settled();
    assert_eq!(renders(&pane), 5, "the press's drawing and the pushed one");
}

#[test]
fn a_push_that_arrives_after_the_view_left_is_dropped() {
    let pane = fixture();
    // The press's drawing answers promptly; the drawing it asks for is
    // held back a moment, so the view can be left before it answers.
    pane.press("Push a render, answering slowly");
    assert_eq!(renders(&pane), 2, "the press's own drawing");
    thread::sleep(Duration::from_millis(100));

    // Leave the view: the drawing it asked for is in flight, and its
    // answer arrives after the view went.
    assert!(pane.launcher.back());
    assert!(matches!(pane.launcher.view().screen, Screen::Root { .. }));
    pane.settled();

    // The answer was dropped: the launcher stays at root search, with no
    // error from a drawing no view asked for, and the view is gone.
    assert!(matches!(pane.launcher.view().screen, Screen::Root { .. }));
    assert!(
        !matches!(pane.launcher.view().status, Status::Error(_)),
        "the late drawing was dropped: {:?}",
        pane.launcher.view().status
    );
    assert_eq!(block_on(pane.launcher.designed_view_count()), 0);
}

#[test]
fn a_push_does_not_overwrite_a_newer_event_answer() {
    let pane = fixture();
    assert_eq!(renders(&pane), 1);

    // An ask and an event, one after the other: both drawings are sent,
    // numbered with the view's events, and whichever answers last the
    // screen shows the newest tree — the count the event changed is not
    // lost behind the pushed drawing, and the pushed drawing does not
    // bring an older one back.
    pane.press("Push a render");
    pane.press("Increment");
    pane.settled();
    let texts = pane.texts();
    assert!(
        texts.contains(&"Count: 1".to_owned()),
        "the increment is not lost: {texts:?}"
    );
    assert!(
        renders(&pane) >= 4,
        "the press's drawing, the increment's and the pushed one: {}",
        renders(&pane)
    );

    // The view carries on asking and answering as before.
    pane.press("Increment");
    assert!(
        pane.texts().contains(&"Count: 2".to_owned()),
        "the view answers events after pushes: {:?}",
        pane.texts()
    );
}

#[test]
fn a_view_that_pushes_while_hidden_is_drawn_when_shown_again() {
    let pane = fixture();
    pane.press("Push a render");
    assert_eq!(renders(&pane), 2);

    // The window hides: the view is not drawn, so its push waits.
    pane.launcher.set_window_presence(WindowPresence::Hidden);
    pane.press("Push a render");
    pane.settled();
    assert_eq!(renders(&pane), 3, "no drawing while hidden");

    // Shown again: the push that waited is drawn at once.
    pane.launcher.set_window_presence(WindowPresence::Shown);
    pane.settled();
    assert_eq!(renders(&pane), 4, "the pushed drawing on showing");
}

/// One Pane with the `designed` fixture and the operations fixtures `a`
/// (its command) and `b` (publishing `wait`) installed, and where their
/// source folders are.
struct SlowPane {
    pane: Pane,
    sources: TempDir,
}

impl SlowPane {
    /// The Pane, with the pushing view not yet open. The operations
    /// fixtures and `a`'s saved sources are written before its launcher
    /// opens the data folder, as the operations tests do
    /// (`tests/operations.rs`): `a` with the command, `b` publishing
    /// `wait`, which holds its caller for ten seconds.
    fn new() -> SlowPane {
        let data = tempfile::tempdir().unwrap();
        let sources = tempfile::tempdir().unwrap();
        let extensions = data.path().join("extensions");
        let waiting = format!(
            r#"{PUBLISHED}, {{ "id": "wait", "version": 1, "component": "fixture.wasm" }}"#
        );
        write_fixture(&sources.path().join("a"), COMMAND, PUBLISHED);
        write_fixture(&sources.path().join("b"), "", &waiting);
        let mut saved = serde_json::Map::new();
        for name in ["a", "b"] {
            saved.insert(
                name.into(),
                PackageIdentity::local(&sources.path().join(name))
                    .unwrap()
                    .key()
                    .into(),
            );
        }
        let a = PackageIdentity::local(&sources.path().join("a"))
            .unwrap()
            .key();
        let settings = serde_json::json!({
            "version": 1,
            "packages": {
                a: { "sources": serde_json::Value::Object(saved).to_string() }
            }
        });
        fs::create_dir_all(&extensions).unwrap();
        fs::write(extensions.join("settings.json"), settings.to_string()).unwrap();
        let launcher =
            Launcher::with_packages(Runtime::start(), Vec::new(), data.path().join("extensions"));
        let pane = Pane { data, launcher };
        pane.install_assembled("designed");
        for name in ["a", "b"] {
            block_on(pane.launcher.install_package(&sources.path().join(name)));
        }
        SlowPane { pane, sources }
    }

    /// The identity of the installed package `name`, for stopping its
    /// slow call.
    fn identity(&self, name: &str) -> PackageIdentity {
        PackageIdentity::local(&self.sources.path().join(name)).unwrap()
    }

    /// What `b`'s `wait` saved: "started", "finished" or nothing.
    fn waiting(&self) -> Option<String> {
        let text = fs::read_to_string(
            self.pane
                .data
                .path()
                .join("extensions")
                .join("settings.json"),
        )
        .unwrap_or_default();
        ["finished", "started"]
            .into_iter()
            .find(|progress| text.contains(&format!("\"waiting\": \"{progress}\"")))
            .map(str::to_owned)
    }

    /// Runs `a`'s "Call b's wait" on another thread, returning once `b`
    /// waits: the call holds `a`'s extension for the ten seconds.
    fn start_waiting(&self) -> (thread::JoinHandle<()>, Instant) {
        let launcher = &self.pane.launcher;
        while !matches!(launcher.view().screen, Screen::Root { .. }) {
            launcher.back();
        }
        block_on(launcher.set_query(""));
        select_title(launcher, "Operations fixture");
        block_on(launcher.activate_selected());
        select_title(launcher, "Call b's wait");
        let calling = launcher.activate_selected();
        let started = Instant::now();
        let calling = thread::spawn(move || {
            block_on(calling);
        });
        while self.waiting().as_deref() != Some("started") {
            assert!(
                started.elapsed() < SLOW,
                "b did not start waiting: {:?}",
                self.waiting()
            );
            thread::sleep(Duration::from_millis(10));
        }
        (calling, started)
    }
}

/// Writes the operations fixture's component as the package at `folder`,
/// with the commands and operations its manifest names.
fn write_fixture(folder: &std::path::Path, commands: &str, operations: &str) {
    fs::create_dir_all(folder).unwrap();
    fs::copy(
        guests::guest_file("operations_fixture.wasm"),
        folder.join("fixture.wasm"),
    )
    .unwrap();
    let manifest = format!(
        r#"{{
            "manifestVersion": 1,
            "title": "Package {}",
            "apiVersion": "0.1",
            "commands": [{commands}],
            "operations": [{operations}]
        }}"#,
        folder.file_name().unwrap().to_string_lossy()
    );
    fs::write(folder.join("pane.json"), manifest).unwrap();
}

/// The operations the fixtures publish, as the operations tests write them.
const PUBLISHED: &str = r#"
    { "id": "echo", "version": 1, "component": "fixture.wasm" },
    { "id": "forward", "version": 1, "component": "fixture.wasm" },
    { "id": "crash", "version": 1, "component": "fixture.wasm" },
    { "id": "not-json", "version": 1, "component": "fixture.wasm" },
    { "id": "remember", "version": 1, "component": "fixture.wasm" }
"#;

/// The fixture's command, as `a` lists it.
const COMMAND: &str =
    r#"{ "id": "fixture", "title": "Operations fixture", "component": "fixture.wasm" }"#;

#[test]
fn a_view_that_pushes_while_another_extension_waits_on_a_slow_service_is_drawn() {
    let slow = SlowPane::new();
    // Another extension waits on the slow service: `a`'s item is calling
    // `b`'s `wait`, which holds it for ten seconds.
    let (calling, started) = slow.start_waiting();

    // The designed view opens and pushes meanwhile: its drawings — the
    // open, the press's and the pushed one — are served and shown without
    // waiting for the slow call, as any call is while another waits.
    let pane = &slow.pane;
    pane.open("", "Designed fixture");
    assert_eq!(renders(pane), 1);
    pane.press("Push a render");
    assert_eq!(renders(pane), 2);
    pane.settled();
    assert_eq!(
        renders(pane),
        3,
        "the pushed drawing, drawn without waiting for the slow call"
    );
    assert_eq!(
        slow.waiting().as_deref(),
        Some("started"),
        "the slow call is still waiting"
    );
    assert!(
        started.elapsed() < SLOW,
        "the view was drawn well within the slow call's wait: {:?}",
        started.elapsed()
    );

    // Stopping the slow service ends the call its caller waits on, so
    // this test waits for no slow guest.
    let disabling = pane.launcher.set_enabled(&slow.identity("b"), false);
    calling.join().unwrap();
    block_on(disabling);
    assert_eq!(slow.waiting().as_deref(), Some("started"));
}
