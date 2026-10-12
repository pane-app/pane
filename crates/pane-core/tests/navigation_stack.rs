//! A designed view's navigation stack (#239): the host owns a stack of
//! view resources per opened command, and the answers of a view's events
//! change it — a push adds a view, a replace swaps the top, a pop (with
//! an optional result) drops the top and tells the view below. The back
//! key pops at once, showing the view below's last tree and then
//! delivering the pop event to it; `window.pop-to-root` and Shift+Esc
//! drop the whole stack; a push that would go beyond
//! [`pane_core::MAX_NAVIGATION_DEPTH`] is the extension's error, the view
//! keeping its last good tree.
//!
//! These checks drive the launcher with the navigation samples in Rust,
//! JavaScript and TypeScript (the same texts, the same buttons, held to
//! the tri-lingual parity the other samples are), and with the `designed`
//! fixture, whose navigation answers are written by hand rather than by
//! an SDK: its pushed views draw the pop events they receive, so the
//! pop event's arrival — and the result it carried — is checked on its
//! own.
//!
//! Components come from `cargo xtask guests`; the JavaScript and
//! TypeScript ones are the prebuilt components in `guests/prebuilt/`.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use futures::executor::block_on;
use pane_core::{
    Launcher, MAX_NAVIGATION_DEPTH, Node, NodeKind, Runtime, Screen, Status, TextContent,
};

use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

use rows::select_title;

/// One language's navigation sample package.
struct Sample {
    package: &'static str,
    language: &'static str,
    title: &'static str,
}

const SAMPLES: [Sample; 3] = [
    Sample {
        package: "sample-nav",
        language: "Rust",
        title: "Navigation sample",
    },
    Sample {
        package: "sample-nav-js",
        language: "JavaScript",
        title: "JavaScript navigation sample",
    },
    Sample {
        package: "sample-nav-ts",
        language: "TypeScript",
        title: "TypeScript navigation sample",
    },
];

/// The assembled package folders `cargo xtask guests` builds.
fn assembled(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(name);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// One test's Pane: its data folder and the launcher.
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
        let folder = assembled(name);
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
    fn tree(&self) -> pane_core::DesignedTree {
        match &self.launcher.view().screen {
            Screen::DesignedView(view) => view.tree.clone(),
            other => panic!("a designed view is open, not {other:?}"),
        }
    }

    /// The first text of the tree on screen.
    fn text(&self) -> String {
        text_of(&self.tree().root).expect("the tree has a text")
    }

    /// The text of the tree on screen that starts with `prefix`, if any.
    fn text_starting(&self, prefix: &str) -> Option<String> {
        starting_with(&self.tree().root, prefix)
    }

    /// The screen's title: the top view's navigation title.
    fn title(&self) -> String {
        self.launcher.view().title
    }

    /// Presses the button with `label` and waits for the tree it answers.
    fn press(&self, label: &str) {
        let (callback, key) = button_of(&self.tree().root, label)
            .unwrap_or_else(|| panic!("the tree has a {label} button"));
        block_on(self.launcher.send_designed_event(callback, key.as_deref()));
    }

    /// Waits for the designed view events the launcher delivers in the
    /// background — the pop event the back key's and a view's own pops
    /// send — to be answered, so what the view below drew of them has
    /// arrived.
    fn settled(&self) {
        assert!(
            self.launcher
                .wait_for_designed_events(Duration::from_secs(30)),
            "the designed view events did not settle"
        );
    }

    /// How many designed views are open in guest instances.
    fn count(&self) -> usize {
        block_on(self.launcher.designed_view_count())
    }

    /// The status line.
    fn status(&self) -> Status {
        self.launcher.view().status.clone()
    }
}

/// The first text node of `node`'s tree, in order.
fn text_of(node: &Node) -> Option<String> {
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
        NodeKind::Text(text) => plain(text),
        // An unknown node draws its fallback, else its children.
        NodeKind::Unknown(_) => node
            .fallback
            .as_deref()
            .and_then(text_of)
            .or_else(|| node.children.iter().find_map(text_of)),
        _ => node.children.iter().find_map(text_of),
    }
}

/// The first text node of `node`'s tree that starts with `prefix`.
fn starting_with(node: &Node, prefix: &str) -> Option<String> {
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
        NodeKind::Text(text)
            if plain(text)
                .as_deref()
                .is_some_and(|content| content.starts_with(prefix)) =>
        {
            plain(text)
        }
        NodeKind::Unknown(_) => node
            .fallback
            .as_deref()
            .and_then(|fallback| starting_with(fallback, prefix))
            .or_else(|| {
                node.children
                    .iter()
                    .find_map(|child| starting_with(child, prefix))
            }),
        _ => node
            .children
            .iter()
            .find_map(|child| starting_with(child, prefix)),
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

#[test]
fn the_samples_push_replace_pop_and_pop_to_root_alike() {
    for Sample {
        package,
        language,
        title,
    } in SAMPLES
    {
        let pane = Pane::new(&[package]);
        pane.open("", title);

        // The root view: the rows, nothing picked yet, the stack one view
        // deep and the screen titled by its navigation title.
        assert_eq!(pane.text(), "Nothing picked yet", "the {language} sample");
        assert_eq!(pane.launcher.designed_stack_depth(), 1);
        assert_eq!(pane.title(), "Navigation sample");

        // A row pushes the detail view; its navigation title is the
        // screen's.
        pane.press("One");
        assert_eq!(pane.text(), "Detail: One", "the {language} sample");
        assert_eq!(pane.launcher.designed_stack_depth(), 2);
        assert_eq!(pane.title(), "One");

        // The detail pushes a deeper view, one more above it.
        pane.press("Deeper");
        assert_eq!(pane.text(), "Deeper: One");
        pane.press("Push another");
        assert_eq!(pane.text(), "Deeper: One");
        assert_eq!(pane.launcher.designed_stack_depth(), 4);

        // The back key pops one view at a time: the deeper view below
        // shows its last tree, then is told the one above it popped.
        assert!(pane.launcher.back());
        assert_eq!(
            pane.launcher.designed_stack_depth(),
            3,
            "one view popped, not the stack"
        );
        assert_eq!(pane.text(), "Deeper: One");
        pane.settled();
        assert_eq!(pane.text(), "Deeper: One");

        // Pop the rest of the deeper views; the rows are below the detail.
        while pane.launcher.designed_stack_depth() > 2 {
            assert!(pane.launcher.back());
        }
        pane.settled();
        assert_eq!(pane.text(), "Detail: One", "the detail, back on top");

        // A replace swaps the detail for a deeper view: the stack's depth
        // stays, its top answered by the new view.
        pane.press("Swap for deeper");
        assert_eq!(pane.text(), "Deeper: One");
        assert_eq!(pane.launcher.designed_stack_depth(), 2);

        // Popping the replacing view tells the rows, whose `onPop` answers
        // a pop that carried no result.
        assert!(pane.launcher.back());
        pane.settled();
        assert_eq!(pane.text(), "Picked: nothing", "the {language} sample");
        assert_eq!(pane.launcher.designed_stack_depth(), 1);

        // A pop from code, with a result: the rows' `onPop` answers it and
        // the view re-renders.
        pane.press("Two");
        assert_eq!(pane.text(), "Detail: Two");
        pane.press("Done");
        pane.settled();
        assert_eq!(pane.text(), "Picked: done:Two", "the {language} sample");
        assert_eq!(pane.launcher.designed_stack_depth(), 1);
        assert_eq!(pane.count(), 1, "the popped view's resource is dropped");

        // `window.pop-to-root`, called by the deeper view: the whole stack
        // is dropped, the command left for root search.
        pane.press("Three");
        pane.press("Deeper");
        pane.press("Pop to root");
        assert!(matches!(pane.launcher.view().screen, Screen::Root { .. }));
        assert_eq!(pane.count(), 0, "every view's resource is dropped");

        // Opening the command again starts a fresh stack.
        pane.open("", title);
        assert_eq!(pane.text(), "Nothing picked yet");
        assert_eq!(pane.launcher.designed_stack_depth(), 1);
    }
}

#[test]
fn a_push_beyond_the_depth_bound_is_the_extensions_error() {
    for Sample {
        package,
        language,
        title,
    } in SAMPLES
    {
        let pane = Pane::new(&[package]);
        pane.open("", title);
        pane.press("One");
        pane.press("Deeper");

        // Views are pushed until the stack holds
        // [`MAX_NAVIGATION_DEPTH`]; the next push is refused as the
        // extension's error, the view keeping its last good tree.
        while pane.launcher.designed_stack_depth() < MAX_NAVIGATION_DEPTH {
            pane.press("Push another");
        }
        assert_eq!(pane.launcher.designed_stack_depth(), MAX_NAVIGATION_DEPTH);
        pane.press("Push another");
        assert!(
            matches!(pane.status(), Status::Error(message) if message.contains("32 views")),
            "{language}: {:?}",
            pane.status()
        );
        assert_eq!(pane.text(), "Deeper: One", "{language}: the tree stays");
        assert_eq!(
            pane.launcher.designed_stack_depth(),
            MAX_NAVIGATION_DEPTH,
            "{language}: the stack stays"
        );

        // The back key unwinds the stack one view at a time, the root's
        // own back leaving the command.
        let mut popped = 0;
        while pane.launcher.back() {
            popped += 1;
        }
        assert_eq!(popped, MAX_NAVIGATION_DEPTH - 1, "{language}");
        pane.settled();
        assert!(matches!(pane.launcher.view().screen, Screen::Root { .. }));
        assert_eq!(pane.count(), 0, "{language}: every view is dropped");
    }
}

/// The `designed` fixture: a view whose trees and answers are written by
/// hand, not by an SDK. Its pushed views draw the pop events they receive.
fn fixture() -> Pane {
    let pane = Pane::new(&["designed"]);
    pane.open("", "Designed fixture");
    pane
}

#[test]
fn the_fixture_pushes_and_the_back_key_pops_one_view_at_a_time() {
    let pane = fixture();
    assert_eq!(pane.text(), "Count: 0");
    assert_eq!(pane.launcher.designed_stack_depth(), 1);

    // A push: the pushed view's own tree, the stack one deeper.
    pane.press("Push a view");
    assert_eq!(pane.text(), "Pushed");
    assert_eq!(pane.launcher.designed_stack_depth(), 2);
    assert_eq!(pane.count(), 2);

    // The back key pops it: the root's last tree shows at once, then the
    // root is told the view above it popped — its pop event, drawn with
    // no result.
    assert!(pane.launcher.back());
    assert_eq!(pane.text(), "Count: 0");
    pane.settled();
    assert_eq!(pane.text(), "Count: 0");
    assert_eq!(
        pane.text_starting("Popped").as_deref(),
        Some("Popped"),
        "the pop event carried no result"
    );
    assert_eq!(pane.launcher.designed_stack_depth(), 1);
    assert_eq!(pane.count(), 1, "the popped view's resource is dropped");

    // The root's own back leaves the command, as leaving its list does.
    assert!(pane.launcher.back());
    assert!(matches!(pane.launcher.view().screen, Screen::Root { .. }));
    assert_eq!(pane.count(), 0);
}

#[test]
fn a_pop_from_code_answers_its_result_to_the_view_below() {
    let pane = fixture();
    pane.press("Push a view");
    assert_eq!(pane.text(), "Pushed");

    // The popped view's answer: the view below is told the result the pop
    // carried, and re-renders.
    pane.press("Pop with a result");
    assert_eq!(pane.launcher.designed_stack_depth(), 1);
    pane.settled();
    assert_eq!(pane.text(), "Count: 0");
    assert_eq!(
        pane.text_starting("Popped").as_deref(),
        Some("Popped: the result"),
        "the pop event carried the result"
    );
    assert_eq!(pane.count(), 1, "the popped view's resource is dropped");

    // Popping the root itself drops the whole stack, leaving the command.
    pane.press("Pop with a result");
    assert!(matches!(pane.launcher.view().screen, Screen::Root { .. }));
    assert_eq!(pane.count(), 0);
}

#[test]
fn a_replace_swaps_the_top_of_the_stack() {
    let pane = fixture();

    // Replacing the root: the stack's depth stays, its top answered by
    // the new view.
    pane.press("Replace this view");
    assert_eq!(pane.text(), "Pushed");
    assert_eq!(pane.launcher.designed_stack_depth(), 1);
    assert_eq!(pane.count(), 1, "the replaced view's resource is dropped");

    // A pushed view can replace itself too, the root staying below it.
    pane.press("Push a view");
    pane.press("Replace this view");
    assert_eq!(pane.text(), "Pushed");
    assert_eq!(pane.launcher.designed_stack_depth(), 2);
    assert_eq!(pane.count(), 2);

    // The back key pops the replacing view, and the root's own back
    // leaves the command.
    assert!(pane.launcher.back());
    pane.settled();
    assert_eq!(pane.text(), "Count: 0");
    assert!(pane.launcher.back());
    assert!(matches!(pane.launcher.view().screen, Screen::Root { .. }));
    assert_eq!(pane.count(), 0);
}

#[test]
fn a_push_that_is_refused_keeps_the_last_good_tree_and_the_view_answers() {
    let pane = fixture();
    pane.press("Push a view");

    // Pushes beyond the depth bound are refused as the extension's error;
    // the top view keeps its last good tree and still answers events.
    while pane.launcher.designed_stack_depth() < MAX_NAVIGATION_DEPTH {
        pane.press("Push a view");
    }
    pane.press("Push a view");
    assert!(
        matches!(pane.status(), Status::Error(message) if message.contains("32 views")),
        "{:?}",
        pane.status()
    );
    assert_eq!(pane.text(), "Pushed");
    assert_eq!(pane.launcher.designed_stack_depth(), MAX_NAVIGATION_DEPTH);

    // The refused push opened a view that is never stacked: it is closed,
    // the count holding the stack alone.
    assert_eq!(pane.count(), MAX_NAVIGATION_DEPTH);

    // The stack unwinds and the command leaves.
    while pane.launcher.back() {}
    pane.settled();
    assert!(matches!(pane.launcher.view().screen, Screen::Root { .. }));
    assert_eq!(pane.count(), 0);
}
