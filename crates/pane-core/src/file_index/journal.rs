//! Catching up from the NTFS change journal (#126, "Catching up and
//! watching, per system"; ADR 0034): what changed on a volume while Pane was
//! not running, read without administrator rights and without a service.
//!
//! [`read_journal`] reads a volume's journal from a saved [`JournalCursor`]
//! through `FSCTL_READ_UNPRIVILEGED_USN_JOURNAL`, on a handle to the
//! volume's root folder opened for reading attributes only; it never
//! creates or resizes a journal. [`resolve`] turns its records, which name
//! entries by file id and parent folder id, into the paths to remove, look
//! at again and walk again, through the index's folder ids
//! ([`FileIndex::folder_ids`](super::FileIndex::folder_ids)). A record
//! whose folder is not indexed (outside the roots, or excluded) resolves to
//! nothing, so the index scope is kept without further checks of folders;
//! the paths it gives still go through the scope's rules for their own
//! names.
//!
//! macOS and Linux have no such journal: macOS replays FSEvents from a
//! saved event id, and Linux, whose fanotify needs privileges, reconciles
//! by walking the folders whose modified time changed (#175).

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Where Pane read a volume's journal up to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalCursor {
    /// The volume's root folder, such as `C:\`.
    pub volume: String,
    /// The journal's id: a new journal (deleted and created again) has
    /// another, and its records cannot continue the old one's.
    pub journal_id: u64,
    /// The next record to read.
    pub next_usn: i64,
}

/// A change record, as the journal gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalRecord {
    pub file_id: u64,
    pub parent_id: u64,
    pub name: OsString,
    /// `USN_REASON_*` flags.
    pub reasons: u32,
    pub is_folder: bool,
    pub usn: i64,
}

pub const REASON_DATA_OVERWRITE: u32 = 0x1;
pub const REASON_DATA_EXTEND: u32 = 0x2;
pub const REASON_DATA_TRUNCATION: u32 = 0x4;
pub const REASON_FILE_CREATE: u32 = 0x100;
pub const REASON_FILE_DELETE: u32 = 0x200;
pub const REASON_RENAME_OLD_NAME: u32 = 0x1000;
pub const REASON_RENAME_NEW_NAME: u32 = 0x2000;
pub const REASON_BASIC_INFO_CHANGE: u32 = 0x8000;
pub const REASON_HARD_LINK_CHANGE: u32 = 0x1_0000;
pub const REASON_REPARSE_POINT_CHANGE: u32 = 0x10_0000;
pub const REASON_CLOSE: u32 = 0x8000_0000;

/// What reading a journal found.
#[derive(Debug)]
pub enum JournalRead {
    /// The records since the cursor, and the cursor after them. Without a
    /// cursor to start from, no records and where the journal is now: what
    /// a first walk saves before it starts, so that changes made while it
    /// runs are caught up afterwards.
    Records {
        records: Vec<JournalRecord>,
        cursor: JournalCursor,
    },
    /// The journal was deleted and created again since the cursor: walk.
    Recreated,
    /// The records after the cursor were already discarded (the journal is
    /// full and wrapped): walk.
    Discarded,
    /// More records than `max_records`: a walk is cheaper.
    TooMany,
    /// The volume keeps no journal (FAT, exFAT, a network share, or NTFS
    /// with its journal switched off), or this system has none: walk.
    NoJournal,
    /// The system refused to read it: walk, and say why.
    Refused(std::io::Error),
}

/// The paths a batch of records changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatchUp {
    /// Gone, or renamed away from: removed from the index, and for a folder
    /// everything under it too.
    pub removed: Vec<PathBuf>,
    pub removed_folders: Vec<PathBuf>,
    /// Created, changed or renamed to: looked at again (indexed if they
    /// exist and the rules admit them, removed otherwise).
    pub touched: Vec<PathBuf>,
    /// Folders renamed or moved to: walked again, since their contents were
    /// indexed under another path.
    pub walk: Vec<PathBuf>,
    /// Folders in which an entry the journal gave no name for went away:
    /// what the index holds in them and the disk no longer does is removed
    /// (see [`FileIndex::children`](super::FileIndex::children)).
    pub listed: Vec<PathBuf>,
    /// Records naming a folder the index does not hold: outside the roots,
    /// excluded, or created and gone before it was read.
    pub unresolved: u64,
}

/// What `records` changed, given each indexed folder's file id (updated as
/// folders are created, renamed and deleted in the records).
///
/// Windows gives no names in the records it lets a user without
/// administrator rights read: a record without one is named by `name_of`,
/// which answers an entry's name now from its file id ([`Names`]), and an
/// entry gone, which has none, is known by its id if it is an indexed
/// folder, and otherwise found missing from its folder ([`CatchUp::listed`]).
pub fn resolve(
    records: &[JournalRecord],
    folders: &mut HashMap<u64, PathBuf>,
    name_of: &mut dyn FnMut(u64) -> Option<OsString>,
) -> CatchUp {
    let mut catch_up = CatchUp::default();
    for record in records {
        let Some(parent) = folders.get(&record.parent_id) else {
            catch_up.unresolved += 1;
            continue;
        };
        let gone = record.reasons & (REASON_FILE_DELETE | REASON_RENAME_OLD_NAME) != 0;
        let path = if !record.name.is_empty() {
            parent.join(&record.name)
        } else if gone {
            match folders.get(&record.file_id).filter(|_| record.is_folder) {
                Some(folder) => folder.clone(),
                None => {
                    catch_up.listed.push(parent.clone());
                    continue;
                }
            }
        } else {
            match name_of(record.file_id) {
                Some(name) => parent.join(name),
                None => {
                    // Gone since: found missing from its folder.
                    catch_up.listed.push(parent.clone());
                    continue;
                }
            }
        };
        if gone {
            if record.is_folder {
                catch_up.removed_folders.push(path.clone());
                // A folder renamed keeps its id until its new name comes,
                // which moves the folders below it; a deleted one has none.
                if record.reasons & REASON_FILE_DELETE != 0
                    && folders.get(&record.file_id) == Some(&path)
                {
                    folders.remove(&record.file_id);
                }
            }
            catch_up.removed.push(path);
            continue;
        }
        if record.is_folder {
            let renamed_to = record.reasons & REASON_RENAME_NEW_NAME != 0;
            if renamed_to {
                // Its folders below keep their ids and take the new path.
                if let Some(old) = folders.get(&record.file_id).cloned()
                    && old != path
                {
                    for known in folders.values_mut() {
                        if let Ok(below) = known.strip_prefix(&old) {
                            *known = path.join(below);
                        }
                    }
                }
                catch_up.walk.push(path.clone());
            }
            folders.insert(record.file_id, path.clone());
        }
        catch_up.touched.push(path);
    }
    for paths in [
        &mut catch_up.removed,
        &mut catch_up.removed_folders,
        &mut catch_up.touched,
        &mut catch_up.walk,
        &mut catch_up.listed,
    ] {
        paths.sort();
        paths.dedup();
    }
    catch_up
}

/// The most entries [`Names`] looks up in one catch-up; past it, a record
/// without a name has its folder listed instead.
const MAX_LOOKUPS: usize = 100_000;

/// Names entries of one volume by their file ids, as they are named now
/// (the names unprivileged journal records leave out); each id is looked up
/// once. Answers nothing on other systems, or for an entry gone.
pub struct Names {
    #[cfg(windows)]
    volume: Option<imp::Handle>,
    known: HashMap<u64, Option<OsString>>,
}

impl Names {
    /// Names entries of the volume holding `root`.
    pub fn for_volume_of(root: &std::path::Path) -> Names {
        #[cfg(not(windows))]
        let _ = root;
        Names {
            #[cfg(windows)]
            volume: imp::open_volume_root(root).ok(),
            known: HashMap::new(),
        }
    }

    /// The name of the entry with file id `file_id` now, if it is there.
    pub fn name(&mut self, file_id: u64) -> Option<OsString> {
        if let Some(known) = self.known.get(&file_id) {
            return known.clone();
        }
        if self.known.len() >= MAX_LOOKUPS {
            return None;
        }
        #[cfg(windows)]
        let name = self
            .volume
            .as_ref()
            .and_then(|volume| imp::name_by_id(volume, file_id));
        #[cfg(not(windows))]
        let name = None;
        self.known.insert(file_id, name.clone());
        name
    }
}

/// Reads the journal of the volume holding `root` from `since` (or, with
/// no cursor, where it is now), at most `max_records` records. Needs no
/// administrator rights on Windows 10 and 11 (`READ_UNPRIVILEGED`); other
/// systems have no journal and answer [`JournalRead::NoJournal`].
pub fn read_journal(
    root: &std::path::Path,
    since: Option<&JournalCursor>,
    max_records: usize,
) -> JournalRead {
    imp::read(root, since, max_records)
}

#[cfg(not(windows))]
mod imp {
    use super::{JournalCursor, JournalRead};

    pub(super) fn read(
        _root: &std::path::Path,
        _since: Option<&JournalCursor>,
        _max_records: usize,
    ) -> JournalRead {
        JournalRead::NoJournal
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::OsString;
    use std::io;
    use std::os::windows::ffi::OsStringExt;
    use std::path::Path;

    use ::windows::Win32::Foundation::{CloseHandle, HANDLE};
    use ::windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_DESCRIPTOR,
        FILE_ID_DESCRIPTOR_0, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FileIdType, FileNameInfo, GetFileInformationByHandleEx,
        GetVolumePathNameW, OPEN_EXISTING, OpenFileById,
    };
    use ::windows::Win32::System::IO::DeviceIoControl;
    use ::windows::Win32::System::Ioctl::{
        FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_UNPRIVILEGED_USN_JOURNAL, READ_USN_JOURNAL_DATA_V1,
        USN_JOURNAL_DATA_V0,
    };
    use ::windows::core::PCWSTR;

    use super::{JournalCursor, JournalRead, JournalRecord};
    use crate::util::wide;

    const ERROR_INVALID_FUNCTION: i32 = 1;
    const ERROR_HANDLE_EOF: i32 = 38;
    const ERROR_NOT_SUPPORTED: i32 = 50;
    const ERROR_JOURNAL_DELETE_IN_PROGRESS: i32 = 1178;
    const ERROR_JOURNAL_NOT_ACTIVE: i32 = 1179;
    const ERROR_JOURNAL_ENTRY_DELETED: i32 = 1181;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

    fn code(error: &::windows::core::Error) -> i32 {
        error.code().0 & 0xFFFF
    }

    pub(super) struct Handle(HANDLE);

    // SAFETY: a file handle may be used and closed from any thread.
    unsafe impl Send for Handle {}

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: opened below and closed once, here.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    /// The root folder of the volume holding `root`, opened only to read
    /// attributes: no administrator rights, unlike a handle to the volume
    /// itself.
    pub(super) fn open_volume_root(root: &Path) -> io::Result<Handle> {
        open_root(&volume_root(root)?)
    }

    fn open_root(volume: &str) -> io::Result<Handle> {
        // SAFETY: a NUL-terminated path; the handle is closed by `Handle`.
        let opened = unsafe {
            CreateFileW(
                PCWSTR(wide(Path::new(volume)).as_ptr()),
                FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        };
        opened
            .map(Handle)
            .map_err(|error| io::Error::from_raw_os_error(code(&error)))
    }

    /// The name now of the entry with file id `file_id` on `volume`'s
    /// volume, opened by its id for reading attributes only (a link not
    /// followed); `None` when it is gone or cannot be opened.
    pub(super) fn name_by_id(volume: &Handle, file_id: u64) -> Option<OsString> {
        let descriptor = FILE_ID_DESCRIPTOR {
            dwSize: size_of::<FILE_ID_DESCRIPTOR>() as u32,
            Type: FileIdType,
            Anonymous: FILE_ID_DESCRIPTOR_0 {
                FileId: file_id as i64,
            },
        };
        // SAFETY: an open handle on the volume and a descriptor of the
        // call's type; the handle opened is closed by `Handle`.
        let opened = unsafe {
            OpenFileById(
                volume.0,
                &descriptor,
                FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            )
        };
        let file = Handle(opened.ok()?);
        // u32s, so that the buffer is aligned for FILE_NAME_INFO.
        let mut buffer = vec![0u32; 1 + 32 * 1024];
        // SAFETY: an open handle and a writable buffer of the size given.
        unsafe {
            GetFileInformationByHandleEx(
                file.0,
                FileNameInfo,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 4) as u32,
            )
        }
        .ok()?;
        let bytes = (buffer[0] as usize).min((buffer.len() - 1) * 4);
        // SAFETY: the name's UTF-16 follows its length, within the buffer.
        let units =
            unsafe { std::slice::from_raw_parts(buffer.as_ptr().add(1).cast::<u16>(), bytes / 2) };
        let name = units
            .rsplit(|&unit| unit == u16::from(b'\\'))
            .next()
            .filter(|name| !name.is_empty())?;
        Some(OsString::from_wide(name))
    }

    /// The root folder of the volume holding `path`, such as `C:\`.
    fn volume_root(path: &Path) -> io::Result<String> {
        let mut buffer = [0u16; 1024];
        // SAFETY: a NUL-terminated path and a writable buffer.
        unsafe { GetVolumePathNameW(PCWSTR(wide(path).as_ptr()), &mut buffer) }
            .map_err(|error| io::Error::from_raw_os_error(code(&error)))?;
        let len = buffer
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(buffer.len());
        Ok(String::from_utf16_lossy(&buffer[..len]))
    }

    pub(super) fn read(
        root: &Path,
        since: Option<&JournalCursor>,
        max_records: usize,
    ) -> JournalRead {
        let volume = match volume_root(root) {
            Ok(volume) => volume,
            Err(error) => return JournalRead::Refused(error),
        };
        let handle = match open_root(&volume) {
            Ok(handle) => handle,
            Err(error) => return JournalRead::Refused(error),
        };
        let mut journal = USN_JOURNAL_DATA_V0::default();
        let mut returned = 0u32;
        // SAFETY: an open handle and an output buffer of the code's type.
        let queried = unsafe {
            DeviceIoControl(
                handle.0,
                FSCTL_QUERY_USN_JOURNAL,
                None,
                0,
                Some((&raw mut journal).cast()),
                size_of::<USN_JOURNAL_DATA_V0>() as u32,
                Some(&mut returned),
                None,
            )
        };
        if let Err(error) = queried {
            return match code(&error) {
                ERROR_JOURNAL_NOT_ACTIVE
                | ERROR_JOURNAL_DELETE_IN_PROGRESS
                | ERROR_INVALID_FUNCTION
                | ERROR_NOT_SUPPORTED => JournalRead::NoJournal,
                other => JournalRead::Refused(io::Error::from_raw_os_error(other)),
            };
        }
        let Some(since) = since else {
            return JournalRead::Records {
                records: Vec::new(),
                cursor: JournalCursor {
                    volume,
                    journal_id: journal.UsnJournalID,
                    next_usn: journal.NextUsn,
                },
            };
        };
        if since.journal_id != journal.UsnJournalID || !since.volume.eq_ignore_ascii_case(&volume) {
            return JournalRead::Recreated;
        }
        if since.next_usn < journal.FirstUsn || since.next_usn < journal.LowestValidUsn {
            return JournalRead::Discarded;
        }
        let end = journal.NextUsn;
        let mut next = since.next_usn;
        let mut records = Vec::new();
        // u64s, so that the buffer is aligned for the records.
        let mut buffer = vec![0u64; 64 * 1024 / 8];
        while next < end {
            let request = READ_USN_JOURNAL_DATA_V1 {
                StartUsn: next,
                ReasonMask: u32::MAX,
                ReturnOnlyOnClose: 0,
                Timeout: 0,
                BytesToWaitFor: 0,
                UsnJournalID: journal.UsnJournalID,
                MinMajorVersion: 2,
                MaxMajorVersion: 3,
            };
            let mut returned = 0u32;
            // SAFETY: an open handle, the request of the code's type and a
            // writable buffer of the size given.
            let read = unsafe {
                DeviceIoControl(
                    handle.0,
                    FSCTL_READ_UNPRIVILEGED_USN_JOURNAL,
                    Some((&raw const request).cast()),
                    size_of::<READ_USN_JOURNAL_DATA_V1>() as u32,
                    Some(buffer.as_mut_ptr().cast()),
                    (buffer.len() * 8) as u32,
                    Some(&mut returned),
                    None,
                )
            };
            if let Err(error) = read {
                return match code(&error) {
                    ERROR_JOURNAL_ENTRY_DELETED => JournalRead::Discarded,
                    ERROR_JOURNAL_NOT_ACTIVE | ERROR_JOURNAL_DELETE_IN_PROGRESS => {
                        JournalRead::Recreated
                    }
                    ERROR_HANDLE_EOF => break,
                    other => JournalRead::Refused(io::Error::from_raw_os_error(other)),
                };
            }
            // SAFETY: the buffer holds `returned` bytes the call wrote, as
            // bytes for parsing.
            let bytes = unsafe {
                std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), returned as usize)
            };
            if bytes.len() < 8 {
                break;
            }
            let after = i64::from_le_bytes(bytes[..8].try_into().expect("8 bytes"));
            parse(&bytes[8..], &mut records);
            if records.len() > max_records {
                return JournalRead::TooMany;
            }
            if after <= next {
                break;
            }
            next = after;
        }
        JournalRead::Records {
            records,
            cursor: JournalCursor {
                volume,
                journal_id: journal.UsnJournalID,
                next_usn: next.max(since.next_usn),
            },
        }
    }

    fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
    }

    fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    }

    fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
        Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
    }

    /// Parses `USN_RECORD_V2` and `V3` records (V3's 128-bit ids are cut to
    /// their low 64 bits, which is the whole id on NTFS).
    pub(super) fn parse(mut bytes: &[u8], records: &mut Vec<JournalRecord>) {
        while bytes.len() >= 8 {
            let Some(length) = u32_at(bytes, 0).map(|n| n as usize) else {
                return;
            };
            if length < 8 || length > bytes.len() {
                return;
            }
            let record = &bytes[..length];
            if let Some(parsed) = fields(record) {
                records.push(parsed);
            }
            bytes = &bytes[length..];
        }
    }

    /// Where a record's fields are, by its major version: file id, parent
    /// id, USN, reasons, attributes, name length, name offset.
    const V2: [usize; 7] = [8, 16, 24, 40, 52, 56, 58];
    const V3: [usize; 7] = [8, 24, 40, 56, 68, 72, 74];

    fn fields(record: &[u8]) -> Option<JournalRecord> {
        let at = match u16_at(record, 4)? {
            2 => V2,
            3 => V3,
            _ => return None,
        };
        let name_len = usize::from(u16_at(record, at[5])?);
        let name_at = usize::from(u16_at(record, at[6])?);
        let name = record.get(name_at..name_at.checked_add(name_len)?)?;
        let units: Vec<u16> = name
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        Some(JournalRecord {
            file_id: u64_at(record, at[0])?,
            parent_id: u64_at(record, at[1])?,
            usn: u64_at(record, at[2])? as i64,
            reasons: u32_at(record, at[3])?,
            is_folder: u32_at(record, at[4])? & FILE_ATTRIBUTE_DIRECTORY != 0,
            name: OsString::from_wide(&units),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn record(
        file_id: u64,
        parent_id: u64,
        name: &str,
        reasons: u32,
        is_folder: bool,
    ) -> JournalRecord {
        JournalRecord {
            file_id,
            parent_id,
            name: name.into(),
            reasons,
            is_folder,
            usn: 0,
        }
    }

    fn home() -> PathBuf {
        Path::new("/home/ana").to_path_buf()
    }

    #[test]
    fn records_resolve_to_paths_through_the_folder_ids() {
        let mut folders = HashMap::from([(1, home()), (2, home().join("Documents"))]);
        let records = [
            record(10, 2, "plan.txt", REASON_FILE_CREATE, false),
            record(10, 2, "plan.txt", REASON_DATA_EXTEND | REASON_CLOSE, false),
            record(11, 1, "old.txt", REASON_FILE_DELETE | REASON_CLOSE, false),
            record(12, 99, "elsewhere.txt", REASON_FILE_CREATE, false),
            // A folder created, and a file in it.
            record(20, 1, "Projects", REASON_FILE_CREATE, true),
            record(21, 20, "idea.md", REASON_FILE_CREATE, false),
        ];
        let catch_up = resolve(&records, &mut folders, &mut |_| None);
        assert_eq!(
            catch_up.touched,
            [
                home().join("Documents").join("plan.txt"),
                home().join("Projects"),
                home().join("Projects").join("idea.md"),
            ]
        );
        assert_eq!(catch_up.removed, [home().join("old.txt")]);
        assert_eq!(catch_up.unresolved, 1);
        assert_eq!(folders.get(&20), Some(&home().join("Projects")));
    }

    #[test]
    fn a_renamed_folder_is_removed_under_its_old_name_and_walked_under_its_new_one() {
        let mut folders = HashMap::from([
            (1, home()),
            (2, home().join("Drafts")),
            (3, home().join("Drafts").join("2026")),
        ]);
        let records = [
            record(2, 1, "Drafts", REASON_RENAME_OLD_NAME, true),
            record(2, 1, "Final", REASON_RENAME_NEW_NAME, true),
            // A file later created in its subfolder resolves to the new path.
            record(30, 3, "letter.txt", REASON_FILE_CREATE, false),
        ];
        let catch_up = resolve(&records, &mut folders, &mut |_| None);
        assert_eq!(catch_up.removed, [home().join("Drafts")]);
        assert_eq!(catch_up.removed_folders, [home().join("Drafts")]);
        assert_eq!(catch_up.walk, [home().join("Final")]);
        assert!(
            catch_up
                .touched
                .contains(&home().join("Final").join("2026").join("letter.txt"))
        );
        assert_eq!(folders.get(&3), Some(&home().join("Final").join("2026")));
    }

    #[test]
    fn a_deleted_folder_forgets_its_id() {
        let mut folders = HashMap::from([(1, home()), (2, home().join("Old"))]);
        let records = [
            record(2, 1, "Old", REASON_FILE_DELETE | REASON_CLOSE, true),
            record(40, 2, "late.txt", REASON_FILE_CREATE, false),
        ];
        let catch_up = resolve(&records, &mut folders, &mut |_| None);
        assert_eq!(catch_up.removed_folders, [home().join("Old")]);
        assert_eq!(catch_up.unresolved, 1);
    }

    /// Records read without administrator rights carry no names: an entry
    /// there is named by its id, a folder gone by the id the index holds,
    /// and a file gone is found missing from its folder.
    #[test]
    fn records_without_names_resolve_through_the_ids() {
        let mut folders = HashMap::from([
            (1, home()),
            (2, home().join("Drafts")),
            (3, home().join("Drafts").join("2026")),
            (4, home().join("Documents")),
        ]);
        let records = [
            record(10, 4, "", REASON_FILE_CREATE, false),
            record(11, 4, "", REASON_FILE_DELETE | REASON_CLOSE, false),
            record(2, 1, "", REASON_RENAME_OLD_NAME, true),
            record(2, 1, "", REASON_RENAME_NEW_NAME, true),
            record(30, 3, "", REASON_FILE_CREATE, false),
            // Created and gone before it was named.
            record(31, 1, "", REASON_FILE_CREATE, false),
        ];
        let mut name_of = |id: u64| match id {
            10 => Some(OsString::from("plan.txt")),
            2 => Some(OsString::from("Final")),
            30 => Some(OsString::from("letter.txt")),
            _ => None,
        };
        let catch_up = resolve(&records, &mut folders, &mut name_of);
        assert_eq!(catch_up.removed_folders, [home().join("Drafts")]);
        assert_eq!(catch_up.walk, [home().join("Final")]);
        assert_eq!(
            catch_up.touched,
            [
                home().join("Documents").join("plan.txt"),
                home().join("Final"),
                home().join("Final").join("2026").join("letter.txt"),
            ]
        );
        assert_eq!(catch_up.listed, [home(), home().join("Documents")]);
    }

    #[cfg(windows)]
    #[test]
    fn records_of_both_versions_are_parsed() {
        let mut bytes = Vec::new();
        for major in [2u16, 3] {
            let name: Vec<u8> = "Résumé.txt"
                .encode_utf16()
                .flat_map(|unit| unit.to_le_bytes())
                .collect();
            let header = if major == 2 { 60 } else { 76 };
            let length = (header + name.len()).next_multiple_of(8);
            let mut record = vec![0u8; length];
            record[0..4].copy_from_slice(&(length as u32).to_le_bytes());
            record[4..6].copy_from_slice(&major.to_le_bytes());
            let (file, parent, usn, reason, attributes, name_len, name_at) = if major == 2 {
                (8, 16, 24, 40, 52, 56, 58)
            } else {
                (8, 24, 40, 56, 68, 72, 74)
            };
            record[file..file + 8].copy_from_slice(&7u64.to_le_bytes());
            record[parent..parent + 8].copy_from_slice(&5u64.to_le_bytes());
            record[usn..usn + 8].copy_from_slice(&99i64.to_le_bytes());
            record[reason..reason + 4].copy_from_slice(&REASON_FILE_CREATE.to_le_bytes());
            record[attributes..attributes + 4].copy_from_slice(&0x20u32.to_le_bytes());
            record[name_len..name_len + 2].copy_from_slice(&(name.len() as u16).to_le_bytes());
            record[name_at..name_at + 2].copy_from_slice(&(header as u16).to_le_bytes());
            record[header..header + name.len()].copy_from_slice(&name);
            bytes.extend(record);
        }
        let mut records = Vec::new();
        imp::parse(&bytes, &mut records);
        let expected = JournalRecord {
            file_id: 7,
            parent_id: 5,
            name: "Résumé.txt".into(),
            reasons: REASON_FILE_CREATE,
            is_folder: false,
            usn: 99,
        };
        assert_eq!(records, [expected.clone(), expected]);
    }

    /// The unprivileged catch-up (ADR 0034): changes made in a folder of
    /// the test's are read back from the volume's journal, without
    /// administrator rights, and resolve to their paths. Pane's CI runs
    /// it on the runner's NTFS volume; it says so and passes when the
    /// temporary folder's volume keeps no journal. Run as a user without
    /// administrator rights for the evidence the ticket asks for.
    #[cfg(windows)]
    #[test]
    fn the_journal_is_read_without_administrator_rights_and_resolves_to_paths() {
        use crate::file_index::{Entry, FileIndex};
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().canonicalize().unwrap();
        let home = PathBuf::from(
            home.to_string_lossy()
                .strip_prefix(r"\\?\")
                .map(str::to_owned)
                .unwrap_or_else(|| home.to_string_lossy().into_owned()),
        );
        fs::create_dir_all(home.join("Documents")).unwrap();
        fs::write(home.join("Documents").join("old.txt"), b"old").unwrap();
        fs::create_dir_all(home.join("Drafts").join("2026")).unwrap();

        let start = match read_journal(&home, None, 1_000_000) {
            JournalRead::Records { cursor, records } => {
                assert!(records.is_empty());
                cursor
            }
            JournalRead::NoJournal => {
                eprintln!("the volume of {} keeps no change journal", home.display());
                return;
            }
            other => {
                panic!("the journal could not be read without administrator rights: {other:?}")
            }
        };

        // An index of the folders, as a first walk leaves it.
        let index_dir = tempfile::tempdir().unwrap();
        let (index, _) = FileIndex::open(index_dir.path(), std::slice::from_ref(&home)).unwrap();
        let scope =
            crate::file_index::Scope::new(crate::file_index::ScopeRules::for_home(home.clone()));
        let found = std::sync::Mutex::new(Vec::<Entry>::new());
        crate::file_index::walk(
            &scope,
            &crate::file_index::WalkOptions::default(),
            &std::sync::atomic::AtomicBool::new(false),
            &|batch| found.lock().unwrap().extend(batch),
        );
        let mut bulk = index.bulk().unwrap();
        bulk.add(index.prepare(found.into_inner().unwrap()))
            .unwrap();
        bulk.finish().unwrap();

        // Changes while "Pane is not running".
        fs::write(home.join("Documents").join("new plan.txt"), b"new").unwrap();
        fs::remove_file(home.join("Documents").join("old.txt")).unwrap();
        fs::rename(home.join("Drafts"), home.join("Final")).unwrap();
        fs::write(home.join("Final").join("2026").join("letter.txt"), b"x").unwrap();

        let records = match read_journal(&home, Some(&start), 1_000_000) {
            JournalRead::Records { records, cursor } => {
                assert_eq!(cursor.journal_id, start.journal_id);
                assert!(cursor.next_usn > start.next_usn);
                records
            }
            other => panic!("the journal could not be read from the cursor: {other:?}"),
        };
        let mut folders = index.folder_ids();
        // Without administrator rights the records carry no names: they are
        // named by their ids.
        let mut names = Names::for_volume_of(&home);
        let catch_up = resolve(&records, &mut folders, &mut |id| names.name(id));
        let documents = home.join("Documents");
        assert!(
            catch_up.touched.contains(&documents.join("new plan.txt")),
            "{catch_up:?}"
        );
        // A file gone is named by the record (with administrator rights), or
        // found missing from its folder.
        let old = documents.join("old.txt");
        assert!(
            catch_up.removed.contains(&old)
                || crate::file_index::missing_from(&index, &catch_up.listed)
                    .contains(&crate::file_index::Change::Remove(old.clone())),
            "{catch_up:?}"
        );
        assert!(
            catch_up.removed_folders.contains(&home.join("Drafts")),
            "{catch_up:?}"
        );
        assert!(catch_up.walk.contains(&home.join("Final")), "{catch_up:?}");
        assert!(
            catch_up
                .touched
                .contains(&home.join("Final").join("2026").join("letter.txt")),
            "{catch_up:?}"
        );
        // The index's own files, elsewhere on the volume, resolve to
        // nothing: their folder is not indexed.
        assert!(
            catch_up
                .touched
                .iter()
                .chain(&catch_up.removed)
                .all(|path| path.starts_with(&home)),
            "{catch_up:?}"
        );
    }
}
