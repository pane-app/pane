#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*};
use pane::LauncherWindow;
use pane_core::develop::Toolchains;
use pane_core::{Launcher, Runtime};

/// What `--install` asks to show for installation.
enum ToPreview {
    Folder(PathBuf),
    /// `npm:<name>` or `npm:<name>@<version>`.
    Npm(String),
    /// `git:<repository>` or `git:<repository>@<branch, tag or commit>`.
    Git(String),
}

/// `pane [--install <folder> | --install npm:<package>[@<version>] |
/// --install git:<repository>[@<reference>]]`: `--install` opens with the
/// package in `<folder>` shown for installation, as if chosen with the
/// folder picker, the npm package, as if named in "Install extension from
/// npm…", or the Git repository, as if named in "Install extension from
/// Git…". `pane --version` prints Pane's version and exits without opening
/// a window, so an installation can check what it installed.
fn package_to_preview() -> Option<ToPreview> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--version" {
            println!("Pane {}", pane::APP_VERSION);
            std::process::exit(0);
        }
        if arg == "--install" {
            let source = args.next()?;
            let text = source.to_str();
            if let Some(spec) = text.and_then(|s| s.strip_prefix("npm:")) {
                return Some(ToPreview::Npm(spec.to_owned()));
            }
            if let Some(spec) = text.and_then(|s| s.strip_prefix("git:")) {
                return Some(ToPreview::Git(spec.to_owned()));
            }
            return Some(ToPreview::Folder(PathBuf::from(source)));
        }
    }
    None
}

fn main() {
    let preview = package_to_preview();
    gpui_platform::application().run(move |cx: &mut App| {
        // Pane's own settings — the appearance preferences recorded in
        // settings.json, with the PANE_THEME/PANE_MATERIAL development
        // overrides winning for this process — and the embedded Geist
        // fonts, before the key bindings: the bindings the Keyboard page
        // recorded are registered from the record the settings hold, so
        // a saved rebind is in force from the first window. A font
        // failure only falls back to the system's default font.
        if let Err(error) = pane::configure_visuals(cx) {
            eprintln!("Pane's fonts could not be loaded: {error:#}");
        }
        pane::bind_keys(cx);
        // The operating system's reduced-motion preference, followed for as
        // long as Pane runs: the launcher's view transitions settle at once
        // while it is set, including mid-transition when the system reports
        // the change.
        pane::observe_reduced_motion(cx);
        let runtime = match pane::cache_dir() {
            Some(dir) => Runtime::start_with_cache(dir),
            None => Runtime::start(),
        };
        // Quitting ends the native helpers still running, which would
        // otherwise outlive Pane, and every call still waiting.
        if let Ok(runtime) = &runtime {
            // The native smokes crash the runtime on purpose, to check that
            // Pane recovers (#17); nothing else sets this, and a release
            // build has no such hook.
            #[cfg(debug_assertions)]
            if let Some(file) = std::env::var_os("PANE_TEST_RUNTIME_FAULTS") {
                runtime.watch_fault_file(PathBuf::from(file));
            }
            let runtime = runtime.clone();
            cx.on_app_quit(move |_| {
                runtime.quit();
                async {}
            })
            .detach();
        }
        // The quick slots' record, beside the host settings in the same
        // data folder (#101).
        let launcher = match pane::data_dir() {
            Some(dir) => {
                Launcher::with_packages(runtime, pane::sample_commands(), dir.join("extensions"))
                    .with_quick_slots(&dir)
            }
            None => Launcher::new(runtime, pane::sample_commands()),
        }
        .with_link_opener(Arc::new(pane::SystemLinks));
        // Development builds can download npm packages from a registry on
        // this computer instead (the tests' and smokes' own); release builds
        // always use registry.npmjs.org.
        #[cfg(debug_assertions)]
        let launcher = match pane_core::npm::Registry::from_dev_env() {
            Some(Ok(registry)) => launcher.with_npm_registry(registry),
            Some(Err(why)) => {
                eprintln!("PANE_NPM_REGISTRY: {why}");
                launcher.show_error(format!("PANE_NPM_REGISTRY: {why}"));
                launcher
            }
            None => launcher,
        };
        // Pane's default extensions are acquired at first setup from Pane's
        // own downloads, which the installer carries none of. A release
        // build acquires them from Pane's published downloads; a development
        // build only where PANE_ARTIFACTS names a source on this computer
        // (the tests' and smokes' own), so that a development checkout
        // installs nothing over the network by itself.
        #[cfg(debug_assertions)]
        let artifact_source = pane_core::defaults::ArtifactSource::from_dev_env();
        #[cfg(not(debug_assertions))]
        let artifact_source: Option<Result<pane_core::defaults::ArtifactSource, String>> =
            Some(Ok(pane_core::defaults::ArtifactSource::published()));
        let launcher = match artifact_source.as_ref() {
            Some(Ok(source)) => launcher.with_defaults(source.clone(), pane::default_extensions()),
            Some(Err(why)) => {
                eprintln!("PANE_ARTIFACTS: {why}");
                launcher.show_error(format!("PANE_ARTIFACTS: {why}"));
                launcher
            }
            None => launcher,
        };
        // Pane's own update (#54 wired the Windows half, #55 the macOS
        // one, #56 the Linux one): the program this Pane runs from is the
        // one an update replaces - pane.exe in the install folder on
        // Windows, the Pane.app bundle's own binary (Contents/MacOS/pane)
        // on macOS, pane in ~/.local/bin on Linux - and the artifact
        // source the default extensions come from names the newer package
        // in its index, the package built for this system (a zip on
        // Windows and macOS, a gzipped tarball on Linux), unpacked by the
        // same platform-independent machinery. Pane checks once, at
        // start, and only the user's choice downloads and installs
        // anything.
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        let launcher = match (std::env::current_exe(), artifact_source.as_ref()) {
            (Ok(exe), Some(Ok(source))) => {
                launcher.with_application_update(pane::APP_VERSION, source.clone(), exe)
            }
            (Err(why), _) => {
                eprintln!(
                    "Pane's own program could not be found, so it checks for no update: {why}"
                );
                launcher
            }
            _ => launcher,
        };
        // Global hotkeys: the system's adapter is made on the main thread,
        // whose run loop receives the presses on macOS.
        let (press_sender, mut presses) = pane_core::hotkeys::channel();
        let launcher = launcher.with_hotkeys(pane_core::hotkeys::native(press_sender));
        // The tray or menu-bar entry: Pane's item in the system's tray
        // (Windows) or menu bar (macOS), whose menu opens the launcher,
        // Settings and Quit — the entry the General page's visibility
        // preference shows and hides, applied here from what the record
        // holds. The adapter is made on the main thread, as the hotkeys'
        // is; on a system whose entry cannot be made, the adapter says
        // why and the page explains.
        let (selection_sender, mut selections) = pane_core::tray::channel();
        let tray = pane_core::tray::native(selection_sender);
        pane::settings::attach_tray(tray.clone(), cx);
        // Quitting removes Pane's native tray/menu-bar entry and releases
        // its global hotkey registrations, whichever way Pane is quit —
        // closing the launcher's window or the tray's Quit item, which
        // does the same itself before it asks the platform to quit.
        let quitting = launcher.clone();
        let quitting_tray = tray.clone();
        cx.on_app_quit(move |_| {
            let _ = quitting_tray.set_visible(false);
            quitting.release_hotkeys();
            async {}
        })
        .detach();
        // Clipboard history: Pane watches the clipboard only while an
        // enabled package keeps history the user turned on.
        let launcher = launcher.with_clipboard(pane_core::clipboard::native());
        // Development mode builds with the author's tools; a JavaScript or
        // TypeScript package with this checkout's build unless
        // PANE_COMPONENTIZE_JS names another.
        let (change_sender, changes) = pane_core::changes::channel();
        let default_js = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/componentize-js/pane_js.py");
        let toolchains = Toolchains::from_env(Some(default_js));
        let launcher = launcher.with_development(Arc::new(toolchains), change_sender);
        // The window takes the launcher; acquiring the default extensions
        // and checking for Pane's own update keep clones, started below
        // once the window exists.
        let acquiring = launcher.clone();
        let checking = launcher.clone();
        // The window is the reference's launcher panel, at its client size
        // (see `pane::launcher_client_size`). No native title bar is drawn:
        // the panel's own glass chrome is the whole window. Its background
        // is the frost material's (acrylic behind the glass panel, opaque
        // otherwise).
        let bounds = Bounds::centered(None, pane::launcher_client_size(), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_background: pane::window_background(cx),
            titlebar: Some(TitlebarOptions {
                title: Some("Pane".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let window = cx
            .open_window(options, |window, cx| {
                // The window's own corners are rounded by the Desktop Window
                // Manager, so nothing shows behind the panel that fills it.
                #[cfg(target_os = "windows")]
                pane::prefer_rounded_window_corners(window);
                cx.new(|cx| {
                    let mut launcher = LauncherWindow::new(launcher, window, cx);
                    launcher.follow_changes(changes, window, cx);
                    match &preview {
                        Some(ToPreview::Folder(folder)) => {
                            launcher.preview_package(folder, window, cx)
                        }
                        Some(ToPreview::Npm(spec)) => launcher.preview_npm(spec, window, cx),
                        Some(ToPreview::Git(spec)) => launcher.preview_git(spec, window, cx),
                        None => {}
                    }
                    launcher
                })
            })
            .expect("failed to open the Pane window");
        // The opt-in native smoke of the HUD (scripts/smoke-windows-hud.ps1)
        // has a development build show one at once, over the application
        // in front.
        #[cfg(debug_assertions)]
        if let Ok(title) = std::env::var("PANE_TEST_SHOW_HUD") {
            window
                .update(cx, |launcher, window, cx| {
                    launcher.show_smoke_hud(title, window, cx)
                })
                .ok();
        }
        // Closing the launcher's own window quits Pane, as closing the one
        // window always did: closing the Settings window, which shares
        // nothing of the launcher's lifecycle, closes only that window,
        // and quitting ends Pane as before.
        let launcher_window = window.window_id();
        cx.on_window_closed(move |cx, closed| {
            if closed == launcher_window {
                cx.quit();
            }
        })
        .detach();
        // A hotkey pressed in any application opens its command here.
        cx.spawn(async move |cx| {
            while let Some(shortcut) = presses.next().await {
                let shown = window.update(cx, |launcher, window, cx| {
                    launcher.hotkey_pressed(&shortcut, window, cx)
                });
                if shown.is_err() {
                    break;
                }
            }
        })
        .detach();
        // A tray or menu-bar selection arrives here the same way: the
        // window's own dispatch runs it, whatever state the windows are
        // in — the launcher may be hidden, and the menu stays usable.
        cx.spawn(async move |cx| {
            while let Some(action) = selections.next().await {
                let shown = window.update(cx, |launcher, window, cx| {
                    launcher.tray_selected(action, window, cx)
                });
                if shown.is_err() {
                    break;
                }
            }
        })
        .detach();
        // Acquiring the default extensions goes on in the background: the
        // window, root search and Manage extensions stay usable, and the
        // status line says what it is doing (the changes channel redraws
        // the window as it goes, as for development builds).
        cx.spawn(async move |_| {
            acquiring.acquire_defaults().await;
        })
        .detach();
        // Checking for a Pane application update does too: it reads only
        // the artifact source's index, and what it finds is offered as a
        // row in root search the user chooses. (A Pane that wires no
        // updater checks for nothing.)
        cx.spawn(async move |_| {
            checking.check_application_update().await;
        })
        .detach();
        cx.activate(true);
    });
}
