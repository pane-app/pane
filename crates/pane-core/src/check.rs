//! The checks `pane-ext check` runs (#224, ADR 0047): exactly the ones
//! Pane runs at install, plus the authoring lint rules, collected as a list
//! of problems rather than stopped at the first refusal.
//!
//! Pane's own messages are the messages: the manifest and its components
//! are checked through Pane's own code — `Manifest::read` and
//! `Runtime::check` — so what the command-line tool tells an author is
//! word for word what Pane would tell the user installing the package, and
//! the two can never disagree. The lint rules are advice to the author, not
//! refusals: they live here, in Pane's core, so Pane can show them later in
//! the same words.
//!
//! Beyond install, the author's eye differs from the user's in one way: an
//! author ships a package for every system it declares, so a helper whose
//! file for another target is missing is reported — as a warning, since
//! `cargo xtask guests` assembles Pane's own sample packages with only the
//! current system's helper file, while the file for this system, which
//! Pane's install refuses the package without, stays an error.

use std::path::Path;

use futures::executor::block_on;

use crate::icons::{self, Icon, IconSource};
use crate::packages::{MANIFEST_FILE, Manifest, ManifestCommand, PackageError};
use crate::preferences::HELP_FILE;
use crate::runtime::Runtime;

/// One problem [`check_package`] reports: an error Pane would refuse the
/// package for at install, or a warning about something allowed but looking
/// wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckProblem {
    /// The lint rule the problem belongs to, stable for tools and CI to
    /// match on: see the constants below.
    pub id: &'static str,
    /// The file the problem is in, relative to the package folder, when
    /// known; `None` for a problem about the package as a whole.
    pub file: Option<String>,
    /// The message, in Pane's words for the errors (the same ones install
    /// shows) and the authoring rules' own words for the warnings.
    pub message: String,
}

/// Everything [`check_package`] found in a package: its errors, which Pane
/// would refuse at install and `pane-ext check` fails on, and its warnings,
/// which it reports but fails on only when asked to
/// (`pane-ext check --deny-warnings`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckReport {
    pub errors: Vec<CheckProblem>,
    pub warnings: Vec<CheckProblem>,
}

impl CheckReport {
    /// Whether the package passed: no errors, whatever its warnings.
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// The manifest, its components or its helper files failed one of Pane's
/// own install checks, with the message install shows.
pub const MANIFEST: &str = "manifest";
/// A component failed the checks install runs on it (compiling, WASI 0.3
/// imports only, the extension API's shape), with the message install
/// shows.
pub const COMPONENT: &str = "component";
/// The runtime that checks components could not start.
pub const RUNTIME: &str = "runtime";
/// A title is not in Title Case.
pub const TITLE_CASE: &str = "title-case";
/// The package has no icon, or its image is smaller than a published
/// extension's.
pub const ICON: &str = "icon";
/// A required preference without a default has no `HELP.md` to show on the
/// Setup screen.
pub const HELP: &str = "help";
/// A declared helper target has no file in the package.
pub const HELPER_FILE: &str = "helper-file";
/// The package has no description, or nowhere to report its problems: fine
/// to install, lacking for a published one.
pub const PUBLICATION: &str = "publication";
/// The tarball `pane-ext pack` would write, or the folder a release
/// revision of a Git-distributed package holds, holds what Pane's
/// unpacking rules refuse: a link, a name some system cannot write, more
/// entries or bytes than Pane allows (#225).
pub const PACKED: &str = "packed";
/// An npm package's `package.json` `files` list does not cover what the
/// tarball needs (#225).
pub const FILES: &str = "files";

/// Checks the package in `folder` as Pane's install does, and then as an
/// author would want: the manifest through Pane's own reading of it
/// (so every refusal carries install's message), each built component
/// through the runtime's own check (the WASI 0.3 and API-shape rules, each
/// refusal in install's words), the file for every target a helper
/// declares, and the lint rules above. Every problem is collected — install
/// stops at the first, but an author wants them all.
pub fn check_package(folder: &Path) -> CheckReport {
    check_with(folder, false)
}

/// As [`check_package`], for a package about to be published (`pane-ext
/// pack`, #225): the one rule that changes is the icon's — a package with
/// no icon of its own, or one smaller than a published extension's
/// 512×512, is an error rather than a warning, because once the package
/// ships there is no author left to fix it.
pub fn check_published_package(folder: &Path) -> CheckReport {
    check_with(folder, true)
}

/// The checks of [`check_package`], with the icon rule failing when the
/// package is to be published.
fn check_with(folder: &Path, published: bool) -> CheckReport {
    let mut report = CheckReport::default();
    let manifest = match Manifest::read(folder) {
        Ok(manifest) => manifest,
        Err(error) => {
            report.errors.push(CheckProblem {
                id: MANIFEST,
                file: Some(MANIFEST_FILE.into()),
                message: error.to_string(),
            });
            return report;
        }
    };
    check_helper_files(&manifest, folder, &mut report);
    check_components(&manifest, folder, &mut report);
    lint(&manifest, folder, &mut report, published);
    report
}

/// Checks that every component the manifest names passes the checks
/// install runs on it, each with the message install would show.
fn check_components(manifest: &Manifest, folder: &Path, report: &mut CheckReport) {
    let runtime = match Runtime::start() {
        Ok(runtime) => runtime,
        Err(error) => {
            report.errors.push(CheckProblem {
                id: RUNTIME,
                file: None,
                message: error.to_string(),
            });
            return;
        }
    };
    let mut checked = Vec::new();
    for (name, component) in manifest.components() {
        // A component serving several commands or operations is checked
        // once, for everything it serves, as install checks it.
        if checked.contains(&component) {
            continue;
        }
        checked.push(component);
        let exports = manifest.exports_of(component);
        let source = folder.join(component);
        let result = block_on(runtime.check_with(&source, exports));
        if let Err(error) = result {
            let error = PackageError::Component {
                command: name,
                error,
            };
            report.errors.push(CheckProblem {
                id: COMPONENT,
                file: Some(component.display().to_string()),
                message: error.to_string(),
            });
        }
    }
}

/// Checks that the file for every target a helper declares is in the
/// package: the one for this system Pane's install already refused the
/// package without (so it is an error that never reaches here), and the
/// others are the author's to ship, reported as warnings.
fn check_helper_files(manifest: &Manifest, folder: &Path, report: &mut CheckReport) {
    for helper in &manifest.helpers {
        for (target, file) in &helper.targets {
            if crate::helpers::runner::check_file(folder, file, *target).is_ok() {
                continue;
            }
            // The file for this system is an error, as Pane's install makes
            // one: reading the manifest normally already reported it, and
            // this covers what changed since. The files for the other
            // targets are the author's to ship: warnings, because `cargo
            // xtask guests` assembles Pane's own sample packages with only
            // the current system's helper file.
            let for_this_system =
                pane_target::Target::current().is_some_and(|this| *target == this);
            let problem = if for_this_system {
                let error = PackageError::Helper {
                    helper: helper.id.clone(),
                    target: *target,
                    reason: "its file is missing or is not a program for this system".into(),
                }
                .to_string();
                CheckProblem {
                    id: HELPER_FILE,
                    file: Some(file.display().to_string()),
                    message: error,
                }
            } else {
                CheckProblem {
                    id: HELPER_FILE,
                    file: Some(file.display().to_string()),
                    message: format!(
                        "the helper `{}` declares the target {}, whose file {} is not in the \
                         package: Pane installs the file for this system only, but a published \
                         package ships one for every target it declares",
                        helper.id,
                        target,
                        file.display()
                    ),
                }
            };
            if for_this_system {
                report.errors.push(problem);
            } else {
                report.warnings.push(problem);
            }
        }
    }
}

/// The authoring lint rules: warnings about what Pane allows but a native
/// extension would not do. The icon rule is the one that differs for a
/// package about to be published: it fails instead of warning (see
/// [`check_published_package`]).
fn lint(manifest: &Manifest, folder: &Path, report: &mut CheckReport, published: bool) {
    // Titles are shown among Pane's own, which are in Title Case.
    lint_title(&manifest.title, None, report);
    for command in &manifest.commands {
        lint_title(&command.title, Some(command), report);
    }
    // A published extension has its own 512×512 icon; a package without one
    // installs and shows a first-letter tile.
    lint_icon(manifest.icon.as_ref(), folder, report, published);
    // The Setup screen a required preference without a default shows has
    // the package's HELP.md beside it.
    if !folder.join(HELP_FILE).is_file() {
        for preference in manifest
            .preferences
            .iter()
            .chain(manifest.commands.iter().flat_map(|c| c.preferences.iter()))
            .filter(|preference| preference.required && preference.default.is_none())
        {
            report.warnings.push(CheckProblem {
                id: HELP,
                file: Some(MANIFEST_FILE.into()),
                message: format!(
                    "the required preference `{}` has no default, so Pane shows its Setup \
                     screen, but the package has no {} beside {} to show on it",
                    preference.name, HELP_FILE, MANIFEST_FILE
                ),
            });
        }
    }
    // What a published package is expected to carry: a description, and
    // somewhere its users can report problems.
    if manifest.description.is_none() {
        report.warnings.push(CheckProblem {
            id: PUBLICATION,
            file: Some(MANIFEST_FILE.into()),
            message: "the package has no description: Pane shows one in its install preview \
                and in the Extensions group in Settings, and a published package needs one"
                .into(),
        });
    }
    if manifest.issues.is_none() && manifest.repository.is_none() {
        report.warnings.push(CheckProblem {
            id: PUBLICATION,
            file: Some(MANIFEST_FILE.into()),
            message: "the package declares neither `issues` nor `repository`: a user who runs \
                into a problem has nowhere to report it, and Pane's \"Report issue\" has \
                nothing to open"
                .into(),
        });
    }
}

/// Warns when `title`, of the package or of `command`, is not in Title
/// Case, with what it would be.
fn lint_title(title: &str, command: Option<&ManifestCommand>, report: &mut CheckReport) {
    let suggested = title_case(title);
    if suggested == title {
        return;
    }
    let whose = match command {
        None => "the package's title".to_owned(),
        Some(command) => format!("the title of command `{}`", command.id),
    };
    report.warnings.push(CheckProblem {
        id: TITLE_CASE,
        file: Some(MANIFEST_FILE.into()),
        message: format!("{whose} is not in Title Case: \"{suggested}\""),
    });
}

/// Warns, or fails when the package is to be published, when the package
/// has no icon of its own, or when its image is smaller than the 512×512
/// a published extension's icon is.
fn lint_icon(icon: Option<&Icon>, folder: &Path, report: &mut CheckReport, published: bool) {
    let Some(problem) = icon_problem(icon, folder) else {
        return;
    };
    if published {
        report.errors.push(problem);
    } else {
        report.warnings.push(problem);
    }
}

/// The problem with the package's icon (in `folder`), in the words the npm
/// and Git installs' caution uses: none at all, or an image smaller than a
/// published extension's. Whether it fails the package is for the caller
/// to say.
fn icon_problem(icon: Option<&Icon>, folder: &Path) -> Option<CheckProblem> {
    let Some(icon) = icon else {
        return Some(CheckProblem {
            id: ICON,
            file: None,
            message: format!(
                "the package has no icon of its own, so Pane shows a first-letter tile for \
                 it; a published extension's icon is a {}×{} image",
                icons::PUBLISHED_ICON_SIZE,
                icons::PUBLISHED_ICON_SIZE
            ),
        });
    };
    let IconSource::Image { light, dark } = &icon.source else {
        return None;
    };
    for file in [light, dark] {
        if !icons::is_png(file) {
            continue;
        }
        let Some((width, height)) = icons::png_size(&folder.join(file)) else {
            return Some(CheckProblem {
                id: ICON,
                file: Some(file.display().to_string()),
                message: format!(
                    "the package's icon {} is not a PNG image Pane can read",
                    file.display()
                ),
            });
        };
        if width < icons::PUBLISHED_ICON_SIZE || height < icons::PUBLISHED_ICON_SIZE {
            return Some(CheckProblem {
                id: ICON,
                file: Some(file.display().to_string()),
                message: format!(
                    "the package's icon {} is {width}×{height}, smaller than the \
                     {}×{} a published extension's icon is",
                    file.display(),
                    icons::PUBLISHED_ICON_SIZE,
                    icons::PUBLISHED_ICON_SIZE
                ),
            });
        }
    }
    None
}

/// The words that are left lowercase in a title, everywhere but its first
/// and last word: the usual English convention, as Pane's own titles are
/// written.
const SMALL_WORDS: [&str; 16] = [
    "a", "an", "and", "as", "at", "but", "by", "for", "in", "nor", "of", "on", "or", "the", "to",
    "with",
];

/// `title` in Title Case, as the lint rule reads it: each word starts with
/// an uppercase letter, except the small words in the middle, which stay
/// lowercase. The rest of each word is left as written, so names like
/// \"TypeScript\" or \"API\" keep their own capital letters.
fn title_case(title: &str) -> String {
    let words: Vec<&str> = title.split_whitespace().collect();
    let words = words
        .iter()
        .enumerate()
        .map(|(at, word)| {
            let first = at == 0;
            let last = at + 1 == words.len();
            let small = SMALL_WORDS
                .iter()
                .any(|small| small.eq_ignore_ascii_case(word));
            if !first && !last && small {
                return word.to_lowercase();
            }
            let mut letters = word.chars();
            match letters.next() {
                Some(first) => first.to_uppercase().chain(letters).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>();
    words.join(" ")
}

/// A title-case check for the tests; `check_package` exercises the rest
/// through real packages.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_cased_as_panes_own_are() {
        assert_eq!(title_case("Calculator"), "Calculator");
        assert_eq!(title_case("Show preferences"), "Show Preferences");
        assert_eq!(title_case("Paste the Last Item"), "Paste the Last Item");
        assert_eq!(title_case("the last item"), "The Last Item");
        // Small words stay lowercase in the middle, and names keep their
        // own capital letters.
        assert_eq!(title_case("Copy and Paste"), "Copy and Paste");
        assert_eq!(title_case("a unit for TypeScript"), "A Unit for TypeScript");
        // The last word is never left lowercase.
        assert_eq!(title_case("notes to copy"), "Notes to Copy");
    }
}
