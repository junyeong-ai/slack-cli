use std::path::PathBuf;
use thiserror::Error;

use super::policy::TokenPolicy;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("no authentication is configured. run: slack-cli auth login")]
    NotConfigured,

    #[error("profile '{0}' is not in the auth store")]
    UnknownProfile(String),

    #[error("no token available for policy {policy} in profile '{profile}'")]
    NoTokenForPolicy {
        profile: String,
        policy: TokenPolicy,
    },

    #[error("failed to read auth store at {path}: {source}")]
    StoreRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write auth store at {path}: {source}")]
    StoreWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("auth store at {path} is corrupted: {source}")]
    StoreParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("auth store schema version {found} is not supported (expected {expected})")]
    UnsupportedSchema { found: u32, expected: u32 },

    #[error("OAuth flow failed: {0}")]
    OAuth(#[from] OAuthError),

    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("port {port} is already in use. close the conflicting process or pick another port")]
    PortInUse { port: u16 },

    #[error("callback received with a mismatched state parameter (possible CSRF)")]
    StateMismatch,

    #[error("Slack returned an error on the authorization callback: {0}")]
    AuthorizationDenied(String),

    #[error("callback URL was malformed: {0}")]
    MalformedCallback(String),

    #[error("token exchange failed: Slack returned '{0}'")]
    ExchangeFailed(String),

    #[error("token exchange response was missing field: {0}")]
    MissingField(&'static str),

    #[error("client_id is not configured. set SLACK_CLI_CLIENT_ID or pass --client-id")]
    MissingClientId,

    #[error("could not open the browser. open the URL manually: {url}")]
    BrowserFailed { url: String },

    #[error("HTTP error during OAuth: {0}")]
    Http(#[from] reqwest::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
