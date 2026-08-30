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

    #[serde(default)]
    pub events: EventsConfig,
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

    /// Scopes to leave out of the authorization request, for an app the
    /// workspace will not grant them to. Every entry must be one the CLI would
    /// otherwise ask for, so a name no method needs is refused instead of
    /// quietly doing nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_scopes: Vec<String>,
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

/// How long a matched event is kept, and therefore how much of other people's
/// conversation reaches this disk at all.
///
/// The axis is duration, not volume: `Stream` keeps nothing, `Spool` keeps an
/// event only until a consumer has acknowledged it, and `Archive` keeps it for
/// `retention_days`. How *much* of each event is kept is the separate
/// `store_body` decision, so a workspace that forbids storing message text can
/// still replay which threads moved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventRetention {
    /// Nothing reaches the event log. Sinks are the only delivery, so a
    /// consumer that is not running when an event arrives never sees it.
    Stream,
    /// Kept until a consumer acknowledges it, then deleted.
    #[default]
    Spool,
    /// Kept for `retention_days`, so past events can be replayed.
    Archive,
}

impl EventRetention {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stream => "stream",
            Self::Spool => "spool",
            Self::Archive => "archive",
        }
    }

    /// Whether an event log exists at all. `false` selects the null store, and
    /// with it the commands that read back events stop being answerable.
    pub const fn durable(self) -> bool {
        !matches!(self, Self::Stream)
    }
}

/// What to do when the in-flight buffer is full because a sink cannot keep up.
///
/// Blocking is deliberately not an option: the socket task acknowledges an
/// envelope to Slack before handing it on, and making that path wait on a slow
/// consumer would turn a slow agent into redelivery and, past Slack's
/// threshold, into a disabled subscription.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverflowPolicy {
    /// Discard the oldest queued event. Keeps the newest view of the
    /// workspace, which is what an assistant reacting to now wants.
    #[default]
    DropOldest,
    /// Discard the event that would not fit, preserving arrival order.
    DropNewest,
}

impl OverflowPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DropOldest => "drop_oldest",
            Self::DropNewest => "drop_newest",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKindConfig {
    Message,
    ReactionAdded,
    ReactionRemoved,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventsConfig {
    #[serde(default)]
    pub mode: EventRetention,

    /// Whether a stored event keeps the message text and the raw payload.
    /// With it off the log is an index of references — channel, ts, author,
    /// which rule matched — and Slack stays the only copy of what was said.
    #[serde(default)]
    pub store_body: bool,

    /// Whether an event carries the Slack payload it was built from.
    ///
    /// Off by default and worth turning on only while writing rules, when
    /// seeing the shape Slack actually sent is the whole point. It puts the
    /// complete message — every field, not just the ones the CLI models — into
    /// the stream, and into the log when `store_body` is also on.
    #[serde(default)]
    pub store_raw: bool,

    #[serde(default = "default_retention_days")]
    pub retention_days: u64,

    #[serde(default = "default_event_buffer")]
    pub buffer: usize,

    #[serde(default)]
    pub on_overflow: OverflowPolicy,

    /// Ceiling on the *live data* in the event log, enforced regardless of
    /// retention, so a misconfigured rule cannot fill the disk.
    ///
    /// Live data, not file size: SQLite keeps the pages a delete frees on a
    /// freelist and returns them to the filesystem only as it vacuums, so the
    /// file trails this figure down rather than tracking it. Measuring the
    /// file instead would make each prune see no progress and delete the whole
    /// log to satisfy a limit only vacuuming could meet.
    #[serde(default = "default_events_max_bytes")]
    pub max_bytes: u64,

    /// Whether a reconnect asks Slack for what was missed. Socket Mode
    /// replays nothing, so this is the only recovery there is — and it reads
    /// `conversations.history`, which for a non-Marketplace app is one request
    /// a minute, hence the bounds below.
    #[serde(default = "default_true")]
    pub backfill: bool,

    #[serde(default = "default_backfill_max_channels")]
    pub backfill_max_channels: usize,

    #[serde(default = "default_backfill_max_age_hours")]
    pub backfill_max_age_hours: i64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_path: Option<PathBuf>,

    #[serde(default, rename = "sink", skip_serializing_if = "Vec::is_empty")]
    pub sinks: Vec<SinkConfig>,

    #[serde(default, rename = "rule", skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RuleConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SinkKind {
    #[default]
    Stdout,
    Exec,
    Http,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SinkConfig {
    pub name: String,

    #[serde(default, rename = "type")]
    pub kind: SinkKind,

    /// `exec`: the program and its arguments. The event JSON arrives on stdin.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,

    /// `http`: where to POST the event JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// How long one delivery may take, covering the whole handoff. Delivery
    /// is serial by design — one task owns the pipeline, which is what keeps
    /// events in order and the deduplication gate race-free — so this is the
    /// only bound on how long a slow sink can hold it.
    #[serde(default = "default_sink_timeout_seconds")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuleConfig {
    pub name: String,

    /// Which events the rule looks at. A rule that subscribes on a reaction
    /// still has to list `message` to match the replies that follow.
    #[serde(default = "default_rule_events")]
    pub on: Vec<EventKindConfig>,

    /// Matches a message that mentions the authenticated user.
    #[serde(default)]
    pub mentions_me: bool,

    /// Case-insensitive substrings; any one matching is enough.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from_users: Vec<String>,

    /// Restricts the rule to these conversations. Channel IDs, not names: a
    /// name is ambiguous and would make the rule depend on a cache the daemon
    /// otherwise has no reason to hold.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<String>,

    /// Reacting with this emoji subscribes the thread; removing it
    /// unsubscribes. Every later reply in a subscribed thread matches, which
    /// is what makes an emoji a subscribe button.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscribe_emoji: Option<String>,

    /// Whether the authenticated user's own messages can match. Off by
    /// default: an assistant that answers itself is a loop.
    #[serde(default)]
    pub include_own_messages: bool,

    /// Which sinks receive a match. Empty means every configured sink.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sinks: Vec<String>,
}

fn default_retention_days() -> u64 {
    7
}
fn default_event_buffer() -> usize {
    1024
}
fn default_events_max_bytes() -> u64 {
    256 * 1024 * 1024
}
fn default_backfill_max_channels() -> usize {
    20
}
fn default_backfill_max_age_hours() -> i64 {
    24
}
fn default_sink_timeout_seconds() -> u64 {
    30
}
fn default_true() -> bool {
    true
}
fn default_rule_events() -> Vec<EventKindConfig> {
    vec![EventKindConfig::Message]
}

impl Default for EventsConfig {
    fn default() -> Self {
        Self {
            mode: EventRetention::default(),
            store_body: false,
            store_raw: false,
            retention_days: default_retention_days(),
            buffer: default_event_buffer(),
            on_overflow: OverflowPolicy::default(),
            max_bytes: default_events_max_bytes(),
            backfill: true,
            backfill_max_channels: default_backfill_max_channels(),
            backfill_max_age_hours: default_backfill_max_age_hours(),
            data_path: None,
            sinks: Vec::new(),
            rules: Vec::new(),
        }
    }
}

impl EventsConfig {
    /// The sink used when none is configured: the events themselves, one JSON
    /// object per line, on stdout. It is what `watch` piped into an agent
    /// needs, and it writes nothing anywhere else.
    pub const DEFAULT_SINK: &'static str = "stdout";

    /// The rule used when none is configured. A personal assistant that
    /// forwards only what names you is the lean default; anything wider is a
    /// deliberate choice made in `config.toml`.
    pub const DEFAULT_RULE: &'static str = "mention";

    pub fn effective_sinks(&self) -> Vec<SinkConfig> {
        if self.sinks.is_empty() {
            return vec![default_sink(Self::DEFAULT_SINK)];
        }
        self.sinks.clone()
    }

    pub fn effective_rules(&self) -> Vec<RuleConfig> {
        if self.rules.is_empty() {
            return vec![RuleConfig {
                name: Self::DEFAULT_RULE.to_string(),
                on: default_rule_events(),
                mentions_me: true,
                keywords: Vec::new(),
                from_users: Vec::new(),
                channels: Vec::new(),
                subscribe_emoji: None,
                include_own_messages: false,
                sinks: Vec::new(),
            }];
        }
        self.rules.clone()
    }
}

/// A sink with the defaults filled in. Named here so tests and the built-in
/// stdout sink construct one the same way the deserializer does.
pub fn default_sink(name: &str) -> SinkConfig {
    SinkConfig {
        name: name.to_string(),
        kind: SinkKind::default(),
        command: Vec::new(),
        url: None,
        timeout_seconds: default_sink_timeout_seconds(),
    }
}

impl EventsConfig {
    /// Every way an events configuration can be wrong, refused at load.
    ///
    /// A daemon runs unattended for days, so a rule that names a sink that
    /// does not exist, or a channel by a name it cannot resolve, has to fail
    /// where someone is watching — not silently forward nothing at 3am.
    fn validate(&self) -> Result<()> {
        if self.buffer == 0 {
            anyhow::bail!("events.buffer must be greater than zero");
        }
        // Below a megabyte the schema and its indexes alone exceed the limit,
        // so every prune would empty the log to chase a figure it can never
        // reach.
        const MIN_MAX_BYTES: u64 = 1024 * 1024;
        if self.max_bytes < MIN_MAX_BYTES {
            anyhow::bail!(
                "events.max_bytes is {} but an empty event log is already larger than that. \
                 Use at least {MIN_MAX_BYTES}",
                self.max_bytes
            );
        }
        if self.mode == EventRetention::Archive && self.retention_days == 0 {
            anyhow::bail!(
                "events.mode = \"archive\" keeps events for events.retention_days, which \
                 must be greater than zero. Use events.mode = \"spool\" to keep them only \
                 until a consumer acknowledges them"
            );
        }
        if self.backfill && (self.backfill_max_channels == 0 || self.backfill_max_age_hours <= 0) {
            anyhow::bail!(
                "events.backfill_max_channels and events.backfill_max_age_hours must be \
                 greater than zero while events.backfill is enabled"
            );
        }

        let sinks = self.effective_sinks();
        let mut seen_sinks: Vec<&str> = Vec::new();
        for sink in &sinks {
            if sink.name.trim().is_empty() {
                anyhow::bail!("every events sink needs a non-empty name");
            }
            if seen_sinks.contains(&sink.name.as_str()) {
                anyhow::bail!("events sink {:?} is declared more than once", sink.name);
            }
            seen_sinks.push(&sink.name);

            if sink.timeout_seconds == 0 {
                anyhow::bail!(
                    "events sink {:?}: timeout_seconds must be greater than zero",
                    sink.name
                );
            }
            match sink.kind {
                SinkKind::Stdout => {
                    if !sink.command.is_empty() || sink.url.is_some() {
                        anyhow::bail!(
                            "events sink {:?} is type \"stdout\" and takes neither command \
                             nor url",
                            sink.name
                        );
                    }
                }
                SinkKind::Exec => {
                    if sink.command.is_empty() {
                        anyhow::bail!(
                            "events sink {:?} is type \"exec\" and needs a command",
                            sink.name
                        );
                    }
                }
                SinkKind::Http => {
                    let url = sink.url.as_deref().unwrap_or_default();
                    if !(url.starts_with("http://") || url.starts_with("https://")) {
                        anyhow::bail!(
                            "events sink {:?} is type \"http\" and needs an http(s) url",
                            sink.name
                        );
                    }
                }
            }
        }

        let mut seen_rules: Vec<&str> = Vec::new();
        for rule in &self.effective_rules() {
            if rule.name.trim().is_empty() {
                anyhow::bail!("every events rule needs a non-empty name");
            }
            if seen_rules.contains(&rule.name.as_str()) {
                anyhow::bail!("events rule {:?} is declared more than once", rule.name);
            }
            seen_rules.push(&rule.name);

            if rule.on.is_empty() {
                anyhow::bail!(
                    "events rule {:?} subscribes to no event kind. List at least one of \
                     message, reaction_added, reaction_removed under `on`",
                    rule.name
                );
            }
            if rule
                .keywords
                .iter()
                .any(|keyword| keyword.trim().is_empty())
            {
                anyhow::bail!(
                    "events rule {:?} lists an empty keyword, which every message contains. \
                     Remove it, or the rule forwards the whole workspace",
                    rule.name
                );
            }
            if !rule.matches_anything_specific() {
                anyhow::bail!(
                    "events rule {:?} states no condition, so it would forward every event \
                     the workspace produces. Give it mentions_me, keywords, from_users, \
                     channels or subscribe_emoji",
                    rule.name
                );
            }
            // A predicate that reads message text needs message events to
            // read. A rule listing only reaction kinds alongside one would
            // validate and then never fire.
            let reads_text = rule.mentions_me || !rule.keywords.is_empty();
            if reads_text && !rule.on.contains(&EventKindConfig::Message) {
                anyhow::bail!(
                    "events rule {:?} matches on message text but does not list message under \
                     `on`, so it would never fire — a reaction event carries no text",
                    rule.name
                );
            }

            if let Some(emoji) = &rule.subscribe_emoji {
                if emoji.contains(':') || emoji.trim().is_empty() {
                    anyhow::bail!(
                        "events rule {:?}: subscribe_emoji is a name without colons, e.g. \"eyes\"",
                        rule.name
                    );
                }
                // The subscription is driven by the reaction, so a rule that
                // never sees one would sit there subscribing nothing.
                if !rule.on.contains(&EventKindConfig::ReactionAdded) {
                    anyhow::bail!(
                        "events rule {:?} subscribes on :{emoji}: but does not list \
                         reaction_added under `on`, so it would never see the reaction",
                        rule.name
                    );
                }
                if !rule.on.contains(&EventKindConfig::Message) {
                    anyhow::bail!(
                        "events rule {:?} subscribes threads on :{emoji}: but does not list \
                         message under `on`, so no reply in a subscribed thread would match",
                        rule.name
                    );
                }
                // Taking the emoji off is how a thread is unsubscribed. A rule
                // that never sees the removal subscribes threads it can never
                // let go of.
                if !rule.on.contains(&EventKindConfig::ReactionRemoved) {
                    anyhow::bail!(
                        "events rule {:?} subscribes on :{emoji}: but does not list \
                         reaction_removed under `on`, so a thread it subscribes could never \
                         be unsubscribed",
                        rule.name
                    );
                }
            }
            for channel in &rule.channels {
                if !is_conversation_id(channel) {
                    anyhow::bail!(
                        "events rule {:?} lists channel {channel:?}. Rules take conversation \
                         IDs, not names — run `slack-cli channels {channel}` to find it",
                        rule.name
                    );
                }
            }
            for name in &rule.sinks {
                if !seen_sinks.contains(&name.as_str()) {
                    anyhow::bail!(
                        "events rule {:?} sends to sink {name:?}, which is not declared",
                        rule.name
                    );
                }
            }
        }

        Ok(())
    }
}

impl RuleConfig {
    /// Whether the rule narrows the stream at all. A rule with no condition
    /// matches every message in the workspace, which at Slack's event volume
    /// is never what someone meant to write.
    fn matches_anything_specific(&self) -> bool {
        self.mentions_me
            || self.subscribe_emoji.is_some()
            || !self.keywords.is_empty()
            || !self.from_users.is_empty()
            || !self.channels.is_empty()
    }
}

/// The shape of a Slack conversation id: `C`/`D`/`G` and then upper-case
/// alphanumerics. Mirrors the check `main` makes on a `<channel>` argument.
fn is_conversation_id(input: &str) -> bool {
    let mut chars = input.chars();
    match chars.next() {
        Some('C' | 'D' | 'G') => {}
        _ => return false,
    }
    chars.clone().count() >= 8 && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
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

        // Reading decides whether the file is there, rather than a prior
        // `exists()`: that question cannot tell a path the user named from the
        // default one, and answers "no" for a file it merely failed to stat —
        // silently discarding a `--config` argument, or a real config behind a
        // trailing slash or an unsearchable parent.
        let named = config_path.is_some();
        let path = config_path.unwrap_or_else(|| paths.config_file());
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                config = toml::from_str(&content)
                    .map_err(|error| parse_error(&path, &content, &error))?;
            }
            // A default location that has not been created is a fresh install.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !named => {}
            Err(error) => {
                return Err(error).context(format!("could not read {}", path.display()));
            }
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

        config.validate(&path)?;

        Ok(config)
    }

    fn validate(&self, path: &Path) -> Result<()> {
        // Every command loads the config, so this message has to stand on its
        // own: the command it would otherwise point at fails the same way.
        for scope in &self.auth.exclude_scopes {
            // An app-level scope is refused for a different reason than an
            // unknown one, and saying "no method requires it" would be false:
            // one does, on an axis this setting cannot reach.
            if scope == crate::slack::scopes::APP_SCOPE {
                anyhow::bail!(
                    "{}: auth.exclude_scopes lists {scope:?}, which is an app-level scope. It is \
                     granted in the Slack app's own configuration rather than by an \
                     authorization, so excluding it here would do nothing. Remove that entry",
                    path.display()
                );
            }
            if !crate::slack::scopes::is_known(scope) {
                anyhow::bail!(
                    "{}: auth.exclude_scopes lists {scope:?}, which no method this CLI calls \
                     requires. Remove that entry",
                    path.display()
                );
            }
        }

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

        self.events.validate()?;

        Ok(())
    }

    pub fn events_dir(&self, paths: &AppPaths) -> PathBuf {
        self.events
            .data_path
            .as_deref()
            .map(paths::expand_home)
            .unwrap_or_else(|| paths.events_dir())
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
        println!("  exclude_scopes: {:?}", self.auth.exclude_scopes);
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
        println!("\nEvents:");
        println!("  mode: {}", self.events.mode.as_str());
        println!("  store_body: {}", self.events.store_body);
        println!("  retention_days: {}", self.events.retention_days);
        println!("  buffer: {}", self.events.buffer);
        println!("  on_overflow: {}", self.events.on_overflow.as_str());
        println!("  max_bytes: {}", self.events.max_bytes);
        println!("  backfill: {}", self.events.backfill);
        println!("  data_path: {}", self.events_dir(paths).display());
        println!(
            "  sinks: {:?}",
            self.events
                .effective_sinks()
                .iter()
                .map(|sink| sink.name.clone())
                .collect::<Vec<_>>()
        );
        println!(
            "  rules: {:?}",
            self.events
                .effective_rules()
                .iter()
                .map(|rule| rule.name.clone())
                .collect::<Vec<_>>()
        );

        Ok(())
    }

    pub fn edit(paths: &AppPaths, config_path: Option<PathBuf>) -> Result<()> {
        let path = config_path.unwrap_or_else(|| paths.config_file());

        if path.exists() && !path.is_file() {
            anyhow::bail!("{} is not a file", path.display());
        }

        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let default = Self::default();
            let content = toml::to_string_pretty(&default)?;
            std::fs::write(&path, content)
                .with_context(|| format!("could not create {}", path.display()))?;
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
        fn excluded_scopes_load_and_must_be_ones_the_cli_asks_for() {
            let dir = tempfile::tempdir().unwrap();

            let good = dir.path().join("good.toml");
            std::fs::write(
                &good,
                "[auth]\nexclude_scopes = [\"pins:write\", \"bookmarks:read\"]\n",
            )
            .unwrap();
            let config = Config::load(&paths(), Some(good), None).unwrap();
            assert_eq!(config.auth.exclude_scopes, ["pins:write", "bookmarks:read"]);

            let bad = dir.path().join("bad.toml");
            std::fs::write(&bad, "[auth]\nexclude_scopes = [\"pins:wrote\"]\n").unwrap();
            let err = Config::load(&paths(), Some(bad.clone()), None).unwrap_err();
            let message = err.to_string();
            assert!(message.contains("pins:wrote"), "{message}");
            // Every command loads the config, so the one it would point at
            // fails too; the message has to name the file instead.
            assert!(
                message.contains(&bad.display().to_string()),
                "should name the file: {message}"
            );
            assert!(!message.contains("auth scopes"), "{message}");
            assert!(message.contains("Remove that entry"), "{message}");
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

    /// `exists()` cannot tell a path the user named from the default one, and
    /// answers "no" for a file it merely failed to stat. Only a default
    /// location that has never been created may load silently.
    #[test]
    fn only_a_missing_default_location_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();

        let absent = dir.path().join("named.toml");
        let err = Config::load(&paths(), Some(absent.clone()), None).unwrap_err();
        assert!(
            err.to_string().contains(&absent.display().to_string()),
            "a named path that is absent must be reported: {err}"
        );

        let file = dir.path().join("real.toml");
        std::fs::write(&file, "[cache]\nttl_users_hours = 24\n").unwrap();
        let unstattable = dir.path().join("real.toml/");
        let err = Config::load(&paths(), Some(unstattable), None).unwrap_err();
        assert!(
            err.to_string().contains("real.toml"),
            "a path that cannot be stat'd must be reported: {err}"
        );

        let loaded = Config::load(&paths(), Some(file), None).unwrap();
        assert_eq!(loaded.cache.ttl_users_hours, 24);
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
