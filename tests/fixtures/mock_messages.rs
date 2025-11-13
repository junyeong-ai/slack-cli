use fake::faker::lorem::en::*;
use fake::{Fake, Faker};
use mcp_slack::slack::types::{MessageChannel, SlackMessage};

/// Error type for MockMessageBuilder
#[derive(Debug)]
pub enum BuildError {
    MissingUser,
    MissingChannel,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::MissingUser => write!(f, "user field is required"),
            BuildError::MissingChannel => write!(f, "channel field is required"),
        }
    }
}

impl std::error::Error for BuildError {}

/// Fluent builder for creating SlackMessage test fixtures
pub struct MockMessageBuilder {
    ts: Option<String>,
    user: Option<String>,
    text: Option<String>,
    thread_ts: Option<String>,
    channel: Option<MessageChannel>,
    reply_count: Option<i32>,
}

impl MockMessageBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            ts: None,
            user: None,
            text: None,
            thread_ts: None,
            channel: None,
            reply_count: None,
        }
    }

    /// Set timestamp (defaults to current time)
    pub fn ts(mut self, ts: impl Into<String>) -> Self {
        self.ts = Some(ts.into());
        self
    }

    /// Set user ID (required)
    pub fn from_user(mut self, user_id: impl Into<String>) -> Self {
        self.user = Some(user_id.into());
        self
    }

    /// Set message text (defaults to fake sentence)
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Set as thread reply
    pub fn in_thread(mut self, thread_ts: impl Into<String>) -> Self {
        self.thread_ts = Some(thread_ts.into());
        self
    }

    /// Set channel (required)
    pub fn in_channel(mut self, channel_id: impl Into<String>, channel_name: impl Into<String>) -> Self {
        self.channel = Some(MessageChannel {
            id: channel_id.into(),
            name: channel_name.into(),
        });
        self
    }

    /// Set reply count for thread parent
    pub fn with_reply_count(mut self, count: i32) -> Self {
        self.reply_count = Some(count);
        self
    }

    /// Build the SlackMessage
    pub fn build(self) -> Result<SlackMessage, BuildError> {
        let user = self.user.ok_or(BuildError::MissingUser)?;
        let channel = self.channel.ok_or(BuildError::MissingChannel)?;

        let ts = self.ts.unwrap_or_else(|| {
            let now = chrono::Utc::now().timestamp();
            let micros = chrono::Utc::now().timestamp_subsec_micros();
            format!("{}.{:06}", now, micros)
        });

        let text = self.text.unwrap_or_else(|| Sentence(3..8).fake());

        Ok(SlackMessage {
            ts,
            user: Some(user),
            text,
            channel: Some(channel),
            thread_ts: self.thread_ts,
            reply_count: self.reply_count,
            reply_users: None,
            reply_users_count: None,
            latest_reply: None,
            parent_user_id: None,
            reactions: None,
            subtype: None,
            edited: None,
            blocks: None,
            attachments: None,
        })
    }
}

impl Default for MockMessageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_message_builder_valid() {
        let msg = MockMessageBuilder::new()
            .from_user("U123456")
            .in_channel("C789012", "general")
            .text("Hello world")
            .build()
            .unwrap();

        assert_eq!(msg.user.as_deref(), Some("U123456"));
        assert_eq!(msg.text, "Hello world");
        assert_eq!(msg.channel.as_ref().unwrap().id, "C789012");
        assert!(!msg.ts.is_empty());
    }

    #[test]
    fn test_mock_message_builder_missing_user() {
        let result = MockMessageBuilder::new()
            .in_channel("C789012", "general")
            .build();

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BuildError::MissingUser));
    }

    #[test]
    fn test_mock_message_builder_missing_channel() {
        let result = MockMessageBuilder::new().from_user("U123456").build();

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BuildError::MissingChannel));
    }

    #[test]
    fn test_mock_message_builder_thread() {
        let msg = MockMessageBuilder::new()
            .from_user("U123456")
            .in_channel("C789012", "general")
            .in_thread("1234567890.123456")
            .text("Thread reply")
            .build()
            .unwrap();

        assert_eq!(msg.thread_ts.as_deref(), Some("1234567890.123456"));
        assert_eq!(msg.text, "Thread reply");
    }

    #[test]
    fn test_mock_message_builder_with_replies() {
        let msg = MockMessageBuilder::new()
            .from_user("U123456")
            .in_channel("C789012", "general")
            .text("Parent message")
            .with_reply_count(5)
            .build()
            .unwrap();

        assert_eq!(msg.reply_count, Some(5));
    }
}
