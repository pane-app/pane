//! The template repository's content (#223, ADR 0047): the folder
//! `packages/extension-template` holds what the `pane-app/extension-template`
//! repository will hold — a TypeScript package from the `list` template,
//! a CI workflow that builds and checks it on every push and commits a
//! release revision on each `v<semver>` tag, and an AGENTS.md pointing at
//! the docs, the WIT files and the examples. Creating that repository is a
//! person's step, with the user's go-ahead; the content is kept here and
//! copied there on release, and these tests are what keeps it from
//! rotting: they run the same steps the content's workflow runs, with this
//! repository's own tools, in the ordinary test tiers — no job of their
//! own, the seam the templates' tests opened (`tests/templates.rs`).
//!
//! The workflow names the registry packages an author installs
//! (`@pane-app/cli`, `@pane-app/extension`), which are not published yet
//! (#281), so the tests stand them in as the templates' tests do: the SDK
//! devDependency points at a copy of `guests/js` beside the package,
//! `@pane-app/cli` is dropped — its `pane-ext` is the binary these tests
//! run — and the build componentizes with the componentizer pane-ext
//! links. The release steps run in a scratch Git repository the test
//! makes with the `git` program (as pane-core's repository tests do, with
//! none of the user's Git configuration): the source is committed and
//! tagged, the tag is moved onto the release commit as the workflow moves
//! it, and the tree at the tag — cloned out, with nothing of the working
//! folder's — is what Pane installs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use futures::executor::block_on;
use pane_core::{Launcher, Runtime, Screen, Status, ToastStyle};

/// The title the package in the template repository carries, and the toast
/// its "Say hello" item shows: the list template's own words, with the
/// name substituted.
const TITLE: &str = "Extension Template";
const GREETING: &str = "Hello from Extension Template";

/// The tag a release of the template repository's package is made under.
const TAG: &str = "v0.1.0";

/// This repository's root.
fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// The template repository's content, as this repository holds it.
fn content() -> PathBuf {
    let folder = repository().join("packages/extension-template");
    assert!(
        folder.join("pane.json").is_file(),
        "{} is missing; the template repository's content belongs there",
        folder.display()
    );
    folder
}

/// A scratch copy of the content, prepared as an author's clone of the
/// template repository is: copied out of this repository (so `npm install`
/// writes nothing in it), its unpublished `@pane-app` devDependencies
/// bridged — the SDK to a copy of `guests/js` beside it, `@pane-app/cli`
/// dropped, its `pane-ext` the binary these tests run — and the
/// dependencies installed, the author's step. Each `name` gets a folder of
/// its own, so the tests that run at once cannot rewrite one another's
/// SDK copy. Answers the copy's folder.
fn prepared(name: &str) -> PathBuf {
    let parent = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("template-repository")
        .join(name);
    let folder = parent.join("package");
    let _ = fs::remove_dir_all(&folder);
    copy_folder(&content(), &folder);
    copy_folder(&repository().join("guests/js"), &parent.join("js"));
    let package = fs::read_to_string(folder.join("package.json")).unwrap();
    let cli_dep = "    \"@pane-app/cli\": \"0.1.0\",\n";
    let sdk_dep = r#""@pane-app/extension": "0.1.0""#;
    let package = package
        .replace(cli_dep, "")
        .replace(sdk_dep, r#""@pane-app/extension": "file:../js""#);
    fs::write(folder.join("package.json"), package).unwrap();
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
    folder
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

/// Whether `output` succeeded, and what it printed, standard output and
/// error together.
fn printed(output: Output) -> (bool, String) {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), text)
}

/// Runs `pane-ext` with `args`, answering whether it succeeded and what it
/// printed.
fn pane_ext(args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_pane-ext"))
        .args(args)
        .output()
        .unwrap();
    printed(output)
}

/// `git` with an environment of the test's own, run in `dir`: none of the
/// user's configuration is read, and the identity is fixed, so the commits
/// are made wherever the test runs. Answers whether it succeeded and what
/// it printed.
fn try_git(dir: &Path, home: &Path, args: &[&str]) -> (bool, String) {
    let mut command = Command::new("git");
    command.env_clear();
    for kept in ["PATH", "SYSTEMROOT", "TMP", "TEMP", "TMPDIR"] {
        if let Some(value) = std::env::var_os(kept) {
            command.env(kept, value);
        }
    }
    command
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
        .env("GIT_AUTHOR_NAME", "Pane tests")
        .env("GIT_AUTHOR_EMAIL", "tests@pane.invalid")
        .env("GIT_COMMITTER_NAME", "Pane tests")
        .env("GIT_COMMITTER_EMAIL", "tests@pane.invalid")
        .current_dir(dir)
        .args(args);
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{command:?}: {error}; this test needs `git`"));
    printed(output)
}

/// `git` in `dir`, which must succeed; answers what it printed.
fn git(dir: &Path, home: &Path, args: &[&str]) -> String {
    let (passed, text) = try_git(dir, home, args);
    assert!(passed, "git {} failed: {text}", args.join(" "));
    text.trim().to_owned()
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

/// The package in `folder` installed in a launcher of its own, as Pane
/// installs a folder a release revision holds.
fn installed(folder: &Path) -> Launcher {
    let data = tempfile::tempdir().unwrap();
    let extensions = data.keep().join("extensions");
    let launcher = Launcher::with_packages(Ok(Runtime::start().unwrap()), vec![], extensions);
    block_on(launcher.install_package(folder));
    launcher.back();
    launcher
}

/// The installed package's command, opened from root search and answered:
/// the list opens, and "Say hello" tells the user what every item's action
/// does.
fn opens(launcher: &Launcher) {
    choose(launcher, TITLE);
    assert_eq!(launcher.view().screen, Screen::Command);
    let view = launcher.view();
    let titles: Vec<&str> = view.rows.iter().map(|row| row.title.as_str()).collect();
    assert!(titles.contains(&"Say hello"), "{titles:?}");
    choose(launcher, "Say hello");
    assert_eq!(shown(launcher), Status::Result(GREETING.into()));
}

#[test]
fn the_package_is_the_list_template_as_pane_ext_new_writes_it() {
    // What the package in the template repository is: one scaffolded from
    // the `list` template under the name it carries, so the two cannot
    // drift apart. The files that differ — the manifests, the README —
    // are the repository's own, with the metadata a published package
    // carries; the icon is a file there rather than the placeholder
    // `pane-ext new` writes.
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("template-repository")
        .join("scaffold");
    let _ = fs::remove_dir_all(&folder);
    let given = [
        "new",
        folder.to_str().unwrap(),
        "--name",
        TITLE,
        "--language",
        "typescript",
        "--template",
        "list",
    ];
    let (passed, printed) = pane_ext(&given);
    assert!(passed, "{printed}");
    for file in [
        "src/index.ts",
        "tsconfig.json",
        "eslint.config.js",
        ".prettierrc.json",
        ".gitignore",
    ] {
        let scaffolded = fs::read_to_string(folder.join(file)).unwrap();
        let held = fs::read_to_string(content().join(file)).unwrap();
        assert_eq!(scaffolded, held, "{file} differs from the template's");
    }
}

#[test]
fn its_ci_builds_checks_and_installs_the_package() {
    // The workflow's push job, on a scratch copy: `npm run pack` builds
    // the components and checks what users will download, `npm run check`
    // adds the package's own eslint, and the package Pane would install is
    // the one an author's push leaves.
    let folder = prepared("check");
    let (passed, printed) = pane_ext(&["pack", folder.to_str().unwrap()]);
    assert!(passed, "{printed}");
    assert!(
        folder.join("dist/extension-template.wasm").is_file(),
        "{printed}"
    );
    let (passed, printed) = pane_ext(&["check", folder.to_str().unwrap()]);
    assert!(passed, "{printed}");
    assert!(
        printed.contains("is a package Pane would install"),
        "{printed}"
    );
    // The template is a model package: nothing an author would be warned
    // about either.
    assert!(!printed.contains("warning: "), "{printed}");
    opens(&installed(&folder));
}

#[test]
fn its_release_steps_tag_a_revision_pane_installs() {
    let folder = prepared("release");
    // The author's repository: the source alone on `main`, tagged with the
    // version the manifest names, as the workflow checks before it
    // releases.
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let pane = fs::read_to_string(folder.join("pane.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&pane).unwrap();
    assert_eq!(format!("v{}", manifest["version"]), TAG);
    git(&folder, home, &["init", "--quiet", "--initial-branch", "main"]);
    // `.gitignore` keeps `node_modules` and `dist` out, as an author's
    // does.
    git(&folder, home, &["add", "."]);
    git(
        &folder,
        home,
        &["commit", "--quiet", "-m", "Extension Template 0.1.0 source"],
    );
    git(&folder, home, &["tag", TAG]);
    // The workflow's release job, on the tagged commit: build the release
    // components (the tarball pack writes beside them is npm's shape, not
    // this repository's, so it is not committed), commit `dist`, and move
    // the tag onto the release commit.
    git(&folder, home, &["checkout", "--quiet", "--detach", TAG]);
    let (passed, printed) = pane_ext(&["pack", folder.to_str().unwrap()]);
    assert!(passed, "{printed}");
    for entry in fs::read_dir(folder.join("dist")).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().ends_with(".tgz") {
            fs::remove_file(entry.path()).unwrap();
        }
    }
    git(&folder, home, &["add", "-f", "dist"]);
    git(&folder, home, &["commit", "--quiet", "-m", "Release v0.1.0"]);
    git(&folder, home, &["tag", "-f", TAG]);
    // The tag now names the release revision, and only it: the source
    // branch stays source-only, which Pane explains to a user who names
    // it, and the workflow's next run on the moved tag stops at its
    // guard, the tag's tree already holding `dist`.
    let released = git(&folder, home, &["rev-parse", TAG]);
    let source = git(&folder, home, &["rev-parse", "main"]);
    assert_ne!(released, source, "the tag was not moved onto the release");
    let dist = format!("{TAG}:dist");
    let (held, printed) = try_git(&folder, home, &["cat-file", "-e", &dist]);
    assert!(held, "the tag's tree holds no dist: {printed}");
    let (held, _) = try_git(&folder, home, &["cat-file", "-e", "main:dist"]);
    assert!(!held, "the source branch is not source-only");
    // What Pane's Git client takes: the tree at the tag, cloned out with
    // nothing of the working folder's, holding the manifest and the
    // component it names. Pane installs that, and the command answers.
    git(&folder, home, &["switch", "--quiet", "main"]);
    let release = tempfile::tempdir().unwrap();
    let revision = release.path().join("revision");
    git(
        &folder,
        home,
        &[
            "clone",
            "--quiet",
            "--branch",
            TAG,
            folder.to_str().unwrap(),
            revision.to_str().unwrap(),
        ],
    );
    let _ = fs::remove_dir_all(revision.join(".git"));
    assert!(revision.join("pane.json").is_file());
    assert!(revision.join("dist/extension-template.wasm").is_file());
    assert!(!revision.join("node_modules").exists());
    opens(&installed(&revision));
}
