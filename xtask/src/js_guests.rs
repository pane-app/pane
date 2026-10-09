//! The prebuilt JS/TS sample components (`guests/prebuilt/`): rebuilt with
//! `cargo xtask js-guests` through `pane-build`'s JavaScript build — the one
//! build path development mode and `pane-ext` use too (#218; what
//! `pane_js.py samples` did), which needs Node.js and npm alone — and
//! checked by `cargo xtask ci-lints` against the manifest this module
//! writes, so the committed components cannot drift from their sources, the
//! SDK, the WIT or the toolchain (what `pane_js.py check` did).
//!
//! Rebuilds are not byte-identical: the QuickJS snapshot the componentizer
//! takes holds build-time state. `inputs_sha256` covers the sources and the
//! toolchain, so a rebuild is asked for whenever what a component contains
//! changed; the tests compare behaviour, not bytes.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use pane_build::{BuildJob, BuildOutcome, Componentizer};
use serde_json::json;

/// (component file in `guests/prebuilt/` and `target/guests/`, source
/// package) of each prebuilt JS/TS sample.
pub(crate) const SAMPLES: [(&str, &str); 35] = [
    ("sample_js", "guests/sample-js"),
    ("sample_ts", "guests/sample-ts"),
    ("sample_settings_js", "guests/sample-settings-js"),
    ("sample_settings_ts", "guests/sample-settings-ts"),
    ("sample_operations_js", "guests/sample-operations-js"),
    ("sample_operations_ts", "guests/sample-operations-ts"),
    ("sample_applications_js", "guests/sample-applications-js"),
    ("sample_applications_ts", "guests/sample-applications-ts"),
    ("sample_query_js", "guests/sample-query-js"),
    ("sample_query_ts", "guests/sample-query-ts"),
    ("sample_no_view_js", "guests/sample-no-view-js"),
    ("sample_no_view_ts", "guests/sample-no-view-ts"),
    ("sample_search_js", "guests/sample-search-js"),
    ("sample_search_ts", "guests/sample-search-ts"),
    ("sample_helper_js", "guests/sample-helper-js"),
    ("sample_helper_ts", "guests/sample-helper-ts"),
    ("sample_files_js", "guests/sample-files-js"),
    ("sample_files_ts", "guests/sample-files-ts"),
    ("sample_clipboard_js", "guests/sample-clipboard-js"),
    ("sample_clipboard_ts", "guests/sample-clipboard-ts"),
    ("sample_npm_js", "guests/sample-npm-js"),
    ("sample_schedule_js", "guests/sample-schedule-js"),
    ("sample_schedule_ts", "guests/sample-schedule-ts"),
    ("sample_service_js", "guests/sample-service-js"),
    ("sample_service_ts", "guests/sample-service-ts"),
    ("sample_actions_js", "guests/sample-actions-js"),
    ("sample_actions_ts", "guests/sample-actions-ts"),
    ("sample_preferences_js", "guests/sample-preferences-js"),
    ("sample_preferences_ts", "guests/sample-preferences-ts"),
    ("sample_arguments_js", "guests/sample-arguments-js"),
    ("sample_arguments_ts", "guests/sample-arguments-ts"),
    ("sample_icons_js", "guests/sample-icons-js"),
    ("sample_icons_ts", "guests/sample-icons-ts"),
    ("sample_programs_js", "guests/sample-programs-js"),
    ("sample_programs_ts", "guests/sample-programs-ts"),
];

/// Pane's WIT, copied beside the world in `guests/js/wit`.
const PANE_WIT: [&str; 16] = [
    "extension.wit",
    "commands.wit",
    "feedback.wit",
    "system.wit",
    "data.wit",
    "preferences.wit",
    "root-results.wit",
    "operations.wit",
    "applications.wit",
    "search.wit",
    "helpers.wit",
    "files.wit",
    "clipboard.wit",
    "service.wit",
    "programs.wit",
    "file-index.wit",
];

/// Toolchain inputs that decide what a component contains: the wasm parts
/// (their digests are in `wasm-parts.json`, which `componentizer.yml`
/// compares against a fresh build), the vendored componentizer, the build
/// itself, and the pins of record.
const TOOL_INPUTS: [&str; 7] = [
    "tools/componentize-js/pins.json",
    "tools/componentize-js/package.json",
    "tools/componentize-js/package-lock.json",
    "tools/componentize-js/wasm-parts",
    "tools/componentize-js/componentize-qjs/crates/core",
    "crates/pane-build/src",
    "xtask/src/js_guests.rs",
];

/// Folders never part of a source digest.
const SKIP_DIRS: [&str; 2] = ["node_modules", ".git"];

/// npm packages bundled into components must keep them permissively
/// licensed, like the Cargo policy in `guests/deny.toml`.
const NPM_LICENSES: [&str; 12] = [
    "MIT",
    "MIT-0",
    "Apache-2.0",
    "Apache-2.0 OR MIT",
    "MIT OR Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "0BSD",
    "Zlib",
    "CC0-1.0",
    "Unlicense",
];

/// Rebuilds `guests/prebuilt/` from the JS/TS sample sources through
/// `pane-build`'s build, writing the manifest `check` verifies.
pub(crate) fn rebuild() -> Result<(), String> {
    let root = crate::root();
    let staging = root.join("target/js-guests");
    std::fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let mut components = serde_json::Map::new();
    for (name, source) in SAMPLES {
        let out = root.join(format!("guests/prebuilt/{name}.wasm"));
        println!("xtask: building {name}");
        let job = BuildJob::echoing(
            staging.clone(),
            std::sync::Arc::new(|line: &str| println!("{line}")),
        );
        match pane_build::build_js_command(&job, &root.join(source), &out, &componentizer()) {
            BuildOutcome::Built => {}
            BuildOutcome::Stopped => return Err(format!("{source} was stopped")),
            BuildOutcome::Failed(reason) => {
                return Err(format!("{source} did not build: {reason}"));
            }
        }
        let built = std::fs::read(&out).map_err(|error| format!("read {out:?} failed: {error}"))?;
        components.insert(
            name.into(),
            json!({
                "source": source,
                "inputs_sha256": component_inputs(&root, source),
                "bytes": built.len(),
                "sha256": sha256(&built),
            }),
        );
    }
    let manifest = json!({
        "about": "JS/TS sample components used by tests and `cargo run -p pane`; rebuild with \
                  `cargo xtask js-guests`. Rebuilds are not byte-identical: the QuickJS snapshot \
                  holds build-time state. inputs_sha256 covers the sources and toolchain pins.",
        "toolchain": toolchain(&root),
        "components": components,
    });
    let manifest = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("the manifest could not be written: {error}"))?;
    std::fs::write(root.join("guests/prebuilt/manifest.json"), manifest + "\n")
        .map_err(|error| error.to_string())?;
    println!("xtask: wrote guests/prebuilt/manifest.json");
    Ok(())
}

/// How `cargo xtask js-guests` componentizes: in this process, with the
/// componentizer pane-build links and the committed wasm parts — unless
/// PANE_COMPONENTIZER names a componentizer binary, which the componentizer
/// workflow sets to the platform's own build, so that binary builds the
/// samples through this same path.
fn componentizer() -> Componentizer {
    match std::env::var_os("PANE_COMPONENTIZER").filter(|value| !value.is_empty()) {
        Some(folder) => Componentizer::Binary(Some(PathBuf::from(folder))),
        None => Componentizer::Linked,
    }
}

/// What the prebuilt components were built with, for the manifest: the pins
/// of record (`tools/componentize-js`), the wasm parts' digests, and the
/// esbuild and TypeScript every sample's lock must pin (which `check`
/// verifies, so the samples cannot drift from the record).
fn toolchain(root: &Path) -> serde_json::Value {
    let pins: serde_json::Value = read_json(&root.join("tools/componentize-js/pins.json"));
    let parts: serde_json::Value =
        read_json(&root.join("tools/componentize-js/wasm-parts/wasm-parts.json"));
    let [esbuild, typescript] = tool_versions(root);
    json!({
        "componentize_qjs": pins["componentize_qjs"],
        "patches": pins_patches(root),
        "runtime_sha256": parts["runtime_sha256"],
        "libc_sha256": parts["libc_sha256"],
        "componentizer": "pane-build's JavaScript build, componentizing in process with the \
                          vendored componentize-qjs (Wasmtime 49.0.1)",
        "esbuild": esbuild,
        "typescript": typescript,
    })
}

/// The esbuild and TypeScript versions of the pin of record
/// (`tools/componentize-js/package-lock.json`).
fn tool_versions(root: &Path) -> [String; 2] {
    let lock: serde_json::Value = read_json(&root.join("tools/componentize-js/package-lock.json"));
    let version = |name: &str| {
        lock["packages"][format!("node_modules/{name}")]["version"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };
    [version("esbuild"), version("typescript")]
}

/// The digests of the patch queue, which the vendored tree carries applied.
fn pins_patches(root: &Path) -> serde_json::Value {
    let pins: serde_json::Value = read_json(&root.join("tools/componentize-js/pins.json"));
    let mut patches = serde_json::Map::new();
    for patch in pins["patches"]
        .as_array()
        .expect("pins.json names the patches")
    {
        let name = patch.as_str().expect("a patch name");
        let digest = std::fs::read(root.join("tools/componentize-js/patches").join(name))
            .expect("the patch queue is committed");
        patches.insert(name.into(), json!(sha256(&digest)));
    }
    serde_json::Value::Object(patches)
}

/// A committed JSON file, read.
fn read_json(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} cannot be read: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not valid JSON: {error}", path.display()))
}

/// Verifies `guests/prebuilt/` against its manifest and the current sources:
/// every component present, matching its recorded digest, and built from the
/// current inputs — the sources, the SDK, the WIT, or the toolchain. Needs
/// no toolchain itself, so `cargo xtask ci-lints` gates the committed
/// components without one.
pub(crate) fn check() -> Result<(), String> {
    let root = crate::root();
    let text = std::fs::read_to_string(root.join("guests/prebuilt/manifest.json"))
        .map_err(|error| format!("guests/prebuilt/manifest.json cannot be read: {error}"))?;
    let manifest: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("guests/prebuilt/manifest.json is not valid JSON: {error}"))?;
    let mut problems = Vec::new();
    for (name, source) in SAMPLES {
        let entry = manifest["components"].get(name);
        let path = root.join(format!("guests/prebuilt/{name}.wasm"));
        let Some(entry) = entry else {
            problems.push(format!("{name} is missing from the manifest"));
            continue;
        };
        if !path.is_file() {
            problems.push(format!("{name} is missing"));
            continue;
        }
        if let Ok(built) = std::fs::read(&path)
            && entry["sha256"].as_str() != Some(&sha256(&built))
        {
            problems.push(format!("{name} does not match its manifest sha256"));
        }
        if entry["inputs_sha256"].as_str() != Some(&component_inputs(&root, source)) {
            problems.push(format!(
                "{name} is stale: {source} or the toolchain changed since it was built"
            ));
        }
    }
    if !problems.is_empty() {
        return Err(format!(
            "{}. Run `cargo xtask js-guests`.",
            problems.join("; ")
        ));
    }
    println!("xtask: prebuilt JS/TS components match their sources");
    check_tool_versions(&root, &mut problems);
    check_npm_licenses(&root, &mut problems);
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    Ok(())
}

/// The samples must pin the esbuild and TypeScript of the record, so the
/// manifest's toolchain names what built them.
fn check_tool_versions(root: &Path, problems: &mut Vec<String>) {
    let [esbuild, typescript] = tool_versions(root);
    for (_, source) in SAMPLES {
        let lock = package_lock(root, source);
        for (name, wanted) in [("esbuild", &esbuild), ("typescript", &typescript)] {
            let version = lock["packages"][format!("node_modules/{name}")]["version"].as_str();
            if version != Some(wanted.as_str()) {
                problems.push(format!(
                    "{source} pins {name} {}, while the toolchain of record has {wanted}",
                    version.unwrap_or("none")
                ));
            }
        }
    }
}

/// Every npm package a sample bundles must be permissively licensed.
fn check_npm_licenses(root: &Path, problems: &mut Vec<String>) {
    for (_, source) in SAMPLES {
        let lock = package_lock(root, source);
        for (path, package) in lock["packages"].as_object().into_iter().flatten() {
            let linked = package
                .get("link")
                .is_some_and(|link| link.as_bool() == Some(true));
            if linked || path.starts_with("..") || path.is_empty() {
                continue; // in-repo packages carry the repository's own license
            }
            let dev = package
                .get("dev")
                .is_some_and(|dev| dev.as_bool() == Some(true));
            let license = package["license"].as_str().unwrap_or("");
            if !dev && !NPM_LICENSES.contains(&license) {
                problems.push(format!("{source}: {path} is licensed {license:?}"));
            }
        }
    }
    println!("xtask: bundled npm packages are permissively licensed");
}

/// The package-lock.json of a sample, as JSON.
fn package_lock(root: &Path, source: &str) -> serde_json::Value {
    read_json(&root.join(source).join("package-lock.json"))
}

/// The digest of everything a component contains: the tool inputs, Pane's
/// WIT and WASI's, the SDK, and the sample's own sources.
fn component_inputs(root: &Path, source: &str) -> String {
    let mut paths: Vec<PathBuf> = TOOL_INPUTS.iter().map(|input| root.join(input)).collect();
    for name in PANE_WIT {
        paths.push(root.join("wit").join(name));
    }
    paths.extend(wasi_wit(root));
    paths.push(root.join("guests/js"));
    paths.push(root.join(source));
    inputs_digest(root, &paths)
}

/// The WASI WIT (`wit/deps`), in name order.
fn wasi_wit(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root.join("wit/deps")) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "wit"))
        .collect();
    paths.sort();
    paths
}

/// The digest of the files under `paths`, by repository-relative path, with
/// line endings normalized (a checkout on Windows has CRLF where one on
/// Linux has LF) and files ordered by their parts, which compare
/// case-sensitively everywhere. A file is digested as its path (POSIX
/// spelling), a NUL, its contents, a NUL.
fn inputs_digest(root: &Path, paths: &[PathBuf]) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    // By their parts, which compare case-sensitively everywhere: Windows
    // paths compare ignoring case, which would order `README.md` and
    // `adapt.js` otherwise than Linux does, and so digest them differently.
    let mut files: Vec<(Vec<String>, PathBuf)> = paths
        .iter()
        .flat_map(|path| tree_files(path))
        .map(|path| (path_parts(&path), path))
        .collect();
    files.sort();
    for (_, path) in files {
        digest.update(
            path.strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/")
                .as_bytes(),
        );
        digest.update([0]);
        let contents = std::fs::read(&path).unwrap_or_default();
        digest.update(normalized(&contents));
        digest.update([0]);
    }
    hex(&digest.finalize())
}

/// The files under `root` (itself when it is one), never entering
/// `node_modules` or `.git`.
fn tree_files(root: &Path) -> Vec<PathBuf> {
    if root.is_file() {
        return vec![root.to_path_buf()];
    }
    let mut files = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if path.is_dir() {
                if !SKIP_DIRS.contains(&name.as_ref()) {
                    folders.push(path);
                }
            } else {
                files.push(path);
            }
        }
    }
    files
}

/// A path's parts, for the case-sensitive order.
fn path_parts(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|part| match part {
            std::path::Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// Contents with Windows' line endings as they are committed.
fn normalized(contents: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(contents.len());
    let mut at = 0;
    while at < contents.len() {
        if contents[at] == b'\r' && contents.get(at + 1) == Some(&b'\n') {
            out.push(b'\n');
            at += 2;
        } else {
            out.push(contents[at]);
            at += 1;
        }
    }
    out
}

/// The sha256 of `bytes`, as lowercase hex.
fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(bytes);
    hex(&digest.finalize())
}

/// A digest as lowercase hex.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
