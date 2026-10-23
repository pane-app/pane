//! The entry Pane's Windows setup leaves in Settings > Apps (#463), kept
//! naming the version that runs after an in-app update: the setup's key
//! under the current user's `Uninstall` key, whose `DisplayVersion` the
//! Apps list shows (and whose `MajorVersion` and `MinorVersion` Inno Setup
//! writes beside it). The key is the user's own, so no administrator rights
//! are needed, and nothing but those three values is changed: the rest of
//! the entry (where Pane is installed, how it is uninstalled) is the
//! setup's, and stays true across an update that replaces only the
//! program.

use std::path::Path;

use ::windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use ::windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD, REG_SZ, RRF_RT_REG_SZ,
    RegCloseKey, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};
use ::windows::core::HSTRING;

use super::{installed_in, major_minor};

/// The setup's Apps entry, under `HKEY_CURRENT_USER`: Inno Setup names a
/// per-user install's key `<AppId>_is1`, and the setup's AppId
/// (`scripts/setup-windows.iss`) never changes.
pub(super) const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{0737054E-4FDB-4FAD-8AD1-20C980B0C16E}_is1";

/// Gives the Apps entry at `key` (under `HKEY_CURRENT_USER`) `version`,
/// when it is the entry of the program in `folder`. `Ok(false)` when there
/// is no such entry (Pane was installed from the zip, or by `install.ps1`)
/// or it is another install's; `Err` when the entry could not be read or
/// written.
pub(super) fn set_version(key: &str, folder: &Path, version: &str) -> Result<bool, String> {
    let mut opened = HKEY::default();
    // SAFETY: `opened` is a handle the call fills in when it succeeds, and
    // nothing else uses it.
    let answer = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            None,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            &mut opened,
        )
    };
    if answer == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    checked(answer, "the Apps entry could not be opened")?;
    let result = set_opened(opened, folder, version);
    // SAFETY: `opened` is the handle the open returned, closed once here.
    // The close's own answer is advisory: nothing can be done about it.
    let _ = unsafe { RegCloseKey(opened) };
    result
}

/// [`set_version`] over the entry's open key.
fn set_opened(key: HKEY, folder: &Path, version: &str) -> Result<bool, String> {
    let Some(location) = string(key, "InstallLocation")? else {
        return Ok(false);
    };
    if !installed_in(&location, folder) {
        return Ok(false);
    }
    let mut text: Vec<u8> = version.encode_utf16().flat_map(u16::to_le_bytes).collect();
    text.extend_from_slice(&[0, 0]);
    // SAFETY: `key` is open for writing, and `text` is a plain byte buffer
    // (a null-terminated UTF-16 string) the call copies before returning.
    let answer = unsafe {
        RegSetValueExW(
            key,
            &HSTRING::from("DisplayVersion"),
            None,
            REG_SZ,
            Some(&text),
        )
    };
    checked(answer, "the Apps entry would not take the new version")?;
    if let Some((major, minor)) = major_minor(version) {
        for (name, number) in [("MajorVersion", major), ("MinorVersion", minor)] {
            // SAFETY: as above, with the number's four bytes.
            let answer = unsafe {
                RegSetValueExW(
                    key,
                    &HSTRING::from(name),
                    None,
                    REG_DWORD,
                    Some(&number.to_le_bytes()),
                )
            };
            checked(answer, "the Apps entry would not take the new version")?;
        }
    }
    Ok(true)
}

/// The string value `name` of the open `key`; `None` when it has none.
fn string(key: HKEY, name: &str) -> Result<Option<String>, String> {
    let name = HSTRING::from(name);
    let mut size = 0u32;
    // SAFETY: `key` is open for reading; with no buffer the call only
    // reports the value's size in bytes.
    let answer =
        unsafe { RegGetValueW(key, None, &name, RRF_RT_REG_SZ, None, None, Some(&mut size)) };
    if answer == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    checked(answer, "the Apps entry could not be read")?;
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: `buffer` holds `size` bytes, which the call writes at most.
    let answer = unsafe {
        RegGetValueW(
            key,
            None,
            &name,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    checked(answer, "the Apps entry could not be read")?;
    // The size counts the terminator, which RegGetValueW always writes.
    buffer.truncate((size as usize / 2).saturating_sub(1));
    Ok(Some(String::from_utf16_lossy(&buffer)))
}

/// `answer`, explained as `what` when it is not a success.
fn checked(answer: WIN32_ERROR, what: &str) -> Result<(), String> {
    if answer == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("{what}: error {}", answer.0))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ::windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, RegCloseKey,
        RegCreateKeyExW, RegDeleteTreeW,
    };
    use ::windows::core::HSTRING;

    use super::*;

    /// A scratch key of this test's own under `HKEY_CURRENT_USER`, standing
    /// in for the setup's entry, removed when dropped: the real entry is
    /// never touched.
    struct Scratch(String);

    impl Scratch {
        fn new(values: &[(&str, &str)]) -> Scratch {
            let name = format!(
                r"Software\pane-test-apps-entry-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            );
            let mut key = HKEY::default();
            // SAFETY: `key` is filled in by the call, and closed below.
            let answer = unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(&name),
                    None,
                    None,
                    REG_OPTION_NON_VOLATILE,
                    KEY_ALL_ACCESS,
                    None,
                    &mut key,
                    None,
                )
            };
            assert_eq!(answer, ERROR_SUCCESS, "the scratch key is made");
            for (value, text) in values {
                let mut data: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
                data.extend_from_slice(&[0, 0]);
                // SAFETY: `key` is open for writing; `data` is copied.
                let answer = unsafe {
                    RegSetValueExW(key, &HSTRING::from(*value), None, REG_SZ, Some(&data))
                };
                assert_eq!(answer, ERROR_SUCCESS);
            }
            // SAFETY: the handle the create returned, closed once.
            let _ = unsafe { RegCloseKey(key) };
            Scratch(name)
        }

        /// The string value `name` the scratch key holds now.
        fn value(&self, name: &str) -> Option<String> {
            let mut key = HKEY::default();
            // SAFETY: as in `set_version`.
            let answer = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(&self.0),
                    None,
                    KEY_QUERY_VALUE,
                    &mut key,
                )
            };
            assert_eq!(answer, ERROR_SUCCESS);
            let value = string(key, name).unwrap();
            // SAFETY: the handle the open returned, closed once.
            let _ = unsafe { RegCloseKey(key) };
            value
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // SAFETY: removes the scratch key this test made, and nothing
            // else.
            let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(&self.0)) };
        }
    }

    /// The key is the one the setup writes: the AppId its script names.
    #[test]
    fn the_key_is_the_setup_s_apps_entry() {
        let id = KEY
            .strip_prefix(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\")
            .and_then(|name| name.strip_suffix("_is1"))
            .expect("an Inno Setup entry's key");
        let script = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/setup-windows.iss"),
        )
        .unwrap();
        // The script writes the id's opening brace twice, as Inno Setup
        // escapes it.
        assert!(
            script.lines().any(|line| line == format!("AppId={{{id}")),
            "the setup's AppId is {id}"
        );
    }

    #[test]
    fn an_update_gives_this_install_s_entry_the_new_version() {
        let entry = Scratch::new(&[
            ("InstallLocation", r"C:\Users\u\AppData\Local\Pane\"),
            ("DisplayVersion", "0.1.0"),
        ]);
        // The program's folder as Pane names it: no trailing separator,
        // another letter case.
        let folder = Path::new(r"c:\users\u\appdata\local\pane");
        assert_eq!(set_version(&entry.0, folder, "0.2.0"), Ok(true));
        assert_eq!(entry.value("DisplayVersion").as_deref(), Some("0.2.0"));
        // Another install's program (a zip unpacked elsewhere) leaves it.
        let elsewhere = Path::new(r"C:\Users\u\Downloads\pane");
        assert_eq!(set_version(&entry.0, elsewhere, "0.3.0"), Ok(false));
        assert_eq!(entry.value("DisplayVersion").as_deref(), Some("0.2.0"));
    }

    #[test]
    fn no_entry_or_one_naming_no_folder_is_left_alone() {
        let folder = Path::new(r"C:\Users\u\AppData\Local\Pane");
        assert_eq!(
            set_version(r"Software\pane-test-no-such-entry", folder, "0.2.0"),
            Ok(false)
        );
        let entry = Scratch::new(&[("DisplayVersion", "0.1.0")]);
        assert_eq!(set_version(&entry.0, folder, "0.2.0"), Ok(false));
        assert_eq!(entry.value("DisplayVersion").as_deref(), Some("0.1.0"));
    }
}
