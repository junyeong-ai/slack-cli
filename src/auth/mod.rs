pub mod cli_handler;
pub mod oauth;

pub(crate) mod authenticator;
pub(crate) mod env;
pub(crate) mod errors;
pub(crate) mod login;
pub(crate) mod method;
pub(crate) mod policy;
pub(crate) mod profile;
pub(crate) mod secret;
pub(crate) mod state;
pub(crate) mod store;

pub use authenticator::{AuthLoadOptions, Authenticator};
pub use env::EnvOverrides;
pub use errors::{AuthError, OAuthError};
pub use method::AuthMethod;
pub use policy::TokenPolicy;

use std::path::PathBuf;

pub fn default_store_path() -> Option<PathBuf> {
    std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config"))
        })
        .map(|mut p| {
            p.push("slack-cli");
            p.push("auth.json");
            p
        })
}
