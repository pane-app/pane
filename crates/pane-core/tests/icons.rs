//! Icons, accessories and tooltips (#139) through the launcher's public
//! interface, with the icons sample in Rust, JavaScript and TypeScript,
//! real guests `cargo xtask guests` assembles: a package's and a command's
//! `icon` in `pane.json`, a command without one showing its package's, and
//! a package without one its first-letter tile, in root search and through
//! `Launcher::icon_of`; an unknown built-in name or an image the package
//! does not ship refusing the package at install, with the reason; a
//! missing or undersized icon cautioned about on an npm install's preview;
//! and an open command's rows with their icons (built-in, packaged with
//! `@light` and `@dark` variants, a pair, tinted, masked, failing with a
//! fallback, the SDKs' avatar and progress ring), tooltips and at most
//! three accessories, a date shown relative to the launcher's clock and
//! kept current, more accessories reported while the package is developed.
//! The drawing is the window's (`crates/pane/tests/icons.rs`).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::executor::block_on;
use pane_core::clipboard::ManualClock;
use pane_core::develop::{Build, BuildJob, BuildOutcome, Builder};
use pane_core::npm::Registry as NpmRegistry;
use pane_core::{
    AccessoryKind, Color, Icon, IconSource, Launcher, Mask, PackageIdentity, RowPresentation,
    Runtime, Screen, Status, Tint, Tone,
};
use serde_json::{Value, json};
use tempfile::TempDir;

#[path = "support/rows.rs"]
mod rows;

#[path = "support/guests.rs"]
mod guests;
#[path = "support/npm_registry.rs"]
mod npm_registry;

use npm_registry::{Registry, greeter_files, pack};
use rows::{select_title, titles};

/// 2026-01-01T00:00:00Z, the "Packaged image" row's date.
const NEW_YEAR: u64 = 1_767_225_600_000;

const HOUR: Duration = Duration::from_secs(3600);

/// One language's icons sample and its plain copy.
struct Fixture {
    package: &'static str,
    title: &'static str,
    /// The command with an icon of its own.
    command: &'static str,
    /// The command without one.
    inherited: &'static str,
    plain: &'static str,
    plain_title: &'static str,
    plain_command: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-icons",
    title: "Icons sample",
    command: "Icons",
    inherited: "Icons (package icon)",
    plain: "sample-icons-plain",
    plain_title: "Plain icons sample",
    plain_command: "Plain icons",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-icons-js",
    title: "JavaScript icons sample",
    command: "Icons (JavaScript)",
    inherited: "Icons (JavaScript, package icon)",
    plain: "sample-icons-plain-js",
    plain_title: "Plain JavaScript icons sample",
    plain_command: "Plain icons (JavaScript)",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-icons-ts",
    title: "TypeScript icons sample",
    command: "Icons (TypeScript)",
    inherited: "Icons (TypeScript, package icon)",
    plain: "sample-icons-plain-ts",
    plain_title: "Plain TypeScript icons sample",
    plain_command: "Plain icons (TypeScript)",
};

const ALL: [&Fixture; 3] = [&RUST, &JAVASCRIPT, &TYPESCRIPT];

/// A build that never runs: development only has to be on.
struct NoBuild;

impl Builder for NoBuild {
    fn build_for(&self, _folder: &Path) -> Result<Arc<dyn Build>, String> {
        Ok(Arc::new(NoBuild))
    }
}

impl Build for NoBuild {
    fn command(&self) -> String {
        "no build".into()
    }

    fn ignores(&self, _path: &Path) -> bool {
        true
    }

    fn run(&self, _job: &BuildJob) -> BuildOutcome {
        BuildOutcome::Stopped
    }
}

/// One test's Pane, with its clock at 2026-01-01T02:00:00Z.
struct Pane {
    sources: TempDir,
    _data: TempDir,
    launcher: Launcher,
    clock: Arc<ManualClock>,
}

impl Pane {
    fn new() -> Pane {
        let sources = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (changes, _) = pane_core::changes::channel();
        let clock = ManualClock::at(NEW_YEAR + 2 * 3_600_000);
        let launcher =
            Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
                .with_development(Arc::new(NoBuild), changes)
                .with_clock(clock.clone());
        Pane {
            sources,
            _data: data,
            launcher,
            clock,
        }
    }

    /// Copies the assembled package `name` into a source folder of its
    /// own, folders and all.
    fn source(&self, name: &str) -> PathBuf {
        let assembled = guests::guest_file("packages").join(name);
        let folder = self.sources.path().join(name);
        copy_folder(&assembled, &folder);
        folder
    }

    /// Installs the assembled package `name`, titled `title`; back to root
    /// search after.
    fn install(&self, name: &str, title: &str) -> PathBuf {
        let folder = self.source(name);
        block_on(self.launcher.install_package(&folder));
        assert_eq!(
            self.launcher.view().status,
            Status::Result(format!("Installed {title}"))
        );
        self.root();
        folder
    }

    fn root(&self) {
        while !matches!(self.launcher.view().screen, Screen::Root { .. }) {
            self.launcher.back();
        }
    }

    /// Opens the command titled `command` from root search.
    fn open(&self, command: &str) {
        self.root();
        block_on(self.launcher.set_query(command));
        select_title(&self.launcher, command);
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
}

fn copy_folder(from: &Path, to: &Path) {
    assert!(
        from.exists(),
        "{} is missing; run `cargo xtask guests`",
        from.display()
    );
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

/// The file name an image icon draws in the light and in the dark theme.
fn image_files(icon: &Icon) -> (String, String) {
    let IconSource::Image { light, dark } = &icon.source else {
        panic!("not an image: {icon:?}");
    };
    let name = |path: &PathBuf| path.file_name().unwrap().to_string_lossy().into_owned();
    (name(light), name(dark))
}

fn builtin(name: &str) -> IconSource {
    IconSource::Builtin {
        name: name.into(),
        filled: false,
    }
}

#[test]
fn a_package_and_its_commands_show_their_icons_and_a_command_without_one_its_packages() {
    for fixture in ALL {
        let pane = Pane::new();
        let folder = pane.install(fixture.package, fixture.title);
        let key = PackageIdentity::local(&folder).unwrap().key();
        let launcher = &pane.launcher;

        let package = launcher.icon_of(&key).expect("the package's icon");
        assert_eq!(
            image_files(&package),
            ("icon.png".into(), "icon.png".into()),
            "{}",
            fixture.title
        );
        let own = launcher
            .icon_of(&format!("{key}#icons"))
            .expect("the command's icon");
        assert_eq!(
            image_files(&own),
            ("command.svg".into(), "command.svg".into())
        );
        let inherited = launcher
            .icon_of(&format!("{key}#inherited"))
            .expect("its package's icon");
        assert_eq!(inherited, package);
        // The managed copy holds what it draws: the icons and the assets.
        let installed = launcher.packages()[0].location.clone();
        let IconSource::Image { light, .. } = &package.source else {
            unreachable!()
        };
        assert!(light.starts_with(&installed), "{light:?}");
        for file in [
            "icon.png",
            "command.svg",
            "assets/logo@dark.png",
            "assets/moon.svg",
        ] {
            assert!(installed.join(file).is_file(), "{file}");
        }

        // Root search draws them on the commands' rows; Pane's own rows
        // keep their tiles.
        block_on(launcher.set_query("icons"));
        assert_eq!(pane.row(fixture.command).icon, Some(own));
        assert_eq!(pane.row(fixture.inherited).icon, Some(package));
        block_on(launcher.set_query("settings"));
        assert_eq!(pane.row("Settings…").icon, None);
        assert_eq!(launcher.icon_of("pane.settings"), None);
    }
}

#[test]
fn a_package_without_an_icon_shows_its_first_letter_tile() {
    for fixture in ALL {
        let pane = Pane::new();
        let folder = pane.install(fixture.plain, fixture.plain_title);
        let key = PackageIdentity::local(&folder).unwrap().key();
        let tile = Icon::letter_of(fixture.plain_title);
        assert_eq!(tile.source, IconSource::Letter('P'));
        assert_eq!(pane.launcher.icon_of(&key), Some(tile.clone()));
        assert_eq!(
            pane.launcher.icon_of(&format!("{key}#plain")),
            Some(tile.clone())
        );
        block_on(pane.launcher.set_query(fixture.plain_command));
        assert_eq!(pane.row(fixture.plain_command).icon, Some(tile));
    }
}

#[test]
fn an_unknown_icon_name_or_a_missing_image_refuses_the_package_with_the_reason() {
    for fixture in ALL {
        let cases: [(&str, Value, &str); 4] = [
            (
                "icon",
                json!("no-such-icon"),
                "Invalid pane.json: the icon of the package names the built-in icon \
                 `no-such-icon`, which Pane does not have",
            ),
            (
                "command",
                json!("assets/gone.png"),
                "Invalid pane.json: the icon of command `icons` names assets/gone.png, which \
                 is not in the package",
            ),
            (
                "command",
                json!({"light": "icon.png", "dark": "dark.png"}),
                "Invalid pane.json: the icon of command `icons` names dark.png, which is not \
                 in the package",
            ),
            (
                "icon",
                json!({"builtin": "star", "fallback": "stra"}),
                "Invalid pane.json: the icon of the package names the built-in icon `stra`",
            ),
        ];
        for (field, icon, why) in cases {
            let pane = Pane::new();
            let folder = pane.source(fixture.package);
            let path = folder.join("pane.json");
            let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            match field {
                "icon" => manifest["icon"] = icon,
                _ => manifest["commands"][0]["icon"] = icon,
            }
            fs::write(&path, manifest.to_string()).unwrap();

            block_on(pane.launcher.install_package(&folder));
            let status = pane.launcher.view().status;
            assert!(
                matches!(&status, Status::Error(text) if text.starts_with(why)),
                "{}: {status:?}",
                fixture.title
            );
            assert!(pane.launcher.packages().is_empty(), "nothing is installed");
        }
    }
}

/// The start of a PNG `width`×`height`: all Pane reads of an icon to know
/// its size.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

#[test]
fn a_missing_or_undersized_icon_is_a_caution_on_an_npm_preview() {
    const GREETER: &str = "@pane-samples/greeter";
    let caution = |details: &[String]| {
        details
            .iter()
            .find(|line| line.starts_with("Caution:"))
            .cloned()
    };
    for (version, icon, expected) in [
        ("0.1.0", None, Some("Caution: it has no icon of its own")),
        (
            "0.2.0",
            Some(png(64, 64)),
            Some("Caution: its icon icon.png is 64×64, smaller than the 512×512"),
        ),
        ("0.3.0", Some(png(512, 512)), None),
    ] {
        let registry = Registry::start();
        let mut files = greeter_files(&guests::guests(), version);
        if let Some(icon) = icon {
            let manifest = files
                .iter_mut()
                .find(|(name, _)| *name == "pane.json")
                .unwrap();
            let mut json: Value = serde_json::from_slice(&manifest.1).unwrap();
            json["icon"] = json!("icon.png");
            manifest.1 = serde_json::to_vec(&json).unwrap();
            files.push(("icon.png", icon));
        }
        registry.publish(GREETER, version, pack(&files));
        let data = tempfile::tempdir().unwrap();
        let launcher = Launcher::with_packages(Runtime::start(), vec![], data.path().join("x"))
            .with_npm_registry(NpmRegistry::local(registry.url()).unwrap());

        block_on(launcher.preview_npm(&format!("{GREETER}@{version}")));
        let details = launcher.view().details().to_vec();
        match expected {
            Some(expected) => assert!(
                caution(&details).is_some_and(|line| line.starts_with(expected)),
                "{details:#?}"
            ),
            None => assert_eq!(caution(&details), None, "{details:#?}"),
        }
        // A caution, not a refusal: it installs.
        assert_eq!(titles(&launcher), ["Install"]);
    }
}

#[test]
fn the_rows_draw_their_icons_accessories_and_tooltips() {
    for fixture in ALL {
        let pane = Pane::new();
        pane.install(fixture.package, fixture.title);
        pane.open(fixture.command);

        let row = pane.row("Built-in icon");
        assert_eq!(
            row.icon.as_ref().map(|icon| &icon.source),
            Some(&builtin("star"))
        );
        assert_eq!(
            row.title_tooltip.as_deref(),
            Some("A built-in icon from the whole reicon set"),
            "{}",
            fixture.title
        );
        assert_eq!(row.accessories.len(), 1);
        assert_eq!(
            (
                row.accessories[0].kind,
                row.accessories[0].text.as_str(),
                row.accessories[0].tooltip.as_deref()
            ),
            (AccessoryKind::Text, "3", Some("Unread"))
        );
        assert_eq!(row.accessories[0].spoken(), "3, Unread");
        // An icon without a tooltip is decoration.
        assert!(row.icon.unwrap().is_decorative());

        // The packaged image's variants, by theme.
        let row = pane.row("Packaged image");
        assert_eq!(
            image_files(row.icon.as_ref().unwrap()),
            ("logo@light.png".into(), "logo@dark.png".into())
        );
        assert_eq!(
            row.subtitle_tooltip.as_deref(),
            Some("logo@light.png in the light theme, logo@dark.png in the dark")
        );
        let date = &row.accessories[0];
        assert_eq!((date.kind, date.text.as_str()), (AccessoryKind::Date, "2h"));
        let offset = pane_core::clipboard_view::local_offset_ms(NEW_YEAR);
        assert_eq!(
            date.tooltip,
            Some(pane_core::absolute_date(NEW_YEAR as i64, offset))
        );

        let row = pane.row("Light and dark pair");
        assert_eq!(
            image_files(row.icon.as_ref().unwrap()),
            ("sun.svg".into(), "moon.svg".into())
        );
        let tag = &row.accessories[0];
        assert_eq!(
            (tag.kind, tag.text.as_str(), tag.color),
            (
                AccessoryKind::Tag,
                "Open",
                Some(Tint::Same(Color::Tone(Tone::Green)))
            )
        );

        let row = pane.row("Tinted icon");
        let icon = row.icon.unwrap();
        assert_eq!(
            (icon.source, icon.tint),
            (builtin("heart"), Some(Tint::Same(Color::Rgba(0xFF6363FF))))
        );
        assert_eq!(
            row.accessories[0].color,
            Some(Tint::Pair {
                light: Color::Rgba(0xB42318FF),
                dark: Color::Rgba(0xFF8A80FF)
            })
        );

        let row = pane.row("Masked image");
        let icon = row.icon.unwrap();
        assert_eq!(image_files(&icon).0, "photo.png");
        assert_eq!(icon.mask, Some(Mask::Circle));
        let owner = &row.accessories[0];
        assert_eq!(owner.text, "");
        assert_eq!(
            owner.icon.as_ref().map(|icon| (&icon.source, icon.tint)),
            Some((&builtin("user"), Some(Tint::Same(Color::Tone(Tone::Blue)))))
        );
        assert_eq!(owner.spoken(), "Owner");

        // The image the package does not ship: its fallback, with its
        // tooltip, which assistive technology reads.
        let row = pane.row("Failing image");
        let icon = row.icon.unwrap();
        assert_eq!(
            (icon.source.clone(), icon.tint, icon.tooltip.as_deref()),
            (
                builtin("warning"),
                Some(Tint::Same(Color::Tone(Tone::Orange))),
                Some("Image missing")
            )
        );
        assert!(!icon.is_decorative());

        // The SDKs' avatar and progress ring: SVG images by `data:` URL.
        let row = pane.row("Avatar and progress");
        let avatar = row.icon.unwrap();
        assert!(
            matches!(&avatar.source, IconSource::Url(url) if url.starts_with("data:image/svg+xml,")),
            "{avatar:?}"
        );
        assert_eq!(
            (avatar.mask, avatar.tooltip.as_deref()),
            (Some(Mask::Circle), Some("Ada Lovelace"))
        );
        let IconSource::Url(url) = &avatar.source else {
            unreachable!()
        };
        let (media, svg) = pane_core::icons::data_url(url).unwrap();
        assert_eq!(media, "image/svg+xml");
        assert!(
            String::from_utf8(svg).unwrap().contains(">AL</text>"),
            "the initials"
        );
        let ring = row.accessories[0].icon.as_ref().unwrap();
        assert!(matches!(&ring.source, IconSource::Url(_)));
        assert_eq!(ring.tint, Some(Tint::Same(Color::Tone(Tone::Accent))));
        assert_eq!(row.accessories[0].text, "40%");

        // A row draws three accessories.
        let row = pane.row("Crowded row");
        assert_eq!(
            row.accessories
                .iter()
                .map(|accessory| accessory.text.as_str())
                .collect::<Vec<_>>(),
            ["1", "2", "3"]
        );
    }
}

#[test]
fn a_date_stays_current_while_the_list_is_open() {
    for fixture in ALL {
        let pane = Pane::new();
        pane.install(fixture.package, fixture.title);
        pane.open(fixture.command);
        assert_eq!(pane.row("Packaged image").accessories[0].text, "2h");
        pane.clock.advance(HOUR);
        assert_eq!(pane.row("Packaged image").accessories[0].text, "3h");
        pane.clock.advance(HOUR * 24 * 3);
        assert_eq!(
            pane.row("Packaged image").accessories[0].text,
            "3d",
            "{}",
            fixture.title
        );
        // Drawn again after an action, the list keeps its looks.
        select_title(&pane.launcher, "Packaged image");
        block_on(pane.launcher.activate_selected());
        assert_eq!(
            pane.launcher.view().status,
            Status::Result("Chose Packaged image".into())
        );
        assert_eq!(pane.row("Packaged image").accessories[0].text, "3d");
    }
}

#[test]
fn development_mode_reports_rows_with_more_accessories_than_drawn() {
    for fixture in ALL {
        let pane = Pane::new();
        let folder = pane.install(fixture.package, fixture.title);
        let identity = PackageIdentity::local(&folder).unwrap();
        // Not developed: opening says nothing.
        pane.open(fixture.command);
        assert_eq!(pane.launcher.view().status, Status::Idle);
        pane.root();

        block_on(pane.launcher.start_developing(&identity));
        assert!(pane.launcher.development(&identity).is_some());
        pane.open(fixture.command);
        let Status::Error(report) = pane.launcher.view().status else {
            panic!("no report: {:?}", pane.launcher.view().status);
        };
        assert!(report.contains("a row shows at most 3"), "{report}");
        assert!(report.contains("“Crowded row” has 5"), "{report}");

        // The same list drawn again after an action is not reported again.
        select_title(&pane.launcher, "Built-in icon");
        block_on(pane.launcher.activate_selected());
        assert_eq!(
            pane.launcher.view().status,
            Status::Result("Chose Built-in icon".into())
        );
        pane.launcher.stop_developing(&identity);
    }
}
