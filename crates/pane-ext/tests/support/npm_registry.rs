//! A local npm registry for `pane-ext`'s tests: it serves, on 127.0.0.1
//! only, the packages a test publishes to it, in the abbreviated metadata
//! format npm's registry answers with, and their tarballs — the same
//! service `crates/pane-core/tests/support/npm_registry.rs` gives Pane's
//! own npm tests. Nothing reaches the network.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde_json::{Value, json};
use sha2::{Digest, Sha512};

/// One published version of a package: its tarball.
struct Version {
    tarball: Vec<u8>,
}

#[derive(Default)]
struct Served {
    /// By package name, each version by number, and its `latest` tag.
    packages: BTreeMap<String, (BTreeMap<String, Version>, Option<String>)>,
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
        let mut served = self.served.lock().unwrap();
        let (versions, latest) = served.packages.entry(name.to_owned()).or_default();
        versions.insert(version.to_owned(), Version { tarball });
        *latest = Some(version.to_owned());
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

/// The address the registry gives for the tarball of `name` at `version`.
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
    let body = {
        let mut served = served.lock().unwrap();
        respond(&served, base, &path)
    };
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

/// The metadata of the package `path` names, or its tarball.
fn respond(served: &Served, base: &str, path: &str) -> Vec<u8> {
    let path = path.trim_start_matches('/');
    if let Some((name, file)) = path.split_once("/-/") {
        let Some((versions, _)) = served.packages.get(name) else {
            return not_found();
        };
        return versions
            .iter()
            .find(|(version, _)| {
                tarball_url(base, name, version).ends_with(&format!("/-/{file}"))
            })
            .map_or_else(not_found, |(_, version)| version.tarball.clone());
    }
    let name = path.replace("%2f", "/").replace("%2F", "/");
    let Some((versions, latest)) = served.packages.get(&name) else {
        return not_found();
    };
    let described: serde_json::Map<String, Value> = versions
        .iter()
        .map(|(number, version)| {
            let dist = json!({
                "tarball": tarball_url(base, &name, number),
                "integrity": integrity(&version.tarball),
                "shasum": "not checked",
            });
            (number.clone(), json!({ "name": name, "version": number, "dist": dist }))
        })
        .collect();
    let mut tags = serde_json::Map::new();
    if let Some(latest) = latest {
        tags.insert("latest".into(), json!(latest));
    }
    let metadata = json!({ "name": name, "dist-tags": tags, "versions": described });
    metadata.to_string().into_bytes()
}

fn not_found() -> Vec<u8> {
    br#"{"error":"Not found"}"#.to_vec()
}
