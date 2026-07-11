use anyhow::{Context, Result};
use chrono::{Local, NaiveDate, TimeZone};
use clap::Parser;
use serde_json::Value;
use slack_cli::{
    auth::{self, AuthError, AuthLoadOptions, Authenticator, EnvOverrides},
    cache::{self, CacheStatus},
    cli::{CacheAction, Cli, Command, ConfigAction, MessageContent, RefreshTarget},
    config, format, slack,
    slack::{MessageMetadata, MessagePayload, SlackApiError},
};
use std::io::Read;
use std::process::ExitCode;
use std::sync::Arc;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let level = if cli.verbose { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(level)
        .with_writer(std::io::stderr)
        .compact()
        .with_target(false)
        .init();

    dotenvy::dotenv().ok();

    let as_json = cli.json;
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let (code, exit) = classify_error(&err);
            if as_json {
                eprintln!(
                    "{}",
                    serde_json::json!({ "error": { "code": code, "message": format!("{err:#}") } })
                );
            } else {
                eprintln!("Error: {err:?}");
            }
            ExitCode::from(exit)
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let config = config::Config::load(cli.config.clone(), cli.data_dir.clone())?;

    if let Command::Config { action } = &cli.command {
        return handle_config_action(action, cli.json, cli.config.clone(), &config);
    }

    let store_path = auth::default_store_path()
        .context("could not determine auth store path (set XDG_CONFIG_HOME or HOME)")?;
    let authenticator = Arc::new(Authenticator::load(AuthLoadOptions {
        store_path,
        overrides: EnvOverrides::capture(),
        explicit_profile: cli.profile.clone(),
    })?);

    if let Command::Auth { action } = cli.command {
        return auth::cli_handler::handle(
            action,
            cli.profile.clone(),
            config,
            authenticator,
            cli.json,
        )
        .await;
    }

    let slack = Arc::new(slack::SlackClient::new(config.clone(), authenticator)?);

    let db_path = config.db_path();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db_path_str = db_path
        .to_str()
        .context("Database path contains invalid UTF-8 characters")?;
    let cache = Arc::new(cache::SqliteCache::new(db_path_str).await?);

    let threshold = config.cache.refresh_threshold_percent;
    let cache_status = cache.get_cache_status(
        config.cache.ttl_users_hours,
        config.cache.ttl_channels_hours,
        threshold,
    )?;

    match cli.command {
        Command::Users {
            query,
            id,
            limit,
            expand,
        } => {
            ensure_users_cache(&slack, &cache, cli.json).await?;
            let users = if let Some(ids) = id {
                cache.get_users_by_ids(&ids)?
            } else {
                cache.search_users(query.as_deref().unwrap_or(""), limit, false)?
            };
            let fields = merge_fields(&config.output.users_fields, expand.as_deref());
            format::print_users(&users, &fields, cli.json);
        }

        Command::Channels {
            query,
            id,
            limit,
            expand,
        } => {
            ensure_channels_cache(&slack, &cache, cli.json).await?;
            let channels = if let Some(ids) = id {
                cache.get_channels_by_ids(&ids)?
            } else {
                cache.search_channels(query.as_deref().unwrap_or(""), limit)?
            };
            let fields = merge_fields(&config.output.channels_fields, expand.as_deref());
            format::print_channels(&channels, &fields, cli.json);
        }

        Command::Send {
            channel,
            content,
            thread,
        } => {
            let payload = build_payload(content)?;
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let result = slack.messages.send(&id, payload, thread.as_deref()).await?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("✓ Sent: {}", result.ts);
            }
        }

        Command::Update {
            channel,
            ts,
            content,
        } => {
            let payload = build_payload(content)?;
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let result = slack.messages.update(&id, &ts, payload).await?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("✓ Updated: {}", result.ts);
            }
        }

        Command::Delete { channel, ts } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let result = slack.messages.delete(&id, &ts).await?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("✓ Deleted: {}", result.ts);
            }
        }

        Command::Permalink { channel, ts } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let link = slack.messages.permalink(&id, &ts).await?;

            if cli.json {
                println!("{}", serde_json::json!({ "permalink": link }));
            } else {
                println!("{}", link);
            }
        }

        Command::Messages {
            channel,
            limit,
            cursor,
            oldest,
            latest,
            exclude_bots,
            expand,
        } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;

            let oldest_ts = oldest.map(|o| parse_timestamp(&o)).transpose()?;
            let latest_ts = latest.map(|l| parse_timestamp(&l)).transpose()?;

            let (mut messages, next_cursor) = slack
                .messages
                .history(
                    &id,
                    limit,
                    cursor.as_deref(),
                    oldest_ts.as_deref(),
                    latest_ts.as_deref(),
                )
                .await?;

            if exclude_bots {
                messages.retain(|m| m.bot_id.is_none());
            }

            let fields = merge_fields(&config.output.messages_fields, expand.as_deref());
            format::print_history(
                &messages,
                next_cursor.as_deref(),
                cli.json,
                &fields,
                Some(&cache),
            );
        }

        Command::Thread {
            channel,
            ts,
            limit,
            exclude_bots,
            expand,
        } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let mut messages = slack.messages.replies(&id, &ts, limit).await?;
            if exclude_bots {
                messages.retain(|m| m.bot_id.is_none());
            }
            let fields = merge_fields(&config.output.messages_fields, expand.as_deref());
            format::print_messages(&messages, cli.json, &fields, Some(&cache));
        }

        Command::Members { channel } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let members = slack.channels.members(&id).await?;
            format::print_members(&members, &cache, cli.json);
        }

        Command::Search {
            query,
            limit,
            channel_types,
            content_types,
            channel,
            before,
            after,
            include_context_messages,
            include_bots,
            include_archived_channels,
            disable_semantic_search,
            sort,
            sort_dir,
        } => {
            let context_channel_id = match channel {
                Some(input) => Some(resolve_channel(&input, &slack, &cache, cli.json).await?),
                None => None,
            };

            let before = before.as_deref().map(parse_unix_seconds).transpose()?;
            let after = after.as_deref().map(parse_unix_seconds).transpose()?;

            let options = slack::SearchOptions {
                limit,
                channel_types,
                content_types,
                context_channel_id,
                include_archived_channels,
                before,
                after,
                include_bots,
                disable_semantic_search,
                sort,
                sort_dir,
                include_context_messages,
                include_message_blocks: cli.json,
                highlight: !cli.json,
            };
            let results = slack.search.context(&query, &options).await?;

            format::print_search_results(&results, cli.json);
        }

        Command::React { channel, ts, emoji } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            slack.reactions.add(&id, &ts, &emoji).await?;

            if cli.json {
                println!("{{\"ok\": true}}");
            } else {
                println!("✓ Added :{}: reaction", emoji.trim_matches(':'));
            }
        }

        Command::Unreact { channel, ts, emoji } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            slack.reactions.remove(&id, &ts, &emoji).await?;

            if cli.json {
                println!("{{\"ok\": true}}");
            } else {
                println!("✓ Removed :{}: reaction", emoji.trim_matches(':'));
            }
        }

        Command::Reactions { channel, ts } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let reactions = slack.reactions.get(&id, &ts).await?;
            format::print_reactions(&reactions, cli.json);
        }

        Command::Emoji { query } => {
            let emoji = if let Some(q) = query {
                slack.emoji.search(&q).await?
            } else {
                slack.emoji.list().await?
            };
            format::print_emoji(&emoji, cli.json);
        }

        Command::Pin { channel, ts } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            slack.pins.add(&id, &ts).await?;

            if cli.json {
                println!("{{\"ok\": true}}");
            } else {
                println!("✓ Pinned message");
            }
        }

        Command::Unpin { channel, ts } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            slack.pins.remove(&id, &ts).await?;

            if cli.json {
                println!("{{\"ok\": true}}");
            } else {
                println!("✓ Unpinned message");
            }
        }

        Command::Pins { channel } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let pins = slack.pins.list(&id).await?;
            format::print_pins(&pins, cli.json);
        }

        Command::Bookmark {
            channel,
            title,
            url,
            emoji,
        } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let bookmark = slack
                .bookmarks
                .add(&id, &title, &url, emoji.as_deref())
                .await?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&bookmark)?);
            } else {
                println!("✓ Added bookmark: {} (id: {})", bookmark.title, bookmark.id);
            }
        }

        Command::Unbookmark {
            channel,
            bookmark_id,
        } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            slack.bookmarks.remove(&id, &bookmark_id).await?;

            if cli.json {
                println!("{{\"ok\": true}}");
            } else {
                println!("✓ Removed bookmark");
            }
        }

        Command::Bookmarks { channel } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let bookmarks = slack.bookmarks.list(&id).await?;
            format::print_bookmarks(&bookmarks, cli.json);
        }

        Command::Cache { action } => match action {
            CacheAction::Refresh { target } => {
                refresh_cache(&slack, &cache, target, cli.json).await?;
            }

            CacheAction::Stats => {
                let (users, channels) = cache.get_counts()?;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::json!({ "users": users, "channels": channels })
                    );
                } else {
                    println!("Users: {}, Channels: {}", users, channels);
                }
            }

            CacheAction::Path => {
                println!("{}", db_path.display());
            }
        },

        Command::Auth { .. } | Command::Config { .. } => unreachable!(),
    }

    if cache_status == CacheStatus::NeedsRefresh && !cli.json {
        eprintln!("Cache is stale. Run `slack-cli cache refresh` to update local lookup data.");
    }

    Ok(())
}

/// Slack Web API error codes that mean the token, not the request, is the
/// problem (Slack's documented common errors shared across methods). They
/// exit with code 3 so a caller knows to re-authenticate instead of retrying.
const AUTH_ERROR_CODES: &[&str] = &[
    "not_authed",
    "invalid_auth",
    "account_inactive",
    "token_revoked",
    "token_expired",
    "no_permission",
    "missing_scope",
    "not_allowed_token_type",
    "ekm_access_denied",
];

/// Machine identity of a failure: the `code` for the `--json` error envelope
/// and the process exit code. Exit codes encode the coarse classes a caller
/// branches on — 0 ok, 1 generic, 2 usage (clap), 3 auth, 4 rate limit — while
/// `code` keeps Slack's own error vocabulary for API failures.
fn classify_error(err: &anyhow::Error) -> (String, u8) {
    if let Some(api) = err.downcast_ref::<SlackApiError>() {
        return match api {
            SlackApiError::Api { code } if code == "ratelimited" => (code.clone(), 4),
            SlackApiError::Api { code } if AUTH_ERROR_CODES.contains(&code.as_str()) => {
                (code.clone(), 3)
            }
            SlackApiError::Api { code } => (code.clone(), 1),
            SlackApiError::RateLimitExhausted { .. } => ("rate_limited".to_string(), 4),
            SlackApiError::Http { .. } => ("http_error".to_string(), 1),
            SlackApiError::Transport { .. } => ("network_error".to_string(), 1),
        };
    }

    if err.downcast_ref::<AuthError>().is_some() {
        return ("auth_error".to_string(), 3);
    }

    ("error".to_string(), 1)
}

fn handle_config_action(
    action: &ConfigAction,
    as_json: bool,
    config_path: Option<std::path::PathBuf>,
    config: &config::Config,
) -> Result<()> {
    match action {
        ConfigAction::Show => config.show(as_json),
        ConfigAction::Path => {
            let path = config_path
                .or_else(config::Config::default_config_path)
                .context("Cannot determine config path")?;
            println!("{}", path.display());
            Ok(())
        }
        ConfigAction::Edit => config::Config::edit(config_path),
    }
}

fn merge_fields(defaults: &[String], expand: Option<&[String]>) -> Vec<String> {
    let mut fields = defaults.to_vec();
    if let Some(extra) = expand {
        for f in extra {
            if !fields.contains(f) {
                fields.push(f.clone());
            }
        }
    }
    fields
}

async fn resolve_channel(
    input: &str,
    slack: &slack::SlackClient,
    cache: &cache::SqliteCache,
    json: bool,
) -> Result<String> {
    if is_slack_conversation_id(input) {
        return Ok(input.to_string());
    }

    if is_slack_user_id(input) {
        if let Some(dm_id) = cache.find_dm_by_user(input)? {
            return Ok(dm_id);
        }
        anyhow::bail!(
            "No DM cached for user {}. Add \"im\" to `cache.channel_types` and run `slack-cli cache refresh`.",
            input
        );
    }

    let name = input.trim_start_matches('#').trim_start_matches('@');
    let mut channels = cache.search_channels(name, 2)?;

    if channels.is_empty() {
        ensure_channels_cache(slack, cache, json).await?;
        channels = cache.search_channels(name, 2)?;
    }

    let name_matches: Vec<&slack::SlackChannel> = channels
        .iter()
        .filter(|c| {
            c.name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
        .collect();

    if channels.len() > 1 && name_matches.is_empty() {
        let suggestions = channels
            .iter()
            .map(|c| match c.name.as_deref() {
                Some(name) => format!("#{name} ({})", c.id),
                None => c.id.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!(
            "Channel name is ambiguous: {}. Matches: {}",
            input,
            suggestions
        );
    }

    name_matches
        .first()
        .copied()
        .or_else(|| channels.first())
        .map(|c| c.id.clone())
        .context(format!("Channel not found: {}", input))
}

fn is_slack_conversation_id(input: &str) -> bool {
    is_slack_id_with_prefix(input, |c| matches!(c, 'C' | 'D' | 'G'))
}

fn is_slack_user_id(input: &str) -> bool {
    is_slack_id_with_prefix(input, |c| matches!(c, 'U' | 'W'))
}

fn is_slack_id_with_prefix(input: &str, allow: impl Fn(char) -> bool) -> bool {
    let mut chars = input.chars();
    match chars.next() {
        Some(first) if allow(first) => {}
        _ => return false,
    }
    chars.clone().count() >= 8 && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

async fn ensure_users_cache(
    slack: &slack::SlackClient,
    cache: &cache::SqliteCache,
    json: bool,
) -> Result<()> {
    let (users, _) = cache.get_counts()?;
    if users == 0 {
        if !json {
            eprint!("Fetching users... ");
        }
        let users = slack.users.list().await?;
        cache.save_users(users).await?;
        if !json {
            eprintln!("done");
        }
    }
    Ok(())
}

async fn ensure_channels_cache(
    slack: &slack::SlackClient,
    cache: &cache::SqliteCache,
    json: bool,
) -> Result<()> {
    let (_, channels) = cache.get_counts()?;
    if channels == 0 {
        if !json {
            eprint!("Fetching channels... ");
        }
        let channels = slack.channels.list().await?;
        cache.save_channels(channels).await?;
        if !json {
            eprintln!("done");
        }
    }
    Ok(())
}

fn parse_unix_seconds(input: &str) -> Result<i64> {
    if let Ok(secs) = input.parse::<f64>() {
        return Ok(secs as i64);
    }

    let date = NaiveDate::parse_from_str(input, "%Y-%m-%d").map_err(|_| {
        anyhow::anyhow!(
            "Invalid date format: {} (expected Unix timestamp or YYYY-MM-DD)",
            input
        )
    })?;
    let dt = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| anyhow::anyhow!("Invalid time"))?;
    let local = Local
        .from_local_datetime(&dt)
        .single()
        .ok_or_else(|| anyhow::anyhow!("Invalid timezone conversion"))?;
    Ok(local.timestamp())
}

fn parse_timestamp(input: &str) -> Result<String> {
    if input.parse::<f64>().is_ok() {
        return Ok(input.to_string());
    }
    parse_unix_seconds(input).map(|s| s.to_string())
}

fn build_payload(content: MessageContent) -> Result<MessagePayload> {
    let MessageContent {
        text,
        markdown_text,
        blocks,
        attachments,
        metadata,
    } = content;

    let stdin_sources = [
        ("blocks", blocks.as_deref()),
        ("attachments", attachments.as_deref()),
        ("metadata", metadata.as_deref()),
    ]
    .into_iter()
    .filter(|(_, src)| matches!(*src, Some("-")))
    .map(|(label, _)| label)
    .collect::<Vec<_>>();

    if stdin_sources.len() > 1 {
        anyhow::bail!(
            "only one flag may read from stdin per invocation; got: {}",
            stdin_sources.join(", ")
        );
    }

    let blocks = blocks.as_deref().map(parse_blocks_source).transpose()?;
    let attachments = attachments
        .as_deref()
        .map(parse_attachments_source)
        .transpose()?;
    let metadata = metadata.as_deref().map(parse_metadata_source).transpose()?;

    Ok(MessagePayload {
        text,
        markdown_text,
        blocks,
        attachments,
        metadata,
    })
}

fn parse_blocks_source(source: &str) -> Result<Vec<Value>> {
    match read_json_source("blocks", source)? {
        Value::Array(arr) => Ok(arr),
        _ => anyhow::bail!("--blocks must be a JSON array"),
    }
}

fn parse_attachments_source(source: &str) -> Result<Vec<Value>> {
    match read_json_source("attachments", source)? {
        Value::Array(arr) => Ok(arr),
        _ => anyhow::bail!("--attachments must be a JSON array"),
    }
}

fn parse_metadata_source(source: &str) -> Result<MessageMetadata> {
    let value = read_json_source("metadata", source)?;
    let obj = value.as_object().ok_or_else(|| {
        anyhow::anyhow!("--metadata must be a JSON object {{event_type, event_payload}}")
    })?;

    let event_type = obj
        .get("event_type")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("--metadata.event_type must be a non-empty string"))?
        .to_string();

    let event_payload = obj
        .get("event_payload")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("--metadata.event_payload is required"))?;
    if !event_payload.is_object() {
        anyhow::bail!("--metadata.event_payload must be a JSON object");
    }

    Ok(MessageMetadata {
        event_type,
        event_payload,
    })
}

fn read_json_source(label: &str, source: &str) -> Result<Value> {
    let body = if source == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .with_context(|| format!("--{label}: failed to read from stdin"))?;
        buf
    } else if let Some(path) = source.strip_prefix('@') {
        std::fs::read_to_string(path)
            .with_context(|| format!("--{label}: failed to read {path}"))?
    } else {
        source.to_string()
    };

    serde_json::from_str(&body).with_context(|| format!("--{label}: invalid JSON"))
}

async fn refresh_cache(
    slack: &slack::SlackClient,
    cache: &cache::SqliteCache,
    target: RefreshTarget,
    json: bool,
) -> Result<()> {
    match target {
        RefreshTarget::Users | RefreshTarget::All => {
            if !json {
                eprint!("Fetching users... ");
            }
            let users = slack.users.list().await?;
            cache.save_users(users).await?;
            if !json {
                eprintln!("✓");
            }
        }
        _ => {}
    }

    match target {
        RefreshTarget::Channels | RefreshTarget::All => {
            if !json {
                eprint!("Fetching channels... ");
            }
            let channels = slack.channels.list().await?;
            cache.save_channels(channels).await?;
            if !json {
                eprintln!("✓");
            }
        }
        _ => {}
    }

    if json {
        println!("{{\"status\": \"ok\"}}");
    } else {
        println!("✓ Cache refreshed");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classify_error_keeps_slack_code_for_generic_api_errors() {
        let err = anyhow::Error::from(SlackApiError::Api {
            code: "channel_not_found".to_string(),
        });
        assert_eq!(classify_error(&err), ("channel_not_found".to_string(), 1));
    }

    #[test]
    fn classify_error_maps_slack_auth_codes_to_exit_3() {
        let err = anyhow::Error::from(SlackApiError::Api {
            code: "invalid_auth".to_string(),
        });
        assert_eq!(classify_error(&err), ("invalid_auth".to_string(), 3));
    }

    #[test]
    fn classify_error_maps_rate_limits_to_exit_4() {
        let exhausted = anyhow::Error::from(SlackApiError::RateLimitExhausted {
            method: "conversations.history".to_string(),
            attempts: 3,
        });
        assert_eq!(classify_error(&exhausted), ("rate_limited".to_string(), 4));

        let in_body = anyhow::Error::from(SlackApiError::Api {
            code: "ratelimited".to_string(),
        });
        assert_eq!(classify_error(&in_body), ("ratelimited".to_string(), 4));
    }

    #[test]
    fn classify_error_survives_context_wrapping() {
        let err = anyhow::Error::from(SlackApiError::Api {
            code: "invalid_auth".to_string(),
        })
        .context("sending message");
        assert_eq!(classify_error(&err).1, 3);
    }

    #[test]
    fn classify_error_maps_auth_errors_to_exit_3() {
        let err = anyhow::Error::from(AuthError::NotConfigured);
        assert_eq!(classify_error(&err), ("auth_error".to_string(), 3));
    }

    #[test]
    fn classify_error_defaults_to_generic() {
        let err = anyhow::anyhow!("boom");
        assert_eq!(classify_error(&err), ("error".to_string(), 1));
    }

    #[test]
    fn build_payload_rejects_two_stdin_sources() {
        let err = build_payload(MessageContent {
            text: None,
            markdown_text: None,
            blocks: Some("-".into()),
            attachments: Some("-".into()),
            metadata: None,
        })
        .unwrap_err();
        assert!(err.to_string().contains("only one flag"));
    }

    #[test]
    fn parse_blocks_inline_array_succeeds() {
        let blocks = parse_blocks_source(r#"[{"type":"section"}]"#).unwrap();
        assert_eq!(blocks[0]["type"], json!("section"));
    }

    #[test]
    fn parse_blocks_rejects_object_root() {
        let err = parse_blocks_source(r#"{"type":"section"}"#).unwrap_err();
        assert!(err.to_string().contains("must be a JSON array"));
    }

    #[test]
    fn parse_blocks_rejects_invalid_json() {
        let err = parse_blocks_source("not json").unwrap_err();
        assert!(err.to_string().contains("invalid JSON"));
    }

    #[test]
    fn parse_metadata_inline_object_succeeds() {
        let metadata = parse_metadata_source(
            r#"{"event_type":"deploy_done","event_payload":{"version":"1.2.3"}}"#,
        )
        .unwrap();
        assert_eq!(metadata.event_type, "deploy_done");
        assert_eq!(metadata.event_payload["version"], json!("1.2.3"));
    }

    #[test]
    fn parse_metadata_rejects_array_root() {
        let err = parse_metadata_source("[]").unwrap_err();
        assert!(err.to_string().contains("must be a JSON object"));
    }

    #[test]
    fn parse_metadata_rejects_missing_event_type() {
        let err = parse_metadata_source(r#"{"event_payload":{}}"#).unwrap_err();
        assert!(err.to_string().contains("event_type"));
    }

    #[test]
    fn parse_metadata_rejects_missing_event_payload() {
        let err = parse_metadata_source(r#"{"event_type":"x"}"#).unwrap_err();
        assert!(err.to_string().contains("event_payload"));
    }

    #[test]
    fn parse_metadata_rejects_non_object_event_payload() {
        let err =
            parse_metadata_source(r#"{"event_type":"x","event_payload":"oops"}"#).unwrap_err();
        assert!(err.to_string().contains("event_payload"));
    }

    #[test]
    fn read_json_source_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blocks.json");
        std::fs::write(&path, r#"[{"type":"section"}]"#).unwrap();
        let arg = format!("@{}", path.display());
        let value = read_json_source("blocks", &arg).unwrap();
        assert!(value.is_array());
    }

    #[test]
    fn read_json_source_missing_file_errors() {
        let err = read_json_source("blocks", "@/definitely/missing/path.json").unwrap_err();
        assert!(err.to_string().contains("failed to read"));
    }
}
