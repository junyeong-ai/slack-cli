use anyhow::{Context, Result};
use chrono::Utc;
use secrecy::ExposeSecret;

use crate::auth::credential::{Credential, TokenSet};
use crate::auth::method::AuthMethod;
use crate::auth::profile::{Profile, WorkspaceInfo};
use crate::auth::secret::Secret;
use crate::slack::{SlackAuthIdentity, SlackClient};

pub async fn run(
    user_token: Option<Secret>,
    bot_token: Option<Secret>,
    app_token: Option<Secret>,
    slack: &SlackClient,
) -> Result<Profile> {
    let mut identity: Option<SlackAuthIdentity> = None;
    for token in [user_token.as_ref(), bot_token.as_ref()]
        .into_iter()
        .flatten()
    {
        let validated = slack.auth.test(token.expose_secret()).await?;
        if let Some(primary) = &identity {
            anyhow::ensure!(
                primary.team_id == validated.team_id,
                "user and bot tokens must belong to the same workspace"
            );
        } else {
            identity = Some(validated);
        }
    }
    let identity = identity.context("at least one of --user-token or --bot-token is required")?;

    Ok(Profile {
        method: AuthMethod::Static,
        workspace: WorkspaceInfo {
            team_id: identity.team_id,
            team_name: identity.team,
            user_id: Some(identity.user_id),
        },
        tokens: TokenSet {
            user: user_token.map(|token| Credential::permanent(token, Vec::new())),
            bot: bot_token.map(|token| Credential::permanent(token, Vec::new())),
            app: app_token.map(crate::auth::app_credential),
        },
        client: None,
        authorized_at: Utc::now(),
    })
}
