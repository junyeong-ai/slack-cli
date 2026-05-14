---
name: slack-workspace
# Consumed by scripts/install.sh for upgrade comparison; bump with the crate version.
version: 0.4.0
description: |
  Execute Slack workspace workflows via slack-cli. Search Slack context with the Real-time Search API,
  look up users/channels from the local cache, send/update/delete messages, add reactions, pin messages,
  manage bookmarks, and list emoji.
allowed-tools: Bash
---

# slack-cli

**Use `--json` for machine parsing.** Combine with `jq` for extraction.

```bash
# Get user ID
slack-cli users "john" --json | jq -r '.[0].id'

# Get channel ID and send a message
ch=$(slack-cli channels "general" --json | jq -r '.[0].id')
slack-cli send "$ch" "Hello"

# Send and capture timestamp for thread reply
ts=$(slack-cli send "#general" "Parent" --json | jq -r '.ts')
slack-cli send "#general" "Reply" --thread "$ts"

# Search Slack context
slack-cli search "What changed in the deploy plan?" --sort timestamp --json
```

## Commands

```bash
# Users & Channels
slack-cli users <query> [--id U1,U2] [--expand <fields>] [--limit N] --json
slack-cli channels <query> [--id C1,C2] [--expand <fields>] [--limit N] --json

# Messages
slack-cli messages <channel> [--limit N] [--oldest DATE] [--latest DATE] [--exclude-bots] [--expand FIELDS] --json
slack-cli thread <channel> <ts> --json
slack-cli search <query> [--limit N] [--channel <ch>] [--before DATE] [--after DATE] [--channel-types TYPES] [--content-types TYPES] [--include-context] [--include-bots] [--include-archived] [--no-semantic] [--sort score|timestamp] [--sort-dir asc|desc] --json
slack-cli send <channel> <text> [--thread <ts>]    # returns {ts, channel}
slack-cli update <channel> <ts> <text>
slack-cli delete <channel> <ts>

# DATE: Unix timestamp or YYYY-MM-DD
# --expand for messages: date, user_name

# Reactions & Pins
slack-cli react <channel> <ts> <emoji>
slack-cli unreact <channel> <ts> <emoji>
slack-cli reactions <channel> <ts> --json
slack-cli pin/unpin <channel> <ts>
slack-cli pins <channel> --json

# Bookmarks
slack-cli bookmark <channel> <title> <url> [--emoji <e>]
slack-cli unbookmark <channel> <bookmark_id>
slack-cli bookmarks <channel> --json

# Cache
slack-cli cache refresh [users|channels|all]
slack-cli cache stats --json
```

## Search

`slack-cli search` uses Slack's Real-time Search API (`assistant.search.context`). It requires a user token for CLI use outside the Slack client. Use granular search scopes as needed: `search:read.public`, `search:read.private`, `search:read.im`, `search:read.mpim`, `search:read.files`, `search:read.users`.

Defaults:

| Option | Default |
|--------|---------|
| `--limit` | `10` (1-100, paginated across 20-result pages) |
| `--channel-types` | `public_channel,private_channel,mpim,im` |
| `--content-types` | `messages` |
| `--sort` | `score` |
| `--sort-dir` | `desc` |

Filters:

| Option | Effect |
|--------|--------|
| `--channel <id\|name>` | Restrict to one channel (resolved via cache) |
| `--before <ts\|YYYY-MM-DD>` | Only results before this instant |
| `--after <ts\|YYYY-MM-DD>` | Only results after this instant |
| `--include-archived` | Include archived channels |
| `--no-semantic` | Force keyword-only matching (skip the API's automatic semantic mode) |

## --expand Fields

| Type | Fields |
|------|--------|
| users | `avatar`, `title`, `timezone`, `status`, `status_emoji`, `display_name`, `is_admin`, `is_bot`, `deleted` |
| channels | `topic`, `purpose`, `created`, `creator`, `is_member`, `is_archived`, `is_private` |
| messages | `date`, `user_name` |

## Channel Format

`#name`, `name`, `C...` (public/private channel), `D...` (DM), `G...` (private channel or MPIM)

## Slack mrkdwn (NOT Markdown)

| Element | Slack Syntax | Wrong |
|---------|--------------|-------|
| Bold | `*text*` | `**text**` |
| Italic | `_text_` | `*text*` |
| Link | `<url\|label>` | `[label](url)` |
| User mention | `<@U123>` | `@user` |
| Channel mention | `<#C123>` | `#channel` |
| List item | `• item` | `- item` |

**Critical**: Markdown syntax renders literally in Slack.
