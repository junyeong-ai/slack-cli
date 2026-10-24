---
name: slack-workspace
metadata:
  version: "0.20.2"
description: Searches, reads and posts Slack messages with the slack-cli command — channel history, threads, workspace search, users and channels, DMs, scheduled messages, file uploads, reactions, pins, bookmarks, and events collected by its Socket Mode daemon. Use when a task involves Slack, such as finding or summarizing a discussion, replying in a thread, posting, editing, deleting or scheduling a message, sending a DM, sharing a file, looking up a person or channel, or handling mentions.
allowed-tools: Bash(slack-cli users *), Bash(slack-cli channels *), Bash(slack-cli members *), Bash(slack-cli messages *), Bash(slack-cli thread *), Bash(slack-cli search *), Bash(slack-cli permalink *), Bash(slack-cli reactions *), Bash(slack-cli pins *), Bash(slack-cli bookmarks *), Bash(slack-cli emoji *), Bash(slack-cli scheduled *), Bash(slack-cli events pull *), Bash(slack-cli events ack *), Bash(slack-cli events stats *), Bash(slack-cli daemon status *), Bash(slack-cli cache *), Bash(slack-cli auth status *), Bash(slack-cli auth profiles *), Bash(slack-cli auth scopes *), Bash(jq *)
---

# slack-cli

## Idiom: `--json | jq`

Default output is human-formatted. Add `--json` whenever you need to parse or chain commands.

```bash
# Discover candidates; select the intended user ID explicitly
slack-cli users "john" --json | jq -r '.[] | "\(.id)\t\(.real_name)"'

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

Use `--json` for machine-readable stdout; event streams use NDJSON. In `--json` mode every stderr line is a JSON object: log records (`level`, `fields`), then on a runtime failure the `{"error": {"code", "message"}}` envelope as the last line. `code` is Slack's error string verbatim for API failures (`channel_not_found`, `missing_scope`, …), otherwise `auth_error` / `rate_limited` / `http_error` / `network_error` / `error`.

Usage errors are the one exception: they exit `2` from clap at parse time with plain diagnostic text, never the envelope — branch on the exit code alone.

Exit codes: `0` ok · `1` generic · `2` usage · `3` auth (inspect credentials, profile selection or permissions) · `4` rate-limited (back off before retrying).

`missing_scope` fails only the command whose Slack method needs the scope; its message names the scope, and other commands keep working. Granting a scope takes a person: it is registered on the Slack app and followed by a new `auth login`. Exit `3` can also mean missing app credentials, mismatched workspaces or a changed authorization; report the specific error rather than retrying.

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
# Lookups (cache-backed; the first lookup fetches the directory). An empty result can
# predate a new user or channel: run `slack-cli cache refresh users|channels` once and retry.
slack-cli users    (<query> | --id U1,U2) [--expand FIELDS] [--limit N] --json
slack-cli channels (<query> | --id C1,C2) [--expand FIELDS] [--limit N] --json
slack-cli members  <channel>
slack-cli dm       <user_id>[,<user_id>…] # 1 user: DM, 2–8: group DM; opens it if missing

# Reading
slack-cli messages <channel> [--limit N] [--cursor C] [--oldest DATE] [--latest DATE] [--exclude-bots] [--expand FIELDS] --json
slack-cli thread   <channel> <ts> [--limit N] [--cursor C] [--exclude-bots] [--expand FIELDS] --json
slack-cli search   <query>   [filters…] --json
slack-cli search   --capabilities --json
slack-cli permalink <channel> <ts> --json

# Writing  (≥1 of -t / --markdown-text / -b / -a is required)
slack-cli send   <channel> [-t TEXT] [--markdown-text MD] [-b BLOCKS] [-a ATTACHMENTS] [-m METADATA] [--thread <ts>] --json
slack-cli update <channel> <ts> [-t TEXT] [--markdown-text MD] [-b BLOCKS] [-a ATTACHMENTS] [-m METADATA] --json
slack-cli delete <channel> <ts>
slack-cli upload <channel> <path> [--title T] [--comment TEXT] [--thread <ts>] [--alt-text TEXT] --json
slack-cli schedule <channel> <at> [-t TEXT] [--markdown-text MD] [-b BLOCKS] [-a ATTACHMENTS] [--thread <ts>] --json
slack-cli scheduled [--channel <channel>] --json   # only what this token scheduled
slack-cli unschedule <channel> <scheduled_message_id>

# Reactions, pins, bookmarks, emoji
slack-cli react      <channel> <ts> <emoji>
slack-cli unreact    <channel> <ts> <emoji>
slack-cli reactions  <channel> <ts> --json
slack-cli pin        <channel> <ts>
slack-cli unpin      <channel> <ts>
slack-cli pins       <channel> --json
slack-cli bookmark   <channel> <title> <url> [--emoji <e>]
slack-cli unbookmark <channel> <bookmark_id>
slack-cli bookmarks  <channel> --json
slack-cli emoji [--query <q>] --json               # custom emoji only

# Collected events (requires a spool/archive log — see below)
slack-cli events pull [--consumer NAME] [--limit N] [--ack] --json
slack-cli events ack --consumer NAME --through <seq>
slack-cli events stats --json
slack-cli daemon status --json

# Cache
slack-cli cache refresh [users|channels|all]
slack-cli cache stats --json

# `self update` replaces the user's binary — a human setup step, not an
# agent action. Do not run it.
#
# `watch`, `daemon run` and `events pull --follow` run until killed. Never
# invoke them: read what a daemon already collected with `events pull`.

# Auth (read-only inspection; `auth login` is a human setup step)
slack-cli auth status [--verify] --json
slack-cli auth profiles --json
slack-cli auth scopes --json
```

`DATE` accepts a Unix timestamp or `YYYY-MM-DD`, which means local midnight. The upper bound (`--latest`, `search --before`) excludes it, so day D is `--oldest D --latest <the day after D>`.

## Reading a discussion

1. Resolve the channel: `channels <name> --json`, or `search` when the channel is unknown.
2. Page `messages <channel> --oldest … --latest … --json`, following `next_cursor` until it is `null`.
3. Page `thread <channel> <ts> --json` the same way for the messages whose `reply_count` matters.
4. Sort what you collected by `ts` before summarizing, and cite with `permalink`.

Under the default `app_distribution = "commercial_external"`, Slack serves
`messages` and `thread` 15 messages a call and one call a minute, so a long
history costs minutes. Narrow it with `--oldest`/`--latest`, or find the
messages with `search` first; exit `4` means wait before the next call.

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
| `#name`, `name` | Unique exact name of an active channel in the local cache (case-insensitive). An archived channel's name fails with its ID in the message; pass the ID |
| `C…`, `G…` | Channel ID (public / private channel, MPIM) — passthrough |
| `D…` | DM channel ID — passthrough |
| `U…`, `W…` | User ID — resolves an existing DM with the token the command uses; never creates one or switches senders. `slack-cli dm <user_id>` opens one with the token `send` uses and prints its ID |

Names resolve via the local cache; partial matches never select a target. An unknown name's error says whether the list was fetched during the command and which conversation types the token cannot list; refresh with `slack-cli cache refresh channels` only when it was not. When a name is ambiguous, inspect `slack-cli channels <name> --json` and select the intended ID.

## JSON response shapes

slack-cli normalizes responses to simpler shapes than raw Slack API. Reach for these field names with `jq`:

- `users --json` → array of the configured fields (default `id, name, real_name, email`) plus `--expand`. Nothing else is present.
- `channels --json` → array, same model (default `id, name, type, members`). Member count is `members`, not `num_members`. `type` is a display string (`Public`, `Private`, `DM`, `Group`) — not the API's `channel_types` tokens.
- `members --json` → array of user-id strings (`["U123", "U456", ...]`), not user objects.
- `dm --json` → `{channel}`.
- `upload --json` → `{id, title}`. Slack shares the file asynchronously, so it can take a moment to appear in the channel.
- `messages --json` → `{messages: [...], next_cursor}` envelope. `next_cursor` is `null` on the last page; otherwise pass it back via `--cursor` to fetch the next page. **Lean default** fields: `ts, user, bot_id, username, text, thread_ts, reply_count, subtype, files, metadata`, plus `--expand`. Fields a message lacks are omitted. `files` lists `{id, name, title, mimetype, size, mode, permalink}` per shared file; a message that only shares a file has empty `text`, and `mode: "tombstone"` marks a deleted one.
- `thread --json` → the same envelope. Only the first page leads with the parent; `next_cursor` continues toward earlier replies, and `--limit` counts replies. A non-null `next_cursor` means the thread continues: collect every page and sort by `ts` before summarizing.
- `send --json`, `update --json`, `delete --json` → `{channel, ts}`.
- `react`, `unreact`, `pin`, `unpin`, `unbookmark`, `unschedule` with `--json` → `{ok: true}`.
- `cache refresh --json` → `{ok: true, unreadable: [{type, scope}]}`. Each listed conversation type was skipped because the token lacks `scope`; names of that type cannot resolve.
- `schedule --json` → `{channel, scheduled_message_id, post_at}`. `scheduled --json` → array of `{id, channel_id, post_at, date_created, text}`.
- `permalink --json` → `{permalink}`. Non-JSON output is the URL alone.
- `reactions --json` → `{channel, ts, reactions: [{name, count, users}]}`.
- `pins --json` → array of `{ts, text, ...}`.
- `bookmarks --json` (list) → array of `{id, channel_id, title, link, type, emoji?, date_created, date_updated}`.
- `bookmark --json` (add) → single object with the same shape.
- `emoji --json` → array of `{name, url, is_alias, alias_for}`. Iterate with `.[]`, do not subscript by emoji name.
- `search --json` → `{messages, files, channels, users}` object. Each `.messages[]` uses `message_ts`, `content`, `channel_id`, `channel_name`, `author_user_id`, `author_name`, `permalink` — **not** the regular `ts`/`text`/`user` shape.
- `cache stats --json` → `{users: N, channels: N}`.
- `search --capabilities --json` → `{is_ai_search_enabled}`. When false the workspace ranks by keyword whatever `--no-semantic` says.
- `auth status --json` → `{profile, active, method, workspace, tokens: {user, bot, app}, …}`; each token is masked, and `null` when the profile does not hold that kind. `--verify` adds the live `auth.test` result under `verified`; a failure appears as `verified.error` and still exits `0`. It reports the stored profile, not environment-token overrides.
- `auth profiles --json` → `{profiles: [{name, active, method, workspace, tokens}]}` where `tokens` lists which kinds are held.
- `auth scopes --json` → `{user: [...], bot: [...], app: [...]}`, the scopes to register on the Slack app, less what `config.toml [auth].exclude_scopes` drops.

## Sending later

A session that ends cannot send later by itself, so hand the time to Slack: `slack-cli schedule <channel> <at>`, where `<at>` is Unix seconds or RFC 3339 with an offset (`2026-10-25T09:00:00+09:00`), at most 120 days ahead. Scheduled messages cannot carry `--metadata`. `unschedule` cancels until 60 seconds before posting.

## Message metadata and retries

Slack lets every message carry a `{event_type, event_payload}` marker. `slack-cli` exposes it as a first-class field on input (`-m`) and output (`metadata` is in the lean message default). Metadata is a correlation marker, not a server-side idempotency key. Record completed event IDs and check previous results before retrying a send. A timeout may mean Slack accepted the message but the response was lost; reconcile that outcome before sending again. History reads are rate-limited, so do not poll history for every event.

## `--expand` fields

A name outside this table exits `2` and lists the valid names.

| Domain | Fields |
|--------|--------|
| users | `avatar` `title` `timezone` `status` `status_emoji` `display_name` `is_admin` `is_bot` `deleted` |
| channels | `topic` `purpose` `created` `creator` `is_member` `is_archived` `is_private` `user` (DM peer's user id) |
| messages / thread | computed: `date` `user_name` · response: `blocks` `attachments` `reactions` `edited` `parent_user_id` `reply_users` `reply_users_count` `latest_reply` `channel` `permalink` |

## `search`

Slack's Real-time Search, paged up to `--limit` (1–100, default 10). Bots, deleted users and archived channels are left out unless `--include-bots`, `--include-deleted-users` or `--include-archived` is given. `--modifiers "has:pin from:@alice"` passes Slack search modifiers, and `--sort timestamp` orders by time instead of relevance. `slack-cli search --help` lists the remaining filters.

## Real-time events

A Socket Mode daemon (`slack-cli daemon run`, started by a human under launchd
or systemd) collects the events a rule matched — a mention, a reply in a thread
the user reacted to. **Reading them is the agent's job; running the daemon is
not.**

```bash
# Check collection. A stopped daemon adds no new events; retained events remain readable.
slack-cli daemon status --json | jq '.running'

# Read pending events using one stable consumer name.
# Acknowledge each event with `events ack --through <seq>` after handling it.
slack-cli events pull --consumer assistant --limit 20 --json
```

When `running` is false, report that collection has stopped and read the
retained events; starting a daemon is a person's job.

The first pull registers its consumer even when no events exist. Reuse the same
consumer name; an unacknowledged position holds spool pruning until ack, subject
to the configured age and size limits.

`events pull --json` emits **one JSON object per line** (NDJSON), not an array —
read it with `jq -c .` or a `while read` loop, never `jq '.[]'`.

Each line is `{schema: "slack-cli.event/1", id, seq, kind, source, channel, ts,
thread_ts, user, subtype, text, reaction, matched: [rule names], received_at,
…}`. `kind` is `message` / `reaction_added` / `reaction_removed`; a reaction
carries `reaction` and `item_user`. `subtype` is Slack's own: absent for a plain
message; `file_share`, `thread_broadcast` and `me_message` for one a person
posted with a file, sent from a thread to the channel, or wrote with `/me`;
`message_changed` and `message_deleted` for an edit and a deletion;
`bot_message` for an integration's post; and a notice such as `channel_join`
otherwise. `source` is
`socket` (live) or `backfill` (read back after a disconnect). `text` is absent
unless the installation sets `events.store_body = true`; fetch the conversation
with `slack-cli thread <channel> <thread_ts // ts>` when you need it.

Events can be delivered again after a restart, so deduplicate on `id`. Pull
without `--ack` and acknowledge with `events ack --through <seq>` once the work
is done: a crash then repeats an event instead of losing it, whereas `--ack`
marks the batch as it is printed. Events are kept only within the configured
`retention_days` and `max_bytes`.

When the user has asked you to answer what arrives, reply only to new messages
and carry the event id so a repeat is visible:

```bash
set -euo pipefail
# What someone wrote: plain, with a file, or a thread reply also sent to the
# channel. Edits, deletions, reactions and channel notices match rules too.
written='.kind == "message" and ((.subtype // "") | IN("", "file_share", "thread_broadcast", "me_message"))'
batch=$(slack-cli events pull --consumer assistant --limit 20 --json)
while IFS= read -r event; do
    [ -n "$event" ] || continue
    seq=$(jq -er '.seq' <<<"$event")
    if jq -e "$written" <<<"$event" >/dev/null; then
        ts=$(jq -er '.thread_ts // .ts' <<<"$event")
        ch=$(jq -er '.channel' <<<"$event")
        id=$(jq -er '.id' <<<"$event")
        metadata=$(jq -cn --arg id "$id" \
            '{event_type:"assistant_reply",event_payload:{source_event:$id}}')
        slack-cli send "$ch" --thread "$ts" -t "on it" -m "$metadata"
    fi
    slack-cli events ack --consumer assistant --through "$seq"
done <<<"$batch"
```

`events pull` needs the installation to be storing events. If it fails saying
`events.mode = "stream"`, the daemon is streaming to a sink instead and there
is nothing to pull — report that rather than retrying.

> A command uses the profile's bot token when it holds one, otherwise the user
> token, except that conversation reads (`messages`, `thread`, `members`, the
> channel directory) prefer the user token and `search` requires it (`auth
> status --json` → `tokens` shows which are held). A read made with the bot
> token can be refused in a conversation the bot cannot see. A message sent with
> a user token goes out **under the user's own name**, with no bot badge. Do not
> send on someone's behalf unless they asked for exactly that; show a draft in
> the current conversation when sending has not been requested.

## Multi-workspace

`SLACK_USER_TOKEN` and `SLACK_BOT_TOKEN` override the profile's credentials, even with `--profile`; unset both before selecting a profile. A `.env` file in the working directory, or the nearest parent directory that has one, fills in whatever the environment does not set, so one that sets them takes over without a word. `SLACK_APP_TOKEN` is independent and can accompany a profile.

`slack-cli <command> … --profile <name>` selects a profile for one command when no environment user/bot tokens are set; `SLACK_PROFILE` does the same for every command. Put the flag after the command: the pre-approved patterns start with `slack-cli <command>`, so `slack-cli --profile <name> <command>` asks for permission every time.

Event stores belong to an API endpoint, profile, workspace and user: keep the same profile for the daemon and its consumers.
