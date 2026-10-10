//! Create Extension and Import Extension through the launcher's public
//! interface (#222, ADR 0047): the authoring rows are listed beside the
//! install rows and ask the window for a folder; the Create Extension form
//! writes the same folder `pane-ext new` writes (the templates pane-core
//! embeds), builds it once with the builder development mode uses, and
//! hands it to Pane's ordinary install preview, which develops the package
//! once the author installs it — as Import Extension develops a folder
//! that already exists. The build is a stand-in that stages the guest the
//! scaffolded manifest names; the real builds are `develop_builds.rs`'s
//! and the templates' own test in `pane-ext`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::executor::block_on;
use pane_core::develop::{Build, BuildJob, BuildOutcome, Builder};
use pane_core::{FolderAsk, Launcher, PackageIdentity, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/guests.rs"]
mod guests;

#[path = "support/rows.rs"]
mod rows;

use guests::guest;
use rows::{select_title, titles};

const CREATE_ROW: &str = "Create Extension…";
const IMPORT_ROW: &str = "Import Extension…";
const INSTALL_ROW: &str = "Install extension from folder…";
const NPM_ROW: &str = "Install extension from npm…";
const GIT_ROW: &str = "Install extension from Git…";
const SETTINGS_ROW: &str = "Settings…";

/// Builds a package folder by staging the guest its `pane.json` names as
/// the command's component, as the real builds do, or failing with `fail`
/// when one was set. The scaffolded templates' components sit in `dist/`
/// or `target/wasm32-wasip2/release/`, which the manifest names.
#[derive(Default)]
struct TemplateBuilder {
    /// Why every build fails, when it does.
    fail: Mutex<Option<String>>,
}

impl TemplateBuilder {
    /// A builder whose builds fail, printing `why`.
    fn failing(why: &str) -> TemplateBuilder {
        TemplateBuilder {
            fail: Mutex::new(Some(why.into())),
        }
    }
}

impl Builder for TemplateBuilder {
    fn build_for(&self, folder: &Path) -> Result<Arc<dyn Build>, String> {
        let component = component_of(folder);
        let fail = self.fail.lock().unwrap().clone();
        Ok(Arc::new(TemplateBuild { component, fail }))
    }
}

struct TemplateBuild {
    component: PathBuf,
    fail: Option<String>,
}

impl Build for TemplateBuild {
    fn command(&self) -> String {
        "fake template build".into()
    }

    fn ignores(&self, path: &Path) -> bool {
        path == self.component.as_path()
    }

    fn run(&self, job: &BuildJob) -> BuildOutcome {
        if let Some(why) = &self.fail {
            job.line(why);
            return BuildOutcome::Failed("fake template build failed".into());
        }
        let component = job.staging().join(&self.component);
        fs::create_dir_all(component.parent().unwrap()).unwrap();
        fs::copy(guest("sample_rust"), &component).unwrap();
        BuildOutcome::Built
    }
}

/// The component the `pane.json` in `folder` names for its first command.
fn component_of(folder: &Path) -> PathBuf {
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(folder.join("pane.json")).unwrap()).unwrap();
    PathBuf::from(manifest["commands"][0]["component"].as_str().unwrap())
}

/// A launcher over `data`'s extensions folder that builds with
/// `builder`.
fn launcher(data: &TempDir, builder: Arc<TemplateBuilder>) -> Launcher {
    Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
        .with_development(builder, pane_core::changes::channel().0)
}

/// The Create Extension form opened for `parent`, filled with `name` and
/// the `language`, submitted, and the view once it settles.
fn created(launcher: &Launcher, parent: &Path, name: &str, language: &str) -> Status {
    launcher.show_create_form(parent);
    launcher.set_field_value("name", name);
    launcher.set_field_value("language", language);
    block_on(launcher.submit_form());
    launcher.view().status
}

#[test]
fn the_authoring_rows_are_listed_and_ask_the_window_for_folders() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let launcher = launcher(&data, Arc::new(TemplateBuilder::default()));
    assert_eq!(
        titles(&launcher),
        [
            INSTALL_ROW,
            NPM_ROW,
            GIT_ROW,
            CREATE_ROW,
            IMPORT_ROW,
            SETTINGS_ROW
        ]
    );

    // Activating either row does nothing in the launcher: the window asks
    // for a folder, as it asks for the folder an Install-from-folder
    // picks.
    for (row, ask) in [
        (CREATE_ROW, FolderAsk::Create),
        (IMPORT_ROW, FolderAsk::Import),
    ] {
        select_title(&launcher, row);
        assert_eq!(launcher.selected_folder_ask(), Some(ask), "{row}");
        block_on(launcher.activate_selected());
        assert!(
            matches!(launcher.view().screen, Screen::Root { .. }),
            "{row}"
        );
        assert_eq!(launcher.view().status, Status::Idle);
    }
    // Not the other rows.
    select_title(&launcher, SETTINGS_ROW);
    assert_eq!(launcher.selected_folder_ask(), None);
}

#[test]
fn the_form_creates_builds_previews_and_develops_the_package() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let launcher = launcher(&data, Arc::new(TemplateBuilder::default()));
    launcher.show_create_form(sources.path());

    let view = launcher.view();
    let Screen::Form(form) = &view.screen else {
        panic!("no form: {:?}", view.screen);
    };
    assert_eq!(view.title, "Create Extension");
    assert_eq!(form.submit_label, "Create extension");
    let fields: Vec<(&str, &str)> = form
        .fields
        .iter()
        .map(|field| (field.id.as_str(), field.value.as_str()))
        .collect();
    assert_eq!(
        fields,
        [
            ("name", ""),
            ("language", "typescript"),
            ("template", "list")
        ]
    );

    // A name that names no package is refused on its field, as an
    // extension's rejection is, and the form stays.
    block_on(launcher.submit_form());
    let view = launcher.view();
    let Screen::Form(form) = &view.screen else {
        panic!("no form: {:?}", view.screen);
    };
    assert!(matches!(view.status, Status::Error(_)));
    assert!(form.fields[0].error.is_some());

    // Filled and submitted: the folder is written, built once with the
    // builder development mode uses, and the install preview offers it.
    launcher.set_field_value("name", "Word Count");
    launcher.set_field_value("language", "rust");
    block_on(launcher.submit_form());
    let folder = sources.path().join("word-count");
    assert!(
        folder.join("Cargo.toml").is_file(),
        "the package is written"
    );
    assert!(folder.join("src/lib.rs").is_file());
    let component = component_of(&folder);
    assert!(
        folder.join(&component).is_file(),
        "the built component is put in the folder"
    );
    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Package { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Word Count");
    assert_eq!(titles(&launcher), ["Install"]);

    // Installing it develops it: the folder is watched from now on.
    block_on(launcher.activate_selected());
    let identity = PackageIdentity::local(&folder).unwrap();
    assert!(launcher.development(&identity).is_some(), "it is developed");
    let view = launcher.view();
    let Status::Result(shown) = &view.status else {
        panic!("the development is shown: {:?}", view.status);
    };
    assert!(shown.starts_with("Developing Word Count"), "{shown}");
    // Its command is listed, as the template wrote it.
    assert!(titles(&launcher).contains(&"Word Count".to_owned()));
}

#[test]
fn a_typescript_package_is_created_the_same_way() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let launcher = launcher(&data, Arc::new(TemplateBuilder::default()));
    let status = created(&launcher, sources.path(), "Word Count", "typescript");
    // `npm install` needs the registry packages an author installs, which
    // are not published yet (#281); it fails and stops nothing — the
    // build is the stand-in here, so the flow reaches the preview anyway.
    assert_eq!(status, Status::Idle);
    let folder = sources.path().join("word-count");
    assert!(folder.join("package.json").is_file());
    let component = component_of(&folder);
    assert!(folder.join(&component).is_file());
    assert!(matches!(launcher.view().screen, Screen::Package { .. }));
    block_on(launcher.activate_selected());
    let identity = PackageIdentity::local(&folder).unwrap();
    assert!(launcher.development(&identity).is_some());
}

#[test]
fn a_folder_that_is_not_empty_is_refused_and_the_form_stays() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let existing = sources.path().join("word-count");
    fs::create_dir_all(&existing).unwrap();
    fs::write(existing.join("keep.txt"), "an author's file").unwrap();
    let launcher = launcher(&data, Arc::new(TemplateBuilder::default()));

    let status = created(&launcher, sources.path(), "Word Count", "rust");
    let Status::Error(why) = &status else {
        panic!("the refusal is shown: {status:?}");
    };
    assert!(why.contains("is not empty"), "{why}");
    // The form stays, and the author's file is kept.
    assert!(
        matches!(launcher.view().screen, Screen::Form(_)),
        "the form stays"
    );
    assert_eq!(
        fs::read_to_string(existing.join("keep.txt")).unwrap(),
        "an author's file"
    );
}

#[test]
fn a_build_failure_is_shown_and_the_folder_is_kept_for_importing() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let launcher = launcher(
        &data,
        Arc::new(TemplateBuilder::failing("error: expected `;`")),
    );
    let status = created(&launcher, sources.path(), "Word Count", "rust");
    let Status::Error(why) = &status else {
        panic!("the failure is shown: {status:?}");
    };
    assert!(
        why.starts_with("Word Count did not build: error: expected"),
        "{why}"
    );
    assert!(why.contains("the whole output is in "), "{why}");
    assert!(why.contains("import the extension once it builds"), "{why}");
    // The folder is written and kept.
    let folder = sources.path().join("word-count");
    assert!(folder.join("Cargo.toml").is_file());
    // The form stays, ready to submit again.
    assert!(matches!(launcher.view().screen, Screen::Form(_)));
}

#[test]
fn a_launcher_without_a_builder_names_that_it_cannot_build() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"));
    let status = created(&launcher, sources.path(), "Word Count", "rust");
    let Status::Error(why) = &status else {
        panic!("the missing builder is shown: {status:?}");
    };
    assert!(
        why.starts_with("Cannot build Word Count: this Pane does not build extensions"),
        "{why}"
    );
    // The folder is still written.
    assert!(sources.path().join("word-count/Cargo.toml").is_file());
}

#[test]
fn importing_previews_a_folder_and_develops_it_once_installed() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let launcher = launcher(&data, Arc::new(TemplateBuilder::default()));

    block_on(launcher.import_extension(&folder));
    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Package { .. }),
        "{:?}",
        view.screen
    );
    assert_eq!(view.title, "Hello");
    assert_eq!(titles(&launcher), ["Install"]);

    block_on(launcher.activate_selected());
    let identity = PackageIdentity::local(&folder).unwrap();
    assert!(launcher.development(&identity).is_some());
    let view = launcher.view();
    let Status::Result(shown) = &view.status else {
        panic!("the development is shown: {:?}", view.status);
    };
    assert!(shown.starts_with("Developing Hello"), "{shown}");
}

#[test]
fn importing_a_folder_that_is_no_package_is_explained_by_the_preview() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let launcher = launcher(&data, Arc::new(TemplateBuilder::default()));

    // The picker cannot look for pane.json; the preview explains what was
    // picked, as it explains any folder an install is offered.
    block_on(launcher.import_extension(sources.path()));
    let view = launcher.view();
    assert!(
        matches!(view.screen, Screen::Package { .. }),
        "{:?}",
        view.screen
    );
    let Status::Error(why) = &view.status else {
        panic!("the folder is explained: {:?}", view.status);
    };
    assert!(why.contains("has no pane.json"), "{why}");
    assert!(titles(&launcher).is_empty(), "nothing is offered");
}

#[test]
fn leaving_the_preview_drops_what_would_be_developed() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let launcher = launcher(&data, Arc::new(TemplateBuilder::default()));

    block_on(launcher.import_extension(&folder));
    // The author backs out of the preview: not installing it is the
    // answer, and a later install of the same folder (as `pane --install`)
    // develops nothing.
    launcher.back();
    assert!(matches!(launcher.view().screen, Screen::Root { .. }));
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    assert!(launcher.development(&identity).is_none());
    let view = launcher.view();
    let Status::Result(shown) = &view.status else {
        panic!("the install is shown: {:?}", view.status);
    };
    assert!(shown.starts_with("Installed Hello"), "{shown}");
}

#[test]
fn an_installed_folder_imported_again_is_updated_and_developed() {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    let launcher = launcher(&data, Arc::new(TemplateBuilder::default()));
    block_on(launcher.install_package(&folder));

    block_on(launcher.import_extension(&folder));
    assert_eq!(titles(&launcher), ["Update"]);
    block_on(launcher.activate_selected());
    let identity = PackageIdentity::local(&folder).unwrap();
    assert!(launcher.development(&identity).is_some());
    let view = launcher.view();
    let Status::Result(shown) = &view.status else {
        panic!("the development is shown: {:?}", view.status);
    };
    assert!(shown.starts_with("Developing Hello"), "{shown}");
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
