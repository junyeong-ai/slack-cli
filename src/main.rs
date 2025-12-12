mod cache;
mod cli;
mod config;
mod format;
mod slack;

use anyhow::{Context, Result};
use cache::CacheStatus;
use chrono::{Local, NaiveDate, TimeZone};
use clap::Parser;
use cli::{CacheAction, Cli, Command, ConfigAction, RefreshTarget};
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;

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

    if let Command::Config { action } = &cli.command {
        return handle_config_action(action, cli.json);
    }

    dotenvy::dotenv().ok();

    let config = config::Config::load(cli.config, cli.token, cli.user_token, cli.data_dir)?;

    let db_path = config.db_path();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let db_path_str = db_path
        .to_str()
        .context("Database path contains invalid UTF-8 characters")?;
    let cache = Arc::new(cache::SqliteCache::new(db_path_str).await?);
    let slack = Arc::new(slack::SlackClient::new(config.clone()));

    let ttl = config.cache.ttl_users_hours;
    let threshold = config.cache.refresh_threshold_percent;
    let cache_status = cache.get_cache_status(ttl, threshold)?;

    if cache_status == CacheStatus::Empty {
        eprintln!("⚠ Cache is empty. Refreshing...");
        refresh_cache(&slack, &cache, RefreshTarget::All, cli.json).await?;
    }

    match cli.command {
        Command::Users {
            query,
            id,
            limit,
            expand,
        } => {
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
            let id = resolve_channel(&channel, &cache)?;
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
            let id = resolve_channel(&channel, &cache)?;
            let result = slack.messages.update_message(&id, &ts, &text).await?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("✓ Updated: {}", result.ts);
            }
        }

        Command::Delete { channel, ts } => {
            let id = resolve_channel(&channel, &cache)?;
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
        } => {
            let id = resolve_channel(&channel, &cache)?;

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

            format::print_messages(&messages, cli.json);
        }

        Command::Thread { channel, ts, limit } => {
            let id = resolve_channel(&channel, &cache)?;
            let messages = slack.messages.get_thread_messages(&id, &ts, limit).await?;
            format::print_messages(&messages, cli.json);
        }

        Command::Members { channel } => {
            let id = resolve_channel(&channel, &cache)?;
            let members = slack.messages.list_channel_members(&id).await?;
            format::print_members(&members, &cache, cli.json);
        }

        Command::Search {
            query,
            channel,
            user,
            limit,
        } => {
            let messages = slack
                .messages
                .search_messages(&query, channel.as_deref(), user.as_deref(), limit)
                .await?;

            format::print_messages(&messages, cli.json);
        }

        Command::React { channel, ts, emoji } => {
            let id = resolve_channel(&channel, &cache)?;
            slack.reactions.add(&id, &ts, &emoji).await?;

            if cli.json {
                println!("{{\"ok\": true}}");
            } else {
                println!("✓ Added :{}: reaction", emoji.trim_matches(':'));
            }
        }

        Command::Unreact { channel, ts, emoji } => {
            let id = resolve_channel(&channel, &cache)?;
            slack.reactions.remove(&id, &ts, &emoji).await?;

            if cli.json {
                println!("{{\"ok\": true}}");
            } else {
                println!("✓ Removed :{}: reaction", emoji.trim_matches(':'));
            }
        }

        Command::Reactions { channel, ts } => {
            let id = resolve_channel(&channel, &cache)?;
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
            let id = resolve_channel(&channel, &cache)?;
            slack.pins.add(&id, &ts).await?;

            if cli.json {
                println!("{{\"ok\": true}}");
            } else {
                println!("✓ Pinned message");
            }
        }

        Command::Unpin { channel, ts } => {
            let id = resolve_channel(&channel, &cache)?;
            slack.pins.remove(&id, &ts).await?;

            if cli.json {
                println!("{{\"ok\": true}}");
            } else {
                println!("✓ Unpinned message");
            }
        }

        Command::Pins { channel } => {
            let id = resolve_channel(&channel, &cache)?;
            let pins = slack.pins.list(&id).await?;
            format::print_pins(&pins, cli.json);
        }

        Command::Bookmark {
            channel,
            title,
            url,
            emoji,
        } => {
            let id = resolve_channel(&channel, &cache)?;
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
            let id = resolve_channel(&channel, &cache)?;
            slack.bookmarks.remove(&id, &bookmark_id).await?;

            if cli.json {
                println!("{{\"ok\": true}}");
            } else {
                println!("✓ Removed bookmark");
            }
        }

        Command::Bookmarks { channel } => {
            let id = resolve_channel(&channel, &cache)?;
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

    let should_refresh = cache_status == CacheStatus::NeedsRefresh
        && !cache.is_within_refresh_cooldown().unwrap_or(true);

    if should_refresh {
        let cache_clone = cache.clone();
        let slack_clone = slack.clone();

        tokio::spawn(async move {
            cache_clone.try_background_refresh(&slack_clone).await;
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}

fn handle_config_action(action: &ConfigAction, as_json: bool) -> Result<()> {
    match action {
        ConfigAction::Init {
            bot_token,
            user_token,
            force,
        } => init_config(bot_token.clone(), user_token.clone(), *force),

        ConfigAction::Show => {
            let config = config::Config::load(None, None, None, None)?;
            config.show_masked(as_json)
        }

        ConfigAction::Path => {
            let path =
                config::Config::default_config_path().context("Cannot determine config path")?;
            println!("{}", path.display());
            Ok(())
        }

        ConfigAction::Edit => config::Config::edit_config(),
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

fn resolve_channel(input: &str, cache: &cache::SqliteCache) -> Result<String> {
    if input.starts_with(['C', 'D', 'G']) {
        return Ok(input.to_string());
    }

    let name = input.trim_start_matches('#').trim_start_matches('@');
    let channels = cache.search_channels(name, 1)?;

    channels
        .first()
        .map(|c| c.id.clone())
        .context(format!("Channel not found: {}", input))
}

fn parse_timestamp(input: &str) -> Result<String> {
    // Unix timestamp: use directly
    if input.parse::<f64>().is_ok() {
        return Ok(input.to_string());
    }

    // ISO date (YYYY-MM-DD)
    if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        let ts = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow::anyhow!("Invalid time"))?;
        let local_ts = Local
            .from_local_datetime(&ts)
            .single()
            .ok_or_else(|| anyhow::anyhow!("Invalid timezone conversion"))?;
        return Ok(local_ts.timestamp().to_string());
    }

    anyhow::bail!(
        "Invalid date format: {} (expected Unix timestamp or YYYY-MM-DD)",
        input
    )
}

fn init_config(bot_token: Option<String>, user_token: Option<String>, force: bool) -> Result<()> {
    let path = config::Config::default_config_path().context("Cannot determine config path")?;

    if path.exists() && !force {
        anyhow::bail!(
            "Config exists: {}\nUse --force to overwrite",
            path.display()
        );
    }

    let bot_token = match bot_token {
        Some(t) => t,
        None => {
            print!("Bot token (xoxb-...): ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            input.trim().to_string()
        }
    };

    let user_token = match user_token {
        Some(t) => Some(t),
        None => {
            print!("User token (optional, Enter to skip): ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let trimmed = input.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
    };

    let config = config::Config {
        bot_token: Some(bot_token),
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
