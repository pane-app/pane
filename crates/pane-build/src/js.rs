//! Pane's JavaScript and TypeScript build (ADR 0047, `#218`, spec `#128`):
//! the steps `tools/componentize-js/pane_js.py` used to run, moved into the
//! crate Pane's development mode and `pane-ext` both build with — so the two
//! can never build a package differently — and `cargo xtask js-guests`
//! rebuilds the committed samples with. A build needs Node.js and npm alone:
//! no Python, no nightly Rust, no wasi-sdk (the wasm parts are the committed
//! files [`crate::js_assets`] embeds; esbuild and TypeScript come from the
//! package's own `node_modules`).
//!
//! The steps, one command of the package's `pane.json` at a time:
//!
//! 1. **Staging** the package into a work folder of the user's cache, beside
//!    a copy of the SDK (`@pane-app/extension`), so its `file:../js`
//!    devDependency installs and dependencies land in the cache rather than
//!    the source tree. `npm ci --ignore-scripts` runs only when the
//!    package-lock.json changed (a marker in `node_modules` records its
//!    digest), so a save that changes no dependency builds at once.
//! 2. **Type-checking** with the package's own `tsc` (from the staged
//!    `node_modules`) when it has a `tsconfig.json`, run in the staged copy
//!    so errors name the author's files (`src/index.ts(3,7): error …`).
//! 3. **Bundling** with esbuild (also from the staged `node_modules`) around
//!    Pane's adapter entry, which installs `console` first and wraps what
//!    the command exports so a handler that throws answers an error instead
//!    of crashing.
//! 4. **Choosing the world** by what the bundle imports: `wasi:http`'s
//!    client only for a command that makes web requests, Pane's system
//!    programs only for one that runs them, and the interfaces the
//!    package.json's `"pane"` options name; the WIT is assembled from the
//!    embedded files.
//! 5. **Componentizing** with [`Componentizer`]: in this process where
//!    pane-build links the componentizer (its `componentizer` feature), or
//!    a componentizer binary otherwise.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde_json::{Map, Value};

use crate::build::{BuildJob, BuildOutcome, Componentizer};
use crate::js_assets;

/// The world a command is built against: `js-extension` plus the exports its
/// package.json's `"pane"` options name, written by [`command_world`].
const COMMAND_WORLD: &str = "js-command";
/// The world the SDK's `wit/world.wit` declares, which `js-command` includes.
const WORLD: &str = "js-extension";
/// The name of the componentizer binary a folder of wasm parts holds.
pub(crate) const COMPONENTIZER: &str = "componentize-qjs-p3";

/// `"pane"` option -> the interface a command setting it also exports.
const EXPORT_OPTIONS: [(&str, &str); 5] = [
    ("rootResults", "pane:extension/root-results@0.1.0"),
    ("indexedResults", "pane:extension/indexed-results@0.1.0"),
    ("operations", "pane:extension/published-operations@0.1.0"),
    ("search", "pane:extension/command-search@0.1.0"),
    ("service", "pane:extension/service@0.1.0"),
];
/// `"pane"` option -> the interface a command setting it also imports,
/// beyond what every command may import (`js-extension`).
const IMPORT_OPTIONS: [(&str, &str); 3] = [
    ("files", "pane:extension/files@0.1.0"),
    ("fileIndex", "pane:extension/file-index@0.1.0"),
    ("clipboardHistory", "pane:extension/clipboard-history@0.1.0"),
];
/// The exports the adapter wraps, by `pane` option, each with the handler
/// that answers errors as text (see `guests/js/adapt.js`).
const ADAPTED_PROVIDERS: [(&str, &str, &str); 5] = [
    ("rootResults", "rootResults", "resultsFor"),
    ("indexedResults", "indexedResults", "results"),
    ("operations", "publishedOperations", "runOperation"),
    ("search", "commandSearch", "search"),
    ("service", "service", "runCycle"),
];
/// `wasi:http`'s client, which a command imports only if its bundle uses it
/// (itself, or through `@pane-app/extension/http`), as a Rust command's
/// component imports only what its code calls: Pane lists a package whose
/// component imports it as one that uses the network.
const HTTP_IMPORT: &str = "wasi:http/client@0.3.0";
/// Pane's system programs (`wit/programs.wit`), which a command imports only
/// if its bundle uses them (itself, or through
/// `@pane-app/extension/programs`), as a Rust command's component imports
/// only what its code calls: Pane lists a package whose component imports
/// them as one that runs system programs.
const PROGRAMS_IMPORT: &str = "pane:extension/programs@0.1.0";

/// Builds the command package in `package`, writing its component to `out`:
/// the steps the module's documentation lists, run by pane-build's `Build` of
/// a `package.json` folder for each component its `pane.json` names and by
/// `cargo xtask js-guests` for each sample (one build path for all of them).
pub fn build_js_command(
    job: &BuildJob,
    package: &Path,
    out: &Path,
    componentizer: &Componentizer,
) -> BuildOutcome {
    let package = match crate::canonical(package) {
        Ok(package) => package,
        Err(error) => {
            return failed(&format!(
                "{} cannot be resolved: {error}",
                package.display()
            ));
        }
    };
    let manifest = match fs::read_to_string(package.join("package.json")) {
        Ok(manifest) => manifest,
        Err(error) => return failed(&format!("{} cannot be read: {error}", package.display())),
    };
    let manifest: Value = match serde_json::from_str(&manifest) {
        Ok(manifest) => manifest,
        Err(error) => return failed(&format!("{} is not valid JSON: {error}", package.display())),
    };
    let Some(entry) = manifest.get("main").and_then(Value::as_str) else {
        return failed(&format!(
            "{}/package.json needs a `main` entry naming the command module",
            package.display()
        ));
    };
    let options = match manifest.get("pane") {
        None => Map::new(),
        Some(Value::Object(options)) => options.clone(),
        Some(_) => return failed("\"pane\" in package.json must be an object".into()),
    };
    let Some(node) = find_node() else {
        return failed(
            "Pane found no Node.js and npm to build it (it looked for node and npm on PATH; \
             a JavaScript or TypeScript package needs Node.js 22 or newer)"
                .into(),
        );
    };

    // The staging copy, kept between builds so `npm ci` runs only when the
    // lockfile changed.
    let work = work_folder(&package);
    let staged = work.join(package.file_name().unwrap_or_default());
    let types = work.join("js");
    if let Err(error) = refresh_staging(&package, &staged, &types) {
        return failed(&format!(
            "Pane could not stage {} into {}: {error}",
            package.display(),
            staged.display()
        ));
    }
    if let Some(outcome) = install(job, &staged) {
        return outcome;
    }
    if staged.join("tsconfig.json").is_file() {
        job.line(&format!("pane-js: type-checking {}", package.display()));
        if let Some(outcome) = type_check(job, &node, &staged) {
            return outcome;
        }
    }

    // The adapter entry and its bundle.
    let adapted = work.join("pane-entry.mjs");
    let bundle = work.join("bundle.mjs");
    let entry = staged.join(entry);
    if let Err(error) = fs::write(&adapted, adapted_entry(&entry, &types, &options)) {
        return failed(&format!("{} cannot be written: {error}", adapted.display()));
    }
    if let Some(outcome) = bundle_command(job, &node, &staged, &adapted, &bundle) {
        return outcome;
    }
    let bundled = match fs::read_to_string(&bundle) {
        Ok(bundled) => bundled,
        Err(error) => return failed(&format!("{} cannot be read: {error}", bundle.display())),
    };
    let world = match command_world(&options, uses_http(&bundled), uses_programs(&bundled)) {
        Ok(world) => world,
        Err(reason) => return failed(&reason),
    };
    let wit = types.join("wit");
    if let Err(error) = js_assets::write_deps(&wit) {
        return failed(&format!("{} cannot be written: {error}", wit.display()));
    }
    let out = out.to_path_buf();
    if let Some(parent) = out.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            return failed(&format!("{} cannot be created: {error}", parent.display()));
        }
    }
    let world_file = wit.join("command.wit");
    if let Err(error) = fs::write(&world_file, world) {
        return failed(&format!(
            "{} cannot be written: {error}",
            world_file.display()
        ));
    }

    // The componentizer: this process's, or the binary a folder holds.
    let start = Instant::now();
    let component: Result<Vec<u8>, BuildOutcome> = match componentizer {
        #[cfg(feature = "componentizer")]
        Componentizer::Linked => {
            linked_componentize(&wit, &bundle).map_err(|reason| failed(&reason))
        }
        #[cfg(not(feature = "componentizer"))]
        Componentizer::Linked => Err(failed(
            "this build of pane-build has no componentizer linked in (it was built without \
             its `componentizer` feature); build it with that feature, or point \
             PANE_COMPONENTIZER at a componentizer folder"
                .into(),
        )),
        Componentizer::Binary(folder) => {
            let folder = match folder {
                Some(folder) => folder.clone(),
                None => match package_componentizer(&package) {
                    Ok(folder) => folder,
                    Err(reason) => return failed(&reason),
                },
            };
            let parts = match componentizer_parts(&folder) {
                Ok(parts) => parts,
                Err(reason) => return failed(&reason),
            };
            match spawn_componentizer(job, &parts, &wit, &bundle, &out) {
                BuildOutcome::Built => fs::read(&out)
                    .map_err(|error| failed(&format!("{} cannot be read: {error}", out.display()))),
                other => Err(other),
            }
        }
    };
    let component = match component {
        Ok(component) => component,
        Err(outcome) => return outcome,
    };
    if let Err(error) = fs::write(&out, &component) {
        return failed(&format!("{} cannot be written: {error}", out.display()));
    }
    job.line(&format!(
        "pane-js: built {} ({} bytes in {} ms)",
        out.display(),
        component.len(),
        start.elapsed().as_millis()
    ));
    BuildOutcome::Built
}

/// The componentizer folder resolved to: its binary, runtime and libc.
#[derive(Debug)]
pub(crate) struct Parts {
    binary: PathBuf,
    runtime: PathBuf,
    libc: PathBuf,
}

/// A build's own failure, which Pane's development mode shows as the
/// build's first error: one line starting `pane-js: error:`.
fn failed(reason: &str) -> BuildOutcome {
    BuildOutcome::Failed(format!("pane-js: error: {reason}"))
}

/// Where the JS build stages a package: the user's cache folder,
/// `pane/js-build/<name>-<hash of the package's path>`, kept between builds
/// so a save whose lockfile did not change installs nothing.
fn work_folder(package: &Path) -> PathBuf {
    let home = crate::build::env_path(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    let cache = if cfg!(windows) {
        crate::build::env_path("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        home.map(|home| home.join("Library/Caches"))
    } else {
        crate::build::env_path("XDG_CACHE_HOME").or_else(|| home.map(|home| home.join(".cache")))
    };
    // FNV-1a, which is stable across Rust versions, unlike `DefaultHasher`.
    let hash = package
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    cache
        .unwrap_or_else(std::env::temp_dir)
        .join("pane")
        .join("js-build")
        .join(format!(
            "{}-{hash:08x}",
            package.file_name().unwrap_or_default().to_string_lossy()
        ))
}

/// Node.js, from the search path; npm comes with it and is spawned by its
/// Windows name (`npm.cmd`, a batch file `Command` starts through `cmd`).
fn find_node() -> Option<PathBuf> {
    crate::build::find_on_path("node", |_| true)
}

/// Writes the SDK beside the staged package and refreshes the package's own
/// files in it, keeping `node_modules` (with the marker of the lockfile its
/// dependencies were installed for) so nothing is installed again.
fn refresh_staging(package: &Path, staged: &Path, types: &Path) -> std::io::Result<()> {
    if let Err(error) = fs::remove_dir_all(types) {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(error);
        }
    }
    js_assets::write_sdk(types)?;
    if staged.is_dir() {
        // Everything but node_modules goes, so files the package dropped do.
        for child in fs::read_dir(staged)? {
            let child = child?.path();
            if child.file_name().is_some_and(|name| name == "node_modules") {
                continue;
            }
            if fs::symlink_metadata(&child)?.is_dir() {
                fs::remove_dir_all(&child)?;
            } else {
                fs::remove_file(&child)?;
            }
        }
    } else {
        fs::create_dir_all(staged)?;
    }
    copy_files(package, staged)?;
    Ok(())
}

/// Copies the package's files into `staged`, skipping `node_modules` and
/// `.git` folders at any depth, so dependencies and the repository itself
/// are never staged.
fn copy_files(package: &Path, staged: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(package)? {
        let entry = entry?.path();
        let name = entry.file_name().unwrap_or_default();
        if name == "node_modules" || name == ".git" {
            continue;
        }
        let to = staged.join(&name);
        if fs::symlink_metadata(&entry)?.is_dir() {
            fs::create_dir_all(&to)?;
            copy_files(&entry, &to)?;
        } else {
            fs::copy(&entry, &to)?;
        }
    }
    Ok(())
}

/// `npm ci --ignore-scripts --no-audit --no-fund` in the staged copy when
/// its lockfile changed since the last install (a marker in `node_modules`
/// records the digest); install scripts are not needed by these packages and
/// are not run. `Some` when the build ended (a failure, or a stop).
fn install(job: &BuildJob, staged: &Path) -> Option<BuildOutcome> {
    let lock = staged.join("package-lock.json");
    if !lock.is_file() {
        return None;
    }
    let lock = match fs::read(&lock) {
        Ok(lock) => lock,
        Err(error) => {
            return Some(failed(&format!(
                "{} cannot be read: {error}",
                staged.join("package-lock.json").display()
            )));
        }
    };
    let digest = sha256(&lock);
    let marker = staged.join("node_modules/.pane-lock-sha256");
    if fs::read_to_string(&marker).is_ok_and(|written| written == digest) {
        return None;
    }
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let mut command = Command::new(npm);
    command
        .current_dir(staged)
        .args(["ci", "--ignore-scripts", "--no-audit", "--no-fund"]);
    crate::build::without_pane_build_environment(&mut command);
    match job.run_command(command, "npm ci --ignore-scripts", None) {
        BuildOutcome::Built => {
            if let Some(parent) = marker.parent() {
                if let Err(error) = fs::create_dir_all(parent) {
                    return Some(failed(&format!(
                        "{} cannot be created: {error}",
                        parent.display()
                    )));
                }
            }
            if let Err(error) = fs::write(&marker, digest) {
                return Some(failed(&format!(
                    "{} cannot be written: {error}",
                    marker.display()
                )));
            }
            None
        }
        other => Some(other),
    }
}

/// The package's `tsc` (from the staged `node_modules`), run in the staged
/// copy so errors name the author's files; `Some` when the build ended.
fn type_check(job: &BuildJob, node: &Path, staged: &Path) -> Option<BuildOutcome> {
    let tsc = staged.join("node_modules/typescript/bin/tsc");
    let tsconfig = staged.join("tsconfig.json");
    let mut command = Command::new(node);
    command
        .current_dir(staged)
        .arg(&tsc)
        .arg("-p")
        .arg(&tsconfig);
    crate::build::without_pane_build_environment(&mut command);
    match job.run_command(command, "tsc -p tsconfig.json", None) {
        BuildOutcome::Built => None,
        other => Some(other),
    }
}

/// The package's esbuild (from the staged `node_modules`), bundling the
/// adapter entry around the command; `wasi:` and `pane:` imports stay
/// external, provided by the world. `Some` when the build ended.
fn bundle_command(
    job: &BuildJob,
    node: &Path,
    staged: &Path,
    adapted: &Path,
    bundle: &Path,
) -> Option<BuildOutcome> {
    let esbuild = staged.join("node_modules/esbuild/bin/esbuild");
    let mut command = Command::new(node);
    command
        .current_dir(staged)
        .arg(&esbuild)
        .arg(adapted)
        .args([
            "--bundle",
            "--format=esm",
            "--platform=neutral",
            "--target=es2020",
            // esbuild spells repeatable flags with `:` rather than `=`.
            "--external:wasi:*",
            "--external:pane:*",
            "--main-fields=module,main",
            "--log-level=warning",
            "--outfile",
        ])
        .arg(bundle);
    crate::build::without_pane_build_environment(&mut command);
    match job.run_command(command, "esbuild --bundle", None) {
        BuildOutcome::Built => None,
        other => Some(other),
    }
}

/// The entry bundled for a command: its exports, with the ones Pane calls
/// wrapped by the SDK's adapter so that what a handler throws is an error,
/// not a crash, and `console` installed before the command's module loads.
fn adapted_entry(entry: &Path, types: &Path, options: &Map<String, Value>) -> String {
    // `console` first, so that the command's own module has it as it loads.
    let console = js_string(&posix(&types.join("console.js")));
    let entry = js_string(&posix(entry));
    let adapter = js_string(&posix(&types.join("adapt.js")));
    let mut lines = vec![
        format!("import {console};"),
        format!("import * as extension from {entry};"),
        format!("import {{ adaptCommand, adaptProvider }} from {adapter};"),
        format!("export * from {entry};"),
        "export const command = adaptCommand(extension.command);".into(),
    ];
    for (option, name, handler) in ADAPTED_PROVIDERS {
        if options
            .get(option)
            .is_some_and(|value| value.as_bool() == Some(true))
        {
            lines.push(format!(
                "export const {name} = adaptProvider(extension.{name}, {handler:?});"
            ));
        }
    }
    lines.join("\n") + "\n"
}

/// `text` as a JavaScript string literal: quoted, with `"`, `\` and control
/// characters escaped (as JSON does); everything else, paths included, stays
/// literal.
fn js_string(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// `path` as JavaScript (and esbuild) spell it: forward slashes, which Node
/// accepts on Windows too.
fn posix(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// The world `js-command`: `js-extension` exporting and importing what
/// `options` name, importing `wasi:http`'s client if `http` and Pane's
/// system programs if `programs`, or why the options name nothing Pane
/// knows.
fn command_world(
    options: &Map<String, Value>,
    http: bool,
    programs: bool,
) -> Result<String, String> {
    let known: Vec<&str> = EXPORT_OPTIONS
        .iter()
        .chain(IMPORT_OPTIONS.iter())
        .map(|(option, _)| *option)
        .collect();
    let mut unknown: Vec<&str> = options
        .keys()
        .map(String::as_str)
        .filter(|option| !known.contains(option))
        .collect();
    unknown.sort_unstable();
    if !unknown.is_empty() {
        return Err(format!(
            "unknown \"pane\" options in package.json: {}",
            unknown.join(", ")
        ));
    }
    let mut imports = String::new();
    let mut exports = String::new();
    for (option, interface) in IMPORT_OPTIONS {
        if options
            .get(option)
            .is_some_and(|value| value.as_bool() == Some(true))
        {
            imports.push_str(&format!("  import {interface};\n"));
        }
    }
    if http {
        imports.push_str(&format!("  import {HTTP_IMPORT};\n"));
    }
    if programs {
        imports.push_str(&format!("  import {PROGRAMS_IMPORT};\n"));
    }
    for (option, interface) in EXPORT_OPTIONS {
        if options
            .get(option)
            .is_some_and(|value| value.as_bool() == Some(true))
        {
            exports.push_str(&format!("  export {interface};\n"));
        }
    }
    Ok(format!(
        "package pane:js-guest@0.1.0;\n\nworld {COMMAND_WORLD} {{\n  include {WORLD};\n{imports}{exports}}}\n"
    ))
}

/// Whether the bundled module imports any `wasi:http` interface: a
/// `from "wasi:http/…"` or `import("wasi:http/…")` anywhere in it.
fn uses_http(bundle: &str) -> bool {
    uses_import(bundle, "wasi:http/")
}

/// Whether the bundled module imports Pane's system programs.
fn uses_programs(bundle: &str) -> bool {
    uses_import(bundle, "pane:extension/programs@")
}

/// Whether `bundle` imports `interface`: an import of it, static or dynamic,
/// anywhere in the bundle. esbuild keeps `wasi:` and `pane:` imports as
/// written, so the specifier appears as a string the `from` or `import`
/// keyword introduces.
fn uses_import(bundle: &str, interface: &str) -> bool {
    bundle.match_indices(interface).any(|(at, _)| {
        let preceding = &bundle[..at];
        // A quote directly before the interface, then whitespace, an
        // optional `(` (a dynamic import), more whitespace, and the keyword.
        let quoted = preceding.strip_suffix(['"', '\'']).unwrap_or(preceding);
        let trimmed = quoted.trim_end();
        let trimmed = trimmed.strip_suffix('(').unwrap_or(trimmed).trim_end();
        trimmed.ends_with("from") || trimmed.ends_with("import")
    })
}

/// The `@pane-app/cli` platform package the package at `folder` installed,
/// which holds the componentizer its build uses — `componentize-qjs-p3`,
/// `runtime.wasm` and `libc.so` in `node_modules/@pane-app/cli-<target>`, as
/// `npm install` puts them there (`@pane-app/cli`'s platform packages,
/// `#219`): the package's pinned CLI version builds it, so its build does
/// not change when Pane updates. A package without one is explained.
pub(crate) fn package_componentizer(folder: &Path) -> Result<PathBuf, String> {
    let Some(target) = pane_target::Target::current() else {
        return Err(
            "Pane names no target for this system, so it cannot find the package's componentizer"
                .into(),
        );
    };
    let package = folder.join(format!("node_modules/@pane-app/cli-{}", target.id()));
    match componentizer_parts(&package) {
        Ok(_) => Ok(package),
        Err(reason) => Err(format!(
            "{} has no componentizer of Pane's installed (Pane looked in {}, which npm \
             install provides); install the package's dependencies and develop it again: {reason}",
            folder.display(),
            package.display()
        )),
    }
}

/// The parts a componentizer folder must hold: the binary, the runtime and
/// the libc, or why it holds none.
pub(crate) fn componentizer_parts(folder: &Path) -> Result<Parts, String> {
    let binary = folder.join(if cfg!(windows) {
        format!("{COMPONENTIZER}.exe")
    } else {
        COMPONENTIZER.to_owned()
    });
    let runtime = folder.join("runtime.wasm");
    let libc = folder.join("libc.so");
    let missing: Vec<String> = [&binary, &runtime, &libc]
        .into_iter()
        .filter(|path| !path.is_file())
        .map(|path| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "{} has no {}",
            folder.display(),
            missing.join(", ")
        ));
    }
    Ok(Parts {
        binary,
        runtime,
        libc,
    })
}

/// Runs the componentizer binary of `parts`: the `p3_build` contract
/// (`<wit> <world> <js> <runtime.wasm> <out.wasm>`), with `QJS_P3_LIBC`
/// naming the libc — the contract #215's workflow builds for every platform
/// and `@pane-app/cli`'s platform packages carry.
fn spawn_componentizer(
    job: &BuildJob,
    parts: &Parts,
    wit: &Path,
    bundle: &Path,
    out: &Path,
) -> BuildOutcome {
    let mut command = Command::new(&parts.binary);
    command
        .current_dir(bundle.parent().unwrap_or(Path::new(".")))
        .arg(wit)
        .arg(COMMAND_WORLD)
        .arg(bundle)
        .arg(&parts.runtime)
        .arg(out)
        .env("QJS_P3_LIBC", &parts.libc);
    crate::build::without_pane_build_environment(&mut command);
    job.run_command(command, "componentize", None)
}

/// The componentizer linked into this process (`pane-build`'s
/// `componentizer` feature): Pane's vendored componentize-qjs
/// (`tools/componentize-js/componentize-qjs`) with the committed wasm parts,
/// driven on a current-thread runtime of its own.
#[cfg(feature = "componentizer")]
fn linked_componentize(wit: &Path, bundle: &Path) -> Result<Vec<u8>, String> {
    let js = fs::read_to_string(bundle)
        .map_err(|error| format!("{} cannot be read: {error}", bundle.display()))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("a runtime to componentize with could not start: {error}"))?;
    let binding = componentize_qjs::ComponentizeOpts {
        wit_path: wit,
        js_source: &js,
        js_path: Some(bundle),
        module_root: None,
        world_name: Some(COMMAND_WORLD),
        stub_wasi: false,
        disable_gc: false,
        runtime: componentize_qjs::Runtime::Custom(js_assets::RUNTIME_WASM),
        libc: Some(js_assets::LIBC_SO),
    };
    let componentize = componentize_qjs::componentize(&binding);
    runtime
        .block_on(componentize)
        .map_err(|error| format!("the componentizer failed: {error:#}"))
}

/// The digest of `bytes` as lowercase hex, for the marker of a lockfile.
fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(bytes);
    let mut out = String::new();
    for byte in digest.finalize() {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_world_declares_what_the_options_name() {
        let mut options = Map::new();
        options.insert("rootResults".into(), Value::Bool(true));
        options.insert("files".into(), Value::Bool(true));
        assert_eq!(
            command_world(&options, false, false).unwrap(),
            "package pane:js-guest@0.1.0;\n\nworld js-command {\n  include js-extension;\n  \
             import pane:extension/files@0.1.0;\n  export pane:extension/root-results@0.1.0;\n}\n"
        );
        let world = command_world(&Map::new(), true, true).unwrap();
        assert!(
            world.contains("  import wasi:http/client@0.3.0;\n"),
            "{world}"
        );
        assert!(
            world.contains("  import pane:extension/programs@0.1.0;\n"),
            "{world}"
        );
        let mut unknown = Map::new();
        unknown.insert("noSuchOption".into(), Value::Bool(true));
        assert!(
            command_world(&unknown, false, false)
                .unwrap_err()
                .contains("unknown \"pane\" options in package.json: noSuchOption")
        );
    }

    #[test]
    fn the_bundle_decides_the_wasi_http_and_programs_imports() {
        assert!(uses_http(
            r#"import { send } from "wasi:http/client@0.3.0";"#
        ));
        assert!(uses_http(
            r#"const m = await import ("wasi:http/types@0.3.0");"#
        ));
        assert!(uses_programs(
            r#"import { run } from "pane:extension/programs@0.1.0";"#
        ));
        assert!(!uses_http("import { send } from \"pane:extension/http\";"));
        // A mention without an import is not a use.
        assert!(!uses_http("const url = 'https://wasi:http/';"));
        assert!(!uses_programs(
            "import { run } from \"pane:extension/other\";"
        ));
        assert!(!uses_http(""));
    }

    #[test]
    fn the_adapter_entry_wraps_what_the_package_declares() {
        let entry = adapted_entry(
            Path::new("/work/hello-ts/src/index.ts"),
            Path::new("/work/js"),
            &Map::new(),
        );
        assert!(
            entry.contains(
                "import \"/work/js/console.js\";\nimport * as extension from \
                            \"/work/hello-ts/src/index.ts\";\nimport { adaptCommand, \
                            adaptProvider } from \"/work/js/adapt.js\";"
            ),
            "{entry}"
        );
        assert!(
            entry.contains("export const command = adaptCommand(extension.command);"),
            "{entry}"
        );
        assert!(!entry.contains("rootResults"), "{entry}");

        let mut options = Map::new();
        options.insert("rootResults".into(), Value::Bool(true));
        options.insert("service".into(), Value::Bool(true));
        let entry = adapted_entry(Path::new("/work/js"), Path::new("/work/js"), &options);
        assert!(
            entry.contains(
                "export const rootResults = adaptProvider(extension.rootResults, \"resultsFor\");"
            ),
            "{entry}"
        );
        assert!(
            entry
                .contains("export const service = adaptProvider(extension.service, \"runCycle\");"),
            "{entry}"
        );
        assert!(!entry.contains("indexedResults"), "{entry}");
    }

    #[test]
    fn javascript_strings_are_escaped_as_json_escapes_them() {
        assert_eq!(js_string("src/index.ts"), "\"src/index.ts\"");
        assert_eq!(js_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(js_string("line\nend"), "\"line\\nend\"");
        assert_eq!(js_string("contröl"), "\"contröl\"");
        assert_eq!(js_string("\u{1}"), "\"\\u0001\"");
        assert_eq!(posix(Path::new("a\\b.mjs")), "a/b.mjs");
    }

    #[test]
    fn the_embedded_sdk_and_wit_are_written() {
        let dir = tempfile::tempdir().unwrap();
        js_assets::write_sdk(&dir.path().join("js")).unwrap();
        // What the bundled entry imports, and `npm ci` of a `file:../js`
        // devDependency needs.
        for file in [
            "js/package.json",
            "js/adapt.js",
            "js/console.js",
            "js/http.js",
            "js/pane.d.ts",
            "js/wasi.d.ts",
            "js/wit/world.wit",
            "js/LICENSE-APACHE",
        ] {
            assert!(dir.path().join(file).is_file(), "{file}");
        }
        js_assets::write_deps(&dir.path().join("js/wit")).unwrap();
        // Pane's contract and WASI's own WIT, beside the SDK's world.
        for file in [
            "js/wit/deps/pane-extension/extension.wit",
            "js/wit/deps/pane-extension/file-index.wit",
            "js/wit/deps/clocks.wit",
            "js/wit/deps/http.wit",
        ] {
            assert!(dir.path().join(file).is_file(), "{file}");
        }
        // The embedded WIT is the repository's own.
        let wit = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../wit");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("js/wit/deps/pane-extension/extension.wit"))
                .unwrap(),
            std::fs::read_to_string(wit.join("extension.wit")).unwrap()
        );
    }

    #[test]
    fn a_componentizer_folder_names_its_parts() {
        let dir = tempfile::tempdir().unwrap();
        let error = componentizer_parts(dir.path()).unwrap_err();
        assert!(
            error.contains(&format!("has no {}, runtime.wasm, libc.so", COMPONENTIZER)),
            "{error}"
        );
        let binary = if cfg!(windows) {
            format!("{COMPONENTIZER}.exe")
        } else {
            COMPONENTIZER.to_owned()
        };
        for file in [binary.as_str(), "runtime.wasm", "libc.so"] {
            std::fs::write(dir.path().join(file), b"part").unwrap();
        }
        let parts = componentizer_parts(dir.path()).unwrap();
        assert_eq!(parts.binary, dir.path().join(&binary));
        assert_eq!(parts.runtime, dir.path().join("runtime.wasm"));
        assert_eq!(parts.libc, dir.path().join("libc.so"));
    }
}
