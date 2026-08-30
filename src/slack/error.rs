use reqwest::StatusCode;
use thiserror::Error;

/// Failure of a `SlackCore` API call, split by where it happened so the CLI
/// boundary can classify without parsing message strings.
#[derive(Debug, Error)]
pub enum SlackApiError {
    /// Slack answered `ok: false`. `code` is Slack's documented error string
    /// (e.g. `channel_not_found`, `missing_scope`) and is surfaced verbatim.
    #[error("{}", api_failure(code, method, required))]
    Api {
        code: String,
        method: String,
        required: Vec<String>,
    },

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

/// A scope refusal names the method and what it needed; every other code is
/// Slack's own vocabulary and is passed through untouched.
fn api_failure(code: &str, method: &str, required: &[String]) -> String {
    if code != "missing_scope" || required.is_empty() {
        return format!("Slack API error: {code}");
    }
    format!(
        "Slack API error: {code}. {method} needs {}; the token in use was not granted it",
        required.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_refusal_names_the_method_and_what_it_needed() {
        let message = api_failure("missing_scope", "pins.add", &["pins:write".to_string()]);
        assert!(message.contains("pins.add"), "{message}");
        assert!(message.contains("pins:write"), "{message}");
    }

    #[test]
    fn every_other_code_is_slacks_own_wording() {
        assert_eq!(
            api_failure("channel_not_found", "chat.postMessage", &[]),
            "Slack API error: channel_not_found"
        );
        assert_eq!(
            api_failure("missing_scope", "chat.postMessage", &[]),
            "Slack API error: missing_scope"
        );
    }
}
