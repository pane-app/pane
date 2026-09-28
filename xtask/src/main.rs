//! Portable developer commands, run as `cargo xtask <command>`.
//!
//! - `guests`: build the Rust guests and copy them, with the prebuilt JS/TS
//!   sample components from `guests/prebuilt/`, into `target/guests/`.
//! - `js-guests`: rebuild the prebuilt JS/TS sample components with the pinned
//!   toolchain in `tools/componentize-js` (prerequisites: guests/README.md),
//!   then run `guests`.
//! - `ci`: build guests, then check formatting, lints and tests.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const GUEST_TARGET: &str = "wasm32-wasip2";

/// Components built by `js-guests` and committed, so that normal builds and
/// tests need no JavaScript toolchain.
const PREBUILT: &[&str] = &["sample_js", "sample_ts"];

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    let result = match task.as_deref() {
        Some("guests") => guests(),
        Some("js-guests") => js_guests(),
        Some("ci") => ci(),
        _ => Err("usage: cargo xtask <guests|js-guests|ci>".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

fn run(command: &mut Command) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("failed to start {command:?}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command:?} failed with {status}"))
    }
}

/// Builds each guest workspace and copies its components to `target/guests`.
fn guests() -> Result<(), String> {
    let root = root();
    let out = root.join("target/guests");
    std::fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    // (workspace directory, component file names)
    let workspaces: [(&str, &[&str]); 2] = [
        ("guests", &["sample_rust", "faulty"]),
        ("guests/fixtures/mixed-p2", &["mixed_p2"]),
    ];
    for (dir, components) in workspaces {
        let dir = root.join(dir);
        run(cargo().current_dir(&dir).args([
            "build",
            "--locked",
            "--release",
            "--target",
            GUEST_TARGET,
        ]))?;
        for name in components {
            let built = dir.join(format!("target/{GUEST_TARGET}/release/{name}.wasm"));
            let dest = out.join(format!("{name}.wasm"));
            std::fs::copy(&built, &dest)
                .map_err(|error| format!("copy {} failed: {error}", built.display()))?;
        }
    }
    for name in PREBUILT {
        let prebuilt = root.join(format!("guests/prebuilt/{name}.wasm"));
        std::fs::copy(&prebuilt, out.join(format!("{name}.wasm")))
            .map_err(|error| format!("copy {} failed: {error}", prebuilt.display()))?;
    }
    println!("guests built into {}", out.display());
    Ok(())
}

/// Rebuilds `guests/prebuilt/` from the JS/TS sample sources, then refreshes
/// `target/guests/`. `PYTHON` names the interpreter if the default is absent.
fn js_guests() -> Result<(), String> {
    run(&mut pane_js("samples"))?;
    guests()
}

/// Runs a `tools/componentize-js/pane_js.py` subcommand with `PYTHON`, or the
/// platform's usual interpreter name.
fn pane_js(subcommand: &str) -> Command {
    let python = std::env::var_os("PYTHON")
        .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.into());
    let mut command = Command::new(python);
    command
        .current_dir(root())
        .args(["tools/componentize-js/pane_js.py", subcommand]);
    command
}

fn ci() -> Result<(), String> {
    guests()?;
    // The prebuilt JS/TS samples must match their sources and pins.
    run(&mut pane_js("check"))?;
    let root = root();
    run(cargo().current_dir(&root).args(["fmt", "--all", "--check"]))?;
    for dir in ["guests", "guests/fixtures/mixed-p2"] {
        run(cargo()
            .current_dir(root.join(dir))
            .args(["fmt", "--all", "--check"]))?;
    }
    run(cargo().current_dir(&root).args([
        "clippy",
        "--locked",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ]))?;
    run(cargo()
        .current_dir(&root)
        .args(["test", "--locked", "--workspace"]))
}
