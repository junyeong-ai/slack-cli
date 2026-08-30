use crate::slack::api_config::{API_METHODS, TokenKind};

/// The OAuth scopes an installation must grant for a token of this kind to
/// reach every method the CLI calls. Derived from the API registry, so a
/// method added there is impossible to forget here.
/// The scopes `auth login` asks Slack for: everything the kind can reach, less
/// what the installation has excluded. An excluded scope costs the commands
/// that need it; `SlackCore` names it when Slack refuses the call.
pub fn requested(kind: TokenKind, excluded: &[String]) -> Vec<&'static str> {
    required(kind)
        .into_iter()
        .filter(|scope| !excluded.iter().any(|entry| entry == scope))
        .collect()
}

/// Whether any method the CLI calls declares this scope, for either kind. What
/// `exclude_scopes` is validated against, so a name that no method needs is
/// rejected rather than silently ignored.
pub fn is_known(scope: &str) -> bool {
    [TokenKind::User, TokenKind::Bot]
        .into_iter()
        .any(|kind| required(kind).contains(&scope))
}

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
    fn an_excluded_scope_is_not_requested() {
        let full = required(TokenKind::User);
        let dropped = full[0].to_string();
        let asked = requested(TokenKind::User, std::slice::from_ref(&dropped));

        assert_eq!(asked.len(), full.len() - 1);
        assert!(!asked.contains(&dropped.as_str()));
    }

    #[test]
    fn excluding_nothing_asks_for_everything() {
        for kind in [TokenKind::User, TokenKind::Bot] {
            assert_eq!(requested(kind, &[]), required(kind));
        }
    }

    /// `exclude_scopes` is validated against this, so a scope no method needs
    /// is refused rather than silently doing nothing.
    #[test]
    fn only_scopes_some_method_declares_are_known() {
        assert!(is_known("users:read"));
        assert!(is_known("metadata.message:read"));
        assert!(!is_known("users:reed"));
        assert!(!is_known("search:read"));
    }

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
