//! Which file a program's name means: an absolute path as given, or a bare
//! name found on the user's search path as it is at the time of the call.
//! On Windows that search path is read from the registry each time (the
//! system's `Path`, then the user's), so a tool installed after Pane started
//! is found, as a newly opened terminal would find it; elsewhere it is
//! Pane's own `PATH` (on macOS with Homebrew's folders added, which a
//! program started from the Dock does not have).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::runner::{ErrorKind, ProgramError};

/// The extensions Windows tries for a bare name without one when `PATHEXT`
/// does not say.
#[cfg(windows)]
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// The search path now: the registry's on Windows, Pane's `PATH` elsewhere.
pub(crate) fn system_search_path() -> OsString {
    #[cfg(windows)]
    {
        let parts: Vec<String> = [registry::machine_path(), registry::user_path()]
            .into_iter()
            .flatten()
            .filter(|part| !part.trim().is_empty())
            .collect();
        if parts.is_empty() {
            // A registry Pane cannot read: the search path it started with.
            return std::env::var_os("PATH").unwrap_or_default();
        }
        OsString::from(parts.join(";"))
    }
    #[cfg(not(windows))]
    {
        let path = std::env::var_os("PATH")
            .unwrap_or_else(|| OsString::from("/usr/local/bin:/usr/bin:/bin"));
        with_homebrew(path)
    }
}

/// `path` with Homebrew's folders after it, where it lacks them.
#[cfg(target_os = "macos")]
fn with_homebrew(path: OsString) -> OsString {
    let mut folders: Vec<PathBuf> = std::env::split_paths(&path).collect();
    for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
        if !folders.iter().any(|folder| folder == Path::new(extra)) {
            folders.push(PathBuf::from(extra));
        }
    }
    std::env::join_paths(folders).unwrap_or(path)
}

/// `path` as it is: only macOS starts applications with a short one.
#[cfg(all(unix, not(target_os = "macos")))]
fn with_homebrew(path: OsString) -> OsString {
    path
}

/// Whether `program` is a bare name: no folder in it.
fn is_bare(program: &str) -> bool {
    let separators: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
    !program.contains(separators) && !(cfg!(windows) && program.contains(':'))
}

/// The file `program` names: an absolute path as given, which must be a
/// file, or a bare name found in a folder of `search_path` (on Windows with
/// each of `PATHEXT`'s extensions when it has none).
pub(crate) fn resolve(program: &str, search_path: &OsStr) -> Result<PathBuf, ProgramError> {
    if program.trim().is_empty() {
        return Err(ProgramError::new(
            ErrorKind::Refused,
            "no program was named: name one by an absolute path or by a bare name such as `git`",
        ));
    }
    let path = Path::new(program);
    if path.is_absolute() {
        return if path.is_file() {
            Ok(path.to_path_buf())
        } else {
            Err(ProgramError::new(
                ErrorKind::NotFound,
                format!("there is no program at {program}"),
            ))
        };
    }
    if !is_bare(program) {
        return Err(ProgramError::new(
            ErrorKind::Refused,
            format!(
                "`{}` is a relative path: name a program by an absolute path, or by a bare \
                 name Pane finds on the search path",
                program.escape_debug()
            ),
        ));
    }
    std::env::split_paths(search_path)
        .filter(|folder| folder.is_absolute())
        .flat_map(|folder| candidates(&folder, program))
        .find(|candidate| runnable(candidate))
        .ok_or_else(|| {
            ProgramError::new(
                ErrorKind::NotFound,
                format!(
                    "there is no program `{}` on the search path",
                    program.escape_debug()
                ),
            )
        })
}

/// The files `name` may be in `folder`.
fn candidates(folder: &Path, name: &str) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        if Path::new(name).extension().is_some() {
            return vec![folder.join(name)];
        }
        let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| DEFAULT_PATHEXT.to_owned());
        extensions
            .split(';')
            .map(str::trim)
            .filter(|extension| extension.starts_with('.') && extension.len() > 1)
            .map(|extension| folder.join(format!("{name}{}", extension.to_ascii_lowercase())))
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec![folder.join(name)]
    }
}

/// Whether `path` is a file the system can run: on Unix one with an
/// execute permission.
fn runnable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The folder a program runs in when the command names none: the user's
/// home folder.
pub(crate) fn home_folder() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|home| home.is_absolute() && home.is_dir())
}

#[cfg(windows)]
mod registry {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
        RegGetValueW,
    };
    use windows::core::HSTRING;

    /// The system's `Path`, its variables expanded.
    pub(super) fn machine_path() -> Option<String> {
        read(
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        )
    }

    /// The user's own `Path`, its variables expanded.
    pub(super) fn user_path() -> Option<String> {
        read(HKEY_CURRENT_USER, "Environment")
    }

    /// The `Path` value of `subkey` under `root`, expanded (`RegGetValueW`
    /// expands a `REG_EXPAND_SZ` value itself).
    fn read(root: HKEY, subkey: &str) -> Option<String> {
        let (subkey, value) = (HSTRING::from(subkey), HSTRING::from("Path"));
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
        let mut bytes: u32 = 0;
        // SAFETY: asks for the size only; every pointer is valid or absent.
        let sized = unsafe {
            RegGetValueW(
                root,
                &subkey,
                &value,
                flags,
                None,
                None,
                Some(&raw mut bytes),
            )
        };
        if sized != ERROR_SUCCESS || bytes == 0 {
            return None;
        }
        // Expanding may need more than the stored size; a few tries cover
        // a value that changes meanwhile.
        for _ in 0..4 {
            let mut buffer = vec![0u16; (bytes as usize).div_ceil(2) + 1];
            let mut size = (buffer.len() * 2) as u32;
            // SAFETY: `buffer` holds `size` bytes and outlives the call.
            let read = unsafe {
                RegGetValueW(
                    root,
                    &subkey,
                    &value,
                    flags,
                    None,
                    Some(buffer.as_mut_ptr().cast::<core::ffi::c_void>()),
                    Some(&raw mut size),
                )
            };
            if read == ERROR_SUCCESS {
                let length = buffer
                    .iter()
                    .position(|&unit| unit == 0)
                    .unwrap_or(buffer.len());
                return Some(String::from_utf16_lossy(&buffer[..length]));
            }
            if size <= bytes {
                return None;
            }
            bytes = size;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_path_or_nothing_is_refused_and_an_absent_path_is_not_found() {
        let empty = OsString::new();
        assert_eq!(resolve("", &empty).unwrap_err().kind, ErrorKind::Refused);
        assert_eq!(
            resolve("bin/tool", &empty).unwrap_err().kind,
            ErrorKind::Refused
        );
        let dir = tempfile::tempdir().unwrap();
        let absent = dir.path().join("absent");
        let error = resolve(absent.to_str().unwrap(), &empty).unwrap_err();
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert!(
            error.message.contains("there is no program at"),
            "{error:?}"
        );
    }

    #[test]
    fn a_bare_name_is_found_in_the_first_folder_of_the_search_path_that_has_it() {
        let (first, second) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let name = if cfg!(windows) { "tool.exe" } else { "tool" };
        for folder in [first.path(), second.path()] {
            std::fs::write(folder.join(name), b"").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(folder.join(name), std::fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
        }
        let search = std::env::join_paths([second.path(), first.path()]).unwrap();

        let found = resolve("tool", &search).unwrap();

        assert_eq!(found.parent(), Some(second.path()));
        let missing = resolve("tool", &OsString::new()).unwrap_err();
        assert_eq!(missing.kind, ErrorKind::NotFound);
        assert_eq!(
            missing.message,
            "there is no program `tool` on the search path"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_without_an_execute_permission_is_not_a_program() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("notes"), b"").unwrap();

        let search = OsString::from(folder.path());

        assert_eq!(
            resolve("notes", &search).unwrap_err().kind,
            ErrorKind::NotFound
        );
    }

    /// The registry's search path always holds Windows' own folder, where
    /// `cmd.exe` is.
    #[cfg(windows)]
    #[test]
    fn the_registry_search_path_finds_the_command_interpreter() {
        let search = system_search_path();

        let cmd = resolve("cmd", &search).unwrap();

        assert!(
            cmd.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("cmd.exe")),
            "{}",
            cmd.display()
        );
    }
}
