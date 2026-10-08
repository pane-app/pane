//! The installed applications' own icons (#172) through the launcher's
//! public interface, with the real Applications guest
//! (`target/guests/packages/applications`), the host's list of applications
//! over a fake system, and a fake extraction whose icons, fingerprints and
//! failures the tests decide: a row shows its application's own icon, and a
//! neutral placeholder of the same kind until it is extracted; a packaged
//! app keeps its light and dark icons; the cache in Pane's cache folder
//! draws them at once after a restart and refreshes each once per start; a
//! changed source is extracted again; a failure keeps the placeholder and is
//! not tried again that start; an unreadable cache is rebuilt; the cache is
//! bounded, the least recently drawn going first; rows on screen go before
//! the background; a pinned application's slot shows its icon; disabling
//! the extension stops the refresh; and an extension's own list shows an
//! application's icon by the reference the import gives. The extraction
//! adapters are `application_icon_adapters.rs`; the pure rules are unit
//! tests of `pane_core::applications::icons`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use futures::executor::block_on;
use pane_core::applications::icons::{BATCH, Extracted, FOLDER, IconCache, IconExtractor};
use pane_core::applications::{Applications, Cached, Discovery, Key, Source};
use pane_core::icons::encode_png;
use pane_core::system_icons::SystemIcon;
use pane_core::{
    Icon, IconSource, Launcher, PackageIdentity, ResultAction, Runtime, Screen, SlotChange, Tint,
    Tone,
};
use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

use rows::{select_title, titles};

/// How long the worker is given to extract what the tests ask.
const PATIENCE: Duration = Duration::from_secs(20);

fn built(path: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(path);
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

/// A system whose applications are shortcuts the tests list.
#[derive(Default)]
struct FakeSystem {
    sources: Mutex<Vec<Source>>,
}

impl Discovery for FakeSystem {
    fn sources(&self) -> Result<Vec<Source>, String> {
        Ok(self.sources.lock().unwrap().clone())
    }

    fn open(&self, _path: &str) -> Result<(), String> {
        Ok(())
    }
}

/// The Start menu shortcut `name` to `target`.
fn shortcut(name: &str, target: &str) -> Source {
    Source::new(
        Key::program(target, ""),
        format!(r"C:\Menu\{name}.lnk"),
        name,
        r"C:\Menu",
        2,
    )
}

/// A `side` × `side` PNG of one colour.
fn png(side: u32, rgba: [u8; 4]) -> Vec<u8> {
    let pixels: Vec<u8> = (0..side * side).flat_map(|_| rgba).collect();
    encode_png(side, side, &pixels).unwrap()
}

/// What the fake system draws for a source.
#[derive(Clone)]
struct Drawing {
    light: Vec<u8>,
    dark: Option<Vec<u8>>,
}

/// The applications' icons as the tests set them up: each source's
/// drawing and fingerprint, what was extracted, and a gate that holds
/// every extraction while closed.
#[derive(Default)]
struct FakeIcons {
    drawings: Mutex<HashMap<String, Drawing>>,
    fingerprints: Mutex<HashMap<String, String>>,
    extracted: Mutex<Vec<String>>,
    closed: Mutex<bool>,
    opened: Condvar,
}

impl FakeIcons {
    fn new() -> Arc<FakeIcons> {
        Arc::new(FakeIcons::default())
    }

    fn draw(&self, source: &str, light: Vec<u8>, dark: Option<Vec<u8>>) {
        self.drawings
            .lock()
            .unwrap()
            .insert(source.to_owned(), Drawing { light, dark });
    }

    fn fingerprint_of(&self, source: &str, fingerprint: &str) {
        self.fingerprints
            .lock()
            .unwrap()
            .insert(source.to_owned(), fingerprint.to_owned());
    }

    fn extracted(&self) -> Vec<String> {
        self.extracted.lock().unwrap().clone()
    }

    fn close(&self) {
        *self.closed.lock().unwrap() = true;
    }

    fn open(&self) {
        *self.closed.lock().unwrap() = false;
        self.opened.notify_all();
    }
}

impl IconExtractor for FakeIcons {
    fn fingerprint(&self, source: &str) -> Option<String> {
        Some(
            self.fingerprints
                .lock()
                .unwrap()
                .get(source)
                .cloned()
                .unwrap_or_else(|| "first".into()),
        )
    }

    fn extract(&self, source: &str) -> Result<Extracted, String> {
        let mut closed = self.closed.lock().unwrap();
        while *closed {
            closed = self.opened.wait(closed).unwrap();
        }
        drop(closed);
        self.extracted.lock().unwrap().push(source.to_owned());
        let drawing = self
            .drawings
            .lock()
            .unwrap()
            .get(source)
            .cloned()
            .ok_or_else(|| format!("the system has no icon for {source}"))?;
        Ok(Extracted {
            light: SystemIcon::Png(drawing.light),
            dark: drawing.dark.map(SystemIcon::Png),
        })
    }
}

/// Pane's data folder and cache folder, kept across restarts.
struct Dirs {
    data: TempDir,
    cache: TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            data: tempfile::tempdir().unwrap(),
            cache: tempfile::tempdir().unwrap(),
        }
    }

    /// The folder of the applications' icons in the cache folder.
    fn icons(&self) -> PathBuf {
        self.cache.path().join(FOLDER)
    }

    /// A launcher, started afresh, listing `system`'s applications, its
    /// icons drawn by `icons` and kept in the cache folder.
    fn launcher(&self, system: &Arc<FakeSystem>, icons: &Arc<FakeIcons>) -> Launcher {
        let runtime = Runtime::start_with_cache(self.cache.path().to_path_buf()).unwrap();
        runtime.set_applications(Arc::new(Cached::new(
            system.clone(),
            Duration::from_secs(3600),
        )));
        Launcher::with_packages(Ok(runtime), vec![], self.data.path().join("extensions"))
            .with_quick_slots(self.data.path())
            .with_application_icons(self.icons(), icons.clone())
    }
}

/// A launcher whose system has the applications `sources`, with the
/// Applications package installed.
fn installed(dirs: &Dirs, sources: Vec<Source>, icons: &Arc<FakeIcons>) -> Launcher {
    let system = Arc::new(FakeSystem {
        sources: Mutex::new(sources),
    });
    let launcher = dirs.launcher(&system, icons);
    install(&launcher, &built("packages/applications"));
    launcher
}

fn install(launcher: &Launcher, folder: &Path) {
    block_on(launcher.install_package(folder));
    while !matches!(launcher.view().screen, Screen::Root { .. }) {
        launcher.back();
    }
}

fn search(launcher: &Launcher, query: &str) {
    block_on(launcher.set_query(query));
}

/// Comes back to root search afresh, as a reopened window does, and types
/// `query` again: the commands' results are asked for anew. (Applications
/// is a root provider, #164, with no command of its own to open and leave.)
fn search_again(launcher: &Launcher, query: &str) {
    launcher.show_root_search();
    search(launcher, query);
}

fn settle(launcher: &Launcher) {
    assert!(
        launcher.wait_for_application_icons(PATIENCE),
        "the application icons are still being extracted"
    );
}

/// The icon root search's row titled `title` shows now.
fn row_icon(launcher: &Launcher, title: &str) -> Icon {
    let (view, presentation) = launcher.presented_view();
    let at = view
        .rows
        .iter()
        .position(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(launcher)));
    presentation.rows[at]
        .icon
        .clone()
        .unwrap_or_else(|| panic!("the row {title:?} has no icon"))
}

/// Whether `icon` is the placeholder an application's row shows while its
/// icon is not there: the application glyph, faded, without a tile.
fn is_placeholder(icon: &Icon) -> bool {
    matches!(&icon.source, IconSource::Builtin { name, .. } if name == "category")
        && icon.tint == Some(Tint::Same(pane_core::Color::Tone(Tone::Secondary)))
}

/// The light and dark files `icon` draws, when it draws a kept image.
fn files(icon: &Icon) -> Option<(PathBuf, PathBuf)> {
    match &icon.source {
        IconSource::Image { light, dark } => Some((light.clone(), dark.clone())),
        _ => None,
    }
}

/// Waits until the row titled `title` draws a kept image.
fn drawn(launcher: &Launcher, title: &str) -> (PathBuf, PathBuf) {
    let deadline = std::time::Instant::now() + PATIENCE;
    loop {
        if let Some(found) = files(&row_icon(launcher, title)) {
            return found;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the row {title:?} never drew its icon"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

const EDITOR: &str = r"C:\Program Files\Editor\editor.exe";
const EDITOR_LINK: &str = r"C:\Menu\Editor.lnk";

#[test]
fn an_applications_row_shows_its_own_icon_and_a_placeholder_until_it_is_extracted() {
    let dirs = Dirs::new();
    let icons = FakeIcons::new();
    let blue = png(256, [0, 0, 255, 255]);
    icons.draw(EDITOR_LINK, blue.clone(), None);
    icons.close();
    let launcher = installed(&dirs, vec![shortcut("Editor", EDITOR)], &icons);

    search(&launcher, "editor");

    // The row keeps its place with the placeholder: no tile, no tooltip.
    let waiting = row_icon(&launcher, "Editor");
    assert!(is_placeholder(&waiting), "{waiting:?}");
    assert!(waiting.is_decorative());
    let (view, presentation) = launcher.presented_view();
    let at = view
        .rows
        .iter()
        .position(|row| row.title == "Editor")
        .unwrap();
    assert_eq!(
        presentation.rows[at].kind,
        Some(pane_core::RowKind::Application)
    );

    icons.open();
    let (light, dark) = drawn(&launcher, "Editor");
    assert_eq!(fs::read(&light).unwrap(), blue);
    assert_eq!(light, dark, "one icon for both themes");
    assert!(light.starts_with(dirs.icons()), "{}", light.display());
    let shown = row_icon(&launcher, "Editor");
    assert!(shown.is_decorative(), "read by its title and subtitle only");
    // Extracted from the application's primary source, off the window.
    assert_eq!(icons.extracted(), [EDITOR_LINK]);
}

#[test]
fn a_packaged_apps_light_and_dark_icons_are_both_kept() {
    let dirs = Dirs::new();
    let icons = FakeIcons::new();
    let (on_light, on_dark) = (png(64, [20, 20, 20, 255]), png(64, [240, 240, 240, 255]));
    let calculator = r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App";
    icons.draw(calculator, on_light.clone(), Some(on_dark.clone()));
    let packaged = Source::new(
        Key::Package {
            family: "microsoft.windowscalculator_8wekyb3d8bbwe".into(),
            app: None,
        },
        calculator,
        "Calculator",
        "Apps folder (packaged apps)",
        5,
    );
    // A Start menu shortcut to the same app is its primary source; the
    // icon is still the package's own, with its variants.
    let link = Source {
        path: r"C:\Menu\Calculator.lnk".into(),
        location: r"C:\Menu".into(),
        place: 2,
        ..packaged.clone()
    };
    let launcher = installed(&dirs, vec![link, packaged], &icons);

    search(&launcher, "calculator");

    let (light, dark) = drawn(&launcher, "Calculator");
    assert_eq!(fs::read(light).unwrap(), on_light);
    assert_eq!(fs::read(dark).unwrap(), on_dark);
    assert_eq!(icons.extracted(), [calculator]);
}

#[test]
fn icons_are_drawn_from_the_cache_after_a_restart_and_refreshed_once_each_start() {
    let dirs = Dirs::new();
    let icons = FakeIcons::new();
    let green = png(256, [0, 200, 0, 255]);
    icons.draw(EDITOR_LINK, green.clone(), None);
    {
        let launcher = installed(&dirs, vec![shortcut("Editor", EDITOR)], &icons);
        search(&launcher, "editor");
        drawn(&launcher, "Editor");
        settle(&launcher);
    }
    assert_eq!(icons.extracted().len(), 1);

    // Pane starts again: the kept icon draws while the refresh waits.
    icons.close();
    let system = Arc::new(FakeSystem {
        sources: Mutex::new(vec![shortcut("Editor", EDITOR)]),
    });
    let launcher = dirs.launcher(&system, &icons);
    search(&launcher, "editor");
    let (light, _) = drawn(&launcher, "Editor");
    assert_eq!(fs::read(&light).unwrap(), green);
    assert_eq!(icons.extracted().len(), 1, "drawn without extracting");

    // The refresh after this start extracts it once, however often root
    // search lists it.
    icons.open();
    settle(&launcher);
    search_again(&launcher, "edit");
    settle(&launcher);
    assert_eq!(icons.extracted().len(), 2, "{:?}", icons.extracted());
}

#[test]
fn a_changed_source_is_extracted_again_at_once() {
    let dirs = Dirs::new();
    let icons = FakeIcons::new();
    icons.draw(EDITOR_LINK, png(256, [10, 10, 10, 255]), None);
    let old = {
        let launcher = installed(&dirs, vec![shortcut("Editor", EDITOR)], &icons);
        search(&launcher, "editor");
        let (light, _) = drawn(&launcher, "Editor");
        settle(&launcher);
        light
    };

    // The application updated: its shortcut changed, and so did its icon.
    let updated = png(256, [250, 100, 0, 255]);
    icons.draw(EDITOR_LINK, updated.clone(), None);
    icons.fingerprint_of(EDITOR_LINK, "second");
    let system = Arc::new(FakeSystem {
        sources: Mutex::new(vec![shortcut("Editor", EDITOR)]),
    });
    let launcher = dirs.launcher(&system, &icons);
    search(&launcher, "editor");
    // Shown on screen: extracted at once, never the old icon.
    let deadline = std::time::Instant::now() + PATIENCE;
    let light = loop {
        if let Some((light, _)) = files(&row_icon(&launcher, "Editor"))
            && light != old
        {
            break light;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never extracted again"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(fs::read(&light).unwrap(), updated);
    settle(&launcher);
    assert!(!old.exists(), "the old icon is removed");
}

#[test]
fn a_failed_extraction_keeps_the_placeholder_and_is_not_tried_again_this_start() {
    let dirs = Dirs::new();
    let icons = FakeIcons::new();
    // The fake system draws nothing for the shortcut.
    let launcher = installed(&dirs, vec![shortcut("Editor", EDITOR)], &icons);

    search(&launcher, "editor");
    row_icon(&launcher, "Editor");
    settle(&launcher);
    assert!(is_placeholder(&row_icon(&launcher, "Editor")));
    search_again(&launcher, "editor");
    row_icon(&launcher, "Editor");
    settle(&launcher);

    assert!(is_placeholder(&row_icon(&launcher, "Editor")));
    assert_eq!(icons.extracted(), [EDITOR_LINK], "tried once this start");
}

#[test]
fn an_unreadable_cache_is_deleted_and_rebuilt() {
    let dirs = Dirs::new();
    fs::create_dir_all(dirs.icons()).unwrap();
    fs::write(dirs.icons().join("index.json"), b"{ not the index").unwrap();
    fs::write(dirs.icons().join("stray.png"), b"left behind").unwrap();
    let icons = FakeIcons::new();
    let red = png(256, [200, 0, 0, 255]);
    icons.draw(EDITOR_LINK, red.clone(), None);
    let launcher = installed(&dirs, vec![shortcut("Editor", EDITOR)], &icons);

    search(&launcher, "editor");
    let (light, _) = drawn(&launcher, "Editor");
    settle(&launcher);

    assert_eq!(fs::read(light).unwrap(), red);
    assert!(!dirs.icons().join("stray.png").exists());
    let index = fs::read(dirs.icons().join("index.json")).unwrap();
    assert!(
        serde_json::from_slice::<serde_json::Value>(&index).is_ok(),
        "the index is written again"
    );
}

/// An icon cache in `folder` over `icons`, whose applications' sources are
/// their ids.
fn cache(folder: &Path, icons: &Arc<FakeIcons>) -> IconCache {
    IconCache::new(
        folder.to_path_buf(),
        icons.clone(),
        Arc::new(|id: &str| Some(id.to_owned())),
        Arc::new(|| {}),
    )
}

#[test]
fn the_cache_is_bounded_and_the_least_recently_drawn_go_first() {
    let folder = tempfile::tempdir().unwrap();
    let icons = FakeIcons::new();
    for id in ["a", "b", "c"] {
        icons.draw(id, png(16, [1, 2, 3, 255]), None);
    }
    let cache = cache(folder.path(), &icons).with_limits(u64::MAX, 2);
    cache.listed(["a".to_owned(), "b".to_owned()]);
    assert!(cache.wait_idle(PATIENCE));
    // "a" is drawn since; "b" is not (its file is read from the index,
    // which does not draw it).
    let a = cache.shown("a").expect("a is kept");
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(folder.path().join("index.json")).unwrap()).unwrap();
    let b = folder
        .path()
        .join(index["icons"]["b"]["light"].as_str().expect("b is kept"));
    assert!(b.exists());
    cache.listed(["a".to_owned(), "b".to_owned(), "c".to_owned()]);
    assert!(cache.wait_idle(PATIENCE));

    assert!(cache.shown("a").is_some());
    assert!(cache.shown("c").is_some());
    assert!(a.light.exists());
    assert!(!b.exists(), "the least recently drawn is removed");

    // Bounded by bytes too: one icon's worth keeps one.
    let folder = tempfile::tempdir().unwrap();
    let bytes = png(16, [1, 2, 3, 255]).len() as u64;
    let cache = self::cache(folder.path(), &icons).with_limits(bytes, 100);
    cache.listed(["a".to_owned(), "b".to_owned()]);
    assert!(cache.wait_idle(PATIENCE));
    let kept = fs::read_dir(folder.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name() != "index.json")
        .count();
    assert_eq!(kept, 1);
}

#[test]
fn rows_on_screen_are_extracted_before_the_background_refresh() {
    let folder = tempfile::tempdir().unwrap();
    let icons = FakeIcons::new();
    let ids: Vec<String> = (0..20).map(|index| format!("app{index:02}")).collect();
    for id in &ids {
        icons.draw(id, png(16, [9, 9, 9, 255]), None);
    }
    icons.close();
    let cache = cache(folder.path(), &icons);
    cache.listed(ids.clone());
    // A row on screen shows the last application listed.
    assert_eq!(cache.shown("app19"), None);
    icons.open();
    assert!(cache.wait_idle(PATIENCE));

    let order = icons.extracted();
    assert_eq!(order.len(), ids.len(), "{order:?}");
    let at = order.iter().position(|id| id == "app19").unwrap();
    // At most the batch already taken goes before it.
    assert!(at <= BATCH, "{order:?}");
    assert!(cache.shown("app19").is_some());
}

#[test]
fn a_pinned_applications_slot_shows_its_icon() {
    let dirs = Dirs::new();
    let icons = FakeIcons::new();
    let purple = png(256, [120, 0, 200, 255]);
    icons.draw(EDITOR_LINK, purple.clone(), None);
    let launcher = installed(&dirs, vec![shortcut("Editor", EDITOR)], &icons);
    search(&launcher, "editor");
    select_title(&launcher, "Editor");
    let target = launcher.view().rows[launcher.view().selected.unwrap()]
        .id
        .clone();
    let (change, recorded) = launcher.change_quick_slots(&target, ResultAction::Pin);
    assert!(matches!(change, SlotChange::Changed(_)), "{change:?}");
    block_on(recorded);
    drawn(&launcher, "Editor");

    let slot = launcher.quick_slots().remove(0);
    let icon = launcher
        .icon_of(&slot.target.key())
        .expect("the slot's icon");
    let (light, _) = files(&icon).expect("the application's own icon");
    assert_eq!(fs::read(light).unwrap(), purple);
}

#[test]
fn disabling_the_applications_extension_stops_refreshing_their_icons() {
    let dirs = Dirs::new();
    let icons = FakeIcons::new();
    let sources: Vec<Source> = (0..40)
        .map(|index| {
            shortcut(
                &format!("Tool {index:02}"),
                &format!(r"C:\Tools\tool{index}.exe"),
            )
        })
        .collect();
    for source in &sources {
        icons.draw(&source.path, png(16, [5, 5, 5, 255]), None);
    }
    icons.close();
    let launcher = installed(&dirs, sources, &icons);
    search(&launcher, "tool");

    let identity = PackageIdentity::local(&built("packages/applications")).unwrap();
    block_on(launcher.set_enabled(&identity, false));
    icons.open();
    settle(&launcher);

    // At most the batch the worker had taken was extracted.
    assert!(icons.extracted().len() <= BATCH, "{:?}", icons.extracted());
}

/// An extension's own list shows an application's icon by the reference
/// the applications import gives with it: the JavaScript and TypeScript
/// samples. (The Applications command is a root provider, #164, with no list
/// of its own; root search draws its results' icons, above.)
fn a_commands_list_shows_the_applications_icon(package: &str, command: &str) {
    let dirs = Dirs::new();
    let icons = FakeIcons::new();
    let orange = png(256, [255, 140, 0, 255]);
    icons.draw(EDITOR_LINK, orange.clone(), None);
    let launcher = installed(&dirs, vec![shortcut("Editor", EDITOR)], &icons);
    install(&launcher, &built(&format!("packages/{package}")));
    search(&launcher, &command.to_lowercase());
    select_title(&launcher, command);
    block_on(launcher.activate_selected());
    assert_eq!(
        launcher.view().screen,
        Screen::Command,
        "{:?}",
        launcher.view()
    );
    assert_eq!(titles(&launcher), ["Editor"]);

    let deadline = std::time::Instant::now() + PATIENCE;
    let light = loop {
        let icon = launcher.presentation().rows[0]
            .icon
            .clone()
            .expect("the item's icon");
        if let Some((light, _)) = files(&icon) {
            break light;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never drawn: {icon:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(fs::read(light).unwrap(), orange);
}

#[test]
fn a_javascript_commands_list_shows_an_applications_icon_by_its_reference() {
    a_commands_list_shows_the_applications_icon(
        "sample-applications-js",
        "JavaScript applications sample",
    );
}

#[test]
fn a_typescript_commands_list_shows_an_applications_icon_by_its_reference() {
    a_commands_list_shows_the_applications_icon(
        "sample-applications-ts",
        "TypeScript applications sample",
    );
}

/// The application `id`'s host gives its icon reference with it.
#[test]
fn the_host_gives_each_application_an_icon_reference() {
    let host = Cached::new(
        Arc::new(FakeSystem {
            sources: Mutex::new(vec![shortcut("Editor", EDITOR)]),
        }),
        Duration::from_secs(3600),
    );
    let application = host.installed().unwrap().remove(0);
    assert_eq!(
        pane_core::applications::icon_reference(&application.id),
        application.id,
        "today the reference is the id; extensions must not rely on it"
    );
    assert_eq!(
        host.icon_source(&application.id).as_deref(),
        Some(EDITOR_LINK)
    );
    assert_eq!(host.icon_source(EDITOR_LINK).as_deref(), Some(EDITOR_LINK));
    assert_eq!(host.icon_source("0123456789abcdef0123456789abcdef"), None);
}
