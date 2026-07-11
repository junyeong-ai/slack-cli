use reqwest::StatusCode;
use thiserror::Error;

/// Failure of a `SlackCore` API call, split by where it happened so the CLI
/// boundary can classify without parsing message strings.
#[derive(Debug, Error)]
pub enum SlackApiError {
    /// Slack answered `ok: false`. `code` is Slack's documented error string
    /// (e.g. `channel_not_found`, `missing_scope`) and is surfaced verbatim.
    #[error("Slack API error: {code}")]
    Api { code: String },

    /// Every retry of a 429 response was consumed.
    #[error("Rate limit exceeded for {method} after {attempts} attempts")]
    RateLimitExhausted { method: String, attempts: u32 },

    /// Slack answered with a non-2xx status outside the 429 retry path.
    #[error("Slack API HTTP error for {method}: {status} {body}")]
    Http {
        method: String,
        status: StatusCode,
        body: String,
    },

    /// The request never produced a usable response (DNS, TLS, timeout, …).
    #[error("HTTP request failed: {source}")]
    Transport {
        #[source]
        source: reqwest::Error,
    },
}
