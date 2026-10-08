//! The bytes of the file index's files: the header every file starts with,
//! variable-length numbers, entries' keys and metadata.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The first bytes of every file the index writes.
pub(crate) const MAGIC: [u8; 8] = *b"PANEFIX\0";

/// The index's format version. An index of another version is rebuilt,
/// never read: bump it whenever a file's layout, the key encoding or what
/// the terms are changes.
pub const FORMAT_VERSION: u32 = 1;

/// What a file of the index holds, after the magic and version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileKind {
    Segment = 1,
    Wal = 2,
}

/// The 16-byte header: magic, format version, kind.
pub(crate) fn header(kind: FileKind) -> [u8; 16] {
    let mut header = [0u8; 16];
    header[..8].copy_from_slice(&MAGIC);
    header[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    header[12..16].copy_from_slice(&(kind as u32).to_le_bytes());
    header
}

/// Whether `bytes` start with this version's header for `kind`.
pub(crate) fn has_header(bytes: &[u8], kind: FileKind) -> bool {
    bytes.len() >= 16 && bytes[..16] == header(kind)
}

pub(crate) fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Reads a number written by [`put_varint`] at `*at`, moving past it.
pub(crate) fn get_varint(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let byte = *bytes.get(*at)?;
        *at += 1;
        if shift > 63 {
            return None;
        }
        value |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
}

pub(crate) fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    put_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

pub(crate) fn get_bytes<'a>(bytes: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let len = usize::try_from(get_varint(bytes, at)?).ok()?;
    let end = at.checked_add(len)?;
    let slice = bytes.get(*at..end)?;
    *at = end;
    Some(slice)
}

pub(crate) fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

pub(crate) fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

/// The separator between a path's components, as a key's byte.
pub(crate) const SEPARATOR: u8 = std::path::MAIN_SEPARATOR as u8;

/// A path as the index keys it, losslessly: its bytes on Unix, and on
/// Windows its UTF-16 as WTF-8 (UTF-8 that also carries unpaired
/// surrogates), so that a name that is not valid Unicode is kept exactly
/// for opening. Keys sort a folder's entries together after it. On
/// Windows a `/` is written as the `\` it stands for, so that one path has
/// one key however its separators were written.
pub(crate) fn path_key(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        wtf8_encode(path.as_os_str().encode_wide().map(|unit| {
            if unit == u16::from(b'/') {
                u16::from(b'\\')
            } else {
                unit
            }
        }))
    }
    #[cfg(not(any(unix, windows)))]
    {
        path.to_string_lossy().as_bytes().to_vec()
    }
}

/// The path a key names.
pub(crate) fn key_path(key: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(OsString::from_vec(key.to_vec()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        PathBuf::from(OsString::from_wide(&wtf8_decode(key)))
    }
    #[cfg(not(any(unix, windows)))]
    {
        PathBuf::from(OsString::from(String::from_utf8_lossy(key).into_owned()))
    }
}

/// WTF-8: UTF-8, except that an unpaired surrogate is written as the three
/// bytes UTF-8 would give its code point.
#[cfg(any(windows, test))]
pub(crate) fn wtf8_encode(units: impl Iterator<Item = u16>) -> Vec<u8> {
    let mut out = Vec::new();
    for unit in char::decode_utf16(units) {
        match unit {
            Ok(c) => {
                let mut buffer = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
            }
            Err(lone) => {
                let code = u32::from(lone.unpaired_surrogate());
                out.push(0xE0 | (code >> 12) as u8);
                out.push(0x80 | ((code >> 6) & 0x3F) as u8);
                out.push(0x80 | (code & 0x3F) as u8);
            }
        }
    }
    out
}

/// The UTF-16 a WTF-8 key encodes.
#[cfg(any(windows, test))]
pub(crate) fn wtf8_decode(bytes: &[u8]) -> Vec<u16> {
    let mut units = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let first = bytes[at];
        let (code, len) = if first < 0x80 {
            (u32::from(first), 1)
        } else if first >> 5 == 0b110 && at + 1 < bytes.len() {
            (
                (u32::from(first & 0x1F) << 6) | u32::from(bytes[at + 1] & 0x3F),
                2,
            )
        } else if first >> 4 == 0b1110 && at + 2 < bytes.len() {
            (
                (u32::from(first & 0x0F) << 12)
                    | (u32::from(bytes[at + 1] & 0x3F) << 6)
                    | u32::from(bytes[at + 2] & 0x3F),
                3,
            )
        } else if first >> 3 == 0b11110 && at + 3 < bytes.len() {
            (
                (u32::from(first & 0x07) << 18)
                    | (u32::from(bytes[at + 1] & 0x3F) << 12)
                    | (u32::from(bytes[at + 2] & 0x3F) << 6)
                    | u32::from(bytes[at + 3] & 0x3F),
                4,
            )
        } else {
            (0xFFFD, 1)
        };
        if code >= 0x1_0000 {
            let code = code - 0x1_0000;
            units.push(0xD800 | (code >> 10) as u16);
            units.push(0xDC00 | (code & 0x3FF) as u16);
        } else {
            units.push(code as u16);
        }
        at += len;
    }
    units
}

/// What kind of entry an indexed path is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    File,
    Folder,
    /// A symbolic link, a junction or another link: indexed as an entry,
    /// never followed.
    Link,
}

impl EntryKind {
    pub(crate) fn code(self) -> u8 {
        match self {
            EntryKind::File => 0,
            EntryKind::Folder => 1,
            EntryKind::Link => 2,
        }
    }

    pub(crate) fn from_code(code: u8) -> Option<EntryKind> {
        match code {
            0 => Some(EntryKind::File),
            1 => Some(EntryKind::Folder),
            2 => Some(EntryKind::Link),
            _ => None,
        }
    }
}

/// What the index keeps of an entry besides its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meta {
    pub kind: EntryKind,
    /// In bytes; 0 for a folder.
    pub size: u64,
    /// Last modified, in whole seconds since 1970; 0 when unknown.
    pub modified: u64,
    /// The file system's id for it: the NTFS file reference number on
    /// Windows (what the change journal names it by), the inode elsewhere;
    /// 0 when unknown.
    pub file_id: u64,
    /// The volume it is on: the volume serial number on Windows, the device
    /// number elsewhere.
    pub volume: u64,
}

/// An entry's metadata, or a deletion (`None`), with a flags byte first.
pub(crate) fn put_meta(out: &mut Vec<u8>, meta: Option<&Meta>) {
    match meta {
        None => out.push(DELETED),
        Some(meta) => {
            out.push(meta.kind.code());
            put_varint(out, meta.size);
            put_varint(out, meta.modified);
            put_varint(out, meta.file_id);
            put_varint(out, meta.volume);
        }
    }
}

const DELETED: u8 = 0x80;

/// Reads what [`put_meta`] wrote: `Some(None)` for a deletion.
pub(crate) fn get_meta(bytes: &[u8], at: &mut usize) -> Option<Option<Meta>> {
    let flags = *bytes.get(*at)?;
    *at += 1;
    if flags == DELETED {
        return Some(None);
    }
    let kind = EntryKind::from_code(flags)?;
    Some(Some(Meta {
        kind,
        size: get_varint(bytes, at)?,
        modified: get_varint(bytes, at)?,
        file_id: get_varint(bytes, at)?,
        volume: get_varint(bytes, at)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_round_trip() {
        let values = [0, 1, 127, 128, 300, u32::MAX as u64, u64::MAX];
        let mut bytes = Vec::new();
        for value in values {
            put_varint(&mut bytes, value);
        }
        let mut at = 0;
        for value in values {
            assert_eq!(get_varint(&bytes, &mut at), Some(value));
        }
        assert_eq!(get_varint(&bytes, &mut at), None);
    }

    #[test]
    fn wtf8_keeps_unpaired_surrogates() {
        let units = [0x0061, 0xD800, 0x00E9, 0xD83D, 0xDE00, 0xDC00];
        let encoded = wtf8_encode(units.iter().copied());
        assert_eq!(wtf8_decode(&encoded), units);
        let plain: Vec<u16> = "Résumé".encode_utf16().collect();
        assert_eq!(wtf8_encode(plain.iter().copied()), "Résumé".as_bytes());
    }

    #[test]
    fn a_path_round_trips_through_its_key() {
        let path = std::env::temp_dir()
            .join("Pane files — ñ")
            .join("Résumé.txt");
        assert_eq!(key_path(&path_key(&path)), path);
    }

    #[test]
    fn metadata_and_deletions_round_trip() {
        let meta = Meta {
            kind: EntryKind::Folder,
            size: 0,
            modified: 1_790_000_000,
            file_id: 0x0005_0000_0000_1234,
            volume: 0xDEAD_BEEF,
        };
        let mut bytes = Vec::new();
        put_meta(&mut bytes, Some(&meta));
        put_meta(&mut bytes, None);
        let mut at = 0;
        assert_eq!(get_meta(&bytes, &mut at), Some(Some(meta)));
        assert_eq!(get_meta(&bytes, &mut at), Some(None));
        assert_eq!(at, bytes.len());
    }
}
