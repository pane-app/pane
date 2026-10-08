//! Windows: an application's icon as Explorer shows it.
//!
//! - A packaged app (`shell:AppsFolder\<AppUserModelID>`): the logo its
//!   manifest names ([`super::appx`]), found through the package's install
//!   folder (`GetPackagesByPackageFamily`, `GetPackagePathByFullName`), with
//!   its light and dark variants. A package Pane cannot read is drawn as
//!   the shell draws it, below.
//! - Anything else, a shortcut first: the shell's image of the source at
//!   256 pixels, icon only (`IShellItemImageFactory`). An image larger than
//!   48 pixels whose content does not fill its box
//!   ([`super::covers_enough`]), a small icon the shell padded into the
//!   jumbo size or framed in a thumbnail (a program that ships only a
//!   32-pixel icon), is rejected for the next: the shortcut's own icon
//!   location (`IShellLinkW::GetIconLocation`, extracted at 256 pixels with
//!   `PrivateExtractIconsW`; an internet shortcut's `IconFile` and
//!   `IconIndex`), then its target program's shell image, then that
//!   program's (or a program source's) own first icon, extracted the same
//!   way, so the system takes the largest image it has and scales it to
//!   256 pixels, and finally the shell's file information icon
//!   (`SHGetFileInfoW`, 32 pixels). Only when every source fails is a
//!   padded or framed image kept.
//!   Whichever is kept, the cache crops a small picture padded into its
//!   canvas and scales it to fill its place ([`super::fill_its_place`]).
//!   A ClickOnce reference (`.appref-ms`) first draws its deployed
//!   program's own icon, then that program's shell image, found where
//!   ClickOnce installs it ([`super::click_once`]), before the shell's
//!   image of the reference itself.
//!
//! Everything runs on the worker's thread, with COM initialized as a
//! single-threaded apartment for the call.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use windows::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::Storage::Packaging::Appx::{
    GetPackagePathByFullName, GetPackagesByPackageFamily,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ,
};
use windows::Win32::UI::Shell::{
    IShellLinkW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW, ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetIconInfo, HICON, ICONINFO, PrivateExtractIconsW,
};
use windows::core::{Interface, PCWSTR, PWSTR};

use super::{Extracted, appx, click_once, covers_enough, file_fingerprint};
use crate::applications::start_menu::{click_once_deployment, shortcut_text};
use crate::system_icons::{ICON_SIZE, SystemIcon};
use crate::util::wide;
use crate::windows_shell::{Com, Pixels, bitmap_pixels, shell_image};

/// What a packaged app's source path starts with, before its
/// AppUserModelID.
const APPS_FOLDER: &str = r"shell:AppsFolder\";

/// The packaged app's AppUserModelID `source` names, if it names one.
fn packaged(source: &str) -> Option<&str> {
    let prefix = source.get(..APPS_FOLDER.len())?;
    prefix
        .eq_ignore_ascii_case(APPS_FOLDER)
        .then(|| &source[APPS_FOLDER.len()..])
        .filter(|aumid| !aumid.is_empty() && !aumid.contains(['\\', '/']))
}

pub(super) fn fingerprint(source: &str) -> Option<String> {
    match packaged(source) {
        // A package's version is in its install folder's name, and its
        // manifest changes with it.
        Some(aumid) => {
            let (family, _) = appx::split_app_user_model_id(aumid)?;
            file_fingerprint(&package_folder(family)?.join("AppxManifest.xml"))
        }
        None => file_fingerprint(Path::new(source)),
    }
}

pub(super) fn extract(source: &str) -> Result<Extracted, String> {
    let _com = Com::new()?;
    if let Some(aumid) = packaged(source)
        && let Some(extracted) = packaged_logo(aumid)
    {
        return Ok(extracted);
    }
    let mut padded: Option<Pixels> = None;
    let mut problems: Vec<String> = Vec::new();
    let mut consider = |found: Result<Pixels, String>| -> Option<Pixels> {
        match found {
            Ok(pixels) if pixels.covers_enough() => Some(pixels),
            Ok(pixels) => {
                problems.push(format!(
                    "a {}×{} image padded or framed around a smaller icon",
                    pixels.width, pixels.height
                ));
                padded.get_or_insert(pixels);
                None
            }
            Err(why) => {
                problems.push(why);
                None
            }
        }
    };
    let has_extension = |wanted: &str| {
        Path::new(source)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(wanted))
    };
    // A ClickOnce reference: the deployed program's own icon, where
    // ClickOnce installs it, before the shell's image of the reference.
    if has_extension("appref-ms")
        && let Some(program) = click_once_program(source)
    {
        let program = program.to_string_lossy();
        if let Some(pixels) = consider(icon_resource(&program, 0)) {
            return pixels.extracted();
        }
        if let Some(pixels) = consider(shell_image(&*program)) {
            return pixels.extracted();
        }
    }
    if let Some(pixels) = consider(shell_image(source)) {
        return pixels.extracted();
    }
    // An internet shortcut (#173) names its icon in its own text.
    if has_extension("url")
        && let Ok(bytes) = std::fs::read(source)
        && let Some((file, index)) = internet_shortcut_icon(&String::from_utf8_lossy(&bytes))
        && let Some(pixels) = consider(icon_resource(&file, index))
    {
        return pixels.extracted();
    }
    // The program the source opens: a shortcut's target, or the source
    // itself.
    let mut program = None;
    if has_extension("lnk")
        && let Ok(link) = read_link(Path::new(source))
    {
        if let Some((file, index)) = &link.icon_location
            && let Some(pixels) = consider(icon_resource(file, *index))
        {
            return pixels.extracted();
        }
        if !link.target.is_empty() {
            if let Some(pixels) = consider(shell_image(&link.target)) {
                return pixels.extracted();
            }
            program = Some(link.target);
        }
    } else if has_extension("exe") {
        program = Some(source.to_owned());
    }
    // A program that ships only a small icon is framed or padded by the
    // shell: its own first icon, the largest image it has scaled to 256
    // pixels, fills its place.
    if let Some(program) = &program
        && let Some(pixels) = consider(icon_resource(program, 0))
    {
        return pixels.extracted();
    }
    match file_info_icon(source) {
        Ok(pixels) => pixels.extracted(),
        Err(why) => match padded {
            Some(pixels) => pixels.extracted(),
            None => {
                problems.push(why);
                Err(problems.join("; "))
            }
        },
    }
}

/// The program the ClickOnce reference at `source` deploys, where
/// ClickOnce installed it ([`click_once::deployed_program`]).
fn click_once_program(source: &str) -> Option<PathBuf> {
    let text = shortcut_text(&std::fs::read(source).ok()?);
    let deployment = click_once_deployment(&text)?;
    click_once::deployed_program(&deployment, &click_once::store()?)
}

impl Pixels {
    fn covers_enough(&self) -> bool {
        covers_enough(self.width, self.height, &self.rgba)
    }

    /// This image as the application's icon for both themes.
    fn extracted(self) -> Result<Extracted, String> {
        crate::icons::encode_png(self.width, self.height, &self.rgba)
            .map(|png| Extracted::one(SystemIcon::Png(png)))
            .ok_or_else(|| "its pixels make no image".to_owned())
    }
}

/// `buffer` up to its first NUL, as text.
fn text(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end]).trim().to_owned()
}

/// What a shortcut points at: its target and its own icon location.
struct Link {
    target: String,
    /// The file and index of the icon the shortcut names, if it names one.
    icon_location: Option<(String, i32)>,
}

/// Reads the shortcut at `path`, without resolving a target that moved.
fn read_link(path: &Path) -> windows::core::Result<Link> {
    let file = wide(path);
    // SAFETY: plain COM calls on interfaces the shell returns, on a thread
    // whose COM apartment outlives them; every buffer outlives its call.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.cast::<IPersistFile>()?
            .Load(PCWSTR(file.as_ptr()), STGM_READ)?;
        let mut target = vec![0u16; 4096];
        let _ = link.GetPath(&mut target, std::ptr::null_mut(), 0);
        let mut location = vec![0u16; 4096];
        let mut index = 0i32;
        let icon_location = link
            .GetIconLocation(&mut location, &mut index)
            .ok()
            .map(|()| expand_environment(&text(&location)))
            .filter(|file| !file.is_empty())
            .map(|file| (file, index));
        Ok(Link {
            target: expand_environment(&text(&target)),
            icon_location,
        })
    }
}

/// The icon an internet shortcut's (`.url`) `text` names: the `IconFile`
/// and `IconIndex` keys of its `[InternetShortcut]` section (a game
/// launcher's link names the game's icon there), the file's environment
/// variables expanded; `None` when it names no icon file.
fn internet_shortcut_icon(text: &str) -> Option<(String, i32)> {
    let mut in_section = false;
    let (mut file, mut index) = (None, 0);
    for line in text.lines() {
        let line = line.trim();
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            in_section = section.trim().eq_ignore_ascii_case("InternetShortcut");
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_section) else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key.eq_ignore_ascii_case("IconFile") && !value.is_empty() {
            file = Some(expand_environment(value));
        } else if key.eq_ignore_ascii_case("IconIndex") {
            index = value.parse().unwrap_or(0);
        }
    }
    file.map(|file| (file, index))
}

/// `text` with each `%NAME%` replaced by the environment variable's value
/// (left as it is when unset), as a shortcut's paths are written.
fn expand_environment(text: &str) -> String {
    let mut expanded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        expanded.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(value) => expanded.push_str(&value),
                    Err(_) => {
                        expanded.push('%');
                        expanded.push_str(name);
                        expanded.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                expanded.push('%');
                rest = after;
            }
        }
    }
    expanded.push_str(rest);
    expanded
}

/// The icon at `index` of `file` (an `.ico`, or a program or library's
/// resources), at 256 pixels: the system takes the size it has closest to
/// that, the largest when all are smaller, and scales it.
fn icon_resource(file: &str, index: i32) -> Result<Pixels, String> {
    let units: Vec<u16> = file.encode_utf16().collect();
    if units.len() >= 260 {
        return Err(format!("the icon file {file} has too long a path"));
    }
    let mut name = [0u16; 260];
    name[..units.len()].copy_from_slice(&units);
    let mut icons = [HICON::default()];
    let side = ICON_SIZE as i32;
    // SAFETY: `name` is a NUL-terminated path of the size the function
    // reads; `icons` holds the one icon asked for.
    let found =
        unsafe { PrivateExtractIconsW(&name, index, side, side, Some(&mut icons), None, 0) };
    if found == 0 || found == u32::MAX || icons[0].is_invalid() {
        return Err(format!("{file} has no icon {index}"));
    }
    // SAFETY: the icon is this caller's, destroyed once read.
    unsafe { icon_pixels(icons[0]) }
}

/// The shell's file information icon of `path`: its large icon (32 pixels
/// at the system's scale), drawn as it is.
fn file_info_icon(path: &str) -> Result<Pixels, String> {
    let name = wide(path);
    let mut info = SHFILEINFOW::default();
    // SAFETY: `info` is a SHFILEINFOW of the size given; `name` outlives
    // the call.
    let found = unsafe {
        SHGetFileInfoW(
            PCWSTR(name.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info as *mut SHFILEINFOW),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if found == 0 || info.hIcon.is_invalid() {
        return Err(format!("the shell has no icon for {path}"));
    }
    // SAFETY: the icon is this caller's, destroyed once read.
    unsafe { icon_pixels(info.hIcon) }
}

/// The pixels of `icon`, which is destroyed.
///
/// # Safety
///
/// `icon` must be a valid icon handle this caller owns.
unsafe fn icon_pixels(icon: HICON) -> Result<Pixels, String> {
    let mut info = ICONINFO::default();
    // SAFETY: `info` is an ICONINFO; the icon is valid.
    let read = unsafe { GetIconInfo(icon, &mut info) }
        .map_err(|error| format!("the icon cannot be read: {error}"))
        .and_then(|()| {
            if info.hbmColor.is_invalid() {
                return Err("the icon has no colours".to_owned());
            }
            // SAFETY: the colour bitmap GetIconInfo made for this caller.
            unsafe { bitmap_pixels(info.hbmColor) }
        });
    // SAFETY: the bitmaps GetIconInfo made and the icon are this caller's;
    // nothing uses them after.
    unsafe {
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ::from(info.hbmColor));
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ::from(info.hbmMask));
        }
        let _ = DestroyIcon(icon);
    }
    read
}

/// The install folder of the package family `family`: its first package
/// for the current user.
fn package_folder(family: &str) -> Option<PathBuf> {
    let family = wide(family);
    let (mut count, mut length) = (0u32, 0u32);
    // SAFETY: asking for the sizes only; `family` outlives the call.
    let sized = unsafe {
        GetPackagesByPackageFamily(PCWSTR(family.as_ptr()), &mut count, None, &mut length, None)
    };
    if sized != ERROR_INSUFFICIENT_BUFFER || count == 0 {
        return None;
    }
    let mut names = vec![PWSTR::null(); count as usize];
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: `names` holds `count` pointers and `buffer` `length` units,
    // as the first call asked; both outlive the call.
    let listed = unsafe {
        GetPackagesByPackageFamily(
            PCWSTR(family.as_ptr()),
            &mut count,
            Some(names.as_mut_ptr()),
            &mut length,
            Some(PWSTR(buffer.as_mut_ptr())),
        )
    };
    if listed != ERROR_SUCCESS {
        return None;
    }
    names.iter().take(count as usize).find_map(|name| {
        // SAFETY: each name points into `buffer`, NUL-terminated.
        let full = unsafe { name.to_string() }.ok()?;
        package_path(&full)
    })
}

/// The install folder of the package with full name `full`.
fn package_path(full: &str) -> Option<PathBuf> {
    let full = wide(full);
    let mut length = 0u32;
    // SAFETY: asking for the size only; `full` outlives the call.
    let sized = unsafe { GetPackagePathByFullName(PCWSTR(full.as_ptr()), &mut length, None) };
    if sized != ERROR_INSUFFICIENT_BUFFER || length == 0 {
        return None;
    }
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: `buffer` holds `length` units, as the first call asked.
    let read = unsafe {
        GetPackagePathByFullName(
            PCWSTR(full.as_ptr()),
            &mut length,
            Some(PWSTR(buffer.as_mut_ptr())),
        )
    };
    (read == ERROR_SUCCESS)
        .then(|| PathBuf::from(text(&buffer)))
        .filter(|path| path.is_dir())
}

/// The logo of the packaged app `aumid`, from its package's manifest: the
/// variant for the dark theme and, when the package ships one, the light
/// theme's. `None` when the package, its manifest or the logo cannot be
/// read.
fn packaged_logo(aumid: &str) -> Option<Extracted> {
    let (family, app) = appx::split_app_user_model_id(aumid)?;
    let folder = package_folder(family)?;
    let manifest = std::fs::read_to_string(folder.join("AppxManifest.xml")).ok()?;
    let logo = appx::manifest_logo(&manifest, app)?;
    let logo = folder.join(logo.replace('/', "\\"));
    let logo_folder = logo.parent()?;
    let logo_name = logo.file_name()?.to_string_lossy().into_owned();
    let files: Vec<String> = std::fs::read_dir(logo_folder)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    let (dark, light) = appx::pick_logos(&logo_name, &files)?;
    let file = |name: &str| SystemIcon::File(logo_folder.join(name));
    Some(match light {
        Some(light) => Extracted {
            light: file(&light),
            dark: Some(file(&dark)),
        },
        None => Extracted::one(file(&dark)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortcut_s_environment_variables_are_expanded() {
        let root = std::env::var("SystemRoot").unwrap();
        assert_eq!(
            expand_environment(r"%SystemRoot%\system32\shell32.dll"),
            format!(r"{root}\system32\shell32.dll")
        );
        assert_eq!(
            expand_environment("%PANE_NO_SUCH_VARIABLE%\\a"),
            "%PANE_NO_SUCH_VARIABLE%\\a"
        );
        assert_eq!(expand_environment("100% sure"), "100% sure");
    }

    #[test]
    fn an_internet_shortcut_names_its_icon_file_and_index() {
        let game = "[{000214A0-0000-0000-C000-000000000046}]\r\nProp3=19,0\r\n\
            [InternetShortcut]\r\nIDList=\r\nIconIndex=2\r\nURL=steam://rungameid/570\r\n\
            IconFile=C:\\Steam\\steam\\games\\dota.ico\r\n";
        assert_eq!(
            internet_shortcut_icon(game),
            Some((r"C:\Steam\steam\games\dota.ico".to_owned(), 2))
        );
        // No icon file: the shell's image of the shortcut is all there is.
        assert_eq!(
            internet_shortcut_icon("[InternetShortcut]\nURL=steam://run/1\nIconIndex=0\n"),
            None
        );
        // Another section's keys do not count.
        assert_eq!(
            internet_shortcut_icon("[Other]\nIconFile=C:\\a.ico\n[InternetShortcut]\nURL=x:y\n"),
            None
        );
    }

    #[test]
    fn a_packaged_app_is_named_by_the_apps_folder() {
        assert_eq!(
            packaged(r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
            Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App")
        );
        assert_eq!(packaged(r"C:\Menu\Calculator.lnk"), None);
        assert_eq!(packaged(r"shell:AppsFolder\"), None);
    }
}
