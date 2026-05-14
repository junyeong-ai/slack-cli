use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy)]
pub enum RequestEncoding {
    Query,
    Json,
}

#[derive(Debug, Clone, Copy)]
pub enum TokenPolicy {
    BotPreferred,
    UserPreferred,
    UserRequired,
}

#[derive(Debug, Clone, Copy)]
pub struct RatePolicy {
    pub requests_per_minute: u32,
    pub max_page_limit: Option<usize>,
}

pub struct ApiConfig {
    pub encoding: RequestEncoding,
    pub token_policy: TokenPolicy,
    pub rate_policy: RatePolicy,
}

impl ApiConfig {
    pub const fn new(
        encoding: RequestEncoding,
        token_policy: TokenPolicy,
        requests_per_minute: u32,
        max_page_limit: Option<usize>,
    ) -> Self {
        Self {
            encoding,
            token_policy,
            rate_policy: RatePolicy {
                requests_per_minute,
                max_page_limit,
            },
        }
    }
}

pub static API_CONFIGS: LazyLock<HashMap<&'static str, ApiConfig>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    m.insert(
        "conversations.history",
        ApiConfig::new(
            RequestEncoding::Query,
            TokenPolicy::UserPreferred,
            50,
            Some(999),
        ),
    );
    m.insert(
        "conversations.replies",
        ApiConfig::new(
            RequestEncoding::Query,
            TokenPolicy::UserPreferred,
            50,
            Some(1000),
        ),
    );
    m.insert(
        "conversations.members",
        ApiConfig::new(
            RequestEncoding::Query,
            TokenPolicy::UserPreferred,
            20,
            Some(1000),
        ),
    );
    m.insert(
        "users.list",
        ApiConfig::new(
            RequestEncoding::Query,
            TokenPolicy::BotPreferred,
            20,
            Some(200),
        ),
    );
    m.insert(
        "conversations.list",
        ApiConfig::new(
            RequestEncoding::Query,
            TokenPolicy::UserPreferred,
            20,
            Some(1000),
        ),
    );

    m.insert(
        "chat.postMessage",
        ApiConfig::new(RequestEncoding::Json, TokenPolicy::BotPreferred, 60, None),
    );
    m.insert(
        "chat.update",
        ApiConfig::new(RequestEncoding::Json, TokenPolicy::BotPreferred, 60, None),
    );
    m.insert(
        "chat.delete",
        ApiConfig::new(RequestEncoding::Json, TokenPolicy::BotPreferred, 60, None),
    );

    m.insert(
        "reactions.add",
        ApiConfig::new(RequestEncoding::Json, TokenPolicy::BotPreferred, 20, None),
    );
    m.insert(
        "reactions.remove",
        ApiConfig::new(RequestEncoding::Json, TokenPolicy::BotPreferred, 20, None),
    );
    m.insert(
        "reactions.get",
        ApiConfig::new(RequestEncoding::Query, TokenPolicy::BotPreferred, 20, None),
    );

    m.insert(
        "pins.add",
        ApiConfig::new(RequestEncoding::Json, TokenPolicy::BotPreferred, 20, None),
    );
    m.insert(
        "pins.remove",
        ApiConfig::new(RequestEncoding::Json, TokenPolicy::BotPreferred, 20, None),
    );
    m.insert(
        "pins.list",
        ApiConfig::new(RequestEncoding::Query, TokenPolicy::BotPreferred, 20, None),
    );

    m.insert(
        "bookmarks.add",
        ApiConfig::new(RequestEncoding::Json, TokenPolicy::BotPreferred, 20, None),
    );
    m.insert(
        "bookmarks.remove",
        ApiConfig::new(RequestEncoding::Json, TokenPolicy::BotPreferred, 20, None),
    );
    m.insert(
        "bookmarks.list",
        ApiConfig::new(RequestEncoding::Query, TokenPolicy::BotPreferred, 20, None),
    );

    m.insert(
        "emoji.list",
        ApiConfig::new(RequestEncoding::Query, TokenPolicy::BotPreferred, 20, None),
    );

    m.insert(
        "assistant.search.context",
        ApiConfig::new(
            RequestEncoding::Json,
            TokenPolicy::UserRequired,
            10,
            Some(20),
        ),
    );

    m
});

pub fn get_api_config(method: &str) -> Option<&'static ApiConfig> {
    API_CONFIGS.get(method)
}
