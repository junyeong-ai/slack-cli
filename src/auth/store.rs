use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use super::errors::AuthError;
use super::state::{AuthState, SCHEMA_VERSION};

pub struct AuthStore {
    path: PathBuf,
}

impl AuthStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<AuthState, AuthError> {
        if !self.path.exists() {
            return Ok(AuthState::default());
        }

        ensure_permissions(&self.path)?;

        let bytes = std::fs::read(&self.path).map_err(|source| AuthError::StoreRead {
            path: self.path.clone(),
            source,
        })?;

        let state: AuthState =
            serde_json::from_slice(&bytes).map_err(|source| AuthError::StoreParse {
                path: self.path.clone(),
                source,
            })?;

        if state.version != SCHEMA_VERSION {
            return Err(AuthError::UnsupportedSchema {
                found: state.version,
                expected: SCHEMA_VERSION,
            });
        }

        Ok(state)
    }

    pub fn save(&self, state: &AuthState) -> Result<(), AuthError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| AuthError::Internal("auth store path has no parent directory".into()))?;

        std::fs::create_dir_all(parent).map_err(|source| AuthError::StoreWrite {
            path: self.path.clone(),
            source,
        })?;

        #[cfg(unix)]
        ensure_dir_permissions(parent)?;

        let mut tmp = NamedTempFile::new_in(parent).map_err(|source| AuthError::StoreWrite {
            path: self.path.clone(),
            source,
        })?;

        let payload = serde_json::to_vec_pretty(state)
            .map_err(|e| AuthError::Internal(format!("failed to serialize auth state: {e}")))?;

        use std::io::Write;
        tmp.as_file_mut()
            .write_all(&payload)
            .and_then(|_| tmp.as_file_mut().sync_all())
            .map_err(|source| AuthError::StoreWrite {
                path: self.path.clone(),
                source,
            })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(tmp.path(), perms).map_err(|source| {
                AuthError::StoreWrite {
                    path: self.path.clone(),
                    source,
                }
            })?;
        }

        tmp.persist(&self.path).map_err(|e| AuthError::StoreWrite {
            path: self.path.clone(),
            source: e.error,
        })?;

        Ok(())
    }
}

#[cfg(unix)]
fn ensure_permissions(path: &Path) -> Result<(), AuthError> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path).map_err(|source| AuthError::StoreRead {
        path: path.to_path_buf(),
        source,
    })?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, perms).map_err(|source| AuthError::StoreRead {
            path: path.to_path_buf(),
            source,
        })?;
        tracing::warn!(
            file = %path.display(),
            previous_mode = format!("{:o}", mode),
            "tightened auth store permissions to 0600"
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_permissions(_: &Path) -> Result<(), AuthError> {
    Ok(())
}

#[cfg(unix)]
fn ensure_dir_permissions(path: &Path) -> Result<(), AuthError> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path).map_err(|source| AuthError::StoreWrite {
        path: path.to_path_buf(),
        source,
    })?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        let perms = std::fs::Permissions::from_mode(0o700);
        std::fs::set_permissions(path, perms).map_err(|source| AuthError::StoreWrite {
            path: path.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::method::AuthMethod;
    use crate::auth::profile::{Profile, TokenSet, WorkspaceInfo};
    use crate::auth::secret;
    use chrono::Utc;
    use tempfile::tempdir;

    fn sample_profile() -> Profile {
        Profile {
            method: AuthMethod::Static,
            workspace: WorkspaceInfo {
                team_id: "T1".into(),
                team_name: "Acme".into(),
                user_id: Some("U1".into()),
            },
            tokens: TokenSet {
                user: Some(secret::new("xoxp-test-1234")),
                bot: None,
            },
            scopes: vec![],
            client_id: None,
            authorized_at: Utc::now(),
        }
    }

    #[test]
    fn roundtrip_preserves_state() {
        let dir = tempdir().unwrap();
        let store = AuthStore::new(dir.path().join("auth.json"));
        let mut state = AuthState::default();
        state.upsert("acme", sample_profile(), true);

        store.save(&state).unwrap();
        let loaded = store.load().unwrap();

        assert_eq!(loaded.active_profile.as_deref(), Some("acme"));
        assert_eq!(loaded.profiles.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn save_sets_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let path = dir.path().join("auth.json");
        let store = AuthStore::new(path.clone());
        let state = AuthState::default();
        store.save(&state).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(&path, br#"{"version":999,"profiles":{}}"#).unwrap();
        let store = AuthStore::new(path);
        let err = store.load().unwrap_err();
        assert!(matches!(
            err,
            AuthError::UnsupportedSchema { found: 999, .. }
        ));
    }

    #[test]
    fn missing_file_returns_default() {
        let dir = tempdir().unwrap();
        let store = AuthStore::new(dir.path().join("missing.json"));
        let state = store.load().unwrap();
        assert!(state.profiles.is_empty());
        assert_eq!(state.version, SCHEMA_VERSION);
    }
}
