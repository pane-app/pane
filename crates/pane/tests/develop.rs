//! Development mode in the native window, on GPUI's test platform: the
//! author develops an installed package from Manage extensions, and the
//! window redraws on its own when a save builds it, when the build fails and
//! when the package is reloaded. The build is a stand-in that copies the
//! guest named in the folder's `source.txt`; `pane-core`'s tests cover the
//! rest of development mode, and its `develop_builds.rs` the real builds.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use pane::LauncherWindow;
use pane_core::develop::{Build, BuildJob, BuildOutcome, Builder};
use pane_core::{Launcher, Runtime, Screen, Status};

#[path = "support/settle.rs"]
mod settle;

use settle::{enter_flow, until};

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

struct FakeBuilder;

struct FakeBuild(PathBuf);

impl Builder for FakeBuilder {
    fn build_for(&self, folder: &Path) -> Result<Arc<dyn Build>, String> {
        Ok(Arc::new(FakeBuild(folder.to_path_buf())))
    }
}

impl Build for FakeBuild {
    fn command(&self) -> String {
        "fake build".into()
    }

    fn ignores(&self, path: &Path) -> bool {
        path == Path::new("hello.wasm")
    }

    fn run(&self, job: &BuildJob) -> BuildOutcome {
        let source = fs::read_to_string(self.0.join("source.txt")).unwrap();
        let source = source.trim();
        if source.starts_with("error") {
            job.line(source);
            return BuildOutcome::Failed("fake build failed".into());
        }
        fs::copy(guest(source), job.staging().join("hello.wasm")).unwrap();
        BuildOutcome::Built
    }
}

fn package(folder: &Path) -> PathBuf {
    fs::create_dir_all(folder).unwrap();
    fs::write(
        folder.join("pane.json"),
        r#"{
  "manifestVersion": 1,
  "title": "Hello",
  "apiVersion": "0.1",
  "commands": [{ "id": "hello", "title": "Say hello", "component": "hello.wasm" }]
}"#,
    )
    .unwrap();
    fs::write(folder.join("source.txt"), "sample_rust").unwrap();
    fs::copy(guest("sample_rust"), folder.join("hello.wasm")).unwrap();
    folder.to_path_buf()
}

fn select(window: &Entity<LauncherWindow>, cx: &mut VisualTestContext, title: &str) {
    cx.update_entity(window, |window, _| {
        let launcher = window.launcher();
        let index = launcher
            .view()
            .rows
            .iter()
            .position(|row| row.title == title)
            .unwrap_or_else(|| panic!("no row {title}"));
        launcher.select(index);
    });
}

#[gpui::test]
fn the_window_shows_a_failed_build_and_the_reload_after_a_fix(cx: &mut TestAppContext) {
    let (sources, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = package(&sources.path().join("hello"));
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);
    let (sender, changes) = pane_core::changes::channel();
    let launcher =
        Launcher::with_packages(Runtime::start(), vec![], data.path().join("extensions"))
            .with_development(Arc::new(FakeBuilder), sender);
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(launcher, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher.preview_package(&folder, window, cx);
        launcher
    });
    until(&window, cx, |view| {
        matches!(view.screen, Screen::Package { .. })
    });
    cx.simulate_keystrokes("enter");
    until(&window, cx, |view| {
        view.status == Status::Result("Installed Hello".into())
    });

    // The extension list, as Settings enters it (#168).
    enter_flow(&window, cx);
    select(&window, cx, "Develop Hello");
    cx.simulate_keystrokes("enter");
    until(
        &window,
        cx,
        |view| matches!(&view.status, Status::Result(text) if text.starts_with("Developing Hello")),
    );
    assert!(cx.debug_bounds("row-Stop developing Hello").is_some());

    // A save that does not build: the window shows it by itself.
    fs::write(folder.join("source.txt"), "error: expected `;`").unwrap();
    until(&window, cx, |view| matches!(view.status, Status::Error(_)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("status-error").is_some());
    assert!(cx.debug_bounds("row-Why Hello did not build").is_some());

    // Its details, then a fixed save: reloaded, back on the extension list.
    select(&window, cx, "Why Hello did not build");
    cx.simulate_keystrokes("enter");
    until(&window, cx, |view| {
        matches!(view.screen, Screen::BuildDetails { .. })
    });
    assert!(cx.debug_bounds("detail-error: expected `;`").is_some());
    fs::write(folder.join("source.txt"), "sample_js").unwrap();
    until(&window, cx, |view| {
        view.status == Status::Result("Reloaded Hello".into())
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("status-result").is_some());
    assert!(cx.debug_bounds("row-Why Hello did not build").is_none());
    assert!(cx.debug_bounds("row-Stop developing Hello").is_some());
}
