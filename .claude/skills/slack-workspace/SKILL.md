---
name: slack-workspace
# version is consumed by scripts/install.sh upgrade comparison — bump with the crate
version: 0.4.0
description: Drive a Slack workspace from the terminal via slack-cli. Use when the user wants to send/edit/delete messages, search Slack history, look up users or channels by name, read threads, add reactions, pin or bookmark messages, or list members in a channel.
allowed-tools: Bash
---

# slack-cli

## Idiom: `--json | jq`

Default output is human-formatted. Add `--json` whenever you need to parse or chain commands.

```bash
# User ID from a name
slack-cli users "john" --json | jq -r '.[0].id'

# Channel ID, then send
ch=$(slack-cli channels "general" --json | jq -r '.[0].id')
slack-cli send "$ch" "Hello"

# Send a parent message, then reply in the thread
ts=$(slack-cli send "#general" "Parent" --json | jq -r '.ts')
slack-cli send "#general" "Reply" --thread "$ts"

# Search workspace content (Real-time Search API)
slack-cli search "deploy plan changes" --sort timestamp --json
```

## Slack mrkdwn — translate before sending

When the user dictates a message in Markdown, convert before passing to `send` / `update`. Markdown syntax renders literally in Slack (e.g. `**bold**` shows the asterisks).

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

# Messages
slack-cli messages <channel> [--limit N] [--oldest DATE] [--latest DATE] [--exclude-bots] [--expand date,user_name] --json
slack-cli thread   <channel> <ts> --json
slack-cli search   <query>   [filters…] --json
slack-cli send     <channel> <text> [--thread <ts>]      # returns {ts, channel}
slack-cli update   <channel> <ts>   <text>
slack-cli delete   <channel> <ts>

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

## Channel identifiers

`#name` · `name` · `C…` (public/private channel) · `D…` (DM) · `G…` (private channel or MPIM)

Names resolve via the local cache. If a lookup says the name is unknown, run `slack-cli cache refresh` and retry.

## JSON response shapes

slack-cli normalizes responses to simpler shapes than raw Slack API. Reach for these field names with `jq`:

- `users --json` → array of `{id, name, real_name, email, ...}`
- `channels --json` → array of `{id, name, type, members}` (member count is `members`, not `num_members`)
- `messages --json`, `thread --json` → array of message objects with `ts`, `user`, `text`, `bot_id`, optional `reply_count`
- `send --json` → `{ts, channel}`
- `reactions --json` → `{reactions: [{name, count, users}]}`
- `pins --json`, `bookmarks --json` → arrays
- `emoji --json` → `{name: url}` map
- `search --json` → `{messages, files, channels, users}` object. Each message uses `message_ts`, `content`, `channel_id`, `channel_name`, `author_user_id`, `author_name`, `permalink` — **not** the regular `ts`/`text`/`user` shape

## `--expand` fields

| Domain | Fields |
|--------|--------|
| users | `avatar` `title` `timezone` `status` `status_emoji` `display_name` `is_admin` `is_bot` `deleted` |
| channels | `topic` `purpose` `created` `creator` `is_member` `is_archived` `is_private` |
| messages | `date` `user_name` |

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
