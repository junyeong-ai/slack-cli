use std::sync::Arc;

use anyhow::Result;

use crate::auth::Authenticator;
use crate::config::Config;

use super::auth::SlackAuthClient;
use super::bookmarks::SlackBookmarkClient;
use super::channels::SlackChannelClient;
use super::core::SlackCore;
use super::emoji::SlackEmojiClient;
use super::messages::SlackMessageClient;
use super::pins::SlackPinClient;
use super::reactions::SlackReactionClient;
use super::search::SlackSearchClient;
use super::users::SlackUserClient;

pub struct SlackClient {
    pub auth: SlackAuthClient,
    pub messages: SlackMessageClient,
    pub users: SlackUserClient,
    pub channels: SlackChannelClient,
    pub reactions: SlackReactionClient,
    pub emoji: SlackEmojiClient,
    pub pins: SlackPinClient,
    pub bookmarks: SlackBookmarkClient,
    pub search: SlackSearchClient,
}

impl SlackClient {
    pub fn new(config: Config, auth: Arc<Authenticator>) -> Result<Self> {
        let core = Arc::new(SlackCore::new(config, auth)?);

        Ok(Self {
            auth: SlackAuthClient::new(core.clone()),
            messages: SlackMessageClient::new(core.clone()),
            users: SlackUserClient::new(core.clone()),
            channels: SlackChannelClient::new(core.clone()),
            reactions: SlackReactionClient::new(core.clone()),
            emoji: SlackEmojiClient::new(core.clone()),
            pins: SlackPinClient::new(core.clone()),
            bookmarks: SlackBookmarkClient::new(core.clone()),
            search: SlackSearchClient::new(core),
        })
    }
}
