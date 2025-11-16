mod cache;
mod cli;
mod config;
mod format;
mod slack;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{CacheAction, Cli, Command, ConfigAction, RefreshTarget};
use std::io::{self, Write};
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

    check_cache_status(&cache, &config)?;

    match cli.command {
        Command::Users { query, limit } => {
            let users = cache.search_users(&query, limit, false)?;
            format::print_users(&users, cli.json);
        }

        Command::Channels { query, limit } => {
            let channels = cache.search_channels(&query, limit)?;
            format::print_channels(&channels, cli.json);
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

        Command::Messages {
            channel,
            limit,
            cursor,
        } => {
            let id = resolve_channel(&channel, &cache)?;
            let (messages, _) = slack
                .messages
                .get_channel_messages(&id, limit, cursor.as_deref())
                .await?;

            format::print_messages(&messages, cli.json);
        }

        Command::Thread { channel, ts, limit } => {
            let messages = slack
                .messages
                .get_thread_messages(&channel, &ts, limit)
                .await?;
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

fn check_cache_status(cache: &cache::SqliteCache, config: &config::Config) -> Result<()> {
    if cache.is_cache_empty()? {
        eprintln!("⚠ Cache is empty. Run: slack-cli cache refresh");
        return Ok(());
    }

    let ttl = config.cache.ttl_users_hours;
    if cache.is_cache_stale(ttl)? {
        eprintln!("⚠ Cache is outdated. Consider running: slack-cli cache refresh");
    }

    Ok(())
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
