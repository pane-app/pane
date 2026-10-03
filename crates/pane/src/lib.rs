//! Pane's native launcher, rendered with GPUI CE: this module exposes the
//! crate's entry points — the key bindings, the build's sample commands and
//! default extensions, the folders Pane keeps, and the host settings — and
//! re-exports the launcher window ([`app`]), the Settings window
//! ([`features::settings`]) and the system's link opener ([`links`]).

use std::path::PathBuf;

use gpui::{App, KeyBinding, WindowBackgroundAppearance, actions};
// `Window` names the rounded-corner preference's parameter, which only
// Windows has; the import follows the same gate so it is not unused on the
// other platforms.
#[cfg(target_os = "windows")]
use gpui::Window;
use pane_core::CommandRegistration;

mod app;
mod extension_views;
mod features;
mod links;
mod ui;

pub mod settings;

pub use app::LauncherWindow;
pub use features::settings::SettingsWindow;
pub use links::SystemLinks;

actions!(
    launcher,
    [
        SelectNext,
        SelectPrevious,
        Confirm,
        Back,
        FocusNext,
        FocusPrevious,
        OpenSettings
    ]
);

/// Registers the launcher's key bindings.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(app::KEY_CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(app::KEY_CONTEXT)),
        KeyBinding::new("enter", Confirm, Some(app::KEY_CONTEXT)),
        KeyBinding::new("escape", Back, Some(app::KEY_CONTEXT)),
        KeyBinding::new("tab", FocusNext, Some(app::KEY_CONTEXT)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(app::KEY_CONTEXT)),
        // The local shortcut that opens Settings: Cmd+, on macOS, Ctrl+, on
        // Windows and Linux, in the launcher's window only. Rebinding it is
        // the Keyboard page's later work.
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-,"
            } else {
                "ctrl-,"
            },
            OpenSettings,
            Some(app::KEY_CONTEXT),
        ),
    ]);
    let text_editing = ui::input::bind_text_editing(cx);
    extension_views::form::bind_keys(cx, &text_editing);
    features::root_search::bind_keys(cx, &text_editing);
    features::footer_menu::bind_keys(cx);
    features::settings::bind_keys(cx);
    extension_views::custom_view::bind_keys(cx);
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

/// The sample commands: (id, title, subtitle, component file name). Each
/// implements the same command in a different extension language.
#[cfg(debug_assertions)]
const SAMPLES: [(&str, &str, &str, &str); 3] = [
    (
        "rust-sample",
        "Rust sample",
        "A sample command implemented by a Rust extension",
        "sample_rust.wasm",
    ),
    (
        "javascript-sample",
        "JavaScript sample",
        "The same command implemented by a JavaScript extension",
        "sample_js.wasm",
    ),
    (
        "typescript-sample",
        "TypeScript sample",
        "The same command implemented by a TypeScript extension",
        "sample_ts.wasm",
    ),
];

/// The commands this build offers from the development checkout: the
/// Rust, JavaScript and TypeScript sample commands. A release build offers
/// none: their components live in the build's `target/guests` folder,
/// which an installed Pane does not have — its features come from the
/// default extensions it acquires at first setup instead.
///
/// Their components are read from `PANE_EXTENSIONS_DIR` when set, otherwise
/// from the development build output `target/guests`. A missing component
/// leaves its command listed; opening it explains what is missing.
pub fn sample_commands() -> Vec<CommandRegistration> {
    #[cfg(not(debug_assertions))]
    return Vec::new();
    #[cfg(debug_assertions)]
    {
        let dir = std::env::var_os("PANE_EXTENSIONS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests")
            });
        SAMPLES
            .iter()
            .map(|&(id, title, subtitle, file)| CommandRegistration {
                id: id.into(),
                title: title.into(),
                subtitle: Some(subtitle.into()),
                component: dir.join(file),
                takes_query: false,
                search: false,
            })
            .collect()
    }
}

/// The default extensions this build of Pane acquires at first setup, from
/// Pane's own downloads (see
/// [`pane_core::defaults`]): the installer carries none of their payloads.
/// The release's default extensions are the calculator, applications,
/// quicklinks, files and clipboard history ([#60](https://github.com/hoangvu12/pane/issues/60),
/// the user's recorded choice): all five enabled by default and each
/// individually disableable, with clipboard history's capture still off
/// until the user turns it on. In development builds the prebuilt-helper
/// sample is acquired with them, so a payload carrying a native helper is
/// acquired and its helper runs without any developer tool.
pub fn default_extensions() -> Vec<pane_core::DefaultExtension> {
    let mut extensions = vec![
        pane_core::DefaultExtension {
            id: "calculator".into(),
            title: "Calculator".into(),
        },
        pane_core::DefaultExtension {
            id: "applications".into(),
            title: "Applications".into(),
        },
        pane_core::DefaultExtension {
            id: "quicklinks".into(),
            title: "Quicklinks".into(),
        },
        pane_core::DefaultExtension {
            id: "files".into(),
            title: "Files".into(),
        },
        pane_core::DefaultExtension {
            id: "clipboard-history".into(),
            title: "Clipboard History".into(),
        },
    ];
    #[cfg(debug_assertions)]
    extensions.push(pane_core::DefaultExtension {
        id: "helper-sample".into(),
        title: "Helper sample".into(),
    });
    extensions
}

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
