//! Web requests through `wasi:http@0.3.0`'s client, which Pane sends for the
//! command (`http` and `https`, over HTTP/1.1, trusting the system's
//! certificates).
//!
//! [`get`] covers the common case: fetch an address and read the whole
//! response. The generated bindings ([`wasi::http`]) are here too for
//! anything else, such as other methods, request bodies or reading a body as
//! it arrives.
//!
//! A request runs inside the call that made it. When Pane stops that call (a
//! search the user replaced by typing on, a command left, a package disabled
//! or reloaded), the request is dropped with it: the connection closes and
//! nothing after the `await` runs.
//!
//! ```ignore
//! let response = pane_extension::http::get("https://example.com/search?q=pane", &[]).await?;
//! if response.status != 200 {
//!     return Err(format!("the service answered {}", response.status));
//! }
//! let text = response.text();
//! ```

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

wit_bindgen::generate!({
    path: "wit",
    world: "http-user",
    default_bindings_module: "pane_extension::http",
    generate_all,
});

pub use wasi::http::types::ErrorCode;
use wasi::http::types::{Fields, Request, Response as WasiResponse, Scheme};

/// A response read to its end.
#[derive(Clone, Debug)]
pub struct Response {
    /// The HTTP status code, such as 200.
    pub status: u16,
    /// The headers, names in lower case, in the order received.
    pub headers: Vec<(String, Vec<u8>)>,
    /// The body's bytes.
    pub body: Vec<u8>,
}

impl Response {
    /// The body as text, invalid UTF-8 replaced with U+FFFD.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The first value of header `name` (compared without case), as text.
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| String::from_utf8_lossy(value).into_owned())
    }
}

/// Fetches `url`, an absolute `http://` or `https://` address, with GET and
/// the extra `headers`, and reads the whole response. Any status is a
/// response; an error is why no response came, such as "connection
/// refused" or "the address could not be resolved", readable as it is.
pub async fn get(url: &str, headers: &[(&str, &str)]) -> Result<Response, String> {
    let (scheme, authority, path) = split_url(url)?;
    let fields = Fields::new();
    for (name, value) in headers {
        fields
            .append(name, value.as_bytes())
            .map_err(|error| format!("header {name} cannot be sent: {error:?}"))?;
    }
    // No body and no trailers.
    let (trailers_writer, trailers) = wit_future::new(|| Ok(None));
    drop(trailers_writer);
    let (request, _sent) = Request::new(fields, None, trailers, None);
    let invalid = |part: &str| format!("{url} is not a web address Pane can send ({part})");
    request
        .set_scheme(Some(&scheme))
        .map_err(|()| invalid("scheme"))?;
    request
        .set_authority(Some(authority))
        .map_err(|()| invalid("host"))?;
    request
        .set_path_with_query(Some(path))
        .map_err(|()| invalid("path"))?;
    let response = wasi::http::client::send(request)
        .await
        .map_err(|error| explain(&error))?;
    let status = response.get_status_code();
    let headers = response
        .get_headers()
        .copy_all()
        .into_iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value))
        .collect();
    let (read_writer, read) = wit_future::new(|| Ok(()));
    drop(read_writer);
    let (body, trailers) = WasiResponse::consume_body(response, read);
    let body = body.collect().await;
    // Whether the body arrived whole: a connection that broke off midway
    // is an error, not a shorter body.
    match trailers.await {
        Ok(_) => Ok(Response {
            status,
            headers,
            body,
        }),
        Err(error) => Err(explain(&error)),
    }
}

/// `url`'s scheme, authority (`host[:port]`) and path with its query.
fn split_url(url: &str) -> Result<(Scheme, &str, &str), String> {
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (Scheme::Https, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (Scheme::Http, rest)
    } else {
        return Err(format!("{url} is not an http:// or https:// address"));
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    if authority.is_empty() {
        return Err(format!("{url} names no host"));
    }
    let path = path.split('#').next().unwrap_or_default();
    Ok((scheme, authority, if path.is_empty() { "/" } else { path }))
}

/// Why no response came, in words.
pub fn explain(error: &ErrorCode) -> String {
    match error {
        ErrorCode::DnsTimeout => "looking up the address timed out".into(),
        ErrorCode::DnsError(_) => "the address could not be resolved".into(),
        ErrorCode::DestinationNotFound => "the host was not found".into(),
        ErrorCode::DestinationUnavailable => "the host is unavailable".into(),
        ErrorCode::DestinationIpUnroutable => "the host cannot be reached".into(),
        ErrorCode::ConnectionRefused => "connection refused".into(),
        ErrorCode::ConnectionTerminated => "the connection was closed".into(),
        ErrorCode::ConnectionTimeout => "connecting timed out".into(),
        ErrorCode::ConnectionReadTimeout => "the service did not answer in time".into(),
        ErrorCode::HttpResponseTimeout => "the service took too long to answer".into(),
        ErrorCode::HttpResponseBodySize(Some(limit)) => {
            format!("the answer is larger than the {limit} bytes Pane accepts")
        }
        ErrorCode::HttpResponseBodySize(None) => "the answer is too large".into(),
        ErrorCode::ConnectionLimitReached => {
            "too many of the extension's requests are open at once".into()
        }
        ErrorCode::TlsCertificateError => "the host's certificate is not trusted".into(),
        ErrorCode::TlsProtocolError | ErrorCode::TlsAlertReceived(_) => {
            "the secure connection failed".into()
        }
        ErrorCode::HttpRequestDenied => "Pane did not send the request".into(),
        ErrorCode::InternalError(Some(message)) => message.clone(),
        other => format!("{other:?}"),
    }
}
