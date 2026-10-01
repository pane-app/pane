//! The update Pane applies by itself, in the native window on GPUI's test
//! platform: the window is told of the change by itself and redraws, so
//! the status line shows the update without any user action. The wiring is
//! the real app's order — the launcher's constructor starts the updater's
//! thread before `with_development` wires the channel that tells the
//! window — and the restart is the smoke's: the first check of a Pane
//! started again finds the newer version (run 36810472261's Windows and
//! macOS frames 267: the update applied, and the status line never showed
//! it, because the thread's launcher snapshot never saw the channel).
//!
//! The registry is a local one on 127.0.0.1 (`pane-core`'s test support);
//! nothing reaches the network.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::TestAppContext;
use pane::LauncherWindow;
use pane_core::develop::Toolchains;
use pane_core::npm::Registry as NpmRegistry;
use pane_core::{Launcher, Runtime, Status};

#[path = "../../pane-core/tests/support/npm_registry.rs"]
mod npm_registry;

use npm_registry::{Registry, greeter_files, pack};

#[path = "support/settle.rs"]
mod settle;

use settle::until;

const NAME: &str = "@pane-samples/greeter";

/// The launcher as the real app builds it up to the development wiring:
/// the constructor — whose installation starts the updater's thread — and
/// the registry, before `with_development` (which wires the channel that
/// tells the window of background changes) runs after, so a thread holding
/// a launcher snapshot from before the wiring still has to reach the
/// window.
fn launcher(runtime: &Runtime, data: &tempfile::TempDir, registry: &Registry) -> Launcher {
    Launcher::with_packages(Ok(runtime.clone()), vec![], data.path().join("extensions"))
        .with_npm_registry(NpmRegistry::local(registry.url()).unwrap())
}

#[gpui::test]
fn an_update_pane_applies_by_itself_is_drawn_without_any_user_action(cx: &mut TestAppContext) {
    let guests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests");
    let registry = Registry::start();
    registry.publish(NAME, "0.1.0", pack(&greeter_files(&guests, "0.1.0")));
    let data = tempfile::tempdir().unwrap();
    cx.executor().allow_parking();
    cx.update(pane::bind_keys);

    // The earlier Pane: 0.1.0 installed, then stopped, and 0.2.0 published.
    let runtime = Runtime::start().unwrap();
    let first = launcher(&runtime, &data, &registry);
    futures::executor::block_on(first.install_npm(NAME));
    drop(first);
    registry.publish(NAME, "0.2.0", pack(&greeter_files(&guests, "0.2.0")));

    let (sender, changes) = pane_core::changes::channel();

    // The Pane started again, as the smoke restarts it: its first check,
    // a second after it starts, finds 0.2.0 and applies it. The window
    // follows the launcher's background changes, and nothing here touches
    // it: the update's result has to arrive by itself, in a drawn frame.
    // `with_development` runs after the constructor started the updater's
    // thread, as in the real app, so the thread's launcher snapshot was
    // taken before the channel existed.
    let second = launcher(&runtime, &data, &registry)
        .with_development(Arc::new(Toolchains::from_env(None)), sender);
    let (window, cx) = cx.add_window_view(|window, cx| {
        let mut launcher = LauncherWindow::new(second, window, cx);
        launcher.follow_changes(changes, window, cx);
        launcher
    });
    let view = until(&window, cx, |view| {
        view.status == Status::Result("Updated Greeter from npm to 0.2.0".into())
    });
    assert_eq!(
        view.status,
        Status::Result("Updated Greeter from npm to 0.2.0".into())
    );
}
