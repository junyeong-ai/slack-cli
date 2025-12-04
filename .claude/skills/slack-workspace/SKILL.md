---
name: slack-workspace
version: 0.1.0
description: |
  Execute Slack workspace queries via slack-cli. Search users/channels, read messages,
  send messages, manage threads. Use when working with Slack data or team communication.
allowed-tools: Bash
---

# slack-cli Command Reference

```bash
# Query (add -j for JSON output)
slack-cli users <query> [--limit N] [-j]
slack-cli channels <query> [--limit N] [-j]
slack-cli messages <channel> [--limit N] [--cursor <cursor>] [-j]
slack-cli thread <channel> <ts> [--limit N] [-j]
slack-cli search <query> [--channel <name>] [--user <name>] [--limit N] [-j]
slack-cli members <channel> [-j]

# Action
slack-cli send <channel> <text> [--thread <ts>]

# Management
slack-cli cache refresh [users|channels|all]
slack-cli config show|init|path|edit
```

## Channel Identifiers

`#general`, `general`, `C0123...`, `D0123...`, `G0123...` → valid
`U0123...` (user ID) → invalid for channel commands

## Token Scope

- `search` → requires user token (`xoxp-`)
- All others → bot token (`xoxb-`)

## Output Format: Slack mrkdwn

When sending messages via `slack-cli send`, use Slack mrkdwn syntax:

| Element | Syntax |
|---------|--------|
| Bold | `*text*` |
| Italic | `_text_` |
| Strike | `~text~` |
| Link | `<url\|text>` |
| List | `• item` (not `-`) |
| Nested | `  ◦ child` |
| Section | `*:emoji: Title*` |

**Critical**: GitHub Markdown (`**bold**`, `[text](url)`, `- item`) renders literally in Slack.
