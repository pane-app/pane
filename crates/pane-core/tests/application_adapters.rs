//! Each system's application adapter. Finding applications is file system
//! work, checked on every system with fixture folders; opening one is checked
//! on its own system only, with a harmless application the test makes, which
//! writes a marker file when it runs. Nothing else is started.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pane_core::applications::{
    AppBundles, Applications, Catalog, DesktopEntries, Discovery, Key, Place, Shortcut,
    ShortcutFolder, ShortcutTarget, Source, StartMenu,
};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn names(found: &[Source]) -> Vec<&str> {
    found.iter().map(|source| source.name.as_str()).collect()
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
        .sources()
        .unwrap();

    assert_eq!(names(&found), ["My Editor", "Chess", "Viewer"]);
    assert_eq!(found[0].path, user.join("editor.desktop").to_string_lossy());
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

    let found = DesktopEntries::new(vec![apps]).sources().unwrap();

    assert_eq!(names(&found), ["With actions", "Shown"]);
}

#[test]
fn missing_folders_find_nothing_and_are_not_errors() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    assert_eq!(
        DesktopEntries::new(vec![missing.clone()]).sources(),
        Ok(vec![])
    );
    assert_eq!(AppBundles::new(vec![missing.clone()]).sources(), Ok(vec![]));
    assert_eq!(StartMenu::new(vec![missing]).sources(), Ok(vec![]));
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
        .sources()
        .unwrap();

    assert_eq!(
        names(&found),
        ["Safari", "Terminal", "Tool", "Xcode", "Mine"]
    );
    assert_eq!(
        found[0].path,
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

    // Read as fixtures: on Windows the shell reads an empty file as a
    // shortcut to nothing, which is not an application.
    let found = StartMenu::new(vec![user.clone(), everyone.clone()])
        .with_resolver(read_fixtures)
        .sources()
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

/// A shortcut reader for fixture files on every system: a file says what
/// it opens as `target|arguments`, optionally followed by `|` and the name
/// Explorer shows for it; an empty one is a shortcut the shell could not
/// read.
fn read_fixtures(paths: &[PathBuf]) -> Vec<Option<Shortcut>> {
    paths
        .iter()
        .map(|path| {
            let text = fs::read_to_string(path).ok()?;
            let mut fields = text.splitn(3, '|');
            let target = fields.next().unwrap_or_default();
            let arguments = fields.next().unwrap_or_default();
            let display_name = fields.next().map(str::to_owned);
            (!target.is_empty()).then(|| Shortcut {
                target: target.into(),
                arguments: arguments.into(),
                display_name,
                ..Shortcut::default()
            })
        })
        .collect()
}

#[test]
fn shortcuts_are_titled_as_explorer_shows_them_and_found_by_their_programs() {
    let dir = tempfile::tempdir().unwrap();
    let programs = dir.path().join("Programs");
    // Localized by its folder: the file's own name still finds it.
    write(
        &programs.join("Paint.lnk"),
        r"C:\Windows\System32\mspaint.exe||Ứng dụng Vẽ",
    );
    write(
        &programs.join("Visual Studio Code.lnk"),
        r"C:\VS Code\Code.exe|",
    );
    // Generic, and one passing arguments: no program name.
    write(&programs.join("Game.lnk"), r"C:\Games\One\Launcher.exe|");
    write(
        &programs.join("Mail.lnk"),
        r"C:\Browser\chrome.exe|--app-id=mail",
    );
    let menu = StartMenu::new(vec![programs]).with_resolver(read_fixtures);

    let catalog = Catalog::new(menu.sources().unwrap());

    let applications = catalog.applications();
    let named: Vec<(&str, Vec<&str>)> = applications
        .iter()
        .map(|app| {
            let alternates = app.alternate_titles.iter().map(String::as_str).collect();
            (app.name.as_str(), alternates)
        })
        .collect();
    assert_eq!(
        named,
        [
            ("Game", vec![]),
            ("Mail", vec![]),
            ("Ứng dụng Vẽ", vec!["Paint", "mspaint"]),
            ("Visual Studio Code", vec!["Code"]),
        ]
    );
}

#[test]
fn same_name_applications_found_by_an_adapter_are_told_apart() {
    let dir = tempfile::tempdir().unwrap();
    let programs = dir.path().join("Programs");
    write(
        &programs.join("Python 3.11/Python.lnk"),
        r"C:\Python311\python.exe|",
    );
    write(
        &programs.join("Python 3.12/Python.lnk"),
        r"C:\Python312\python.exe|",
    );
    write(&programs.join("Editor.lnk"), r"C:\Editor\editor.exe|");
    let menu = StartMenu::new(vec![programs]).with_resolver(read_fixtures);

    let applications = Catalog::new(menu.sources().unwrap()).applications();

    let distinctions: Vec<(&str, Option<&str>)> = applications
        .iter()
        .map(|app| (app.name.as_str(), app.distinction.as_deref()))
        .collect();
    assert_eq!(
        distinctions,
        [
            ("Editor", None),
            ("Python", Some("Python311")),
            ("Python", Some("Python312")),
        ]
    );
    // Their program's name is their title, and both have it: no alternate.
    assert!(applications[1].alternate_titles.is_empty());
}

#[test]
fn shortcuts_to_one_program_are_one_application_whatever_its_version_folder() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user/Programs");
    let everyone = dir.path().join("everyone/Programs");
    let chat = |version: &str| format!(r"C:\Users\Ann\AppData\Local\Chat\{version}\Chat.exe");
    write(&user.join("Chat.lnk"), &chat("app-1.0.1"));
    // The installer's own shortcut, to the program before its last update.
    write(&everyone.join("Vendor/Chat.lnk"), &chat("app-1.0.0"));
    // One program with different arguments: two applications.
    write(&user.join("Browser.lnk"), r"C:\Browser\browser.exe|");
    write(
        &user.join("Mail.lnk"),
        r"C:\Browser\browser.exe|--app-id=mail",
    );
    write(&user.join("Unreadable.lnk"), "");
    let menu = || StartMenu::new(vec![user.clone(), everyone.clone()]).with_resolver(read_fixtures);

    let catalog = Catalog::new(menu().sources().unwrap());

    let applications = catalog.applications();
    let titles: Vec<&str> = applications.iter().map(|app| app.name.as_str()).collect();
    assert_eq!(titles, ["Browser", "Chat", "Mail", "Unreadable"]);
    let chat_id = Key::program(&chat("app-9.9.9"), "").id();
    assert_eq!(applications[1].id, chat_id);
    // The user's own shortcut is the one Pane opens.
    let found = catalog.find(&chat_id).unwrap();
    assert_eq!(found.sources.len(), 2);
    assert_eq!(
        found.primary().path,
        user.join("Chat.lnk").to_string_lossy()
    );
    assert_ne!(applications[0].id, applications[2].id);
    assert_eq!(
        applications[3].id,
        Key::Path(user.join("Unreadable.lnk").to_string_lossy().to_lowercase()).id()
    );

    // The program updates into a new version folder and its shortcuts are
    // rewritten: after a restart, the application keeps its id.
    write(&user.join("Chat.lnk"), &chat("app-1.0.10"));
    write(&everyone.join("Vendor/Chat.lnk"), &chat("app-1.0.10"));
    let updated = Catalog::new(menu().sources().unwrap());
    assert_eq!(updated.applications()[1].id, chat_id);
}

#[test]
fn the_hosts_list_finds_an_application_by_its_id_and_by_its_path_from_before() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user/Programs");
    let everyone = dir.path().join("everyone/Programs");
    write(&user.join("Tool.lnk"), r"C:\Tool\tool.exe");
    write(&everyone.join("Tool.lnk"), r"C:\Tool\tool.exe");
    write(&everyone.join("Tools/Tool.lnk"), r"C:\Tool\tool.exe");
    let menu = StartMenu::new(vec![user.clone(), everyone.clone()]).with_resolver(read_fixtures);
    let host =
        pane_core::applications::Cached::new(std::sync::Arc::new(menu), Duration::from_secs(3600));
    let mine = user.join("Tool.lnk").to_string_lossy().into_owned();
    // As the adapter wrote it, separators and all.
    let installer = everyone
        .join("Tools")
        .join("Tool.lnk")
        .to_string_lossy()
        .into_owned();

    // Nothing is kept before the first listing.
    assert_eq!(host.current_id(&installer), None);
    let listed = host.installed().unwrap();

    assert_eq!(listed.len(), 1);
    let id = &listed[0].id;
    assert_eq!(host.source(id).as_deref(), Some(mine.as_str()));
    // A path that was the application's id before identities.
    assert_eq!(host.source(&installer).as_deref(), Some(mine.as_str()));
    assert_eq!(host.current_id(&installer).as_deref(), Some(id.as_str()));
    assert_eq!(host.current_id(id).as_deref(), Some(id.as_str()));
    assert_eq!(host.source("C:\\Gone.lnk"), None);
}

/// The folders Windows finds shortcuts in, below `root`, each in its place:
/// the Start menus walked into their subfolders, the Desktops and the
/// taskbar pins not.
fn windows_folders(root: &Path) -> Vec<ShortcutFolder> {
    let folder = |name: &str, place: Place, subfolders: bool| ShortcutFolder {
        path: root.join(name),
        place,
        subfolders,
    };
    vec![
        folder("user/Programs", Place::UserStartMenu, true),
        folder("everyone/Programs", Place::AllUsersStartMenu, true),
        folder("user/Desktop", Place::UserDesktop, false),
        folder("Public/Desktop", Place::AllUsersDesktop, false),
        folder("user/TaskBar", Place::TaskbarPins, false),
    ]
}

/// An internet shortcut to `url`, as Explorer writes them.
fn internet_shortcut(url: &str) -> String {
    format!(
        "[{{000214A0-0000-0000-C000-000000000046}}]\r\nProp3=19,0\r\n[InternetShortcut]\r\nIDList=\r\nURL={url}\r\nIconIndex=0\r\n"
    )
}

/// A ClickOnce application reference to `deployment`, in UTF-16 with its
/// byte order mark, as the ClickOnce installer writes them.
fn click_once_reference(path: &Path, deployment: &str) {
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(deployment.encode_utf16().flat_map(u16::to_le_bytes));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

#[test]
fn desktop_shortcuts_and_taskbar_pins_are_applications() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // On the user's Desktop and in their Start menu: one application, which
    // Pane opens by the Desktop's shortcut, as Explorer prefers it.
    write(&root.join("user/Desktop/My Tool.lnk"), r"C:\Tool\tool.exe");
    write(
        &root.join("user/Programs/Tool/Tool.lnk"),
        r"C:\Tool\tool.exe",
    );
    // Installed only with a shortcut on every user's Desktop.
    write(
        &root.join("Public/Desktop/Viewer.lnk"),
        r"C:\Viewer\viewer.exe",
    );
    // A folder on the Desktop is not looked into.
    write(
        &root.join("user/Desktop/Stuff/Inner.lnk"),
        r"C:\Inner\inner.exe",
    );
    // A portable program reachable only from the taskbar.
    write(
        &root.join("user/TaskBar/Portable.lnk"),
        r"D:\Portable\portable.exe",
    );

    let catalog = Catalog::new(
        StartMenu::with_folders(windows_folders(root))
            .with_resolver(read_fixtures)
            .sources()
            .unwrap(),
    );

    let mut titles: Vec<String> = catalog
        .applications()
        .into_iter()
        .map(|app| app.name)
        .collect();
    titles.sort();
    assert_eq!(titles, ["My Tool", "Portable", "Viewer"]);
    let tool = catalog
        .find(&Key::program(r"C:\Tool\tool.exe", "").id())
        .unwrap();
    assert_eq!(tool.sources.len(), 2);
    assert_eq!(
        tool.primary().path,
        root.join("user/Desktop")
            .join("My Tool.lnk")
            .to_string_lossy()
    );
    let portable = catalog
        .find(&Key::program(r"D:\Portable\portable.exe", "").id())
        .unwrap();
    assert_eq!(portable.primary().place, Place::TaskbarPins as usize);
}

#[test]
fn internet_shortcuts_with_a_handled_scheme_and_click_once_references_are_applications() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let game = "steam://rungameid/570";
    write(
        &root.join("user/Programs/Steam/Dota 2.url"),
        &internet_shortcut(game),
    );
    write(
        &root.join("user/Desktop/Dota 2.url"),
        &internet_shortcut(game),
    );
    // A scheme nothing handles, and a web page.
    write(
        &root.join("user/Desktop/Unknown.url"),
        &internet_shortcut("unknown-launcher://game/1"),
    );
    write(
        &root.join("user/Programs/Vendor/Web site.url"),
        &internet_shortcut("https://example.com/"),
    );
    let deployment = "http://apps.example.com/Orders/Orders.application#Orders.application, \
                      Culture=neutral, PublicKeyToken=0123456789abcdef, processorArchitecture=msil";
    click_once_reference(
        &root.join("user/Programs/Example/Orders.appref-ms"),
        deployment,
    );

    let menu = StartMenu::with_folders(windows_folders(root))
        .with_resolver(read_fixtures)
        .with_handlers(|scheme| matches!(scheme, "steam" | "https"));
    let catalog = Catalog::new(menu.sources().unwrap());

    let applications = catalog.applications();
    let titles: Vec<&str> = applications.iter().map(|app| app.name.as_str()).collect();
    assert_eq!(titles, ["Orders", "Dota 2"]);
    let game = catalog.find(&Key::Link(game.into()).id()).unwrap();
    // Found twice, listed once, opened by the Desktop's.
    assert_eq!(game.sources.len(), 2);
    assert_eq!(
        game.primary().path,
        root.join("user/Desktop")
            .join("Dota 2.url")
            .to_string_lossy()
    );
    assert_eq!(
        applications[0].id,
        Key::Link(deployment.to_lowercase()).id()
    );
}

#[test]
fn startup_shortcuts_uninstallers_broken_shortcuts_and_documents_are_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let programs = root.join("user/Programs");
    write(&programs.join("Tool/Tool.lnk"), r"C:\Tool\tool.exe");
    // Run at sign-in, not listed; a folder of that name deeper down is.
    write(&programs.join("Startup/Agent.lnk"), r"C:\Agent\agent.exe");
    write(
        &root.join("everyone/Programs/StartUp/Updater.lnk"),
        r"C:\Updater\updater.exe",
    );
    write(
        &programs.join("Vendor/Startup/Helper.lnk"),
        r"C:\Helper\helper.exe",
    );
    // Uninstallers, by name and by program.
    write(
        &programs.join("Tool/Uninstall Tool.lnk"),
        r"C:\Tool\uninstall.exe",
    );
    write(
        &programs.join("Tool/Remove Tool.lnk"),
        r"C:\Tool\unins000.exe",
    );
    // A broken shortcut, a folder and documents.
    write(&programs.join("Gone.lnk"), r"C:\Gone\gone.exe");
    write(&root.join("user/Desktop/Projects.lnk"), r"C:\Projects");
    write(&programs.join("Tool/Read me.lnk"), r"C:\Tool\readme.txt");
    write(&programs.join("Tool/Manual.lnk"), r"C:\Tool\manual.pdf");

    let menu = StartMenu::with_folders(windows_folders(root))
        .with_resolver(read_fixtures)
        .with_targets(|target| match target {
            r"C:\Gone\gone.exe" => ShortcutTarget::Missing,
            r"C:\Projects" => ShortcutTarget::Folder,
            _ => ShortcutTarget::File,
        });
    let found = menu.sources().unwrap();

    assert_eq!(names(&found), ["Tool", "Helper"]);
}

#[test]
fn the_sources_pane_does_not_have_on_macos_and_linux_are_stated() {
    // Users of macOS and Linux are told what root search covers there,
    // rather than finding Windows' extra sources silently missing.
    let docs = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/applications.md"),
    )
    .unwrap();
    let row = docs
        .lines()
        .find(|line| line.starts_with("| Not looked for |"))
        .expect("docs/applications.md has a \"Not looked for\" row");
    let cells: Vec<&str> = row.split('|').map(str::trim).collect();
    // | Not looked for | Windows | macOS | Linux |
    assert_eq!(cells.len(), 6, "{row}");
    for (system, cell) in [("macOS", cells[3]), ("Linux", cells[4])] {
        for source in ["Desktop", "Dock", "internet shortcuts"] {
            if system == "Linux" && source == "Dock" {
                continue;
            }
            assert!(
                cell.contains(source),
                "{system} does not state {source}: {cell}"
            );
        }
    }
}

#[test]
fn application_bundles_are_identified_by_their_bundle_identifier() {
    let dir = tempfile::tempdir().unwrap();
    let applications = dir.path().join("Applications");
    let user = dir.path().join("home/Applications");
    let info = |identifier: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
             <key>CFBundleIdentifier</key><string>{identifier}</string></dict></plist>\n"
        )
    };
    write(
        &applications.join("Editor.app/Contents/Info.plist"),
        &info("com.example.Editor"),
    );
    // Moved and renamed by the user: the same application.
    write(
        &user.join("My Editor.app/Contents/Info.plist"),
        &info("com.example.editor"),
    );
    fs::create_dir_all(applications.join("Plain.app")).unwrap();

    let found = AppBundles::new(vec![applications.clone(), user.clone()])
        .sources()
        .unwrap();

    assert_eq!(names(&found), ["Editor", "Plain", "My Editor"]);
    assert_eq!(found[0].key, Key::Bundle("com.example.editor".into()));
    // No identifier: identified by its path, as before.
    assert_eq!(
        found[1].key,
        Key::Path(
            applications
                .join("Plain.app")
                .to_string_lossy()
                .into_owned()
        )
    );
    let catalog = Catalog::new(found);
    assert_eq!(catalog.applications().len(), 2);
    // /Applications is preferred to ~/Applications.
    let editor = catalog
        .find(&Key::Bundle("com.example.editor".into()).id())
        .unwrap();
    assert_eq!(
        editor.primary().path,
        applications.join("Editor.app").to_string_lossy()
    );
}

#[test]
fn bundles_are_titled_by_their_display_name_and_found_by_their_folder_name() {
    let dir = tempfile::tempdir().unwrap();
    let applications = dir.path().join("Applications");
    fs::create_dir_all(applications.join("Calculator.app")).unwrap();
    fs::create_dir_all(applications.join("Notes.app")).unwrap();
    fs::create_dir_all(applications.join("Tool.app")).unwrap();
    // As Finder names them for a Vietnamese user (with every extension
    // shown, for Tool).
    let bundles = AppBundles::new(vec![applications]).with_display_names(|bundle| {
        match bundle.file_name()?.to_str()? {
            "Calculator.app" => Some("Máy tính".into()),
            "Tool.app" => Some("Tool.app".into()),
            _ => None,
        }
    });

    let catalog = Catalog::new(bundles.sources().unwrap());

    let applications = catalog.applications();
    let named: Vec<(&str, &[String])> = applications
        .iter()
        .map(|app| (app.name.as_str(), app.alternate_titles.as_slice()))
        .collect();
    assert_eq!(
        named,
        [
            ("Máy tính", &["Calculator".to_owned()][..]),
            ("Notes", &[][..]),
            ("Tool", &[][..]),
        ]
    );
}

#[test]
fn desktop_entries_are_named_and_found_for_the_user_s_locale() {
    let dir = tempfile::tempdir().unwrap();
    let apps = dir.path().join("applications");
    write(
        &apps.join("org.gnome.Nautilus.desktop"),
        "[Desktop Entry]\nType=Application\nName=Files\nName[vi]=Tệp\n\
         Keywords=folder;manager;\nKeywords[vi]=thư mục;\n\
         Exec=nautilus --new-window %U\n",
    );
    write(
        &apps.join("org.gnome.Terminal.desktop"),
        "[Desktop Entry]\nType=Application\nName=Terminal\n\
         Exec=/usr/bin/gnome-terminal\n",
    );
    // A web app: the browser's name is not its alternate title.
    write(
        &apps.join("mail.desktop"),
        "[Desktop Entry]\nType=Application\nName=Mail\n\
         Exec=chromium --profile-directory=Default --app-id=abc\n",
    );
    write(
        &apps.join("chromium.desktop"),
        "[Desktop Entry]\nType=Application\nName=Chromium Web Browser\n\
         Exec=chromium %U\n",
    );

    let vietnamese = DesktopEntries::new(vec![apps.clone()]).with_locale("vi_VN.UTF-8");
    let applications = Catalog::new(vietnamese.sources().unwrap()).applications();

    let found: Vec<(&str, Vec<&str>, Vec<&str>)> = applications
        .iter()
        .map(|app| {
            (
                app.name.as_str(),
                app.alternate_titles.iter().map(String::as_str).collect(),
                app.keywords.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("Chromium Web Browser", vec!["chromium"], vec![]),
            ("Mail", vec![], vec![]),
            ("Tệp", vec!["Files", "nautilus"], vec!["thư mục"]),
            ("Terminal", vec!["gnome-terminal"], vec![]),
        ]
    );

    // Without a locale: the plain values.
    let plain = Catalog::new(DesktopEntries::new(vec![apps]).sources().unwrap()).applications();
    assert_eq!(plain[2].name, "Files");
    assert_eq!(plain[2].keywords, ["folder", "manager"]);
}

#[test]
fn desktop_entries_are_identified_by_their_desktop_file_id() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user/applications");
    let system = dir.path().join("system/applications");
    write(&user.join("editor.desktop"), &entry("My Editor", ""));
    write(&system.join("games/chess.desktop"), &entry("Chess", ""));

    let found = DesktopEntries::new(vec![user, system]).sources().unwrap();

    assert_eq!(
        found.iter().map(|source| &source.key).collect::<Vec<_>>(),
        [
            &Key::DesktopFile("editor.desktop".into()),
            &Key::DesktopFile("games-chess.desktop".into())
        ]
    );
    assert_eq!(
        Catalog::new(found).applications()[1].id,
        Key::DesktopFile("games-chess.desktop".into()).id()
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
        assert_eq!(names(&entries.sources().unwrap()), ["Marker"]);

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
        let native = pane_core::applications::native();
        let found = native.installed().unwrap();
        let calculator = found
            .iter()
            .find(|app| app.id == Key::Bundle("com.apple.calculator".into()).id())
            .expect("Calculator is in /System/Applications");
        // Titled as Finder shows it, in the user's language; its folder's
        // name finds it too when that differs.
        assert!(
            calculator.name == "Calculator"
                || calculator
                    .alternate_titles
                    .iter()
                    .any(|name| name == "Calculator"),
            "{calculator:?}"
        );
        assert!(!calculator.name.ends_with(".app"), "{calculator:?}");
        // Identified by its bundle identifier, and opened by its bundle.
        assert_eq!(
            calculator.id,
            Key::Bundle("com.apple.calculator".into()).id(),
            "{calculator:?}"
        );
        let bundle = native.source(&calculator.id).unwrap();
        assert!(bundle.ends_with("Calculator.app"), "{bundle}");
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
        let found = bundles.sources().unwrap();
        assert_eq!(names(&found), ["Pane Marker"]);
        assert_eq!(found[0].key, Key::Bundle(identifier.to_lowercase()));

        bundles.open(&found[0].path).unwrap();

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

    /// `path`, an existing folder, by its long names: what
    /// `fs::canonicalize` resolves, without its `\\?\` prefix.
    fn long_path(path: &Path) -> PathBuf {
        let resolved = fs::canonicalize(path).unwrap();
        let text = resolved.to_string_lossy();
        PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
    }

    /// A shortcut to `target` passing `arguments`, made as the shell makes
    /// them.
    fn shortcut_to(path: &Path, target: &Path, arguments: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let script = format!(
            "$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{}'); \
             $s.TargetPath = '{}'; $s.Arguments = '{}'; $s.Save()",
            path.display(),
            target.display(),
            arguments
        );
        let status = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .status()
            .unwrap();
        assert!(status.success(), "the shortcut was not made");
    }

    #[test]
    fn shortcuts_to_a_program_in_two_version_folders_are_one_application() {
        let dir = tempfile::tempdir().unwrap();
        // The folder by its long name: a runner's temporary folder is
        // under an 8.3 short name (`C:\Users\RUNNER~1\…`), and the shell
        // reads a shortcut's target back by its long names.
        let root = long_path(dir.path());
        let cmd = PathBuf::from(std::env::var("SystemRoot").unwrap()).join(r"System32\cmd.exe");
        let program = |version: &str| root.join(format!(r"Chat\{version}\chat.exe"));
        for version in ["app-1.0.0", "app-1.0.1"] {
            fs::create_dir_all(program(version).parent().unwrap()).unwrap();
            fs::copy(&cmd, program(version)).unwrap();
        }
        let programs = root.join("Programs");
        shortcut_to(&programs.join("Chat.lnk"), &program("app-1.0.0"), "");
        shortcut_to(
            &programs.join("Chat (updated).lnk"),
            &program("app-1.0.1"),
            "",
        );
        shortcut_to(
            &programs.join("Chat work.lnk"),
            &program("app-1.0.1"),
            "--profile work",
        );

        let catalog = Catalog::new(StartMenu::new(vec![programs.clone()]).sources().unwrap());

        let applications = catalog.applications();
        let titles: Vec<&str> = applications.iter().map(|app| app.name.as_str()).collect();
        assert_eq!(titles, ["Chat", "Chat work"]);
        let id = Key::program(&program("app-2.0.0").to_string_lossy(), "").id();
        assert_eq!(applications[0].id, id);
        assert_eq!(
            catalog.find(&id).unwrap().primary().path,
            programs.join("Chat.lnk").to_string_lossy()
        );
        assert_ne!(applications[1].id, id);
    }

    #[test]
    fn a_folder_s_localized_names_title_its_shortcuts_as_explorer_shows_them() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = PathBuf::from(std::env::var("SystemRoot").unwrap()).join(r"System32\cmd.exe");
        let programs = dir.path().join("Programs");
        shortcut_to(&programs.join("Tool.lnk"), &cmd, "");
        shortcut_to(&programs.join("Plain.lnk"), &cmd, "/k");
        // The folder's own localized names, in UTF-16 as Windows writes
        // them; Explorer reads them from a system folder's hidden, system
        // `desktop.ini`.
        let ini = "[LocalizedFileNames]\r\nTool.lnk=Công cụ\r\n";
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(ini.encode_utf16().flat_map(u16::to_le_bytes));
        fs::write(programs.join("desktop.ini"), bytes).unwrap();
        let attrib = |arguments: &[&str], path: &Path| {
            let status = Command::new("attrib")
                .args(arguments)
                .arg(path)
                .status()
                .unwrap();
            assert!(status.success(), "attrib failed on {}", path.display());
        };
        attrib(&["+s", "+h"], &programs.join("desktop.ini"));
        attrib(&["+s"], &programs);

        let applications =
            Catalog::new(StartMenu::new(vec![programs.clone()]).sources().unwrap()).applications();

        let tool = applications
            .iter()
            .find(|app| app.name == "Công cụ")
            .unwrap_or_else(|| panic!("no localized title among {applications:?}"));
        assert!(
            tool.alternate_titles.iter().any(|name| name == "Tool"),
            "{tool:?}"
        );
        assert!(applications.iter().any(|app| app.name == "Plain"));
        // The folder is the test's own: it goes with its attributes.
        attrib(&["-s"], &programs);
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
        let found = menu.sources().unwrap();
        assert_eq!(names(&found), ["Pane Marker"]);

        menu.open(&found[0].path).unwrap();

        assert_eq!(wait_for(&marker).trim(), "launched");
    }

    #[test]
    fn the_desktops_and_taskbar_pins_are_looked_in_without_their_subfolders() {
        let menu = StartMenu::from_env();
        let places: Vec<(Place, bool)> = menu
            .folders()
            .iter()
            .map(|folder| (folder.place, folder.subfolders))
            .collect();
        assert_eq!(
            places,
            [
                (Place::UserStartMenu, true),
                (Place::AllUsersStartMenu, true),
                (Place::UserDesktop, false),
                (Place::AllUsersDesktop, false),
                (Place::TaskbarPins, false),
            ]
        );
        let taskbar = &menu.folders()[4].path;
        assert!(
            taskbar.ends_with(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar"),
            "{}",
            taskbar.display()
        );
    }

    /// A URL scheme registered for the current user while it is held, as a
    /// game launcher registers its own, removed when dropped.
    struct Scheme(String);

    impl Scheme {
        fn register(name: &str) -> Scheme {
            let key = format!(r"HKCU\Software\Classes\{name}");
            let reg = |args: &[&str]| {
                let status = Command::new("reg").args(args).status().unwrap();
                assert!(status.success(), "reg {args:?} failed");
            };
            reg(&["add", &key, "/ve", "/d", "URL:Pane test", "/f"]);
            reg(&["add", &key, "/v", "URL Protocol", "/d", "", "/f"]);
            reg(&[
                "add",
                &format!(r"{key}\shell\open\command"),
                "/ve",
                "/d",
                r#""C:\Windows\System32\cmd.exe" /c exit "%1""#,
                "/f",
            ]);
            Scheme(key)
        }
    }

    impl Drop for Scheme {
        fn drop(&mut self) {
            let _ = Command::new("reg").args(["delete", &self.0, "/f"]).status();
        }
    }

    #[test]
    fn an_internet_shortcut_is_found_when_its_scheme_has_a_registered_handler() {
        let registered = format!("pane-test-{}", std::process::id());
        let unregistered = format!("pane-unregistered-{}", std::process::id());
        let _scheme = Scheme::register(&registered);
        let dir = tempfile::tempdir().unwrap();
        let programs = dir.path().join("Programs");
        let game = format!("{registered}://rungameid/570");
        write(&programs.join("Game.url"), &internet_shortcut(&game));
        write(
            &programs.join("Other.url"),
            &internet_shortcut(&format!("{unregistered}://game/1")),
        );
        write(
            &programs.join("Web site.url"),
            &internet_shortcut("https://example.com/"),
        );

        let found = StartMenu::new(vec![programs]).sources().unwrap();

        assert_eq!(names(&found), ["Game"]);
        assert_eq!(found[0].key, Key::Link(game.to_lowercase()));
    }

    #[test]
    fn a_click_once_reference_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let programs = dir.path().join("Programs");
        let deployment = "http://apps.example.com/Orders/Orders.application#Orders.application, \
                          Culture=neutral, PublicKeyToken=0123456789abcdef, \
                          processorArchitecture=msil";
        click_once_reference(&programs.join("Example/Orders.appref-ms"), deployment);

        let found = StartMenu::new(vec![programs]).sources().unwrap();

        assert_eq!(names(&found), ["Orders"]);
        assert_eq!(found[0].key, Key::Link(deployment.to_lowercase()));
    }

    #[test]
    fn broken_shortcuts_folders_documents_and_uninstallers_on_this_disk_are_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = PathBuf::from(std::env::var("SystemRoot").unwrap()).join(r"System32\cmd.exe");
        let program = dir.path().join(r"Tool\tool.exe");
        let gone = dir.path().join(r"Gone\gone.exe");
        let uninstaller = dir.path().join(r"Tool\unins000.exe");
        let document = dir.path().join(r"Tool\readme.txt");
        for copy in [&program, &gone, &uninstaller] {
            fs::create_dir_all(copy.parent().unwrap()).unwrap();
            fs::copy(&cmd, copy).unwrap();
        }
        write(&document, "read me");
        let desktop = dir.path().join("Desktop");
        shortcut_to(&desktop.join("Tool.lnk"), &program, "");
        shortcut_to(&desktop.join("Gone.lnk"), &gone, "");
        shortcut_to(&desktop.join("Remove Tool.lnk"), &uninstaller, "");
        shortcut_to(&desktop.join("Read me.lnk"), &document, "");
        shortcut_to(&desktop.join("Projects.lnk"), &dir.path().join("Tool"), "");
        // The program the shortcut opens is removed: the shortcut is broken.
        fs::remove_file(&gone).unwrap();

        let menu = StartMenu::with_folders(vec![ShortcutFolder {
            path: desktop,
            place: Place::UserDesktop,
            subfolders: false,
        }]);
        let found = menu.sources().unwrap();

        assert_eq!(names(&found), ["Tool"]);
    }

    #[test]
    fn this_systems_start_menu_and_packaged_apps_are_listed() {
        let found = pane_core::applications::StartMenu::from_env()
            .sources()
            .unwrap();
        // Inbox packaged apps (Calculator on Windows 11, Settings on Windows
        // Server) have no Start menu shortcut; the Apps folder finds them,
        // identified by their package family, which is Microsoft's: a Store
        // app's (`8wekyb3d8bbwe`) or a system app's (`cw5n1h2txyewy`).
        let packaged: Vec<&Source> = found
            .iter()
            .filter(|source| source.path.starts_with(r"shell:AppsFolder\"))
            .collect();
        let inbox = packaged
            .iter()
            .find(|source| source.name == "Calculator" || source.name == "Settings")
            .unwrap_or_else(|| panic!("no inbox packaged app among {packaged:?}"));
        assert!(
            matches!(&inbox.key, Key::Package { family, .. }
                if family.ends_with("_8wekyb3d8bbwe") || family.ends_with("_cw5n1h2txyewy")),
            "{inbox:?}"
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

/// Each adapter's watcher, on whichever system runs the tests (the native
/// file watcher: ReadDirectoryChangesW, FSEvents or inotify): a source
/// made in a watched temporary folder is reported within two seconds, and
/// a folder that does not exist yet is watched for. Nothing outside the
/// test's own folders is watched.
mod watching {
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    use pane_core::applications::{
        AppBundles, Change, Changes, DesktopEntries, Discovery, Place, ShortcutFolder, StartMenu,
    };

    use super::write;

    /// Where a watch reports, and what it reported.
    fn sink() -> (Changes, mpsc::Receiver<Change>) {
        let (send, reported) = mpsc::channel();
        let send = Mutex::new(send);
        let changes: Changes = Arc::new(move |change| {
            let _ = send.lock().unwrap().send(change);
        });
        (changes, reported)
    }

    /// The first change reported, within two seconds.
    fn within_two_seconds(reported: &mpsc::Receiver<Change>) -> Change {
        reported
            .recv_timeout(Duration::from_secs(2))
            .expect("the watcher reported nothing within two seconds")
    }

    #[test]
    fn a_shortcut_made_in_a_watched_start_menu_folder_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let programs = dir.path().join("Programs");
        let desktop = dir.path().join("Desktop");
        std::fs::create_dir_all(programs.join("Suite")).unwrap();
        std::fs::create_dir_all(&desktop).unwrap();
        let menu = StartMenu::with_folders(vec![
            ShortcutFolder {
                path: programs.clone(),
                place: Place::UserStartMenu,
                subfolders: true,
            },
            ShortcutFolder {
                path: desktop.clone(),
                place: Place::UserDesktop,
                subfolders: false,
            },
        ]);
        let (changes, reported) = sink();
        let watch = menu.watch(changes).unwrap();
        assert!(watch.complete());

        // In a subfolder of a folder whose subfolders are looked into.
        write(&programs.join("Suite/Editor.lnk"), "");
        assert_eq!(within_two_seconds(&reported), Change::Changed);
        while reported.recv_timeout(Duration::from_millis(300)).is_ok() {}

        write(&desktop.join("Mail.lnk"), "");
        assert_eq!(within_two_seconds(&reported), Change::Changed);
    }

    #[test]
    fn an_application_folder_that_does_not_exist_yet_is_watched_for() {
        let dir = tempfile::tempdir().unwrap();
        let applications = dir.path().join("share/applications");
        let entries = DesktopEntries::new(vec![applications.clone()]);
        let (changes, reported) = sink();
        let watch = entries.watch(changes).unwrap();
        assert!(!watch.complete(), "it does not watch the folder itself yet");

        std::fs::create_dir_all(&applications).unwrap();

        assert_eq!(within_two_seconds(&reported), Change::Changed);
    }

    #[test]
    fn a_desktop_entry_made_in_a_watched_applications_folder_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let applications = dir.path().join("applications");
        std::fs::create_dir_all(applications.join("vendor")).unwrap();
        let entries = DesktopEntries::new(vec![applications.clone()]);
        let (changes, reported) = sink();
        let watch = entries.watch(changes).unwrap();
        assert!(watch.complete());

        write(
            &applications.join("vendor/editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=editor\n",
        );

        assert_eq!(within_two_seconds(&reported), Change::Changed);
    }

    #[test]
    fn a_bundle_made_in_a_watched_applications_folder_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let applications = dir.path().join("Applications");
        std::fs::create_dir_all(&applications).unwrap();
        let bundles = AppBundles::new(vec![applications.clone()]);
        let (changes, reported) = sink();
        let watch = bundles.watch(changes).unwrap();
        assert!(watch.complete());

        std::fs::create_dir_all(applications.join("Editor.app/Contents")).unwrap();

        assert_eq!(within_two_seconds(&reported), Change::Changed);
    }
}
