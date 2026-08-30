use std::io::Write;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::AsyncWriteExt;

use super::envelope::Event;
use crate::config::{SinkConfig, SinkKind};

/// How the stdout sink renders an event. The `--json` flag decides, the same
/// way it decides every other command's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdoutFormat {
    /// One JSON object per line — what a consuming process reads.
    Ndjson,
    /// One terse line per event, for a person watching a terminal.
    Line,
}

enum Delivery {
    Stdout {
        format: StdoutFormat,
        timeout: Duration,
    },
    Exec {
        command: Vec<String>,
        timeout: Duration,
    },
    Http {
        url: String,
        client: reqwest::Client,
        timeout: Duration,
    },
}

pub struct Sink {
    name: String,
    delivery: Delivery,
}

/// Every configured destination, and the counters that say how they are doing.
pub struct SinkSet {
    sinks: Vec<Arc<Sink>>,
    delivered: AtomicU64,
    failed: AtomicU64,
}

impl SinkSet {
    pub fn build(configs: &[SinkConfig], stdout: StdoutFormat) -> Result<Self> {
        let sinks = configs
            .iter()
            .map(|config| {
                let timeout = Duration::from_secs(config.timeout_seconds);
                let delivery = match config.kind {
                    SinkKind::Stdout => Delivery::Stdout {
                        format: stdout,
                        timeout,
                    },
                    SinkKind::Exec => Delivery::Exec {
                        command: config.command.clone(),
                        timeout,
                    },
                    SinkKind::Http => Delivery::Http {
                        url: config.url.clone().context("an http sink needs a url")?,
                        client: reqwest::Client::builder()
                            .timeout(timeout)
                            .build()
                            .context("could not build the sink HTTP client")?,
                        timeout,
                    },
                };
                Ok(Arc::new(Sink {
                    name: config.name.clone(),
                    delivery,
                }))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            sinks,
            delivered: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        })
    }

    pub fn names(&self) -> Vec<String> {
        self.sinks.iter().map(|sink| sink.name.clone()).collect()
    }

    pub fn delivered(&self) -> u64 {
        self.delivered.load(Ordering::Relaxed)
    }

    pub fn failed(&self) -> u64 {
        self.failed.load(Ordering::Relaxed)
    }

    /// Sends one event to the sinks a rule named.
    ///
    /// Deliberately serial, and serial across events too: one task owns the
    /// pipeline, which is what lets the deduplication gate be a plain read
    /// followed by a commit, and what keeps a consumer seeing events in the
    /// order they arrived. A slow sink therefore slows the pipeline — that
    /// pressure is absorbed by the bounded queue in front of it, never by the
    /// acknowledgement path.
    ///
    /// A sink is delivered to at least once and never exactly once: a timeout
    /// may still have acted on the far side, and a restart re-delivers what it
    /// cannot prove arrived, so a consumer that cannot tolerate a repeat keys
    /// off the event's `id`.
    ///
    /// It is not delivered to *reliably*. A failure is counted and logged and
    /// the pipeline moves on — one unreachable sink must not stop the daemon,
    /// and retrying inline would stall every event behind it. The event is
    /// still in the log, so a `spool` consumer reading with `events pull` gets
    /// it; a push sink is the low-latency path, not the guaranteed one.
    pub async fn deliver(&self, event: &Event, targets: &[String]) {
        for sink in &self.sinks {
            if !targets.iter().any(|name| name == &sink.name) {
                continue;
            }
            match sink.send(event).await {
                Ok(()) => {
                    self.delivered.fetch_add(1, Ordering::Relaxed);
                }
                Err(err) => {
                    self.failed.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(sink = %sink.name, event = %event.id, "delivery failed: {err:#}");
                }
            }
        }
    }
}

impl Sink {
    async fn send(&self, event: &Event) -> Result<()> {
        match &self.delivery {
            Delivery::Stdout { format, timeout } => write_stdout(event, *format, *timeout).await,
            Delivery::Exec { command, timeout } => run_command(command, event, *timeout).await,
            Delivery::Http {
                url,
                client,
                timeout,
            } => post(client, url, event, *timeout).await,
        }
    }
}

/// Writes one line, on a blocking thread.
///
/// stdout is usually a pipe, and a pipe whose reader is slow blocks the
/// writer. Doing that on a runtime worker would stall whatever else that
/// thread was carrying — including, on a busy runtime, the socket task whose
/// only job is to keep acknowledging. Awaiting the blocking task keeps the
/// lines in order while leaving the worker free.
async fn write_stdout(event: &Event, format: StdoutFormat, timeout: Duration) -> Result<()> {
    let line = match format {
        StdoutFormat::Ndjson => event.to_ndjson()?,
        StdoutFormat::Line => summarize(event),
    };

    let write = tokio::task::spawn_blocking(move || -> Result<()> {
        let mut out = std::io::stdout().lock();
        writeln!(out, "{line}").context("could not write to stdout")?;
        out.flush().context("could not flush stdout")
    });

    match tokio::time::timeout(timeout, write).await {
        Ok(joined) => joined.context("the stdout writer panicked")?,
        // A blocking write into a full pipe cannot be interrupted, so the
        // thread is left to finish while the pipeline moves on. That is the
        // point: without this the one sink with no timeout of its own would
        // hold every later event, and the shutdown drain with them.
        Err(_) => anyhow::bail!(
            "stdout did not accept the event within {}s; its reader has stopped reading",
            timeout.as_secs()
        ),
    }
}

/// One line a person can scan: when, where, who, which rule, and enough of
/// the text to recognise it.
fn summarize(event: &Event) -> String {
    let text = event.text.as_deref().unwrap_or("");
    let mut trimmed: String = text.chars().take(80).collect();
    if text.chars().count() > 80 {
        trimmed.push('…');
    }
    format!(
        "{} [{}] {} {} {}{}",
        event.received_at.format("%H:%M:%S"),
        event.matched.join(","),
        event.channel.as_deref().unwrap_or("-"),
        event
            .user
            .as_deref()
            .unwrap_or(event.bot_id.as_deref().unwrap_or("-")),
        trimmed.replace('\n', " "),
        event
            .reaction
            .as_deref()
            .map(|emoji| format!(":{emoji}:"))
            .unwrap_or_default(),
    )
}

/// Runs the handler with the event on its stdin.
///
/// The timeout covers the write as well as the wait. A handler that never
/// reads its stdin blocks the writer once the pipe buffer is full — and one
/// task owns this whole pipeline, so a write that never returns is a daemon
/// that stops delivering, stops draining its queue, and cannot even be shut
/// down. Timing only the wait would leave exactly that hole.
async fn run_command(command: &[String], event: &Event, timeout: Duration) -> Result<()> {
    let (program, args) = command
        .split_first()
        .context("an exec sink needs a program to run")?;

    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // A shutdown that gives up on the drain aborts this task mid-call, and
        // dropping a `Child` does not stop the process it owns. Without this, a
        // handler outlives the daemon that started it.
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("could not start {program}"))?;

    let payload = event.to_ndjson()?;
    let feed_and_wait = async {
        if let Some(mut stdin) = child.stdin.take() {
            // A handler that does not read its input has not failed. It may
            // exit the moment it starts — a trigger, a notifier, anything that
            // acts on the fact of an event rather than its contents — and the
            // write then lands on a pipe that is already closed. Its exit
            // status is what says whether the work happened, so a broken pipe
            // here is passed over and the status below decides.
            match write_event(&mut stdin, payload.as_bytes()).await {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => {}
                Err(err) => return Err(err),
            }
        }
        child.wait().await
    };

    let status = match tokio::time::timeout(timeout, feed_and_wait).await {
        Ok(status) => status?,
        Err(_) => {
            // The child is past helping — it either will not read or will not
            // finish. Killing it keeps a wedged handler from accumulating
            // processes, and releases this task either way.
            let _ = child.kill().await;
            anyhow::bail!("{program} did not finish within {}s", timeout.as_secs());
        }
    };

    if !status.success() {
        anyhow::bail!("{program} exited with {status}");
    }
    Ok(())
}

/// POSTs the event, naming only the host if anything goes wrong.
///
/// A webhook URL is a credential — Slack's own are a secret in the path, and
/// others put one in the query or the userinfo. The daemon's warnings end up
/// in a supervisor's log, which is not where that belongs, so the URL never
/// appears in an error and `reqwest`'s own message (which embeds it) is
/// replaced rather than wrapped.
async fn post(client: &reqwest::Client, url: &str, event: &Event, timeout: Duration) -> Result<()> {
    let response = client
        .post(url)
        .timeout(timeout)
        .json(event)
        .send()
        .await
        .map_err(|err| {
            anyhow::anyhow!(
                "could not POST to {}: {}",
                safe_host(url),
                describe_transport(&err)
            )
        })?;

    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("{} answered {status}", safe_host(url));
    }
    Ok(())
}

async fn write_event(
    stdin: &mut tokio::process::ChildStdin,
    payload: &[u8],
) -> std::io::Result<()> {
    stdin.write_all(payload).await?;
    stdin.write_all(b"\n").await?;
    stdin.shutdown().await
}

/// The scheme and host, which is enough to say which sink failed and carries
/// no secret. Falls back to a placeholder rather than echoing an unparseable
/// URL that might be one.
fn safe_host(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => match parsed.host_str() {
            Some(host) => match parsed.port() {
                Some(port) => format!("{}://{host}:{port}", parsed.scheme()),
                None => format!("{}://{host}", parsed.scheme()),
            },
            None => "the configured url".to_string(),
        },
        Err(_) => "the configured url".to_string(),
    }
}

/// `reqwest`'s `Display` embeds the request URL, so the kind of failure is
/// reported instead of the error's own text.
fn describe_transport(err: &reqwest::Error) -> &'static str {
    if err.is_timeout() {
        "timed out"
    } else if err.is_connect() {
        "could not connect"
    } else if err.is_body() || err.is_decode() {
        "the response could not be read"
    } else {
        "the request failed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_sink;
    use crate::events::envelope::{EVENT_SCHEMA, EventKind, EventSource};
    use chrono::Utc;

    fn event() -> Event {
        Event {
            schema: EVENT_SCHEMA.to_string(),
            id: "Ev1".into(),
            seq: 1,
            kind: EventKind::Message,
            source: EventSource::Socket,
            team_id: None,
            channel: Some("C01".into()),
            channel_type: None,
            user: Some("U01".into()),
            bot_id: None,
            ts: Some("1700000000.000100".into()),
            event_ts: None,
            thread_ts: None,
            subtype: None,
            text: Some("hello there".into()),
            reaction: None,
            item_user: None,
            received_at: Utc::now(),
            matched: vec!["mention".into()],
            raw: None,
        }
    }

    #[test]
    fn a_summary_line_names_the_rule_that_matched() {
        let line = summarize(&event());
        assert!(line.contains("[mention]"));
        assert!(line.contains("C01"));
        assert!(line.contains("hello there"));
    }

    #[test]
    fn a_long_message_is_cut_rather_than_wrapped() {
        let long = Event {
            text: Some("x".repeat(500)),
            ..event()
        };
        let line = summarize(&long);
        assert!(line.contains('…'));
        assert!(line.chars().count() < 160);
    }

    #[test]
    fn a_newline_never_breaks_one_event_into_two_lines() {
        let multiline = Event {
            text: Some("first\nsecond".into()),
            ..event()
        };
        assert!(!summarize(&multiline).contains('\n'));
    }

    /// A webhook URL is a credential. It must not reach a log line, and
    /// `reqwest`'s own error text embeds it, so that is replaced rather than
    /// wrapped.
    #[test]
    fn a_failing_http_sink_never_names_the_url() {
        let secret = "https://hooks.example.com/services/T000/B000/aVerySecretToken?key=hunter2";
        let host = safe_host(secret);

        assert_eq!(host, "https://hooks.example.com");
        assert!(!host.contains("aVerySecretToken"));
        assert!(!host.contains("hunter2"));

        assert_eq!(
            safe_host("http://127.0.0.1:9000/hook"),
            "http://127.0.0.1:9000"
        );
        assert_eq!(safe_host("not a url"), "the configured url");
    }

    #[test]
    fn an_http_sink_without_a_url_is_refused_at_build_time() {
        let broken = SinkConfig {
            kind: SinkKind::Http,
            url: None,
            ..default_sink("agent")
        };
        assert!(SinkSet::build(&[broken], StdoutFormat::Ndjson).is_err());
    }

    #[tokio::test]
    async fn a_command_that_fails_is_counted_and_does_not_propagate() {
        let sinks = SinkSet::build(
            &[SinkConfig {
                kind: SinkKind::Exec,
                command: vec!["false".into()],
                ..default_sink("agent")
            }],
            StdoutFormat::Ndjson,
        )
        .unwrap();

        sinks.deliver(&event(), &["agent".to_string()]).await;
        assert_eq!(sinks.failed(), 1);
        assert_eq!(sinks.delivered(), 0);
    }

    #[tokio::test]
    async fn an_event_reaches_only_the_sinks_it_was_addressed_to() {
        let sinks = SinkSet::build(
            &[
                SinkConfig {
                    kind: SinkKind::Exec,
                    command: vec!["false".into()],
                    ..default_sink("unused")
                },
                SinkConfig {
                    kind: SinkKind::Exec,
                    command: vec!["true".into()],
                    ..default_sink("agent")
                },
            ],
            StdoutFormat::Ndjson,
        )
        .unwrap();

        sinks.deliver(&event(), &["agent".to_string()]).await;
        assert_eq!(sinks.delivered(), 1);
        assert_eq!(sinks.failed(), 0);
    }

    /// A handler that never reads its stdin blocks the writer once the pipe
    /// buffer fills. One task owns the pipeline, so timing only the wait would
    /// leave a daemon that neither delivers nor shuts down.
    /// A handler can exit before it reads anything — a trigger that acts on
    /// the fact of an event rather than its contents. The write then lands on
    /// a closed pipe, and counting that as a failed delivery would report
    /// every such handler as broken. It is the exit status that decides.
    ///
    /// This is the race macOS lost and Linux won: `true` exits immediately,
    /// so whether the write completes at all is a matter of scheduling.
    #[tokio::test]
    async fn a_handler_that_ignores_its_input_still_counts_as_delivered() {
        let sinks = SinkSet::build(
            &[SinkConfig {
                kind: SinkKind::Exec,
                command: vec!["true".into()],
                ..default_sink("agent")
            }],
            StdoutFormat::Ndjson,
        )
        .unwrap();

        // Large enough that the write cannot complete in one go before the
        // child has exited, so the broken pipe is reached deterministically.
        let mut wide = event();
        wide.text = Some("x".repeat(4 * 1024 * 1024));

        for _ in 0..5 {
            sinks.deliver(&wide, &["agent".to_string()]).await;
        }
        assert_eq!(sinks.failed(), 0, "a closed stdin is not a failed delivery");
        assert_eq!(sinks.delivered(), 5);
    }

    #[tokio::test]
    async fn a_command_that_never_reads_its_input_is_killed_at_the_timeout() {
        let sinks = SinkSet::build(
            &[SinkConfig {
                kind: SinkKind::Exec,
                // Never reads stdin, never exits.
                command: vec!["sleep".into(), "60".into()],
                timeout_seconds: 1,
                ..default_sink("agent")
            }],
            StdoutFormat::Ndjson,
        )
        .unwrap();

        let mut huge = event();
        huge.text = Some("x".repeat(1024 * 1024));

        let started = std::time::Instant::now();
        sinks.deliver(&huge, &["agent".to_string()]).await;
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "a handler that will not read must not wedge the pipeline"
        );
        assert_eq!(sinks.failed(), 1);
    }

    #[tokio::test]
    async fn a_command_that_never_finishes_is_killed_at_the_timeout() {
        let sinks = SinkSet::build(
            &[SinkConfig {
                kind: SinkKind::Exec,
                command: vec!["sleep".into(), "30".into()],
                timeout_seconds: 1,
                ..default_sink("agent")
            }],
            StdoutFormat::Ndjson,
        )
        .unwrap();

        let started = std::time::Instant::now();
        sinks.deliver(&event(), &["agent".to_string()]).await;
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(sinks.failed(), 1);
    }
}
