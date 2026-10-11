//! The open windows on Windows (see the parent module): `EnumWindows`
//! walks the top-level windows in z-order, topmost first, and the
//! predicate beside this says which of them count; each one that does is
//! read — its title, its own AppUserModelID, its process's program path
//! and package family, whether it is minimized, maximized, elevated, and
//! (a window the shell cloaked) which desktop it is on, through the
//! documented virtual desktop manager — and resolved to its application
//! against the installed applications. The window in front is listed
//! first when it counts; while Pane's own window is in front (the
//! launcher open), the first window below it in the z-order that counts
//! is — the application the user was in before opening Pane — and the
//! rest keep the z-order. Bringing one to the front restores it if it is
//! minimized, then follows the paste worker's path (#253): the plain
//! foreground call, then attaching to the foreground thread's input and
//! trying again.

use std::collections::HashMap;
use std::sync::OnceLock;

use ::windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM};
use ::windows::Win32::Graphics::Dwm::{
    DWM_CLOAKED_APP, DWM_CLOAKED_SHELL, DWMWA_CLOAKED, DwmGetWindowAttribute,
};
use ::windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use ::windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use ::windows::Win32::System::Threading::GetCurrentProcessId;
use ::windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
use ::windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetForegroundWindow, GetPropW, GetWindowLongPtrW,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, IsZoomed, SW_RESTORE,
    ShowWindow, WINDOW_EX_STYLE, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
};
use ::windows::core::{BOOL, HSTRING, w};

use super::predicate::{Attributes, Scope, counts, shows_all_desktops};
use super::{Facts, SwitchWindows, Window, WindowsError, window as record_of};
use crate::applications::identity::Catalog;
use crate::applications::{Discovery, StartMenu};
use crate::system::front::windows::{class_name, family, owned, program_path, tool};
use crate::system::{app_user_model_id, bring_to_front, elevated, window_title};
use crate::windows_shell::Com;

/// Where the user's Alt+Tab setting is: `VirtualDesktopAltTabFilter`,
/// 0 showing every desktop's windows.
const EXPLORER: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const ALT_TAB_DESKTOPS: &str = "VirtualDesktopAltTabFilter";

/// The open windows on Windows.
pub struct WindowsSwitchWindows {
    /// The installed applications, scanned once, best-effort, for the
    /// windows' applications' names and icons. What is installed after
    /// the first scan is named by its window's title until Pane restarts
    /// (#124 refines this).
    installed: OnceLock<Catalog>,
}

impl WindowsSwitchWindows {
    /// The open windows, with the installed applications scanned once,
    /// best-effort, when they are first asked for.
    pub fn new() -> WindowsSwitchWindows {
        WindowsSwitchWindows {
            installed: OnceLock::new(),
        }
    }

    /// The installed applications, scanned once, best-effort: the Start
    /// menu, the Desktops, the taskbar's pins and the Apps folder, as
    /// the applications module finds them — the same scan the front
    /// application's resolution makes (#253).
    fn installed(&self) -> &Catalog {
        self.installed.get_or_init(|| {
            let sources = StartMenu::from_env().sources().unwrap_or_default();
            Catalog::new(sources)
        })
    }
}

impl Default for WindowsSwitchWindows {
    fn default() -> WindowsSwitchWindows {
        WindowsSwitchWindows::new()
    }
}

impl SwitchWindows for WindowsSwitchWindows {
    fn list(&self) -> Result<Vec<Window>, WindowsError> {
        let scope = Scope {
            all_desktops: all_desktops(),
        };
        let facts = listed(&scope)?;
        Ok(facts
            .into_iter()
            .map(|facts| record_of(&facts, self.installed()))
            .collect())
    }

    fn activate(&self, id: &str) -> Result<(), WindowsError> {
        // The ids the listing gives are the windows themselves, spelled
        // as numbers; anything else names no window of this session.
        let number = id.parse::<usize>().map_err(|_| unknown(id))?;
        let window = HWND(number as *mut _);
        // SAFETY: a handle the listing gave; a gone window is checked.
        if !unsafe { IsWindow(Some(window)) }.as_bool() {
            return Err(WindowsError::Failed(
                "that window closed since it was listed".into(),
            ));
        }
        // Restore it first if it is minimized, as Alt+Tab does.
        // SAFETY: as above.
        if unsafe { IsIconic(window) }.as_bool() {
            // SAFETY: as above.
            let _ = unsafe { ShowWindow(window, SW_RESTORE) };
        }
        // A window on another virtual desktop is activated the same way:
        // Windows brings its desktop forward as it does from Alt+Tab,
        // with no virtual-desktop call of Pane's own (those are
        // undocumented).
        bring_to_front(window)
            .map_err(|_| WindowsError::Failed("that window did not come to the front".into()))
    }
}

/// An id no window of this session has, as `activate` answers it.
fn unknown(id: &str) -> WindowsError {
    WindowsError::Failed(format!("“{id}” is no window of this session"))
}

/// The windows that count, in z-order with the front application's
/// window first (see the module doc). The calling thread must hold COM
/// for the AppUserModelIDs and the desktop manager.
fn listed(scope: &Scope) -> Result<Vec<Facts>, WindowsError> {
    // The shell needs COM for the property stores and the desktop
    // manager; without it there is nothing to list, said as a failure.
    let _com = Com::new().map_err(|why| {
        WindowsError::Failed(format!("the open windows could not be listed: {why}"))
    })?;
    let desktops = desktop_manager();
    // Every top-level window, topmost first, with the place of each in
    // the z-order, so the window in front can be put first.
    let order = z_order();
    let places: HashMap<usize, usize> = order
        .iter()
        .enumerate()
        .map(|(place, window)| (window.0 as usize, place))
        .collect();
    let mut facts = Vec::new();
    for window in order {
        let mut process = 0u32;
        // SAFETY: `window` is a window handle; `process` is writable.
        unsafe { GetWindowThreadProcessId(window, Some(&raw mut process)) };
        if process == 0 {
            continue;
        }
        let mut attributes = attributes(window, process);
        // Only a window the shell cloaked is asked which desktop it is
        // on: any other is on the one shown.
        if attributes.shell_cloaked {
            attributes.elsewhere = elsewhere(window, &desktops);
        }
        if counts(&attributes, scope).is_some() {
            facts.push(record(window, process, attributes.elsewhere));
        }
    }
    // The window in front is listed first when it counts; while Pane's
    // own is in front (the launcher open), the first window below it in
    // the z-order that counts is: the application the user was in before
    // opening Pane, whatever floats above the two.
    if let Some(front) = front_of(&facts, &places)
        && let Some(at) = facts.iter().position(|facts| facts.window == front)
    {
        let first = facts.remove(at);
        facts.insert(0, first);
    }
    Ok(facts)
}

/// The top-level windows in z-order, topmost first, as `EnumWindows`
/// walks them.
fn z_order() -> Vec<HWND> {
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: `windows` outlives the call, and the callback only records
    // each window on this thread.
    let _ = unsafe { EnumWindows(Some(collected), LPARAM(&raw mut windows as isize)) };
    windows
}

/// `EnumWindows`' callback: records each window, walking on. A panic must
/// not unwind into Windows, which would end Pane.
unsafe extern "system" fn collected(window: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the `Vec` the caller gave, alive for the walk,
    // and only this callback touches it.
    let recorded = std::panic::catch_unwind(|| unsafe {
        (*(lparam.0 as *mut Vec<HWND>)).push(window);
    });
    BOOL::from(recorded.is_ok())
}

/// What the rule reads of `window`, whose process is `process`. The
/// calling thread must hold COM for the AppUserModelID its record reads.
fn attributes(window: HWND, process: u32) -> Attributes {
    let (cloaked, shell_cloaked) = cloak(window);
    Attributes {
        // SAFETY: a window handle; each call asks for one fact of it.
        visible: unsafe { IsWindowVisible(window) }.as_bool(),
        owned: owned(window),
        no_activate: has_style(window, WS_EX_NOACTIVATE),
        tool: tool(window),
        forced: has_style(window, WS_EX_APPWINDOW),
        removed: removed(window),
        cloaked,
        shell_cloaked,
        // Filled in for a shell-cloaked window, which alone is asked.
        elsewhere: false,
        class: class_name(window),
        program: program_path(process),
        // SAFETY: no arguments.
        own: process == unsafe { GetCurrentProcessId() },
        title: window_title(window),
    }
}

/// Whether `window` has the extended style `style`.
fn has_style(window: HWND, style: WINDOW_EX_STYLE) -> bool {
    // SAFETY: a window handle; the style is a plain value it answers.
    let styles = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) };
    styles & style.0 as isize != 0
}

/// Whether the application cloaked `window`, and whether the shell did:
/// `DWMWA_CLOAKED`'s flags. A window DWM does not know is not cloaked.
fn cloak(window: HWND) -> (bool, bool) {
    let mut flags = 0u32;
    // SAFETY: `window` is a window handle; `flags` is writable for its
    // size.
    let read = unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            &raw mut flags as *mut core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    if read.is_err() {
        return (false, false);
    }
    (flags & DWM_CLOAKED_APP != 0, flags & DWM_CLOAKED_SHELL != 0)
}

/// Whether the application removed `window` from the taskbar
/// (`ITaskbarList::DeleteTab`): the shell's mark of it, read as a window
/// property.
fn removed(window: HWND) -> bool {
    // SAFETY: a plain property name; a window without it answers no
    // handle.
    unsafe { !GetPropW(window, w!("ITaskList_Deleted")).is_invalid() }
}

/// The documented virtual desktop manager, on this thread, if the shell
/// gives one: it says which desktop a window is on. The calling thread
/// must hold COM.
fn desktop_manager() -> Option<IVirtualDesktopManager> {
    // SAFETY: the shell's own coclass, asked on this thread; the
    // interface is released with it.
    unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL) }.ok()
}

/// Whether `window` is on another virtual desktop than the one shown, as
/// `desktops` says; without a manager, or for a window it cannot place,
/// it is not said to be.
fn elsewhere(window: HWND, desktops: &Option<IVirtualDesktopManager>) -> bool {
    // SAFETY: the manager's own method, on this thread.
    desktops
        .as_ref()
        .and_then(|manager| unsafe { manager.IsWindowOnCurrentVirtualDesktop(window) }.ok())
        .is_some_and(|current| !current.as_bool())
}

/// The window to list first: the window in front when it counts, else
/// the first one below it in the z-order that does (the application the
/// user was in before opening Pane; Pane's own windows, never listed,
/// are passed over).
fn front_of(facts: &[Facts], places: &HashMap<usize, usize>) -> Option<usize> {
    // SAFETY: no arguments.
    let front = unsafe { GetForegroundWindow() };
    if front.is_invalid() {
        return None;
    }
    let front = front.0 as usize;
    if facts.iter().any(|facts| facts.window == front) {
        return Some(front);
    }
    // The window in front is not listed (Pane's own, or one the rule
    // drops): the first window below it in the z-order that counts is
    // the front application's.
    let place = places.get(&front).copied()?;
    facts
        .iter()
        .filter(|facts| {
            places
                .get(&facts.window)
                .is_some_and(|below| *below > place)
        })
        .min_by_key(|facts| places.get(&facts.window))
        .map(|facts| facts.window)
}

/// The window `window`, which counts, as the facts its record is built
/// of. The calling thread must hold COM for the AppUserModelID.
fn record(window: HWND, process: u32, elsewhere: bool) -> Facts {
    Facts {
        window: window.0 as usize,
        title: window_title(window),
        aumid: app_user_model_id(window),
        program: program_path(process),
        family: family(process),
        // SAFETY: a window handle; each call asks for one fact of it.
        minimized: unsafe { IsIconic(window) }.as_bool(),
        // SAFETY: as above.
        maximized: unsafe { IsZoomed(window) }.as_bool(),
        elsewhere,
        elevated: elevated(process),
    }
}

/// Whether the user's Alt+Tab shows the windows of every desktop, as
/// the registry says ([`EXPLORER`]'s `VirtualDesktopAltTabFilter`), read
/// the way the tray reads the taskbar's settings.
fn all_desktops() -> bool {
    let (key, value) = (HSTRING::from(EXPLORER), HSTRING::from(ALT_TAB_DESKTOPS));
    let mut data = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: a NUL-terminated key and value name; `data` is writable for
    // `size` bytes.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            &value,
            RRF_RT_REG_DWORD,
            None,
            Some(&raw mut data as *mut core::ffi::c_void),
            Some(&raw mut size),
        )
    };
    shows_all_desktops((read == ERROR_SUCCESS).then_some(data))
}
