//! Services the package search samples cannot use, on 127.0.0.1 only: a
//! port that refuses connections, and an `https` service whose certificate
//! no system trusts. Apart from the fixture service (`service.rs`), which
//! the window tests include too, as these need pane-core's own
//! dependencies (tokio, rustls).

#![allow(dead_code)]

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// An `https` service on 127.0.0.1 whose certificate no system trusts
/// (self-signed, in `untrusted.pem` beside this file): every TLS handshake
/// with it fails on the certificate. It answers nothing else.
pub struct UntrustedService {
    address: SocketAddr,
}

impl UntrustedService {
    pub fn start() -> UntrustedService {
        use rustls::pki_types::pem::PemObject;
        use rustls::pki_types::{CertificateDer, PrivateKeyDer};

        let pem = include_bytes!("untrusted.pem");
        let certificates = CertificateDer::pem_slice_iter(pem)
            .collect::<Result<Vec<_>, _>>()
            .expect("the test certificate");
        let key = PrivateKeyDer::from_pem_slice(pem).expect("the test key");
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(certificates, key)
            .expect("a TLS configuration");
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let config = config.clone();
                thread::spawn(move || {
                    let Ok(mut connection) = rustls::ServerConnection::new(config) else {
                        return;
                    };
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                    // Until the client gives up on the certificate.
                    while connection.is_handshaking() {
                        if connection.complete_io(&mut stream).is_err() {
                            return;
                        }
                    }
                });
            }
        });
        UntrustedService { address }
    }

    /// `https://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("https://{}", self.address)
    }
}

/// A port of 127.0.0.1 that refuses connections for as long as this is
/// kept: a socket bound to it that never listens, so nothing else can take
/// the port meanwhile (as it could once a listener was released).
pub struct ClosedPort(tokio::net::TcpSocket);

impl ClosedPort {
    pub fn new() -> ClosedPort {
        let socket = tokio::net::TcpSocket::new_v4().expect("a socket");
        socket
            .bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("a free port");
        ClosedPort(socket)
    }

    pub fn port(&self) -> u16 {
        self.0.local_addr().unwrap().port()
    }

    /// `http://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port())
    }
}
