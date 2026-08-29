use std::time::Duration;

use anyhow::Result;
use chrono::Utc;

use crate::auth::credential::TokenSet;
use crate::auth::method::AuthMethod;
use crate::auth::oauth::callback::LoopbackReceiver;
use crate::auth::oauth::client::OAuthClient;
use crate::auth::oauth::exchange::TokenExchange;
use crate::auth::oauth::flow::Authorization;
use crate::auth::profile::{Profile, WorkspaceInfo};

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Request {
    pub client: OAuthClient,
    pub api_base_url: String,
    pub port: u16,
    pub no_browser: bool,
    pub user_scopes: Vec<String>,
    pub bot_scopes: Vec<String>,
}

pub async fn run(request: Request) -> Result<Profile> {
    let receiver = LoopbackReceiver::bind(request.port).await?;
    let exchange = TokenExchange {
        api_base_url: request.api_base_url,
        http: reqwest::Client::builder()
            .timeout(EXCHANGE_TIMEOUT)
            .build()?,
    };

    let response = Authorization {
        client: &request.client,
        user_scopes: &request.user_scopes,
        bot_scopes: &request.bot_scopes,
        no_browser: request.no_browser,
        callback_timeout: CALLBACK_TIMEOUT,
    }
    .run(receiver, exchange)
    .await?;

    let issued_at = Utc::now();
    let team = response.team()?.clone();

    Ok(Profile {
        method: AuthMethod::for_client(&request.client),
        workspace: WorkspaceInfo {
            team_id: team.id,
            team_name: team.name,
            user_id: response.user_id,
        },
        tokens: TokenSet {
            user: response.user.map(|token| token.into_credential(issued_at)),
            bot: response.bot.map(|token| token.into_credential(issued_at)),
        },
        client: Some(request.client),
        authorized_at: issued_at,
    })
}
