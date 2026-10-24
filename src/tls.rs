//! TLS trust for every connection the CLI opens: Slack's Web API and Socket
//! Mode, the OAuth exchange, release downloads and HTTP sinks.

use std::sync::{Arc, LazyLock};

use rustls::{ClientConfig, RootCertStore};
use rustls_platform_verifier::BuilderVerifierExt;

static CLIENT_CONFIG: LazyLock<Arc<ClientConfig>> =
    LazyLock::new(|| Arc::new(client_config_for_system()));

/// The operating system decides what is trusted, so a root installed there,
/// such as a corporate proxy's, applies to every connection alike. A system
/// that provides no CA certificates at all, such as a minimal Linux container,
/// trusts the Mozilla roots built into the binary instead of nothing. Decided
/// once per process, so every connection shares the answer.
pub fn client_config() -> Arc<ClientConfig> {
    Arc::clone(&CLIENT_CONFIG)
}

/// A `reqwest` client builder that trusts what [`client_config`] trusts.
#[allow(clippy::disallowed_methods)]
pub fn http_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder().tls_backend_preconfigured(ClientConfig::clone(&CLIENT_CONFIG))
}

fn client_config_for_system() -> ClientConfig {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let builder = || {
        ClientConfig::builder_with_provider(Arc::clone(&provider))
            .with_safe_default_protocol_versions()
            .expect("the bundled crypto provider supports the default TLS versions")
    };
    // The platform verifier fails only where it reads the system's roots
    // itself and finds none; macOS and Windows verify through the OS.
    match builder().with_platform_verifier() {
        Ok(verified) => verified.with_no_client_auth(),
        Err(err) => {
            tracing::debug!(%err, "the system provides no CA certificates; trusting the bundled Mozilla roots");
            let roots = RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            builder()
                .with_root_certificates(roots)
                .with_no_client_auth()
        }
    }
}
