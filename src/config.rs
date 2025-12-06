use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Expand tilde (~) to home directory in path
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
    pub bot_token: Option<String>,
    pub user_token: Option<String>,

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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionConfig {
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,

    #[serde(default = "default_max_idle_per_host")]
    pub max_idle_per_host: i32,

    #[serde(default = "default_pool_idle_timeout_seconds")]
    pub pool_idle_timeout_seconds: u64,

    #[serde(default = "default_rate_limit_per_minute")]
    pub rate_limit_per_minute: u32,
}

fn default_ttl_hours() -> u64 {
    168
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
            timeout_seconds: 30,
            max_idle_per_host: 10,
            pool_idle_timeout_seconds: 90,
            rate_limit_per_minute: 20,
        }
    }
}

impl Config {
    pub fn load(
        config_path: Option<PathBuf>,
        cli_token: Option<String>,
        cli_user_token: Option<String>,
        cli_data_dir: Option<PathBuf>,
    ) -> Result<Self> {
        let mut config = Self::default();

        let path = config_path.or_else(Self::default_config_path);
        if let Some(p) = path.filter(|p| p.exists()) {
            let content = std::fs::read_to_string(&p)
                .context(format!("Failed to read config: {}", p.display()))?;
            config = toml::from_str(&content).context("Failed to parse config.toml")?;
        }

        if let Ok(token) = std::env::var("SLACK_BOT_TOKEN") {
            config.bot_token = Some(token);
        }
        if let Ok(token) = std::env::var("SLACK_USER_TOKEN") {
            config.user_token = Some(token);
        }

        if let Some(token) = cli_token {
            config.bot_token = Some(token);
        }
        if let Some(token) = cli_user_token {
            config.user_token = Some(token);
        }
        if let Some(dir) = cli_data_dir {
            config.cache.data_path = Some(dir);
        }

        if config.bot_token.is_none() && config.user_token.is_none() {
            anyhow::bail!(
                "No Slack token found. Set via:\n\
                 - Config file: {}\n\
                 - Environment: SLACK_BOT_TOKEN or SLACK_USER_TOKEN\n\
                 - CLI flag: --token\n\
                 - Or run: slack-cli config init",
                Self::default_config_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "~/.config/slack-cli/config.toml".to_string())
            );
        }

        Ok(config)
    }

    pub fn default_config_path() -> Option<PathBuf> {
        // Use XDG Base Directory or ~/.config for all platforms
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
        // Place cache under config directory for simplicity: ~/.config/slack-cli/cache
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
            .map(|p| expand_tilde(&p)) // Expand ~ to home directory
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

    pub fn show_masked(&self, as_json: bool) -> Result<()> {
        let mut masked = self.clone();

        if let Some(token) = &masked.bot_token {
            masked.bot_token = Some(mask_token(token));
        }
        if let Some(token) = &masked.user_token {
            masked.user_token = Some(mask_token(token));
        }

        if as_json {
            println!("{}", serde_json::to_string_pretty(&masked)?);
        } else {
            println!("Configuration:");
            println!(
                "  bot_token: {}",
                masked.bot_token.as_deref().unwrap_or("-")
            );
            println!(
                "  user_token: {}",
                masked.user_token.as_deref().unwrap_or("-")
            );
            println!("\nCache:");
            println!("  ttl_users_hours: {}", masked.cache.ttl_users_hours);
            println!("  ttl_channels_hours: {}", masked.cache.ttl_channels_hours);
            println!(
                "  refresh_threshold_percent: {}",
                masked.cache.refresh_threshold_percent
            );
            println!(
                "  data_path: {}",
                masked
                    .cache
                    .data_path
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| Self::default_data_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "-".to_string()))
            );
            println!("\nOutput:");
            println!("  users_fields: {:?}", masked.output.users_fields);
            println!("  channels_fields: {:?}", masked.output.channels_fields);
            println!("\nRetry:");
            println!("  max_attempts: {}", masked.retry.max_attempts);
            println!("  initial_delay_ms: {}", masked.retry.initial_delay_ms);
            println!("  max_delay_ms: {}", masked.retry.max_delay_ms);
            println!("  exponential_base: {}", masked.retry.exponential_base);
            println!("\nConnection:");
            println!("  timeout_seconds: {}", masked.connection.timeout_seconds);
            println!(
                "  max_idle_per_host: {}",
                masked.connection.max_idle_per_host
            );
            println!(
                "  pool_idle_timeout_seconds: {}",
                masked.connection.pool_idle_timeout_seconds
            );
            println!(
                "  rate_limit_per_minute: {}",
                masked.connection.rate_limit_per_minute
            );
        }

        Ok(())
    }

    pub fn edit_config() -> Result<()> {
        let path = Self::default_config_path().context("Cannot determine config path")?;

        if !path.exists() {
            println!(
                "Config file does not exist: {}\nRun: slack-cli config init",
                path.display()
            );
            return Ok(());
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

fn mask_token(token: &str) -> String {
    if token.len() <= 8 {
        "*".repeat(token.len())
    } else {
        let prefix = &token[..4];
        let suffix = &token[token.len() - 4..];
        format!("{}...{}", prefix, suffix)
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
            // Tilde in middle should NOT be expanded
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

    mod mask_token_tests {
        use super::*;

        #[test]
        fn masks_short_token() {
            assert_eq!(mask_token("abc"), "***");
            assert_eq!(mask_token("12345678"), "********");
        }

        #[test]
        fn masks_long_token_with_ellipsis() {
            assert_eq!(mask_token("xoxb-123456789"), "xoxb...6789");
            assert_eq!(mask_token("123456789"), "1234...6789");
        }

        #[test]
        fn handles_empty_token() {
            assert_eq!(mask_token(""), "");
        }

        #[test]
        fn handles_exact_boundary() {
            // 8 chars = fully masked
            assert_eq!(mask_token("12345678"), "********");
            // 9 chars = prefix...suffix
            assert_eq!(mask_token("123456789"), "1234...6789");
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
            assert_eq!(config.timeout_seconds, 30);
            assert_eq!(config.max_idle_per_host, 10);
            assert_eq!(config.pool_idle_timeout_seconds, 90);
            assert_eq!(config.rate_limit_per_minute, 20);
        }
    }
}
