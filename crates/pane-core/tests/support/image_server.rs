//! The image server of the icons samples' web images (#142): a small HTTP/1.1
//! server on a free port of 127.0.0.1, which the samples are pointed at
//! through their `imageServer` setting. It never reaches beyond this
//! computer.
//!
//! - `GET /favicon.ico`: a 16×16 PNG, the site's favicon.
//! - `GET /images/slow.png`: a 32×32 PNG, held back until the test calls
//!   [`ImageServer::release`] (or the client hangs up, or a minute passes).
//! - `GET /images/missing.png`, and any other path: `404 Not Found`.
//! - `GET /images/page.png`: a web page, not an image.
//!
//! Every request's path is recorded in order (see
//! [`ImageServer::requests`]).

#![allow(dead_code)]

use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How long the slow image is held back at most.
const HELD: Duration = Duration::from_secs(60);

/// A running image server; it stops serving when dropped.
pub struct ImageServer {
    address: SocketAddr,
    shared: Arc<Shared>,
    listener: Option<TcpListener>,
}

#[derive(Default)]
struct Shared {
    requests: Mutex<Vec<String>>,
    /// Set once the slow image may be answered.
    released: Mutex<bool>,
    release: Condvar,
    /// Set once the server is dropped, so its accept loop ends.
    stopped: Mutex<bool>,
}

/// A `side`×`side` PNG of one colour (`rgba`).
pub fn png(side: u32, rgba: [u8; 4]) -> Vec<u8> {
    let pixels: Vec<u8> = (0..side * side).flat_map(|_| rgba).collect();
    pane_core::icons::encode_png(side, side, &pixels).expect("a PNG")
}

impl ImageServer {
    /// Serves on a free port of 127.0.0.1.
    pub fn start() -> ImageServer {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a free port on 127.0.0.1");
        let address = listener.local_addr().unwrap();
        let shared = Arc::new(Shared::default());
        let accepting = listener.try_clone().unwrap();
        let serving = shared.clone();
        thread::spawn(move || {
            for stream in accepting.incoming() {
                if *serving.stopped.lock().unwrap() {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let shared = serving.clone();
                thread::spawn(move || {
                    let _ = serve(stream, &shared);
                });
            }
        });
        ImageServer {
            address,
            shared,
            listener: Some(listener),
        }
    }

    /// The address the samples are told to use: `http://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// `127.0.0.1:<port>`.
    pub fn address(&self) -> String {
        self.address.to_string()
    }

    /// Lets the slow image be answered, now and from now on.
    pub fn release(&self) {
        *self.shared.released.lock().unwrap() = true;
        self.shared.release.notify_all();
    }

    /// The path of every request received so far, in order.
    pub fn requests(&self) -> Vec<String> {
        self.shared.requests.lock().unwrap().clone()
    }

    /// How many requests for `path` were received so far.
    pub fn count(&self, path: &str) -> usize {
        self.requests().iter().filter(|seen| *seen == path).count()
    }

    /// Waits up to `limit` until a request for `path` came; whether one
    /// did.
    pub fn wait_for(&self, path: &str, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if self.count(path) > 0 {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }
}

impl Drop for ImageServer {
    fn drop(&mut self) {
        *self.shared.stopped.lock().unwrap() = true;
        // Lets a held request go, and wakes the accept loop so it sees that
        // it stopped.
        self.release();
        let _ = TcpStream::connect(self.address);
        self.listener.take();
    }
}

fn serve(mut stream: TcpStream, shared: &Shared) -> io::Result<()> {
    let Some(path) = read_request(&mut stream)? else {
        return Ok(());
    };
    shared.requests.lock().unwrap().push(path.clone());
    match path.as_str() {
        "/favicon.ico" => respond(&mut stream, 200, "image/png", &png(16, [229, 72, 77, 255])),
        "/images/slow.png" => {
            let deadline = Instant::now() + HELD;
            let mut released = shared.released.lock().unwrap();
            while !*released && Instant::now() < deadline {
                released = shared
                    .release
                    .wait_timeout(released, Duration::from_millis(20))
                    .unwrap()
                    .0;
                drop(released);
                if hung_up(&mut stream)? {
                    return Ok(());
                }
                released = shared.released.lock().unwrap();
            }
            drop(released);
            respond(&mut stream, 200, "image/png", &png(32, [0, 144, 255, 255]))
        }
        "/images/page.png" => respond(
            &mut stream,
            200,
            "text/html",
            b"<!doctype html><html><body>Not an image</body></html>",
        ),
        _ => respond(&mut stream, 404, "text/plain", b"not found"),
    }
}

/// Reads a request's head; its path, or `None` for a connection that
/// closed without one (such as the one waking the accept loop).
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

/// Whether the client hung up, checked without waiting long.
fn hung_up(stream: &mut TcpStream) -> io::Result<bool> {
    stream.set_read_timeout(Some(Duration::from_millis(1)))?;
    let mut buffer = [0; 64];
    match stream.read(&mut buffer) {
        Ok(0) => Ok(true),
        Ok(_) => Ok(false),
        Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
            Ok(false)
        }
        Err(_) => Ok(true),
    }
}

fn respond(stream: &mut TcpStream, status: u16, kind: &str, body: &[u8]) -> io::Result<()> {
    let reason = if status == 200 { "OK" } else { "Not Found" };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}
