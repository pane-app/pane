//! The maintenance commands of the JS/TS toolchain and the npm packages
//! that ship it (`#219`, spec `#128`): what `tools/componentize-js/
//! pane_js.py` did, in the repository's task runner — so building, checking
//! and shipping a JavaScript or TypeScript package needs no Python, and the
//! toolchain's own maintenance needs only rustup: the pinned nightly and
//! wasi-sdk for the runtime, the repository's stable Rust for everything
//! else, and curl (present on every system) for the one download.
//!
//! - `cli-package <dir>`: build `pane-ext` and the componentizer
//!   (`componentize-qjs`'s `p3_build` example) for this system with the
//!   repository's stable Rust, and assemble the npm packages that ship them
//!   under `dir`: `cli/`, `@pane-app/cli`'s shim package, and
//!   `cli-<target>/`, the platform package whose `pane-ext` program the
//!   shim runs and whose componentizer, `runtime.wasm` and `libc.so` a
//!   package's build spawns — the spawn contract `PANE_COMPONENTIZER` names
//!   and the package's own `node_modules` holds. Each is ready for
//!   `npm pack`; the workflow packs them, a person publishes them, and CI
//!   never publishes (ADR 0047).
//! - `wasm-parts <dir>`: build the QuickJS runtime for `wasm32-wasip3` with
//!   the pinned nightly Rust and wasi-sdk, copy the SDK's WASI 0.3
//!   `libc.so`, and record both with their digests in `wasm-parts.json`.
//! - `check-parts <dir>`: compare the committed wasm parts
//!   (`tools/componentize-js/wasm-parts`, which the repository's builds
//!   embed and the platform packages ship) with a fresh build in `dir`, so
//!   those cannot drift from the vendored source they were built from.
//!
//! `check` (run by `cargo xtask sdks`) verifies the packages' committed
//! files against each other: every platform template matches the target
//! ids pane-build looks for, and the shim names them all.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

/// The componentize-js toolchain's folder.
const COMPONENTIZE_JS: &str = "tools/componentize-js";
/// The committed wasm parts, which the repository's builds embed and the
/// platform packages ship.
const WASM_PARTS: &str = "tools/componentize-js/wasm-parts";
/// The npm packages this module assembles: the shim package and the six
/// platform package templates, one folder per `pane_target` id.
const CLI: &str = "packages/cli";

/// npm's spellings of the six systems Pane ships for, as the shim's target
/// table keys them.
const NPM_SYSTEMS: [&str; 6] = [
    "darwin-arm64",
    "darwin-x64",
    "linux-arm64",
    "linux-x64",
    "win32-arm64",
    "win32-x64",
];

/// `cargo xtask wasm-parts <dir>`: the QuickJS runtime built for
/// `wasm32-wasip3` with the pinned nightly Rust and wasi-sdk, wasi-sdk's
/// WASI 0.3 `libc.so`, and what built them in `wasm-parts.json`.
pub(crate) fn wasm_parts(into: &Path) -> Result<(), String> {
    let root = crate::root();
    std::fs::create_dir_all(into)
        .map_err(|error| format!("{} cannot be created: {error}", into.display()))?;
    let sdk = ensure_sdk(&root)?;
    let runtime = into.join("runtime.wasm");
    let libc = into.join("libc.so");
    build_runtime(&root, &sdk, &runtime)?;
    std::fs::copy(
        sdk.join("share/wasi-sysroot/lib/wasm32-wasip3/libc.so"),
        &libc,
    )
    .map_err(|error| format!("copy the SDK's libc.so failed: {error}"))?;
    let runtime_sha256 = sha256_file(&runtime)?;
    let libc_sha256 = sha256_file(&libc)?;
    let pins = pins(&root);
    let patches = patch_digests(&root)?;
    let rustc = nightly_rustc(&root)?;
    let record = json!({
        "componentize_qjs": pins["componentize_qjs"],
        "patches": patches,
        "runtime_rustc": rustc,
        "wasi_sdk": pins["wasi_sdk"]["version"],
        "runtime_sha256": runtime_sha256,
        "libc_sha256": libc_sha256,
    });
    write_json(&into.join("wasm-parts.json"), &record)?;
    println!("xtask: runtime.wasm {runtime_sha256}, libc.so {libc_sha256}");
    Ok(())
}

/// `cargo xtask check-parts <dir>`: the committed wasm parts against the
/// fresh build in `dir` — their bytes and the record of what built them,
/// so the committed files cannot drift from the vendored source they
/// should have been built from.
pub(crate) fn check_parts(fresh: &Path) -> Result<(), String> {
    let committed = crate::root().join(WASM_PARTS);
    let different: Vec<&str> = ["runtime.wasm", "libc.so", "wasm-parts.json"]
        .into_iter()
        .filter(|name| sha256_file(&fresh.join(name)) != sha256_file(&committed.join(name)))
        .collect();
    if !different.is_empty() {
        return Err(format!(
            "the committed wasm parts ({}) do not match the fresh build: {} differ. Rebuild \
             them with `cargo xtask wasm-parts {}`, commit them, and rebuild the samples \
             with `cargo xtask js-guests`.",
            committed.display(),
            different.join(", "),
            committed.display(),
        ));
    }
    println!("xtask: the committed wasm parts match the fresh build");
    Ok(())
}

/// `cargo xtask cli-package <dir>`: `pane-ext` and the componentizer built
/// for this system, assembled with the committed wasm parts into the npm
/// packages that ship them — `cli/`, `@pane-app/cli`'s shim package, and
/// `cli-<target>/`, the platform package — each ready for `npm pack`. The
/// platform package also holds `toolchain.json`, the record of what it was
/// built from, which its `files` list keeps out of the tarball.
pub(crate) fn cli_package(out: &Path) -> Result<(), String> {
    let root = crate::root();
    // The parts the packages ship are the ones the repository embeds, and
    // this keeps their provenance honest before anything is built from
    // them: they are the ones their record names, built from the pins of
    // record.
    check_committed_parts(&root)?;
    let target = pane_target::Target::current()
        .ok_or("Pane names no target for this system, so it has no platform package")?;
    let exe = target.exe_suffix();
    // The programs, in release: the CLI the shim runs, and the
    // componentizer a package's build spawns.
    crate::run(crate::cargo().current_dir(&root).args([
        "build",
        "--release",
        "--locked",
        "-p",
        "pane-ext",
    ]))?;
    crate::run(crate::cargo().current_dir(&root).args([
        "build",
        "--release",
        "--locked",
        "-p",
        "componentize-qjs",
        "--example",
        "p3_build",
    ]))?;
    // Where cargo built them: `CARGO_TARGET_DIR`, else
    // `CARGO_BUILD_TARGET_DIR`, else `target`.
    let dir = ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"]
        .into_iter()
        .find_map(std::env::var_os)
        .map(|dir| root.join(dir))
        .unwrap_or_else(|| root.join("target"));
    let pane_ext = dir.join("release").join(format!("pane-ext{exe}"));
    let componentizer = dir
        .join("release")
        .join("examples")
        .join(format!("p3_build{exe}"));
    for (what, path) in [("pane-ext", &pane_ext), ("the componentizer", &componentizer)] {
        if !path.is_file() {
            return Err(format!("cargo built no {what} at {}", path.display()));
        }
    }
    // The program the shim runs starts, and says its version.
    let version = capture(Command::new(&pane_ext).arg("--version"))?;

    // The shim package: its own files, complete — the package.json, the
    // README, the licence and the shim (the platform templates beside them
    // in the repository are not part of it).
    let cli = out.join("cli");
    let _ = std::fs::remove_dir_all(&cli);
    for file in ["package.json", "README.md", "LICENSE-GPL"] {
        copy_file(&root.join(CLI).join(file), &cli.join(file))?;
    }
    copy_dir(&root.join(CLI).join("bin"), &cli.join("bin"))?;

    // The platform package: its committed template (package.json, README
    // and the licences of what it carries), then the built programs, the
    // committed wasm parts, and the record of what it was built from.
    let platform = out.join(format!("cli-{}", target.id()));
    let _ = std::fs::remove_dir_all(&platform);
    copy_dir(&root.join(CLI).join(target.id()), &platform)?;
    copy_file(&pane_ext, &platform.join(format!("pane-ext{exe}")))?;
    copy_file(
        &componentizer,
        &platform.join(format!("componentize-qjs-p3{exe}")),
    )?;
    for part in ["runtime.wasm", "libc.so"] {
        copy_file(&root.join(WASM_PARTS).join(part), &platform.join(part))?;
    }
    let (arch, system) = host()?;
    let rustc = stable_rustc(&root)?;
    let mut record = read_json(&root.join(WASM_PARTS).join("wasm-parts.json"))?;
    record["componentizer_rustc"] = json!(rustc);
    record["built_on"] = json!(format!("{arch}-{system}"));
    record["pane_ext"] = json!(version);
    write_json(&platform.join("toolchain.json"), &record)?;
    println!("xtask: {} and {} are ready for npm pack", cli.display(), platform.display());
    Ok(())
}

/// Checks that the CLI's npm packages match each other, so they cannot
/// drift apart: `@pane-app/cli`'s `optionalDependencies` name the six
/// platform packages at its own version, each template matches its target
/// (its name, `os` and `cpu`, and the four files `cargo xtask cli-package`
/// puts beside them), and the shim's target table names those target ids
/// and nothing else — the ids pane-build looks for in a package's own
/// `node_modules`.
pub(crate) fn check(root: &Path) -> Result<(), String> {
    let cli = read_json(&root.join(CLI).join("package.json"))?;
    let Some(version) = cli["version"].as_str() else {
        return Err("@pane-app/cli's package.json names no version".into());
    };
    let mut problems = Vec::new();
    if cli["name"] != "@pane-app/cli" {
        problems.push(format!("{}'s package.json is {}, not @pane-app/cli", CLI, name_of(&cli)));
    }
    if cli["bin"]["pane-ext"] != "bin/pane-ext.js" {
        problems.push(format!("{CLI}'s package.json does not run bin/pane-ext.js as pane-ext"));
    }
    if cli["dependencies"]["esbuild"].is_null() {
        problems.push(format!("{CLI}'s package.json does not depend on esbuild"));
    }
    // The six platform packages, one per target Pane ships for, at the
    // shim package's own version.
    let mut targets: Vec<pane_target::Target> = pane_target::Platform::ALL
        .into_iter()
        .flat_map(|os| {
            pane_target::Arch::ALL
                .into_iter()
                .map(move |arch| pane_target::Target { os, arch })
        })
        .collect();
    targets.sort();
    // The optional dependencies, as a map to remove each expected one
    // from: whatever is left names no platform package.
    let mut optional = match &cli["optionalDependencies"] {
        Value::Object(optional) => optional.clone(),
        _ => serde_json::Map::new(),
    };
    for &target in &targets {
        let name = format!("@pane-app/cli-{}", target.id());
        match optional.remove(&name) {
            Some(at) if at.as_str() == Some(version) => {}
            Some(_) => {
                problems.push(format!("{name} is not at @pane-app/cli's version {version}"))
            }
            None => problems.push(format!("@pane-app/cli does not list {name}")),
        }
        problems.extend(platform_problems(root, target, version));
    }
    for (left, _) in optional {
        problems.push(format!("@pane-app/cli lists {left}, which is no platform package"));
    }
    problems.extend(shim_problems(root, &targets));
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    println!("xtask: @pane-app/cli and its platform packages match each other");
    Ok(())
}

/// The problems with the platform package template of `target`: its name,
/// version, `os` and `cpu` (npm's spellings, which make it skip the package
/// on other systems), its `files` list, and its README.
fn platform_problems(root: &Path, target: pane_target::Target, version: &str) -> Vec<String> {
    let id = target.id();
    let folder = root.join(CLI).join(&id);
    let Ok(package) = read_json(&folder.join("package.json")) else {
        return vec![format!(
            "{CLI}/{id} is not a platform package template: its package.json is missing"
        )];
    };
    let mut problems = Vec::new();
    let name = format!("@pane-app/cli-{id}");
    if package["name"].as_str() != Some(name.as_str()) {
        problems.push(format!("{CLI}/{id}/package.json is {}, not {name}", name_of(&package)));
    }
    if package["version"].as_str() != Some(version) {
        problems.push(format!("{CLI}/{id}/package.json is not at @pane-app/cli's version {version}"));
    }
    let (os, cpu) = (npm_os(target.os), cpu_of(target.arch));
    if package["os"] != json!([os]) {
        problems.push(format!("{CLI}/{id}/package.json does not name its os as {os}"));
    }
    if package["cpu"] != json!([cpu]) {
        problems.push(format!("{CLI}/{id}/package.json does not name its cpu as {cpu}"));
    }
    // The four files `cargo xtask cli-package` puts in the folder: the two
    // programs (a Windows program ends in .exe) and the wasm parts.
    let exe = target.exe_suffix();
    if package["files"] != json!([
        format!("pane-ext{exe}"),
        format!("componentize-qjs-p3{exe}"),
        "runtime.wasm",
        "libc.so",
    ]) {
        problems.push(format!(
            "{CLI}/{id}/package.json does not list the pane-ext{exe}, \
             componentize-qjs-p3{exe}, runtime.wasm and libc.so the package carries"
        ));
    }
    if !folder.join("README.md").is_file() {
        problems.push(format!("{CLI}/{id} has no README.md"));
    }
    problems
}

/// npm's spelling of `platform`.
fn npm_os(platform: pane_target::Platform) -> &'static str {
    match platform {
        pane_target::Platform::Windows => "win32",
        pane_target::Platform::Macos => "darwin",
        pane_target::Platform::Linux => "linux",
    }
}

/// npm's spelling of `arch`.
fn cpu_of(arch: pane_target::Arch) -> &'static str {
    match arch {
        pane_target::Arch::X86_64 => "x64",
        pane_target::Arch::Aarch64 => "arm64",
    }
}

/// The problems with the shim's target table: it maps exactly the six
/// systems Pane ships for, by npm's platform and arch spellings
/// ([`NPM_SYSTEMS`]), to the target ids pane-build looks for.
fn shim_problems(root: &Path, targets: &[pane_target::Target]) -> Vec<String> {
    let Ok(shim) = std::fs::read_to_string(root.join(CLI).join("bin/pane-ext.js")) else {
        return vec![format!("{CLI}/bin/pane-ext.js cannot be read")];
    };
    // The table's lines: `"<platform>-<arch>": "<target>",`.
    let mut keys: Vec<&str> = Vec::new();
    let mut values: Vec<&str> = Vec::new();
    for line in shim.lines() {
        let line = line.trim();
        if !(line.starts_with('"') && line.ends_with(',')) {
            continue;
        }
        let Some((key, value)) = line.trim_end_matches(',').split_once("\": \"") else {
            continue;
        };
        keys.push(key.trim_start_matches('"'));
        values.push(value.trim_end_matches('"'));
    }
    let mut problems = Vec::new();
    keys.sort();
    if keys != NPM_SYSTEMS {
        problems.push(format!("{CLI}/bin/pane-ext.js maps {keys:?}, not the six systems Pane ships for"));
    }
    let mut expected_values: Vec<String> = targets.iter().map(|target| target.id()).collect();
    expected_values.sort();
    values.sort();
    if values != expected_values {
        problems.push(format!(
            "{CLI}/bin/pane-ext.js names {values:?}, not the six targets \
             pane-build looks for ({expected_values:?})"
        ));
    }
    problems
}

/// The wasi-sdk the runtime builds against: downloaded (with its digest
/// verified) when the cache does not hold it. Its folder, which the cache
/// keeps extracted.
fn ensure_sdk(root: &Path) -> Result<PathBuf, String> {
    let (arch, system) = host()?;
    let pins = pins(root);
    let pinned = &pins["wasi_sdk"];
    let version = pinned["version"].as_str().expect("pins.json names a version");
    let name = format!("wasi-sdk-{version}-{arch}-{system}");
    let cache = cache_root();
    let sdk = cache.join(&name);
    let libc = sdk.join("share/wasi-sysroot/lib/wasm32-wasip3/libc.so");
    if libc.is_file() {
        return Ok(sdk);
    }
    let digest = pinned["archive_sha256"][format!("{arch}-{system}")]
        .as_str()
        .ok_or_else(|| format!("pins.json names no wasi-sdk digest for {arch}-{system}"))?
        .to_owned();
    let release = pinned["release"].as_str().expect("pins.json names a release");
    let archive = cache.join("downloads").join(format!("{name}.tar.gz"));
    let url = format!(
        "https://github.com/WebAssembly/wasi-sdk/releases/download/{release}/{name}.tar.gz"
    );
    download(&url, &archive, &digest)?;
    // The archive holds one top-level folder, the SDK itself.
    let file = std::fs::File::open(&archive)
        .map_err(|error| format!("{} cannot be read: {error}", archive.display()))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(gz);
    tar.unpack(&cache)
        .map_err(|error| format!("the {name} archive cannot be extracted: {error}"))?;
    if !libc.is_file() {
        return Err(format!(
            "the {name} archive holds no {}",
            libc.file_name().unwrap_or_default().to_string_lossy()
        ));
    }
    Ok(sdk)
}

/// Builds the QuickJS runtime for `wasm32-wasip3` with the pinned nightly
/// Rust and the SDK's compiler, from a cache copy of the vendored runtime
/// crate (so its build artifacts stay out of the repository; the crate is
/// excluded from the workspace). The crate carries the `Cargo.lock` the
/// first build resolved, committed in the vendored tree, and every build
/// after that runs `--locked` against it, so the same source, lock and
/// toolchain give the same bytes: that is what `check-parts` compares.
fn build_runtime(root: &Path, sdk: &Path, out: &Path) -> Result<(), String> {
    let nightly = nightly_of(root);
    let rustup = which("rustup")?;
    crate::run(
        Command::new(&rustup)
            .arg("toolchain")
            .arg("install")
            .arg(&nightly)
            .arg("--profile")
            .arg("minimal")
            .arg("--component")
            .arg("rust-src"),
    )?;
    let cache = cache_root();
    let scratch = cache.join("src/runtime");
    let _ = std::fs::remove_dir_all(&scratch);
    copy_dir(
        &root.join(COMPONENTIZE_JS)
            .join("componentize-qjs/crates/runtime"),
        &scratch,
    )?;
    let clang = sdk.join("bin").join(format!("clang{}", exe_suffix()));
    let target_dir = cache.join("runtime-target");
    let mut build = Command::new(&rustup);
    build
        .arg("run")
        .arg(&nightly)
        .arg("cargo")
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-wasip3")
        .arg("-Zbuild-std=std,panic_abort")
        .arg("--manifest-path")
        .arg(scratch.join("Cargo.toml"));
    if scratch.join("Cargo.lock").is_file() {
        build.arg("--locked");
    }
    // The caller's environment without what `cargo xtask` injected for the
    // repository's own builds, so the pinned nightly and the SDK's linker
    // settings are the only words on how the runtime is built.
    without_cargo_environment(&mut build);
    build.env("CARGO_TARGET_DIR", &target_dir);
    build.env("PATH", path_with(&sdk.join("bin")));
    build.env("CARGO_TARGET_WASM32_WASIP3_LINKER", &clang);
    // Encoded (unit-separator-separated) so a cache path containing spaces
    // stays one argument. With `--target`, these reach only the Wasm target.
    build.env("CARGO_ENCODED_RUSTFLAGS", rustflags(sdk));
    build.env("WASI_SDK", sdk);
    build.env("WASI_SDK_PATH", sdk);
    // rquickjs' bindgen loads the SDK's libclang (bin/ on Windows, lib/
    // elsewhere).
    let libclang = if cfg!(windows) { "bin" } else { "lib" };
    build.env("LIBCLANG_PATH", sdk.join(libclang));
    build.env("CC_wasm32_wasip3", &clang);
    build.env(
        "AR_wasm32_wasip3",
        sdk.join("bin").join(format!("llvm-ar{}", exe_suffix())),
    );
    // rquickjs-sys would pass the sysroot through CFLAGS, which cc splits
    // on whitespace, so a cache path with spaces breaks it. The SDK's clang
    // finds its own sysroot; bindgen gets it quoted instead.
    build.env("RQUICKJS_SYS_NO_WASI_SDK", "1");
    let sysroot = format!("--sysroot={}", sdk.join("share/wasi-sysroot").display());
    build.env("BINDGEN_EXTRA_CLANG_ARGS_wasm32_wasip3", quoted(&sysroot));
    build.env("CFLAGS_wasm32_wasip3", "--target=wasm32-wasip3 -fPIC -Oz");
    crate::run(&mut build)?;
    let built = target_dir.join("wasm32-wasip3/release/componentize_qjs_runtime.wasm");
    std::fs::copy(&built, out)
        .map_err(|error| format!("copy {} failed: {error}", built.display()))?;
    Ok(())
}

/// The rustflags the runtime builds with: position-independent code linked
/// as a shared module with no entry, exporting the TLS info the runtime's
/// host reads, against the SDK's libraries — unit-separator-encoded as
/// `CARGO_ENCODED_RUSTFLAGS` takes them.
fn rustflags(sdk: &Path) -> String {
    let libraries = sdk.join("share/wasi-sysroot/lib/wasm32-wasip3");
    [
        "-Crelocation-model=pic".to_owned(),
        "-Clink-arg=--target=wasm32-wasip3".to_owned(),
        "-Clink-arg=-shared".to_owned(),
        "-Clink-arg=-Wl,--no-entry".to_owned(),
        "-Clink-arg=-Wl,--allow-undefined".to_owned(),
        "-Clink-arg=-Wl,--export=__wasm_library_tls_info".to_owned(),
        "-L".to_owned(),
        format!("native={}", libraries.display()),
    ]
    .join("\u{1f}")
}

/// This system as wasi-sdk's release assets spell it: (arch, system).
fn host() -> Result<(String, String), String> {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "arm64",
        other => return Err(format!("no wasi-sdk build for {other} processors")),
    };
    let system = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "macos",
        "windows" => "windows",
        other => return Err(format!("no wasi-sdk build for {other}")),
    };
    Ok((arch.to_owned(), system.to_owned()))
}

/// Where the toolchain's downloads and scratch builds live:
/// `PANE_COMPONENTIZE_CACHE`, else `pane/componentize-js` in the user's
/// cache folder.
fn cache_root() -> PathBuf {
    if let Some(configured) = std::env::var_os("PANE_COMPONENTIZE_CACHE")
        .filter(|value| !value.is_empty())
    {
        return PathBuf::from(configured);
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home.map(|home| PathBuf::from(home).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|home| PathBuf::from(home).join(".cache")))
    };
    base.unwrap_or_else(std::env::temp_dir)
        .join("pane")
        .join("componentize-js")
}

/// Removes from `command`'s environment what `cargo xtask` injected for
/// the repository's own builds: every `CARGO_*` but `CARGO_HOME`, and the
/// toolchain pointers, so the toolchain this task builds with is the one it
/// asks for, not the ambient one.
fn without_cargo_environment(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        let Some(key) = key.to_str() else { continue };
        let cargo_setting = key.starts_with("CARGO_") && key != "CARGO_HOME";
        let toolchain = matches!(
            key,
            "CARGO" | "RUSTUP_TOOLCHAIN" | "RUSTC" | "RUSTDOC" | "RUSTFLAGS"
        );
        if cargo_setting || toolchain {
            command.env_remove(key);
        }
    }
}

/// The search path with `folder` in front: the SDK's tools first.
fn path_with(folder: &Path) -> std::ffi::OsString {
    let existing = std::env::var_os("PATH").unwrap_or_default();
    let with_folder =
        std::iter::once(folder.to_path_buf()).chain(std::env::split_paths(&existing));
    std::env::join_paths(with_folder).unwrap_or(existing)
}

/// `text` quoted the way clang takes an argument: in single quotes when it
/// could otherwise split, so a cache path with spaces stays one argument.
fn quoted(text: &str) -> String {
    if text.chars().any(char::is_whitespace) {
        format!("'{text}'")
    } else {
        text.to_owned()
    }
}

/// Downloads `url` to `path` with curl (the one tool every system the
/// workflow runs on has), verifying its sha256: the digest is this
/// repository's own check, the transport curl's.
fn download(url: &str, path: &Path, expected: &str) -> Result<(), String> {
    if path.is_file() && sha256_file(path).as_deref() == Ok(expected) {
        return Ok(());
    }
    println!("xtask: downloading {url}");
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("{} cannot be created: {error}", folder.display()))?;
    }
    let partial = path.with_extension("part");
    crate::run(
        Command::new("curl")
            .args(["--fail", "--location", "--silent", "--show-error", "--output"])
            .arg(&partial)
            .arg(url),
    )?;
    std::fs::rename(&partial, path)
        .map_err(|error| format!("{} cannot be written: {error}", path.display()))?;
    let actual = sha256_file(path)?;
    if actual != expected {
        return Err(format!(
            "{} has sha256 {actual}, expected {expected}",
            path.display()
        ));
    }
    Ok(())
}

/// The pinned nightly, which builds the runtime.
fn nightly_of(root: &Path) -> String {
    pins(root)["rust_nightly"]
        .as_str()
        .expect("pins.json names the nightly")
        .to_owned()
}

/// The nightly's `rustc -V`, for the record of what built the runtime.
fn nightly_rustc(root: &Path) -> Result<String, String> {
    let nightly = nightly_of(root);
    capture(
        Command::new(which("rustup")?)
            .arg("run")
            .arg(&nightly)
            .arg("rustc")
            .arg("-V"),
    )
}

/// The repository's pinned stable toolchain, which builds the programs the
/// packages ship: read from `rust-toolchain.toml`.
fn stable_rustc(root: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(root.join("rust-toolchain.toml"))
        .map_err(|error| format!("rust-toolchain.toml cannot be read: {error}"))?;
    let channel = text
        .split("channel = \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next());
    let Some(channel) = channel else {
        return Err("rust-toolchain.toml names no channel".into());
    };
    capture(
        Command::new(which("rustup")?)
            .arg("run")
            .arg(channel)
            .arg("rustc")
            .arg("-V"),
    )
}

/// The committed wasm parts are the ones their record names, built from
/// the pins of record.
fn check_committed_parts(root: &Path) -> Result<(), String> {
    let record = read_json(&root.join(WASM_PARTS).join("wasm-parts.json"))?;
    for (file, key) in [("runtime.wasm", "runtime_sha256"), ("libc.so", "libc_sha256")] {
        let actual = sha256_file(&root.join(WASM_PARTS).join(file))?;
        if record[key].as_str() != Some(actual.as_str()) {
            return Err(format!(
                "the committed {file} does not match its digest in wasm-parts.json"
            ));
        }
    }
    let pins = pins(root);
    if record["componentize_qjs"]["commit"] != pins["componentize_qjs"]["commit"]
        || record["patches"] != patch_digests(root)?
    {
        return Err(
            "the committed wasm parts were built from other pins or patches than pins.json \
             names"
                .into(),
        );
    }
    Ok(())
}

/// pins.json, the toolchain's pins of record.
fn pins(root: &Path) -> Value {
    read_json(&root.join(COMPONENTIZE_JS).join("pins.json"))
        .expect("tools/componentize-js/pins.json is committed")
}

/// The digests of the patch queue, which the vendored tree carries applied.
fn patch_digests(root: &Path) -> Result<Value, String> {
    let pins = pins(root);
    let mut patches = serde_json::Map::new();
    for patch in pins["patches"].as_array().expect("pins.json names the patches") {
        let name = patch.as_str().expect("a patch name");
        let digest = sha256_file(&root.join(COMPONENTIZE_JS).join("patches").join(name))?;
        patches.insert(name.to_owned(), json!(digest));
    }
    Ok(Value::Object(patches))
}

/// A JSON file, read.
fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))
}

/// A JSON document, written pretty with a trailing newline.
fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| format!("{} cannot be written: {error}", path.display()))?;
    std::fs::write(path, text + "\n")
        .map_err(|error| format!("{} cannot be written: {error}", path.display()))
}

/// What `command` prints on standard output, trimmed.
fn capture(command: &mut Command) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|error| format!("failed to start {command:?}: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(format!("{command:?} failed with {}", output.status))
    }
}

/// The name a package.json reads, when it reads one.
fn name_of(package: &Value) -> String {
    package["name"].as_str().unwrap_or("none").to_owned()
}

/// A program on the search path.
fn which(name: &str) -> Result<PathBuf, String> {
    let path = std::env::var_os("PATH").ok_or("no search path to find programs on")?;
    let program = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    std::env::split_paths(&path)
        .map(|dir| dir.join(&program))
        .find(|program| program.is_file())
        .ok_or_else(|| format!("`{name}` was not found on PATH"))
}

/// The executable suffix this system's programs carry.
fn exe_suffix() -> &'static str {
    std::env::consts::EXE_SUFFIX
}

/// The sha256 of a file, as lowercase hex.
fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)
        .map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
    let digest = Sha256::digest(&bytes);
    Ok(hex(&digest))
}

/// A digest as lowercase hex.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Copies one file, its folder made if it is not there.
fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(folder) = to.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("{} cannot be created: {error}", folder.display()))?;
    }
    std::fs::copy(from, to)
        .map_err(|error| format!("copy {} failed: {error}", from.display()))
        .map(|_| ())
}

/// Copies the folder `from` to `to` (which must not exist), recursively.
fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to)
        .map_err(|error| format!("{} cannot be created: {error}", to.display()))?;
    for entry in std::fs::read_dir(from)
        .map_err(|error| format!("{} cannot be read: {error}", from.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            copy_dir(&path, &to.join(entry.file_name()))?;
        } else {
            std::fs::copy(&path, to.join(entry.file_name()))
                .map_err(|error| format!("copy {} failed: {error}", path.display()))?;
        }
    }
    Ok(())
}
