//! Pane's own Create Extension and Import Extension commands (#222,
//! ADR 0047): an author starts in the app, writing a new extension
//! package or importing one whose source already exists.
//!
//! Create Extension asks for the parent folder (the window's folder
//! picker, as the row's activation hands it there), then a name, a
//! language (Rust or TypeScript) and a template (list, detail, form or
//! no-view) on a form. Submitting it writes the same files `pane-ext new`
//! writes — the templates pane-core embeds (see [`crate::templates`]) —
//! runs `npm install` for TypeScript when npm is found, builds the
//! package once with the same builder development mode builds with, and
//! puts the built components in the folder as `pane-ext dev`'s first run
//! does; then Pane's ordinary install preview shows the package, the
//! author confirms it there, and Pane develops the package from then on,
//! each save rebuilding and reloading it. A missing tool (Node and npm,
//! or rustup with the `wasm32-wasip2` target) is named, with where to
//! get it; a build failure is the build's own first error, with the
//! folder and the log.
//!
//! Import Extension asks for the folder of a package that already exists
//! and shows the same install preview of it — a folder without
//! `pane.json`, or one whose components are not built, is explained by
//! Pane's own messages, since the picker cannot look for them — and
//! developing starts the same way once the author installs it: as Manage
//! extensions' local install with development does today, without the
//! folder being copied anywhere.
//!
//! What both leave in `State.develop_after` is dropped when the
//! preview is left: not installing the package is the author's answer.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use super::developing::slot;
use super::off_thread;
use super::pane_form::{self, PaneFieldKind, PaneForm, PaneFormField};
use super::{FormPurpose, Launcher, State, Status, next_form_id};
use crate::develop::PaneManifest;
use crate::packages::PackageIdentity;
use crate::templates::{self, Kind, Language, Name};

/// The id of the root row that starts Create Extension.
pub(in crate::launcher) const CREATE_EXTENSION: &str = "pane.create-extension";

/// The id of the root row that starts Import Extension.
pub(in crate::launcher) const IMPORT_EXTENSION: &str = "pane.import-extension";

/// The id of the Create Extension form's name field.
pub(in crate::launcher) const NAME_FIELD: &str = "name";

/// The id of its language field.
const LANGUAGE_FIELD: &str = "language";

/// The id of its template field.
const TEMPLATE_FIELD: &str = "template";

/// Which of Pane's own root rows asks for a folder, which the window
/// picks before the launcher acts (see [`Launcher::selected_folder_ask`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderAsk {
    /// Create Extension: the parent folder a new package is written into.
    Create,
    /// Import Extension: the folder of a package that already exists.
    Import,
}

/// What submitting the Create Extension form set running: the parent
/// folder the window picked, and the package the form's fields name.
pub(in crate::launcher) struct Creation {
    parent: PathBuf,
    name: Name,
    language: Language,
    kind: Kind,
}

/// The template's label on the form's choice, as `--template` spells its
/// id.
fn template_label(kind: Kind) -> &'static str {
    match kind {
        Kind::List => "List",
        Kind::Detail => "Detail",
        Kind::Form => "Form",
        Kind::NoView => "No view",
    }
}

/// The choices of the form's template field, one per template.
fn template_choices() -> Vec<(String, String)> {
    Kind::ALL
        .iter()
        .map(|kind| (kind.name().to_owned(), template_label(*kind).to_owned()))
        .collect()
}

impl Launcher {
    /// Shows the Create Extension form for the parent folder the window
    /// picked: the extension's name, its language and its template. The
    /// window asks for the folder (see [`FolderAsk::Create`]) as it asks
    /// for the folder an Install-from-folder picks, and calls this once it
    /// is chosen; activating the row itself does nothing in the launcher.
    pub fn show_create_form(&self, parent: &Path) {
        let form = PaneForm::of(
            next_form_id(),
            "Create Extension",
            "Create extension",
            vec![
                PaneFormField {
                    key: NAME_FIELD.into(),
                    kind: PaneFieldKind::Text,
                    title: "Extension name".into(),
                    placeholder: Some("such as Word Count".into()),
                    info: None,
                    value: String::new(),
                    required: true,
                },
                PaneFormField {
                    key: LANGUAGE_FIELD.into(),
                    kind: PaneFieldKind::Choice(vec![
                        (
                            Language::TypeScript.name().to_ascii_lowercase(),
                            Language::TypeScript.name().to_owned(),
                        ),
                        (
                            Language::Rust.name().to_ascii_lowercase(),
                            Language::Rust.name().to_owned(),
                        ),
                    ]),
                    title: "Language".into(),
                    placeholder: None,
                    info: None,
                    value: Language::TypeScript.name().to_ascii_lowercase(),
                    required: false,
                },
                PaneFormField {
                    key: TEMPLATE_FIELD.into(),
                    kind: PaneFieldKind::Choice(template_choices()),
                    title: "Template".into(),
                    placeholder: None,
                    info: Some(
                        "List opens items to run, Detail answers a query, Form asks for \
                         values, No view runs without a screen"
                            .into(),
                    ),
                    value: Kind::List.name().into(),
                    required: false,
                },
            ],
            None,
        );
        let mut state = self.lock();
        pane_form::open_pane_form(
            &mut state,
            FormPurpose::Create(parent.to_path_buf()),
            form,
            "Create Extension".into(),
        );
    }

    /// Shows the install preview of the package in `folder`, which Pane
    /// develops once the author installs it: Import Extension's pick
    /// (#222). The folder is checked as any install is, so a folder
    /// without `pane.json`, or a source-only package whose components are
    /// not built, is explained by Pane's own messages.
    pub fn import_extension(&self, folder: &Path) -> impl Future<Output = ()> + Send + 'static {
        if let Ok(identity) = PackageIdentity::local(folder) {
            self.lock().develop_after = Some(identity);
        }
        self.preview_package(folder)
    }

    /// Reads the open Create Extension form in `state`, which its
    /// submission set running: the name is parsed, and a bad one is
    /// marked on its field as an extension's rejection is, with the form
    /// staying. `None` when no such form is open or submitting, or the
    /// name names no package.
    pub(in crate::launcher) fn begun_creation(
        state: &mut State,
        values: &[(String, String)],
    ) -> Option<Creation> {
        if !matches!(state.view.screen, super::Screen::PaneForm(_)) {
            return None;
        }
        let parent = match &state.form {
            Some(super::OpenForm {
                purpose: FormPurpose::Create(parent),
                submitting: false,
                ..
            }) => parent.clone(),
            _ => return None,
        };
        let value = |id: &str| {
            values
                .iter()
                .find(|(field, _)| field == id)
                .map(|(_, value)| value.clone())
                .unwrap_or_default()
        };
        let name = match Name::parse(&value(NAME_FIELD)) {
            Ok(name) => name,
            Err(problem) => {
                if let super::Screen::PaneForm(form) = &mut state.view.screen
                    && pane_form::mark_error(&mut form.tree, NAME_FIELD, &problem)
                {
                    state.view.status = Status::Error(format!("Extension name: {problem}"));
                }
                return None;
            }
        };
        // The choice fields' ids are the spellings the templates parse,
        // so their values always name one.
        let language = Language::parse(&value(LANGUAGE_FIELD)).unwrap_or(Language::TypeScript);
        let kind = Kind::parse(&value(TEMPLATE_FIELD)).unwrap_or(Kind::List);
        if let Some(open) = &mut state.form {
            open.submitting = true;
        }
        state.view.status = Status::Running {
            since: Instant::now(),
        };
        Some(Creation {
            parent,
            name,
            language,
            kind,
        })
    }

    /// Writes the package the form named, builds it once and hands it to
    /// Pane's ordinary install preview, which the author confirms before
    /// Pane develops the package: the flow the module's documentation
    /// describes. The status line says each step, and the window is told
    /// of each through the changes channel, as a development build's
    /// progress is.
    pub(in crate::launcher) async fn finish_creation(&self, epoch: u64, creation: Creation) {
        let Creation {
            parent,
            name,
            language,
            kind,
        } = creation;
        let title = name.title.clone();
        let folder = parent.join(&name.package);
        // The folder, off the window's thread: an author's files are never
        // written over, so a folder that is not empty is refused and the
        // form stays.
        let scaffold = {
            let (folder, name) = (folder.clone(), name.clone());
            off_thread(move || templates::scaffold(&folder, &name, language, kind)).await
        };
        if let Err(why) = scaffold {
            self.creation_failed(epoch, why);
            return;
        }
        // The build, with the same builder development mode builds with
        // (the one build crate, ADR 0047): a Pane without one cannot
        // create a package either.
        let Some(builder) = self.developing.builder() else {
            self.creation_failed(
                epoch,
                format!(
                    "Cannot build {title}: this Pane does not build extensions. The folder is \
                     {}; import the extension once it builds",
                    folder.display()
                ),
            );
            return;
        };
        // The language's tool, so a missing one is named with where to get
        // it: the build's own message names the tool when it cannot run,
        // and this says where to find it and what to run.
        if let Some(missing) = missing_tool(language) {
            let why = format!("{title} was written to {}, but {missing}", folder.display());
            self.creation_failed(epoch, why);
            return;
        }
        // TypeScript's dependencies, the author's step the templates'
        // README names: `npm install` writes the lockfile the build's
        // `npm ci` runs from, and the `node_modules` the package's own
        // scripts use. Out of the flow's way: a failure stops nothing,
        // and the build that follows explains what is missing, as it does
        // when npm install was skipped.
        if language == Language::TypeScript {
            let installing = format!("Installing {title}'s dependencies: npm install");
            self.creation_progress(epoch, installing);
            npm_install(&folder).await;
        }
        let Some(installation) = self.installation.as_ref() else {
            let why = "this launcher does not install packages".to_owned();
            self.creation_failed(epoch, why);
            return;
        };
        let identity = match PackageIdentity::local(&folder) {
            Ok(identity) => identity,
            Err(error) => {
                self.creation_failed(epoch, error.to_string());
                return;
            }
        };
        // Where this one build stages and keeps its log, in the package's
        // development folder.
        let work = installation.dir.join("develop").join(slot(&identity));
        let staging = work.join("create");
        let log = staging.join("build.log");
        // The first build: the status names the command, as a
        // development build's does, and the whole output goes to the log.
        let command = match builder.build_for(&folder) {
            Ok(build) => build.command(),
            Err(why) => {
                let message = format!("{title} was written to {}, but {why}", folder.display());
                self.creation_failed(epoch, message);
                return;
            }
        };
        let building = format!("Building {title}: {command}");
        self.creation_progress(epoch, building);
        let built = {
            let (builder, folder, staging, log) =
                (builder, folder.clone(), staging.clone(), log.clone());
            off_thread(move || {
                pane_build::build_package(&*builder, &PaneManifest, &folder, &staging, Some(&log))
            })
            .await
        };
        match built {
            Ok(_) => {
                // The built components into the folder the author picked,
                // as `pane-ext dev`'s first run does: the preview and the
                // install are of this build, and a later Reload reloads
                // it.
                {
                    let (staging, folder) = (staging.clone(), folder.clone());
                    off_thread(move || {
                        pane_build::copy_components(&PaneManifest, &staging, &folder)
                    })
                    .await
                };
                // The package is developed once the author installs it;
                // the preview the author confirms, as any install's.
                self.lock().develop_after = Some(identity);
                self.preview_package(&folder).await;
            }
            Err(failure) => {
                let whole = match &failure.log {
                    Some(log) => format!("the whole output is in {}", log.display()),
                    None => "Pane could not keep its output".into(),
                };
                self.creation_failed(
                    epoch,
                    format!(
                        "{title} did not build: {}. {whole}. The folder is {}; import the \
                         extension once it builds",
                        failure.summary,
                        folder.display()
                    ),
                );
            }
        }
    }

    /// Shows `why` a creation stopped, if the form is still on screen: the
    /// form stays, ready to submit again, and the window is told.
    fn creation_failed(&self, epoch: u64, why: String) {
        if let Some(mut state) = self.lock_if_current(epoch) {
            if let Some(open) = &mut state.form {
                open.submitting = false;
            }
            state.view.status = Status::Error(why);
        }
        self.developing.changed();
    }

    /// Shows `what` a creation is doing, if the form is still on screen,
    /// and tells the window.
    fn creation_progress(&self, epoch: u64, what: String) {
        if let Some(mut state) = self.lock_if_current(epoch) {
            state.view.status = Status::Progress(what);
        }
        self.developing.changed();
    }
}

/// The language's missing build tool, with where to get it and what to
/// run: Node.js (npm comes with it) for a TypeScript package, cargo
/// (rustup's, with the `wasm32-wasip2` target) for a Rust one. `None`
/// when the tool is there — the build's own message then names anything
/// else that is missing, as it always does.
fn missing_tool(language: Language) -> Option<String> {
    match language {
        Language::TypeScript if find_node().is_none() => Some(
            "Pane found no Node.js and npm to build it (it looked for node and npm on PATH; a \
             JavaScript or TypeScript package needs Node.js 22 or newer). Install Node.js from \
             https://nodejs.org and run npm install in the folder; import the extension once \
             it builds"
                .into(),
        ),
        Language::Rust if find_cargo().is_none() => Some(
            "Pane found no cargo to build it (it looked for cargo on PATH, then \
             ~/.cargo/bin/cargo). Install rustup from https://rustup.rs and the wasm32-wasip2 \
             target (rustup target add wasm32-wasip2); import the extension once it builds"
                .into(),
        ),
        _ => None,
    }
}

/// Node.js, from the search path, as Pane's JavaScript build finds it
/// (`pane-build`'s): npm comes with it.
fn find_node() -> Option<PathBuf> {
    find_on_path("node")
}

/// Cargo, from the search path then `~/.cargo/bin`, as Pane's Rust build
/// finds it: rustup's proxy, so the package's toolchain file applies.
fn find_cargo() -> Option<PathBuf> {
    find_on_path("cargo").or_else(|| {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
        let cargo = Path::new(&home)
            .join(".cargo/bin")
            .join(executable("cargo"));
        cargo.is_file().then_some(cargo)
    })
}

/// The first `name` (with `.exe` on Windows) on the search path.
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(executable(name)))
        .find(|candidate| candidate.is_file())
}

/// `name` as an executable is spelled on this system.
fn executable(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// `npm install` in the package's `folder`: whether it succeeded. Run off
/// the window's thread, with npm's Windows name (`npm.cmd`, a batch file
/// `Command` starts through `cmd`), as the build runs it; a failure stops
/// nothing — the build that follows explains what is missing.
async fn npm_install(folder: &Path) -> bool {
    let folder = folder.to_path_buf();
    off_thread(move || {
        let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
        let mut command = Command::new(npm);
        command
            .current_dir(&folder)
            .args(["install", "--ignore-scripts", "--no-audit", "--no-fund"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    })
    .await
}
