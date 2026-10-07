//! Guests' web requests: `wasi:http@0.3.0`'s `client.send`, which Pane
//! sends for them.
//!
//! Wasmtime's `wasi:http` implementation turns a guest's request into an
//! [`http::Request`] and hands it to [`Sender::send_request`], which connects
//! (TLS for `https`, trusting the system's certificates), sends it over
//! HTTP/1.1 and hands back the response as it arrives. Everything runs on the
//! runtime thread, inside the guest call that asked for it: when that call
//! stops (its generation ended, or the launcher cancelled a search the user
//! replaced), the instance and its store are dropped, and with them the
//! connection, so an abandoned request goes no further.
//!
//! Pane bounds every request, whatever the guest asks ([`HttpLimits`]):
//! connecting, the response's head, the wait between two pieces of its
//! body, the whole request, and the body's size, so a service that stalls
//! or answers endlessly ends the request with an error the guest receives
//! (a `wasi:http` error code), not a hung or crashed call. A package has at
//! most a few connections open at once, and Pane notes the hosts each
//! package tried to reach this session ([`Network::contacted`]). Redirects
//! are not followed: a `3xx` is the guest's response.
//!
//! Code whose generation has ended cannot start a request, like the other
//! host imports it may not use any more. Otherwise extensions are trusted
//! code: any `http` or `https` address is allowed, as their own native code
//! would be, including this computer's own services (`localhost`), the
//! local network and link-local addresses such as `169.254.169.254`.

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use bytes::Bytes;
use http::uri::Scheme;
use http_body::{Body, Frame};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{Instant, Sleep, sleep_until, timeout_at};
use wasmtime_wasi_http::io::TokioIo;
use wasmtime_wasi_http::{Error, RequestOptions, WasiBody, WasiHttpHooks};

use crate::extension_data::PackageData;
use crate::runtime::{Watch, lock};

/// The ceilings Pane puts on each web request of a guest, whatever timeouts
/// the guest asks for: a guest may ask for shorter ones, never longer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[doc(hidden)]
pub struct HttpLimits {
    /// Connecting, the TLS handshake included.
    pub connect: Duration,
    /// The response's head, once the request is sent.
    pub first_byte: Duration,
    /// The wait for the next piece of the response's body.
    pub between_bytes: Duration,
    /// The whole request, from sending it to the end of its response.
    pub deadline: Duration,
    /// The response body's size, in bytes.
    pub body: u64,
    /// Connections one package may have open at once.
    pub connections: usize,
}

impl Default for HttpLimits {
    fn default() -> HttpLimits {
        HttpLimits {
            connect: Duration::from_secs(10),
            first_byte: Duration::from_secs(20),
            between_bytes: Duration::from_secs(10),
            deadline: Duration::from_secs(30),
            body: 4 * 1024 * 1024,
            connections: 4,
        }
    }
}

/// The runtime's web requests, shared by its threads (a restarted one
/// included): their limits, and what each package did this session.
#[derive(Default)]
pub(crate) struct Network {
    limits: Mutex<HttpLimits>,
    /// By package, the key of its identity (`""` for commands built into
    /// Pane).
    packages: Mutex<HashMap<String, PackageUse>>,
}

/// What one package did on the network this session.
struct PackageUse {
    /// `host:port` of every address it tried to reach.
    contacted: BTreeSet<String>,
    /// Its connections open now, one permit each.
    connections: Arc<Semaphore>,
}

impl Network {
    pub(crate) fn limits(&self) -> HttpLimits {
        *lock(&self.limits)
    }

    /// Sets the limits of requests started from now on. A package's
    /// connection limit is set when it first connects. For tests only.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn set_limits(&self, limits: HttpLimits) {
        *lock(&self.limits) = limits;
    }

    /// `host:port` of every address the package with key `owner` tried to
    /// reach this session, sorted.
    pub(crate) fn contacted(&self, owner: &str) -> Vec<String> {
        lock(&self.packages)
            .get(owner)
            .map(|used| used.contacted.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Notes that Pane reached `address` on behalf of the package `owner`
    /// (a web image one of its icons names, #142): the package's network
    /// use lists it with the addresses its own requests reached.
    pub(crate) fn note_contacted(&self, owner: &str, address: String) {
        let connections = self.limits().connections;
        lock(&self.packages)
            .entry(owner.to_owned())
            .or_insert_with(|| PackageUse {
                contacted: BTreeSet::new(),
                connections: Arc::new(Semaphore::new(connections)),
            })
            .contacted
            .insert(address);
    }

    /// Notes that the package `owner` is reaching `address` and takes one of
    /// its connections, given back when the permit is dropped; refused while
    /// all are open.
    fn connect(&self, owner: &str, address: String) -> Result<OwnedSemaphorePermit, Error> {
        let connections = self.limits().connections;
        let mut packages = lock(&self.packages);
        let used = packages
            .entry(owner.to_owned())
            .or_insert_with(|| PackageUse {
                contacted: BTreeSet::new(),
                connections: Arc::new(Semaphore::new(connections)),
            });
        used.contacted.insert(address);
        used.connections
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::ConnectionLimitReached)
    }
}

/// The rest of a response being received, once its head has arrived: it
/// completes when the connection finishes, with why if it failed.
type Receiving = Box<dyn Future<Output = Result<(), Error>> + Send>;

/// What sending a request resolves to.
type Sent = Box<dyn Future<Output = Result<(http::Response<WasiBody>, Receiving), Error>> + Send>;

/// Sends one guest instance's requests. It knows the instance's extension
/// data to refuse stopped code and to tell whose requests they are, and the
/// runtime thread running it, whose fence stops a built-in command's code
/// and where sending is marked as host work.
pub(crate) struct Sender {
    data: Option<PackageData>,
    network: Arc<Network>,
    watch: Arc<Watch>,
}

impl Sender {
    pub(crate) fn new(
        data: Option<PackageData>,
        network: Arc<Network>,
        watch: Arc<Watch>,
    ) -> Sender {
        Sender {
            data,
            network,
            watch,
        }
    }

    /// Whether the code sending is stopped: its generation ended, or Pane
    /// gave up on its runtime thread (the check every host interface makes,
    /// `runtime::code_stopped`).
    fn stopped(&self) -> bool {
        crate::runtime::code_stopped(self.data.as_ref(), self.watch.fence()).is_some()
    }
}

impl WasiHttpHooks for Sender {
    fn send_request(
        &mut self,
        request: http::Request<WasiBody>,
        options: Option<RequestOptions>,
        // Where Wasmtime reports whether the guest read the response through;
        // nothing here needs it.
        _received: Box<dyn Future<Output = Result<(), Error>> + Send>,
    ) -> Sent {
        if self.stopped() {
            // Stopped code starts no more work.
            return Box::new(async { Err(Error::HttpRequestDenied) });
        }
        let owner = self
            .data
            .as_ref()
            .map_or_else(String::new, |data| data.owner().to_owned());
        let (network, watch) = (self.network.clone(), self.watch.clone());
        // On its generation's undo list until its response is received: the
        // generation's end stops it at once, wherever it waits.
        let (undo, mut aborted) = match &self.data {
            Some(data) => {
                let (abort, aborted) = tokio::sync::oneshot::channel::<()>();
                let undo = data.generation().on_end("web request", move || {
                    let _ = abort.send(());
                    Ok(())
                });
                (Some(undo), Some(aborted))
            }
            None => (None, None),
        };
        // Sending is host work: its polls are not the guest's computing, nor
        // are those receiving the response (see `send`).
        Box::new(crate::runtime::deadlines::hosted(
            self.watch.clone(),
            async move {
                let limits = network.limits();
                let target = Target::of(&request)?;
                let permit = network.connect(&owner, target.address())?;
                let ceilings = Ceilings::new(limits, options);
                let sending = send(request, target, ceilings, permit, watch);
                let Some(sent) = unless_aborted(sending, aborted.as_mut()).await else {
                    return Err(Error::HttpRequestDenied);
                };
                let (response, receiving) = sent?;
                let receiving: Receiving = Box::new(async move {
                    let _undo = undo;
                    unless_aborted(Box::into_pin(receiving), aborted.as_mut())
                        .await
                        .unwrap_or(Err(Error::HttpRequestDenied))
                });
                Ok((response, receiving))
            },
        ))
    }
}

/// `work`'s output, or `None` if `aborted` (a request's generation ended,
/// see [`Sender::send_request`]) resolves first; a command built into Pane
/// has none, and is never aborted.
async fn unless_aborted<T>(
    work: impl Future<Output = T>,
    aborted: Option<&mut tokio::sync::oneshot::Receiver<()>>,
) -> Option<T> {
    let mut work = pin!(work);
    let mut aborted = aborted;
    std::future::poll_fn(|cx| {
        if let Some(aborted) = aborted.as_mut()
            && Pin::new(&mut **aborted).poll(cx).is_ready()
        {
            return Poll::Ready(None);
        }
        work.as_mut().poll(cx).map(Some)
    })
    .await
}

/// Where a request goes.
struct Target {
    tls: bool,
    host: String,
    port: u16,
}

impl Target {
    fn of(request: &http::Request<WasiBody>) -> Result<Target, Error> {
        Target::of_uri(request.uri())
    }

    fn of_uri(uri: &http::Uri) -> Result<Target, Error> {
        let tls = uri.scheme() == Some(&Scheme::HTTPS);
        let authority = uri.authority().ok_or(Error::HttpRequestUriInvalid)?;
        let host = authority
            .host()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned();
        let port = authority.port_u16().unwrap_or(if tls { 443 } else { 80 });
        Ok(Target { tls, host, port })
    }

    /// `host:port`, an IPv6 host in brackets.
    fn address(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// When each part of one request must be done by.
#[derive(Clone, Copy)]
struct Ceilings {
    connect: Duration,
    first_byte: Duration,
    between_bytes: Duration,
    body: u64,
    /// When the whole request must be done.
    deadline: Instant,
}

impl Ceilings {
    /// `limits`, or the guest's shorter `options`, from now.
    fn new(limits: HttpLimits, options: Option<RequestOptions>) -> Ceilings {
        let shorter = |asked: Option<Duration>, limit: Duration| {
            asked.map_or(limit, |asked| asked.min(limit))
        };
        let options = options.unwrap_or_default();
        Ceilings {
            connect: shorter(options.connect_timeout, limits.connect),
            first_byte: shorter(options.first_byte_timeout, limits.first_byte),
            between_bytes: shorter(options.between_bytes_timeout, limits.between_bytes),
            body: limits.body,
            deadline: Instant::now() + limits.deadline,
        }
    }

    /// `after` from now, or the deadline if that comes first.
    fn within(&self, after: Duration) -> Instant {
        (Instant::now() + after).min(self.deadline)
    }
}

/// A connection, plain or TLS.
trait Connection: AsyncRead + AsyncWrite + Send + Sync + Unpin + 'static {}
impl<T: AsyncRead + AsyncWrite + Send + Sync + Unpin + 'static> Connection for T {}

/// A connection's sending half, and the connection, which does the reading
/// and writing while it is driven.
type Connected<B> = (
    hyper::client::conn::http1::SendRequest<B>,
    hyper::client::conn::http1::Connection<TokioIo<Box<dyn Connection>>, B>,
);

/// Connects to `target` (TLS for `https`, trusting the system's
/// certificates) and starts HTTP/1.1 on the connection, by `by`.
async fn connect<B>(target: &Target, by: Instant) -> Result<Connected<B>, Error>
where
    B: Body + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let connecting = TcpStream::connect((target.host.as_str(), target.port));
    let stream = match timeout_at(by, connecting).await {
        Ok(stream) => stream.map_err(Error::Connect)?,
        Err(_) => return Err(Error::ConnectionTimeout),
    };
    let stream: Box<dyn Connection> = if target.tls {
        let config = tls_config().map_err(|problem| Error::InternalError(Some(problem)))?;
        let name = rustls::pki_types::ServerName::try_from(target.host.clone())
            .map_err(|_| Error::HttpRequestUriInvalid)?;
        let connecting = tokio_rustls::TlsConnector::from(config).connect(name, stream);
        match timeout_at(by, connecting).await {
            Ok(stream) => Box::new(stream.map_err(tls_error)?),
            Err(_) => return Err(Error::ConnectionTimeout),
        }
    } else {
        Box::new(stream)
    };
    timeout_at(
        by,
        hyper::client::conn::http1::handshake(TokioIo::new(stream)),
    )
    .await
    .map_err(|_| Error::ConnectionTimeout)?
    .map_err(Error::from)
}

/// Connects to `target` and sends `request`, resolving once the response's
/// head has arrived; its body follows as it is received, within
/// `ceilings`. The connection holds `permit` until it closes. Driving the
/// connection and reading the body are host work on `watch`'s runtime
/// thread, never the guest's computing.
async fn send(
    mut request: http::Request<WasiBody>,
    target: Target,
    ceilings: Ceilings,
    permit: OwnedSemaphorePermit,
    watch: Arc<Watch>,
) -> Result<(http::Response<WasiBody>, Receiving), Error> {
    let (mut sender, connection) = connect(&target, ceilings.within(ceilings.connect)).await?;

    // The request line names only the path: the host travels in its Host
    // header, which Wasmtime set.
    let path = request
        .uri()
        .path_and_query()
        .map_or("/", |path| path.as_str())
        .to_owned();
    *request.uri_mut() = http::Uri::builder()
        .path_and_query(path)
        .build()
        .map_err(|_| Error::HttpRequestUriInvalid)?;

    let mut response = pin!(async {
        timeout_at(
            ceilings.within(ceilings.first_byte),
            sender.send_request(request),
        )
        .await
        .map_err(|_| Error::ConnectionReadTimeout)?
        .map_err(Error::from)
    });
    let mut connection = Some(connection);
    // The connection does the reading and writing: drive it while waiting
    // for the response's head.
    let response = std::future::poll_fn(|cx| match response.as_mut().poll(cx) {
        Poll::Ready(result) => Poll::Ready(result),
        Poll::Pending => {
            let Some(driving) = connection.as_mut() else {
                return Poll::Pending;
            };
            let finished = ready!(Pin::new(driving).poll(cx));
            connection = None;
            match finished {
                Ok(()) => response.as_mut().poll(cx),
                Err(error) => Poll::Ready(Err(Error::from(error))),
            }
        }
    })
    .await?;
    // A body announced as too large is refused before it is read.
    let announced = response
        .headers()
        .get(http::header::CONTENT_LENGTH)
        .and_then(|length| length.to_str().ok()?.parse::<u64>().ok());
    if announced.is_some_and(|length| length > ceilings.body) {
        return Err(Error::HttpResponseBodySize(Some(ceilings.body)));
    }
    let deadline = ceilings.deadline;
    let receiving: Receiving = Box::new(crate::runtime::deadlines::hosted(
        watch.clone(),
        async move {
            // Given back once the connection closes.
            let _permit = permit;
            let Some(connection) = connection else {
                return Ok(());
            };
            match timeout_at(deadline, connection).await {
                Ok(finished) => finished.map_err(Error::from),
                Err(_) => Err(Error::HttpResponseTimeout),
            }
        },
    ));
    Ok((
        response.map(|body| Limited::new(body, ceilings, watch).boxed_unsync()),
        receiving,
    ))
}

/// A response body read within a request's ceilings: it ends with an error
/// once it grows past the size limit, once the next piece takes too long,
/// or at the request's deadline. Reading it is host work on the runtime
/// thread `watch` follows (see [`Watch::host`]).
struct Limited {
    body: Incoming,
    watch: Arc<Watch>,
    received: u64,
    most: u64,
    between_bytes: Duration,
    /// When the next piece must have arrived.
    idle: Pin<Box<Sleep>>,
    deadline: Pin<Box<Sleep>>,
    /// Set once it ended with an error: it yields nothing more.
    failed: bool,
}

impl Limited {
    fn new(body: Incoming, ceilings: Ceilings, watch: Arc<Watch>) -> Limited {
        Limited {
            body,
            watch,
            received: 0,
            most: ceilings.body,
            between_bytes: ceilings.between_bytes,
            idle: Box::pin(sleep_until(ceilings.within(ceilings.between_bytes))),
            deadline: Box::pin(sleep_until(ceilings.deadline)),
            failed: false,
        }
    }

    fn fail(&mut self, error: Error) -> Poll<Option<Result<Frame<Bytes>, Error>>> {
        self.failed = true;
        Poll::Ready(Some(Err(error)))
    }
}

impl Body for Limited {
    type Data = Bytes;
    type Error = Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Error>>> {
        let this = &mut *self;
        if this.failed {
            return Poll::Ready(None);
        }
        // Pane's work, not the guest's computing.
        let watch = this.watch.clone();
        let _host = watch.host();
        match Pin::new(&mut this.body).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    this.received += data.len() as u64;
                    if this.received > this.most {
                        return this.fail(Error::HttpResponseBodySize(Some(this.most)));
                    }
                }
                let next = (Instant::now() + this.between_bytes).min(this.deadline.deadline());
                this.idle.as_mut().reset(next);
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(error))) => this.fail(Error::from(error)),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => {
                if this.deadline.as_mut().poll(cx).is_ready() {
                    return this.fail(Error::HttpResponseTimeout);
                }
                if this.idle.as_mut().poll(cx).is_ready() {
                    return this.fail(Error::ConnectionReadTimeout);
                }
                Poll::Pending
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        !self.failed && self.body.is_end_stream()
    }
}

/// The `wasi:http` error for a failed TLS handshake: a certificate the
/// system does not trust is told apart from the other failures.
fn tls_error(error: std::io::Error) -> Error {
    match error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    {
        Some(rustls::Error::InvalidCertificate(_) | rustls::Error::NoCertificatesPresented) => {
            Error::TlsCertificateError
        }
        Some(rustls::Error::AlertReceived(alert)) => Error::TlsAlertReceived {
            alert_id: Some(u8::from(*alert)),
            alert_message: Some(format!("{alert:?}")),
        },
        Some(_) => Error::TlsProtocolError,
        None => Error::Tls(error),
    }
}

/// TLS settings trusting the system's certificates. Read once they could
/// be; while they cannot (none could be read), each request tries again.
fn tls_config() -> Result<Arc<rustls::ClientConfig>, String> {
    static CONFIG: Mutex<Option<Arc<rustls::ClientConfig>>> = Mutex::new(None);
    let mut config = lock(&CONFIG);
    if let Some(config) = &*config {
        return Ok(config.clone());
    }
    let built = system_tls_config()?;
    *config = Some(built.clone());
    Ok(built)
}

fn system_tls_config() -> Result<Arc<rustls::ClientConfig>, String> {
    let mut roots = rustls::RootCertStore::empty();
    let found = rustls_native_certs::load_native_certs();
    for certificate in found.certs {
        // A certificate the system lists but rustls cannot use is skipped,
        // as browsers skip them.
        let _ = roots.add(certificate);
    }
    if roots.is_empty() {
        let why = found
            .errors
            .first()
            .map_or_else(|| "none are installed".to_owned(), ToString::to_string);
        return Err(format!(
            "no certificates of the system could be read ({why})"
        ));
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map(|config| Arc::new(config.with_root_certificates(roots).with_no_client_auth()))
        .map_err(|error| error.to_string())
}

/// `(scheme, authority, rest)` of an `http://` or `https://` URL without
/// user information: how Pane reads the addresses of its own requests,
/// the npm registry's and the artifact source's.
pub(crate) fn split_url(url: &str) -> Option<(&str, &str, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, rest) = rest.split_at(end);
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    Some((scheme, authority, rest))
}

/// The base address of a source on this computer — `url`, ending with `/`
/// — for tests and development builds only: `url` must be `http://` or
/// `https://` on a loopback address written as one (`127.0.0.1`, any
/// `127.x.y.z`, or `[::1]`), with an optional port and path. `refusal`
/// names the source when it is not, so each source that can be replaced
/// (the npm registry, Pane's artifact source) says what it is. Any other
/// address is refused, `localhost` included (a name could resolve
/// elsewhere), so that nothing but the real source is ever reached over
/// the network.
#[cfg(any(test, debug_assertions))]
pub(crate) fn loopback_base(url: &str, refusal: impl Fn() -> String) -> Result<String, String> {
    let (_, authority, _) = split_url(url).ok_or_else(&refusal)?;
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if !host.ends_with(':') && port.chars().all(|c| c.is_ascii_digit()) => {
            host
        }
        _ => authority,
    };
    let literal = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    let loopback = literal
        .parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback());
    // `[::1]` in brackets, `127.0.0.1` without.
    if !loopback || (literal.contains(':') && literal == host) {
        return Err(refusal());
    }
    let mut base = url.to_owned();
    if !base.ends_with('/') {
        base.push('/');
    }
    Ok(base)
}

/// The value of the environment variable `name` naming a source on this
/// computer, in development builds only; `None` when it is not set or is
/// empty.
#[cfg(any(test, debug_assertions))]
pub(crate) fn dev_env(name: &str) -> Option<String> {
    let url = std::env::var(name).ok()?;
    (!url.is_empty()).then_some(url)
}

/// Refuses `url` unless it is an HTTPS address: Pane sends its own requests
/// only over HTTPS, unless what it asks is on this computer (`loopback`,
/// tests and development builds only).
pub(crate) fn https_only(loopback: bool, url: &str) -> Result<(), String> {
    if loopback || url.starts_with("https://") {
        Ok(())
    } else {
        Err(format!("`{url}` is not an HTTPS address"))
    }
}

/// A source Pane downloads from for itself and that tests and development
/// builds can replace with one on this computer: the npm registry, Pane's
/// artifact source. It is reached only over HTTPS unless it is on this
/// computer, within [`OWN_LIMITS`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Origin {
    /// Its address, ending with `/`.
    base: String,
    /// Whether it is on this computer, reached without a proxy and over
    /// plain HTTP if its address says so.
    loopback: bool,
}

impl Origin {
    /// The source at `base`, which ends with `/`, reached over HTTPS.
    pub(crate) fn public(base: &str) -> Origin {
        Origin {
            base: base.to_owned(),
            loopback: false,
        }
    }

    /// A source on this computer at `url`, for tests and development builds
    /// only (see [`loopback_base`]); `refusal` says why another is refused.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn local(url: &str, refusal: impl Fn() -> String) -> Result<Origin, String> {
        let base = loopback_base(url, refusal)?;
        Ok(Origin {
            base,
            loopback: true,
        })
    }

    /// Its address, ending with `/`.
    pub(crate) fn url(&self) -> &str {
        &self.base
    }

    /// Whether it is on this computer.
    #[cfg(test)]
    pub(crate) fn is_loopback(&self) -> bool {
        self.loopback
    }

    /// Asks for `url`, with `headers`, a body of at most `most` bytes
    /// ([`get_blocking`]): only over HTTPS unless the source is on this
    /// computer.
    pub(crate) fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        most: u64,
    ) -> Result<Answer, GetError> {
        https_only(self.loopback, url).map_err(GetError::Failed)?;
        get_blocking(url, headers, most, OWN_LIMITS)
    }

    /// Like [`Origin::get`], telling `progress` of the bytes of the body so
    /// far ([`get_blocking_progressing`]).
    pub(crate) fn get_progressing(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        most: u64,
        progress: &(dyn Fn(u64) + Send + Sync),
    ) -> Result<Answer, GetError> {
        https_only(self.loopback, url).map_err(GetError::Failed)?;
        get_blocking_progressing(url, headers, most, OWN_LIMITS, progress)
    }
}

/// The ceilings of a request Pane sends for itself ([`get_blocking`]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct OwnLimits {
    /// Connecting, the TLS handshake included.
    pub connect: Duration,
    /// The wait for the response's head.
    pub first_byte: Duration,
    /// The wait for each piece of its body.
    pub between_bytes: Duration,
    /// The whole request.
    pub deadline: Duration,
}

/// The ceilings of every request Pane sends for itself: to the npm
/// registry, the artifact source and Git hosts alike.
pub(crate) const OWN_LIMITS: OwnLimits = OwnLimits {
    connect: Duration::from_secs(30),
    first_byte: Duration::from_secs(60),
    between_bytes: Duration::from_secs(60),
    deadline: Duration::from_secs(300),
};

/// The answer to a request Pane sent for itself.
pub(crate) struct Answer {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Why a request Pane sent for itself has no answer.
#[derive(Debug)]
pub(crate) enum GetError {
    /// It could not be sent, or its answer not received: why.
    Failed(String),
    /// Its body is larger than the most asked for.
    TooLarge,
}

/// Sends a GET for `url`, with `headers`, for Pane itself rather than for
/// a guest (downloading npm and Git packages), and receives its answer,
/// whose body may be at most `most` bytes, within `limits`. It connects as a
/// guest's request does, trusting the system's certificates for `https`,
/// follows no redirect (a `3xx` is the answer) and uses no proxy. A
/// `User-Agent` among `headers` replaces Pane's own. Blocks the calling
/// thread, which must not be running an async runtime, on a runtime of its
/// own.
pub(crate) fn get_blocking(
    url: &str,
    headers: &[(&str, &str)],
    most: u64,
    limits: OwnLimits,
) -> Result<Answer, GetError> {
    send_blocking(http::Method::GET, url, headers, None, most, limits, &|_| {})
}

/// Like [`get_blocking`], telling `progress` of the bytes of the answer's
/// body so far (acquiring a default extension's payload, whose size the
/// artifact index gives, so the window can say how far it has come).
pub(crate) fn get_blocking_progressing(
    url: &str,
    headers: &[(&str, &str)],
    most: u64,
    limits: OwnLimits,
    progress: &(dyn Fn(u64) + Send + Sync),
) -> Result<Answer, GetError> {
    send_blocking(
        http::Method::GET,
        url,
        headers,
        None,
        most,
        limits,
        progress,
    )
}

/// Downloads `url` for Pane on an extension's behalf, a web image one of
/// its icons names (#142), within the ceilings of the extension's own web
/// requests (`limits`, ADR 0018: connecting, the response's head, the wait
/// between two pieces of its body, the whole request and its size). As
/// [`get_blocking`] it trusts the system's certificates, follows no
/// redirect and blocks the calling thread, which must not be running an
/// async runtime. `aborted` resolving (the extension's generation ended)
/// ends it at once.
pub(crate) fn get_for_extension(
    url: &str,
    limits: HttpLimits,
    aborted: Option<tokio::sync::oneshot::Receiver<()>>,
) -> Result<Answer, GetError> {
    let own = OwnLimits {
        connect: limits.connect,
        first_byte: limits.first_byte,
        between_bytes: limits.between_bytes,
        deadline: limits.deadline,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| GetError::Failed(error.to_string()))?;
    runtime.block_on(async move {
        let mut aborted = aborted;
        let unwatched = |_: u64| {};
        let request = own_request(
            http::Method::GET,
            url,
            &[],
            None,
            limits.body,
            own,
            &unwatched,
        );
        unless_aborted(request, aborted.as_mut())
            .await
            .unwrap_or_else(|| Err(GetError::Failed("its extension stopped".into())))
    })
}

/// `host:port` of `url`, an `http(s)` address, as a package's network use
/// lists it; `None` for anything else.
pub(crate) fn address_of(url: &str) -> Option<String> {
    let uri: http::Uri = url.parse().ok()?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return None;
    }
    Target::of_uri(&uri).ok().map(|target| target.address())
}

/// Like [`get_blocking`], a POST of `body` (Git's `git-upload-pack`
/// requests).
pub(crate) fn post_blocking(
    url: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
    most: u64,
    limits: OwnLimits,
) -> Result<Answer, GetError> {
    send_blocking(
        http::Method::POST,
        url,
        headers,
        Some(body),
        most,
        limits,
        &|_| {},
    )
}

fn send_blocking(
    method: http::Method,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<Vec<u8>>,
    most: u64,
    limits: OwnLimits,
    progress: &(dyn Fn(u64) + Send + Sync),
) -> Result<Answer, GetError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| GetError::Failed(error.to_string()))?;
    runtime.block_on(own_request(
        method, url, headers, body, most, limits, progress,
    ))
}

#[allow(clippy::too_many_arguments)]
async fn own_request(
    method: http::Method,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<Vec<u8>>,
    most: u64,
    limits: OwnLimits,
    progress: &(dyn Fn(u64) + Send + Sync),
) -> Result<Answer, GetError> {
    let failed = |error: Error| GetError::Failed(error.to_string());
    let uri: http::Uri = url
        .parse()
        .map_err(|_| GetError::Failed(format!("`{url}` is not a web address")))?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return Err(GetError::Failed(format!("`{url}` is not a web address")));
    }
    let target = Target::of_uri(&uri).map_err(failed)?;
    let deadline = Instant::now() + limits.deadline;
    let within = |after: Duration| (Instant::now() + after).min(deadline);
    let (mut sender, connection) =
        connect::<http_body_util::Full<Bytes>>(&target, within(limits.connect))
            .await
            .map_err(failed)?;
    // The connection does the reading and writing; it ends with the
    // request, or is stopped below.
    let driving = tokio::spawn(connection);
    let host = uri.authority().map_or("", |authority| authority.as_str());
    let mut request = http::Request::builder()
        .method(method)
        .uri(uri.path_and_query().map_or("/", |path| path.as_str()))
        .header(http::header::HOST, host);
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
    {
        request = request.header(
            http::header::USER_AGENT,
            concat!("pane/", env!("CARGO_PKG_VERSION")),
        );
    }
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = request
        .body(http_body_util::Full::new(Bytes::from(
            body.unwrap_or_default(),
        )))
        .map_err(|error| GetError::Failed(error.to_string()))?;
    let answered = async {
        let response = timeout_at(within(limits.first_byte), sender.send_request(request))
            .await
            .map_err(|_| failed(Error::ConnectionReadTimeout))?
            .map_err(|error| failed(Error::from(error)))?;
        let status = response.status().as_u16();
        let announced = response
            .headers()
            .get(http::header::CONTENT_LENGTH)
            .and_then(|length| length.to_str().ok()?.parse::<u64>().ok());
        if announced.is_some_and(|length| length > most) {
            return Err(GetError::TooLarge);
        }
        let mut body = response.into_body();
        let mut received = Vec::new();
        loop {
            let piece = match timeout_at(within(limits.between_bytes), body.frame()).await {
                Err(_) if Instant::now() >= deadline => {
                    return Err(failed(Error::HttpResponseTimeout));
                }
                Err(_) => return Err(failed(Error::ConnectionReadTimeout)),
                Ok(None) => break,
                Ok(Some(piece)) => piece.map_err(|error| failed(Error::from(error)))?,
            };
            if let Ok(data) = piece.into_data() {
                if received.len() as u64 + data.len() as u64 > most {
                    return Err(GetError::TooLarge);
                }
                received.extend_from_slice(&data);
                progress(received.len() as u64);
            }
        }
        Ok(Answer {
            status,
            body: received,
        })
    }
    .await;
    driving.abort();
    answered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_data::ExtensionData;
    use crate::packages::PackageIdentity;

    fn request(url: &str) -> http::Request<WasiBody> {
        http::Request::get(url)
            .body(
                http_body_util::Empty::new()
                    .map_err(|never| match never {})
                    .boxed_unsync(),
            )
            .unwrap()
    }

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn code_whose_generation_ended_sends_nothing() {
        let folder = tempfile::tempdir().unwrap();
        let data = ExtensionData::open(folder.path());
        let identity = PackageIdentity::local(folder.path()).unwrap();
        let owned = data.owned_by(&identity);
        // Something listens, so only the refusal can end the request.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        data.set_enabled(&identity, false);
        let network = Arc::new(Network::default());
        let mut sender = Sender::new(Some(owned), network.clone(), Arc::default());
        let sent = sender.send_request(request(&url), None, Box::new(async { Ok(()) }));
        let refused = block_on(Box::into_pin(sent));
        assert!(matches!(refused, Err(Error::HttpRequestDenied)));
        // Nothing was tried.
        assert_eq!(network.contacted(&identity.key()), Vec::<String>::new());
        listener.set_nonblocking(true).unwrap();
        assert!(listener.accept().is_err());
    }

    /// Code of a runtime thread Pane gave up on (a runtime hang) is fenced:
    /// installed or built in, it sends nothing.
    #[test]
    fn code_of_a_runtime_thread_given_up_on_sends_nothing() {
        let folder = tempfile::tempdir().unwrap();
        let data = ExtensionData::open(folder.path());
        let identity = PackageIdentity::local(folder.path()).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let network = Arc::new(Network::default());
        let watch = Arc::new(Watch::default());
        let owned = data.owned_by(&identity).fenced(watch.fence().clone());
        let mut installed = Sender::new(Some(owned), network.clone(), watch.clone());
        let mut built_in = Sender::new(None, network.clone(), watch.clone());
        watch.fence().close();
        for sender in [&mut installed, &mut built_in] {
            let sent = sender.send_request(request(&url), None, Box::new(async { Ok(()) }));
            let refused = block_on(Box::into_pin(sent));
            assert!(matches!(refused, Err(Error::HttpRequestDenied)));
        }
        assert_eq!(network.contacted(&identity.key()), Vec::<String>::new());
        listener.set_nonblocking(true).unwrap();
        assert!(listener.accept().is_err());
    }

    #[test]
    fn a_package_has_at_most_its_limit_of_connections_open() {
        let network = Network::default();
        network.set_limits(HttpLimits {
            connections: 2,
            ..HttpLimits::default()
        });
        let first = network.connect("a", "one:80".into()).unwrap();
        let _second = network.connect("a", "two:80".into()).unwrap();
        assert!(matches!(
            network.connect("a", "three:80".into()),
            Err(Error::ConnectionLimitReached)
        ));
        // Another package's are its own.
        let _other = network.connect("b", "one:80".into()).unwrap();
        // One closes: another may open.
        drop(first);
        let _third = network.connect("a", "three:80".into()).unwrap();
        assert_eq!(network.contacted("a"), ["one:80", "three:80", "two:80"]);
        assert_eq!(network.contacted("b"), ["one:80"]);
        assert_eq!(network.contacted("c"), Vec::<String>::new());
    }
}
