//! Quicklinks, a default extension, through the launcher's public interface:
//! a quicklink created in its form is kept in the extension's settings,
//! found in root search, also after a restart, and opened with the system's
//! link handler, which these tests replace with a recording fake so that no
//! browser opens. The package is the one `cargo xtask guests` assembles in
//! `target/guests/packages/quicklinks`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use pane_core::{Launcher, LinkOpener, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

use rows::titles;

fn built(path: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(path);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// A link handler that records what it is asked to open, or refuses with
/// `refusal`.
#[derive(Clone, Default)]
struct FakeOpener {
    opened: Arc<Mutex<Vec<String>>>,
    refusal: Option<String>,
}

impl FakeOpener {
    fn refusing(reason: &str) -> FakeOpener {
        FakeOpener {
            refusal: Some(reason.into()),
            ..FakeOpener::default()
        }
    }

    fn opened(&self) -> Vec<String> {
        self.opened.lock().unwrap().clone()
    }
}

impl LinkOpener for FakeOpener {
    fn open(&self, url: &str) -> Result<(), String> {
        match &self.refusal {
            Some(reason) => Err(reason.clone()),
            None => {
                self.opened.lock().unwrap().push(url.into());
                Ok(())
            }
        }
    }
}

/// Pane's data location for one test, which outlives restarts.
struct Pane {
    data: TempDir,
    opener: FakeOpener,
}

impl Pane {
    fn new() -> Pane {
        Pane {
            data: tempfile::tempdir().unwrap(),
            opener: FakeOpener::default(),
        }
    }

    /// Starts Pane on this data location, as after a restart.
    fn start(&self) -> Launcher {
        Launcher::with_packages(
            Runtime::start(),
            vec![],
            self.data.path().join("extensions"),
        )
        .with_link_opener(Arc::new(self.opener.clone()))
    }

    /// Starts Pane with Quicklinks installed.
    fn with_quicklinks(&self) -> Launcher {
        let launcher = self.start();
        install(&launcher, &built("packages/quicklinks"));
        launcher.back();
        launcher
    }
}

fn install(launcher: &Launcher, folder: &Path) {
    block_on(launcher.install_package(folder));
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
}

fn search(launcher: &Launcher, query: &str) {
    block_on(launcher.set_query(query));
}

/// Opens the Quicklinks command from root search.
fn open_quicklinks(launcher: &Launcher) {
    launcher.back();
    search(launcher, "Quicklinks");
    let index = titles(launcher)
        .iter()
        .position(|title| title == "Quicklinks")
        .expect("the Quicklinks command is listed");
    launcher.select(index);
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().screen, Screen::Command);
}

/// Opens the form of the Quicklinks item titled `title`.
fn open_item(launcher: &Launcher, title: &str) {
    let index = titles(launcher)
        .iter()
        .position(|row| row == title)
        .unwrap_or_else(|| panic!("no item {title} in {:?}", titles(launcher)));
    launcher.select(index);
    block_on(launcher.activate_selected());
    assert!(launcher.view().form().is_some(), "{title} opens a form");
}

/// Fills in the open form's fields and submits it.
fn submit(launcher: &Launcher, values: &[(&str, &str)]) -> Status {
    for (field, value) in values {
        launcher.set_field_value(field, value);
    }
    block_on(launcher.submit_form());
    launcher.view().status
}

/// Creates the quicklink `name` for `url` through the form, then returns to
/// root search.
fn create(launcher: &Launcher, name: &str, url: &str) {
    open_quicklinks(launcher);
    open_item(launcher, "Create quicklink");
    let status = submit(launcher, &[("name", name), ("url", url)]);
    assert_eq!(status, Status::Result(format!("Saved quicklink “{name}”")));
    launcher.back();
    launcher.back();
}

#[test]
fn a_created_quicklink_is_found_in_root_search_and_opened() {
    let pane = Pane::new();
    let launcher = pane.with_quicklinks();

    create(
        &launcher,
        "Pane issues",
        "https://github.com/hoangvu12/pane/issues",
    );
    search(&launcher, "pane iss");

    let view = launcher.view();
    assert_eq!(titles(&launcher)[0], "Pane issues");
    assert_eq!(
        view.rows[0].subtitle.as_deref(),
        Some("Quicklink · https://github.com/hoangvu12/pane/issues")
    );
    assert_eq!(view.selected, Some(0));
    block_on(launcher.activate_selected());
    assert_eq!(
        pane.opener.opened(),
        ["https://github.com/hoangvu12/pane/issues"]
    );
    assert_eq!(
        launcher.view().status,
        Status::Result("Opened https://github.com/hoangvu12/pane/issues".into())
    );
    assert_eq!(launcher.view().query(), Some("pane iss"));
    // Its address finds it too; other words do not.
    search(&launcher, "github");
    assert_eq!(titles(&launcher), ["Pane issues"]);
    search(&launcher, "gitlab");
    assert_eq!(titles(&launcher), Vec::<String>::new());
}

#[test]
fn invalid_inputs_are_marked_on_their_fields_and_the_form_stays_open() {
    let pane = Pane::new();
    let launcher = pane.with_quicklinks();
    create(&launcher, "Docs", "https://docs.example.com");
    open_quicklinks(&launcher);
    open_item(&launcher, "Create quicklink");

    let rejected = [
        (("", "https://example.com"), "Name: Enter a name"),
        (
            ("Example", "example.com"),
            "URL: Enter a web address starting with http:// or https://",
        ),
        (
            ("Example", "ftp://example.com"),
            "URL: Enter a web address starting with http:// or https://",
        ),
        (("Example", "https://"), "URL: The address has no host"),
        (
            ("Example", "https://exa mple.com"),
            "URL: The address cannot contain spaces",
        ),
        (
            ("docs", "https://example.com"),
            "Name: A quicklink named “Docs” already exists",
        ),
    ];
    for ((name, url), error) in rejected {
        let status = submit(&launcher, &[("name", name), ("url", url)]);
        assert_eq!(status, Status::Error(error.into()), "{name} {url}");
        assert!(launcher.view().form().is_some(), "the form stays open");
    }

    // Corrected, the same form saves; nothing rejected was saved.
    let status = submit(
        &launcher,
        &[("name", "Example"), ("url", "HTTPS://example.com/a?b=c#d")],
    );
    assert_eq!(status, Status::Result("Saved quicklink “Example”".into()));
    launcher.back();
    launcher.back();
    search(&launcher, "example");
    assert_eq!(titles(&launcher), ["Example", "Docs"]);
}

#[test]
fn a_quicklink_is_edited_and_removed_through_its_form() {
    let pane = Pane::new();
    let launcher = pane.with_quicklinks();
    create(&launcher, "Docs", "https://docs.example.com");
    create(&launcher, "News", "https://news.example.com");

    open_quicklinks(&launcher);
    assert_eq!(
        titles(&launcher),
        ["Create quicklink", "Docs", "News"],
        "each quicklink is an item"
    );
    // Empty fields keep their value: only the address changes.
    open_item(&launcher, "Docs");
    let status = submit(&launcher, &[("url", "https://docs.example.org")]);
    assert_eq!(status, Status::Result("Saved quicklink “Docs”".into()));
    launcher.back();
    launcher.back();
    search(&launcher, "docs");
    assert_eq!(
        launcher.view().rows[0].subtitle.as_deref(),
        Some("Quicklink · https://docs.example.org")
    );

    // Renamed: found by the new name only.
    open_quicklinks(&launcher);
    open_item(&launcher, "Docs");
    let status = submit(&launcher, &[("name", "Manual")]);
    assert_eq!(status, Status::Result("Saved quicklink “Manual”".into()));
    launcher.back();
    launcher.back();
    search(&launcher, "manual");
    assert_eq!(titles(&launcher), ["Manual"]);
    search(&launcher, "docs");
    assert_eq!(titles(&launcher), ["Manual"], "found by its address");

    // Renaming onto another quicklink's name is refused.
    open_quicklinks(&launcher);
    open_item(&launcher, "Manual");
    let status = submit(&launcher, &[("name", "news")]);
    assert_eq!(
        status,
        Status::Error("Name: A quicklink named “News” already exists".into())
    );
    launcher.back();

    // Removed.
    open_item(&launcher, "News");
    let status = submit(&launcher, &[("then", "remove")]);
    assert_eq!(status, Status::Result("Removed quicklink “News”".into()));
    launcher.back();
    launcher.back();
    search(&launcher, "news");
    assert_eq!(titles(&launcher), Vec::<String>::new());
    open_quicklinks(&launcher);
    assert_eq!(titles(&launcher), ["Create quicklink", "Manual"]);
}

#[test]
fn quicklinks_survive_a_restart() {
    let pane = Pane::new();
    create(&pane.with_quicklinks(), "Docs", "https://docs.example.com");

    let restarted = pane.start();
    search(&restarted, "docs");

    assert_eq!(titles(&restarted), ["Docs"]);
    block_on(restarted.activate_selected());
    assert_eq!(pane.opener.opened(), ["https://docs.example.com"]);
}

#[test]
fn a_disabled_quicklinks_hides_its_quicklinks_and_keeps_them() {
    let pane = Pane::new();
    let launcher = pane.with_quicklinks();
    create(&launcher, "Docs", "https://docs.example.com");
    let identity = launcher.packages()[0].identity.clone();

    block_on(launcher.set_enabled(&identity, false));
    search(&launcher, "docs");
    assert_eq!(titles(&launcher), Vec::<String>::new());
    search(&launcher, "quicklinks");
    assert_eq!(titles(&launcher), Vec::<String>::new(), "nor its command");

    // Still disabled after a restart, then enabled again with its entries.
    let restarted = pane.start();
    search(&restarted, "docs");
    assert_eq!(titles(&restarted), Vec::<String>::new());
    block_on(restarted.set_enabled(&identity, true));
    // Enabled again, it answers from the next change of the query.
    search(&restarted, "doc");
    assert_eq!(titles(&restarted), ["Docs"]);
    assert!(restarted.packages()[0].enabled);
}

#[test]
fn a_link_the_system_cannot_open_is_explained() {
    let mut pane = Pane::new();
    pane.opener = FakeOpener::refusing("no program to open web links is installed");
    let launcher = pane.with_quicklinks();
    create(&launcher, "Docs", "https://docs.example.com");
    search(&launcher, "docs");

    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        Status::Error(
            "Could not open https://docs.example.com: no program to open web links is installed"
                .into()
        )
    );
    // Root search stays usable.
    search(&launcher, "quicklinks");
    assert_eq!(titles(&launcher), ["Quicklinks"]);
}

#[test]
fn pane_opens_a_link_of_any_scheme() {
    // The faulty fixture offers a file: link for "file link". Opening is
    // unfiltered (ADR 0037): the system's handler gets it.
    let pane = Pane::new();
    let launcher = pane.start();
    let folder = pane.data.path().join("faulty");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{"manifestVersion": 1, "title": "Faulty", "apiVersion": "0.1",
            "commands": [{"id": "faulty", "title": "Faulty answers",
            "component": "command.wasm", "rootResults": true}]}"#,
    )
    .unwrap();
    fs::copy(built("faulty.wasm"), folder.join("command.wasm")).unwrap();
    install(&launcher, &folder);
    launcher.back();

    search(&launcher, "file link");
    assert_eq!(titles(&launcher), ["A local file"]);
    block_on(launcher.activate_selected());

    assert_eq!(pane.opener.opened(), ["file:///etc/hosts"]);
}

#[test]
fn a_launcher_without_a_link_handler_explains_it() {
    let pane = Pane::new();
    let launcher = Launcher::with_packages(
        Runtime::start(),
        vec![],
        pane.data.path().join("extensions"),
    );
    install(&launcher, &built("packages/quicklinks"));
    launcher.back();
    create(&launcher, "Docs", "https://docs.example.com");
    search(&launcher, "docs");

    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        Status::Error(
            "Could not open https://docs.example.com: this Pane has no link handler".into()
        )
    );
}
