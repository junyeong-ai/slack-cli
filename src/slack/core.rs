use anyhow::{Context, Result};
use governor::{
    Jitter, Quota, RateLimiter,
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use reqwest::{Client as HttpClient, StatusCode};
use serde_json::Value;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

use crate::config::{Config, SlackAppDistribution};
use crate::slack::api_config::{
    API_CONFIGS, RatePolicy, RequestEncoding, TokenPolicy, get_api_config,
};

type SimpleRateLimiter = Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>>;

pub struct SlackCore {
    pub(crate) config: Config,
    pub(crate) http_client: HttpClient,
    pub(crate) rate_limiters: HashMap<&'static str, SimpleRateLimiter>,
}

impl SlackCore {
    pub fn new(config: Config) -> Result<Self> {
        let http_client = HttpClient::builder()
            .timeout(Duration::from_secs(config.connection.timeout_seconds))
            .pool_max_idle_per_host(config.connection.max_idle_per_host as usize)
            .pool_idle_timeout(Duration::from_secs(
                config.connection.pool_idle_timeout_seconds,
            ))
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
                Ok((*method, Arc::new(RateLimiter::direct(quota))))
            })
            .collect::<Result<HashMap<_, _>>>()?;

        Ok(Self {
            config,
            http_client,
            rate_limiters,
        })
    }

    pub(crate) fn get_token(&self, policy: TokenPolicy) -> Result<&str> {
        match policy {
            TokenPolicy::UserRequired => {
                self.config.user_token.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Slack user token is required for this API method")
                })
            }
            TokenPolicy::UserPreferred => self
                .config
                .user_token
                .as_deref()
                .or(self.config.bot_token.as_deref())
                .ok_or_else(|| anyhow::anyhow!("No Slack token available")),
            TokenPolicy::BotPreferred => self
                .config
                .bot_token
                .as_deref()
                .or(self.config.user_token.as_deref())
                .ok_or_else(|| anyhow::anyhow!("No Slack token available")),
        }
    }

    pub async fn api_call(&self, method: &str, mut params: Value) -> Result<Value> {
        let api_config = get_api_config(method)
            .ok_or_else(|| anyhow::anyhow!("Unknown API method: {}", method))?;

        let token = self.get_token(api_config.token_policy)?;
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

        if let Some(rate_limiter) = self.rate_limiters.get(method) {
            rate_limiter
                .until_ready_with_jitter(Jitter::up_to(Duration::from_millis(100)))
                .await;
        }

        let mut retry_count = 0;
        let max_attempts = self.config.retry.max_attempts;
        let mut delay = self.config.retry.initial_delay_ms;

        loop {
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

                    self.http_client
                        .get(&url)
                        .header("Authorization", format!("Bearer {}", token))
                        .send()
                        .await
                }
                RequestEncoding::Json => {
                    self.http_client
                        .post(&endpoint)
                        .header("Authorization", format!("Bearer {}", token))
                        .header("Content-Type", "application/json")
                        .json(&params)
                        .send()
                        .await
                }
            };

            let response = response.map_err(|e| anyhow::anyhow!("HTTP request failed: {}", e))?;
            let status = response.status();
            let headers = response.headers().clone();

            if status == StatusCode::TOO_MANY_REQUESTS {
                retry_count += 1;
                if retry_count >= max_attempts {
                    return Err(anyhow::anyhow!(
                        "Rate limit exceeded for {} after {} attempts",
                        method,
                        max_attempts
                    ));
                }

                let wait_time = if let Some(retry_after) = headers.get("Retry-After") {
                    if let Ok(retry_after_str) = retry_after.to_str() {
                        if let Ok(retry_seconds) = retry_after_str.parse::<u64>() {
                            retry_seconds * 1000
                        } else {
                            delay
                        }
                    } else {
                        delay
                    }
                } else {
                    delay
                };

                warn!(
                    "Rate limited for {}, retrying in {}ms (attempt {}/{})",
                    method, wait_time, retry_count, max_attempts
                );
                tokio::time::sleep(Duration::from_millis(wait_time)).await;

                if !headers.contains_key("Retry-After") {
                    delay = (delay as f64 * self.config.retry.exponential_base) as u64;
                    delay = delay.min(self.config.retry.max_delay_ms);
                }

                continue;
            }

            let response_text = response
                .text()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

            if !status.is_success() {
                return Err(anyhow::anyhow!(
                    "Slack API HTTP error for {}: {} {}",
                    method,
                    status,
                    response_text
                ));
            }

            let json_response: Value = serde_json::from_str(&response_text)
                .map_err(|e| anyhow::anyhow!("Failed to parse JSON response: {}", e))?;

            if let Some(false) = json_response.get("ok").and_then(|v| v.as_bool()) {
                let error = json_response
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("unknown_error");
                return Err(anyhow::anyhow!("Slack API error: {}", error));
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

#[cfg(test)]
mod tests {
    use super::*;

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
