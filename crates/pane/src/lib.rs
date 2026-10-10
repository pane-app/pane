//! Pane's native launcher, rendered with GPUI CE: this module exposes the
//! crate's entry points — the key bindings, the default extensions, the
//! folders Pane keeps, and the host settings — and
//! re-exports the launcher window ([`app`]), the Settings window
//! ([`features::settings`]) and the system's link opener ([`links`]).

use std::path::PathBuf;

use gpui::{App, KeyBinding, WindowBackgroundAppearance, actions};
// `Window` names the rounded-corner preference's and the HUD's
// click-through's parameter.
use gpui::Window;
use pane_core::Keyboard;

mod app;
mod background;
mod extension_views;
mod features;
mod keyboard;
mod links;
mod ui;

pub mod placement;
pub mod settings;
pub mod taskbar;

pub use app::LauncherWindow;
pub use features::settings::SettingsWindow;
pub use links::SystemLinks;

actions!(
    launcher,
    [
        SelectNext,
        SelectPrevious,
        SelectNextPage,
        SelectPreviousPage,
        Confirm,
        Back,
        FocusNext,
        FocusPrevious,
        OpenSettings,
        OpenActions,
        ReturnToRoot,
        DismissLauncher
    ]
);

/// Registers the launcher's key bindings: the full registration over the
/// bindings the host settings hold (see [`keyboard`]), so a record the
/// Keyboard page saved is in force from the first window. The settings
/// must have been initialized first, as the binary does before this
/// runs.
pub fn bind_keys(cx: &mut App) {
    bind_keys_with(cx, &settings::keyboard_of(cx), settings::navigation_of(cx));
}

/// The full key registration over `keyboard`: [`bind_keys`] is this over
/// the host settings' bindings, and the settings entity itself re-runs it
/// over the keyboard it holds as a choice changes
/// ([`keyboard::rebuild`]) — it cannot read itself back through
/// [`settings::keyboard_of`] while its own update is in flight.
pub(crate) fn bind_keys_with(
    cx: &mut App,
    keyboard: &Keyboard,
    navigation: pane_core::NavigationBindings,
) {
    cx.bind_keys([
        KeyBinding::new("tab", FocusNext, Some(app::KEY_CONTEXT)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(app::KEY_CONTEXT)),
        // Page Down and Page Up move the selection by the rows in view
        // (#165), also while the query field has focus, which does not
        // take them; a binding the Keyboard page records for them wins,
        // registered after.
        KeyBinding::new("pagedown", SelectNextPage, Some(app::KEY_CONTEXT)),
        KeyBinding::new("pageup", SelectPreviousPage, Some(app::KEY_CONTEXT)),
    ]);
    let text_editing = ui::input::bind_text_editing(cx);
    extension_views::form::bind_keys(cx, &text_editing);
    features::root_search::bind_keys(cx, &text_editing, keyboard);
    features::root_search::arguments::bind_keys(cx, &text_editing);
    features::clipboard_history::bind_keys(cx, &text_editing, keyboard);
    features::extension_log::bind_keys(cx);
    features::quick_slots::bind_keys(cx);
    features::footer_menu::bind_keys(cx);
    features::toast::bind_keys(cx);
    features::confirmation::bind_keys(cx);
    features::actions_panel::bind_keys(cx, &text_editing);
    features::settings::bind_keys(cx);
    ui::select::bind_keys(cx);
    extension_views::custom_view::bind_keys(cx);
    keyboard::bind_keys(cx, keyboard, navigation);
}

/// The version of Pane this build is: the workspace's version, or the one
/// `cargo xtask package-windows --package-version` gave the program when
/// it packed it (a build whose version the packaging overrode, so an
/// update's version transition can be checked). This is the version
/// `pane --version` prints and the one an application update compares
/// itself with.
pub const APP_VERSION: &str = match option_env!("PANE_PACKAGE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// The pins this build of Pane sets its default extensions up from:
/// `defaults.json`, committed beside the crate, naming each default
/// extension's id, title, repository, release tag and that tag's commit
/// (ADR 0045). The default extensions are the calculator, applications,
/// quicklinks, files and clipboard history
/// ([#60](https://github.com/pane-app/pane/issues/60), the user's
/// recorded choice), in every build: all five set up at first setup and
/// each individually disableable, clipboard history recording what is
/// copied from the first start (#166, ADR 0042). The installer carries
/// none of them: first setup fetches each pinned commit from its
/// repository with Pane's own Git client (ADR 0021). The samples are no
/// default extension (#162): a contributor installs one by hand with
/// `pane --install <folder>`. An install that acquired the helper sample
/// as a default before keeps it as an ordinary installed package, which
/// the user can uninstall; Pane does not remove it.
///
/// The committed pins name the five repositories under
/// `https://github.com/pane-app/<id>`, each at the commit of its
/// repository's initial release revision — v0.5.0 for the calculator,
/// applications and quicklinks, v0.8.0 for files, v0.6.0 for the
/// clipboard history (#279, #282). A release's pins are what a fresh
/// install acquires, so they are moved with the release that tested
/// them, not between releases. A development build can replace them
/// with a file of its own through `PANE_DEFAULTS` (see
/// [`pane_core::defaults::pins_from_dev_env`]).
///
/// The Windows power features' default extensions (ADR 0040) — Run
/// (#254), System Commands (#255) and Switch Windows (#263) — are not
/// pinned yet: this repository still builds them (`guests/run`,
/// `guests/system-commands`, `guests/switch-windows`) until their own
/// repositories release, when their pins join the committed ones.
pub fn default_extensions() -> Vec<pane_core::DefaultExtension> {
    pane_core::defaults::parse_pins(DEFAULT_EXTENSIONS_PINS)
        .expect("the committed pins are valid; a test checks them")
}

/// The committed pins, as this build sets its default extensions up.
const DEFAULT_EXTENSIONS_PINS: &str = include_str!("../defaults.json");

/// Initializes Pane's host settings — the appearance preferences, the
/// Open Pane hotkey and the launch-at-login choice recorded in
/// `settings.json` in Pane's data folder, which both windows follow as
/// they change, with the development overrides `PANE_THEME` and
/// `PANE_MATERIAL` winning for this process and the platform's login
/// integration reconciled with the saved choice
/// — and embeds the Geist fonts.
/// The binary calls this once at startup, before opening the first window;
/// a font error is returned but the caller may continue with the system's
/// default font. Tests never call it: a window built without initialized
/// settings falls back to the in-memory defaults (see [`settings::ensure`]).
pub fn configure_visuals(cx: &mut App) -> gpui::Result<()> {
    settings::init(data_dir(), cx);
    ui::load_fonts(cx)
}

/// Follows the operating system's reduced-motion preference for the whole
/// app, once, before the first window opens: what is actually read on each
/// system, what falls back where nothing is readable, and how a Windows
/// change is applied while Pane runs are documented on the policy itself
/// (`ui::motion`). Call before the first frame draws; the launcher's view
/// transitions (and anything else that consults
/// [`gpui::App::reduce_motion`]) then follow the preference.
pub fn observe_reduced_motion(cx: &mut App) {
    ui::motion::observe_reduced_motion(cx)
}

/// The launcher window's client size: the reference root panel's 760×518
/// logical pixels (64 search header + 404 results + 50 footer), which the
/// binary opens the window at.
pub fn launcher_client_size() -> gpui::Size<gpui::Pixels> {
    let (width, height) = ui::shell::LAUNCHER_CLIENT;
    gpui::size(gpui::px(width), gpui::px(height))
}

/// The window background appearance the host settings' material asks for,
/// for the binary to pass into `WindowOptions::window_background`: blurred
/// behind a glass panel on the frost-capable platforms, opaque otherwise
/// and for the solid material. The windows keep following it as the
/// material changes (see [`settings`]).
pub fn window_background(cx: &mut App) -> WindowBackgroundAppearance {
    settings::window_background(cx)
}

/// Asks Windows's Desktop Window Manager to round the window's own corners
/// — the platform's equivalent of the window-server rounding a macOS window
/// gets — so the panel that fills the window ends in a rounded silhouette
/// with nothing showing behind it: the compositor clips the acrylic frost
/// and the opaque surface to the same curve it gives other applications.
/// Before the corner-preference attribute existed the call fails without
/// effect and the window stays square. The panel paints no radius of its
/// own on Windows (see `ui::theme::Geometry`); a painted curve there left
/// the frost — or the opaque white clear — visible as a plate behind the
/// rounded corners.
#[cfg(target_os = "windows")]
pub fn prefer_rounded_window_corners(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
    };
    // `Window` also has an inherent `window_handle` (GPUI's own identifier),
    // so the raw-window-handle trait is named rather than called through
    // the receiver.
    let handle = match HasWindowHandle::window_handle(window) {
        Ok(handle) => handle,
        Err(_) => return,
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    // SAFETY: `hwnd` is this window, which GPUI created before the handle
    // was read, and `DWMWCP_ROUND` is passed by reference with its size, as
    // the attribute's contract requires.
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &DWMWCP_ROUND as *const _ as *const _,
            std::mem::size_of_val(&DWMWCP_ROUND) as u32,
        )
    };
}

/// Makes `window`, a HUD's (see `features::hud`), let the pointer through
/// to what is under it and never activate, where the system allows: on
/// Windows a layered, transparent, non-activating window (fully opaque, so
/// it still shows); on macOS one that ignores mouse events. Elsewhere, and
/// on GPUI's test platform, it stays as GPUI made it: a pop-up that was
/// shown without the focus.
#[cfg(target_os = "windows")]
pub(crate) fn make_click_through(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes, SetWindowLongPtrW,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT,
    };
    let handle = match HasWindowHandle::window_handle(window) {
        Ok(handle) => handle,
        Err(_) => return,
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    let added = (WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE).0 as isize;
    // SAFETY: `hwnd` is this window, which GPUI created before the handle
    // was read; only its extended style and its layered opacity change.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | added);
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
    }
}

/// Makes `window`, a HUD's, let the pointer through (see the Windows
/// version): an AppKit window that ignores mouse events.
#[cfg(target_os = "macos")]
pub(crate) fn make_click_through(window: &Window) {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::NSObject;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = match HasWindowHandle::window_handle(window) {
        Ok(handle) => handle,
        Err(_) => return,
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: `view` is the `NSView` GPUI built this window from, and its
    // `NSWindow` is held only for this call, which changes nothing but
    // whether it takes mouse events.
    let view = handle.ns_view.cast::<NSObject>();
    let ns_window: Option<Retained<NSObject>> = unsafe { msg_send![view, window] };
    if let Some(ns_window) = ns_window {
        let _: () = unsafe { msg_send![&*ns_window, setIgnoresMouseEvents: true] };
    }
}

/// Makes `window`, a HUD's, let the pointer through where the system
/// allows: on Linux the HUD stays the pop-up GPUI made, shown without the
/// focus.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn make_click_through(_window: &Window) {}

/// Where Pane keeps disposable cached data, such as compiled extension code:
/// `%LOCALAPPDATA%\Pane\cache` on Windows, `~/Library/Caches/Pane` on
/// macOS and `$XDG_CACHE_HOME/pane` (default `~/.cache/pane`) elsewhere.
pub fn cache_dir() -> Option<PathBuf> {
    // Wasmtime's compile cache needs an absolute directory: a relative
    // HOME or XDG_CACHE_HOME (the smokes' clean home is one) would otherwise
    // stop the runtime from starting, so the path is made absolute against
    // the folder Pane was started in.
    platform_dir(
        r"Pane\cache",
        "Library/Caches/Pane",
        ("XDG_CACHE_HOME", ".cache"),
    )
    .and_then(|dir| std::path::absolute(dir).ok())
}

/// Where Pane keeps installed extension packages: `PANE_DATA_DIR` when set,
/// otherwise `%LOCALAPPDATA%\Pane\data` on Windows,
/// `~/Library/Application Support/Pane` on macOS and `$XDG_DATA_HOME/pane`
/// (default `~/.local/share/pane`) elsewhere. Packages go in its
/// `extensions` folder.
pub fn data_dir() -> Option<PathBuf> {
    env_dir("PANE_DATA_DIR").or_else(|| {
        platform_dir(
            r"Pane\data",
            "Library/Application Support/Pane",
            ("XDG_DATA_HOME", ".local/share"),
        )
    })
}

/// Where Pane keeps its own log and the marker of the run in progress
/// (#133, [`pane_core::diagnostics`]): `logs` in `PANE_DATA_DIR` when set
/// (tests and smokes), otherwise `%LOCALAPPDATA%\Pane\logs` on Windows,
/// `~/Library/Logs/Pane` on macOS (where Console shows it) and
/// `$XDG_STATE_HOME/pane/logs` (default `~/.local/state/pane/logs`)
/// elsewhere.
pub fn logs_dir() -> Option<PathBuf> {
    if let Some(data) = env_dir("PANE_DATA_DIR") {
        return Some(data.join("logs"));
    }
    let dir = platform_dir(
        r"Pane\logs",
        "Library/Logs/Pane",
        ("XDG_STATE_HOME", ".local/state"),
    )?;
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        Some(dir)
    } else {
        Some(dir.join("logs"))
    }
}

/// Opens Pane's log in [`logs_dir`] and this run's crash record, which the
/// binary does first thing: every diagnostic and panic goes to the log from
/// then on, and the record says whether the run before ended unexpectedly.
/// `None` when no logs folder can be named.
pub fn start_crash_record() -> Option<std::sync::Arc<pane_core::diagnostics::CrashRecord>> {
    let folder = logs_dir()?;
    let record = pane_core::diagnostics::start(&folder, APP_VERSION);
    Some(std::sync::Arc::new(record))
}

/// A per-user folder: `windows` under `%LOCALAPPDATA%`, `macos` under
/// `$HOME`, and elsewhere `pane` under the XDG variable `xdg.0`, or under
/// `$HOME/xdg.1` when that is unset.
fn platform_dir(windows: &str, macos: &str, xdg: (&str, &str)) -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env_dir("LOCALAPPDATA").map(|dir| dir.join(windows))
    } else if cfg!(target_os = "macos") {
        env_dir("HOME").map(|home| home.join(macos))
    } else {
        let (variable, fallback) = xdg;
        env_dir(variable)
            .or_else(|| env_dir("HOME").map(|home| home.join(fallback)))
            .map(|dir| dir.join("pane"))
    }
}

/// The folder in environment variable `name`, if it is set and not empty.
fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed pins name the five default extensions, each once,
    /// in every build: no sample is set up at first setup (#162).
    #[test]
    fn the_default_set_is_the_five_default_extensions_without_the_samples() {
        let ids: Vec<String> = super::default_extensions()
            .into_iter()
            .map(|extension| extension.id)
            .collect();
        assert_eq!(
            ids,
            [
                "calculator",
                "applications",
                "quicklinks",
                pane_core::search_files::FILES,
                pane_core::clipboard_view::CLIPBOARD_HISTORY
            ]
        );
    }

    /// The committed pins are well-formed: every default extension named
    /// exactly once, with a title, its own repository in Pane's
    /// organization, a `v…` release tag and a full commit id that is not
    /// the placeholder all-zero one: the pins name the repositories'
    /// initial release revisions (#282), which the placeholders stood in
    /// for until the repositories released (#279).
    #[test]
    fn the_committed_pins_are_well_formed() {
        let pins = default_extensions();
        for pin in &pins {
            assert!(!pin.title.trim().is_empty(), "{pin:?}");
            assert_eq!(
                pin.repository,
                format!("https://github.com/pane-app/{}", pin.id),
                "{pin:?}"
            );
            assert!(pin.tag.starts_with('v'), "{pin:?}");
            assert!(
                pin.tag[1..]
                    .split('.')
                    .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())),
                "{pin:?}"
            );
            assert_eq!(pin.commit.len(), 40, "{pin:?}");
            assert!(pin.commit.chars().all(|c| c.is_ascii_hexdigit()), "{pin:?}");
            // A full id that is not the placeholder: the file's commits
            // were all zeros until the repositories released (#279);
            // this build pins their release revisions (#282).
            assert_ne!(
                pin.commit, "0000000000000000000000000000000000000000",
                "the pin still names the placeholder commit"
            );
        }
        // The five ids, each once: `the_default_set_is…` holds the order;
        // this holds that no id is named twice, which the parse refuses
        // of a file but a committed list could still drift into by
        // repeating one under another name.
        let mut ids: Vec<&str> = pins.iter().map(|pin| pin.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 5);
    }
}
