//! Create Extension and Import Extension in the native window, on GPUI's
//! test platform, with real key events: the authoring rows are listed, the
//! Create Extension form is filled with the keyboard (a choice picked with
//! its own keys) and its submission writes the package, builds it once and
//! hands it to the install preview, which develops it once Enter installs
//! it; Import Extension previews a folder that already exists and develops
//! it the same way. The build is a stand-in that stages the guest the
//! scaffolded manifest names; `pane-core`'s tests cover the rest of the
//! flow, and `develop.rs` the development that follows.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::develop::{Build, BuildJob, BuildOutcome, Builder};
use pane_core::{Launcher, LauncherView, PackageIdentity, Runtime, Screen, Status};

#[path = "support/settle.rs"]
mod settle;

use settle::{enter_flow, settle, settle_shown, until};

const CREATE_ROW: &str = "Create Extension…";
const IMPORT_ROW: &str = "Import Extension…";
const INSTALL_ROW: &str = "Install extension from folder…";
const NPM_ROW: &str = "Install extension from npm…";
const GIT_ROW: &str = "Install extension from Git…";
const SETTINGS_ROW: &str = "Settings…";

fn guest(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/guests")
        .join(format!("{name}.wasm"));
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

fn titles(view: &LauncherView) -> Vec<&str> {
    view.rows.iter().map(|row| row.title.as_str()).collect()
}

/// Builds a package folder by staging the guest its `pane.json` names as
/// the command's component, as the real builds do.
struct TemplateBuilder;

impl Builder for TemplateBuilder {
    fn build_for(&self, folder: &Path) -> Result<Arc<dyn Build>, String> {
        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(folder.join("pane.json")).unwrap()).unwrap();
        let component = manifest["commands"][0]["component"]
            .as_str()
            .unwrap()
            .to_owned();
        Ok(Arc::new(TemplateBuild {
            component: PathBuf::from(component),
        }))
    }
}

struct TemplateBuild {
    component: PathBuf,
}

impl Build for TemplateBuild {
    fn command(&self) -> String {
        "fake template build".into()
    }

    fn ignores(&self, path: &Path) -> bool {
        path == self.component.as_path()
    }

    fn run(&self, job: &BuildJob) -> BuildOutcome {
        let component = job.staging().join(&self.component);
        fs::create_dir_all(component.parent().unwrap()).unwrap();
        fs::copy(guest("sample_rust"), &component).unwrap();
        BuildOutcome::Built
    }
}

/// A window over a launcher that builds with the stand-in, following the
/// changes channel so background work redraws it.
fn open<'a>(
    cx: &'a mut TestAppContext,
    data: &tempfile::TempDir,
) -> (Entity<LauncherWindow>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (sender, changes) = pane_core::changes::channel();
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_development(Arc::new(TemplateBuilder), sender);
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher
    });
    (window, cx)
}

/// Writes a package folder titled "Hello" whose component is the Rust
/// sample's guest, built.
fn package(folder: &Path) -> PathBuf {
    fs::create_dir_all(folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{
  "manifestVersion": 1,
  "title": "Hello",
  "apiVersion": "0.1",
  "commands": [{ "id": "hello", "title": "Say hello", "component": "command.wasm" }]
}"#,
    )
    .unwrap();
    fs::copy(guest("sample_rust"), folder.join("command.wasm")).unwrap();
    folder.to_path_buf()
}

/// The value the open form holds in field `id`.
fn field_value(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, id: &str) -> String {
    let view = cx.read_entity(window, |window, _| window.launcher().view());
    let form = view.form().expect("a form is open");
    let field = form.fields.iter().find(|field| field.id == id);
    field.expect("the field exists").value.clone()
}

/// Selects the row titled `title` and presses Enter.
fn press_enter_on(
    window: &Entity<LauncherWindow>,
    cx: &mut VisualTestContext,
    title: &str,
) -> LauncherView {
    let launcher = cx.read_entity(window, |window, _| window.launcher().clone());
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.title == title)
        .unwrap_or_else(|| panic!("no row {title:?}"));
    launcher.select(index);
    cx.simulate_keystrokes("enter");
    settle(window, cx)
}

#[gpui::test]
fn the_create_row_opens_the_form_and_creates_builds_and_develops(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (window, cx) = open(cx, &data);
    assert_eq!(
        titles(&settle(&window, cx)),
        [
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            CREATE_ROW,
            IMPORT_ROW,
            SETTINGS_ROW
        ]
    );

    // The row asks for a folder; the window's pick opens the form, focus on
    // its name field.
    window.update_in(cx, |this, window, cx| {
        this.create_extension(sources.path(), window, cx);
    });
    let view = settle(&window, cx);
    assert!(view.form().is_some(), "{:?}", view.screen);
    assert_eq!(view.title, "Create Extension");
    for field in ["field-name", "field-language", "field-template"] {
        assert!(cx.debug_bounds(field).is_some(), "{field} is drawn");
    }

    // The language's choice is picked with its own keys.
    cx.simulate_keystrokes("tab");
    cx.simulate_keystrokes("down");
    assert_eq!(field_value(&window, cx, "language"), "rust");

    // A name typed and submitted: the package is written, built once and
    // previewed.
    cx.simulate_keystrokes("shift-tab");
    cx.simulate_input("Word Count");
    cx.simulate_keystrokes("enter");
    let folder = sources.path().join("word-count");
    let view = until(&window, cx, |view| {
        matches!(view.screen, Screen::Package { .. })
    });
    assert!(
        folder.join("Cargo.toml").is_file(),
        "the package is written"
    );
    assert!(
        folder
            .join("target/wasm32-wasip2/release/word_count.wasm")
            .is_file(),
        "the built component is in the folder"
    );
    assert_eq!(view.title, "Word Count");
    assert_eq!(titles(&view), ["Install"]);
    assert!(cx.debug_bounds("row-Install").is_some());

    // Installing it develops it: the window says so by itself, and the
    // extension list offers to stop.
    cx.simulate_keystrokes("enter");
    let view = until(
        &window,
        cx,
        |view| matches!(&view.status, Status::Result(text) if text.starts_with("Developing Word Count")),
    );
    assert!(cx.debug_bounds("status-result").is_some());
    let identity = PackageIdentity::local(&folder).unwrap();
    assert!(
        cx.read_entity(&window, |window, _| window
            .launcher()
            .development(&identity)
            .is_some()),
        "the package is developed"
    );

    // The template's command is listed and runs, its component being the
    // sample guest the stand-in build stages.
    let view = until(&window, cx, |view| {
        view.rows.iter().any(|row| row.title == "Word Count")
    });
    assert!(cx.debug_bounds("row-Word Count").is_some());
    let view = press_enter_on(&window, cx, "Word Count");
    assert_eq!(view.screen, Screen::Command, "{:?}", view.status);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        settle_shown(&window, cx),
        Status::Result("Hello from the Rust guest".into())
    );
}

#[gpui::test]
fn a_name_that_names_no_package_marks_the_field(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (window, cx) = open(cx, &data);
    window.update_in(cx, |this, window, cx| {
        this.create_extension(sources.path(), window, cx);
    });
    settle(&window, cx);

    // Submitted with nothing typed: the field is marked, the form stays.
    cx.simulate_keystrokes("enter");
    let view = settle(&window, cx);
    assert!(view.form().is_some(), "the form stays");
    assert!(matches!(view.status, Status::Error(_)));
    assert!(cx.debug_bounds("field-error-name").is_some());

    // A name typed and submitted again: it works.
    cx.simulate_input("Word Count");
    cx.simulate_keystrokes("enter");
    until(&window, cx, |view| {
        matches!(view.screen, Screen::Package { .. })
    });
    assert!(sources.path().join("word-count/pane.json").is_file());
}

#[gpui::test]
fn the_import_row_previews_a_folder_and_develops_it_once_installed(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let (window, cx) = open(cx, &data);

    // The row asks for a folder; the window's pick previews it.
    window.update_in(cx, |this, window, cx| {
        this.import_extension(&folder, window, cx);
    });
    let view = until(&window, cx, |view| {
        matches!(view.screen, Screen::Package { .. })
    });
    assert_eq!(view.title, "Hello");
    assert!(cx.debug_bounds("row-Install").is_some());

    cx.simulate_keystrokes("enter");
    until(
        &window,
        cx,
        |view| matches!(&view.status, Status::Result(text) if text.starts_with("Developing Hello")),
    );
    let identity = PackageIdentity::local(&folder).unwrap();
    assert!(
        cx.read_entity(&window, |window, _| window
            .launcher()
            .development(&identity)
            .is_some()),
        "the package is developed"
    );
    // The development rows, as Manage extensions shows them.
    let view = enter_flow(&window, cx);
    assert!(matches!(view.screen, Screen::Extensions { .. }));
    assert!(cx.debug_bounds("row-Stop developing Hello").is_some());
}
