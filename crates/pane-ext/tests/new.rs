//! `pane-ext new` as a process (#221): the flags write a package without
//! a terminal to ask, an existing folder is written into only while
//! empty, and `new command` adds a command to a package pane-ext itself
//! wrote. The prompts the interactive terminal gets are covered by the
//! unit tests on them (see `src/new.rs`); here stdin is not a terminal,
//! which is what a script — or `npm create @pane-app` — runs with.
//! Building and installing what every template writes is the templates
//! test's.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

/// Runs `pane-ext new` with `args`, returning whether it succeeded and
/// what it printed, standard output and error together.
fn new(args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_pane-ext"))
        .arg("new")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), printed)
}

/// The files under `folder`, relative paths in order.
fn files_under(folder: &Path) -> Vec<String> {
    fn walk(folder: &Path, prefix: String, files: &mut Vec<String>) {
        for entry in fs::read_dir(folder).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = format!("{prefix}{name}");
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), format!("{path}/"), files);
            } else {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(folder, String::new(), &mut files);
    files.sort();
    files
}

#[test]
fn the_flags_write_a_package_without_a_terminal() {
    let folder = tempfile::tempdir().unwrap();
    let package = folder.path().join("notes");
    let (passed, printed) = new(&[
        package.to_str().unwrap(),
        "--name",
        "Word Count",
        "--language",
        "rust",
        "--template",
        "form",
    ]);
    assert!(passed, "{printed}");
    assert!(
        printed.contains("wrote Word Count (the form template, in Rust) into"),
        "{printed}"
    );
    assert!(
        files_under(&package).contains(&"src/lib.rs".to_owned()),
        "{:?}",
        files_under(&package)
    );
    // The manifest carries the title and the command the name became.
    let manifest = fs::read_to_string(package.join("pane.json")).unwrap();
    assert!(manifest.contains("\"title\": \"Word Count\""), "{manifest}");
    assert!(manifest.contains("\"id\": \"word-count\""), "{manifest}");
    assert!(
        manifest.contains("$schema"),
        "the manifest names a schema an editor checks"
    );
    assert!(package.join("icon.png").is_file());
    // The same scaffold, with the name taken from the folder.
    let other = folder.path().join("word-count");
    let (passed, printed) = new(&[other.to_str().unwrap(), "--language", "typescript"]);
    assert!(passed, "{printed}");
    assert!(
        printed.contains("wrote Word Count (the list template, in TypeScript)"),
        "{printed}"
    );
    assert!(other.join("package.json").is_file());
    let package = fs::read_to_string(other.join("package.json")).unwrap();
    assert!(package.contains("\"dev\": \"pane-ext dev\""), "{package}");
    assert!(package.contains("@pane-app/cli"), "{package}");
    assert!(package.contains("@pane-app/extension"), "{package}");
}

#[test]
fn an_argument_that_is_not_a_choice_fails_with_the_choices() {
    let (passed, printed) = new(&["--language", "javascript"]);
    assert!(!passed, "{printed}");
    assert!(
        printed.contains("`javascript` is not a --language choice: rust, typescript"),
        "{printed}"
    );
    let (passed, printed) = new(&["--template", "table"]);
    assert!(!passed, "{printed}");
    assert!(
        printed.contains("`table` is not a --template choice: list, detail, form, no-view"),
        "{printed}"
    );
}

#[test]
fn a_folder_that_is_not_empty_is_refused() {
    let folder = tempfile::tempdir().unwrap();
    fs::write(folder.path().join("keep me"), "an author's file").unwrap();
    let (passed, printed) = new(&[folder.path().to_str().unwrap()]);
    assert!(!passed, "{printed}");
    assert!(printed.contains("is not empty"), "{printed}");
    // An empty folder is written into.
    let empty = tempfile::tempdir().unwrap();
    let (passed, printed) = new(&[empty.path().to_str().unwrap()]);
    assert!(passed, "{printed}");
    assert!(empty.path().join("pane.json").is_file());
}

#[test]
fn without_a_terminal_new_needs_a_folder_or_a_name() {
    let (passed, printed) = new(&[]);
    assert!(!passed, "{printed}");
    assert!(
        printed.contains("new needs a package folder, or a --name"),
        "{printed}"
    );
}

#[test]
fn new_command_adds_a_command_to_a_package_pane_ext_wrote() {
    let folder = tempfile::tempdir().unwrap();
    let (passed, printed) = new(&[folder.path().to_str().unwrap()]);
    assert!(passed, "{printed}");
    let (passed, printed) = new(&[
        "command",
        folder.path().to_str().unwrap(),
        "--id",
        "note",
        "--template",
        "detail",
    ]);
    assert!(passed, "{printed}");
    assert!(
        printed.contains("added the command `note` (the detail template)"),
        "{printed}"
    );
    let manifest = fs::read_to_string(folder.path().join("pane.json")).unwrap();
    assert!(manifest.contains("\"id\": \"note\""), "{manifest}");
    assert!(
        manifest.contains("\"takesQuery\": true"),
        "the detail template's command takes a query: {manifest}"
    );
    assert!(folder.path().join("src/note.ts").is_file());
    let entry = fs::read_to_string(folder.path().join("src/index.ts")).unwrap();
    assert!(
        entry.contains("case \"note\": return note.render(launch);"),
        "{entry}"
    );

    // A second command of the same id is refused, leaving the package as
    // it was.
    let (passed, printed) = new(&[
        "command",
        folder.path().to_str().unwrap(),
        "--id",
        "note",
        "--template",
        "form",
    ]);
    assert!(!passed, "{printed}");
    assert!(
        printed.contains("already has a command named `note`"),
        "{printed}"
    );
}

#[test]
fn new_command_refuses_a_folder_that_is_no_package() {
    let folder = tempfile::tempdir().unwrap();
    let (passed, printed) = new(&["command", folder.path().to_str().unwrap()]);
    assert!(!passed, "{printed}");
    assert!(printed.contains("is not a package"), "{printed}");

    let (passed, printed) = new(&["command"]);
    assert!(!passed, "{printed}");
    assert!(
        printed.contains("new command needs the folder"),
        "{printed}"
    );
}
