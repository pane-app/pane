//! The marker that tells the next start whether this run ended cleanly
//! (#133). At start Pane writes `running-<process id>.json` in the logs
//! folder, holding its process id, when the process started and Pane's
//! version; a clean quit removes it. Before writing its own, a start reads
//! every marker it finds and asks the system's process table about each
//! one's process:
//!
//! - no longer running, or running but started at another time (its id
//!   was given to another process since): that run ended unexpectedly, and
//!   the marker goes;
//! - still running, started when the marker says: another Pane on the same
//!   folder, whose marker is left alone.
//!
//! Where the system does not say when a process started, a running process
//! counts as the marker's, so a Pane still running is never reported as
//! crashed. The process table is a seam ([`ProcessTable`]): the system's
//! ([`SystemProcesses`]) or a test's.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// What the process table says of one process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    /// No process with that id runs (or one runs only as a zombie, whose
    /// end its parent has not collected).
    Ended,
    /// A process with that id runs, started at `started` (in the system's
    /// own units, only ever compared with another reading of the same
    /// system), or at a time the system does not say.
    Running { started: Option<u64> },
}

/// Which processes run: the system's table ([`SystemProcesses`]), or a
/// test's.
pub trait ProcessTable: Send + Sync {
    /// Whether the process with id `process` runs, and since when.
    fn look_up(&self, process: u32) -> Liveness;
}

/// The system's own process table: on Windows a process handle's exit code
/// and creation time, on Linux `/proc/<id>/stat`, on macOS the process's
/// BSD information.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProcesses;

impl ProcessTable for SystemProcesses {
    fn look_up(&self, process: u32) -> Liveness {
        platform::look_up(process)
    }
}

/// What a marker holds.
#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    process: u32,
    /// When the process started, as the process table said at the start.
    started: Option<u64>,
    /// The version of Pane that wrote it.
    pane: String,
}

/// This run's crash record: its marker in the logs folder, and whether a
/// run before it on the same folder ended unexpectedly. Made once at
/// start ([`super::start`]), or by a test over a folder of its own.
#[derive(Debug)]
pub struct CrashRecord {
    folder: PathBuf,
    marker: PathBuf,
    unexpected: bool,
    quit: AtomicBool,
}

impl CrashRecord {
    /// Reads the markers in the logs folder `folder` (made, readable by the
    /// user only, if it is not there), each against `processes`, removes
    /// those whose run ended, and writes this run's: process id, start
    /// time and `version`.
    pub fn open(folder: &Path, version: &str, processes: &dyn ProcessTable) -> CrashRecord {
        let _ = super::create_private_dir(folder);
        let own = std::process::id();
        let mut unexpected = false;
        if let Ok(entries) = fs::read_dir(folder) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_marker = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(is_marker_name);
                if !is_marker {
                    continue;
                }
                let marker = fs::read(&path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Marker>(&bytes).ok());
                match marker {
                    Some(marker) if !ended(&marker, processes) => {}
                    Some(_) => {
                        unexpected = true;
                        let _ = fs::remove_file(&path);
                    }
                    // A marker that cannot be read says nothing about how
                    // its run ended; it is not kept either.
                    None => {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
        let started = match processes.look_up(own) {
            Liveness::Running { started } => started,
            Liveness::Ended => None,
        };
        let marker = folder.join(marker_name(own));
        let record = Marker {
            process: own,
            started,
            pane: version.to_owned(),
        };
        if let Ok(json) = serde_json::to_vec_pretty(&record) {
            let _ =
                crate::atomic::write_atomically(&marker, &json, crate::atomic::Readers::OwnerOnly);
        }
        CrashRecord {
            folder: folder.to_path_buf(),
            marker,
            unexpected,
            quit: AtomicBool::new(false),
        }
    }

    /// The logs folder.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// This run's marker.
    pub fn marker(&self) -> &Path {
        &self.marker
    }

    /// Whether a run before this one on the same folder ended unexpectedly.
    pub fn ended_unexpectedly(&self) -> bool {
        self.unexpected
    }

    /// Records that this run quits cleanly: its marker goes, and the log
    /// says what it held back. Calling it again does nothing.
    pub fn clean_quit(&self) {
        if self.quit.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = fs::remove_file(&self.marker);
        super::finish();
    }
}

/// The marker's file name for process `process`.
fn marker_name(process: u32) -> String {
    format!("running-{process}.json")
}

fn is_marker_name(name: &str) -> bool {
    name.strip_prefix("running-")
        .and_then(|rest| rest.strip_suffix(".json"))
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Whether the run `marker` records has ended: its process no longer runs,
/// or the process with its id started at another time.
fn ended(marker: &Marker, processes: &dyn ProcessTable) -> bool {
    let started = match processes.look_up(marker.process) {
        Liveness::Ended => return true,
        Liveness::Running { started } => started,
    };
    // Where either start is not known, the running process counts as the
    // marker's.
    matches!((marker.started, started), (Some(then), Some(now)) if then != now)
}

#[cfg(windows)]
mod platform {
    use ::windows::Win32::Foundation::{CloseHandle, FILETIME, STILL_ACTIVE};
    use ::windows::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    use super::Liveness;

    /// `ERROR_ACCESS_DENIED`: the process runs, but Pane may not ask about
    /// it.
    const ACCESS_DENIED: i32 = 5;

    pub(super) fn look_up(process: u32) -> Liveness {
        // SAFETY: plain values; the handle is closed below.
        let opened = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) };
        let handle = match opened {
            Ok(handle) => handle,
            Err(error) if error.code().0 & 0xFFFF == ACCESS_DENIED => {
                return Liveness::Running { started: None };
            }
            // No such process.
            Err(_) => return Liveness::Ended,
        };
        let mut code = 0u32;
        // SAFETY: an open process handle; `code` is writable.
        let running = unsafe { GetExitCodeProcess(handle, &mut code) }.is_ok()
            && code == STILL_ACTIVE.0 as u32;
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: an open process handle; the times are writable.
        let timed = unsafe {
            GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user).is_ok()
        };
        // SAFETY: opened above.
        let _ = unsafe { CloseHandle(handle) };
        if !running {
            return Liveness::Ended;
        }
        let created = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        Liveness::Running {
            started: timed.then_some(created),
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::Liveness;

    pub(super) fn look_up(process: u32) -> Liveness {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{process}/stat")) else {
            return Liveness::Ended;
        };
        // `<id> (<name>) <state> <parent> …`: the name may hold spaces and
        // parentheses, so the fields are counted after its last `)`. The
        // state is the third field and the start time (in clock ticks
        // since boot) the twenty-second.
        let Some((_, fields)) = stat.rsplit_once(')') else {
            return Liveness::Running { started: None };
        };
        let fields: Vec<&str> = fields.split_whitespace().collect();
        if matches!(fields.first(), Some(&"Z" | &"X" | &"x")) {
            return Liveness::Ended;
        }
        let started = fields.get(22 - 3).and_then(|field| field.parse().ok());
        Liveness::Running { started }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Liveness;

    pub(super) fn look_up(process: u32) -> Liveness {
        let Ok(id) = libc::c_int::try_from(process) else {
            return Liveness::Ended;
        };
        // SAFETY: a plain C structure, for the call to fill.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: `info` is writable for `size` bytes.
        let written = unsafe {
            libc::proc_pidinfo(id, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size)
        };
        if written < size {
            return Liveness::Ended;
        }
        if info.pbi_status == libc::SZOMB {
            return Liveness::Ended;
        }
        let started = info
            .pbi_start_tvsec
            .saturating_mul(1_000_000)
            .saturating_add(info.pbi_start_tvusec);
        Liveness::Running {
            started: Some(started),
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use super::Liveness;

    /// A system whose process table Pane does not read: every marker's
    /// process counts as running, so none is reported as crashed.
    pub(super) fn look_up(_process: u32) -> Liveness {
        Liveness::Running { started: None }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    /// A process table a test writes: each process's start, or nothing for
    /// one that has ended.
    #[derive(Default)]
    struct FakeTable(Mutex<HashMap<u32, Option<u64>>>);

    impl FakeTable {
        fn running(&self, process: u32, started: u64) {
            self.0.lock().unwrap().insert(process, Some(started));
        }

        fn ended(&self, process: u32) {
            self.0.lock().unwrap().remove(&process);
        }
    }

    impl ProcessTable for FakeTable {
        fn look_up(&self, process: u32) -> Liveness {
            match self.0.lock().unwrap().get(&process) {
                Some(started) => Liveness::Running { started: *started },
                None => Liveness::Ended,
            }
        }
    }

    /// Writes the marker of another run: process `process`, started at
    /// `started`.
    fn marker_of(folder: &Path, process: u32, started: u64) -> PathBuf {
        let path = folder.join(marker_name(process));
        let marker = Marker {
            process,
            started: Some(started),
            pane: "0.0.1".into(),
        };
        fs::create_dir_all(folder).unwrap();
        fs::write(&path, serde_json::to_vec(&marker).unwrap()).unwrap();
        path
    }

    fn markers(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| is_marker_name(name))
            .collect();
        names.sort();
        names
    }

    /// A process id that is not this test's.
    const OTHER: u32 = 4_000_000_001;

    #[test]
    fn an_ended_processs_marker_is_an_unexpected_end() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("logs");
        let stale = marker_of(&folder, OTHER, 77);
        let table = FakeTable::default();
        table.running(std::process::id(), 5);

        let record = CrashRecord::open(&folder, "1.0.0", &table);
        assert!(record.ended_unexpectedly());
        assert!(!stale.exists(), "the ended run's marker goes");
        // This run's marker is written in its place, with its process and
        // its start.
        assert_eq!(markers(&folder), [marker_name(std::process::id())]);
        let bytes = fs::read(record.marker()).unwrap();
        let own: Marker = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            (own.process, own.started, own.pane.as_str()),
            (std::process::id(), Some(5), "1.0.0")
        );
    }

    #[test]
    fn a_running_processs_marker_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("logs");
        let other = marker_of(&folder, OTHER, 77);
        let table = FakeTable::default();
        table.running(OTHER, 77);
        table.running(std::process::id(), 5);

        let record = CrashRecord::open(&folder, "1.0.0", &table);
        assert!(!record.ended_unexpectedly(), "another Pane still runs");
        assert!(other.exists(), "its marker is left alone");
        // Sorted as `markers` sorts them: by name, so the order depends on
        // this test's process id.
        let mut both = [marker_name(OTHER), marker_name(std::process::id())];
        both.sort();
        assert_eq!(markers(&folder), both);
    }

    #[test]
    fn a_reused_process_id_with_another_start_counts_as_ended() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("logs");
        let stale = marker_of(&folder, OTHER, 77);
        let table = FakeTable::default();
        // The id now belongs to a process that started later.
        table.running(OTHER, 78);

        let record = CrashRecord::open(&folder, "1.0.0", &table);
        assert!(record.ended_unexpectedly());
        assert!(!stale.exists());
    }

    #[test]
    fn a_running_process_whose_start_the_system_does_not_say_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("logs");
        let other = marker_of(&folder, OTHER, 77);
        let table = FakeTable::default();
        table.0.lock().unwrap().insert(OTHER, None);

        let record = CrashRecord::open(&folder, "1.0.0", &table);
        assert!(!record.ended_unexpectedly());
        assert!(other.exists());
    }

    #[test]
    fn a_clean_quit_removes_the_marker_and_the_next_start_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("logs");
        let table = FakeTable::default();
        table.running(std::process::id(), 5);
        let first = CrashRecord::open(&folder, "1.0.0", &table);
        assert!(!first.ended_unexpectedly(), "a first start");
        first.clean_quit();
        first.clean_quit();
        assert!(markers(&folder).is_empty());
        // The process ended; a start after it finds nothing to report.
        table.ended(std::process::id());
        let second = CrashRecord::open(&folder, "1.0.0", &table);
        assert!(!second.ended_unexpectedly());
    }

    #[test]
    fn a_run_that_ended_without_quitting_is_reported_by_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("logs");
        let table = FakeTable::default();
        table.running(std::process::id(), 5);
        let first = CrashRecord::open(&folder, "1.0.0", &table);
        // The process ends without a clean quit.
        drop(first);
        table.ended(std::process::id());
        let second = CrashRecord::open(&folder, "1.0.0", &table);
        assert!(second.ended_unexpectedly());
    }

    #[test]
    fn this_process_runs_by_the_systems_own_table() {
        let found = SystemProcesses.look_up(std::process::id());
        assert!(
            matches!(found, Liveness::Running { .. }),
            "this test's own process: {found:?}"
        );
        // Asked twice, the same start.
        assert_eq!(found, SystemProcesses.look_up(std::process::id()));
        if cfg!(any(windows, target_os = "linux", target_os = "macos")) {
            assert!(
                matches!(found, Liveness::Running { started: Some(_) }),
                "{found:?}"
            );
        }
        // An id no system gives a process (beyond Linux's and macOS's
        // limits, and far beyond the ids Windows hands out) has ended.
        if cfg!(any(windows, target_os = "linux", target_os = "macos")) {
            assert_eq!(SystemProcesses.look_up(4_000_000_000), Liveness::Ended);
        }
    }
}
