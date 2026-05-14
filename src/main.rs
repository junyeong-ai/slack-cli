use anyhow::{Context, Result};
use chrono::{Local, NaiveDate, TimeZone};
use clap::Parser;
use slack_cli::{
    cache::{self, CacheStatus},
    cli::{CacheAction, Cli, Command, ConfigAction, RefreshTarget},
    config, format, slack,
};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let level = if cli.verbose { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(level)
        .with_writer(std::io::stderr)
        .compact()
        .with_target(false)
        .init();

    dotenvy::dotenv().ok();

    if let Command::Config { action } = &cli.command {
        return handle_config_action(
            action,
            cli.json,
            cli.config.clone(),
            cli.token.clone(),
            cli.user_token.clone(),
            cli.data_dir.clone(),
        );
    }

    let config = config::Config::load(cli.config, cli.token, cli.user_token, cli.data_dir)?;

    let db_path = config.db_path();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let db_path_str = db_path
        .to_str()
        .context("Database path contains invalid UTF-8 characters")?;
    let cache = Arc::new(cache::SqliteCache::new(db_path_str).await?);
    let slack = Arc::new(slack::SlackClient::new(config.clone())?);

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
            text,
            thread,
        } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let result = slack
                .messages
                .send_message(&id, &text, thread.as_deref())
                .await?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("✓ Sent: {}", result.ts);
            }
        }

        Command::Update { channel, ts, text } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let result = slack.messages.update_message(&id, &ts, &text).await?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("✓ Updated: {}", result.ts);
            }
        }

        Command::Delete { channel, ts } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let result = slack.messages.delete_message(&id, &ts).await?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("✓ Deleted: {}", result.ts);
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

            let (mut messages, _) = slack
                .messages
                .get_channel_messages(
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

            let expand_fields = expand.unwrap_or_default();
            format::print_messages(&messages, cli.json, &expand_fields, Some(&cache));
        }

        Command::Thread { channel, ts, limit } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let messages = slack.messages.get_thread_messages(&id, &ts, limit).await?;
            format::print_messages(&messages, cli.json, &[], None);
        }

        Command::Members { channel } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let members = slack.channels.list_members(&id).await?;
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
            if cli.verbose {
                match slack.search.capabilities().await {
                    Ok(capabilities) => {
                        tracing::debug!(
                            ai_search_enabled = capabilities.is_ai_search_enabled,
                            "Slack search capabilities"
                        );
                    }
                    Err(error) => {
                        tracing::debug!(?error, "Unable to read Slack search capabilities");
                    }
                }
            }

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
            let results = slack.search.search(&query, &options).await?;

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

        Command::Config { .. } => unreachable!(),
    }

    if cache_status == CacheStatus::NeedsRefresh && !cli.json {
        eprintln!("Cache is stale. Run `slack-cli cache refresh` to update local lookup data.");
    }

    Ok(())
}

fn handle_config_action(
    action: &ConfigAction,
    as_json: bool,
    config_path: Option<PathBuf>,
    cli_token: Option<String>,
    cli_user_token: Option<String>,
    cli_data_dir: Option<PathBuf>,
) -> Result<()> {
    match action {
        ConfigAction::Init {
            bot_token,
            user_token,
            force,
        } => init_config(config_path, bot_token.clone(), user_token.clone(), *force),

        ConfigAction::Show => {
            let config =
                config::Config::load(config_path, cli_token, cli_user_token, cli_data_dir)?;
            config.show_masked(as_json)
        }

        ConfigAction::Path => {
            let path = config_path
                .or_else(config::Config::default_config_path)
                .context("Cannot determine config path")?;
            println!("{}", path.display());
            Ok(())
        }

        ConfigAction::Edit => config::Config::edit_config(config_path),
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

    let name = input.trim_start_matches('#').trim_start_matches('@');
    let mut channels = cache.search_channels(name, 2)?;

    if channels.is_empty() {
        ensure_channels_cache(slack, cache, json).await?;
        channels = cache.search_channels(name, 2)?;
    }

    if channels.len() > 1 && !channels.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
        let matches = channels
            .iter()
            .map(|c| format!("#{} ({})", c.name, c.id))
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!("Channel name is ambiguous: {}. Matches: {}", input, matches);
    }

    channels
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(name))
        .or_else(|| channels.first())
        .map(|c| c.id.clone())
        .context(format!("Channel not found: {}", input))
}

fn is_slack_conversation_id(input: &str) -> bool {
    let mut chars = input.chars();
    matches!(chars.next(), Some('C' | 'D' | 'G'))
        && chars.clone().count() >= 8
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
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
        let users = slack.users.fetch_all_users().await?;
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
        let channels = slack.channels.fetch_all_channels().await?;
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

fn init_config(
    config_path: Option<PathBuf>,
    bot_token: Option<String>,
    user_token: Option<String>,
    force: bool,
) -> Result<()> {
    let path = config_path
        .or_else(config::Config::default_config_path)
        .context("Cannot determine config path")?;

    if path.exists() && !force {
        anyhow::bail!(
            "Config exists: {}\nUse --force to overwrite",
            path.display()
        );
    }

    let bot_token = bot_token.and_then(trim_token);
    let user_token = user_token.and_then(trim_token);

    let (bot_token, user_token) = match (bot_token, user_token) {
        (bot_token, user_token) if bot_token.is_some() || user_token.is_some() => {
            (bot_token, user_token)
        }
        _ if io::stdin().is_terminal() => {
            let user_token = prompt_optional("User token (xoxp-..., recommended): ")?;
            let bot_token = prompt_optional("Bot token (xoxb-..., optional): ")?;
            (bot_token, user_token)
        }
        _ => anyhow::bail!(
            "No Slack token provided. Use --user-token xoxp-... or --bot-token xoxb-..."
        ),
    };

    if bot_token.is_none() && user_token.is_none() {
        anyhow::bail!("At least one Slack token is required");
    }

    let config = config::Config {
        bot_token,
        user_token,
        ..Default::default()
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(&config)?;
    std::fs::write(&path, content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&path, perms)?;
    }

    println!("✓ Config saved: {}", path.display());
    println!("\nRun: slack-cli cache refresh");

    Ok(())
}

fn prompt_optional(prompt: &str) -> Result<Option<String>> {
    print!("{}", prompt);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let value = input.trim();
    Ok((!value.is_empty()).then(|| value.to_string()))
}

fn trim_token(token: String) -> Option<String> {
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
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
            let users = slack.users.fetch_all_users().await?;
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
            let channels = slack.channels.fetch_all_channels().await?;
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
