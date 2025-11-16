---
name: slack-workspace
version: 0.1.0
description: |
  Query Slack workspace data: search users by name/email, get channel messages,
  search channels, read thread replies, list members. Works with public/private
  channels, DMs, group messages. Use when searching Slack conversations, finding
  team members, checking message history, or user mentions Slack, workspace search,
  channels, threads, or team communication.
allowed-tools: Bash
---

# Slack Workspace Query

## Commands

```bash
# Search users
slack-cli users <query> [--limit 10]

# Search channels
slack-cli channels <query> [--limit 10]

# Get messages
slack-cli messages <channel> [--limit 100]

# Read thread
slack-cli thread <channel> <timestamp>

# List members
slack-cli members <channel>

# Search messages (requires user token)
slack-cli search <query> [--channel <name>] [--user <name>]

# Cache management
slack-cli cache refresh [users|channels|all]
slack-cli cache stats

# Config
slack-cli config show
slack-cli config init --bot-token <token>
```

## Key Facts

**Channel formats**: `#channel-name`, `@username`, or IDs (`C123...`, `U456...`)
**Cache**: Users/channels cached locally, messages fetched from API
**Search command**: Requires user token (`xoxp-`), not bot token
**Refresh**: `slack-cli cache refresh` updates user/channel cache

## Examples

```bash
# Get recent messages
slack-cli messages "#team-tech" --limit 20

# Find colleague
slack-cli users "john"

# Search workspace (needs user token)
slack-cli search "bug" --channel "#dev"

# Refresh stale cache
slack-cli cache refresh
```
