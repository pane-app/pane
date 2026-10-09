//! Development mode with Pane's real builds, on copies of the development
//! samples (`guests/hello-rust`, `guests/hello-js`, `guests/hello-ts`):
//! saving an edit builds the package with its documented command and
//! reloads it, a save that does not build keeps the working code and shows
//! the compiler's diagnostics, and fixing it reloads it again.
//!
//! The Rust test runs `cargo build --release --target wasm32-wasip2` (the
//! pinned toolchain and its `wasm32-wasip2` target, as `cargo xtask guests`
//! needs). The JavaScript and TypeScript tests run pane-build's own build
//! with the componentizer linked in: Node.js and npm alone build them
//! (`npm ci` of the package's locked dependencies, its `tsc`, esbuild,
//! componentization), with no Python and no toolchain (#218).
//!
//! Each copy is kept in Cargo's test folder between runs, so later runs
//! build incrementally; only its sources are replaced.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::develop::{BuildJob, BuildOutcome, Builder, Toolchains};
use pane_core::{Launcher, PackageIdentity, Runtime, Status};

#[path = "support/feedback.rs"]
mod feedback;

use feedback::shown;

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn toolchains() -> Toolchains {
    // The componentizer linked in, as pane-core's dev-dependencies build with
    // (#218): Node.js and npm alone build a JavaScript or TypeScript package.
    Toolchains::from_env(None)
}

/// A development sample: its folder in `guests`, its title, the file
/// holding its greeting and that greeting's declaration.
struct Sample {
    name: &'static str,
    title: &'static str,
    files: &'static [&'static str],
    source: &'static str,
    greeting: &'static str,
}

impl Sample {
    /// A fresh copy of the sample's sources in Cargo's test folder, keeping
    /// what earlier runs built there; for Rust, with the path to
    /// `pane-extension` and the repository's toolchain file.
    fn copy(&self) -> PathBuf {
        self.copy_as(&format!("develop-{}", self.name))
    }

    /// Like [`Sample::copy`], in the test folder's `name`.
    fn copy_as(&self, name: &str) -> PathBuf {
        let from = repository().join("guests").join(self.name);
        let to = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
        let _ = fs::remove_dir_all(to.join("src"));
        fs::create_dir_all(to.join("src")).unwrap();
        for file in self.files {
            fs::copy(from.join(file), to.join(file)).unwrap();
        }
        if to.join("Cargo.toml").exists() {
            let guest = repository().join("guests/pane-extension");
            let manifest = fs::read_to_string(to.join("Cargo.toml")).unwrap().replace(
                r#"path = "../pane-extension""#,
                &format!("path = {:?}", guest.to_str().unwrap()),
            );
            fs::write(to.join("Cargo.toml"), manifest).unwrap();
            fs::copy(
                repository().join("rust-toolchain.toml"),
                to.join("rust-toolchain.toml"),
            )
            .unwrap();
        }
        to.canonicalize().unwrap()
    }

    /// Saves the sample's source with its greeting replaced by `line`.
    fn save(&self, folder: &Path, line: &str) {
        let original = fs::read_to_string(
            repository()
                .join("guests")
                .join(self.name)
                .join(self.source),
        )
        .unwrap();
        assert!(original.contains(self.greeting), "{}", self.greeting);
        fs::write(
            folder.join(self.source),
            original.replace(self.greeting, line),
        )
        .unwrap();
    }
}

const RUST: Sample = Sample {
    name: "hello-rust",
    title: "Hello Rust",
    files: &["Cargo.toml", "Cargo.lock", "pane.json", "src/lib.rs"],
    source: "src/lib.rs",
    greeting: r#"const GREETING: &str = "Hello from Rust";"#,
};

const JAVASCRIPT: Sample = Sample {
    name: "hello-js",
    title: "Hello JavaScript",
    files: &[
        "package.json",
        "package-lock.json",
        "tsconfig.json",
        "pane.json",
        "src/index.js",
    ],
    source: "src/index.js",
    greeting: r#"const GREETING = "Hello from JavaScript";"#,
};

const TYPESCRIPT: Sample = Sample {
    name: "hello-ts",
    title: "Hello TypeScript",
    files: &[
        "package.json",
        "package-lock.json",
        "tsconfig.json",
        "pane.json",
        "src/index.ts",
    ],
    source: "src/index.ts",
    greeting: r#"const GREETING: string = "Hello from TypeScript";"#,
};

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(600);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Builds the package in `folder` as development mode does, then puts its
/// components in the folder, as its author would before installing it.
fn build_once(folder: &Path) {
    let staged = build_with(&toolchains(), folder);
    for command in pane_core::Manifest::read(staged.path()).unwrap().commands {
        let component = command.component;
        let target = folder.join(&component);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        let _ = fs::remove_file(&target);
        fs::copy(staged.path().join(&component), target).unwrap();
    }
}

/// Builds the package in `folder` with `toolchains` once, as development mode
/// does, returning its staging folder, which holds the built components.
fn build_with(toolchains: &Toolchains, folder: &Path) -> tempfile::TempDir {
    let build = toolchains.build_for(folder).unwrap();
    let staging = tempfile::tempdir().unwrap();
    fs::copy(folder.join("pane.json"), staging.path().join("pane.json")).unwrap();
    let job = BuildJob::new(staging.path().to_path_buf());
    let outcome = build.run(&job);
    assert_eq!(outcome, BuildOutcome::Built, "{:#?}", job.output());
    staging
}

/// A launcher that develops with Pane's builds, keeping its data in `data`.
fn launcher(data: &Path) -> Launcher {
    let (changes, _) = pane_core::changes::channel();
    Launcher::with_packages(
        Ok(Runtime::start().unwrap()),
        vec![],
        data.join("extensions"),
    )
    .with_development(Arc::new(toolchains()), changes)
}

/// Opens the sample's command from root search and runs "Say hello",
/// returning what it showed: its toast, or the status line.
fn say_hello(launcher: &Launcher, title: &str) -> Status {
    for _ in 0..3 {
        launcher.back();
    }
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.title == title)
        .unwrap();
    launcher.select(index);
    block_on(launcher.activate_selected());
    let index = launcher
        .view()
        .rows
        .iter()
        .position(|row| row.title == "Say hello")
        .unwrap();
    launcher.select(index);
    block_on(launcher.activate_selected());
    shown(launcher)
}

/// Builds the sample once, installs it, develops it, and saves a change,
/// an edit that does not build, and a fix.
fn develop(sample: &Sample, greeting: &str, broken: &str, again: &str, fixed: &str) {
    let folder = sample.copy();
    sample.save(&folder, sample.greeting);
    build_once(&folder);

    let data = tempfile::tempdir().unwrap();
    let launcher = launcher(data.path());
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    block_on(launcher.start_developing(&identity));
    assert_eq!(
        say_hello(&launcher, sample.title),
        Status::Result(greeting.into())
    );
    let handled = |count: u64| {
        wait_until(&format!("build {count}"), || {
            launcher
                .development(&identity)
                .is_some_and(|development| development.finished >= count)
        });
        // Its status is shown at root search.
        for _ in 0..3 {
            launcher.back();
        }
    };

    sample.save(&folder, &again.replace("{}", "Hello again"));
    handled(1);
    assert_eq!(
        launcher.view().status,
        Status::Result(format!("Reloaded {}", sample.title))
    );
    assert_eq!(
        say_hello(&launcher, sample.title),
        Status::Result("Hello again".into())
    );

    sample.save(&folder, broken);
    handled(2);
    let Status::Error(message) = launcher.view().status else {
        panic!("{:?}", launcher.view().status);
    };
    assert!(
        message.starts_with(&format!("{} did not build: ", sample.title)),
        "{message}"
    );
    assert!(message.contains("error"), "{message}");
    let failure = launcher.development(&identity).unwrap().failure.unwrap();
    // The compiler's first error, its diagnostics, then which command
    // failed; all of it in the log.
    assert!(message.contains(&failure.summary), "{message}");
    let output = failure.output.join("\n");
    assert!(output.contains("error"), "{output}");
    assert!(output.contains("` failed (exit code "), "{output}");
    let log = fs::read_to_string(failure.log.as_ref().unwrap()).unwrap();
    assert!(log.contains(&failure.summary), "{log}");
    assert_eq!(
        say_hello(&launcher, sample.title),
        Status::Result("Hello again".into())
    );

    sample.save(&folder, fixed);
    handled(3);
    assert_eq!(
        launcher.view().status,
        Status::Result(format!("Reloaded {}", sample.title))
    );
    assert_eq!(launcher.development(&identity).unwrap().failure, None);
    assert_eq!(
        say_hello(&launcher, sample.title),
        Status::Result("Hello once more".into())
    );
}

#[test]
fn a_rust_package_is_built_with_cargo_and_reloaded_on_save() {
    develop(
        &RUST,
        "Hello from Rust",
        r#"const GREETING: &str = 42;"#,
        r#"const GREETING: &str = "{}";"#,
        r#"const GREETING: &str = "Hello once more";"#,
    );
}

#[test]
fn a_rust_package_built_elsewhere_reloads_what_cargo_built_this_time() {
    // Its own target folder, as a workspace member or `build.target-dir`
    // has: nothing is built where pane.json names the component.
    let folder = RUST.copy_as("develop-hello-rust-target-dir");
    fs::create_dir_all(folder.join(".cargo")).unwrap();
    fs::write(
        folder.join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"../develop-hello-rust-elsewhere\"\n",
    )
    .unwrap();
    RUST.save(&folder, RUST.greeting);
    build_once(&folder);
    let data = tempfile::tempdir().unwrap();
    let launcher = launcher(data.path());
    block_on(launcher.install_package(&folder));
    let identity = PackageIdentity::local(&folder).unwrap();
    block_on(launcher.start_developing(&identity));
    let finished = |count: u64| {
        wait_until(&format!("build {count}"), || {
            launcher
                .development(&identity)
                .is_some_and(|development| development.finished >= count)
        });
        // Its status is shown at root search.
        for _ in 0..3 {
            launcher.back();
        }
    };

    // The older file where pane.json points is not what is reloaded.
    RUST.save(&folder, r#"const GREETING: &str = "Hello from elsewhere";"#);
    finished(1);
    assert_eq!(
        say_hello(&launcher, RUST.title),
        Status::Result("Hello from elsewhere".into())
    );

    // A component cargo does not build is refused, even with a file there.
    let manifest = fs::read_to_string(folder.join("pane.json")).unwrap();
    let stale = "target/wasm32-wasip2/release/stale.wasm";
    fs::copy(
        folder.join("target/wasm32-wasip2/release/hello_rust.wasm"),
        folder.join(stale),
    )
    .unwrap();
    fs::write(
        folder.join("pane.json"),
        manifest.replace("target/wasm32-wasip2/release/hello_rust.wasm", stale),
    )
    .unwrap();
    finished(2);
    let Status::Error(message) = launcher.view().status else {
        panic!("{:?}", launcher.view().status);
    };
    assert!(
        message.starts_with("Hello Rust did not build: cargo built no stale.wasm this time"),
        "{message}"
    );
    assert_eq!(
        say_hello(&launcher, RUST.title),
        Status::Result("Hello from elsewhere".into())
    );
}

/// The `@pane-app/cli` platform package a package installs with `npm install`
/// (#219), staged into the package's `node_modules` from the componentizer
/// `cargo xtask guests` builds, with the committed wasm parts: the binary a
/// development build of an installed Pane spawns (no tool of this checkout
/// runs).
fn install_cli(folder: &Path) -> PathBuf {
    let built = repository().join("target/guests/componentizer");
    let target = pane_target::Target::current()
        .expect("Pane names this system's target")
        .id()
        .to_owned();
    let binary = if cfg!(windows) {
        built.join("componentize-qjs-p3.exe")
    } else {
        built.join("componentize-qjs-p3")
    };
    assert!(
        binary.is_file(),
        "{} is missing; run `cargo xtask guests`",
        binary.display()
    );
    let cli = folder.join(format!("node_modules/@pane-app/cli-{target}"));
    fs::create_dir_all(&cli).unwrap();
    let parts = repository().join("tools/componentize-js/wasm-parts");
    for part in [
        binary,
        parts.join("runtime.wasm"),
        parts.join("libc.so"),
    ] {
        fs::copy(&part, cli.join(part.file_name().unwrap())).unwrap();
    }
    cli
}

#[test]
fn a_typescript_package_with_pane_cli_installed_builds_with_its_componentizer() {
    // An installed Pane (no componentizer of its own linked in, none named)
    // builds a package that has Pane's CLI installed with the componentizer
    // the package itself holds — no tool of this checkout runs.
    let folder = TYPESCRIPT.copy_as("develop-hello-ts-cli");
    TYPESCRIPT.save(&folder, TYPESCRIPT.greeting);
    install_cli(&folder);
    let package_toolchains = Toolchains {
        componentizer: pane_core::develop::Componentizer::Binary(None),
        ..toolchains()
    };
    build_with(&package_toolchains, &folder);

    // A package without it is explained, as an installed Pane is for one
    // whose author has not run npm install.
    let plain = TYPESCRIPT.copy_as("develop-hello-ts-no-cli");
    let error = package_toolchains.build_for(&plain).unwrap_err();
    assert!(error.contains("npm install"), "{error}");
    assert!(error.contains("node_modules/@pane-app/cli-"), "{error}");
}

#[test]
fn a_javascript_package_is_built_and_reloaded_on_save() {
    develop(
        &JAVASCRIPT,
        "Hello from JavaScript",
        r#"const GREETING = 42;"#,
        r#"const GREETING = "{}";"#,
        r#"const GREETING = "Hello once more";"#,
    );
}

#[test]
fn a_typescript_package_is_built_and_reloaded_on_save() {
    develop(
        &TYPESCRIPT,
        "Hello from TypeScript",
        r#"const GREETING: string = 42;"#,
        r#"const GREETING: string = "{}";"#,
        r#"const GREETING: string = "Hello once more";"#,
    );
}
