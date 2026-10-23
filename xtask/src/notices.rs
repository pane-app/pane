//! The third-party notices every package of Pane carries (#454): the
//! `THIRD-PARTY-NOTICES.txt` beside the program, naming each third-party
//! part the `pane` program is built from with its licence, and giving the
//! texts of those licences.
//!
//! The Rust crates are read from Cargo itself, for the system the package
//! is built on: `cargo tree -e normal` names the crates a build of `pane`
//! compiles, with the features it resolves (no development dependency's
//! features, which `cargo metadata`'s graph would add), and `cargo
//! metadata` gives each one's licence, repository, authors and folder. The
//! crates shipped are those reached from `pane` through normal dependencies,
//! not through a procedural macro or a build script, whose code runs only
//! at build time. Each crate's licence texts are the files its package
//! holds (`LICENSE*`, `COPYING*`, `COPYRIGHT*`, `NOTICE*`, `UNLICENSE*`,
//! `PATENTS*`); a crate from a git repository without such files of its own
//! takes its repository's. A crate packaged with no licence file at all is
//! given the standard texts of the licences its manifest names, from
//! `xtask/licenses/`; one whose licences have no text there stops the
//! package, so a new licence is reviewed, never silently left out.
//!
//! What Cargo does not list is added by hand here: the Rust standard
//! library, the fonts and icons Pane embeds, WASI's WIT files, and Pane's
//! own parts under another licence. Adding an embedded font, picture or
//! bundled library means adding it to [`OTHER_PARTS`]
//! (`docs/research/licensing-audit.md`, 2.3).
//!
//! The notices are the same bytes for the same sources: the crates are in
//! name and version order, each text is given once (line endings made
//! `\n`), in the order the crates first name it.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The standard texts of the licences a crate packaged without a licence
/// file may name, by SPDX id (`LLVM-exception` being the exception Apache-2.0
/// is taken `WITH`). The MIT text names no copyright holder: its holders are
/// the authors listed with each crate it is given for.
const STANDARD_TEXTS: [(&str, &str); 4] = [
    ("Apache-2.0", include_str!("../licenses/Apache-2.0.txt")),
    ("CC0-1.0", include_str!("../licenses/CC0-1.0.txt")),
    (
        "LLVM-exception",
        include_str!("../licenses/LLVM-exception.txt"),
    ),
    ("MIT", include_str!("../licenses/MIT.txt")),
];

/// The file names (lowercased) that hold a crate's licence texts and
/// notices begin with one of these.
const LICENCE_FILES: [&str; 7] = [
    "licence",
    "license",
    "copying",
    "copyright",
    "notice",
    "unlicense",
    "patents",
];

/// A part of the program Cargo does not list: what it is, its licence, and
/// where its licence texts are — a file of this repository, or a standard
/// text by SPDX id.
struct OtherPart {
    what: &'static str,
    licence: &'static str,
    texts: &'static [Text],
}

enum Text {
    Repository(&'static str),
    Standard(&'static str),
}

const OTHER_PARTS: [OtherPart; 5] = [
    OtherPart {
        what: "The Rust standard library (std, core, alloc) and compiler_builtins, which the \
               Rust compiler links into every program (https://github.com/rust-lang/rust; \
               copyright the Rust project's contributors)",
        licence: "MIT OR Apache-2.0; compiler_builtins' parts from LLVM's compiler-rt \
                  Apache-2.0 WITH LLVM-exception",
        texts: &[
            Text::Standard("MIT"),
            Text::Standard("Apache-2.0"),
            Text::Standard("LLVM-exception"),
        ],
    },
    OtherPart {
        what: "The Geist and Geist Mono fonts, embedded in the program \
               (https://github.com/vercel/geist-font)",
        licence: "OFL-1.1",
        texts: &[Text::Repository("crates/pane/assets/fonts/OFL.txt")],
    },
    OtherPart {
        what: "The reicon icons, 1.2.5, embedded in the program (https://github.com/dqev/reicon)",
        licence: "MIT",
        texts: &[Text::Repository("crates/pane-core/assets/reicon/LICENSE")],
    },
    OtherPart {
        what: "WASI's WIT files (wasi:clocks, wasi:http and the packages it names), copied \
               unchanged from wasmtime-wasi-http 49.0.1 and embedded for building JavaScript \
               and TypeScript extensions (https://github.com/WebAssembly/WASI)",
        licence: "Apache-2.0 WITH LLVM-exception",
        texts: &[
            Text::Standard("Apache-2.0"),
            Text::Standard("LLVM-exception"),
        ],
    },
    OtherPart {
        what: "Pane's own JavaScript and TypeScript SDK and the WIT of its extension API, \
               embedded for building extensions (guests/js and wit/ in Pane's source; \
               copyright the Pane contributors)",
        licence: "Apache-2.0 OR MIT",
        texts: &[
            Text::Repository("guests/LICENSE-APACHE"),
            Text::Repository("guests/LICENSE-MIT"),
        ],
    },
];

/// One third-party crate the program is built from.
struct Crate {
    name: String,
    version: String,
    licence: Option<String>,
    repository: Option<String>,
    authors: Vec<String>,
    folder: PathBuf,
    from_git: bool,
    licence_file: Option<String>,
}

/// The licence texts, each once, with what gives it.
#[derive(Default)]
struct Texts {
    texts: Vec<(String, Vec<String>)>,
    index: HashMap<String, usize>,
}

impl Texts {
    fn add(&mut self, text: &str, given_by: String) {
        let text = format!("{}\n", text.replace("\r\n", "\n").trim_end());
        let at = *self.index.entry(text.clone()).or_insert_with(|| {
            self.texts.push((text, Vec::new()));
            self.texts.len() - 1
        });
        self.texts[at].1.push(given_by);
    }
}

/// The notices of the `pane` program a package of `version` holds, built
/// for the system this runs on.
pub fn third_party_notices(root: &Path, version: &str) -> Result<String, String> {
    let target = host_target(root)?;
    let crates = shipped_crates(root, &target)?;
    let mut texts = Texts::default();
    let mut out = format!(
        "Pane {version} for {target}: third-party notices

Pane is free software under the GNU General Public License, version 3 or
(at your option) any later version: LICENSE.txt, beside this file. Where
its source is, README.txt says. The pane program is built from the
third-party parts below, each under its own licence, whose texts follow
them. Where a part offers a choice of licences, Pane takes it under one
that GPL-3.0-or-later can be combined with.

"
    );
    out.push_str("PARTS CARGO DOES NOT LIST\n\n");
    for part in &OTHER_PARTS {
        out.push_str(&wrap(part.what, "  "));
        out.push_str(&wrap(part.licence, "    "));
        out.push('\n');
        for text in part.texts {
            let body = match text {
                Text::Repository(path) => fs::read_to_string(root.join(path))
                    .map_err(|error| format!("read {path} failed: {error}"))?,
                Text::Standard(id) => standard_text(id).to_owned(),
            };
            texts.add(&body, first_line(part.what));
        }
    }
    let names: HashSet<&str> = crates.iter().map(|krate| krate.name.as_str()).collect();
    if names.contains("self_cell") {
        out.push_str(
            "  self_cell is offered under Apache-2.0 OR GPL-2.0-only; Pane takes it under\n  \
             Apache-2.0.\n\n",
        );
    }
    if names.contains("freetype-sys") {
        out.push_str(
            "  freetype-sys links the system's FreeType when the build machine has it, as\n  \
             the package's build does. Built from its bundled copy instead, portions of\n  \
             this software are copyright The FreeType Project (www.freetype.org), all\n  \
             rights reserved, under the FreeType License (FTL).\n\n",
        );
    }
    out.push_str(&format!("RUST CRATES ({})\n\n", crates.len()));
    for krate in &crates {
        let id = format!("{} {}", krate.name, krate.version);
        out.push_str(&format!("  {id}\n"));
        let mut licence = krate
            .licence
            .as_deref()
            .unwrap_or("no licence in its manifest; its repository's follows")
            .to_owned();
        if let Some(repository) = &krate.repository {
            licence.push_str(&format!("; {repository}"));
        }
        out.push_str(&wrap(&licence, "    "));
        if !krate.authors.is_empty() {
            out.push_str(&wrap(&format!("by {}", krate.authors.join(", ")), "    "));
        }
        for (given_by, body) in crate_texts(krate)? {
            texts.add(&body, format!("{id} ({given_by})"));
        }
    }
    out.push_str("\nLICENCE TEXTS\n");
    for (number, (text, given_by)) in texts.texts.iter().enumerate() {
        out.push_str(&format!("\n{}\n", "-".repeat(78)));
        out.push_str(&format!("Text {}, for:\n", number + 1));
        for part in given_by {
            out.push_str(&format!("  {part}\n"));
        }
        out.push_str(&format!("{}\n\n{text}", "-".repeat(78)));
    }
    Ok(out)
}

/// `text` in lines of at most 78 columns (a word longer than that keeps a
/// line of its own), each starting with `indent`.
fn wrap(text: &str, indent: &str) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && indent.len() + line.len() + 1 + word.len() > 78 {
            out.push_str(&format!("{indent}{line}\n"));
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    out.push_str(&format!("{indent}{line}\n"));
    out
}

/// The first line of a part's description, to name it by beside a text.
fn first_line(what: &str) -> String {
    what.split(" (").next().unwrap_or(what).to_owned()
}

fn standard_text(id: &str) -> &'static str {
    STANDARD_TEXTS
        .iter()
        .find(|(standard, _)| *standard == id)
        .map(|(_, text)| *text)
        .expect("every standard text named is in xtask/licenses")
}

/// The licence texts of `krate`, each with what gives it: its own files, its
/// repository's (a crate from git), or the standard texts of the licences
/// it names.
fn crate_texts(krate: &Crate) -> Result<Vec<(String, String)>, String> {
    let mut folder = krate.folder.clone();
    let mut files = licence_files(&folder, krate.licence_file.as_deref())?;
    // A git repository's crates often leave the licence at its root: the
    // checkout's root is the folder Cargo marks with `.cargo-ok`.
    while files.is_empty() && krate.from_git && !folder.join(".cargo-ok").exists() {
        let Some(parent) = folder.parent() else { break };
        folder = parent.to_path_buf();
        files = licence_files(&folder, None)?;
    }
    if !files.is_empty() {
        let theirs = if folder == krate.folder {
            ""
        } else {
            "its repository's "
        };
        return files
            .into_iter()
            .map(|(name, path)| {
                let bytes = fs::read(&path)
                    .map_err(|error| format!("read {} failed: {error}", path.display()))?;
                Ok((
                    format!("{theirs}{name}"),
                    String::from_utf8_lossy(&bytes).into_owned(),
                ))
            })
            .collect();
    }
    let licence = krate.licence.as_deref().unwrap_or_default();
    let known: Vec<(String, String)> = licence_ids(licence)
        .into_iter()
        .filter_map(|id| {
            STANDARD_TEXTS
                .iter()
                .find(|(standard, _)| *standard == id)
                .map(|(id, text)| {
                    (
                        format!("no licence file; the standard {id}"),
                        (*text).to_owned(),
                    )
                })
        })
        .collect();
    if known.is_empty() {
        return Err(format!(
            "{} {} is packaged without a licence file, and xtask/licenses has the standard \
             text of none of its licences (`{licence}`): add the text, after checking the \
             licence may ship in Pane (deny.toml)",
            krate.name, krate.version
        ));
    }
    Ok(known)
}

/// The SPDX ids an expression names, in its order, each once.
fn licence_ids(expression: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for word in expression
        .split(|c: char| c.is_whitespace() || matches!(c, '/' | '(' | ')'))
        .filter(|word| !word.is_empty() && !matches!(*word, "AND" | "OR" | "WITH"))
    {
        if !ids.iter().any(|id| id == word) {
            ids.push(word.to_owned());
        }
    }
    ids
}

/// The licence files directly in `folder`, by name, in name order, with
/// the one the manifest names (`license-file`) when it is elsewhere.
fn licence_files(folder: &Path, named: Option<&str>) -> Result<Vec<(String, PathBuf)>, String> {
    let mut files = BTreeSet::new();
    for entry in fs::read_dir(folder)
        .map_err(|error| format!("read {} failed: {error}", folder.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let lower = name.to_lowercase();
        if LICENCE_FILES.iter().any(|word| lower.starts_with(word)) && entry.path().is_file() {
            files.insert((name, entry.path()));
        }
    }
    if let Some(named) = named {
        let path = folder.join(named);
        let name = named.replace('\\', "/");
        if path.is_file() && !files.iter().any(|(_, known)| *known == path) {
            files.insert((name, path));
        }
    }
    Ok(files.into_iter().collect())
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// What `command` printed, or why it failed.
fn output(command: &mut Command) -> Result<Vec<u8>, String> {
    let output = command
        .output()
        .map_err(|error| format!("failed to start {command:?}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{command:?} failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output.stdout)
}

/// The target triple of the system this runs on, which the package's
/// program is built for, as the pinned toolchain's `rustc` names it.
fn host_target(root: &Path) -> Result<String, String> {
    let version = output(Command::new("rustc").current_dir(root).arg("-vV"))?;
    String::from_utf8_lossy(&version)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or_else(|| "rustc -vV names no host".to_owned())
}

/// The third-party crates a build of `pane` for `target` compiles into the
/// program, in name and version order.
fn shipped_crates(root: &Path, target: &str) -> Result<Vec<Crate>, String> {
    let metadata: Value = serde_json::from_slice(&output(cargo().current_dir(root).args([
        "metadata",
        "--locked",
        "--format-version",
        "1",
        "--filter-platform",
        target,
    ]))?)
    .map_err(|error| format!("cargo metadata printed no JSON: {error}"))?;
    let tree = output(cargo().current_dir(root).args([
        "tree", "--locked", "-p", "pane", "-e", "normal", "--target", target, "--prefix", "none",
        "-f", "{p}",
    ]))?;
    // `name vX.Y.Z (source)`, a line per crate the build compiles.
    let built: HashSet<(String, String)> = String::from_utf8_lossy(&tree)
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let name = words.next()?;
            let version = words.next()?.strip_prefix('v')?;
            Some((name.to_owned(), version.to_owned()))
        })
        .collect();
    let packages: HashMap<&str, &Value> = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata listed no packages")?
        .iter()
        .filter_map(|package| Some((package["id"].as_str()?, package)))
        .collect();
    let nodes: HashMap<&str, &Value> = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("cargo metadata resolved no graph")?
        .iter()
        .filter_map(|node| Some((node["id"].as_str()?, node)))
        .collect();
    let pane = packages
        .iter()
        .find(|(_, package)| package["name"] == "pane" && package["source"].is_null())
        .map(|(id, _)| *id)
        .ok_or("cargo metadata lists no pane crate")?;
    let mut reached = BTreeSet::new();
    let mut next = vec![pane];
    while let Some(id) = next.pop() {
        if !reached.insert(id) {
            continue;
        }
        let deps = nodes.get(id).and_then(|node| node["deps"].as_array());
        for dep in deps.into_iter().flatten() {
            let normal = dep["dep_kinds"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind["kind"].is_null()));
            let Some(dep_id) = dep["pkg"].as_str() else {
                continue;
            };
            let Some(package) = packages.get(dep_id) else {
                continue;
            };
            let name = package["name"].as_str().unwrap_or_default().to_owned();
            let version = package["version"].as_str().unwrap_or_default().to_owned();
            if normal && !proc_macro(package) && built.contains(&(name, version)) {
                next.push(dep_id);
            }
        }
    }
    let mut crates: Vec<Crate> = reached
        .into_iter()
        .map(|id| packages[id])
        .filter(|package| !package["source"].is_null())
        .map(|package| {
            let text = |key: &str| package[key].as_str().map(str::to_owned);
            let manifest = PathBuf::from(text("manifest_path").unwrap_or_default());
            Crate {
                name: text("name").unwrap_or_default(),
                version: text("version").unwrap_or_default(),
                licence: text("license"),
                repository: text("repository"),
                authors: package["authors"]
                    .as_array()
                    .map(|authors| {
                        authors
                            .iter()
                            .filter_map(|author| author.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                folder: manifest.parent().map(Path::to_path_buf).unwrap_or_default(),
                from_git: text("source").is_some_and(|source| source.starts_with("git+")),
                licence_file: text("license_file"),
            }
        })
        .collect();
    crates.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    Ok(crates)
}

/// Whether `package` is a procedural macro, whose code runs only in the
/// compiler.
fn proc_macro(package: &Value) -> bool {
    package["targets"].as_array().is_some_and(|targets| {
        targets.iter().any(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "proc-macro"))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_expression_names_its_ids_once_in_order() {
        assert_eq!(
            licence_ids("(MIT OR Apache-2.0) AND Unicode-3.0"),
            ["MIT", "Apache-2.0", "Unicode-3.0"]
        );
        assert_eq!(
            licence_ids("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT"),
            ["Apache-2.0", "LLVM-exception", "MIT"]
        );
        assert_eq!(licence_ids("MIT/Apache-2.0"), ["MIT", "Apache-2.0"]);
    }

    #[test]
    fn a_text_is_given_once_whatever_its_line_endings() {
        let mut texts = Texts::default();
        texts.add("MIT License\r\n\r\nPermission\r\n", "a 1.0.0".to_owned());
        texts.add("MIT License\n\nPermission\n\n", "b 2.0.0".to_owned());
        texts.add("Apache License\n", "c 3.0.0".to_owned());
        assert_eq!(texts.texts.len(), 2);
        assert_eq!(texts.texts[0].0, "MIT License\n\nPermission\n");
        assert_eq!(texts.texts[0].1, ["a 1.0.0", "b 2.0.0"]);
    }

    #[test]
    fn a_description_wraps_at_78_columns() {
        let wrapped = wrap(&"word ".repeat(40), "    ");
        assert!(
            wrapped
                .lines()
                .all(|line| line.len() <= 78 && line.starts_with("    word"))
        );
        assert_eq!(wrapped.split_whitespace().count(), 40);
    }

    #[test]
    fn every_standard_text_is_there() {
        for (id, text) in STANDARD_TEXTS {
            assert!(text.len() > 500, "{id}'s standard text is missing");
        }
        for part in &OTHER_PARTS {
            for text in part.texts {
                if let Text::Standard(id) = text {
                    standard_text(id);
                }
            }
        }
    }
}
