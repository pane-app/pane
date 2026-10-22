//! What kind of volume holds a folder (#126 "Scope and rules", #184):
//! network shares and removable drives are left out of the index unless the
//! user includes other volumes (`ScopeRules::include_other_volumes`), and a
//! network share that is included is never watched, only reconciled. The
//! scope asks through [`VolumeKinds`], so that tests can say what a folder
//! is on; the system's answer is [`volume_kind`]:
//!
//! - Windows: a network path (`\\server\share`) is a network share by its
//!   text, before any call; any other path is resolved to the root of the
//!   volume holding it (`GetVolumePathNameW`: a drive letter, a mapped
//!   drive's letter, or the folder a volume is mounted in) and that root's
//!   drive type asked (`GetDriveTypeW`): remote is a network share,
//!   removable or CD-ROM a removable drive.
//! - macOS: `statfs`'s flags: a file system without `MNT_LOCAL` is a network
//!   share, one with `MNT_REMOVABLE` (removable media) a removable drive.
//! - Linux: `statfs`'s file system type: NFS, SMB, CIFS, FUSE (sshfs, rclone
//!   and the like), AFS and Lustre are network shares, FAT and exFAT
//!   removable drives.
//!
//! A folder the system says nothing about (the call failed) is of an
//! unknown kind, left out as another volume is unless the user includes
//! them. The scope asks on a helper thread, giving the system
//! [`VOLUME_ANSWER`] to answer ([`ask_within`]), so that a stalled network
//! mount holds up only that thread: one that does not answer in time is
//! taken for a network share. A root that is not there (an unplugged
//! drive) is asked again once it is back, and keeps its entries meanwhile.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// What kind of volume holds a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VolumeKind {
    /// A disk of this computer, or one the system does not tell apart.
    Local,
    /// A network share: an SMB or NFS share, a mapped drive, a FUSE mount.
    Network,
    /// A removable drive: a USB stick, a memory card, a disc.
    Removable,
    /// The system did not say (its call failed): left out as a network
    /// share or a removable drive is, unless other volumes are included.
    Unknown,
}

/// How long the system is given to say what kind of volume holds a folder
/// (#184): one that does not answer by then is taken for a network share,
/// so that a stalled network mount never holds up the index.
pub(crate) const VOLUME_ANSWER: Duration = Duration::from_secs(2);

/// What [`ask_within`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// The folder is not there (an unplugged drive).
    Away,
    Kind(VolumeKind),
    /// The system did not answer in time.
    NoAnswer,
}

/// Asks `volumes` what kind of volume holds the folder `path`, on a helper
/// thread of its own, waiting at most `limit`: a stalled network mount
/// holds up only that thread, left behind to end whenever the system
/// answers it. The folder is looked at first, on the same thread: one not
/// there is [`Asked::Away`].
pub(crate) fn ask_within(volumes: &VolumeKinds, path: &Path, limit: Duration) -> Asked {
    let (answer, answered) = std::sync::mpsc::channel();
    let ask = {
        let volumes = volumes.clone();
        let path = path.to_path_buf();
        move || {
            let kind = path.is_dir().then(|| volumes(path.as_path()));
            let _ = answer.send(kind);
        }
    };
    let spawned = std::thread::Builder::new()
        .name("pane-volume-kind".into())
        .spawn(ask);
    if spawned.is_err() {
        // No thread to spare: asked here, as long as it takes.
        return if path.is_dir() {
            Asked::Kind(volumes(path))
        } else {
            Asked::Away
        };
    }
    match answered.recv_timeout(limit) {
        Ok(Some(kind)) => Asked::Kind(kind),
        Ok(None) => Asked::Away,
        Err(_) => Asked::NoAnswer,
    }
}

/// Tells what kind of volume holds a folder (see
/// [`super::Scope::with_volumes`]): the system's ([`volume_kind`]) unless
/// a test says otherwise.
pub type VolumeKinds = Arc<dyn Fn(&Path) -> VolumeKind + Send + Sync>;

/// The kind of volume holding `path`, as the system says (see the module
/// docs); [`VolumeKind::Unknown`] when it does not say. Blocking, for as
/// long as the system takes: the scope asks through [`ask_within`].
pub fn volume_kind(path: &Path) -> VolumeKind {
    system_kind(path)
}

/// The kind a Windows drive type (`GetDriveTypeW`'s answer) stands for.
#[cfg(any(windows, test))]
fn drive_kind(drive_type: u32) -> VolumeKind {
    // The values of `DRIVE_REMOVABLE`, `DRIVE_REMOTE` and `DRIVE_CDROM`.
    match drive_type {
        4 => VolumeKind::Network,
        2 | 5 => VolumeKind::Removable,
        _ => VolumeKind::Local,
    }
}

/// The kind macOS's `statfs` flags (`f_flags`) stand for.
#[cfg(any(target_os = "macos", test))]
fn mount_kind(flags: u32) -> VolumeKind {
    /// `MNT_LOCAL` in `<sys/mount.h>`: a file system of this computer.
    const MNT_LOCAL: u32 = 0x0000_1000;
    /// `MNT_REMOVABLE` in `<sys/mount.h>` (not in the `libc` crate):
    /// mounted from removable media.
    const MNT_REMOVABLE: u32 = 0x0000_0200;
    if flags & MNT_LOCAL == 0 {
        VolumeKind::Network
    } else if flags & MNT_REMOVABLE != 0 {
        VolumeKind::Removable
    } else {
        VolumeKind::Local
    }
}

/// The kind Linux's `statfs` file system type (`f_type`) stands for.
#[cfg(any(target_os = "linux", test))]
fn file_system_kind(kind: i64) -> VolumeKind {
    const NETWORK: [i64; 7] = [
        0x6969,      // NFS
        0x517B,      // SMB
        0xFF53_4D42, // CIFS
        0xFE53_4D42, // SMB2
        0x6573_5546, // FUSE (sshfs, rclone and the like)
        0x5346_414F, // AFS
        0x0BD0_0BD0, // Lustre
    ];
    const REMOVABLE: [i64; 2] = [
        0x0000_4D44, // FAT
        0x2011_BAB0, // exFAT
    ];
    if NETWORK.contains(&kind) {
        VolumeKind::Network
    } else if REMOVABLE.contains(&kind) {
        VolumeKind::Removable
    } else {
        VolumeKind::Local
    }
}

#[cfg(windows)]
fn system_kind(path: &Path) -> VolumeKind {
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows::core::PCWSTR;

    if crate::files::is_network_path(path) {
        return VolumeKind::Network;
    }
    let Some(root) = volume_root(path) else {
        return VolumeKind::Unknown;
    };
    // SAFETY: a NUL-terminated root folder.
    drive_kind(unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) })
}

/// The root folder of the volume holding `path`, NUL-terminated: `C:\`, a
/// mapped drive's `Z:\`, or `C:\Mounts\Stick\` for a volume mounted in a
/// folder; a drive letter's root when the system does not resolve it (a
/// mapped drive that is not connected now).
#[cfg(windows)]
fn volume_root(path: &Path) -> Option<Vec<u16>> {
    use std::path::{Component, Prefix};

    use windows::Win32::Storage::FileSystem::GetVolumePathNameW;
    use windows::core::PCWSTR;

    let mut buffer = [0u16; 1024];
    let wide = crate::util::wide(path);
    // SAFETY: a NUL-terminated path and a writable buffer.
    if unsafe { GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut buffer) }.is_ok() {
        let len = buffer.iter().position(|&unit| unit == 0)?;
        return Some(buffer[..=len].to_vec());
    }
    match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                Some(crate::util::wide(format!("{}:\\", char::from(letter))))
            }
            _ => None,
        },
        _ => None,
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn system_kind(path: &Path) -> VolumeKind {
    use std::os::unix::ffi::OsStrExt;

    // A path the system cannot be asked about, or a call that fails, says
    // nothing: left out as another volume is (as Linux's walker did before
    // #184), unless other volumes are included.
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return VolumeKind::Unknown;
    };
    // SAFETY: a plain C structure, for which all zeroes is a valid value.
    let mut stats: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: a NUL-terminated path and a structure of the call's own type.
    if unsafe { libc::statfs(path.as_ptr(), &mut stats) } != 0 {
        return VolumeKind::Unknown;
    }
    #[cfg(target_os = "macos")]
    {
        mount_kind(stats.f_flags)
    }
    #[cfg(target_os = "linux")]
    {
        // `f_type` is an `i64` on the 64-bit targets Pane builds for, and
        // narrower elsewhere.
        #[allow(clippy::unnecessary_cast)]
        let kind = stats.f_type as i64;
        file_system_kind(kind)
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn system_kind(_path: &Path) -> VolumeKind {
    VolumeKind::Local
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use super::*;

    /// What a test says a folder is on, standing for the system's answer: a
    /// network share under `share`, a removable drive under `stick`, a
    /// local disk elsewhere.
    pub(crate) fn fake_volumes(share: &Path, stick: &Path) -> VolumeKinds {
        let share: PathBuf = share.to_path_buf();
        let stick: PathBuf = stick.to_path_buf();
        Arc::new(move |path: &Path| {
            if path.starts_with(&share) {
                VolumeKind::Network
            } else if path.starts_with(&stick) {
                VolumeKind::Removable
            } else {
                VolumeKind::Local
            }
        })
    }

    #[test]
    fn windows_drive_types_read_as_network_shares_removable_drives_or_local_disks() {
        // GetDriveTypeW's answers: a mapped drive or a share is remote.
        assert_eq!(drive_kind(4), VolumeKind::Network);
        assert_eq!(drive_kind(2), VolumeKind::Removable);
        assert_eq!(drive_kind(5), VolumeKind::Removable);
        // Fixed, a RAM disk, unknown and no root folder at all.
        for local in [3, 6, 0, 1] {
            assert_eq!(drive_kind(local), VolumeKind::Local, "{local}");
        }
    }

    #[test]
    fn macos_mount_flags_read_as_network_shares_removable_drives_or_local_disks() {
        // An SMB share: no MNT_LOCAL.
        assert_eq!(mount_kind(0x0000_0018), VolumeKind::Network);
        // A memory card: MNT_LOCAL and MNT_REMOVABLE.
        assert_eq!(mount_kind(0x0000_1200), VolumeKind::Removable);
        // The data volume: MNT_LOCAL, journaled, and the like.
        assert_eq!(mount_kind(0x0080_9080), VolumeKind::Local);
    }

    #[test]
    fn linux_file_system_types_read_as_network_shares_removable_drives_or_local_disks() {
        assert_eq!(file_system_kind(0x6969), VolumeKind::Network);
        assert_eq!(file_system_kind(0xFF53_4D42), VolumeKind::Network);
        assert_eq!(file_system_kind(0x4D44), VolumeKind::Removable);
        // ext4, Btrfs, tmpfs.
        for local in [0xEF53, 0x9123_683E, 0x0102_1994] {
            assert_eq!(file_system_kind(local), VolumeKind::Local, "{local:#x}");
        }
    }

    #[test]
    fn the_system_says_a_temporary_folder_is_on_a_local_disk() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(volume_kind(dir.path()), VolumeKind::Local);
        // Missing: statfs fails, and says nothing (Windows still names the
        // drive a missing path would be on).
        #[cfg(unix)]
        assert_eq!(
            volume_kind(&dir.path().join("missing")),
            VolumeKind::Unknown
        );
    }

    /// The scope's question goes through a helper thread: a folder away is
    /// told apart, and a system that does not answer in time does not hold
    /// up the asker (#184).
    #[test]
    fn a_volume_that_does_not_answer_in_time_holds_up_only_its_helper() {
        let dir = tempfile::tempdir().unwrap();
        let local: VolumeKinds = Arc::new(|_: &Path| VolumeKind::Local);
        let limit = Duration::from_secs(5);
        assert_eq!(
            ask_within(&local, dir.path(), limit),
            Asked::Kind(VolumeKind::Local)
        );
        assert_eq!(
            ask_within(&local, &dir.path().join("unplugged"), limit),
            Asked::Away
        );

        let stalled: VolumeKinds = Arc::new(|_: &Path| {
            std::thread::sleep(Duration::from_secs(3));
            VolumeKind::Local
        });
        let started = std::time::Instant::now();
        assert_eq!(
            ask_within(&stalled, dir.path(), Duration::from_millis(100)),
            Asked::NoAnswer
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(windows)]
    #[test]
    fn a_network_path_is_a_network_share_by_its_text() {
        assert_eq!(
            volume_kind(Path::new(r"\\server\share\Projects")),
            VolumeKind::Network
        );
        assert_eq!(
            volume_kind(Path::new(r"\\?\UNC\server\share\Projects")),
            VolumeKind::Network
        );
    }
}
