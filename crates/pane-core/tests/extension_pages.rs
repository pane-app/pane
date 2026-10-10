//! What Pane's Settings window needs from the launcher to manage
//! extensions, one page per extension (#168, ADR 0043), through the
//! launcher's public interface: a command turned on and off on its own,
//! the launcher's rows that Settings handles ("Manage Extensions", the
//! install rows), the extension list without its explanations, an
//! extension's description, its source folder, its update check and its
//! mark. Real guests from `cargo xtask guests`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use pane_core::{
    ExtensionOperation, Launcher, OperationKind, PackageIdentity, Runtime, Screen, SettingsTarget,
    Status,
};
use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

use rows::{select_title, titles};

/// The assembled Rust settings sample, copied into `folder` as a package
/// of its own identity, its manifest given `description` when one is
/// given.
fn settings_package(folder: &Path, description: Option<&str>) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages/sample-settings");
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(assembled.join("pane.json")).unwrap()).unwrap();
    if let Some(description) = description {
        manifest["description"] = description.into();
    }
    fs::write(folder.join("pane.json"), manifest.to_string()).unwrap();
    fs::copy(
        assembled.join("sample_settings.wasm"),
        folder.join("sample_settings.wasm"),
    )
    .unwrap();
    folder.to_path_buf()
}

/// A launcher keeping its records in `data`, with the settings sample in
/// `folder` installed.
fn installed(data: &TempDir, folder: &Path) -> Launcher {
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    block_on(launcher.install_package(folder));
    launcher
}

/// The id of the settings sample's command: its package's key and its id
/// in `pane.json`.
fn greeting(folder: &Path) -> String {
    format!("{}#greeting", PackageIdentity::local(folder).unwrap().key())
}

#[test]
fn a_command_turned_off_leaves_root_search_and_keeps_its_alias_until_it_is_on_again() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"), None);
    let launcher = installed(&data, &folder);
    let command = greeting(&folder);
    assert!(titles(&launcher).contains(&"Greeting".to_owned()));
    block_on(
        launcher
            .set_alias(&command, "gr")
            .expect("the alias is taken"),
    );

    // Off: root search no longer offers it, nor its alias; its extension
    // stays enabled, and its alias is kept, not active, saying why.
    assert_eq!(
        block_on(launcher.set_command_enabled(&command, false)),
        Ok("Turned off Greeting".to_owned())
    );
    assert!(!titles(&launcher).contains(&"Greeting".to_owned()));
    block_on(launcher.set_query("gr"));
    assert!(!titles(&launcher).contains(&"Greeting".to_owned()));
    block_on(launcher.set_query(""));
    let package = launcher.packages().pop().expect("installed");
    assert!(package.enabled, "the extension stays enabled");
    let listed = package.listed_commands();
    assert_eq!(listed.len(), 1, "its page still lists the command");
    assert!(!listed[0].enabled);
    assert!(package.commands().is_empty(), "nothing is offered");
    let shortcut = launcher
        .shortcut_catalog()
        .groups
        .into_iter()
        .flat_map(|group| group.commands)
        .find(|shortcut| shortcut.id == command)
        .expect("the catalog lists it");
    assert_eq!(shortcut.alias.as_deref(), Some("gr"));
    assert_eq!(
        shortcut.alias_inactive.as_deref(),
        Some("Greeting is turned off")
    );
    assert!(!shortcut.hotkey_editable);

    // Recorded: a launcher started over the same records keeps it off.
    drop(launcher);
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let package = launcher.packages().pop().expect("installed");
    assert!(!package.command_enabled("greeting"));
    assert!(!titles(&launcher).contains(&"Greeting".to_owned()));

    // Asked again for what it is, nothing changes.
    assert_eq!(
        block_on(launcher.set_command_enabled(&command, false)),
        Ok("Greeting is off".to_owned())
    );

    // On: it is back, with its alias.
    assert_eq!(
        block_on(launcher.set_command_enabled(&command, true)),
        Ok("Turned on Greeting".to_owned())
    );
    assert!(titles(&launcher).contains(&"Greeting".to_owned()));
    block_on(launcher.set_query("gr"));
    assert!(titles(&launcher).contains(&"Greeting".to_owned()));
}

#[test]
fn a_command_turned_off_stays_off_after_its_extension_is_reloaded() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"), None);
    let launcher = installed(&data, &folder);
    let command = greeting(&folder);
    block_on(launcher.set_command_enabled(&command, false)).unwrap();

    // The extension list's Reload replaces its code; the choice is the
    // record's, kept.
    launcher.manage_extensions();
    select_title(&launcher, "Reload Settings sample");
    block_on(launcher.activate_selected());
    let package = launcher.packages().pop().expect("installed");
    assert!(!package.command_enabled("greeting"));
}

#[test]
fn an_unknown_command_cannot_be_turned_off() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"), None);
    let launcher = installed(&data, &folder);
    let missing = format!("{}#gone", PackageIdentity::local(&folder).unwrap().key());
    let refused = block_on(launcher.set_command_enabled(&missing, false));
    assert_eq!(
        refused,
        Err("Settings sample has no command `gone` now".to_owned())
    );
    assert!(block_on(launcher.set_command_enabled("local:/nowhere#x", false)).is_err());

    // A launcher that installs nothing has nothing to turn off.
    let bare = Launcher::new(Runtime::start(), vec![]);
    assert!(block_on(bare.set_command_enabled(&missing, false)).is_err());
}

#[test]
fn the_launcher_hands_extensions_and_installs_to_settings() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"), None);
    let launcher = installed(&data, &folder);

    // "Manage Extensions" is Pane's command, opening Settings at the
    // extensions; the install rows open its install flow; the Settings row
    // opens it. A command of an extension is the launcher's own to run.
    for (title, target) in [
        ("Manage Extensions", Some(SettingsTarget::Extensions)),
        (
            "Install extension from folder…",
            Some(SettingsTarget::InstallFromFolder),
        ),
        (
            "Install extension from npm…",
            Some(SettingsTarget::InstallFromNpm),
        ),
        (
            "Install extension from Git…",
            Some(SettingsTarget::InstallFromGit),
        ),
        ("Create Extension…", None),
        ("Import Extension…", None),
        ("Settings…", Some(SettingsTarget::Settings)),
        ("Greeting", None),
    ] {
        select_title(&launcher, title);
        assert_eq!(launcher.selected_settings_target(), target, "{title}");
    }
}

#[test]
fn the_extension_list_explains_nothing_and_is_not_restored() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"), None);
    let launcher = installed(&data, &folder);

    // The flow Settings drives: no paragraphs of explanation above its
    // rows, read or entered.
    assert!(launcher.extension_list().details().is_empty());
    launcher.manage_extensions();
    let view = launcher.view();
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert!(view.details().is_empty(), "{:?}", view.details());
    // The launcher window has no screen for it: a reopened launcher does
    // not restore it, and Escape leaves it for root search.
    assert!(!launcher.restorable_view());
    assert!(launcher.back());
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
}

#[test]
fn an_extensions_page_reads_its_description_its_folder_and_its_state() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(
        &sources.path().join("settings"),
        Some("  Keeps the greeting style you choose.  "),
    );
    let opened = Arc::new(Opened::default());
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_link_opener(opened.clone());
    block_on(launcher.install_package(&folder));
    let package = launcher.packages().pop().expect("installed");
    let identity = package.identity.clone();

    // Its description, trimmed.
    assert_eq!(
        package.description(),
        Some("Keeps the greeting style you choose.")
    );
    // Nothing to say about it: enabled, running, readable.
    assert_eq!(launcher.extension_mark(&identity), None);
    // A folder's extension has no source to check for an update: it is
    // reloaded instead.
    assert!(launcher.check_for_update(&identity).is_none());
    // Its folder is its source folder, shown through the launcher's opener.
    let source = identity.local_folder().unwrap().to_path_buf();
    assert_eq!(launcher.source_folder(&identity), Some(source.clone()));
    let shown = block_on(launcher.show_source_folder(&identity));
    assert_eq!(shown, Ok(format!("Showed {}", source.display())));
    assert_eq!(opened.0.lock().unwrap().as_slice(), [source]);

    // One not installed has none of it.
    let gone = PackageIdentity::npm("not-installed");
    assert!(launcher.source_folder(&gone).is_none());
    assert!(block_on(launcher.show_source_folder(&gone)).is_err());
}

/// The operation of `kind` the launcher offers for `identity`.
fn operation_of(
    launcher: &Launcher,
    identity: &PackageIdentity,
    kind: OperationKind,
) -> ExtensionOperation {
    launcher
        .extension_operations()
        .into_iter()
        .find(|operation| operation.kind == kind && operation.owner.as_ref() == Some(identity))
        .unwrap_or_else(|| panic!("no {kind:?} for {identity}"))
}

#[test]
fn settings_runs_typed_operations_without_moving_the_launcher_off_its_screen() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = settings_package(&sources.path().join("settings"), None);
    let launcher = installed(&data, &folder);
    let identity = PackageIdentity::local(&folder).unwrap();
    block_on(launcher.set_query("gree"));

    // What each operation is, whose, and whether it is on, is the
    // launcher's to say: nothing is read from a row's words.
    let operations = launcher.extension_operations();
    let kinds: Vec<OperationKind> = operations
        .iter()
        .filter(|operation| operation.owner.as_ref() == Some(&identity))
        .map(|operation| operation.kind)
        .collect();
    for kind in [
        OperationKind::Enable,
        OperationKind::Reload,
        OperationKind::ClearCache,
        OperationKind::Uninstall,
        OperationKind::Develop,
    ] {
        assert!(kinds.contains(&kind), "{kind:?} in {kinds:?}");
    }
    let enable = operation_of(&launcher, &identity, OperationKind::Enable);
    assert_eq!(enable.on, Some(true));
    let reload = operation_of(&launcher, &identity, OperationKind::Reload);
    assert_eq!(reload.label, "Reload");
    assert_eq!(reload.title, "Reload Settings sample");
    let global = operations
        .iter()
        .find(|operation| {
            operation.kind == OperationKind::AutomaticUpdates && operation.owner.is_none()
        })
        .expect("every extension's automatic updates");
    assert!(global.on.is_some());
    let fallback_owners: Vec<_> = operations
        .iter()
        .filter(|operation| operation.kind == OperationKind::Alias)
        .map(|operation| (operation.owner.clone(), operation.command.clone()))
        .collect();
    assert!(
        fallback_owners.contains(&(Some(identity.clone()), Some(greeting(&folder)))),
        "{fallback_owners:?}"
    );

    // Run from Settings, an operation leaves the launcher where the user
    // had it: root search, the query typed.
    block_on(launcher.run_extension_operation(&reload));
    assert!(
        matches!(launcher.screen(), Screen::Root { ref query } if query == "gree"),
        "{:?}",
        launcher.screen()
    );
    assert!(
        matches!(launcher.view().status, Status::Result(_)),
        "{:?}",
        launcher.view().status
    );
    block_on(launcher.run_extension_operation(&enable));
    assert!(!launcher.packages()[0].enabled);
    assert_eq!(
        operation_of(&launcher, &identity, OperationKind::Enable).on,
        Some(false)
    );
    assert!(matches!(launcher.screen(), Screen::Root { .. }));
    block_on(launcher.run_extension_operation(&enable));
    assert!(launcher.packages()[0].enabled);

    // A confirmation shows instead, for Settings to answer; cancelled, the
    // launcher returns to root search, not to a list.
    let clear = operation_of(&launcher, &identity, OperationKind::ClearCache);
    block_on(launcher.run_extension_operation(&clear));
    assert!(matches!(launcher.screen(), Screen::Confirm { .. }));
    select_title(&launcher, "Cancel");
    block_on(launcher.activate_selected());
    assert!(
        matches!(launcher.screen(), Screen::Root { .. }),
        "{:?}",
        launcher.screen()
    );

    // An operation no longer offered does nothing.
    let gone = ExtensionOperation {
        id: "reload:gone".into(),
        ..reload
    };
    block_on(launcher.run_extension_operation(&gone));
    assert!(matches!(launcher.screen(), Screen::Root { .. }));
}

/// A link opener that records the files and folders it is asked to open.
#[derive(Default)]
struct Opened(Mutex<Vec<PathBuf>>);

impl pane_core::LinkOpener for Opened {
    fn open(&self, _url: &str) -> Result<(), String> {
        Ok(())
    }

    fn open_file(&self, path: &Path) -> Result<(), String> {
        self.0.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }
}
