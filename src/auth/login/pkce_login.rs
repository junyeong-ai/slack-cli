use std::time::Duration;

use anyhow::{Result, anyhow};
use chrono::Utc;

use crate::auth::method::AuthMethod;
use crate::auth::oauth::callback::LoopbackReceiver;
use crate::auth::oauth::exchange::TokenExchange;
use crate::auth::oauth::flow::{PkceRunOptions, run_pkce};
use crate::auth::oauth::scopes::REQUIRED_USER_SCOPES;
use crate::auth::profile::{Profile, TokenSet, WorkspaceInfo};

pub struct Request {
    pub client_id: String,
    pub api_base_url: String,
    pub port: u16,
    pub no_browser: bool,
}

pub async fn run(request: Request) -> Result<Profile> {
    let receiver = LoopbackReceiver::bind(request.port).await?;
    let exchange = TokenExchange {
        api_base_url: request.api_base_url,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?,
    };

    let response = run_pkce(
        &request.client_id,
        receiver,
        exchange,
        PkceRunOptions {
            no_browser: request.no_browser,
            callback_timeout: Duration::from_secs(300),
        },
    )
    .await?;

    let user_token = response
        .user_token
        .ok_or_else(|| anyhow!("Slack did not return a user token"))?;

    let scopes = if response.scopes.is_empty() {
        REQUIRED_USER_SCOPES
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    } else {
        response.scopes
    };

    Ok(Profile {
        method: AuthMethod::Pkce,
        workspace: WorkspaceInfo {
            team_id: response.team_id,
            team_name: response.team_name,
            user_id: response.user_id,
        },
        tokens: TokenSet {
            user: Some(user_token),
            bot: response.bot_token,
        },
        scopes,
        client_id: Some(request.client_id),
        authorized_at: Utc::now(),
    })
}
