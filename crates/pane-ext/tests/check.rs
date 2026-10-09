//! `pane-ext check` as a process (#224), against the packages `cargo xtask
//! guests` assembles and folders built around the same guest components the
//! pane-core tests use: a valid sample passes; an invalid one fails with
//! Pane's own messages; the same invalid package fed to Pane's install
//! preview and to `pane-ext check` is refused with the same message (the
//! parity the spec asks for); each lint rule has a fixture that triggers it;
//! and `--json` prints a report whose contents are stable. The package's own
//! eslint runs only where npm is found, so that leg is covered by the unit
//! tests on its detection and invocation (see `src/check.rs`); CI's runners
//! install Node only where a job asks for it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use futures::executor::block_on;
use pane_core::{Launcher, Runtime, Status};

/// The guest components and packages `cargo xtask guests` puts in
/// `target/guests`.
fn guests() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests")
}

/// The file `file` in `target/guests`; panics if it was not built.
fn guest_file(file: &str) -> PathBuf {
    let path = guests().join(file);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

fn guest(name: &str) -> PathBuf {
    guest_file(&format!("{name}.wasm"))
}

/// A package folder for the manifest `manifest` with the guest `component`
/// copied in as `hello.wasm`.
fn package(folder: &Path, manifest: &str, component: &str) -> PathBuf {
    fs::create_dir_all(folder).unwrap();
    fs::write(folder.join("pane.json"), manifest).unwrap();
    fs::copy(guest(component), folder.join("hello.wasm")).unwrap();
    folder.to_path_buf()
}

/// A one-command manifest whose entry is spliced with `extra` fields.
fn manifest(extra: &str) -> String {
    format!(
        r#"{{
  "manifestVersion": 1,
  "title": "Hello",
  "version": "1.0.0",
  "apiVersion": "0.1",
  "commands": [
    {{
      "id": "hello",
      "title": "Say hello",
      "subtitle": "Greets you",
      "component": "hello.wasm"{extra}
    }}
  ]
}}"#
    )
}

/// Runs `pane-ext check` on `folder` with `args`, returning whether it
/// succeeded and what it printed, standard output and error together.
fn check(folder: &Path, args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_pane-ext"))
        .arg("check")
        .arg(folder)
        .args(args)
        .output()
        .unwrap();
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), printed)
}

/// What Pane says about `folder`: the error its install preview shows.
fn pane_refuses(folder: &Path) -> String {
    let data = tempfile::tempdir().unwrap();
    let launcher = Launcher::with_packages(
        Runtime::start().unwrap(),
        vec![],
        data.path().join("extensions"),
    );
    block_on(launcher.preview_package(folder));
    match launcher.view().status {
        Status::Error(message) => message,
        other => panic!("Pane accepted the package: {other:?}"),
    }
}

#[test]
fn a_valid_sample_package_passes() {
    let folder = guest_file("packages/sample-rust");
    let (passed, printed) = check(&folder, &[]);
    assert!(passed, "{printed}");
    assert!(
        printed.contains("is a package Pane would install"),
        "{printed}"
    );
    // The samples carry no metadata a published package would: warnings,
    // which do not fail the check.
    assert!(printed.contains("warning: "), "{printed}");
    assert!(!printed.contains("error: "), "{printed}");
}

#[test]
fn an_invalid_manifest_fails_with_panes_message() {
    let repeated = manifest(r#""#.replace(
        r#""commands": ["#,
        r#""commands": [
    { "id": "hello", "title": "Say hello", "component": "hello.wasm" },
"#,
    ));
    let keywords = format!(
        r#"{{ "manifestVersion": 1, "title": "Hello", "apiVersion": "0.1", "keywords": {}, "commands": [{{ "id": "hello", "title": "Say hello", "component": "hello.wasm" }}] }}"#,
        format!("{:?}", vec!["word"; 21])
    );
    let cases: [(&str, String, &str); 4] = [
        (
            "unknown mode",
            manifest(r#", "mode": "sometimes""#),
            "a command's `mode` is \"view\" (it opens a screen, the default), \"no-view\"",
        ),
        (
            "newer manifest",
            r#"{ "manifestVersion": 2, "whatever": true }"#.to_owned(),
            "uses manifest version 2, but this Pane reads version 1; a newer Pane is needed",
        ),
        (
            "repeated command id",
            repeated,
            "command id `hello` is repeated",
        ),
        (
            "too many keywords",
            keywords,
            "`keywords` lists 21 entries; at most 20",
        ),
    ];
    for (name, manifest, message) in cases {
        let sources = tempfile::tempdir().unwrap();
        let folder = package(&sources.path().join(name), &manifest, "sample_rust");
        let (passed, printed) = check(&folder, &[]);
        assert!(!passed, "{name}: {printed}");
        assert!(printed.contains("error: "), "{name}: {printed}");
        assert!(printed.contains(message), "{name}: {printed}");
    }
}

#[test]
fn the_same_invalid_package_is_refused_alike_by_pane_and_pane_ext_check() {
    // What Pane's install preview refuses is what `pane-ext check` reports:
    // the same fixtures, the same messages (the spec's parity rule).
    let sources = tempfile::tempdir().unwrap();
    let source_only = r#"{
  "manifestVersion": 1,
  "title": "Hello",
  "version": "1.0.0",
  "apiVersion": "0.1",
  "commands": [
    { "id": "hello", "title": "Say hello", "component": "dist/hello.wasm" }
  ]
}"#;
    let cases: [(&str, String, &str); 4] = [
        (
            "unknown mode",
            manifest(r#", "mode": "sometimes""#),
            "sample_rust",
        ),
        ("source-only package", source_only.to_owned(), "sample_rust"),
        ("older api shape", manifest(""), "old_api"),
        ("wasi 0.2 component", manifest(""), "mixed_p2"),
    ];
    for (name, manifest, component) in cases {
        let folder = package(&sources.path().join(name), &manifest, component);
        let pane = pane_refuses(&folder);
        let (passed, printed) = check(&folder, &[]);
        assert!(!passed, "{name}: {printed}");
        assert!(
            printed.contains(&pane),
            "{name}: pane said {pane:?}, check said:\n{printed}"
        );
    }
}

#[test]
fn every_lint_rule_has_a_fixture_that_triggers_it() {
    let sources = tempfile::tempdir().unwrap();
    // A package a published one would not look like: lowercase titles, no
    // icon, no description, nowhere to report problems, and a required
    // preference with no default and no help beside it.
    let manifest = r#"{
  "manifestVersion": 1,
  "title": "my little tool",
  "version": "1.0.0",
  "apiVersion": "0.1",
  "commands": [
    { "id": "go", "title": "run it", "component": "hello.wasm" }
  ],
  "preferences": [
    { "name": "key", "type": "text", "title": "Key", "required": true }
  ]
}"#;
    let folder = package(&sources.path().join("lints"), manifest, "sample_rust");
    let (passed, printed) = check(&folder, &[]);
    // Warnings only: the package installs.
    assert!(passed, "{printed}");
    for warning in [
        "the package's title is not in Title Case: \"My Little Tool\"",
        "the title of command `go` is not in Title Case: \"Run It\"",
        "the package has no icon of its own",
        "the required preference `key` has no default",
        "the package has no description",
        "the package declares neither `issues` nor `repository`",
    ] {
        assert!(printed.contains(warning), "missing {warning:?}:\n{printed}");
    }

    // The help warning goes when the package ships its HELP.md.
    fs::write(folder.join("HELP.md"), "# My Little Tool\n\nSet the key.\n").unwrap();
    let (_, printed) = check(&folder, &[]);
    assert!(
        !printed.contains("the required preference `key` has no default"),
        "{printed}"
    );

    // An icon smaller than a published extension's is warned about too.
    let sources = tempfile::tempdir().unwrap();
    let folder = package(
        &sources.path().join("small-icon"),
        &manifest(r#", "icon": "icon.png""#),
        "sample_rust",
    );
    fs::write(folder.join("icon.png"), TINY_PNG).unwrap();
    let (passed, printed) = check(&folder, &[]);
    assert!(passed, "{printed}");
    assert!(
        printed.contains("its icon icon.png is 1×1, smaller than the 512×512"),
        "{printed}"
    );

    // A helper target without its file: the helper sample package carries
    // only this system's file, as `cargo xtask guests` assembles it.
    let folder = guest_file("packages/sample-helper");
    let (passed, printed) = check(&folder, &[]);
    assert!(passed, "{printed}");
    assert!(
        printed.contains("whose file helpers/") && printed.contains("is not in the package"),
        "{printed}"
    );

    // Warnings fail the check only when they are asked to.
    let sources = tempfile::tempdir().unwrap();
    let folder = package(
        &sources.path().join("deny"),
        &manifest(r#", "mode": "no-view""#),
        "sample_rust",
    );
    let (passed, printed) = check(&folder, &["--deny-warnings"]);
    assert!(!passed, "{printed}");
    assert!(printed.contains("--deny-warnings"), "{printed}");
}

/// A 1×1 transparent PNG, to name an icon smaller than a published
/// extension's.
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

#[test]
fn the_json_report_is_stable() {
    let sources = tempfile::tempdir().unwrap();
    let folder = package(
        &sources.path().join("report"),
        &manifest(r#", "mode": "sometimes""#),
        "sample_rust",
    );
    let (passed, printed) = check(&folder, &["--json"]);
    assert!(!passed, "{printed}");
    let report: serde_json::Value = serde_json::from_str(printed.trim()).unwrap();

    // The report says what a tool needs: the shape's version, the folder,
    // whether the package passed, and every problem with its lint id, its
    // message and the file it is in.
    let expected = serde_json::json!({
        "version": 1,
        "folder": folder.display().to_string(),
        "ok": false,
        "errors": [{
            "id": "manifest",
            "message": pane_refuses(&folder),
            "file": "pane.json",
        }],
        "warnings": [],
        "eslint": {
            "ran": false,
            "note": "the package has no eslint of its own",
        },
    });
    assert_eq!(report, expected);

    // A package that passes reports so, with its warnings listed.
    let folder = package(
        &sources.path().join("report-warnings"),
        &manifest(r#", "mode": "no-view""#),
        "sample_rust",
    );
    let (passed, printed) = check(&folder, &["--json"]);
    assert!(passed, "{printed}");
    let report: serde_json::Value = serde_json::from_str(printed.trim()).unwrap();
    assert_eq!(report["ok"], serde_json::json!(true));
    assert_eq!(report["errors"], serde_json::json!([]));
    assert!(
        report["warnings"]
            .as_array()
            .is_some_and(|list| !list.is_empty()),
        "{report}"
    );
    assert_eq!(
        report["eslint"]["note"],
        serde_json::json!("the package has no eslint of its own")
    );
}

#[test]
fn an_argument_check_does_not_take_is_a_usage_error() {
    let sources = tempfile::tempdir().unwrap();
    let folder = package(
        &sources.path().join("usage"),
        &manifest(r#", "mode": "no-view""#),
        "sample_rust",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_pane-ext"))
        .arg("check")
        .arg(&folder)
        .arg("--nope")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        printed.contains("is not an argument check takes"),
        "{printed}"
    );
    assert!(printed.contains("Usage: pane-ext"), "{printed}");
}
