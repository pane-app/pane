#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*};
use pane::LauncherWindow;
use pane_core::develop::Toolchains;
use pane_core::{Launcher, Runtime, local_channel};

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

/// For the native smoke of the system functions: reveals the file
/// `PANE_TEST_REVEAL` names and moves the one `PANE_TEST_TRASH` names to the
/// Recycle Bin, as a command's `reveal` and `trash` do, and writes what each
/// answered to `log` ("reveal: ok", "trash: <why not>").
#[cfg(debug_assertions)]
fn smoke_system(log: PathBuf) {
    let system = pane_core::system::native();
    let mut answers = String::new();
    if let Some(path) = std::env::var_os("PANE_TEST_REVEAL") {
        let answer = system
            .reveal(std::path::Path::new(&path))
            .err()
            .unwrap_or_else(|| "ok".into());
        answers.push_str(&format!("reveal: {answer}\n"));
    }
    if let Some(path) = std::env::var_os("PANE_TEST_TRASH") {
        let not_moved = system.trash(&[PathBuf::from(path)]);
        let answer = not_moved
            .first()
            .map_or_else(|| "ok".to_owned(), |not| not.reason.clone());
        answers.push_str(&format!("trash: {answer}\n"));
    }
    if let Err(error) = std::fs::write(&log, answers) {
        pane_core::diagnostic!("PANE_TEST_SYSTEM_LOG: {error}");
    }
}

fn main() {
    // Pane's own program serves as the selected-text worker (#262) when
    // it is started with the internal argument: checked before anything
    // else, so the worker starts no window, no runtime, no settings and
    // no tray — only the UI Automation reads it is asked for, ending when
    // Pane ends it. The argument is Pane's own, never shown and never
    // parsed as the user's interface.
    #[cfg(windows)]
    if std::env::args_os()
        .skip(1)
        .any(|arg| arg == pane_core::system::selected::WORKER_ARGUMENT)
    {
        pane_core::system::selected::serve();
    }
    let preview = package_to_preview();
    // Pane's own log and crash record (#133), before anything else can
    // write a diagnostic or panic: the log keeps what standard error says,
    // and the record tells whether the run before ended unexpectedly.
    let crash_record = pane::start_crash_record();
    gpui_platform::application().run(move |cx: &mut App| {
        // Pane's own settings — the appearance preferences recorded in
        // settings.json, with the PANE_THEME/PANE_MATERIAL development
        // overrides winning for this process — and the embedded Geist
        // fonts, before the key bindings: the bindings the Keyboard page
        // recorded are registered from the record the settings hold, so
        // a saved rebind is in force from the first window. A font
        // failure only falls back to the system's default font.
        if let Err(error) = pane::configure_visuals(cx) {
            pane_core::diagnostic!("Pane's fonts could not be loaded: {error:#}");
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
        // data folder (#101). Pane registers no command of its build's
        // own: every root row comes from an installed package (the
        // samples are installed by hand, #162).
        let launcher = match pane::data_dir() {
            Some(dir) => Launcher::with_packages(runtime, Vec::new(), dir.join("extensions"))
                .with_quick_slots(&dir),
            None => Launcher::new(runtime, Vec::new()),
        }
        .with_link_opener(Arc::new(pane::SystemLinks))
        // What commands copy, open, reveal and recycle reaches the system's
        // own clipboard, handlers, file manager and Recycle Bin (#145).
        .with_system(pane_core::system::native())
        // What commands run through the Run dialog's work reaches the
        // shell, Windows' elevation prompt and the Run dialog's own
        // history (#254).
        .with_run(pane_core::run::native())
        // What commands lock, log out, restart, shut down, sleep, hibernate,
        // turn the displays off of and start the screen saver of reaches the
        // system's own session and power (#255).
        .with_system_commands(pane_core::system_commands::native())
        // What commands list of the open windows and which one they bring
        // to the front reaches the system's own windows (#263).
        .with_switch_windows(pane_core::switch_windows::native());
        // That Pane quit unexpectedly last time, told in root search and on
        // the About page; a clean quit removes this run's marker.
        let launcher = match crash_record.clone() {
            Some(record) => launcher.with_crash_record(record),
            None => launcher,
        };
        // File search's index of the home folder (#126, #175), kept in
        // Pane's cache folder; it runs only while an enabled extension uses
        // it, and never indexes Pane's own folders.
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .filter(|home| !home.is_empty())
            .map(PathBuf::from);
        let cache = pane::cache_dir();
        // The native smokes index a fixture folder of their own instead of
        // the home folder, keeping the index in their own data folder;
        // nothing else sets this, and a release build has no such hook.
        #[cfg(debug_assertions)]
        let (cache, home) = match std::env::var_os("PANE_TEST_FILE_INDEX_HOME") {
            Some(fixture) => (pane::data_dir(), Some(PathBuf::from(fixture))),
            None => (cache, home),
        };
        let launcher = match (cache, home) {
            (Some(cache), Some(home)) => {
                let own = pane::data_dir().into_iter().collect();
                launcher.with_file_index(pane_core::file_index::IndexerConfig::native(
                    &cache, home, own,
                ))
            }
            _ => launcher,
        };
        // Development builds can download npm packages from a registry on
        // this computer instead (the tests' and smokes' own); release builds
        // always use registry.npmjs.org.
        #[cfg(debug_assertions)]
        let launcher = match pane_core::npm::Registry::from_dev_env() {
            Some(Ok(registry)) => launcher.with_npm_registry(registry),
            Some(Err(why)) => {
                pane_core::diagnostic!("PANE_NPM_REGISTRY: {why}");
                launcher.show_error(format!("PANE_NPM_REGISTRY: {why}"));
                launcher
            }
            None => launcher,
        };
        // Pane's default extensions are set up at first setup from the
        // commits this release pins, each fetched from its own repository
        // with Pane's own Git client (ADR 0021, ADR 0045); the installer
        // carries none of them. The committed pins name them; a development
        // build can replace them with a file of its own through
        // PANE_DEFAULTS (the tests' and smokes' own, pointing at
        // repositories served on this computer), so that a development
        // checkout reaches no real Git host unless it chooses to. A
        // release build has no override.
        #[cfg(debug_assertions)]
        let pins = match pane_core::defaults::pins_from_dev_env() {
            Some(Ok(pins)) => pins,
            Some(Err(why)) => {
                pane_core::diagnostic!("PANE_DEFAULTS: {why}");
                launcher.show_error(format!("PANE_DEFAULTS: {why}"));
                pane::default_extensions()
            }
            None => pane::default_extensions(),
        };
        #[cfg(not(debug_assertions))]
        let pins = pane::default_extensions();
        let launcher = launcher.with_defaults(pins);
        // Pane's own update (#54 wired the Windows half, #55 the macOS
        // one, #56 the Linux one): the program this Pane runs from is the
        // one an update replaces - pane.exe in the install folder on
        // Windows, the Pane.app bundle's own binary (Contents/MacOS/pane)
        // on macOS, pane in ~/.local/bin on Linux - and the artifact
        // source names the newer package in its index, the package built
        // for this system (a zip on Windows and macOS, a gzipped tarball
        // on Linux), unpacked by the same platform-independent machinery.
        // Pane checks once, at start, and only the user's choice downloads
        // and installs anything. A release build reads Pane's published
        // downloads; a development build only where PANE_ARTIFACTS names a
        // source on this computer (the tests' and smokes' own), so that a
        // development checkout checks nothing over the network by itself.
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        let launcher = {
            #[cfg(debug_assertions)]
            let artifact_source = pane_core::defaults::ArtifactSource::from_dev_env();
            #[cfg(not(debug_assertions))]
            let artifact_source: Option<
                Result<pane_core::defaults::ArtifactSource, String>,
            > = Some(Ok(pane_core::defaults::ArtifactSource::published()));
            match (std::env::current_exe(), artifact_source.as_ref()) {
                (Ok(exe), Some(Ok(source))) => {
                    launcher.with_application_update(pane::APP_VERSION, source.clone(), exe)
                }
                (Err(why), _) => {
                    pane_core::diagnostic!(
                        "Pane's own program could not be found, so it checks for no update: {why}"
                    );
                    launcher
                }
                _ => launcher,
            }
        };
        // Global hotkeys: the system's adapter is made on the main thread,
        // whose run loop receives the presses on macOS.
        let (press_sender, mut presses) = pane_core::hotkeys::channel();
        let launcher = launcher.with_hotkeys(pane_core::hotkeys::native(press_sender));
        // Game mode's foreground source (#125): the system's own
        // foreground event hook on Windows, which the launcher
        // subscribes to, deciding on each window that comes to the front
        // whether a game is in it — so Pane's hotkeys pause and return by
        // themselves while game mode is on. Everywhere else there is
        // none: game mode is Windows only, and the Keyboard page says so.
        let launcher = match pane_core::game_mode::native() {
            Some(source) => launcher.with_foreground(source),
            None => launcher,
        };
        // The taskbar while the launcher is open (#268): Windows' adapter
        // — which shows a taskbar that hides itself and puts it back as
        // the user had it — or none, and the General page explains the
        // choice where the platform has no taskbar of the kind.
        pane::settings::attach_taskbar(pane::taskbar::native(), cx);
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
        // does the same itself before it asks the platform to quit — and
        // it is a clean quit, whose marker goes (#133): GPUI runs these
        // hooks for the system ending the session too (`WM_ENDSESSION` on
        // Windows, the termination notification on macOS).
        let quitting = launcher.clone();
        let quitting_tray = tray.clone();
        cx.on_app_quit(move |_| {
            let _ = quitting_tray.set_visible(false);
            quitting.release_hotkeys();
            quitting.quit_cleanly();
            async {}
        })
        .detach();
        // On Linux and macOS, SIGTERM, SIGINT and SIGHUP (the session's
        // end, Ctrl+C in a terminal) quit cleanly too, on a thread of their
        // own, before Pane ends: what clipboard history keeps waiting in a
        // batch is written (#192).
        let signalled = launcher.clone();
        pane_core::diagnostics::quit_cleanly_on_signals(move || signalled.quit_cleanly());
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
        let developing = launcher.clone();
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
        // The opt-in native smoke of revealing and the Recycle Bin
        // (scripts/smoke-windows-system.ps1) has a development build reveal
        // one file and recycle another at once, through the system
        // functions commands use (#145), and write what each answered.
        #[cfg(debug_assertions)]
        if let Some(log) = std::env::var_os("PANE_TEST_SYSTEM_LOG") {
            std::thread::spawn(move || smoke_system(PathBuf::from(log)));
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
        // `pane-ext dev` hands its builds to this Pane over the local
        // channel (#217), which asks the window to show the install preview
        // of a folder Pane has not installed. It listens where PANE_CHANNEL
        // says, if it says, as pane-ext looks there: a second Pane beside an
        // installed one, and the tests.
        let endpoint = local_channel::Endpoint::from_env();
        match endpoint.and_then(|endpoint| local_channel::serve(developing, &endpoint)) {
            Ok((server, mut previews)) => {
                cx.spawn(async move |cx| {
                    // Pane listens for as long as it runs.
                    let _server = server;
                    while let Some(folder) = previews.next().await {
                        let shown = window.update(cx, |launcher, window, cx| {
                            launcher.present_package(&folder, window, cx)
                        });
                        if shown.is_err() {
                            break;
                        }
                    }
                })
                .detach();
            }
            Err(error) => pane_core::diagnostic!("pane-ext cannot reach this Pane: {error}"),
        }
        // Acquiring the default extensions goes on in the background: the
        // window, root search and the extension list stay usable, and the
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
