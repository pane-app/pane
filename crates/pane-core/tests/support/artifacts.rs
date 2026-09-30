//! A local artifact source for tests: it serves, on 127.0.0.1 only, the
//! index document and payload tarballs of Pane's default extensions a
//! test publishes to it, as Pane's own downloads do. Nothing reaches the
//! network or Pane's published downloads.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha512};

/// The index document's name, as Pane reads it.
const INDEX_FILE: &str = "pane-defaults.json";

/// What a published payload is served as: its tarball and the entry the
/// index gives for it.
#[derive(Clone)]
struct Published {
    tarball: Vec<u8>,
    entry: Value,
}

/// How one request for a path is answered, instead of the payload, a
/// number of times.
#[derive(Clone)]
enum Behavior {
    /// Answer with this status and no body.
    Status(u16),
    /// Send `after` bytes of the body, then close the connection: a
    /// download interrupted partway, after `wait` (so the bytes are seen
    /// arriving before the connection goes).
    Drop {
        after: usize,
        wait: Option<Duration>,
    },
    /// Send the first piece of the body, wait `wait`, then the rest: a
    /// slow payload, whose bytes arrive one at a time.
    Stall { after: usize, wait: Duration },
}

/// One behavior to apply, to the next requests for a path ending in it.
struct Planned {
    path: String,
    behavior: Behavior,
    times: usize,
}

#[derive(Default)]
struct Served {
    /// By default extension id: (version, file, published).
    payloads: BTreeMap<String, Published>,
    /// The index document itself, served as written; `None` for the one
    /// the payloads describe.
    index: Option<String>,
    /// Every path asked for, in order.
    requests: Vec<String>,
    /// What to answer instead, for the next requests asking for a path.
    behaviors: Vec<Planned>,
}

pub struct Artifacts {
    url: String,
    served: Arc<Mutex<Served>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Artifacts {
    /// Starts serving on a free port of 127.0.0.1.
    pub fn start() -> Artifacts {
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
                        std::thread::spawn(move || answer(stream, &served));
                    }
                }
            })
        };
        Artifacts {
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

    fn served(&self) -> MutexGuard<'_, Served> {
        self.served.lock().unwrap()
    }

    /// Publishes the payload `files` (path in the package, contents) as
    /// `version` of the default extension `id`, in the index with its
    /// sha512 integrity and size, tagged as Pane's own downloads do.
    pub fn publish(&self, id: &str, version: &str, files: &[(&str, Vec<u8>)]) {
        let tarball = pack(files);
        let file = format!("{id}-{version}.tgz");
        let entry = json!({
            "id": id,
            "version": version,
            "file": file,
            "integrity": integrity(&tarball),
            "size": tarball.len(),
        });
        self.served()
            .payloads
            .insert(id.to_owned(), Published { tarball, entry });
    }

    /// Serves `index` as the index document itself, whatever it says: the
    /// document a broken source would give.
    pub fn serve_index(&self, index: String) {
        self.served().index = Some(index);
    }

    /// Answers the next `times` requests for a path ending in `path` with
    /// this status instead.
    pub fn fail_status(&self, path: &str, status: u16, times: usize) {
        self.plan(path, Behavior::Status(status), times);
    }

    /// Sends `after` bytes of the payload a path ending in `path` asks
    /// for, then closes the connection, the next `times` times: a
    /// download interrupted partway.
    pub fn drop_after(&self, path: &str, after: usize, times: usize) {
        self.plan(path, Behavior::Drop { after, wait: None }, times);
    }

    /// As [`Artifacts::drop_after`], but the bytes arrive, `wait` passes,
    /// and only then the connection closes: an interruption slow enough
    /// that the core is seen still working while it happens.
    pub fn drop_after_waiting(&self, path: &str, after: usize, wait: Duration, times: usize) {
        self.plan(
            path,
            Behavior::Drop {
                after,
                wait: Some(wait),
            },
            times,
        );
    }

    /// Sends the first `after` bytes of the payload a path ending in
    /// `path` asks for, waits `wait`, then the rest, every time: a slow
    /// payload whose bytes arrive separately.
    pub fn stall(&self, path: &str, after: usize, wait: Duration) {
        self.plan(path, Behavior::Stall { after, wait }, usize::MAX);
    }

    fn plan(&self, path: &str, behavior: Behavior, times: usize) {
        self.served().behaviors.push(Planned {
            path: path.to_owned(),
            behavior,
            times,
        });
    }

    /// Stops answering a path ending in `path` other than with its
    /// payload: a source that works again.
    pub fn stop_failing(&self, path: &str) {
        self.served()
            .behaviors
            .retain(|planned| planned.path != path);
    }

    /// Damages the stored payload of the default extension `id`: its
    /// bytes no longer match the integrity its index entry gives.
    pub fn corrupt(&self, id: &str) {
        let mut served = self.served();
        let Some(published) = served.payloads.get_mut(id) else {
            panic!("no payload of {id} published");
        };
        let mut damaged = published.tarball.clone();
        let at = damaged.len() / 2;
        damaged[at] = damaged[at].wrapping_add(1);
        published.tarball = damaged;
    }

    /// Every path asked for so far, in order.
    pub fn requests(&self) -> Vec<String> {
        self.served().requests.clone()
    }

    /// The payload tarballs asked for so far.
    pub fn payload_requests(&self) -> Vec<String> {
        self.requests()
            .into_iter()
            .filter(|path| path.ends_with(".tgz"))
            .collect()
    }

    /// The index document the payloads describe, for writing a broken one
    /// from a working one.
    pub fn index(&self) -> String {
        index_of(&self.served())
    }
}

impl Drop for Artifacts {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the accepting thread, which then stops.
        let _ = TcpStream::connect(self.url.trim_start_matches("http://").trim_end_matches('/'));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The index document `served`'s payloads describe.
fn index_of(served: &Served) -> String {
    let defaults: Vec<&Value> = served
        .payloads
        .values()
        .map(|published| &published.entry)
        .collect();
    json!({ "formatVersion": 1, "defaults": defaults }).to_string()
}

/// The path a request's first line names.
fn requested(reader: &mut BufReader<TcpStream>) -> Option<String> {
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let path = line.split_whitespace().nth(1)?.to_owned();
    // The rest of the request's head.
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => break,
            Ok(_) if header == "\r\n" || header == "\n" => break,
            Ok(_) => {}
        }
    }
    Some(path)
}

fn answer(stream: TcpStream, served: &Mutex<Served>) {
    let mut reader = BufReader::new(stream);
    let Some(path) = requested(&mut reader) else {
        return;
    };
    let mut stream = reader.into_inner();
    let mut served = served.lock().unwrap();
    served.requests.push(path.clone());
    let planned = served
        .behaviors
        .iter()
        .position(|planned| path.ends_with(&planned.path));
    let (status, body, behavior) = match planned {
        Some(at) => {
            let planned = &mut served.behaviors[at];
            planned.times -= 1;
            let behavior = planned.behavior.clone();
            if planned.times == 0 {
                served.behaviors.remove(at);
            }
            let (status, body) = answer_behavior(&served, &path, &behavior);
            (status, body, Some(behavior))
        }
        None => {
            let (status, body) = answer_payload(&served, &path);
            (status, body, None)
        }
    };
    drop(served);
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: \
         application/octet-stream\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    match behavior {
        Some(Behavior::Drop { after, wait }) => {
            let _ = stream.write_all(&body[..after]);
            let _ = stream.flush();
            if let Some(wait) = wait {
                std::thread::sleep(wait);
            }
            // The connection ends partway through the body it announced.
            let _ = stream.shutdown(std::net::Shutdown::Write);
        }
        Some(Behavior::Stall { after, wait }) => {
            let _ = stream.write_all(&body[..after]);
            let _ = stream.flush();
            std::thread::sleep(wait);
            let _ = stream.write_all(&body[after..]);
            let _ = stream.flush();
        }
        _ => {
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        }
    }
}

/// What a behavior answers for `path`: a status, and the body it is said
/// to hold (the whole of what the path would answer).
fn answer_behavior(served: &Served, path: &str, behavior: &Behavior) -> (&'static str, Vec<u8>) {
    let (_, body) = answer_payload(served, path);
    match behavior {
        Behavior::Status(status) => (
            match *status {
                404 => "404 Not Found",
                403 => "403 Forbidden",
                500 => "500 Internal Server Error",
                503 => "503 Service Unavailable",
                _ => "418 I'm a teapot",
            },
            Vec::new(),
        ),
        Behavior::Drop { .. } | Behavior::Stall { .. } => ("200 OK", body),
    }
}

/// What `path` asks for: the index document, or a published payload.
fn answer_payload(served: &Served, path: &str) -> (&'static str, Vec<u8>) {
    let path = path.trim_start_matches('/');
    if path == INDEX_FILE {
        let index = served.index.clone().unwrap_or_else(|| index_of(served));
        return ("200 OK", index.into_bytes());
    }
    let found = served
        .payloads
        .values()
        .find(|published| published.entry["file"] == path);
    match found {
        Some(published) => ("200 OK", published.tarball.clone()),
        None => ("404 Not Found", Vec::new()),
    }
}

/// `sha512-<base64>` of `bytes`, as the index names integrity.
pub fn integrity(bytes: &[u8]) -> String {
    format!("sha512-{}", base64(&Sha512::digest(bytes)))
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                text.push(ALPHABET[(n >> shift) as usize & 63] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// A gzipped tarball holding `files` (path in the package, contents)
/// under `package/`, as Pane's own downloads pack a payload.
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
