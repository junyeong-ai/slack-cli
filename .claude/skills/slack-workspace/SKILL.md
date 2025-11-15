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
slack users <query> [--limit 10]

# Search channels
slack channels <query> [--limit 10]

# Get messages
slack messages <channel> [--limit 100]

# Read thread
slack thread <channel> <timestamp>

# List members
slack members <channel>

# Search messages (requires user token)
slack search <query> [--channel <name>] [--user <name>]

# Cache management
slack cache refresh [users|channels|all]
slack cache stats

# Config
slack config show
slack config init --bot-token <token>
```

## Key Facts

**Channel formats**: `#channel-name`, `@username`, or IDs (`C123...`, `U456...`)
**Cache**: Users/channels cached locally, messages fetched from API
**Search command**: Requires user token (`xoxp-`), not bot token
**Refresh**: `slack cache refresh` updates user/channel cache

## Examples

```bash
# Get recent messages
slack messages "#team-tech" --limit 20

# Find colleague
slack users "john"

# Search workspace (needs user token)
slack search "bug" --channel "#dev"

# Refresh stale cache
slack cache refresh
```
