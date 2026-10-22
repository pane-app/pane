//! The guest components and packages `cargo xtask guests` puts in
//! `target/guests`, shared by the test binaries that run them.

#![allow(dead_code)]

use std::path::PathBuf;

/// The folder `cargo xtask guests` builds into.
pub fn guests() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests")
}

/// The file `file` in `target/guests`; panics if it was not built.
pub fn guest_file(file: &str) -> PathBuf {
    let path = guests().join(file);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// The guest component `name` (`<name>.wasm`) in `target/guests`; panics
/// if it was not built.
pub fn guest(name: &str) -> PathBuf {
    guest_file(&format!("{name}.wasm"))
}
