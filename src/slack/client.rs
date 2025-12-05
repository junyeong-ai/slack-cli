use std::sync::Arc;

use crate::config::Config;

use super::bookmarks::SlackBookmarkClient;
use super::channels::SlackChannelClient;
use super::core::SlackCore;
use super::emoji::SlackEmojiClient;
use super::messages::SlackMessageClient;
use super::pins::SlackPinClient;
use super::reactions::SlackReactionClient;
use super::users::SlackUserClient;

pub struct SlackClient {
    pub messages: SlackMessageClient,
    pub users: SlackUserClient,
    pub channels: SlackChannelClient,
    pub reactions: SlackReactionClient,
    pub emoji: SlackEmojiClient,
    pub pins: SlackPinClient,
    pub bookmarks: SlackBookmarkClient,
}

impl SlackClient {
    pub fn new(config: Config) -> Self {
        let core = Arc::new(SlackCore::new(config));

        Self {
            messages: SlackMessageClient::new(core.clone()),
            users: SlackUserClient::new(core.clone()),
            channels: SlackChannelClient::new(core.clone()),
            reactions: SlackReactionClient::new(core.clone()),
            emoji: SlackEmojiClient::new(core.clone()),
            pins: SlackPinClient::new(core.clone()),
            bookmarks: SlackBookmarkClient::new(core),
        }
    }
}
