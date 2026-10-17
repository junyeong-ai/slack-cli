use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::{AuthError, Authenticator, TokenKind, TokenPolicy};
use crate::slack::SlackClient;
use secrecy::ExposeSecret;

#[derive(Clone, Serialize, Deserialize)]
pub struct EventOwner {
    pub profile: String,
    endpoint: String,
    pub team_id: String,
    pub user_id: String,
}

impl EventOwner {
    pub async fn resolve(
        authenticator: &Authenticator,
        slack: &SlackClient,
        dir: &Path,
        endpoint: &str,
        explicit: Option<&str>,
    ) -> Result<Self> {
        let endpoint = endpoint.trim_end_matches('/').to_owned();
        if !authenticator.uses_env_tokens() {
            let snapshot = authenticator.snapshot().await;
            let name = snapshot.resolve(explicit).ok_or(AuthError::NotConfigured)?;
            let profile = snapshot
                .profiles
                .get(name)
                .ok_or_else(|| AuthError::UnknownProfile(name.to_owned()))?;
            let owner =
                Self {
                    profile: name.to_owned(),
                    endpoint,
                    team_id: profile.workspace.team_id.clone(),
                    user_id: profile.workspace.user_id.clone().context(
                        "profile has no verified user identity; run slack-cli auth login",
                    )?,
                };
            owner.validate()?;
            return Ok(owner);
        }

        let bindings = dir.join("identities");
        let binding = bindings.join(format!("{}.json", authenticator.cache_namespace().await?));
        match tokio::fs::read(&binding).await {
            Ok(bytes) => {
                let owner: Self =
                    serde_json::from_slice(&bytes).context("invalid event identity binding")?;
                owner.validate()?;
                anyhow::ensure!(
                    owner.endpoint == endpoint && owner.profile == "env",
                    "event identity binding does not match this endpoint"
                );
                return Ok(owner);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        let mut primary: Option<Self> = None;
        for policy in [TokenPolicy::UserRequired, TokenPolicy::BotPreferred] {
            let (token, kind) = match authenticator.token_for(policy).await {
                Ok(token) => token,
                Err(AuthError::NoTokenForPolicy { .. }) => continue,
                Err(error) => return Err(error.into()),
            };
            if kind == TokenKind::User && primary.is_some() {
                continue;
            }
            let identity = slack
                .auth
                .test(token.expose_secret())
                .await
                .context("could not verify new environment credentials for the event store")?;
            if let Some(owner) = &primary {
                anyhow::ensure!(
                    owner.team_id == identity.team_id,
                    "user and bot tokens must belong to the same workspace"
                );
            } else {
                primary = Some(Self {
                    profile: "env".into(),
                    endpoint: endpoint.clone(),
                    team_id: identity.team_id,
                    user_id: identity.user_id,
                });
            }
        }
        let owner = primary.ok_or(AuthError::NotConfigured)?;
        owner.validate()?;
        let bytes = serde_json::to_vec(&owner)?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            std::fs::create_dir_all(&bindings)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&bindings, std::fs::Permissions::from_mode(0o700))?;
            }
            let mut staged = tempfile::NamedTempFile::new_in(&bindings)?;
            staged.write_all(&bytes)?;
            staged.as_file().sync_all()?;
            staged.persist(&binding)?;
            Ok(())
        })
        .await??;
        Ok(owner)
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.team_id.is_empty() && !self.user_id.is_empty(),
            "event storage requires a verified workspace and user identity"
        );
        Ok(())
    }

    pub fn namespace(&self) -> Result<String> {
        let digest = Sha256::digest(serde_json::to_vec(self)?);
        Ok(format!(
            "{}-{}",
            self.profile,
            digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ))
    }

    pub fn accepts(&self, identity: &crate::slack::SlackAuthIdentity) -> bool {
        self.team_id == identity.team_id && self.user_id == identity.user_id
    }
}
