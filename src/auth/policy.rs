use std::fmt;

use super::credential::TokenKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenPolicy {
    BotPreferred,
    UserPreferred,
    UserRequired,
    /// Satisfied only by the app-level token. `apps.connections.open` is the
    /// whole of this surface: it authorizes a Socket Mode connection, not a
    /// workspace, so no installation credential can ever stand in for it.
    AppRequired,
}

impl TokenPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BotPreferred => "bot_preferred",
            Self::UserPreferred => "user_preferred",
            Self::UserRequired => "user_required",
            Self::AppRequired => "app_required",
        }
    }

    /// Whether a token of this kind may ever satisfy the policy. This is what
    /// decides which scopes an installation needs to request for each token.
    ///
    /// The app axis is disjoint from the installation axes in both
    /// directions: an app-level token satisfies nothing but `AppRequired`,
    /// and `AppRequired` accepts nothing else. That disjointness is what
    /// keeps `connections:write` out of the OAuth request and keeps every
    /// installation scope out of what an app-level token is asked to carry.
    pub const fn accepts(&self, kind: TokenKind) -> bool {
        match (self, kind) {
            (Self::AppRequired, TokenKind::App) => true,
            (Self::AppRequired, _) | (_, TokenKind::App) => false,
            (Self::UserRequired, TokenKind::Bot) => false,
            _ => true,
        }
    }

    /// The token kind to use given which kinds are held, or `None` when none
    /// of them can satisfy the policy. `holds` is a predicate rather than a
    /// pair of flags so the same decision serves a stored `TokenSet` and the
    /// environment overrides without either shape leaking into the other.
    pub fn select(&self, holds: impl Fn(TokenKind) -> bool) -> Option<TokenKind> {
        let order: &[TokenKind] = match self {
            Self::BotPreferred => &[TokenKind::Bot, TokenKind::User],
            Self::UserPreferred => &[TokenKind::User, TokenKind::Bot],
            Self::UserRequired => &[TokenKind::User],
            Self::AppRequired => &[TokenKind::App],
        };
        order.iter().copied().find(|kind| holds(*kind))
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

    /// Reads as the old positional form did, so the intent of each case stays
    /// legible: which of the installation kinds a profile holds.
    fn held(kinds: &[TokenKind]) -> impl Fn(TokenKind) -> bool + '_ {
        move |kind| kinds.contains(&kind)
    }

    #[test]
    fn user_required_only_ever_selects_a_user_token() {
        assert_eq!(
            TokenPolicy::UserRequired.select(held(&[TokenKind::User, TokenKind::Bot])),
            Some(TokenKind::User)
        );
        assert_eq!(
            TokenPolicy::UserRequired.select(held(&[TokenKind::Bot])),
            None
        );
    }

    #[test]
    fn preferences_fall_back_to_the_other_kind() {
        assert_eq!(
            TokenPolicy::UserPreferred.select(held(&[TokenKind::Bot])),
            Some(TokenKind::Bot)
        );
        assert_eq!(
            TokenPolicy::BotPreferred.select(held(&[TokenKind::User])),
            Some(TokenKind::User)
        );
    }

    #[test]
    fn preferences_win_when_both_kinds_are_held() {
        let both = [TokenKind::User, TokenKind::Bot];
        assert_eq!(
            TokenPolicy::BotPreferred.select(held(&both)),
            Some(TokenKind::Bot)
        );
        assert_eq!(
            TokenPolicy::UserPreferred.select(held(&both)),
            Some(TokenKind::User)
        );
    }

    #[test]
    fn no_kind_is_selected_from_an_empty_profile() {
        for policy in [
            TokenPolicy::BotPreferred,
            TokenPolicy::UserPreferred,
            TokenPolicy::UserRequired,
            TokenPolicy::AppRequired,
        ] {
            assert_eq!(policy.select(held(&[])), None);
        }
    }

    #[test]
    fn user_required_methods_never_accept_a_bot_token() {
        assert!(TokenPolicy::UserRequired.accepts(TokenKind::User));
        assert!(!TokenPolicy::UserRequired.accepts(TokenKind::Bot));
        assert!(TokenPolicy::BotPreferred.accepts(TokenKind::Bot));
        assert!(TokenPolicy::UserPreferred.accepts(TokenKind::Bot));
    }

    /// The invariant that keeps `connections:write` out of `auth login`: no
    /// installation policy reaches the app axis, and the app policy reaches
    /// nothing else. Without it `scopes::required` would union an app-level
    /// scope into the OAuth request, which Slack refuses as a whole.
    #[test]
    fn the_app_axis_is_disjoint_from_the_installation_axes() {
        for policy in [
            TokenPolicy::BotPreferred,
            TokenPolicy::UserPreferred,
            TokenPolicy::UserRequired,
        ] {
            assert!(
                !policy.accepts(TokenKind::App),
                "{policy} reached the app axis"
            );
        }

        assert!(TokenPolicy::AppRequired.accepts(TokenKind::App));
        assert!(!TokenPolicy::AppRequired.accepts(TokenKind::User));
        assert!(!TokenPolicy::AppRequired.accepts(TokenKind::Bot));
    }

    #[test]
    fn app_required_selects_only_the_app_token() {
        assert_eq!(
            TokenPolicy::AppRequired.select(held(&[TokenKind::App])),
            Some(TokenKind::App)
        );
        assert_eq!(
            TokenPolicy::AppRequired.select(held(&[TokenKind::User, TokenKind::Bot])),
            None
        );
    }
}
