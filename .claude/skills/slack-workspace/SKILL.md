---
name: slack-workspace
# version is not part of the official skill frontmatter; scripts/install.sh
# uses it for upgrade comparison — bump with the crate
version: 0.6.0
description: Drive a Slack workspace from the terminal via slack-cli. Use when the user wants to send/edit/delete messages (plain text, Markdown, or Block Kit), search Slack history, look up users or channels by name, read threads or paginated channel history, add reactions, pin or bookmark messages, fetch a message permalink, or attach message metadata for idempotent notifications.
allowed-tools: Bash(slack-cli *), Bash(jq *)
---

# slack-cli

## Idiom: `--json | jq`

Default output is human-formatted. Add `--json` whenever you need to parse or chain commands.

```bash
# User ID from a name
slack-cli users "john" --json | jq -r '.[0].id'

# Channel ID, then send
ch=$(slack-cli channels "general" --json | jq -r '.[0].id')
slack-cli send "$ch" -t "Hello"

# Send a parent message, then reply in the thread
ts=$(slack-cli send "#general" -t "Parent" --json | jq -r '.ts')
slack-cli send "#general" -t "Reply" --thread "$ts"

# Page through channel history until next_cursor is null
page=$(slack-cli messages "#general" --json)
echo "$page" | jq '.messages[]'
cursor=$(echo "$page" | jq -r '.next_cursor // empty')
[ -n "$cursor" ] && slack-cli messages "#general" --cursor "$cursor" --json

# Search workspace content (Real-time Search API)
slack-cli search "deploy plan changes" --sort timestamp --json
```

## Errors and exit codes

stdout is always parseable data or empty. Failures print to stderr — in `--json` mode as an `{"error": {"code", "message"}}` envelope. `code` is Slack's error string verbatim for API failures (`channel_not_found`, `missing_scope`, …), otherwise `auth_error` / `rate_limited` / `http_error` / `network_error` / `error`.

Exit codes: `0` ok · `1` generic · `2` usage · `3` auth (re-run `slack-cli auth login`) · `4` rate-limited (back off before retrying).

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
slack-cli permalink <channel> <ts> --json

# Writing  (≥1 of -t / --markdown-text / -b / -a is required)
slack-cli send   <channel> [-t TEXT] [--markdown-text MD] [-b BLOCKS] [-a ATTACHMENTS] [-m METADATA] [--thread <ts>] --json
slack-cli update <channel> <ts> [-t TEXT] [--markdown-text MD] [-b BLOCKS] [-a ATTACHMENTS] [-m METADATA] --json
slack-cli delete <channel> <ts>

# Reactions, pins, bookmarks, emoji
slack-cli react      <channel> <ts> <emoji>
slack-cli unreact    <channel> <ts> <emoji>
slack-cli reactions  <channel> <ts> --json
slack-cli pin   | unpin   <channel> <ts>
slack-cli pins  <channel> --json
slack-cli bookmark   <channel> <title> <url> [--emoji <e>]
slack-cli unbookmark <channel> <bookmark_id>
slack-cli bookmarks  <channel> --json
slack-cli emoji [--query <q>] --json

# Cache
slack-cli cache refresh [users|channels|all]
slack-cli cache stats --json
```

`DATE` accepts a Unix timestamp or `YYYY-MM-DD`.

## JSON sources for `--blocks`, `--attachments`, `--metadata`

All three flags share one input vocabulary:

| Form | Meaning |
|---|---|
| `-` | Read JSON from stdin (at most **one** flag per invocation) |
| `@path.json` | Read JSON from a file |
| anything else | Inline JSON literal |

Shape is validated **before** any HTTP call:

- `--blocks` / `--attachments` must be a JSON **array**
- `--metadata` must be a JSON **object** `{event_type: string, event_payload: object}` — both fields required

```bash
# Block Kit from file with fallback text for notifications
slack-cli send "#alerts" -t "Deploy v1.2.3 done" -b @blocks.json

# Idempotent marker (event_type/event_payload) — survives `messages --json`
slack-cli send "#alerts" \
  -t "Deploy v1.2.3 done" \
  -m '{"event_type":"deploy_done","event_payload":{"version":"1.2.3"}}'

# Pipe from a generator
generate-blocks.sh | slack-cli send "#alerts" -t "fallback" -b -
```

## Channel identifiers

| Form | Resolves to |
|---|---|
| `#name`, `name` | Channel matched by cache lookup |
| `C…`, `G…` | Channel ID (public / private channel, MPIM) — passthrough |
| `D…` | DM channel ID — passthrough |
| `U…`, `W…` | User ID — auto-resolves to that user's DM channel via cache (`channel_types` must include `im` and cache must be refreshed) |

Names resolve via the local cache. If a lookup says the name is unknown, run `slack-cli cache refresh` and retry.

## JSON response shapes

slack-cli normalizes responses to simpler shapes than raw Slack API. Reach for these field names with `jq`:

- `users --json` → array. Fields filtered by config defaults (`id, name, real_name, email`) plus anything passed to `--expand` (`avatar, title, timezone, status, status_emoji, display_name, is_admin, is_bot, deleted`). Anything outside that union is absent.
- `channels --json` → array. Same field-filter model. Defaults `id, name, type, members`; `--expand` adds `topic, purpose, created, creator, is_member, is_archived, is_private`. Member count is `members`, not `num_members`.
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
- `auth status --json` / `auth profiles --json` → metadata about stored profile(s); tokens are always masked (`xoxp...abcd`). On `auth status --verify`, the `verified` object echoes the live `auth.test` shape (`team, team_id, user, user_id`, plus optional `url, bot_id, enterprise_id, enterprise_name, is_enterprise_install`).

## Message metadata (idempotency)

Slack lets every message carry a `{event_type, event_payload}` marker. `slack-cli` exposes it as a first-class field on input (`-m`) and output (`metadata` is in the lean message default). Use it when a job may retry: read recent history with `messages --json | jq '.messages[].metadata'`, dedupe by your own key inside `event_payload`, skip re-sending.

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
| `--include-archived` | off | include archived channels |
| `--no-semantic` | off | keyword-only matching |
| `--sort` | `score` | or `timestamp` |
| `--sort-dir` | `desc` | or `asc` |

## Multi-workspace

`slack-cli --profile <name> <command>` runs a single command against a specific stored workspace. The flag is global, position-independent.
