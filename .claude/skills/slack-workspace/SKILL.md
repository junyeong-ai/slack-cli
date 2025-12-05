---
name: slack-workspace
version: 0.1.0
description: |
  Execute Slack workspace queries via slack-cli. Search users/channels, send/update/delete messages,
  add reactions, pin messages, manage bookmarks, list emoji. Use when working with Slack data,
  team communication, or automating Slack workflows.
allowed-tools: Bash
---

# slack-cli Commands

```bash
# Search & Query
slack-cli users <query> [--limit N] [-j]
slack-cli channels <query> [--limit N] [-j]
slack-cli messages <channel> [--limit N] [-j]
slack-cli thread <channel> <ts> [--limit N] [-j]
slack-cli search <query> [--channel <ch>] [--user <u>] [--limit N] [-j]
slack-cli members <channel> [-j]
slack-cli emoji [--query <q>] [-j]

# Messages
slack-cli send <channel> <text> [--thread <ts>]
slack-cli update <channel> <ts> <text>
slack-cli delete <channel> <ts>

# Reactions
slack-cli react <channel> <ts> <emoji>
slack-cli unreact <channel> <ts> <emoji>
slack-cli reactions <channel> <ts> [-j]

# Pins
slack-cli pin <channel> <ts>
slack-cli unpin <channel> <ts>
slack-cli pins <channel> [-j]

# Bookmarks
slack-cli bookmark <channel> <title> <url> [--emoji <e>]
slack-cli unbookmark <channel> <bookmark_id>
slack-cli bookmarks <channel> [-j]

# Cache & Config
slack-cli cache refresh [users|channels]
slack-cli cache stats
slack-cli config show|init
```

## Channel Format

Valid: `#general`, `general`, `C0123...`, `D0123...`, `G0123...`

## Slack mrkdwn Syntax

When sending messages, use Slack mrkdwn (not GitHub Markdown):

| Element | Syntax |
|---------|--------|
| Bold | `*text*` |
| Italic | `_text_` |
| Strike | `~text~` |
| Code | `` `code` `` |
| Link | `<url\|label>` |
| User | `<@U123>` |
| Channel | `<#C123>` |
| List | `• item` (bullet, not `-`) |

**Critical**: `**bold**`, `[link](url)`, `- list` render literally in Slack.
