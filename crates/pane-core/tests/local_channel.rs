//! The local channel (#217) as `pane-ext dev` uses it, against a launcher
//! listening on an endpoint of the test's own, as the running Pane does. A
//! folder Pane has not installed is shown for installation with the build
//! handed over, and developed once the author installs it; declining the
//! preview refuses the build; an installed folder is developed at once.
//! One extension of a collection, named by its id (ADR 0044), is developed
//! alone, with its own identity and preview, while the collection's other
//! extensions are untouched.
//! Pane's messages and what the package prints come back as log events; a
//! build that failed elsewhere keeps the working code and is shown as Pane's
//! own are; a later build is reloaded; closing the connection stops the
//! development, and stopping it in Pane ends the connection's. The builds
//! are the guests `cargo xtask guests` builds, staged as `pane-ext` stages
//! its builds.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::local_channel::{self, Endpoint, Event, Previews, Request, Sender};
use pane_core::{Launcher, PackageIdentity, Runtime, Status};

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guest;
use rows::select_title;

const DEADLINE: Duration = Duration::from_secs(120);

const MANIFEST: &str = r#"{
  "manifestVersion": 1,
  "title": "Dev",
  "apiVersion": "0.1",
  "commands": [{ "id": "open", "title": "Open Dev", "component": "command.wasm" }]
}"#;

/// The manifest of a package titled `title`, with one command.
fn manifest(title: &str) -> String {
    format!(
        r#"{{
  "manifestVersion": 1,
  "title": "{title}",
  "apiVersion": "0.1",
  "commands": [{{ "id": "open", "title": "Open {title}", "component": "command.wasm" }}]
}}"#
    )
}

/// What the window does with an install preview it is asked to show.
#[derive(Clone, Copy)]
enum Answer {
    Install,
    Leave,
}

/// A launcher listening on an endpoint of the test's own, with a window
/// that answers each install preview as `answer` says, and the package's
/// source folder, holding only its manifest: its builds are staged
/// elsewhere, as `pane-ext` stages them.
struct Pane {
    dir: tempfile::TempDir,
    launcher: Launcher,
    endpoint: Endpoint,
    _server: local_channel::Server,
    folder: PathBuf,
    identity: PackageIdentity,
}

impl Pane {
    fn new(answer: Answer) -> Pane {
        let dir = tempfile::tempdir().unwrap();
        let launcher = Launcher::with_packages(
            Ok(Runtime::start().unwrap()),
            vec![],
            dir.path().join("data/extensions"),
        );
        let endpoint = if cfg!(windows) {
            let name = dir.path().file_name().unwrap().to_string_lossy();
            Endpoint::at(format!(r"\\.\pipe\pane-channel-test-{name}"))
        } else {
            // Its folder is left for Pane to make, as only this user's: a
            // temporary folder others can enter is refused.
            Endpoint::at(dir.path().join("pane").join("channel"))
        };
        let (server, previews) = local_channel::serve(launcher.clone(), &endpoint).unwrap();
        window(launcher.clone(), previews, answer);
        let folder = dir.path().join("Dev");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("pane.json"), MANIFEST).unwrap();
        let identity = PackageIdentity::local(&folder).unwrap();
        Pane {
            dir,
            launcher,
            endpoint,
            _server: server,
            folder,
            identity,
        }
    }

    /// A build of the package from the guest `source`, staged in its own
    /// folder.
    fn staged(&self, name: &str, source: &str) -> PathBuf {
        let staging = self.dir.path().join("staging").join(name);
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("pane.json"), MANIFEST).unwrap();
        fs::copy(guest(source), staging.join("command.wasm")).unwrap();
        staging
    }

    fn develop(&self, staging: &Path) -> Request {
        Request::Develop {
            folder: self.folder.clone(),
            extension: None,
            staging: staging.to_path_buf(),
            command: "cargo build".into(),
        }
    }

    /// A collection of two extensions at `tools`, each a package of its
    /// own: `clock`, whose builds are staged elsewhere, and `timers`,
    /// which stays as it is installed.
    fn collection(&self) -> PathBuf {
        let root = self.dir.path().join("tools");
        for (id, title) in [("clock", "Clock"), ("timers", "Timers")] {
            let folder = root.join("extensions").join(id);
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("pane.json"), manifest(title)).unwrap();
        }
        fs::write(
            root.join("pane-collection.json"),
            r#"{ "extensions": [ { "id": "clock", "path": "extensions/clock" },
                                 { "id": "timers", "path": "extensions/timers" } ] }"#,
        )
        .unwrap();
        root
    }

    /// A build of the extension titled `title` from the guest `source`,
    /// staged in its own folder.
    fn staged_extension(&self, name: &str, title: &str, source: &str) -> PathBuf {
        let staging = self.dir.path().join("staging").join(name);
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("pane.json"), manifest(title)).unwrap();
        fs::copy(guest(source), staging.join("command.wasm")).unwrap();
        staging
    }

    /// A `develop` request for the extension `id` of the collection at
    /// `root`, handing over the build staged in `staging`.
    fn develop_extension(&self, root: &Path, id: &str, staging: &Path) -> Request {
        Request::Develop {
            folder: root.to_path_buf(),
            extension: Some(id.into()),
            staging: staging.to_path_buf(),
            command: "cargo build".into(),
        }
    }

    /// A connection to it, subscribed to the package's log, as `pane-ext`
    /// connects.
    fn connect(&self) -> Client {
        let (sender, events) = local_channel::connect(&self.endpoint).unwrap();
        assert!(sender.send(&Request::Subscribe));
        Client {
            sender,
            events,
            log: Vec::new(),
        }
    }

    /// From root search, opens the command titled `command` and runs its
    /// item titled `item`.
    fn run(&self, command: &str, item: &str) -> Status {
        for _ in 0..3 {
            self.launcher.back();
        }
        select_title(&self.launcher, command);
        block_on(self.launcher.activate_selected());
        select_title(&self.launcher, item);
        block_on(self.launcher.activate_selected());
        shown(&self.launcher)
    }

    fn wait_until(&self, what: &str, mut done: impl FnMut(&Launcher) -> bool) {
        let deadline = Instant::now() + DEADLINE;
        while !done(&self.launcher) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Stands in for Pane's window: shows each install preview it is asked
/// for, and answers it.
fn window(launcher: Launcher, mut previews: Previews, answer: Answer) {
    std::thread::spawn(move || {
        while let Some(asked) = block_on(previews.next()) {
            match &asked {
                local_channel::ToPreview::Folder(folder) => {
                    block_on(launcher.preview_package(folder))
                }
                local_channel::ToPreview::Collection(folder, id) => {
                    block_on(launcher.preview_collection(folder, id))
                }
            }
            match answer {
                Answer::Install => {
                    select_title(&launcher, "Install");
                    block_on(launcher.activate_selected());
                }
                Answer::Leave => {
                    // After reading it, as an author would: Pane checks
                    // what is on show from time to time.
                    std::thread::sleep(Duration::from_secs(1));
                    launcher.back();
                }
            }
        }
    });
}

/// `pane-ext`'s end of a connection.
struct Client {
    sender: Sender,
    events: Receiver<Event>,
    /// The log events received so far, as "<source>: <text>".
    log: Vec<String>,
}

impl Client {
    fn receive(&mut self) -> Option<Event> {
        let event = self.events.recv_timeout(DEADLINE).ok()?;
        if let Event::Log { source, text, .. } = &event {
            self.log.push(format!("{source}: {text}"));
        }
        Some(event)
    }

    /// The next event that is not a log line.
    fn answer(&mut self) -> Event {
        loop {
            match self.receive() {
                Some(Event::Log { .. }) => {}
                Some(answer) => return answer,
                None => panic!("no answer; the log: {:#?}", self.log),
            }
        }
    }

    /// Receives log lines until one that holds `text`.
    fn wait_for_log(&mut self, text: &str) {
        while !self.log.iter().any(|line| line.contains(text)) {
            match self.receive() {
                Some(Event::Log { .. }) => {}
                other => panic!("{other:?} before {text:?}; the log: {:#?}", self.log),
            }
        }
    }
}

#[test]
fn a_folder_not_installed_is_previewed_then_developed_until_the_connection_closes() {
    let pane = Pane::new(Answer::Install);
    let mut client = pane.connect();

    // The first build: Pane shows its install preview, with this build,
    // and develops the package once it is installed.
    let first = pane.staged("first", "sample_settings");
    assert!(client.sender.send(&pane.develop(&first)));
    assert_eq!(
        client.answer(),
        Event::Previewing {
            title: "Dev".into()
        }
    );
    let Event::Developing {
        title,
        replaced,
        installed,
    } = client.answer()
    else {
        panic!("not developed; the log: {:#?}", client.log);
    };
    assert_eq!(title, "Dev");
    assert!(replaced);
    assert!(installed.unwrap().join("pane.json").is_file());
    assert!(pane.launcher.development(&pane.identity).is_some());
    // The preview put the build in the source folder, as development does.
    assert!(pane.folder.join("command.wasm").is_file());
    client.wait_for_log("pane: Developing Dev with pane-ext");

    // What the package prints comes as log events.
    assert_eq!(
        pane.run("Open Dev", "Write to the log"),
        Status::Result("Wrote to the log".into())
    );
    client.wait_for_log("stdout: an info line");
    client.wait_for_log("stderr: an error line");

    // A build that failed in pane-ext's terminal is shown as Pane's own
    // are, and the working code is kept.
    assert!(client.sender.send(&Request::Building));
    assert!(client.sender.send(&Request::Failed {
        summary: "error: expected `;`".into(),
        output: vec!["error: expected `;`".into()],
        earlier: 0,
        log: None,
    }));
    client.wait_for_log("pane: Dev did not build: error: expected `;`");
    let development = pane.launcher.development(&pane.identity).unwrap();
    assert_eq!(development.command, "cargo build");
    assert_eq!(development.failure.unwrap().summary, "error: expected `;`");
    assert_eq!(
        pane.run("Open Dev", "Write to the log"),
        Status::Result("Wrote to the log".into())
    );

    // A later build is reloaded.
    let second = pane.staged("second", "sample_settings");
    assert!(client.sender.send(&pane.develop(&second)));
    let Event::Developing { replaced, .. } = client.answer() else {
        panic!("not reloaded; the log: {:#?}", client.log);
    };
    assert!(replaced);
    client.wait_for_log("pane: Reloaded Dev");
    let development = pane.launcher.development(&pane.identity).unwrap();
    assert_eq!(development.failure, None);

    // Closing the connection, as pane-ext does when it stops, stops the
    // development; the package stays installed.
    drop(client);
    pane.wait_until("the development to stop", |launcher| {
        launcher.development(&pane.identity).is_none()
    });
    assert!(
        pane.launcher
            .packages()
            .iter()
            .any(|package| package.identity == pane.identity)
    );
}

#[test]
fn a_folder_whose_preview_is_left_is_not_developed() {
    let pane = Pane::new(Answer::Leave);
    let mut client = pane.connect();
    let first = pane.staged("first", "sample_settings");
    assert!(client.sender.send(&pane.develop(&first)));
    assert_eq!(
        client.answer(),
        Event::Previewing {
            title: "Dev".into()
        }
    );
    let Event::Refused { message } = client.answer() else {
        panic!("not refused; the log: {:#?}", client.log);
    };
    assert!(message.contains("Dev was not installed"), "{message}");
    assert!(pane.launcher.packages().is_empty());
    assert!(pane.launcher.development(&pane.identity).is_none());
}

#[test]
fn an_installed_folder_is_developed_at_once_until_pane_stops_it() {
    let pane = Pane::new(Answer::Leave);
    fs::copy(guest("sample_settings"), pane.folder.join("command.wasm")).unwrap();
    block_on(pane.launcher.install_package(&pane.folder));
    let mut client = pane.connect();

    // No preview: the build is reloaded at once.
    let first = pane.staged("first", "sample_settings");
    assert!(client.sender.send(&pane.develop(&first)));
    let Event::Developing { replaced, .. } = client.answer() else {
        panic!("not developed; the log: {:#?}", client.log);
    };
    assert!(replaced);
    client.wait_for_log("pane: Reloaded Dev");

    // Stopping it in Pane ends the connection's development.
    pane.launcher.stop_developing(&pane.identity);
    assert_eq!(
        client.answer(),
        Event::Ended {
            message: "Pane stopped developing Dev".into()
        }
    );
}

#[test]
fn a_request_pane_does_not_answer_is_refused_saying_why() {
    let pane = Pane::new(Answer::Leave);
    let mut client = pane.connect();
    // A development of a folder that is not there.
    let missing = Request::Develop {
        folder: pane.dir.path().join("missing"),
        extension: None,
        staging: pane.staged("first", "sample_settings"),
        command: "cargo build".into(),
    };
    assert!(client.sender.send(&missing));
    assert!(matches!(client.answer(), Event::Refused { .. }));
}

#[test]
fn one_extension_of_a_collection_is_previewed_developed_and_reloaded_alone() {
    let pane = Pane::new(Answer::Install);
    let tools = pane.collection();
    let clock = PackageIdentity::local_extension(&tools, "clock").unwrap();
    let timers = PackageIdentity::local_extension(&tools, "timers").unwrap();
    // The collection's other extension is installed, as its own package:
    // developing `clock` never touches it.
    let timers_folder = tools.join("extensions/timers");
    fs::copy(guest("sample_rust"), timers_folder.join("command.wasm")).unwrap();
    block_on(pane.launcher.preview_collection(&tools, "timers"));
    select_title(&pane.launcher, "Install");
    block_on(pane.launcher.activate_selected());
    assert_eq!(
        pane.launcher.view().status,
        Status::Result("Installed Timers".into())
    );
    let mut client = pane.connect();

    // Clock is not installed: its first build is shown in its own install
    // preview, naming the id, with this build, and developed once the
    // author installs it.
    let first = pane.staged_extension("clock-first", "Clock", "sample_settings");
    assert!(
        client
            .sender
            .send(&pane.develop_extension(&tools, "clock", &first))
    );
    assert_eq!(
        client.answer(),
        Event::Previewing {
            title: "Clock".into()
        }
    );
    let Event::Developing {
        title,
        replaced,
        installed,
    } = client.answer()
    else {
        panic!("not developed; the log: {:#?}", client.log);
    };
    assert_eq!(title, "Clock");
    assert!(replaced);
    assert!(installed.unwrap().join("pane.json").is_file());
    // The development is Clock's alone, with its own identity; Timers is
    // not developed, and still answers its own command.
    assert!(pane.launcher.development(&clock).is_some());
    assert!(pane.launcher.development(&timers).is_none());
    assert_eq!(
        pane.run("Open Timers", "Say hello"),
        Status::Result("Hello from the Rust guest".into())
    );
    // The preview put the build in the extension's folder, as development
    // does — never in the collection's root or the other extension's.
    assert!(tools.join("extensions/clock/command.wasm").is_file());
    assert_eq!(
        fs::read(timers_folder.join("command.wasm")).unwrap(),
        fs::read(guest("sample_rust")).unwrap()
    );
    // The extension log the terminal follows is Clock's.
    client.wait_for_log("pane: Developing Clock with pane-ext");
    assert_eq!(
        pane.run("Open Clock", "Write to the log"),
        Status::Result("Wrote to the log".into())
    );
    client.wait_for_log("stdout: an info line");

    // A later build is reloaded, and the other extension is untouched.
    let second = pane.staged_extension("clock-second", "Clock", "sample_settings");
    assert!(
        client
            .sender
            .send(&pane.develop_extension(&tools, "clock", &second))
    );
    let Event::Developing { replaced, .. } = client.answer() else {
        panic!("not reloaded; the log: {:#?}", client.log);
    };
    assert!(replaced);
    client.wait_for_log("pane: Reloaded Clock");
    assert!(pane.launcher.development(&timers).is_none());
    assert_eq!(
        pane.run("Open Timers", "Say hello"),
        Status::Result("Hello from the Rust guest".into())
    );

    // Closing the connection stops Clock's development; both extensions
    // stay installed.
    drop(client);
    pane.wait_until("the development to stop", |launcher| {
        launcher.development(&clock).is_none()
    });
    let installed: Vec<PackageIdentity> = pane
        .launcher
        .packages()
        .iter()
        .map(|package| package.identity.clone())
        .collect();
    assert!(installed.contains(&clock), "{installed:?}");
    assert!(installed.contains(&timers), "{installed:?}");

    // A second run, with the extension now installed, is developed at
    // once, with no preview: the build is reloaded.
    let mut client = pane.connect();
    let third = pane.staged_extension("clock-third", "Clock", "sample_settings");
    assert!(
        client
            .sender
            .send(&pane.develop_extension(&tools, "clock", &third))
    );
    let Event::Developing { replaced, .. } = client.answer() else {
        panic!("not developed; the log: {:#?}", client.log);
    };
    assert!(replaced);
    client.wait_for_log("pane: Reloaded Clock");
    assert!(pane.launcher.development(&timers).is_none());
}

#[test]
fn a_collection_without_an_id_is_explained_and_an_unknown_one_is_refused() {
    let pane = Pane::new(Answer::Leave);
    let tools = pane.collection();
    let mut client = pane.connect();
    let root = PackageIdentity::local(&tools)
        .unwrap()
        .local_folder()
        .unwrap()
        .to_path_buf();

    // A collection named without an id is explained: name one of its
    // extensions, after `#`.
    let no_id = Request::Develop {
        folder: tools.clone(),
        extension: None,
        staging: pane.staged("first", "sample_settings"),
        command: "cargo build".into(),
    };
    assert!(client.sender.send(&no_id));
    let Event::Refused { message } = client.answer() else {
        panic!("not refused; the log: {:#?}", client.log);
    };
    assert!(
        message.contains(&format!(
            "The folder {} is a collection, not one extension: its root holds \
             pane-collection.json, which lists the extensions it offers by id; name the one to \
             develop after `#`",
            root.display()
        )),
        "{message}"
    );
    assert!(pane.launcher.packages().is_empty());

    // An id the collection does not list is refused as an install of one
    // is.
    let missing = pane.staged("second", "sample_settings");
    assert!(
        client
            .sender
            .send(&pane.develop_extension(&tools, "nobody", &missing))
    );
    let Event::Refused { message } = client.answer() else {
        panic!("not refused; the log: {:#?}", client.log);
    };
    assert!(
        message.contains(&format!(
            "The collection at {} lists no extension `nobody` in its \
             pane-collection.json",
            root.display()
        )),
        "{message}"
    );
    // A `#<id>` on the suite's one-extension folder names no extension of
    // a collection, as an install naming one is told.
    let plain = Request::Develop {
        folder: pane.folder.clone(),
        extension: Some("clock".into()),
        staging: pane.staged("third", "sample_settings"),
        command: "cargo build".into(),
    };
    assert!(client.sender.send(&plain));
    let Event::Refused { message } = client.answer() else {
        panic!("not refused; the log: {:#?}", client.log);
    };
    assert!(
        message.contains("is not a collection: its root holds pane.json, one extension"),
        "{message}"
    );
    assert!(pane.launcher.packages().is_empty());
}
