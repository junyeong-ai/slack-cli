use anyhow::Result;
use governor::{
    Jitter, Quota, RateLimiter, clock::DefaultClock, middleware::NoOpMiddleware,
    state::InMemoryState,
};
use reqwest::{Client as HttpClient, StatusCode};
use serde_json::Value;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

use crate::config::Config;
use crate::slack::api_config::{ApiMethod, get_api_config};

type SimpleRateLimiter = Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>>;
use governor::state::NotKeyed;

/// Core Slack API client with shared functionality
pub struct SlackCore {
    pub(crate) config: Config,
    pub(crate) http_client: HttpClient,
    pub(crate) rate_limiter: SimpleRateLimiter,
}

impl SlackCore {
    pub fn new(config: Config) -> Self {
        let http_client = HttpClient::builder()
            .timeout(Duration::from_secs(config.connection.timeout_seconds))
            .build()
            .expect("Failed to create HTTP client");

        // Create rate limiter with conservative limits
        // Safe because 20 is non-zero
        let quota = Quota::per_minute(NonZeroU32::new(20).expect("20 is non-zero"));
        let rate_limiter = Arc::new(RateLimiter::direct(quota));

        Self {
            config,
            http_client,
            rate_limiter,
        }
    }

    /// Get appropriate token based on preference
    pub(crate) fn get_token(&self, prefer_user: bool) -> Result<&str> {
        if prefer_user && let Some(token) = &self.config.user_token {
            return Ok(token);
        }

        if let Some(token) = &self.config.bot_token {
            return Ok(token);
        }

        if let Some(token) = &self.config.user_token {
            return Ok(token);
        }

        Err(anyhow::anyhow!("No Slack token available"))
    }

    /// Core API call method shared by all specialized clients
    pub async fn api_call(
        &self,
        method: &str,
        params: Value,
        files: Option<Vec<(&str, Vec<u8>)>>,
        prefer_user_token: bool,
    ) -> Result<Value> {
        // Get API configuration
        let api_config = get_api_config(method)
            .ok_or_else(|| anyhow::anyhow!("Unknown API method: {}", method))?;

        // Determine token preference
        let actual_prefer_user = prefer_user_token || api_config.prefer_user_token;
        let token = self.get_token(actual_prefer_user)?;

        // Rate limiting
        self.rate_limiter
            .until_ready_with_jitter(Jitter::up_to(Duration::from_millis(100)))
            .await;

        // Retry logic with exponential backoff
        let mut retry_count = 0;
        let max_retries = self.config.retry.max_attempts;
        let mut delay = self.config.retry.initial_delay_ms;

        loop {
            let response = match &api_config.method {
                ApiMethod::Get => {
                    let mut url = format!("https://slack.com/api/{}", method);
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
                ApiMethod::PostJson => {
                    self.http_client
                        .post(format!("https://slack.com/api/{}", method))
                        .header("Authorization", format!("Bearer {}", token))
                        .header("Content-Type", "application/json")
                        .json(&params)
                        .send()
                        .await
                }
                ApiMethod::PostForm => {
                    if let Some(files) = &files {
                        // Multipart form for file uploads
                        let mut form = reqwest::multipart::Form::new();

                        // Add regular parameters
                        if let Some(obj) = params.as_object() {
                            for (key, value) in obj {
                                let text_value = match value {
                                    Value::String(s) => s.to_string(),
                                    Value::Number(n) => n.to_string(),
                                    Value::Bool(b) => b.to_string(),
                                    _ => {
                                        let s = value.to_string();
                                        s.trim_matches('"').to_owned()
                                    }
                                };
                                form = form.text(key.to_string(), text_value);
                            }
                        }

                        // Add files
                        for (field_name, file_data) in files {
                            let field_name_str = field_name.to_string();
                            let part = reqwest::multipart::Part::bytes(file_data.clone())
                                .file_name(field_name_str.clone());
                            form = form.part(field_name_str, part);
                        }

                        self.http_client
                            .post(format!("https://slack.com/api/{}", method))
                            .header("Authorization", format!("Bearer {}", token))
                            .multipart(form)
                            .send()
                            .await
                    } else {
                        // Regular form data
                        self.http_client
                            .post(format!("https://slack.com/api/{}", method))
                            .header("Authorization", format!("Bearer {}", token))
                            .header("Content-Type", "application/x-www-form-urlencoded")
                            .form(&params)
                            .send()
                            .await
                    }
                }
            };

            let response = response.map_err(|e| anyhow::anyhow!("HTTP request failed: {}", e))?;
            let status = response.status();
            let headers = response.headers().clone();

            // Handle rate limiting (429)
            if status == StatusCode::TOO_MANY_REQUESTS {
                retry_count += 1;
                if retry_count >= max_retries {
                    return Err(anyhow::anyhow!(
                        "Rate limit exceeded for {} after {} retries",
                        method,
                        max_retries
                    ));
                }

                // Use Retry-After header if provided, otherwise fallback to exponential backoff
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
                    method, wait_time, retry_count, max_retries
                );
                tokio::time::sleep(Duration::from_millis(wait_time)).await;

                // Only update delay for exponential backoff if we didn't use Retry-After
                if !headers.contains_key("Retry-After") {
                    delay = (delay as f64 * self.config.retry.exponential_base) as u64;
                    delay = delay.min(self.config.retry.max_delay_ms);
                }

                continue;
            }

            // Parse response
            let response_text = response
                .text()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

            let json_response: Value = serde_json::from_str(&response_text)
                .map_err(|e| anyhow::anyhow!("Failed to parse JSON response: {}", e))?;

            // Check for Slack API errors
            if let Some(ok) = json_response.get("ok").and_then(|v| v.as_bool())
                && !ok
            {
                let error = json_response
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("unknown_error");

                return Err(anyhow::anyhow!("Slack API error: {}", error));
            }

            return Ok(json_response);
        }
    }
}
