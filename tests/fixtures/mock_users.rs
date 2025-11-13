use fake::faker::internet::en::*;
use fake::faker::name::en::*;
use fake::{Fake, Faker};
use mcp_slack::slack::types::{SlackUser, SlackUserProfile};

/// Fluent builder for creating SlackUser test fixtures with realistic fake data
pub struct MockUserBuilder {
    id: Option<String>,
    name: Option<String>,
    real_name: Option<String>,
    display_name: Option<String>,
    email: Option<String>,
    is_bot: bool,
    is_admin: bool,
    deleted: bool,
}

impl MockUserBuilder {
    /// Create a new builder with default fake data
    pub fn new() -> Self {
        Self {
            id: None,
            name: None,
            real_name: None,
            display_name: None,
            email: None,
            is_bot: false,
            is_admin: false,
            deleted: false,
        }
    }

    /// Set user ID (defaults to random U{number})
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set username (defaults to fake username)
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set real name (defaults to fake full name)
    pub fn real_name(mut self, name: impl Into<String>) -> Self {
        self.real_name = Some(name.into());
        self
    }

    /// Set display name (defaults to None)
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Set email (defaults to fake email)
    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// Mark as bot user
    pub fn as_bot(mut self) -> Self {
        self.is_bot = true;
        self
    }

    /// Mark as admin user
    pub fn as_admin(mut self) -> Self {
        self.is_admin = true;
        self
    }

    /// Mark as deleted user
    pub fn as_deleted(mut self) -> Self {
        self.deleted = true;
        self
    }

    /// Build the SlackUser with fake data for unset fields
    pub fn build(self) -> SlackUser {
        let id = self
            .id
            .unwrap_or_else(|| format!("U{}", (100000..999999).fake::<u32>()));
        let name = self.name.unwrap_or_else(|| Username().fake());
        let real_name = self.real_name.or_else(|| Some(Name().fake()));
        let email = self.email.or_else(|| Some(SafeEmail().fake()));

        SlackUser {
            id,
            name,
            is_bot: self.is_bot,
            is_admin: self.is_admin,
            deleted: self.deleted,
            profile: Some(SlackUserProfile {
                real_name,
                display_name: self.display_name,
                email,
                status_text: None,
                status_emoji: None,
            }),
        }
    }
}

impl Default for MockUserBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_user_builder_default() {
        let user = MockUserBuilder::new().build();
        assert!(user.id.starts_with('U'));
        assert!(!user.name.is_empty());
        assert!(!user.is_bot);
    }

    #[test]
    fn test_mock_user_builder_custom() {
        let user = MockUserBuilder::new()
            .id("U123456")
            .name("testuser")
            .email("test@example.com")
            .as_bot()
            .build();

        assert_eq!(user.id, "U123456");
        assert_eq!(user.name, "testuser");
        assert!(user.is_bot);
        assert_eq!(
            user.profile.as_ref().unwrap().email.as_deref(),
            Some("test@example.com")
        );
    }
}
