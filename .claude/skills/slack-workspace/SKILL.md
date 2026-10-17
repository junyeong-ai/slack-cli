---
name: slack-workspace
metadata:
  version: "0.19.5"
description: Use slack-cli to search and read Slack, manage messages, reactions, pins and bookmarks, look up users and channels, or consume collected Socket Mode events.
allowed-tools: Bash(slack-cli *), Bash(jq *)
---

# slack-cli

## Idiom: `--json | jq`

Default output is human-formatted. Add `--json` whenever you need to parse or chain commands.

```bash
# Discover candidates; select the intended user ID explicitly
slack-cli users "john" --json | jq '.[] | {id, name, real_name, email}'

# Send to an exact channel name
slack-cli send "#general" -t "Hello"

# Send a parent message, then reply in the thread
ts=$(slack-cli send "#general" -t "Parent" --json | jq -r '.ts')
slack-cli send "#general" -t "Reply" --thread "$ts"

# Fetch a history page, then request the next page when a cursor is present
page=$(slack-cli messages "#general" --json)
echo "$page" | jq '.messages[]'
cursor=$(echo "$page" | jq -r '.next_cursor // empty')
[ -n "$cursor" ] && slack-cli messages "#general" --cursor "$cursor" --json

# Search workspace content (Real-time Search API)
slack-cli search "deploy plan changes" --sort timestamp --json
```

## Errors and exit codes

Use `--json` for machine-readable stdout; event streams use NDJSON. Runtime failures print to stderr — in `--json` mode as an `{"error": {"code", "message"}}` envelope. `code` is Slack's error string verbatim for API failures (`channel_not_found`, `missing_scope`, …), otherwise `auth_error` / `rate_limited` / `http_error` / `network_error` / `error`.

Usage errors are the one exception: they exit `2` from clap at parse time with plain diagnostic text, never the envelope — branch on the exit code alone.

Exit codes: `0` ok · `1` generic · `2` usage · `3` auth (inspect credentials, profile selection or permissions) · `4` rate-limited (back off before retrying).

Rotating tokens renew before use. Exit code `3` can also indicate missing app credentials, mismatched workspaces or a changed authorization; follow the specific error instead of always logging in again.

## Markdown: prefer `--markdown-text`

`send`/`update` accept `--markdown-text` — standard Markdown that Slack renders server-side (max 12,000 chars). Use it whenever the whole message body is Markdown; no translation needed. It cannot be combined with `-t`/`-b` (attachments and metadata are fine).

```bash
slack-cli send "#general" --markdown-text "**Deploy done** — see [runbook](https://example.com)"
```

## Slack mrkdwn — translate when using `-t` or blocks

Inside `-t` text or Block Kit `text` objects, Markdown renders literally (e.g. `**bold**` shows the asterisks). Convert first:

| Element | Slack | Wrong |
|---------|-------|-------|
| Bold | `*text*` | `**text**` |
| Italic | `_text_` | `*text*` |
| Strikethrough | `~text~` | `~~text~~` |
| Inline code | `` `text` `` | (same) |
| Link | `<url\|label>` | `[label](url)` |
| User mention | `<@U123>` | `@user` |
| Channel mention | `<#C123>` | `#channel` |
| List item | `• item` (U+2022) | `- item` |

## Commands

```bash
# Lookups (cache-backed; one-time `cache refresh` after login)
slack-cli users    <query>   [--id U1,U2] [--expand FIELDS] [--limit N] --json
slack-cli channels <query>   [--id C1,C2] [--expand FIELDS] [--limit N] --json
slack-cli members  <channel>

# Reading
slack-cli messages <channel> [--limit N] [--oldest DATE] [--latest DATE] [--exclude-bots] [--expand FIELDS] --json
slack-cli thread   <channel> <ts> [--limit N] [--exclude-bots] [--expand FIELDS] --json
slack-cli search   <query>   [filters…] --json
slack-cli search   --capabilities --json
slack-cli permalink <channel> <ts> --json

# Writing  (≥1 of -t / --markdown-text / -b / -a is required)
slack-cli send   <channel> [-t TEXT] [--markdown-text MD] [-b BLOCKS] [-a ATTACHMENTS] [-m METADATA] [--thread <ts>] --json
slack-cli update <channel> <ts> [-t TEXT] [--markdown-text MD] [-b BLOCKS] [-a ATTACHMENTS] [-m METADATA] --json
slack-cli delete <channel> <ts>

# Reactions, pins, bookmarks, emoji
slack-cli react      <channel> <ts> <emoji>
slack-cli unreact    <channel> <ts> <emoji>
slack-cli reactions  <channel> <ts> --json
slack-cli pin        <channel> <ts>
slack-cli unpin      <channel> <ts>
slack-cli pins  <channel> --json
slack-cli bookmark   <channel> <title> <url> [--emoji <e>]
slack-cli unbookmark <channel> <bookmark_id>
slack-cli bookmarks  <channel> --json
slack-cli emoji [--query <q>] --json

# Real-time events (only when a daemon is running — see below)
slack-cli events pull [--consumer NAME] [--limit N] [--ack] [--follow] --json
slack-cli events ack --consumer NAME --through <seq>
slack-cli events stats --json
slack-cli daemon status --json

# Cache
slack-cli cache refresh [users|channels|all]
slack-cli cache stats --json

# `self update` replaces the user's binary — a human setup step, not an
# agent action. Do not run it.
#
# `slack-cli watch` and `slack-cli daemon run` run until killed. Never invoke
# either: an agent reads what a daemon already collected with `events pull`.

# Auth (read-only inspection; `auth login` is a human setup step)
slack-cli auth status [--verify] --json
slack-cli auth profiles --json
slack-cli auth scopes --json
```

`DATE` accepts a Unix timestamp or `YYYY-MM-DD`.

## JSON sources for `--blocks`, `--attachments`, `--metadata`

All three flags share one input vocabulary:

| Form | Meaning |
|---|---|
| `-` | Read JSON from stdin (at most **one** flag per invocation) |
| `@path.json` | Read JSON from a file |
| anything else | Inline JSON literal |

JSON source shapes are validated before the message request:

- `--blocks` / `--attachments` must be a JSON **array**
- `--metadata` must be a JSON **object** `{event_type: string, event_payload: object}` — both fields required

```bash
# Block Kit from file with fallback text for notifications
slack-cli send "#alerts" -t "Deploy v1.2.3 done" -b @blocks.json

# Correlation marker (event_type/event_payload) — survives `messages --json`
slack-cli send "#alerts" \
  -t "Deploy v1.2.3 done" \
  -m '{"event_type":"deploy_done","event_payload":{"version":"1.2.3"}}'

# Pipe from a generator
generate-blocks.sh | slack-cli send "#alerts" -t "fallback" -b -
```

## Channel identifiers

| Form | Resolves to |
|---|---|
| `#name`, `name` | Unique exact name in the local cache (case-insensitive) |
| `C…`, `G…` | Channel ID (public / private channel, MPIM) — passthrough |
| `D…` | DM channel ID — passthrough |
| `U…`, `W…` | User ID — resolves an existing DM with the operation's selected token; does not create a DM or switch senders |

Names resolve via the local cache; partial matches never select a target. If the name is unknown, run `slack-cli cache refresh` and retry. When a name is ambiguous, inspect `slack-cli channels <name> --json` and select the intended ID.

## JSON response shapes

slack-cli normalizes responses to simpler shapes than raw Slack API. Reach for these field names with `jq`:

- `users --json` → array. Fields filtered by config defaults (`id, name, real_name, email`) plus anything passed to `--expand` (`avatar, title, timezone, status, status_emoji, display_name, is_admin, is_bot, deleted`). Anything outside that union is absent.
- `channels --json` → array. Same field-filter model. Defaults `id, name, type, members`; `--expand` adds `topic, purpose, created, creator, is_member, is_archived, is_private`. Member count is `members`, not `num_members`. `type` is a display string (`Public`, `Private`, `DM`, `Group`) — not the API's `channel_types` tokens.
- `members --json` → array of user-id strings (`["U123", "U456", ...]`), not user objects.
- `messages --json` → `{messages: [...], next_cursor}` envelope. `next_cursor` is `null` on the last page; otherwise pass it back via `--cursor` to fetch the next page. Message objects are projected through the `messages_fields` whitelist. **Lean default**: `ts, user, bot_id, username, text, thread_ts, reply_count, subtype, metadata`. Use `--expand` to opt in to verbose fields (`blocks, attachments, reactions, edited, parent_user_id, reply_users, reply_users_count, latest_reply, channel, permalink`) or computed fields (`date, user_name`). Optional struct fields are omitted when absent.
- `thread --json` → bare array of message objects (paginates internally up to `--limit`); same field model as `messages`.
- `send --json`, `update --json` → `{channel, ts}`.
- `permalink --json` → `{permalink}`. Non-JSON output is the URL alone.
- `reactions --json` → `{channel, ts, reactions: [{name, count, users}]}`.
- `pins --json` → array of `{ts, text, ...}`.
- `bookmarks --json` (list) → array of `{id, channel_id, title, link, type, emoji?, date_created, date_updated}`.
- `bookmark --json` (add) → single object with the same shape.
- `emoji --json` → array of `{name, url, is_alias, alias_for}`. Iterate with `.[]`, do not subscript by emoji name.
- `search --json` → `{messages, files, channels, users}` object. Each `.messages[]` uses `message_ts`, `content`, `channel_id`, `channel_name`, `author_user_id`, `author_name`, `permalink` — **not** the regular `ts`/`text`/`user` shape.
- `cache stats --json` → `{users: N, channels: N}`.
- `search --capabilities --json` → `{is_ai_search_enabled}`. When false the workspace ranks by keyword whatever `--no-semantic` says.
- `auth status --json` → `{profile, active, method, workspace, client_id, tokens: {user, bot, app}, authorized_at}`. `app` is the Socket Mode app-level token and is `null` unless one has been registered. Each token is `{token, expires_at, renewable, scopes}` with `token` masked (`xoxp...abcd`) and `expires_at` null when no expiry is recorded. With no stored profiles, the response is `{profiles: []}`. On `auth status --verify`, success adds the live `auth.test` shape under `verified` (`team, team_id, user, user_id`, plus optional `url, bot_id, enterprise_id, enterprise_name, is_enterprise_install`). Verification failures instead appear as `verified.error` and the report still exits `0`; inspect that field. This command inspects stored profiles, not environment-token overrides.
- `auth profiles --json` → `{profiles: [{name, active, method, workspace, tokens}]}` where `tokens` lists which kinds are held.
- `auth scopes --json` → `{user: [...], bot: [...], app: [...]}`, the scopes to register on the Slack app, less anything `config.toml [auth].exclude_scopes` drops. `app` contains the Socket Mode scope (`connections:write`), separate from the CLI's user/bot installation request and therefore never excludable.

## Message metadata and retries

Slack lets every message carry a `{event_type, event_payload}` marker. `slack-cli` exposes it as a first-class field on input (`-m`) and output (`metadata` is in the lean message default). Metadata is a correlation marker, not a server-side idempotency key. Record completed event IDs and check previous results before retrying a send. A timeout may mean Slack accepted the message but the response was lost; reconcile that outcome before sending again. History reads are rate-limited, so do not poll history for every event.

`conversations.history` and `conversations.replies` always request `include_all_metadata=true`, so no extra flag is needed to see the field.

## `--expand` fields

| Domain | Fields |
|--------|--------|
| users | `avatar` `title` `timezone` `status` `status_emoji` `display_name` `is_admin` `is_bot` `deleted` |
| channels | `topic` `purpose` `created` `creator` `is_member` `is_archived` `is_private` `user` (DM peer's user id) |
| messages / thread | computed: `date` `user_name` · response: `blocks` `attachments` `reactions` `edited` `parent_user_id` `reply_users` `reply_users_count` `latest_reply` `channel` `permalink` |

## `search` filters

`assistant.search.context` (Slack Real-time Search). Auto-paginates up to `--limit`.

| Option | Default | Effect |
|--------|---------|--------|
| `--limit` | `10` | total cap, 1–100 |
| `--channel <id\|name>` | — | restrict to one channel |
| `--before <ts\|YYYY-MM-DD>` | — | upper time bound |
| `--after <ts\|YYYY-MM-DD>` | — | lower time bound |
| `--channel-types` | all | `public_channel,private_channel,mpim,im` |
| `--content-types` | `messages` | comma-separated |
| `--include-context` | off | surrounding messages |
| `--include-bots` | off | include bot-authored |
| `--include-deleted-users` | off | include deleted users |
| `--include-archived` | off | include archived channels |
| `--modifiers <expr>` | — | search modifiers, e.g. `"has:pin from:@alice"` |
| `--no-semantic` | off | keyword-only matching |
| `--sort` | `score` | or `timestamp` |
| `--sort-dir` | `desc` | or `asc` |
| `--capabilities` | — | report semantic-search availability instead of querying (takes no query) |

## Real-time events

A Socket Mode daemon (`slack-cli daemon run`, started by a human under launchd
or systemd) collects the events a rule matched — a mention, a reply in a thread
the user reacted to. **Reading them is the agent's job; running the daemon is
not.**

```bash
# Is anything collecting?  running:false means no daemon — say so, do not start one.
slack-cli daemon status --json | jq '.running'

# Read pending events using one stable consumer name.
# Acknowledge each event with `events ack --through <seq>` after handling it.
slack-cli events pull --consumer assistant --limit 20 --json
```

The first pull registers its consumer even when no events exist. Reuse the same
consumer name; an unacknowledged position holds spool pruning until ack, subject
to the configured age and size limits.

`events pull --json` emits **one JSON object per line** (NDJSON), not an array —
read it with `jq -c .` or a `while read` loop, never `jq '.[]'`.

Each line is `{schema: "slack-cli.event/1", id, seq, kind, source, channel, ts,
thread_ts, user, text, matched: [rule names], received_at}`. `kind` is
`message` / `reaction_added` / `reaction_removed`; `source` is `socket` (live)
or `backfill` (read back after a disconnect). `text` is absent when the
installation stores references only — fetch the body with
`slack-cli thread <channel> <thread_ts>` when you need it.

Persisted events can be delivered again after a restart, so deduplicate on
`id`. While an event remains in the log, push redelivery preserves its original `id` and `seq`. Replay lasts only while rows remain within `retention_days` and
`max_bytes`; the bounded input queue and disconnect recovery can lose events. `--ack` is the one place that flips: it marks
the batch as it is printed, so a crash between printing and acting loses it.
When replying, carry the id so a repeat is visible:

```bash
# No --ack here: the position moves only once the work has actually happened.
set -euo pipefail
batch=$(slack-cli events pull --consumer assistant --limit 20 --json)
while IFS= read -r event; do
    [ -n "$event" ] || continue
    ts=$(jq -er '.thread_ts // .ts' <<<"$event")
    ch=$(jq -er '.channel' <<<"$event")
    id=$(jq -er '.id' <<<"$event")
    seq=$(jq -er '.seq' <<<"$event")
    metadata=$(jq -cn --arg id "$id" \
        '{event_type:"assistant_reply",event_payload:{source_event:$id}}')
    slack-cli send "$ch" --thread "$ts" -t "on it" -m "$metadata"
    slack-cli events ack --consumer assistant --through "$seq"
done <<<"$batch"
```

`events pull` needs the installation to be storing events. If it fails saying
`events.mode = "stream"`, the daemon is streaming to a sink instead and there
is nothing to pull — report that rather than retrying.

> A message sent with a user token goes out **under the user's own name**, with
> no bot badge. Do not auto-send on someone's behalf unless they asked for
> exactly that; show a draft in the current conversation when sending has not been requested.

## Multi-workspace

`SLACK_USER_TOKEN` and `SLACK_BOT_TOKEN` override stored installation credentials even with `--profile`. Unset both before selecting a stored workspace. `SLACK_APP_TOKEN` is independent and can accompany a stored profile. Environment tokens are never renewed by the CLI. For rotating app tokens (`xoxe.xapp-`), a human or service must manage renewal and restart the daemon with the replacement; stored app-token login supports non-rotating `xapp-` only.

Event stores belong to an API endpoint, profile, workspace and user. Keep the
same profile and installation credentials for the daemon and its consumers.
Environment credentials are verified on first use; their identity binding then
allows local reads without Slack. A replacement token for the same owner keeps
the store, while a different workspace or user starts a separate one.

`slack-cli --profile <name> <command>` selects a stored workspace for one command when no environment user/bot tokens are set. The flag is global, position-independent.
