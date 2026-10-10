//! Every template, in both languages, through what an author runs
//! (#221): `pane-ext new` with flags writes the package, the package
//! builds with the build development mode uses, installs through the
//! Launcher as Pane would, and its command opens and answers. This is the
//! CI seam for "every template is built and installed": the ordinary
//! test tiers run it, with no job of its own.
//!
//! The templates name the registry packages an author installs
//! (`pane-extension` from crates.io, `@pane-app/extension` and
//! `@pane-app/cli` from npm), which are not published yet (#281), so the
//! test stands them in with this repository's own copies, as the
//! development samples do: the Rust template's dependency points at
//! `guests/pane-extension`, and the TypeScript template's SDK devDependency
//! at a copy of `guests/js` beside the package, which `npm install` (the
//! author's step) resolves. `@pane-app/cli` is dropped instead: the build
//! never runs it, and the staging provides the SDK alone. The componentizer
//! is pane-ext's own, linked in.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use futures::executor::block_on;
use pane_core::develop::{BuildJob, BuildOutcome, Builder, Toolchains};
use pane_core::templates::{Kind, Language};
use pane_core::{Launcher, Manifest, Runtime, Screen, Status, ToastStyle};

/// The kinds, with the title each package is scaffolded by: `Template
/// List` and so on, so one launcher can install all four.
const KINDS: [Kind; 4] = Kind::ALL;

/// What the no-view template's command says when it was sent nothing,
/// as its own source spells it.
const NOTHING: &str = "Nothing yet: give this command an alias, or make it a fallback, then \
                       type into root search";

/// The title the `kind` package is scaffolded with.
fn title(kind: Kind) -> &'static str {
    match kind {
        Kind::List => "Template List",
        Kind::Detail => "Template Detail",
        Kind::Form => "Template Form",
        Kind::NoView => "Template No View",
    }
}

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Scaffolds the `kind` template in `language` with `pane-ext new` (what
/// an author runs), into Cargo's test folder where the builds below keep
/// what they made between runs, and stands the templates' unpublished
/// dependencies in with the repository's own copies.
fn scaffold(kind: Kind, language: Language) -> PathBuf {
    let parent = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(match language {
        Language::Rust => "templates-rust",
        Language::TypeScript => "templates-typescript",
    });
    let folder = parent.join(kind.name().replace('-', "_"));
    let _ = fs::remove_dir_all(&folder);
    let given = [
        "new",
        folder.to_str().unwrap(),
        "--name",
        title(kind),
        "--language",
        match language {
            Language::Rust => "rust",
            Language::TypeScript => "typescript",
        },
        "--template",
        kind.name(),
    ];
    let output = Command::new(env!("CARGO_BIN_EXE_pane-ext"))
        .args(given)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "pane-ext new {} failed: {}{}",
        kind.name(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    match language {
        Language::Rust => {
            // The crate comes from crates.io for an author; here it is the
            // repository's own, as the development samples' is, with the
            // repository's toolchain file over the template's stable one
            // (its `wasm32-wasip2` target is what the build needs).
            let manifest = fs::read_to_string(folder.join("Cargo.toml")).unwrap();
            let sdk = repository().join("guests/pane-extension");
            let dependency = format!("pane-extension = {{ path = {:?} }}", sdk.display());
            let manifest = manifest.replace(r#"pane-extension = "0.1""#, &dependency);
            fs::write(folder.join("Cargo.toml"), manifest).unwrap();
            fs::copy(
                repository().join("rust-toolchain.toml"),
                folder.join("rust-toolchain.toml"),
            )
            .unwrap();
        }
        Language::TypeScript => {
            // The SDK comes from npm for an author; here it is a copy of
            // the repository's own, beside the package as the development
            // samples' is, so the `file:../js` devDependency resolves both
            // in the package folder and in the build's staging of it (the
            // staging provides the SDK alone). The unpublished
            // `@pane-app/cli`, whose `pane-ext` the scripts run and this
            // test never runs, is dropped rather than stubbed.
            copy_folder(&repository().join("guests/js"), &parent.join("js"));
            let package = fs::read_to_string(folder.join("package.json")).unwrap();
            let cli_dep = "    \"@pane-app/cli\": \"0.1.0\",\n";
            let sdk_dep = r#""@pane-app/extension": "0.1.0""#;
            let package = package
                .replace(cli_dep, "")
                .replace(sdk_dep, r#""@pane-app/extension": "file:../js""#);
            fs::write(folder.join("package.json"), package).unwrap();
            // The author's step: install the dependencies the build runs
            // (the package's tsc and esbuild among them).
            let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
            let output = Command::new(npm)
                .current_dir(&folder)
                .args(["install", "--ignore-scripts", "--no-audit", "--no-fund"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "npm install in {} failed: {}{}",
                folder.display(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    folder.canonicalize().unwrap()
}

/// Copies `from` into `to`, replacing what is there.
fn copy_folder(from: &Path, to: &Path) {
    let _ = fs::remove_dir_all(to);
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_folder(&entry.path(), &path);
        } else {
            fs::copy(entry.path(), &path).unwrap();
        }
    }
}

/// Builds the package in `folder` once, as development mode does, and
/// puts its components in the folder, as its author would before
/// installing it.
fn build(folder: &Path) {
    let started = Instant::now();
    let toolchains = Toolchains::from_env(None);
    let build = toolchains.build_for(folder).unwrap();
    let staging = tempfile::tempdir().unwrap();
    fs::copy(folder.join("pane.json"), staging.path().join("pane.json")).unwrap();
    let job = BuildJob::new(staging.path().to_path_buf());
    let outcome = build.run(&job);
    assert_eq!(outcome, BuildOutcome::Built, "{:#?}", job.output());
    for command in Manifest::read(staging.path()).unwrap().commands {
        let component = folder.join(&command.component);
        fs::create_dir_all(component.parent().unwrap()).unwrap();
        let _ = fs::remove_file(&component);
        fs::copy(staging.path().join(&command.component), &component).unwrap();
    }
    println!(
        "built {} in {} s",
        folder.display(),
        started.elapsed().as_secs()
    );
}

/// What the launcher shows of the last outcome: the status line, else the
/// toast in the footer.
fn shown(launcher: &Launcher) -> Status {
    let status = launcher.view().status;
    if status != Status::Idle {
        return status;
    }
    match launcher.toast() {
        None => Status::Idle,
        Some(shown) => match shown.toast.style {
            ToastStyle::Success => Status::Result(shown.toast.text()),
            ToastStyle::Failure => Status::Error(shown.toast.text()),
            ToastStyle::Animated => Status::Progress(shown.toast.text()),
        },
    }
}

/// Chooses the row `title` in `launcher` and activates it.
fn choose(launcher: &Launcher, title: &str) {
    let rows = launcher.view().rows;
    let Some(index) = rows.iter().position(|row| row.title == title) else {
        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        panic!("no row {title}: {titles:?}");
    };
    launcher.select(index);
    block_on(launcher.activate_selected());
}

/// The package in `folder` installed in a launcher of its own.
fn installed(folder: &Path) -> Launcher {
    let data = tempfile::tempdir().unwrap();
    let extensions = data.keep().join("extensions");
    let launcher = Launcher::with_packages(Ok(Runtime::start().unwrap()), vec![], extensions);
    block_on(launcher.install_package(folder));
    launcher.back();
    launcher
}

/// What the `kind` command does once the package is installed and its
/// row in root search is chosen.
fn opens(kind: Kind, launcher: &Launcher) {
    match kind {
        // The view commands open their screen; "Say hello" then answers
        // in a toast.
        Kind::List => {
            assert_eq!(launcher.view().screen, Screen::Command, "{kind:?}");
            choose(launcher, "Say hello");
            assert_eq!(
                shown(launcher),
                Status::Result(format!("Hello from {}", title(kind)))
            );
        }
        // Without text sent, the detail says so.
        Kind::Detail => {
            assert_eq!(launcher.view().screen, Screen::Command, "{kind:?}");
            let view = launcher.view();
            assert!(
                view.rows
                    .iter()
                    .any(|row| row.title.starts_with("Nothing yet")),
                "{:?}",
                view.rows
            );
        }
        // The form's item opens the form, which answers what was filled
        // in.
        Kind::Form => {
            assert_eq!(launcher.view().screen, Screen::Command, "{kind:?}");
            choose(launcher, "Greet someone");
            assert!(matches!(launcher.view().screen, Screen::Form(_)));
            launcher.set_field_value("name", "Ada");
            launcher.set_field_value("greeting", "hello");
            block_on(launcher.submit_form());
            assert_eq!(launcher.view().status, Status::Result("Hello, Ada".into()));
        }
        // The no-view command runs without a screen, root search staying
        // as it was, and answers in a toast.
        Kind::NoView => {
            assert_eq!(shown(launcher), Status::Result(NOTHING.into()));
        }
    }
}

#[test]
fn every_rust_template_builds_installs_and_opens() {
    for kind in KINDS {
        let folder = scaffold(kind, Language::Rust);
        build(&folder);
        let launcher = installed(&folder);
        choose(&launcher, title(kind));
        opens(kind, &launcher);
    }
}

#[test]
fn every_typescript_template_builds_installs_and_opens() {
    for kind in KINDS {
        let folder = scaffold(kind, Language::TypeScript);
        build(&folder);
        let launcher = installed(&folder);
        choose(&launcher, title(kind));
        opens(kind, &launcher);
    }
}
