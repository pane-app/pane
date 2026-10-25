//! One Pane at a time (ADR 0052, #542): a second start of Pane while one
//! runs shows the running Pane's launcher and exits, as Raycast does,
//! rather than starting a second Pane with a second keyboard hook, tray
//! icon and set of hotkeys.
//!
//! The running Pane holds the **instance lock** for as long as it runs, and
//! listens on the **instance endpoint**, which only the same user can open:
//!
//! - On Windows, the named mutex `Local\pane-instance-<key>`, owned by
//!   Pane's main thread (`Local\` is the user's sign-in session), and the
//!   named pipe `\\.\pipe\pane-instance-<the user's SID>-<session>-<key>`,
//!   with the local channel's protected security descriptor. Pipe names are
//!   shared by every session, so the pipe's name names the session too.
//! - On macOS and Linux, an exclusive `flock` on `instance-<key>.lock` and
//!   the Unix-domain socket `instance-<key>`, both in the folder the local
//!   channel's socket is in: `pane` in `$XDG_RUNTIME_DIR`, else `pane-<uid>`
//!   in the temporary folder, the user's alone.
//!
//! The system lets go of the lock when Pane ends, however it ends, so a
//! Pane that crashed never keeps the next one from starting. `<key>` names
//! Pane's data folder ([`key`]): a development Pane with a data folder of
//! its own (`PANE_DATA_DIR`, as the tests and smokes give) runs beside the
//! installed one, and two Panes never share one data folder.
//!
//! A second start ([`start`]) sends one JSON line, a [`Request`] (an empty
//! line for a quiet start), and waits for the running Pane to answer `ok`
//! before it exits. The running Pane's answer comes from a thread of its
//! own, so even a busy window answers; the window then shows the launcher,
//! with the install preview a second `pane --install` asked for. A Pane
//! that is quitting no longer answers but holds the lock until the system
//! has ended it, so a start tries the lock again while it waits. The start
//! of a restart into an update (ADR 0050) takes the lock only after the
//! Pane it replaces has exited.
//!
//! This is not the local channel (`pane-ext`'s, see
//! [`crate::local_channel`]), whose endpoint a test or a development Pane
//! may move with `PANE_CHANNEL`; the two only share the platform code.

use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::local_channel::platform as channel;

/// How long a second start waits for the running Pane to answer: the
/// running Pane may itself be starting, between taking the lock and
/// listening.
pub const ANSWER_LIMIT: Duration = Duration::from_secs(10);

/// How often a second start tries the endpoint again while nobody listens
/// there yet.
const RETRY: Duration = Duration::from_millis(100);

/// What a second `pane --install …` asks the running Pane to show for
/// installation, as the first start shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Install {
    /// The package in `folder`.
    Folder { folder: PathBuf },
    /// The extension `id` of the collection at `folder` (ADR 0044).
    Collection { folder: PathBuf, id: String },
    /// `<name>` or `<name>@<version>` from npm.
    Npm { spec: String },
    /// `<repository>[@<reference>]`, with `#<id>` naming one extension of a
    /// collection the repository holds.
    Git { spec: String },
}

impl Install {
    /// The same, with a folder made absolute: the running Pane was started
    /// in another folder than the second start.
    pub fn absolute(self) -> Install {
        let absolute = |folder: PathBuf| std::path::absolute(&folder).unwrap_or(folder);
        match self {
            Install::Folder { folder } => Install::Folder {
                folder: absolute(folder),
            },
            Install::Collection { folder, id } => Install::Collection {
                folder: absolute(folder),
                id,
            },
            other => other,
        }
    }
}

/// What a second start asks of the running Pane.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case")]
pub enum Request {
    /// Show the launcher, with `install` shown for installation if named.
    Show {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        install: Option<Install>,
    },
}

/// The name a data folder gives the lock and the endpoint: a hash of its
/// absolute path (letter case ignored on Windows), the same in every
/// version of Pane, as FNV-1a is.
pub fn key(data: &Path) -> String {
    let path = std::path::absolute(data).unwrap_or_else(|_| data.to_path_buf());
    let mut text = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        text = text.to_lowercase();
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Whether this start is the one Pane ([`claim`]).
pub enum Claim {
    /// No other Pane runs on this data folder: this one holds the lock.
    First(Instance),
    /// Another Pane runs on it: this start asks it ([`ask`]) and exits.
    Running,
}

/// The lock, held until dropped (or until the process ends).
pub struct Instance {
    _lock: platform::Lock,
    endpoint: PathBuf,
}

/// Takes the instance lock of the data folder `key` names, unless another
/// Pane holds it. On Windows the calling thread owns it, so call this from
/// the thread that lasts as long as Pane: its main thread.
pub fn claim(key: &str) -> io::Result<Claim> {
    let endpoint = platform::endpoint(key)?;
    Ok(match platform::Lock::take(key, &endpoint)? {
        Some(lock) => Claim::First(Instance {
            _lock: lock,
            endpoint,
        }),
        None => Claim::Running,
    })
}

/// What later starts asked of this Pane.
pub struct Requests {
    asked: UnboundedReceiver<Request>,
    _stop: tokio::sync::oneshot::Sender<()>,
}

impl Requests {
    /// The next request, or `None` once Pane stopped listening.
    pub async fn next(&mut self) -> Option<Request> {
        self.asked.recv().await
    }
}

impl Instance {
    /// Listens on the instance endpoint, on a thread of its own, until the
    /// returned [`Requests`] are dropped. Each request is answered as it
    /// arrives, before the window acts on it.
    pub fn listen(&self) -> io::Result<Requests> {
        let (sender, asked) = unbounded_channel::<Request>();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let (bound, binding) = std::sync::mpsc::sync_channel(1);
        let path = self.endpoint.clone();
        std::thread::Builder::new()
            .name("pane-instance".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .enable_time()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = bound.send(Err(error));
                        return;
                    }
                };
                runtime.block_on(async move {
                    let listener = match channel::Listener::bind(&path) {
                        Ok(listener) => listener,
                        Err(error) => {
                            let _ = bound.send(Err(error));
                            return;
                        }
                    };
                    let _ = bound.send(Ok(()));
                    let accepting = std::pin::pin!(accept(listener, sender));
                    let _ = futures::future::select(accepting, stopped).await;
                });
            })?;
        binding
            .recv()
            .map_err(|_| io::Error::other("the instance endpoint's thread ended"))??;
        Ok(Requests { asked, _stop: stop })
    }
}

async fn accept(mut listener: channel::Listener, asked: UnboundedSender<Request>) {
    loop {
        match listener.accept().await {
            Ok(stream) => {
                tokio::spawn(answer(stream, asked.clone()));
            }
            Err(error) => {
                crate::diagnostic!("pane: a second start could not reach this Pane: {error}");
                tokio::time::sleep(RETRY).await;
            }
        }
    }
}

/// Reads one request from a second start, answers `ok`, and passes the
/// request on. A request this Pane does not know (a later Pane's) still
/// shows the launcher: that much every second start asks.
async fn answer<S>(stream: S, asked: UnboundedSender<Request>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + 'static,
{
    let (read, mut write) = tokio::io::split(stream);
    let mut read = tokio::io::BufReader::new(read);
    let mut line = String::new();
    match read.read_line(&mut line).await {
        Ok(0) | Err(_) => return,
        Ok(_) => {}
    }
    if let Some(request) = parse(&line) {
        let _ = asked.send(request);
    }
    let _ = write.write_all(b"ok\n").await;
    let _ = write.flush().await;
    // The connection stays until the second start has read the answer and
    // closed it, so that the answer is never lost with the connection.
    let mut rest = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), read.read_line(&mut rest)).await;
}

/// The request `line` says: `None` for an empty line (a quiet start only
/// asking whether a Pane answers), and a plain [`Request::Show`] for one
/// this Pane does not know.
fn parse(line: &str) -> Option<Request> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    Some(serde_json::from_str(line).unwrap_or(Request::Show { install: None }))
}

/// What became of a start ([`start`]).
pub enum Started {
    /// This Pane holds the lock.
    First(Instance),
    /// The Pane holding the lock answered, and was asked what this start
    /// asks: this start exits.
    Answered,
    /// A Pane held the lock throughout the wait without answering: this
    /// one runs without the lock, rather than leave the user with no Pane.
    Unanswered(io::Error),
}

/// Takes the instance lock of the data folder `key` names, or asks the Pane
/// that holds it to do `request` (`None`: only whether it answers, for a
/// quiet start), trying both again for up to `limit`. The Pane holding the
/// lock may be starting, before it listens, or quitting: it no longer
/// listens while the system ends it, which may take a few seconds, and the
/// lock is free once it has ended. On Windows the calling thread owns the
/// lock (see [`claim`]).
pub fn start(key: &str, request: Option<&Request>, limit: Duration) -> io::Result<Started> {
    let endpoint = platform::endpoint(key)?;
    let mut line = match request {
        Some(request) => serde_json::to_string(request).map_err(io::Error::other)?,
        None => String::new(),
    };
    line.push('\n');
    let deadline = Instant::now() + limit;
    loop {
        if let Claim::First(instance) = claim(key)? {
            return Ok(Started::First(instance));
        }
        if request.is_some() {
            platform::allow_foreground();
        }
        let left = deadline.saturating_duration_since(Instant::now());
        match send_within(&endpoint, &line, left.max(RETRY)) {
            Ok(()) => return Ok(Started::Answered),
            Err(error) if Instant::now() >= deadline => return Ok(Started::Unanswered(error)),
            Err(_) => std::thread::sleep(RETRY),
        }
    }
}

/// [`send`], waited for at most `limit`, on a thread of its own: a Pane
/// that takes the connection and never answers is not waited for past it.
fn send_within(endpoint: &Path, line: &str, limit: Duration) -> io::Result<()> {
    let (answered, answer) = std::sync::mpsc::sync_channel(1);
    let (endpoint, line) = (endpoint.to_path_buf(), line.to_owned());
    std::thread::Builder::new()
        .name("pane-instance-ask".into())
        .spawn(move || {
            let _ = answered.send(send(&endpoint, &line));
        })?;
    answer.recv_timeout(limit).map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "the running Pane took the request and did not answer",
        )
    })?
}

/// Sends `line` to the endpoint at `path` and reads the answer.
fn send(path: &Path, line: &str) -> io::Result<()> {
    let mut stream = platform::connect(path)?;
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer)?;
    if answer.trim() == "ok" {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "the running Pane answered `{}`",
            answer.trim()
        )))
    }
}

#[cfg(windows)]
mod platform {
    use std::io;
    use std::path::{Path, PathBuf};

    use ::windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
    use ::windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use ::windows::Win32::System::Threading::{
        CreateMutexW, GetCurrentProcessId, ReleaseMutex, WaitForSingleObject,
    };
    use ::windows::core::HSTRING;

    use crate::atomic::owner_only::user_sid;

    pub(super) fn endpoint(key: &str) -> io::Result<PathBuf> {
        let mut session = 0u32;
        // SAFETY: `session` is writable.
        unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }
            .map_err(io::Error::other)?;
        Ok(PathBuf::from(format!(
            r"\\.\pipe\pane-instance-{}-{session}-{key}",
            user_sid()?
        )))
    }

    /// The named mutex, owned by the thread that took it.
    pub(super) struct Lock(HANDLE);

    // SAFETY: the handle is only closed, from whichever thread drops it.
    unsafe impl Send for Lock {}

    impl Lock {
        /// The mutex `key` names, if no other thread owns it. A mutex whose
        /// owner ended without letting go is abandoned, and taken.
        pub(super) fn take(key: &str, _endpoint: &Path) -> io::Result<Option<Lock>> {
            let name = HSTRING::from(format!(r"Local\pane-instance-{key}"));
            // SAFETY: a plain call with a valid name; the handle is closed
            // below or on drop.
            let handle = unsafe { CreateMutexW(None, false, &name) }.map_err(io::Error::other)?;
            // SAFETY: a valid handle.
            let waited = unsafe { WaitForSingleObject(handle, 0) };
            if waited == WAIT_OBJECT_0 || waited == WAIT_ABANDONED {
                Ok(Some(Lock(handle)))
            } else {
                // SAFETY: opened above, closed once here.
                let _ = unsafe { CloseHandle(handle) };
                Ok(None)
            }
        }
    }

    impl Drop for Lock {
        fn drop(&mut self) {
            // SAFETY: the handle is valid; releasing fails harmlessly on a
            // thread that does not own it, and the handle is closed once.
            unsafe {
                let _ = ReleaseMutex(self.0);
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub(super) fn connect(path: &Path) -> io::Result<std::fs::File> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
    }

    /// Lets the running Pane take the foreground this start was given (the
    /// user just started it), which Windows otherwise refuses a process in
    /// the background.
    pub(super) fn allow_foreground() {
        use ::windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
        // SAFETY: a plain call.
        let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
    }
}

#[cfg(unix)]
mod platform {
    use std::fs::File;
    use std::io;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};

    use crate::local_channel::platform as channel;

    pub(super) fn endpoint(key: &str) -> io::Result<PathBuf> {
        Ok(channel::for_this_user()?.with_file_name(format!("instance-{key}")))
    }

    /// The lock file, locked until it is closed.
    pub(super) struct Lock {
        _file: File,
    }

    impl Lock {
        /// The lock file beside `endpoint`, if no other process holds it.
        pub(super) fn take(key: &str, endpoint: &Path) -> io::Result<Option<Lock>> {
            if let Some(folder) = channel::folder_of(endpoint) {
                channel::own_folder(folder)?;
            }
            let path = endpoint.with_file_name(format!("instance-{key}.lock"));
            // Opened close-on-exec, as Rust opens every file: the helpers
            // Pane starts never inherit the lock.
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(path)?;
            // SAFETY: a plain system call on a descriptor this owns.
            let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if locked == 0 {
                return Ok(Some(Lock { _file: file }));
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::WouldBlock {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }

    /// A connection to the socket at `path`, if its folder is this user's
    /// alone.
    pub(super) fn connect(path: &Path) -> io::Result<UnixStream> {
        if let Some(folder) = channel::folder_of(path)
            && let Ok(metadata) = std::fs::symlink_metadata(folder)
            && !channel::is_own(&metadata)
        {
            return Err(channel::not_own(folder));
        }
        UnixStream::connect(path)
    }

    /// macOS and X11 let the running Pane's window come forward as it is.
    pub(super) fn allow_foreground() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_tagged_json_lines() {
        let plain = Request::Show { install: None };
        let line = serde_json::to_string(&plain).unwrap();
        assert_eq!(line, r#"{"request":"show"}"#);
        assert_eq!(parse(&line), Some(plain.clone()));
        let install = Request::Show {
            install: Some(Install::Collection {
                folder: PathBuf::from("kit"),
                id: "clock".into(),
            }),
        };
        let line = serde_json::to_string(&install).unwrap();
        assert_eq!(parse(&line), Some(install));
        // A request a later Pane added still shows the launcher.
        assert_eq!(parse(r#"{"request":"open-settings"}"#), Some(plain.clone()));
        assert_eq!(parse("not json"), Some(plain));
        // A quiet start's empty line asks nothing.
        assert_eq!(
            parse(
                "
"
            ),
            None
        );
    }

    #[test]
    fn an_installed_folder_is_made_absolute() {
        let Install::Folder { folder } = (Install::Folder {
            folder: PathBuf::from("kit"),
        })
        .absolute() else {
            panic!("not a folder");
        };
        assert!(folder.is_absolute(), "{}", folder.display());
        let npm = Install::Npm {
            spec: "kit@1".into(),
        };
        assert_eq!(npm.clone().absolute(), npm);
    }

    #[test]
    fn the_key_names_the_data_folder() {
        let folder = tempfile::tempdir().unwrap();
        let data = folder.path().join("data");
        assert_eq!(key(&data), key(&data));
        assert_eq!(key(&data).len(), 16);
        assert_ne!(key(&data), key(&folder.path().join("other")));
        if cfg!(windows) {
            let upper = PathBuf::from(data.to_string_lossy().to_uppercase());
            assert_eq!(key(&data), key(&upper));
        }
    }

    /// A key of this test's own, so the tests never meet a real Pane.
    fn test_key() -> (tempfile::TempDir, String) {
        let folder = tempfile::tempdir().unwrap();
        let key = key(&folder.path().join("data"));
        (folder, key)
    }

    /// `start` on a thread of its own: another process, in Pane, and on
    /// Windows a mutex is its owning thread's again.
    fn start_elsewhere(key: &str, request: Option<Request>, limit: Duration) -> &'static str {
        let key = key.to_owned();
        std::thread::spawn(move || match start(&key, request.as_ref(), limit) {
            Ok(Started::First(_)) => "first",
            Ok(Started::Answered) => "answered",
            Ok(Started::Unanswered(_)) => "unanswered",
            Err(_) => "error",
        })
        .join()
        .unwrap()
    }

    #[test]
    fn a_second_start_finds_the_first_and_asks_it() {
        let (_folder, key) = test_key();
        let Claim::First(instance) = claim(&key).unwrap() else {
            panic!("another Pane holds a fresh key");
        };
        let mut requests = instance.listen().unwrap();
        let second = key.clone();
        let running = std::thread::spawn(move || matches!(claim(&second), Ok(Claim::Running)))
            .join()
            .unwrap();
        assert!(running, "the second start took the lock too");
        // A quiet start only learns that the Pane answers, and asks nothing.
        assert_eq!(
            start_elsewhere(&key, None, Duration::from_secs(10)),
            "answered"
        );
        let asked = Request::Show {
            install: Some(Install::Npm { spec: "kit".into() }),
        };
        let started = start_elsewhere(&key, Some(asked.clone()), Duration::from_secs(10));
        assert_eq!(started, "answered");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert_eq!(runtime.block_on(requests.next()), Some(asked));
    }

    #[test]
    fn the_lock_goes_with_the_pane_that_held_it() {
        let (_folder, key) = test_key();
        let Claim::First(instance) = claim(&key).unwrap() else {
            panic!("another Pane holds a fresh key");
        };
        drop(instance);
        assert!(matches!(claim(&key).unwrap(), Claim::First(_)));
    }

    /// A Pane that ended without letting go (it crashed, or was ended)
    /// leaves the mutex abandoned, which the next start takes: here the
    /// thread that owned it ends without dropping it.
    #[cfg(windows)]
    #[test]
    fn an_abandoned_lock_is_taken() {
        let (_folder, key) = test_key();
        let first = key.clone();
        std::thread::spawn(move || {
            let claimed = claim(&first).unwrap();
            assert!(matches!(claimed, Claim::First(_)));
            std::mem::forget(claimed);
        })
        .join()
        .unwrap();
        assert!(matches!(claim(&key).unwrap(), Claim::First(_)));
    }

    #[test]
    fn a_start_takes_the_lock_a_quitting_pane_lets_go_of() {
        let (_folder, key) = test_key();
        // A Pane quitting: it holds the lock, but no longer listens.
        let Claim::First(instance) = claim(&key).unwrap() else {
            panic!("another Pane holds a fresh key");
        };
        let ended = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            instance
        });
        let show = Some(Request::Show { install: None });
        let later = key.clone();
        let starting =
            std::thread::spawn(move || start_elsewhere(&later, show, Duration::from_secs(10)));
        // The lock goes once the quitting Pane has ended (dropped here, on
        // the thread that owns it).
        drop(ended.join().unwrap());
        assert_eq!(starting.join().unwrap(), "first");
    }

    #[test]
    fn a_pane_that_never_answers_is_not_waited_for_past_the_limit() {
        let (_folder, key) = test_key();
        let Claim::First(_instance) = claim(&key).unwrap() else {
            panic!("another Pane holds a fresh key");
        };
        let show = Some(Request::Show { install: None });
        let started = Instant::now();
        assert_eq!(
            start_elsewhere(&key, show, Duration::from_millis(500)),
            "unanswered"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
