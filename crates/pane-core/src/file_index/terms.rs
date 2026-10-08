//! What an entry is found by: the words of its name and of the folders it
//! is in below its root, as the index's terms, and the fixed-size hint the
//! query ranks candidates by before reading them.

use std::path::Path;

use super::format::{EntryKind, Meta, SEPARATOR, path_key};
use super::text;

/// A term naming a word of an entry's own name.
pub(crate) const NAME_TAG: u8 = b'n';
/// A term naming a word of a folder an entry is in, below its root.
pub(crate) const FOLDER_TAG: u8 = b'f';

/// The roots of the index scope, as keys: a path's words are counted from
/// the root it is under, so the home folder's own path ("C:\Users\ana")
/// names nothing.
#[derive(Clone, Debug, Default)]
pub(crate) struct Roots {
    keys: Vec<Vec<u8>>,
}

impl Roots {
    pub(crate) fn new(roots: &[impl AsRef<Path>]) -> Roots {
        let mut keys: Vec<Vec<u8>> = roots
            .iter()
            .map(|root| {
                let mut key = path_key(root.as_ref());
                while key.len() > 1 && key.last() == Some(&SEPARATOR) {
                    key.pop();
                }
                key
            })
            .collect();
        // Longest first, so a root inside another one wins.
        keys.sort_by_key(|key| std::cmp::Reverse(key.len()));
        Roots { keys }
    }

    /// `key`'s folders below its root (outermost first) and its own name.
    pub(crate) fn split<'a>(&self, key: &'a [u8]) -> (Vec<&'a [u8]>, &'a [u8]) {
        if self.keys.iter().any(|root| root.as_slice() == key) {
            let name = key
                .rsplit(|&byte| byte == SEPARATOR)
                .find(|part| !part.is_empty())
                .unwrap_or(key);
            return (Vec::new(), name);
        }
        let below = self
            .keys
            .iter()
            .find_map(|root| {
                if key.len() > root.len()
                    && key.starts_with(root)
                    && (key[root.len()] == SEPARATOR || root.last() == Some(&SEPARATOR))
                {
                    let start = if key[root.len()] == SEPARATOR {
                        root.len() + 1
                    } else {
                        root.len()
                    };
                    Some(&key[start..])
                } else {
                    None
                }
            })
            .unwrap_or(key);
        let mut parts: Vec<&[u8]> = below
            .split(|&byte| byte == SEPARATOR)
            .filter(|part| !part.is_empty())
            .collect();
        let name = parts.pop().unwrap_or(below);
        (parts, name)
    }
}

/// An entry ready to be written: its key, metadata, terms and hint.
#[derive(Clone, Debug)]
pub(crate) struct Prepared {
    pub(crate) key: Vec<u8>,
    pub(crate) meta: Option<Meta>,
    pub(crate) terms: Vec<Vec<u8>>,
    pub(crate) hint: u32,
}

impl Prepared {
    pub(crate) fn entry(roots: &Roots, path: &Path, meta: Meta) -> Prepared {
        let key = path_key(path);
        let (terms, hint) = terms_and_hint(roots, &key, Some(&meta));
        Prepared {
            key,
            meta: Some(meta),
            terms,
            hint,
        }
    }

    /// A deletion of `key`: no terms, found by no query.
    pub(crate) fn deletion(key: Vec<u8>) -> Prepared {
        Prepared {
            key,
            meta: None,
            terms: Vec::new(),
            hint: 0,
        }
    }
}

/// The terms of `key` (tagged name words, then tagged folder words) and its
/// hint.
pub(crate) fn terms_and_hint(
    roots: &Roots,
    key: &[u8],
    meta: Option<&Meta>,
) -> (Vec<Vec<u8>>, u32) {
    let Some(meta) = meta else {
        return (Vec::new(), 0);
    };
    let (folders, name) = roots.split(key);
    let mut terms = Vec::new();
    for word in text::words(&String::from_utf8_lossy(name)) {
        let mut term = Vec::with_capacity(word.len() + 1);
        term.push(NAME_TAG);
        term.extend_from_slice(word.as_bytes());
        terms.push(term);
    }
    let mut seen: Vec<String> = Vec::new();
    for folder in &folders {
        for word in text::words(&String::from_utf8_lossy(folder)) {
            if !seen.contains(&word) {
                let mut term = Vec::with_capacity(word.len() + 1);
                term.push(FOLDER_TAG);
                term.extend_from_slice(word.as_bytes());
                terms.push(term);
                seen.push(word);
            }
        }
    }
    (terms, hint(meta, folders.len()))
}

/// The hint: the day it was last modified (16 bits), how deep it is below
/// its root (8 bits) and its kind (8 bits).
pub(crate) fn hint(meta: &Meta, depth: usize) -> u32 {
    let day = (meta.modified / 86_400).min(u64::from(u16::MAX)) as u32;
    (day << 16) | ((depth.min(255) as u32) << 8) | u32::from(meta.kind.code())
}

pub(crate) fn hint_day(hint: u32) -> u32 {
    hint >> 16
}

pub(crate) fn hint_depth(hint: u32) -> u32 {
    (hint >> 8) & 0xFF
}

pub(crate) fn hint_kind(hint: u32) -> Option<EntryKind> {
    EntryKind::from_code((hint & 0xFF) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn home() -> PathBuf {
        std::env::temp_dir().join("home")
    }

    fn meta() -> Meta {
        Meta {
            kind: EntryKind::File,
            size: 1,
            modified: 86_400 * 3,
            file_id: 7,
            volume: 1,
        }
    }

    #[test]
    fn words_are_counted_from_the_root() {
        let roots = Roots::new(&[home()]);
        let path = home().join("Invoices 2026").join("Résumé plan.pdf");
        let prepared = Prepared::entry(&roots, &path, meta());
        let terms: Vec<String> = prepared
            .terms
            .iter()
            .map(|term| String::from_utf8(term.clone()).unwrap())
            .collect();
        assert_eq!(terms, ["nresume", "nplan", "npdf", "finvoices", "f2026"]);
        assert_eq!(hint_day(prepared.hint), 3);
        assert_eq!(hint_depth(prepared.hint), 1);
        assert_eq!(hint_kind(prepared.hint), Some(EntryKind::File));
    }

    #[test]
    fn the_root_itself_is_named_by_its_own_name() {
        let roots = Roots::new(&[home()]);
        let key = path_key(&home());
        let (folders, name) = roots.split(&key);
        assert!(folders.is_empty());
        assert_eq!(name, b"home");
    }

    #[test]
    fn a_deletion_has_no_terms() {
        let prepared = Prepared::deletion(b"x".to_vec());
        assert!(prepared.terms.is_empty() && prepared.meta.is_none());
    }
}
