//! The release notes' breaking-changes record (#321, ADR 0046): the
//! committed source each Pane release's notes carry, one Breaking
//! changes section per extension API version, each change with code
//! before and after, for Rust and for JS/TS —
//! `docs/breaking-changes.md`, written when a version moves. This check
//! is the WIT guard's sibling: #319 refuses a breaking change without a
//! new version, this refuses a new version without its announced
//! changes. It fails when the version `wit/` declares has no section in
//! the record, matched by the heading convention the record's own
//! header documents. While `wit/previous/` holds no world no version
//! has moved, and the check passes vacuously; once the folder exists
//! (#320 left the 0.1 world there), the current version's section is
//! required. `ci-lints` runs this beside the guard, in the shape of the
//! checks there: a failing message names the version, the record and
//! what to do, passing prints one line.

use std::collections::BTreeSet;
use std::path::Path;

/// Where the release notes' breaking-changes record lives: one section
/// per extension API version, each change with code before and after,
/// for Rust and for JS/TS (ADR 0046). Each Pane release whose extension
/// API moved carries the sections of the versions it moved, as they are
/// written in this file.
const RECORD: &str = "docs/breaking-changes.md";

/// Checks that the extension API version `wit/` declares has its
/// Breaking changes section in the record, once a version has moved:
/// `wit/previous/` holding the previous world is what a move left
/// behind. `ci-lints` runs this in the shape of the checks beside it: a
/// failing message names the version, the record and what to do, and
/// passing prints one line.
pub(crate) fn check() -> Result<(), String> {
    let root = crate::root();
    let mut problems = Vec::new();
    // Whether wit/previous/ holds the previous world: a version that
    // moved left it behind (#320). While it holds none, no version has
    // moved and the record is not asked for — the check passes
    // vacuously, as it did before 0.2 moved.
    let previous_world = root.join("wit/previous").is_dir();
    // The version and the record are read only while a section is asked
    // for, each read's problem pushed as it is met: the version with
    // api_versions' own reader, so the two checks cannot read the
    // number differently.
    let version = if previous_world {
        version_of(&root, &mut problems)
    } else {
        None
    };
    let record = if previous_world {
        read(&root, &mut problems)
    } else {
        None
    };
    if let Some(problem) = problem(previous_world, version, record.as_deref()) {
        problems.push(problem);
    }
    if problems.is_empty() {
        // Passing prints one line, as the checks beside this do.
        match version {
            Some((major, minor)) => println!(
                "xtask: the breaking changes of extension API {major}.{minor} are \
                 recorded in {RECORD}"
            ),
            None => println!(
                "xtask: wit/previous holds no world, so no extension API version has \
                 moved and the record is not asked for"
            ),
        }
        return Ok(());
    }
    Err(failure(&problems))
}

/// The record's problem, if it has one, from the facts the check reads:
/// whether `wit/previous/` holds the previous world, the extension API
/// version `wit/`'s files declare, and the record's text — each `None`
/// when it was not read, because no world asked for it or because
/// reading it failed, which `check` has already reported. None when the
/// check passes: while no previous world exists no version has moved
/// and the record is not asked for; with the world, when the record
/// holds the version's section.
fn problem(
    previous_world: bool,
    version: Option<(u64, u64)>,
    record: Option<&str>,
) -> Option<String> {
    // While wit/previous/ holds no world, no version has moved: no
    // section is asked for, whatever the record holds.
    if !previous_world {
        return None;
    }
    // A wit/ that declares no one version: api_versions names the
    // details of a disagreement when it runs beside this, and alone
    // this says what cannot be done.
    let Some(version) = version else {
        return Some(
            "wit/ declares no one extension API version, so the record's section \
             cannot be asked for"
                .to_owned(),
        );
    };
    // A record that could not be read holds no section, as one without
    // the version's heading does: the message says what to write.
    if let Some(record) = record
        && has_section(record, version)
    {
        return None;
    }
    Some(format!(
        "{RECORD} holds no Breaking changes section for extension API {}.{}: write \
         the section — each change with code before and after, for Rust and for \
         JS/TS, under a `## Extension API {}.{}` heading (ADR 0046)",
        version.0, version.1, version.0, version.1
    ))
}

/// Whether the record holds the section for `version`, by the
/// convention its own header documents: a heading line that is exactly
/// `## Extension API X.Y` — that line alone, so a heading of another
/// level, one with more on its line, or the heading quoted inside a
/// fenced code block is not a section, and neither is another
/// version's: `## Extension API 0.20` is not 0.2's.
fn has_section(record: &str, version: (u64, u64)) -> bool {
    let heading = format!("## Extension API {}.{}", version.0, version.1);
    let mut fenced = false;
    for line in record.lines() {
        // A fenced block opens and closes with a line of backticks;
        // what it holds is quoted, not written.
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced && line.trim() == heading {
            return true;
        }
    }
    false
}

/// The extension API version the `.wit` files directly under `wit/`
/// declare, read with `api_versions`' own reader so the two checks
/// cannot read the number differently: every file's `package
/// pane:extension@…` line must agree on one, which `api_versions`
/// checks in its own detail when it runs beside this. None when the
/// files cannot be read, declare nothing, or disagree — each read
/// problem is pushed here, and the missing number is `problem`'s to
/// name.
fn version_of(root: &Path, problems: &mut Vec<String>) -> Option<(u64, u64)> {
    let dir = root.join("wit");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) => {
            problems.push(format!("read wit failed: {error}"));
            return None;
        }
    };
    // Only the package's own files, as api_versions walks them: `deps/`
    // holds WASI's own WIT, and `previous/` is the previous world,
    // which is never the version this asks for.
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_file() && name.ends_with(".wit") {
            names.push(name);
        }
    }
    names.sort();
    let mut versions = BTreeSet::new();
    for name in names {
        // A file that cannot be read is reported and counts as
        // declaring nothing.
        let version = match std::fs::read_to_string(dir.join(&name)) {
            Ok(text) => crate::api_versions::wit_package_version(&text),
            Err(error) => {
                problems.push(format!("read wit/{name} failed: {error}"));
                None
            }
        };
        if let Some((major, minor, _)) = version {
            versions.insert((major, minor));
        }
    }
    // One version among the files: the number the record's section is
    // asked for. None or more than one is not one number.
    if versions.len() != 1 {
        return None;
    }
    versions.into_iter().next()
}

/// The record's text, or the problem pushed for why it cannot be read.
fn read(root: &Path, problems: &mut Vec<String>) -> Option<String> {
    match std::fs::read_to_string(root.join(RECORD)) {
        Ok(text) => Some(text),
        Err(error) => {
            problems.push(format!("read {RECORD} failed: {error}"));
            None
        }
    }
}

/// The check's failure: every problem, and the rule — the WIT guard's
/// sibling, refusing a version that moved without its announced changes
/// as the guard refuses a breaking change without a new version (ADR
/// 0046).
fn failure(problems: &[String]) -> String {
    format!(
        "{}. A breaking change needs a new extension API version, and a new version \
         needs its Breaking changes section in the record (ADR 0046)",
        problems.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record in the shape the real one is written: a header that
    /// documents the convention (no line of it a section's heading),
    /// then one version's section.
    const RECORD: &str = "\
# Breaking changes

The record each Pane release's notes carry, its header documenting the
convention: a section's heading is a line of its own, exactly
`## Extension API X.Y` and nothing more.

## Extension API 0.2

- the change, with code before and after, for Rust and for JS/TS
";

    /// The message a version without its section is named by.
    const MISSING: &str = "docs/breaking-changes.md holds no Breaking changes section \
         for extension API 0.2: write the section — each change with code before \
         and after, for Rust and for JS/TS, under a `## Extension API 0.2` heading \
         (ADR 0046)";

    #[test]
    fn a_version_with_its_section_passes() {
        // A previous world exists, as #320 left it, and the record holds
        // the current version's section.
        assert_eq!(problem(true, Some((0, 2)), Some(RECORD)), None);
    }

    #[test]
    fn a_version_without_its_section_fails() {
        // Another version's section is not this version's, and a record
        // that could not be read holds no section at all.
        assert_eq!(
            problem(true, Some((0, 2)), Some("## Extension API 0.1\n")),
            Some(MISSING.to_owned())
        );
        assert_eq!(problem(true, Some((0, 2)), None), Some(MISSING.to_owned()));
        // A wit/ that declares no one version is named, too.
        assert_eq!(
            problem(true, None, Some(RECORD)),
            Some(
                "wit/ declares no one extension API version, so the record's section \
                 cannot be asked for"
                    .to_owned()
            )
        );
    }

    #[test]
    fn while_no_previous_world_exists_no_section_is_asked_for() {
        // The pass before a version moved: whatever the record holds and
        // whatever wit/ declares, no section is asked for.
        assert_eq!(problem(false, None, None), None);
        assert_eq!(problem(false, Some((0, 2)), Some("nothing")), None);
    }

    #[test]
    fn the_heading_convention_is_matched_exactly() {
        // The heading line, exactly.
        assert!(has_section("## Extension API 0.2\n", (0, 2)));
        // Indented as Markdown allows, with trailing spaces: still the
        // heading's own line.
        assert!(has_section("  ## Extension API 0.2  \n", (0, 2)));
        // Another version's heading is not this one's, whichever side
        // asks: 0.20 is not 0.2.
        assert!(!has_section("## Extension API 0.20\n", (0, 2)));
        assert!(!has_section("## Extension API 0.2\n", (0, 20)));
        // A version with a patch is not the convention's shape.
        assert!(!has_section("## Extension API 0.2.0\n", (0, 2)));
        // Another level, or more on the line, is not a section.
        assert!(!has_section("### Extension API 0.2\n", (0, 2)));
        assert!(!has_section("## Extension API 0.2 and 0.3\n", (0, 2)));
        // A mention of the convention in prose is not the line.
        assert!(!has_section(
            "its heading is exactly `## Extension API 0.2`\n",
            (0, 2)
        ));
        // The heading quoted inside a fenced block is not a section, and
        // a fence that is never closed quotes the rest: the check fails
        // closed, not open.
        assert!(!has_section("```\n## Extension API 0.2\n```\n", (0, 2)));
        assert!(!has_section("```\n## Extension API 0.2\n", (0, 2)));
    }

    #[test]
    fn the_version_is_read_from_wit_s_own_files() {
        // The walk api_versions makes: the `.wit` files directly under
        // wit/, the subfolders left out, one version among them.
        let root = std::env::temp_dir().join("xtask-release-notes-test");
        let wit = root.join("wit");
        std::fs::create_dir_all(wit.join("deps")).unwrap();
        std::fs::create_dir_all(wit.join("previous")).unwrap();
        std::fs::write(wit.join("a.wit"), "package pane:extension@0.2.0;\n").unwrap();
        std::fs::write(wit.join("b.wit"), "package pane:extension@0.2.0;\n").unwrap();
        std::fs::write(wit.join("readme.txt"), "no WIT\n").unwrap();
        // A foreign package's WIT under deps/, and the previous world's
        // under previous/: neither is a file this asks for.
        std::fs::write(wit.join("deps/wasi.wit"), "package wasi:cli@0.3.0;\n").unwrap();
        let old = wit.join("previous/old.wit");
        std::fs::write(&old, "package pane:extension@0.1.0;\n").unwrap();
        let mut problems = Vec::new();
        assert_eq!(version_of(&root, &mut problems), Some((0, 2)));
        assert!(problems.is_empty());
        // Files that disagree: no one number to ask for.
        std::fs::write(wit.join("b.wit"), "package pane:extension@0.3.0;\n").unwrap();
        let mut problems = Vec::new();
        assert_eq!(version_of(&root, &mut problems), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_record_that_cannot_be_read_is_a_problem() {
        // A root that holds no record, as a run beside a missing file.
        let root = std::env::temp_dir();
        let mut problems = Vec::new();
        assert!(read(&root, &mut problems).is_none());
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("read docs/breaking-changes.md failed:"));
    }

    #[test]
    fn the_failure_names_every_problem_and_the_rule() {
        assert_eq!(
            failure(&["one".to_owned(), "two".to_owned()]),
            "one; two. A breaking change needs a new extension API version, and a new \
             version needs its Breaking changes section in the record (ADR 0046)"
        );
    }
}
