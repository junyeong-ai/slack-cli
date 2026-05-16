use super::secret::{self, Secret};

const ENV_USER_TOKEN: &str = "SLACK_USER_TOKEN";
const ENV_BOT_TOKEN: &str = "SLACK_BOT_TOKEN";

#[derive(Debug, Clone, Default)]
pub struct EnvOverrides {
    pub user_token: Option<Secret>,
    pub bot_token: Option<Secret>,
}

impl EnvOverrides {
    pub fn capture() -> Self {
        Self {
            user_token: read_secret(ENV_USER_TOKEN),
            bot_token: read_secret(ENV_BOT_TOKEN),
        }
    }

    pub fn has_inline_tokens(&self) -> bool {
        self.user_token.is_some() || self.bot_token.is_some()
    }
}

fn read_secret(key: &str) -> Option<Secret> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(secret::new)
}
