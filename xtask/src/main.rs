//! Portable developer commands, run as `cargo xtask <command>`.
//!
//! - `guests`: build the Rust guests and copy them, with the prebuilt JS/TS
//!   sample components from `guests/prebuilt/`, into `target/guests/`, and
//!   assemble the sample packages into `target/guests/packages/`, with the
//!   helper sample's native helper built for this system, the npm sample
//!   into `target/guests/npm/`, packed as npm packs it, and the Git sample
//!   into `target/guests/git/greeter/`, its source with its built component
//!   under `dist/`, as a release revision holds it. It also builds the
//!   componentizer (`componentize-qjs`'s `p3_build` example, with the
//!   committed wasm parts) into `target/guests/componentizer/`, the binary
//!   a development build of an installed Pane spawns when the package has
//!   no `@pane-app/cli` platform package of its own.
//! - `js-guests`: rebuild the prebuilt JS/TS sample components through
//!   `pane-build`'s JavaScript build (prerequisites: Node.js 22+ with npm;
//!   the samples' `package.json`s pin esbuild and TypeScript), then run
//!   `guests`.
//! - `schema`: generate `pane.json`'s JSON Schema from pane-core's own
//!   manifest types and check the committed copy against it (`--write` to
//!   rewrite the file), so the schema cannot drift from what Pane reads
//!   (#224). `ci-lints` runs it, and ci-fast.yml regenerates a stale copy
//!   on a push and uploads it as an artifact, as it does the JS/TS samples.
//! - `ci`: the lints of `ci-lints`, then the tests of `ci-tests`.
//! - `ci-lints`: check formatting, the committed `pane.json` schema, the
//!   prebuilt JS/TS samples, the SDKs' packages (`sdks`) and clippy: the
//!   half of `ci` that runs no tests and builds no guest but the Rust SDK.
//!   The prebuilt-samples check needs no toolchain at all (#218): it
//!   verifies digests and staleness in `js_guests`.
//! - `sdks`: check that the SDKs package as they would be published,
//!   publishing nothing: the Rust SDK's copy of the WIT is `wit/`, `cargo
//!   publish --dry-run` packages `pane-extension` and builds it from the
//!   package alone, and `npm pack` packs `@pane-app/extension` and
//!   `@pane-app/create` into `target/sdks/`. Publishing them is a person's
//!   step, never CI's (#128).
//! - `ci-tests`: build the guests, then run the workspace's tests with
//!   cargo-nextest, which retries a failing test twice before the run
//!   fails for it, so one flaky failure costs time, not the run. Options
//!   after `ci-tests` (or `ci`) go to `cargo nextest run` as they are: CI
//!   passes `--partition hash:1/3` to run one shard of the tests, `-E
//!   <filter>` to run only some, `--no-run` to build them only.
//! - `package-linux`: build Pane's Linux package and the artifacts its
//!   default extensions are acquired from, under `target/dist/` (with
//!   `--dev`, the package's program is the development profile; see
//!   `package.rs`).
//! - `package-windows`: build Pane's Windows package and the same
//!   artifacts, under `target/dist/` (`--dev` as for `package-linux`;
//!   `package.rs` assembles the artifacts everywhere and builds the
//!   package itself only on Windows).
//! - `package-macos`: build Pane's macOS package and the same artifacts,
//!   under `target/dist/` (`--dev` as for `package-linux`; `package.rs`
//!   assembles the artifacts everywhere and builds the package itself
//!   only on macOS).
//! - `file-index-bench`: build the file index's benchmark in release and
//!   run it with the options that follow (#174; by default a generated
//!   tree of 450,000 entries; `--home` indexes the real home folder, read
//!   only; the options are listed in
//!   `crates/pane-core/examples/file_index_bench.rs`). It runs on demand;
//!   CI runs only `file-index-guard`.
//! - `file-index-guard`: the file index's regression guard (#183): the
//!   same benchmark, as the tests built it (development profile), over a
//!   reduced generated tree of about 20,000 entries with `--guard`, which
//!   fails when a measure is over its generous ceiling. `ci-branch.yml`'s
//!   Linux tests run it in one shard, after the tests.

mod js_guests;
mod package;
mod zip;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const GUEST_TARGET: &str = "wasm32-wasip2";

/// npm's own fixed time for packed files, 1985-10-26T08:15:00Z: the
/// tarballs this repository packs (the npm sample, the default
/// extensions' payloads, the Linux package) are the same on every system,
/// so a source serves one integrity everywhere. `pane-ext pack`'s
/// tarballs use it too, from where it lives (`pane_core::pack`).
pub(crate) use pane_core::pack::PACKED_MTIME;

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    // These pass their options on as they are: the benchmark's are its
    // own, the tests' are cargo-nextest's.
    let options = || std::env::args().skip(2).collect::<Vec<_>>();
    let passing_on = match task.as_deref() {
        Some("file-index-bench") => Some(file_index_bench(options())),
        Some("ci-tests") => Some(ci_tests(&options())),
        Some("ci") => Some(ci(&options())),
        _ => None,
    };
    if let Some(result) = passing_on {
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("xtask: {message}");
                ExitCode::FAILURE
            }
        };
    }
    let dev = std::env::args().any(|arg| arg == "--dev");
    // The version a package names its program by, when it is not this
    // workspace's own: dotted numbers, as Pane reads versions.
    let version = std::env::args()
        .position(|arg| arg == "--package-version")
        .and_then(|at| std::env::args().nth(at + 1))
        .map(|version| {
            version
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
                .then_some(version.clone())
                .ok_or_else(|| {
                    format!(
                        "--package-version must be dotted numbers such as 99.0.0, not \
                         `{version}`: it is the version the packaged program reports and \
                         Pane's index names"
                    )
                })
        })
        .transpose();
    // `schema --write` rewrites the committed schema it differs from.
    let write = std::env::args().any(|arg| arg == "--write");
    let result = match (task.as_deref(), version) {
        (Some("guests"), _) => guests(),
        (Some("js-guests"), _) => js_guests(std::env::args().any(|arg| arg == "--check")),
        (Some("schema"), _) => schema(write),
        (Some("ci-lints"), _) => ci_lints(),
        (Some("sdks"), _) => sdks(),
        (Some("file-index-guard"), _) => file_index_guard(),
        (Some("package-linux"), Ok(version)) => package::linux(dev, version),
        (Some("package-windows"), Ok(version)) => package::windows(dev, version),
        (Some("package-macos"), Ok(version)) => package::macos(dev, version),
        (_, Err(why)) => Err(why),
        _ => Err("usage: cargo xtask \
             <guests|js-guests|ci-lints|sdks|schema|file-index-guard|package-linux|\
             package-windows|package-macos> [--dev] [--package-version <version>], cargo xtask \
             schema [--write], cargo xtask js-guests --check (the staleness check alone), \
             cargo xtask <ci|ci-tests> [nextest options], or cargo xtask \
             file-index-bench [options]"
            .into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

fn run(command: &mut Command) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("failed to start {command:?}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command:?} failed with {status}"))
    }
}

/// Builds each guest workspace and copies its components to `target/guests`.
fn guests() -> Result<(), String> {
    let root = root();
    let out = root.join("target/guests");
    std::fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    // (workspace directory, component file names)
    let workspaces: [(&str, &[&str]); 2] = [
        (
            "guests",
            &[
                "sample_rust",
                "sample_settings",
                "calculator",
                "applications",
                "quicklinks",
                "files",
                "clipboard_history",
                "sample_operations",
                "sample_dependencies",
                "sample_query",
                "sample_no_view",
                "sample_search",
                "sample_schedule",
                "sample_service",
                "sample_actions",
                "sample_preferences",
                "sample_arguments",
                "sample_helper",
                "sample_icons",
                "sample_programs",
                "sample_files",
                "faulty",
                "folder_files",
                "operations_fixture",
                "old_api",
                "mismatched_api",
                "failing_start",
                "refusing_view",
                "tree_fixture",
                "git_greeter",
            ],
        ),
        ("guests/fixtures/mixed-p2", &["mixed_p2"]),
    ];
    for (dir, components) in workspaces {
        let dir = root.join(dir);
        run(cargo().current_dir(&dir).args([
            "build",
            "--locked",
            "--release",
            "--target",
            GUEST_TARGET,
        ]))?;
        for name in components {
            let built = dir.join(format!("target/{GUEST_TARGET}/release/{name}.wasm"));
            let dest = out.join(format!("{name}.wasm"));
            std::fs::copy(&built, &dest)
                .map_err(|error| format!("copy {} failed: {error}", built.display()))?;
        }
    }
    for (name, _) in js_guests::SAMPLES {
        let prebuilt = root.join(format!("guests/prebuilt/{name}.wasm"));
        std::fs::copy(&prebuilt, out.join(format!("{name}.wasm")))
            .map_err(|error| format!("copy {} failed: {error}", prebuilt.display()))?;
        // Its source map, which the prebuilt samples carry beside their
        // components: the tests that develop one want the stacks the sample
        // throws mapped to its sources (#214).
        let map = root.join(format!("guests/prebuilt/{name}.wasm.map"));
        if map.exists() {
            std::fs::copy(&map, out.join(format!("{name}.wasm.map")))
                .map_err(|error| format!("copy {} failed: {error}", map.display()))?;
        }
    }
    // Ready-to-run sample packages: each manifest in guests/packages with the
    // component it names, and the images it shows (#139): the other files
    // and folders beside its manifest.
    for (package, component) in SAMPLE_PACKAGES {
        let dest = out.join("packages").join(package);
        std::fs::create_dir_all(&dest).map_err(|error| error.to_string())?;
        let copies = [
            (
                root.join(format!("guests/packages/{package}/pane.json")),
                dest.join("pane.json"),
            ),
            (
                out.join(format!("{component}.wasm")),
                dest.join(format!("{component}.wasm")),
            ),
        ];
        for (from, to) in copies {
            std::fs::copy(&from, &to)
                .map_err(|error| format!("copy {} failed: {error}", from.display()))?;
        }
        // The component's source map, when the prebuilt sample has one, for
        // the tests that develop the package (#214).
        let map = out.join(format!("{component}.wasm.map"));
        if map.exists() {
            std::fs::copy(&map, dest.join(format!("{component}.wasm.map")))
                .map_err(|error| format!("copy {} failed: {error}", map.display()))?;
        }
        // Everything beside the package's pane.json: its icons, its
        // assets/ folder, and the help (HELP.md) its Setup screen shows.
        copy_package_files(&root.join("guests/packages").join(package), &dest)?;
    }
    echo_helper(&root, &out)?;
    componentizer(&root, &out)?;
    npm_sample(&root, &out)?;
    git_sample(&root, &out)?;
    println!("guests built into {}", out.display());
    Ok(())
}

/// Builds the componentizer for this system — `componentize-qjs`'s
/// `p3_build` example, the vendored crate of the workspace — and puts it,
/// with the committed wasm parts, in `target/guests/componentizer/`: the
/// binary a development build spawns when it does not link the componentizer
/// in-process (a Pane checkout's default, see `pane/src/main.rs`), and the
/// tests' stand-in for the `@pane-app/cli` platform package a package
/// installs with `npm install` (#218's development-mode contract; #219's
/// packages).
fn componentizer(root: &Path, out: &Path) -> Result<(), String> {
    run(cargo().current_dir(root).args([
        "build",
        "--locked",
        "-p",
        "componentize-qjs",
        "--example",
        "p3_build",
    ]))?;
    // Where cargo built it: `CARGO_TARGET_DIR`, else `CARGO_BUILD_TARGET_DIR`
    // (`build.target-dir` from the environment), else `target`, in the
    // development profile's `debug`, as the tests built the workspace.
    let target = ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"]
        .into_iter()
        .find_map(std::env::var_os)
        .map(|dir| root.join(dir))
        .unwrap_or_else(|| root.join("target"));
    let binary = target
        .join("debug/examples")
        .join(format!("p3_build{}", std::env::consts::EXE_SUFFIX));
    let dest = out.join("componentizer");
    std::fs::create_dir_all(&dest).map_err(|error| error.to_string())?;
    let componentizer = if cfg!(windows) {
        "componentize-qjs-p3.exe"
    } else {
        "componentize-qjs-p3"
    };
    std::fs::copy(&binary, dest.join(componentizer))
        .map_err(|error| format!("copy {} failed: {error}", binary.display()))?;
    for part in ["runtime.wasm", "libc.so"] {
        std::fs::copy(
            root.join("tools/componentize-js/wasm-parts").join(part),
            dest.join(part),
        )
        .map_err(|error| format!("copy {part} failed: {error}"))?;
    }
    Ok(())
}

/// Copies what a sample package folder `from` holds besides its
/// `pane.json` (an icon, its `assets` folder) into `to`, folders and all.
fn copy_package_files(from: &Path, to: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(from)
        .map_err(|error| format!("read {} failed: {error}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            copy_package_files(&path, &target)?;
        } else if entry.file_name() != "pane.json" {
            std::fs::copy(&path, &target)
                .map_err(|error| format!("copy {} failed: {error}", path.display()))?;
        }
    }
    Ok(())
}

/// Builds the helper samples' native helper, `pane-echo`, for the system
/// this runs on, and puts it in each helper sample package (Rust,
/// JavaScript, TypeScript) as that target's file, as its `pane.json` names
/// it. CI runs this natively on each of its three systems, so each runs a
/// real helper built for it. An author ships one build per target;
/// Pane runs the one for its own system and compiles nothing.
fn echo_helper(root: &Path, out: &Path) -> Result<(), String> {
    let dir = root.join("guests/helpers/echo");
    run(cargo()
        .current_dir(&dir)
        .args(["build", "--locked", "--release"]))?;
    let target = pane_target::Target::current()
        .ok_or("Pane names no helper target for this system and processor")?;
    let file = format!("pane-echo{}", target.exe_suffix());
    let target = target.id();
    let built = dir.join("target/release").join(&file);
    for package in ["sample-helper", "sample-helper-js", "sample-helper-ts"] {
        let dest = out
            .join("packages")
            .join(package)
            .join("helpers")
            .join(&target);
        std::fs::create_dir_all(&dest).map_err(|error| error.to_string())?;
        std::fs::copy(&built, dest.join(&file))
            .map_err(|error| format!("copy {} failed: {error}", built.display()))?;
    }
    Ok(())
}

/// Assembles the npm sample (`guests/npm/greeter`: its `package.json`, its
/// `pane.json` and the components it names) into `target/guests/npm/greeter/`
/// and packs it into `target/guests/npm/pane-samples-greeter-0.1.0.tgz`, the
/// tarball `npm pack` makes of that folder: every file under `package/`, in a
/// gzipped tar. The tarball is the same on every system (fixed times, owners
/// and modes, files in name order), so the local registry of the tests and
/// smokes serves one integrity everywhere. It is never published.
fn npm_sample(root: &Path, out: &Path) -> Result<(), String> {
    let source = root.join("guests/npm/greeter");
    let dest = out.join("npm/greeter");
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest).map_err(|error| error.to_string())?;
    let mut files = vec!["package.json".to_owned(), "pane.json".to_owned()];
    let manifest = std::fs::read_to_string(source.join("package.json"))
        .map_err(|error| format!("read the npm sample's package.json failed: {error}"))?;
    // Its one component, the prebuilt `sample_npm_js`.
    let component = "sample_npm_js.wasm";
    if !manifest.contains(component) {
        return Err(format!(
            "the npm sample's package.json does not list {component}"
        ));
    }
    files.push(component.to_owned());
    for file in &files {
        let from = match file.ends_with(".wasm") {
            true => out.join(file),
            false => source.join(file),
        };
        std::fs::copy(&from, dest.join(file))
            .map_err(|error| format!("copy {} failed: {error}", from.display()))?;
    }
    files.sort();
    let mut tar = tar::Builder::new(Vec::new());
    for file in &files {
        let contents = std::fs::read(dest.join(file)).map_err(|error| error.to_string())?;
        let mut header = tar::Header::new_ustar();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(PACKED_MTIME);
        header.set_uid(0);
        header.set_gid(0);
        header.set_entry_type(tar::EntryType::Regular);
        tar.append_data(&mut header, format!("package/{file}"), contents.as_slice())
            .map_err(|error| error.to_string())?;
    }
    let tar = tar.into_inner().map_err(|error| error.to_string())?;
    let mut gz = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), flate2::Compression::best());
    std::io::Write::write_all(&mut gz, &tar).map_err(|error| error.to_string())?;
    let tgz = gz.finish().map_err(|error| error.to_string())?;
    let packed = out.join("npm/pane-samples-greeter-0.1.0.tgz");
    std::fs::write(&packed, tgz)
        .map_err(|error| format!("write {} failed: {error}", packed.display()))?;
    Ok(())
}

/// Assembles the Git sample (`guests/git/greeter`) into
/// `target/guests/git/greeter/`: its source files (`pane.json`,
/// `Cargo.toml`, `README.md`, `src/lib.rs`) and, under `dist/`, the
/// component built from them, `dist/git_greeter.wasm`. The tests and smokes
/// commit the source files as a source-only revision and then `dist/` as a
/// release revision of a repository they make; it is never pushed anywhere.
fn git_sample(root: &Path, out: &Path) -> Result<(), String> {
    let source = root.join("guests/git/greeter");
    let dest = out.join("git/greeter");
    let _ = std::fs::remove_dir_all(&dest);
    for file in ["pane.json", "Cargo.toml", "README.md", "src/lib.rs"] {
        let to = dest.join(file);
        std::fs::create_dir_all(to.parent().expect("inside the sample"))
            .map_err(|error| error.to_string())?;
        std::fs::copy(source.join(file), &to)
            .map_err(|error| format!("copy {file} of the Git sample failed: {error}"))?;
    }
    let built = out.join("git_greeter.wasm");
    std::fs::create_dir_all(dest.join("dist")).map_err(|error| error.to_string())?;
    std::fs::copy(&built, dest.join("dist/git_greeter.wasm"))
        .map_err(|error| format!("copy {} failed: {error}", built.display()))?;
    Ok(())
}

/// (package folder in `guests/packages`, component) of each sample package,
/// and of the default extensions (the calculator, applications and
/// quicklinks).
const SAMPLE_PACKAGES: [(&str, &str); 59] = [
    ("sample-rust", "sample_rust"),
    ("sample-settings", "sample_settings"),
    ("sample-js", "sample_js"),
    ("sample-ts", "sample_ts"),
    ("sample-settings-js", "sample_settings_js"),
    ("sample-settings-ts", "sample_settings_ts"),
    ("calculator", "calculator"),
    ("applications", "applications"),
    ("quicklinks", "quicklinks"),
    ("files", "files"),
    ("clipboard-history", "clipboard_history"),
    ("sample-operations", "sample_operations"),
    ("sample-operations-js", "sample_operations_js"),
    ("sample-operations-ts", "sample_operations_ts"),
    ("sample-dependencies", "sample_dependencies"),
    ("sample-dependencies-npm", "sample_dependencies"),
    ("sample-applications-js", "sample_applications_js"),
    ("sample-applications-ts", "sample_applications_ts"),
    ("sample-query", "sample_query"),
    ("sample-query-js", "sample_query_js"),
    ("sample-query-ts", "sample_query_ts"),
    ("sample-no-view", "sample_no_view"),
    ("sample-no-view-js", "sample_no_view_js"),
    ("sample-no-view-ts", "sample_no_view_ts"),
    ("sample-search", "sample_search"),
    ("sample-schedule", "sample_schedule"),
    ("sample-service", "sample_service"),
    ("sample-schedule-js", "sample_schedule_js"),
    ("sample-schedule-ts", "sample_schedule_ts"),
    ("sample-service-js", "sample_service_js"),
    ("sample-service-ts", "sample_service_ts"),
    ("sample-search-js", "sample_search_js"),
    ("sample-search-ts", "sample_search_ts"),
    ("sample-helper", "sample_helper"),
    ("sample-helper-js", "sample_helper_js"),
    ("sample-helper-ts", "sample_helper_ts"),
    ("sample-files", "sample_files"),
    ("sample-files-js", "sample_files_js"),
    ("sample-files-ts", "sample_files_ts"),
    ("sample-clipboard-js", "sample_clipboard_js"),
    ("sample-clipboard-ts", "sample_clipboard_ts"),
    ("sample-actions", "sample_actions"),
    ("sample-actions-js", "sample_actions_js"),
    ("sample-actions-ts", "sample_actions_ts"),
    ("sample-preferences", "sample_preferences"),
    ("sample-preferences-js", "sample_preferences_js"),
    ("sample-preferences-ts", "sample_preferences_ts"),
    ("sample-arguments", "sample_arguments"),
    ("sample-arguments-js", "sample_arguments_js"),
    ("sample-arguments-ts", "sample_arguments_ts"),
    ("sample-icons", "sample_icons"),
    ("sample-icons-js", "sample_icons_js"),
    ("sample-icons-ts", "sample_icons_ts"),
    ("sample-icons-plain", "sample_icons"),
    ("sample-icons-plain-js", "sample_icons_js"),
    ("sample-icons-plain-ts", "sample_icons_ts"),
    ("sample-programs", "sample_programs"),
    ("sample-programs-js", "sample_programs_js"),
    ("sample-programs-ts", "sample_programs_ts"),
];

/// Rebuilds `guests/prebuilt/` from the JS/TS sample sources with
/// `pane-build`'s JavaScript build, then refreshes `target/guests/`. With
/// `--check`, only verifies the committed components against their manifest
/// and the current sources (the staleness half of `ci-lints`), without
/// building anything.
fn js_guests(check_only: bool) -> Result<(), String> {
    if check_only {
        return js_guests::check();
    }
    js_guests::rebuild()?;
    guests()
}

/// Checks the committed `pane.json` schema against what Pane's manifest
/// types generate, or rewrites it (`--write`). `ci-lints` runs the check,
/// and ci-fast.yml regenerates a stale copy on a push and uploads it as an
/// artifact, as it does the JS/TS samples (#224).
fn schema(write: bool) -> Result<(), String> {
    let path = root().join(SCHEMA);
    let generated = pane_core::schema::manifest_schema();
    if !write {
        // The file is committed with LF endings and checked out with
        // whatever this system uses; the generated schema is one text.
        let committed = std::fs::read_to_string(&path)
            .map_err(|error| format!("read {} failed: {error}", path.display()))?
            .replace("\r\n", "\n");
        if committed == generated {
            println!(
                "{} matches what Pane's manifest types generate",
                path.display()
            );
            return Ok(());
        }
        return Err(format!(
            "{} does not match what Pane's manifest types generate: rewrite it with \
             `cargo xtask schema --write`",
            path.display()
        ));
    }
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    std::fs::write(&path, generated)
        .map_err(|error| format!("write {} failed: {error}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// Where the generated schema is committed: beside the SDK that ships it,
/// inside the `@pane-app/extension` package (`npm pack` carries the folder).
const SCHEMA: &str = "guests/js/schema/pane.schema.json";

/// The lints half of `ci`: the formatting checks, the prebuilt-samples
/// check, the SDKs' packages and clippy — everything that runs no tests
/// and builds no guest but the Rust SDK, which `sdks` builds from its
/// package. `ci-branch.yml` runs this as a job beside `ci-tests`, so the
/// lints and the tests of a push finish in the time of the slower one.
fn ci_lints() -> Result<(), String> {
    // Formatting first: it is free, so a formatting error is seen at once
    // instead of after the guests and the checks have been built.
    let root = root();
    run(cargo().current_dir(&root).args(["fmt", "--all", "--check"]))?;
    for dir in [
        "guests",
        "guests/fixtures/mixed-p2",
        "guests/helpers/echo",
        "guests/hello-rust",
        // The Rust templates pane-ext new writes, each a package of its
        // own that no workspace holds (#221).
        "crates/pane-core/templates/rust/list",
        "crates/pane-core/templates/rust/detail",
        "crates/pane-core/templates/rust/form",
        "crates/pane-core/templates/rust/no-view",
    ] {
        run(cargo()
            .current_dir(root.join(dir))
            .args(["fmt", "--all", "--check"]))?;
    }
    // The command files pane-ext new command writes are no package's
    // source, so rustfmt checks them directly.
    let rustfmt = if cfg!(windows) {
        "rustfmt.exe"
    } else {
        "rustfmt"
    };
    run(Command::new(rustfmt)
        .current_dir(&root)
        .arg("--check")
        .arg("--edition")
        .arg("2024")
        .args([
            "crates/pane-core/templates/rust/command/list.rs",
            "crates/pane-core/templates/rust/command/detail.rs",
            "crates/pane-core/templates/rust/command/form.rs",
            "crates/pane-core/templates/rust/command/no-view.rs",
        ]))?;
    // The committed pane.json schema is the one Pane's manifest types
    // generate, before anything slower runs.
    schema(false)?;
    // The prebuilt JS/TS samples must match their sources and pins.
    js_guests::check()?;
    sdks()?;
    let clippy = [
        "clippy",
        "--locked",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ];
    run(cargo().current_dir(&root).args(clippy))?;
    // The helper sample's native helper, an ordinary program of its own.
    run(cargo()
        .current_dir(root.join("guests/helpers/echo"))
        .args(clippy))
}

/// Checks that the SDKs package as they would be published, publishing
/// nothing: the WIT the Rust SDK carries, and is generated from, is a copy
/// of `wit/`; `cargo publish --dry-run` packages `pane-extension` and
/// builds it from its package alone, as crates.io would; and `npm pack`
/// packs `@pane-app/extension` and `@pane-app/create` (the package `npm
/// create @pane-app` runs, #221) into `target/sdks/`. Publishing them is a
/// person's step, never CI's (#128).
fn sdks() -> Result<(), String> {
    let root = root();
    same_files(&root.join("wit"), &root.join("guests/pane-extension/wit"))?;
    let guests = root.join("guests");
    run(cargo().current_dir(&guests).args([
        "publish",
        "--dry-run",
        "--allow-dirty",
        "-p",
        "pane-extension",
        "--target",
        GUEST_TARGET,
    ]))?;
    let out = root.join("target/sdks");
    std::fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    // npm is a batch file on Windows, which is started by its full name.
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    run(Command::new(npm)
        .current_dir(root.join("guests/js"))
        .arg("pack")
        .arg("--pack-destination")
        .arg(&out))?;
    // The create package packs the same way, so what npm would publish of
    // it is checked: its one dependency (@pane-app/cli) resolves at
    // install, never at pack.
    run(Command::new(npm)
        .current_dir(root.join("packages/create"))
        .arg("pack")
        .arg("--pack-destination")
        .arg(&out))
}

/// Checks that the folder `copy` holds the files `original` holds, with the
/// same contents, and nothing else.
fn same_files(original: &Path, copy: &Path) -> Result<(), String> {
    if files_under(original)? == files_under(copy)? {
        Ok(())
    } else {
        Err(format!(
            "{} is not a copy of {}: copy the folder over it again",
            copy.display(),
            original.display()
        ))
    }
}

/// Every file under `dir`, by its path relative to `dir`, with its contents.
fn files_under(dir: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let entries = std::fs::read_dir(&folder)
            .map_err(|error| format!("read {} failed: {error}", folder.display()))?;
        for entry in entries {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let contents = std::fs::read(&path)
                    .map_err(|error| format!("read {} failed: {error}", path.display()))?;
                files.insert(path.strip_prefix(dir).unwrap().to_path_buf(), contents);
            }
        }
    }
    Ok(files)
}

/// The tests half of `ci`: build the guests the tests use, then run the
/// workspace's tests with cargo-nextest, which retries a failing test
/// twice before the run fails for it, so one flaky failure costs time
/// rather than the run. cargo-nextest runs no doc tests; this workspace
/// has none (its documentation's code fences are `text` and `json`, not
/// Rust), so nothing that `cargo test` ran is lost. `nextest` goes on to
/// `cargo nextest run` as it is (a shard, a filter, `--no-run`).
fn ci_tests(nextest: &[String]) -> Result<(), String> {
    guests()?;
    run(cargo()
        .current_dir(root())
        .args([
            "nextest",
            "run",
            "--locked",
            "--workspace",
            "--retries",
            "2",
        ])
        .args(nextest))
}

fn ci(nextest: &[String]) -> Result<(), String> {
    ci_lints()?;
    ci_tests(nextest)
}

/// Builds the file index's benchmark in release and runs it with `args`.
fn file_index_bench(args: Vec<String>) -> Result<(), String> {
    run(cargo()
        .current_dir(root())
        .args([
            "run",
            "--locked",
            "--release",
            "-p",
            "pane-core",
            "--example",
            "file_index_bench",
            "--",
        ])
        .args(args))
}

/// The file index's regression guard (#183): the benchmark over a reduced
/// generated tree (about 20,000 entries, one run, the walk at normal
/// priority) with its ceilings (`--guard`), failing when a measure is over
/// one. It runs the benchmark as the workspace's tests build it, in the
/// development profile, so that CI reuses their build instead of building
/// Pane's dependencies again in release: `cargo build --workspace
/// --examples` selects the packages and features the tests do, so it only
/// builds the example, if the tests have not already. Its ceilings are set
/// for that unoptimized build.
fn file_index_guard() -> Result<(), String> {
    let root = root();
    run(cargo()
        .current_dir(&root)
        .args(["build", "--locked", "--workspace", "--examples"]))?;
    // Where cargo built it: `CARGO_TARGET_DIR`, else `CARGO_BUILD_TARGET_DIR`
    // (`build.target-dir` from the environment), else `target`, in the
    // development profile's `debug`. A `build.target-dir` in a cargo
    // configuration file is not read: Pane's `.cargo/config.toml` sets none.
    let target = ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"]
        .into_iter()
        .find_map(std::env::var_os)
        .map(|dir| root.join(dir))
        .unwrap_or_else(|| root.join("target"));
    let program = format!("file_index_bench{}", std::env::consts::EXE_SUFFIX);
    let program = target.join("debug").join("examples").join(program);
    let options = [
        "--generate",
        "20000",
        "--runs",
        "1",
        "--queries",
        "550",
        "--foreground",
        "--guard",
    ];
    run(Command::new(program).current_dir(&root).args(options))
}
