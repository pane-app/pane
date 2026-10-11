//! The index of a collection: a repository — or folder — whose root holds
//! `pane-collection.json` rather than `pane.json`, listing the extensions it
//! offers, each by id and the folder holding it (ADR 0044). The id, not the
//! folder, names an extension, so moving a folder keeps its identity;
//! `renamed` maps an old id to its new one, or to `null` for one that was
//! removed, and is followed wherever an id is resolved (#310). The index
//! holds no titles or versions: each extension's own manifest carries
//! them, so the two cannot drift.
//!
//! The index is text the collection's author chose, so it is read within a
//! size limit, and everything of it that reaches an explanation is checked
//! first: an id is lowercase letters, digits and `-`, and a path is a plain
//! relative folder under the same name checks as a Git tree entry
//! ([`crate::downloads::check_part`]). A malformed index is refused saying
//! what is wrong with it, as a package's manifest is.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::Deserialize;

use crate::git::shown;

/// The file at the root of a repository or folder that makes it a
/// collection of extensions rather than one extension.
pub(crate) const COLLECTION_FILE: &str = "pane-collection.json";

/// The most bytes of a collection index Pane reads: a collection listing
/// more extensions than this cannot be installed from.
const MAX_INDEX: u64 = 1 << 20;

/// One collection's index, as a revision or folder holds it at its root:
/// the extensions it offers, and the `renamed` map an old id resolves
/// through where the index lists no extension with it — mapped to the
/// extension's new id, or to `null` for one that was removed (ADR 0044,
/// #310).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Collection {
    extensions: Vec<Extension>,
    renamed: BTreeMap<String, Option<String>>,
}

/// One extension of a collection, as the index lists it: the id that names
/// it, and the folder holding it, inside the collection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Extension {
    pub(crate) id: String,
    pub(crate) path: String,
}

/// One extension of a collection as an id resolves to it (ADR 0044, #310):
/// the extension the index lists, and the id it lists it by — the id asked,
/// or, where the index lists none and the `renamed` map was followed, the
/// extension's current id. A fresh install of an old id takes the current
/// id's identity; an installed copy keeps the id it was installed with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub(crate) extension: Extension,
    pub(crate) id: String,
}

impl Collection {
    /// The extension the id `id` names, or `None` where the collection
    /// lists no extension with that id.
    pub(crate) fn find(&self, id: &str) -> Option<&Extension> {
        self.extensions.iter().find(|extension| extension.id == id)
    }

    /// The extensions the collection offers, in the order its index
    /// lists them: the choice reads them all (#308), one row each.
    pub(crate) fn extensions(&self) -> &[Extension] {
        &self.extensions
    }

    /// The `renamed` map, as the index holds it: an old id mapped to its
    /// new id, or to `null` for one that was removed.
    pub(crate) fn renamed(&self) -> &BTreeMap<String, Option<String>> {
        &self.renamed
    }

    /// The extension the id `id` names, resolved through the index
    /// (ADR 0044, #310): the one the index lists with the id, or — where
    /// it lists none — the one its `renamed` map names, the old id
    /// followed through the chain to the extension's current id.
    /// `None` where the collection no longer offers the id: its map
    /// renames it to `null`, or names no entry at all.
    pub(crate) fn resolve(&self, id: &str) -> Option<Resolved> {
        if let Some(extension) = self.find(id) {
            return Some(Resolved {
                extension: extension.clone(),
                id: id.to_owned(),
            });
        }
        // The chain of renames: each step names a new id the index may
        // list — the extension, resolved — or rename again. A cycle
        // among the ids the map names is refused when the index is read,
        // so the chain ends: at an extension, a `null`, an id the map
        // names no entry for (one the collection no longer offers), or
        // an id the index does not list (its author's map is broken, and
        // the id resolves to nothing either way).
        let mut current = id.to_owned();
        loop {
            let next = self.renamed.get(&current).and_then(Option::as_deref)?;
            if let Some(extension) = self.find(next) {
                return Some(Resolved {
                    extension: extension.clone(),
                    id: next.to_owned(),
                });
            }
            current = next.to_owned();
        }
    }
}

/// `true` for the id of one extension of a collection: lowercase letters,
/// digits and `-`, one or more (ADR 0044).
pub(crate) fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Reads the collection index at the root of `folder`: `None` where the
/// root holds no `pane-collection.json`. Every way an index cannot be
/// taken says what is wrong with it, without naming the file — the caller
/// names it — so a caller refuses it as it refuses an unreadable manifest.
pub(crate) fn read(folder: &Path) -> Result<Option<Collection>, String> {
    let text = match fs::read_to_string(folder.join(COLLECTION_FILE)) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("it cannot be read: {error}")),
    };
    parse(&text).map(Some)
}

/// The index `text` writes (`pane-collection.json`'s contents), read and
/// checked exactly as [`read`] checks the file a folder holds: the same
/// ways it cannot be taken, with the same words, so a collection fetched
/// from Git is refused as one read from a folder is wherever its index is
/// checked (#311: the partial fetch reads the index from the blob it
/// fetched, before anything is written).
pub(crate) fn parse(text: &str) -> Result<Collection, String> {
    if text.len() as u64 > MAX_INDEX {
        return Err(format!(
            "it is larger than the {} KiB Pane reads",
            MAX_INDEX >> 10
        ));
    }
    let json: CollectionJson = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let renamed = json.renamed.unwrap_or_default();
    let mut extensions = Vec::new();
    for entry in json.extensions {
        if !is_id(&entry.id) {
            return Err(format!(
                "the extension id `{}` must be lowercase letters, digits and `-`",
                shown(&entry.id)
            ));
        }
        if renamed.contains_key(&entry.id) {
            return Err(format!(
                "the extension id `{}` is in its `renamed` map, so it is not one: an id is \
                 never reused",
                shown(&entry.id)
            ));
        }
        if extensions
            .iter()
            .any(|seen: &Extension| seen.id == entry.id)
        {
            return Err(format!(
                "the extension id `{}` is listed twice",
                shown(&entry.id)
            ));
        }
        for part in entry.path.split('/') {
            if let Err(why) = crate::downloads::check_part(part) {
                return Err(format!(
                    "the path `{}` of `{}` {why}",
                    shown(&entry.path),
                    shown(&entry.id)
                ));
            }
        }
        if extensions.iter().any(|seen| seen.path == entry.path) {
            return Err(format!("the path `{}` is listed twice", shown(&entry.path)));
        }
        extensions.push(Extension {
            id: entry.id,
            path: entry.path,
        });
    }
    for (old, new) in renamed.iter() {
        if !is_id(old) {
            return Err(format!(
                "the `renamed` id `{}` must be lowercase letters, digits and `-`",
                shown(old)
            ));
        }
        if let Some(new) = new
            && !is_id(new)
        {
            return Err(format!(
                "the id `{}` that `renamed` maps `{old}` to must be lowercase letters, digits \
                 and `-`",
                shown(new)
            ));
        }
    }
    // A chain of renames must end: an id the map names that leads back
    // to one the walk has followed is a cycle — a broken index, whose
    // ids resolve to no extension (#310).
    for old in renamed.keys() {
        let mut chain = vec![old.clone()];
        let mut current = old.clone();
        while let Some(next) = renamed.get(&current).and_then(Option::as_deref) {
            if chain.iter().any(|id| id == next) {
                chain.push(next.to_owned());
                let cycle = chain.join(" → ");
                return Err(format!("its `renamed` map holds a cycle: {cycle}"));
            }
            chain.push(next.to_owned());
            current = next.to_owned();
        }
    }
    Ok(Collection {
        extensions,
        renamed,
    })
}

/// The index as `pane-collection.json` writes it. A field it does not know
/// is refused: an index is the one place a collection is described, unlike
/// a package's `pane.json`, whose future fields this Pane may ignore.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionJson {
    extensions: Vec<ExtensionJson>,
    /// An old id mapped to its new id, or to `null` for one that was
    /// removed.
    #[serde(default)]
    renamed: Option<BTreeMap<String, Option<String>>>,
}

/// One extension the index lists.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtensionJson {
    id: String,
    path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder whose root holds `index` as its `pane-collection.json`.
    fn folder(index: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(COLLECTION_FILE), index).unwrap();
        dir
    }

    #[test]
    fn a_collection_index_is_read_and_an_extension_found_by_its_id() {
        let index = r#"{
            "extensions": [
                { "id": "clock", "path": "extensions/clock" },
                { "id": "timers", "path": "extensions/timers" }
            ],
            "renamed": { "old-clock": "clock", "retired": null, "older-clock": "old-clock" }
        }"#;
        let read = read(folder(index).path())
            .unwrap()
            .expect("the index holds a collection");
        assert_eq!(
            read.find("clock").map(|extension| extension.path.as_str()),
            Some("extensions/clock")
        );
        assert_eq!(
            read.find("timers").map(|extension| extension.id.as_str()),
            Some("timers")
        );
        // An id only the `renamed` map names is no extension of the
        // collection, and an unknown one is none.
        assert!(read.find("old-clock").is_none());
        assert!(read.find("nobody").is_none());
    }

    /// An id resolves through the index as an install does (#310): the
    /// extension the index lists with it, or — where it lists none — the
    /// one its `renamed` map names, a chain followed to the extension's
    /// current id; one mapped to `null`, or named by no entry at all,
    /// resolves to nothing — the collection no longer offers it.
    #[test]
    fn an_id_resolves_through_the_renamed_map_to_the_current_extension() {
        let index = r#"{
            "extensions": [
                { "id": "clock", "path": "extensions/clock" }
            ],
            "renamed": { "old-clock": "older-clock", "older-clock": "clock",
                         "retired": null, "lost": "nowhere" }
        }"#;
        let read = read(folder(index).path())
            .unwrap()
            .expect("the index holds a collection");
        // The id the index lists: the extension, by that id.
        let direct = read.resolve("clock").unwrap();
        assert_eq!(direct.id, "clock");
        assert_eq!(direct.extension.path, "extensions/clock");
        // An old id, renamed through a chain: the extension, by its
        // current id.
        let followed = read.resolve("old-clock").unwrap();
        assert_eq!(followed.id, "clock");
        assert_eq!(followed.extension.path, "extensions/clock");
        assert_eq!(read.resolve("older-clock").unwrap().id, "clock");
        // An id mapped to `null`, an id the map names no entry for, and
        // one the map maps to an id the index does not list: the
        // collection no longer offers any of them.
        assert!(read.resolve("retired").is_none());
        assert!(read.resolve("nobody").is_none());
        assert!(read.resolve("lost").is_none());
    }

    #[test]
    fn a_root_without_the_index_holds_no_collection() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()).unwrap(), None);
    }

    #[test]
    fn every_way_an_index_cannot_be_taken_says_what_is_wrong() {
        for (index, why) in [
            (r#"{}"#, "missing field `extensions`"),
            (
                r#"{"unknown": 1, "extensions": []}"#,
                "unknown field `unknown`",
            ),
            (
                r#"{"extensions": [{"id": "clock"}]}"#,
                "missing field `path`",
            ),
            (
                r#"{"extensions": [{"path": "clock"}]}"#,
                "missing field `id`",
            ),
            (
                r#"{"extensions": [{"id": "Clock", "path": "x"}]}"#,
                "the extension id `Clock` must be lowercase letters, digits and `-`",
            ),
            (
                r#"{"extensions": [{"id": "clock", "path": "x"},
                                   {"id": "clock", "path": "y"}]}"#,
                "the extension id `clock` is listed twice",
            ),
            (
                r#"{"extensions": [{"id": "clock", "path": "x"},
                                   {"id": "timer", "path": "x"}]}"#,
                "the path `x` is listed twice",
            ),
            (
                r#"{"extensions": [{"id": "clock", "path": "../x"}]}"#,
                "the path `../x` of `clock` which climbs out with `..`",
            ),
            (
                r#"{"extensions": [{"id": "clock", "path": "a\\b"}]}"#,
                "which would put it in another folder",
            ),
            (
                r#"{"extensions": [{"id": "clock", "path": ""}]}"#,
                "an empty or `.` part",
            ),
            (
                r#"{"extensions": [{"id": "clock", "path": "a//b"}]}"#,
                "an empty or `.` part",
            ),
            (
                r#"{"extensions": [{"id": "clock", "path": "con"}]}"#,
                "a Windows device name",
            ),
            (
                r#"{"renamed": {"old": "Clock"}, "extensions": []}"#,
                "the id `Clock` that `renamed` maps `old` to must be lowercase letters, \
                 digits and `-`",
            ),
            (
                r#"{"renamed": {"Old": null}, "extensions": []}"#,
                "the `renamed` id `Old` must be lowercase letters, digits and `-`",
            ),
            (
                r#"{"extensions": [{"id": "clock", "path": "x"}],
                    "renamed": {"clock": "timer"}}"#,
                "the extension id `clock` is in its `renamed` map, so it is not one",
            ),
            (
                r#"{"extensions": [], "renamed": {"clock": "timer", "timer": "clock"}}"#,
                "its `renamed` map holds a cycle: clock → timer → clock",
            ),
            (
                r#"{"extensions": [], "renamed": {"clock": "clock"}}"#,
                "its `renamed` map holds a cycle: clock → clock",
            ),
            (
                r#"{"extensions": [], "renamed": {"a": "b", "b": "c", "c": "b"}}"#,
                "its `renamed` map holds a cycle: b → c → b",
            ),
        ] {
            let error = read(folder(index).path()).unwrap_err();
            assert!(error.contains(why), "{index}: {error}");
        }
    }
}
