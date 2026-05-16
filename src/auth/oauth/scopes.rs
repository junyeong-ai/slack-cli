/// User-token scopes requested by `slack-cli auth login --method pkce`.
/// Derived from the set of API methods this CLI calls.
///
/// PKCE + localhost redirects are limited to user scopes by Slack — bot
/// scopes would be rejected by the authorization endpoint.
pub const REQUIRED_USER_SCOPES: &[&str] = &[
    "users:read",
    "users:read.email",
    "channels:read",
    "channels:history",
    "groups:read",
    "groups:history",
    "mpim:read",
    "mpim:history",
    "im:read",
    "im:history",
    "chat:write",
    "reactions:read",
    "reactions:write",
    "pins:read",
    "pins:write",
    "bookmarks:read",
    "bookmarks:write",
    "emoji:read",
    "search:read",
];
