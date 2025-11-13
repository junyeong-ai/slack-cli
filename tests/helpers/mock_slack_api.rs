use serde_json::json;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};
use mcp_slack::slack::types::{SlackUser, SlackChannel, SlackMessage};

/// Wrapper around wiremock::MockServer with Slack-specific helpers
pub struct MockSlackApiServer {
    server: MockServer,
}

impl MockSlackApiServer {
    /// Start new mock server on random port
    pub async fn start() -> Self {
        let server = MockServer::start().await;
        Self { server }
    }

    /// Get base URL for configuring client
    pub fn url(&self) -> String {
        self.server.uri()
    }

    /// Get reference to inner MockServer for advanced usage
    pub fn server(&self) -> &MockServer {
        &self.server
    }

    /// Mount mock for users.list endpoint
    pub async fn mock_users_list(&self, users: Vec<SlackUser>) {
        Mock::given(method("GET"))
            .and(path("/api/users.list"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "members": users,
            })))
            .mount(&self.server)
            .await;
    }

    /// Mount mock for conversations.list endpoint
    pub async fn mock_channels_list(&self, channels: Vec<SlackChannel>) {
        Mock::given(method("GET"))
            .and(path("/api/conversations.list"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "channels": channels,
            })))
            .mount(&self.server)
            .await;
    }

    /// Mount mock for chat.postMessage endpoint
    pub async fn mock_post_message(&self, response_ts: &str) {
        Mock::given(method("POST"))
            .and(path("/api/chat.postMessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "ts": response_ts,
                "channel": "C123456",
            })))
            .mount(&self.server)
            .await;
    }

    /// Mount mock for conversations.history endpoint
    pub async fn mock_channel_history(&self, messages: Vec<SlackMessage>) {
        Mock::given(method("GET"))
            .and(path("/api/conversations.history"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "messages": messages,
                "has_more": false,
            })))
            .mount(&self.server)
            .await;
    }

    /// Mount mock for conversations.replies endpoint
    pub async fn mock_thread_replies(&self, messages: Vec<SlackMessage>) {
        Mock::given(method("GET"))
            .and(path("/api/conversations.replies"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "messages": messages,
                "has_more": false,
            })))
            .mount(&self.server)
            .await;
    }

    /// Mount mock for rate limit (429) response
    pub async fn mock_rate_limit(&self, retry_after: u64) {
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("Retry-After", retry_after.to_string())
                    .set_body_json(json!({
                        "ok": false,
                        "error": "rate_limited",
                    })),
            )
            .mount(&self.server)
            .await;
    }

    /// Mount mock for error response
    pub async fn mock_error(&self, endpoint: &str, error_code: &str) {
        Mock::given(method("GET"))
            .and(path(endpoint))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": false,
                "error": error_code,
            })))
            .mount(&self.server)
            .await;
    }

    /// Mount mock for conversations.members endpoint
    pub async fn mock_channel_members(&self, user_ids: Vec<String>) {
        Mock::given(method("GET"))
            .and(path("/api/conversations.members"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "members": user_ids,
            })))
            .mount(&self.server)
            .await;
    }

    /// Mount mock for search.messages endpoint
    pub async fn mock_search_messages(&self, messages: Vec<SlackMessage>) {
        Mock::given(method("GET"))
            .and(path("/api/search.messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok": true,
                "messages": {
                    "matches": messages,
                    "total": messages.len(),
                },
            })))
            .mount(&self.server)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::MockUserBuilder;

    #[tokio::test]
    async fn test_mock_server_starts() {
        let server = MockSlackApiServer::start().await;
        assert!(!server.url().is_empty());
        assert!(server.url().starts_with("http://"));
    }

    #[tokio::test]
    async fn test_mock_users_list() {
        let server = MockSlackApiServer::start().await;
        let user = MockUserBuilder::new().id("U123").build();

        server.mock_users_list(vec![user.clone()]).await;

        // Test that mock is mounted by making a request
        let client = reqwest::Client::new();
        let response = client
            .get(format!("{}/api/users.list", server.url()))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["ok"], true);
        assert_eq!(body["members"][0]["id"], "U123");
    }

    #[tokio::test]
    async fn test_mock_rate_limit() {
        let server = MockSlackApiServer::start().await;
        server.mock_rate_limit(60).await;

        let client = reqwest::Client::new();
        let response = client
            .get(format!("{}/api/users.list", server.url()))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 429);
        assert_eq!(response.headers().get("Retry-After").unwrap(), "60");
    }
}
