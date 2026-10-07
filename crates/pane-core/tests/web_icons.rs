//! Web images and system icons in rows and in actions' icons (#142),
//! through the launcher's public interface, with the icons sample in Rust,
//! JavaScript and TypeScript (real guests `cargo xtask guests` assembles)
//! and the image server (`support/image_server.rs`), a local test server
//! the sample is pointed at through its `imageServer` setting: nothing
//! reaches beyond this computer.
//!
//! A web image shows its fallback at once and its image once Pane
//! downloaded it; the list never waits. Downloads keep to the ceilings of
//! the extension's own web requests (ADR 0018), and one that fails or is
//! over them leaves the fallback. Rows and actions naming one URL share
//! one download.
//! Downloaded images are the package's extension cache: found again after
//! a restart without a download, and removed by "Clear cache". A system
//! icon by path shows the icon the host extracted (a stand-in for the
//! system's here; the Windows adapter's own test is
//! `system_icon_adapters.rs`), and a path that does not exist its
//! fallback. The SDKs' favicon and file icon helpers give the same icons
//! in every language. The drawing is the window's
//! (`crates/pane/tests/web_icons.rs`).

#[path = "support/image_server.rs"]
mod image_server;
#[path = "support/rows.rs"]
mod rows;

#[path = "support/guests.rs"]
mod guests;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use image_server::{ImageServer, png};
use pane_core::icons::web_image_stem;
use pane_core::system_icons::{SystemIcon, SystemIcons};
use pane_core::{
    Color, HttpLimits, Icon, IconSource, Launcher, PackageIdentity, RowPresentation, Runtime,
    Screen, Status, Tint, Tone,
};
use rows::{manage, select_title, titles, to_root};
use serde_json::json;
use tempfile::TempDir;

/// How long a load may take on a slow machine.
const LOADED: Duration = Duration::from_secs(30);

/// One language's icons sample.
struct Fixture {
    package: &'static str,
    title: &'static str,
    command: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-icons",
    title: "Icons sample",
    command: "Icons",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-icons-js",
    title: "JavaScript icons sample",
    command: "Icons (JavaScript)",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-icons-ts",
    title: "TypeScript icons sample",
    command: "Icons (TypeScript)",
};

const ALL: [&Fixture; 3] = [&RUST, &JAVASCRIPT, &TYPESCRIPT];

/// The host's icon extraction, stood in for: a small PNG for any path that
/// exists, counting the paths it was asked for.
#[derive(Default)]
struct FakeIcons {
    asked: AtomicUsize,
}

impl SystemIcons for FakeIcons {
    fn icon(&self, path: &Path) -> Result<SystemIcon, String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        if path.exists() {
            Ok(SystemIcon::Png(png(8, [48, 164, 108, 255])))
        } else {
            Err(format!("{} does not exist", path.display()))
        }
    }
}

/// One test's Pane: its folders, the sample's source folder, and the
/// system icons it extracts with.
struct Pane {
    sources: TempDir,
    data: TempDir,
    folder: PathBuf,
    runtime: Runtime,
    launcher: Launcher,
    icons: Arc<FakeIcons>,
    fixture: &'static Fixture,
}

impl Pane {
    /// Installs `fixture`'s sample, pointed at `server`, its "File icon" at
    /// a file that exists and its "Application icon" at one that does not.
    fn new(fixture: &'static Fixture, server: &ImageServer) -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let folder = sources.path().join(fixture.package);
        copy_folder(
            &guests::guest_file("packages").join(fixture.package),
            &folder,
        );
        let notes = sources.path().join("notes.txt");
        fs::write(&notes, "notes").unwrap();
        let gone = sources.path().join("gone.exe");
        // The sample reads where its images and files are from its
        // settings, which Pane keeps by the package's identity.
        let key = PackageIdentity::local(&folder).unwrap().key();
        let settings = json!({
            "version": 1,
            "packages": {
                key: {
                    "imageServer": server.url(),
                    "iconFile": notes.to_string_lossy(),
                    "iconApplication": gone.to_string_lossy(),
                }
            }
        });
        let packages = data.path().join("extensions");
        fs::create_dir_all(&packages).unwrap();
        fs::write(packages.join("settings.json"), settings.to_string()).unwrap();
        let runtime = Runtime::start().unwrap();
        let icons = Arc::new(FakeIcons::default());
        let launcher = Pane::launcher(&runtime, &packages, &icons);
        block_on(launcher.install_package(&folder));
        assert_eq!(
            launcher.view().status,
            Status::Result(format!("Installed {}", fixture.title))
        );
        let pane = Pane {
            sources,
            data,
            folder,
            runtime,
            launcher,
            icons,
            fixture,
        };
        pane.root();
        pane
    }

    fn launcher(runtime: &Runtime, packages: &Path, icons: &Arc<FakeIcons>) -> Launcher {
        Launcher::with_packages(Ok(runtime.clone()), vec![], packages.to_path_buf())
            .with_system_icons(icons.clone())
    }

    /// Pane started again on the same data: a new runtime and launcher.
    fn restart(self) -> Pane {
        let Pane {
            sources,
            data,
            folder,
            icons,
            fixture,
            ..
        } = self;
        let runtime = Runtime::start().unwrap();
        let launcher = Pane::launcher(&runtime, &data.path().join("extensions"), &icons);
        let pane = Pane {
            sources,
            data,
            folder,
            runtime,
            launcher,
            icons,
            fixture,
        };
        pane.root();
        pane
    }

    fn identity(&self) -> PackageIdentity {
        PackageIdentity::local(&self.folder).unwrap()
    }

    /// Where Pane keeps its data.
    fn packages(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    fn root(&self) {
        while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.launcher.back();
        }
    }

    /// Opens the sample's command from root search: its list is drawn when
    /// this returns, whatever its images are doing.
    fn open(&self) {
        self.root();
        block_on(self.launcher.set_query(self.fixture.command));
        select_title(&self.launcher, self.fixture.command);
        block_on(self.launcher.activate_selected());
        let view = self.launcher.view();
        assert_eq!(
            (&view.screen, view.title.as_str()),
            (&Screen::Command, "Icons sample"),
            "{:?}",
            view.status
        );
    }

    /// The presentation of the row titled `title` on screen.
    fn row(&self, title: &str) -> RowPresentation {
        let at = titles(&self.launcher)
            .iter()
            .position(|row| row == title)
            .unwrap_or_else(|| panic!("no row {title:?} in {:?}", titles(&self.launcher)));
        self.launcher.presentation().rows[at].clone()
    }

    /// The icon of the row titled `title`.
    fn icon(&self, title: &str) -> Icon {
        self.row(title)
            .icon
            .unwrap_or_else(|| panic!("{title} has no icon"))
    }

    /// The icon of the action at `index` of the item titled `item`, as the
    /// Actions panel draws it now; the item is selected after.
    fn action_icon(&self, item: &str, index: usize) -> Icon {
        select_title(&self.launcher, item);
        let actions = self.launcher.item_actions().expect("its actions");
        actions.actions[index]
            .icon
            .clone()
            .unwrap_or_else(|| panic!("{item}'s action {index} has no icon"))
    }

    /// Waits until the row titled `title` shows an image file, which it
    /// returns.
    fn image_of(&self, title: &str) -> PathBuf {
        let deadline = Instant::now() + LOADED;
        loop {
            if let IconSource::Image { light, dark } = self.icon(title).source {
                assert_eq!(light, dark);
                return light;
            }
            assert!(
                Instant::now() < deadline,
                "{} ({}): {title} shows no image: {:?}",
                self.fixture.title,
                self.fixture.command,
                self.icon(title)
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Waits until no image is loading.
    fn loaded(&self) {
        assert!(
            self.launcher.wait_for_icons(LOADED),
            "the icons kept loading"
        );
    }

    /// Clears the sample's cache in the extension manager.
    fn clear_cache(&self) -> Status {
        to_root(&self.launcher);
        manage(&self.launcher);
        select_title(
            &self.launcher,
            &format!("Clear cache of {}", self.fixture.title),
        );
        block_on(self.launcher.activate_selected());
        assert!(matches!(
            self.launcher.view().screen,
            Screen::Confirm { .. }
        ));
        select_title(&self.launcher, "Clear cache");
        block_on(self.launcher.activate_selected());
        self.launcher.view().status
    }
}

fn copy_folder(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn builtin(name: &str) -> IconSource {
    IconSource::Builtin {
        name: name.into(),
        filled: false,
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

/// The files under `dir`, if it exists.
fn files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files_in(&path));
        } else {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// The list is drawn at once with each web image's fallback; each image
/// replaces its fallback once downloaded, in a row as in an action's icon
/// in the Actions panel; two rows and an action naming one URL share one
/// download; an address without an image keeps its fallback. The
/// favicon helper names the site's `/favicon.ico` with a globe as its
/// fallback in every language.
#[test]
fn a_web_image_shows_its_fallback_at_once_and_the_image_when_it_arrives() {
    for fixture in ALL {
        let server = ImageServer::start();
        let pane = Pane::new(fixture, &server);
        pane.open();
        // Drawn while the slow image is held back: its fallback.
        let slow = pane.icon("Slow web image");
        assert_eq!(slow.source, builtin("clock"), "{}", fixture.title);
        assert_eq!(slow.tint, Some(Tint::Same(Color::Tone(Tone::Secondary))));
        assert_eq!(pane.icon("Same slow image").source, builtin("clock"));
        // So does an action's icon in the Actions panel ("Open Image").
        let action = pane.action_icon("Built-in icon", 2);
        assert_eq!(
            (action.source, action.tint),
            (
                builtin("clock"),
                Some(Tint::Same(Color::Tone(Tone::Secondary)))
            ),
            "{}",
            fixture.title
        );

        // While it downloads, it is on its package's undo list.
        assert!(server.wait_for("/images/slow.png", LOADED));
        assert!(
            pane.launcher
                .undo_list(&pane.identity())
                .contains(&"web image"),
            "{:?}",
            pane.launcher.undo_list(&pane.identity())
        );

        // The favicon arrives by itself, kept as the package's cache.
        let favicon = pane.image_of("Favicon");
        let url = format!("{}/favicon.ico", server.url());
        assert_eq!(file_name(&favicon), format!("{}.png", web_image_stem(&url)));
        assert!(favicon.starts_with(pane.packages().join("web-images")));
        assert_eq!(
            pane.icon("Favicon")
                .fallback
                .map(|globe| (globe.source, globe.tint)),
            Some((
                builtin("global"),
                Some(Tint::Same(Color::Tone(Tone::Secondary)))
            ))
        );

        server.release();
        let slow = pane.image_of("Slow web image");
        assert_eq!(pane.image_of("Same slow image"), slow);
        pane.loaded();
        // The action's icon is the same image, its fallback kept.
        let action = pane.action_icon("Built-in icon", 2);
        assert_eq!(
            action.source,
            IconSource::Image {
                light: slow.clone(),
                dark: slow.clone()
            },
            "{}",
            fixture.title
        );
        assert_eq!(
            action.fallback.map(|clock| clock.source),
            Some(builtin("clock"))
        );
        // One download each, however many rows and actions name it.
        assert_eq!(
            server.count("/images/slow.png"),
            1,
            "{:?}",
            server.requests()
        );
        assert_eq!(server.count("/favicon.ico"), 1);

        // The server has no such image: its fallback stays, with its
        // tooltip.
        let broken = pane.icon("Broken image");
        assert_eq!(
            (broken.source, broken.tooltip.as_deref()),
            (builtin("link-broken"), Some("Image unavailable"))
        );
        assert_eq!(server.count("/images/missing.png"), 1);
        // Nothing left downloading.
        assert!(
            !pane
                .launcher
                .undo_list(&pane.identity())
                .contains(&"web image")
        );
    }
}

/// A download over the size limit, or whose answer is slower than the
/// limit, ends with the fallback in place; it is not tried again.
#[test]
fn downloads_keep_to_the_web_limits_and_leave_the_fallback() {
    let server = ImageServer::start();
    let pane = Pane::new(&RUST, &server);
    pane.runtime.set_http_limits(HttpLimits {
        body: 16,
        first_byte: Duration::from_millis(500),
        ..HttpLimits::default()
    });
    pane.open();
    assert!(server.wait_for("/favicon.ico", LOADED));
    assert!(server.wait_for("/images/slow.png", LOADED));
    pane.loaded();
    // Over 16 bytes: the globe stays.
    assert_eq!(pane.icon("Favicon").source, builtin("global"));
    // No answer within half a second: the clock stays, even once the
    // image could come.
    assert_eq!(pane.icon("Slow web image").source, builtin("clock"));
    server.release();
    pane.open();
    pane.loaded();
    assert_eq!(pane.icon("Slow web image").source, builtin("clock"));
    assert_eq!(server.count("/images/slow.png"), 1);
    assert!(files_in(&pane.packages().join("web-images")).is_empty());
}

/// Downloaded images are kept as the package's cache: a restart shows them
/// at once without downloading them again, and "Clear cache" removes them,
/// so the next list downloads them again.
#[test]
fn cached_images_are_reused_after_a_restart_and_removed_by_clear_cache() {
    let server = ImageServer::start();
    server.release();
    let pane = Pane::new(&RUST, &server);
    pane.open();
    let favicon = pane.image_of("Favicon");
    let slow = pane.image_of("Slow web image");
    pane.loaded();
    let images = pane.packages().join("web-images");
    assert_eq!(files_in(&images), {
        let mut both = vec![favicon.clone(), slow.clone()];
        both.sort();
        both
    });

    let pane = pane.restart();
    pane.open();
    // At once, from the cache.
    assert_eq!(
        pane.icon("Favicon").source,
        IconSource::Image {
            light: favicon.clone(),
            dark: favicon.clone()
        }
    );
    assert_eq!(pane.image_of("Slow web image"), slow);
    pane.loaded();
    assert_eq!(server.count("/favicon.ico"), 1);
    assert_eq!(server.count("/images/slow.png"), 1);

    let cleared = pane.clear_cache();
    assert!(
        matches!(&cleared, Status::Result(text) if text.starts_with("Cleared the cache of Icons sample")),
        "{cleared:?}"
    );
    assert!(files_in(&images).is_empty(), "{:?}", files_in(&images));

    pane.open();
    assert_eq!(pane.image_of("Favicon"), favicon);
    pane.loaded();
    assert_eq!(server.count("/favicon.ico"), 2);
}

/// A system icon by path shows the icon the host extracted, kept as a PNG
/// in Pane's folder and extracted once; a path that does not exist shows
/// the file icon helper's fallback, a document, in every language.
#[test]
fn a_system_icon_shows_the_files_icon_and_a_missing_path_its_fallback() {
    for fixture in ALL {
        let server = ImageServer::start();
        server.release();
        let pane = Pane::new(fixture, &server);
        pane.open();
        let document = (
            builtin("document"),
            Some(Tint::Same(Color::Tone(Tone::Secondary))),
        );
        // The missing application: its fallback from the start.
        let application = pane.icon("Application icon");
        assert_eq!((application.source, application.tint), document);

        let file = pane.image_of("File icon");
        assert!(
            file.starts_with(pane.packages().join("system-icons")),
            "{}",
            file.display()
        );
        assert_eq!(
            fs::read(&file).unwrap(),
            png(8, [48, 164, 108, 255]),
            "{}",
            fixture.title
        );
        let shown = pane.icon("File icon");
        assert_eq!(
            shown
                .fallback
                .map(|fallback| (fallback.source, fallback.tint)),
            Some(document)
        );
        pane.loaded();
        // Drawn again, it is not extracted again.
        pane.open();
        pane.loaded();
        assert_eq!(pane.image_of("File icon"), file);
        assert_eq!(pane.icons.asked.load(Ordering::SeqCst), 1);
    }
}
