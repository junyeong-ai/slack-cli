use anyhow::{Context, Result};
use governor::{
    Jitter, Quota, RateLimiter,
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use reqwest::{Client as HttpClient, StatusCode};
use secrecy::ExposeSecret;
use serde_json::Value;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;
use tracing::warn;

use crate::auth::{Authenticator, TokenKind};
use crate::config::{Config, SlackAppDistribution};
use crate::slack::api_config::{
    API_CONFIGS, ApiConfig, RatePolicy, RequestEncoding, get_api_config,
};
use crate::slack::error::SlackApiError;

type SimpleRateLimiter = Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>>;

struct RateGate {
    limiter: SimpleRateLimiter,
    ready_at: Mutex<Instant>,
}

impl RateGate {
    fn new(quota: Quota) -> Self {
        Self {
            limiter: Arc::new(RateLimiter::direct(quota)),
            ready_at: Mutex::new(Instant::now()),
        }
    }

    async fn wait(&self) {
        loop {
            let ready_at = *self
                .ready_at
                .lock()
                .expect("rate gate lock is not poisoned");
            tokio::time::sleep_until(ready_at).await;
            self.limiter
                .until_ready_with_jitter(Jitter::up_to(Duration::from_millis(100)))
                .await;
            if Instant::now()
                >= *self
                    .ready_at
                    .lock()
                    .expect("rate gate lock is not poisoned")
            {
                return;
            }
        }
    }

    fn defer(&self, delay: Duration) -> Option<()> {
        let until = Instant::now().checked_add(delay)?;
        let mut ready_at = self
            .ready_at
            .lock()
            .expect("rate gate lock is not poisoned");
        *ready_at = (*ready_at).max(until);
        Some(())
    }
}

/// HTTP connection pool tuning. Internal — no user-facing knob.
/// A CLI process is short-lived: 10 idle slots is generous for any realistic
/// concurrent call pattern, and a 90 s pool timeout outlives every single
/// invocation, so callers never observe the difference.
const HTTP_POOL_MAX_IDLE_PER_HOST: usize = 10;
const HTTP_POOL_IDLE_TIMEOUT_SECONDS: u64 = 90;

pub struct SlackCore {
    pub(crate) config: Config,
    pub(crate) auth: Arc<Authenticator>,
    pub(crate) http: HttpClient,
    rate_limiters: HashMap<&'static str, RateGate>,
}

impl SlackCore {
    pub fn new(config: Config, auth: Arc<Authenticator>) -> Result<Self> {
        let http = HttpClient::builder()
            .timeout(Duration::from_secs(config.connection.timeout_seconds))
            .pool_max_idle_per_host(HTTP_POOL_MAX_IDLE_PER_HOST)
            .pool_idle_timeout(Duration::from_secs(HTTP_POOL_IDLE_TIMEOUT_SECONDS))
            .build()
            .context("Failed to create Slack HTTP client")?;

        let rate_limiters = API_CONFIGS
            .iter()
            .map(|(method, api_config)| {
                let rate_policy = Self::effective_rate_policy(
                    &config.connection.app_distribution,
                    method,
                    api_config.rate_policy,
                );
                let limit = config
                    .connection
                    .rate_limit_per_minute
                    .min(rate_policy.requests_per_minute)
                    .max(1);
                let quota_limit =
                    NonZeroU32::new(limit).context("Rate limit must be greater than zero")?;
                let quota = Quota::per_minute(quota_limit);
                Ok((*method, RateGate::new(quota)))
            })
            .collect::<Result<HashMap<_, _>>>()?;

        Ok(Self {
            config,
            auth,
            http,
            rate_limiters,
        })
    }

    pub async fn api_call(&self, method: &str, params: Value) -> Result<Value> {
        let api_config = lookup_config(method)?;
        let (token, kind) = self.auth.token_for(api_config.token_policy).await?;
        self.dispatch(
            method,
            api_config,
            params,
            token.expose_secret(),
            Some(kind),
        )
        .await
    }

    pub(crate) async fn api_call_in_conversation(
        &self,
        method: &str,
        mut params: Value,
        channel_field: &str,
    ) -> Result<Value> {
        let config = lookup_config(method)?;
        let (token, kind) = self.auth.token_for(config.token_policy).await?;
        if let Some(user) = params
            .get(channel_field)
            .and_then(Value::as_str)
            .filter(|id| super::is_user_id(id))
        {
            let channel = self.dm_for(user, token.expose_secret(), kind).await?;
            params[channel_field] = Value::String(channel);
        }
        self.dispatch(method, config, params, token.expose_secret(), Some(kind))
            .await
    }

    async fn dm_for(&self, user: &str, token: &str, kind: TokenKind) -> Result<String> {
        let config = lookup_config("conversations.list")?;
        let mut cursor = None;
        loop {
            let mut params =
                serde_json::json!({ "types": "im", "limit": 200, "exclude_archived": false });
            if let Some(cursor) = cursor {
                params["cursor"] = Value::String(cursor);
            }
            let response = self
                .dispatch("conversations.list", config, params, token, Some(kind))
                .await?;
            let channels = response
                .get("channels")
                .and_then(Value::as_array)
                .context("Missing channels in conversations.list response")?;
            for channel in channels {
                if channel.get("user").and_then(Value::as_str) == Some(user)
                    && channel.get("is_im").and_then(Value::as_bool) == Some(true)
                {
                    return channel
                        .get("id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                        .context("Missing DM ID in conversations.list response");
                }
            }
            cursor = response
                .pointer("/response_metadata/next_cursor")
                .and_then(Value::as_str)
                .filter(|cursor| !cursor.is_empty())
                .map(ToOwned::to_owned);
            if cursor.is_none() {
                anyhow::bail!(
                    "No existing DM with user {user} is accessible to the selected {kind} token. Open one with `slack-cli dm {user}`, or use an accessible D channel ID"
                );
            }
        }
    }

    pub async fn api_call_with(&self, method: &str, params: Value, token: &str) -> Result<Value> {
        let api_config = lookup_config(method)?;
        self.dispatch(method, api_config, params, token, None).await
    }

    async fn dispatch(
        &self,
        method: &str,
        api_config: &'static ApiConfig,
        mut params: Value,
        token: &str,
        kind: Option<TokenKind>,
    ) -> Result<Value> {
        let rate_policy = Self::effective_rate_policy(
            &self.config.connection.app_distribution,
            method,
            api_config.rate_policy,
        );
        if let Some(max_limit) = rate_policy.max_page_limit
            && let Some(limit) = params.get("limit").and_then(|v| v.as_u64())
            && limit > max_limit as u64
        {
            params["limit"] = Value::from(max_limit);
        }

        let mut retry_count = 0;
        let max_attempts = self.config.retry.max_attempts;
        let mut delay = self.config.retry.initial_delay_ms;

        loop {
            if let Some(gate) = self.rate_limiters.get(method) {
                gate.wait().await;
            }
            let endpoint = format!(
                "{}/{}",
                self.config.connection.api_base_url.trim_end_matches('/'),
                method
            );
            let response = match &api_config.encoding {
                RequestEncoding::Query => {
                    let mut url = endpoint;
                    if !params
                        .as_object()
                        .unwrap_or(&serde_json::Map::new())
                        .is_empty()
                    {
                        let query_string = serde_urlencoded::to_string(&params)?;
                        url.push_str(&format!("?{}", query_string));
                    }

                    self.http
                        .get(&url)
                        .header("Authorization", format!("Bearer {}", token))
                        .send()
                        .await
                }
                RequestEncoding::Json => {
                    self.http
                        .post(&endpoint)
                        .header("Authorization", format!("Bearer {}", token))
                        .header("Content-Type", "application/json")
                        .json(&params)
                        .send()
                        .await
                }
                RequestEncoding::Form => {
                    self.http
                        .post(&endpoint)
                        .header("Authorization", format!("Bearer {}", token))
                        .header("Content-Type", "application/x-www-form-urlencoded")
                        .body(serde_urlencoded::to_string(&params)?)
                        .send()
                        .await
                }
            };

            let response = response.map_err(|source| SlackApiError::Transport { source })?;
            let status = response.status();
            let headers = response.headers().clone();

            if status == StatusCode::TOO_MANY_REQUESTS {
                retry_count += 1;
                let retry_after = headers
                    .get("Retry-After")
                    .and_then(|header| header.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .map(Duration::from_secs);
                let wait = retry_after.unwrap_or_else(|| Duration::from_millis(delay));
                let scheduled = self
                    .rate_limiters
                    .get(method)
                    .is_some_and(|gate| gate.defer(wait).is_some());
                if retry_count >= max_attempts || !scheduled {
                    return Err(SlackApiError::RateLimitExhausted {
                        method: method.to_string(),
                        attempts: retry_count,
                    }
                    .into());
                }
                warn!(
                    method,
                    ?wait,
                    attempt = retry_count,
                    max_attempts,
                    "Rate limited; waiting before retry"
                );
                if retry_after.is_none() {
                    delay = ((delay as f64 * self.config.retry.exponential_base) as u64)
                        .min(self.config.retry.max_delay_ms);
                }
                continue;
            }

            let response_text = response
                .text()
                .await
                .map_err(|source| SlackApiError::Transport { source })?;

            if !status.is_success() {
                return Err(SlackApiError::Http {
                    method: method.to_string(),
                    status,
                    body: response_text,
                }
                .into());
            }

            let json_response: Value = serde_json::from_str(&response_text)
                .map_err(|e| anyhow::anyhow!("Failed to parse JSON response: {}", e))?;

            let ok = json_response
                .get("ok")
                .and_then(Value::as_bool)
                .with_context(|| {
                    format!("Invalid Slack API response for {method}: expected a boolean ok field")
                })?;
            if !ok {
                let code = json_response
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("unknown_error");
                return Err(SlackApiError::Api {
                    code: code.to_string(),
                    method: method.to_string(),
                    required: scope_requirement(api_config, kind),
                }
                .into());
            }

            return Ok(json_response);
        }
    }

    fn effective_rate_policy(
        app_distribution: &SlackAppDistribution,
        method: &str,
        base_policy: RatePolicy,
    ) -> RatePolicy {
        match (app_distribution, method) {
            (
                SlackAppDistribution::CommercialExternal,
                "conversations.history" | "conversations.replies",
            ) => RatePolicy {
                requests_per_minute: 1,
                max_page_limit: Some(15),
            },
            _ => base_policy,
        }
    }
}

fn lookup_config(method: &str) -> Result<&'static ApiConfig> {
    get_api_config(method).ok_or_else(|| anyhow::anyhow!("Unknown API method: {}", method))
}

/// The scopes the method declares for the kind of token that was actually
/// sent. Slack says only that a scope is missing, not which; this is what the
/// registry already knows about the call that failed.
fn scope_requirement(api_config: &'static ApiConfig, kind: Option<TokenKind>) -> Vec<String> {
    let Some(kind) = kind else {
        return Vec::new();
    };
    api_config
        .scopes
        .of(kind)
        .iter()
        .map(|scope| (*scope).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn cooldown_is_shared_and_cannot_be_shortened_by_another_response() {
        let gate = Arc::new(RateGate::new(Quota::per_minute(
            NonZeroU32::new(20).unwrap(),
        )));
        gate.defer(Duration::from_secs(30)).unwrap();
        gate.defer(Duration::from_secs(1)).unwrap();
        let start = Instant::now();
        let first = gate.clone();
        let second = gate.clone();
        let ((), ()) = tokio::join!(first.wait(), second.wait());
        assert!(start.elapsed() >= Duration::from_secs(30));
    }

    #[tokio::test]
    async fn an_unrepresentable_retry_after_is_rejected_without_panicking() {
        let gate = RateGate::new(Quota::per_minute(NonZeroU32::new(20).unwrap()));
        assert!(gate.defer(Duration::from_secs(u64::MAX)).is_none());
    }

    #[test]
    fn commercial_external_uses_restricted_history_limits() {
        let base = RatePolicy {
            requests_per_minute: 50,
            max_page_limit: Some(999),
        };

        let policy = SlackCore::effective_rate_policy(
            &SlackAppDistribution::CommercialExternal,
            "conversations.history",
            base,
        );

        assert_eq!(policy.requests_per_minute, 1);
        assert_eq!(policy.max_page_limit, Some(15));
    }

    #[test]
    fn marketplace_or_internal_keeps_tiered_history_limits() {
        let base = RatePolicy {
            requests_per_minute: 50,
            max_page_limit: Some(999),
        };

        let policy = SlackCore::effective_rate_policy(
            &SlackAppDistribution::MarketplaceOrInternal,
            "conversations.history",
            base,
        );

        assert_eq!(policy.requests_per_minute, 50);
        assert_eq!(policy.max_page_limit, Some(999));
    }
}
