//! Replacing a small file whole, for Pane's own records such as
//! `installed.json` and `settings.json`.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Who may read a file Pane writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Readers {
    /// Whoever the folder and the process's defaults allow (the umask on
    /// Unix).
    Default,
    /// Only the user Pane runs as: on Unix the file is created with mode
    /// 0600, whatever the umask. On Windows the file takes its folder's
    /// permissions, as with `Default`.
    OwnerOnly,
}

/// Distinguishes the temporary files of one process's writes.
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Replaces the file at `path` with `contents`, readable by `readers`,
/// creating its folder if needed. The new file never has wider permissions
/// than `readers`, even for a moment, and it replaces any earlier file's
/// permissions.
///
/// The contents go to a new temporary file in the same folder, which is
/// flushed to disk and then renamed over `path`; on Unix the folder is
/// flushed too, so the rename itself survives a power loss. A crash or power
/// loss therefore leaves either the old file or the new one, never a torn
/// one. (Windows offers no way to flush a folder, so there a power loss just
/// after the rename can still leave the old file.)
///
/// Each write uses its own temporary name, including the process id, so
/// writers in two processes or threads never share one. The file is not
/// locked, though: when two writers each read, change and write it back, the
/// later rename wins and the other change is lost.
pub(crate) fn write_atomically(path: &Path, contents: &[u8], readers: Readers) -> io::Result<()> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other(format!("{} names no file", path.display())))?;
    let temporary = dir.join(format!(
        ".{}.{}-{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        restrict(&mut options, readers);
        let mut file = options.open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written?;
    sync_dir(dir)
}

#[cfg(unix)]
fn restrict(options: &mut OpenOptions, readers: Readers) {
    use std::os::unix::fs::OpenOptionsExt;
    if readers == Readers::OwnerOnly {
        options.mode(0o600);
    }
}

#[cfg(not(unix))]
fn restrict(_options: &mut OpenOptions, _readers: Readers) {}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Readers, write_atomically};
    use std::fs;

    #[test]
    fn writers_that_overlap_each_leave_a_whole_file_and_no_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("records").join("installed.json");
        let payloads: Vec<String> = (0..8)
            .map(|writer| {
                format!(
                    "{{\"writer\": {writer}, \"padding\": \"{}\"}}",
                    "x".repeat(4096)
                )
            })
            .collect();
        std::thread::scope(|scope| {
            for payload in &payloads {
                let path = &path;
                scope.spawn(move || {
                    for _ in 0..50 {
                        match write_atomically(path, payload.as_bytes(), Readers::Default) {
                            Ok(()) => {}
                            // Windows can refuse a rename onto a file that
                            // another rename is replacing at that moment; the
                            // write fails whole, which is still not torn.
                            Err(error)
                                if cfg!(windows)
                                    && error.kind() == std::io::ErrorKind::PermissionDenied => {}
                            Err(error) => panic!("{error}"),
                        }
                    }
                });
            }
        });
        let written = fs::read_to_string(&path).unwrap();
        assert!(payloads.contains(&written), "a torn file: {written:.80}");
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["installed.json"]);
    }
}
