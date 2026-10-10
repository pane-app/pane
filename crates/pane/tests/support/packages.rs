//! The extension packages the window tests install, from the guests
//! `cargo xtask guests` builds into `target/guests`. Shared by the test
//! binaries of `pane`.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

/// Writes a package folder whose one command is the Rust sample.
pub fn package(folder: &Path) -> PathBuf {
    let guest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/sample_rust.wasm");
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
  "title": "Hello",
  "version": "1.0.0",
  "apiVersion": "0.1",
  "commands": [{ "id": "hello", "title": "Say hello", "component": "hello.wasm" }]
}"#,
    )
    .unwrap();
    fs::copy(guest, folder.join("hello.wasm")).unwrap();
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
