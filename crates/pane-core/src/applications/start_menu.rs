//! Windows: the shortcuts (`.lnk` files) in the Start menu's Programs
//! folders, and the packaged (AppX/MSIX) apps of the shell's Apps folder
//! (`shell:AppsFolder`) that have no such shortcut, such as Calculator on
//! Windows 11. Both are opened as Explorer opens them (the shell's
//! `ShellExecuteEx`): a shortcut by its path, a packaged app by
//! `shell:AppsFolder\<AppUserModelID>`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::{Application, Applications, env_dir, has_extension, id_path, sorted_entries};

/// How deep Pane looks into the Programs folders' subfolders.
const MAX_DEPTH: usize = 8;

/// What a packaged app's id starts with, before its AppUserModelID.
const APPS_FOLDER: &str = r"shell:AppsFolder\";

/// The shortcuts in some Start menu Programs folders, and, when enabled, the
/// packaged apps in the Apps folder.
#[derive(Clone, Debug)]
pub struct StartMenu {
    /// The Programs folders, the one whose shortcuts win first: a shortcut
    /// at the same place below a later folder is not listed again.
    folders: Vec<PathBuf>,
    /// Whether the Apps folder's packaged apps are listed too.
    packaged: bool,
}

impl StartMenu {
    /// The shortcuts in `folders` and their subfolders, the first winning.
    /// Packaged apps are not listed.
    pub fn new(folders: Vec<PathBuf>) -> StartMenu {
        StartMenu {
            folders,
            packaged: false,
        }
    }

    /// The current user's Start menu
    /// (`%APPDATA%\Microsoft\Windows\Start Menu\Programs`), then every
    /// user's (`%ProgramData%\Microsoft\Windows\Start Menu\Programs`), then
    /// the Apps folder's packaged apps without a shortcut of the same name.
    pub fn from_env() -> StartMenu {
        let programs = |root: PathBuf| root.join(r"Microsoft\Windows\Start Menu\Programs");
        let folders = env_dir("APPDATA")
            .into_iter()
            .chain(env_dir("ProgramData"))
            .map(programs)
            .collect();
        StartMenu {
            folders,
            packaged: true,
        }
    }
}

fn collect(
    root: &Path,
    dir: &Path,
    depth: usize,
    seen: &mut HashSet<String>,
    found: &mut Vec<Application>,
) {
    for entry in sorted_entries(dir) {
        let path = entry.path();
        if path.is_dir() {
            if depth < MAX_DEPTH {
                collect(root, &path, depth + 1, seen, found);
            }
            continue;
        }
        if !has_extension(&path, "lnk") {
            continue;
        }
        let Some(name) = path
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        // Uninstallers are shortcuts too, but not applications to open.
        if name.to_lowercase().starts_with("uninstall") {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        if !seen.insert(relative.to_string_lossy().to_lowercase()) {
            continue;
        }
        found.push(Application {
            id: path.to_string_lossy().into_owned(),
            name,
            location: dir.display().to_string(),
        });
    }
}

/// Adds to `found` the packaged apps among the Apps folder's `items`, each
/// (display name, parsing name): those whose parsing name is an
/// AppUserModelID (`<package family>!<app>`), not a file or a folder, and
/// whose name no shortcut already has (the Apps folder lists the
/// shortcuts' programs too).
fn add_packaged(found: &mut Vec<Application>, items: Vec<(String, String)>) {
    let mut names: HashSet<String> = found.iter().map(|app| app.name.to_lowercase()).collect();
    for (name, parsing) in items {
        let aumid = parsing.contains('!') && !parsing.contains(['\\', '/', ':', '{']);
        if !aumid || name.trim().is_empty() || !names.insert(name.to_lowercase()) {
            continue;
        }
        found.push(Application {
            id: format!("{APPS_FOLDER}{parsing}"),
            name,
            location: "Apps folder (packaged apps)".into(),
        });
    }
}

impl Applications for StartMenu {
    fn installed(&self) -> Result<Vec<Application>, String> {
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        for folder in &self.folders {
            collect(folder, folder, 0, &mut seen, &mut found);
        }
        if self.packaged {
            add_packaged(&mut found, apps_folder()?);
        }
        Ok(found)
    }

    fn open(&self, id: &str) -> Result<(), String> {
        if let Some(aumid) = id.strip_prefix(APPS_FOLDER) {
            if aumid.is_empty() || aumid.contains(['\\', '/']) {
                return Err(format!("{id} is not a packaged app"));
            }
            return shell_execute(id);
        }
        let path = id_path(id, "lnk", "a Start menu shortcut")?;
        shell_execute(&path.to_string_lossy())
    }
}

/// COM initialized on this thread for as long as it is held; the shell may
/// use COM to enumerate the Apps folder or open a shortcut.
#[cfg(windows)]
struct Com {
    /// Whether this guard initialized COM and must uninitialize it: not when
    /// the thread already had it in another mode.
    initialized: bool,
}

#[cfg(windows)]
impl Com {
    fn new() -> Result<Com, String> {
        use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
        use windows::Win32::System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
        };
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

#[cfg(windows)]
impl Drop for Com {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: paired with the successful CoInitializeEx in `new`.
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

/// The Apps folder's items, each (display name, parsing name).
#[cfg(windows)]
fn apps_folder() -> Result<Vec<(String, String)>, String> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, KF_FLAG_DEFAULT,
        SHGetKnownFolderItem, SIGDN, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
    };

    let failed = |error: windows::core::Error| format!("cannot list the Apps folder: {error}");
    let _com = Com::new()?;
    let name = |item: &IShellItem, kind: SIGDN| -> Result<String, String> {
        // SAFETY: the returned string is owned by the caller and freed here.
        unsafe {
            let text = item.GetDisplayName(kind).map_err(failed)?;
            let name = text.to_string().unwrap_or_default();
            CoTaskMemFree(Some(text.0 as *const _));
            Ok(name)
        }
    };
    let mut items = Vec::new();
    // SAFETY: plain COM calls on interfaces the shell returns, on a thread
    // with COM initialized for their lifetime.
    unsafe {
        let folder: IShellItem =
            SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None).map_err(failed)?;
        let entries: IEnumShellItems = folder
            .BindToHandler(None, &BHID_EnumItems)
            .map_err(failed)?;
        loop {
            let mut item = [None];
            let mut fetched = 0;
            entries
                .Next(&mut item, Some(&mut fetched))
                .map_err(failed)?;
            let Some(item) = item[0].take().filter(|_| fetched == 1) else {
                break;
            };
            items.push((
                name(&item, SIGDN_NORMALDISPLAY)?,
                name(&item, SIGDN_PARENTRELATIVEPARSING)?,
            ));
        }
    }
    Ok(items)
}

#[cfg(not(windows))]
fn apps_folder() -> Result<Vec<(String, String)>, String> {
    Err("the Apps folder exists only on Windows".into())
}

/// Opens `file`, a shortcut's path or `shell:AppsFolder\<AppUserModelID>`,
/// as Explorer does.
#[cfg(windows)]
fn shell_execute(file: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;

    let file: Vec<u16> = file.encode_utf16().chain([0]).collect();
    let _com = Com::new()?;
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // Wait until the shell has started it (the thread ends next), and
        // report a failure here instead of in a dialog.
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `info` is initialized with its size and a valid,
    // NUL-terminated file name that outlives the call.
    unsafe { ShellExecuteExW(&mut info) }.map_err(|error| error.message())
}

#[cfg(not(windows))]
fn shell_execute(file: &str) -> Result<(), String> {
    Err(format!(
        "{file} is a Windows shortcut or app; Pane opens those only on Windows"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut(name: &str) -> Application {
        Application {
            id: format!(r"C:\Menu\{name}.lnk"),
            name: name.into(),
            location: r"C:\Menu".into(),
        }
    }

    #[test]
    fn packaged_apps_without_a_shortcut_are_added_by_their_app_user_model_id() {
        let mut found = vec![shortcut("Notepad++"), shortcut("Paint")];
        let item = |name: &str, parsing: &str| (name.to_owned(), parsing.to_owned());
        add_packaged(
            &mut found,
            vec![
                item(
                    "Calculator",
                    "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
                ),
                // A shortcut of the same name is already listed.
                item("paint", "Microsoft.Paint_8wekyb3d8bbwe!App"),
                // A desktop program's entry: covered by its shortcut, or not an app.
                item("Notepad++", r"C:\Program Files\Notepad++\notepad++.exe"),
                item("Control Panel", "{26EE0668-A00A-44D7-9371-BEB064C98683}\\0"),
                item("Help", "https://example.com/help"),
                item("", "Nameless_1234!App"),
                // Listed twice by the shell: once.
                item(
                    "Calculator",
                    "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
                ),
            ],
        );

        let names: Vec<&str> = found.iter().map(|app| app.name.as_str()).collect();
        assert_eq!(names, ["Notepad++", "Paint", "Calculator"]);
        assert_eq!(
            found[2].id,
            r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
        );
    }
}
