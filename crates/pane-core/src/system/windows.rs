//! The system functions on Windows (see the parent module): the clipboard
//! through the clipboard adapter's own window, opening with
//! `ShellExecuteExW`, File Explorer's selection with
//! `SHOpenFolderAndSelectItems`, and the Recycle Bin with
//! `SHFileOperationW`. COM is initialized on the calling thread (one of
//! Pane's own, for this call) while the shell is used.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use ::windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use ::windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use ::windows::Win32::UI::Shell::{
    FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING,
    ILCreateFromPathW, ILFree, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
    SHFILEOPSTRUCTW, SHFileOperationW, SHOpenFolderAndSelectItems, ShellExecuteExW,
};
use ::windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use ::windows::core::{HRESULT, PCWSTR};

use super::{Clip, MAX_CLIPBOARD_TEXT, NotTrashed, System, missing};
use crate::clipboard::windows::{put_clip, read_clip};

/// The system functions on Windows.
pub(super) struct WindowsSystem;

impl System for WindowsSystem {
    fn copy(&self, clip: &Clip, concealed: bool) -> Result<(), String> {
        put_clip(clip, concealed)
    }

    fn read_clipboard(&self) -> Result<Option<Clip>, String> {
        read_clip(MAX_CLIPBOARD_TEXT)
    }

    fn open(&self, target: &str, application: Option<&str>) -> Result<(), String> {
        match application {
            None => shell_execute(target, None),
            // The application (its program, its Start menu shortcut or its
            // `shell:AppsFolder` id) opens with the target as its one
            // argument, as "Open with" does.
            Some(application) => shell_execute(application, Some(&argument(target))),
        }
    }

    fn reveal(&self, path: &Path) -> Result<(), String> {
        if let Some(missing) = missing(path) {
            return Err(missing);
        }
        let _com = Com::new()?;
        let wide = wide(path.as_os_str().encode_wide());
        // SAFETY: a NUL-terminated path that outlives the call; the list it
        // answers is freed below.
        let item = unsafe { ILCreateFromPathW(PCWSTR(wide.as_ptr())) };
        if item.is_null() {
            return Err(format!("File Explorer cannot find {}", path.display()));
        }
        // SAFETY: `item` is the list made above; with no children given,
        // File Explorer opens its folder and selects it.
        let shown = unsafe { SHOpenFolderAndSelectItems(item, None, 0) };
        // SAFETY: made by ILCreateFromPathW above, freed once.
        unsafe { ILFree(Some(item.cast_const())) };
        shown.map_err(|error| format!("File Explorer did not show it: {}", error.message()))
    }

    fn trash(&self, paths: &[PathBuf]) -> Vec<NotTrashed> {
        paths
            .iter()
            .filter_map(|path| {
                recycle(path).err().map(|reason| NotTrashed {
                    path: path.clone(),
                    reason,
                })
            })
            .collect()
    }
}

/// `units` followed by a NUL.
fn wide(units: impl Iterator<Item = u16>) -> Vec<u16> {
    units.chain([0]).collect()
}

/// `text` as one argument on a Windows command line, quoted as
/// `CommandLineToArgvW` reads it back: a quote is escaped, and the
/// backslashes before a quote (or the closing one) are doubled.
fn argument(text: &str) -> String {
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for character in text.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        let escapes = if character == '"' {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        quoted.push_str(&"\\".repeat(escapes));
        backslashes = 0;
        quoted.push(character);
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

/// Opens `file` (a URL, a path or an application's id) as Explorer does,
/// with `parameters` as its command line when it is a program, waiting
/// only until the shell has started it.
fn shell_execute(file: &str, parameters: Option<&str>) -> Result<(), String> {
    let file = wide(file.encode_utf16());
    let parameters = parameters.map(|parameters| wide(parameters.encode_utf16()));
    let _com = Com::new()?;
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // Wait until the shell has started it (this thread ends next), and
        // report a failure here instead of in a dialog.
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: parameters
            .as_ref()
            .map_or(PCWSTR::null(), |parameters| PCWSTR(parameters.as_ptr())),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `info` is initialized with its size, and its strings are
    // NUL-terminated and outlive the call.
    unsafe { ShellExecuteExW(&mut info) }
        .map_err(|error| format!("Windows did not open it: {}", error.message()))
}

/// Moves `path` to the Recycle Bin, or says why it did not. A path that
/// cannot be recycled (on a drive without a Recycle Bin, or too large for
/// it) is not deleted for good without the user's say: Windows asks first.
fn recycle(path: &Path) -> Result<(), String> {
    if let Some(missing) = missing(path) {
        return Err(missing);
    }
    // The list of paths ends with an empty one.
    let from: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let flags =
        FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT | FOF_WANTNUKEWARNING;
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        // The flags fit the field's 16 bits.
        fFlags: flags.0 as u16,
        ..Default::default()
    };
    // SAFETY: `operation` names a double-NUL-terminated list that outlives
    // the call, and no other pointer.
    let result = unsafe { SHFileOperationW(&mut operation) };
    if operation.fAnyOperationsAborted.as_bool() {
        return Err("Moving it was cancelled".into());
    }
    if result != 0 {
        let message = HRESULT::from_win32(result as u32).message();
        return Err(format!(
            "Windows did not move it to the Recycle Bin: {message} (code {result:#x})"
        ));
    }
    Ok(())
}

/// COM initialized on this thread for as long as it is held, as the shell
/// needs.
struct Com {
    /// Whether this guard initialized COM and must uninitialize it: not when
    /// the thread already had it in another mode.
    initialized: bool,
}

impl Com {
    fn new() -> Result<Com, String> {
        // SAFETY: no reserved pointer; paired with CoUninitialize in `drop`.
        let result =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if result == RPC_E_CHANGED_MODE {
            // Already initialized as multithreaded: usable as it is.
            return Ok(Com { initialized: false });
        }
        result
            .ok()
            .map_err(|error| format!("cannot start COM: {error}"))?;
        Ok(Com { initialized: true })
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: paired with the successful CoInitializeEx in `new`.
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_is_one_quoted_argument() {
        assert_eq!(argument("https://example.com"), r#""https://example.com""#);
        assert_eq!(argument(r"C:\My Notes\a.txt"), r#""C:\My Notes\a.txt""#);
        // Backslashes before the closing quote are doubled.
        assert_eq!(argument(r"C:\My Notes\"), r#""C:\My Notes\\""#);
        // A quote is escaped, with the backslashes before it doubled.
        assert_eq!(argument(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(argument(r#"a\"b"#), r#""a\\\"b""#);
    }
}
