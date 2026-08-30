use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::paths::{self, AppPaths};
use crate::slack::ConversationType;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub auth: AuthConfig,

    #[serde(default)]
    pub cache: CacheConfig,

    #[serde(default)]
    pub output: OutputConfig,

    #[serde(default)]
    pub retry: RetryConfig,

    #[serde(default)]
    pub connection: ConnectionConfig,
}

/// The Slack app a browser login authorizes against.
///
/// Keeping the id here spares every `auth login` a flag or an environment
/// variable. It is the app's public identifier — it travels in the authorize
/// URL — so nothing secret is stored by recording it.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    #[serde(default = "default_ttl_hours")]
    pub ttl_users_hours: u64,

    #[serde(default = "default_ttl_hours")]
    pub ttl_channels_hours: u64,

    #[serde(default = "default_refresh_threshold_percent")]
    pub refresh_threshold_percent: u64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_path: Option<PathBuf>,

    #[serde(default = "default_channel_types")]
    pub channel_types: Vec<ConversationType>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    #[serde(default = "default_users_fields")]
    pub users_fields: Vec<String>,

    #[serde(default = "default_channels_fields")]
    pub channels_fields: Vec<String>,

    #[serde(default = "default_messages_fields")]
    pub messages_fields: Vec<String>,
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

fn default_messages_fields() -> Vec<String> {
    vec![
        "ts",
        "user",
        "bot_id",
        "username",
        "text",
        "thread_ts",
        "reply_count",
        "subtype",
        "metadata",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            users_fields: default_users_fields(),
            channels_fields: default_channels_fields(),
            messages_fields: default_messages_fields(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct ConnectionConfig {
    #[serde(default = "default_api_base_url")]
    pub api_base_url: String,

    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,

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
            rate_limit_per_minute: 20,
            app_distribution: SlackAppDistribution::default(),
        }
    }
}

impl Config {
    pub fn load(
        paths: &AppPaths,
        config_path: Option<PathBuf>,
        cli_data_dir: Option<PathBuf>,
    ) -> Result<Self> {
        let mut config = Self::default();

        let path = config_path.unwrap_or_else(|| paths.config_file());
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .context(format!("Failed to read config: {}", path.display()))?;
            config =
                toml::from_str(&content).map_err(|error| parse_error(&path, &content, &error))?;
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

        if self.connection.timeout_seconds == 0 || self.connection.rate_limit_per_minute == 0 {
            anyhow::bail!("connection timeout and rate limit values must be greater than zero");
        }

        Ok(())
    }

    pub fn db_path(&self, paths: &AppPaths) -> PathBuf {
        self.cache
            .data_path
            .as_deref()
            .map(paths::expand_home)
            .unwrap_or_else(|| paths.cache_dir())
            .join("slack.db")
    }

    pub fn show(&self, paths: &AppPaths, as_json: bool) -> Result<()> {
        if as_json {
            println!("{}", serde_json::to_string_pretty(self)?);
            return Ok(());
        }

        println!("Auth:");
        println!(
            "  client_id: {}",
            self.auth.client_id.as_deref().unwrap_or("(unset)")
        );
        println!("\nCache:");
        println!("  ttl_users_hours: {}", self.cache.ttl_users_hours);
        println!("  ttl_channels_hours: {}", self.cache.ttl_channels_hours);
        println!(
            "  refresh_threshold_percent: {}",
            self.cache.refresh_threshold_percent
        );
        println!(
            "  data_path: {}",
            self.db_path(paths)
                .parent()
                .unwrap_or(&PathBuf::new())
                .display()
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
        println!("  messages_fields: {:?}", self.output.messages_fields);
        println!("\nRetry:");
        println!("  max_attempts: {}", self.retry.max_attempts);
        println!("  initial_delay_ms: {}", self.retry.initial_delay_ms);
        println!("  max_delay_ms: {}", self.retry.max_delay_ms);
        println!("  exponential_base: {}", self.retry.exponential_base);
        println!("\nConnection:");
        println!("  api_base_url: {}", self.connection.api_base_url);
        println!("  timeout_seconds: {}", self.connection.timeout_seconds);
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

    pub fn edit(paths: &AppPaths, config_path: Option<PathBuf>) -> Result<()> {
        let path = config_path.unwrap_or_else(|| paths.config_file());

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

/// `toml` reports a parse failure by quoting the offending line. A config the
/// CLI refuses is often one still holding a credential a newer schema no longer
/// accepts, and the refusal repeats on every invocation, so that rendering
/// would spread the value to the terminal and to whatever captures it. The
/// position carries the same information without it.
///
/// serde names the rejected key but not what it holds, which is what makes this
/// enough for every key a schema change has dropped. It does echo a value that
/// lands in a field of the wrong type or fails to name an enum variant, neither
/// of which a schema change produces: no field here has changed type and no
/// variant has been renamed.
fn parse_error(path: &Path, content: &str, error: &toml::de::Error) -> anyhow::Error {
    let Some(span) = error.span() else {
        return anyhow!("{}: {}", path.display(), error.message());
    };
    let (line, column) = line_column(content, span.start);
    anyhow!(
        "{}: line {line}, column {column}: {}",
        path.display(),
        error.message()
    )
}

fn line_column(content: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for (index, character) in content.char_indices() {
        if index >= offset {
            break;
        }
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> AppPaths {
        AppPaths::resolve().unwrap()
    }

    mod auth_config {
        use super::*;

        #[test]
        fn the_auth_section_supplies_the_oauth_app() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            std::fs::write(&path, "[auth]\nclient_id = \"1.2\"\n").unwrap();

            let config = Config::load(&paths(), Some(path), None).unwrap();
            assert_eq!(config.auth.client_id.as_deref(), Some("1.2"));
        }

        #[test]
        fn an_absent_auth_section_is_not_an_error() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            std::fs::write(&path, "[cache]\nttl_users_hours = 1\n").unwrap();

            let config = Config::load(&paths(), Some(path), None).unwrap();
            assert!(config.auth.client_id.is_none());
        }

        /// 0.10.0 accepted `client_secret` here. Surfacing the stale key
        /// beats silently ignoring a credential the CLI no longer reads.
        #[test]
        fn load_rejects_the_obsolete_client_secret() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            std::fs::write(&path, "[auth]\nclient_secret = \"shh\"\n").unwrap();

            let err = Config::load(&paths(), Some(path), None).unwrap_err();
            let chain: String = err
                .chain()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" | ");
            assert!(chain.contains("client_secret"), "chain: {chain}");
        }
    }

    /// A key the schema has dropped is often still holding the credential it
    /// was added for, and the refusal repeats on every invocation. The
    /// diagnostic has to name the key and the position so the user can delete
    /// the line, without carrying the value into the terminal or a log.
    #[test]
    fn a_dropped_key_is_named_without_its_value() {
        let dir = tempfile::tempdir().unwrap();
        for (name, body, key, value) in [
            (
                "auth.toml",
                "[auth]\nclient_id = \"1.2\"\nclient_secret = \"shh-canary\"\n",
                "client_secret",
                "shh-canary",
            ),
            (
                "legacy.toml",
                "user_token = \"xoxp-canary\"\n",
                "user_token",
                "xoxp-canary",
            ),
            (
                "legacy-bot.toml",
                "bot_token = \"xoxb-canary\"\n",
                "bot_token",
                "xoxb-canary",
            ),
            (
                "pool.toml",
                "[connection]\nmax_idle_per_host = \"canary\"\n",
                "max_idle_per_host",
                "canary",
            ),
            (
                "pool-dotted.toml",
                "connection.pool_idle_timeout_seconds = \"canary\"\n",
                "pool_idle_timeout_seconds",
                "canary",
            ),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, body).unwrap();

            let err = Config::load(&paths(), Some(path), None).unwrap_err();
            let chain: String = err
                .chain()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" | ");

            assert!(chain.contains(key), "{name} lost the key: {chain}");
            assert!(!chain.contains(value), "{name} echoed the value: {chain}");
        }
    }

    /// The position is what sends the user to the right line once the offending
    /// line is no longer quoted back at them. Columns count characters, as
    /// `toml`'s own rendering does, so a multi-byte line still points at the
    /// right place.
    #[test]
    fn a_parse_failure_reports_where_it_happened() {
        let dir = tempfile::tempdir().unwrap();
        for (name, body, line, column) in [
            (
                "trailing.toml",
                "[cache]\n# 한글 주석\nttl_users_hours = \n",
                3,
                19,
            ),
            ("multibyte.toml", "\"한글키\" = \n", 1, 9),
            ("first.toml", "user_token = \"x\"\n", 1, 1),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, body).unwrap();

            let err = Config::load(&paths(), Some(path.clone()), None).unwrap_err();
            let message = err.to_string();

            assert!(
                message.contains(&format!("line {line}, column {column}:")),
                "{name}: {message}"
            );
            assert!(
                message.contains(&path.display().to_string()),
                "{name} should name the file: {message}"
            );
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

            let config = Config::load(&paths(), Some(path), None).unwrap();
            assert_eq!(config.connection.api_base_url, "https://slack.com/api");
        }

        #[test]
        fn load_rejects_empty_channel_types() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            std::fs::write(&path, "[cache]\nchannel_types = []\n").unwrap();

            let err = Config::load(&paths(), Some(path), None).unwrap_err();
            assert!(err.to_string().contains("channel_types must not be empty"));
        }

        #[test]
        fn load_rejects_invalid_connection_values() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            std::fs::write(&path, "[connection]\ntimeout_seconds = 0\n").unwrap();

            let err = Config::load(&paths(), Some(path), None).unwrap_err();
            assert!(
                err.to_string()
                    .contains("connection timeout and rate limit values must be greater than zero")
            );
        }

        #[test]
        fn load_rejects_obsolete_top_level_tokens() {
            // v0.5.0 removed user_token / bot_token from config.toml; deny_unknown_fields
            // surfaces stale configs instead of silently ignoring them.
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            std::fs::write(&path, "user_token = \"xoxp-stale\"\n").unwrap();

            let err = Config::load(&paths(), Some(path), None).unwrap_err();
            let chain: String = err
                .chain()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" | ");
            assert!(chain.contains("user_token"), "chain: {chain}");
        }

        #[test]
        fn load_rejects_invalid_retry_values() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            std::fs::write(&path, "[retry]\nmax_attempts = 0\n").unwrap();

            let err = Config::load(&paths(), Some(path), None).unwrap_err();
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

            let err = Config::load(&paths(), Some(path), None).unwrap_err();
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
            assert_eq!(config.rate_limit_per_minute, 20);
            assert!(matches!(
                config.app_distribution,
                SlackAppDistribution::CommercialExternal
            ));
        }
    }
}
