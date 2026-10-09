//! What `pane-ext pack` assembles and checks (#225, ADR 0047): the files
//! users will download, gathered from a package whose components are built
//! and checked with Pane's own rules, so what pack refuses is what Pane
//! would refuse and the two can never disagree.
//!
//! The package's release components are built by `pane-ext` with the same
//! `pane-build` code development mode uses and copied back into the package
//! folder before [`pack_package`] is called on it; nothing here builds.
//!
//! An npm-distributed package — one whose folder holds a `package.json` —
//! is packed into the tarball `npm pack` makes of it: every file under
//! `package/`, in a gzipped tar written the same on every system (fixed
//! times, owner and modes, files in name order), so a source serves one
//! integrity everywhere. The tarball holds exactly what Pane's install
//! copies into the managed copy — `pane.json`, the components, the icons
//! and assets it names, the helper files for every target it declares,
//! `HELP.md` — and the `package.json` npm itself needs; `package.json`'s
//! `files` list must cover all of that, so no package is published without
//! what its manifest names by accident. The tarball is written into the
//! package's `dist` folder, and never published: publishing is a person's
//! step (ADR 0047).
//!
//! A Git-distributed package has no tarball: its release revision is the
//! repository's tree itself, so pack checks the folder a release revision
//! will hold, as Pane's Git client takes one: regular files and folders
//! whose names every system can write, within the entry, byte and depth
//! bounds of a revision.
//!
//! A package without a 512×512 icon is refused rather than warned about
//! ([`crate::check::check_published_package`]): its author is the one left
//! to fix it.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::archive::MAX_ENTRIES;
use crate::check::{CheckProblem, CheckReport, FILES, PACKED};
use crate::downloads::check_part;
use crate::git::{MAX_DEPTH, MAX_ENTRIES as REVISION_ENTRIES, MAX_UNPACKED as REVISION_UNPACKED};
use crate::npm::{MAX_TARBALL, MAX_UNPACKED};
use crate::packages::{ASSETS_DIR, MANIFEST_FILE, Manifest, map_beside};
use crate::preferences::HELP_FILE;

/// npm's own fixed time for packed files, 1985-10-26T08:15:00Z: the
/// tarballs this repository packs (the npm sample, the default extensions'
/// payloads, the Linux package, and what `pane-ext pack` writes) are the
/// same on every system, so a source serves one integrity everywhere.
pub const PACKED_MTIME: u64 = 499_162_500;

/// The folder pack writes an npm package's tarball into, beside what it
/// packs: the folder a built release lives in.
const DIST: &str = "dist";

/// The folders no release revision holds: `.git`, which Pane's write-out
/// of a revision refuses, and the build folders, which no author commits.
/// What the manifest names inside them is still checked, as every file it
/// names is.
const NOT_IN_A_REVISION: [&str; 3] = [".git", "target", "node_modules"];

/// What [`pack_package`] found and made: every problem, and what users
/// will download.
pub struct Packed {
    /// Every problem: the errors Pane (or npm) would refuse the package
    /// for — install's own checks, the unpacking rules, the `files` list —
    /// and the warnings about what a published package should have.
    pub report: CheckReport,
    /// The tarball written for an npm package, with the files it holds;
    /// `None` when the package was refused, or is Git-distributed.
    pub tarball: Option<Tarball>,
    /// For a Git-distributed package: the folder a release revision will
    /// hold, as pack checked it. `None` otherwise, or when it was refused.
    pub revision: Option<Revision>,
}

/// The tarball `pane-ext pack` writes for an npm package, in its `dist`
/// folder: what its users download.
pub struct Tarball {
    /// Where it was written.
    pub path: PathBuf,
    /// The files it holds, in the order it holds them, relative to the
    /// package folder.
    pub files: Vec<PathBuf>,
    /// Its size in bytes.
    pub bytes: u64,
}

/// The folder a release revision of a Git-distributed package holds, as
/// pack checked it.
pub struct Revision {
    /// How many files and folders it holds.
    pub entries: usize,
    /// The bytes of its files, all together.
    pub bytes: u64,
}

/// Checks the package in `folder`, whose components are built, as
/// `pane-ext pack` does, and assembles what its users will download:
/// everything install checks, with the icon a published package needs as
/// an error; the files users download taken through Pane's unpacking
/// rules; and, for an npm package, `package.json`'s `files` list covering
/// it all, the tarball written into the package's `dist` folder when
/// nothing was refused — for a Git-distributed one, the folder checked as
/// the release revision it will hold. Nothing is published, and nothing
/// reaches the network.
pub fn pack_package(folder: &Path) -> Packed {
    let mut report = crate::check::check_published_package(folder);
    let Ok(manifest) = Manifest::read(folder) else {
        // The report says why, in Pane's words; nothing can be assembled.
        return Packed {
            report,
            tarball: None,
            revision: None,
        };
    };
    if folder.join("package.json").is_file() {
        pack_npm(folder, &manifest, &mut report)
    } else {
        pack_git(folder, &manifest, &mut report)
    }
}

/// Packs the npm package in `folder` (`manifest` read): assembles what
/// `npm pack` would hold, checks it with Pane's unpacking rules and the
/// `files` list, and writes the tarball when nothing is refused.
fn pack_npm(folder: &Path, manifest: &Manifest, report: &mut CheckReport) -> Packed {
    let package = package_json(folder, report);
    // What the tarball holds: everything install copies into the managed
    // copy, plus the package.json npm itself needs, in name order.
    let mut files = candidates(manifest, folder, report);
    files.push(PathBuf::from("package.json"));
    files.sort();
    files.dedup();
    // The files that are there to pack, and every rule they break.
    let packed = walk(folder, &files, report);
    if let Some(package) = &package {
        check_files_covered(&package.files, &packed, report);
    }
    let tarball = match (package, report.ok()) {
        (Some(package), true) => write_tarball(folder, &packed, &package, report),
        _ => None,
    };
    Packed {
        report: std::mem::take(report),
        tarball,
        revision: None,
    }
}

/// Checks the folder a release revision of the Git-distributed package in
/// `folder` (`manifest` read) will hold, as Pane's Git client takes one:
/// regular files and folders only, names every system can write and no
/// two differing only in case, within the entry, byte and depth bounds of
/// a revision. The folders no revision holds are skipped (see
/// [`NOT_IN_A_REVISION`]); what the manifest names inside them is still
/// name-checked, as it is for every file the manifest names.
fn pack_git(folder: &Path, manifest: &Manifest, report: &mut CheckReport) -> Packed {
    for file in named(manifest) {
        // Only what the walk skips; everything else in the tree is checked
        // as the walk takes it.
        let in_skipped = file
            .components()
            .next()
            .and_then(|part| part.as_os_str().to_str())
            .is_some_and(|part| NOT_IN_A_REVISION.contains(&part));
        if in_skipped
            && let Some(why) = bad_name(&file)
        {
            report.errors.push(CheckProblem {
                id: PACKED,
                file: Some(file.display().to_string()),
                message: format!(
                    "the package would ship `{}`, {why}; Pane takes only files and folders \
                     every system can write",
                    file.display()
                ),
            });
        }
    }
    let mut revision = Revision {
        entries: 0,
        bytes: 0,
    };
    walk_revision(folder, Path::new(""), 0, report, &mut revision);
    let revision = report.ok().then_some(revision);
    Packed {
        report: std::mem::take(report),
        tarball: None,
        revision,
    }
}

/// The npm `package.json` of the package in `folder`: its name, version
/// and `files` list, or the problem with reading one.
fn package_json(folder: &Path, report: &mut CheckReport) -> Option<PackageJson> {
    let refused = |message: String| {
        report.errors.push(CheckProblem {
            id: FILES,
            file: Some("package.json".into()),
            message,
        });
    };
    let text = match fs::read_to_string(folder.join("package.json")) {
        Ok(text) => text,
        Err(error) => {
            refused(format!("the package's package.json cannot be read: {error}"));
            return None;
        }
    };
    let json: serde_json::Value = match serde_json::from_str(&text) {
        Ok(json) => json,
        Err(error) => {
            refused(format!("the package's package.json cannot be read: {error}"));
            return None;
        }
    };
    let field = |name: &str| json.get(name).and_then(|value| value.as_str());
    let (Some(name), Some(version)) = (field("name"), field("version")) else {
        let missing = if field("name").is_none() {
            "name"
        } else {
            "version"
        };
        refused(format!(
            "the package's package.json names no `{missing}`, which npm needs to pack it"
        ));
        return None;
    };
    let files = json
        .get("files")
        .and_then(|files| files.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|file| file.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Some(PackageJson {
        name: name.to_owned(),
        version: version.to_owned(),
        files,
    })
}

/// An npm `package.json`, as pack reads it.
struct PackageJson {
    name: String,
    version: String,
    /// Its `files` list, as written; empty when there is none, in which
    /// case npm packs everything in the folder.
    files: Vec<String>,
}

/// The files the package offers users to download, in name order:
/// everything the manifest names (components, icons and their variants,
/// the helper files for every target it declares), the source map kept
/// beside a component, the `assets` folder whole, and `HELP.md` when the
/// package ships it — what install copies into the managed copy, which is
/// what the tarball holds besides `package.json`. A name that is not valid
/// UTF-8 is refused here, since nothing can be said of it.
fn candidates(manifest: &Manifest, folder: &Path, report: &mut CheckReport) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = vec![PathBuf::from(MANIFEST_FILE)];
    let mut add = |file: PathBuf| {
        if !files.contains(&file) {
            files.push(file);
        }
    };
    for (_, component) in manifest.components() {
        add(component.to_path_buf());
        if let Some(map) = map_beside(component) {
            add(map);
        }
    }
    for file in manifest.icon_files() {
        add(file);
    }
    for helper in &manifest.helpers {
        for file in helper.targets.values() {
            add(file.clone());
        }
    }
    // The help the Setup screen shows, when the package ships it as a
    // regular file: a link is not, and install copies no link.
    if fs::symlink_metadata(folder.join(HELP_FILE)).is_ok_and(|meta| meta.is_file()) {
        add(PathBuf::from(HELP_FILE));
    }
    // The `assets` folder, whole, as install copies it.
    let assets = folder.join(ASSETS_DIR);
    if fs::symlink_metadata(&assets).is_ok_and(|meta| meta.is_dir()) {
        collect(&assets, Path::new(ASSETS_DIR), &mut files, report);
    }
    files.sort();
    files
}

/// Adds the entries of the folder `folder` (`prefix` its path relative to
/// the package) to `files`, folders recursed into: everything there,
/// links included, for the walk to refuse what Pane's rules refuse.
fn collect(folder: &Path, prefix: &Path, files: &mut Vec<PathBuf>, report: &mut CheckReport) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            report.errors.push(CheckProblem {
                id: PACKED,
                file: None,
                message: format!(
                    "the package contains `{}`, whose name is not valid UTF-8; Pane unpacks \
                     only paths inside the package",
                    prefix.join(name.to_string_lossy()).display()
                ),
            });
            continue;
        };
        let path = prefix.join(name);
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            collect(&folder.join(name), &path, files, report);
        } else {
            files.push(path);
        }
    }
}

/// Walks the files `files` (relative to `folder`) as Pane's unpacker takes
/// a tarball: every name one every system takes — a name the manifest
/// declares is checked whether or not the file is there, since the package
/// is broken by declaring it — every entry there a regular file, for a
/// link is what Pane's unpacker refuses, and the entries and bytes within
/// npm's bounds. What is missing is left to install's own checks, which
/// have already said it. Returns the files that are there to pack.
fn walk(folder: &Path, files: &[PathBuf], report: &mut CheckReport) -> Vec<PathBuf> {
    let mut packed = Vec::new();
    let mut entries = 0;
    let mut bytes = 0;
    for file in files {
        if let Some(why) = bad_name(file) {
            report.errors.push(CheckProblem {
                id: PACKED,
                file: Some(file.display().to_string()),
                message: format!(
                    "the package would ship `{}`, {why}; Pane unpacks only paths inside the \
                     package",
                    file.display()
                ),
            });
        }
        let Ok(meta) = fs::symlink_metadata(folder.join(file)) else {
            continue;
        };
        if meta.is_file() {
            packed.push(file.clone());
            entries += 1;
            bytes = bytes.saturating_add(meta.len());
            if entries > MAX_ENTRIES {
                report.errors.push(CheckProblem {
                    id: PACKED,
                    file: None,
                    message: format!(
                        "the package holds more than {MAX_ENTRIES} entries, more than Pane \
                         unpacks"
                    ),
                });
                return packed;
            }
            if bytes > MAX_UNPACKED {
                report.errors.push(CheckProblem {
                    id: PACKED,
                    file: None,
                    message: format!(
                        "the package unpacks to more than the {} MiB Pane allows",
                        MAX_UNPACKED >> 20
                    ),
                });
                return packed;
            }
        } else if !meta.is_dir() {
            // A folder where a file is named is a missing file, which the
            // install checks have already said; anything else is what
            // Pane's unpacker refuses.
            report.errors.push(CheckProblem {
                id: PACKED,
                file: Some(file.display().to_string()),
                message: format!(
                    "the package contains {}, `{}`; Pane unpacks only files and folders",
                    what_entry(&meta.file_type()),
                    file.display()
                ),
            });
        }
    }
    packed
}

/// Walks the folder `folder` (`prefix` its path relative to the package,
/// `depth` how deep it nests) as Pane's Git client writes a revision out:
/// regular files and folders only, every name one every system can write
/// and no two in one folder differing only in case, within the entry, byte
/// and depth bounds of a revision. The folders no revision holds are
/// skipped at the root (see [`NOT_IN_A_REVISION`]).
fn walk_revision(
    folder: &Path,
    prefix: &Path,
    depth: usize,
    report: &mut CheckReport,
    tally: &mut Revision,
) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let mut seen: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            report.errors.push(CheckProblem {
                id: PACKED,
                file: None,
                message: format!(
                    "the package contains `{}`, whose name is not valid UTF-8; Pane takes only \
                     files and folders every system can write",
                    prefix.join(name.to_string_lossy()).display()
                ),
            });
            continue;
        };
        let path = prefix.join(name);
        let folded = name.to_lowercase();
        if seen.contains(&folded) {
            report.errors.push(CheckProblem {
                id: PACKED,
                file: Some(path.display().to_string()),
                message: format!(
                    "the package contains `{}`, whose name differs only in case from another in \
                     its folder, which some systems cannot hold both of; Pane takes only files \
                     and folders every system can write",
                    path.display()
                ),
            });
            continue;
        }
        seen.push(folded);
        if let Err(why) = check_part(name) {
            report.errors.push(CheckProblem {
                id: PACKED,
                file: Some(path.display().to_string()),
                message: format!(
                    "the package contains `{}`, {why}; Pane takes only files and folders every \
                     system can write",
                    path.display()
                ),
            });
            continue;
        }
        if depth == 0 && NOT_IN_A_REVISION.contains(&name) {
            continue;
        }
        tally.entries += 1;
        if tally.entries > REVISION_ENTRIES {
            report.errors.push(CheckProblem {
                id: PACKED,
                file: None,
                message: format!(
                    "the package holds more than {REVISION_ENTRIES} files and folders"
                ),
            });
            return;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth + 1 > MAX_DEPTH {
                report.errors.push(CheckProblem {
                    id: PACKED,
                    file: Some(path.display().to_string()),
                    message: format!(
                        "the package's folders nest more than {MAX_DEPTH} deep, at `{}`",
                        path.display()
                    ),
                });
                return;
            }
            walk_revision(&folder.join(name), &path, depth + 1, report, tally);
        } else if kind.is_file() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            tally.bytes = tally.bytes.saturating_add(meta.len());
            if tally.bytes > REVISION_UNPACKED {
                report.errors.push(CheckProblem {
                    id: PACKED,
                    file: None,
                    message: format!(
                        "the package's files take more than the {} MiB Pane allows",
                        REVISION_UNPACKED >> 20
                    ),
                });
                return;
            }
        } else {
            report.errors.push(CheckProblem {
                id: PACKED,
                file: Some(path.display().to_string()),
                message: format!(
                    "the package contains {}, `{}`; Pane takes only files and folders every \
                     system can write",
                    what_entry(&kind),
                    path.display()
                ),
            });
        }
    }
}

/// What an entry that is neither a file nor a folder is, in Pane's words
/// for it.
fn what_entry(kind: &std::fs::FileType) -> &'static str {
    if kind.is_symlink() {
        return "a symbolic link";
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if kind.is_fifo() {
            return "a named pipe";
        }
        if kind.is_char_device() || kind.is_block_device() {
            return "a device";
        }
        if kind.is_socket() {
            return "a socket";
        }
    }
    "an entry that is neither a file nor a folder"
}

/// Why some part of `path` is not a name every system takes, in the words
/// of Pane's unpacking rules ([`check_part`]), if it is not.
fn bad_name(path: &Path) -> Option<&'static str> {
    path.components()
        .filter_map(|part| part.as_os_str().to_str())
        .find_map(|part| check_part(part).err())
}

/// The files the manifest names, relative to the package folder: the
/// components, the icons with their variants, and the helper files for
/// every target it declares.
fn named(manifest: &Manifest) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = manifest
        .components()
        .map(|(_, component)| component.to_path_buf())
        .collect();
    for file in manifest.icon_files() {
        if !files.contains(&file) {
            files.push(file);
        }
    }
    for helper in &manifest.helpers {
        for file in helper.targets.values() {
            if !files.contains(file) {
                files.push(file.clone());
            }
        }
    }
    files
}

/// Checks that `package.json`'s `files` list covers every file the tarball
/// holds besides `package.json` itself, which npm always includes: a file
/// npm would leave out leaves the package Pane installs without something
/// its manifest names, and its author learns that here rather than from a
/// user. An empty list packs everything, as npm does.
fn check_files_covered(files: &[String], packed: &[PathBuf], report: &mut CheckReport) {
    if files.is_empty() {
        return;
    }
    let missing: Vec<String> = packed
        .iter()
        .filter(|file| file.as_os_str() != "package.json")
        .filter(|file| !covered(files, file))
        .map(|file| format!("`{}`", file.display()))
        .collect();
    if missing.is_empty() {
        return;
    }
    let them = if missing.len() == 1 { "it" } else { "them" };
    report.errors.push(CheckProblem {
        id: FILES,
        file: Some("package.json".into()),
        message: format!(
            "`files` does not cover {}: npm would pack the package without {them}, and Pane \
             would refuse it",
            missing.join(", ")
        ),
    });
}

/// Whether the `files` list covers `file` (relative to the package): some
/// entry names it, names a folder holding it, or matches it with `*` (any
/// run of characters but the folder separator) and `?` (one character)
/// within a path part, as npm's simple entries do; an entry starting `!`
/// takes the coverage away, as npm's exclusions do.
fn covered(files: &[String], file: &Path) -> bool {
    let Some(file) = file.to_str() else {
        return false;
    };
    let included = files.iter().filter(|entry| !entry.starts_with('!'));
    let excluded = files.iter().filter(|entry| entry.starts_with('!'));
    included.any(|entry| entry_covers(entry, file))
        && !excluded.any(|entry| entry_covers(&entry[1..], file))
}

/// Whether one `files` entry covers `file`.
fn entry_covers(entry: &str, file: &str) -> bool {
    let entry = entry.trim_end_matches('/');
    if entry == file {
        return true;
    }
    // A folder holds everything under it.
    if !entry.contains(['*', '?']) && file.starts_with(&format!("{entry}/")) {
        return true;
    }
    pattern_matches(entry, file)
}

/// Whether `pattern` matches `path` part by part: `*` any run of
/// characters but `/`, `?` one character. A pattern never crosses a
/// folder, as npm's do not.
fn pattern_matches(pattern: &str, path: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    if pattern.len() != path.len() {
        return false;
    }
    pattern
        .iter()
        .zip(&path)
        .all(|(part, name)| part_matches(part, name))
}

/// Whether `pattern` matches all of `name`, `*` standing for any run of
/// characters and `?` for one.
fn part_matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    // Whether pattern[i..] matches name[j..], built from the ends.
    let mut matches = vec![vec![false; name.len() + 1]; pattern.len() + 1];
    matches[pattern.len()][name.len()] = true;
    for i in (0..pattern.len()).rev() {
        for j in (0..=name.len()).rev() {
            matches[i][j] = match pattern[i] {
                '*' => matches[i + 1][j] || (j < name.len() && matches[i][j + 1]),
                '?' => j < name.len() && matches[i + 1][j + 1],
                c => j < name.len() && name[j] == c && matches[i + 1][j + 1],
            };
        }
    }
    matches[0][0]
}

/// Writes the tarball of `files` (relative to `folder`, in name order) as
/// `npm pack` writes one, into the package's `dist` folder, named as npm
/// names it: every file under `package/`, in a gzipped tar that is the
/// same on every system — ustar headers, mode 0644, owner 0, npm's fixed
/// time, files in name order — so a source serves one integrity
/// everywhere. Nothing is published.
fn write_tarball(
    folder: &Path,
    files: &[PathBuf],
    package: &PackageJson,
    report: &mut CheckReport,
) -> Option<Tarball> {
    let refused = |message: String, report: &mut CheckReport| {
        report.errors.push(CheckProblem {
            id: PACKED,
            file: None,
            message,
        });
    };
    let bytes = match tarball_bytes(folder, files) {
        Ok(bytes) => bytes,
        Err(error) => {
            refused(format!("the tarball cannot be written: {error}"), report);
            return None;
        }
    };
    if bytes.len() as u64 > MAX_TARBALL {
        refused(
            format!(
                "the tarball is larger than the {} MiB Pane downloads",
                MAX_TARBALL >> 20
            ),
            report,
        );
        return None;
    }
    let dist = folder.join(DIST);
    if let Err(error) = fs::create_dir_all(&dist) {
        refused(format!("the tarball cannot be written: {error}"), report);
        return None;
    }
    let path = dist.join(tarball_name(&package.name, &package.version));
    if let Err(error) = fs::write(&path, &bytes) {
        refused(format!("the tarball cannot be written: {error}"), report);
        return None;
    }
    Some(Tarball {
        path,
        files: files.to_vec(),
        bytes: bytes.len() as u64,
    })
}

/// The tarball of `files` as bytes: the tar `npm pack` makes of them.
fn tarball_bytes(folder: &Path, files: &[PathBuf]) -> std::io::Result<Vec<u8>> {
    let mut tar = tar::Builder::new(Vec::new());
    for file in files {
        let contents = fs::read(folder.join(file))?;
        let mut header = tar::Header::new_ustar();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(PACKED_MTIME);
        header.set_uid(0);
        header.set_gid(0);
        header.set_entry_type(tar::EntryType::Regular);
        tar.append_data(
            &mut header,
            format!("package/{}", file.display()),
            contents.as_slice(),
        )?;
    }
    let tar = tar.into_inner()?;
    let mut gz = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), flate2::Compression::best());
    gz.write_all(&tar)?;
    Ok(gz.finish()?)
}

/// The file npm names the tarball of `name` at `version`: the scope and
/// name joined with `-`, then the version, then `.tgz`.
fn tarball_name(name: &str, version: &str) -> String {
    let name = name.trim_start_matches('@').replace('/', "-");
    format!("{name}-{version}.tgz")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_files_entry_covers_a_file_a_folder_or_a_pattern() {
        let files = ["pane.json".to_owned(), "assets".to_owned()];
        assert!(covered(&files, &PathBuf::from("pane.json")));
        assert!(covered(&files, &PathBuf::from("assets/logo.png")));
        assert!(!covered(&files, &PathBuf::from("hello.wasm")));
        // A pattern matches within a path part, and never crosses a folder.
        let patterns = ["*.wasm".to_owned(), "dist/*.wasm".to_owned()];
        assert!(covered(&patterns, &PathBuf::from("hello.wasm")));
        assert!(covered(&patterns, &PathBuf::from("dist/hello.wasm")));
        assert!(!covered(&patterns, &PathBuf::from("assets/hello.wasm")));
        let question = ["hello?wasm".to_owned()];
        assert!(covered(&question, &PathBuf::from("hello1wasm")));
        assert!(!covered(&question, &PathBuf::from("hello12wasm")));
        // An empty list packs everything, as npm does; `check_files_covered`
        // holds that rule, so it never asks here.
        assert!(!covered(&[], &PathBuf::from("anything")));
    }

    #[test]
    fn a_files_exclusion_takes_coverage_away() {
        let files = ["assets".to_owned(), "!assets/secret.json".to_owned()];
        assert!(covered(&files, &PathBuf::from("assets/logo.png")));
        assert!(!covered(&files, &PathBuf::from("assets/secret.json")));
    }

    #[test]
    fn npm_names_a_tarball_by_its_package_and_version() {
        assert_eq!(
            tarball_name("@pane-samples/greeter", "0.1.0"),
            "pane-samples-greeter-0.1.0.tgz"
        );
        assert_eq!(tarball_name("hello", "2.0.0"), "hello-2.0.0.tgz");
    }
}
