use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, Clone)]
pub enum ApiMethod {
    Get,      // Use GET with query parameters
    PostJson, // Use POST with JSON body
    PostForm, // Use POST with form data
}

pub struct ApiConfig {
    pub method: ApiMethod,
    pub prefer_user_token: bool,
}

impl ApiConfig {
    pub const fn new(method: ApiMethod, prefer_user_token: bool) -> Self {
        Self {
            method,
            prefer_user_token,
        }
    }
}

// Centralized API method configuration
pub static API_CONFIGS: LazyLock<HashMap<&'static str, ApiConfig>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    // GET methods
    m.insert(
        "conversations.history",
        ApiConfig::new(ApiMethod::Get, true),
    ); // Prefer user token for private channel access
    m.insert(
        "conversations.replies",
        ApiConfig::new(ApiMethod::Get, true),
    ); // Prefer user token for private channel access
    m.insert(
        "conversations.members",
        ApiConfig::new(ApiMethod::Get, true),
    ); // Prefer user token for private channel members
    m.insert("users.list", ApiConfig::new(ApiMethod::Get, false));
    m.insert("conversations.list", ApiConfig::new(ApiMethod::Get, true)); // Prefer user token for private channels

    // POST JSON methods
    m.insert(
        "chat.postMessage",
        ApiConfig::new(ApiMethod::PostJson, false),
    );
    m.insert(
        "chat.scheduleMessage",
        ApiConfig::new(ApiMethod::PostJson, false),
    );
    m.insert(
        "conversations.open",
        ApiConfig::new(ApiMethod::PostJson, false),
    );
    m.insert("reactions.add", ApiConfig::new(ApiMethod::PostJson, false));
    m.insert(
        "reactions.remove",
        ApiConfig::new(ApiMethod::PostJson, false),
    );
    m.insert(
        "users.profile.set",
        ApiConfig::new(ApiMethod::PostJson, true),
    ); // Requires user token

    // POST Form methods
    m.insert("search.messages", ApiConfig::new(ApiMethod::PostForm, true));

    m
});

/// Get API configuration for a method
pub fn get_api_config(method: &str) -> Option<&'static ApiConfig> {
    API_CONFIGS.get(method)
}
