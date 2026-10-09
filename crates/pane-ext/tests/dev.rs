//! `pane-ext dev` against a launcher that listens on an endpoint of the
//! test's own, as the running Pane does (#217). On a copy of the Rust
//! development sample (`guests/hello-rust`): the terminal shows the build,
//! Pane's install preview, which is confirmed, Pane's messages about the
//! package and what the package prints; a save that does not build prints
//! the compiler's errors there while Pane keeps running the working code; a
//! fix is reloaded; and closing `pane-ext`, as Ctrl+C does, stops the
//! development. With no Pane listening and none to start, `pane-ext dev`
//! says where it looked for one, without waiting for its build.
//!
//! The Rust builds run `cargo build --release --target wasm32-wasip2` (the
//! pinned toolchain and its `wasm32-wasip2` target, as `cargo xtask guests`
//! needs). The TypeScript one runs pane-build's JavaScript build with the
//! componentizer `pane-ext` links in (#218): `npm ci` of the package's
//! locked dependencies, its `tsc`, esbuild, componentization — Node.js and
//! npm alone. `PANE_APP` names a file that does not exist, so that
//! `pane-ext` never starts a Pane of its own here.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use pane_core::local_channel::{self, Endpoint, Previews};
use pane_core::{Launcher, PackageIdentity, Runtime, Screen, Status, ToastStyle};

/// How long a build or a step may take: the first build of the sample
/// builds its dependencies too.
const DEADLINE: Duration = Duration::from_secs(600);

/// The sample's greeting, and what its "Say hello" does.
const GREETING: &str = r#"const GREETING: &str = "Hello from Rust";"#;
const SAY_HELLO: &str = "show_toast(Toast::success(GREETING));";

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// A fresh copy of the sample's sources in Cargo's test folder's `name`,
/// keeping what earlier runs built there, with the path to `pane-extension` and
/// the repository's toolchain file, saved with `greeting`.
fn sample(name: &str, greeting: &str) -> PathBuf {
    let from = repository().join("guests/hello-rust");
    let to = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(to.join("src"));
    fs::create_dir_all(to.join("src")).unwrap();
    for file in ["Cargo.toml", "Cargo.lock", "pane.json"] {
        fs::copy(from.join(file), to.join(file)).unwrap();
    }
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
    save(&to, greeting);
    to.canonicalize().unwrap()
}

/// Saves the sample's source with its greeting declared as `greeting`, and
/// "Say hello" also writing a line to the extension log.
fn save(folder: &Path, greeting: &str) {
    let source = fs::read_to_string(repository().join("guests/hello-rust/src/lib.rs")).unwrap();
    assert!(source.contains(GREETING), "{GREETING}");
    assert!(source.contains(SAY_HELLO), "{SAY_HELLO}");
    let logged = format!("pane_extension::info!(\"saying hello\");\n            {SAY_HELLO}");
    let source = source
        .replace(GREETING, greeting)
        .replace(SAY_HELLO, &logged);
    fs::write(folder.join("src/lib.rs"), source).unwrap();
}

/// A fresh copy of the TypeScript development sample
/// (`guests/hello-ts`) in the test folder's `name`, saved with `greeting`:
/// what `pane-ext dev` builds with the componentizer it links, needing
/// Node.js and npm alone.
fn js_sample(name: &str, greeting: &str) -> PathBuf {
    let from = repository().join("guests/hello-ts");
    let to = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(to.join("src"));
    fs::create_dir_all(to.join("src")).unwrap();
    for file in [
        "package.json",
        "package-lock.json",
        "tsconfig.json",
        "pane.json",
        "src/index.ts",
    ] {
        fs::copy(from.join(file), to.join(file)).unwrap();
    }
    js_save(&to, greeting);
    to.canonicalize().unwrap()
}

/// Saves the TypeScript sample's source with its greeting declared as
/// `greeting`.
fn js_save(folder: &Path, greeting: &str) {
    let source = fs::read_to_string(repository().join("guests/hello-ts/src/index.ts")).unwrap();
    const GREETING: &str = r#"const GREETING: string = "Hello from TypeScript";"#;
    assert!(source.contains(GREETING), "{GREETING}");
    fs::write(
        folder.join("src/index.ts"),
        source.replace(GREETING, greeting),
    )
    .unwrap();
}

/// An endpoint of this test's own. On Unix its folder is left for Pane to
/// make, as only this user's: a temporary folder others can enter is refused.
fn endpoint(folder: &Path) -> Endpoint {
    if cfg!(windows) {
        let name = folder.file_name().unwrap().to_string_lossy();
        Endpoint::at(format!(r"\\.\pipe\pane-ext-dev-{name}"))
    } else {
        Endpoint::at(folder.join("pane").join("channel"))
    }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool, shown: impl Fn() -> String) {
    let deadline = Instant::now() + DEADLINE;
    while !done() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; pane-ext printed:\n{}",
            shown()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// What `pane-ext` printed so far, standard output and error together.
#[derive(Clone, Default)]
struct Terminal(Arc<Mutex<Vec<String>>>);

impl Terminal {
    fn read(&self, from: impl Read + Send + 'static) {
        let lines = self.0.clone();
        std::thread::spawn(move || {
            let mut from = BufReader::new(from);
            let mut line = Vec::new();
            while from.read_until(b'\n', &mut line).is_ok_and(|read| read > 0) {
                let text = String::from_utf8_lossy(&line);
                lines.lock().unwrap().push(text.trim_end().to_owned());
                line.clear();
            }
        });
    }

    fn text(&self) -> String {
        self.0.lock().unwrap().join("\n")
    }

    fn wait_for(&self, text: &str) {
        let what = format!("`{text}` in pane-ext's output");
        wait_until(&what, || self.text().contains(text), || self.text());
    }
}

/// `pane-ext dev` on `folder`, reaching Pane on `endpoint`; killed when
/// dropped.
struct PaneExt(Child);

impl PaneExt {
    fn dev(folder: &Path, endpoint: &Endpoint, data: &Path, terminal: &Terminal) -> PaneExt {
        let mut child = Command::new(env!("CARGO_BIN_EXE_pane-ext"))
            .arg("dev")
            .arg(folder)
            .env(local_channel::ENDPOINT_VARIABLE, endpoint.path())
            .env("PANE_APP", data.join("no-pane"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        terminal.read(child.stdout.take().unwrap());
        terminal.read(child.stderr.take().unwrap());
        PaneExt(child)
    }

    /// Ends it at once, as Ctrl+C does.
    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for PaneExt {
    fn drop(&mut self) {
        self.kill();
    }
}

/// The install previews shown: each one's title and details.
type Shown = Arc<Mutex<Vec<(String, Vec<String>)>>>;

/// Stands in for Pane's window: shows each install preview it is asked
/// for, records its title and details, and chooses Install.
fn window(launcher: Launcher, mut previews: Previews) -> Shown {
    let shown = Arc::new(Mutex::new(Vec::new()));
    let recorded = shown.clone();
    std::thread::spawn(move || {
        while let Some(folder) = block_on(previews.next()) {
            block_on(launcher.preview_package(&folder));
            let view = launcher.view();
            if let Screen::Package { details } = &view.screen {
                recorded
                    .lock()
                    .unwrap()
                    .push((view.title.clone(), details.clone()));
            }
            if let Some(index) = view.rows.iter().position(|row| row.title == "Install") {
                launcher.select(index);
                block_on(launcher.activate_selected());
            }
        }
    });
    shown
}

/// What the launcher shows of the last outcome: the status line, else the
/// toast in the footer.
fn shown(launcher: &Launcher) -> Status {
    let status = launcher.view().status;
    if status != Status::Idle {
        return status;
    }
    match launcher.toast() {
        None => Status::Idle,
        Some(shown) => match shown.toast.style {
            ToastStyle::Success => Status::Result(shown.toast.text()),
            ToastStyle::Failure => Status::Error(shown.toast.text()),
            ToastStyle::Animated => Status::Progress(shown.toast.text()),
        },
    }
}

fn choose(launcher: &Launcher, title: &str) {
    let rows = launcher.view().rows;
    let Some(index) = rows.iter().position(|row| row.title == title) else {
        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        panic!("no row {title}: {titles:?}");
    };
    launcher.select(index);
    block_on(launcher.activate_selected());
}

/// Opens Hello Rust from root search and runs "Say hello", returning what
/// it showed.
fn say_hello(launcher: &Launcher) -> Status {
    for _ in 0..3 {
        launcher.back();
    }
    choose(launcher, "Hello Rust");
    choose(launcher, "Say hello");
    shown(launcher)
}

#[test]
fn pane_ext_dev_builds_in_the_terminal_and_develops_in_pane_until_it_closes() {
    let folder = sample("pane-ext-dev", GREETING);
    let identity = PackageIdentity::local(&folder).unwrap();
    let data = tempfile::tempdir().unwrap();
    let launcher = Launcher::with_packages(
        Ok(Runtime::start().unwrap()),
        vec![],
        data.path().join("extensions"),
    );
    let endpoint = endpoint(data.path());
    let (_server, previews) = local_channel::serve(launcher.clone(), &endpoint).unwrap();
    let previewed = window(launcher.clone(), previews);
    let terminal = Terminal::default();
    let mut pane_ext = PaneExt::dev(&folder, &endpoint, data.path(), &terminal);

    // The build prints here; Pane shows its install preview, which is
    // confirmed, and develops the package with that build.
    terminal.wait_for("Compiling hello-rust");
    terminal.wait_for("Pane shows the install preview of Hello Rust");
    terminal.wait_for("Pane develops Hello Rust");
    assert!(launcher.development(&identity).is_some());
    let previewed = previewed.lock().unwrap().clone();
    assert_eq!(previewed.len(), 1, "{previewed:?}");
    assert_eq!(previewed[0].0, "Hello Rust");
    assert!(
        previewed[0].1.contains(&format!("Source: {identity}")),
        "{previewed:?}"
    );

    // Pane's messages about the package and what the package prints come
    // here as Pane's extension log has them.
    terminal.wait_for("Pane: Developing Hello Rust with pane-ext");
    assert_eq!(
        say_hello(&launcher),
        Status::Result("Hello from Rust".into())
    );
    terminal.wait_for("saying hello");

    // A save that does not build: the compiler's errors print here, and
    // Pane keeps running the working code.
    save(&folder, r#"const GREETING: &str = 42;"#);
    terminal.wait_for("mismatched types");
    terminal.wait_for("Pane: Hello Rust did not build");
    let failed = || {
        launcher
            .development(&identity)
            .is_some_and(|development| development.failure.is_some())
    };
    wait_until("the build failure in Pane", failed, || terminal.text());
    assert_eq!(
        say_hello(&launcher),
        Status::Result("Hello from Rust".into())
    );

    // A fix is built here and reloaded there.
    save(&folder, r#"const GREETING: &str = "Hello from pane-ext";"#);
    terminal.wait_for("Pane: Reloaded Hello Rust");
    assert_eq!(
        say_hello(&launcher),
        Status::Result("Hello from pane-ext".into())
    );

    // Closing pane-ext, as Ctrl+C does, stops the development; the package
    // stays installed.
    pane_ext.kill();
    let stopped = || launcher.development(&identity).is_none();
    wait_until("the development to stop", stopped, || terminal.text());
    assert!(
        launcher
            .packages()
            .iter()
            .any(|package| package.identity == identity)
    );
}

/// The TypeScript sample's greeting, as it is committed.
const TYPESCRIPT_GREETING: &str = r#"const GREETING: string = "Hello from TypeScript";"#;

#[test]
fn pane_ext_dev_builds_a_typescript_package_with_the_componentizer_it_links() {
    let folder = js_sample("pane-ext-dev-ts", TYPESCRIPT_GREETING);
    let identity = PackageIdentity::local(&folder).unwrap();
    let data = tempfile::tempdir().unwrap();
    let launcher = Launcher::with_packages(
        Ok(Runtime::start().unwrap()),
        vec![],
        data.path().join("extensions"),
    );
    let endpoint = endpoint(data.path());
    let (_server, previews) = local_channel::serve(launcher.clone(), &endpoint).unwrap();
    let _previewed = window(launcher.clone(), previews);
    let terminal = Terminal::default();
    let mut pane_ext = PaneExt::dev(&folder, &endpoint, data.path(), &terminal);

    // The build (npm ci of the locked dependencies, tsc, esbuild,
    // componentization) prints here, and Pane develops the package with it.
    terminal.wait_for("pane-js: built");
    terminal.wait_for("Pane develops Hello TypeScript");
    assert!(launcher.development(&identity).is_some());
    assert_eq!(
        say_hello_typescript(&launcher),
        Status::Result("Hello from TypeScript".into())
    );

    // A save with a type error prints tsc's errors here, and Pane keeps
    // running the working code.
    js_save(&folder, r#"const GREETING: string = 42;"#);
    terminal.wait_for("error TS2322");
    terminal.wait_for("Pane: Hello TypeScript did not build");

    // A fix is built here and reloaded there.
    js_save(
        &folder,
        r#"const GREETING: string = "Hello from pane-ext";"#,
    );
    terminal.wait_for("Pane: Reloaded Hello TypeScript");
    assert_eq!(
        say_hello_typescript(&launcher),
        Status::Result("Hello from pane-ext".into())
    );

    // Closing pane-ext, as Ctrl+C does, stops the development.
    pane_ext.kill();
    let stopped = || launcher.development(&identity).is_none();
    wait_until("the development to stop", stopped, || terminal.text());
}

/// Opens Hello TypeScript from root search and runs "Say hello", returning
/// what it showed.
fn say_hello_typescript(launcher: &Launcher) -> Status {
    for _ in 0..3 {
        launcher.back();
    }
    choose(launcher, "Hello TypeScript");
    choose(launcher, "Say hello");
    shown(launcher)
}

#[test]
fn with_no_pane_listening_pane_ext_dev_says_where_it_looked_for_one() {
    // It looks while the first build runs, and stops it.
    let folder = sample("pane-ext-dev-no-pane", GREETING);
    let data = tempfile::tempdir().unwrap();
    let endpoint = endpoint(data.path());
    let output = Command::new(env!("CARGO_BIN_EXE_pane-ext"))
        .arg("dev")
        .arg(&folder)
        .env(local_channel::ENDPOINT_VARIABLE, endpoint.path())
        .env("PANE_APP", data.path().join("no-pane"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{printed}");
    assert!(printed.contains("no Pane answered on"), "{printed}");
    assert!(printed.contains("PANE_APP ("), "{printed}");
    assert!(printed.contains("no-pane"), "{printed}");
}
