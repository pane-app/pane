#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, prelude::*};
use pane::LauncherWindow;
use pane_core::develop::Toolchains;
use pane_core::{Launcher, Runtime, local_channel};

/// What `--install` asks to show for installation.
enum ToPreview {
    Folder(PathBuf),
    /// `npm:<name>` or `npm:<name>@<version>`.
    Npm(String),
    /// `git:<repository>[@<reference>]`, with `#<id>` naming one extension
    /// of a collection the repository holds.
    Git(String),
    /// One extension of a collection in a folder: `<folder>#<id>`.
    Collection(PathBuf, String),
}

/// `pane [--install <folder> | --install <folder>#<id> | --install
/// npm:<package>[@<version>] | --install git:<repository>[@<reference>]]`:
/// `--install` opens with the package in `<folder>` shown for installation,
/// as if chosen with the folder picker, the npm package, as if named in
/// "Install extension from npm…", or the Git repository, as if named in
/// "Install extension from Git…". A `#<id>` names one extension of a
/// collection (ADR 0044): a local folder's, after the last `#` of its path,
/// or a repository's, before any `@<reference>`; without one, a folder or
/// repository holding a collection opens the choice of its extensions
/// (#308). `pane --version` prints Pane's version and exits without opening
/// a window, so an installation can check what it installed.
/// `pane --launch-at-login` turns on Settings' "Launch Pane at login" as Pane
/// starts (see [`launch_at_login_asked`]).
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
            // A folder path may name one extension of a collection at it:
            // the part after the last `#` is the id (a folder name may hold
            // a `#` of its own, so an id is named after the last one).
            if let Some((folder, id)) = text.and_then(|text| text.rsplit_once('#')) {
                return Some(ToPreview::Collection(PathBuf::from(folder), id.to_owned()));
            }
            return Some(ToPreview::Folder(PathBuf::from(source)));
        }
    }
    None
}

/// Whether Pane was started with `--launch-at-login`: Pane's Windows setup
/// starts it so when the user ticks "Start Pane when I sign in" (#463), and
/// Pane then takes that choice as the General page's switch takes it — the
/// login registration and the settings record together — rather than the
/// setup registering Pane behind the setting's back, which the start-time
/// reconciliation would undo.
fn launch_at_login_asked() -> bool {
    std::env::args_os()
        .skip(1)
        .any(|arg| arg == "--launch-at-login")
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
    // Whether Pane runs from a Linux AppImage (#464), read before any
    // thread starts: the AppImage runtime's variables are then taken out
    // of the environment, so the programs Pane starts do not inherit them.
    // SAFETY: nothing but this thread runs yet.
    unsafe { pane_core::appimage::adopt() };
    let preview = package_to_preview();
    let launch_at_login = launch_at_login_asked();
    // Pane's own log and crash record (#133), before anything else can
    // write a diagnostic or panic: the log keeps what standard error says,
    // and the record tells whether the run before ended unexpectedly.
    let crash_record = pane::start_crash_record();
    let application = gpui_platform::application();
    // On macOS Pane is an accessory application, as menu-bar launchers
    // are (#458): no Dock icon and no Cmd+Tab entry, reached by its hotkey
    // and its menu-bar entry, its windows still activated when shown.
    // Set here, before launching finishes, rather than as `LSUIElement` in
    // the bundle's Info.plist, so a development build run from the
    // checkout behaves as the installed app does and no Dock icon shows
    // even for a moment.
    #[cfg(target_os = "macos")]
    let application = application.with_activation_policy(gpui::MacActivationPolicy::Accessory);
    application.run(move |cx: &mut App| {
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
        // The applications' icons kept from before are read now, off this
        // thread, so root search's first rows draw them at once (#172).
        launcher.prepare_application_icons();
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
        // on macOS, pane in ~/.local/bin on Linux - and the newest of
        // Pane's releases on GitHub carries the newer package, the package
        // built for this system (a zip on Windows and macOS, a gzipped
        // tarball on Linux), unpacked by the same platform-independent
        // machinery. Pane checks at start, then every four hours and when
        // the launcher is shown after a stale check, keeping what it learns
        // in its cache folder; it downloads and stages a newer version by
        // itself, and only the user's choice installs it (ADR 0050). A
        // development build can check more often (PANE_UPDATE_INTERVAL,
        // in seconds), as the smokes do. A release build reads
        // Pane's published releases; a development build only where
        // PANE_RELEASES names a stand-in on this computer (the tests' and
        // smokes' own), so that a development checkout checks nothing over
        // the network by itself. A Pane running from a Linux AppImage
        // replaces the AppImage file with the release's AppImage instead
        // (#464, ADR 0049): the program inside it is in a read-only mount.
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        let launcher = {
            #[cfg(debug_assertions)]
            let release_source = pane_core::ReleaseSource::from_dev_env();
            #[cfg(not(debug_assertions))]
            let release_source: Option<Result<pane_core::ReleaseSource, String>> =
                Some(Ok(pane_core::ReleaseSource::published()));
            match (
                pane_core::appimage::running(),
                std::env::current_exe(),
                release_source.as_ref(),
            ) {
                (Some(appimage), _, Some(Ok(source))) => launcher.with_appimage_update(
                    pane::APP_VERSION,
                    source.clone(),
                    appimage.to_path_buf(),
                ),
                (None, Ok(exe), Some(Ok(source))) => {
                    launcher.with_application_update(pane::APP_VERSION, source.clone(), exe)
                }
                (None, Err(why), _) => {
                    pane_core::diagnostic!(
                        "Pane's own program could not be found, so it checks for no update: {why}"
                    );
                    launcher
                }
                _ => launcher,
            }
        };
        let launcher = match pane::cache_dir() {
            Some(cache) => launcher.with_application_update_cache(&cache),
            None => launcher,
        };
        #[cfg(debug_assertions)]
        let launcher = match std::env::var("PANE_UPDATE_INTERVAL")
            .ok()
            .and_then(|seconds| seconds.trim().parse::<u64>().ok())
            .filter(|seconds| *seconds > 0)
        {
            Some(seconds) => {
                launcher.with_application_update_interval(std::time::Duration::from_secs(seconds))
            }
            None => launcher,
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
        // closing the launcher's window, or the tray's Quit item and root
        // search's Quit Pane, which do the same themselves before they ask
        // the platform to quit (`LauncherWindow::quit`) — and
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
        // TypeScript package with this checkout's componentizer (which
        // `cargo xtask guests` builds into target/guests/componentizer)
        // unless PANE_COMPONENTIZER names another, else the package's own
        // `@pane-app/cli` platform package.
        let (change_sender, changes) = pane_core::changes::channel();
        let componentizer =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/componentizer");
        let toolchains = Toolchains::from_env(Some(componentizer));
        let launcher = launcher.with_development(Arc::new(toolchains), change_sender);
        // As Raycast does, root search's results supplied ahead of the query
        // are asked for as soon as their commands can run - now, and once
        // first setup or an update installs one - rather than at the first
        // search: the installed applications are listed and their icons
        // extracted in the background, so even the first search after a
        // fresh install or an update draws real icons.
        let launcher = launcher.with_eager_indexed_results();
        // The window takes the launcher; acquiring the default extensions
        // and checking for Pane's own update keep clones, started below
        // once the window exists.
        let acquiring = launcher.clone();
        let checking = launcher.clone();
        let developing = launcher.clone();
        // The window is the reference's launcher panel, with the frost
        // material's background (acrylic behind the glass panel, opaque
        // otherwise), no native title bar, and no taskbar button or
        // window-switcher entry (see `pane::open_launcher_window`).
        let window = pane::open_launcher_window(cx, |window, cx| {
            cx.new(|cx| {
                let mut launcher = LauncherWindow::new(launcher, window, cx);
                launcher.follow_changes(changes, window, cx);
                match &preview {
                    Some(ToPreview::Folder(folder)) => launcher.preview_package(folder, window, cx),
                    Some(ToPreview::Collection(folder, id)) => {
                        launcher.preview_collection(folder, id, window, cx)
                    }
                    Some(ToPreview::Npm(spec)) => launcher.preview_npm(spec, window, cx),
                    Some(ToPreview::Git(spec)) => launcher.preview_git(spec, window, cx),
                    None => {}
                }
                launcher
            })
        })
        .expect("failed to open the Pane window");
        // The setup's "Start Pane when I sign in" (`--launch-at-login`),
        // taken once the window exists: its constructor attached the
        // launcher to the settings, resolving a fresh data folder's Open
        // Pane hotkey, which the choice's save then records.
        if launch_at_login {
            pane::settings::choose_launch_at_login(cx);
        }
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
        // and quitting ends Pane as before. On macOS the launcher refuses
        // its window's close request and hides instead (#439, see
        // `LauncherWindow::new`), so there only quitting closes it.
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
        // of a package Pane has not installed — the extension's own, where
        // it develops one extension of a collection (ADR 0044). It listens
        // where PANE_CHANNEL says, if it says, as pane-ext looks there: a
        // second Pane beside an installed one, and the tests.
        let endpoint = local_channel::Endpoint::from_env();
        match endpoint.and_then(|endpoint| local_channel::serve(developing, &endpoint)) {
            Ok((server, mut previews)) => {
                cx.spawn(async move |cx| {
                    // Pane listens for as long as it runs.
                    let _server = server;
                    while let Some(asked) = previews.next().await {
                        let shown = window.update(cx, |launcher, window, cx| match &asked {
                            local_channel::ToPreview::Folder(folder) => {
                                launcher.present_package(folder, window, cx)
                            }
                            local_channel::ToPreview::Collection(folder, id) => {
                                launcher.present_collection(folder, id, window, cx)
                            }
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
        // First setup (#283): on a data folder whose first setup has not
        // chosen, the launcher shows the official extensions with the
        // defaults ticked, and its Set Up row sets the ticked ones up;
        // once the choice is made, each default that is missing and not
        // declined is acquired by itself. Acquiring goes on in the
        // background: the window, root search and the extension list stay
        // usable, and the status line says what it is doing (the changes
        // channel redraws the window as it goes, as for development
        // builds).
        cx.spawn(async move |_| {
            acquiring.first_setup().await;
        })
        .detach();
        // Checking for a Pane application update does too, on a schedule
        // of its own: at once if a check is due, then every few hours. A
        // newer version is downloaded and staged quietly, and root search
        // lists the row that installs it once it is ready. (A Pane that
        // wires no updater checks for nothing.)
        checking.start_application_update_checks();
        cx.activate(true);
    });
}
