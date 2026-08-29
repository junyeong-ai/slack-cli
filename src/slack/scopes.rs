use crate::slack::api_config::{API_METHODS, TokenKind};

/// The OAuth scopes an installation must grant for a token of this kind to
/// reach every method the CLI calls. Derived from the API registry, so a
/// method added there is impossible to forget here.
pub fn required(kind: TokenKind) -> Vec<&'static str> {
    let mut scopes: Vec<&'static str> = API_METHODS
        .iter()
        .filter(|(_, config)| config.token_policy.accepts(kind))
        .flat_map(|(_, config)| config.scopes.of(kind))
        .copied()
        .collect();
    scopes.sort_unstable();
    scopes.dedup();
    scopes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_are_sorted_and_deduplicated() {
        for kind in [TokenKind::User, TokenKind::Bot] {
            let scopes = required(kind);
            let mut expected = scopes.clone();
            expected.sort_unstable();
            expected.dedup();
            assert_eq!(scopes, expected);
            assert!(!scopes.is_empty());
        }
    }

    #[test]
    fn user_scopes_cover_every_real_time_search_scope() {
        let scopes = required(TokenKind::User);
        for scope in [
            "search:read.files",
            "search:read.im",
            "search:read.mpim",
            "search:read.private",
            "search:read.public",
            "search:read.users",
        ] {
            assert!(scopes.contains(&scope), "missing {scope}");
        }
    }

    #[test]
    fn bot_scopes_exclude_the_user_only_search_surface() {
        let scopes = required(TokenKind::Bot);
        for scope in ["search:read.im", "search:read.mpim", "search:read.private"] {
            assert!(!scopes.contains(&scope), "unusable bot scope {scope}");
        }
    }

    #[test]
    fn the_legacy_search_scope_is_never_requested() {
        for kind in [TokenKind::User, TokenKind::Bot] {
            assert!(!required(kind).contains(&"search:read"));
        }
    }

    /// Slack grants a scope set as a whole, so one scope the token kind cannot
    /// hold fails the entire authorization with "Invalid permissions
    /// requested". `metadata.message:read` is a bot-only scope, and asking for
    /// it as a user scope made every browser login fail.
    #[test]
    fn the_metadata_scope_is_requested_for_bots_only() {
        assert!(required(TokenKind::Bot).contains(&"metadata.message:read"));
        assert!(!required(TokenKind::User).contains(&"metadata.message:read"));
    }
}
