//! First setup's choice of the official extensions (#283, ADR 0045):
//! on a data folder whose first setup has not chosen yet, Pane lists the
//! official extensions this system sets up — each pin's title and
//! description, before anything is fetched — with the default extensions
//! ticked, and the Set Up row selected. Unticking one and choosing Set Up
//! records the choice in `installed.json`: an unticked default is
//! declined, as an uninstall declines one, and is never set up by
//! itself; the ticked ones are set up in the background as first setup
//! always set them up, with the rows that try again when a fetch fails.
//! A declined default stays installable from Settings' Extensions group,
//! across restarts.
//!
//! The repositories are served from 127.0.0.1 (`support/repo_server.rs`)
//! and made from the samples `cargo xtask guests` assembles, which stand
//! in for the defaults' own repositories (#285); nothing reaches the
//! network.

use std::fs;
use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::defaults::DefaultExtension;
use pane_core::{Launcher, OperationKind, PackageIdentity, Runtime, SavedData, Screen, Status};
use serde_json::Value;
use tempfile::TempDir;

#[path = "support/defaults.rs"]
mod defaults;
#[path = "support/repo_server.rs"]
mod repo_server;
#[path = "support/rows.rs"]
mod rows;
#[path = "support/unreachable.rs"]
mod unreachable;

use defaults::from_sample;
use repo_server::Server;
use rows::{select_title, titles};

/// The screen's Set Up row, after the extensions.
const SET_UP: &str = "Set Up";

/// Root search's row that opens first setup's choice again while it is
/// not made.
const FIRST_SETUP_ROW: &str = "Set Up Official Extensions…";

/// Pane's data folder, its compiled code cache and runtime, and the
/// served repositories of two defaults, for one test.
struct Dirs {
    data: TempDir,
    /// Where the repositories' work trees live, which the server reads
    /// them from.
    _repos: TempDir,
    _cache: TempDir,
    runtime: Runtime,
    server: Server,
    pins: Vec<DefaultExtension>,
}

impl Dirs {
    /// Two defaults, the icons sample and the greet sample, each served
    /// from its own repository.
    fn new() -> Dirs {
        let cache = tempfile::tempdir().unwrap();
        let repos = tempfile::tempdir().unwrap();
        let server = Server::start();
        let pins = vec![
            from_sample(
                &server,
                repos.path(),
                "sample-icons",
                "Icons sample",
                "sample-icons",
            ),
            from_sample(
                &server,
                repos.path(),
                "sample-greet",
                "Greet sample",
                "sample-greet",
            ),
        ];
        Dirs {
            data: tempfile::tempdir().unwrap(),
            _repos: repos,
            runtime: Runtime::start_with_cache(cache.path().to_path_buf()).unwrap(),
            server,
            pins,
            _cache: cache,
        }
    }

    fn packages_dir(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// A launcher on this data folder with the served defaults; a new one
    /// is a restart of Pane.
    fn launcher(&self) -> Launcher {
        Launcher::with_packages(Ok(self.runtime.clone()), vec![], self.packages_dir())
            .with_defaults(self.pins.clone())
    }

    /// `installed.json`, as Pane wrote it.
    fn registry(&self) -> Value {
        let text = fs::read_to_string(self.packages_dir().join("installed.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    /// How many times the repository of the default `id` was asked for.
    fn fetches(&self, id: &str) -> usize {
        let wanted = format!("GET /{id}.git/info/refs");
        self.server
            .requests()
            .iter()
            .filter(|request| request.starts_with(&wanted))
            .count()
    }
}

/// The installed packages' titles, in installed order.
fn installed(launcher: &Launcher) -> Vec<String> {
    launcher.packages().iter().map(|p| p.title()).collect()
}

/// Selects the Set Up row and chooses it, awaiting the setting up it
/// starts.
fn set_up(launcher: &Launcher) {
    select_title(launcher, SET_UP);
    block_on(launcher.activate_selected());
}

#[test]
fn first_setup_lists_the_official_extensions_with_the_defaults_ticked() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();

    block_on(launcher.first_setup());

    // The choice is on screen: one row per official extension, with the
    // title and description its pin gives, all of them ticked, and the
    // Set Up row after them, selected — Enter sets up the defaults.
    let view = launcher.view();
    let Screen::Choice(choice) = &view.screen else {
        panic!("not first setup's choice: {view:#?}");
    };
    assert_eq!(view.title, "Pane's Official Extensions");
    assert_eq!(titles(&launcher), ["Icons sample", "Greet sample", SET_UP]);
    let descriptions: Vec<_> = choice
        .extensions
        .iter()
        .map(|extension| extension.description.clone())
        .collect();
    assert_eq!(
        descriptions,
        [
            Some("What the Icons sample does".to_owned()),
            Some("What the Greet sample does".to_owned())
        ]
    );
    assert_eq!(choice.ticked, [true, true]);
    assert_eq!(view.selected, Some(2), "{view:#?}");
    assert_eq!(
        view.rows[2].subtitle.as_deref(),
        Some("Set up the 2 ticked extensions")
    );
    // Nothing is fetched before the user chooses, and nothing is
    // recorded.
    assert!(dirs.server.requests().is_empty());
    assert!(launcher.packages().is_empty());
}

#[test]
fn an_unticked_default_is_not_acquired_and_the_choice_is_recorded_as_an_uninstall_is() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    block_on(launcher.first_setup());

    // The user unticks the greet sample and sets up the rest.
    assert!(launcher.toggle_choice_tick("sample-greet"));
    let Screen::Choice(choice) = launcher.view().screen else {
        panic!("not first setup's choice");
    };
    assert_eq!(choice.ticked, [true, false]);
    set_up(&launcher);

    // Back at root search, the ticked default set up and the unticked one
    // not even fetched.
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert_eq!(installed(&launcher), ["Icons sample"]);
    assert_eq!(
        launcher.view().status,
        Status::Result("Set up the Icons sample".into())
    );
    assert_eq!(dirs.fetches("sample-greet"), 0);
    // The choice is recorded with the packages: made, and the greet
    // sample declined — the record an uninstall writes too.
    let registry = dirs.registry();
    assert_eq!(registry["firstSetupChosen"], true);
    assert_eq!(
        registry["declinedDefaults"],
        serde_json::json!(["sample-greet"])
    );

    // After a restart, first setup does not ask again and sets up nothing
    // more: the declined default stays out.
    drop(launcher);
    let launcher = dirs.launcher();
    block_on(launcher.first_setup());
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert_eq!(installed(&launcher), ["Icons sample"]);
    assert_eq!(dirs.fetches("sample-greet"), 0);
    assert!(!titles(&launcher).contains(&FIRST_SETUP_ROW.to_owned()));
}

#[test]
fn a_default_left_out_is_installable_from_settings_and_the_choice_holds_across_restarts() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    block_on(launcher.first_setup());
    launcher.toggle_choice_tick("sample-greet");
    set_up(&launcher);
    assert_eq!(installed(&launcher), ["Icons sample"]);

    // Settings' Extensions group lists the official extension left out,
    // with what it does, and offers to install it.
    let official = launcher.official_extensions();
    let greet = official
        .iter()
        .find(|extension| extension.id == "sample-greet")
        .expect("the greet sample is listed");
    assert_eq!(greet.title, "Greet sample");
    assert_eq!(
        greet.description.as_deref(),
        Some("What the Greet sample does")
    );
    assert!(!greet.installed);
    assert!(
        official
            .iter()
            .any(|extension| extension.id == "sample-icons" && extension.installed)
    );
    let identity = PackageIdentity::default_extension("sample-greet");
    let install = |launcher: &Launcher| {
        launcher
            .extension_operations()
            .into_iter()
            .find(|operation| {
                operation.kind == OperationKind::InstallOfficial
                    && operation.owner.as_ref() == Some(&identity)
            })
    };
    // The choice holds across a restart: still declined, still offered.
    drop(launcher);
    let launcher = dirs.launcher();
    block_on(launcher.first_setup());
    assert_eq!(installed(&launcher), ["Icons sample"]);
    let operation = install(&launcher).expect("Settings offers to install it");
    assert_eq!(operation.title, "Install Greet sample");
    assert_eq!(operation.label, "Install");
    assert_eq!(operation.unavailable, None);

    block_on(launcher.run_extension_operation(&operation));

    // Installed as its default, no longer declined, and no longer offered.
    assert_eq!(
        installed(&launcher),
        ["Icons sample", "Rust greet provider sample"]
    );
    assert!(launcher.packages().iter().any(|p| p.identity == identity));
    assert_eq!(dirs.registry().get("declinedDefaults"), None);
    assert!(install(&launcher).is_none());
    assert!(
        launcher
            .official_extensions()
            .iter()
            .all(|extension| extension.installed)
    );
    // And stays installed after a restart.
    drop(launcher);
    let launcher = dirs.launcher();
    block_on(launcher.first_setup());
    assert_eq!(
        installed(&launcher),
        ["Icons sample", "Rust greet provider sample"]
    );
}

#[test]
fn an_uninstalled_default_is_declined_whether_its_data_is_kept_or_not() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    block_on(launcher.first_setup());
    set_up(&launcher);
    assert_eq!(
        installed(&launcher),
        ["Icons sample", "Rust greet provider sample"]
    );

    // Uninstalled, deleting its saved data: declined all the same, as
    // unticking it at first setup declines it.
    let identity = PackageIdentity::default_extension("sample-greet");
    block_on(launcher.uninstall(&identity, SavedData::Delete));
    assert_eq!(
        dirs.registry()["declinedDefaults"],
        serde_json::json!(["sample-greet"])
    );
    let fetched = dirs.fetches("sample-greet");

    drop(launcher);
    let launcher = dirs.launcher();
    block_on(launcher.first_setup());
    block_on(launcher.acquire_defaults());
    assert_eq!(installed(&launcher), ["Icons sample"]);
    assert_eq!(dirs.fetches("sample-greet"), fetched);
    // Settings offers it again.
    assert!(launcher.extension_operations().iter().any(|operation| {
        operation.kind == OperationKind::InstallOfficial
            && operation.owner.as_ref() == Some(&identity)
    }));
}

#[test]
fn without_a_connection_first_setup_explains_itself_and_offers_each_default_again() {
    let dirs = Dirs::new();
    // Nothing answers where the repositories are.
    let closed = unreachable::ClosedPort::new();
    let pins: Vec<DefaultExtension> = dirs
        .pins
        .iter()
        .map(|pin| DefaultExtension {
            repository: format!("{}/{}.git", closed.url(), pin.id),
            ..pin.clone()
        })
        .collect();
    let launcher = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir())
        .with_defaults(pins.clone());
    block_on(launcher.first_setup());
    set_up(&launcher);

    // Why is said, and each default is offered again as a row.
    let Status::Error(error) = launcher.view().status else {
        panic!("no failure explained: {:?}", launcher.view().status);
    };
    assert!(
        error.starts_with("Could not set up the Greet sample: ")
            && error.contains("Could not reach the Git repository 127.0.0.1:"),
        "{error}"
    );
    assert!(launcher.packages().is_empty());
    let shown = titles(&launcher);
    assert!(
        shown.contains(&"Set up Icons sample".to_owned()),
        "{shown:?}"
    );
    assert!(
        shown.contains(&"Set up Greet sample".to_owned()),
        "{shown:?}"
    );
    // The choice is made, so the screen is not shown again; nothing is
    // declined, so a restart tries each again by itself.
    assert!(!shown.contains(&FIRST_SETUP_ROW.to_owned()), "{shown:?}");
    assert_eq!(dirs.registry()["firstSetupChosen"], true);
    drop(launcher);
    let launcher = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir())
        .with_defaults(dirs.pins.clone());
    block_on(launcher.first_setup());
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert_eq!(
        installed(&launcher),
        ["Icons sample", "Rust greet provider sample"]
    );
}

#[test]
fn leaving_first_setup_chooses_nothing_and_root_search_offers_it_again() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    block_on(launcher.first_setup());

    // Back leaves for root search: nothing set up, nothing recorded, and a
    // row there opens the choice again.
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert!(launcher.packages().is_empty());
    assert!(dirs.server.requests().is_empty());
    assert!(!dirs.packages_dir().join("installed.json").exists());
    select_title(&launcher, FIRST_SETUP_ROW);
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Choice(_)));
    assert_eq!(titles(&launcher), ["Icons sample", "Greet sample", SET_UP]);

    // A restart asks again.
    launcher.back();
    drop(launcher);
    let launcher = dirs.launcher();
    block_on(launcher.first_setup());
    assert!(matches!(launcher.view().screen, Screen::Choice(_)));
}

#[test]
fn unticking_every_default_sets_up_none_and_declines_them_all() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    block_on(launcher.first_setup());
    launcher.toggle_choice_tick("sample-icons");
    launcher.toggle_choice_tick("sample-greet");
    assert_eq!(
        launcher.view().rows[2].subtitle.as_deref(),
        Some("Set up none of them: Pane starts with no extensions")
    );
    set_up(&launcher);

    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert!(launcher.packages().is_empty());
    assert!(dirs.server.requests().is_empty());
    assert_eq!(
        dirs.registry()["declinedDefaults"],
        serde_json::json!(["sample-icons", "sample-greet"])
    );
    drop(launcher);
    let launcher = dirs.launcher();
    block_on(launcher.first_setup());
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert!(launcher.packages().is_empty());
}

#[test]
fn an_install_set_up_before_the_choice_existed_is_not_asked() {
    let dirs = Dirs::new();
    // An earlier Pane set the defaults up with no choice to make.
    let launcher = dirs.launcher();
    block_on(launcher.acquire_defaults());
    assert_eq!(
        installed(&launcher),
        ["Icons sample", "Rust greet provider sample"]
    );
    assert_eq!(dirs.registry().get("firstSetupChosen"), None);
    drop(launcher);

    let launcher = dirs.launcher();
    block_on(launcher.first_setup());
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert!(!titles(&launcher).contains(&FIRST_SETUP_ROW.to_owned()));
}

#[test]
fn a_launcher_with_no_default_extensions_asks_nothing() {
    let dirs = Dirs::new();
    let launcher = Launcher::with_packages(Ok(dirs.runtime.clone()), vec![], dirs.packages_dir())
        .with_defaults(Vec::new());
    block_on(launcher.first_setup());
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    assert!(!titles(&launcher).contains(&FIRST_SETUP_ROW.to_owned()));
}
