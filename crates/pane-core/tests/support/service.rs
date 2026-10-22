//! The fixture service: a small, made-up package registry answering JSON
//! over HTTP/1.1 on 127.0.0.1 only, which the package search samples
//! (`guests/sample-search*`) query. Tests start it on a free port;
//! `cargo run -p pane-core --example fixture_service` serves it on port 8740,
//! the samples' default address, for trying the samples by hand, or on the
//! port `--port` names (0 for a free one, as the smoke checks use, which
//! then set it as the sample's address). It never reaches beyond this
//! computer.
//!
//! - `GET /search?q=<text>`: `{"results": [{"name", "summary"}, ...]}`,
//!   the packages whose name contains the text, ignoring case. A text
//!   starting with `slow` is answered only after ten seconds, unless the
//!   client hangs up first, which is recorded (see [`Service::abandoned`]);
//!   the text `down` is answered `503 Service Unavailable` with
//!   `{"error": ...}`.
//! - `GET /packages/<name>`: `{"name", "summary", "version", "license"}`,
//!   or `404 Not Found`.
//!
//! Misbehaving answers, for either path, by the search text or package
//! name they start with (each ends when the client hangs up, which is
//! recorded as for `slow`, or after a minute):
//!
//! - `huge`: a body that never ends, sent as fast as it is read.
//! - `stall`: the head of an answer, then nothing more.
//! - `drip`: the head, then one byte of body every 50 ms.
//!
//! The search text `misbehaving` lists `huge-details`, `stall-details` and
//! `drip-details`, whose details misbehave so.
//!
//! Every request's path is recorded in order (see [`Service::requests`]).

#![allow(dead_code)]

use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How long a search starting with `slow` is held before it is answered.
pub const SLOW: Duration = Duration::from_secs(10);

/// The made-up packages: name, summary, version, license.
const PACKAGES: [(&str, &str, &str, &str); 10] = [
    (
        "aurora-charts",
        "Charts that draw in the terminal",
        "2.1.0",
        "MIT",
    ),
    (
        "aurora-cli",
        "Command-line parsing with subcommands",
        "0.9.3",
        "Apache-2.0",
    ),
    ("basalt", "An embedded key-value store", "1.4.2", "MIT"),
    (
        "cobalt-http",
        "A small HTTP client",
        "3.0.1",
        "MIT OR Apache-2.0",
    ),
    ("copper-json", "A streaming JSON reader", "1.0.0", "ISC"),
    (
        "driftwood",
        "Log rotation for long-running services",
        "0.4.0",
        "MIT",
    ),
    (
        "ember-tz",
        "Time zones without a database download",
        "2.2.0",
        "Apache-2.0",
    ),
    (
        "fennel-rpc",
        "Remote procedure calls over WebSockets",
        "0.7.1",
        "MIT",
    ),
    (
        "granite-uuid",
        "UUID generation and parsing",
        "5.3.0",
        "BSD-3-Clause",
    ),
    (
        "harbor-auth",
        "Sign-in flows for desktop apps",
        "1.1.0",
        "MIT",
    ),
];

/// A running fixture service; it stops serving when dropped.
pub struct Service {
    address: SocketAddr,
    log: Arc<Log>,
    listener: Option<TcpListener>,
}

#[derive(Default)]
struct Log {
    requests: Mutex<Vec<String>>,
    abandoned: Mutex<Vec<String>>,
    /// Set once the service is dropped, so its accept loop ends.
    stopped: Mutex<bool>,
}

impl Service {
    /// Serves on a free port of 127.0.0.1.
    pub fn start() -> Service {
        Service::start_on(0).expect("a free port on 127.0.0.1")
    }

    /// Serves on `port` of 127.0.0.1 (0 for any free one).
    pub fn start_on(port: u16) -> io::Result<Service> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let address = listener.local_addr()?;
        let log = Arc::new(Log::default());
        let accepting = listener.try_clone()?;
        let serving = log.clone();
        thread::spawn(move || {
            for stream in accepting.incoming() {
                if *serving.stopped.lock().unwrap() {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let log = serving.clone();
                thread::spawn(move || {
                    let _ = serve(stream, &log);
                });
            }
        });
        Ok(Service {
            address,
            log,
            listener: Some(listener),
        })
    }

    /// The address the samples are told to use: `http://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    pub fn port(&self) -> u16 {
        self.address.port()
    }

    /// The path (with its query) of every request received so far, in
    /// order.
    pub fn requests(&self) -> Vec<String> {
        self.log.requests.lock().unwrap().clone()
    }

    /// The paths of the requests whose client hung up before they were
    /// answered.
    pub fn abandoned(&self) -> Vec<String> {
        self.log.abandoned.lock().unwrap().clone()
    }

    /// Waits up to `limit` for a request whose client hung up before it was
    /// answered; whether one came.
    pub fn wait_for_abandoned(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if !self.abandoned().is_empty() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        *self.log.stopped.lock().unwrap() = true;
        // Wakes the accept loop so it sees that it stopped.
        let _ = TcpStream::connect(self.address);
        self.listener.take();
    }
}

fn serve(mut stream: TcpStream, log: &Log) -> io::Result<()> {
    let Some(path) = read_request(&mut stream)? else {
        return Ok(());
    };
    log.requests.lock().unwrap().push(path.clone());
    let name = query_of(&path).or_else(|| path.strip_prefix("/packages/").map(decode));
    if let Some(misbehaving) = name.as_deref().and_then(Misbehaving::of) {
        if misbehaving.answer(&mut stream)? {
            log.abandoned.lock().unwrap().push(path);
        }
        return Ok(());
    }
    let (status, body) = answer(&path);
    if status == 0 {
        // A slow search: held until the client hangs up or SLOW passes.
        if hung_up_within(&mut stream, SLOW)? {
            log.abandoned.lock().unwrap().push(path);
            return Ok(());
        }
        let query = query_of(&path).unwrap_or_default();
        return respond(&mut stream, 200, &search(query.trim_start_matches("slow")));
    }
    respond(&mut stream, status, &body)
}

/// An answer that misbehaves, chosen by the start of a search text or
/// package name.
#[derive(Clone, Copy)]
enum Misbehaving {
    /// A body that never ends.
    Huge,
    /// A head, then nothing.
    Stall,
    /// A head, then a byte every 50 ms.
    Drip,
}

/// How long a misbehaving answer goes on if the client never hangs up.
const MISBEHAVING: Duration = Duration::from_secs(60);

impl Misbehaving {
    fn of(name: &str) -> Option<Misbehaving> {
        [
            ("huge", Misbehaving::Huge),
            ("stall", Misbehaving::Stall),
            ("drip", Misbehaving::Drip),
        ]
        .into_iter()
        .find_map(|(start, misbehaving)| name.starts_with(start).then_some(misbehaving))
    }

    /// Answers so on `stream`; whether the client hung up.
    fn answer(self, stream: &mut TcpStream) -> io::Result<bool> {
        stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
              Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
        )?;
        stream.flush()?;
        let deadline = Instant::now() + MISBEHAVING;
        match self {
            Misbehaving::Stall => hung_up_within(stream, MISBEHAVING),
            Misbehaving::Huge => {
                let chunk = [b' '; 64 * 1024];
                while Instant::now() < deadline {
                    let sent = write!(stream, "{:x}\r\n", chunk.len())
                        .and_then(|()| stream.write_all(&chunk))
                        .and_then(|()| stream.write_all(b"\r\n"));
                    if sent.is_err() {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Misbehaving::Drip => {
                while Instant::now() < deadline {
                    if stream
                        .write_all(b"1\r\n \r\n")
                        .and_then(|()| stream.flush())
                        .is_err()
                    {
                        return Ok(true);
                    }
                    if hung_up_within(stream, Duration::from_millis(50))? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }
}

/// Reads a request's head; its path with query, or `None` for a
/// connection that closed without one (such as the one waking the accept
/// loop).
fn read_request(stream: &mut TcpStream) -> io::Result<Option<String>> {
    let mut head = Vec::new();
    let mut buffer = [0; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            return Ok(None);
        }
        head.extend_from_slice(&buffer[..read]);
        if head.len() > 16 * 1024 {
            return Ok(None);
        }
    }
    let head = String::from_utf8_lossy(&head);
    let line = head.lines().next().unwrap_or_default();
    let mut parts = line.split(' ');
    match (parts.next(), parts.next()) {
        (Some("GET"), Some(path)) => Ok(Some(path.to_owned())),
        _ => Ok(Some(format!("unsupported: {line}"))),
    }
}

/// The status and body for `path`; status 0 for a search to hold.
fn answer(path: &str) -> (u16, String) {
    if let Some(query) = query_of(path) {
        if query.starts_with("slow") {
            return (0, String::new());
        }
        if query == "down" {
            return (
                503,
                r#"{"error": "the registry is down for maintenance"}"#.into(),
            );
        }
        if query == "misbehaving" {
            let results: Vec<String> = ["huge", "stall", "drip"]
                .iter()
                .map(|how| {
                    format!(r#"{{"name": "{how}-details", "summary": "Its details misbehave"}}"#)
                })
                .collect();
            return (200, format!(r#"{{"results": [{}]}}"#, results.join(", ")));
        }
        return (200, search(&query));
    }
    if let Some(name) = path.strip_prefix("/packages/") {
        let name = decode(name);
        return match PACKAGES.iter().find(|package| package.0 == name) {
            Some((name, summary, version, license)) => (
                200,
                format!(
                    r#"{{"name": {}, "summary": {}, "version": {}, "license": {}}}"#,
                    json(name),
                    json(summary),
                    json(version),
                    json(license)
                ),
            ),
            None => (404, r#"{"error": "no such package"}"#.into()),
        };
    }
    (404, r#"{"error": "not found"}"#.into())
}

/// The search text of a `/search?q=` path, decoded.
fn query_of(path: &str) -> Option<String> {
    let query = path.strip_prefix("/search?")?;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("q="))
        .map(decode)
}

/// The search results for `query`, as JSON.
fn search(query: &str) -> String {
    let query = query.to_lowercase();
    let results: Vec<String> = PACKAGES
        .iter()
        .filter(|package| package.0.contains(&query))
        .map(|(name, summary, ..)| {
            format!(
                r#"{{"name": {}, "summary": {}}}"#,
                json(name),
                json(summary)
            )
        })
        .collect();
    format!(r#"{{"results": [{}]}}"#, results.join(", "))
}

/// Whether the client hangs up within `limit`.
fn hung_up_within(stream: &mut TcpStream, limit: Duration) -> io::Result<bool> {
    stream.set_read_timeout(Some(Duration::from_millis(20)))?;
    let deadline = Instant::now() + limit;
    let mut buffer = [0; 64];
    while Instant::now() < deadline {
        match stream.read(&mut buffer) {
            Ok(0) => return Ok(true),
            Ok(_) => {}
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return Ok(true),
        }
    }
    Ok(false)
}

fn respond(stream: &mut TcpStream, status: u16, body: &str) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Service Unavailable",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

/// Decodes `%XX` escapes and `+` (a space) in a URL part.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => decoded.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                let hex = |byte: u8| char::from(byte).to_digit(16);
                match (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                    (Some(high), Some(low)) => {
                        decoded.push((high * 16 + low) as u8);
                        index += 2;
                    }
                    _ => decoded.push(b'%'),
                }
            }
            byte => decoded.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// `text` as a JSON string.
fn json(text: &str) -> String {
    let mut quoted = String::from('"');
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}
