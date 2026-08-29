use std::fmt;

use super::credential::TokenKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenPolicy {
    BotPreferred,
    UserPreferred,
    UserRequired,
}

impl TokenPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BotPreferred => "bot_preferred",
            Self::UserPreferred => "user_preferred",
            Self::UserRequired => "user_required",
        }
    }

    /// Whether a token of this kind may ever satisfy the policy. This is what
    /// decides which scopes an installation needs to request for each token.
    pub const fn accepts(&self, kind: TokenKind) -> bool {
        !matches!((self, kind), (Self::UserRequired, TokenKind::Bot))
    }

    /// The token kind to use given which kinds a profile holds, or `None` when
    /// the profile can not satisfy the policy at all.
    pub fn select(&self, has_user: bool, has_bot: bool) -> Option<TokenKind> {
        let (first, second) = match self {
            Self::BotPreferred => (TokenKind::Bot, Some(TokenKind::User)),
            Self::UserPreferred => (TokenKind::User, Some(TokenKind::Bot)),
            Self::UserRequired => (TokenKind::User, None),
        };
        let holds = |kind: TokenKind| match kind {
            TokenKind::User => has_user,
            TokenKind::Bot => has_bot,
        };
        Some(first)
            .filter(|kind| holds(*kind))
            .or_else(|| second.filter(|kind| holds(*kind)))
    }
}

impl fmt::Display for TokenPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_required_only_ever_selects_a_user_token() {
        assert_eq!(
            TokenPolicy::UserRequired.select(true, true),
            Some(TokenKind::User)
        );
        assert_eq!(TokenPolicy::UserRequired.select(false, true), None);
    }

    #[test]
    fn preferences_fall_back_to_the_other_kind() {
        assert_eq!(
            TokenPolicy::UserPreferred.select(false, true),
            Some(TokenKind::Bot)
        );
        assert_eq!(
            TokenPolicy::BotPreferred.select(true, false),
            Some(TokenKind::User)
        );
    }

    #[test]
    fn preferences_win_when_both_kinds_are_held() {
        assert_eq!(
            TokenPolicy::BotPreferred.select(true, true),
            Some(TokenKind::Bot)
        );
        assert_eq!(
            TokenPolicy::UserPreferred.select(true, true),
            Some(TokenKind::User)
        );
    }

    #[test]
    fn no_kind_is_selected_from_an_empty_profile() {
        for policy in [
            TokenPolicy::BotPreferred,
            TokenPolicy::UserPreferred,
            TokenPolicy::UserRequired,
        ] {
            assert_eq!(policy.select(false, false), None);
        }
    }

    #[test]
    fn user_required_methods_never_accept_a_bot_token() {
        assert!(TokenPolicy::UserRequired.accepts(TokenKind::User));
        assert!(!TokenPolicy::UserRequired.accepts(TokenKind::Bot));
        assert!(TokenPolicy::BotPreferred.accepts(TokenKind::Bot));
        assert!(TokenPolicy::UserPreferred.accepts(TokenKind::Bot));
    }
}
