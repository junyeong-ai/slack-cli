use anyhow::{Context, Result};
use chrono::{Local, NaiveDate, TimeZone};
use clap::Parser;
use serde_json::Value;
use slack_cli::{
    auth::{self, AuthError, AuthLoadOptions, Authenticator, EnvOverrides},
    cache::{self, CacheStatus},
    cli::{
        CacheAction, Cli, Command, ConfigAction, DaemonAction, EventsAction, MessageContent,
        RefreshTarget, SelfAction,
    },
    config, events, format,
    paths::AppPaths,
    slack,
    slack::{MessageMetadata, MessagePayload, SlackApiError},
    update,
};
use std::io::Read;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

#[tokio::main]
async fn main() -> ExitCode {
    // Before anything reads the environment: clap binds `--profile` and
    // `--client-id` to env vars at parse time, and the log filter below reads
    // RUST_LOG.
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    let default_level = if cli.verbose {
        LevelFilter::DEBUG
    } else {
        LevelFilter::WARN
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(default_level.into())
                .from_env_lossy(),
        )
        .with_writer(std::io::stderr)
        .compact()
        .with_target(false)
        .init();

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
    let paths = AppPaths::resolve()?;
    // `config path` and `config edit` are how a rejected config gets repaired,
    // so neither may depend on it loading.
    if let Command::Config { action } = &cli.command
        && let Some(outcome) = handle_unloadable_config(action, cli.config.clone(), &paths)
    {
        return outcome;
    }

    let config = config::Config::load(&paths, cli.config.clone(), cli.data_dir.clone())?;

    if let Command::Config { action } = &cli.command {
        return handle_config_action(action, cli.json, &paths, &config);
    }

    // Neither an auth store nor a cache is involved in replacing the binary.
    if let Command::SelfCmd { action } = &cli.command {
        return handle_self_action(action, cli.json).await;
    }

    let authenticator = Arc::new(
        Authenticator::load(AuthLoadOptions {
            store_path: paths.auth_store(),
            api_base_url: config.connection.api_base_url.clone(),
            overrides: EnvOverrides::capture(),
            explicit_profile: cli.profile.clone(),
        })
        .await?,
    );

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

    let slack = Arc::new(slack::SlackClient::new(
        config.clone(),
        authenticator.clone(),
    )?);

    // Before the cache: nothing here resolves a name, and a daemon that runs
    // for days has no business holding a connection pool it never reads.
    if matches!(
        cli.command,
        Command::Watch | Command::Daemon { .. } | Command::Events { .. }
    ) {
        let profile = events_profile(&authenticator, cli.profile.as_deref()).await;
        return handle_events_command(cli, config, &paths, &profile, slack).await;
    }

    let db_path = config.db_path(&paths);
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
            let mut messages = slack.messages.replies(&id, &ts, limit, None).await?;
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
            capabilities,
            limit,
            channel_types,
            content_types,
            channel,
            before,
            after,
            include_context_messages,
            include_bots,
            include_deleted_users,
            modifiers,
            include_archived_channels,
            disable_semantic_search,
            sort,
            sort_dir,
        } => {
            if capabilities {
                let capabilities = slack.search.info().await?;
                format::print_search_capabilities(&capabilities, cli.json);
                return Ok(());
            }

            let query = query.context("a search query is required")?;
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
                include_deleted_users,
                modifiers,
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

        Command::Auth { .. }
        | Command::Config { .. }
        | Command::SelfCmd { .. }
        | Command::Watch
        | Command::Daemon { .. }
        | Command::Events { .. } => {
            unreachable!("handled before the cache is opened")
        }
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
            SlackApiError::Api { code, .. } if code == "ratelimited" => (code.clone(), 4),
            SlackApiError::Api { code, .. } if AUTH_ERROR_CODES.contains(&code.as_str()) => {
                (code.clone(), 3)
            }
            SlackApiError::Api { code, .. } => (code.clone(), 1),
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

/// A heartbeat older than this means the daemon is gone rather than quiet: it
/// publishes on a 30-second cycle, so three missed cycles is not a hiccup.
const DAEMON_STALE_AFTER_SECONDS: i64 = 90;

/// How long `events pull --follow` waits between polls. The daemon writes as
/// events arrive; this only decides how quickly a follower notices.
const FOLLOW_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Which profile's events these are.
///
/// `--profile` has to be honoured here and not only by the authenticator:
/// events are stored per profile, so resolving the active one instead would
/// read another workspace's log while holding this workspace's tokens.
async fn events_profile(authenticator: &Authenticator, explicit: Option<&str>) -> String {
    // Environment tokens bypass the store entirely, so the active profile
    // names an installation this run is not talking to. Filing the events
    // under it would write one workspace's messages into another's databases.
    if authenticator.uses_env_tokens() {
        return "env".to_string();
    }

    authenticator
        .snapshot()
        .await
        .resolve(explicit)
        .unwrap_or("default")
        .to_string()
}

async fn handle_events_command(
    cli: Cli,
    config: config::Config,
    paths: &AppPaths,
    profile: &str,
    slack: Arc<slack::SlackClient>,
) -> Result<()> {
    let dir = config.events_dir(paths);
    let as_json = cli.json;

    match cli.command {
        // `watch` is the streaming mode made explicit: whatever the config
        // says about retention, this run keeps nothing.
        Command::Watch => {
            let runtime = events::EventRuntime::open_with(
                &dir,
                profile,
                &config,
                config::EventRetention::Stream,
            )?;
            events::run(
                slack,
                config,
                runtime,
                events::DaemonOptions {
                    stdout: if as_json {
                        events::StdoutFormat::Ndjson
                    } else {
                        events::StdoutFormat::Line
                    },
                    // `watch` keeps nothing and shows everything here: it
                    // overrides both the retention mode and the sinks, so an
                    // installation configured for a daemon still gets what
                    // this command promises.
                    stdout_only: true,
                    announce: !as_json,
                },
            )
            .await
        }

        Command::Daemon { action } => match action {
            DaemonAction::Run => {
                let runtime = events::EventRuntime::open(&dir, profile, &config)?;
                events::run(
                    slack,
                    config,
                    runtime,
                    events::DaemonOptions {
                        stdout: if as_json {
                            events::StdoutFormat::Ndjson
                        } else {
                            events::StdoutFormat::Line
                        },
                        stdout_only: false,
                        announce: !as_json,
                    },
                )
                .await
            }

            DaemonAction::Status => {
                let runtime = events::EventRuntime::open(&dir, profile, &config)?;
                let status = runtime.state.daemon_status()?;
                // Same question `stop` asks, answered the same way: taking the
                // lock means nobody holds it, so no daemon is running. The
                // heartbeat cannot answer it — see `stop_daemon`.
                let running = events::DaemonLock::acquire(&runtime.paths.lock_file()).is_err();
                format::print_daemon_status(
                    status.as_ref(),
                    profile,
                    running,
                    DAEMON_STALE_AFTER_SECONDS,
                    as_json,
                );
                Ok(())
            }

            DaemonAction::Stop => {
                let runtime = events::EventRuntime::open(&dir, profile, &config)?;
                stop_daemon(&runtime, as_json)
            }
        },

        Command::Events { action } => {
            let runtime = events::EventRuntime::open(&dir, profile, &config)?;
            handle_events_action(action, &config, &runtime, profile, as_json).await
        }

        _ => unreachable!("dispatched on the events commands only"),
    }
}

async fn handle_events_action(
    action: EventsAction,
    config: &config::Config,
    runtime: &events::EventRuntime,
    profile: &str,
    as_json: bool,
) -> Result<()> {
    match action {
        EventsAction::Pull {
            consumer,
            limit,
            ack,
            follow,
        } => {
            // Where to read from when nothing is being acknowledged. Without
            // it a follower would re-print the same batch for as long as it
            // kept asking — and, once the backlog reached `limit`, would do so
            // without ever pausing.
            let mut emitted: Option<i64> = None;

            loop {
                let batch = runtime.store.pull(&consumer, emitted, usize::from(limit))?;
                if batch.is_empty() && !follow {
                    // Silence would be ambiguous: nothing pending reads the
                    // same as a command that did not run.
                    format::print_events(&batch, as_json);
                    return Ok(());
                }
                if let Some(last) = batch.last() {
                    format::print_events(&batch, as_json);
                    if ack {
                        runtime.store.ack(&consumer, last.seq)?;
                    } else {
                        emitted = Some(last.seq);
                    }
                }

                if !follow {
                    return Ok(());
                }
                // Only wait when the log is drained: a backlog is walked at
                // full speed, and the poll is what idles.
                if batch.len() < usize::from(limit) {
                    tokio::time::sleep(FOLLOW_POLL_INTERVAL).await;
                }
            }
        }

        EventsAction::Ack { consumer, through } => {
            let pending = runtime.store.ack(&consumer, through)?;
            if as_json {
                println!(
                    "{}",
                    serde_json::json!({ "consumer": consumer, "acked_seq": through, "pending": pending })
                );
            } else {
                println!("✓ {consumer} acknowledged through {through} ({pending} pending)");
            }
            Ok(())
        }

        EventsAction::Stats => {
            let stats = runtime.store.stats()?;
            format::print_event_stats(
                &stats,
                &runtime.store.caps(),
                config.events.mode.as_str(),
                profile,
                as_json,
            );
            Ok(())
        }

        EventsAction::Prune => {
            let outcome = runtime.store.prune()?;
            format::print_prune_outcome(&outcome, as_json);
            Ok(())
        }

        EventsAction::Path => {
            // stdout is parseable data or nothing, which under `--json` means
            // an object rather than a bare path: an agent piping this to `jq`
            // gets a parse error otherwise.
            if as_json {
                println!(
                    "{}",
                    serde_json::json!({ "path": runtime.paths.root().display().to_string() })
                );
            } else {
                println!("{}", runtime.paths.root().display());
            }
            Ok(())
        }
    }
}

/// Signals the running daemon to stop.
///
/// It is a signal rather than a flag in the database because the daemon spends
/// its life awaiting a socket: a flag would only be noticed on the next event,
/// which in a quiet workspace could be hours away.
///
/// Liveness comes from the lock, not from the heartbeat. A daemon that was
/// killed a moment ago leaves a fresh heartbeat behind, and signalling the pid
/// it recorded would send SIGTERM to whatever the operating system has since
/// given that number to.
fn stop_daemon(runtime: &events::EventRuntime, as_json: bool) -> Result<()> {
    let Some(status) = runtime.state.daemon_status()? else {
        anyhow::bail!("no daemon has run for this profile");
    };

    if events::DaemonLock::acquire(&runtime.paths.lock_file()).is_ok() {
        anyhow::bail!(
            "no daemon is running for this profile; the record it left behind says pid {} \
             started at {}",
            status.pid,
            format::format_epoch(status.started_at)
        );
    }

    signal_stop(status.pid)?;

    if as_json {
        println!(
            "{}",
            serde_json::json!({ "stopped": true, "pid": status.pid })
        );
    } else {
        println!("✓ Asked the daemon (pid {}) to stop", status.pid);
    }
    Ok(())
}

/// Sends SIGTERM, after confirming the pid still belongs to this program.
///
/// The lock says *a* daemon is running; this says the recorded pid is the one.
/// Between them, a recycled pid cannot be signalled by mistake.
#[cfg(unix)]
fn signal_stop(pid: i64) -> Result<()> {
    let running = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .context("could not ask ps what that process is")?;
    let command = String::from_utf8_lossy(&running.stdout);
    if !command.contains("slack-cli") {
        anyhow::bail!(
            "pid {pid} is not a slack-cli process ({}), so it was not signalled. The daemon \
             record is stale; stop the daemon through whatever started it",
            command.trim()
        );
    }

    let status = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .context("could not run kill")?;
    if !status.success() {
        anyhow::bail!("could not signal pid {pid}");
    }
    Ok(())
}

#[cfg(not(unix))]
fn signal_stop(pid: i64) -> Result<()> {
    anyhow::bail!(
        "stopping a daemon by signal is not supported on this platform; stop pid {pid} \
         through the service manager that started it"
    )
}

async fn handle_self_action(action: &SelfAction, as_json: bool) -> Result<()> {
    let SelfAction::Update {
        version,
        check,
        force,
        yes,
    } = action;

    let outcome = update::run(update::UpdateRequest {
        version: version.clone(),
        check: *check,
        force: *force,
        assume_yes: *yes,
        api_base: None,
        binary: None,
        cosign: None,
    })
    .await?;

    format::print_update_outcome(&outcome, as_json);
    Ok(())
}

/// The config actions that must run while the file itself is unusable: `path`
/// says where it is, `edit` opens it. `None` for the action that needs the
/// config to have loaded.
fn handle_unloadable_config(
    action: &ConfigAction,
    config_path: Option<std::path::PathBuf>,
    paths: &AppPaths,
) -> Option<Result<()>> {
    match action {
        ConfigAction::Path => {
            let path = config_path.unwrap_or_else(|| paths.config_file());
            println!("{}", path.display());
            Some(Ok(()))
        }
        ConfigAction::Edit => Some(config::Config::edit(paths, config_path)),
        ConfigAction::Show => None,
    }
}

fn handle_config_action(
    action: &ConfigAction,
    as_json: bool,
    paths: &AppPaths,
    config: &config::Config,
) -> Result<()> {
    match action {
        ConfigAction::Show => config.show(paths, as_json),
        ConfigAction::Path | ConfigAction::Edit => {
            unreachable!("handled before the config is loaded")
        }
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
            method: "conversations.history".into(),
            required: Vec::new(),
        });
        assert_eq!(classify_error(&err), ("channel_not_found".to_string(), 1));
    }

    #[test]
    fn classify_error_maps_slack_auth_codes_to_exit_3() {
        let err = anyhow::Error::from(SlackApiError::Api {
            code: "invalid_auth".to_string(),
            method: "conversations.history".into(),
            required: Vec::new(),
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
            method: "conversations.history".into(),
            required: Vec::new(),
        });
        assert_eq!(classify_error(&in_body), ("ratelimited".to_string(), 4));
    }

    #[test]
    fn classify_error_survives_context_wrapping() {
        let err = anyhow::Error::from(SlackApiError::Api {
            code: "invalid_auth".to_string(),
            method: "conversations.history".into(),
            required: Vec::new(),
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
