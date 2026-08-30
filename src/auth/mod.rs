pub mod cli_handler;
pub mod oauth;

pub(crate) mod authenticator;
pub(crate) mod credential;
pub(crate) mod env;
pub(crate) mod errors;
pub(crate) mod login;
pub(crate) mod method;
pub(crate) mod migrate;
pub(crate) mod policy;
pub(crate) mod profile;
pub(crate) mod secret;
pub(crate) mod state;
pub(crate) mod store;

pub use authenticator::{AuthLoadOptions, Authenticator};
pub use credential::{Credential, Readiness, TokenKind, TokenSet};
pub use env::EnvOverrides;
pub use errors::{AuthError, OAuthError};
pub use method::AuthMethod;
pub use policy::TokenPolicy;

use credential::Credential as AppCredential;

/// The stored form of an app-level token. It never expires and carries the one
/// scope it can hold, so it is built here rather than at each call site.
pub fn app_credential(token: secret::Secret) -> AppCredential {
    AppCredential::permanent(token, vec![crate::slack::scopes::APP_SCOPE.to_string()])
}
