//! Linux: the system keeps no change history Pane can read without
//! privileges (fanotify needs them), so the catch-up at start is a
//! reconciling walk, which reads only the folders whose modified time
//! changed. Live changes come from inotify, one watch per indexed folder
//! (and one on a repository's `.git/info`, whose `exclude` sets ignore
//! rules, #186), shallowest first; once the per-user watch limit
//! (`fs.inotify.max_user_watches`) is reached, the folders left are
//! reported as unwatched, and the coordinator reconciles them every few
//! minutes and says so on the File search page.

use std::collections::HashMap;
use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::{Caught, CaughtUpBy, ChangeSource, Changed, FolderIds, Sink, Watching};
use crate::file_index::journal::JournalCursor;
use crate::file_index::reconcile::reconcile;
use crate::file_index::scope::Scope;
use crate::file_index::store::FileIndex;
use crate::file_index::walker::WalkOptions;

const MASK: u32 = libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_CLOSE_WRITE
    | libc::IN_ATTRIB
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF
    | libc::IN_ONLYDIR
    | libc::IN_DONT_FOLLOW
    | libc::IN_EXCL_UNLINK;

#[derive(Default)]
pub(crate) struct Inotify;

impl ChangeSource for Inotify {
    fn cursors(&self, _scope: &Scope) -> Vec<JournalCursor> {
        Vec::new()
    }

    fn catch_up(
        &self,
        index: &FileIndex,
        scope: &Scope,
        _cursors: &[JournalCursor],
        _folders: &mut FolderIds<'_>,
        cancel: &AtomicBool,
    ) -> Caught {
        let reconciled = reconcile(
            index,
            scope,
            &scope.kept_roots(),
            &WalkOptions::default(),
            cancel,
        );
        Caught::Changes {
            changes: reconciled.changes,
            walk: Vec::new(),
            reconcile: Vec::new(),
            // A folder read again holding an ignore file or a repository: the
            // rules below it may have changed with it (#186).
            recheck: reconciled.recheck,
            cursors: Vec::new(),
            how: CaughtUpBy::ReconcilingWalk,
            note: None,
        }
    }

    /// One watch per indexed folder.
    fn watches_folders(&self) -> bool {
        true
    }

    fn watch(
        &self,
        scope: &Scope,
        _cursors: &[JournalCursor],
        folders: Vec<PathBuf>,
        sink: Sink,
    ) -> Result<Box<dyn Watching>, String> {
        // SAFETY: no pointers; the descriptor is closed by `Shared`'s drop.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(format!(
                "Pane could not watch for changes: {}",
                std::io::Error::last_os_error()
            ));
        }
        let shared = Arc::new(Shared {
            fd,
            stop: AtomicBool::new(false),
            watched: Mutex::new(HashMap::new()),
            roots: scope.watched_roots(),
            sink,
        });
        let mut watch = Watch {
            shared: shared.clone(),
        };
        // A network share is reconciled now and then instead (the folders
        // given are not on one).
        let mut all = scope.watched_roots();
        all.extend(folders);
        watch.add_folders(&all);
        std::thread::Builder::new()
            .name("pane-file-watch".into())
            .spawn(move || shared.read_events())
            .map_err(|error| format!("Pane could not watch for changes: {error}"))?;
        Ok(Box::new(watch))
    }
}

struct Shared {
    fd: libc::c_int,
    stop: AtomicBool,
    /// Each watch's folder, by watch descriptor.
    watched: Mutex<HashMap<libc::c_int, PathBuf>>,
    roots: Vec<PathBuf>,
    sink: Sink,
}

impl Drop for Shared {
    fn drop(&mut self) {
        // SAFETY: the descriptor opened in `watch`, closed once, here.
        unsafe { libc::close(self.fd) };
    }
}

impl Shared {
    fn watched(&self) -> std::sync::MutexGuard<'_, HashMap<libc::c_int, PathBuf>> {
        self.watched
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Reads events until the watch is dropped.
    fn read_events(&self) {
        crate::file_index::lower_current_thread();
        let mut buffer = vec![0u8; 64 * 1024];
        while !self.stop.load(Ordering::Relaxed) {
            let mut poll = libc::pollfd {
                fd: self.fd,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one valid pollfd. Half a second, so that a dropped
            // watch ends the thread soon after.
            let ready = unsafe { libc::poll(&mut poll, 1, 500) };
            if ready <= 0 {
                continue;
            }
            // SAFETY: reads into the buffer, at most its length.
            let read = unsafe {
                libc::read(
                    self.fd,
                    buffer.as_mut_ptr().cast::<libc::c_void>(),
                    buffer.len(),
                )
            };
            if read <= 0 {
                continue;
            }
            let paths = self.parse(&buffer[..read as usize]);
            if !paths.is_empty() && self.sink.send(Changed::Paths(paths)).is_err() {
                return;
            }
        }
    }

    /// The paths the events in `bytes` name; an overflow asks for every
    /// root to be reconciled.
    fn parse(&self, bytes: &[u8]) -> Vec<PathBuf> {
        let header = std::mem::size_of::<libc::inotify_event>();
        let mut paths = Vec::new();
        let mut at = 0;
        while at + header <= bytes.len() {
            // SAFETY: the kernel writes whole events; read unaligned.
            let event: libc::inotify_event = unsafe {
                std::ptr::read_unaligned(bytes[at..].as_ptr().cast::<libc::inotify_event>())
            };
            let name_len = event.len as usize;
            let name_bytes = bytes
                .get(at + header..at + header + name_len)
                .unwrap_or(&[]);
            at += header + name_len;
            if event.mask & libc::IN_Q_OVERFLOW != 0 {
                for root in &self.roots {
                    let _ = self.sink.send(Changed::Rescan(root.clone()));
                }
                continue;
            }
            let folder = self.watched().get(&event.wd).cloned();
            if event.mask & libc::IN_IGNORED != 0 {
                self.watched().remove(&event.wd);
                continue;
            }
            let Some(folder) = folder else {
                continue;
            };
            let name: &[u8] = match name_bytes.iter().position(|&byte| byte == 0) {
                Some(end) => &name_bytes[..end],
                None => name_bytes,
            };
            if name.is_empty() {
                // The watched folder itself: deleted or moved.
                paths.push(folder);
            } else {
                paths.push(folder.join(OsStr::from_bytes(name)));
            }
        }
        paths.sort();
        paths.dedup();
        paths
    }
}

struct Watch {
    shared: Arc<Shared>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

impl Watching for Watch {
    fn add_folders(&mut self, folders: &[PathBuf]) {
        let mut unwatched = Vec::new();
        let mut full = false;
        for folder in folders {
            if full {
                unwatched.push(folder.clone());
                continue;
            }
            match add_watch(self.shared.fd, folder) {
                Ok(wd) => {
                    self.shared.watched().insert(wd, folder.clone());
                    // A repository's own ignore rules (`.git/info/exclude`)
                    // are in a folder never indexed: watched too (#186), or
                    // the folder is reported unwatched, so that what the
                    // rules learned of it is read again each time instead.
                    let info = folder.join(".git").join("info");
                    if info.is_dir() {
                        match add_watch(self.shared.fd, &info) {
                            Ok(wd) => {
                                self.shared.watched().insert(wd, info);
                            }
                            Err(error) => {
                                full = error.raw_os_error() == Some(libc::ENOSPC);
                                unwatched.push(folder.clone());
                            }
                        }
                    }
                }
                Err(error) if error.raw_os_error() == Some(libc::ENOSPC) => {
                    // The watch limit: this folder and the rest are
                    // reconciled instead.
                    full = true;
                    unwatched.push(folder.clone());
                }
                // Gone or unreadable meanwhile: the next change says so.
                Err(_) => {}
            }
        }
        if !unwatched.is_empty() {
            let _ = self.shared.sink.send(Changed::Unwatched(unwatched));
        }
    }
}

fn add_watch(fd: libc::c_int, folder: &Path) -> std::io::Result<libc::c_int> {
    let path = CString::new(folder.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: a NUL-terminated path; the descriptor is ours.
    let wd = unsafe { libc::inotify_add_watch(fd, path.as_ptr(), MASK) };
    if wd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(wd)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::file_index::ScopeRules;
    use crate::file_index::indexer::Message;

    /// The paths inotify reports within a few seconds.
    fn reported(received: &std::sync::mpsc::Receiver<Message>, wanted: &Path) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match received.recv_timeout(left) {
                Ok(Message::Changed(Changed::Paths(paths)))
                    if paths.iter().any(|p| p == wanted) =>
                {
                    return true;
                }
                Ok(_) => {}
                Err(_) => return false,
            }
        }
        false
    }

    #[test]
    fn inotify_reports_a_change_in_a_watched_folder_and_stops_when_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs_create(&home.join("Documents"));
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        let (sender, received) = channel();
        let watch = Inotify
            .watch(
                &scope,
                &[],
                vec![home.join("Documents")],
                Sink::new(sender, None),
            )
            .unwrap();
        let created = home.join("Documents/plan.txt");
        std::fs::write(&created, "plan").unwrap();
        assert!(reported(&received, &created));
        // A folder made later is watched once the coordinator adds it.
        let mut watch = watch;
        fs_create(&home.join("Documents/Later"));
        watch.add_folders(&[home.join("Documents/Later")]);
        let later = home.join("Documents/Later/minutes.txt");
        std::fs::write(&later, "x").unwrap();
        assert!(reported(&received, &later));
        drop(watch);
        std::thread::sleep(Duration::from_millis(700));
        std::fs::write(home.join("Documents/after.txt"), "x").unwrap();
        assert!(!reported(&received, &home.join("Documents/after.txt")));
    }

    /// A repository's own ignore rules, in its `.git/info/exclude`, are
    /// watched with its folder (#186), though `.git` is never indexed.
    #[test]
    fn a_repositorys_own_ignore_rules_are_watched_with_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs_create(&home.join("repo/.git/info"));
        let scope = Scope::new(ScopeRules::for_home(home.clone()));
        let (sender, received) = channel();
        let _watch = Inotify
            .watch(
                &scope,
                &[],
                vec![home.join("repo")],
                Sink::new(sender, None),
            )
            .unwrap();
        let exclude = home.join("repo/.git/info/exclude");
        std::fs::write(&exclude, "*.draft\n").unwrap();
        assert!(reported(&received, &exclude));
    }

    fn fs_create(folder: &Path) {
        std::fs::create_dir_all(folder).unwrap();
    }
}
