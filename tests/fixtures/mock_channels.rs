use fake::faker::lorem::en::*;
use fake::{Fake, Faker};
use mcp_slack::slack::types::SlackChannel;

/// Fluent builder for creating SlackChannel test fixtures
pub struct MockChannelBuilder {
    id: Option<String>,
    name: Option<String>,
    is_channel: bool,
    is_private: bool,
    is_im: bool,
    is_mpim: bool,
    is_archived: bool,
    is_general: bool,
    num_members: Option<i32>,
}

impl MockChannelBuilder {
    /// Create a new builder with defaults (public channel)
    pub fn new() -> Self {
        Self {
            id: None,
            name: None,
            is_channel: true,
            is_private: false,
            is_im: false,
            is_mpim: false,
            is_archived: false,
            is_general: false,
            num_members: None,
        }
    }

    /// Set channel ID (defaults to random C{number})
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set channel name (defaults to fake word)
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Make private channel
    pub fn private(mut self) -> Self {
        self.is_private = true;
        self.is_im = false;
        self.is_mpim = false;
        self
    }

    /// Make direct message
    pub fn direct_message(mut self) -> Self {
        self.is_im = true;
        self.is_private = false;
        self.is_mpim = false;
        self.is_channel = false;
        self
    }

    /// Make multi-person DM
    pub fn multi_person_dm(mut self) -> Self {
        self.is_mpim = true;
        self.is_private = false;
        self.is_im = false;
        self.is_channel = false;
        self
    }

    /// Mark as archived
    pub fn archived(mut self) -> Self {
        self.is_archived = true;
        self
    }

    /// Mark as general channel
    pub fn general(mut self) -> Self {
        self.is_general = true;
        self
    }

    /// Set member count
    pub fn with_members(mut self, count: i32) -> Self {
        self.num_members = Some(count);
        self
    }

    /// Build the SlackChannel
    pub fn build(self) -> SlackChannel {
        let id = self.id.unwrap_or_else(|| {
            if self.is_im {
                format!("D{}", (100000..999999).fake::<u32>())
            } else if self.is_mpim {
                format!("G{}", (100000..999999).fake::<u32>())
            } else {
                format!("C{}", (100000..999999).fake::<u32>())
            }
        });

        let name = self.name.unwrap_or_else(|| {
            Word()
                .fake::<String>()
                .to_lowercase()
                .replace(' ', "-")
        });

        SlackChannel {
            id,
            name,
            is_channel: self.is_channel,
            is_private: self.is_private,
            is_archived: self.is_archived,
            is_general: self.is_general,
            is_im: self.is_im,
            is_mpim: self.is_mpim,
            is_member: true,
            created: Some(chrono::Utc::now().timestamp()),
            creator: Some(format!("U{}", (100000..999999).fake::<u32>())),
            num_members: self.num_members,
            topic: None,
            purpose: None,
        }
    }
}

impl Default for MockChannelBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_channel_builder_default() {
        let channel = MockChannelBuilder::new().build();
        assert!(channel.id.starts_with('C'));
        assert!(!channel.name.is_empty());
        assert!(channel.is_channel);
        assert!(!channel.is_private);
        assert!(!channel.is_im);
        assert!(!channel.is_mpim);
    }

    #[test]
    fn test_mock_channel_builder_private() {
        let channel = MockChannelBuilder::new()
            .id("C123456")
            .name("private-channel")
            .private()
            .with_members(10)
            .build();

        assert_eq!(channel.id, "C123456");
        assert_eq!(channel.name, "private-channel");
        assert!(channel.is_private);
        assert!(!channel.is_im);
        assert!(!channel.is_mpim);
        assert_eq!(channel.num_members, Some(10));
    }

    #[test]
    fn test_mock_channel_builder_direct_message() {
        let channel = MockChannelBuilder::new().direct_message().build();

        assert!(channel.id.starts_with('D'));
        assert!(channel.is_im);
        assert!(!channel.is_channel);
        assert!(!channel.is_private);
        assert!(!channel.is_mpim);
    }

    #[test]
    fn test_mock_channel_builder_mpim() {
        let channel = MockChannelBuilder::new().multi_person_dm().build();

        assert!(channel.id.starts_with('G'));
        assert!(channel.is_mpim);
        assert!(!channel.is_channel);
        assert!(!channel.is_private);
        assert!(!channel.is_im);
    }
}
