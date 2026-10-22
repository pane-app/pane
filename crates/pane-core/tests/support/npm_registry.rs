//! A local npm registry for tests: it serves, on 127.0.0.1 only, the
//! packages a test publishes to it, in the abbreviated metadata format npm's
//! registry answers with, and their tarballs. Nothing reaches the network.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde_json::{Value, json};
use sha2::{Digest, Sha512};

/// One published version of a package.
#[derive(Clone)]
pub struct Version {
    pub tarball: Vec<u8>,
    /// The `dist` object served for it; `None` for the one the registry
    /// computes (the tarball's address and its sha512 integrity).
    pub dist: Option<Value>,
}

#[derive(Default)]
struct Served {
    /// By package name, each version by number, and its `latest` tag.
    packages: BTreeMap<String, (BTreeMap<String, Version>, Option<String>)>,
    /// Every path asked for, in order.
    requests: Vec<String>,
    /// The answers held back until their hold is dropped, by path: a
    /// stand-in for a source slow to answer, so a test can catch a pass
    /// mid-flight (see [`Registry::hold`]).
    held: BTreeMap<String, Arc<Held>>,
}

/// An answer the registry holds back until it is let go.
#[derive(Default)]
struct Held {
    go: Mutex<bool>,
    let_go: std::sync::Condvar,
}

impl Held {
    /// Waits until let go. The answering thread holds no registry lock
    /// while it waits.
    fn wait(&self) {
        let mut go = self
            .go
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while !*go {
            go = self
                .let_go
                .wait(go)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

/// The registry's answer to one path, held back until this is dropped:
/// a test's stand-in for a source slow to answer, so a pass can be caught
/// mid-flight — its toast, its record — before it goes on.
pub struct HeldAnswer(Arc<Held>);

impl Drop for HeldAnswer {
    fn drop(&mut self) {
        let mut go = self
            .0
            .go
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *go = true;
        self.0.let_go.notify_all();
    }
}

pub struct Registry {
    url: String,
    served: Arc<Mutex<Served>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Registry {
    /// Starts serving on a free port of 127.0.0.1.
    pub fn start() -> Registry {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let served = Arc::new(Mutex::new(Served::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let served = served.clone();
            let stop = stop.clone();
            let url = url.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        let served = served.clone();
                        let url = url.clone();
                        std::thread::spawn(move || answer(stream, &served, &url));
                    }
                }
            })
        };
        Registry {
            url,
            served,
            stop,
            thread: Some(thread),
        }
    }

    /// Its address, such as `http://127.0.0.1:43127/`.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Publishes `tarball` as `version` of `name`, tagged `latest`.
    pub fn publish(&self, name: &str, version: &str, tarball: Vec<u8>) {
        self.publish_with(name, version, tarball, None);
        self.tag_latest(name, version);
    }

    /// Publishes `tarball` as `version` of `name` with the `dist` object
    /// `dist` (else the one the registry computes), without tagging it.
    pub fn publish_with(&self, name: &str, version: &str, tarball: Vec<u8>, dist: Option<Value>) {
        let mut served = self.served.lock().unwrap();
        let (versions, _) = served.packages.entry(name.to_owned()).or_default();
        versions.insert(version.to_owned(), Version { tarball, dist });
    }

    /// Tags `version` of `name` as its latest.
    pub fn tag_latest(&self, name: &str, version: &str) {
        let mut served = self.served.lock().unwrap();
        served.packages.get_mut(name).unwrap().1 = Some(version.to_owned());
    }

    /// Every path asked for so far.
    pub fn requests(&self) -> Vec<String> {
        self.served.lock().unwrap().requests.clone()
    }

    /// The tarballs asked for so far.
    pub fn tarball_requests(&self) -> Vec<String> {
        self.requests()
            .into_iter()
            .filter(|path| path.ends_with(".tgz"))
            .collect()
    }

    /// The address the registry gives for the tarball of `name` at `version`.
    pub fn tarball_url(&self, name: &str, version: &str) -> String {
        tarball_url(&self.url, name, version)
    }

    /// Holds the answer to `path` back until the returned hold is
    /// dropped: the path is asked for and counted as usual, but the
    /// answer waits, as a slow source's would. A test so catches a pass
    /// mid-flight. The metadata path of `name` is
    /// `/{name with / written %2f}`.
    pub fn hold(&self, path: &str) -> HeldAnswer {
        let mut served = self.served.lock().unwrap();
        let held = Arc::new(Held::default());
        served.held.insert(path.to_owned(), held.clone());
        HeldAnswer(held)
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the accepting thread, which then stops.
        let _ = TcpStream::connect(self.url.trim_start_matches("http://").trim_end_matches('/'));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn tarball_url(base: &str, name: &str, version: &str) -> String {
    let file = name.rsplit('/').next().unwrap();
    format!("{base}{name}/-/{file}-{version}.tgz")
}

/// `sha512-<base64>` of `bytes`, as npm writes integrity.
pub fn integrity(bytes: &[u8]) -> String {
    format!("sha512-{}", base64(&Sha512::digest(bytes)))
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                text.push(ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}

fn answer(stream: TcpStream, served: &Mutex<Served>, base: &str) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
    // The rest of the request's head.
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => break,
            Ok(_) if header == "\r\n" || header == "\n" => break,
            Ok(_) => {}
        }
    }
    let mut stream = reader.into_inner();
    let (status, body, held) = {
        let mut served = served.lock().unwrap();
        served.requests.push(path.clone());
        let answer = respond(&served, base, &path);
        (answer.0, answer.1, served.held.get(&path).cloned())
    };
    // The held answer waits here, holding no lock: other requests answer.
    if let Some(held) = held {
        held.wait();
    }
    // `/redirect/<path>` sends the client to `/<path>`, as a registry
    // pointing elsewhere would.
    let location = path
        .strip_prefix("/redirect/")
        .map(|rest| format!("Location: {base}{rest}\r\n"))
        .unwrap_or_default();
    let status = if location.is_empty() {
        status
    } else {
        "302 Found"
    };
    let head = format!(
        "HTTP/1.1 {status}\r\n{location}Content-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

fn respond(served: &Served, base: &str, path: &str) -> (&'static str, Vec<u8>) {
    let not_found = ("404 Not Found", br#"{"error":"Not found"}"#.to_vec());
    let path = path.trim_start_matches('/');
    if let Some((name, file)) = path.split_once("/-/") {
        let Some((versions, _)) = served.packages.get(name) else {
            return not_found;
        };
        return versions
            .iter()
            .find(|(version, _)| tarball_url(base, name, version).ends_with(&format!("/-/{file}")))
            .map_or(not_found, |(_, version)| {
                ("200 OK", version.tarball.clone())
            });
    }
    let name = path.replace("%2f", "/").replace("%2F", "/");
    let Some((versions, latest)) = served.packages.get(&name) else {
        return not_found;
    };
    let described: serde_json::Map<String, Value> = versions
        .iter()
        .map(|(number, version)| {
            let dist = version.dist.clone().unwrap_or_else(|| {
                json!({
                    "tarball": tarball_url(base, &name, number),
                    "integrity": integrity(&version.tarball),
                    "shasum": "not checked",
                })
            });
            (
                number.clone(),
                json!({ "name": name, "version": number, "dist": dist }),
            )
        })
        .collect();
    let mut tags = serde_json::Map::new();
    if let Some(latest) = latest {
        tags.insert("latest".into(), json!(latest));
    }
    let metadata = json!({ "name": name, "dist-tags": tags, "versions": described });
    ("200 OK", metadata.to_string().into_bytes())
}

/// A gzipped npm tarball holding `files` (path in the package, contents)
/// under `package/`.
pub fn pack(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    for (path, contents) in files {
        let mut header = tar::Header::new_ustar();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        tar.append_data(&mut header, format!("package/{path}"), contents.as_slice())
            .unwrap();
    }
    let tar = tar.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

/// A gzipped tarball of raw entries `(path, kind, contents)`, headers
/// written as given, so that paths and kinds npm never packs can be made.
pub fn pack_raw(entries: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
    let mut tar = Vec::new();
    for (path, kind, contents) in entries {
        let mut header = tar::Header::new_gnu();
        header.as_old_mut().name[..path.len()].copy_from_slice(path.as_bytes());
        header.set_entry_type(*kind);
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        if *kind == tar::EntryType::Symlink {
            header.set_link_name("../../../outside").unwrap();
        }
        header.set_cksum();
        tar.extend_from_slice(header.as_bytes());
        tar.extend_from_slice(contents);
        tar.resize(tar.len().div_ceil(512) * 512, 0);
    }
    tar.resize(tar.len() + 1024, 0);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

/// The files of the npm sample `cargo xtask guests` assembles in
/// `target/guests/npm/greeter`, with `package.json` and `pane.json` at
/// `version`.
pub fn greeter_files(guests: &Path, version: &str) -> Vec<(&'static str, Vec<u8>)> {
    let folder = guests.join("npm/greeter");
    assert!(
        folder.is_dir(),
        "{} is missing; run `cargo xtask guests`",
        folder.display()
    );
    let with_version = |file: &str| {
        let mut json: Value =
            serde_json::from_slice(&std::fs::read(folder.join(file)).unwrap()).unwrap();
        json["version"] = json!(version);
        serde_json::to_vec_pretty(&json).unwrap()
    };
    vec![
        ("package.json", with_version("package.json")),
        ("pane.json", with_version("pane.json")),
        (
            "sample_npm_js.wasm",
            std::fs::read(folder.join("sample_npm_js.wasm")).unwrap(),
        ),
    ]
}
