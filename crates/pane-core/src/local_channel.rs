//! The local channel: how `pane-ext dev` reaches the running Pane (ADR 0047,
//! #217).
//!
//! Pane listens on an endpoint of the user it runs as, which no other user
//! can open:
//!
//! - On Windows, the named pipe `\\.\pipe\pane-<the user's SID>`, whose
//!   protected security descriptor lets only that user open it, and which
//!   refuses clients on other computers.
//! - On macOS and Linux, the Unix-domain socket `channel` in a folder of the
//!   user's own: `pane` in the user's runtime folder (`$XDG_RUNTIME_DIR`),
//!   else `pane-<uid>` in the temporary folder (per user on macOS). The
//!   folder is the user's alone (mode 0700; Pane does not listen in one that
//!   is not), the socket is mode 0600, and a connection from another user is
//!   closed unanswered.
//!
//! [`ENDPOINT_VARIABLE`] names another endpoint, for tests and for a second
//! Pane in development.
//!
//! What is said on it is JSON, one object a line. `pane-ext` sends
//! [`Request`]s, each with the channel's [`VERSION`]; Pane answers with
//! [`Event`]s. A connection develops one package folder:
//!
//! 1. `subscribe` asks for the package's development status and extension
//!    log, as `log` events, from the moment Pane develops it.
//! 2. `develop` hands over a build of the folder, staged in a folder of
//!    `pane-ext`'s. If Pane has not installed the folder, it shows its
//!    ordinary install preview of it, with this build, which the author
//!    confirms in Pane (`previewing`). Pane then develops the package (see
//!    `Launcher::develop_remotely`): it answers `developing` once the
//!    package runs that build, or `refused` saying why not. Each later
//!    `develop` reloads the package from its build.
//! 3. `building` and `failed` tell Pane of `pane-ext`'s later builds, so that
//!    Pane shows them as it shows its own.
//! 4. `stop`, or the connection closing (`pane-ext` stopped with Ctrl+C),
//!    ends the development, as Stop developing does.
//!
//! Pane sends `build` when the author asks it to build the package again,
//! and `ended` when the development ends otherwise: stopped in Pane, the
//! package disabled or uninstalled, or Pane quitting.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, UNIX_EPOCH};

use pane_build::BuildFailure;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::develop::PaneManifest;
use crate::extension_log::{LogLine, LogSource, LogStream};
use crate::launcher::{BuildNow, InstallPreview, Launcher, Remote};
use crate::packages::{Manifest, PackageIdentity};

/// The version of the requests this Pane answers. A request names it;
/// Pane refuses one of another version, saying so.
pub const VERSION: u32 = 1;

/// The environment variable naming another endpoint than the user's own:
/// a named pipe's name on Windows, a socket's path elsewhere.
pub const ENDPOINT_VARIABLE: &str = "PANE_CHANNEL";

/// How long Pane waits for its window to show an install preview it was
/// asked for.
const PREVIEW_LIMIT: Duration = Duration::from_secs(30);

/// How often Pane checks whether the author installed the package shown.
const PREVIEW_POLL: Duration = Duration::from_millis(100);

/// Where Pane listens: a named pipe's name on Windows, a socket's path
/// elsewhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint(PathBuf);

impl Endpoint {
    /// The endpoint at `path`.
    pub fn at(path: impl Into<PathBuf>) -> Endpoint {
        Endpoint(path.into())
    }

    /// The endpoint of the user this process runs as.
    pub fn for_this_user() -> io::Result<Endpoint> {
        platform::for_this_user().map(Endpoint)
    }

    /// The endpoint [`ENDPOINT_VARIABLE`] names, else the user's own.
    pub fn from_env() -> io::Result<Endpoint> {
        match std::env::var_os(ENDPOINT_VARIABLE).filter(|path| !path.is_empty()) {
            Some(path) => Ok(Endpoint::at(path)),
            None => Endpoint::for_this_user(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

/// What `pane-ext` asks of Pane.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case")]
pub enum Request {
    /// Develop the package in `folder` with the build staged in `staging`,
    /// which `command` built: its `pane.json`, components and helpers.
    Develop {
        folder: PathBuf,
        staging: PathBuf,
        command: String,
    },
    /// A build of the developed package began.
    Building,
    /// The build of the developed package failed: its first error, the end
    /// of what it printed, how many earlier lines it printed, and the file
    /// holding all of it.
    Failed {
        summary: String,
        output: Vec<String>,
        earlier: usize,
        log: Option<PathBuf>,
    },
    /// Send the developed package's extension log, and say when its
    /// development ends.
    Subscribe,
    /// Stop developing the package.
    Stop,
}

/// What Pane tells `pane-ext`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    /// Pane shows the install preview of the package titled `title`: the
    /// author installs it in Pane.
    Previewing { title: String },
    /// Pane develops the package with the build handed over; `replaced`
    /// says whether it replaced the package's code with it. `installed` is
    /// where the installed copy is.
    Developing {
        title: String,
        replaced: bool,
        installed: Option<PathBuf>,
    },
    /// Pane did not do what was asked, for this reason.
    Refused { message: String },
    /// The author asked Pane to build the package now, as a save would.
    Build,
    /// A line of the package's extension log: `source` is `pane`, `stdout`
    /// or `stderr`; `level` is `debug`, `info`, `warn` or `error`; `time` is
    /// in milliseconds since 1970.
    Log {
        time: u64,
        source: String,
        level: String,
        command: Option<String>,
        text: String,
    },
    /// The development ended in Pane, for this reason.
    Ended { message: String },
}

impl Event {
    /// The `log` event of `line`.
    pub fn log(line: &LogLine) -> Event {
        let time = line
            .time
            .duration_since(UNIX_EPOCH)
            .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        let source = match line.source {
            LogSource::Pane => "pane",
            LogSource::Extension(LogStream::Stdout) => "stdout",
            LogSource::Extension(LogStream::Stderr) => "stderr",
        };
        Event::Log {
            time,
            source: source.into(),
            level: line.level.name().into(),
            command: line.command.clone(),
            text: line.text.clone(),
        }
    }
}

/// `request` as the line `pane-ext` sends, with the channel's version.
pub fn request_line(request: &Request) -> String {
    let mut value = serde_json::to_value(request).expect("a request is JSON");
    value["version"] = VERSION.into();
    value.to_string()
}

/// The request on `line`, or why it is not one this Pane answers.
pub fn parse_request(line: &str) -> Result<Request, String> {
    let value: serde_json::Value = serde_json::from_str(line)
        .map_err(|error| format!("Pane could not read the request: {error}"))?;
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(version) if version == u64::from(VERSION) => {}
        Some(version) => {
            return Err(format!(
                "This Pane answers version {VERSION} of the local channel, and the request is \
                 version {version}: use a pane-ext of the same release as Pane"
            ));
        }
        None => return Err("The request names no version of the local channel".into()),
    }
    serde_json::from_value(value)
        .map_err(|error| format!("Pane does not know the request: {error}"))
}

fn event_line(event: &Event) -> String {
    serde_json::to_string(event).expect("an event is JSON")
}

/// Pane listening on its endpoint, until this is dropped.
pub struct Server {
    endpoint: Endpoint,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Server {
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

/// The folders whose install preview the window is asked to show, with the
/// window brought forward: `pane-ext dev`'s first run of a folder Pane has
/// not installed.
pub struct Previews(UnboundedReceiver<PathBuf>);

impl Previews {
    /// The next folder to preview, or `None` once Pane stopped listening.
    pub async fn next(&mut self) -> Option<PathBuf> {
        self.0.recv().await
    }
}

/// Listens on `endpoint` for `pane-ext`, developing what it asks with
/// `launcher`, on a thread of its own. Fails if Pane cannot listen there,
/// such as when another Pane already does.
pub fn serve(launcher: Launcher, endpoint: &Endpoint) -> io::Result<(Server, Previews)> {
    let (previews, asked) = unbounded_channel();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let (bound, binding) = std::sync::mpsc::sync_channel(1);
    let path = endpoint.path().to_path_buf();
    std::thread::Builder::new()
        .name("pane-channel".into())
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
                let listener = match platform::Listener::bind(&path) {
                    Ok(listener) => listener,
                    Err(error) => {
                        let _ = bound.send(Err(error));
                        return;
                    }
                };
                let _ = bound.send(Ok(()));
                let accepting = std::pin::pin!(accept(listener, launcher, previews));
                let _ = futures::future::select(accepting, stopped).await;
            });
        })?;
    binding
        .recv()
        .map_err(|_| io::Error::other("the local channel's thread ended"))?
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("Pane could not listen on {endpoint}: {error}"),
            )
        })?;
    let server = Server {
        endpoint: endpoint.clone(),
        stop: Some(stop),
    };
    Ok((server, Previews(asked)))
}

async fn accept(
    mut listener: platform::Listener,
    launcher: Launcher,
    previews: UnboundedSender<PathBuf>,
) {
    loop {
        match listener.accept().await {
            Ok(stream) => {
                tokio::spawn(connection(stream, launcher.clone(), previews.clone()));
            }
            Err(error) => {
                crate::diagnostic!("pane: the local channel could not take a connection: {error}");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

/// Serves one connection: its requests are handled in order on a thread of
/// their own, which may wait (for the author to install the package, for a
/// reload), while this task reads the next ones and notes when it closes.
async fn connection<S>(stream: S, launcher: Launcher, previews: UnboundedSender<PathBuf>)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (read, write) = tokio::io::split(stream);
    let (events, outgoing) = unbounded_channel();
    tokio::spawn(write_lines(write, outgoing));
    let (requests, incoming) = std::sync::mpsc::channel();
    let closed = Arc::new(AtomicBool::new(false));
    let handler = Handler {
        launcher,
        previews,
        events,
        closed: closed.clone(),
        developed: None,
        subscribed: false,
        following: false,
    };
    let started = std::thread::Builder::new()
        .name("pane-channel-connection".into())
        .spawn(move || handler.run(incoming));
    if started.is_err() {
        return;
    }
    let mut lines = BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if requests.send(line).is_err() {
            break;
        }
    }
    closed.store(true, Ordering::SeqCst);
}

/// Writes each line sent to `lines`, until the connection or the channel
/// closes.
async fn write_lines<W: AsyncWrite + Unpin>(mut write: W, mut lines: UnboundedReceiver<String>) {
    while let Some(mut line) = lines.recv().await {
        line.push('\n');
        if write.write_all(line.as_bytes()).await.is_err() || write.flush().await.is_err() {
            return;
        }
    }
}

/// One connection's requests, handled in order.
struct Handler {
    launcher: Launcher,
    previews: UnboundedSender<PathBuf>,
    events: UnboundedSender<String>,
    /// Set once the connection closed.
    closed: Arc<AtomicBool>,
    /// The package this connection develops.
    developed: Option<Remote>,
    /// Whether the extension log was asked for, and whether it is followed.
    subscribed: bool,
    following: bool,
}

impl Handler {
    fn run(mut self, requests: Receiver<String>) {
        for line in requests {
            match parse_request(&line) {
                Ok(request) => self.handle(request),
                Err(message) => self.send(&Event::Refused { message }),
            }
        }
        // The connection closed: the development ends, as Stop developing
        // ends it.
        self.developed = None;
    }

    fn send(&self, event: &Event) {
        let _ = self.events.send(event_line(event));
    }

    fn refuse(&self, message: String) {
        self.send(&Event::Refused { message });
    }

    fn handle(&mut self, request: Request) {
        match request {
            Request::Develop {
                folder,
                staging,
                command,
            } => self.develop(&folder, &staging, &command),
            Request::Building => {
                if let Some(remote) = &self.developed {
                    remote.building();
                }
            }
            Request::Failed {
                summary,
                output,
                earlier,
                log,
            } => {
                if let Some(remote) = &self.developed {
                    remote.failed(BuildFailure {
                        summary,
                        output,
                        earlier,
                        log,
                    });
                }
            }
            Request::Subscribe => {
                self.subscribed = true;
                self.follow(None);
            }
            Request::Stop => self.developed = None,
        }
    }

    fn develop(&mut self, folder: &Path, staging: &Path, command: &str) {
        let identity = match PackageIdentity::local(folder) {
            Ok(identity) => identity,
            Err(error) => return self.refuse(error.to_string()),
        };
        if let Some(remote) = &self.developed {
            // Ended in Pane meanwhile (stopped there, or taken over by
            // another pane-ext): it is not taken back.
            if !remote.is_current() {
                let message = format!("Pane no longer develops {}", remote.title());
                return self.refuse(message);
            }
            if *remote.identity() != identity {
                let message = format!("This connection develops {} already", remote.title());
                return self.refuse(message);
            }
            return self.deliver(staging);
        }
        let installed = self
            .launcher
            .packages()
            .iter()
            .any(|package| package.identity == identity);
        if !installed && let Err(message) = self.preview(&identity, staging) {
            return self.refuse(message);
        }
        let build: BuildNow = {
            let events = self.events.clone();
            Arc::new(move || {
                let _ = events.send(event_line(&Event::Build));
            })
        };
        let lines = match self.launcher.develop_remotely(&identity, command, build) {
            Ok((remote, lines)) => {
                self.developed = Some(remote);
                lines
            }
            Err(message) => return self.refuse(message),
        };
        self.following = false;
        self.follow(Some(lines));
        if installed {
            self.deliver(staging);
        } else {
            // The preview installed this build.
            self.developing(true);
        }
    }

    /// Reloads the developed package from the build in `staging`.
    fn deliver(&mut self, staging: &Path) {
        let Some(remote) = &mut self.developed else {
            return;
        };
        match remote.deliver(staging) {
            Ok(replaced) => self.developing(replaced),
            Err(message) => {
                self.developed = None;
                self.refuse(message);
            }
        }
    }

    fn developing(&self, replaced: bool) {
        if let Some(remote) = &self.developed {
            self.send(&Event::Developing {
                title: remote.title(),
                replaced,
                installed: remote.installed(),
            });
        }
    }

    /// Shows the install preview of the package in `identity`'s folder,
    /// with the build in `staging`, and waits for the author to install it.
    fn preview(&self, identity: &PackageIdentity, staging: &Path) -> Result<(), String> {
        let Some(folder) = identity.local_folder() else {
            return Err(format!("{identity} is not a local folder"));
        };
        // The preview shows, and installing installs, this build.
        pane_build::copy_components(&PaneManifest, staging, folder);
        let title = Manifest::read(folder)
            .map(|manifest| manifest.title)
            .unwrap_or_else(|_| identity.to_string());
        self.send(&Event::Previewing {
            title: title.clone(),
        });
        if self.previews.send(folder.to_path_buf()).is_err() {
            return Err("This Pane has no window to show the install preview in".into());
        }
        let asked = Instant::now();
        let mut shown = false;
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return Err(format!("pane-ext left before {title} was installed"));
            }
            match self.launcher.previewing(identity) {
                InstallPreview::Installed => return Ok(()),
                InstallPreview::Shown => shown = true,
                InstallPreview::Refused(why) => {
                    return Err(format!("{title} cannot be installed: {why}"));
                }
                InstallPreview::Elsewhere if shown => {
                    return Err(format!(
                        "{title} was not installed, so Pane does not develop it"
                    ));
                }
                InstallPreview::Elsewhere if asked.elapsed() > PREVIEW_LIMIT => {
                    return Err(format!("Pane did not show the install preview of {title}"));
                }
                InstallPreview::Elsewhere => {}
            }
            std::thread::sleep(PREVIEW_POLL);
        }
    }

    /// Sends each line of the developed package's extension log, once it
    /// was asked for, and `ended` when its development ends: the lines of
    /// `lines`, if given, else those from now on.
    fn follow(&mut self, lines: Option<Receiver<LogLine>>) {
        if !self.subscribed || self.following {
            return;
        }
        let Some(remote) = &self.developed else {
            return;
        };
        self.following = true;
        let lines = lines.unwrap_or_else(|| self.launcher.follow_extension_log(remote.identity()));
        let events = self.events.clone();
        let title = remote.title();
        let _ = std::thread::Builder::new()
            .name("pane-channel-log".into())
            .spawn(move || {
                for line in lines {
                    if events.send(event_line(&Event::log(&line))).is_err() {
                        return;
                    }
                }
                let message = format!("Pane stopped developing {title}");
                let _ = events.send(event_line(&Event::Ended { message }));
            });
    }
}

/// Sends requests to the Pane a [`connect`]ion reached. Cloning shares it.
#[derive(Clone, Debug)]
pub struct Sender(UnboundedSender<String>);

impl Sender {
    /// Sends `request`; false once the connection has closed.
    pub fn send(&self, request: &Request) -> bool {
        self.0.send(request_line(request)).is_ok()
    }
}

/// Connects to the Pane listening on `endpoint`: the requests are sent with
/// the [`Sender`], and Pane's events arrive in order on the receiver, which
/// ends when the connection closes. Dropping every clone of the sender, or
/// the receiver, closes it. Fails with [`io::ErrorKind::NotFound`] or
/// [`io::ErrorKind::ConnectionRefused`] when no Pane listens there.
pub fn connect(endpoint: &Endpoint) -> io::Result<(Sender, Receiver<Event>)> {
    let path = endpoint.path().to_path_buf();
    let (connected, connecting) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("pane-channel-client".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = connected.send(Err(error));
                    return;
                }
            };
            runtime.block_on(async move {
                let stream = match platform::connect(&path).await {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = connected.send(Err(error));
                        return;
                    }
                };
                let (requests, outgoing) = unbounded_channel();
                let (events, received) = std::sync::mpsc::channel();
                let _ = connected.send(Ok((Sender(requests), received)));
                let (read, write) = tokio::io::split(stream);
                let writing = std::pin::pin!(write_lines(write, outgoing));
                let reading = std::pin::pin!(read_events(read, events));
                // Whichever ends first closes the connection.
                let _ = futures::future::select(writing, reading).await;
            });
        })?;
    connecting
        .recv()
        .map_err(|_| io::Error::other("the local channel's thread ended"))?
}

/// Passes on each event Pane sends on `read`, until it closes the
/// connection or nobody receives them.
async fn read_events<R: AsyncRead + Unpin>(read: R, events: std::sync::mpsc::Sender<Event>) {
    let mut lines = BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        // An event a later Pane added is skipped.
        if let Ok(event) = serde_json::from_str::<Event>(&line)
            && events.send(event).is_err()
        {
            return;
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use ::windows::Win32::Foundation::ERROR_PIPE_BUSY;

    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };

    use crate::atomic::owner_only::{user_sid, with_attributes};

    /// How long a client waits for a pipe instance while every one is busy.
    const BUSY_LIMIT: Duration = Duration::from_secs(5);

    pub(super) fn for_this_user() -> io::Result<PathBuf> {
        Ok(PathBuf::from(format!(r"\\.\pipe\pane-{}", user_sid()?)))
    }

    /// The pipe, with an instance waiting for the next client.
    pub(super) struct Listener {
        path: PathBuf,
        /// Full control for this user, and nobody else.
        sddl: String,
        next: NamedPipeServer,
    }

    impl Listener {
        pub(super) fn bind(path: &Path) -> io::Result<Listener> {
            let sddl = format!("D:P(A;;GA;;;{})", user_sid()?);
            // The first instance: another Pane (or anyone) already
            // listening there makes this fail.
            let next = create(path, &sddl, true)?;
            Ok(Listener {
                path: path.to_path_buf(),
                sddl,
                next,
            })
        }

        /// The next client's connection, once one connects.
        pub(super) async fn accept(&mut self) -> io::Result<NamedPipeServer> {
            if let Err(error) = self.next.connect().await {
                self.next = create(&self.path, &self.sddl, false)?;
                return Err(error);
            }
            let next = create(&self.path, &self.sddl, false)?;
            Ok(std::mem::replace(&mut self.next, next))
        }
    }

    /// A new instance of the pipe at `path`, which only this computer's
    /// clients that `sddl` lets in can open.
    fn create(path: &Path, sddl: &str, first: bool) -> io::Result<NamedPipeServer> {
        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(first)
            .reject_remote_clients(true);
        with_attributes(sddl, |attributes| {
            let attributes = std::ptr::from_ref(attributes).cast_mut().cast();
            // SAFETY: valid SECURITY_ATTRIBUTES for the call, which only
            // reads them.
            unsafe { options.create_with_security_attributes_raw(path, attributes) }
        })?
    }

    pub(super) async fn connect(path: &Path) -> io::Result<NamedPipeClient> {
        let deadline = Instant::now() + BUSY_LIMIT;
        loop {
            match ClientOptions::new().open(path) {
                Err(error)
                    if error.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32)
                        && Instant::now() < deadline => {}
                opened => return opened,
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    use tokio::net::{UnixListener, UnixStream};

    /// The user this process runs as.
    fn uid() -> u32 {
        // SAFETY: getuid has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }

    pub(super) fn for_this_user() -> io::Result<PathBuf> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|folder| !folder.is_empty())
            .map(PathBuf::from)
            .filter(|folder| folder.is_dir());
        let folder = match runtime {
            Some(runtime) => runtime.join("pane"),
            None => std::env::temp_dir().join(format!("pane-{}", uid())),
        };
        Ok(folder.join("channel"))
    }

    /// The socket, until dropped: its file goes with it.
    pub(super) struct Listener {
        path: PathBuf,
        listener: UnixListener,
    }

    impl Listener {
        pub(super) fn bind(path: &Path) -> io::Result<Listener> {
            if let Some(folder) = folder_of(path) {
                own_folder(folder)?;
            }
            let listener = match UnixListener::bind(path) {
                Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
                    // A socket left by a Pane that did not quit cleanly
                    // answers nobody, and is replaced.
                    if std::os::unix::net::UnixStream::connect(path).is_ok() {
                        return Err(io::Error::new(
                            io::ErrorKind::AddrInUse,
                            "another Pane already listens there",
                        ));
                    }
                    std::fs::remove_file(path)?;
                    UnixListener::bind(path)?
                }
                bound => bound?,
            };
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            Ok(Listener {
                path: path.to_path_buf(),
                listener,
            })
        }

        /// The next connection from this user; another user's is closed
        /// unanswered.
        pub(super) async fn accept(&mut self) -> io::Result<UnixStream> {
            loop {
                let (stream, _) = self.listener.accept().await?;
                if stream.peer_cred().is_ok_and(|peer| peer.uid() == uid()) {
                    return Ok(stream);
                }
            }
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Makes `folder` (whose parent must exist) the user's alone, or says
    /// why it is not.
    fn own_folder(folder: &Path) -> io::Result<()> {
        match std::fs::DirBuilder::new().mode(0o700).create(folder) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let metadata = std::fs::symlink_metadata(folder)?;
        if !is_own(&metadata) {
            return Err(not_own(folder));
        }
        Ok(())
    }

    /// The folder the socket at `path` is in, if it names one.
    fn folder_of(path: &Path) -> Option<&Path> {
        let folder = path.parent()?;
        (!folder.as_os_str().is_empty()).then_some(folder)
    }

    /// Whether `metadata` is of a folder only this user can open.
    fn is_own(metadata: &std::fs::Metadata) -> bool {
        metadata.is_dir() && metadata.uid() == uid() && metadata.mode() & 0o077 == 0
    }

    fn not_own(folder: &Path) -> io::Error {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} is not a folder only this user can open",
                folder.display()
            ),
        )
    }

    /// A connection to the socket at `path`, if its folder is this user's
    /// alone: a folder another user made in the temporary folder first is
    /// not trusted with the builds.
    pub(super) async fn connect(path: &Path) -> io::Result<UnixStream> {
        if let Some(folder) = folder_of(path)
            && let Ok(metadata) = std::fs::symlink_metadata(folder)
            && !is_own(&metadata)
        {
            return Err(not_own(folder));
        }
        UnixStream::connect(path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_json_lines_with_the_channel_version() {
        let request = Request::Develop {
            folder: PathBuf::from("hello"),
            staging: PathBuf::from("staging"),
            command: "cargo build".into(),
        };
        let line = request_line(&request);
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["request"], "develop");
        assert_eq!(value["version"], VERSION);
        assert_eq!(value["folder"], "hello");
        assert_eq!(parse_request(&line), Ok(request));
        for request in [Request::Building, Request::Subscribe, Request::Stop] {
            assert_eq!(parse_request(&request_line(&request)), Ok(request));
        }
    }

    #[test]
    fn a_request_of_another_version_or_none_is_refused_saying_why() {
        let error = parse_request(r#"{"version": 2, "request": "stop"}"#).unwrap_err();
        assert!(error.contains("version 1"), "{error}");
        assert!(error.contains("version 2"), "{error}");
        let error = parse_request(r#"{"request": "stop"}"#).unwrap_err();
        assert!(error.contains("no version"), "{error}");
        let error = parse_request(r#"{"version": 1, "request": "publish"}"#).unwrap_err();
        assert!(error.contains("does not know"), "{error}");
        assert!(parse_request("not json").is_err());
    }

    #[test]
    fn events_are_tagged_json() {
        let event = Event::Developing {
            title: "Hello".into(),
            replaced: true,
            installed: None,
        };
        let line = event_line(&event);
        assert_eq!(
            line,
            r#"{"event":"developing","title":"Hello","replaced":true,"installed":null}"#
        );
        assert_eq!(serde_json::from_str::<Event>(&line).unwrap(), event);
        assert_eq!(event_line(&Event::Build), r#"{"event":"build"}"#);
    }

    /// A launcher that installs into `data`.
    fn launcher(data: &Path) -> Launcher {
        Launcher::with_packages(crate::Runtime::start(), Vec::new(), data.join("extensions"))
    }

    /// An endpoint of this test's own. On Unix its folder is left for Pane
    /// to make, as only this user's: a temporary folder others can enter is
    /// refused.
    fn endpoint(folder: &Path) -> Endpoint {
        if cfg!(windows) {
            let name = folder.file_name().unwrap().to_string_lossy();
            Endpoint::at(format!(r"\\.\pipe\pane-test-{name}"))
        } else {
            Endpoint::at(folder.join("pane").join("channel"))
        }
    }

    #[test]
    fn a_connection_is_answered_and_a_second_pane_cannot_listen_there() {
        let data = tempfile::tempdir().unwrap();
        let endpoint = endpoint(data.path());
        let (server, _previews) = serve(launcher(data.path()), &endpoint).unwrap();
        assert_eq!(server.endpoint(), &endpoint);
        let error = serve(launcher(data.path()), &endpoint).err().unwrap();
        assert!(
            error.to_string().contains("Pane could not listen on"),
            "{error}"
        );

        let (sender, events) = connect(&endpoint).unwrap();
        assert!(sender.send(&Request::Develop {
            folder: data.path().join("missing"),
            staging: data.path().join("staging"),
            command: "cargo build".into(),
        }));
        let event = events.recv_timeout(Duration::from_secs(30)).unwrap();
        assert!(matches!(event, Event::Refused { .. }), "{event:?}");

        // Once Pane stops listening, nobody answers there.
        drop(server);
        let deadline = Instant::now() + Duration::from_secs(30);
        while connect(&endpoint).is_ok() {
            assert!(Instant::now() < deadline, "still listening");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_socket_is_open_to_this_user_only() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let data = tempfile::tempdir().unwrap();
        let folder = data.path().join("pane");
        let endpoint = Endpoint::at(folder.join("channel"));
        let (_server, _previews) = serve(launcher(data.path()), &endpoint).unwrap();
        let socket = std::fs::metadata(endpoint.path()).unwrap();
        assert_eq!(socket.permissions().mode() & 0o777, 0o600);
        let made = std::fs::metadata(&folder).unwrap();
        assert_eq!(made.mode() & 0o777, 0o700);
        // SAFETY: getuid has no preconditions.
        assert_eq!(made.uid(), unsafe { libc::getuid() });

        // A folder others can enter is not listened in.
        let open = data.path().join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        let error = serve(launcher(data.path()), &Endpoint::at(open.join("channel")))
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
    }

    #[cfg(windows)]
    #[test]
    fn the_pipe_is_open_to_this_user_only() {
        use std::os::windows::io::AsRawHandle;

        use ::windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
        use ::windows::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
            SE_KERNEL_OBJECT,
        };
        use ::windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
        use ::windows::core::PWSTR;

        let data = tempfile::tempdir().unwrap();
        let endpoint = endpoint(data.path());
        let (_server, _previews) = serve(launcher(data.path()), &endpoint).unwrap();
        let pipe = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(endpoint.path())
            .unwrap();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: an open handle; the descriptor is freed below.
        let status = unsafe {
            GetSecurityInfo(
                HANDLE(pipe.as_raw_handle()),
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                Some(&raw mut descriptor),
            )
        };
        assert!(status.is_ok(), "{status:?}");
        let mut text = PWSTR::null();
        // SAFETY: a valid descriptor; `text` is freed below.
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                None,
            )
        }
        .unwrap();
        // SAFETY: a NUL-terminated string the call allocated.
        let sddl = unsafe { text.to_string() }.unwrap();
        // SAFETY: both were allocated with LocalAlloc by the calls above.
        unsafe {
            LocalFree(Some(HLOCAL(text.0.cast())));
            LocalFree(Some(HLOCAL(descriptor.0)));
        }
        let mut user = crate::atomic::owner_only::user_sid().unwrap();
        // SDDL names the built-in Administrator account by its alias.
        if user.starts_with("S-1-5-21-") && user.ends_with("-500") {
            user = "LA".into();
        }
        assert!(sddl.starts_with("D:P"), "{sddl}");
        let entries: Vec<&str> = sddl["D:P".len()..]
            .trim_matches(['(', ')'])
            .split(")(")
            .collect();
        assert_eq!(entries.len(), 1, "{sddl}");
        assert!(entries[0].starts_with("A;"), "{sddl}");
        assert!(entries[0].ends_with(&format!(";;;{user}")), "{sddl}");
    }
}
