//! The parallel walk (#126, "Scope and rules", "Priority and safety
//! valves"): every folder under the roots that the rules admit is listed
//! once, on threads of the walker's own at background priority, and its
//! entries handed to a sink in batches. Links are indexed, never followed.
//! On Windows each folder is read with the file ids of its entries in the
//! same call, so that change-journal records resolve to index entries.
//!
//! A folder that does not answer within [`WalkOptions::hung_after`] (a
//! stalled network mount, a dying disk) is skipped for this walk and
//! listed ([`WalkReport::hung_folders`]): each walker thread lists through
//! a helper thread of its own ([`Lister`]), and leaves a helper that hangs
//! behind, so that the folder holds up only itself (#176).

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use super::Entry;
use super::format::{EntryKind, Meta};
use super::scope::{CACHE_TAG, Candidate, Context, OwnFiles, Scope};

/// The ceiling of a walk (#126's proposed value): it stops after this many
/// entries, and says so.
pub const MAX_ENTRIES: u64 = 5_000_000;

/// How long a folder may take to answer before a walk skips it (#126's
/// "Hung folders").
pub const HUNG_AFTER: Duration = Duration::from_secs(10);

/// How a walk runs.
#[derive(Clone, Debug)]
pub struct WalkOptions {
    /// Threads listing folders at once.
    pub threads: usize,
    /// The ceiling: a walk stops after this many entries, and says so.
    pub max_entries: u64,
    /// Run the walk's threads at background priority.
    pub background: bool,
    /// A folder whose listing takes longer is skipped for this walk and
    /// listed as not answering ([`HUNG_AFTER`]); `None` waits for every
    /// folder as long as it takes, listing on the walker's own threads.
    pub hung_after: Option<Duration>,
}

impl Default for WalkOptions {
    fn default() -> WalkOptions {
        WalkOptions {
            threads: std::thread::available_parallelism()
                .map_or(4, |n| n.get())
                .clamp(2, 8),
            max_entries: MAX_ENTRIES,
            background: true,
            hung_after: Some(HUNG_AFTER),
        }
    }
}

/// What a walk did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WalkReport {
    /// Entries handed to the sink, folders included.
    pub entries: u64,
    pub folders: u64,
    /// Entries the rules left out (a folder counts once, not its contents).
    pub excluded: u64,
    /// Folders that could not be listed: they are indexed, their contents
    /// are not. At most the first 100 are named. A folder macOS's privacy
    /// protection refused is counted apart ([`WalkReport::refused`]).
    pub unreadable: u64,
    pub unreadable_folders: Vec<PathBuf>,
    /// Folders macOS's privacy protection did not let Pane read (Desktop,
    /// Documents, Downloads, a removable or network volume), told from the
    /// system's answer (see `privacy`): indexed, their contents not. At most
    /// the first 100 are named.
    pub refused: u64,
    pub refused_folders: Vec<PathBuf>,
    /// Folders that did not answer within [`WalkOptions::hung_after`]:
    /// indexed, their contents skipped for this walk. At most the first 100
    /// are named.
    pub hung: u64,
    pub hung_folders: Vec<PathBuf>,
    /// The walk stopped at [`WalkOptions::max_entries`].
    pub ceiling_reached: bool,
    pub cancelled: bool,
}

/// Why a folder was not listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unlisted {
    /// The system refused or failed to list it.
    Unreadable,
    /// macOS's privacy protection refused Pane (see `privacy`).
    Refused,
    /// It did not answer in time.
    Hung,
}

/// A helper thread that lists folders for one walker thread (or one
/// reconciling walk), so that a folder that never answers holds up only the
/// helper: the walker waits [`WalkOptions::hung_after`] and then leaves it
/// behind, and the helper ends once the system answers it at last.
pub(crate) struct Lister {
    asks: Sender<(PathBuf, u64)>,
    answers: Receiver<io::Result<Vec<Listed>>>,
}

impl Lister {
    fn start(background: bool) -> Option<Lister> {
        let (asks, asked) = channel::<(PathBuf, u64)>();
        let (answer, answers) = channel();
        std::thread::Builder::new()
            .name("pane-file-lister".into())
            .spawn(move || {
                if background {
                    super::priority::lower_current_thread();
                }
                for (path, volume) in asked {
                    #[cfg(test)]
                    stall_for_tests(&path);
                    if answer.send(list(&path, volume)).is_err() {
                        // Left behind: nobody waits for this answer.
                        break;
                    }
                }
            })
            .ok()?;
        Some(Lister { asks, answers })
    }
}

/// Lists the folder at `path`, on the volume `volume`, waiting at most
/// `options.hung_after` for it through `lister` (started here when there is
/// none, and dropped when it is left behind with a folder that hangs).
pub(crate) fn list_within(
    path: &Path,
    volume: u64,
    options: &WalkOptions,
    lister: &mut Option<Lister>,
) -> Result<Vec<Listed>, Unlisted> {
    let Some(limit) = options.hung_after else {
        return list(path, volume).map_err(|error| unlisted(&error));
    };
    if lister.is_none() {
        *lister = Lister::start(options.background);
    }
    let Some(helper) = lister.as_ref() else {
        // No thread to spare: listed here, as long as it takes.
        return list(path, volume).map_err(|error| unlisted(&error));
    };
    if helper.asks.send((path.to_path_buf(), volume)).is_err() {
        *lister = None;
        return list(path, volume).map_err(|error| unlisted(&error));
    }
    let asleep = super::power::asleep_since_start();
    let mut answer = helper.answers.recv_timeout(limit);
    // The computer slept while it waited: the sleep is no time the folder
    // took to answer, so it is given its limit again, awake (the limit
    // counts awake time only).
    if matches!(answer, Err(RecvTimeoutError::Timeout))
        && super::power::asleep_since_start().saturating_sub(asleep) >= super::power::NOTICED
    {
        answer = helper.answers.recv_timeout(limit);
    }
    match answer {
        Ok(Ok(listing)) => Ok(listing),
        Ok(Err(error)) => Err(unlisted(&error)),
        Err(RecvTimeoutError::Timeout) => {
            // Left behind; the next folder gets a helper of its own.
            *lister = None;
            Err(Unlisted::Hung)
        }
        Err(RecvTimeoutError::Disconnected) => {
            *lister = None;
            Err(Unlisted::Unreadable)
        }
    }
}

/// Why a folder the system did not list was not: macOS's privacy
/// protection refused it, or it could not be read.
fn unlisted(error: &io::Error) -> Unlisted {
    if super::privacy::refused_by_privacy(error) {
        Unlisted::Refused
    } else {
        Unlisted::Unreadable
    }
}

/// Folders whose listing the tests hold up, with how long, standing for a
/// stalled mount.
#[cfg(test)]
pub(crate) static STALLED: Mutex<Vec<(PathBuf, Duration)>> = Mutex::new(Vec::new());

#[cfg(test)]
fn stall_for_tests(path: &Path) {
    let stall = STALLED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .find(|(stalled, _)| stalled == path)
        .map(|(_, how_long)| *how_long);
    if let Some(how_long) = stall {
        std::thread::sleep(how_long);
    }
}

/// One entry of a folder, as listed.
pub(crate) struct Listed {
    pub(crate) name: OsString,
    pub(crate) kind: EntryKind,
    pub(crate) size: u64,
    pub(crate) modified: u64,
    pub(crate) file_id: u64,
    pub(crate) volume: u64,
    pub(crate) hidden_attribute: bool,
}

/// A folder to list: its own entry (the parent's listing gave it) and the
/// context of the folder it is in.
struct Job {
    path: PathBuf,
    meta: Meta,
    /// `None` for a root: its context comes from its own listing.
    parent: Option<Context>,
    root: usize,
}

struct Queue {
    jobs: Vec<Job>,
    /// Jobs taken and not finished.
    active: usize,
}

struct Shared<'a> {
    scope: &'a Scope,
    options: &'a WalkOptions,
    cancel: &'a AtomicBool,
    queue: Mutex<Queue>,
    wake: Condvar,
    entries: AtomicU64,
    folders: AtomicU64,
    excluded: AtomicU64,
    ceiling: AtomicBool,
    unreadable: Mutex<(u64, Vec<PathBuf>)>,
    refused: Mutex<(u64, Vec<PathBuf>)>,
    hung: Mutex<(u64, Vec<PathBuf>)>,
}

/// Walks every root of `scope` (or, with [`walk_folders`], some folders
/// below them), handing each folder's admitted entries to `sink` from the
/// walker's threads. Stops early when `cancel` is set.
pub fn walk(
    scope: &Scope,
    options: &WalkOptions,
    cancel: &AtomicBool,
    sink: &(dyn Fn(Vec<Entry>) + Sync),
) -> WalkReport {
    let mut jobs = Vec::new();
    for (root, path) in scope.rules().roots.iter().enumerate() {
        if let Ok(meta) = meta_at(path) {
            jobs.push(Job {
                path: path.clone(),
                meta,
                parent: None,
                root,
            });
        }
    }
    run(scope, options, cancel, sink, jobs)
}

/// Walks the folders `folders` again, each with everything under it the
/// rules admit, as a catch-up does for a folder renamed or moved into the
/// scope. A folder the rules do not admit is skipped.
pub fn walk_folders(
    scope: &Scope,
    folders: &[PathBuf],
    options: &WalkOptions,
    cancel: &AtomicBool,
    sink: &(dyn Fn(Vec<Entry>) + Sync),
) -> WalkReport {
    let mut known = super::scope::Admitted::default();
    let mut jobs = Vec::new();
    for folder in folders {
        let Some(root) = scope.root_of(folder) else {
            continue;
        };
        let Ok(meta) = meta_at(folder) else {
            continue;
        };
        if meta.kind != EntryKind::Folder || !scope.admits(folder, true, &mut known) {
            continue;
        }
        let parent = match folder.parent() {
            Some(parent) if *folder != scope.rules().roots[root] => {
                scope.context_at(root, parent, &mut known)
            }
            _ => None,
        };
        jobs.push(Job {
            path: folder.clone(),
            meta,
            parent,
            root,
        });
    }
    run(scope, options, cancel, sink, jobs)
}

fn run(
    scope: &Scope,
    options: &WalkOptions,
    cancel: &AtomicBool,
    sink: &(dyn Fn(Vec<Entry>) + Sync),
    jobs: Vec<Job>,
) -> WalkReport {
    let shared = Shared {
        scope,
        options,
        cancel,
        queue: Mutex::new(Queue { jobs, active: 0 }),
        wake: Condvar::new(),
        entries: AtomicU64::new(0),
        folders: AtomicU64::new(0),
        excluded: AtomicU64::new(0),
        ceiling: AtomicBool::new(false),
        unreadable: Mutex::new((0, Vec::new())),
        refused: Mutex::new((0, Vec::new())),
        hung: Mutex::new((0, Vec::new())),
    };
    std::thread::scope(|threads| {
        for n in 0..options.threads.max(1) {
            let shared = &shared;
            let spawned = std::thread::Builder::new()
                .name(format!("pane-file-walker-{n}"))
                .spawn_scoped(threads, move || {
                    if shared.options.background {
                        super::priority::lower_current_thread();
                    }
                    work(shared, sink);
                });
            if spawned.is_err() && n == 0 {
                // Not even one thread: walk on this one.
                work(shared, sink);
            }
        }
    });
    let (unreadable, unreadable_folders) = shared
        .unreadable
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (refused, refused_folders) = shared
        .refused
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (hung, hung_folders) = shared
        .hung
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    WalkReport {
        entries: shared.entries.into_inner(),
        folders: shared.folders.into_inner(),
        excluded: shared.excluded.into_inner(),
        unreadable,
        unreadable_folders,
        refused,
        refused_folders,
        hung,
        hung_folders,
        ceiling_reached: shared.ceiling.into_inner(),
        cancelled: cancel.load(Ordering::Relaxed),
    }
}

/// Takes jobs until none is left and none is running.
fn work(shared: &Shared<'_>, sink: &(dyn Fn(Vec<Entry>) + Sync)) {
    // This thread's helper for listing, started with its first folder.
    let mut lister = None;
    loop {
        let job = {
            let mut queue = shared
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            loop {
                if shared.cancel.load(Ordering::Relaxed) || shared.ceiling.load(Ordering::Relaxed) {
                    queue.jobs.clear();
                }
                if let Some(job) = queue.jobs.pop() {
                    queue.active += 1;
                    break Some(job);
                }
                if queue.active == 0 {
                    break None;
                }
                queue = shared
                    .wake
                    .wait(queue)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        };
        let Some(job) = job else {
            shared.wake.notify_all();
            return;
        };
        let (children, entries) = list_job(shared, job, &mut lister);
        if !entries.is_empty() {
            let count = entries.len() as u64;
            let total = shared.entries.fetch_add(count, Ordering::Relaxed) + count;
            if total >= shared.options.max_entries {
                shared.ceiling.store(true, Ordering::Relaxed);
            }
            sink(entries);
        }
        let mut queue = shared
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        queue.active -= 1;
        let woken = !children.is_empty() || queue.active == 0;
        queue.jobs.extend(children);
        drop(queue);
        if woken {
            shared.wake.notify_all();
        }
    }
}

/// Lists one folder: the folders under it to list next, and the entries to
/// index (the folder's own first).
fn list_job(shared: &Shared<'_>, job: Job, lister: &mut Option<Lister>) -> (Vec<Job>, Vec<Entry>) {
    let listing = match list_within(&job.path, job.meta.volume, shared.options, lister) {
        Ok(listing) => listing,
        Err(why) => {
            let counted = match why {
                Unlisted::Unreadable => &shared.unreadable,
                Unlisted::Refused => &shared.refused,
                Unlisted::Hung => &shared.hung,
            };
            let mut counted = counted
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            counted.0 += 1;
            if counted.1.len() < 100 {
                counted.1.push(job.path.clone());
            }
            drop(counted);
            shared.folders.fetch_add(1, Ordering::Relaxed);
            return (
                Vec::new(),
                vec![Entry {
                    path: job.path,
                    meta: job.meta,
                }],
            );
        }
    };
    let names = |wanted: &str| listing.iter().any(|entry| entry.name == *wanted);
    if names(CACHE_TAG) {
        shared.excluded.fetch_add(1, Ordering::Relaxed);
        return (Vec::new(), Vec::new());
    }
    let own = OwnFiles {
        ignore: names(".ignore"),
        gitignore: names(".gitignore"),
        git: names(".git"),
    };
    let context = match &job.parent {
        None => shared.scope.root_context(job.root, own),
        Some(parent) => shared.scope.folder_context(parent, &job.path, own),
    };
    shared.folders.fetch_add(1, Ordering::Relaxed);
    let mut entries = Vec::with_capacity(listing.len() + 1);
    entries.push(Entry {
        path: job.path.clone(),
        meta: job.meta,
    });
    let mut children = Vec::new();
    for listed in listing {
        let path = job.path.join(&listed.name);
        let candidate = Candidate {
            path: &path,
            name: &listed.name,
            is_dir: listed.kind == EntryKind::Folder,
            hidden_attribute: listed.hidden_attribute,
        };
        if shared.scope.excludes(&context, &candidate).is_some()
            || (listed.kind == EntryKind::Folder
                && !shared.scope.rules().include_other_volumes
                && listed.volume != job.meta.volume
                && other_volume(&path))
        {
            shared.excluded.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let meta = Meta {
            kind: listed.kind,
            size: listed.size,
            modified: listed.modified,
            file_id: listed.file_id,
            volume: listed.volume,
        };
        if listed.kind == EntryKind::Folder {
            children.push(Job {
                path,
                meta,
                parent: Some(context.clone()),
                root: job.root,
            });
        } else {
            entries.push(Entry { path, meta });
        }
    }
    (children, entries)
}

/// The entry at `path` itself, not following a link.
pub(crate) fn meta_at(path: &Path) -> io::Result<Meta> {
    let metadata = path.symlink_metadata()?;
    let kind = if metadata.is_dir() {
        EntryKind::Folder
    } else if metadata.is_symlink() {
        EntryKind::Link
    } else {
        EntryKind::File
    };
    let (file_id, volume) = file_id_and_volume(path, &metadata);
    Ok(Meta {
        kind,
        size: if metadata.is_dir() { 0 } else { metadata.len() },
        modified: seconds(metadata.modified().ok()),
        file_id,
        volume,
    })
}

fn seconds(time: Option<std::time::SystemTime>) -> u64 {
    time.and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs())
}

#[cfg(unix)]
fn file_id_and_volume(_path: &Path, metadata: &std::fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (metadata.ino(), metadata.dev())
}

#[cfg(windows)]
fn file_id_and_volume(path: &Path, _metadata: &std::fs::Metadata) -> (u64, u64) {
    windows::file_id_and_volume(path).unwrap_or((0, 0))
}

#[cfg(not(any(unix, windows)))]
fn file_id_and_volume(_path: &Path, _metadata: &std::fs::Metadata) -> (u64, u64) {
    (0, 0)
}

/// Whether the folder at `path`, on another volume than its parent, is on a
/// network or removable file system, which a walk does not enter unless the
/// user includes other volumes. Only Linux mounts other volumes under a
/// home folder in practice; on Windows another volume under a root is
/// reached only through a junction or mount point, a link never followed.
#[cfg(target_os = "linux")]
fn other_volume(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    const NETWORK: [i64; 9] = [
        0x6969,      // NFS
        0x517B,      // SMB
        0xFF53_4D42, // CIFS
        0xFE53_4D42, // SMB2
        0x6573_5546, // FUSE (sshfs, rclone and the like)
        0x5346_414F, // AFS
        0x0BD0_0BD0, // Lustre
        0x0000_4D44, // FAT (removable drives)
        0x2011_BAB0, // exFAT
    ];
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return true;
    };
    // SAFETY: a plain C structure, for which all zeroes is a valid value.
    let mut stats: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: a NUL-terminated path and a structure of the call's own type.
    if unsafe { libc::statfs(path.as_ptr(), &mut stats) } != 0 {
        return true;
    }
    // `f_type` is an `i64` on the 64-bit targets Pane builds for, and narrower elsewhere.
    #[allow(clippy::unnecessary_cast)]
    let kind = stats.f_type as i64;
    NETWORK.contains(&kind)
}

#[cfg(not(target_os = "linux"))]
fn other_volume(_path: &Path) -> bool {
    false
}

/// The entries of the folder at `path`, on the volume `volume`, without
/// `.` and `..`.
#[cfg(not(windows))]
pub(crate) fn list(path: &Path, _volume: u64) -> io::Result<Vec<Listed>> {
    let mut listing = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let Ok(entry) = entry else {
            continue;
        };
        // Not following links: the entry's own metadata.
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let file_type = metadata.file_type();
        let kind = if file_type.is_symlink() {
            EntryKind::Link
        } else if file_type.is_dir() {
            EntryKind::Folder
        } else {
            EntryKind::File
        };
        let (file_id, volume) = file_id_and_volume(&entry.path(), &metadata);
        listing.push(Listed {
            name: entry.file_name(),
            kind,
            size: if kind == EntryKind::Folder {
                0
            } else {
                metadata.len()
            },
            modified: seconds(metadata.modified().ok()),
            file_id,
            volume,
            hidden_attribute: false,
        });
    }
    Ok(listing)
}

#[cfg(windows)]
pub(crate) fn list(path: &Path, volume: u64) -> io::Result<Vec<Listed>> {
    windows::list(path, volume)
}

#[cfg(windows)]
mod windows {
    use std::ffi::OsString;
    use std::io;
    use std::os::windows::ffi::OsStringExt;
    use std::path::Path;

    use ::windows::Win32::Foundation::{CloseHandle, HANDLE};
    use ::windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_BOTH_DIR_INFO, FILE_LIST_DIRECTORY,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo, GetFileInformationByHandle,
        GetFileInformationByHandleEx, OPEN_EXISTING,
    };
    use ::windows::core::PCWSTR;

    use super::{EntryKind, Listed};

    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    /// A reparse tag naming another file or folder (a symbolic link, a
    /// junction, a mount point), as opposed to one of cloud files, whose
    /// placeholders are listed like ordinary entries.
    const NAME_SURROGATE: u32 = 0x2000_0000;
    const ERROR_NO_MORE_FILES: i32 = 18;
    /// 100-ns intervals between 1601 and 1970.
    const UNIX_EPOCH_FILETIME: i64 = 116_444_736_000_000_000;

    fn failed(error: ::windows::core::Error) -> io::Error {
        io::Error::from_raw_os_error(error.code().0 & 0xFFFF)
    }

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: opened by `open` and closed once, here.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    fn open(path: &Path, access: u32) -> io::Result<Handle> {
        let wide = crate::util::wide(path);
        // SAFETY: `wide` is NUL-terminated; the handle is closed by `Handle`.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                None,
            )
        }
        .map_err(failed)?;
        Ok(Handle(handle))
    }

    fn unix_seconds(filetime: i64) -> u64 {
        u64::try_from((filetime - UNIX_EPOCH_FILETIME) / 10_000_000).unwrap_or(0)
    }

    /// A path's file id (its NTFS file reference number) and its volume's
    /// serial number.
    pub(super) fn file_id_and_volume(path: &Path) -> io::Result<(u64, u64)> {
        let handle = open(path, FILE_READ_ATTRIBUTES.0)?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: an open handle and a structure of the call's own type.
        unsafe { GetFileInformationByHandle(handle.0, &mut info) }.map_err(failed)?;
        Ok((
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
            u64::from(info.dwVolumeSerialNumber),
        ))
    }

    /// Lists a folder with `GetFileInformationByHandleEx`, which gives each
    /// entry's attributes, size, times and file id in one call per buffer.
    pub(super) fn list(path: &Path, volume: u64) -> io::Result<Vec<Listed>> {
        let handle = open(path, FILE_LIST_DIRECTORY.0)?;
        // u64s, so that the buffer is aligned for the entries.
        let mut buffer = vec![0u64; 64 * 1024 / 8];
        let mut class = FileIdBothDirectoryRestartInfo;
        let mut listing = Vec::new();
        loop {
            // SAFETY: `buffer` is writable for its whole size.
            let read = unsafe {
                GetFileInformationByHandleEx(
                    handle.0,
                    class,
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * 8) as u32,
                )
            };
            match read {
                Ok(()) => {}
                Err(error) if error.code().0 & 0xFFFF == ERROR_NO_MORE_FILES => break,
                Err(error) => return Err(failed(error)),
            }
            class = FileIdBothDirectoryInfo;
            let bytes = buffer.len() * 8;
            let base = buffer.as_ptr().cast::<u8>();
            let mut offset = 0usize;
            loop {
                if offset + size_of::<FILE_ID_BOTH_DIR_INFO>() > bytes {
                    break;
                }
                // SAFETY: inside the buffer, at an entry the call wrote
                // (entries are 8-byte aligned), read without assuming more.
                let info = unsafe {
                    std::ptr::read_unaligned(base.add(offset).cast::<FILE_ID_BOTH_DIR_INFO>())
                };
                let name_len = info.FileNameLength as usize / 2;
                let name_at = offset + std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
                if name_at + name_len * 2 > bytes {
                    break;
                }
                // SAFETY: the name lies inside the buffer, checked above.
                let name: Vec<u16> = (0..name_len)
                    .map(|n| unsafe {
                        std::ptr::read_unaligned(base.add(name_at + n * 2).cast::<u16>())
                    })
                    .collect();
                let dots = name == [0x2E] || name == [0x2E, 0x2E];
                if !dots {
                    let attributes = info.FileAttributes;
                    let link = attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
                        && info.EaSize & NAME_SURROGATE != 0;
                    let kind = if link {
                        EntryKind::Link
                    } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                        EntryKind::Folder
                    } else {
                        EntryKind::File
                    };
                    listing.push(Listed {
                        name: OsString::from_wide(&name),
                        kind,
                        size: if kind == EntryKind::Folder {
                            0
                        } else {
                            u64::try_from(info.EndOfFile).unwrap_or(0)
                        },
                        modified: unix_seconds(info.LastWriteTime),
                        file_id: info.FileId as u64,
                        volume,
                        hidden_attribute: attributes
                            & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM)
                            != 0,
                    });
                }
                if info.NextEntryOffset == 0 {
                    break;
                }
                offset += info.NextEntryOffset as usize;
            }
        }
        Ok(listing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_index::scope::ScopeRules;
    use std::fs;
    use std::sync::Mutex;

    fn tree(files: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(&home).unwrap();
        for file in files {
            let path = home.join(file);
            if file.ends_with('/') {
                fs::create_dir_all(&path).unwrap();
            } else {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, b"12345").unwrap();
            }
        }
        (dir, home)
    }

    fn walked(scope: &Scope, options: &WalkOptions) -> (WalkReport, Vec<Entry>) {
        let found = Mutex::new(Vec::new());
        let report = walk(scope, options, &AtomicBool::new(false), &|batch| {
            found.lock().unwrap().extend(batch);
        });
        let mut found = found.into_inner().unwrap();
        found.sort_by(|a, b| a.path.cmp(&b.path));
        (report, found)
    }

    fn relative(home: &Path, entries: &[Entry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| {
                entry
                    .path
                    .strip_prefix(home)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn the_walk_finds_what_the_rules_admit_and_nothing_else() {
        let (_dir, home) = tree(&[
            "Documents/Résumé plan ü.txt",
            "Documents/deep/a/b/c/d/e/f/g/h/i/j/leaf.txt",
            "empty/",
            ".hidden/secret.txt",
            "code/repo/.git/HEAD",
            "code/repo/.gitignore",
            "code/repo/target/debug.bin",
            "code/repo/src/main.rs",
            "code/repo/node_modules/x.js",
            "tagged/CACHEDIR.TAG",
            "tagged/blob",
        ]);
        fs::write(home.join("code/repo/.gitignore"), "target/\n").unwrap();
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        let options = WalkOptions {
            threads: 4,
            ..WalkOptions::default()
        };
        let (report, entries) = walked(&scope, &options);
        let found = relative(&home, &entries);
        let expected = [
            "",
            "Documents",
            "Documents/Résumé plan ü.txt",
            "Documents/deep",
            "Documents/deep/a",
            "Documents/deep/a/b",
            "Documents/deep/a/b/c",
            "Documents/deep/a/b/c/d",
            "Documents/deep/a/b/c/d/e",
            "Documents/deep/a/b/c/d/e/f",
            "Documents/deep/a/b/c/d/e/f/g",
            "Documents/deep/a/b/c/d/e/f/g/h",
            "Documents/deep/a/b/c/d/e/f/g/h/i",
            "Documents/deep/a/b/c/d/e/f/g/h/i/j",
            "Documents/deep/a/b/c/d/e/f/g/h/i/j/leaf.txt",
            "code",
            "code/repo",
            "code/repo/src",
            "code/repo/src/main.rs",
            "empty",
        ];
        let mut expected: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
        expected.sort();
        let mut found_sorted = found.clone();
        found_sorted.sort();
        assert_eq!(found_sorted, expected);
        assert_eq!(report.entries, expected.len() as u64);
        assert!(!report.ceiling_reached && !report.cancelled);
        let document = entries
            .iter()
            .find(|entry| entry.path.ends_with("Résumé plan ü.txt"))
            .unwrap();
        assert_eq!(document.meta.kind, EntryKind::File);
        assert_eq!(document.meta.size, 5);
        assert!(document.meta.modified > 1_600_000_000);
        assert_ne!(document.meta.file_id, 0);
        let folder = entries
            .iter()
            .find(|entry| entry.path.ends_with("empty"))
            .unwrap();
        assert_eq!(folder.meta.kind, EntryKind::Folder);
    }

    #[test]
    fn one_thread_finds_the_same_as_many() {
        let files: Vec<String> = (0..300)
            .map(|n| format!("f{}/g{}/file {n}.txt", n % 10, n % 7))
            .collect();
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        let (_dir, home) = tree(&files);
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        let one = WalkOptions {
            threads: 1,
            background: false,
            ..WalkOptions::default()
        };
        let many = WalkOptions {
            threads: 8,
            ..WalkOptions::default()
        };
        let (_, a) = walked(&scope, &one);
        let (_, b) = walked(&scope, &many);
        assert_eq!(relative(&home, &a), relative(&home, &b));
        // The home folder, 10 + 70 folders and 300 files.
        assert_eq!(a.len(), 1 + 10 + 70 + 300);
    }

    #[test]
    fn the_walk_stops_at_the_ceiling_and_when_cancelled() {
        let files: Vec<String> = (0..200)
            .map(|n| format!("d{}/file {n}.txt", n % 20))
            .collect();
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        let (_dir, home) = tree(&files);
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        let options = WalkOptions {
            threads: 2,
            max_entries: 30,
            background: false,
            hung_after: None,
        };
        let (report, entries) = walked(&scope, &options);
        assert!(report.ceiling_reached);
        assert!(entries.len() < 200, "{}", entries.len());
        let cancel = AtomicBool::new(true);
        let report = walk(&scope, &WalkOptions::default(), &cancel, &|_| {});
        assert!(report.cancelled);
        assert_eq!(report.entries, 0);
    }

    #[test]
    fn a_folder_that_does_not_answer_is_skipped_for_the_walk_and_listed() {
        let (_dir, home) = tree(&["fast/one.txt", "stuck/two.txt", "stuck/deeper/three.txt"]);
        let stuck = home.join("stuck");
        STALLED
            .lock()
            .unwrap()
            .push((stuck.clone(), std::time::Duration::from_secs(3)));
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        let options = WalkOptions {
            threads: 2,
            background: false,
            hung_after: Some(std::time::Duration::from_millis(200)),
            ..WalkOptions::default()
        };
        let started = std::time::Instant::now();
        let (report, entries) = walked(&scope, &options);
        STALLED.lock().unwrap().retain(|(path, _)| *path != stuck);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "the walk did not wait for the folder"
        );
        let mut found = relative(&home, &entries);
        found.sort();
        // The folder itself is indexed; what is in it is skipped this time.
        assert_eq!(found, ["", "fast", "fast/one.txt", "stuck"]);
        assert_eq!(report.hung, 1);
        assert_eq!(report.hung_folders, [stuck]);
        assert_eq!(report.unreadable, 0);
    }

    #[test]
    fn a_renamed_folder_is_walked_again_alone() {
        let (_dir, home) = tree(&["a/one.txt", "b/two.txt", "b/.hidden/x.txt"]);
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        let found = Mutex::new(Vec::new());
        walk_folders(
            &scope,
            &[home.join("b"), home.join("b/.hidden"), home.join("missing")],
            &WalkOptions::default(),
            &AtomicBool::new(false),
            &|batch| found.lock().unwrap().extend(batch),
        );
        let mut found = relative(&home, &found.into_inner().unwrap());
        found.sort();
        assert_eq!(found, ["b", "b/two.txt"]);
    }

    #[cfg(unix)]
    #[test]
    fn links_are_indexed_and_never_followed() {
        let (_dir, home) = tree(&["real/file.txt"]);
        std::os::unix::fs::symlink(home.join("real"), home.join("link")).unwrap();
        std::os::unix::fs::symlink(&home, home.join("real/loop")).unwrap();
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        let (_, entries) = walked(&scope, &WalkOptions::default());
        let mut found = relative(&home, &entries);
        found.sort();
        assert_eq!(found, ["", "link", "real", "real/file.txt", "real/loop"]);
        let link = entries.iter().find(|e| e.path.ends_with("link")).unwrap();
        assert_eq!(link.meta.kind, EntryKind::Link);
    }

    #[cfg(windows)]
    #[test]
    fn junctions_are_indexed_and_never_followed_and_hidden_attributes_are_left_out() {
        let (_dir, home) = tree(&["real/file.txt", "attr/x.txt"]);
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(home.join("junction"))
            .arg(home.join("real"))
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
        let hidden = std::process::Command::new("attrib")
            .arg("+H")
            .arg(home.join("attr"))
            .output()
            .unwrap();
        assert!(hidden.status.success(), "{hidden:?}");
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        let (_, entries) = walked(&scope, &WalkOptions::default());
        let mut found = relative(&home, &entries);
        found.sort();
        assert_eq!(found, ["", "junction", "real", "real/file.txt"]);
        let junction = entries
            .iter()
            .find(|e| e.path.ends_with("junction"))
            .unwrap();
        assert_eq!(junction.meta.kind, EntryKind::Link);
    }
}
