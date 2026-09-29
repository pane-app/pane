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
}

struct Served {
    repositories: BTreeMap<String, PathBuf>,
    requests: Vec<String>,
    mode: Mode,
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
            mode: Mode::Normal,
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

    /// Every request received, as `METHOD /path?query`.
    pub fn requests(&self) -> Vec<String> {
        self.served.lock().unwrap().requests.clone()
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
    let (mode, repositories) = {
        let mut served = served.lock().unwrap();
        served.requests.push(format!("{method} {target}"));
        (served.mode, served.repositories.clone())
    };
    let mut stream = stream;
    let mut respond = |status: &str, headers: &[(&str, String)], body: &[u8]| {
        let mut head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", body.len());
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("Connection: close\r\n\r\n");
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body);
    };
    match mode {
        Mode::Redirect => {
            return respond(
                "301 Moved Permanently",
                &[("Location", "https://elsewhere.invalid/".into())],
                b"",
            );
        }
        Mode::SignIn => return respond("401 Unauthorized", &[], b"sign in"),
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
        _ => return respond("404 Not Found", &[], b""),
    };
    let name = name.strip_suffix(".git").unwrap_or(name);
    let Some(dir) = repositories.get(name) else {
        return respond("404 Not Found", &[], b"");
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
        return respond("500 Internal Server Error", &[], &output.stderr);
    }
    respond("200 OK", &[("Content-Type", content_type.into())], &out);
}
