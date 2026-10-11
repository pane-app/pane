//! The session and power commands on Windows (see the parent module):
//! each decision the parent makes is carried out through a documented
//! Windows API. The privilege a session-ending call needs is enabled for
//! that call alone and put back as it was after.
//!
//! - **Lock Screen**: `LockWorkStation`.
//! - **Log Out, Restart, Shut Down**: `ExitWindowsEx` with a planned
//!   reason (`SHTDN_REASON_FLAG_PLANNED`, an application's maintenance),
//!   `EWX_FORCE` — applications closed without the chance to save — for
//!   a restart and a shutdown only, as the user chose (ADR 0040), and the
//!   shutdown privilege (`SeShutdownPrivilege`) enabled around the call.
//! - **Sleep, Hibernate**: `SetSuspendState`, or the displays'
//!   monitor-power message on a computer that enters Modern Standby when
//!   they turn off.
//! - **Volume Up, Volume Down, Toggle Mute, Set Volume**: Core Audio: the
//!   default output endpoint's `IAudioEndpointVolume`, its level set as a
//!   scalar of 100 and its mute set with it.
//! - **Toggle Microphone Mute**: the active capture endpoints, each
//!   reached through its own `IAudioEndpointVolume` to set its mute.
//! - **Turn Off Displays, Start Screen Saver**: a `WM_SYSCOMMAND`
//!   (`SC_MONITORPOWER` or `SC_SCREENSAVE`) sent with `SendMessageTimeoutW`
//!   to a hidden window of Pane's own, never a broadcast that a hung
//!   application could block; Windows' own procedure (`DefWindowProcW`)
//!   turns the command into what it does. A screen saver that is off in
//!   Windows' settings (`SPI_GETSCREENSAVEACTIVE`) is explained rather
//!   than started.
//! - **What the computer can do**: `GetPwrCapabilities`, whose `AoAc`
//!   says the computer enters Modern Standby when its displays turn off,
//!   and whose `HiberFilePresent` says there is a hibernation file.
//! - **Open Recycle Bin, Empty Recycle Bin**: the Recycle Bin's known
//!   folder (`SHGetKnownFolderPath`) opened through the shell
//!   (`ShellExecuteExW`); `SHQueryRecycleBinW` says what the bin holds,
//!   so an already empty one is answered as a success, and
//!   `SHEmptyRecycleBinW` empties it with no confirmation of its own
//!   (the command asked first), no progress window and no sound.
//! - **Toggle System Appearance**: the personalization values
//!   (`AppsUseLightTheme`, `SystemUsesLightTheme`) read and written
//!   together through the registry, followed by a `WM_SETTINGCHANGE`
//!   broadcast naming the colour setting, sent with `SendMessageTimeoutW`
//!   and the abort-if-hung flag, so that a hung application cannot block
//!   it.
//! - **Toggle HDR**: the display configuration's advanced colour state:
//!   `QueryDisplayConfig` for the active paths, `DisplayConfigGetDeviceInfo`
//!   for whether a display supports and is showing advanced colour, and
//!   `DisplayConfigSetDeviceInfo` to turn it on or off.
//! - **Show Desktop**: the shell's own `ToggleDesktop` (`IShellDispatch4`).
//! - **Toggle Hidden Files**: File Explorer's `Hidden` value in its
//!   `Advanced` key, read and written, followed by the refreshing of
//!   every open Explorer window through the shell's windows collection
//!   (`IShellWindows`, each window's `Refresh`).
//! - **Eject Removable Drives**: each removable drive's volume is locked
//!   (`FSCTL_LOCK_VOLUME`) and dismounted (`FSCTL_DISMOUNT_VOLUME`), then
//!   the disk behind it — found by the device number the volume and the
//!   disk-drive class both answer — is ejected through the configuration
//!   manager (`CM_Request_Device_EjectW`).
//! - **Toggle Bluetooth**: the radios API: `BluetoothFindFirstRadio` and
//!   `BluetoothFindNextRadio` find the radios,
//!   `BluetoothEnumerateInstalledServices` says whether the radio's
//!   provider service is enabled (which is the radio being on), and
//!   `BluetoothSetServiceState` enables or disables it for the radio's
//!   own address, which turns the radio on or off.

use std::mem::size_of;

use ::windows::Win32::Devices::Bluetooth::{
    BLUETOOTH_ADDRESS, BLUETOOTH_ADDRESS_0, BLUETOOTH_DEVICE_INFO, BLUETOOTH_FIND_RADIO_PARAMS,
    BLUETOOTH_RADIO_INFO, BLUETOOTH_SERVICE_DISABLE, BLUETOOTH_SERVICE_ENABLE,
    BluetoothEnumerateInstalledServices, BluetoothFindFirstRadio, BluetoothFindNextRadio,
    BluetoothFindRadioClose, BluetoothGetRadioInfo, BluetoothSetServiceState, SVCID_BTH_PROVIDER,
};
use ::windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Request_Device_EjectW, CR_SUCCESS, DIGCF_PRESENT, GUID_DEVCLASS_DISKDRIVE, HDEVINFO,
    PNP_VETO_TYPE, PNP_VetoInsufficientRights, PNP_VetoOutstandingOpen, PNP_VetoPendingClose,
    PNP_VetoWindowsApp, PNP_VetoWindowsService, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList,
    SetupDiEnumDeviceInfo, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW,
};
use ::windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE,
    DisplayConfigGetDeviceInfo, DisplayConfigSetDeviceInfo, GetDisplayConfigBufferSizes,
    QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use ::windows::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, GENERIC_READ, HANDLE, HWND, LPARAM, LRESULT,
    LUID, WPARAM,
};
use ::windows::Win32::Graphics::Gdi::SC_SCREENSAVE;
use ::windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use ::windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole,
    eRender,
};
use ::windows::Win32::Security::{
    AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
    SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_PRIVILEGES_ATTRIBUTES,
    TOKEN_QUERY,
};
use ::windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, GetDriveTypeW,
    GetLogicalDriveStringsW, OPEN_EXISTING,
};
use ::windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
use ::windows::Win32::System::IO::DeviceIoControl;
use ::windows::Win32::System::Ioctl::{
    FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, GUID_DEVINTERFACE_DISK,
    IOCTL_STORAGE_GET_DEVICE_NUMBER, STORAGE_DEVICE_NUMBER,
};
use ::windows::Win32::System::Power::{
    GetPwrCapabilities, SYSTEM_POWER_CAPABILITIES, SetSuspendState,
};
use ::windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD, RRF_RT_REG_DWORD, RegCloseKey, RegGetValueW,
    RegOpenKeyExW, RegSetValueExW,
};
use ::windows::Win32::System::Shutdown::{
    EWX_FORCE, EWX_LOGOFF, EWX_POWEROFF, EWX_REBOOT, ExitWindowsEx, LockWorkStation,
    SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_APPLICATION, SHTDN_REASON_MINOR_MAINTENANCE,
    SHUTDOWN_REASON,
};
use ::windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use ::windows::Win32::System::Variant::{VARIANT, VT_I4};
use ::windows::Win32::System::WindowsProgramming::DRIVE_REMOVABLE;
use ::windows::Win32::UI::Shell::{
    FOLDERID_RecycleBinFolder, IShellDispatch4, IShellWindows, IWebBrowser2, KF_FLAG_DEFAULT,
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, SHERB_NOCONFIRMATION,
    SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW, SHGetKnownFolderPath, SHQUERYRBINFO,
    SHQueryRecycleBinW, ShellExecuteExW,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, HWND_BROADCAST, SC_MONITORPOWER, SMTO_ABORTIFHUNG, SPI_GETSCREENSAVEACTIVE,
    SW_SHOWNORMAL, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SendMessageTimeoutW, SystemParametersInfoW,
    WM_SETTINGCHANGE, WM_SYSCOMMAND,
};
use ::windows::core::{BOOL, GUID, HSTRING, Interface, PCWSTR};

use super::{
    Appearance, BluetoothRadio, Capabilities, Drive, HdrDisplay, Microphone, PowerRequest,
    RecycleBin, SystemCommands, Volume,
};
use crate::threads::windows::WindowClass;
use crate::util::wide;
use crate::windows_shell::Com;

/// The class of the hidden windows the commands' messages are sent
/// through: top-level windows of Pane's own, never shown, whose messages
/// Windows' own procedure handles.
static COMMANDS: WindowClass = WindowClass::new("PaneSystemCommands", procedure);

/// The procedure of the commands' windows: Windows' own, which turns the
/// monitor-power and screen-saver commands into what they do.
extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: the arguments are those this procedure was called with.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// How long a command's message is given to be handled, so that a hung
/// path cannot hold the calling thread up.
const MESSAGE_TIMEOUT_MS: u32 = 1_000;

/// How long a setting-change broadcast is given, so that a hung
/// application cannot hold the calling thread up.
const SETTING_TIMEOUT_MS: u32 = 2_000;

/// The shell's own COM object (`Shell.Application`), through which the
/// desktop is toggled and the file manager's windows listed.
const CLSID_SHELL: GUID = GUID::from_u128(0x13709620_c279_11ce_a49e_444553540000);

/// The personalization key under the current user, whose values say the
/// light or the dark mode of applications and of the system.
const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

/// The personalization value that says whether applications use the
/// light mode.
const APPS_USE_LIGHT_THEME: &str = "AppsUseLightTheme";

/// The personalization value that says whether the system does.
const SYSTEM_USES_LIGHT_THEME: &str = "SystemUsesLightTheme";

/// The setting a change of the appearance names, which the broadcast
/// carries so that applications re-read their colors.
const COLOR_SETTING: &str = "ImmersiveColorSet";

/// The key under the current user that holds the file manager's own
/// settings, among them whether it shows hidden files.
const EXPLORER_ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";

/// The value in it that says whether hidden files are shown.
const HIDDEN: &str = "Hidden";

/// The value that says hidden files are shown.
const HIDDEN_SHOWN: u32 = 2;

/// The value that says they are not.
const HIDDEN_NOT_SHOWN: u32 = 1;

/// The bit of the advanced colour info that says the display can show
/// HDR, the first of its bits.
const ADVANCED_COLOR_SUPPORTED: u32 = 1;

/// The bit that says the display is showing advanced colour now, the
/// second of them.
const ADVANCED_COLOR_ENABLED: u32 = 1 << 1;

/// Why a display the toggle was given cannot be reached: it has gone
/// since it was listed.
const DISPLAY_GONE: &str = "The display is gone";

/// Why a Bluetooth radio the toggle was given cannot be reached: it has
/// gone since it was listed.
const RADIO_GONE: &str = "The Bluetooth radio is gone";

/// What the monitor-power command turns the displays to: off.
const MONITOR_OFF: isize = 2;

/// The session and power commands on Windows.
pub(super) struct WindowsSystemCommands;

impl SystemCommands for WindowsSystemCommands {
    fn capabilities(&self) -> Result<Capabilities, String> {
        let mut power = SYSTEM_POWER_CAPABILITIES::default();
        // SAFETY: `power` is a valid, writable struct for the call.
        if !unsafe { GetPwrCapabilities(&mut power) } {
            return Err("Windows would not say what this computer can do".into());
        }
        Ok(Capabilities {
            modern_standby: power.AoAc,
            hibernation_file: power.HiberFilePresent,
        })
    }

    fn lock_screen(&self) -> Result<(), String> {
        // SAFETY: no arguments.
        unsafe { LockWorkStation() }
            .map_err(|error| format!("Windows did not lock the screen: {}", error.message()))
    }

    fn power_off(&self, request: PowerRequest, force: bool) -> Result<(), String> {
        let (flags, doing) = match request {
            PowerRequest::LogOut => (EWX_LOGOFF, "log the user out"),
            PowerRequest::Restart => (EWX_REBOOT, "restart the computer"),
            PowerRequest::ShutDown => (EWX_POWEROFF, "power the computer off"),
        };
        let flags = if force { flags | EWX_FORCE } else { flags };
        // The privilege the call needs, enabled for it alone: put back as
        // it was once the call has been made.
        let _privilege = ShutdownPrivilege::enable()?;
        // SAFETY: plain values; the flags are as decided above.
        unsafe { ExitWindowsEx(flags, PLANNED) }
            .map_err(|error| format!("Windows did not {doing}: {}", error.message()))
    }

    fn suspend(&self, hibernate: bool) -> Result<(), String> {
        // SAFETY: plain flags: not forced, so Windows asks applications,
        // and wake events are left on.
        let suspended = unsafe { SetSuspendState(hibernate, false, false) };
        if !suspended {
            return Err(if hibernate {
                "Windows did not hibernate the computer".into()
            } else {
                "Windows did not suspend the computer".into()
            });
        }
        Ok(())
    }

    fn displays_off(&self) -> Result<(), String> {
        // SC_MONITORPOWER, the monitor-power hint, which Windows' own
        // procedure carries to the display driver.
        command(SC_MONITORPOWER, MONITOR_OFF, "turn the displays off")
    }

    fn screen_saver(&self) -> Result<(), String> {
        let mut active = BOOL::default();
        // SAFETY: `active` is a valid, writable BOOL for the call; the
        // action reads only it.
        unsafe {
            SystemParametersInfoW(
                SPI_GETSCREENSAVEACTIVE,
                0,
                Some(&mut active as *mut _ as _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS::default(),
            )
        }
        .map_err(|error| format!("Windows did not say whether a screen saver is set: {error}"))?;
        if !active.as_bool() {
            return Err(
                "No screen saver is set: choose one in Windows' lock screen settings".into(),
            );
        }
        // SC_SCREENSAVE, the screen-saver command, which Windows' own
        // procedure runs the chosen screen saver for.
        command(SC_SCREENSAVE, 0, "start the screen saver")
    }

    fn volume(&self) -> Result<Volume, String> {
        let _com = Com::new()?;
        let control = default_output()?;
        // SAFETY: plain reads of the endpoint the enumerator answered: its
        // level as a scalar, and its mute.
        let scalar = unsafe { control.GetMasterVolumeLevelScalar() }
            .map_err(|error| format!("Windows did not say the volume: {}", error.message()))?;
        // SAFETY: as above, of its mute.
        let muted = unsafe { control.GetMute() }
            .map_err(|error| format!("Windows did not say the volume: {}", error.message()))?;
        Ok(Volume {
            level: (scalar * 100.0).round() as u8,
            muted: muted.as_bool(),
        })
    }

    fn set_volume(&self, volume: Volume) -> Result<Volume, String> {
        let _com = Com::new()?;
        let control = default_output()?;
        // SAFETY: plain values: the level as a scalar of 100 and the mute,
        // with no event context naming a caller.
        unsafe {
            control
                .SetMasterVolumeLevelScalar(volume.level as f32 / 100.0, std::ptr::null())
                .map_err(|error| format!("Windows did not set the volume: {}", error.message()))?;
            control
                .SetMute(volume.muted, std::ptr::null())
                .map_err(|error| format!("Windows did not set the volume: {}", error.message()))?;
        }
        Ok(volume)
    }

    fn microphones(&self) -> Result<Vec<Microphone>, String> {
        let _com = Com::new()?;
        let enumerator = audio_enumerator()?;
        let not_listed = |error: ::windows::core::Error| {
            format!("Windows did not list the microphones: {}", error.message())
        };
        // SAFETY: the capture endpoints that are active, which are the
        // microphones there are.
        let devices = unsafe { enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) }
            .map_err(not_listed)?;
        // SAFETY: the collection's own count.
        let count = unsafe { devices.GetCount() }.map_err(not_listed)?;
        let mut microphones = Vec::with_capacity(count as usize);
        for index in 0..count {
            // SAFETY: an index within the count the collection answered.
            let device = unsafe { devices.Item(index) }.map_err(not_listed)?;
            // SAFETY: the id the enumerator itself answered for the device,
            // copied out of the string it points to, which is freed here.
            let id = unsafe { device.GetId() }.map_err(not_listed)?;
            // SAFETY: the string the enumerator gave is valid UTF-16, read
            // before the handle that owns it is freed below.
            let text = unsafe { id.to_string() }.ok();
            // SAFETY: the handle `GetId` answered, whose string was copied.
            unsafe { CoTaskMemFree(Some(id.0 as *const _)) };
            let Some(id) = text.filter(|id| !id.is_empty()) else {
                return Err("Windows did not name a microphone".into());
            };
            let control = endpoint_volume(&device)?;
            // SAFETY: a plain read of the endpoint's mute.
            let muted = unsafe { control.GetMute() }.map_err(not_listed)?;
            microphones.push(Microphone {
                id,
                muted: muted.as_bool(),
            });
        }
        Ok(microphones)
    }

    fn set_microphone_mute(&self, id: &str, muted: bool) -> Result<(), String> {
        let _com = Com::new()?;
        let enumerator = audio_enumerator()?;
        let wide_id = wide(id);
        // SAFETY: an id the enumerator itself answered, as `GetId` gave it.
        let device =
            unsafe { enumerator.GetDevice(PCWSTR(wide_id.as_ptr())) }.map_err(|error| {
                format!("Windows did not reach the microphone: {}", error.message())
            })?;
        let control = endpoint_volume(&device)?;
        // SAFETY: a plain value, with no event context naming a caller.
        unsafe { control.SetMute(muted, std::ptr::null()) }.map_err(|error| {
            format!(
                "Windows did not set the microphone's mute: {}",
                error.message()
            )
        })
    }

    fn recycle_bin(&self) -> Result<RecycleBin, String> {
        let mut query = SHQUERYRBINFO {
            cbSize: size_of::<SHQUERYRBINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `query` is a valid, writable struct of the size its
        // `cbSize` says, and the root asked about is every drive's bin.
        unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut query) }.map_err(|error| {
            format!(
                "Windows did not say what the Recycle Bin holds: {}",
                error.message()
            )
        })?;
        Ok(RecycleBin {
            items: query.i64NumItems.max(0) as u64,
            size: query.i64Size.max(0) as u64,
        })
    }

    fn open_recycle_bin(&self) -> Result<(), String> {
        let _com = Com::new()?;
        let wide_folder = wide(recycle_bin_folder()?);
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            // Wait until the shell has opened it (this thread ends
            // next), and report a failure here instead of in a dialog.
            fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpFile: PCWSTR(wide_folder.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        // SAFETY: `info` is initialized with its size, and its file is
        // NUL-terminated and outlives the call.
        unsafe { ShellExecuteExW(&mut info) }
            .map_err(|error| format!("Windows did not open the Recycle Bin: {}", error.message()))
    }

    fn empty_recycle_bin(&self) -> Result<(), String> {
        // SAFETY: plain flags: no confirmation of its own (the command
        // asked first), no progress window, no sound; the root is every
        // drive's bin.
        unsafe {
            SHEmptyRecycleBinW(
                None,
                PCWSTR::null(),
                SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
            )
        }
        .map_err(|error| format!("Windows did not empty the Recycle Bin: {}", error.message()))
    }

    fn appearance(&self) -> Result<Appearance, String> {
        // The applications' value says the mode; a value that is not
        // there is the light mode, as Windows treats it.
        let light = user_dword(PERSONALIZE, APPS_USE_LIGHT_THEME)?.unwrap_or(1) != 0;
        Ok(if light {
            Appearance::Light
        } else {
            Appearance::Dark
        })
    }

    fn set_appearance(&self, appearance: Appearance) -> Result<Appearance, String> {
        let light = matches!(appearance, Appearance::Light) as u32;
        write_user_dword(PERSONALIZE, APPS_USE_LIGHT_THEME, light)?;
        write_user_dword(PERSONALIZE, SYSTEM_USES_LIGHT_THEME, light)?;
        broadcast(COLOR_SETTING)?;
        Ok(appearance)
    }

    fn hdr_displays(&self) -> Result<Vec<HdrDisplay>, String> {
        let mut displays = Vec::new();
        for path in active_paths()? {
            let target = path.targetInfo;
            let mut info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default();
            info.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
            info.header.size = size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32;
            info.header.adapterId = target.adapterId;
            info.header.id = target.id;
            // SAFETY: `info` is initialized with its size and the target
            // the path named; the request reads it and fills the rest.
            // A display it cannot answer is not one it says can show
            // HDR, so it is skipped, not a failure.
            if unsafe { DisplayConfigGetDeviceInfo(&mut info.header) } != ERROR_SUCCESS.0 as i32 {
                continue;
            }
            // SAFETY: reading the union's value, which the call filled
            // with the flags the info is.
            let flags = unsafe { info.Anonymous.value };
            if flags & ADVANCED_COLOR_SUPPORTED == 0 {
                continue;
            }
            displays.push(HdrDisplay {
                id: display_id(target.adapterId, target.id),
                hdr: flags & ADVANCED_COLOR_ENABLED != 0,
            });
        }
        Ok(displays)
    }

    fn set_hdr(&self, id: &str, hdr: bool) -> Result<(), String> {
        let (adapter, target) = display(id)?;
        let mut state = DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE::default();
        state.header.r#type = DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE;
        state.header.size = size_of::<DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE>() as u32;
        state.header.adapterId = adapter;
        state.header.id = target;
        // The enabled bit alone, as the request's flags are.
        state.Anonymous.value = if hdr { ADVANCED_COLOR_ENABLED } else { 0 };
        // SAFETY: `state` is initialized with its size and the target
        // the id named; the request sets the state it says.
        let answered = unsafe { DisplayConfigSetDeviceInfo(&state.header) };
        if answered != ERROR_SUCCESS.0 as i32 {
            return Err("Windows did not change the display's HDR".into());
        }
        Ok(())
    }

    fn show_desktop(&self) -> Result<(), String> {
        let _com = Com::new()?;
        let shell: IShellDispatch4 =
            // SAFETY: plain COM creation: the class is the shell's own
            // object, and the context any in-process server.
            unsafe { CoCreateInstance(&CLSID_SHELL, None, CLSCTX_ALL) }.map_err(|error| {
                format!("Windows did not reach the shell: {}", error.message())
            })?;
        // SAFETY: a plain call on the shell's own object.
        unsafe { shell.ToggleDesktop() }
            .map_err(|error| format!("Windows did not show the desktop: {}", error.message()))
    }

    fn hidden_files(&self) -> Result<bool, String> {
        // A value that is not there is the hidden-files-hidden one, as
        // Windows treats it.
        Ok(user_dword(EXPLORER_ADVANCED, HIDDEN)?.unwrap_or(HIDDEN_NOT_SHOWN) == HIDDEN_SHOWN)
    }

    fn set_hidden_files(&self, shown: bool) -> Result<(), String> {
        let value = if shown {
            HIDDEN_SHOWN
        } else {
            HIDDEN_NOT_SHOWN
        };
        write_user_dword(EXPLORER_ADVANCED, HIDDEN, value)?;
        refresh_explorer()
    }

    fn removable_drives(&self) -> Result<Vec<Drive>, String> {
        // SAFETY: the size-only call, so that the buffer can be as long
        // as the list is.
        let needed = unsafe { GetLogicalDriveStringsW(None) };
        if needed == 0 {
            return Err("Windows did not say which drives there are".into());
        }
        let mut roots = vec![0u16; needed as usize + 1];
        // SAFETY: `roots` is as long as the size-only call asked for, and
        // holds the list's double-NUL-terminated strings.
        let filled = unsafe { GetLogicalDriveStringsW(Some(&mut roots)) };
        if filled == 0 || filled > needed {
            return Err("Windows did not say which drives there are".into());
        }
        let mut drives = Vec::new();
        for root in roots[..filled as usize].split(|&unit| unit == 0) {
            let Some(path) = String::from_utf16(root).ok() else {
                continue;
            };
            if path.len() < 2 {
                continue;
            }
            let id = path[..2].to_owned();
            let wide_root = wide(path);
            // SAFETY: the root path the list holds is NUL-terminated in
            // the buffer and valid for the call.
            if unsafe { GetDriveTypeW(PCWSTR(wide_root.as_ptr())) } == DRIVE_REMOVABLE {
                drives.push(Drive { id });
            }
        }
        Ok(drives)
    }

    fn eject_drive(&self, id: &str) -> Result<(), String> {
        eject(id)
    }

    fn bluetooth_radios(&self) -> Result<Vec<BluetoothRadio>, String> {
        radios()
    }

    fn set_bluetooth(&self, id: &str, on: bool) -> Result<(), String> {
        let state = if on {
            BLUETOOTH_SERVICE_ENABLE
        } else {
            BLUETOOTH_SERVICE_DISABLE
        };
        set_service(id, state)
    }
}

/// Core Audio's device enumerator. The calling thread must hold [`Com`]
/// for as long as it uses the interfaces the enumerator answers.
fn audio_enumerator() -> Result<IMMDeviceEnumerator, String> {
    // SAFETY: plain COM creation: the class is the device enumerator, and
    // the context any in-process server.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(|error| {
        format!(
            "Windows did not reach the audio devices: {}",
            error.message()
        )
    })
}

/// The default output device's endpoint volume, which the volume commands
/// read and set. The calling thread must hold [`Com`].
fn default_output() -> Result<IAudioEndpointVolume, String> {
    let enumerator = audio_enumerator()?;
    // SAFETY: the default output endpoint, of the kind and the role
    // Windows plays console sound through.
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
        .map_err(|error| format!("Windows did not say the volume: {}", error.message()))?;
    endpoint_volume(&device)
}

/// The endpoint volume of `device`, which its volume and its mute are
/// read and set through. The calling thread must hold [`Com`].
fn endpoint_volume(device: &IMMDevice) -> Result<IAudioEndpointVolume, String> {
    // SAFETY: plain COM activation of the device's own endpoint volume.
    unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(|error| {
        format!(
            "Windows did not reach an audio endpoint: {}",
            error.message()
        )
    })
}

/// The shutdown reason every session-ending call carries: a planned one,
/// an application's maintenance, as the user asked for by name.
const PLANNED: SHUTDOWN_REASON = SHUTDOWN_REASON(
    SHTDN_REASON_FLAG_PLANNED.0
        | SHTDN_REASON_MAJOR_APPLICATION.0
        | SHTDN_REASON_MINOR_MAINTENANCE.0,
);

/// Sends a `WM_SYSCOMMAND` (`command`, with `value`) to a hidden window
/// of Pane's own and answers whether it was handled — never a broadcast
/// that a hung application's window could block, and never a wait without
/// an end. The window is made for this call and destroyed with it, and
/// Windows' own procedure ([`procedure`]) does what the command says.
fn command(code: u32, value: isize, doing: &str) -> Result<(), String> {
    let window = COMMANDS.hidden_window()?;
    let mut result = 0usize;
    // SAFETY: the window is this thread's own; `result` is writable, and
    // the values plain.
    let sent = unsafe {
        SendMessageTimeoutW(
            window.handle(),
            WM_SYSCOMMAND,
            WPARAM(code as usize),
            LPARAM(value),
            SMTO_ABORTIFHUNG,
            MESSAGE_TIMEOUT_MS,
            Some(&mut result),
        )
    };
    if sent.0 == 0 {
        return Err(format!(
            "Windows did not {doing}: the message was not handled in time"
        ));
    }
    Ok(())
}

/// The shutdown privilege (`SeShutdownPrivilege`), which ending the
/// session through `ExitWindowsEx` needs: enabled while held and put back
/// as it was when dropped.
struct ShutdownPrivilege {
    token: HANDLE,
    /// What the privilege said before, put back as it was on drop.
    was: TOKEN_PRIVILEGES,
}

impl ShutdownPrivilege {
    /// Enables the shutdown privilege of this process, answering the
    /// holder, which restores it when dropped.
    fn enable() -> Result<ShutdownPrivilege, String> {
        // SAFETY: no arguments; the handle is this process's own.
        let process = unsafe { GetCurrentProcess() };
        let mut token = HANDLE::default();
        // SAFETY: `token` is a valid, writable handle for the call; the
        // process handle is this process's own.
        unsafe { OpenProcessToken(process, TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token) }
            .map_err(|error| {
                format!(
                    "Pane could not open its own access token: {}",
                    error.message()
                )
            })?;
        let enabled = match privileges(SE_PRIVILEGE_ENABLED) {
            Ok(enabled) => enabled,
            Err(problem) => {
                close(token);
                return Err(problem);
            }
        };
        let mut was = TOKEN_PRIVILEGES::default();
        let mut length = 0u32;
        // SAFETY: `enabled` is fully initialized and outlives the call;
        // `was` is a valid, writable buffer of the length given.
        let adjusted = unsafe {
            AdjustTokenPrivileges(
                token,
                false,
                Some(&enabled),
                size_of::<TOKEN_PRIVILEGES>() as u32,
                Some(&mut was),
                Some(&mut length),
            )
        };
        if let Err(error) = adjusted {
            close(token);
            return Err(format!(
                "Pane could not enable the shutdown privilege: {}",
                error.message()
            ));
        }
        Ok(ShutdownPrivilege { token, was })
    }
}

impl Drop for ShutdownPrivilege {
    fn drop(&mut self) {
        // SAFETY: `self.was` is fully initialized and outlives the call.
        let _ = unsafe {
            AdjustTokenPrivileges(
                self.token,
                false,
                Some(&self.was),
                size_of::<TOKEN_PRIVILEGES>() as u32,
                None,
                None,
            )
        };
        close(self.token);
    }
}

/// The state of the `SeShutdownPrivilege` privilege, as `attributes` say
/// it, for [`AdjustTokenPrivileges`].
fn privileges(attributes: TOKEN_PRIVILEGES_ATTRIBUTES) -> Result<TOKEN_PRIVILEGES, String> {
    let mut id = LUID::default();
    // SAFETY: `id` is a valid, writable LUID for the call.
    let found = unsafe { LookupPrivilegeValueW(None, SE_SHUTDOWN_NAME, &mut id) };
    found.map_err(|error| {
        format!(
            "Pane could not find the shutdown privilege: {}",
            error.message()
        )
    })?;
    Ok(TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: id,
            Attributes: attributes,
        }],
    })
}

/// Closes `token`; whether it closed is nothing to report here.
fn close(token: HANDLE) {
    // SAFETY: a handle `OpenProcessToken` answered that its holder will
    // not use again.
    let _ = unsafe { CloseHandle(token) };
}

/// The Recycle Bin's folder, as the shell knows it: the Recycle Bin's
/// known folder, whose string Windows answers and the caller copies
/// before its handle is freed. The calling thread must hold [`Com`].
fn recycle_bin_folder() -> Result<String, String> {
    // SAFETY: the Recycle Bin's known folder, with the default flag and
    // no other token than the caller's own.
    let path = unsafe { SHGetKnownFolderPath(&FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, None) }
        .map_err(|error| {
        format!("Windows did not find the Recycle Bin: {}", error.message())
    })?;
    // SAFETY: the string Windows answered is valid UTF-16, read before
    // the handle that owns it is freed below.
    let folder = unsafe { PCWSTR(path.as_ptr()).to_string() };
    // SAFETY: the handle `SHGetKnownFolderPath` answered, whose string
    // was copied above.
    unsafe { CoTaskMemFree(Some(path.0 as *const _)) };
    folder.map_err(|error| format!("Windows did not name the Recycle Bin: {error}"))
}

/// The DWORD `value` of `subkey` under the current user, as
/// `RegGetValueW` reads it: `None` when the value is not there, which
/// Windows treats as the setting's default.
fn user_dword(subkey: &str, value: &str) -> Result<Option<u32>, String> {
    let (wide_subkey, wide_value) = (HSTRING::from(subkey), HSTRING::from(value));
    let mut number: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: the strings are NUL-terminated and outlive the call;
    // `number` is writable for the DWORD asked for, and `size` says its
    // bytes and answers them.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &wide_subkey,
            &wide_value,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut number as *mut _ as _),
            Some(&mut size),
        )
    };
    if read == ERROR_SUCCESS {
        return Ok(Some(number));
    }
    if read == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    Err(format!(
        "Windows did not read its {value} setting (error {})",
        read.0
    ))
}

/// Writes the DWORD `value` of `subkey` under the current user to
/// `number`, as `RegSetValueExW` writes it: the key is opened for the
/// call alone and closed with it.
fn write_user_dword(subkey: &str, value: &str, number: u32) -> Result<(), String> {
    let (wide_subkey, wide_value) = (HSTRING::from(subkey), HSTRING::from(value));
    let mut key = HKEY::default();
    // SAFETY: the strings are NUL-terminated and outlive the call, and
    // `key` is a writable handle for the one it answers.
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &wide_subkey,
            None,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if opened != ERROR_SUCCESS {
        return Err(format!(
            "Windows did not reach its {value} setting (error {})",
            opened.0
        ));
    }
    let bytes = number.to_ne_bytes();
    // SAFETY: `key` is the handle `RegOpenKeyExW` answered, and `bytes`
    // holds the four a DWORD is.
    let written = unsafe { RegSetValueExW(key, &wide_value, None, REG_DWORD, Some(&bytes)) };
    // SAFETY: the handle `RegOpenKeyExW` answered, which this call ends.
    let _ = unsafe { RegCloseKey(key) };
    if written != ERROR_SUCCESS {
        return Err(format!(
            "Windows did not write its {value} setting (error {})",
            written.0
        ));
    }
    Ok(())
}

/// Tells every application that a setting changed (`name`): the
/// `WM_SETTINGCHANGE` broadcast, given the abort-if-hung flag and a
/// timeout, so that a hung application cannot block it.
fn broadcast(name: &str) -> Result<(), String> {
    let wide_name = wide(name);
    let mut result = 0usize;
    // SAFETY: the string is NUL-terminated and outlives the call, and
    // `result` is writable for the answer.
    let sent = unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(wide_name.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            SETTING_TIMEOUT_MS,
            Some(&mut result),
        )
    };
    if sent.0 == 0 {
        return Err("Windows did not take the setting change to its applications in time".into());
    }
    Ok(())
}

/// Refreshes every open File Explorer window, so that hidden files that
/// just came to show or hide appear in them: the shell's own windows
/// collection, each window refreshed as its own view is. A window that
/// fails is skipped — the setting is saved, whatever one window does.
/// The calling thread must hold [`Com`].
fn refresh_explorer() -> Result<(), String> {
    let _com = Com::new()?;
    // SAFETY: plain COM creation: the class is the shell's own object,
    // and the context any in-process server.
    let shell: IShellDispatch4 = unsafe { CoCreateInstance(&CLSID_SHELL, None, CLSCTX_ALL) }
        .map_err(|error| format!("Windows did not reach the shell: {}", error.message()))?;
    // SAFETY: a plain call on the shell's own object, which answers the
    // collection of the windows it has open.
    let windows = unsafe { shell.Windows() }
        .and_then(|windows| windows.cast::<IShellWindows>())
        .map_err(|error| format!("Windows did not list its windows: {}", error.message()))?;
    // SAFETY: the collection's own count.
    let count = unsafe { windows.Count() }
        .map_err(|error| format!("Windows did not list its windows: {}", error.message()))?;
    for position in 0..count {
        // SAFETY: the collection's own item at the position, and the
        // interface its windows answer.
        let window =
            unsafe { windows.Item(&at(position)) }.and_then(|window| window.cast::<IWebBrowser2>());
        // SAFETY: a plain call on the window, which refreshes its own
        // view; whether it did is nothing to report here.
        if let Ok(window) = window {
            let _ = unsafe { window.Refresh() };
        }
    }
    Ok(())
}

/// `number` as the index the shell's windows collection takes: a 32-bit
/// integer variant, which nothing but the call reads.
fn at(number: i32) -> VARIANT {
    let mut index = VARIANT::default();
    // SAFETY: writing the fields of the plain value a 32-bit integer
    // variant is; the index was zeroed first, so the fields it leaves
    // alone are not read.
    unsafe {
        (*index.Anonymous.Anonymous).vt = VT_I4;
        (*index.Anonymous.Anonymous).Anonymous.lVal = number;
    }
    index
}

/// The paths of the displays that are on now, as the display
/// configuration holds them: `GetDisplayConfigBufferSizes` for how many
/// there are, then `QueryDisplayConfig` over the active ones.
fn active_paths() -> Result<Vec<DISPLAYCONFIG_PATH_INFO>, String> {
    let could_not = "Windows would not say what the displays are doing";
    let mut paths = 0u32;
    let mut modes = 0u32;
    // SAFETY: the size call: `paths` and `modes` are writable for the
    // counts it answers.
    if unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths, &mut modes) }
        != ERROR_SUCCESS
    {
        return Err(could_not.into());
    }
    let mut path_array = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
    let mut mode_array = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
    // SAFETY: both arrays are as long as the size call said, and the
    // counts are writable again for how many were filled.
    let answered = unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut paths,
            path_array.as_mut_ptr(),
            &mut modes,
            mode_array.as_mut_ptr(),
            None,
        )
    };
    if answered != ERROR_SUCCESS {
        return Err(could_not.into());
    }
    path_array.truncate(paths as usize);
    Ok(path_array)
}

/// The id of the display the configuration's path names: its adapter and
/// its target, which `display` names back.
fn display_id(adapter: LUID, target: u32) -> String {
    let adapter = (adapter.HighPart as u64) << 32 | adapter.LowPart as u64;
    format!("{adapter}:{target}")
}

/// The adapter and the target the display's `id` names, as `display_id`
/// made it; an id that is not one, or a display that has gone since it
/// was listed, answers why.
fn display(id: &str) -> Result<(LUID, u32), String> {
    let (adapter, target) = id.split_once(':').ok_or(DISPLAY_GONE)?;
    let adapter: u64 = adapter.parse().map_err(|_| DISPLAY_GONE)?;
    let target: u32 = target.parse().map_err(|_| DISPLAY_GONE)?;
    Ok((adapter_of(adapter), target))
}

/// The LUID the number the display's id carries is.
fn adapter_of(number: u64) -> LUID {
    LUID {
        HighPart: (number >> 32) as u32 as i32,
        LowPart: number as u32,
    }
}

/// Ejects the drive `id`: its volume is locked and dismounted, then the
/// disk behind it ejected, as Windows' own safe removal does. A drive in
/// use answers why it was not.
fn eject(id: &str) -> Result<(), String> {
    let volume = open_volume(id)?;
    let answer = locked_and_ejected(volume);
    // SAFETY: the handle `open_volume` answered, which this call ends.
    let _ = unsafe { CloseHandle(volume) };
    answer
}

/// Locks and dismounts `volume`, then ejects the disk behind it through
/// the configuration manager: the drive's own device is what leaves, not
/// only its volume.
fn locked_and_ejected(volume: HANDLE) -> Result<(), String> {
    control(volume, FSCTL_LOCK_VOLUME, "lock the drive")?;
    control(volume, FSCTL_DISMOUNT_VOLUME, "dismount the drive")?;
    let number = device_number(volume, "the drive")?;
    let devinst = disk_device(number)?;
    let mut veto = PNP_VETO_TYPE::default();
    // SAFETY: the device instance the disk class answered; the veto type
    // is writable for the answer the ejection leaves when it is
    // refused.
    let answered = unsafe { CM_Request_Device_EjectW(devinst, Some(&mut veto), None, 0) };
    if answered != CR_SUCCESS {
        return Err(format!("Windows would not eject it: {}", veto_why(veto)));
    }
    Ok(())
}

/// Opens the volume of the drive `id` (`\\.\E:`), as the lock, the
/// dismount and the device number ask.
fn open_volume(id: &str) -> Result<HANDLE, String> {
    let volume = wide(format!(r"\\.\{id}"));
    // SAFETY: the volume of the drive, opened for reading what the
    // ioctls ask, shared as the drive's files are, and existing as it
    // does.
    unsafe {
        CreateFileW(
            PCWSTR(volume.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES::default(),
            None,
        )
    }
    .map_err(|error| format!("Windows did not reach the drive: {}", error.message()))
}

/// Opens the disk its `path` (a device interface's path) names, as the
/// query the device number asks.
fn open_disk(path: &str) -> Result<HANDLE, String> {
    let wide_path = wide(path);
    // SAFETY: the disk the interface path names, opened for the query
    // alone (any access), shared as it is, and existing as it does.
    unsafe {
        CreateFileW(
            PCWSTR(wide_path.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES::default(),
            None,
        )
    }
    .map_err(|error| format!("Windows did not reach the disk: {}", error.message()))
}

/// Asks `volume` for `control` — the lock or the dismount — answering
/// `doing` when Windows did not take it.
fn control(volume: HANDLE, code: u32, doing: &str) -> Result<(), String> {
    let mut returned = 0u32;
    // SAFETY: plain values: the control code, no input, no output.
    let asked =
        unsafe { DeviceIoControl(volume, code, None, 0, None, 0, Some(&mut returned), None) };
    asked.map_err(|error| format!("Windows did not {doing}: {}", error.message()))
}

/// The device number of the volume `handle` is on, which the disk behind
/// it shares: the ejection finds that disk by it. `whose` names what
/// failed in the message.
fn device_number(handle: HANDLE, whose: &str) -> Result<STORAGE_DEVICE_NUMBER, String> {
    let mut number = STORAGE_DEVICE_NUMBER::default();
    let mut returned = 0u32;
    // SAFETY: plain values: the query and the struct it fills, which is
    // writable and outlives the call.
    let asked = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_GET_DEVICE_NUMBER,
            None,
            0,
            Some(&mut number as *mut _ as _),
            size_of::<STORAGE_DEVICE_NUMBER>() as u32,
            Some(&mut returned),
            None,
        )
    };
    asked.map_err(|error| {
        format!(
            "Windows did not say the disk behind {whose}: {}",
            error.message()
        )
    })?;
    Ok(number)
}

/// The device instance of the disk whose device number `number` is, as
/// the disk-drive class lists it: the device the ejection removes.
fn disk_device(number: STORAGE_DEVICE_NUMBER) -> Result<u32, String> {
    // SAFETY: plain class and flags: the disk drives present now.
    let disks = unsafe {
        SetupDiGetClassDevsW(
            Some(&GUID_DEVCLASS_DISKDRIVE),
            PCWSTR::null(),
            None,
            DIGCF_PRESENT,
        )
    }
    .map_err(|error| format!("Windows did not list the disks: {}", error.message()))?;
    let answer = disk_in(&disks, number);
    // SAFETY: the set `SetupDiGetClassDevsW` answered, which this call
    // ends.
    let _ = unsafe { SetupDiDestroyDeviceInfoList(disks) };
    answer
}

/// The device instance of the disk in `disks` whose device number is
/// `number`, which the volume's own number says; none matching answers
/// why.
fn disk_in(disks: &HDEVINFO, number: STORAGE_DEVICE_NUMBER) -> Result<u32, String> {
    let mut member = 0;
    loop {
        let mut device = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        // SAFETY: `device` is initialized with its size; the member walks
        // the set until the call answers that there are no more.
        if unsafe { SetupDiEnumDeviceInfo(*disks, member, &mut device) }.is_err() {
            break;
        }
        member += 1;
        if let Some(devinst) = disk_if_matching(disks, &device, number) {
            return Ok(devinst);
        }
    }
    Err("Windows did not find the disk behind the drive".into())
}

/// The device instance of `device` if the disk it is has the device
/// number `number`, which the volume's own number says; `None` when it
/// is another disk, or one that cannot be asked.
fn disk_if_matching(
    disks: &HDEVINFO,
    device: &SP_DEVINFO_DATA,
    number: STORAGE_DEVICE_NUMBER,
) -> Option<u32> {
    let mut interface = SP_DEVICE_INTERFACE_DATA {
        cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
        ..Default::default()
    };
    // SAFETY: `interface` is initialized with its size; the disk
    // interface of the device the set named is asked for.
    let asked = unsafe {
        SetupDiEnumDeviceInterfaces(
            *disks,
            Some(device),
            &GUID_DEVINTERFACE_DISK,
            0,
            &mut interface,
        )
    };
    if asked.is_err() {
        return None;
    }
    let path = interface_path(disks, &mut interface, device)?;
    let disk = open_disk(&path).ok()?;
    let its = device_number(disk, "it").ok()?;
    // SAFETY: the handle `open_disk` answered, which this call ends.
    let _ = unsafe { CloseHandle(disk) };
    (its.DeviceNumber == number.DeviceNumber && its.DeviceType == number.DeviceType)
        .then_some(device.DevInst)
}

/// The path of the interface `interface` of `device`, which opening
/// reaches the disk; `None` when Windows would not say it.
fn interface_path(
    disks: &HDEVINFO,
    interface: &mut SP_DEVICE_INTERFACE_DATA,
    device: &SP_DEVINFO_DATA,
) -> Option<String> {
    let mut needed = 0u32;
    // SAFETY: the size-only call: no detail buffer to fill, and `needed`
    // is writable for the size it answers.
    let _ = unsafe {
        SetupDiGetDeviceInterfaceDetailW(
            *disks,
            interface,
            None,
            0,
            Some(&mut needed),
            Some(device as *const _ as *mut _),
        )
    };
    if needed == 0 {
        return None;
    }
    let length = needed as usize;
    let head = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
    if length < head {
        return None;
    }
    // A buffer of the length asked for, aligned as the detail struct
    // needs its `cbSize` to be; the path follows the struct in it.
    let mut buffer = vec![0u64; length.div_ceil(8)];
    let detail = buffer
        .as_mut_ptr()
        .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
    // SAFETY: the buffer is as long as the size-only call asked for and
    // aligned for the struct; the `cbSize` it starts with is the size
    // the call takes; `device` is the one the interface belongs to.
    let read = unsafe {
        (*detail).cbSize = head as u32;
        SetupDiGetDeviceInterfaceDetailW(
            *disks,
            interface,
            Some(detail),
            needed,
            None,
            Some(device as *const _ as *mut _),
        )
    };
    read.ok()?;
    // SAFETY: the path is UTF-16 in the same buffer, after the struct,
    // within the length the call was given and NUL-terminated.
    let path = unsafe {
        let start = (detail as *const u16).add(head / 2);
        let path = std::slice::from_raw_parts(start, (length - head) / 2);
        let end = path
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(path.len());
        String::from_utf16_lossy(&path[..end])
    };
    Some(path)
}

/// Why a device eject was refused, as the veto the configuration manager
/// answered says.
#[allow(non_upper_case_globals)]
fn veto_why(veto: PNP_VETO_TYPE) -> &'static str {
    match veto {
        PNP_VetoWindowsApp => "a Windows application is using it",
        PNP_VetoWindowsService => "a Windows service is using it",
        PNP_VetoOutstandingOpen => "a program has it open",
        PNP_VetoPendingClose => "Windows is not done closing it",
        PNP_VetoInsufficientRights => "Pane may not eject it",
        _ => "Windows would not say",
    }
}

/// The Bluetooth radios there are, as the radios API finds them, each
/// with whether its provider service is enabled, which is the radio
/// being on. No radio at all is none, not a failure.
fn radios() -> Result<Vec<BluetoothRadio>, String> {
    let mut radio = HANDLE::default();
    let params = BLUETOOTH_FIND_RADIO_PARAMS {
        dwSize: size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32,
    };
    // SAFETY: `params` is initialized with its size and `radio` is a
    // writable handle for the first radio; no radio answers an error.
    let find = match unsafe { BluetoothFindFirstRadio(&params, &mut radio) } {
        Ok(find) => find,
        Err(_) => return Ok(Vec::new()),
    };
    let mut radios = Vec::new();
    let walked = loop {
        let read = read_radio(radio);
        // SAFETY: the handle the find answered, whose information was
        // read or not, which this call ends; the find handle stays for
        // the next radio.
        let _ = unsafe { CloseHandle(radio) };
        match read {
            Ok(radio) => radios.push(radio),
            Err(why) => break Err(why),
        }
        // SAFETY: the find handle, asking for the next radio; `radio` is
        // writable for it.
        if unsafe { BluetoothFindNextRadio(find, &mut radio) }.is_err() {
            break Ok(());
        }
    };
    // SAFETY: the handle `BluetoothFindFirstRadio` answered, whose radios
    // were all read or none more was.
    let _ = unsafe { BluetoothFindRadioClose(find) };
    walked?;
    Ok(radios)
}

/// One radio as the toggle sees it: its address as its id, and whether
/// its provider service is enabled.
fn read_radio(radio: HANDLE) -> Result<BluetoothRadio, String> {
    let mut info = BLUETOOTH_RADIO_INFO {
        dwSize: size_of::<BLUETOOTH_RADIO_INFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is initialized with its size and the handle is the
    // one the find answered.
    if unsafe { BluetoothGetRadioInfo(radio, &mut info) } != 0 {
        return Err("Windows did not say what the Bluetooth radio is".into());
    }
    // SAFETY: reading the union's field, which the call filled.
    let address = unsafe { info.address.Anonymous.ullLong };
    Ok(BluetoothRadio {
        id: format!("{address:x}"),
        on: provider_on(radio, address)?,
    })
}

/// Whether the radio `radio`'s provider service is enabled, which says
/// the radio is on: the services installed for the radio's own address,
/// as the radios API enumerates them.
fn provider_on(radio: HANDLE, address: u64) -> Result<bool, String> {
    let device = radio_device(address);
    let mut count = 0u32;
    // SAFETY: `device` is initialized with its size and carries the
    // radio's own address; no array is given, so the count is what the
    // call answers.
    let asked =
        unsafe { BluetoothEnumerateInstalledServices(Some(radio), &device, &mut count, None) };
    if asked != 0 {
        return Err("Windows did not say whether the Bluetooth radio is on".into());
    }
    let mut services = vec![GUID::zeroed(); count as usize];
    // SAFETY: the array is as long as the count the first call answered,
    // and `count` is writable again for how many were written.
    let listed = unsafe {
        BluetoothEnumerateInstalledServices(
            Some(radio),
            &device,
            &mut count,
            Some(services.as_mut_ptr()),
        )
    };
    if listed != 0 {
        return Err("Windows did not say whether the Bluetooth radio is on".into());
    }
    Ok(services[..count as usize].contains(&SVCID_BTH_PROVIDER))
}

/// The radio's own address as the device the service calls take, which
/// is how the radio's own service state is reached.
fn radio_device(address: u64) -> BLUETOOTH_DEVICE_INFO {
    BLUETOOTH_DEVICE_INFO {
        dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
        Address: BLUETOOTH_ADDRESS {
            Anonymous: BLUETOOTH_ADDRESS_0 { ullLong: address },
        },
        ..Default::default()
    }
}

/// Sets the provider service of the radio `id` to `state`, which turns
/// the radio on or off: the service call takes the radio's own address.
fn set_service(id: &str, state: u32) -> Result<(), String> {
    let Ok(wanted) = u64::from_str_radix(id, 16) else {
        return Err(RADIO_GONE.into());
    };
    let mut radio = HANDLE::default();
    let params = BLUETOOTH_FIND_RADIO_PARAMS {
        dwSize: size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32,
    };
    // SAFETY: `params` is initialized with its size and `radio` is a
    // writable handle for the first radio; no radio at all is one that
    // is gone.
    let find = match unsafe { BluetoothFindFirstRadio(&params, &mut radio) } {
        Ok(find) => find,
        Err(_) => return Err(RADIO_GONE.into()),
    };
    let mut answer = Err(RADIO_GONE.into());
    loop {
        let mut info = BLUETOOTH_RADIO_INFO {
            dwSize: size_of::<BLUETOOTH_RADIO_INFO>() as u32,
            ..Default::default()
        };
        let mut acted = false;
        // SAFETY: `info` is initialized with its size and the handle is
        // the one the find answered.
        if unsafe { BluetoothGetRadioInfo(radio, &mut info) } == 0 {
            // SAFETY: reading the union's field, which the call filled.
            let address = unsafe { info.address.Anonymous.ullLong };
            if address == wanted {
                // SAFETY: `device` is initialized with its size and
                // carries the radio's own address; the state is the one
                // given.
                let device = radio_device(address);
                let failed = unsafe {
                    BluetoothSetServiceState(Some(radio), &device, &SVCID_BTH_PROVIDER, state)
                };
                answer = if failed == 0 {
                    Ok(())
                } else {
                    Err("Windows did not turn the Bluetooth radio on or off".into())
                };
                acted = true;
            }
        } else {
            answer = Err("Windows did not say what the Bluetooth radio is".into());
            acted = true;
        }
        // SAFETY: the handle the find answered, which this call ends; the
        // find handle stays for the next radio.
        let _ = unsafe { CloseHandle(radio) };
        if acted || unsafe { BluetoothFindNextRadio(find, &mut radio) }.is_err() {
            break;
        }
    }
    // SAFETY: the handle `BluetoothFindFirstRadio` answered, whose radios
    // were all walked.
    let _ = unsafe { BluetoothFindRadioClose(find) };
    answer
}
