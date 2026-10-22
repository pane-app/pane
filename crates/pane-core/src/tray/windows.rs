//! The tray entry on Windows: an icon in the notification area
//! (`Shell_NotifyIcon`), owned by a hidden window of a thread of Pane's
//! own whose message loop receives the shell's callback messages. A
//! right click on the icon (or the menu key while it has the keyboard)
//! opens the menu — Open Pane, Settings, Exit, the platform's own labels
//! — and a left click (or Enter) summons the launcher, as the tray's
//! primary action convention is. No permission is needed.
//!
//! Showing and hiding are done by that thread, as registering a hotkey
//! is: the caller queues the request, wakes the thread with a thread
//! message and waits for its answer, because the icon and its menu
//! belong to the thread that made them. Dropping the adapter ends the
//! thread, which deletes the icon and destroys its menu as it ends — so
//! the preference's every change, and the quit path, leave no icon
//! behind.
//!
//! The icon keeps itself in the notification area (#131):
//!
//! - **After Explorer restarts.** The notification area forgets every
//!   icon when Explorer restarts, and Windows then broadcasts
//!   `TaskbarCreated` to every top-level window. The icon's window
//!   receives it and adds the icon again — its identity, icon, tooltip
//!   and callback — and sets its notification version again, but only
//!   while the visibility Pane last asked for is shown. A hidden icon
//!   stays hidden. A re-add the system refuses is reported as a
//!   diagnostic and tried again at the next broadcast; the preference is
//!   the application's and does not change.
//! - **A stable identity.** The icon is added with a GUID, a name-based
//!   UUID of the program's canonical path in Pane's own namespace
//!   ([`icon_guid`]): the same across restarts and application updates,
//!   which replace the program at the same path, so Windows keeps the
//!   user's "always show" choice for it. Windows ties an unsigned
//!   program's icon GUID to its path, so a development build elsewhere
//!   gets a GUID of its own. If the system refuses the GUID, the icon
//!   falls back to the window's numeric id, saying why.
//! - **An icon that follows the taskbar.** The taskbar's theme is the
//!   personalization setting `SystemUsesLightTheme` — not the
//!   applications' theme, which can differ, and not Pane's own
//!   Appearance choice ([`taskbar_variant`]). The icon is the black
//!   variant of Pane's mark on a light taskbar and the white one on a
//!   dark taskbar, swapped in place when the system broadcasts a theme
//!   change (`ImmersiveColorSet`). Under high contrast the variant
//!   follows the high-contrast theme's own window colour instead
//!   (proposed). Each variant is an `.ico` of every small-icon size
//!   from 100% to 400% scaling, embedded in the program
//!   (`assets/tray`, drawn by `scripts/icons/tray-icons.py`), and the
//!   icon is made at the notification area's small-icon size for the
//!   display's DPI.
//!
//! What decides each of these is [`NotifyIcon`], behind a narrow seam,
//! [`Shell`]: the four `Shell_NotifyIcon` calls, the taskbar's theme and
//! where diagnostics go. The adapter's shell is [`NotificationArea`]; the
//! unit tests use one of their own, so they check the behaviour without
//! the real notification area.

use std::collections::VecDeque;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use ::windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM, LRESULT, POINT, WPARAM};
use ::windows::Win32::Graphics::Gdi::{COLOR_WINDOW, GetSysColor};
use ::windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use ::windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use ::windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use ::windows::Win32::UI::Shell::{
    NIF_GUID, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIM_SETVERSION, NIN_SELECT, NINF_KEY, NOTIFY_ICON_MESSAGE, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, NOTIFYICONDATAW_0, Shell_NotifyIconW,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, ChangeWindowMessageFilterEx, CreateIconFromResourceEx, CreatePopupMenu,
    DefWindowProcW, DestroyIcon, DestroyMenu, GetCursorPos, HICON, HMENU, IDI_APPLICATION,
    LR_DEFAULTCOLOR, LoadIconW, MENU_ITEM_FLAGS, MF_STRING, MSG, MSGFLT_ALLOW, PostMessageW,
    RegisterWindowMessageW, SM_CXSMICON, SPI_GETHIGHCONTRAST, SPI_SETHIGHCONTRAST,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetForegroundWindow, SystemParametersInfoW,
    TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_CONTEXTMENU,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WM_SETTINGCHANGE,
};
use ::windows::core::{GUID, HSTRING, PCWSTR, w};

use sha1_checked::Sha1;
use sha1_checked::digest::Update;

use super::{SelectionSender, Tray, TrayAction, TrayError};
use crate::threads::windows::{MessageThread, WM_WAKE, Window, WindowClass, stop_sent};

/// The icon's numeric id, in `Shell_NotifyIcon`'s terms: its identity
/// with its window when it has no GUID, or the system refused it.
const ICON_ID: u32 = 1;
/// The message the shell sends to the owning window when the icon is
/// used: in the `WM_APP` range, clear of the threads module's wake and
/// stop.
const TRAY_MESSAGE: u32 = 0x8000 + 3;
/// The icon's tooltip.
const TIP: &str = "Pane";
/// The icon's tooltip while game mode has paused Pane's hotkeys for a
/// game in front (#125): the entry says so, so a press that does
/// nothing while the game has the keys is understood.
const TIP_PAUSED: &str = "Pane — hotkeys paused for a game";
/// The menu's commands, as `TrackPopupMenuEx` reports them.
const OPEN_PANE: usize = 1;
const SETTINGS: usize = 2;
const EXIT: usize = 3;
/// The keyboard's selection of the icon (Enter or Space), with the
/// notification version 4 Pane asks for.
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

/// Pane's namespace for the name-based UUID of its tray icon: a random
/// UUID (`da8fe9f2-34cb-4924-bc6c-180e89a5ae57`), fixed for good —
/// changing it changes every installation's icon identity, and Windows
/// would forget the user's notification-area choice for it.
const NAMESPACE: [u8; 16] = [
    0xda, 0x8f, 0xe9, 0xf2, 0x34, 0xcb, 0x49, 0x24, 0xbc, 0x6c, 0x18, 0x0e, 0x89, 0xa5, 0xae, 0x57,
];

/// Where the user's personalization settings are, and the one that holds
/// the taskbar's (and Start's) theme: 1 for light, 0 for dark.
const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const TASKBAR_THEME: &str = "SystemUsesLightTheme";
/// The setting a theme change names in its `WM_SETTINGCHANGE` broadcast.
const THEME_SETTING: &str = "ImmersiveColorSet";

/// Pane's mark in black, for a light taskbar, and in white, for a dark
/// one: every small-icon size from 16 to 64 pixels.
static MARK_DARK: &[u8] = include_bytes!("../../assets/tray/mark-dark.ico");
static MARK_LIGHT: &[u8] = include_bytes!("../../assets/tray/mark-light.ico");

/// The window class of the tray icon's owner: a hidden top-level window,
/// made by [`WindowClass::hidden_window`], because the shell addresses
/// the owner, its menus need a real window to belong to, and the
/// broadcasts it listens for (`TaskbarCreated`, `WM_SETTINGCHANGE`) reach
/// top-level windows only.
static TRAY_CLASS: WindowClass = WindowClass::new("PaneTray", procedure);

/// The `TaskbarCreated` message, once registered (0 before).
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// How the notification area knows the icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Identity {
    /// By its GUID, for which Windows keeps the user's choice to show it.
    Guid(GUID),
    /// By its window and this number.
    Number(u32),
}

/// Which variant of Pane's mark the icon shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variant {
    /// The black mark, for a light taskbar.
    Dark,
    /// The white mark, for a dark taskbar.
    Light,
}

/// The icon as Pane asks the notification area to show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Notification {
    identity: Identity,
    variant: Variant,
    tip: &'static str,
    /// The message the shell sends the owning window when the icon is
    /// used.
    callback: u32,
}

/// The seam between the icon's behaviour and the system: the
/// notification area's four calls, the taskbar's theme, and where a
/// diagnostic goes. Each call answers why the system refused it.
trait Shell {
    /// Adds the icon (`NIM_ADD`).
    fn add(&mut self, icon: &Notification) -> Result<(), String>;
    /// Asks for notification version 4 for the icon (`NIM_SETVERSION`).
    fn set_version(&mut self, identity: Identity) -> Result<(), String>;
    /// Changes the icon in place (`NIM_MODIFY`).
    fn modify(&mut self, icon: &Notification) -> Result<(), String>;
    /// Removes the icon (`NIM_DELETE`).
    fn delete(&mut self, identity: Identity) -> Result<(), String>;
    /// The variant the taskbar's theme asks for now.
    fn taskbar(&self) -> Variant;
    /// Reports a diagnostic.
    fn report(&mut self, diagnostic: &str);
}

/// The icon's behaviour: what it is added as, and when.
struct NotifyIcon<S: Shell> {
    shell: S,
    /// What the icon is added as: its GUID until the system refuses it.
    identity: Identity,
    /// The variant shown, or to show, for the taskbar's theme.
    variant: Variant,
    /// Whether the tooltip says Pane's hotkeys are paused for a game
    /// (game mode, #125), kept even while the icon is not shown, so the
    /// next add — after a hide, or an Explorer restart — shows it.
    paused: bool,
    /// The visibility Pane last asked for — the preference as the
    /// application applies it, kept even when the system refused it, so
    /// the next `TaskbarCreated` tries again.
    wanted: bool,
    /// Whether the notification area holds the icon now.
    shown: bool,
    /// Whether the shell took notification version 4 for the icon, which
    /// decides what its callback messages mean.
    version_4: bool,
}

impl<S: Shell> NotifyIcon<S> {
    /// The icon, hidden, with the identity `guid` derived for it — or the
    /// numeric id, with a diagnostic, if none could be.
    fn new(mut shell: S, guid: Result<GUID, String>) -> NotifyIcon<S> {
        let identity = match guid {
            Ok(guid) => Identity::Guid(guid),
            Err(why) => {
                shell.report(&format!(
                    "Pane's tray icon has no identity of its own ({why}); it uses a number \
                     instead, so Windows may not keep its notification-area setting"
                ));
                Identity::Number(ICON_ID)
            }
        };
        let variant = shell.taskbar();
        NotifyIcon {
            shell,
            identity,
            variant,
            paused: false,
            wanted: false,
            shown: false,
            version_4: false,
        }
    }

    /// The icon as `identity`, at the variant in effect.
    fn notification(&self, identity: Identity) -> Notification {
        Notification {
            identity,
            variant: self.variant,
            tip: if self.paused { TIP_PAUSED } else { TIP },
            callback: TRAY_MESSAGE,
        }
    }

    /// Shows the icon, or hides it. Repeating the state in effect
    /// succeeds; `Err` says why the system refused the change, and the
    /// icon stays as it was.
    fn set_visible(&mut self, visible: bool) -> Result<(), TrayError> {
        self.wanted = visible;
        if self.shown == visible {
            return Ok(());
        }
        if visible {
            self.add().map_err(TrayError::Refused)
        } else {
            let identity = self.identity;
            self.shell.delete(identity).map_err(TrayError::Refused)?;
            self.shown = false;
            Ok(())
        }
    }

    /// Says in the icon's tooltip whether Pane's hotkeys are paused for a
    /// game in front (game mode, #125): the icon is changed in place,
    /// where the notification area holds it, and the choice is kept for
    /// the next add where it does not. Repeating the state in effect
    /// succeeds.
    fn set_hotkeys_paused(&mut self, paused: bool) -> Result<(), TrayError> {
        if self.paused == paused {
            return Ok(());
        }
        self.paused = paused;
        self.redraw();
        Ok(())
    }

    /// Adds the icon as its identity. A GUID the system refuses gives way
    /// to the numeric id, for good, with a diagnostic saying why.
    fn add(&mut self) -> Result<(), String> {
        let refused = match self.add_as(self.identity) {
            Ok(()) => return Ok(()),
            Err(why) => why,
        };
        let Identity::Guid(guid) = self.identity else {
            return Err(refused);
        };
        let number = Identity::Number(ICON_ID);
        if self.add_as(number).is_err() {
            // Nothing took: the GUID's refusal is the one to explain, and
            // the GUID stays the identity to try next time.
            return Err(refused);
        }
        self.identity = number;
        self.shell.report(&format!(
            "Windows refused Pane's tray icon identity {guid:?} ({refused}); the icon uses a \
             number instead, so Windows may not keep its notification-area setting"
        ));
        Ok(())
    }

    /// Adds the icon as `identity`, then asks for notification version 4.
    /// A refused add is tried once more after removing any icon of that
    /// identity the notification area still holds — one a previous run of
    /// Pane left when it ended without removing it.
    fn add_as(&mut self, identity: Identity) -> Result<(), String> {
        let icon = self.notification(identity);
        if self.shell.add(&icon).is_err() {
            let _ = self.shell.delete(identity);
            self.shell.add(&icon)?;
        }
        self.shown = true;
        self.version_4 = match self.shell.set_version(identity) {
            Ok(()) => true,
            Err(why) => {
                self.shell.report(&format!(
                    "Windows refused the notification version of Pane's tray icon ({why}); \
                     the icon answers clicks as older versions of Windows send them"
                ));
                false
            }
        };
        Ok(())
    }

    /// Explorer restarted, and the notification area forgot every icon:
    /// adds the icon again while Pane asked for it to show. A refusal is
    /// reported and left for the next broadcast.
    fn taskbar_created(&mut self) {
        self.shown = false;
        if !self.wanted {
            return;
        }
        if let Err(why) = self.add() {
            self.shell.report(&format!(
                "Windows refused to show Pane's tray icon again after Explorer restarted \
                 ({why}); Pane tries again when Explorer next starts its taskbar"
            ));
        }
    }

    /// The system's theme changed: the icon takes the variant the
    /// taskbar's theme asks for now, in place.
    fn theme_changed(&mut self) {
        let variant = self.shell.taskbar();
        if variant == self.variant {
            return;
        }
        self.variant = variant;
        self.redraw();
    }

    /// Gives the notification area the icon again, in place, if it holds
    /// it: after the theme or the display changed.
    fn redraw(&mut self) {
        if !self.shown {
            return;
        }
        let icon = self.notification(self.identity);
        if let Err(why) = self.shell.modify(&icon) {
            let diagnostic = format!("Windows refused to change Pane's tray icon ({why})");
            self.shell.report(&diagnostic);
        }
    }
}

/// The GUID of the tray icon of the program at `program`, its canonical
/// path: a name-based UUID (version 5, SHA-1, RFC 9562) in Pane's
/// namespace, of the path's UTF-16 code units, little-endian, as Windows
/// holds the path — without the verbatim prefix `std::fs::canonicalize`
/// gives it (`\\?\C:\…` is `C:\…`, `\\?\UNC\server\…` is `\\server\…`).
fn icon_guid(program: &Path) -> GUID {
    let mut name = Vec::new();
    for unit in without_verbatim_prefix(program) {
        name.extend_from_slice(&unit.to_le_bytes());
    }
    let mut hasher = Sha1::new();
    hasher.update(&NAMESPACE);
    hasher.update(&name);
    // A collision attack on a program's path is no threat to an icon's
    // identity: the hash is used whatever the detection says.
    let result = hasher.try_finalize();
    let digest: [u8; 20] = (*result.hash()).into();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // The version (5) and the variant (RFC 9562's).
    bytes[6] = (bytes[6] & 0x0F) | 0x50;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    GUID::from_u128(u128::from_be_bytes(bytes))
}

/// The UTF-16 code units of `path` as it is usually written: without the
/// verbatim prefix `\\?\` (`\\?\UNC\` becomes `\\`).
fn without_verbatim_prefix(path: &Path) -> Vec<u16> {
    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    let wide = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
    if let Some(rest) = units.strip_prefix(wide(r"\\?\UNC\").as_slice()) {
        let mut plain = wide(r"\\");
        plain.extend_from_slice(rest);
        plain
    } else if let Some(rest) = units.strip_prefix(wide(r"\\?\").as_slice()) {
        rest.to_vec()
    } else {
        units
    }
}

/// The GUID of this program's tray icon, or why there is none.
fn program_guid() -> Result<GUID, String> {
    let program = std::env::current_exe()
        .map_err(|error| format!("Pane could not find its own program: {error}"))?;
    let canonical = std::fs::canonicalize(&program).map_err(|error| {
        format!(
            "Pane could not resolve its program's path {}: {error}",
            program.display()
        )
    })?;
    Ok(icon_guid(&canonical))
}

/// The variant the taskbar's theme asks for. Under high contrast, the one
/// that stands out from the high-contrast theme's window colour
/// `high_contrast` (a `COLORREF`); otherwise the taskbar's own theme, the
/// personalization value `read(TASKBAR_THEME)` — never the applications'
/// theme (`AppsUseLightTheme`), which can differ, and never Pane's own
/// Appearance choice. A Windows without the value (before Windows 10
/// 1903) has a dark taskbar.
fn taskbar_variant(read: impl Fn(&str) -> Option<u32>, high_contrast: Option<u32>) -> Variant {
    if let Some(window) = high_contrast {
        return if bright(window) {
            Variant::Dark
        } else {
            Variant::Light
        };
    }
    match read(TASKBAR_THEME) {
        Some(light) if light != 0 => Variant::Dark,
        _ => Variant::Light,
    }
}

/// Whether the `COLORREF` `colour` is bright enough that black stands
/// out from it more than white does: its relative luminance is above
/// the point where the two contrast equally.
fn bright(colour: u32) -> bool {
    let channel = |shift: u32| {
        let value = f64::from((colour >> shift) & 0xFF) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(0) + 0.7152 * channel(8) + 0.0722 * channel(16) > 0.179
}

/// The current user's personalization value `value`, a `REG_DWORD`, if
/// it is set.
fn personalization(value: &str) -> Option<u32> {
    let (key, value) = (HSTRING::from(PERSONALIZE), HSTRING::from(value));
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: `data` holds `size` bytes and outlives the call; the flag
    // accepts a `REG_DWORD` only, which fits it.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            &value,
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut data).cast::<core::ffi::c_void>()),
            Some(&raw mut size),
        )
    };
    (read == ERROR_SUCCESS).then_some(data)
}

/// The high-contrast theme's window colour, while high contrast is on.
fn high_contrast_window() -> Option<u32> {
    let mut contrast = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..HIGHCONTRASTW::default()
    };
    // SAFETY: `contrast` is the structure the action fills, its size set,
    // and outlives the call.
    unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            Some((&raw mut contrast).cast::<core::ffi::c_void>()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .ok()?;
    if !contrast.dwFlags.contains(HCF_HIGHCONTRASTON) {
        return None;
    }
    // SAFETY: no pointers; it reads a system colour.
    Some(unsafe { GetSysColor(COLOR_WINDOW) })
}

/// The notification area's small-icon size, in pixels, at `dpi` (that of
/// the icon's window; 0 when it has none, read as 100%).
fn small_icon_size(dpi: u32) -> i32 {
    let dpi = if dpi == 0 { 96 } else { dpi };
    // SAFETY: no pointers; it reads a system metric.
    let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) };
    if size > 0 {
        size
    } else {
        (16 * dpi / 96) as i32
    }
}

/// The image of the `.ico` file `ico` to make an icon of `size` pixels
/// from: the image of that size, else the smallest larger one, else the
/// largest. `None` for bytes that are not an icon file.
fn icon_image(ico: &[u8], size: u32) -> Option<&[u8]> {
    let u16_at = |at: usize| -> Option<u16> {
        Some(u16::from_le_bytes(ico.get(at..at + 2)?.try_into().ok()?))
    };
    let u32_at = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(ico.get(at..at + 4)?.try_into().ok()?))
    };
    if u16_at(0)? != 0 || u16_at(2)? != 1 {
        return None;
    }
    let mut best: Option<(u32, usize, usize)> = None;
    for index in 0..usize::from(u16_at(4)?) {
        let entry = 6 + 16 * index;
        let width = match *ico.get(entry)? {
            0 => 256,
            width => u32::from(width),
        };
        let length = u32_at(entry + 8)? as usize;
        let offset = u32_at(entry + 12)? as usize;
        let better = match best {
            None => true,
            // One that fits already gives way only to a smaller one that
            // fits too; one too small, to any larger one.
            Some((chosen, ..)) if chosen >= size => width >= size && width < chosen,
            Some((chosen, ..)) => width > chosen,
        };
        if better {
            best = Some((width, offset, length));
        }
    }
    let (_, offset, length) = best?;
    ico.get(offset..offset.checked_add(length)?)
}

/// Pane's mark in `variant`, made at `size` pixels from its embedded
/// icon file. The caller destroys it.
fn load_icon(variant: Variant, size: i32) -> Result<HICON, String> {
    let file = match variant {
        Variant::Dark => MARK_DARK,
        Variant::Light => MARK_LIGHT,
    };
    let image = icon_image(file, size.max(1) as u32)
        .ok_or_else(|| "the embedded icon file has no image".to_string())?;
    // SAFETY: `image` is one image of an icon file — a header and its
    // pixels, as an icon resource holds them — and outlives the call;
    // 0x00030000 is the format version every Windows since 3.x reads.
    unsafe { CreateIconFromResourceEx(image, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR) }
        .map_err(|error| error.message())
}

/// The notification area itself: the [`Shell`] the adapter uses. It owns
/// the icon's window and the icon images it gave the notification area.
struct NotificationArea {
    /// The icon's owner, whose window procedure receives its callbacks.
    window: Window,
    /// The image the notification area was last given — its variant,
    /// size and handle — kept until it is replaced.
    loaded: Option<(Variant, i32, HICON)>,
    /// Images replaced, destroyed once the call that replaced them is
    /// made.
    retired: Vec<HICON>,
}

impl NotificationArea {
    /// The image of `variant` at the small-icon size for the window's
    /// display, made once for each change of either. A build whose image
    /// cannot be made shows the system's application icon, with a
    /// diagnostic.
    fn icon(&mut self, variant: Variant) -> HICON {
        // SAFETY: this thread's own window.
        let size = small_icon_size(unsafe { GetDpiForWindow(self.window.handle()) });
        if let Some((loaded, at, icon)) = self.loaded
            && loaded == variant
            && at == size
        {
            return icon;
        }
        match load_icon(variant, size) {
            Ok(icon) => {
                if let Some((_, _, old)) = self.loaded.replace((variant, size, icon)) {
                    self.retired.push(old);
                }
                icon
            }
            Err(why) => {
                log(&format!(
                    "Pane could not make its tray icon ({why}); it shows the system's \
                     application icon instead"
                ));
                // SAFETY: the shared system icon, which no one owns.
                unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default()
            }
        }
    }

    /// Calls `Shell_NotifyIconW` with `message` for the icon as
    /// `identity`, with `icon`'s image, tooltip and callback when given.
    fn notify(
        &mut self,
        message: NOTIFY_ICON_MESSAGE,
        identity: Identity,
        icon: Option<&Notification>,
    ) -> Result<(), String> {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.window.handle(),
            ..NOTIFYICONDATAW::default()
        };
        match identity {
            Identity::Guid(guid) => {
                data.uFlags |= NIF_GUID;
                data.guidItem = guid;
            }
            Identity::Number(number) => data.uID = number,
        }
        if let Some(icon) = icon {
            // With version 4 the standard tooltip shows only when asked
            // for (`NIF_SHOWTIP`).
            data.uFlags |= NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
            data.uCallbackMessage = icon.callback;
            data.hIcon = self.icon(icon.variant);
            let tip = wide(icon.tip);
            let length = tip.len().min(data.szTip.len() - 1);
            data.szTip[..length].copy_from_slice(&tip[..length]);
        }
        if message == NIM_SETVERSION {
            data.Anonymous = NOTIFYICONDATAW_0 {
                uVersion: NOTIFYICON_VERSION_4,
            };
        }
        // SAFETY: `data` is fully initialized; the window and the image
        // are this thread's own.
        let done = unsafe { Shell_NotifyIconW(message, &data) };
        // It reads the calling thread's last error, which a failed call
        // just left there — before anything else can change it.
        let error = (!done.as_bool()).then(::windows::core::Error::from_thread);
        for old in self.retired.drain(..) {
            // SAFETY: an image this shell made and no longer gives out.
            unsafe { DestroyIcon(old) }.ok();
        }
        match error {
            None => Ok(()),
            // The notification area often refuses without a reason.
            Some(error) if error.code().is_ok() => {
                Err("the notification area did not take it".into())
            }
            Some(error) => Err(error.message()),
        }
    }
}

impl Shell for NotificationArea {
    fn add(&mut self, icon: &Notification) -> Result<(), String> {
        self.notify(NIM_ADD, icon.identity, Some(icon))
    }

    fn set_version(&mut self, identity: Identity) -> Result<(), String> {
        self.notify(NIM_SETVERSION, identity, None)
    }

    fn modify(&mut self, icon: &Notification) -> Result<(), String> {
        self.notify(NIM_MODIFY, icon.identity, Some(icon))
    }

    fn delete(&mut self, identity: Identity) -> Result<(), String> {
        self.notify(NIM_DELETE, identity, None)
    }

    fn taskbar(&self) -> Variant {
        taskbar_variant(personalization, high_contrast_window())
    }

    fn report(&mut self, diagnostic: &str) {
        log(diagnostic);
    }
}

impl Drop for NotificationArea {
    /// Destroys the images it made; the window goes with its own drop.
    fn drop(&mut self) {
        let loaded = self.loaded.take().map(|(_, _, icon)| icon);
        for icon in self.retired.drain(..).chain(loaded) {
            // SAFETY: an image this shell made, on this thread.
            unsafe { DestroyIcon(icon) }.ok();
        }
    }
}

/// A change to apply, with where to answer it.
enum Request {
    Show(mpsc::Sender<Result<(), TrayError>>),
    Hide(mpsc::Sender<Result<(), TrayError>>),
    /// Says in the tooltip whether Pane's hotkeys are paused for a game
    /// (game mode, #125).
    Pause(bool, mpsc::Sender<Result<(), TrayError>>),
}

type Requests = Arc<Mutex<VecDeque<Request>>>;

/// The adapter: the thread that owns the icon and its menu.
pub struct WindowsTray {
    thread: MessageThread,
    requests: Requests,
}

/// What the tray thread owns: the icon, its menu, and where the menu's
/// selections go. Held in a thread-local the window procedure reads, as
/// the clipboard listener's is, on that thread only.
struct Entry {
    selections: SelectionSender,
    /// The icon's owner, which its menu belongs to.
    owner: HWND,
    /// The menu a right click opens; destroyed with the entry.
    menu: HMENU,
    /// The icon, with the window that owns it.
    icon: NotifyIcon<NotificationArea>,
}

thread_local! {
    static ENTRY: std::cell::RefCell<Option<Entry>> = const { std::cell::RefCell::new(None) };
}

/// A NUL-terminated wide copy of `text`, as the shell's APIs take it.
fn wide(text: &str) -> Vec<u16> {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    wide
}

/// The tray's menu, with the platform's own labels. Called on the tray
/// thread.
fn menu() -> Result<HMENU, String> {
    // SAFETY: a new menu of this thread.
    let menu = unsafe { CreatePopupMenu() }.map_err(|error| error.message())?;
    for (command, label) in [
        (OPEN_PANE, "Open Pane"),
        (SETTINGS, "Settings"),
        (EXIT, "Exit"),
    ] {
        let label = wide(label);
        // SAFETY: the menu this thread made; the label is fully
        // initialized for the call.
        unsafe {
            AppendMenuW(
                menu,
                MENU_ITEM_FLAGS(MF_STRING.0),
                command,
                PCWSTR(label.as_ptr()),
            )
        }
        .map_err(|error| error.message())?;
    }
    Ok(menu)
}

/// What a use of the icon asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Click {
    /// The primary action: the launcher is summoned.
    Summon,
    /// The menu opens.
    Menu,
}

/// What the shell's callback `event` (the low word of its `lParam`)
/// asks for, under notification version 4 or, when the shell refused
/// it, the older messages.
fn click(event: u32, version_4: bool) -> Option<Click> {
    if version_4 {
        // The shell's own selection events: a left click or Enter, and
        // a right click or the menu key. The raw mouse messages that
        // also arrive are left alone, so nothing happens twice.
        match event {
            NIN_SELECT | NIN_KEYSELECT => Some(Click::Summon),
            WM_CONTEXTMENU => Some(Click::Menu),
            _ => None,
        }
    } else {
        match event {
            WM_LBUTTONUP => Some(Click::Summon),
            WM_RBUTTONUP | WM_CONTEXTMENU => Some(Click::Menu),
            _ => None,
        }
    }
}

/// Opens the entry's menu at `anchor` (the pointer when none is given)
/// and reports the command chosen (0 when the menu was dismissed). Called
/// on the tray thread, from the window procedure.
fn open_menu(entry: &mut Entry, anchor: Option<POINT>) -> usize {
    let anchor = anchor.unwrap_or_else(|| {
        let mut where_clicked = POINT::default();
        // SAFETY: writes this thread's own point. A failure leaves the
        // origin, which is as good a place as any the menu can open at
        // when no pointer position is there (no pointer attached).
        let _ = unsafe { GetCursorPos(&mut where_clicked) };
        where_clicked
    });
    // The two calls the platform's tray menus need around them: the
    // owning window takes the foreground so the menu dismisses when the
    // user clicks elsewhere, and a no-op message follows the menu so the
    // taskbar gives the foreground back.
    // SAFETY: this thread's own window. The platform's answer is not
    // ours to act on: a refusal leaves the menu to dismiss on its own.
    let _ = unsafe { SetForegroundWindow(entry.owner) };
    let flags = TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0 | TPM_BOTTOMALIGN.0;
    // SAFETY: this thread's own menu and window; with `TPM_RETURNCMD`
    // the return value is the command chosen, not only whether it ran.
    let chosen =
        unsafe { TrackPopupMenuEx(entry.menu, flags, anchor.x, anchor.y, entry.owner, None) };
    // SAFETY: this thread's own window; a no-op message.
    unsafe { PostMessageW(Some(entry.owner), WM_NULL, WPARAM(0), LPARAM(0)) }.ok();
    chosen.0 as usize
}

/// Whether the `lParam` of a `WM_SETTINGCHANGE` names the setting `name`.
fn setting_named(lparam: LPARAM, name: &str) -> bool {
    if lparam.0 == 0 {
        return false;
    }
    let setting = PCWSTR(lparam.0 as *const u16);
    // SAFETY: a `WM_SETTINGCHANGE`'s non-zero `lParam` is the
    // NUL-terminated name of the setting that changed, valid while the
    // message is handled.
    let setting = unsafe { setting.as_wide() };
    setting.iter().copied().eq(name.encode_utf16())
}

/// Runs `change` on the entry this thread holds, if it holds one and
/// nothing is using it already (nothing a click waits on calls back into
/// it); whether it ran.
fn with_entry(change: impl FnOnce(&mut Entry)) -> bool {
    ENTRY.with(|entry| {
        if let Ok(mut held) = entry.try_borrow_mut()
            && let Some(entry) = held.as_mut()
        {
            change(entry);
            return true;
        }
        false
    })
}

/// A broadcast the icon acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Broadcast {
    /// Explorer started its taskbar (`TaskbarCreated`).
    TaskbarCreated,
    /// The theme or high contrast changed.
    ThemeChanged,
    /// The display's size or scaling changed.
    DisplayChanged,
}

thread_local! {
    /// The broadcasts that arrived while the entry was in use — its menu
    /// is open, and the menu's modal loop goes on dispatching this
    /// window's messages — each once, applied when the menu has closed.
    static DEFERRED: std::cell::RefCell<Vec<Broadcast>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Applies `broadcast` to the icon, or keeps it for when the entry is free
/// again (see [`DEFERRED`]), so an Explorer restart or a theme change while
/// the menu is open is not lost.
fn broadcast(event: Broadcast) {
    let applied = with_entry(|entry| match event {
        Broadcast::TaskbarCreated => entry.icon.taskbar_created(),
        Broadcast::ThemeChanged => entry.icon.theme_changed(),
        Broadcast::DisplayChanged => entry.icon.redraw(),
    });
    if !applied {
        let _ = DEFERRED.try_with(|deferred| {
            let mut deferred = deferred.borrow_mut();
            if !deferred.contains(&event) {
                deferred.push(event);
            }
        });
    }
}

/// Applies the broadcasts kept while the entry was in use, in the order
/// they arrived.
fn apply_deferred() {
    let deferred = DEFERRED
        .try_with(|deferred| std::mem::take(&mut *deferred.borrow_mut()))
        .unwrap_or_default();
    for each in deferred {
        broadcast(each);
    }
}

/// The tray window's procedure: the shell's callback messages for the
/// icon, the broadcasts that ask it to be added again or redrawn, and
/// the stop message. A panic must not unwind into Windows, which would
/// end Pane.
extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let handled = std::panic::catch_unwind(|| handle(window, message, wparam, lparam));
    match handled {
        Ok(result) => result,
        Err(_) => {
            log("Pane's tray entry failed on a message; it goes on listening");
            LRESULT(0)
        }
    }
}

fn handle(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if stop_sent(message) {
        return LRESULT(0);
    }
    if message == TRAY_MESSAGE {
        // The low word of `lParam` is the event. With version 4,
        // `wParam` is where the menu belongs, in screen coordinates.
        let event = (lparam.0 & 0xFFFF) as u32;
        with_entry(|entry| {
            let version_4 = entry.icon.version_4;
            let anchor = version_4.then_some(POINT {
                x: i32::from((wparam.0 & 0xFFFF) as u16 as i16),
                y: i32::from(((wparam.0 >> 16) & 0xFFFF) as u16 as i16),
            });
            let action = match click(event, version_4) {
                Some(Click::Summon) => Some(TrayAction::OpenPane),
                Some(Click::Menu) => match open_menu(entry, anchor) {
                    OPEN_PANE => Some(TrayAction::OpenPane),
                    SETTINGS => Some(TrayAction::Settings),
                    EXIT => Some(TrayAction::Quit),
                    _ => None,
                },
                None => None,
            };
            if let Some(action) = action {
                entry.selections.send(action);
            }
        });
        // What arrived while the menu was open.
        apply_deferred();
        return LRESULT(0);
    }
    let created = TASKBAR_CREATED.load(Ordering::Relaxed);
    if created != 0 && message == created {
        broadcast(Broadcast::TaskbarCreated);
        return LRESULT(0);
    }
    // A theme change, or high contrast turned on or off; the display's
    // size or scaling changed.
    let theme = message == WM_SETTINGCHANGE
        && (wparam.0 as u32 == SPI_SETHIGHCONTRAST.0 || setting_named(lparam, THEME_SETTING));
    if theme {
        broadcast(Broadcast::ThemeChanged);
    } else if message == WM_DISPLAYCHANGE || message == WM_DPICHANGED {
        broadcast(Broadcast::DisplayChanged);
    }
    // SAFETY: the arguments are those this procedure was called with.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Registers `TaskbarCreated`, the message Windows broadcasts when
/// Explorer has started its taskbar, for the icon's window `owner`.
fn listen_for_taskbar(owner: HWND) {
    // SAFETY: a constant, NUL-terminated name.
    let created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    if created == 0 {
        return;
    }
    TASKBAR_CREATED.store(created, Ordering::Relaxed);
    // An elevated Pane would otherwise never hear Explorer, which runs at
    // a lower integrity level. A refusal leaves the message to arrive as
    // it can.
    // SAFETY: this thread's own window; no filter structure is asked for.
    let _ = unsafe { ChangeWindowMessageFilterEx(owner, created, MSGFLT_ALLOW, None) };
}

/// Writes `message` to standard error, if there is one, and to Pane's log
/// (see `crate::diagnostics`). It cannot panic.
fn log(message: &str) {
    crate::diagnostics::report_line(message);
}

impl WindowsTray {
    /// Starts the tray thread: its window, the icon's owner, and the
    /// menu a right click opens. Returns once they are made, with the
    /// reason they could not be.
    pub fn start(selections: SelectionSender) -> Result<WindowsTray, String> {
        let requests: Requests = Arc::default();
        let served = requests.clone();
        let thread = MessageThread::spawn(
            "pane-tray",
            move || {
                let window = TRAY_CLASS.hidden_window()?;
                let menu = menu()?;
                let owner = window.handle();
                listen_for_taskbar(owner);
                let area = NotificationArea {
                    window,
                    loaded: None,
                    retired: Vec::new(),
                };
                let icon = NotifyIcon::new(area, program_guid());
                ENTRY.with(|entry| {
                    entry.replace(Some(Entry {
                        selections,
                        owner,
                        menu,
                        icon,
                    }))
                });
                Ok(((), Some(owner)))
            },
            move |_, message| serve(message, &served),
            |()| finish(),
        )?;
        Ok(WindowsTray { thread, requests })
    }

    /// Queues `request` and wakes the thread; false if it is gone.
    fn send(&self, request: Request) -> bool {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_back(request);
        self.thread.post(WM_WAKE)
    }
}

/// Serves the queued show and hide requests. Runs on the tray thread.
fn serve(message: &MSG, requests: &Requests) {
    if message.message != WM_WAKE {
        return;
    }
    loop {
        let request = requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pop_front();
        match request {
            None => break,
            Some(Request::Show(answer)) => {
                let _ = answer.send(change(true));
            }
            Some(Request::Hide(answer)) => {
                let _ = answer.send(change(false));
            }
            Some(Request::Pause(paused, answer)) => {
                let _ = answer.send(paused_tip(paused));
            }
        }
    }
    // A broadcast that arrived while a change held the entry.
    apply_deferred();
}

/// Applies the pause to the entry this thread holds. Runs on the tray
/// thread.
fn paused_tip(paused: bool) -> Result<(), TrayError> {
    ENTRY.with(|entry| {
        let Ok(mut held) = entry.try_borrow_mut() else {
            return Err(TrayError::Refused(
                "the tray entry was busy on a click".to_string(),
            ));
        };
        match held.as_mut() {
            Some(entry) => entry.icon.set_hotkeys_paused(paused),
            None => Err(TrayError::Refused(
                "the tray entry was not made".to_string(),
            )),
        }
    })
}

/// Applies `shown` to the entry this thread holds. Runs on the tray
/// thread.
fn change(shown: bool) -> Result<(), TrayError> {
    ENTRY.with(|entry| {
        let Ok(mut held) = entry.try_borrow_mut() else {
            return Err(TrayError::Refused(
                "the tray entry was busy on a click".to_string(),
            ));
        };
        match held.as_mut() {
            Some(entry) => entry.icon.set_visible(shown),
            None => Err(TrayError::Refused(
                "the tray entry was not made".to_string(),
            )),
        }
    })
}

/// Deletes the icon, destroys the menu and the window that owned them.
/// Runs on the tray thread, as it ends.
fn finish() {
    ENTRY.with(|entry| {
        if let Ok(mut held) = entry.try_borrow_mut()
            && let Some(mut entry) = held.take()
        {
            // The icon goes first, while the window it belongs to is
            // still there for the shell to find.
            let _ = entry.icon.set_visible(false);
            // SAFETY: this thread's own menu.
            unsafe { DestroyMenu(entry.menu) }.ok();
            // The window and the icon's images go with the entry: their
            // own drops destroy them, on this thread.
        }
    });
}

impl Tray for WindowsTray {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn set_visible(&self, visible: bool) -> Result<(), TrayError> {
        let (answer, answered) = mpsc::channel();
        let request = if visible {
            Request::Show(answer)
        } else {
            Request::Hide(answer)
        };
        if !self.send(request) {
            return Err(TrayError::Refused("the tray thread stopped".into()));
        }
        answered
            .recv()
            .unwrap_or_else(|_| Err(TrayError::Refused("the tray thread stopped".into())))
    }

    fn set_hotkeys_paused(&self, paused: bool) -> Result<(), TrayError> {
        let (answer, answered) = mpsc::channel();
        if !self.send(Request::Pause(paused, answer)) {
            return Err(TrayError::Refused("the tray thread stopped".into()));
        }
        answered
            .recv()
            .unwrap_or_else(|_| Err(TrayError::Refused("the tray thread stopped".into())))
    }
}

impl Drop for WindowsTray {
    /// Ends the thread, which deletes the icon and destroys the menu and
    /// the window as it ends. It never waits on another program, so this
    /// waits until it has.
    fn drop(&mut self) {
        self.thread.stop(None);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use ::windows::Win32::UI::Shell::NIN_SELECT;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, WM_CONTEXTMENU, WM_LBUTTONUP, WM_RBUTTONUP,
    };
    use ::windows::core::GUID;

    use super::{
        Click, ICON_ID, Identity, MARK_DARK, MARK_LIGHT, NIN_KEYSELECT, Notification, NotifyIcon,
        Shell, TIP, TIP_PAUSED, TRAY_MESSAGE, TrayError, Variant, click, icon_guid, icon_image,
        load_icon, small_icon_size, taskbar_variant,
    };

    /// A call the icon made to the notification area.
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Call {
        Add(Notification),
        SetVersion(Identity),
        Modify(Notification),
        Delete(Identity),
    }

    /// A notification area of the test's own: it records every call,
    /// refuses what the test says, and has the taskbar theme the test
    /// sets.
    #[derive(Default)]
    struct FakeShell {
        calls: Vec<Call>,
        reports: Vec<String>,
        /// Refuses every add, as a notification area not there yet does.
        refuse_adds: bool,
        /// Refuses an add by GUID, as Windows does for a GUID tied to
        /// another program's path.
        refuse_guids: bool,
        refuse_version: bool,
        /// Refuses every removal.
        refuse_deletes: bool,
        light_taskbar: bool,
    }

    impl Shell for FakeShell {
        fn add(&mut self, icon: &Notification) -> Result<(), String> {
            self.calls.push(Call::Add(*icon));
            let guid = matches!(icon.identity, Identity::Guid(_));
            if self.refuse_adds || (guid && self.refuse_guids) {
                return Err("refused".into());
            }
            Ok(())
        }

        fn set_version(&mut self, identity: Identity) -> Result<(), String> {
            self.calls.push(Call::SetVersion(identity));
            if self.refuse_version {
                return Err("no version 4".into());
            }
            Ok(())
        }

        fn modify(&mut self, icon: &Notification) -> Result<(), String> {
            self.calls.push(Call::Modify(*icon));
            Ok(())
        }

        fn delete(&mut self, identity: Identity) -> Result<(), String> {
            self.calls.push(Call::Delete(identity));
            if self.refuse_deletes {
                return Err("refused".into());
            }
            Ok(())
        }

        fn taskbar(&self) -> Variant {
            if self.light_taskbar {
                Variant::Dark
            } else {
                Variant::Light
            }
        }

        fn report(&mut self, diagnostic: &str) {
            self.reports.push(diagnostic.to_owned());
        }
    }

    fn guid() -> GUID {
        icon_guid(Path::new(r"C:\Program Files\Pane\pane.exe"))
    }

    fn fresh() -> NotifyIcon<FakeShell> {
        NotifyIcon::new(FakeShell::default(), Ok(guid()))
    }

    /// The icon as it is added: its GUID, tooltip and callback.
    fn added(variant: Variant) -> Notification {
        Notification {
            identity: Identity::Guid(guid()),
            variant,
            tip: TIP,
            callback: TRAY_MESSAGE,
        }
    }

    #[test]
    fn saying_the_hotkeys_are_paused_changes_the_tooltip_and_it_survives_a_taskbar_restart() {
        let mut icon = fresh();
        icon.set_visible(true).unwrap();
        icon.shell.calls.clear();
        // Game mode paused Pane's hotkeys for a game in front (#125): the
        // icon is changed in place, its tooltip saying so.
        icon.set_hotkeys_paused(true).unwrap();
        assert_eq!(
            icon.shell.calls,
            [Call::Modify(Notification {
                tip: TIP_PAUSED,
                ..added(Variant::Light)
            })]
        );
        // Explorer restarted: the icon is added again with the same
        // tooltip, not the resting one.
        icon.shell.calls.clear();
        icon.taskbar_created();
        assert_eq!(
            icon.shell.calls,
            [
                Call::Add(Notification {
                    tip: TIP_PAUSED,
                    ..added(Variant::Light)
                }),
                Call::SetVersion(Identity::Guid(guid())),
            ]
        );
        // The game left the front: the tooltip is Pane's again. Repeating
        // either state changes nothing, as it does not while hidden.
        icon.shell.calls.clear();
        icon.set_hotkeys_paused(true).unwrap();
        assert!(icon.shell.calls.is_empty(), "{:?}", icon.shell.calls);
        icon.set_hotkeys_paused(false).unwrap();
        assert_eq!(icon.shell.calls, [Call::Modify(added(Variant::Light))]);
        // A hidden icon keeps the choice for its next add: nothing is
        // changed in place while it is not there to change.
        icon.set_visible(false).unwrap();
        icon.shell.calls.clear();
        icon.set_hotkeys_paused(true).unwrap();
        assert!(icon.shell.calls.is_empty(), "{:?}", icon.shell.calls);
        icon.set_visible(true).unwrap();
        assert!(
            icon.shell.calls.contains(&Call::Add(Notification {
                tip: TIP_PAUSED,
                ..added(Variant::Light)
            })),
            "{:?}",
            icon.shell.calls
        );
    }

    #[test]
    fn taskbar_created_adds_the_icon_again_while_it_should_show() {
        let mut icon = fresh();
        icon.set_visible(true).unwrap();
        let first = icon.shell.calls.clone();
        assert_eq!(
            first,
            [
                Call::Add(added(Variant::Light)),
                Call::SetVersion(Identity::Guid(guid())),
            ]
        );
        icon.shell.calls.clear();
        // Explorer restarted and broadcast `TaskbarCreated`: the icon is
        // added again exactly as it first was, version and all.
        icon.taskbar_created();
        assert_eq!(icon.shell.calls, first);
        assert!(icon.shell.reports.is_empty(), "{:?}", icon.shell.reports);
        // And again at the next restart.
        icon.shell.calls.clear();
        icon.taskbar_created();
        assert_eq!(icon.shell.calls, first);
    }

    #[test]
    fn a_hidden_icon_stays_hidden_after_taskbar_created() {
        // Never shown.
        let mut icon = fresh();
        icon.taskbar_created();
        assert!(icon.shell.calls.is_empty(), "{:?}", icon.shell.calls);
        // Shown, then hidden.
        icon.set_visible(true).unwrap();
        icon.set_visible(false).unwrap();
        icon.shell.calls.clear();
        icon.taskbar_created();
        assert!(icon.shell.calls.is_empty(), "{:?}", icon.shell.calls);
        // Hiding it again changes nothing either: it is not there.
        icon.set_visible(false).unwrap();
        assert!(icon.shell.calls.is_empty(), "{:?}", icon.shell.calls);
    }

    #[test]
    fn a_refused_re_add_is_reported_and_tried_again_at_the_next_broadcast() {
        let mut icon = fresh();
        icon.set_visible(true).unwrap();
        icon.shell.calls.clear();
        icon.shell.refuse_adds = true;
        icon.taskbar_created();
        assert!(!icon.shown);
        assert_eq!(icon.shell.reports.len(), 1, "{:?}", icon.shell.reports);
        assert!(
            icon.shell.reports[0].contains("tries again"),
            "{:?}",
            icon.shell.reports
        );
        // The preference is untouched: Pane still asks for the icon, and
        // a refused re-add never gives way to the numeric id.
        assert!(icon.wanted);
        assert_eq!(icon.identity, Identity::Guid(guid()));
        // The next broadcast finds the notification area ready.
        icon.shell.refuse_adds = false;
        icon.shell.calls.clear();
        icon.taskbar_created();
        assert!(icon.shown);
        assert_eq!(
            icon.shell.calls,
            [
                Call::Add(added(Variant::Light)),
                Call::SetVersion(Identity::Guid(guid())),
            ]
        );
        // Showing it now repeats the state in effect: nothing to call.
        icon.shell.calls.clear();
        assert_eq!(icon.set_visible(true), Ok(()));
        assert!(icon.shell.calls.is_empty(), "{:?}", icon.shell.calls);
    }

    #[test]
    fn a_show_refused_at_start_is_tried_again_when_the_taskbar_starts() {
        // Pane started at sign-in before Explorer's taskbar: the show is
        // refused, and the taskbar's first broadcast adds the icon.
        let mut icon = fresh();
        icon.shell.refuse_adds = true;
        assert!(matches!(icon.set_visible(true), Err(TrayError::Refused(_))));
        icon.shell.refuse_adds = false;
        icon.taskbar_created();
        assert!(icon.shown);
    }

    /// A change the system refused, which Settings rolls back by applying
    /// the preference again, leaves the icon following the preference when
    /// Explorer restarts: a refused show (the preference hidden) does not
    /// appear, and a refused hide (the preference shown) comes back.
    #[test]
    fn a_refused_change_rolled_back_to_the_preference_follows_it_after_explorer_restarts() {
        let mut icon = fresh();
        icon.shell.refuse_adds = true;
        assert!(matches!(icon.set_visible(true), Err(TrayError::Refused(_))));
        icon.shell.refuse_adds = false;
        assert_eq!(icon.set_visible(false), Ok(()), "the rollback");
        icon.shell.calls.clear();
        icon.taskbar_created();
        assert!(!icon.shown);
        assert!(icon.shell.calls.is_empty(), "{:?}", icon.shell.calls);

        let mut icon = fresh();
        icon.set_visible(true).unwrap();
        icon.shell.refuse_deletes = true;
        assert!(matches!(
            icon.set_visible(false),
            Err(TrayError::Refused(_))
        ));
        icon.shell.refuse_deletes = false;
        assert_eq!(icon.set_visible(true), Ok(()), "the rollback");
        icon.shell.calls.clear();
        icon.taskbar_created();
        assert!(icon.shown);
        assert_eq!(
            icon.shell.calls,
            [
                Call::Add(added(Variant::Light)),
                Call::SetVersion(Identity::Guid(guid())),
            ]
        );
    }

    #[test]
    fn the_guid_is_the_same_for_a_path_and_differs_for_another() {
        let installed = Path::new(r"C:\Program Files\Pane\pane.exe");
        let development = Path::new(r"C:\Users\dev\pane\target\debug\pane.exe");
        assert_eq!(icon_guid(installed), icon_guid(installed));
        assert_ne!(icon_guid(installed), icon_guid(development));
        // The canonical path Pane derives it from is verbatim
        // (`std::fs::canonicalize`); it names the same program.
        assert_eq!(
            icon_guid(Path::new(r"\\?\C:\Program Files\Pane\pane.exe")),
            icon_guid(installed)
        );
        assert_eq!(
            icon_guid(Path::new(r"\\?\UNC\server\share\pane.exe")),
            icon_guid(Path::new(r"\\server\share\pane.exe"))
        );
        // RFC 9562's version 5 of these names in Pane's namespace, as
        // Python's `uuid` computes them over the same bytes.
        assert_eq!(
            icon_guid(installed),
            GUID::from_u128(0x919b2a6c29de5d1197c5a6ccccda4900)
        );
        assert_eq!(
            icon_guid(development),
            GUID::from_u128(0x61dc83578fe65ee99c024fd13553ac4d)
        );
    }

    #[test]
    fn a_refused_guid_falls_back_to_the_numeric_id_with_a_diagnostic() {
        let mut icon = fresh();
        icon.shell.refuse_guids = true;
        assert_eq!(icon.set_visible(true), Ok(()));
        let number = Identity::Number(ICON_ID);
        // The GUID was tried (and tried again after clearing a stale icon
        // of it), then the numeric id took.
        assert_eq!(
            icon.shell.calls,
            [
                Call::Add(added(Variant::Light)),
                Call::Delete(Identity::Guid(guid())),
                Call::Add(added(Variant::Light)),
                Call::Add(Notification {
                    identity: number,
                    ..added(Variant::Light)
                }),
                Call::SetVersion(number),
            ]
        );
        assert_eq!(icon.shell.reports.len(), 1, "{:?}", icon.shell.reports);
        assert!(
            icon.shell.reports[0].contains(&format!("{:?}", guid())),
            "{:?}",
            icon.shell.reports
        );
        // The numeric id is the identity from now on: hidden by it, and
        // added again by it after Explorer restarts.
        icon.shell.calls.clear();
        icon.set_visible(false).unwrap();
        icon.set_visible(true).unwrap();
        icon.taskbar_created();
        assert!(
            icon.shell.calls.iter().all(|call| match call {
                Call::Add(shown) => shown.identity == number,
                Call::Delete(identity) | Call::SetVersion(identity) => *identity == number,
                Call::Modify(_) => false,
            }),
            "{:?}",
            icon.shell.calls
        );
        assert_eq!(icon.shell.reports.len(), 1, "{:?}", icon.shell.reports);
    }

    #[test]
    fn no_guid_uses_the_numeric_id_with_a_diagnostic() {
        let mut icon = NotifyIcon::new(FakeShell::default(), Err("no program".into()));
        assert_eq!(icon.identity, Identity::Number(ICON_ID));
        assert_eq!(icon.shell.reports.len(), 1);
        assert!(icon.shell.reports[0].contains("no program"));
        icon.set_visible(true).unwrap();
        assert!(matches!(
            icon.shell.calls[0],
            Call::Add(Notification {
                identity: Identity::Number(ICON_ID),
                ..
            })
        ));
    }

    #[test]
    fn a_refused_version_keeps_the_older_clicks() {
        let mut icon = fresh();
        icon.shell.refuse_version = true;
        icon.set_visible(true).unwrap();
        assert!(icon.shown);
        assert!(!icon.version_4);
        assert_eq!(icon.shell.reports.len(), 1, "{:?}", icon.shell.reports);
        icon.shell.refuse_version = false;
        icon.taskbar_created();
        assert!(icon.version_4);
    }

    #[test]
    fn a_light_taskbar_selects_the_dark_mark_and_a_dark_one_the_light_mark() {
        let light = FakeShell {
            light_taskbar: true,
            ..FakeShell::default()
        };
        let mut icon = NotifyIcon::new(light, Ok(guid()));
        icon.set_visible(true).unwrap();
        assert_eq!(icon.shell.calls[0], Call::Add(added(Variant::Dark)));
        let mut icon = fresh();
        icon.set_visible(true).unwrap();
        assert_eq!(icon.shell.calls[0], Call::Add(added(Variant::Light)));
    }

    #[test]
    fn a_theme_change_swaps_the_icon_in_place() {
        let mut icon = fresh();
        icon.set_visible(true).unwrap();
        icon.shell.calls.clear();
        // The taskbar turned light: the same icon, modified, never
        // removed and added.
        icon.shell.light_taskbar = true;
        icon.theme_changed();
        assert_eq!(icon.shell.calls, [Call::Modify(added(Variant::Dark))]);
        // A broadcast that changes nothing the taskbar shows (an accent
        // colour, say) leaves it alone.
        icon.shell.calls.clear();
        icon.theme_changed();
        assert!(icon.shell.calls.is_empty(), "{:?}", icon.shell.calls);
        // While hidden, the change waits for the next add.
        icon.set_visible(false).unwrap();
        icon.shell.light_taskbar = false;
        icon.shell.calls.clear();
        icon.theme_changed();
        assert!(icon.shell.calls.is_empty(), "{:?}", icon.shell.calls);
        icon.set_visible(true).unwrap();
        assert_eq!(icon.shell.calls[0], Call::Add(added(Variant::Light)));
    }

    #[test]
    fn the_taskbar_theme_alone_picks_the_variant() {
        let personalization = |values: &[(&str, u32)]| {
            let values: HashMap<String, u32> = values
                .iter()
                .map(|(name, value)| ((*name).to_owned(), *value))
                .collect();
            move |name: &str| values.get(name).copied()
        };
        // A dark taskbar beside light applications: the white mark. The
        // applications' theme is not what the taskbar shows, and Pane's
        // own Appearance choice is no input at all.
        let dark = personalization(&[("SystemUsesLightTheme", 0), ("AppsUseLightTheme", 1)]);
        assert_eq!(taskbar_variant(dark, None), Variant::Light);
        // A light taskbar beside dark applications: the black mark.
        let light = personalization(&[("SystemUsesLightTheme", 1), ("AppsUseLightTheme", 0)]);
        assert_eq!(taskbar_variant(light, None), Variant::Dark);
        // No setting (Windows before 10 1903): a dark taskbar.
        assert_eq!(taskbar_variant(personalization(&[]), None), Variant::Light);
    }

    #[test]
    fn high_contrast_follows_the_themes_own_window_colour() {
        let light_taskbar = |name: &str| (name == "SystemUsesLightTheme").then_some(1u32);
        // Black windows (Aquatic, Dusk, Night sky): the white mark, whatever
        // the taskbar setting says.
        assert_eq!(
            taskbar_variant(light_taskbar, Some(0x00_00_00_00)),
            Variant::Light
        );
        // White windows (Desert): the black mark.
        assert_eq!(
            taskbar_variant(|_: &str| Some(0u32), Some(0x00_FF_FF_FF)),
            Variant::Dark
        );
    }

    #[test]
    fn clicks_mean_the_same_under_either_notification_version() {
        // Version 4: the shell's own selections, and nothing twice.
        assert_eq!(click(NIN_SELECT, true), Some(Click::Summon));
        assert_eq!(click(NIN_KEYSELECT, true), Some(Click::Summon));
        assert_eq!(click(WM_CONTEXTMENU, true), Some(Click::Menu));
        assert_eq!(click(WM_LBUTTONUP, true), None);
        assert_eq!(click(WM_RBUTTONUP, true), None);
        // The older messages, when the shell refused version 4.
        assert_eq!(click(WM_LBUTTONUP, false), Some(Click::Summon));
        assert_eq!(click(WM_RBUTTONUP, false), Some(Click::Menu));
        assert_eq!(click(NIN_SELECT, false), None);
    }

    #[test]
    fn the_icon_is_made_at_the_small_icon_size_for_the_displays_dpi() {
        // 100% to 400% scaling.
        for dpi in [96, 120, 144, 168, 192, 216, 240, 288, 336, 384] {
            let size = small_icon_size(dpi);
            assert_eq!(size, 16 * dpi as i32 / 96, "at {dpi} DPI");
            for (variant, file) in [(Variant::Dark, MARK_DARK), (Variant::Light, MARK_LIGHT)] {
                // The file holds an image of exactly that size, so none
                // is scaled: its header's width.
                let image = icon_image(file, size as u32).unwrap();
                let width = i32::from_le_bytes(image[4..8].try_into().unwrap());
                assert_eq!(width, size, "{variant:?} at {dpi} DPI");
                let made = load_icon(variant, size).unwrap();
                // SAFETY: the icon this test made.
                unsafe { DestroyIcon(made) }.unwrap();
            }
        }
        // Between sizes, the next larger image; past the largest, the
        // largest; and no image from bytes that are not an icon file.
        let width = |image: &[u8]| i32::from_le_bytes(image[4..8].try_into().unwrap());
        assert_eq!(width(icon_image(MARK_DARK, 17).unwrap()), 20);
        assert_eq!(width(icon_image(MARK_DARK, 1000).unwrap()), 64);
        assert_eq!(icon_image(b"not an icon", 16), None);
        // No DPI (no window): 100%.
        assert_eq!(small_icon_size(0), 16);
    }
}
