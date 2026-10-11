//! A designed view reaches Pane through ADR 0036's envelope: `open-view`
//! opens it, `render` answers its tree, and `handle-event` takes the
//! callback ids the tree named. These checks drive the launcher with the
//! designed view samples in Rust, JavaScript and TypeScript (the counter:
//! the same tree, the same presses, the same state), held to the
//! tri-lingual parity the other samples are, and with the `designed`
//! fixture, whose JSON is written by hand rather than by an SDK, so that
//! Pane's reading of it is checked against what an SDK never writes: an
//! unknown node with and without a fallback, a newer minor and another
//! major version, a tree over the node limit, an error, and text that is
//! not JSON.
//!
//! Components come from `cargo xtask guests`; the JavaScript and
//! TypeScript ones are the prebuilt components in `guests/prebuilt/`.

use std::fs;
use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::{DesignedTree, Launcher, Node, NodeKind, Runtime, Screen, Status, TextContent};

use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

use rows::select_title;

/// One language's designed view sample package.
struct Sample {
    package: &'static str,
    language: &'static str,
    title: &'static str,
    /// The title of its components gallery command.
    components: &'static str,
}

const SAMPLES: [Sample; 3] = [
    Sample {
        package: "sample-view",
        language: "Rust",
        title: "Designed view sample",
        components: "Components sample",
    },
    Sample {
        package: "sample-view-js",
        language: "JavaScript",
        title: "JavaScript designed view sample",
        components: "JavaScript components sample",
    },
    Sample {
        package: "sample-view-ts",
        language: "TypeScript",
        title: "TypeScript designed view sample",
        components: "TypeScript components sample",
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

    /// Installs a one-command package whose component is `component` (a
    /// `.wasm` under `target/guests`) and whose manifest is `manifest`,
    /// for the shapes only a hand-written manifest gives a command.
    fn install_manifest(&self, component: &str, manifest: &str) {
        let guest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests")
            .join(format!("{component}.wasm"));
        assert!(
            guest.exists(),
            "{} is missing; run `cargo xtask guests`",
            guest.display()
        );
        let source = self.data.path().join(component);
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("pane.json"), manifest).unwrap();
        fs::copy(guest, source.join(format!("{component}.wasm"))).unwrap();
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
    fn tree(&self) -> DesignedTree {
        match &self.launcher.view().screen {
            Screen::DesignedView(view) => view.tree.clone(),
            other => panic!("a designed view is open, not {other:?}"),
        }
    }

    /// The text of the tree's first text node.
    fn text(&self) -> String {
        text_of(&self.tree().root).expect("the tree has a text")
    }

    /// Presses the button with `label` and waits for the tree it answers.
    fn press(&self, label: &str) {
        let (callback, key) = button_of(&self.tree().root, label)
            .unwrap_or_else(|| panic!("the tree has a {label} button"));
        block_on(self.launcher.send_designed_event(callback, key.as_deref()));
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
fn opening_the_sample_shows_its_first_tree_and_presses_change_it() {
    for Sample {
        package,
        language,
        title,
        ..
    } in SAMPLES
    {
        let pane = Pane::new(&[package]);
        pane.open("", title);
        assert_eq!(
            pane.text(),
            "Count: 0",
            "the {language} sample's first tree"
        );

        pane.press("Increment");
        assert_eq!(
            pane.text(),
            "Count: 1",
            "the {language} sample after one press"
        );
        pane.press("Increment");
        pane.press("Increment");
        assert_eq!(pane.text(), "Count: 3");
        pane.press("Decrement");
        assert_eq!(pane.text(), "Count: 2");
        // A destructive reset, and the count's floor.
        pane.press("Reset");
        assert_eq!(pane.text(), "Count: 0");
        pane.press("Decrement");
        assert_eq!(pane.text(), "Count: 0");

        // Escape leaves the command: the view is closed and root search
        // is back.
        assert!(pane.launcher.back());
        assert!(matches!(pane.launcher.view().screen, Screen::Root { .. }));
        assert_eq!(block_on(pane.launcher.designed_view_count()), 0);

        // Opening it again starts a fresh view.
        pane.open("", title);
        assert_eq!(pane.text(), "Count: 0");
    }
}

#[test]
fn a_press_of_a_callback_the_tree_does_not_name_is_dropped() {
    let pane = Pane::new(&[SAMPLES[0].package]);
    pane.open("", SAMPLES[0].title);
    assert_eq!(pane.text(), "Count: 0");

    // A callback id no button named: the SDK drops it, as it drops an
    // event older than its last two renders, and the tree stays.
    block_on(pane.launcher.send_designed_event(99, None));
    assert_eq!(pane.text(), "Count: 0");
    assert_eq!(pane.status(), Status::Idle);

    // The view still answers events after that.
    pane.press("Increment");
    assert_eq!(pane.text(), "Count: 1");
    assert_eq!(pane.status(), Status::Idle);
}

/// The `designed` fixture: a view whose trees and answers are written by
/// hand, not by an SDK.
fn fixture() -> Pane {
    let pane = Pane::new(&["designed"]);
    pane.open("", "Designed fixture");
    pane
}

#[test]
fn errors_and_unreadable_trees_keep_the_last_good_tree() {
    let pane = fixture();
    assert_eq!(pane.text(), "Count: 0");
    pane.press("Increment");
    pane.press("Increment");
    assert_eq!(pane.text(), "Count: 2");

    // An error the view answers with: shown, and the tree stays.
    pane.press("Answer an error");
    assert!(
        matches!(pane.status(), Status::Error(message) if message.contains("on purpose")),
        "{:?}",
        pane.status()
    );
    assert_eq!(pane.text(), "Count: 2");

    // An event the view refuses: the tree stays too.
    pane.press("Answer an unknown callback");
    assert_eq!(pane.text(), "Count: 2");

    // A tree that is not JSON: the command's failure, the tree stays.
    pane.press("Answer an unreadable tree");
    assert!(
        matches!(pane.status(), Status::Error(ref message) if message.contains("could not read")),
        "{:?}",
        pane.status()
    );
    assert_eq!(pane.text(), "Count: 2");

    // The view still answers events after all that.
    pane.press("Increment");
    assert_eq!(pane.text(), "Count: 3");
    assert_eq!(pane.status(), Status::Idle);
}

#[test]
fn an_over_limit_tree_is_the_extensions_error_and_keeps_the_last_good_tree() {
    let pane = fixture();
    pane.press("Increment");
    assert_eq!(pane.text(), "Count: 1");

    pane.press("Answer an over-limit tree");
    assert!(
        matches!(pane.status(), Status::Error(message) if message.contains("10001 nodes")),
        "{:?}",
        pane.status()
    );
    assert_eq!(pane.text(), "Count: 1");
}

#[test]
fn unknown_nodes_draw_their_fallback_or_their_children() {
    // An unknown node with a fallback: the fallback's text is drawn.
    let pane = fixture();
    pane.press("Draw an unknown node with a fallback");
    assert_eq!(pane.text(), "The fallback");

    // An unknown node without one: its children are.
    let pane = fixture();
    pane.press("Draw an unknown node without a fallback");
    assert_eq!(pane.text(), "Inside");

    // Both degraded to the counter again.
    let pane = fixture();
    pane.press("Increment");
    assert_eq!(pane.text(), "Count: 1");
}

#[test]
fn a_newer_minor_is_drawn_and_another_major_is_refused() {
    // A document of a newer minor version: drawn for what Pane
    // understands.
    let pane = fixture();
    pane.press("Draw a newer minor tree");
    assert_eq!(pane.text(), "Newer");

    // A document of another major version: the extension's error, naming
    // both versions; the tree stays.
    let pane = fixture();
    pane.press("Draw another major's tree");
    assert!(
        matches!(
            pane.status(),
            Status::Error(message)
                if message.contains("UI component set 2.0")
                    && message.contains("this Pane renders 1.3")
        ),
        "{:?}",
        pane.status()
    );
    assert_eq!(pane.text(), "Count: 0");
}

#[test]
fn the_press_carries_the_node_it_was_raised_on() {
    // The fixture's "Increment" counts only when the event's key names it,
    // so counting proves Pane sent the node's key with the event.
    let pane = fixture();
    let (callback, _) = button_of(&pane.tree().root, "Increment").unwrap();

    // Without the key, the press is delivered but changes nothing.
    block_on(pane.launcher.send_designed_event(callback, None));
    assert_eq!(pane.text(), "Count: 0");
    // With the node's key, it counts.
    block_on(
        pane.launcher
            .send_designed_event(callback, Some("increment")),
    );
    assert_eq!(pane.text(), "Count: 1");
}

#[test]
fn a_view_the_guest_refuses_to_open_is_an_error() {
    // The trees fixture answers `open-view` with an error; a manifest
    // declaring its command designed leaves the user at root search with
    // the error, as a list command whose tree could not be read does.
    let pane = Pane::new(&[]);
    pane.install_manifest(
        "tree_fixture",
        r#"{
  "manifestVersion": 1,
  "title": "Refusing",
  "version": "1.0.0",
  "apiVersion": "0.1",
  "commands": [
    { "id": "refusing", "title": "Refusing", "component": "tree_fixture.wasm", "mode": "designed" }
  ]
}"#,
    );
    pane.open("", "Refusing");
    assert!(matches!(pane.launcher.view().screen, Screen::Root { .. }));
    assert!(
        matches!(
            pane.status(),
            Status::Error(message) if message.contains("unknown designed view")
        ),
        "{:?}",
        pane.status()
    );
}

/// The kinds of node a tree holds, in order.
fn kinds_of(node: &Node) -> Vec<String> {
    let kind = |node: &Node| -> String {
        match &node.kind {
            NodeKind::Column(_) => "column".to_owned(),
            NodeKind::Row(_) => "row".to_owned(),
            NodeKind::Stack(_) => "stack".to_owned(),
            NodeKind::Scroll { .. } => "scroll".to_owned(),
            NodeKind::Spacer => "spacer".to_owned(),
            NodeKind::Divider { .. } => "divider".to_owned(),
            NodeKind::Text(_) => "text".to_owned(),
            NodeKind::Button(_) => "button".to_owned(),
            NodeKind::Link(_) => "link".to_owned(),
            NodeKind::Icon(_) => "icon".to_owned(),
            NodeKind::IconTile(_) => "icon-tile".to_owned(),
            NodeKind::Image(_) => "image".to_owned(),
            NodeKind::RichRow(_) => "rich-row".to_owned(),
            NodeKind::Keycap(_) => "keycap".to_owned(),
            NodeKind::KeySequence(_) => "key-sequence".to_owned(),
            NodeKind::Tag(_) => "tag".to_owned(),
            NodeKind::Badge(_) => "badge".to_owned(),
            NodeKind::Form(_) => "form".to_owned(),
            NodeKind::DatePicker(_) => "date-picker".to_owned(),
            NodeKind::DateTimePicker(_) => "date-time-picker".to_owned(),
            NodeKind::TagPicker(_) => "tag-picker".to_owned(),
            NodeKind::FilePicker(_) => "file-picker".to_owned(),
            NodeKind::FolderPicker(_) => "folder-picker".to_owned(),
            NodeKind::Toggle(_) => "toggle".to_owned(),
            NodeKind::Checkbox(_) => "checkbox".to_owned(),
            NodeKind::Segmented(_) => "segmented".to_owned(),
            NodeKind::Slider(_) => "slider".to_owned(),
            NodeKind::Progress(_) => "progress".to_owned(),
            NodeKind::Loading(_) => "loading".to_owned(),
            NodeKind::Markdown(_) => "markdown".to_owned(),
            NodeKind::Card(_) => "card".to_owned(),
            NodeKind::SectionHeader(_) => "section-header".to_owned(),
            NodeKind::MetadataList(_) => "metadata-list".to_owned(),
            NodeKind::EmptyState(_) => "empty-state".to_owned(),
            NodeKind::TextInput(_) => "text-input".to_owned(),
            NodeKind::PasswordInput(_) => "password-input".to_owned(),
            NodeKind::TextArea(_) => "text-area".to_owned(),
            NodeKind::Select(_) => "select".to_owned(),
            NodeKind::List(_) => "list".to_owned(),
            NodeKind::Grid(_) => "grid".to_owned(),
            NodeKind::ListSection(_) => "list-section".to_owned(),
            NodeKind::ListItem(_) => "list-item".to_owned(),
            NodeKind::GridItem(_) => "grid-item".to_owned(),
            NodeKind::ListDropdown(_) => "list-dropdown".to_owned(),
            NodeKind::Detail(_) => "detail".to_owned(),
            NodeKind::Canvas(_) => "canvas".to_owned(),
            NodeKind::Unknown(kind) => kind.to_owned(),
        }
    };
    let mut kinds = vec![kind(node)];
    for child in &node.children {
        kinds.extend(kinds_of(child));
    }
    kinds
}

/// The first toggle of `node`'s tree: whether it is on, and its callback.
fn toggle_of(node: &Node) -> Option<(bool, u32)> {
    match &node.kind {
        NodeKind::Toggle(toggle) => toggle.on_change.map(|callback| (toggle.on, callback)),
        NodeKind::Unknown(_) => node
            .fallback
            .as_deref()
            .and_then(toggle_of)
            .or_else(|| node.children.iter().find_map(toggle_of)),
        _ => node.children.iter().find_map(toggle_of),
    }
}

#[test]
fn the_component_set_draws_and_a_change_flips_its_toggle() {
    let pane = fixture();
    pane.press("Draw the component set");
    // Every kind of node the fixture drew: the layout primitives, the
    // shared components and the inputs.
    let kinds = kinds_of(&pane.tree().root);
    for kind in [
        "scroll",
        "stack",
        "spacer",
        "divider",
        "card",
        "rich-row",
        "keycap",
        "key-sequence",
        "tag",
        "badge",
        "toggle",
        "checkbox",
        "segmented",
        "slider",
        "progress",
        "loading",
        "markdown",
        "metadata-list",
        "empty-state",
        "text-input",
        "password-input",
        "text-area",
        "select",
    ] {
        assert!(kinds.contains(&kind.to_owned()), "the tree draws a {kind}");
    }
    // The toggle starts off, with a callback to change it.
    let (on, callback) = toggle_of(&pane.tree().root).expect("a toggle");
    assert!(!on);

    // A change of the toggle: the payload names the value, and the next
    // tree shows it flipped — Pane sent what the user chose.
    block_on(pane.launcher.send_designed_change(
        callback,
        Some("toggle"),
        r#"{"value":true}"#.to_owned(),
    ));
    let (on, _) = toggle_of(&pane.tree().root).expect("the toggle still drawn");
    assert!(on, "the change flipped the toggle");

    // And back.
    block_on(pane.launcher.send_designed_change(
        callback,
        Some("toggle"),
        r#"{"value":false}"#.to_owned(),
    ));
    let (on, _) = toggle_of(&pane.tree().root).expect("the toggle still drawn");
    assert!(!on);
    assert_eq!(pane.status(), Status::Idle);
}

#[test]
fn the_components_gallery_of_the_three_samples_matches() {
    // The gallery command answers the same tree in Rust, JavaScript and
    // TypeScript: the same nodes, in order, with the same toggle state —
    // the parity the counter is held to.
    let mut galleries = Vec::new();
    for Sample {
        package,
        language,
        components,
        ..
    } in SAMPLES
    {
        let pane = Pane::new(&[package]);
        pane.open("", components);
        let tree = pane.tree();
        let (on, _) = toggle_of(&tree.root).expect("a toggle");
        galleries.push((language, kinds_of(&tree.root), on));
    }
    let (_, kinds, on) = &galleries[0];
    assert!(on == &false, "the Rust gallery's toggle starts off");
    assert!(
        kinds.contains(&"toggle".to_owned())
            && kinds.contains(&"markdown".to_owned())
            && kinds.contains(&"select".to_owned()),
        "the Rust gallery draws the components: {kinds:?}"
    );
    for (language, other, other_on) in &galleries[1..] {
        assert_eq!(kinds.len(), other.len(), "the {language} gallery");
        for (at, (kind, other_kind)) in kinds.iter().zip(other.iter()).enumerate() {
            assert_eq!(kind, other_kind, "node {at} of the {language} gallery");
        }
        assert_eq!(on, other_on, "the {language} gallery's toggle");
    }
}
