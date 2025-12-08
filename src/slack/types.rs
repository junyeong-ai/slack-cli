use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackUserProfile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub real_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_emoji: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "image_72")]
    pub avatar: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "tz")]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackUser {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub deleted: bool,
    pub profile: Option<SlackUserProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackChannel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub is_channel: bool,
    #[serde(default)]
    pub is_private: bool,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub is_general: bool,
    #[serde(default)]
    pub is_im: bool,
    #[serde(default)]
    pub is_mpim: bool,
    #[serde(default)]
    pub is_group: bool,
    #[serde(default)]
    pub is_member: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_members: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<ChannelTopic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<ChannelPurpose>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelTopic {
    pub value: String,
    pub creator: String,
    pub last_set: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPurpose {
    pub value: String,
    pub creator: String,
    pub last_set: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageChannel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackMessage {
    pub ts: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub username: Option<String>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<MessageChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_ts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_users: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_users_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_reply: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reactions: Option<Vec<Reaction>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited: Option<EditedInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditedInfo {
    pub user: String,
    pub ts: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reaction {
    pub name: String,
    pub users: Vec<String>,
    pub count: i32,
}
