use anyhow::{Context, Result};
use chrono::{Local, NaiveDate, TimeZone};
use clap::Parser;
use slack_cli::{
    auth::{self, AuthLoadOptions, Authenticator, EnvOverrides},
    cache::{self, CacheStatus},
    cli::{CacheAction, Cli, Command, ConfigAction, RefreshTarget},
    config, format, slack,
};
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
            text,
            thread,
        } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let result = slack.messages.send(&id, &text, thread.as_deref()).await?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("✓ Sent: {}", result.ts);
            }
        }

        Command::Update { channel, ts, text } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let result = slack.messages.update(&id, &ts, &text).await?;

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

            let expand_fields = expand.unwrap_or_default();
            format::print_messages(&messages, cli.json, &expand_fields, Some(&cache));
        }

        Command::Thread { channel, ts, limit } => {
            let id = resolve_channel(&channel, &slack, &cache, cli.json).await?;
            let messages = slack.messages.replies(&id, &ts, limit).await?;
            format::print_messages(&messages, cli.json, &[], None);
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
