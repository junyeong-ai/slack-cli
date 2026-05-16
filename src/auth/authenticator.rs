use std::path::PathBuf;

use tokio::sync::RwLock;

use super::env::EnvOverrides;
use super::errors::AuthError;
use super::policy::TokenPolicy;
use super::profile::Profile;
use super::secret::Secret;
use super::state::AuthState;
use super::store::AuthStore;

pub struct AuthLoadOptions {
    pub store_path: PathBuf,
    pub overrides: EnvOverrides,
    pub explicit_profile: Option<String>,
}

pub struct Authenticator {
    store: AuthStore,
    state: RwLock<AuthState>,
    overrides: EnvOverrides,
    explicit_profile: Option<String>,
}

impl Authenticator {
    pub fn load(opts: AuthLoadOptions) -> Result<Self, AuthError> {
        let store = AuthStore::new(opts.store_path);
        let state = store.load()?;
        Ok(Self {
            store,
            state: RwLock::new(state),
            overrides: opts.overrides,
            explicit_profile: opts.explicit_profile,
        })
    }

    pub async fn token_for(&self, policy: TokenPolicy) -> Result<Secret, AuthError> {
        if self.overrides.has_inline_tokens() {
            return policy
                .pick(
                    self.overrides.user_token.clone(),
                    self.overrides.bot_token.clone(),
                )
                .ok_or_else(|| AuthError::NoTokenForPolicy {
                    profile: "env".into(),
                    policy,
                });
        }

        let state = self.state.read().await;
        let name = self
            .explicit_profile
            .as_deref()
            .or(state.active_profile.as_deref())
            .ok_or(AuthError::NotConfigured)?;
        let profile = state
            .profiles
            .get(name)
            .ok_or_else(|| AuthError::UnknownProfile(name.to_string()))?;

        policy
            .pick(profile.tokens.user.clone(), profile.tokens.bot.clone())
            .ok_or_else(|| AuthError::NoTokenForPolicy {
                profile: name.to_string(),
                policy,
            })
    }

    pub async fn snapshot(&self) -> AuthState {
        self.state.read().await.clone()
    }

    pub async fn upsert_profile(
        &self,
        name: &str,
        profile: Profile,
        make_active: bool,
    ) -> Result<(), AuthError> {
        let mut state = self.state.write().await;
        let mut next = state.clone();
        next.upsert(name, profile, make_active);
        self.store.save(&next)?;
        *state = next;
        Ok(())
    }

    pub async fn remove_profile(&self, name: &str) -> Result<Option<Profile>, AuthError> {
        let mut state = self.state.write().await;
        let mut next = state.clone();
        let removed = next.remove(name);
        self.store.save(&next)?;
        *state = next;
        Ok(removed)
    }

    pub async fn clear_all(&self) -> Result<(), AuthError> {
        let mut state = self.state.write().await;
        let next = AuthState::default();
        self.store.save(&next)?;
        *state = next;
        Ok(())
    }

    pub async fn set_active(&self, name: &str) -> Result<(), AuthError> {
        let mut state = self.state.write().await;
        if !state.profiles.contains_key(name) {
            return Err(AuthError::UnknownProfile(name.to_string()));
        }
        let mut next = state.clone();
        next.active_profile = Some(name.to_string());
        self.store.save(&next)?;
        *state = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::secret;
    use secrecy::ExposeSecret;

    fn s(value: &str) -> Secret {
        secret::new(value)
    }

    #[test]
    fn user_required_picks_user() {
        let picked = TokenPolicy::UserRequired.pick(Some(s("xoxp")), Some(s("xoxb")));
        assert_eq!(picked.unwrap().expose_secret(), "xoxp");
    }

    #[test]
    fn user_required_fails_without_user() {
        assert!(
            TokenPolicy::UserRequired
                .pick(None, Some(s("xoxb")))
                .is_none()
        );
    }

    #[test]
    fn user_preferred_falls_back_to_bot() {
        let picked = TokenPolicy::UserPreferred.pick(None, Some(s("xoxb")));
        assert_eq!(picked.unwrap().expose_secret(), "xoxb");
    }

    #[test]
    fn bot_preferred_falls_back_to_user() {
        let picked = TokenPolicy::BotPreferred.pick(Some(s("xoxp")), None);
        assert_eq!(picked.unwrap().expose_secret(), "xoxp");
    }

    #[test]
    fn bot_preferred_with_both_picks_bot() {
        let picked = TokenPolicy::BotPreferred.pick(Some(s("xoxp")), Some(s("xoxb")));
        assert_eq!(picked.unwrap().expose_secret(), "xoxb");
    }
}
