//! Launch at login on Windows: the `Run` key under
//! `HKEY_CURRENT_USER` — the per-user startup list the shell starts at
//! login. One value, `Pane`, holds the command that starts Pane: the
//! program's own path in double quotes, so a path with spaces stays one
//! command and not several. Setting the value replaces whatever it held,
//! so repeated changes keep exactly one registration, and deleting it
//! removes Pane from the list. No administrator rights are needed: the
//! key is the user's own.
//!
//! The value is Pane's per-user slot: a Pane that finds it holding a
//! different command (an install that moved, an older path) does not
//! treat that as another Pane's registration — it is this Pane's
//! registration, stale, and enabling again rewrites it with the path of
//! the program actually running now. Whether the command then works is
//! the shell's business at login; Pane cannot see it start.

use std::path::{Path, PathBuf};

use ::windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use ::windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SAM_FLAGS, REG_SZ, RegCloseKey,
    RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use ::windows::core::HSTRING;

use super::{Autostart, Registration};

/// The subkey of `HKEY_CURRENT_USER` that holds the per-user startup
/// list: `Software\Microsoft\Windows\CurrentVersion\Run`.
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// The value that starts Pane, in that list.
const NAME: &str = "Pane";

/// The adapter: the startup list, reached through the program's own path.
pub struct WindowsLogin {
    /// The program this Pane runs from, the one the registration starts.
    exe: PathBuf,
}

impl WindowsLogin {
    /// The adapter over the running program's own path. `Err` with the
    /// problem when the program's path cannot be found — there is then
    /// no command to register.
    pub fn new() -> Result<WindowsLogin, String> {
        let exe = std::env::current_exe()
            .map_err(|why| format!("Pane's own program could not be found: {why}"))?;
        Ok(WindowsLogin { exe })
    }
}

impl Autostart for WindowsLogin {
    fn unavailable(&self) -> Option<String> {
        // Nothing in this integration needs a permission or a particular
        // Windows setup: the key is the user's own.
        None
    }

    fn registered(&self) -> Result<Registration, String> {
        let result = open(KEY_READ, |key| {
            // The value's presence is the registration; its data is the
            // command, which the optional outputs here leave unread.
            // SAFETY: `key` is open for reading.
            unsafe { RegQueryValueExW(key, &name(), None, None, None, None) }
        });
        match result {
            Ok(problem) if problem == ERROR_SUCCESS => Ok(Registration::Enabled),
            Ok(problem) if problem == ERROR_FILE_NOT_FOUND => Ok(Registration::Disabled),
            Ok(problem) => Err(format!(
                "the startup list answered error {}: Pane's entry could not be asked for",
                problem.0
            )),
            Err(problem) => Err(format!(
                "the startup list could not be opened: error {}",
                problem.0
            )),
        }
    }

    fn enable(&self) -> Result<Registration, String> {
        let command = command(&self.exe);
        // `REG_SZ` is a null-terminated UTF-16 string, so the bytes are
        // the command's UTF-16 encoding with its terminator.
        let mut data: Vec<u8> = command.encode_utf16().flat_map(u16::to_le_bytes).collect();
        data.extend_from_slice(&[0, 0]);
        let result = open(KEY_SET_VALUE, |key| {
            // SAFETY: `key` is open for writing, and `data` is a plain
            // byte buffer the call copies before returning.
            unsafe { RegSetValueExW(key, &name(), None, REG_SZ, Some(data.as_slice())) }
        });
        checked(result, "the startup list would not take the command")?;
        Ok(Registration::Enabled)
    }

    fn disable(&self) -> Result<Registration, String> {
        let result = open(KEY_SET_VALUE, |key| {
            // SAFETY: `key` is open for writing.
            unsafe { RegDeleteValueW(key, &name()) }
        });
        match result {
            // A list without Pane's value is what removing it asks for,
            // whether it was there to remove or not.
            Ok(problem) if problem == ERROR_SUCCESS || problem == ERROR_FILE_NOT_FOUND => {
                Ok(Registration::Disabled)
            }
            Ok(problem) => Err(format!(
                "the startup list would not release Pane: error {}",
                problem.0
            )),
            Err(problem) => Err(format!(
                "the startup list could not be opened: error {}",
                problem.0
            )),
        }
    }
}

/// Runs `ask` with the startup list's key open for `access`, closing the
/// key after, and answers what `ask` reported — or the open's own error
/// when the key could not be opened.
fn open(
    access: REG_SAM_FLAGS,
    ask: impl FnOnce(HKEY) -> WIN32_ERROR,
) -> Result<WIN32_ERROR, WIN32_ERROR> {
    let mut key = HKEY::default();
    // SAFETY: `key` is a handle the call fills in when it succeeds, and
    // nothing else uses it.
    let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &subkey(), None, access, &mut key) };
    if opened != ERROR_SUCCESS {
        return Err(opened);
    }
    let answered = ask(key);
    // SAFETY: `key` is the handle the open returned, asked for once here.
    // The close's own answer is advisory: the key is already going away,
    // and there is nothing Pane can do with a failure here.
    let _ = unsafe { RegCloseKey(key) };
    Ok(answered)
}

/// The open's or the call's error `result`, as the message `what` phrases
/// it. `Ok(())` when the call succeeded.
fn checked(result: Result<WIN32_ERROR, WIN32_ERROR>, what: &str) -> Result<(), String> {
    match result {
        Ok(problem) if problem == ERROR_SUCCESS => Ok(()),
        Ok(problem) | Err(problem) => Err(format!("{what}: error {}", problem.0)),
    }
}

/// The subkey of the startup list, as a wide string.
fn subkey() -> HSTRING {
    HSTRING::from(RUN)
}

/// The value that starts Pane, as a wide string.
fn name() -> HSTRING {
    HSTRING::from(NAME)
}

/// The command that starts the program at `exe`: its path in double
/// quotes, so a path with spaces stays one command.
fn command(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{RUN, command};

    /// The command is the program's path, quoted — a path with spaces
    /// stays one command, not several.
    #[test]
    fn the_command_quotes_the_programs_path() {
        assert_eq!(
            command(Path::new(r"C:\Users\vu\AppData\Local\Pane\pane.exe")),
            r#""C:\Users\vu\AppData\Local\Pane\pane.exe""#
        );
        assert_eq!(
            command(Path::new(r"C:\Program Files\Pane\pane.exe")),
            r#""C:\Program Files\Pane\pane.exe""#
        );
    }

    /// The value sits in the per-user startup list's key.
    #[test]
    fn the_value_is_the_per_user_run_keys_slot() {
        assert_eq!(RUN, r"Software\Microsoft\Windows\CurrentVersion\Run");
    }
}
