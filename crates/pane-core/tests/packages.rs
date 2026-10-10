//! Installing local extension packages through the launcher's public
//! interface: previewing a package folder, installing it into Pane's managed
//! location, running the installed command, and the duplicate, update and
//! incompatible outcomes. Packages are built in temporary folders around the
//! real guest components from `cargo xtask guests`.

use std::fs;
use std::path::{Path, PathBuf};

use futures::executor::block_on;
use pane_core::{
    CallError, CommandRegistration, Launcher, PackageIdentity, Platform, Runtime, Screen, Status,
    Unavailable,
};
use tempfile::TempDir;

#[path = "support/platforms.rs"]
mod platforms;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/guests.rs"]
mod guests;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use guests::guest;
use rows::titles;

/// A package manifest for one command backed by `component`.
fn manifest(title: &str, version: &str, component: &str) -> String {
    format!(
        r#"{{
  "manifestVersion": 1,
  "title": "{title}",
  "version": "{version}",
  "apiVersion": "0.1",
  "commands": [
    {{ "id": "hello", "title": "Say hello", "subtitle": "Greets you", "component": "{component}" }}
  ]
}}"#
    )
}

/// Writes a package folder at `folder` with `pane.json` and the named guest
/// copied to `hello.wasm`.
fn package(folder: &Path, title: &str, version: &str, guest_name: &str) -> PathBuf {
    fs::create_dir_all(folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        manifest(title, version, "hello.wasm"),
    )
    .unwrap();
    fs::copy(guest(guest_name), folder.join("hello.wasm")).unwrap();
    folder.to_path_buf()
}

/// Test-local directories: package sources and Pane's data location.
struct Dirs {
    sources: TempDir,
    data: TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            sources: tempfile::tempdir().unwrap(),
            data: tempfile::tempdir().unwrap(),
        }
    }

    fn source(&self, name: &str) -> PathBuf {
        self.sources.path().join(name)
    }

    fn launcher(&self) -> Launcher {
        Launcher::with_packages(
            Runtime::start(),
            vec![],
            self.data.path().join("extensions"),
        )
    }
}

const INSTALL_ROW: &str = "Install extension from folder…";
const NPM_ROW: &str = "Install extension from npm…";
const GIT_ROW: &str = "Install extension from Git…";
const MANAGE_ROW: &str = "Manage Extensions";
const SETTINGS_ROW: &str = "Settings…";

#[test]
fn a_previewed_local_package_installs_and_its_command_runs() {
    let dirs = Dirs::new();
    let folder = package(&dirs.source("hello"), "Hello", "1.0.0", "sample_rust");
    let launcher = dirs.launcher();
    assert_eq!(
        titles(&launcher),
        [INSTALL_ROW, NPM_ROW, GIT_ROW, SETTINGS_ROW]
    );
    assert!(launcher.selected_asks_for_folder());

    block_on(launcher.preview_package(&folder));
    assert!(!launcher.selected_asks_for_folder());

    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Package { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Hello");
    let identity = PackageIdentity::local(&folder).unwrap();
    assert!(
        view.details().contains(&format!("Source: {identity}")),
        "{:?}",
        view.details()
    );
    assert!(
        view.details().contains(&"Version: 1.0.0".to_owned()),
        "{:?}",
        view.details()
    );
    assert!(
        view.details()
            .iter()
            .any(|line| line.starts_with("Compatible:")),
        "{:?}",
        view.details()
    );
    assert_eq!(titles(&launcher), ["Install"]);

    block_on(launcher.activate_selected());

    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Root { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(
        titles(&launcher),
        [
            "Say hello",
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            MANAGE_ROW,
            SETTINGS_ROW
        ]
    );
    assert_eq!(view.selected, Some(0));
    assert_eq!(view.status, Status::Result("Installed Hello".into()));
    // Row ids come from the identity's stable key, not its display text.
    assert_eq!(view.rows[0].id, format!("{}#hello", identity.key()));
    assert!(!view.rows[0].id.contains("local folder"));
    assert!(!launcher.selected_asks_for_folder());

    block_on(launcher.activate_selected());
    assert_eq!(launcher.view().title, "Rust sample");
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(&launcher),
        Status::Result("Hello from the Rust guest".into())
    );
}

fn error(launcher: &Launcher) -> String {
    match launcher.view().status {
        Status::Error(message) => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

/// (identity, title, version) of every installed package.
fn installed(launcher: &Launcher) -> Vec<(PackageIdentity, String, Option<String>)> {
    launcher
        .packages()
        .into_iter()
        .map(|package| (package.identity.clone(), package.title(), package.version()))
        .collect()
}

#[test]
fn a_second_explicit_install_of_the_same_folder_is_rejected() {
    let dirs = Dirs::new();
    let folder = package(&dirs.source("hello"), "Hello", "1.0.0", "sample_rust");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Hello".into())
    );

    block_on(launcher.install_package(&folder));

    let message = error(&launcher);
    assert!(
        message.starts_with("Already installed from local folder"),
        "{message}"
    );
    assert_eq!(installed(&launcher).len(), 1);
    assert_eq!(
        titles(&launcher),
        [
            "Say hello",
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            MANAGE_ROW,
            SETTINGS_ROW
        ]
    );
}

#[test]
fn a_new_version_in_the_same_folder_is_an_update_of_the_tracked_package() {
    let dirs = Dirs::new();
    let folder = package(&dirs.source("hello"), "Hello", "1.0.0", "sample_rust");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    // The author renames the package and builds a different component.
    package(&folder, "Hello renamed", "2.0.0", "sample_ts");

    block_on(launcher.preview_package(&folder));
    let view = launcher.view();
    assert_eq!(titles(&launcher), ["Update"]);
    assert!(
        view.details()
            .contains(&"Installed: version 1.0.0 from this folder".to_owned()),
        "{:?}",
        view.details()
    );

    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        Status::Result("Updated Hello renamed to 2.0.0".into())
    );
    let identity = PackageIdentity::local(&folder).unwrap();
    assert_eq!(
        installed(&launcher),
        [(identity, "Hello renamed".into(), Some("2.0.0".into()))]
    );
    block_on(launcher.activate_selected());
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(&launcher),
        Status::Result("Hello from the TypeScript guest".into())
    );
}

#[test]
fn copies_in_different_folders_are_distinct_packages_despite_the_same_title() {
    let dirs = Dirs::new();
    let published = package(&dirs.source("published"), "Hello", "1.0.0", "sample_rust");
    let development = package(&dirs.source("development"), "Hello", "1.0.0", "sample_js");
    let launcher = dirs.launcher();

    block_on(launcher.install_package(&published));
    block_on(launcher.install_package(&development));

    let identities: Vec<PackageIdentity> = installed(&launcher)
        .into_iter()
        .map(|(id, ..)| id)
        .collect();
    assert_eq!(
        identities,
        [
            PackageIdentity::local(&published).unwrap(),
            PackageIdentity::local(&development).unwrap()
        ]
    );
    assert_eq!(
        titles(&launcher),
        [
            "Say hello",
            "Say hello",
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            MANAGE_ROW,
            SETTINGS_ROW
        ]
    );
    // Each runs its own copy.
    for (index, answer) in [(0, "Rust"), (1, "JavaScript")] {
        launcher.back();
        launcher.select(index);
        block_on(launcher.activate_selected());
        block_on(launcher.activate_selected());
        assert_eq!(
            shown(&launcher),
            Status::Result(format!("Hello from the {answer} guest"))
        );
    }
}

/// Every file under `dir` with its contents, sorted by path.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(snapshot(&path));
        } else {
            files.push((path.clone(), fs::read(&path).unwrap()));
        }
    }
    files.sort();
    files
}

#[test]
fn installing_leaves_the_source_folder_untouched_and_runs_a_managed_copy() {
    let dirs = Dirs::new();
    let folder = package(&dirs.source("hello"), "Hello", "1.0.0", "sample_rust");
    fs::write(folder.join("notes.txt"), "the author's own file").unwrap();
    let before = snapshot(&folder);
    let launcher = dirs.launcher();

    block_on(launcher.install_package(&folder));
    package(&folder, "Hello", "1.0.1", "sample_rust");
    block_on(launcher.preview_package(&folder));
    block_on(launcher.activate_selected());

    let mut after = snapshot(&folder);
    after.retain(|(path, _)| !path.ends_with("pane.json"));
    let mut expected = before;
    expected.retain(|(path, _)| !path.ends_with("pane.json"));
    assert_eq!(after, expected, "only the author changed the folder");
    let managed = &launcher.packages()[0].location;
    assert!(!managed.starts_with(&folder));
    assert!(
        !managed.join("notes.txt").exists(),
        "only package files are copied"
    );

    // The installed copy does not depend on the source folder.
    fs::remove_dir_all(&folder).unwrap();
    block_on(launcher.activate_selected());
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(&launcher),
        Status::Result("Hello from the Rust guest".into())
    );
}

#[test]
fn installed_commands_are_listed_after_a_restart_without_running_any_guest() {
    let dirs = Dirs::new();
    let folder = package(&dirs.source("hello"), "Hello", "1.0.0", "sample_rust");
    block_on(dirs.launcher().install_package(&folder));

    // A runtime that cannot start runs no guest at all.
    let unavailable = Err(CallError::RuntimeUnavailable("no engine".into()));
    let restarted =
        Launcher::with_packages(unavailable, vec![], dirs.data.path().join("extensions"));

    let view = restarted.view();
    assert_eq!(
        titles(&restarted),
        [
            "Say hello",
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            MANAGE_ROW,
            SETTINGS_ROW
        ]
    );
    assert_eq!(view.rows[0].subtitle.as_deref(), Some("Greets you"));
    assert_eq!(
        installed(&restarted),
        [(
            PackageIdentity::local(&folder).unwrap(),
            "Hello".into(),
            Some("1.0.0".into())
        )]
    );
}

/// Builds one unsupported package folder in `folder`.
type Unsupported = fn(&Path);

fn with_manifest(folder: &Path, manifest: &str) {
    fs::create_dir_all(folder).unwrap();
    fs::write(folder.join("pane.json"), manifest).unwrap();
}

/// (case, how to build it, what the explanation must say)
/// Explains a component built for another shape of extension API 0.1.
const OLDER_SHAPE: &str = "it was built for an older extension API shape: rebuild it against Pane's current extension API 0.1";

const UNSUPPORTED: [(&str, Unsupported, &str); 11] = [
    ("no folder", |_| {}, "Cannot open"),
    (
        "no manifest",
        |folder| fs::create_dir_all(folder).unwrap(),
        "Not an extension package: ",
    ),
    (
        "malformed manifest",
        |folder| with_manifest(folder, "{ not json"),
        "Invalid pane.json: ",
    ),
    (
        "newer manifest format",
        |folder| with_manifest(folder, r#"{ "manifestVersion": 2, "whatever": true }"#),
        "uses manifest version 2, but this Pane reads version 1; a newer Pane is needed",
    ),
    (
        "unsupported extension API",
        |folder| {
            package(folder, "Hello", "1.0.0", "sample_rust");
            let manifest = manifest("Hello", "1.0.0", "hello.wasm").replace("0.1", "0.2");
            with_manifest(folder, &manifest);
        },
        "it needs Pane extension API 0.2, but this Pane provides 0.1",
    ),
    (
        "source-only package",
        |folder| with_manifest(folder, &manifest("Hello", "1.0.0", "dist/hello.wasm")),
        "the component dist/hello.wasm of \"Say hello\" is missing. This looks like a source-only package",
    ),
    (
        "component outside the package",
        |folder| with_manifest(folder, &manifest("Hello", "1.0.0", "../hello.wasm")),
        "component `../hello.wasm` must be a relative path inside the package folder",
    ),
    (
        "WASI 0.2 component",
        |folder| {
            package(folder, "Hello", "1.0.0", "mixed_p2");
        },
        "Pane supports only WASI 0.3, but it imports wasi:io/poll@0.2",
    ),
    (
        // From before #19 and #21: exports are missing.
        "older API shape",
        |folder| {
            package(folder, "Hello", "1.0.0", "old_api");
        },
        OLDER_SHAPE,
    ),
    (
        // Every export is there by name, but a record differs in type.
        "mismatched API shape",
        |folder| {
            package(folder, "Hello", "1.0.0", "mismatched_api");
        },
        OLDER_SHAPE,
    ),
    (
        "not a component",
        |folder| {
            package(folder, "Hello", "1.0.0", "sample_rust");
            fs::write(folder.join("hello.wasm"), "text, not WebAssembly").unwrap();
        },
        "\"Say hello\": Could not load the extension",
    ),
];

#[test]
fn unsupported_packages_are_explained_and_not_installed() {
    for (case, build, explanation) in UNSUPPORTED {
        let dirs = Dirs::new();
        let folder = dirs.source("a package");
        build(&folder);
        let launcher = dirs.launcher();

        block_on(launcher.preview_package(&folder));
        let view = launcher.view();
        assert!(matches!(view.screen, Screen::Package { .. }), "{case}");
        assert!(view.rows.is_empty(), "{case}: nothing to install");
        let message = error(&launcher);
        assert!(message.contains(explanation), "{case}: {message}");

        block_on(launcher.install_package(&folder));
        assert!(error(&launcher).contains(explanation), "{case}");
        assert!(launcher.packages().is_empty(), "{case}");

        launcher.back();
        assert_eq!(
            titles(&launcher),
            [INSTALL_ROW, NPM_ROW, GIT_ROW, SETTINGS_ROW],
            "{case}"
        );
    }
}

#[test]
fn a_folder_path_with_spaces_and_unicode_is_its_identity() {
    let dirs = Dirs::new();
    let relative = Path::new("Mes extensions ü 日本").join("héllo pkg");
    let folder = package(
        &dirs.source("").join(&relative),
        "Hello",
        "1.0.0",
        "sample_rust",
    );
    let launcher = dirs.launcher();

    block_on(launcher.install_package(&folder));

    let identity = &launcher.packages()[0].identity;
    let path = identity.local_folder().unwrap();
    assert!(
        path.is_absolute() && path.ends_with(&relative),
        "{identity}"
    );
    assert!(
        !path.to_string_lossy().starts_with(r"\\?\"),
        "no Windows verbatim prefix: {identity}"
    );
    block_on(launcher.activate_selected());
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(&launcher),
        Status::Result("Hello from the Rust guest".into())
    );
}

/// Creates a directory symbolic link, or `None` where the OS does not allow
/// it (Windows without Developer Mode or administrator rights).
fn symlink_dir(target: &Path, link: &Path) -> Option<()> {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(target, link);
    made.ok()
}

#[test]
fn a_symbolic_link_identifies_the_folder_it_points_to() {
    let dirs = Dirs::new();
    let folder = package(&dirs.source("hello"), "Hello", "1.0.0", "sample_rust");
    let link = dirs.source("link to hello");
    let Some(()) = symlink_dir(&folder, &link) else {
        eprintln!("skipped: this system does not allow directory symbolic links");
        return;
    };
    let launcher = dirs.launcher();

    block_on(launcher.install_package(&link));
    assert_eq!(
        launcher.packages()[0].identity,
        PackageIdentity::local(&folder).unwrap()
    );

    block_on(launcher.install_package(&folder));
    assert!(error(&launcher).starts_with("Already installed"));
}

/// Two spellings of one folder name: the same package where the file system
/// treats them as one folder, otherwise two packages. Pane itself folds
/// neither case nor Unicode normalization.
fn spellings_follow_the_file_system(first: &str, second: &str) {
    let dirs = Dirs::new();
    let folder = package(&dirs.source(first), "Hello", "1.0.0", "sample_rust");
    let other = dirs.source(second);
    let same_folder = other.exists();
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));

    if same_folder {
        assert_eq!(
            PackageIdentity::local(&other).unwrap(),
            PackageIdentity::local(&folder).unwrap()
        );
        block_on(launcher.install_package(&other));
        assert!(error(&launcher).starts_with("Already installed"));
        assert_eq!(launcher.packages().len(), 1);
    } else {
        package(&other, "Hello", "1.0.0", "sample_rust");
        block_on(launcher.install_package(&other));
        assert_eq!(launcher.packages().len(), 2);
    }
}

#[test]
fn a_damaged_installed_copy_is_listed_with_its_problem_and_others_still_run() {
    let dirs = Dirs::new();
    let broken = package(&dirs.source("broken"), "Broken", "1.0.0", "sample_rust");
    let working = package(&dirs.source("working"), "Working", "1.0.0", "sample_rust");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&broken));
    block_on(launcher.install_package(&working));
    fs::write(launcher.packages()[0].location.join("pane.json"), "{").unwrap();

    let restarted = dirs.launcher();

    assert_eq!(
        titles(&restarted),
        [
            "Say hello",
            "broken",
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            MANAGE_ROW,
            SETTINGS_ROW
        ]
    );
    restarted.select(1);
    block_on(restarted.activate_selected());
    let message = error(&restarted);
    assert!(message.starts_with("broken cannot load from"), "{message}");
    assert!(message.contains("Invalid pane.json"), "{message}");
    restarted.select(0);
    block_on(restarted.activate_selected());
    assert_eq!(restarted.view().screen, Screen::Command);
}

#[test]
fn an_unreadable_install_record_is_explained_and_never_overwritten() {
    let dirs = Dirs::new();
    let folder = package(&dirs.source("hello"), "Hello", "1.0.0", "sample_rust");
    let record = dirs.data.path().join("extensions").join("installed.json");
    fs::create_dir_all(record.parent().unwrap()).unwrap();
    fs::write(&record, "not a record").unwrap();

    let launcher = dirs.launcher();
    assert!(error(&launcher).starts_with("Cannot read Pane's installed extensions"));

    block_on(launcher.install_package(&folder));
    assert!(error(&launcher).starts_with("Could not update Pane's installed extensions"));
    assert_eq!(fs::read_to_string(&record).unwrap(), "not a record");
}

/// A launcher whose build offers the JavaScript sample command.
fn launcher_with_build_command(dirs: &Dirs) -> Launcher {
    let command = CommandRegistration {
        id: "javascript-sample".into(),
        title: "JavaScript sample".into(),
        subtitle: None,
        component: guest("sample_js"),
        takes_query: false,
        search: false,
        keywords: Vec::new(),
        when: pane_core::CommandWhen::Always,
        matches: pane_core::CommandMatches::Title,
    };
    Launcher::with_packages(
        Runtime::start(),
        vec![command],
        dirs.data.path().join("extensions"),
    )
}

fn selected_title(launcher: &Launcher) -> Option<String> {
    let view = launcher.view();
    Some(view.rows.get(view.selected?)?.title.clone())
}

#[test]
fn an_install_finishing_in_the_background_keeps_a_command_the_user_is_opening() {
    let dirs = Dirs::new();
    let folder = package(&dirs.source("hello"), "Hello", "1.0.0", "sample_rust");
    let launcher = launcher_with_build_command(&dirs);
    block_on(launcher.preview_package(&folder));
    let installing = launcher.activate_selected();
    // The user leaves the preview and opens a command before it finishes.
    launcher.back();
    launcher.select(0);
    let opening = launcher.activate_selected();

    block_on(installing);
    block_on(opening);

    let view = launcher.view();
    assert_eq!(
        (view.screen, view.title.as_str()),
        (Screen::Command, "JavaScript sample")
    );
}

#[test]
fn an_install_finishing_in_the_background_keeps_the_selected_row() {
    let dirs = Dirs::new();
    let folder = package(&dirs.source("hello"), "Hello", "1.0.0", "sample_rust");
    let launcher = launcher_with_build_command(&dirs);
    block_on(launcher.preview_package(&folder));
    let installing = launcher.activate_selected();
    launcher.back();
    launcher.select(1);
    assert_eq!(selected_title(&launcher).as_deref(), Some(INSTALL_ROW));

    block_on(installing);

    assert_eq!(
        titles(&launcher),
        [
            "JavaScript sample",
            "Say hello",
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            MANAGE_ROW,
            SETTINGS_ROW
        ]
    );
    assert_eq!(selected_title(&launcher).as_deref(), Some(INSTALL_ROW));
}

/// Sets the permission bits of `path` (Unix).
#[cfg(unix)]
fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(unix)]
#[test]
fn a_replaced_copy_that_could_not_be_removed_is_removed_at_the_next_start() {
    let dirs = Dirs::new();
    let folder = package(&dirs.source("hello"), "Hello", "1.0.0", "sample_rust");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    let old = launcher.packages()[0].location.clone();
    // Its files cannot be deleted, as on Windows while a file is in use.
    chmod(&old, 0o555);
    if fs::remove_file(old.join("pane.json")).is_ok() {
        // Permissions do not apply (running as root): nothing to test.
        return;
    }
    package(&folder, "Hello", "2.0.0", "sample_ts");
    block_on(launcher.preview_package(&folder));
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().status,
        Status::Result("Updated Hello to 2.0.0".into())
    );
    assert!(old.exists(), "the replaced copy is left behind");
    let current = launcher.packages()[0].location.clone();
    let current_files = snapshot(&current);
    // A copy the install record never listed, as after a lost record.
    let unlisted = old.with_file_name("99");
    fs::create_dir_all(&unlisted).unwrap();
    chmod(&old, 0o755);

    let restarted = dirs.launcher();

    assert!(!old.exists(), "the left-behind copy is removed");
    assert_eq!(
        snapshot(&current),
        current_files,
        "the installed copy is kept"
    );
    assert!(unlisted.exists(), "a copy never recorded is not touched");
    assert_eq!(restarted.packages()[0].location, current);
    // Once removed, it is not tried again.
    let again = dirs.launcher();
    assert_eq!(again.packages()[0].location, current);
}

#[test]
fn installing_after_the_install_record_is_lost_keeps_existing_managed_copies() {
    let dirs = Dirs::new();
    let first = package(&dirs.source("first"), "First", "1.0.0", "sample_rust");
    let second = package(&dirs.source("second"), "Second", "1.0.0", "sample_js");
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&first));
    let kept = launcher.packages()[0].location.clone();
    let before = snapshot(&kept);
    fs::remove_file(dirs.data.path().join("extensions").join("installed.json")).unwrap();

    let restarted = dirs.launcher();
    block_on(restarted.install_package(&second));

    assert_eq!(
        restarted.view().status,
        Status::Result("Installed Second".into())
    );
    assert_ne!(restarted.packages()[0].location, kept);
    assert_eq!(snapshot(&kept), before, "the earlier copy is not replaced");
}

#[test]
fn letter_case_follows_the_file_system() {
    spellings_follow_the_file_system("HelloPkg", "hellopkg");
}

#[test]
fn unicode_normalization_follows_the_file_system() {
    // "café" composed (NFC) and decomposed (NFD).
    spellings_follow_the_file_system("caf\u{e9}", "cafe\u{301}");
}

fn repository(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

#[test]
fn the_assembled_sample_packages_install_and_run_in_every_language() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    for (index, (name, language)) in [
        ("sample-rust", "Rust"),
        ("sample-js", "JavaScript"),
        ("sample-ts", "TypeScript"),
    ]
    .into_iter()
    .enumerate()
    {
        let folder = repository("target/guests/packages").join(name);
        assert!(
            folder.exists(),
            "{} is missing; run `cargo xtask guests`",
            folder.display()
        );

        block_on(launcher.install_package(&folder));

        let title = format!("{language} sample");
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {title}"))
        );
        assert_eq!(launcher.view().selected, Some(index));
        block_on(launcher.activate_selected());
        assert_eq!(launcher.view().title, title);
        block_on(launcher.activate_selected());
        assert_eq!(
            shown(&launcher),
            Status::Result(format!("Hello from the {language} guest"))
        );
    }
}

#[test]
fn a_sample_package_source_without_its_built_component_is_explained() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&repository("guests/packages/sample-rust")));

    assert!(error(&launcher).contains("This looks like a source-only package"));
}

/// Writes the Hello package declaring `platforms` (a JSON array).
fn package_for(folder: &Path, platforms: &str) -> PathBuf {
    package(folder, "Hello", "1.0.0", "sample_rust");
    let manifest = manifest("Hello", "1.0.0", "hello.wasm").replace(
        r#""apiVersion": "0.1","#,
        &format!(r#""apiVersion": "0.1", "platforms": {platforms},"#),
    );
    with_manifest(folder, &manifest);
    folder.to_path_buf()
}

/// A JSON array of the `pane.json` names of `systems`.
fn json_list(systems: &[Platform]) -> String {
    let ids: Vec<String> = systems
        .iter()
        .map(|platform| format!(r#""{}""#, platform.id()))
        .collect();
    format!("[{}]", ids.join(", "))
}

#[test]
fn a_package_only_for_other_systems_is_explained_and_not_installed() {
    let dirs = Dirs::new();
    let folder = package_for(
        &dirs.source("elsewhere"),
        &json_list(&platforms::other_systems()),
    );
    let launcher = dirs.launcher();
    let explanation = platforms::only("this package", &platforms::other_names());

    block_on(launcher.preview_package(&folder));
    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Package { .. }),
        "{:?}",
        view.screen
    );
    assert!(view.rows.is_empty(), "nothing to install");
    assert_eq!(error(&launcher), explanation);

    block_on(launcher.install_package(&folder));
    assert_eq!(error(&launcher), explanation);
    assert!(launcher.packages().is_empty());
}

#[test]
fn a_package_declaring_no_system_is_explained_and_not_installed() {
    let dirs = Dirs::new();
    let folder = package_for(&dirs.source("nowhere"), "[]");
    let launcher = dirs.launcher();
    let explanation = platforms::nowhere("this package");

    block_on(launcher.preview_package(&folder));
    assert_eq!(error(&launcher), explanation);

    block_on(launcher.install_package(&folder));
    assert_eq!(error(&launcher), explanation);
    assert!(launcher.packages().is_empty());
}

#[test]
fn a_package_for_this_system_shows_its_systems_and_installs() {
    let dirs = Dirs::new();
    let [other, _] = platforms::other_systems();
    let folder = package_for(
        &dirs.source("here"),
        &json_list(&[other, platforms::this_system()]),
    );
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&folder));
    let view = launcher.view();
    let details = view.details();
    let line = details
        .iter()
        .find(|line| line.starts_with("Supported systems: "))
        .unwrap_or_else(|| panic!("{details:?}"));
    assert_eq!(
        line,
        &format!(
            "Supported systems: {} and {} (this system)",
            platforms::name(other),
            platforms::name(platforms::this_system())
        )
    );
    block_on(launcher.activate_selected());

    assert_eq!(
        launcher.view().status,
        Status::Result("Installed Hello".into())
    );
    assert_eq!(
        titles(&launcher),
        [
            "Say hello",
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            MANAGE_ROW,
            SETTINGS_ROW
        ]
    );
}

#[test]
fn an_installed_copy_for_other_systems_lists_its_commands_as_unavailable() {
    let dirs = Dirs::new();
    let this = platforms::this_system();
    let [other, _] = platforms::other_systems();
    let folder = package_for(&dirs.source("hello"), &json_list(&[this]));
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    // As if the data folder had been copied from another system.
    let copy = launcher.packages()[0].location.join("pane.json");
    let text = fs::read_to_string(&copy).unwrap();
    fs::write(&copy, text.replace(this.id(), other.id())).unwrap();
    let explanation = platforms::only("this package", platforms::name(other));

    let restarted = dirs.launcher();

    assert_eq!(
        titles(&restarted),
        [
            "Say hello",
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            MANAGE_ROW,
            SETTINGS_ROW
        ]
    );
    let row = restarted.view().rows[0].clone();
    assert_eq!(row.subtitle.as_deref(), Some("Greets you"));
    assert_eq!(
        row.unavailable,
        Some(Unavailable::OnThisSystem(explanation.clone()))
    );
    block_on(restarted.activate_selected());
    let view = restarted.view();
    assert_eq!(
        (view.query(), &view.status),
        (Some(""), &Status::Error(explanation))
    );
}

/// Writes a package whose three commands, all backed by the Rust sample,
/// declare the other systems, this one, and none at all.
fn package_with_command_platforms(folder: &Path) -> PathBuf {
    package(folder, "Hello", "1.0.0", "sample_rust");
    let manifest = format!(
        r#"{{
  "manifestVersion": 1,
  "title": "Hello",
  "apiVersion": "0.1",
  "commands": [
    {{ "id": "there", "title": "Elsewhere", "platforms": {there}, "component": "hello.wasm" }},
    {{ "id": "here", "title": "Here", "platforms": {here}, "component": "hello.wasm" }},
    {{ "id": "nowhere", "title": "Nowhere", "platforms": [], "component": "hello.wasm" }}
  ]
}}"#,
        there = json_list(&platforms::other_systems()),
        here = json_list(&[platforms::this_system()]),
    );
    with_manifest(folder, &manifest);
    folder.to_path_buf()
}

#[test]
fn a_command_for_other_systems_is_listed_with_its_reason_and_others_still_open() {
    let dirs = Dirs::new();
    let folder = package_with_command_platforms(&dirs.source("hello"));
    let launcher = dirs.launcher();
    block_on(launcher.install_package(&folder));
    let elsewhere = platforms::only("this command", &platforms::other_names());
    let nowhere = platforms::nowhere("this command");

    for launcher in [launcher, dirs.launcher()] {
        let reasons: Vec<(String, Option<String>)> = launcher
            .view()
            .rows
            .into_iter()
            .map(|row| (row.title, row.unavailable.map(|u| u.reason().to_owned())))
            .collect();
        assert_eq!(
            reasons,
            [
                ("Elsewhere".into(), Some(elsewhere.clone())),
                ("Here".into(), None),
                ("Nowhere".into(), Some(nowhere.clone())),
                (INSTALL_ROW.into(), None),
                (NPM_ROW.into(), None),
                (GIT_ROW.into(), None),
                (MANAGE_ROW.into(), None),
                (SETTINGS_ROW.into(), None),
            ]
        );

        for (index, reason) in [(0, &elsewhere), (2, &nowhere)] {
            launcher.select(index);
            block_on(launcher.activate_selected());
            let view = launcher.view();
            assert_eq!(
                (view.query(), &view.status),
                (Some(""), &Status::Error(reason.clone()))
            );
        }
        launcher.select(1);
        block_on(launcher.activate_selected());
        assert_eq!(launcher.view().screen, Screen::Command);
    }
}

#[test]
fn a_platform_list_pane_does_not_know_is_an_invalid_manifest() {
    for (platforms, explanation) in [
        (r#"["windows", "beos"]"#, "unknown platform `beos`"),
        (r#""linux""#, "invalid type"),
    ] {
        let dirs = Dirs::new();
        let folder = package_for(&dirs.source("hello"), platforms);
        let launcher = dirs.launcher();

        block_on(launcher.preview_package(&folder));

        let message = error(&launcher);
        assert!(message.starts_with("Invalid pane.json: "), "{message}");
        assert!(message.contains(explanation), "{platforms}: {message}");
    }
}

#[test]
fn a_command_platform_list_pane_does_not_know_is_an_invalid_manifest() {
    let dirs = Dirs::new();
    let folder = package(&dirs.source("hello"), "Hello", "1.0.0", "sample_rust");
    let manifest = manifest("Hello", "1.0.0", "hello.wasm")
        .replace(r#""component""#, r#""platforms": ["beos"], "component""#);
    with_manifest(&folder, &manifest);
    let launcher = dirs.launcher();

    block_on(launcher.preview_package(&folder));

    let message = error(&launcher);
    assert!(message.starts_with("Invalid pane.json: "), "{message}");
    assert!(
        message.contains("unknown platform `beos` in `platforms` of command `hello`"),
        "{message}"
    );
}

/// Installs the Rust sample as package "Hello" on `runtime`, opens its
/// command and its color picker, and returns the launcher and package folder.
fn installed_color_view(dirs: &Dirs, runtime: &Runtime) -> (Launcher, PathBuf) {
    let folder = package(&dirs.source("hello"), "Hello", "1.0.0", "sample_rust");
    let launcher = Launcher::with_packages(
        Ok(runtime.clone()),
        vec![],
        dirs.data.path().join("extensions"),
    );
    block_on(launcher.install_package(&folder));
    open_installed_color_view(&launcher);
    (launcher, folder)
}

/// From root search, opens the installed command and its color picker.
fn open_installed_color_view(launcher: &Launcher) {
    launcher.select(0);
    block_on(launcher.activate_selected());
    let color = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.title == "Choose a color")
        .expect("the color item is listed");
    launcher.select(color);
    block_on(launcher.activate_selected());
    assert!(
        matches!(launcher.view().screen, Screen::CustomView(_)),
        "{:?}",
        launcher.view().screen
    );
}

#[test]
fn an_update_finishing_while_its_view_is_open_closes_the_view_at_once() {
    let dirs = Dirs::new();
    let runtime = Runtime::start().unwrap();
    let (launcher, folder) = installed_color_view(&dirs, &runtime);
    launcher.back();
    launcher.back();
    package(&folder, "Hello", "2.0.0", "sample_rust");
    block_on(launcher.preview_package(&folder));
    let updating = launcher.activate_selected();
    // The user leaves the preview and opens the old copy's view meanwhile.
    launcher.back();
    open_installed_color_view(&launcher);

    block_on(updating);

    let view = launcher.view();
    assert_eq!(
        (view.query(), &view.status),
        (Some(""), &Status::Result("Updated Hello to 2.0.0".into()))
    );
    assert_eq!(block_on(runtime.view_count()), 0);
}

#[test]
fn disabling_a_package_closes_its_open_view_at_once() {
    let dirs = Dirs::new();
    let runtime = Runtime::start().unwrap();
    let (launcher, folder) = installed_color_view(&dirs, &runtime);
    let identity = PackageIdentity::local(&folder).unwrap();

    block_on(launcher.set_enabled(&identity, false));

    let view = launcher.view();
    assert_eq!(
        (view.query(), &view.status),
        (Some(""), &Status::Result("Disabled Hello".into()))
    );
    assert_eq!(block_on(runtime.view_count()), 0);
}
