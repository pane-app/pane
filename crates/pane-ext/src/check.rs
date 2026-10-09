//! `pane-ext check [folder] [--json] [--deny-warnings]`: everything Pane
//! would refuse at install, before anything is published (#224, ADR 0047).
//!
//! The checks are pane-core's own (`pane_core::check`), so every message is
//! the one Pane gives the user installing the same package, word for word.
//! The package's own eslint runs where it has one — a configuration and
//! eslint among its devDependencies — through npm, inside the package
//! folder with the package's locked dependencies; `pane-ext` never
//! reimplements a tool it can run (ADR 0047). A package without one, or a
//! computer without npm, simply skips it: the leg is noted, never a
//! refusal.
//!
//! Errors exit non-zero, so a CI job that runs `pane-ext check` fails a
//! broken release; warnings do not, unless `--deny-warnings` is given.
//! `--json` prints one machine-readable report (a stable document: its
//! `version` field changes when its shape does) instead of the readable
//! one, for an editor or CI to show inline.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use pane_core::check::{CheckProblem, CheckReport};
use serde_json::json;

/// The version of the `--json` report's shape: bumped when the report's
/// fields change, so a reader can tell.
const REPORT_VERSION: u32 = 1;

/// Checks the package in `folder`, or the current folder, printing the
/// problems as a person reads them or as one JSON document.
pub(crate) fn run(folder: Option<PathBuf>, json: bool, deny_warnings: bool) -> ExitCode {
    let folder = match folder {
        Some(folder) => folder,
        None => match std::env::current_dir() {
            Ok(folder) => folder,
            Err(error) => {
                eprintln!("pane-ext: the current folder cannot be read: {error}");
                return ExitCode::FAILURE;
            }
        },
    };
    let mut report = pane_core::check::check_package(&folder);
    let eslint = run_eslint(&folder, &mut report);
    let failed = !report.ok() || (deny_warnings && !report.warnings.is_empty());
    if json {
        print!("{}", json_report(&folder, &report, &eslint));
    } else {
        human_report(&folder, &report);
    }
    match (failed, report.ok()) {
        (false, _) => ExitCode::SUCCESS,
        (true, true) => {
            // Only warnings failed it, and only because they were asked to.
            let count = report.warnings.len();
            let plural = if count == 1 { "" } else { "s" };
            eprintln!(
                "pane-ext: {count} warning{plural} failed the check, as --deny-warnings asks"
            );
            ExitCode::FAILURE
        }
        (true, false) => {
            if !json {
                let count = report.errors.len();
                let plural = if count == 1 { "" } else { "s" };
                eprintln!(
                    "pane-ext: {} has {count} problem{plural} Pane would refuse it for; fix \
                     them and check again",
                    folder.display()
                );
            }
            ExitCode::FAILURE
        }
    }
}

/// The problems of `report`, for a person: one line each, with what to do.
fn human_report(folder: &Path, report: &CheckReport) {
    println!("pane-ext: checking {}", folder.display());
    for problem in &report.errors {
        println!("error: {}", problem.message);
    }
    for problem in &report.warnings {
        println!("warning: {}", problem.message);
    }
    if report.ok() && report.warnings.is_empty() {
        println!(
            "pane-ext: {} is a package Pane would install",
            folder.display()
        );
    } else if report.ok() {
        println!(
            "pane-ext: {} is a package Pane would install, with {} warning{}",
            folder.display(),
            report.warnings.len(),
            if report.warnings.len() == 1 { "" } else { "s" }
        );
    }
}

/// The problems of `report` as one JSON document, for a tool: stable in
/// shape, with each problem's lint id and the file it is in when known.
fn json_report(folder: &Path, report: &CheckReport, eslint: &Eslint) -> String {
    let problem = |problem: &CheckProblem| {
        let mut entry = json!({
            "id": problem.id,
            "message": problem.message,
        });
        if let Some(file) = &problem.file {
            entry
                .as_object_mut()
                .expect("the entry is an object")
                .insert("file".into(), json!(file));
        }
        entry
    };
    let document = json!({
        "version": REPORT_VERSION,
        "folder": folder.display().to_string(),
        "ok": report.ok(),
        "errors": report.errors.iter().map(problem).collect::<Vec<_>>(),
        "warnings": report.warnings.iter().map(problem).collect::<Vec<_>>(),
        "eslint": {
            "ran": eslint.ran(),
            "note": eslint.note(),
        },
    });
    let mut text =
        serde_json::to_string_pretty(&document).expect("a report of strings is always writable");
    text.push('\n');
    text
}

/// Whether the package's own eslint ran, and what to say about it: what
/// it found is in the report's errors, in eslint's own words, not here.
enum Eslint {
    /// Nothing ran, for the reason.
    Skipped(String),
    /// It ran: what it found, if anything, is already in the report.
    Ran,
}

impl Eslint {
    fn ran(&self) -> bool {
        matches!(self, Eslint::Ran)
    }

    /// Why nothing ran, for the report.
    fn note(&self) -> Option<String> {
        match self {
            Eslint::Skipped(reason) => Some(reason.clone()),
            Eslint::Ran => None,
        }
    }
}

/// Runs the package's own eslint, in `folder`, when it has one, and adds
/// what it found to `report`. The tool's own output is relayed, because
/// `pane-ext` never reimplements what it can run: the problems are
/// eslint's to say, in its own words.
fn run_eslint(folder: &Path, report: &mut CheckReport) -> Eslint {
    if !has_eslint(folder) {
        return Eslint::Skipped("the package has no eslint of its own".into());
    }
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    if !on_path(npm) {
        return Eslint::Skipped(format!(
            "the package has eslint, but npm was not found to run it (looked for `{npm}` on PATH)"
        ));
    }
    // `npm exec` runs the eslint the package installed, and `--offline`
    // keeps it to that: a package whose dependencies are not installed is
    // told to install them rather than fetched from behind its back.
    let output = Command::new(npm)
        .current_dir(folder)
        .args(["exec", "--offline", "--", "eslint", "."])
        .stdin(Stdio::null())
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            report.errors.push(CheckProblem {
                id: ESLINT,
                file: None,
                message: format!("the package's eslint could not run: {error}"),
            });
            return Eslint::Ran;
        }
    };
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprint!("{printed}");
    if !printed.is_empty() && !printed.ends_with('\n') {
        eprintln!();
    }
    if output.status.success() {
        return Eslint::Ran;
    }
    report.errors.push(CheckProblem {
        id: ESLINT,
        file: None,
        message: "the package's own eslint found problems (its output is above)".into(),
    });
    Eslint::Ran
}

/// The lint id of the package's own eslint, when it runs.
const ESLINT: &str = "eslint";

/// Whether the package in `folder` has an eslint of its own to run: a
/// `package.json` whose devDependencies carry eslint, and a configuration
/// for it (eslint's own file, or one in `package.json`).
fn has_eslint(folder: &Path) -> bool {
    let Ok(manifest) = std::fs::read_to_string(folder.join("package.json")) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&manifest) else {
        // Pane's npm install rules refuse a package.json that is not JSON
        // long before this; a check of a package that is not one of those
        // does not run its lint on the strength of a broken file.
        return false;
    };
    let declares = manifest
        .get("devDependencies")
        .and_then(|dev| dev.get("eslint"))
        .is_some();
    if !declares {
        return false;
    }
    let config = [
        "eslint.config.js",
        "eslint.config.mjs",
        "eslint.config.cjs",
        "eslint.config.yaml",
        "eslint.config.yml",
        "eslint.config.json",
        ".eslintrc.js",
        ".eslintrc.cjs",
        ".eslintrc.json",
        ".eslintrc.yaml",
        ".eslintrc.yml",
        ".eslintrc",
    ]
    .iter()
    .any(|file| folder.join(file).is_file());
    config || manifest.get("eslintConfig").is_some()
}

/// Whether `program` (a name as the operating system spells it, such as
/// `npm` or `npm.cmd`) can be found on the PATH.
fn on_path(program: &str) -> bool {
    let path = match std::env::var_os("PATH") {
        Some(path) => path,
        None => return false,
    };
    std::env::split_paths(&path).any(|folder| {
        folder.join(program).is_file() || folder.join(format!("{program}.exe")).is_file()
    })
}

/// The ids a report's problems come from, for the tests to keep aligned
/// with the ones pane-core names.
#[cfg(test)]
mod tests {
    use super::*;
    use pane_core::check::{COMPONENT, MANIFEST, RUNTIME};

    #[test]
    fn a_package_without_eslint_has_none_to_run() {
        let folder = tempfile::tempdir().unwrap();
        assert!(!has_eslint(folder.path()));
        std::fs::write(folder.path().join("package.json"), "{}").unwrap();
        assert!(!has_eslint(folder.path()));
    }

    #[test]
    fn a_package_with_eslint_among_its_dev_dependencies_has_one() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(
            folder.path().join("package.json"),
            r#"{ "devDependencies": { "eslint": "^9" } }"#,
        )
        .unwrap();
        assert!(!has_eslint(folder.path()), "without a configuration");
        std::fs::write(folder.path().join("eslint.config.js"), "{}\n").unwrap();
        assert!(has_eslint(folder.path()));

        // A configuration inside package.json counts as one too.
        let other = tempfile::tempdir().unwrap();
        std::fs::write(
            other.path().join("package.json"),
            r#"{ "devDependencies": { "eslint": "^9" }, "eslintConfig": {} }"#,
        )
        .unwrap();
        assert!(has_eslint(other.path()));
    }

    #[test]
    fn the_report_names_the_ids_pane_core_defines() {
        // The report's ids are pane-core's, so a tool matching them does not
        // depend on this file's spelling of them.
        assert_eq!(MANIFEST, "manifest");
        assert_eq!(COMPONENT, "component");
        assert_eq!(RUNTIME, "runtime");
    }
}
