//! Each system's application adapter. Finding applications is file system
//! work, checked on every system with fixture folders; opening one is checked
//! on its own system only, with a harmless application the test makes, which
//! writes a marker file when it runs. Nothing else is started.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pane_core::applications::{AppBundles, Application, Applications, DesktopEntries, StartMenu};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn names(found: &[Application]) -> Vec<&str> {
    found.iter().map(|app| app.name.as_str()).collect()
}

/// Waits for the application the test opened to write `marker`.
#[allow(dead_code)]
fn wait_for(marker: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(text) = fs::read_to_string(marker)
            && !text.is_empty()
        {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "the application did not run: {} was not written",
            marker.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn entry(name: &str, extra: &str) -> String {
    format!("[Desktop Entry]\nType=Application\nName={name}\nExec=true\n{extra}")
}

#[test]
fn desktop_entries_are_found_in_the_first_folder_that_has_their_id() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user/applications");
    let system = dir.path().join("system/applications");
    write(&user.join("editor.desktop"), &entry("My Editor", ""));
    write(&system.join("editor.desktop"), &entry("Editor", ""));
    // Removed by the user: a hidden entry hides the system's.
    write(&user.join("mail.desktop"), &entry("Mail", "Hidden=true"));
    write(&system.join("mail.desktop"), &entry("Mail", ""));
    // Its id is `games-chess.desktop`, whichever folder it is in.
    write(&system.join("games/chess.desktop"), &entry("Chess", ""));
    write(
        &system.join("games-chess.desktop"),
        &entry("Other chess", ""),
    );
    write(
        &system.join("viewer.desktop"),
        &entry("Viewer", "# a comment\n"),
    );

    let found = DesktopEntries::new(vec![user.clone(), system.clone()])
        .installed()
        .unwrap();

    assert_eq!(names(&found), ["My Editor", "Chess", "Viewer"]);
    assert_eq!(found[0].id, user.join("editor.desktop").to_string_lossy());
    assert_eq!(found[0].location, user.display().to_string());
    assert_eq!(
        found[1].location,
        system.join("games").display().to_string()
    );
}

#[test]
fn desktop_entries_a_launcher_does_not_show_are_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let apps = dir.path().join("applications");
    write(&apps.join("shown.desktop"), &entry("Shown", ""));
    write(
        &apps.join("nodisplay.desktop"),
        &entry("Settings panel", "NoDisplay=true"),
    );
    write(
        &apps.join("link.desktop"),
        "[Desktop Entry]\nType=Link\nName=Web site\nURL=https://example.com\n",
    );
    write(
        &apps.join("noexec.desktop"),
        "[Desktop Entry]\nType=Application\nName=No program\n",
    );
    write(
        &apps.join("gnome.desktop"),
        &entry("GNOME only", "OnlyShowIn=GNOME;"),
    );
    write(
        &apps.join("tryexec.desktop"),
        &entry("Missing", "TryExec=/nonexistent/pane-test-program"),
    );
    write(&apps.join("other.txt"), &entry("Not an entry", ""));
    // Field codes used against the specification: skipped, not guessed.
    write(
        &apps.join("badexec.desktop"),
        &entry("Bad exec", "").replace("Exec=true", "Exec=viewer %f%u"),
    );
    // Keys of other groups are not the entry's.
    write(
        &apps.join("actions.desktop"),
        "[Desktop Entry]\nType=Application\nName=With actions\nExec=true\n\
         [Desktop Action new]\nName=New window\nNoDisplay=true\n",
    );

    let found = DesktopEntries::new(vec![apps]).installed().unwrap();

    assert_eq!(names(&found), ["With actions", "Shown"]);
}

#[test]
fn missing_folders_find_nothing_and_are_not_errors() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    assert_eq!(
        DesktopEntries::new(vec![missing.clone()]).installed(),
        Ok(vec![])
    );
    assert_eq!(
        AppBundles::new(vec![missing.clone()]).installed(),
        Ok(vec![])
    );
    assert_eq!(StartMenu::new(vec![missing]).installed(), Ok(vec![]));
}

#[test]
fn application_bundles_are_found_in_the_folders_and_their_subfolders() {
    let dir = tempfile::tempdir().unwrap();
    let applications = dir.path().join("Applications");
    let user = dir.path().join("home/Applications");
    fs::create_dir_all(applications.join("Safari.app/Contents/MacOS")).unwrap();
    // A helper inside a bundle is not an application of its own.
    fs::create_dir_all(applications.join("Xcode.app/Contents/Applications/Helper.app")).unwrap();
    fs::create_dir_all(applications.join("Utilities/Terminal.app")).unwrap();
    fs::create_dir_all(applications.join("Vendor/Suite/Tool.app")).unwrap();
    fs::create_dir_all(applications.join("a/b/c/Too Deep.app")).unwrap();
    write(&applications.join("Notes.app"), "a file, not a bundle");
    fs::create_dir_all(user.join("Mine.app")).unwrap();

    let found = AppBundles::new(vec![applications.clone(), user.clone()])
        .installed()
        .unwrap();

    assert_eq!(
        names(&found),
        ["Safari", "Terminal", "Tool", "Xcode", "Mine"]
    );
    assert_eq!(
        found[0].id,
        applications.join("Safari.app").to_string_lossy()
    );
    assert_eq!(
        found[1].location,
        applications.join("Utilities").display().to_string()
    );
}

#[test]
fn start_menu_shortcuts_are_found_once_without_uninstallers() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user/Programs");
    let everyone = dir.path().join("everyone/Programs");
    write(&user.join("Editor.lnk"), "");
    write(&user.join("Tools/Paint.LNK"), "");
    write(&everyone.join("tools/paint.lnk"), "");
    write(&everyone.join("Editor Help.lnk"), "");
    write(&everyone.join("Suite/Uninstall Suite.lnk"), "");
    write(&everyone.join("Suite/Suite.lnk"), "");
    write(&everyone.join("Web site.url"), "");
    write(&everyone.join("desktop.ini"), "");

    let found = StartMenu::new(vec![user.clone(), everyone.clone()])
        .installed()
        .unwrap();

    assert_eq!(names(&found), ["Editor", "Paint", "Editor Help", "Suite"]);
    assert_eq!(found[1].location, user.join("Tools").display().to_string());
}

#[test]
fn opening_something_that_is_not_an_application_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("notes.txt");
    write(&text, "");
    let text = text.to_string_lossy().into_owned();
    let gone = dir
        .path()
        .join("gone.desktop")
        .to_string_lossy()
        .into_owned();
    let entries = DesktopEntries::new(vec![]);
    assert_eq!(
        entries.open(&text),
        Err(format!("{text} is not a desktop entry"))
    );
    assert_eq!(entries.open(&gone), Err(format!("{gone} no longer exists")));
    assert_eq!(
        AppBundles::new(vec![]).open(&text),
        Err(format!("{text} is not an application bundle"))
    );
    assert_eq!(
        StartMenu::new(vec![]).open(&text),
        Err(format!("{text} is not a Start menu shortcut"))
    );
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    fn desktop_file(dir: &Path, name: &str, text: &str) -> String {
        let path = dir.join("applications").join(name);
        write(&path, text);
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_desktop_entry_runs_its_program() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("launched by pane");
        let exec = format!("sh -c \"echo %c > '{}'\" %U", marker.display());
        let id = desktop_file(
            dir.path(),
            "marker.desktop",
            &format!("[Desktop Entry]\nType=Application\nName=Marker\nExec={exec}\n"),
        );
        let entries = DesktopEntries::new(vec![dir.path().join("applications")]);
        assert_eq!(names(&entries.installed().unwrap()), ["Marker"]);

        entries.open(&id).unwrap();

        assert_eq!(wait_for(&marker), "Marker\n");
    }

    #[test]
    fn a_desktop_entry_that_cannot_run_is_explained() {
        let dir = tempfile::tempdir().unwrap();
        let entries = DesktopEntries::new(vec![]);
        let missing = desktop_file(
            dir.path(),
            "missing.desktop",
            &entry("Missing", "").replace("Exec=true", "Exec=pane-no-such-program --flag"),
        );
        assert_eq!(
            entries.open(&missing),
            Err("cannot find the program pane-no-such-program".into())
        );
        // Which terminal emulator runs a `Terminal=true` entry depends on
        // what is installed; choosing one is unit tested.
        let invalid = desktop_file(
            dir.path(),
            "invalid.desktop",
            &entry("Invalid", "").replace("Exec=true", "Exec=viewer --icon=%i"),
        );
        assert_eq!(
            entries.open(&invalid),
            Err("its Exec key uses %i inside an argument (--icon=%i)".into())
        );
    }

    #[test]
    fn this_systems_desktop_entries_can_be_listed() {
        // Whatever is installed; the folders exist or are skipped.
        pane_core::applications::native().installed().unwrap();
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn the_systems_own_applications_are_found() {
        let found = pane_core::applications::native().installed().unwrap();
        let calculator = found
            .iter()
            .find(|app| app.name == "Calculator")
            .expect("Calculator is in /System/Applications");
        assert!(calculator.id.ends_with("Calculator.app"), "{calculator:?}");
    }

    #[test]
    fn a_bundle_is_opened_by_launch_services() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("launched by pane");
        let bundle = dir.path().join("Applications/Pane Marker.app");
        let identifier = format!("dev.pane.test.marker{}", std::process::id());
        write(
            &bundle.join("Contents/Info.plist"),
            &format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
                 \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                 <plist version=\"1.0\"><dict>\
                 <key>CFBundleExecutable</key><string>marker</string>\
                 <key>CFBundleIdentifier</key><string>{identifier}</string>\
                 <key>CFBundleName</key><string>Pane Marker</string>\
                 <key>CFBundlePackageType</key><string>APPL</string>\
                 </dict></plist>\n"
            ),
        );
        let program = bundle.join("Contents/MacOS/marker");
        write(
            &program,
            &format!("#!/bin/sh\necho launched > '{}'\n", marker.display()),
        );
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let bundles = AppBundles::new(vec![dir.path().join("Applications")]);
        let found = bundles.installed().unwrap();
        assert_eq!(names(&found), ["Pane Marker"]);

        bundles.open(&found[0].id).unwrap();

        assert_eq!(wait_for(&marker), "launched\n");
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::process::Command;

    /// A Start menu shortcut to `cmd.exe` running `command`, made as the
    /// shell makes them.
    fn shortcut(path: &Path, command: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let script = format!(
            "$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{}'); \
             $s.TargetPath = \"$env:SystemRoot\\System32\\cmd.exe\"; \
             $s.Arguments = '/c {}'; $s.WindowStyle = 7; $s.Save()",
            path.display(),
            command
        );
        let status = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .status()
            .unwrap();
        assert!(status.success(), "the shortcut was not made");
    }

    #[test]
    fn a_shortcut_is_opened_by_the_shell() {
        let dir = tempfile::tempdir().unwrap();
        let marker: PathBuf = dir.path().join("launched by pane.txt");
        let programs = dir.path().join("Programs");
        shortcut(
            &programs.join("Pane Marker.lnk"),
            &format!("echo launched> \"{}\"", marker.display()),
        );
        let menu = StartMenu::new(vec![programs]);
        let found = menu.installed().unwrap();
        assert_eq!(names(&found), ["Pane Marker"]);

        menu.open(&found[0].id).unwrap();

        assert_eq!(wait_for(&marker).trim(), "launched");
    }

    #[test]
    fn this_systems_start_menu_and_packaged_apps_are_listed() {
        let found = pane_core::applications::StartMenu::from_env()
            .installed()
            .unwrap();
        // Inbox packaged apps (Calculator on Windows 11, Settings on Windows
        // Server) have no Start menu shortcut; the Apps folder finds them.
        let packaged: Vec<&Application> = found
            .iter()
            .filter(|app| app.id.starts_with(r"shell:AppsFolder\"))
            .collect();
        assert!(
            packaged
                .iter()
                .any(|app| app.name == "Calculator" || app.name == "Settings"),
            "no inbox packaged app among {packaged:?}"
        );
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn a_bundle_is_opened_only_on_macos() {
    let dir = tempfile::tempdir().unwrap();
    let bundle = dir.path().join("Tool.app");
    fs::create_dir_all(&bundle).unwrap();
    let error = AppBundles::new(vec![])
        .open(&bundle.to_string_lossy())
        .unwrap_err();
    assert!(error.ends_with("Pane opens those only on macOS"), "{error}");
}

#[cfg(not(windows))]
#[test]
fn a_shortcut_is_opened_only_on_windows() {
    let dir = tempfile::tempdir().unwrap();
    let shortcut: PathBuf = dir.path().join("Tool.lnk");
    write(&shortcut, "");
    let error = StartMenu::new(vec![])
        .open(&shortcut.to_string_lossy())
        .unwrap_err();
    assert!(
        error.ends_with("Pane opens those only on Windows"),
        "{error}"
    );
}
