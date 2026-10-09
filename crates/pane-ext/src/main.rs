//! `pane-ext`, the command-line tool authors use beside the app to develop
//! an extension package (ADR 0047, #128). `pane-ext dev [folder]` builds the
//! package in the terminal and hands each build to the running Pane (see
//! `dev`); `new`, `check` and `pack` are to follow.

use std::path::PathBuf;
use std::process::ExitCode;

mod dev;
mod start;

const USAGE: &str = "\
Usage: pane-ext dev [folder]

  dev [folder]  Build the package in folder (the current folder by default)
                here, hand the build to the running Pane, starting Pane if
                none is running, then build it again after each save and
                have Pane reload it, until Ctrl+C.

  --version     Print pane-ext's version.
";

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let command = args.next();
    match command.as_ref().and_then(|command| command.to_str()) {
        Some("dev") => {
            let folder = args.next().map(PathBuf::from);
            if args.next().is_some() {
                return usage_error();
            }
            dev::run(folder)
        }
        Some("--version" | "-V") => {
            println!("pane-ext {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h" | "help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(later @ ("new" | "check" | "pack")) => {
            eprintln!("pane-ext: `pane-ext {later}` is not available yet; `pane-ext dev` is");
            ExitCode::FAILURE
        }
        _ => usage_error(),
    }
}

fn usage_error() -> ExitCode {
    eprint!("{USAGE}");
    ExitCode::from(2)
}
