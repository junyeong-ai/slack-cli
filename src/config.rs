use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::slack::ConversationType;

fn expand_tilde(path: &Path) -> PathBuf {
    if let Some(path_str) = path.to_str() {
        if let Some(stripped) = path_str.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home).join(stripped);
            }
        } else if path_str == "~"
            && let Ok(home) = std::env::var("HOME")
        {
            return PathBuf::from(home);
        }
    }
    path.to_path_buf()
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default)]
    pub cache: CacheConfig,

    #[serde(default)]
    pub output: OutputConfig,

    #[serde(default)]
    pub retry: RetryConfig,

    #[serde(default)]
    pub connection: ConnectionConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CacheConfig {
    #[serde(default = "default_ttl_hours")]
    pub ttl_users_hours: u64,

    #[serde(default = "default_ttl_hours")]
    pub ttl_channels_hours: u64,

    #[serde(default = "default_refresh_threshold_percent")]
    pub refresh_threshold_percent: u64,

    pub data_path: Option<PathBuf>,

    #[serde(default = "default_channel_types")]
    pub channel_types: Vec<ConversationType>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OutputConfig {
    #[serde(default = "default_users_fields")]
    pub users_fields: Vec<String>,

    #[serde(default = "default_channels_fields")]
    pub channels_fields: Vec<String>,
}

fn default_users_fields() -> Vec<String> {
    vec!["id", "name", "real_name", "email"]
        .into_iter()
        .map(String::from)
        .collect()
}

fn default_channels_fields() -> Vec<String> {
    vec!["id", "name", "type", "members"]
        .into_iter()
        .map(String::from)
        .collect()
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            users_fields: default_users_fields(),
            channels_fields: default_channels_fields(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RetryConfig {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,

    #[serde(default = "default_initial_delay_ms")]
    pub initial_delay_ms: u64,

    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,

    #[serde(default = "default_exponential_base")]
    pub exponential_base: f64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackAppDistribution {
    #[default]
    CommercialExternal,
    MarketplaceOrInternal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionConfig {
    #[serde(default = "default_api_base_url")]
    pub api_base_url: String,

    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,

    #[serde(default = "default_max_idle_per_host")]
    pub max_idle_per_host: i32,

    #[serde(default = "default_pool_idle_timeout_seconds")]
    pub pool_idle_timeout_seconds: u64,

    #[serde(default = "default_rate_limit_per_minute")]
    pub rate_limit_per_minute: u32,

    #[serde(default)]
    pub app_distribution: SlackAppDistribution,
}

fn default_ttl_hours() -> u64 {
    168
}
fn default_channel_types() -> Vec<ConversationType> {
    vec![
        ConversationType::PublicChannel,
        ConversationType::PrivateChannel,
    ]
}
fn default_refresh_threshold_percent() -> u64 {
    10
}
fn default_max_attempts() -> u32 {
    3
}
fn default_initial_delay_ms() -> u64 {
    1000
}
fn default_max_delay_ms() -> u64 {
    60000
}
fn default_exponential_base() -> f64 {
    2.0
}
fn default_timeout_seconds() -> u64 {
    30
}
fn default_api_base_url() -> String {
    "https://slack.com/api".to_string()
}
fn default_max_idle_per_host() -> i32 {
    10
}
fn default_pool_idle_timeout_seconds() -> u64 {
    90
}
fn default_rate_limit_per_minute() -> u32 {
    20
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            ttl_users_hours: 168,
            ttl_channels_hours: 168,
            refresh_threshold_percent: 10,
            data_path: None,
            channel_types: default_channel_types(),
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 60000,
            exponential_base: 2.0,
        }
    }
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            api_base_url: default_api_base_url(),
            timeout_seconds: 30,
            max_idle_per_host: 10,
            pool_idle_timeout_seconds: 90,
            rate_limit_per_minute: 20,
            app_distribution: SlackAppDistribution::default(),
        }
    }
}

impl Config {
    pub fn load(config_path: Option<PathBuf>, cli_data_dir: Option<PathBuf>) -> Result<Self> {
        let mut config = Self::default();

        let path = config_path.or_else(Self::default_config_path);
        if let Some(p) = path.filter(|p| p.exists()) {
            let content = std::fs::read_to_string(&p)
                .context(format!("Failed to read config: {}", p.display()))?;
            config = toml::from_str(&content).context("Failed to parse config.toml")?;
        }

        if let Some(dir) = cli_data_dir {
            config.cache.data_path = Some(dir);
        }

        config.connection.api_base_url = config
            .connection
            .api_base_url
            .trim()
            .trim_end_matches('/')
            .to_string();

        config.validate()?;

        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.cache.ttl_users_hours == 0 || self.cache.ttl_channels_hours == 0 {
            anyhow::bail!("cache TTL values must be greater than zero");
        }

        if self.cache.refresh_threshold_percent == 0 || self.cache.refresh_threshold_percent > 100 {
            anyhow::bail!("cache.refresh_threshold_percent must be between 1 and 100");
        }

        if self.cache.channel_types.is_empty() {
            anyhow::bail!(
                "cache.channel_types must not be empty. Allowed values: \
                 public_channel, private_channel, mpim, im"
            );
        }

        if self.retry.max_attempts == 0 {
            anyhow::bail!("retry.max_attempts must be greater than zero");
        }

        if self.retry.initial_delay_ms == 0 || self.retry.max_delay_ms == 0 {
            anyhow::bail!("retry delay values must be greater than zero");
        }

        if self.retry.initial_delay_ms > self.retry.max_delay_ms {
            anyhow::bail!(
                "retry.initial_delay_ms must be less than or equal to retry.max_delay_ms"
            );
        }

        if self.retry.exponential_base < 1.0 || !self.retry.exponential_base.is_finite() {
            anyhow::bail!("retry.exponential_base must be finite and at least 1.0");
        }

        if self.connection.api_base_url.trim().is_empty() {
            anyhow::bail!("connection.api_base_url must not be empty");
        }

        if self.connection.timeout_seconds == 0
            || self.connection.pool_idle_timeout_seconds == 0
            || self.connection.rate_limit_per_minute == 0
        {
            anyhow::bail!("connection timeout and rate limit values must be greater than zero");
        }

        if self.connection.max_idle_per_host < 0 {
            anyhow::bail!("connection.max_idle_per_host must not be negative");
        }

        Ok(())
    }

    pub fn default_config_path() -> Option<PathBuf> {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|home| PathBuf::from(home).join(".config"))
            })
            .map(|mut p| {
                p.push("slack-cli");
                p.push("config.toml");
                p
            })
    }

    pub fn default_data_dir() -> Option<PathBuf> {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|home| PathBuf::from(home).join(".config"))
            })
            .map(|mut p| {
                p.push("slack-cli");
                p.push("cache");
                p
            })
    }

    pub fn db_path(&self) -> PathBuf {
        let mut path = self
            .cache
            .data_path
            .clone()
            .map(|p| expand_tilde(&p))
            .or_else(Self::default_data_dir)
            .unwrap_or_else(|| {
                #[cfg(target_os = "macos")]
                let fallback = std::path::PathBuf::from(
                    std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
                )
                .join("Library/Application Support/slack-cli/cache");

                #[cfg(not(target_os = "macos"))]
                let fallback = std::path::PathBuf::from(
                    std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
                )
                .join(".local/share/slack-cli/cache");

                fallback
            });

        if let Ok(canonical) = path.canonicalize() {
            path = canonical;
        }

        path.push("slack.db");
        path
    }

    pub fn show(&self, as_json: bool) -> Result<()> {
        if as_json {
            println!("{}", serde_json::to_string_pretty(self)?);
            return Ok(());
        }

        println!("Cache:");
        println!("  ttl_users_hours: {}", self.cache.ttl_users_hours);
        println!("  ttl_channels_hours: {}", self.cache.ttl_channels_hours);
        println!(
            "  refresh_threshold_percent: {}",
            self.cache.refresh_threshold_percent
        );
        println!(
            "  data_path: {}",
            self.cache
                .data_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| Self::default_data_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "-".to_string()))
        );
        let channel_types: Vec<&str> = self
            .cache
            .channel_types
            .iter()
            .map(|t| t.as_api_str())
            .collect();
        println!("  channel_types: {:?}", channel_types);
        println!("\nOutput:");
        println!("  users_fields: {:?}", self.output.users_fields);
        println!("  channels_fields: {:?}", self.output.channels_fields);
        println!("\nRetry:");
        println!("  max_attempts: {}", self.retry.max_attempts);
        println!("  initial_delay_ms: {}", self.retry.initial_delay_ms);
        println!("  max_delay_ms: {}", self.retry.max_delay_ms);
        println!("  exponential_base: {}", self.retry.exponential_base);
        println!("\nConnection:");
        println!("  api_base_url: {}", self.connection.api_base_url);
        println!("  timeout_seconds: {}", self.connection.timeout_seconds);
        println!("  max_idle_per_host: {}", self.connection.max_idle_per_host);
        println!(
            "  pool_idle_timeout_seconds: {}",
            self.connection.pool_idle_timeout_seconds
        );
        println!(
            "  rate_limit_per_minute: {}",
            self.connection.rate_limit_per_minute
        );
        println!(
            "  app_distribution: {}",
            match self.connection.app_distribution {
                SlackAppDistribution::CommercialExternal => "commercial_external",
                SlackAppDistribution::MarketplaceOrInternal => "marketplace_or_internal",
            }
        );

        Ok(())
    }

    pub fn edit(config_path: Option<PathBuf>) -> Result<()> {
        let path = config_path
            .or_else(Self::default_config_path)
            .context("Cannot determine config path")?;

        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let default = Self::default();
            let content = toml::to_string_pretty(&default)?;
            std::fs::write(&path, content)?;
        }

        let editor = std::env::var("EDITOR").unwrap_or_else(|_| {
            if cfg!(target_os = "macos") {
                "open".to_string()
            } else if cfg!(target_os = "windows") {
                "notepad".to_string()
            } else {
                "vi".to_string()
            }
        });

        let status = std::process::Command::new(&editor)
            .arg(&path)
            .status()
            .context(format!("Failed to launch editor: {}", editor))?;

        if !status.success() {
            anyhow::bail!("Editor exited with error");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    mod expand_tilde_tests {
        use super::*;

        #[test]
        fn expands_tilde_prefix() {
            let home = env::var("HOME").unwrap();
            let path = Path::new("~/test/path");
            let result = expand_tilde(path);
            assert_eq!(result, PathBuf::from(home).join("test/path"));
        }

        #[test]
        fn expands_tilde_only() {
            let home = env::var("HOME").unwrap();
            let path = Path::new("~");
            let result = expand_tilde(path);
            assert_eq!(result, PathBuf::from(home));
        }

        #[test]
        fn preserves_absolute_path() {
            let path = Path::new("/absolute/path");
            let result = expand_tilde(path);
            assert_eq!(result, PathBuf::from("/absolute/path"));
        }

        #[test]
        fn preserves_relative_path() {
            let path = Path::new("relative/path");
            let result = expand_tilde(path);
            assert_eq!(result, PathBuf::from("relative/path"));
        }

        #[test]
        fn handles_tilde_in_middle() {
            let path = Path::new("/path/~user/test");
            let result = expand_tilde(path);
            assert_eq!(result, PathBuf::from("/path/~user/test"));
        }

        #[test]
        fn handles_empty_path() {
            let path = Path::new("");
            let result = expand_tilde(path);
            assert_eq!(result, PathBuf::from(""));
        }
    }

    mod config_defaults {
        use super::*;

        #[test]
        fn cache_config_defaults() {
            let config = CacheConfig::default();
            assert_eq!(config.ttl_users_hours, 168);
            assert_eq!(config.ttl_channels_hours, 168);
            assert_eq!(config.refresh_threshold_percent, 10);
            assert!(config.data_path.is_none());
            assert_eq!(
                config.channel_types,
                vec![
                    ConversationType::PublicChannel,
                    ConversationType::PrivateChannel,
                ]
            );
        }

        #[test]
        fn load_normalizes_api_base_url() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            std::fs::write(
                &path,
                "[connection]\napi_base_url = \" https://slack.com/api/ \"\n",
            )
            .unwrap();

            let config = Config::load(Some(path), None).unwrap();
            assert_eq!(config.connection.api_base_url, "https://slack.com/api");
        }

        #[test]
        fn load_rejects_empty_channel_types() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            std::fs::write(&path, "[cache]\nchannel_types = []\n").unwrap();

            let err = Config::load(Some(path), None).unwrap_err();
            assert!(err.to_string().contains("channel_types must not be empty"));
        }

        #[test]
        fn load_rejects_invalid_connection_values() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            std::fs::write(&path, "[connection]\nmax_idle_per_host = -1\n").unwrap();

            let err = Config::load(Some(path), None).unwrap_err();
            assert!(
                err.to_string()
                    .contains("max_idle_per_host must not be negative")
            );
        }

        #[test]
        fn load_rejects_invalid_retry_values() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            std::fs::write(&path, "[retry]\nmax_attempts = 0\n").unwrap();

            let err = Config::load(Some(path), None).unwrap_err();
            assert!(
                err.to_string()
                    .contains("retry.max_attempts must be greater than zero")
            );
        }

        #[test]
        fn load_rejects_invalid_cache_threshold() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            std::fs::write(&path, "[cache]\nrefresh_threshold_percent = 101\n").unwrap();

            let err = Config::load(Some(path), None).unwrap_err();
            assert!(
                err.to_string()
                    .contains("refresh_threshold_percent must be between 1 and 100")
            );
        }

        #[test]
        fn retry_config_defaults() {
            let config = RetryConfig::default();
            assert_eq!(config.max_attempts, 3);
            assert_eq!(config.initial_delay_ms, 1000);
            assert_eq!(config.max_delay_ms, 60000);
            assert!((config.exponential_base - 2.0).abs() < f64::EPSILON);
        }

        #[test]
        fn connection_config_defaults() {
            let config = ConnectionConfig::default();
            assert_eq!(config.api_base_url, "https://slack.com/api");
            assert_eq!(config.timeout_seconds, 30);
            assert_eq!(config.max_idle_per_host, 10);
            assert_eq!(config.pool_idle_timeout_seconds, 90);
            assert_eq!(config.rate_limit_per_minute, 20);
            assert!(matches!(
                config.app_distribution,
                SlackAppDistribution::CommercialExternal
            ));
        }
    }
}
