pub fn is_conversation_id(input: &str) -> bool {
    is_id_with_prefix(input, |c| matches!(c, 'C' | 'D' | 'G'))
}

pub fn is_user_id(input: &str) -> bool {
    is_id_with_prefix(input, |c| matches!(c, 'U' | 'W'))
}

fn is_id_with_prefix(input: &str, allow: impl Fn(char) -> bool) -> bool {
    let mut chars = input.chars();
    match chars.next() {
        Some(first) if allow(first) => {}
        _ => return false,
    }
    chars.clone().count() >= 8 && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

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

    /// The scope that lets a token list conversations of this type.
    pub const fn read_scope(self) -> &'static str {
        match self {
            Self::PublicChannel => "channels:read",
            Self::PrivateChannel => "groups:read",
            Self::Mpim => "mpim:read",
            Self::Im => "im:read",
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
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub files: Option<Vec<MessageFile>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permalink: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<MessageMetadata>,
}

/// A file shared in a message, reduced to what identifies it. Slack's file
/// object also carries a dozen thumbnail and download URLs; `mode` is kept
/// because a deleted (`tombstone`) or plan-limited (`hidden_by_limit`) file
/// arrives without a name or permalink.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageFile {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mimetype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub permalink: Option<String>,
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
    fn message_files_keep_what_identifies_them() {
        let msg: SlackMessage = serde_json::from_value(json!({
            "ts": "1700000000.000100",
            "text": "",
            "files": [
                {
                    "id": "F1",
                    "name": "report.pdf",
                    "title": "Q3 report",
                    "mimetype": "application/pdf",
                    "size": 52341,
                    "mode": "hosted",
                    "permalink": "https://acme.slack.com/files/U1/F1/report.pdf",
                    "url_private": "https://files.slack.com/files-pri/T1-F1/report.pdf",
                    "thumb_pdf": "https://files.slack.com/files-tmb/T1-F1/report_thumb_pdf.png"
                },
                {"id": "F2", "mode": "tombstone"}
            ],
        }))
        .unwrap();

        let out = serde_json::to_value(&msg).unwrap();
        assert_eq!(
            out["files"],
            json!([
                {
                    "id": "F1",
                    "name": "report.pdf",
                    "title": "Q3 report",
                    "mimetype": "application/pdf",
                    "size": 52341,
                    "mode": "hosted",
                    "permalink": "https://acme.slack.com/files/U1/F1/report.pdf"
                },
                {"id": "F2", "mode": "tombstone"}
            ])
        );
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
