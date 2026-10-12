//! The keyed state of a designed view, at the Launcher seam (#238): the
//! events its partially controlled inputs send (`input` as the user
//! types, `change` on a commit) and their payloads; the stale-event rule
//! (an event raised on a node the user could see is delivered while the
//! node with that key still names a handler, and dropped otherwise, or
//! when the view has rendered twice since); and the key problems a
//! developed package's log reports — keys shared by siblings, stateful
//! nodes without one.
//!
//! The `designed` fixture answers these by hand (its fields' tree draws
//! the events it receives), so Pane's side of the contract is checked
//! against what an SDK never writes. Components come from
//! `cargo xtask guests`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::executor::block_on;
use pane_core::develop::{Build, BuildJob, BuildOutcome, Builder};
use pane_core::{DesignedHandler, Launcher, Runtime, Screen, Status};

use tempfile::TempDir;

#[path = "support/guests.rs"]
mod guests;

#[path = "support/rows.rs"]
mod rows;

use guests::guest;
use rows::select_title;

/// Builds a folder by staging the guest its `source.txt` names, as the
/// development tests' stand-in does: the developed package is the
/// designed fixture.
struct CopyBuilder;

struct CopyBuild(PathBuf);

impl Builder for CopyBuilder {
    fn build_for(&self, folder: &Path) -> Result<Arc<dyn Build>, String> {
        Ok(Arc::new(CopyBuild(folder.to_path_buf())))
    }
}

impl Build for CopyBuild {
    fn command(&self) -> String {
        "copy build".into()
    }

    fn ignores(&self, path: &Path) -> bool {
        path == Path::new("command.wasm")
    }

    fn run(&self, job: &BuildJob) -> BuildOutcome {
        let source = fs::read_to_string(self.0.join("source.txt")).unwrap();
        fs::copy(guest(source.trim()), job.staging().join("command.wasm")).unwrap();
        BuildOutcome::Built
    }
}

/// One test's Pane: its data folder, the launcher, and the development
/// the key problems' reports need.
struct Pane {
    _sources: TempDir,
    _data: TempDir,
    sources: PathBuf,
    launcher: Launcher,
}

impl Pane {
    /// A launcher that can develop the packages it installs.
    fn new() -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let runtime = Runtime::start().unwrap();
        let (changes, _) = pane_core::changes::channel();
        let launcher =
            Launcher::with_packages(Ok(runtime), Vec::new(), data.path().join("extensions"))
                .with_development(Arc::new(CopyBuilder), changes);
        Pane {
            sources: sources.path().to_path_buf(),
            _sources: sources,
            _data: data,
            launcher,
        }
    }

    /// Installs the designed fixture's package, titled `title`, and
    /// returns its identity.
    fn install(&self, title: &str) -> pane_core::PackageIdentity {
        let folder = self.sources.join("Fixture");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("pane.json"),
            format!(
                r#"{{
  "manifestVersion": 1,
  "title": "{title}",
  "apiVersion": "0.1",
  "commands": [
    {{ "id": "open", "title": "{title}", "component": "command.wasm", "mode": "designed" }}
  ]
}}"#
            ),
        )
        .unwrap();
        fs::write(folder.join("source.txt"), "designed").unwrap();
        fs::copy(guest("designed"), folder.join("command.wasm")).unwrap();
        block_on(self.launcher.install_package(&folder));
        assert!(
            matches!(self.launcher.view().status, Status::Result(_)),
            "{:?}",
            self.launcher.view().status
        );
        pane_core::PackageIdentity::local(&folder).unwrap()
    }

    /// Installs the fixture and develops it, so its key problems and
    /// dropped events are reported in its log.
    fn developing(&self, title: &str) -> pane_core::PackageIdentity {
        let identity = self.install(title);
        block_on(self.launcher.start_developing(&identity));
        assert!(
            self.launcher.development(&identity).is_some(),
            "{:?}",
            self.launcher.view().status
        );
        identity
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

    /// The text of the tree's first text node.
    fn text(&self) -> String {
        text_of(&self.tree().root).expect("the tree has a text")
    }

    /// Every text node's text, in order, joined by lines.
    fn texts(&self) -> String {
        let mut held = Vec::new();
        texts_of(&self.tree().root, &mut held);
        held.join("\n")
    }

    /// The render number of the tree on screen, as the events Pane sends
    /// carry it back.
    fn render(&self) -> u64 {
        match &self.launcher.view().screen {
            Screen::DesignedView(view) => view.render,
            other => panic!("a designed view is open, not {other:?}"),
        }
    }

    /// Presses the button with `label` and waits for the tree it answers.
    fn press(&self, label: &str) {
        let (callback, key) = button_of(&self.tree().root, label)
            .unwrap_or_else(|| panic!("the tree has a {label} button"));
        block_on(self.launcher.send_designed_event(callback, key.as_deref()));
    }

    /// Sends one event raised on the tree of the render `seen` (`None`
    /// for the tree on screen now), as the window's controls do.
    fn send(
        &self,
        handler: DesignedHandler,
        callback: u32,
        key: Option<&str>,
        seen: Option<u64>,
        payload: &str,
    ) {
        block_on(self.launcher.send_designed_seen(
            handler,
            callback,
            key,
            seen,
            payload.to_owned(),
        ));
    }

    /// The status line.
    fn status(&self) -> Status {
        self.launcher.view().status.clone()
    }
}

/// The first text node of `node`'s tree, in order.
fn text_of(node: &pane_core::Node) -> Option<String> {
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
        pane_core::NodeKind::Text(text) => plain(text),
        pane_core::NodeKind::Unknown(_) => node
            .fallback
            .as_deref()
            .and_then(text_of)
            .or_else(|| node.children.iter().find_map(text_of)),
        _ => node.children.iter().find_map(text_of),
    }
}

/// Every text node's text of `node`'s tree, in order.
fn texts_of(node: &pane_core::Node, held: &mut Vec<String>) {
    if let Some(text) = text_of(node) {
        held.push(text);
    }
    for child in &node.children {
        texts_of(child, held);
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

/// The text input with key `key` in `node`'s tree: its `onInput` and
/// `onChange` callback ids.
fn input_of(node: &pane_core::Node, key: &str) -> Option<(Option<u32>, Option<u32>)> {
    match &node.kind {
        pane_core::NodeKind::TextInput(input) if node.key.as_deref() == Some(key) => {
            Some((input.on_input, input.on_change))
        }
        _ => node
            .children
            .iter()
            .find_map(|child| input_of(child, key))
            .or_else(|| {
                node.fallback
                    .as_deref()
                    .and_then(|child| input_of(child, key))
            }),
    }
}

#[test]
fn the_input_and_change_events_carry_their_payloads() {
    let pane = Pane::new();
    let identity = pane.install("Designed fixture");
    pane.open("", "Designed fixture");
    pane.press("Draw the fields");
    assert_eq!(pane.text(), "The fields", "{:?}", identity);
    let (on_input, on_change) = input_of(&pane.tree().root, "name").expect("the name field");
    assert_eq!(on_input, Some(29), "the field asks for input events");
    assert_eq!(on_change, Some(30), "the field names its commits");

    // An input event, as the user types: the payload names the value, the
    // key names the node, and the fixture draws what it received.
    pane.send(
        DesignedHandler::Input,
        29,
        Some("name"),
        None,
        r#"{"value":"Ada"}"#,
    );
    assert!(
        pane.texts()
            .contains(r#"Sent: 29 on name: {"value":"Ada"}"#),
        "the input event's payload reached the view: {}",
        pane.texts()
    );

    // A change, on a commit: the payload names the value committed.
    pane.send(
        DesignedHandler::Change,
        30,
        Some("name"),
        None,
        r#"{"value":"Ada Lovelace"}"#,
    );
    assert!(
        pane.texts()
            .contains(r#"Sent: 30 on name: {"value":"Ada Lovelace"}"#),
        "the change event's payload reached the view: {}",
        pane.texts()
    );
    assert_eq!(pane.status(), Status::Idle);

    // The tree is still the fields', the state held by the keys.
    assert_eq!(pane.text(), "The fields");
}

#[test]
fn an_event_raised_on_an_older_tree_is_delivered_while_its_key_is_handled() {
    let pane = Pane::new();
    pane.install("Designed fixture");
    pane.open("", "Designed fixture");
    pane.press("Increment");
    assert_eq!(pane.text(), "Count: 1");

    // A press of the tree the user saw — one render older than the tree
    // on screen — is delivered: the node with that key still names a
    // handler, so what the user saw still works.
    let (callback, key) = button_of(&pane.tree().root, "Increment").unwrap();
    let saw = pane.render() - 1;
    pane.send(
        DesignedHandler::Press,
        callback,
        key.as_deref(),
        Some(saw),
        "{}",
    );
    assert_eq!(pane.text(), "Count: 2");
}

#[test]
fn an_event_whose_key_went_is_dropped() {
    let pane = Pane::new();
    let identity = pane.developing("Designed fixture");
    pane.open("", "Designed fixture");
    let (callback, _) = button_of(&pane.tree().root, "Increment").unwrap();
    pane.press("Draw the fields");
    assert_eq!(pane.text(), "The fields");

    // The tree the user saw named "Increment"; this one does not. The
    // press is dropped — the view never answers it, so the fields' tree
    // stays — and, the package being developed, its log says so.
    pane.send(
        DesignedHandler::Press,
        callback,
        Some("increment"),
        Some(1),
        "{}",
    );
    assert_eq!(pane.text(), "The fields");
    assert_eq!(pane.status(), Status::Idle);
    let lines = pane.launcher.extension_log(&identity);
    assert!(
        lines.iter().any(|line| line
            .text
            .contains("a press on the key \"increment\" of render 1 was dropped")),
        "the dropped event is reported: {:?}",
        lines
    );

    // The count never moved: the view below is the same one, uncounted.
    pane.press("Answer the counter again");
    assert_eq!(pane.text(), "Count: 0");
}

#[test]
fn an_event_two_renders_old_is_dropped() {
    let pane = Pane::new();
    pane.install("Designed fixture");
    pane.open("", "Designed fixture");
    pane.press("Increment");
    pane.press("Increment");
    assert_eq!(pane.text(), "Count: 2");

    // The render the user saw is two renders past: the SDKs keep two, and
    // Pane drops the event without sending it.
    let (callback, key) = button_of(&pane.tree().root, "Increment").unwrap();
    pane.send(
        DesignedHandler::Press,
        callback,
        key.as_deref(),
        Some(1),
        "{}",
    );
    assert_eq!(pane.text(), "Count: 2");
    assert_eq!(pane.status(), Status::Idle);
}

#[test]
fn development_reports_keys_shared_and_missing() {
    let pane = Pane::new();
    let identity = pane.developing("Designed fixture");
    pane.open("", "Designed fixture");
    pane.press("Draw a tree with key problems");

    // The tree draws — the shared keys matched by position, the keyless
    // field too — while the developed package's log reports each problem
    // once.
    assert_eq!(pane.text(), "Key problems");
    let lines = pane.launcher.extension_log(&identity);
    assert!(
        lines.iter().any(|line| line
            .text
            .contains("the key \"shared\" is shared by two siblings")),
        "the shared key is reported: {:?}",
        lines
    );
    assert!(
        lines
            .iter()
            .any(|line| line.text.contains("a stateful text-input has no key")),
        "the keyless field is reported: {:?}",
        lines
    );
    // One line each, not one per render: the tree re-renders identically
    // and reports nothing new.
    let reported = lines
        .iter()
        .filter(|line| line.text.contains("shared") || line.text.contains("text-input"))
        .count();
    assert_eq!(reported, 2, "{:?}", lines);
}
