//! `pane-ext`, the command-line tool authors use beside the app to create,
//! develop, check and pack an extension package (ADR 0047, #128).
//! `pane-ext dev [folder]` builds the package in the terminal and hands
//! each build to the running Pane (see `dev`); `new [folder]` writes a
//! fresh package from the templates (see `new`); `check [folder]` reports
//! everything Pane would refuse at install, with Pane's own messages, plus
//! the authoring lint rules and the package's own eslint (see `check`);
//! `pack [folder]` builds the release components with the same code and
//! assembles what users will download, checked with Pane's own rules (see
//! `pack`).

use std::path::PathBuf;
use std::process::ExitCode;

mod check;
mod dev;
mod new;
mod pack;
mod start;

const USAGE: &str = "\
Usage: pane-ext dev [folder]
       pane-ext new [folder] [options]
       pane-ext new command <folder> [options]
       pane-ext check [folder] [options]
       pane-ext pack [folder]

  dev [folder]  Build the package in folder (the current folder by default)
                here, hand the build to the running Pane, starting Pane if
                none is running, then build it again after each save and
                have Pane reload it, until Ctrl+C.

  new [folder]  Write a fresh extension package into folder, from the
                templates, with the choices the options below give, or
                asked for in an interactive terminal. An existing folder
                is written into only while empty; without a folder, the
                package's own name becomes one: pane-ext new notes --name
                \"Word Count\"

    --name <name>        The extension's title (\"Word Count\"), from which
                         its package name and command id come; the folder's
                         name when not given.
    --language <lang>    rust or typescript (typescript by default).
    --template <kind>    list, detail, form or no-view (list by default).

  new command <folder>
                Add a command to the package in folder: an entry in its
                pane.json (served by the component its other commands
                are), a source file, and the entry file's dispatch arm,
                so the package still builds and the command runs.

    --id <id>            The command's id, its title's when not given.
    --title <title>      The command's title, its id's when not given.
    --template <kind>    list, detail, form or no-view (list by default).

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

  pack [folder]
                Build the release components of the package in folder (the
                current folder by default) with the same build development
                mode uses, then assemble and check what its users will
                download with Pane's own rules. An npm package (one with a
                package.json) is packed into the tarball npm pack makes, in
                the package's dist folder, with its `files` list checked to
                cover everything pane.json names; a Git-distributed one is
                checked as the folder its release revision will hold. A
                package without a 512×512 icon is an error here, not a
                warning. pack never publishes anything: publishing is the
                author's step, never pane-ext's.

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
        Some("new") => new::run(args),
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
        Some("pack") => {
            let folder = match pack_arguments(args) {
                Ok(folder) => folder,
                Err(problem) => {
                    eprintln!("pane-ext: {problem}");
                    return usage_error();
                }
            };
            pack::run(folder)
        }
        Some("--version" | "-V") => {
            println!("pane-ext {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h" | "help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => usage_error(),
    }
}

/// The arguments of `check`: an optional folder, then `--json` and
/// `--deny-warnings` in any order.
fn check_arguments(
    args: std::iter::Skip<std::env::ArgsOs>,
) -> Result<(Option<PathBuf>, bool, bool), String> {
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

/// The argument of `pack`: one optional package folder.
fn pack_arguments(args: std::iter::Skip<std::env::ArgsOs>) -> Result<Option<PathBuf>, String> {
    let mut folder = None;
    for argument in args {
        if argument.to_string_lossy().starts_with('-') {
            return Err(format!(
                "`{}` is not an argument pack takes: a package folder",
                argument.to_string_lossy()
            ));
        } else if folder.replace(PathBuf::from(&argument)).is_some() {
            return Err("pack takes one package folder, not several".into());
        }
    }
    Ok(folder)
}

fn usage_error() -> ExitCode {
    eprint!("{USAGE}");
    ExitCode::from(2)
}
