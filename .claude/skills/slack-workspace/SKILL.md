---
name: slack-workspace
version: 0.1.0
description: |
  Execute Slack workspace queries via slack-cli. Search users/channels, send/update/delete messages,
  add reactions, pin messages, manage bookmarks, list emoji. Use when working with Slack data,
  team communication, or automating Slack workflows.
allowed-tools: Bash
---

# slack-cli

**Use `--json` for parsing.** Combine with `jq` for extraction.

```bash
# Get user ID
slack-cli users "john" --json | jq -r '.[0].id'

# Get channel ID and send message
ch=$(slack-cli channels "general" --json | jq -r '.[0].id')
slack-cli send "$ch" "Hello"

# Send and capture timestamp for thread reply
ts=$(slack-cli send "#general" "Parent" --json | jq -r '.ts')
slack-cli send "#general" "Reply" --thread "$ts"
```

## Commands

```bash
# Users & Channels
slack-cli users <query> [--id U1,U2] [--expand <fields>] [--limit N] --json
slack-cli channels <query> [--id C1,C2] [--expand <fields>] [--limit N] --json

# Messages
slack-cli messages <channel> [--limit N] [--oldest DATE] [--latest DATE] [--exclude-bots] [--expand FIELDS] --json
slack-cli thread <channel> <ts> --json
slack-cli search <query> [--channel C] [--user U] --json
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

## --expand Fields

| Type | Fields |
|------|--------|
| users | `avatar`, `title`, `timezone`, `status`, `status_emoji`, `display_name`, `is_admin`, `is_bot`, `deleted` |
| channels | `topic`, `purpose`, `created`, `creator`, `is_member`, `is_archived`, `is_private` |
| messages | `date`, `user_name` |

## Channel Format

`#name`, `name`, `C0123...` (public), `D0123...` (DM), `G0123...` (group)

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
