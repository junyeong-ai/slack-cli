# Slack CLI

[![CI](https://github.com/junyeong-ai/slack-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/slack-cli/actions)
[![Rust](https://img.shields.io/badge/rust-1.91.1%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)

> **English** | **[한국어](README.md)**

**Take full control of Slack from your terminal.** From messaging to reactions, pins, and bookmarks — do everything without opening a browser.

---

## Why Slack CLI?

- **Fast** — Millisecond searches powered by SQLite FTS5
- **Complete** — 21 commands covering all Slack features
- **Automatable** — Integrates with scripts, CI/CD, and AI agents

---

## Quick Start

```bash
# Install
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/install.sh | bash

# Configure
slack-cli config init --bot-token xoxb-your-token
slack-cli cache refresh

# Use
slack-cli users "john"
slack-cli send "#general" "Hello!"
```

---

## Key Features

### Messages
```bash
slack-cli send "#general" "Announcement"          # Send
slack-cli update "#general" 1234.5678 "Edited"    # Update
slack-cli delete "#general" 1234.5678             # Delete
slack-cli messages "#general" --limit 20          # List
slack-cli thread "#general" 1234.5678             # Thread
slack-cli search "keyword" --channel "#dev"       # Search
```

### Reactions
```bash
slack-cli react "#general" 1234.5678 thumbsup     # Add
slack-cli unreact "#general" 1234.5678 thumbsup   # Remove
slack-cli reactions "#general" 1234.5678          # List
```

### Pins & Bookmarks
```bash
slack-cli pin "#general" 1234.5678                # Pin
slack-cli unpin "#general" 1234.5678              # Unpin
slack-cli pins "#general"                         # List pins

slack-cli bookmark "#general" "Wiki" "https://..."  # Add bookmark
slack-cli bookmarks "#general"                      # List bookmarks
```

### Search & Query
```bash
slack-cli users "john" --limit 10                 # Search users
slack-cli channels "dev"                          # Search channels
slack-cli members "#dev-team"                     # List members
slack-cli emoji --query "party"                   # Search emoji
```

### Cache & Config
```bash
slack-cli cache stats                             # Check status
slack-cli cache refresh                           # Refresh
slack-cli config show                             # Show config
```

---

## Installation

### Automated Install (Recommended)
```bash
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/install.sh | bash
```

### Cargo
```bash
cargo install slack-cli
```

### Build from Source
```bash
git clone https://github.com/junyeong-ai/slack-cli && cd slack-cli
cargo build --release
```

**Requirements**: Rust 1.91.1+

---

## Slack Token Setup

### 1. Create App
[api.slack.com/apps](https://api.slack.com/apps) → Create New App → From scratch

### 2. Add Permissions

**User Token Scopes** (recommended):
```
channels:read  channels:history  groups:read  groups:history
im:read  im:history  mpim:read  mpim:history
users:read  users:read.email  chat:write  search:read
reactions:read  reactions:write  pins:read  pins:write
bookmarks:read  bookmarks:write  emoji:read
```

### 3. Install and Copy Token
Install to Workspace → Copy `xoxp-...` token

### 4. Configure CLI
```bash
slack-cli config init --user-token xoxp-your-token
```

---

## Configuration

### Environment Variables
```bash
export SLACK_USER_TOKEN="xoxp-..."
export SLACK_BOT_TOKEN="xoxb-..."
```

### Config File
`~/.config/slack-cli/config.toml`:
```toml
user_token = "xoxp-..."
bot_token = "xoxb-..."

[cache]
ttl_users_hours = 24
ttl_channels_hours = 24

[connection]
rate_limit_per_minute = 20
timeout_seconds = 30
```

**Priority**: CLI options > Environment variables > Config file

---

## Command Reference

| Command | Description |
|---------|-------------|
| `users <query>` | Search users |
| `channels <query>` | Search channels |
| `send <ch> <text>` | Send message |
| `update <ch> <ts> <text>` | Update message |
| `delete <ch> <ts>` | Delete message |
| `messages <ch>` | List messages |
| `thread <ch> <ts>` | List thread |
| `members <ch>` | List members |
| `search <query>` | Search messages |
| `react <ch> <ts> <emoji>` | Add reaction |
| `unreact <ch> <ts> <emoji>` | Remove reaction |
| `reactions <ch> <ts>` | List reactions |
| `emoji` | List emoji |
| `pin <ch> <ts>` | Pin message |
| `unpin <ch> <ts>` | Unpin message |
| `pins <ch>` | List pins |
| `bookmark <ch> <title> <url>` | Add bookmark |
| `unbookmark <ch> <id>` | Remove bookmark |
| `bookmarks <ch>` | List bookmarks |
| `cache stats/refresh` | Cache management |
| `config init/show` | Config management |

### Common Options
- `--json` — JSON output
- `--limit <N>` — Limit results
- `--thread <ts>` — Thread reply (send)

---

## Troubleshooting

### Reset Cache
```bash
rm -rf ~/.config/slack-cli/cache && slack-cli cache refresh
```

### Permission Errors
Check token scopes → Reinstall to Workspace

### Debug
```bash
RUST_LOG=debug slack-cli users "john"
```

---

## Support

- [GitHub Issues](https://github.com/junyeong-ai/slack-cli/issues)
- [Developer Guide](CLAUDE.md)

---

<div align="center">

**English** | **[한국어](README.md)**

Made with Rust

</div>
