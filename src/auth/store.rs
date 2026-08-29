use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tempfile::NamedTempFile;

use super::errors::AuthError;
use super::migrate;
use super::state::{AuthState, SCHEMA_VERSION};

const LOCK_SUFFIX: &str = ".lock";

pub struct AuthStore {
    path: PathBuf,
    lock_path: PathBuf,
}

pub struct LoadedState {
    pub state: AuthState,
    /// Set when the file on disk used an older schema and the returned state
    /// is the upgraded form, which the caller should persist.
    pub upgraded: bool,
}

/// Holds the cross-process lock on the store for as long as it is alive.
/// The advisory lock is released by the operating system even if the process
/// dies, so a crash can never strand it.
pub struct StoreGuard {
    file: File,
}

impl StoreGuard {
    /// The lock file holds no content — only the advisory lock on it matters —
    /// so it is created private rather than tightened afterwards, which would
    /// raise the warning the store itself reserves for a loosened token file.
    fn acquire(lock_path: &Path) -> std::io::Result<Self> {
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true).truncate(false);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);

        let file = options.open(lock_path)?;
        file.lock()?;
        Ok(Self { file })
    }
}

impl Drop for StoreGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[derive(Deserialize)]
struct VersionProbe {
    version: u32,
}

impl AuthStore {
    pub fn new(path: PathBuf) -> Self {
        let mut lock_path = path.clone().into_os_string();
        lock_path.push(LOCK_SUFFIX);
        Self {
            path,
            lock_path: lock_path.into(),
        }
    }

    /// Serializes every read-modify-write of the store across processes.
    ///
    /// The lock lives in a sibling file rather than the store itself: writes
    /// replace the store by rename, which would leave a lock held on an
    /// unlinked inode that no other process can see.
    ///
    /// Acquiring it waits on another process for an unbounded time, so it runs
    /// on the blocking pool: waiting on a runtime worker would strand the very
    /// task that is meant to release the lock.
    pub async fn lock(&self) -> Result<StoreGuard, AuthError> {
        self.prepare_directory()?;
        let lock_path = self.lock_path.clone();
        let guard = tokio::task::spawn_blocking(move || StoreGuard::acquire(&lock_path))
            .await
            .map_err(|e| {
                AuthError::Internal(format!("failed to acquire the auth store lock: {e}"))
            })?
            .map_err(|source| self.write_error(source))?;
        Ok(guard)
    }

    pub fn read(&self) -> Result<LoadedState, AuthError> {
        if !self.path.exists() {
            return Ok(LoadedState {
                state: AuthState::default(),
                upgraded: false,
            });
        }

        restrict_file(&self.path)?;

        let bytes = std::fs::read(&self.path).map_err(|source| AuthError::StoreRead {
            path: self.path.clone(),
            source,
        })?;

        let probe: VersionProbe =
            serde_json::from_slice(&bytes).map_err(|source| self.parse_error(source))?;

        match probe.version {
            SCHEMA_VERSION => Ok(LoadedState {
                state: serde_json::from_slice(&bytes).map_err(|source| self.parse_error(source))?,
                upgraded: false,
            }),
            1 => Ok(LoadedState {
                state: migrate::from_v1(&bytes).map_err(|source| self.parse_error(source))?,
                upgraded: true,
            }),
            found => Err(AuthError::UnsupportedSchema {
                found,
                expected: SCHEMA_VERSION,
            }),
        }
    }

    /// Replaces the store. Taking the guard by reference is what keeps the
    /// lock discipline honest: there is no way to reach this without holding
    /// the cross-process lock, so no caller can write a state it read before
    /// another process had finished writing its own.
    pub fn write(&self, _guard: &StoreGuard, state: &AuthState) -> Result<(), AuthError> {
        let parent = self.prepare_directory()?;

        let mut tmp = NamedTempFile::new_in(parent).map_err(|source| self.write_error(source))?;

        let payload = serde_json::to_vec_pretty(state)
            .map_err(|e| AuthError::Internal(format!("failed to serialize auth state: {e}")))?;

        use std::io::Write;
        tmp.as_file_mut()
            .write_all(&payload)
            .and_then(|_| tmp.as_file_mut().sync_all())
            .map_err(|source| self.write_error(source))?;

        restrict_file(tmp.path())?;

        tmp.persist(&self.path)
            .map_err(|e| self.write_error(e.error))?;

        Ok(())
    }

    fn prepare_directory(&self) -> Result<&Path, AuthError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| AuthError::Internal("auth store path has no parent directory".into()))?;
        std::fs::create_dir_all(parent).map_err(|source| self.write_error(source))?;
        restrict_directory(parent)?;
        Ok(parent)
    }

    fn write_error(&self, source: std::io::Error) -> AuthError {
        AuthError::StoreWrite {
            path: self.path.clone(),
            source,
        }
    }

    fn parse_error(&self, source: serde_json::Error) -> AuthError {
        AuthError::StoreParse {
            path: self.path.clone(),
            source,
        }
    }
}

#[cfg(unix)]
fn restrict_file(path: &Path) -> Result<(), AuthError> {
    let current = mode_of(path)?;
    if current == 0o600 {
        return Ok(());
    }
    apply_mode(path, 0o600)?;
    tracing::warn!(
        file = %path.display(),
        previous_mode = format!("{current:o}"),
        "tightened auth store permissions to 0600"
    );
    Ok(())
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> Result<(), AuthError> {
    if mode_of(path)? & 0o077 == 0 {
        return Ok(());
    }
    apply_mode(path, 0o700)
}

#[cfg(unix)]
fn mode_of(path: &Path) -> Result<u32, AuthError> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o777)
        .map_err(|source| AuthError::StoreRead {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(unix)]
fn apply_mode(path: &Path, mode: u32) -> Result<(), AuthError> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|source| {
        AuthError::StoreWrite {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn restrict_file(_: &Path) -> Result<(), AuthError> {
    Ok(())
}

#[cfg(not(unix))]
fn restrict_directory(_: &Path) -> Result<(), AuthError> {
    Ok(())
}
