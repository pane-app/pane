//! A stand-in for Pane's releases on GitHub, for tests: it serves, on
//! 127.0.0.1 only, the listing GitHub's API gives of Pane's releases
//! (`/repos/pane-app/pane/releases`), and each release's assets — the
//! application package and its `.sha256` — under the address GitHub
//! downloads them from (`/pane-app/pane/releases/download/v<version>/
//! <name>`), which redirects to where the bytes are (`/assets/<name>`), as
//! GitHub redirects to the host it serves release assets from. Nothing
//! reaches the network or Pane's published releases.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The path of the listing of Pane's releases, as Pane asks GitHub's API
/// for it (without its query).
pub const LISTING: &str = "/repos/pane-app/pane/releases";

/// The path an asset's bytes are served at, once the download's redirect
/// was followed.
pub fn asset_path(name: &str) -> String {
    format!("/assets/{name}")
}

/// How one request for a path is answered, instead of what it asks for, a
/// number of times.
#[derive(Clone)]
enum Behavior {
    /// Answer with this status and no body.
    Status(u16),
    /// Answer as GitHub does when an address used up its rate limit: 403,
    /// no requests remaining, the limit lifting in ten minutes.
    RateLimited,
    /// Redirect to this address.
    Redirect(String),
    /// Send `after` bytes of the body, then close the connection: a
    /// download interrupted partway, after `wait` (so the bytes are seen
    /// arriving before the connection goes).
    Drop {
        after: usize,
        wait: Option<Duration>,
    },
    /// Send the first piece of the body, wait `wait`, then the rest: a
    /// slow package, whose bytes arrive one at a time.
    Stall { after: usize, wait: Duration },
}

/// One behavior to apply, to the next requests for a path ending in it.
struct Planned {
    path: String,
    behavior: Behavior,
    times: usize,
}

/// One release: its version and its assets (name, bytes), the package
/// and, unless a test took it away, the `.sha256` beside it.
struct Published {
    version: String,
    assets: Vec<(String, Vec<u8>)>,
}

#[derive(Default)]
struct Served {
    /// The releases, oldest first; the listing names them newest first.
    releases: Vec<Published>,
    /// The listing itself, served as written; `None` for the one the
    /// releases describe.
    listing: Option<String>,
    /// Every path asked for, in order.
    requests: Vec<String>,
    /// What to answer instead, for the next requests asking for a path.
    behaviors: Vec<Planned>,
}

pub struct Releases {
    url: String,
    served: Arc<Mutex<Served>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Releases {
    /// Starts serving on a free port of 127.0.0.1.
    pub fn start() -> Releases {
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
        Releases {
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

    /// Publishes the application package `zip` as the release `v<version>`
    /// of Pane for `target` (such as `windows-x86_64`), named as
    /// `cargo xtask package-windows` names it, with its `.sha256` beside
    /// it. A release of that version already published is replaced.
    pub fn publish_application(&self, version: &str, target: &str, zip: &[u8]) {
        self.publish(version, format!("pane-{version}-{target}.zip"), zip);
    }

    /// As [`Releases::publish_application`], but the package is the
    /// gzipped tarball the Linux package is (`pane-<version>-<target>
    /// .tar.gz`).
    pub fn publish_application_tgz(&self, version: &str, target: &str, tarball: &[u8]) {
        self.publish(version, format!("pane-{version}-{target}.tar.gz"), tarball);
    }

    fn publish(&self, version: &str, name: String, package: &[u8]) {
        let checksum = format!("{}  {name}\n", sha256(package));
        let mut served = self.served();
        served.releases.retain(|release| release.version != version);
        served.releases.push(Published {
            version: version.to_owned(),
            assets: vec![
                (format!("{name}.sha256"), checksum.into_bytes()),
                (name, package.to_vec()),
            ],
        });
    }

    /// Takes the `.sha256` files away from every release: packages that
    /// cannot be checked.
    pub fn remove_checksums(&self) {
        for release in &mut self.served().releases {
            release
                .assets
                .retain(|(name, _)| !name.ends_with(".sha256"));
        }
    }

    /// Serves `listing` as the listing itself, whatever it says: the
    /// listing a broken source would give.
    pub fn serve_listing(&self, listing: String) {
        self.served().listing = Some(listing);
    }

    /// Serves the listing the releases describe again, after one was
    /// served as written.
    pub fn derived_listing(&self) {
        self.served().listing = None;
    }

    /// The listing the releases describe, for writing a broken one from a
    /// working one.
    pub fn listing(&self) -> Value {
        listing_of(&self.served(), &self.url)
    }

    /// Answers the next `times` requests for a path ending in `path` with
    /// this status instead.
    pub fn fail_status(&self, path: &str, status: u16, times: usize) {
        self.plan(path, Behavior::Status(status), times);
    }

    /// Answers the next `times` requests for a path ending in `path` as
    /// GitHub does once an address used up its rate limit.
    pub fn rate_limit(&self, path: &str, times: usize) {
        self.plan(path, Behavior::RateLimited, times);
    }

    /// Redirects the next `times` requests for a path ending in `path` to
    /// `location`.
    pub fn redirect(&self, path: &str, location: &str, times: usize) {
        self.plan(path, Behavior::Redirect(location.to_owned()), times);
    }

    /// Sends `after` bytes of what a path ending in `path` asks for, then
    /// closes the connection, the next `times` times: a download
    /// interrupted partway.
    pub fn drop_after(&self, path: &str, after: usize, times: usize) {
        self.plan(path, Behavior::Drop { after, wait: None }, times);
    }

    /// Sends the first `after` bytes of what a path ending in `path` asks
    /// for, waits `wait`, then the rest, every time: a slow package whose
    /// bytes arrive separately.
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

    /// Stops answering a path ending in `path` other than with what it
    /// asks for: a source that works again.
    pub fn stop_failing(&self, path: &str) {
        self.served()
            .behaviors
            .retain(|planned| planned.path != path);
    }

    /// Damages the stored application package of the newest release: its
    /// bytes no longer match the sha256 its `.sha256` gives.
    pub fn corrupt_application(&self) {
        let mut served = self.served();
        let Some(release) = served.releases.last_mut() else {
            panic!("no application package published");
        };
        let (_, package) = release
            .assets
            .iter_mut()
            .find(|(name, _)| !name.ends_with(".sha256"))
            .expect("the release holds its package");
        let at = package.len() / 2;
        package[at] = package[at].wrapping_add(1);
    }

    /// Every path asked for so far, in order, with its query.
    pub fn requests(&self) -> Vec<String> {
        self.served().requests.clone()
    }

    /// How many times an asset whose name ends with `suffix` was
    /// downloaded: its bytes asked for, once its redirect was followed.
    pub fn downloaded(&self, suffix: &str) -> usize {
        self.requests()
            .iter()
            .filter(|path| path.starts_with("/assets/") && path.ends_with(suffix))
            .count()
    }

    /// How many times the listing was asked for.
    pub fn listed(&self) -> usize {
        self.requests()
            .iter()
            .filter(|path| without_query(path) == LISTING)
            .count()
    }
}

impl Drop for Releases {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the accepting thread, which then stops.
        let _ = TcpStream::connect(self.url.trim_start_matches("http://").trim_end_matches('/'));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The listing `served`'s releases describe, newest first, as GitHub's
/// API writes one (the fields Pane reads, and a few it does not).
fn listing_of(served: &Served, url: &str) -> Value {
    let releases: Vec<Value> = served
        .releases
        .iter()
        .rev()
        .map(|release| {
            let tag = format!("v{}", release.version);
            let assets: Vec<Value> = release
                .assets
                .iter()
                .map(|(name, bytes)| {
                    json!({
                        "name": name,
                        "size": bytes.len(),
                        "state": "uploaded",
                        "browser_download_url":
                            format!("{url}pane-app/pane/releases/download/{tag}/{name}"),
                    })
                })
                .collect();
            json!({
                "tag_name": tag,
                "name": format!("Pane {}", release.version),
                "draft": false,
                "prerelease": true,
                "assets": assets,
            })
        })
        .collect();
    Value::Array(releases)
}

/// `path` without its query.
fn without_query(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
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

/// What one request is answered with: its status line, its headers
/// beyond the length, and its body.
struct Reply {
    status: &'static str,
    headers: Vec<String>,
    body: Vec<u8>,
}

impl Reply {
    fn ok(body: Vec<u8>) -> Reply {
        Reply {
            status: "200 OK",
            headers: Vec::new(),
            body,
        }
    }

    fn not_found() -> Reply {
        Reply {
            status: "404 Not Found",
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn redirect(location: String) -> Reply {
        Reply {
            status: "302 Found",
            headers: vec![format!("Location: {location}")],
            body: Vec::new(),
        }
    }
}

fn answer(stream: TcpStream, served: &Mutex<Served>, url: &str) {
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
        .position(|planned| without_query(&path).ends_with(&planned.path));
    let (reply, behavior) = match planned {
        Some(at) => {
            let planned = &mut served.behaviors[at];
            planned.times -= 1;
            let behavior = planned.behavior.clone();
            if planned.times == 0 {
                served.behaviors.remove(at);
            }
            let reply = answer_behavior(&served, &path, url, &behavior);
            (reply, Some(behavior))
        }
        None => (answer_path(&served, &path, url), None),
    };
    drop(served);
    let mut head = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\n\
         Connection: close\r\n",
        reply.status,
        reply.body.len()
    );
    for header in &reply.headers {
        head.push_str(header);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let body = reply.body;
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

/// What a behavior answers for `path`: a status and the body it is said
/// to hold (the whole of what the path would answer).
fn answer_behavior(served: &Served, path: &str, url: &str, behavior: &Behavior) -> Reply {
    match behavior {
        Behavior::Status(status) => Reply {
            status: match *status {
                404 => "404 Not Found",
                403 => "403 Forbidden",
                429 => "429 Too Many Requests",
                500 => "500 Internal Server Error",
                503 => "503 Service Unavailable",
                _ => "418 I'm a teapot",
            },
            headers: Vec::new(),
            body: Vec::new(),
        },
        Behavior::RateLimited => {
            let reset = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 600;
            Reply {
                status: "403 Forbidden",
                headers: vec![
                    "X-RateLimit-Limit: 60".to_owned(),
                    "X-RateLimit-Remaining: 0".to_owned(),
                    format!("X-RateLimit-Reset: {reset}"),
                ],
                body: br#"{"message":"API rate limit exceeded"}"#.to_vec(),
            }
        }
        Behavior::Redirect(location) => Reply::redirect(location.clone()),
        Behavior::Drop { .. } | Behavior::Stall { .. } => answer_path(served, path, url),
    }
}

/// What `path` asks for: the listing, a release's download (a redirect to
/// its bytes), or the bytes.
fn answer_path(served: &Served, path: &str, url: &str) -> Reply {
    let path = without_query(path);
    if path == LISTING {
        let listing = served
            .listing
            .clone()
            .unwrap_or_else(|| listing_of(served, url).to_string());
        return Reply::ok(listing.into_bytes());
    }
    let find = |name: &str, version: Option<&str>| {
        served
            .releases
            .iter()
            .filter(|release| version.is_none_or(|version| release.version == version))
            .flat_map(|release| release.assets.iter())
            .find(|(asset, _)| asset == name)
            .map(|(_, bytes)| bytes.clone())
    };
    if let Some(rest) = path.strip_prefix("/pane-app/pane/releases/download/v")
        && let Some((version, name)) = rest.split_once('/')
    {
        return match find(name, Some(version)) {
            Some(_) => Reply::redirect(format!("{url}assets/{name}")),
            None => Reply::not_found(),
        };
    }
    if let Some(name) = path.strip_prefix("/assets/")
        && let Some(bytes) = find(name, None)
    {
        return Reply::ok(bytes);
    }
    Reply::not_found()
}

/// The sha256 of `bytes`, in lowercase hexadecimal, as `sha256sum`
/// writes it.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A gzipped tarball holding `files` (path in the package, contents)
/// under `pane/`, as `cargo xtask package-linux` packs Pane's Linux
/// package: what a Pane application update on Linux downloads and
/// unpacks.
pub fn pack_tgz(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    pack_under("pane", files)
}

/// A gzipped tarball holding `files` under the folder `top`.
fn pack_under(top: &str, files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    for (path, contents) in files {
        let mut header = tar::Header::new_ustar();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        tar.append_data(&mut header, format!("{top}/{path}"), contents.as_slice())
            .unwrap();
    }
    let tar = tar.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

/// A zip holding `files` (path in the archive, contents) under `pane/`,
/// as `cargo xtask package-windows` packs Pane's Windows package: every
/// entry deflated, no directory entries, the same shape `xtask`'s zip writer
/// packs. (A hand-rolled writer rather than reusing `xtask`'s, because
/// the tests cannot reach into the xtask crate; the pane-core reader's
/// unit tests pin the reading of both methods, stored and deflated.)
pub fn pack_zip(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut zip: Vec<u8> = Vec::new();
    // (name, crc, size, deflated bytes, where the local header is).
    let mut entries: Vec<(String, u32, u64, Vec<u8>, u64)> = Vec::new();
    for (path, contents) in files {
        let name = format!("pane/{path}");
        let mut deflated =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        deflated.write_all(contents).unwrap();
        let packed = deflated.finish().unwrap();
        let crc = crc32fast::hash(contents);
        let at = zip.len() as u64;
        push_u32(&mut zip, 0x0403_4b50);
        push_u16(&mut zip, 20);
        push_u16(&mut zip, 0);
        push_u16(&mut zip, 8);
        push_u16(&mut zip, 0);
        push_u16(&mut zip, 0);
        push_u32(&mut zip, crc);
        push_u32(&mut zip, packed.len() as u32);
        push_u32(&mut zip, contents.len() as u32);
        push_u16(&mut zip, name.len() as u16);
        push_u16(&mut zip, 0);
        zip.extend_from_slice(name.as_bytes());
        zip.extend_from_slice(&packed);
        entries.push((name, crc, contents.len() as u64, packed, at));
    }
    let directory_at = zip.len() as u64;
    for (name, crc, size, packed, at) in &entries {
        push_u32(&mut zip, 0x0201_4b50);
        push_u16(&mut zip, 20);
        push_u16(&mut zip, 20);
        push_u16(&mut zip, 0);
        push_u16(&mut zip, 8);
        push_u16(&mut zip, 0);
        push_u16(&mut zip, 0);
        push_u32(&mut zip, *crc);
        push_u32(&mut zip, packed.len() as u32);
        push_u32(&mut zip, *size as u32);
        push_u16(&mut zip, name.len() as u16);
        push_u16(&mut zip, 0);
        push_u16(&mut zip, 0);
        push_u16(&mut zip, 0);
        push_u16(&mut zip, 0);
        push_u32(&mut zip, 0);
        push_u32(&mut zip, *at as u32);
        zip.extend_from_slice(name.as_bytes());
    }
    let directory_size = (zip.len() as u64 - directory_at) as u32;
    push_u32(&mut zip, 0x0605_4b50);
    push_u16(&mut zip, 0);
    push_u16(&mut zip, 0);
    push_u16(&mut zip, entries.len() as u16);
    push_u16(&mut zip, entries.len() as u16);
    push_u32(&mut zip, directory_size);
    push_u32(&mut zip, directory_at as u32);
    push_u16(&mut zip, 0);
    zip
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
