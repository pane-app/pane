//! `pane-ext`, the command-line tool authors use beside the app to create,
//! develop, check and pack an extension package (ADR 0047, #128).
//! `pane-ext dev [folder]` builds the package in the terminal and hands
//! each build to the running Pane (see `dev`); `check [folder]` reports
//! everything Pane would refuse at install, with Pane's own messages, plus
//! the authoring lint rules and the package's own eslint (see `check`);
//! `new` and `pack` are to follow.

use std::path::PathBuf;
use std::process::ExitCode;

mod check;
mod dev;
mod start;

const USAGE: &str = "\
Usage: pane-ext dev [folder]
       pane-ext check [folder] [options]

  dev [folder]  Build the package in folder (the current folder by default)
                here, hand the build to the running Pane, starting Pane if
                none is running, then build it again after each save and
                have Pane reload it, until Ctrl+C.

  check [folder]
                Check the package in folder (the current folder by default)
                as Pane checks it at install, with Pane's own messages: its
                manifest, its built components (WASI 0.3 only, the extension
                API shape), its helper files, and the authoring lint rules,
                plus the package's own eslint when it has one. Errors exit
                non-zero, so CI can fail a broken release; warnings do not,
                unless --deny-warnings is given.

    --json           Print one machine-readable report instead, with each
                     problem's lint id, for an editor or CI to show inline.
    --deny-warnings  Fail on warnings as well as errors.

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
        Some("check") => {
            let (folder, json, deny_warnings) = match check_arguments(args) {
                Ok(arguments) => arguments,
                Err(problem) => {
                    eprintln!("pane-ext: {problem}");
                    return usage_error();
                }
            };
            check::run(folder, json, deny_warnings)
        }
        Some("--version" | "-V") => {
            println!("pane-ext {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h" | "help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(later @ ("new" | "pack")) => {
            eprintln!("pane-ext: `pane-ext {later}` is not available yet; `dev` and `check` are");
            ExitCode::FAILURE
        }
        _ => usage_error(),
    }
}

/// The arguments of `check`: an optional folder, then `--json` and
/// `--deny-warnings` in any order.
fn check_arguments(args: std::env::ArgsOs) -> Result<(Option<PathBuf>, bool, bool), String> {
    let mut folder = None;
    let mut json = false;
    let mut deny_warnings = false;
    for argument in args {
        if argument == "--json" {
            json = true;
        } else if argument == "--deny-warnings" {
            deny_warnings = true;
        } else if argument.to_string_lossy().starts_with('-') {
            return Err(format!(
                "`{}` is not an argument check takes: a package folder, --json or \
                 --deny-warnings",
                argument.to_string_lossy()
            ));
        } else if folder.replace(PathBuf::from(&argument)).is_some() {
            return Err("check takes one package folder, not several".into());
        }
    }
    Ok((folder, json, deny_warnings))
}

fn usage_error() -> ExitCode {
    eprint!("{USAGE}");
    ExitCode::from(2)
}
