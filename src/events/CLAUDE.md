# events/ — Socket Mode daemon

Opens Slack's Socket Mode WebSocket, normalizes what arrives, keeps only what a
rule matched, and hands it to whatever is consuming. The reason it exists is
arithmetic: `conversations.history` can be limited to one request a minute and 15 messages for
commercially distributed apps outside the Marketplace; internal apps retain their tiered limits, so polling cannot follow a workspace. Events are pushed
on a separate axis, capped at 30,000 per workspace per hour.

## The pipeline, in order

```
socket ──ack──▶ queue ──▶ dedupe ─▶ rules ─▶ [store] ─▶ sinks ─▶ commit
                  │                    │
           bounded, drops         subscriptions
```

The order *is* the design:

1. **Acknowledge first.** Slack redelivers what is not acknowledged and stops
   delivering to an app that falls below its response threshold. `SocketStream`
   acknowledges inside its read loop, before the caller sees the envelope, so
   no consumer can ever slow that down.
2. **The queue drops; it never blocks — for the producer that cannot wait.**
   Back-pressure must not reach the acknowledgement path, so the socket's push
   is bounded, applies `on_overflow`, and counts every drop; with no event log
   behind it, loss is a policy the operator chose and a policy has to be
   visible.

   Two things qualify that. Recovery *can* wait — nothing has been
   acknowledged on its behalf — so it awaits capacity rather than overflowing
   the queue with the very events it went to a rationed endpoint to fetch. And
   what an overflow discards is the oldest **evictable** item: a recovered
   event is never the one chosen, because Slack will not send it again and the
   newer events queued behind it would move its channel's cursor past the hole.
3. **Rules run before the store.** Only matches are ever written down. An
   unmatched event moves the channel cursor and is discarded, which is what
   makes "keep nothing" the same code path as "keep everything".
4. **Commit last.** `state.is_seen` is a read; `mark_seen` and
   `advance_cursor` happen only after the event has been stored and delivered.
   A crash in between therefore leaves the event recoverable rather than lost.
   One task writes this state, so check-then-commit cannot race.

## Two databases, and why

| | Holds | Retention applies |
|---|---|---|
| `state.db` | cursors, thread subscriptions, dedupe keys, the daemon record | never — always durable |
| `events.db` | the events themselves | yes — this is the only content on disk |

A cursor is a reference, not a copy: `events.mode = "stream"` keeps no message
text at all and still recovers from a disconnect, unsubscribes a thread and
refuses a duplicate. That separation is what makes the streaming mode a real
mode rather than a degraded one.

Neither file lives under `cache_dir`. The cache drops and rebuilds every table
when its schema version moves, which is sound because its contents are
refetchable — and exactly wrong here, since Socket Mode never replays.
`db::migrate` applies steps forward and **refuses a database written by a newer
build** rather than rebuilding it.

A consumer is registered at its first pull, even when the log is empty. Registration and batch selection share a transaction; spool pruning includes its unacknowledged position immediately. Age and size limits still apply.

## Retention is a capability, not a branch

`EventStore` has two implementations and everything upstream is written against
the trait. `events.mode = "stream"` is not a second code path; it is
`NullStore`, which hands out positions and writes nothing. A command that needs
to read events back checks `caps().replayable` and fails **by name** — an empty
result would look like a quiet workspace.

`store_body` is the orthogonal axis: how *much* of a matched event is kept.
With it off the log is an index of references and Slack stays the only copy of
what was said.

## What is guaranteed, and to whom

Persisted rows are replayable within the retention and size limits. An event that matched is appended before it is
delivered and the commit happens after both, so a crash re-delivers rather than
drops — and on the re-run `append` returns `None` for a row that is already
there, which is answered by delivering *again*, because at-least-once makes a
duplicate the right failure and a silent skip the wrong one.

Sinks are best-effort on top of that. A failed push is counted and logged, not
retried: retrying inline would stall every event behind it, and one unreachable
handler must not stop the daemon. A consumer that needs the guarantee reads the
log with `events pull`.

A storage failure is different from a sink failure and is retried with backoff
before the daemon stops, because Slack has already been told the event
was received and will not send it again — and an edit or a reaction cannot be
reconstructed from history at all.

Preparation failures are retried before sink delivery. Commit failures are retried separately and never replay a sink. Persistent storage failures stop the pipeline; recovery completion cannot pass a failed event.

Recovery pins channel and thread positions durably before opening each socket. Live traffic advances its high-water marks without changing those positions. A non-evictable completion item follows the recovered events through the serial queue and releases only its own reconnect generation. Errors and cancellation retain the checkpoint; partial thread reads advance it only through the processed prefix. Channel page limits retain the older checkpoint.

## `watch` overrides both halves of delivery

`daemon run` honours the configuration. `watch` does not, deliberately: it
forces `EventRetention::Stream` so nothing is written down, and `stdout_only`
so events go to stdout whatever sinks are configured. Its promise is "show me
the events here", and an installation set up with an `exec` sink for its daemon
would otherwise watch the command connect, match, and print nothing.

## Push and pull are alternatives, not layers

A matched event is delivered to its sinks *and* appended to the log, so an
installation that configures an `exec` sink and also runs `events pull` gets
every event twice. Sinks are the low-latency path and lose what arrives while
the consumer is down; `pull` is the cursor-based path that survives a restart.
Both are offered because the choice depends on whether the consumer is always
on — but only one of them should be wired up at a time.

## Invariants

1. **One daemon per Slack app.** Slack load-balances an app's payloads across
   its open connections, so a second daemon splits the stream rather than
   duplicating it, and each half sees a partial workspace. `DaemonLock` refuses
   a second process for the same profile. Different profiles and machines have
   separate locks, so the operator must assign each daemon its own Slack app.
2. **The daemon never holds a token.** Every call goes through
   `Authenticator::token_for`, so the 12-hour PKCE rotation is handled by the
   existing cross-process transaction. Caching a token at startup would work
   for twelve hours and then stop.
3. **Deduplication is by content, not by delivery.** `Event::dedupe_key` is
   derived from the channel and timestamp, so a redelivered envelope and a
   backfilled message collapse onto the same key. Keying on `event_id` would
   not, and recovery would double every message it re-read.

   The exception proves the rule: an edit, a deletion and a reaction are *not*
   about a message's own existence — their `ts` names something else — so
   `event_ts` joins their key. Without it, two edits of one message share a key
   and the second is dropped, and a re-added subscribe emoji is discarded as a
   repeat of the first, which breaks the toggle. Those three shapes are exactly
   the ones `conversations.history` can never return, so nothing recovery
   produces stops collapsing.
4. **Recovery is bounded in three directions** — channels a rule cares about,
   `backfill_max_channels`, and `backfill_max_age_hours`. A full catch-up is
   not a slow option at one request a minute; it is an impossible one. Every
   read starts from a stored position and never from the beginning: `oldest` on
   the request so the pages cover the missed tail rather than the seen head,
   and a client-side check on the way back, because Slack treats `oldest`
   inclusively and returns a thread's parent regardless.

   The age bound is a **clamp on the read, not a filter on the channel**. A
   cursor older than the horizon means the read starts at the horizon and the
   stretch before it is reported; dropping the channel instead would lose
   everything inside the window as well, without a word — which is the one
   thing this module's handling of loss is not allowed to do.
5. **A reaction subscribes; it does not match.** Only the authenticated user's
   own reaction counts, and what matches is the replies that arrive afterwards.
   Recovery therefore reads subscribed threads through `conversations.replies`
   as well as channels through `conversations.history`: the latter never
   returns a thread's replies, so without that pass the one flow the emoji rule
   exists for is the one a disconnect loses.

   That pass is bounded by a cursor on `watched_thread`, exactly as the channel
   pass is bounded by `channel_cursor`. Every deduplication layer expires — the
   seen keys after a day, the log rows as soon as a consumer acknowledges them
   — so a thread read from the top on each reconnect would eventually be
   delivered in full, again and again. The cursor starts where the subscription
   was made, which is also what keeps recovery from delivering a conversation
   that happened before anyone asked to follow it. Each rule keeps an immutable
   `subscribed_from_ts` separately from its advancing cursor. Recovery reads
   from the earliest rule position, while matching checks the individual
   boundary. Schema 5 preserves a legacy row's recovery/cursor boundary; the
   original subscription timestamp cannot be reconstructed. Both channel and
   thread recovery apply the configured age limit and report truncated gaps.
6. **Delivery is serial, across sinks and across events.** One task owns the
   pipeline. That is what lets the deduplication gate be a plain read followed
   by a commit with no lock, and what keeps a consumer seeing events in the
   order they arrived. A slow sink slows the pipeline by design; the bounded
   queue in front absorbs it so the acknowledgement path never does.
7. **Event stores have a verified owner.** `owner.rs` binds the API endpoint,
   profile name, team ID and user ID to a durable namespace. Re-login and token
   rotation for the same owner retain the store. Environment credentials are
   validated once per credential set and keep a private, nonsecret identity
   binding for offline reads. Never infer ownership of a legacy directory from
   its name or contents. Preserve it without importing it. The daemon checks
   its live identity against the selected owner before processing events.
8. **What arrives is checked against who this is.** The app-level token and the
   user token are registered separately and Slack never checks they belong
   together, so one mistaken paste would file another workspace's messages
   under this one. A payload not authorized for this installation is discarded
   and reported.

   The judgement is made on the delivery envelope, not on the normalized event,
   and it reads `authorizations` rather than the top-level `team_id`. In an
   externally shared channel Slack sets that field to the workspace a message
   came *from*, so comparing it would discard everything a partner org says in
   a Slack Connect channel — and then blame the app token for it. An org-wide
   Grid install owns whatever it hears from, and an authorization naming only
   an enterprise is accepted rather than judged against a team this daemon does
   not have.
9. **Rules are data.** They are validated when the config loads, including the
   combinations that would silently never fire: a blank keyword that matches
   everything, a text predicate on a rule that only sees reactions, a
   subscribing rule that never sees the removal that would unsubscribe it.

## Adding an event kind

1. `envelope.rs`: add the variant to `EventKind`, map it in `from_slack`, and
   fill the fields in `from_event_callback` — Slack puts a message's body in
   different places depending on subtype, so read the shape, not the name.
2. `config.rs`: add the matching `EventKindConfig` variant.
3. `envelope.rs`: extend `EventKind::matches_config`.
4. `rules.rs`: decide whether any predicate applies to it.
5. Both READMEs: the Event Subscriptions list in the Socket Mode section.

## Adding a sink

`config.rs` gains a `SinkKind` variant and its validation; `sink.rs` gains the
`Delivery` arm. Delivery is serial — one task owns the pipeline — and a failure
is counted and logged, never propagated. One unreachable sink must not stop the
daemon, and a sink that needs a guarantee is the wrong shape: point the
consumer at `events pull` instead.

## Execution boundaries

`pipeline.rs` prepares and commits events on the state database executor and awaits sinks between those operations. Processing stays serial. Recovery and housekeeping use the same bounded executor; no pool wait or SQLite busy handler occupies the Socket Mode runtime. CLI event handlers live in `commands/events.rs`.

`daemon stop` records a request in the profile state database. The daemon checks it during startup and while connected, then drains its queue before exiting. No external PID is signalled. Shutdown cancels recovery, drains the processing queue and waits for active database workers before releasing the profile lock. A record from an older daemon has no control flag; it must be stopped through its service manager before upgrading.
