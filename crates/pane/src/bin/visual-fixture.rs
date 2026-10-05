//! `pane-visual-fixture`: the visual workbench's native fixture (#91).
//!
//! `pane-visual-fixture --scenario <name> --manifest <file> [--data-dir
//! <dir>] [--theme dark|light] [--material glass|opaque] [--perturb
//! none|row-padding-plus-4|selected-fill]` opens one scenario's window of
//! production components and writes its manifest after the first frame;
//! `pane-visual-fixture --registry <file>` writes the scenario registry
//! the capture helpers follow. `scripts/visual-workbench.ps1` runs it;
//! see `docs/visual-workbench.md`.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use pane::visual_fixture::{execute, parse_args};

fn main() {
    if let Err(error) = parse_args(std::env::args().skip(1)).and_then(execute) {
        eprintln!("pane-visual-fixture: {error}");
        std::process::exit(2);
    }
}
