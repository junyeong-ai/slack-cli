use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use secrecy::ExposeSecret;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use super::credential::{Credential, Readiness, TokenKind};
use super::env::EnvOverrides;
use super::errors::AuthError;
use super::oauth::exchange::{Grant, TokenExchange};
use super::policy::TokenPolicy;
use super::profile::Profile;
use super::secret::Secret;
use super::state::AuthState;
use super::store::AuthStore;

const RENEWAL_TIMEOUT: Duration = Duration::from_secs(30);

pub struct AuthLoadOptions {
    pub store_path: PathBuf,
    pub api_base_url: String,
    pub overrides: EnvOverrides,
    pub explicit_profile: Option<String>,
}

/// Resolves the token every Slack call is made with, and owns the lifecycle of
/// the credentials behind it: which profile is active, when a rotating token
/// must be renewed, and every write to the store.
pub struct Authenticator {
    store: AuthStore,
    state: RwLock<AuthState>,
    overrides: EnvOverrides,
    explicit_profile: Option<String>,
    exchange: TokenExchange,
}

impl Authenticator {
    pub async fn load(opts: AuthLoadOptions) -> Result<Self, AuthError> {
        let store = AuthStore::new(opts.store_path);
        let mut loaded = store.read()?;
        if loaded.upgraded {
            let guard = store.lock().await?;
            // Re-read now that the lock is held. A sibling process may have
            // migrated and then written a profile in the interval; its state
            // supersedes the snapshot taken before the lock, and writing that
            // snapshot would erase the sibling's work.
            loaded = store.read()?;
            if loaded.upgraded {
                store.write(&guard, &loaded.state)?;
            }
        }

        let http = reqwest::Client::builder()
            .timeout(RENEWAL_TIMEOUT)
            .build()
            .map_err(|e| AuthError::Internal(format!("failed to create OAuth client: {e}")))?;

        let explicit_profile = opts
            .explicit_profile
            .or_else(|| loaded.state.active_profile.clone());
        Ok(Self {
            store,
            state: RwLock::new(loaded.state),
            overrides: opts.overrides,
            explicit_profile,
            exchange: TokenExchange {
                api_base_url: opts.api_base_url,
                http,
            },
        })
    }

    /// The token to send, with the kind it came from: a caller that gets a
    /// scope refusal from Slack needs the kind to name what the method wanted.
    pub async fn token_for(&self, policy: TokenPolicy) -> Result<(Secret, TokenKind), AuthError> {
        // App credentials use their own environment/profile resolution and
        // do not enter this CLI's user/bot renewal flow.
        if policy == TokenPolicy::AppRequired {
            return self.app_token().await.map(|token| (token, TokenKind::App));
        }

        if self.overrides.has_inline_tokens() {
            return policy
                .select(|kind| self.overrides.holds(kind))
                .and_then(|kind| Some((self.overrides.get(kind).cloned()?, kind)))
                .ok_or_else(|| AuthError::NoTokenForPolicy {
                    profile: "env".into(),
                    policy,
                });
        }

        let (name, kind) = {
            let state = self.state.read().await;
            let name = state
                .resolve(self.explicit_profile.as_deref())
                .ok_or(AuthError::NotConfigured)?
                .to_string();
            let profile = state
                .profiles
                .get(&name)
                .ok_or_else(|| AuthError::UnknownProfile(name.clone()))?;
            let kind = policy
                .select(|kind| profile.tokens.holds(kind))
                .ok_or_else(|| AuthError::NoTokenForPolicy {
                    profile: name.clone(),
                    policy,
                })?;
            (name, kind)
        };

        Ok((self.token_for_profile(&name, kind).await?, kind))
    }

    /// The app-level token: the environment first, then the resolved profile.
    ///
    /// Its absence is its own error rather than `NotConfigured`, because a
    /// fully authenticated installation still holds none until one is
    /// registered, and "run auth login" would be the wrong instruction.
    async fn app_token(&self) -> Result<Secret, AuthError> {
        if let Some(token) = self.overrides.get(TokenKind::App).cloned() {
            return Ok(token);
        }

        let name = {
            let state = self.state.read().await;
            state
                .resolve(self.explicit_profile.as_deref())
                .filter(|name| {
                    state
                        .profiles
                        .get(*name)
                        .is_some_and(|profile| profile.tokens.holds(TokenKind::App))
                })
                .map(ToOwned::to_owned)
        };

        match name {
            Some(name) => self.token_for_profile(&name, TokenKind::App).await,
            None => Err(AuthError::NoAppToken),
        }
    }

    /// The usable token of one kind from a named profile, renewed first if it
    /// is due. Commands that act on a specific profile rather than the active
    /// one — verifying it, revoking it — go through here so they see the same
    /// live credential every API call does.
    pub async fn token_for_profile(
        &self,
        name: &str,
        kind: TokenKind,
    ) -> Result<Secret, AuthError> {
        let (token, readiness, authorization) = {
            let state = self.state.read().await;
            let profile = state
                .profiles
                .get(name)
                .ok_or_else(|| AuthError::UnknownProfile(name.to_string()))?;
            let credential = profile
                .tokens
                .get(kind)
                .ok_or_else(|| AuthError::NoSuchToken {
                    profile: name.to_string(),
                    kind,
                })?;
            (
                credential.token.clone(),
                credential.readiness(Utc::now()),
                profile.clone(),
            )
        };

        match readiness {
            Readiness::Ready => Ok(token),
            Readiness::NeedsRenewal => self.renew(name, kind, &authorization).await,
            Readiness::Expired => Err(AuthError::NotRenewable {
                profile: name.to_string(),
                kind,
            }),
        }
    }

    /// Exchanges an expiring credential for a fresh one.
    ///
    /// Slack revokes a refresh token once it is used, so the exchange runs
    /// under the store lock and re-reads from disk first: a sibling process
    /// that renewed a moment ago has already written the successor, and this
    /// call adopts it instead of spending a token that is no longer current.
    async fn renew(
        &self,
        name: &str,
        kind: TokenKind,
        expected: &Profile,
    ) -> Result<Secret, AuthError> {
        let guard = self.store.lock().await?;
        let mut state = self.store.read()?.state;
        if state
            .profiles
            .get(name)
            .is_none_or(|profile| !profile.same_authorization(expected))
        {
            return Err(AuthError::AuthorizationChanged(name.to_owned()));
        }
        let now = Utc::now();

        let (current, refresh_token, client) = {
            let profile = state
                .profiles
                .get(name)
                .ok_or_else(|| AuthError::UnknownProfile(name.to_string()))?;
            let credential = profile
                .tokens
                .get(kind)
                .ok_or_else(|| AuthError::NoSuchToken {
                    profile: name.to_string(),
                    kind,
                })?;

            match credential.readiness(now) {
                Readiness::NeedsRenewal => {}
                Readiness::Ready => {
                    let token = credential.token.clone();
                    *self.state.write().await = state;
                    return Ok(token);
                }
                Readiness::Expired => {
                    return Err(AuthError::NotRenewable {
                        profile: name.to_string(),
                        kind,
                    });
                }
            }

            (
                credential.clone(),
                credential
                    .refresh_token
                    .clone()
                    .expect("NeedsRenewal implies a refresh token"),
                profile.client.clone(),
            )
        };

        let Some(client) = client else {
            return self
                .keep_using(
                    state,
                    current,
                    AuthError::ClientUnknown {
                        profile: name.to_string(),
                        kind,
                    },
                )
                .await;
        };

        let response = match self
            .exchange
            .execute(
                &client,
                Grant::RefreshToken {
                    refresh_token: &refresh_token,
                },
            )
            .await
        {
            Ok(response) => response,
            Err(source) => {
                return self
                    .keep_using(
                        state,
                        current,
                        AuthError::RenewalFailed {
                            profile: name.to_string(),
                            kind,
                            source,
                        },
                    )
                    .await;
            }
        };

        let issued = match kind {
            TokenKind::User => response.user,
            TokenKind::Bot => response.bot,
            // Stored app credentials carry no expiry or refresh metadata.
            // This exchange only handles installation credentials.
            TokenKind::App => None,
        };
        let Some(issued) = issued else {
            return self
                .keep_using(
                    state,
                    current,
                    AuthError::RenewalMismatch {
                        profile: name.to_string(),
                        kind,
                    },
                )
                .await;
        };

        let renewed = issued.into_credential(Utc::now());
        let token = renewed.token.clone();

        state
            .profiles
            .get_mut(name)
            .expect("profile was read from this state")
            .tokens
            .set(kind, renewed);

        self.store.write(&guard, &state)?;
        *self.state.write().await = state;

        tracing::debug!(profile = name, kind = kind.as_str(), "renewed Slack token");
        Ok(token)
    }

    /// A failed renewal may reuse the credential only while its recorded
    /// expiry still permits use, checked after the network and state-lock waits.
    async fn keep_using(
        &self,
        state: AuthState,
        credential: Credential,
        reason: AuthError,
    ) -> Result<Secret, AuthError> {
        let mut snapshot = self.state.write().await;
        if !credential.is_live(Utc::now()) {
            return Err(reason);
        }
        tracing::warn!(
            error = %reason,
            "could not renew the Slack token; continuing with the one still in date"
        );
        *snapshot = state;
        Ok(credential.token)
    }

    /// Whether the environment is supplying the installation tokens, in which
    /// case no stored profile is in play at all. Anything that namespaces
    /// state by profile has to know, or it would file one installation's data
    /// under another's name.
    pub fn uses_env_tokens(&self) -> bool {
        self.overrides.has_inline_tokens()
    }

    pub async fn cache_namespace(&self) -> Result<String, AuthError> {
        let identity = if self.uses_env_tokens() {
            serde_json::json!([
                "env",
                self.overrides
                    .user_token
                    .as_ref()
                    .map(|token| token.expose_secret()),
                self.overrides
                    .bot_token
                    .as_ref()
                    .map(|token| token.expose_secret()),
            ])
        } else {
            let state = self.state.read().await;
            let name = state
                .resolve(self.explicit_profile.as_deref())
                .ok_or(AuthError::NotConfigured)?;
            let profile = state
                .profiles
                .get(name)
                .ok_or_else(|| AuthError::UnknownProfile(name.to_owned()))?;
            serde_json::json!(["profile", name, profile.workspace, profile.authorized_at])
        };
        let digest = Sha256::digest(
            serde_json::to_vec(&(self.exchange.api_base_url.trim_end_matches('/'), identity))
                .map_err(|err| AuthError::Internal(err.to_string()))?,
        );
        Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    pub async fn snapshot(&self) -> AuthState {
        self.state.read().await.clone()
    }

    /// Applies a change to the store as one cross-process transaction: the
    /// lock is held while the state is re-read, mutated, and written back, so
    /// concurrent invocations can never overwrite each other's profiles.
    async fn transact<T>(
        &self,
        apply: impl FnOnce(&mut AuthState) -> Result<T, AuthError>,
    ) -> Result<T, AuthError> {
        let guard = self.store.lock().await?;
        let mut state = self.store.read()?.state;
        let outcome = apply(&mut state)?;
        self.store.write(&guard, &state)?;
        *self.state.write().await = state;
        Ok(outcome)
    }

    pub async fn upsert_profile(
        &self,
        name: &str,
        profile: Profile,
        make_active: bool,
    ) -> Result<(), AuthError> {
        self.save_profile(name, profile, make_active, true).await
    }

    pub async fn save_profile(
        &self,
        name: &str,
        profile: Profile,
        make_active: bool,
        allow_team_change: bool,
    ) -> Result<(), AuthError> {
        self.transact(|state| {
            if !allow_team_change
                && let Some(existing) = state.profiles.get(name)
                && existing.workspace.team_id != profile.workspace.team_id
            {
                return Err(AuthError::ProfileNameConflict {
                    profile: name.to_owned(),
                    existing_team: existing.workspace.team_id.clone(),
                    team: profile.workspace.team_id.clone(),
                });
            }
            state.upsert(name, profile, make_active);
            Ok(())
        })
        .await
    }

    pub async fn token_for_authorization(
        &self,
        name: &str,
        kind: TokenKind,
        expected: &Profile,
    ) -> Result<Secret, AuthError> {
        let profile = {
            let _guard = self.store.lock().await?;
            let state = self.store.read()?.state;
            state
                .profiles
                .get(name)
                .filter(|profile| profile.same_authorization(expected))
                .cloned()
                .ok_or_else(|| AuthError::AuthorizationChanged(name.to_owned()))?
        };
        let credential = profile
            .tokens
            .get(kind)
            .ok_or_else(|| AuthError::NoSuchToken {
                profile: name.to_owned(),
                kind,
            })?;
        match credential.readiness(Utc::now()) {
            Readiness::Ready => Ok(credential.token.clone()),
            Readiness::NeedsRenewal => self.renew(name, kind, expected).await,
            Readiness::Expired => Err(AuthError::NotRenewable {
                profile: name.to_owned(),
                kind,
            }),
        }
    }

    pub async fn remove_authorizations(&self, expected: &AuthState) -> Result<usize, AuthError> {
        self.transact(|state| {
            let mut removed = 0;
            for (name, authorization) in &expected.profiles {
                if state
                    .profiles
                    .get(name)
                    .is_some_and(|profile| profile.same_authorization(authorization))
                {
                    state.remove(name);
                    removed += 1;
                }
            }
            Ok(removed)
        })
        .await
    }

    pub async fn remove_profile(&self, name: &str) -> Result<Option<Profile>, AuthError> {
        self.transact(|state| Ok(state.remove(name))).await
    }

    pub async fn clear_all(&self) -> Result<(), AuthError> {
        self.transact(|state| {
            *state = AuthState::default();
            Ok(())
        })
        .await
    }

    /// Adds one credential to an existing profile, leaving the others alone.
    ///
    /// This is how a token that no login flow can produce reaches the store:
    /// an app-level token is minted in the Slack app's own configuration, so
    /// it is attached to the installation rather than issued alongside it.
    pub async fn attach_token(
        &self,
        name: &str,
        kind: TokenKind,
        credential: Credential,
    ) -> Result<(), AuthError> {
        self.transact(|state| {
            let profile = state
                .profiles
                .get_mut(name)
                .ok_or_else(|| AuthError::UnknownProfile(name.to_string()))?;
            profile.tokens.set(kind, credential);
            Ok(())
        })
        .await
    }

    pub async fn set_active(&self, name: &str) -> Result<(), AuthError> {
        self.transact(|state| {
            if !state.profiles.contains_key(name) {
                return Err(AuthError::UnknownProfile(name.to_string()));
            }
            state.active_profile = Some(name.to_string());
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[tokio::test]
    async fn renewal_failure_checks_expiry_at_the_fallback_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let auth = Authenticator::load(AuthLoadOptions {
            store_path: dir.path().join("auth.json"),
            api_base_url: "http://127.0.0.1".into(),
            overrides: EnvOverrides::default(),
            explicit_profile: None,
        })
        .await
        .unwrap();
        for minutes in [-1, 1] {
            let expires_at = Utc::now() + Duration::minutes(minutes);
            let mut credential =
                Credential::permanent(super::super::secret::new("xoxe-current"), vec![]);
            credential.expires_at = Some(expires_at);
            let previously_live = credential.is_live(expires_at - Duration::seconds(1));
            assert!(previously_live);
            let result = auth
                .keep_using(
                    AuthState::default(),
                    credential,
                    AuthError::ClientUnknown {
                        profile: "test".into(),
                        kind: TokenKind::User,
                    },
                )
                .await;
            if minutes < 0 {
                assert!(matches!(result, Err(AuthError::ClientUnknown { .. })));
            } else {
                assert_eq!(result.unwrap().expose_secret(), "xoxe-current");
            }
        }
    }
}
