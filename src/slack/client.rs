use std::sync::Arc;

use crate::config::Config;

use super::channels::SlackChannelClient;
use super::core::SlackCore;
use super::messages::SlackMessageClient;
use super::users::SlackUserClient;

/// Slack client with specialized sub-clients
///
/// Access sub-clients:
/// - `messages` for message operations
/// - `users` for user operations
/// - `channels` for channel operations
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
