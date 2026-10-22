//! The write-ahead log: each change applied to the memory table is
//! appended here first, so that changes not yet in a segment survive Pane
//! stopping. A log is replayed into the memory table when the index opens
//! and deleted once its changes are in a segment.
//!
//! Each record is its length and CRC-32 (`u32` each), then its payload; a
//! record cut short or failing its check ends the replay there (a crash
//! while it was written).

use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use super::format::{
    self, FileKind, Meta, get_bytes, get_meta, get_varint, put_bytes, put_meta, put_varint,
};

/// A change, as the log keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Put {
        seq: u64,
        key: Vec<u8>,
        meta: Meta,
    },
    Delete {
        seq: u64,
        key: Vec<u8>,
    },
    /// Everything whose key starts with `prefix`.
    DeleteUnder {
        seq: u64,
        prefix: Vec<u8>,
    },
}

impl Op {
    pub(crate) fn seq(&self) -> u64 {
        match self {
            Op::Put { seq, .. } | Op::Delete { seq, .. } | Op::DeleteUnder { seq, .. } => *seq,
        }
    }
}

pub(crate) struct Wal {
    pub(crate) path: PathBuf,
    file: BufWriter<File>,
}

impl Wal {
    /// Starts a new, empty log at `path`.
    pub(crate) fn create(path: PathBuf) -> io::Result<Wal> {
        let mut file = BufWriter::new(super::segment::create_private(&path)?);
        file.write_all(&format::header(FileKind::Wal))?;
        file.flush()?;
        Ok(Wal { path, file })
    }

    /// Appends `ops` and hands them to the system. They are not flushed to
    /// the disk: a power loss may lose the last of them, which the index's
    /// catch-up finds again from the file system's own records.
    pub(crate) fn append(&mut self, ops: &[Op]) -> io::Result<()> {
        let mut payload = Vec::new();
        for op in ops {
            payload.clear();
            match op {
                Op::Put { seq, key, meta } => {
                    payload.push(1);
                    put_varint(&mut payload, *seq);
                    put_bytes(&mut payload, key);
                    put_meta(&mut payload, Some(meta));
                }
                Op::Delete { seq, key } => {
                    payload.push(2);
                    put_varint(&mut payload, *seq);
                    put_bytes(&mut payload, key);
                }
                Op::DeleteUnder { seq, prefix } => {
                    payload.push(3);
                    put_varint(&mut payload, *seq);
                    put_bytes(&mut payload, prefix);
                }
            }
            self.file.write_all(&(payload.len() as u32).to_le_bytes())?;
            self.file
                .write_all(&crc32fast::hash(&payload).to_le_bytes())?;
            self.file.write_all(&payload)?;
        }
        self.file.flush()
    }
}

/// The changes a log at `path` holds, up to its first torn record. A file
/// that is not a log of this format is an error.
pub(crate) fn replay(path: &Path) -> io::Result<Vec<Op>> {
    let mut bytes = Vec::new();
    File::open(path)?.read_to_end(&mut bytes)?;
    if !format::has_header(&bytes, FileKind::Wal) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is not a log of this format", path.display()),
        ));
    }
    let mut ops = Vec::new();
    let mut at = 16;
    while at + 8 <= bytes.len() {
        let len = u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes")) as usize;
        let crc = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("4 bytes"));
        let Some(payload) = bytes.get(at + 8..at + 8 + len) else {
            break;
        };
        if crc32fast::hash(payload) != crc {
            break;
        }
        match decode(payload) {
            Some(op) => ops.push(op),
            None => break,
        }
        at += 8 + len;
    }
    Ok(ops)
}

fn decode(payload: &[u8]) -> Option<Op> {
    let mut at = 1;
    let seq = get_varint(payload, &mut at)?;
    let bytes = get_bytes(payload, &mut at)?.to_vec();
    match payload.first()? {
        1 => Some(Op::Put {
            seq,
            key: bytes,
            meta: get_meta(payload, &mut at)??,
        }),
        2 => Some(Op::Delete { seq, key: bytes }),
        3 => Some(Op::DeleteUnder { seq, prefix: bytes }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_index::format::EntryKind;

    fn ops() -> Vec<Op> {
        vec![
            Op::Put {
                seq: 1,
                key: b"/home/a.txt".to_vec(),
                meta: Meta {
                    kind: EntryKind::File,
                    size: 3,
                    modified: 4,
                    file_id: 5,
                    volume: 6,
                },
            },
            Op::Delete {
                seq: 2,
                key: b"/home/b.txt".to_vec(),
            },
            Op::DeleteUnder {
                seq: 3,
                prefix: b"/home/old/".to_vec(),
            },
        ]
    }

    #[test]
    fn changes_are_replayed_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("1.wal");
        let mut wal = Wal::create(path.clone()).unwrap();
        wal.append(&ops()).unwrap();
        drop(wal);
        assert_eq!(replay(&path).unwrap(), ops());
    }

    #[test]
    fn a_torn_last_record_ends_the_replay() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("1.wal");
        let mut wal = Wal::create(path.clone()).unwrap();
        wal.append(&ops()).unwrap();
        drop(wal);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.truncate(bytes.len() - 3);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(replay(&path).unwrap(), ops()[..2]);
        // A flipped byte fails the record's check.
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 20;
        bytes[last] ^= 0xFF;
        std::fs::write(&path, &bytes).unwrap();
        assert!(replay(&path).unwrap().len() < 2);
    }

    #[test]
    fn a_log_of_another_format_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("1.wal");
        std::fs::write(&path, b"PANEFIX\0\x63\0\0\0\x02\0\0\0").unwrap();
        assert!(replay(&path).is_err());
    }
}
