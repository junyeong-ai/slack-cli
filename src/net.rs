//! Every connection the CLI opens: Slack's Web API and Socket Mode, the OAuth
//! exchange, release downloads and HTTP sinks.
//!
//! The operating system decides what is trusted, so a root installed there,
//! such as a corporate proxy's, applies to every connection alike. A system
//! that provides no CA certificates at all, such as a minimal Linux container,
//! trusts the Mozilla roots built into the binary instead of nothing.
//!
//! The environment's proxy settings (`HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`
//! and `NO_PROXY`) are read by the matcher `reqwest` uses, so a WebSocket is
//! routed exactly as an HTTPS request to the same host. Both are decided once
//! per process, so every connection shares the answer.

use std::io;
use std::sync::{Arc, LazyLock};

use http::Uri;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::connect::proxy::Tunnel;
use hyper_util::client::proxy::matcher::{Intercept, Matcher};
use rustls::{ClientConfig, RootCertStore};
use rustls_platform_verifier::BuilderVerifierExt;
use tokio::net::TcpStream;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream, tungstenite};
use tower_service::Service;

static CLIENT_CONFIG: LazyLock<Arc<ClientConfig>> =
    LazyLock::new(|| Arc::new(client_config_for_system()));

static PROXIES: LazyLock<Matcher> = LazyLock::new(Matcher::from_system);

/// A `reqwest` client builder with the CLI's trust.
#[allow(clippy::disallowed_methods)]
pub fn http_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder().tls_backend_preconfigured(ClientConfig::clone(&CLIENT_CONFIG))
}

/// Opens a WebSocket connection with the CLI's trust, through the proxy the
/// environment names for its host.
pub async fn connect_websocket(
    url: &str,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Error> {
    connect_websocket_via(url, &PROXIES).await
}

#[allow(clippy::disallowed_methods)]
async fn connect_websocket_via(
    url: &str,
    proxies: &Matcher,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Error> {
    let connector = Connector::Rustls(Arc::clone(&CLIENT_CONFIG));
    let destination = https_equivalent(url)?;
    let (stream, _) = match proxies.intercept(&destination) {
        None => {
            tokio_tungstenite::connect_async_tls_with_config(url, None, false, Some(connector))
                .await?
        }
        Some(proxy) => {
            let tunnel = tunnel(&proxy, destination).await?;
            tokio_tungstenite::client_async_tls_with_config(url, tunnel, None, Some(connector))
                .await?
        }
    };
    Ok(stream)
}

/// The `https` (or, for `ws`, `http`) URI a proxy decision is made for, with
/// the port spelled out because a CONNECT request always names one.
fn https_equivalent(url: &str) -> Result<Uri, tungstenite::Error> {
    let invalid = |reason: &str| {
        tungstenite::Error::Url(tungstenite::error::UrlError::UnableToConnect(format!(
            "{url}: {reason}"
        )))
    };
    let parsed: Uri = url.parse().map_err(|_| invalid("not a valid URL"))?;
    let (scheme, default_port) = match parsed.scheme_str() {
        Some("wss") => ("https", 443),
        Some("ws") => ("http", 80),
        _ => return Err(invalid("not a WebSocket URL")),
    };
    let host = parsed.host().ok_or_else(|| invalid("no host"))?;
    let port = parsed.port_u16().unwrap_or(default_port);
    format!("{scheme}://{host}:{port}/")
        .parse()
        .map_err(|_| invalid("not a valid URL"))
}

/// An HTTP CONNECT tunnel to `destination`. Only an `http://` proxy carries
/// one here; any other kind is refused rather than bypassed.
async fn tunnel(proxy: &Intercept, destination: Uri) -> Result<TcpStream, tungstenite::Error> {
    let scheme = proxy.uri().scheme_str().unwrap_or_default();
    if scheme != "http" {
        return Err(tungstenite::Error::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "Socket Mode connects only through an http:// proxy; the environment names a {scheme} proxy"
            ),
        )));
    }
    let mut tunnel = Tunnel::new(proxy.uri().clone(), HttpConnector::new());
    if let Some(credentials) = proxy.basic_auth() {
        tunnel = tunnel.with_auth(credentials.clone());
    }
    std::future::poll_fn(|cx| tunnel.poll_ready(cx))
        .await
        .map_err(io::Error::other)?;
    let stream = tunnel.call(destination).await.map_err(io::Error::other)?;
    Ok(stream.into_inner())
}

fn client_config_for_system() -> ClientConfig {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let builder = || {
        ClientConfig::builder_with_provider(Arc::clone(&provider))
            .with_safe_default_protocol_versions()
            .expect("the bundled crypto provider supports the default TLS versions")
    };
    // The platform verifier fails only where it reads the roots itself and
    // finds none; macOS and Windows verify through the OS.
    match builder().with_platform_verifier() {
        Ok(verified) => verified.with_no_client_auth(),
        Err(err) => {
            if names_its_own_roots() {
                tracing::warn!(
                    %err,
                    "SSL_CERT_FILE and SSL_CERT_DIR provide no CA certificates; trusting the bundled Mozilla roots"
                );
            } else {
                tracing::debug!(
                    %err,
                    "the system provides no CA certificates; trusting the bundled Mozilla roots"
                );
            }
            let roots = RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            builder()
                .with_root_certificates(roots)
                .with_no_client_auth()
        }
    }
}

/// Whether the environment replaces the system's roots, read the way
/// `rustls-native-certs` reads it: a set `SSL_CERT_FILE`, or an `SSL_CERT_DIR`
/// naming at least one directory. Trust someone configured is not silently
/// swapped for the bundled roots.
fn names_its_own_roots() -> bool {
    std::env::var_os("SSL_CERT_FILE").is_some()
        || std::env::var_os("SSL_CERT_DIR")
            .is_some_and(|dirs| std::env::split_paths(&dirs).any(|dir| !dir.as_os_str().is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;

    #[tokio::test]
    async fn a_websocket_tunnels_through_the_proxy_named_for_its_host() {
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = server.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            socket.send(Message::text("hello")).await.unwrap();
        });

        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy.local_addr().unwrap();
        let connect_request = tokio::spawn(async move {
            let (mut client, _) = proxy.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(client.read_u8().await.unwrap());
            }
            client
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
                .unwrap();
            let mut upstream = TcpStream::connect(server_addr).await.unwrap();
            tokio::spawn(async move {
                let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
            });
            String::from_utf8(head).unwrap()
        });

        let proxies = Matcher::builder()
            .all(format!("http://user:secret@{proxy_addr}"))
            .build();
        let mut socket = connect_websocket_via(&format!("ws://{server_addr}/link"), &proxies)
            .await
            .unwrap();

        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::text("hello")
        );
        let request = connect_request.await.unwrap();
        assert!(
            request.starts_with(&format!("CONNECT {server_addr} HTTP/1.1\r\n")),
            "{request}"
        );
        assert!(
            request.contains("Proxy-Authorization: Basic dXNlcjpzZWNyZXQ=\r\n"),
            "{request}"
        );
    }

    #[tokio::test]
    async fn a_proxy_that_cannot_carry_a_tunnel_is_refused_not_bypassed() {
        let proxies = Matcher::builder().all("socks5://127.0.0.1:9").build();

        let err = connect_websocket_via("wss://wss-primary.slack.com/link", &proxies)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("http:// proxy"), "{err}");
    }
}
