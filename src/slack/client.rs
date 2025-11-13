use std::sync::Arc;

use crate::config::Config;

use super::channels::SlackChannelClient;
use super::core::SlackCore;
use super::messages::SlackMessageClient;
use super::users::SlackUserClient;

/// Unified Slack client with specialized sub-clients
///
/// Access sub-clients directly for their functionality:
/// - `client.messages` for message operations
/// - `client.users` for user operations
/// - `client.channels` for channel operations
pub struct SlackClient {
    pub messages: SlackMessageClient,
    pub users: SlackUserClient,
    pub channels: SlackChannelClient,
}

impl SlackClient {
    pub fn new(config: Config) -> Self {
        let core = Arc::new(SlackCore::new(config));

        Self {
            messages: SlackMessageClient::new(core.clone()),
            users: SlackUserClient::new(core.clone()),
            channels: SlackChannelClient::new(core),
        }
    }
}
