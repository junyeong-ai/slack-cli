use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use etcetera::BaseStrategy;

const APP_DIR: &str = "slack-cli";

/// Every on-disk location the CLI owns, resolved once from the platform's
/// base-directory convention: XDG on Unix, Known Folders on Windows.
#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Result<Self> {
        let strategy = etcetera::choose_base_strategy()
            .context("could not determine the user configuration directory")?;
        Ok(Self {
            root: strategy.config_dir().join(APP_DIR),
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub fn auth_store(&self) -> PathBuf {
        self.root.join("auth.json")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Where the event daemon keeps its state and, when configured to, its
    /// event log. Deliberately not under `cache_dir`: the cache is dropped and
    /// rebuilt whenever its schema version moves, and an event Slack will
    /// never send again cannot be refetched.
    pub fn events_dir(&self) -> PathBuf {
        self.root.join("events")
    }
}

/// Expands a leading `~` in a user-authored path against the home directory.
/// Paths the CLI derives itself are already absolute and never pass through here.
pub fn expand_home(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    let rest = match text {
        "~" => "",
        _ => match text.strip_prefix("~/") {
            Some(rest) => rest,
            None => return path.to_path_buf(),
        },
    };
    match etcetera::home_dir() {
        Ok(home) => home.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_paths_share_one_root() {
        let paths = AppPaths::resolve().unwrap();
        let root = paths.config_file().parent().unwrap().to_path_buf();
        assert_eq!(paths.auth_store().parent().unwrap(), root);
        assert_eq!(paths.cache_dir().parent().unwrap(), root);
        assert_eq!(paths.events_dir().parent().unwrap(), root);
        assert_eq!(root.file_name().unwrap(), APP_DIR);
    }

    /// The cache rebuilds itself on any schema change, so an event log sharing
    /// its directory would be one migration away from being deleted.
    #[test]
    fn the_event_store_is_not_inside_the_cache() {
        let paths = AppPaths::resolve().unwrap();
        assert!(!paths.events_dir().starts_with(paths.cache_dir()));
        assert_ne!(paths.events_dir(), paths.cache_dir());
    }

    #[test]
    fn expands_tilde_prefix() {
        let home = etcetera::home_dir().unwrap();
        assert_eq!(
            expand_home(Path::new("~/test/path")),
            home.join("test/path")
        );
        assert_eq!(expand_home(Path::new("~")), home);
    }

    #[test]
    fn preserves_paths_without_a_leading_tilde() {
        assert_eq!(
            expand_home(Path::new("/absolute/path")),
            PathBuf::from("/absolute/path")
        );
        assert_eq!(
            expand_home(Path::new("relative/path")),
            PathBuf::from("relative/path")
        );
        assert_eq!(
            expand_home(Path::new("/path/~user/test")),
            PathBuf::from("/path/~user/test")
        );
        assert_eq!(expand_home(Path::new("")), PathBuf::from(""));
    }
}
