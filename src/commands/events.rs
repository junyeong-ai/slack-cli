use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use slack_cli::{
    cli::{Cli, Command, DaemonAction, EventsAction},
    config, events, format, outln,
    paths::AppPaths,
    slack,
};

/// A heartbeat older than this means the daemon is gone rather than quiet: it
/// publishes on a 30-second cycle, so three missed cycles is not a hiccup.
const DAEMON_STALE_AFTER_SECONDS: i64 = 90;

/// How long `events pull --follow` waits between polls. The daemon writes as
/// events arrive; this only decides how quickly a follower notices.
const FOLLOW_POLL_INTERVAL: Duration = Duration::from_secs(1);

pub(crate) async fn handle(
    cli: Cli,
    config: config::Config,
    paths: &AppPaths,
    owner: events::owner::EventOwner,
    slack: Arc<slack::SlackClient>,
) -> Result<()> {
    let dir = config.events_dir(paths);
    let as_json = cli.json;
    let namespace = owner.namespace()?;
    let profile = owner.profile.as_str();

    match cli.command {
        // `watch` is the streaming mode made explicit: whatever the config
        // says about retention, this run keeps nothing.
        Command::Watch => {
            let runtime = events::EventRuntime::open_async(
                &dir,
                &namespace,
                &config,
                config::EventRetention::Stream,
            )
            .await?;
            events::run(
                slack,
                config,
                runtime.with_owner(owner.clone()),
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
                let runtime =
                    events::EventRuntime::open_async(&dir, &namespace, &config, config.events.mode)
                        .await?;
                events::run(
                    slack,
                    config,
                    runtime.with_owner(owner.clone()),
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
                let runtime =
                    events::EventRuntime::open_async(&dir, &namespace, &config, config.events.mode)
                        .await?;
                let status = runtime
                    .execute(|runtime| runtime.state.daemon_status())
                    .await?;
                // Same question `stop` asks, answered the same way: taking the
                // lock means nobody holds it, so no daemon is running. The
                // heartbeat cannot answer it — see `stop_daemon`.
                let running =
                    events::DaemonLock::try_acquire(&runtime.paths.lock_file())?.is_none();
                format::print_daemon_status(
                    status.as_ref(),
                    profile,
                    running,
                    DAEMON_STALE_AFTER_SECONDS,
                    as_json,
                )?;
                Ok(())
            }

            DaemonAction::Stop => {
                let runtime =
                    events::EventRuntime::open_async(&dir, &namespace, &config, config.events.mode)
                        .await?;
                runtime
                    .execute(move |runtime| stop_daemon(runtime, as_json))
                    .await
            }
        },

        Command::Events { action } => {
            let runtime =
                events::EventRuntime::open_async(&dir, &namespace, &config, config.events.mode)
                    .await?;
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
                let name = consumer.clone();
                let batch = runtime
                    .execute(move |runtime| runtime.store.pull(&name, emitted, usize::from(limit)))
                    .await?;
                if batch.is_empty() && !follow {
                    // Silence would be ambiguous: nothing pending reads the
                    // same as a command that did not run.
                    format::print_events(&batch, as_json)?;
                    return Ok(());
                }
                if let Some(last) = batch.last() {
                    format::print_events(&batch, as_json)?;
                    if ack {
                        let name = consumer.clone();
                        let through = last.seq;
                        runtime
                            .execute(move |runtime| runtime.store.ack(&name, through))
                            .await?;
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
            let name = consumer.clone();
            let pending = runtime
                .execute(move |runtime| runtime.store.ack(&name, through))
                .await?;
            if as_json {
                format::print_json(
                    &serde_json::json!({ "consumer": consumer, "acked_seq": through, "pending": pending }),
                )?;
            } else {
                outln!("✓ {consumer} acknowledged through {through} ({pending} pending)")?;
            }
            Ok(())
        }

        EventsAction::Stats => {
            let stats = runtime.execute(|runtime| runtime.store.stats()).await?;
            format::print_event_stats(
                &stats,
                &runtime.store.caps(),
                config.events.mode.as_str(),
                profile,
                as_json,
            )?;
            Ok(())
        }

        EventsAction::Prune => {
            let outcome = runtime.execute(|runtime| runtime.store.prune()).await?;
            format::print_prune_outcome(&outcome, as_json)?;
            Ok(())
        }

        EventsAction::Path => {
            format::print_path(runtime.paths.root(), as_json)?;
            Ok(())
        }
    }
}

fn stop_daemon(runtime: &events::EventRuntime, as_json: bool) -> Result<()> {
    let Some(status) = runtime.state.daemon_status()? else {
        anyhow::bail!("no daemon has run for this profile");
    };

    if events::DaemonLock::try_acquire(&runtime.paths.lock_file())?.is_some() {
        anyhow::bail!(
            "no daemon is running for this profile; the record it left behind says pid {} \
             started at {}",
            status.pid,
            format::format_epoch(status.started_at)
        );
    }

    runtime.state.request_stop()?;

    if as_json {
        format::print_json(&serde_json::json!({ "stopped": true, "pid": status.pid }))?;
    } else {
        outln!("✓ Asked the daemon (pid {}) to stop", status.pid)?;
    }
    Ok(())
}
