use std::collections::HashMap;
use std::sync::LazyLock;

pub use crate::auth::TokenKind;
pub use crate::auth::TokenPolicy;

#[derive(Debug, Clone, Copy)]
pub enum RequestEncoding {
    /// GET with the arguments in the query string.
    Query,
    /// POST with a JSON body, which is what most write methods take.
    Json,
    /// POST with a form-encoded body. Slack documents this content type for a
    /// handful of methods, and sends the rest of the API through JSON quite
    /// happily — so this exists for the ones where the documented contract is
    /// the only thing to go on.
    Form,
}

#[derive(Debug, Clone, Copy)]
pub struct RatePolicy {
    pub requests_per_minute: u32,
    pub max_page_limit: Option<usize>,
}

/// The scopes a token must carry for this CLI's use of a method. Where the
/// CLI always sends an optional argument that widens the requirement — the
/// message metadata on conversation reads, the email field on users — the
/// scope behind it belongs here too.
#[derive(Debug, Clone, Copy)]
pub struct MethodScopes {
    pub user: &'static [&'static str],
    pub bot: &'static [&'static str],
    /// Scopes an app-level token carries. Its namespace is disjoint from the
    /// installation ones and it is never part of an OAuth grant, so it lives
    /// on its own axis: a scope written here can never leak into what
    /// `auth login` asks Slack for.
    pub app: &'static [&'static str],
}

impl MethodScopes {
    pub const fn shared(scopes: &'static [&'static str]) -> Self {
        Self {
            user: scopes,
            bot: scopes,
            app: &[],
        }
    }

    /// Scopes for the installation axes, leaving the app axis empty.
    pub const fn installation(user: &'static [&'static str], bot: &'static [&'static str]) -> Self {
        Self {
            user,
            bot,
            app: &[],
        }
    }

    pub const fn app_level(scopes: &'static [&'static str]) -> Self {
        Self {
            user: &[],
            bot: &[],
            app: scopes,
        }
    }

    pub fn of(&self, kind: TokenKind) -> &'static [&'static str] {
        match kind {
            TokenKind::User => self.user,
            TokenKind::Bot => self.bot,
            TokenKind::App => self.app,
        }
    }
}

pub struct ApiConfig {
    pub encoding: RequestEncoding,
    pub token_policy: TokenPolicy,
    pub rate_policy: RatePolicy,
    pub scopes: MethodScopes,
}

const fn rate(requests_per_minute: u32, max_page_limit: Option<usize>) -> RatePolicy {
    RatePolicy {
        requests_per_minute,
        max_page_limit,
    }
}

const UNSCOPED: MethodScopes = MethodScopes::shared(&[]);
/// Slack supports `metadata.message:read` on bot tokens only, so requesting it
/// as a user scope makes the whole authorization fail with "Invalid permissions
/// requested" — the grant is refused as a set, not trimmed to what is valid.
const HISTORY: MethodScopes = MethodScopes::installation(
    &[
        "channels:history",
        "groups:history",
        "im:history",
        "mpim:history",
    ],
    &[
        "channels:history",
        "groups:history",
        "im:history",
        "mpim:history",
        "metadata.message:read",
    ],
);
const CONVERSATIONS: MethodScopes =
    MethodScopes::shared(&["channels:read", "groups:read", "im:read", "mpim:read"]);
const DIRECTORY: MethodScopes = MethodScopes::shared(&["users:read", "users:read.email"]);
const WRITE_MESSAGES: MethodScopes = MethodScopes::shared(&["chat:write"]);
const READ_REACTIONS: MethodScopes = MethodScopes::shared(&["reactions:read"]);
const WRITE_REACTIONS: MethodScopes = MethodScopes::shared(&["reactions:write"]);
const READ_PINS: MethodScopes = MethodScopes::shared(&["pins:read"]);
const WRITE_PINS: MethodScopes = MethodScopes::shared(&["pins:write"]);
const READ_BOOKMARKS: MethodScopes = MethodScopes::shared(&["bookmarks:read"]);
const WRITE_BOOKMARKS: MethodScopes = MethodScopes::shared(&["bookmarks:write"]);
const READ_EMOJI: MethodScopes = MethodScopes::shared(&["emoji:read"]);
const SEARCH: MethodScopes = MethodScopes::installation(
    &[
        "search:read.files",
        "search:read.im",
        "search:read.mpim",
        "search:read.private",
        "search:read.public",
        "search:read.users",
    ],
    &[
        "search:read.files",
        "search:read.public",
        "search:read.users",
    ],
);
const SEARCH_CAPABILITIES: MethodScopes = MethodScopes::shared(&["search:read.public"]);
/// The one scope an app-level token can hold. It authorizes opening a Socket
/// Mode connection and nothing else, which is why it never joins an OAuth
/// request — Slack refuses a grant that mixes it with installation scopes.
const SOCKET_MODE: MethodScopes = MethodScopes::app_level(&["connections:write"]);

/// Every Slack method this CLI calls, with the request shape, token policy,
/// rate ceiling and scope requirement each one carries. Nothing about a method
/// is declared anywhere else.
pub const API_METHODS: &[(&str, ApiConfig)] = &[
    (
        "conversations.history",
        ApiConfig {
            encoding: RequestEncoding::Query,
            token_policy: TokenPolicy::UserPreferred,
            rate_policy: rate(50, Some(999)),
            scopes: HISTORY,
        },
    ),
    (
        "conversations.replies",
        ApiConfig {
            encoding: RequestEncoding::Query,
            token_policy: TokenPolicy::UserPreferred,
            rate_policy: rate(50, Some(1000)),
            scopes: HISTORY,
        },
    ),
    (
        "conversations.members",
        ApiConfig {
            encoding: RequestEncoding::Query,
            token_policy: TokenPolicy::UserPreferred,
            rate_policy: rate(20, Some(1000)),
            scopes: CONVERSATIONS,
        },
    ),
    (
        "conversations.list",
        ApiConfig {
            encoding: RequestEncoding::Query,
            token_policy: TokenPolicy::UserPreferred,
            rate_policy: rate(20, Some(1000)),
            scopes: CONVERSATIONS,
        },
    ),
    (
        "users.list",
        ApiConfig {
            encoding: RequestEncoding::Query,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(20, Some(200)),
            scopes: DIRECTORY,
        },
    ),
    (
        "chat.postMessage",
        ApiConfig {
            encoding: RequestEncoding::Json,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(60, None),
            scopes: WRITE_MESSAGES,
        },
    ),
    (
        "chat.update",
        ApiConfig {
            encoding: RequestEncoding::Json,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(60, None),
            scopes: WRITE_MESSAGES,
        },
    ),
    (
        "chat.delete",
        ApiConfig {
            encoding: RequestEncoding::Json,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(60, None),
            scopes: WRITE_MESSAGES,
        },
    ),
    (
        "chat.getPermalink",
        ApiConfig {
            encoding: RequestEncoding::Query,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(100, None),
            scopes: UNSCOPED,
        },
    ),
    (
        "reactions.add",
        ApiConfig {
            encoding: RequestEncoding::Json,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(20, None),
            scopes: WRITE_REACTIONS,
        },
    ),
    (
        "reactions.remove",
        ApiConfig {
            encoding: RequestEncoding::Json,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(20, None),
            scopes: WRITE_REACTIONS,
        },
    ),
    (
        "reactions.get",
        ApiConfig {
            encoding: RequestEncoding::Query,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(20, None),
            scopes: READ_REACTIONS,
        },
    ),
    (
        "pins.add",
        ApiConfig {
            encoding: RequestEncoding::Json,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(20, None),
            scopes: WRITE_PINS,
        },
    ),
    (
        "pins.remove",
        ApiConfig {
            encoding: RequestEncoding::Json,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(20, None),
            scopes: WRITE_PINS,
        },
    ),
    (
        "pins.list",
        ApiConfig {
            encoding: RequestEncoding::Query,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(20, None),
            scopes: READ_PINS,
        },
    ),
    (
        "bookmarks.add",
        ApiConfig {
            encoding: RequestEncoding::Json,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(20, None),
            scopes: WRITE_BOOKMARKS,
        },
    ),
    (
        "bookmarks.remove",
        ApiConfig {
            encoding: RequestEncoding::Json,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(20, None),
            scopes: WRITE_BOOKMARKS,
        },
    ),
    (
        "bookmarks.list",
        ApiConfig {
            encoding: RequestEncoding::Query,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(20, None),
            scopes: READ_BOOKMARKS,
        },
    ),
    (
        "emoji.list",
        ApiConfig {
            encoding: RequestEncoding::Query,
            token_policy: TokenPolicy::BotPreferred,
            rate_policy: rate(20, None),
            scopes: READ_EMOJI,
        },
    ),
    (
        "assistant.search.context",
        ApiConfig {
            encoding: RequestEncoding::Json,
            token_policy: TokenPolicy::UserRequired,
            rate_policy: rate(10, Some(20)),
            scopes: SEARCH,
        },
    ),
    (
        "assistant.search.info",
        ApiConfig {
            encoding: RequestEncoding::Json,
            token_policy: TokenPolicy::UserPreferred,
            rate_policy: rate(20, None),
            scopes: SEARCH_CAPABILITIES,
        },
    ),
    (
        "auth.test",
        ApiConfig {
            encoding: RequestEncoding::Json,
            token_policy: TokenPolicy::UserPreferred,
            rate_policy: rate(60, None),
            scopes: UNSCOPED,
        },
    ),
    (
        "auth.revoke",
        ApiConfig {
            encoding: RequestEncoding::Json,
            token_policy: TokenPolicy::UserPreferred,
            rate_policy: rate(60, None),
            scopes: UNSCOPED,
        },
    ),
    (
        "apps.connections.open",
        ApiConfig {
            // The one method the daemon cannot start without, and the one
            // Slack documents as form-encoded. Nothing else in the CLI depends
            // on a single call this way, so it follows the documented contract
            // rather than the convention.
            encoding: RequestEncoding::Form,
            token_policy: TokenPolicy::AppRequired,
            rate_policy: rate(20, None),
            scopes: SOCKET_MODE,
        },
    ),
];

pub static API_CONFIGS: LazyLock<HashMap<&'static str, &'static ApiConfig>> =
    LazyLock::new(|| API_METHODS.iter().map(|(name, c)| (*name, c)).collect());

pub fn get_api_config(method: &str) -> Option<&'static ApiConfig> {
    API_CONFIGS.get(method).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_method_is_registered_exactly_once() {
        let mut names: Vec<&str> = API_METHODS.iter().map(|(name, _)| *name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
        assert_eq!(API_CONFIGS.len(), total);
    }

    #[test]
    fn user_required_methods_declare_no_reachable_bot_scopes() {
        for (name, config) in API_METHODS {
            if matches!(config.token_policy, TokenPolicy::UserRequired) {
                assert!(
                    !config.token_policy.accepts(TokenKind::Bot),
                    "{name} would contribute unusable bot scopes"
                );
            }
        }
    }

    /// The app axis carries `connections:write` and the installation axes
    /// carry none of it. This is the registry half of the invariant that keeps
    /// an app-level scope out of the authorization request.
    #[test]
    fn socket_mode_declares_its_scope_on_the_app_axis_alone() {
        let open = get_api_config("apps.connections.open").unwrap();
        assert_eq!(open.scopes.app, &["connections:write"]);
        assert!(open.scopes.user.is_empty());
        assert!(open.scopes.bot.is_empty());
        assert_eq!(open.token_policy, TokenPolicy::AppRequired);
    }

    #[test]
    fn no_installation_method_declares_an_app_level_scope() {
        for (name, config) in API_METHODS {
            if !matches!(config.token_policy, TokenPolicy::AppRequired) {
                assert!(
                    config.scopes.app.is_empty(),
                    "{name} declares an app-level scope on an installation method"
                );
            }
        }
    }

    #[test]
    fn search_requires_the_granular_real_time_search_scopes() {
        let search = get_api_config("assistant.search.context").unwrap();
        assert!(search.scopes.user.contains(&"search:read.public"));
        assert!(search.scopes.user.contains(&"search:read.private"));
        assert!(!search.scopes.user.contains(&"search:read"));
    }
}
