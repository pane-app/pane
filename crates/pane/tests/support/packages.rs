//! The extension packages the window tests install, from the guests
//! `cargo xtask guests` builds into `target/guests`. Shared by the test
//! binaries of `pane`.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

/// The Rust sample's guest, as `cargo xtask guests` builds it.
fn rust_guest() -> PathBuf {
    let guest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/sample_rust.wasm");
    assert!(
        guest.exists(),
        "{} is missing; run `cargo xtask guests`",
        guest.display()
    );
    guest
}

/// Writes a package folder whose one command is the Rust sample.
pub fn package(folder: &Path) -> PathBuf {
    fs::create_dir_all(folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{
  "manifestVersion": 1,
  "title": "Hello",
  "version": "1.0.0",
  "apiVersion": "0.1",
  "commands": [{ "id": "hello", "title": "Say hello", "component": "hello.wasm" }]
}"#,
    )
    .unwrap();
    fs::copy(rust_guest(), folder.join("hello.wasm")).unwrap();
    folder.to_path_buf()
}

/// Writes a package folder whose one command names one of Pane's built-in
/// glyphs (reicon's `star`) as its icon: what the window draws on Pane's
/// neutral command tile (ADR 0035, #247), run by the Rust sample.
pub fn glyph_package(folder: &Path) -> PathBuf {
    fs::create_dir_all(folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{
  "manifestVersion": 1,
  "title": "Star",
  "version": "1.0.0",
  "apiVersion": "0.1",
  "commands": [{
    "id": "star",
    "title": "Star command",
    "subtitle": "Names one of Pane's built-in glyphs as its icon",
    "icon": "star",
    "component": "star.wasm"
  }]
}"#,
    )
    .unwrap();
    fs::copy(rust_guest(), folder.join("star.wasm")).unwrap();
    folder.to_path_buf()
}

/// Writes a package folder whose one command computes root results with
/// the faulty fixture (the calculator's slow one): the query "0 + 0" is
/// answered only after about a second of busy work, with one result
/// titled "Slow answer" — the loading bar's slow case (#248).
pub fn slow_provider(folder: &Path) -> PathBuf {
    let guest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/faulty.wasm");
    assert!(
        guest.exists(),
        "{} is missing; run `cargo xtask guests`",
        guest.display()
    );
    fs::create_dir_all(folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{
  "manifestVersion": 1,
  "title": "Slow",
  "version": "1.0.0",
  "apiVersion": "0.1",
  "commands": [{
    "id": "command",
    "title": "Slow answers",
    "component": "command.wasm",
    "rootResults": true
  }]
}"#,
    )
    .unwrap();
    fs::copy(guest, folder.join("command.wasm")).unwrap();
    folder.to_path_buf()
}

/// Copies the assembled sample package `sample` (a folder of
/// `target/guests/packages`) to `folder`, folders and all.
pub fn assembled_package(sample: &str, folder: &Path) -> PathBuf {
    let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests/packages")
        .join(sample);
    assert!(
        assembled.is_dir(),
        "{} is missing; run `cargo xtask guests`",
        assembled.display()
    );
    copy_folder(&assembled, folder);
    folder.to_path_buf()
}

/// Copies `from` to `to`, folders and all.
fn copy_folder(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            copy_folder(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}
