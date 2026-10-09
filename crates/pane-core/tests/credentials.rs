//! Local credentials protected on disk (#130), through the launcher's
//! public interface, beside the Clear cache and uninstall checks: on
//! Windows each value is encrypted with DPAPI for the current user
//! (`credentials.json` version 2), a version-1 file an earlier Pane wrote is
//! converted at start (and kept readable when that cannot be written), and
//! a value Windows cannot decrypt is explained to the extension, counted as
//! unreadable and never dropped by Pane on its own. On every system a file
//! of a version this Pane does not know is refused and never overwritten.
//! Every check runs against the settings sample in Rust, JavaScript and
//! TypeScript, real guests from `cargo xtask guests`, which keeps a sign-in
//! token as a local credential.

use std::fs;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::select_title;

const MANAGE_ROW: &str = "Manage Extensions";
const TOKEN: &str = "sample-token";

/// What "Show what Pane keeps" answers once every kind of data is saved.
const EVERYTHING_KEPT: &str =
    "Style: formal · Note: Water the plants · Signed in: yes · Cached greeting: Good day to you";

/// A settings sample package: the same command in each language.
struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    component: &'static str,
    title: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-settings",
    component: "sample_settings.wasm",
    title: "Settings sample",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-settings-js",
    component: "sample_settings_js.wasm",
    title: "JavaScript settings sample",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-settings-ts",
    component: "sample_settings_ts.wasm",
    title: "TypeScript settings sample",
};

/// Copies the assembled settings sample package of `fixture` into `folder`,
/// titled "Settings sample".
fn settings_package(fixture: &Fixture, folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(fixture.package);
    assert!(
        assembled.exists(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    fs::create_dir_all(folder).unwrap();
    let manifest = fs::read_to_string(assembled.join("pane.json")).unwrap();
    let original = format!("\"{}\"", fixture.title);
    assert!(manifest.contains(&original), "{manifest}");
    let manifest = manifest.replace(&original, "\"Settings sample\"");
    fs::write(folder.join("pane.json"), manifest).unwrap();
    fs::copy(
        assembled.join(fixture.component),
        folder.join(fixture.component),
    )
    .unwrap();
    folder.to_path_buf()
}

/// Pane's data folder and the package's source folder for one test, which
/// outlive restarts, with the package installed.
struct Pane {
    _sources: TempDir,
    data: TempDir,
    folder: PathBuf,
}

impl Pane {
    fn installed(fixture: &Fixture) -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let folder = settings_package(fixture, &sources.path().join("settings"));
        let pane = Pane {
            _sources: sources,
            data: tempfile::tempdir().unwrap(),
            folder,
        };
        block_on(pane.start().install_package(&pane.folder));
        pane
    }

    /// A launcher on this data folder; a new one is a restart of Pane.
    fn start(&self) -> Launcher {
        Launcher::with_packages(
            Runtime::start(),
            vec![],
            self.data.path().join("extensions"),
        )
    }

    /// The package's identity key, under which its values are kept.
    fn key(&self) -> String {
        PackageIdentity::local(&self.folder).unwrap().key()
    }

    fn credentials_path(&self) -> PathBuf {
        self.data.path().join("extensions").join("credentials.json")
    }

    /// `credentials.json` as written.
    fn credentials(&self) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(self.credentials_path()).unwrap()).unwrap()
    }

    #[cfg(windows)]
    fn write_credentials(&self, file: &serde_json::Value) {
        fs::write(self.credentials_path(), file.to_string()).unwrap();
    }
}

/// From root search, opens the Greeting command and runs its item titled
/// `item`, returning the outcome: its toast, or the status line.
fn run(launcher: &Launcher, item: &str) -> Status {
    launcher.back();
    launcher.back();
    select_title(launcher, "Greeting");
    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().screen, Screen::Command, "Greeting opened");
    select_title(launcher, item);
    block_on(launcher.activate_selected());
    shown(launcher)
}

/// Saves one value of each kind with the Greeting command, the sign-in
/// token among them.
fn save_everything(launcher: &Launcher) {
    for (item, answer) in [
        ("Use a formal greeting", "Saved the formal greeting"),
        ("Greet me", "Good day to you"),
        ("Save a note", "Saved a note"),
        ("Sign in", "Signed in on this computer"),
    ] {
        assert_eq!(run(launcher, item), Status::Result(answer.into()));
    }
    assert_eq!(kept(launcher), Status::Result(EVERYTHING_KEPT.into()));
}

/// What the Greeting command says Pane keeps for it.
fn kept(launcher: &Launcher) -> Status {
    run(launcher, "Show what Pane keeps")
}

/// Opens the extension manager from root search.
fn manage(launcher: &Launcher) {
    launcher.back();
    launcher.back();
    select_title(launcher, MANAGE_ROW);
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Extensions { .. }));
}

/// Chooses the row titled `title` of the extension manager, then the row
/// titled `choice` of the confirmation it asks, returning the status.
fn confirm(launcher: &Launcher, title: &str, choice: &str) -> Status {
    manage(launcher);
    select_title(launcher, title);
    block_on(launcher.activate_selected());
    assert!(matches!(launcher.view().screen, Screen::Confirm { .. }));
    select_title(launcher, choice);
    block_on(launcher.activate_selected());
    launcher.view().status
}

/// The subtitle of the extension manager's row titled `title`.
#[cfg(windows)]
fn subtitle(launcher: &Launcher, title: &str) -> String {
    launcher
        .view()
        .rows
        .into_iter()
        .find(|row| row.title == title)
        .and_then(|row| row.subtitle)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", rows::titles(launcher)))
}

/// The explanation an extension gets for a credential Windows cannot
/// decrypt, as the launcher shows the error it answers with.
#[cfg(windows)]
fn assert_explained(status: &Status) {
    let Status::Error(message) = status else {
        panic!("expected an error, got {status:?}");
    };
    assert!(
        message.contains(
            "Pane cannot read this credential on this computer: Windows could not decrypt it ("
        ),
        "{message}"
    );
    assert!(message.contains("). Sign in again."), "{message}");
}

/// A token saved by an extension reads back unchanged, also after a
/// restart. On Windows the file's bytes do not hold the token, whose value
/// records that DPAPI protects it (version 2); on macOS and Linux it stays
/// as before, plain in a file only the user can read (version 1).
fn a_token_reads_back_and_is_written_as_this_system_protects_it(fixture: &Fixture) {
    let pane = Pane::installed(fixture);
    save_everything(&pane.start());

    let bytes = fs::read(pane.credentials_path()).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let file = pane.credentials();
    let token = &file["packages"][&pane.key()]["token"];
    if cfg!(windows) {
        assert!(!text.contains(TOKEN), "{text}");
        assert_eq!(file["version"], 2);
        assert!(token["dpapi"].is_string(), "{token}");
    } else {
        assert_eq!(file["version"], 1);
        assert_eq!(*token, TOKEN);
    }
    assert_eq!(kept(&pane.start()), Status::Result(EVERYTHING_KEPT.into()));
}

/// A credentials file of a version this Pane does not know (a newer
/// Pane's) is refused, explained, and never overwritten: not by a save,
/// and not by an uninstall, which says it could not delete the
/// credentials.
fn a_credentials_file_of_an_unknown_version_is_refused_and_kept(fixture: &Fixture) {
    let pane = Pane::installed(fixture);
    save_everything(&pane.start());
    let newer = r#"{ "version": 3, "packages": {} }"#;
    fs::write(pane.credentials_path(), newer).unwrap();

    let launcher = pane.start();
    for item in ["Show what Pane keeps", "Sign in"] {
        match run(&launcher, item) {
            Status::Error(message) => assert!(
                message.contains("it has version 3, this Pane reads 1 and 2"),
                "{message}"
            ),
            other => panic!("expected an error, got {other:?}"),
        }
    }
    match confirm(
        &launcher,
        "Uninstall Settings sample",
        "Uninstall and keep saved data",
    ) {
        Status::Error(message) => assert!(
            message.contains("could not delete its credentials: Cannot read "),
            "{message}"
        ),
        other => panic!("expected an error, got {other:?}"),
    }
    assert_eq!(fs::read_to_string(pane.credentials_path()).unwrap(), newer);
}

/// Windows: a version-1 file, as an earlier Pane wrote it, is converted to
/// version 2 at start, and its token still reads.
#[cfg(windows)]
fn a_version_1_file_is_converted_at_start(fixture: &Fixture) {
    let pane = Pane::installed(fixture);
    save_everything(&pane.start());
    let earlier = serde_json::json!({
        "version": 1,
        "packages": { pane.key(): { "token": TOKEN } },
    });
    pane.write_credentials(&earlier);

    let launcher = pane.start();
    let text = fs::read_to_string(pane.credentials_path()).unwrap();
    assert!(!text.contains(TOKEN), "{text}");
    assert_eq!(pane.credentials()["version"], 2);
    assert_eq!(kept(&launcher), Status::Result(EVERYTHING_KEPT.into()));
}

#[cfg(not(windows))]
fn a_version_1_file_is_converted_at_start(_fixture: &Fixture) {}

/// Windows: a conversion whose write fails (here `credentials.json` is held
/// open without sharing deletion, so Windows refuses to replace it) leaves
/// the version-1 file as it was and every token readable; the next start
/// converts it.
#[cfg(windows)]
fn a_conversion_that_cannot_be_written_is_tried_again_at_the_next_start(fixture: &Fixture) {
    use std::os::windows::fs::OpenOptionsExt;
    /// `FILE_SHARE_READ`: others may read it, never replace or delete it.
    const SHARE_READ: u32 = 1;
    let pane = Pane::installed(fixture);
    save_everything(&pane.start());
    let earlier = serde_json::json!({
        "version": 1,
        "packages": { pane.key(): { "token": TOKEN } },
    });
    pane.write_credentials(&earlier);
    let written = fs::read_to_string(pane.credentials_path()).unwrap();

    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ)
        .open(pane.credentials_path())
        .unwrap();
    let launcher = pane.start();
    assert_eq!(
        fs::read_to_string(pane.credentials_path()).unwrap(),
        written
    );
    assert_eq!(kept(&launcher), Status::Result(EVERYTHING_KEPT.into()));
    drop(launcher);
    drop(held);

    let launcher = pane.start();
    let text = fs::read_to_string(pane.credentials_path()).unwrap();
    assert!(!text.contains(TOKEN), "{text}");
    assert_eq!(pane.credentials()["version"], 2);
    assert_eq!(kept(&launcher), Status::Result(EVERYTHING_KEPT.into()));
}

#[cfg(not(windows))]
fn a_conversion_that_cannot_be_written_is_tried_again_at_the_next_start(_fixture: &Fixture) {}

/// Windows: after a conversion at start that could not be written, an
/// uninstall, which reads the file again (still version 1), protects what
/// it writes: another package's token is never written as it is.
#[cfg(windows)]
fn a_removal_after_a_failed_conversion_protects_what_it_writes(fixture: &Fixture) {
    use std::os::windows::fs::OpenOptionsExt;
    /// `FILE_SHARE_READ`: others may read it, never replace or delete it.
    const SHARE_READ: u32 = 1;
    let pane = Pane::installed(fixture);
    save_everything(&pane.start());
    let another = PackageIdentity::local(pane.data.path()).unwrap().key();
    let earlier = serde_json::json!({
        "version": 1,
        "packages": {
            pane.key(): { "token": TOKEN },
            another.clone(): { "token": "another-token" },
        },
    });
    pane.write_credentials(&earlier);

    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ)
        .open(pane.credentials_path())
        .unwrap();
    let launcher = pane.start();
    drop(held);
    assert!(matches!(
        confirm(
            &launcher,
            "Uninstall Settings sample",
            "Uninstall and keep saved data"
        ),
        Status::Result(_)
    ));
    let text = fs::read_to_string(pane.credentials_path()).unwrap();
    assert!(!text.contains("another-token"), "{text}");
    let file = pane.credentials();
    assert_eq!(file["version"], 2);
    assert!(
        file["packages"][&another]["token"]["dpapi"].is_string(),
        "{file}"
    );
    assert!(file["packages"].get(pane.key()).is_none(), "{file}");
}

#[cfg(not(windows))]
fn a_removal_after_a_failed_conversion_protects_what_it_writes(_fixture: &Fixture) {}

/// Windows: a token whose protected bytes are damaged gives the extension
/// the explaining error, while its other values still read and save; Pane
/// keeps the damaged value as it was, also across restarts, and the
/// extension's `set` (Sign in) replaces it. A damaged credential under
/// another key leaves the token reading, and is kept as it was too.
#[cfg(windows)]
fn a_damaged_token_is_explained_kept_and_replaced_by_signing_in(fixture: &Fixture) {
    let pane = Pane::installed(fixture);
    save_everything(&pane.start());
    // Another credential of the package is damaged: the token still reads.
    let mut file = pane.credentials();
    file["packages"][&pane.key()]["other"] = serde_json::json!({ "dpapi": "AAAA" });
    pane.write_credentials(&file);
    let launcher = pane.start();
    assert_eq!(kept(&launcher), Status::Result(EVERYTHING_KEPT.into()));
    drop(launcher);

    // Then the token itself.
    let mut file = pane.credentials();
    file["packages"][&pane.key()]["token"]["dpapi"] = "AAAA".into();
    pane.write_credentials(&file);

    let launcher = pane.start();
    assert_explained(&kept(&launcher));
    // Its settings, content and cache still read and save.
    assert_eq!(
        run(&launcher, "Greet me"),
        Status::Result("Good day to you".into())
    );
    assert_eq!(
        run(&launcher, "Save a note"),
        Status::Result("Saved a note".into())
    );
    drop(launcher);
    let launcher = pane.start();
    assert_explained(&kept(&launcher));
    assert_eq!(
        pane.credentials()["packages"][&pane.key()]["token"]["dpapi"],
        "AAAA"
    );

    assert_eq!(
        run(&launcher, "Sign in"),
        Status::Result("Signed in on this computer".into())
    );
    assert_eq!(kept(&launcher), Status::Result(EVERYTHING_KEPT.into()));
    assert_ne!(
        pane.credentials()["packages"][&pane.key()]["token"]["dpapi"],
        "AAAA"
    );
    assert_eq!(
        pane.credentials()["packages"][&pane.key()]["other"]["dpapi"],
        "AAAA"
    );
}

#[cfg(not(windows))]
fn a_damaged_token_is_explained_kept_and_replaced_by_signing_in(_fixture: &Fixture) {}

/// Windows: Manage extensions counts a credential it cannot decrypt as
/// such, here for retained data whose credentials an earlier removal left
/// behind, and deleting the retained data removes them without decrypting
/// them.
#[cfg(windows)]
fn manage_extensions_counts_an_unreadable_credential(fixture: &Fixture) {
    let pane = Pane::installed(fixture);
    let launcher = pane.start();
    save_everything(&launcher);
    let readable = pane.credentials()["packages"][&pane.key()]["token"].clone();
    assert!(matches!(
        confirm(
            &launcher,
            "Uninstall Settings sample",
            "Uninstall and keep saved data"
        ),
        Status::Result(_)
    ));
    drop(launcher);
    // As if an earlier removal could not delete them: one readable, one
    // damaged.
    let mut file = pane.credentials();
    file["packages"][&pane.key()] = serde_json::json!({
        "token": { "dpapi": "AAAA" },
        "other": readable,
    });
    pane.write_credentials(&file);

    let launcher = pane.start();
    manage(&launcher);
    let identity = PackageIdentity::local(&pane.folder).unwrap();
    assert_eq!(
        subtitle(&launcher, "Delete retained data of Settings sample"),
        format!(
            "Not installed · keeps 1 setting, 1 content record and 2 credentials, 1 unreadable · \
             {identity}"
        )
    );
    assert_eq!(
        confirm(
            &launcher,
            "Delete retained data of Settings sample",
            "Delete retained data"
        ),
        Status::Result("Deleted the retained data of Settings sample".into())
    );
    assert!(pane.credentials()["packages"].get(pane.key()).is_none());
}

#[cfg(not(windows))]
fn manage_extensions_counts_an_unreadable_credential(_fixture: &Fixture) {}

/// Declares one test per check for each language's settings sample.
macro_rules! contract {
    ($($check:ident),* $(,)?) => {
        mod rust {
            $(#[test] fn $check() { super::$check(&super::RUST) })*
        }
        mod javascript {
            $(#[test] fn $check() { super::$check(&super::JAVASCRIPT) })*
        }
        mod typescript {
            $(#[test] fn $check() { super::$check(&super::TYPESCRIPT) })*
        }
    };
}

contract!(
    a_token_reads_back_and_is_written_as_this_system_protects_it,
    a_credentials_file_of_an_unknown_version_is_refused_and_kept,
    a_version_1_file_is_converted_at_start,
    a_conversion_that_cannot_be_written_is_tried_again_at_the_next_start,
    a_removal_after_a_failed_conversion_protects_what_it_writes,
    a_damaged_token_is_explained_kept_and_replaced_by_signing_in,
    manage_extensions_counts_an_unreadable_credential,
);
