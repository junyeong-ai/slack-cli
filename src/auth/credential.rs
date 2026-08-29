use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::secret::{self, Secret};

/// How long before a recorded expiry a credential is treated as due for
/// renewal. Matches the window used by Slack's own SDKs, and leaves ample
/// room for a request issued now to complete before the deadline.
const RENEWAL_WINDOW: Duration = Duration::minutes(120);

/// One Slack access token together with everything needed to keep it alive.
///
/// `expires_at` is `None` for non-rotating tokens: those never expire and are
/// never renewed. When it is `Some`, `refresh_token` is what exchanges the
/// expiring token for a fresh pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    #[serde(with = "secret::required")]
    pub token: Secret,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "secret::option"
    )]
    pub refresh_token: Option<Secret>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
}

/// What a credential can do for a request issued now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// Usable as it stands.
    Ready,
    /// Close enough to expiry to be exchanged, and holding the refresh token
    /// that exchanges it.
    NeedsRenewal,
    /// Past its expiry with no refresh token to trade in.
    Expired,
}

impl Credential {
    /// A credential that never expires — a pasted token, or one issued by an
    /// app without token rotation.
    pub fn permanent(token: Secret, scopes: Vec<String>) -> Self {
        Self {
            token,
            refresh_token: None,
            expires_at: None,
            scopes,
        }
    }

    pub fn expires(&self) -> bool {
        self.expires_at.is_some()
    }

    /// Whether the token itself is still accepted by Slack, independent of
    /// whether it is due for renewal.
    pub fn is_live(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_none_or(|expiry| now < expiry)
    }

    pub fn readiness(&self, now: DateTime<Utc>) -> Readiness {
        let Some(expiry) = self.expires_at else {
            return Readiness::Ready;
        };
        if self.refresh_token.is_some() && now + RENEWAL_WINDOW >= expiry {
            return Readiness::NeedsRenewal;
        }
        if now >= expiry {
            return Readiness::Expired;
        }
        Readiness::Ready
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    User,
    Bot,
}

impl TokenKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Bot => "bot",
        }
    }
}

impl std::fmt::Display for TokenKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<Credential>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot: Option<Credential>,
}

impl TokenSet {
    pub fn get(&self, kind: TokenKind) -> Option<&Credential> {
        match kind {
            TokenKind::User => self.user.as_ref(),
            TokenKind::Bot => self.bot.as_ref(),
        }
    }

    pub fn set(&mut self, kind: TokenKind, credential: Credential) {
        match kind {
            TokenKind::User => self.user = Some(credential),
            TokenKind::Bot => self.bot = Some(credential),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (TokenKind, &Credential)> {
        [
            (TokenKind::User, self.user.as_ref()),
            (TokenKind::Bot, self.bot.as_ref()),
        ]
        .into_iter()
        .filter_map(|(kind, credential)| credential.map(|c| (kind, c)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expiring_in(minutes: i64) -> Credential {
        Credential {
            token: secret::new("xoxe.xoxp-test"),
            refresh_token: Some(secret::new("xoxe-refresh")),
            expires_at: Some(Utc::now() + Duration::minutes(minutes)),
            scopes: vec![],
        }
    }

    fn without_refresh_token(minutes: i64) -> Credential {
        Credential {
            refresh_token: None,
            ..expiring_in(minutes)
        }
    }

    #[test]
    fn permanent_credentials_are_always_ready() {
        let credential = Credential::permanent(secret::new("xoxp-static"), vec![]);
        assert!(!credential.expires());
        assert_eq!(credential.readiness(Utc::now()), Readiness::Ready);
    }

    #[test]
    fn renewal_is_due_inside_the_window_and_after_expiry() {
        assert_eq!(
            expiring_in(119).readiness(Utc::now()),
            Readiness::NeedsRenewal
        );
        assert_eq!(
            expiring_in(-1).readiness(Utc::now()),
            Readiness::NeedsRenewal
        );
    }

    #[test]
    fn renewal_is_not_due_outside_the_window() {
        assert_eq!(expiring_in(121).readiness(Utc::now()), Readiness::Ready);
        assert_eq!(expiring_in(720).readiness(Utc::now()), Readiness::Ready);
    }

    #[test]
    fn liveness_tracks_the_expiry_alone() {
        let now = Utc::now();
        assert!(Credential::permanent(secret::new("xoxp"), vec![]).is_live(now));
        assert!(expiring_in(1).is_live(now));
        assert!(!expiring_in(-1).is_live(now));
    }

    #[test]
    fn a_credential_that_cannot_be_renewed_is_used_until_it_actually_expires() {
        assert_eq!(
            without_refresh_token(30).readiness(Utc::now()),
            Readiness::Ready
        );
        assert_eq!(
            without_refresh_token(-1).readiness(Utc::now()),
            Readiness::Expired
        );
    }

    #[test]
    fn token_set_round_trips_by_kind() {
        let mut tokens = TokenSet::default();
        tokens.set(TokenKind::Bot, expiring_in(600));
        assert!(tokens.get(TokenKind::User).is_none());
        assert!(tokens.get(TokenKind::Bot).is_some());
        assert_eq!(tokens.iter().count(), 1);
    }
}
