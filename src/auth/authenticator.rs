use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::RwLock;

use super::credential::{Readiness, TokenKind};
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

        Ok(Self {
            store,
            state: RwLock::new(loaded.state),
            overrides: opts.overrides,
            explicit_profile: opts.explicit_profile,
            exchange: TokenExchange {
                api_base_url: opts.api_base_url,
                http,
            },
        })
    }

    pub async fn token_for(&self, policy: TokenPolicy) -> Result<Secret, AuthError> {
        if self.overrides.has_inline_tokens() {
            return policy
                .select(
                    self.overrides.user_token.is_some(),
                    self.overrides.bot_token.is_some(),
                )
                .and_then(|kind| self.overrides.get(kind).cloned())
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
                .select(profile.tokens.user.is_some(), profile.tokens.bot.is_some())
                .ok_or_else(|| AuthError::NoTokenForPolicy {
                    profile: name.clone(),
                    policy,
                })?;
            (name, kind)
        };

        self.token_for_profile(&name, kind).await
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
        let (token, readiness) = {
            let state = self.state.read().await;
            let credential = state
                .profiles
                .get(name)
                .ok_or_else(|| AuthError::UnknownProfile(name.to_string()))?
                .tokens
                .get(kind)
                .ok_or_else(|| AuthError::NoSuchToken {
                    profile: name.to_string(),
                    kind,
                })?;
            (credential.token.clone(), credential.readiness(Utc::now()))
        };

        match readiness {
            Readiness::Ready => Ok(token),
            Readiness::NeedsRenewal => self.renew(name, kind).await,
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
    async fn renew(&self, name: &str, kind: TokenKind) -> Result<Secret, AuthError> {
        let guard = self.store.lock().await?;
        let mut state = self.store.read()?.state;
        let now = Utc::now();

        let (current, still_live, refresh_token, client) = {
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
                credential.token.clone(),
                credential.is_live(now),
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
                    still_live,
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
                        still_live,
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
        };
        let Some(issued) = issued else {
            return self
                .keep_using(
                    state,
                    current,
                    still_live,
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

    /// Renewal could not complete. A credential only enters the renewal
    /// window while it is still valid, so the command runs on the token it
    /// already holds and the next invocation retries. Once the token is
    /// genuinely spent, the failure is the answer.
    async fn keep_using(
        &self,
        state: AuthState,
        token: Secret,
        still_live: bool,
        reason: AuthError,
    ) -> Result<Secret, AuthError> {
        if !still_live {
            return Err(reason);
        }
        tracing::warn!(
            error = %reason,
            "could not renew the Slack token; continuing with the one still in date"
        );
        *self.state.write().await = state;
        Ok(token)
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
        self.transact(|state| {
            state.upsert(name, profile, make_active);
            Ok(())
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
