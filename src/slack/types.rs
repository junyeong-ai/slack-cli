use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ConversationType {
    PublicChannel,
    PrivateChannel,
    Mpim,
    Im,
}

impl ConversationType {
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::PublicChannel => "public_channel",
            Self::PrivateChannel => "private_channel",
            Self::Mpim => "mpim",
            Self::Im => "im",
        }
    }
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
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

/// `conversations.history` emits `channel` either as a bare id string or as
/// an `{id, name}` object depending on the message shape; both deserialize
/// here, and output always serializes the object form.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "MessageChannelRepr")]
pub struct MessageChannel {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum MessageChannelRepr {
    Full {
        id: String,
        #[serde(default)]
        name: Option<String>,
    },
    Id(String),
}

impl From<MessageChannelRepr> for MessageChannel {
    fn from(repr: MessageChannelRepr) -> Self {
        match repr {
            MessageChannelRepr::Full { id, name } => Self { id, name },
            MessageChannelRepr::Id(id) => Self { id, name: None },
        }
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permalink: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<MessageMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditedInfo {
    pub user: String,
    pub ts: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetadata {
    pub event_type: String,
    #[serde(default)]
    pub event_payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reaction {
    pub name: String,
    pub users: Vec<String>,
    pub count: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_channel_deserializes_from_object() {
        let msg: SlackMessage = serde_json::from_value(json!({
            "ts": "1700000000.000100",
            "text": "hi",
            "channel": {"id": "C123", "name": "general"},
        }))
        .unwrap();
        let channel = msg.channel.unwrap();
        assert_eq!(channel.id, "C123");
        assert_eq!(channel.name.as_deref(), Some("general"));
    }

    #[test]
    fn message_channel_deserializes_from_bare_id_string() {
        let msg: SlackMessage = serde_json::from_value(json!({
            "ts": "1700000000.000100",
            "text": "hi",
            "channel": "C048DJ9BWGK",
        }))
        .unwrap();
        let channel = msg.channel.unwrap();
        assert_eq!(channel.id, "C048DJ9BWGK");
        assert_eq!(channel.name, None);
    }

    #[test]
    fn message_channel_serializes_as_object_regardless_of_input() {
        let msg: SlackMessage = serde_json::from_value(json!({
            "ts": "1700000000.000100",
            "text": "hi",
            "channel": "C123",
        }))
        .unwrap();
        let out = serde_json::to_value(&msg).unwrap();
        assert_eq!(out["channel"], json!({"id": "C123"}));
    }
}
