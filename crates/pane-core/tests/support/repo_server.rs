//! Git repositories for tests, made inside the test with the `git` program
//! and served over Git's smart HTTP protocol from 127.0.0.1 only: nothing
//! reaches the network. The server runs `git upload-pack --stateless-rpc`
//! for each request, as `git http-backend` does, so Pane's client is
//! checked against Git's own server.
//!
//! Only these tests run `git`, and with none of the user's configuration:
//! the environment is cleared, `HOME` is a folder of the test's own, and
//! the system and global configuration files are not read. Pane itself
//! never runs `git`.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tempfile::TempDir;

/// `git` with an environment of the test's own, run in `dir`.
pub fn git_in(dir: &Path, home: &Path) -> Command {
    let mut command = Command::new("git");
    command.env_clear();
    for kept in ["PATH", "SYSTEMROOT", "TMP", "TEMP", "TMPDIR"] {
        if let Some(value) = std::env::var_os(kept) {
            command.env(kept, value);
        }
    }
    command
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
        .env("GIT_AUTHOR_NAME", "Pane tests")
        .env("GIT_AUTHOR_EMAIL", "tests@pane.invalid")
        .env("GIT_COMMITTER_NAME", "Pane tests")
        .env("GIT_COMMITTER_EMAIL", "tests@pane.invalid")
        .env("GIT_AUTHOR_DATE", "2026-09-29T12:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-09-29T12:00:00Z")
        .current_dir(dir);
    command
}

fn run(mut command: Command) -> String {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{command:?}: {error}; these tests need `git`"));
    assert!(
        output.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// A repository made by a test, with its work tree.
pub struct Repo {
    dir: PathBuf,
    home: PathBuf,
}

impl Repo {
    /// A new repository in `dir`, whose default branch is `main`.
    pub fn init(dir: &Path, home: &Path) -> Repo {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::create_dir_all(home).unwrap();
        let repo = Repo {
            dir: dir.to_path_buf(),
            home: home.to_path_buf(),
        };
        repo.git(&["init", "--quiet", "--initial-branch=main"]);
        repo
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Runs `git` with `args` in the work tree; its output.
    pub fn git(&self, args: &[&str]) -> String {
        let mut command = git_in(&self.dir, &self.home);
        command.args(args);
        run(command)
    }

    /// Copies `files` (path in the work tree, contents) into the work tree,
    /// adds everything and commits it as `message`; the commit's id.
    pub fn commit(&self, files: &[(&str, Vec<u8>)], message: &str) -> String {
        for (path, contents) in files {
            let path = self.dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        self.git(&["add", "--all", "--force"]);
        self.git(&["commit", "--quiet", "--allow-empty", "-m", message]);
        self.head()
    }

    pub fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    pub fn tag(&self, name: &str) {
        self.git(&["tag", "--annotate", "-m", name, name]);
    }

    /// Allows the partial-clone filter (`uploadpack.allowFilter`), so the
    /// server advertises `filter` and answers `filter blob:none`: a host
    /// that serves partial clones, as GitHub's and GitLab's servers do
    /// (#311, ADR 0044).
    pub fn allow_filter(&self) {
        self.git(&["config", "uploadpack.allowFilter", "true"]);
    }

    /// Adds a tree entry of `mode` (`120000` a link, `160000` a submodule)
    /// named `path` pointing to `target`'s contents or commit, without a
    /// file in the work tree.
    pub fn add_entry(&self, mode: &str, target: &str, path: &str) {
        let id = if mode == "160000" {
            target.to_owned()
        } else {
            let mut command = git_in(&self.dir, &self.home);
            command
                .args(["hash-object", "-w", "--stdin"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped());
            let mut child = command.spawn().unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(target.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        self.git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("{mode},{id},{path}"),
        ]);
    }

    /// Writes `data` as an object of `kind` without Git checking it
    /// (`hash-object --literally`), so that a test can make objects `git`
    /// itself refuses to make, such as a tree entry named `../x`; its id.
    pub fn write_object(&self, kind: &str, data: &[u8]) -> String {
        let mut command = git_in(&self.dir, &self.home);
        command
            .args(["hash-object", "-w", "--literally", "-t", kind, "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(data).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "hash-object: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    /// Commits a tree made of `entries` (mode, raw name, file contents; a
    /// folder, mode `40000`, is empty), each name written as given, whatever
    /// it holds, and tags the commit `tag`; the commit's id.
    pub fn commit_raw_tree(&self, entries: &[(&str, &[u8], &[u8])], tag: &str) -> String {
        let mut tree = Vec::new();
        for (mode, name, contents) in entries {
            let blob = match *mode {
                "40000" => self.write_object("tree", b""),
                _ => self.write_object("blob", contents),
            };
            tree.extend_from_slice(format!("{mode} ").as_bytes());
            tree.extend_from_slice(name);
            tree.push(0);
            for i in 0..20 {
                tree.push(u8::from_str_radix(&blob[2 * i..2 * i + 2], 16).unwrap());
            }
        }
        let tree = self.write_object("tree", &tree);
        let commit = self.git(&["commit-tree", &tree, "-m", tag]);
        self.git(&["tag", tag, &commit]);
        commit
    }
}

/// The files of the Git sample `cargo xtask guests` assembles
/// (`target/guests/git/greeter`): its source, and with `built`, its
/// component under `dist/` too, as a release revision holds it.
pub fn greeter_files(guests: &Path, built: bool) -> Vec<(&'static str, Vec<u8>)> {
    let sample = guests.join("git/greeter");
    let mut files = Vec::new();
    for file in ["pane.json", "Cargo.toml", "README.md", "src/lib.rs"] {
        let contents = std::fs::read(sample.join(file)).unwrap_or_else(|error| {
            panic!("{}: {error}; run `cargo xtask guests`", sample.display())
        });
        files.push((file, contents));
    }
    if built {
        files.push((
            "dist/git_greeter.wasm",
            std::fs::read(sample.join("dist/git_greeter.wasm")).unwrap(),
        ));
    }
    files
}

/// The files of a collection offering that sample as one extension,
/// `clock` (ADR 0044): the index `index` at its root, and the sample's
/// package under `extensions/clock`, its titles saying the extension's id,
/// with its built component only when `built` (else the revision holds the
/// source only).
pub fn collection_files(guests: &Path, index: &str, built: bool) -> Vec<(&'static str, Vec<u8>)> {
    let sample = guests.join("git/greeter");
    let read = |file: &str| {
        std::fs::read(sample.join(file)).unwrap_or_else(|error| {
            panic!("{}: {error}; run `cargo xtask guests`", sample.display())
        })
    };
    let manifest = String::from_utf8(read("pane.json"))
        .unwrap()
        .replace("Greeter from Git", "Clock from Git");
    let mut files = vec![
        ("pane-collection.json", index.as_bytes().to_vec()),
        ("extensions/clock/pane.json", manifest.into_bytes()),
        ("extensions/clock/src/lib.rs", read("src/lib.rs")),
        ("README.md", b"The collection".to_vec()),
    ];
    if built {
        files.push((
            "extensions/clock/dist/git_greeter.wasm",
            read("dist/git_greeter.wasm"),
        ));
    }
    files
}

/// The files of a collection offering the Git sample as the extensions
/// `extensions` names, each by its id and by whether that one ships its
/// built component (one that does not holds the source only, which an
/// install of it explains), each manifest saying its own title,
/// description and version — what the choice's rows read of it (#308) —
/// and the first one shipping an icon its manifest names. The index is
/// the caller's, naming the extensions by id and folder.
pub fn extension_collection_files(
    guests: &Path,
    extensions: &[(&'static str, bool)],
) -> Vec<(&'static str, Vec<u8>)> {
    let sample = guests.join("git/greeter");
    let read = |file: &str| {
        std::fs::read(sample.join(file)).unwrap_or_else(|error| {
            panic!("{}: {error}; run `cargo xtask guests`", sample.display())
        })
    };
    let mut files: Vec<(&'static str, Vec<u8>)> = Vec::new();
    for (at, (id, built)) in extensions.iter().enumerate() {
        let title: String = id
            .chars()
            .enumerate()
            .map(|(at, character)| {
                if at == 0 {
                    character.to_ascii_uppercase()
                } else {
                    character
                }
            })
            .collect();
        let icon = (at == 0).then(|| r#", "icon": "icon.svg""#).unwrap_or("");
        let manifest = format!(
            r#"{{ "manifestVersion": 1, "title": "{title} from Git",
                 "description": "The {id} extension of the tools collection",
                 "version": "0.1.0"{icon}, "apiVersion": "0.1",
                 "commands": [{{ "id": "sample", "title": "{title} from Git",
                                 "component": "dist/git_greeter.wasm" }}],
                 "operations": [{{ "id": "greet", "version": 1,
                                   "component": "dist/git_greeter.wasm" }}] }}"#
        );
        let folder: &'static str = Box::leak(format!("extensions/{id}").into_boxed_str());
        files.push((
            Box::leak(format!("{folder}/pane.json").into_boxed_str()),
            manifest.into_bytes(),
        ));
        files.push((
            Box::leak(format!("{folder}/src/lib.rs").into_boxed_str()),
            read("src/lib.rs"),
        ));
        if *built {
            files.push((
                Box::leak(format!("{folder}/dist/git_greeter.wasm").into_boxed_str()),
                read("dist/git_greeter.wasm"),
            ));
        }
        if at == 0 {
            files.push((
                Box::leak(format!("{folder}/icon.svg").into_boxed_str()),
                br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"/>"#.to_vec(),
            ));
        }
    }
    files
}

/// How the server answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Git's protocol version 2, as a current server.
    Normal,
    /// Version 2 after a `# service=` line, as some servers answer.
    ServiceLine,
    /// Only protocol version 0, as an old server.
    VersionZero,
    /// A redirect to another address for every request.
    Redirect,
    /// `401`, as for a private repository.
    SignIn,
    /// A reference listing (`ls-refs`) longer than the 16 MiB Pane reads,
    /// as a repository with very many tags might answer; the rest as
    /// `Normal`.
    LongListing,
    /// Every answer comes `wait` later: a slow repository, whose fetch is
    /// in flight long enough to watch the status line while it runs.
    Slow(std::time::Duration),
}

struct Served {
    repositories: BTreeMap<String, PathBuf>,
    requests: Vec<String>,
    /// Each `fetch` command received, as its argument lines (`want …`,
    /// `filter …`, `deepen …`, `done`): what a test asks a partial fetch
    /// to have taken, read from what the server was asked for (#311).
    fetches: Vec<Vec<String>>,
    mode: Mode,
    /// Answer the next fetch (`command=fetch`) by closing the connection
    /// partway through its answer: a fetch interrupted.
    drop_next: bool,
}

/// A smart HTTP server for the repositories a test adds, on a free port of
/// 127.0.0.1.
pub struct Server {
    url: String,
    home: TempDir,
    served: Arc<Mutex<Served>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let home = tempfile::tempdir().unwrap();
        let served = Arc::new(Mutex::new(Served {
            repositories: BTreeMap::new(),
            requests: Vec::new(),
            fetches: Vec::new(),
            mode: Mode::Normal,
            drop_next: false,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let served = served.clone();
            let stop = stop.clone();
            let home = home.path().to_path_buf();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        let served = served.clone();
                        let home = home.clone();
                        std::thread::spawn(move || answer(stream, &served, &home));
                    }
                }
            })
        };
        Server {
            url,
            home,
            served,
            stop,
            thread: Some(thread),
        }
    }

    /// Its address, such as `http://127.0.0.1:43127/`.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Serves `repo` as `<name>` (and `<name>.git`); returns its address,
    /// `http://127.0.0.1:<port>/<name>.git`.
    pub fn serve(&self, name: &str, repo: &Repo) -> String {
        self.served
            .lock()
            .unwrap()
            .repositories
            .insert(name.to_owned(), repo.dir().to_path_buf());
        format!("{}{name}.git", self.url)
    }

    /// A folder for a new repository's `HOME`.
    pub fn home(&self) -> &Path {
        self.home.path()
    }

    pub fn set_mode(&self, mode: Mode) {
        self.served.lock().unwrap().mode = mode;
    }

    /// Answers the next fetch (`command=fetch`) by sending part of its
    /// answer and closing the connection: a fetch interrupted partway,
    /// which Pane tries again.
    pub fn drop_once(&self) {
        self.served.lock().unwrap().drop_next = true;
    }

    /// Every request received, as `METHOD /path?query`.
    pub fn requests(&self) -> Vec<String> {
        self.served.lock().unwrap().requests.clone()
    }

    /// Each `fetch` command received, as the lines it was sent (its `want`
    /// lines, `filter blob:none` where it filtered, `deepen 1`, `done`):
    /// what was asked of the server, so a test checks that a partial fetch
    /// took the trees and the chosen extensions' folders' blobs, and
    /// nothing else of the repository (#311).
    pub fn fetches(&self) -> Vec<Vec<String>> {
        self.served.lock().unwrap().fetches.clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.url.trim_start_matches("http://").trim_end_matches('/'));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn answer(stream: TcpStream, served: &Mutex<Served>, home: &Path) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return;
    };
    let (method, target) = (method.to_owned(), target.to_owned());
    let mut length = 0;
    let mut version_2 = false;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
            break;
        }
        let (name, value) = header.split_once(':').unwrap_or((&header, ""));
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            length = value.parse().unwrap_or(0);
        }
        if name.eq_ignore_ascii_case("git-protocol") && value.contains("version=2") {
            version_2 = true;
        }
    }
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    // Whether this request is a fetch (`command=fetch` in its body): a
    // fetch `drop_once` armed is answered by cutting the connection
    // partway through, and the arming lasts until a fetch asks, so other
    // requests leave it armed. Decided here, because the body is moved
    // into the server below.
    let is_fetch = body.windows(13).any(|part| part == b"command=fetch");
    let (mode, repositories, interrupt) = {
        let mut served = served.lock().unwrap();
        served.requests.push(format!("{method} {target}"));
        if is_fetch {
            served.fetches.push(arguments_of(&body));
        }
        (
            served.mode,
            served.repositories.clone(),
            is_fetch && std::mem::take(&mut served.drop_next),
        )
    };
    if let Mode::Slow(wait) = mode {
        std::thread::sleep(wait);
    }
    let mut stream = stream;
    match mode {
        Mode::Redirect => {
            return respond(
                stream,
                "301 Moved Permanently",
                &[("Location", "https://elsewhere.invalid/".into())],
                b"",
            );
        }
        Mode::SignIn => return respond(stream, "401 Unauthorized", &[], b"sign in"),
        _ => {}
    }
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let path = path.trim_start_matches('/');
    let (name, service) = match (
        path.strip_suffix("/info/refs"),
        path.strip_suffix("/git-upload-pack"),
    ) {
        (Some(name), _) if method == "GET" && query == "service=git-upload-pack" => {
            (name, "advertise")
        }
        (_, Some(name)) if method == "POST" => (name, "upload-pack"),
        _ => return respond(stream, "404 Not Found", &[], b""),
    };
    let name = name.strip_suffix(".git").unwrap_or(name);
    let Some(dir) = repositories.get(name) else {
        return respond(stream, "404 Not Found", &[], b"");
    };
    if mode == Mode::LongListing
        && service == "upload-pack"
        && body.windows(15).any(|part| part == b"command=ls-refs")
    {
        let line = format!("{} refs/tags/padding\n", "0".repeat(40));
        let line = format!("{:04x}{line}", line.len() + 4).into_bytes();
        let mut out = Vec::with_capacity((16 << 20) + 2 * line.len());
        while out.len() <= 16 << 20 {
            out.extend(&line);
        }
        out.extend(b"0000");
        return respond(
            stream,
            "200 OK",
            &[(
                "Content-Type",
                "application/x-git-upload-pack-result".into(),
            )],
            &out,
        );
    }
    let version_2 = version_2 && mode != Mode::VersionZero;
    let mut command = git_in(dir, home);
    command.arg("upload-pack").arg("--stateless-rpc");
    if service == "advertise" {
        command.arg("--advertise-refs");
    }
    command.arg(".");
    if version_2 {
        command.env("GIT_PROTOCOL", "version=2");
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&body);
    });
    let output = child.wait_with_output().unwrap();
    let _ = writer.join();
    let mut out = Vec::new();
    let content_type = if service == "advertise" {
        if !version_2 || mode == Mode::ServiceLine {
            let line = "# service=git-upload-pack\n";
            out.extend(format!("{:04x}{line}0000", line.len() + 4).into_bytes());
        }
        "application/x-git-upload-pack-advertisement"
    } else {
        "application/x-git-upload-pack-result"
    };
    out.extend(output.stdout);
    if !output.status.success() && out.is_empty() {
        return respond(stream, "500 Internal Server Error", &[], &output.stderr);
    }
    // A fetch interrupted partway: the answer announces its whole length
    // but the connection ends partway through it.
    if interrupt {
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/x-git-upload-pack-result\r\nConnection: close\r\n\r\n",
            out.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&out[..out.len() / 2]);
        let _ = stream.flush();
        let _ = stream.shutdown(std::net::Shutdown::Write);
        return;
    }
    respond(
        stream,
        "200 OK",
        &[("Content-Type", content_type.into())],
        &out,
    );
}

/// Writes one answer to `stream`: the status, headers and body, then the
/// connection ends.
fn respond(mut stream: TcpStream, status: &str, headers: &[(&str, String)], body: &[u8]) {
    let mut head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", body.len());
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

/// The argument lines of the protocol version 2 command in `body` (a
/// `fetch`): the pkt-lines after its delimiter, each without its newline —
/// `want <id>`, `filter blob:none`, `deepen 1`, `no-progress`, `done`.
fn arguments_of(body: &[u8]) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut at = 0;
    let mut past = false;
    while at + 4 <= body.len() {
        let length = std::str::from_utf8(&body[at..at + 4])
            .ok()
            .and_then(|head| usize::from_str_radix(head, 16).ok());
        let Some(length) = length else {
            break;
        };
        match length {
            0 => at += 4,
            1 => {
                at += 4;
                past = true;
            }
            4.. => {
                let Some(line) = body.get(at + 4..at + length) else {
                    break;
                };
                if past {
                    arguments.push(String::from_utf8_lossy(line).trim_end().to_owned());
                }
                at += length;
            }
            _ => break,
        }
    }
    arguments
}
