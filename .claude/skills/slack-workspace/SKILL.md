---
name: slack-workspace
version: 0.1.0
description: |
  Search Slack workspace data using the slack-cli tool. Find team members by name/email,
  search channels, read message history, check threads, send messages, and list channel members.
  Use this when the user asks to find colleagues, check Slack conversations, search messages,
  view channel info, send Slack messages, or mentions: Slack, workspace, teammates, channels,
  DMs, threads, message history, team communication.
allowed-tools: Bash
---

# Slack Workspace Query

## Commands

```bash
# Search
slack-cli users <query> [--limit 10]
slack-cli channels <query> [--limit 10]

# Messages
slack-cli messages <channel> [--limit 100] [--cursor <cursor>]
slack-cli thread <channel> <ts> [--limit 100]
slack-cli search <query> [--channel <name>] [--user <name>] [--limit 10]

# Actions
slack-cli send <channel> <text> [--thread <ts>]
slack-cli members <channel>

# Setup
slack-cli config init --bot-token xoxb-... [--user-token xoxp-...]
slack-cli cache refresh [users|channels|all]
```

## Identifiers

- `#general`, `general` → channel name
- `@alice`, `alice` → user mention
- `C0123...`, `D0123...`, `G0123...` → channel/DM/group ID

**Note**: `U0123...` (user IDs) invalid for channel commands.

## Token Requirements

- **Bot token** (`xoxb-`): users, channels, send, messages, thread, members
- **User token** (`xoxp-`): search

## Examples

```bash
# Find user
slack-cli users alice

# Get messages
slack-cli messages "#engineering" --limit 20

# Send message
slack-cli send "#ops" "Deploy complete"

# Reply in thread
slack-cli send "#dev" "Fixed!" --thread 1234567890.123456

# List members
slack-cli members "#leadership"

# Search messages (user token required)
slack-cli search "bug report" --channel "#dev"

# Refresh cache
slack-cli cache refresh
```

## Error Handling

If command fails:
- **Cache empty**: Run `slack-cli cache refresh`
- **Token error**: Verify with `slack-cli config show`
- **Search fails**: Requires user token (`xoxp-`), not bot token
