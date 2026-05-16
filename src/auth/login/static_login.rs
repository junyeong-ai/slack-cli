use anyhow::{Context, Result};
use chrono::Utc;
use secrecy::ExposeSecret;

use crate::auth::method::AuthMethod;
use crate::auth::profile::{Profile, TokenSet, WorkspaceInfo};
use crate::auth::secret::Secret;
use crate::slack::SlackClient;

pub async fn run(
    user_token: Option<Secret>,
    bot_token: Option<Secret>,
    slack: &SlackClient,
) -> Result<Profile> {
    let validation = user_token
        .as_ref()
        .or(bot_token.as_ref())
        .context("at least one of --user-token or --bot-token is required")?;
    let identity = slack.auth.test(validation.expose_secret()).await?;

    Ok(Profile {
        method: AuthMethod::Static,
        workspace: WorkspaceInfo {
            team_id: identity.team_id,
            team_name: identity.team,
            user_id: Some(identity.user_id),
        },
        tokens: TokenSet {
            user: user_token,
            bot: bot_token,
        },
        scopes: Vec::new(),
        client_id: None,
        authorized_at: Utc::now(),
    })
}
