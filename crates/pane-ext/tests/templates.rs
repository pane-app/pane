//! Every template, in both languages, through what an author runs
//! (#221): `pane-ext new` with flags writes the package, the package
//! builds with the build development mode uses, installs through the
//! Launcher as Pane would, and its command opens and answers. This is the
//! CI seam for "every template is built and installed": the ordinary
//! test tiers run it, with no job of its own.
//!
//! The templates name the registry packages an author installs
//! (`pane-extension` from crates.io, `@pane-app/extension` and
//! `@pane-app/cli` from npm). The test stands them in with this
//! repository's own copies, as the development samples do, so each
//! template builds against this checkout's SDKs, not the last ones
//! published: the Rust template's dependency points at
//! `guests/pane-extension`, and the TypeScript template's SDK
//! devDependency at a copy of `guests/js` beside the package, which
//! `npm install` (the author's step) resolves.
//! `@pane-app/cli` is dropped instead: the build never runs it, and the
//! staging provides the SDK alone. The componentizer is pane-ext's own,
//! linked in.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_build::stage_package;
use pane_core::develop::{BuildJob, BuildOutcome, Builder, PaneManifest, Toolchains};
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
/// what they made between runs, and stands the templates' registry
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
            let manifest = manifest.replace(r#"pane-extension = "0.2""#, &dependency);
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
            // staging provides the SDK alone). `@pane-app/cli`, whose
            // `pane-ext` the scripts run and this test never runs, is
            // dropped rather than stubbed.
            copy_folder(&repository().join("guests/js"), &parent.join("js"));
            let package = fs::read_to_string(folder.join("package.json")).unwrap();
            // The line is dropped whole, whatever the checkout's line
            // endings are: the Windows runner's text files are CRLF.
            let package = package
                .lines()
                .filter(|line| !line.contains("\"@pane-app/cli\":"))
                .collect::<Vec<_>>()
                .join("\n");
            let sdk_dep = r#""@pane-app/extension": "0.2.0""#;
            let package = package.replace(sdk_dep, r#""@pane-app/extension": "file:../js""#);
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
    // Staged as development mode stages it, the placeholder icon with the
    // manifest: a reload takes the package from the staged folder, so it
    // must read as the package it is: a staged build without the icon its
    // manifest names is refused, as every new extension's was (#486).
    stage_package(&PaneManifest, folder, staging.path()).unwrap();
    assert!(staging.path().join("icon.png").is_file());
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

/// Every text a designed tree holds, collected in document order.
fn tree_texts(node: &pane_core::Node, into: &mut Vec<String>) {
    if let pane_core::NodeKind::Text(text) = &node.kind {
        match &text.content {
            pane_core::TextContent::Plain(text) => into.push(text.clone()),
            pane_core::TextContent::Spans(spans) => {
                into.extend(spans.iter().map(|span| span.text.clone()));
            }
        }
    }
    for child in &node.children {
        tree_texts(child, into);
    }
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
        // Without text sent, the detail says so: the command's screen
        // draws its rows asynchronously, waited for as the samples' own
        // tests wait for their answers.
        Kind::Detail => {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let view = launcher.view();
                if matches!(view.screen, Screen::Command)
                    && view
                        .rows
                        .iter()
                        .any(|row| row.title.starts_with("Nothing yet"))
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "the detail never drew: {:?} (status {:?})",
                    view.screen,
                    view.status
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        // The form command opens its form as a designed view (#241),
        // whose submission answers what was filled in. The answer lands
        // asynchronously: the tree first draws the form's loading text,
        // the answer replacing it when the submission completes — waited
        // for as the samples' own tests wait for their answers, and read
        // as the tree's own text (a form's tree holds no list, so no
        // rows carry it).
        Kind::Form => {
            assert!(
                matches!(launcher.view().screen, Screen::DesignedView(_)),
                "{:?}",
                launcher.view().screen
            );
            block_on(launcher.submit_designed_form(
                None,
                vec![
                    (
                        "name".to_owned(),
                        pane_core::FormValue::Text("Ada".to_owned()),
                    ),
                    (
                        "greeting".to_owned(),
                        pane_core::FormValue::Text("hello".to_owned()),
                    ),
                ],
            ));
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let view = launcher.view();
                if let Screen::DesignedView(shown) = &view.screen {
                    let mut found = Vec::new();
                    tree_texts(&shown.tree.root, &mut found);
                    let joined = found.join(" ");
                    if joined.contains("Hello, Ada") {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "{kind:?}: the submission's answer never drew: {joined:?} ({:?})",
                        view.status
                    );
                } else {
                    assert!(
                        Instant::now() < deadline,
                        "{kind:?}: the form's screen left: {:?}",
                        view.screen
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
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
