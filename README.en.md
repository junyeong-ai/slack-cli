# Slack CLI

[![CI](https://github.com/junyeong-ai/slack-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/slack-cli/actions)
[![Rust](https://img.shields.io/badge/rust-1.91.1%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![DeepWiki](https://img.shields.io/badge/DeepWiki-junyeong--ai%2Fslack--cli-blue.svg?logo=data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACwAAAAyCAYAAAAnWDnqAAAAAXNSR0IArs4c6QAAA05JREFUaEPtmUtyEzEQhtWTQyQLHNak2AB7ZnyXZMEjXMGeK/AIi+QuHrMnbChYY7MIh8g01fJoopFb0uhhEqqcbWTp06/uv1saEDv4O3n3dV60RfP947Mm9/SQc0ICFQgzfc4CYZoTPAswgSJCCUJUnAAoRHOAUOcATwbmVLWdGoH//PB8mnKqScAhsD0kYP3j/Yt5LPQe2KvcXmGvRHcDnpxfL2zOYJ1mFwrryWTz0advv1Ut4CJgf5uhDuDj5eUcAUoahrdY/56ebRWeraTjMt/00Sh3UDtjgHtQNHwcRGOC98BJEAEymycmYcWwOprTgcB6VZ5JK5TAJ+fXGLBm3FDAmn6oPPjR4rKCAoJCal2eAiQp2x0vxTPB3ALO2CRkwmDy5WohzBDwSEFKRwPbknEggCPB/imwrycgxX2NzoMCHhPkDwqYMr9tRcP5qNrMZHkVnOjRMWwLCcr8ohBVb1OMjxLwGCvjTikrsBOiA6fNyCrm8V1rP93iVPpwaE+gO0SsWmPiXB+jikdf6SizrT5qKasx5j8ABbHpFTx+vFXp9EnYQmLx02h1QTTrl6eDqxLnGjporxl3NL3agEvXdT0WmEost648sQOYAeJS9Q7bfUVoMGnjo4AZdUMQku50McDcMWcBPvr0SzbTAFDfvJqwLzgxwATnCgnp4wDl6Aa+Ax283gghmj+vj7feE2KBBRMW3FzOpLOADl0Isb5587h/U4gGvkt5v60Z1VLG8BhYjbzRwyQZemwAd6cCR5/XFWLYZRIMpX39AR0tjaGGiGzLVyhse5C9RKC6ai42ppWPKiBagOvaYk8lO7DajerabOZP46Lby5wKjw1HCRx7p9sVMOWGzb/vA1hwiWc6jm3MvQDTogQkiqIhJV0nBQBTU+3okKCFDy9WwferkHjtxib7t3xIUQtHxnIwtx4mpg26/HfwVNVDb4oI9RHmx5WGelRVlrtiw43zboCLaxv46AZeB3IlTkwouebTr1y2NjSpHz68WNFjHvupy3q8TFn3Hos2IAk4Ju5dCo8B3wP7VPr/FGaKiG+T+v+TQqIrOqMTL1VdWV1DdmcbO8KXBz6esmYWYKPwDL5b5FA1a0hwapHiom0r/cKaoqr+27/XcrS5UwSMbQAAAABJRU5ErkJggg==)](https://deepwiki.com/junyeong-ai/slack-cli)

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
slack-cli messages "#general" --oldest 2025-01-01 --latest 2025-01-31  # Date filter
slack-cli messages "#general" --exclude-bots      # Exclude bot messages
slack-cli messages "#general" --expand date,user_name  # Expand date/name
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
slack-cli users --id U123,U456                    # Lookup by IDs
slack-cli users "john" --expand avatar,title      # Include extra fields
slack-cli channels "dev"                          # Search channels
slack-cli channels --id C123,C456                 # Lookup by IDs
slack-cli channels "dev" --expand topic,purpose   # Include extra fields
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
ttl_users_hours = 168          # 1 week
ttl_channels_hours = 168
refresh_threshold_percent = 10 # Background refresh at 10% of TTL

[output]
users_fields = ["id", "name", "real_name", "email"]
channels_fields = ["id", "name", "type", "members"]

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
| `users --id <ids>` | Lookup by IDs (comma-separated) |
| `channels <query>` | Search channels |
| `channels --id <ids>` | Lookup by IDs (comma-separated) |
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
- `--expand <fields>` — Extra fields (users/channels/messages)
  - users: `avatar`, `title`, `timezone`, `status`, `is_admin`, `is_bot`, `deleted`
  - channels: `topic`, `purpose`, `created`, `creator`, `is_archived`, `is_private`
  - messages: `date`, `user_name`

### messages Options
- `--oldest <date>` — Start time (Unix timestamp or YYYY-MM-DD)
- `--latest <date>` — End time (Unix timestamp or YYYY-MM-DD)
- `--exclude-bots` — Exclude bot messages

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
