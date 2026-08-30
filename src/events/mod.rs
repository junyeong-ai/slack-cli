pub mod backfill;
mod db;
pub mod envelope;
pub mod queue;
pub mod rules;
pub mod sink;
pub mod socket;
pub mod state;
pub mod store;

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub use envelope::{EVENT_SCHEMA, Event, EventKind, EventSource};
pub use sink::StdoutFormat;
pub use state::{Counters, DaemonStatus, EventState};
pub use store::{
    ConsumerLag, EventStore, NullStore, PruneOutcome, SqliteStore, StoreCaps, StoreStats,
};

use crate::auth::AuthError;
use crate::config::{Config, EventRetention, EventsConfig};
use crate::slack::{SlackApiError, SlackClient};
use queue::EventQueue;
use rules::RuleEngine;
use sink::SinkSet;
use socket::SocketStream;

/// How often the daemon publishes its counters and trims the tables that grow
/// on their own. Frequent enough that `daemon status` is not stale, rare
/// enough that it costs nothing.
const HOUSEKEEPING_INTERVAL: Duration = Duration::from_secs(30);

/// Reconnect backoff. Slack refreshes connections on its own schedule, so a
/// disconnect is routine and the pause after one is short; repeated failure,
/// and a connection that does not survive long enough to be useful, back off.
const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(60);

/// How long a connection has to last to count as healthy rather than a flap.
/// Slack's own refresh cycle is far longer than this, so anything shorter is
/// something going wrong — and reopening it at full speed would hammer
/// `apps.connections.open` and fill the log.
const HEALTHY_CONNECTION: Duration = Duration::from_secs(30);

/// How long a shutdown waits for the queue to drain through the sinks.
const SHUTDOWN_DRAIN: Duration = Duration::from_secs(20);

/// How many times one event is retried before it is given up on, and the first
/// pause between attempts. The failures worth retrying are storage ones — a
/// busy database, a pool timeout — and those clear in milliseconds. The event
/// has already been acknowledged to Slack, so dropping it on the first stumble
/// would lose it outright.
const HANDLE_ATTEMPTS: u32 = 4;
const HANDLE_BACKOFF: Duration = Duration::from_millis(100);

/// How long the startup identity lookup is retried before giving up. A daemon
/// starting while the network is still coming up should wait for it, not exit.
const IDENTITY_ATTEMPTS: u32 = 5;

/// The files one profile's daemon owns. Kept apart per profile because the
/// credentials, and therefore the events, belong to one workspace each.
pub struct EventPaths {
    root: PathBuf,
}

impl EventPaths {
    pub fn new(dir: &Path, profile: &str) -> Self {
        Self {
            root: dir.join(profile_dir(profile)),
        }
    }

    pub fn state_db(&self) -> PathBuf {
        self.root.join("state.db")
    }

    pub fn events_db(&self) -> PathBuf {
        self.root.join("events.db")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.root.join("daemon.lock")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// The directory name for a profile.
///
/// A profile name is user-chosen and reaches the filesystem here, so it is
/// reduced to characters that mean the same thing on every platform — and
/// that reduction is not injective: `Acme Inc.` and `Acme-Inc` would land on
/// the same directory, which would silently merge two workspaces' cursors,
/// subscriptions and event logs, and make one daemon's lock exclude the
/// other's. A digest of the original name is appended so the mapping is
/// one-to-one again, with the readable part kept in front of it.
fn profile_dir(profile: &str) -> String {
    const STEM_LIMIT: usize = 40;

    let cleaned: String = profile
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    let stem: String = if trimmed.is_empty() {
        "profile".to_string()
    } else {
        trimmed.chars().take(STEM_LIMIT).collect()
    };

    let digest = Sha256::digest(profile.as_bytes());
    let suffix: String = digest
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect();

    format!("{stem}-{suffix}")
}

/// The state and the log, opened without connecting to Slack.
///
/// `events pull`, `events stats` and `daemon status` are answered from here,
/// which is why the store is chosen by retention mode and not by whether a
/// daemon happens to be running.
pub struct EventRuntime {
    pub state: EventState,
    pub store: Arc<dyn EventStore>,
    pub paths: EventPaths,
}

impl EventRuntime {
    pub fn open(dir: &Path, profile: &str, config: &Config) -> Result<Self> {
        Self::open_with(dir, profile, config, config.events.mode)
    }

    /// Opens with an explicit retention mode, which is how `watch` runs
    /// ephemerally against a configuration that would otherwise persist.
    pub fn open_with(
        dir: &Path,
        profile: &str,
        config: &Config,
        mode: EventRetention,
    ) -> Result<Self> {
        let paths = EventPaths::new(dir, profile);
        std::fs::create_dir_all(paths.root())
            .with_context(|| format!("could not create {}", paths.root().display()))?;

        // The state database is always durable, whatever the retention mode:
        // it holds positions and subscriptions, never anything anyone said.
        let state = EventState::open(&paths.state_db())?;

        let store: Arc<dyn EventStore> = if mode.durable() {
            Arc::new(SqliteStore::open(&paths.events_db(), &config.events)?)
        } else {
            Arc::new(NullStore::new())
        };

        Ok(Self {
            state,
            store,
            paths,
        })
    }
}

/// Holds the one-daemon-per-profile guarantee for as long as it is alive.
///
/// Slack load-balances an app's payloads across its open connections, so a
/// second daemon on the same app does not duplicate the stream — it *splits*
/// it, and each half then sees a partial workspace. That failure is silent,
/// which is why it is refused here rather than left to be noticed.
#[derive(Debug)]
pub struct DaemonLock {
    _file: File,
}

impl DaemonLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true).truncate(false);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);

        let file = options
            .open(path)
            .with_context(|| format!("could not open {}", path.display()))?;

        file.try_lock().map_err(|_| {
            anyhow::anyhow!(
                "another slack-cli daemon is already running for this profile (`daemon run` and \
                 `watch` both take this lock). Slack splits an app's events across its open \
                 connections, so a second one would see only part of the workspace — and both \
                 would be writing the same cursors and subscriptions. Stop the first, or give \
                 this one its own profile and Slack app"
            )
        })?;

        Ok(Self { _file: file })
    }
}

#[derive(Default)]
struct Tally {
    received: AtomicU64,
    matched: AtomicU64,
    stored: AtomicU64,
    delivered: AtomicU64,
    failed: AtomicU64,
    reconnects: AtomicU64,
    backfilled: AtomicU64,
}

impl Tally {
    fn snapshot(&self, dropped: u64) -> Counters {
        Counters {
            received: self.received.load(Ordering::Relaxed),
            matched: self.matched.load(Ordering::Relaxed),
            stored: self.stored.load(Ordering::Relaxed),
            dropped,
            delivered: self.delivered.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            reconnects: self.reconnects.load(Ordering::Relaxed),
            backfilled: self.backfilled.load(Ordering::Relaxed),
        }
    }
}

pub struct DaemonOptions {
    pub stdout: StdoutFormat,
    /// Deliver to stdout and nowhere else, whatever sinks are configured.
    ///
    /// What `watch` means. Its whole promise is "show me the events here", and
    /// an installation that has configured an `exec` sink for its daemon would
    /// otherwise see the command connect, match, and print nothing at all.
    pub stdout_only: bool,
    /// Emit a startup summary on stderr. Off for `watch --json`, where stdout
    /// is a data stream and stderr is the only place a note belongs.
    pub announce: bool,
}

/// Runs the pipeline until the process is asked to stop.
///
/// ```text
/// socket ──ack──▶ queue ──▶ dedupe ─▶ rules ─▶ [store] ─▶ sinks ─▶ commit
///                   │                    │
///            bounded, drops         subscriptions
/// ```
///
/// The order is the design. Acknowledging first keeps Slack delivering;
/// evaluating rules before the store means only matches are ever written down;
/// committing last means a crash re-delivers rather than loses.
pub async fn run(
    slack: Arc<SlackClient>,
    config: Config,
    runtime: EventRuntime,
    options: DaemonOptions,
) -> Result<()> {
    let _lock = DaemonLock::acquire(&runtime.paths.lock_file())?;

    let identity = resolve_identity(&slack).await?;

    let configured = config.events.effective_sinks();
    let sinks = Arc::new(if options.stdout_only {
        SinkSet::build(&[crate::config::default_sink("stdout")], options.stdout)?
    } else {
        SinkSet::build(&configured, options.stdout)?
    });

    // With delivery overridden, a rule naming a sink that is no longer there
    // would match and reach nothing. Clearing the names makes every match go
    // to whatever sinks exist, which in that mode is the one on stdout.
    let mut rules = config.events.effective_rules();
    if options.stdout_only {
        for rule in &mut rules {
            rule.sinks.clear();
        }
    }
    let engine = Arc::new(RuleEngine::new(
        rules,
        sinks.names(),
        Some(identity.user_id.clone()),
    ));

    // A channel a rule names is a recovery candidate from the start, before it
    // has ever produced a match.
    for channel in engine.declared_channels() {
        runtime.state.mark_recoverable(channel)?;
    }
    runtime.state.claim_daemon(std::process::id())?;

    // Reported from the store that was actually opened, not from the setting:
    // `watch` runs ephemerally against a configuration that says otherwise,
    // and announcing "spool" while keeping nothing would be a lie about the
    // one thing someone starting this needs to know.
    let retention = if runtime.store.caps().durable {
        config.events.mode.as_str()
    } else {
        EventRetention::Stream.as_str()
    };

    if options.announce {
        eprintln!(
            "watching {} as {} — rules: {} | sinks: {} | retention: {}",
            identity.team,
            identity.user,
            engine.rule_names().join(", "),
            sinks.names().join(", "),
            retention,
        );
    }

    let queue = Arc::new(EventQueue::new(
        config.events.buffer,
        config.events.on_overflow,
    ));
    let tally = Arc::new(Tally::default());
    let connected = Arc::new(AtomicBool::new(false));
    let state = Arc::new(runtime.state);
    let store = runtime.store;

    let processor = tokio::spawn(process(
        queue.clone(),
        state.clone(),
        store.clone(),
        engine.clone(),
        sinks.clone(),
        tally.clone(),
    ));

    let housekeeping = tokio::spawn(housekeep(
        state.clone(),
        store.clone(),
        queue.clone(),
        tally.clone(),
        connected.clone(),
    ));

    let outcome = tokio::select! {
        result = connect_loop(
            slack.clone(),
            config.clone(),
            queue.clone(),
            state.clone(),
            tally.clone(),
            connected.clone(),
            Installation {
                team_id: identity.team_id.clone(),
                org_wide: identity.is_enterprise_install.unwrap_or(false),
            },
        ) => result,
        () = shutdown_signal() => {
            tracing::info!("stopping");
            Ok(())
        }
    };

    // Draining before exit: everything already acknowledged to Slack is
    // Slack's job done, so it has to be this process's job finished. Bounded,
    // because a deep backlog through a slow sink would otherwise hold the
    // process open past any supervisor's patience — and a supervisor that
    // gives up sends SIGKILL, which drains nothing at all.
    queue.close();
    let mut processor = processor;
    let drained = tokio::time::timeout(SHUTDOWN_DRAIN, &mut processor).await;
    if let Ok(Err(panicked)) = &drained {
        tracing::error!("the event processor stopped unexpectedly: {panicked}");
    }
    if drained.is_err() {
        tracing::warn!(
            queued = queue.len(),
            "gave up draining the queue after {}s; those events were acknowledged to Slack but \
             not delivered",
            SHUTDOWN_DRAIN.as_secs()
        );
        // Borrowed rather than consumed above, so the task can still be
        // stopped. Dropping the handle would detach it instead, leaving it
        // writing state after the lock this function holds has been released.
        processor.abort();
        let _ = processor.await;
    }
    housekeeping.abort();
    let _ = housekeeping.await;
    connected.store(false, Ordering::Relaxed);
    let _ = state.heartbeat(false, tally.snapshot(queue.dropped()));

    outcome
}

/// The installation this daemon speaks for.
///
/// The app-level token that opens the connection and the user token that
/// answers `auth.test` are registered separately and nothing makes Slack check
/// that they belong together. Pairing an app token from one workspace with a
/// profile from another is a slip a person can make in one paste — and the
/// result is one workspace's messages filed, matched and delivered under
/// another's name. So what arrives is checked against who this is.
#[derive(Debug, Clone)]
struct Installation {
    team_id: String,
    /// An org-wide Grid install legitimately receives events from several
    /// workspaces, so for one the team is not a single value to compare.
    org_wide: bool,
}

impl Installation {
    /// Whether a delivered payload belongs to this installation.
    ///
    /// The top-level `team_id` is not the answer on its own. In an externally
    /// shared channel Slack sets it to the workspace the message *came from* —
    /// a partner org's — and names the installation that is being delivered to
    /// in `authorizations`. Comparing the top-level field alone would discard
    /// every message a partner sends into a Slack Connect channel, and then
    /// report it as a mispaired token, which is both a data loss and a wrong
    /// diagnosis.
    ///
    /// The check this exists for still works: a genuinely mispaired app token
    /// authorizes a different installation, so nothing in `authorizations`
    /// matches either.
    fn owns(&self, payload: &Value) -> bool {
        if self.org_wide {
            return true;
        }

        // Authoritative when present: who Slack is delivering this to.
        if let Some(authorizations) = payload.get("authorizations").and_then(Value::as_array) {
            let mut named = false;
            for authorization in authorizations {
                if let Some(team) = authorization.get("team_id").and_then(Value::as_str) {
                    named = true;
                    if team == self.team_id {
                        return true;
                    }
                }
                // An org-wide authorization names an enterprise rather than a
                // team, and this daemon has no enterprise to compare — accept
                // rather than drop what it cannot judge.
                if authorization
                    .get("enterprise_id")
                    .and_then(Value::as_str)
                    .is_some()
                    && authorization.get("team_id").is_none()
                {
                    return true;
                }
            }
            // Named someone, and none of them us: this delivery is for a
            // different installation of the same app, which a Socket Mode
            // connection legitimately carries.
            //
            // Slack truncates `authorizations` at around ten entries and
            // publishes the rest through `apps.event.authorizations.list`, so
            // an event visible to more installations than that could list ten
            // foreign ones, omit this one, and be discarded here. A CLI whose
            // app is installed once or twice never reaches it; an app that did
            // would need that method rather than this field.
            if named {
                return false;
            }
        }

        // No authorizations to go on. A shared channel is the one case where
        // the top-level team is expected to be someone else's.
        if payload
            .get("is_ext_shared_channel")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return true;
        }

        match payload.get("team_id").and_then(Value::as_str) {
            Some(team) => team == self.team_id,
            // Recovered events carry the team this daemon asked for, and some
            // payload shapes omit it entirely.
            None => true,
        }
    }
}

/// The teams a payload says it was authorized for, for the one log line that
/// has to name them.
fn authorized_teams(payload: &Value) -> String {
    let teams: Vec<&str> = payload
        .get("authorizations")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("team_id").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();

    if teams.is_empty() {
        return payload
            .get("team_id")
            .and_then(Value::as_str)
            .unwrap_or("-")
            .to_string();
    }
    teams.join(",")
}

/// Who this daemon is running as, waited for rather than demanded.
///
/// A daemon started at boot may come up before the network does. A refused
/// token will not fix itself and is reported at once; anything else is worth
/// a few seconds.
async fn resolve_identity(slack: &SlackClient) -> Result<crate::slack::SlackAuthIdentity> {
    let mut pause = RECONNECT_MIN;

    for attempt in 1..=IDENTITY_ATTEMPTS {
        match slack.auth.identity().await {
            Ok(identity) => return Ok(identity),
            Err(err) if is_fatal(&err) || attempt == IDENTITY_ATTEMPTS => {
                return Err(err).context(
                    "the daemon needs to know which user it is running as before it can \
                     recognise a mention. Check `slack-cli auth status --verify`",
                );
            }
            Err(err) => {
                tracing::warn!(attempt, "could not resolve this installation yet: {err:#}");
                tokio::time::sleep(pause).await;
                pause = (pause * 2).min(RECONNECT_MAX);
            }
        }
    }

    unreachable!("the final attempt returns")
}

/// Whether an error will still be there on the next attempt.
///
/// A daemon is supposed to outlast a network blip, so almost everything is
/// retried. A missing app-level token or a refused scope is not a blip:
/// retrying it every minute forever only buries the one message that says what
/// to do about it.
fn is_fatal(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        if let Some(auth) = cause.downcast_ref::<AuthError>() {
            return matches!(
                auth,
                AuthError::NoAppToken
                    | AuthError::NotConfigured
                    | AuthError::UnknownProfile(_)
                    | AuthError::NoSuchToken { .. }
                    | AuthError::NoTokenForPolicy { .. }
                    | AuthError::NotRenewable { .. }
            );
        }
        if let Some(SlackApiError::Api { code, .. }) = cause.downcast_ref::<SlackApiError>() {
            return matches!(
                code.as_str(),
                "invalid_auth"
                    | "not_authed"
                    | "account_inactive"
                    | "token_revoked"
                    | "token_expired"
                    | "missing_scope"
                    | "not_allowed_token_type"
            );
        }
        false
    })
}

/// Opens a connection, feeds the queue from it, and opens another when it
/// ends. Every reconnect starts recovery, because Socket Mode replays nothing
/// across the gap.
async fn connect_loop(
    slack: Arc<SlackClient>,
    config: Config,
    queue: Arc<EventQueue<Event>>,
    state: Arc<EventState>,
    tally: Arc<Tally>,
    connected: Arc<AtomicBool>,
    installation: Installation,
) -> Result<()> {
    let mut backoff = RECONNECT_MIN;
    let recovering = Arc::new(AtomicBool::new(false));

    loop {
        let opened = tokio::time::Instant::now();
        let outcome = connect_once(
            &slack,
            &config,
            &queue,
            &state,
            &tally,
            &connected,
            &installation,
            &recovering,
        )
        .await;
        connected.store(false, Ordering::Relaxed);

        match outcome {
            Ok(()) => {
                tally.reconnects.fetch_add(1, Ordering::Relaxed);
                // A connection Slack refreshed after it had done its job earns
                // an immediate retry. One that died on arrival is a flap, and
                // reopening it at once would spin on `apps.connections.open`
                // and fill the log with the same line.
                if opened.elapsed() >= HEALTHY_CONNECTION {
                    backoff = RECONNECT_MIN;
                } else {
                    backoff = (backoff * 2).min(RECONNECT_MAX);
                    tracing::debug!(?backoff, "the connection did not last; backing off");
                }
                tokio::time::sleep(backoff).await;
            }
            Err(err) if is_fatal(&err) => return Err(err),
            Err(err) => {
                tracing::warn!("Socket Mode connection failed: {err:#}");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(RECONNECT_MAX);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn connect_once(
    slack: &Arc<SlackClient>,
    config: &Config,
    queue: &Arc<EventQueue<Event>>,
    state: &Arc<EventState>,
    tally: &Arc<Tally>,
    connected: &AtomicBool,
    installation: &Installation,
    recovering: &Arc<AtomicBool>,
) -> Result<()> {
    let url = slack.apps.connection().await?;
    let mut socket = SocketStream::connect(&url).await?;
    connected.store(true, Ordering::Relaxed);

    // Published straight away rather than waiting for the next housekeeping
    // tick. Otherwise `daemon status` reports a working daemon as
    // "reconnecting" with nothing received for the first half-minute of its
    // life — which is exactly when someone is watching it.
    if let Err(err) = state.heartbeat(true, tally.snapshot(queue.dropped())) {
        tracing::warn!("could not publish the connection heartbeat: {err:#}");
    }

    start_recovery(
        slack,
        config,
        queue,
        state,
        tally,
        &installation.team_id,
        recovering,
    );

    let keep_raw = config.events.store_raw;
    let mut reported_foreign = false;
    while let Some(envelope) = socket.next_event().await? {
        // Judged on the delivery envelope, not the normalized event: only the
        // envelope carries who Slack authorized this for.
        if !installation.owns(&envelope.payload) {
            if !reported_foreign {
                reported_foreign = true;
                tracing::error!(
                    expected = %installation.team_id,
                    authorized_for = %authorized_teams(&envelope.payload),
                    "this connection is delivering events authorized for another workspace, so \
                     they are being discarded. The app-level token belongs to a different Slack \
                     app than the profile's user token — re-register one of them"
                );
            }
            continue;
        }
        let Some(event) = envelope::from_event_callback(&envelope.payload, keep_raw) else {
            continue;
        };
        tally.received.fetch_add(1, Ordering::Relaxed);
        queue.push(event);
    }

    socket.close().await;
    Ok(())
}

/// Starts gap recovery alongside the read loop rather than in front of it.
///
/// Recovery reads `conversations.history`, which a non-Marketplace app may
/// call once a minute — so twenty channels is twenty minutes. Doing that
/// before the first `next_event` would leave the socket open and unread for
/// all of it: Slack's frames would sit unacknowledged, it would redeliver,
/// and past its threshold it would stop delivering to the app altogether. So
/// it runs as its own task, feeding the same queue, and the deduplication key
/// collapses whatever it and the socket both saw.
///
/// At most one runs at a time: a flapping connection must not stack recovery
/// passes on top of each other, each spending the same rationed requests.
fn start_recovery(
    slack: &Arc<SlackClient>,
    config: &Config,
    queue: &Arc<EventQueue<Event>>,
    state: &Arc<EventState>,
    tally: &Arc<Tally>,
    team_id: &str,
    recovering: &Arc<AtomicBool>,
) {
    if !config.events.backfill || recovering.swap(true, Ordering::SeqCst) {
        return;
    }

    let slack = slack.clone();
    let queue = queue.clone();
    let state = state.clone();
    let tally = tally.clone();
    let recovering = recovering.clone();
    let events: EventsConfig = config.events.clone();
    let team_id = team_id.to_string();

    tokio::spawn(async move {
        match backfill::recover(&slack, &state, &events, Some(&team_id)).await {
            Ok(recovered) => {
                let room = (queue.capacity() / 2).max(1);
                for event in recovered {
                    // Recovery may wait, and here it must: a deep catch-up can
                    // outrun the pipeline, and under the default policy the
                    // events dropped to make room are the oldest — the very
                    // ones the cursor has not passed yet. Overflowing here
                    // would turn a gap this pass just closed into a permanent
                    // hole.
                    queue.wait_for_room(room).await;
                    tally.backfilled.fetch_add(1, Ordering::Relaxed);
                    tally.received.fetch_add(1, Ordering::Relaxed);
                    queue.push(event);
                }
            }
            Err(err) => tracing::warn!("recovery failed: {err:#}"),
        }
        recovering.store(false, Ordering::SeqCst);
    });
}

/// The one consumer of the queue, and the only writer of state.
///
/// Being single means the read-then-commit around `seen` needs no lock, and
/// that ordering is what turns a crash into a redelivery instead of a hole.
async fn process(
    queue: Arc<EventQueue<Event>>,
    state: Arc<EventState>,
    store: Arc<dyn EventStore>,
    engine: Arc<RuleEngine>,
    sinks: Arc<SinkSet>,
    tally: Arc<Tally>,
) {
    while let Some(mut event) = queue.pop().await {
        let mut pause = HANDLE_BACKOFF;
        for attempt in 1..=HANDLE_ATTEMPTS {
            // This retry covers only what happens before delivery — reading
            // the deduplication gate and evaluating the rules. Once the sinks
            // have the event, `handle` retries the commit itself and returns
            // `Ok`, so a busy database can never repeat a delivery here.
            match handle(&mut event, &state, &store, &engine, &sinks).await {
                // Counted here rather than inside, so a retry does not report
                // the same event twice.
                Ok(handled) => {
                    if handled.matched {
                        tally.matched.fetch_add(1, Ordering::Relaxed);
                    }
                    if handled.stored {
                        tally.stored.fetch_add(1, Ordering::Relaxed);
                    }
                    break;
                }
                // The event was acknowledged to Slack before it got here, so
                // nothing will send it again — and a reaction or an edit
                // cannot be read back from history at all. A busy database is
                // worth waiting out rather than losing it to.
                Err(err) if attempt < HANDLE_ATTEMPTS => {
                    tracing::warn!(
                        event = %event.id,
                        attempt,
                        "could not process an event, retrying: {err:#}"
                    );
                    tokio::time::sleep(pause).await;
                    pause *= 2;
                }
                Err(err) => {
                    tracing::error!(
                        event = %event.id,
                        attempts = HANDLE_ATTEMPTS,
                        "gave up on an event that Slack will not send again: {err:#}"
                    );
                }
            }
        }
        tally.delivered.store(sinks.delivered(), Ordering::Relaxed);
        tally.failed.store(sinks.failed(), Ordering::Relaxed);
    }
}

/// What one pass over an event did, for the counters that report it. Returned
/// rather than tallied inside, because the pass is retried and a retry must not
/// report the same event again.
#[derive(Debug, Default, Clone, Copy)]
struct Handled {
    matched: bool,
    stored: bool,
}

async fn handle(
    event: &mut Event,
    state: &EventState,
    store: &Arc<dyn EventStore>,
    engine: &RuleEngine,
    sinks: &SinkSet,
) -> Result<Handled> {
    if state.is_seen(event)? {
        return Ok(Handled::default());
    }

    let outcome = engine.evaluate(event, state)?;
    let interesting = outcome.matched() || outcome.subscribed > 0;
    let mut handled = Handled {
        matched: outcome.matched(),
        stored: false,
    };

    if outcome.matched() {
        event.matched = outcome.rules.clone();

        match store.append(event)? {
            Some(seq) => {
                event.seq = seq;
                handled.stored = true;
                sinks.deliver(event, &outcome.sinks).await;
            }
            // Already in the log. Usually that means a crash between the
            // append and the commit, so the sinks may never have seen it —
            // and the contract is at-least-once, which makes a second copy
            // the right answer and a silent drop the wrong one. Delivered
            // again, deliberately.
            None => {
                tracing::debug!(
                    event = %event.id,
                    "already in the event log; delivering again rather than risk a drop"
                );
                sinks.deliver(event, &outcome.sinks).await;
            }
        }
    }

    // Committed only now. Until this point a crash leaves the event
    // recoverable; after it, the cursor says the channel is followed to here.
    //
    // Retried here rather than by the caller, and never by re-running what
    // came before it. Everything above has already delivered to the sinks, so
    // starting the pass again on a busy database would repeat a webhook or an
    // exec handler — turning a moment of contention into four real duplicates.
    // Failing to commit only risks the event being seen once more later, which
    // is the smaller of the two.
    let mut pause = HANDLE_BACKOFF;
    for attempt in 1..=HANDLE_ATTEMPTS {
        match commit(event, state, interesting) {
            Ok(()) => return Ok(handled),
            Err(err) if attempt < HANDLE_ATTEMPTS => {
                tracing::warn!(
                    event = %event.id,
                    attempt,
                    "could not record an event as handled, retrying: {err:#}"
                );
                tokio::time::sleep(pause).await;
                pause *= 2;
            }
            Err(err) => {
                tracing::error!(
                    event = %event.id,
                    "delivered an event but could not record it; it may be seen again: {err:#}"
                );
            }
        }
    }

    Ok(handled)
}

/// Records that an event has been handled and how far its conversation has
/// been followed. The last thing to happen, and the only part of the pass that
/// is safe to repeat once the sinks have seen the event.
fn commit(event: &Event, state: &EventState, interesting: bool) -> Result<()> {
    state.mark_seen(event)?;

    if event.kind == EventKind::Message
        && let (Some(channel), Some(ts)) = (event.channel.as_deref(), event.ts.as_deref())
    {
        state.advance_cursor(channel, ts, interesting)?;
        // A subscribed thread keeps its own position, so recovery reads the
        // part of it nobody has seen rather than the whole conversation.
        if let Some(root) = event.thread_ts.as_deref() {
            state.advance_thread_cursor(channel, root, ts)?;
        }
    }

    Ok(())
}

async fn housekeep(
    state: Arc<EventState>,
    store: Arc<dyn EventStore>,
    queue: Arc<EventQueue<Event>>,
    tally: Arc<Tally>,
    connected: Arc<AtomicBool>,
) {
    let mut ticker = tokio::time::interval(HOUSEKEEPING_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut reported_drops = 0;

    loop {
        ticker.tick().await;

        // Reported per tick rather than per event: an overflowing buffer drops
        // continuously, and a line each time would bury the reason under the
        // symptom.
        let dropped = queue.dropped();
        if dropped > reported_drops {
            tracing::warn!(
                dropped = dropped - reported_drops,
                total = dropped,
                "the event buffer overflowed; events were discarded. Raise events.buffer, or \
                 make the sink faster — delivery is serial, so one slow handler holds the \
                 whole pipeline"
            );
            reported_drops = dropped;
        }

        if let Err(err) = state.heartbeat(
            connected.load(Ordering::Relaxed),
            tally.snapshot(queue.dropped()),
        ) {
            tracing::warn!("could not publish the daemon heartbeat: {err:#}");
        }
        if let Err(err) = state.prune_seen() {
            tracing::warn!("could not trim the deduplication table: {err:#}");
        }
        match store.prune() {
            Ok(outcome) if outcome.total() > 0 => {
                tracing::debug!(
                    acknowledged = outcome.acknowledged,
                    expired = outcome.expired,
                    over_budget = outcome.over_budget,
                    "trimmed the event log"
                );
            }
            Ok(_) => {}
            Err(err) => tracing::warn!("could not trim the event log: {err:#}"),
        }
    }
}

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut term = match signal(SignalKind::terminate()) {
        Ok(stream) => stream,
        Err(err) => {
            tracing::warn!("could not listen for SIGTERM: {err}");
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_name_becomes_a_directory_name_that_travels() {
        for profile in ["acme-inc", "Acme Inc.", "../escape", "한국팀", ""] {
            let name = profile_dir(profile);
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "{profile:?} produced {name:?}"
            );
            assert!(!name.contains(".."), "{name:?} could climb out of the root");
            assert_eq!(name, profile_dir(profile), "the mapping must be stable");
        }

        assert!(profile_dir("acme-inc").starts_with("acme-inc-"));
        assert!(profile_dir("한국팀").starts_with("profile-"));
    }

    /// The reduction to filesystem-safe characters is not injective, so two
    /// distinct profiles could otherwise share cursors, subscriptions, an
    /// event log and a lock.
    #[test]
    fn names_that_reduce_alike_still_get_their_own_directory() {
        let collisions = [
            ("Acme Inc.", "Acme-Inc-"),
            ("Acme/Inc-", "Acme-Inc-"),
            ("한국팀", "팀한국"),
            ("", "   "),
        ];
        for (left, right) in collisions {
            assert_ne!(
                profile_dir(left),
                profile_dir(right),
                "{left:?} and {right:?} landed on one directory"
            );
        }
    }

    #[test]
    fn each_profile_gets_its_own_files() {
        let dir = Path::new("/tmp/events");
        let one = EventPaths::new(dir, "acme");
        let two = EventPaths::new(dir, "other");

        assert_ne!(one.state_db(), two.state_db());
        assert_ne!(one.events_db(), two.events_db());
        assert_ne!(one.state_db(), one.events_db());
    }

    /// A second daemon would not duplicate the stream, it would split it, so
    /// the second one is refused rather than allowed to see half a workspace.
    fn payload(team: Option<&str>) -> serde_json::Value {
        match team {
            Some(team) => serde_json::json!({ "team_id": team, "event": { "type": "message" } }),
            None => serde_json::json!({ "event": { "type": "message" } }),
        }
    }

    fn here() -> Installation {
        Installation {
            team_id: "T_HERE".into(),
            org_wide: false,
        }
    }

    /// The app-level token and the user token are registered separately and
    /// Slack never checks they belong together, so a single mistaken paste
    /// would file another workspace's messages under this one.
    #[test]
    fn events_authorized_for_another_workspace_are_not_this_installations() {
        assert!(here().owns(&payload(Some("T_HERE"))));
        assert!(!here().owns(&payload(Some("T_ELSEWHERE"))));
        // Recovered events and some payload shapes carry no team at all; those
        // came from a call this daemon made, so they are its own.
        assert!(here().owns(&payload(None)));
    }

    /// `authorizations` names who Slack is delivering to, and outranks the
    /// top-level team.
    #[test]
    fn the_authorization_decides_rather_than_the_top_level_team() {
        let authorized = serde_json::json!({
            "team_id": "T_ELSEWHERE",
            "authorizations": [{ "team_id": "T_HERE", "user_id": "U1" }],
        });
        assert!(here().owns(&authorized));

        let elsewhere = serde_json::json!({
            "team_id": "T_HERE",
            "authorizations": [{ "team_id": "T_ELSEWHERE", "user_id": "U1" }],
        });
        assert!(!here().owns(&elsewhere));
    }

    /// In an externally shared channel Slack sets the top-level team to the
    /// workspace a message came *from*. Comparing that alone would drop every
    /// message a partner org sends — and then blame the app token for it.
    #[test]
    fn a_partner_message_in_a_shared_channel_is_still_ours() {
        let connect = serde_json::json!({
            "team_id": "T_PARTNER",
            "is_ext_shared_channel": true,
            "event": { "type": "message" },
        });
        assert!(here().owns(&connect));

        let connect_with_authorization = serde_json::json!({
            "team_id": "T_PARTNER",
            "is_ext_shared_channel": true,
            "authorizations": [{ "team_id": "T_HERE", "user_id": "U1" }],
        });
        assert!(here().owns(&connect_with_authorization));
    }

    /// An org-wide Grid install receives events from every workspace in the
    /// org by design, so for one there is no single team to compare against.
    #[test]
    fn an_org_wide_install_owns_every_workspace_it_hears_from() {
        let org = Installation {
            team_id: "T_HERE".into(),
            org_wide: true,
        };
        assert!(org.owns(&payload(Some("T_ELSEWHERE"))));
    }

    /// An org-wide authorization names an enterprise and no team. The daemon
    /// has nothing to compare it against, so it accepts rather than discards
    /// what it cannot judge.
    #[test]
    fn an_authorization_without_a_team_is_not_judged() {
        let enterprise = serde_json::json!({
            "team_id": "T_ELSEWHERE",
            "authorizations": [{ "enterprise_id": "E1", "user_id": "U1" }],
        });
        assert!(here().owns(&enterprise));
    }

    #[test]
    fn the_discard_log_names_who_the_payload_was_authorized_for() {
        let authorized = serde_json::json!({
            "team_id": "T_ELSEWHERE",
            "authorizations": [{ "team_id": "T_A" }, { "team_id": "T_B" }],
        });
        assert_eq!(authorized_teams(&authorized), "T_A,T_B");
        assert_eq!(authorized_teams(&payload(Some("T_X"))), "T_X");
        assert_eq!(authorized_teams(&payload(None)), "-");
    }

    #[test]
    fn only_one_daemon_may_hold_a_profile() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("daemon.lock");

        let first = DaemonLock::acquire(&lock_path).unwrap();
        let err = DaemonLock::acquire(&lock_path).unwrap_err();
        assert!(err.to_string().contains("already running"), "{err}");

        drop(first);
        assert!(DaemonLock::acquire(&lock_path).is_ok());
    }

    #[test]
    fn the_retention_mode_alone_decides_whether_a_log_exists() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.events.mode = EventRetention::Stream;

        let streaming = EventRuntime::open(dir.path(), "acme", &config).unwrap();
        assert!(!streaming.store.caps().durable);
        assert!(!streaming.paths.events_db().exists());
        // The state database exists regardless: it holds positions, not words.
        assert!(streaming.paths.state_db().exists());

        config.events.mode = EventRetention::Spool;
        let spooling = EventRuntime::open(dir.path(), "acme", &config).unwrap();
        assert!(spooling.store.caps().durable);
        assert!(spooling.paths.events_db().exists());
    }

    /// What `watch` relies on: an ephemeral run against a configuration that
    /// would otherwise persist, without touching the log that configuration
    /// describes.
    #[test]
    fn an_ephemeral_run_writes_no_event_log() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.events.mode = EventRetention::Archive;

        let ephemeral =
            EventRuntime::open_with(dir.path(), "acme", &config, EventRetention::Stream).unwrap();
        assert!(!ephemeral.store.caps().replayable);
        assert!(!ephemeral.paths.events_db().exists());
    }
}
