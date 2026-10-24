//! TLS trust for every connection the CLI opens: Slack's Web API and Socket
//! Mode, the OAuth exchange, release downloads and HTTP sinks.
//!
//! The operating system decides what is trusted, so a root installed there,
//! such as a corporate proxy's, applies to every connection alike. A system
//! that provides no CA certificates at all, such as a minimal Linux container,
//! trusts the Mozilla roots built into the binary instead of nothing. Decided
//! once per process, so every connection shares the answer.

use std::sync::{Arc, LazyLock};

use rustls::{ClientConfig, RootCertStore};
use rustls_platform_verifier::BuilderVerifierExt;
use tokio::net::TcpStream;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream, tungstenite};

static CLIENT_CONFIG: LazyLock<Arc<ClientConfig>> =
    LazyLock::new(|| Arc::new(client_config_for_system()));

/// A `reqwest` client builder with the CLI's trust.
#[allow(clippy::disallowed_methods)]
pub fn http_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder().tls_backend_preconfigured(ClientConfig::clone(&CLIENT_CONFIG))
}

/// Opens a WebSocket connection with the CLI's trust.
#[allow(clippy::disallowed_methods)]
pub async fn connect_websocket(
    url: &str,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Error> {
    let connector = Connector::Rustls(Arc::clone(&CLIENT_CONFIG));
    let (stream, _) =
        tokio_tungstenite::connect_async_tls_with_config(url, None, false, Some(connector)).await?;
    Ok(stream)
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
