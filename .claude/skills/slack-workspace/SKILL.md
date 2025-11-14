---
name: slack-workspace
version: 0.1.0
description: |
  Execute Slack workspace queries: find users by name or email, retrieve channel
  messages and conversation history, search channels, read thread replies, list
  channel members. Works with public/private channels, DMs, and group messages.

  Use when working with Slack data, searching team conversations, finding colleagues,
  checking message history, reading threads, or when user mentions slack channels,
  workspace search, team members, or message retrieval.
allowed-tools:
  - Bash
---

# Slack Workspace CLI

Binary: `slack`

## Core Commands

**Get messages**: `slack messages <channel> [--limit 100]`
**Find user**: `slack users <query> [--limit 10]`
**Search channels**: `slack channels <query> [--limit 10]`
**Check thread**: `slack thread <channel> <ts>`
**List members**: `slack members <channel>`
**Search workspace**: `slack search <query> [--channel] [--user]` (**user token REQUIRED**)
**Manage**: `slack cache refresh all` | `slack config show`

## Key Details

**Tokens**: Bot (default) | **User REQUIRED for search**
**Channel formats**: `#name`, `@username`, `C123...` (ID), `U456...` (user ID)
**Cache**: Users/channels cached (<10ms), messages via API
**Refresh cache**: `slack cache refresh all` (positional arg, no dashes)

## Example

```bash
# Most common: Get channel messages
slack messages "#team-tech" --limit 20

# Find user
slack users "john"

# Search (needs user token)
slack search "bug" --channel "#dev"
```

**Troubleshoot**: Empty results? Try `slack cache refresh`
