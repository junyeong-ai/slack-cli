use std::fmt;

use super::secret::Secret;

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

    /// Resolves a token from the user/bot pair according to this policy.
    /// Returns `None` when no candidate matches.
    pub fn pick(&self, user: Option<Secret>, bot: Option<Secret>) -> Option<Secret> {
        match self {
            Self::UserRequired => user,
            Self::UserPreferred => user.or(bot),
            Self::BotPreferred => bot.or(user),
        }
    }
}

impl fmt::Display for TokenPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
