//! The default extensions' repositories for tests, made from the
//! assembled sample packages `cargo xtask guests` leaves under
//! `target/guests/packages`, served over Git's smart HTTP protocol from
//! 127.0.0.1 (`repo_server.rs`): the repositories a Pane release pins its
//! default extensions to, and the pins that name them — a test's stand-in
//! for the committed ones (`crates/pane/defaults.json`). The default
//! extensions' own sources left this repository for their own (#285), so
//! a test that needs a default pins a sample's package over the default's
//! identity, or files of its own (`made`); nothing reaches the network or
//! a real Git host.
//!
//! The pins these helpers make name no platform, so every system sets
//! them up, as the five committed ones do; a test that needs the platform
//! gate builds the pin itself — the fields are public, so a clone of one
//! can name a system (`pin.platform = Some(…)`).
//!
//! Only these tests run `git` (the `repo_server` module does, with none
//! of the user's configuration); Pane itself never does. The module
//! needs `repo_server` included beside it.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use pane_core::DefaultExtension;

use super::repo_server::{Repo, Server};

/// The folder `cargo xtask guests` builds into, from either crate whose
/// tests include this module.
fn guests() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests")
}

/// The files of the assembled sample package `sample` under
/// `target/guests/packages`, by their path in the package.
pub fn package_files(sample: &str) -> Vec<(String, Vec<u8>)> {
    let folder = guests().join("packages").join(sample);
    assert!(
        folder.is_dir(),
        "{} is missing; run `cargo xtask guests`",
        folder.display()
    );
    read_files(&folder, "")
}

/// The files of `folder`, by their path under it (as a package holds
/// them, `/`-separated), in alphabetical order.
fn read_files(folder: &Path, prefix: &str) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap())
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().into_string().unwrap();
        let in_package = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if path.is_dir() {
            files.extend(read_files(&path, &in_package));
        } else {
            files.push((in_package, fs::read(&path).unwrap()));
        }
    }
    files
}

/// The version a package's `pane.json` declares.
pub fn version_of(files: &[(String, Vec<u8>)]) -> String {
    let manifest = files
        .iter()
        .find(|(path, _)| path == "pane.json")
        .map(|(_, contents)| contents.clone())
        .expect("the package has a pane.json");
    let manifest: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    manifest["version"]
        .as_str()
        .expect("the package's version")
        .to_owned()
}

/// Makes the repository of the default extension `id` (`title` in Pane's
/// messages), whose default branch holds `files` (path in the package,
/// contents), tagged `tag` — a release revision, as the repositories a
/// Pane release pins hold their built components — without serving it,
/// and returns it with the pin that names the address it will be served
/// at. The repository's work tree is made in `work`, which the test
/// keeps for as long as the server serves it.
pub fn made(
    server: &Server,
    work: &Path,
    id: &str,
    title: &str,
    tag: &str,
    files: &[(String, Vec<u8>)],
) -> (Repo, DefaultExtension) {
    let repo = Repo::init(&work.join(id), server.home());
    let borrowed: Vec<(&str, Vec<u8>)> = files
        .iter()
        .map(|(path, contents)| (path.as_str(), contents.clone()))
        .collect();
    let commit = repo.commit(&borrowed, &format!("Release {tag}"));
    repo.tag(tag);
    let repository = format!("{}{id}.git", server.url());
    (
        repo,
        DefaultExtension {
            id: id.into(),
            title: title.into(),
            repository,
            tag: tag.into(),
            commit,
            platform: None,
        },
    )
}

/// As [`made`], and the repository is served as `<id>`: the pin names
/// the commit the tag points to, at the address the server answers.
pub fn pinned(
    server: &Server,
    work: &Path,
    id: &str,
    title: &str,
    tag: &str,
    files: &[(String, Vec<u8>)],
) -> DefaultExtension {
    let (repo, pin) = made(server, work, id, title, tag, files);
    server.serve(id, &repo);
    pin
}

/// The pin of the default extension `id` over the assembled sample
/// package `sample` under `target/guests/packages`, tagged as its
/// manifest's version, made and served as [`pinned`] does: the sample
/// stands in for the default's own repository, which lives outside this
/// one. The pin's id names the default's identity (`default:<id>`),
/// whatever the sample's manifest titles it.
pub fn from_sample(
    server: &Server,
    work: &Path,
    id: &str,
    title: &str,
    sample: &str,
) -> DefaultExtension {
    let files = package_files(sample);
    let tag = format!("v{}", version_of(&files));
    pinned(server, work, id, title, &tag, &files)
}
