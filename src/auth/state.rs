use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::profile::Profile;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthState {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

impl Default for AuthState {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            active_profile: None,
            profiles: BTreeMap::new(),
        }
    }
}

impl AuthState {
    pub fn upsert(&mut self, name: &str, profile: Profile, make_active: bool) {
        self.profiles.insert(name.to_string(), profile);
        if make_active || self.active_profile.is_none() {
            self.active_profile = Some(name.to_string());
        }
    }

    pub fn remove(&mut self, name: &str) -> Option<Profile> {
        let removed = self.profiles.remove(name);
        if self.active_profile.as_deref() == Some(name) {
            self.active_profile = None;
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::method::AuthMethod;
    use crate::auth::profile::{TokenSet, WorkspaceInfo};
    use chrono::Utc;

    fn empty_profile(team: &str) -> Profile {
        Profile {
            method: AuthMethod::Static,
            workspace: WorkspaceInfo {
                team_id: team.into(),
                team_name: team.into(),
                user_id: None,
            },
            tokens: TokenSet::default(),
            scopes: vec![],
            client_id: None,
            authorized_at: Utc::now(),
        }
    }

    #[test]
    fn upsert_promotes_first_profile_to_active() {
        let mut s = AuthState::default();
        s.upsert("a", empty_profile("A"), false);
        assert_eq!(s.active_profile.as_deref(), Some("a"));
    }

    #[test]
    fn upsert_with_make_active_switches_active() {
        let mut s = AuthState::default();
        s.upsert("a", empty_profile("A"), false);
        s.upsert("b", empty_profile("B"), true);
        assert_eq!(s.active_profile.as_deref(), Some("b"));
    }

    #[test]
    fn upsert_without_make_active_keeps_existing_active() {
        let mut s = AuthState::default();
        s.upsert("a", empty_profile("A"), false);
        s.upsert("b", empty_profile("B"), false);
        assert_eq!(s.active_profile.as_deref(), Some("a"));
    }

    #[test]
    fn remove_active_clears_active_without_auto_promoting() {
        let mut s = AuthState::default();
        s.upsert("a", empty_profile("A"), false);
        s.upsert("b", empty_profile("B"), false);
        s.remove("a");
        assert!(s.active_profile.is_none());
        assert!(s.profiles.contains_key("b"));
    }

    #[test]
    fn remove_inactive_preserves_active() {
        let mut s = AuthState::default();
        s.upsert("a", empty_profile("A"), false);
        s.upsert("b", empty_profile("B"), false);
        s.remove("b");
        assert_eq!(s.active_profile.as_deref(), Some("a"));
    }
}
