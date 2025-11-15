# Slack CLI

[![CI](https://github.com/junyeong-ai/slack-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/slack-cli/actions)
[![Lint](https://github.com/junyeong-ai/slack-cli/workflows/Lint/badge.svg)](https://github.com/junyeong-ai/slack-cli/actions)
[![Rust](https://img.shields.io/badge/rust-1.91.1%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-0.1.0-blue?style=flat-square)](https://github.com/junyeong-ai/slack-cli/releases)

> **🌐 [한국어](README.md)** | **English**

---

> **⚡ Fast and Powerful Slack Command-Line Tool**
>
> - 🚀 **Millisecond searches** (SQLite FTS5 full-text search)
> - 💾 **Local cache** (instant user/channel queries)
> - 🔍 **Fuzzy matching** (typo-tolerant search)
> - 🛠️ **9 commands** (search, messaging, config management)

---

## ⚡ Quick Start (1 minute)

```bash
# 1. Install
git clone https://github.com/junyeong-ai/slack-cli
cd slack-cli
cargo build --release

# 2. Global install (optional)
./scripts/install.sh

# 3. Initialize config
slack config init --bot-token xoxb-your-token

# 4. Refresh cache
slack cache refresh

# 5. Start using! 🎉
slack users "john"
slack channels "general"
slack send "#general" "Hello team!"
```

**Tip**: Using a user token (`xoxp-`) provides more features.

---

## 🎯 Key Features

### Powerful Search
```bash
# Search users (name, email, display name)
slack users "john" --limit 5

# Search channels (name, topic, description)
slack channels "dev" --limit 10

# Search messages (workspace-wide)
slack search "deadline" --channel "#dev-team"
```

### Message Management
```bash
# Send message to channel
slack send "#general" "Meeting in 10 minutes"

# Send DM
slack send "@john.doe" "Hello!"

# Reply to thread
slack send "#dev-team" "Done!" --thread 1234567890.123456

# Get channel messages
slack messages "#general" --limit 20

# Get full thread
slack thread "#dev-team" 1234567890.123456
```

### Channel Management
```bash
# List channel members
slack members "#dev-team"

# JSON output
slack channels "general" --json | jq
```

### Cache & Config
```bash
# Check cache status
slack cache stats

# Refresh cache
slack cache refresh           # all
slack cache refresh users     # users only
slack cache refresh channels  # channels only

# Config management
slack config show            # show config (masked tokens)
slack config path            # config file path
slack config edit            # edit with default editor
```

**Important Notes**:
- Stale cache (>24h): Search returns stale data. Run `slack cache refresh` to update
- `search` command: Not cached, queries API directly. Requires user token + `search:read` scope
- Channel formats: `#channel-name`, `@username`, or IDs (`C123...`, `U456...`). Prefix optional for IDs

---

## 📦 Installation

### Method 1: Prebuilt Binary (Recommended) ⭐

**Automated install**:
```bash
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/install.sh | bash
```

**Manual install**:
1. Download binary from [Releases](https://github.com/junyeong-ai/slack-cli/releases)
2. Extract: `tar -xzf slack-*.tar.gz`
3. Move to PATH: `mv slack ~/.local/bin/`

### Method 2: Cargo

```bash
cargo install slack-cli
```

### Method 3: Build from Source

```bash
git clone https://github.com/junyeong-ai/slack-cli
cd slack-cli
./scripts/install.sh
```

**Requirements**: Rust 1.91.1+

### 🤖 Claude Code Skill (Optional)

When running `./scripts/install.sh`, you can choose to install the Claude Code skill:

- **User-level** (recommended): Available in all projects
- **Project-level**: Auto-distributed to team via git
- **Skip**: Manual installation later

The skill enables natural language Slack data queries in Claude Code.

---

## 🔑 Generate Slack Token

### User Token (Recommended) ⭐

1. Visit [api.slack.com/apps](https://api.slack.com/apps)
2. "Create New App" → "From scratch"
3. Add **User Token Scopes**:
   ```
   channels:read channels:history groups:read groups:history
   im:read im:history mpim:read mpim:history
   users:read users:read.email chat:write search:read
   ```
4. "Install to Workspace" → Copy token (starts with `xoxp-`)

### Bot Token (Alternative)

1. Same app creation as above
2. Add **Bot Token Scopes**:
   ```
   channels:read channels:history groups:read groups:history
   im:read im:history mpim:read mpim:history
   users:read users:read.email chat:write
   ```
3. "Install to Workspace" → Copy token (starts with `xoxb-`)

### Token Comparison

| Feature | User Token ⭐ | Bot Token |
|---------|--------------|-----------|
| Channel Access | ✅ Automatic | ⚠️ Requires invitation |
| Message Search | ✅ Available | ❌ Unavailable |
| Sender | You | Bot account |

---

## ⚙️ Configuration

### Environment Variables

```bash
export SLACK_BOT_TOKEN="xoxb-..."      # Bot token
export SLACK_USER_TOKEN="xoxp-..."    # User token (recommended)
```

### Config File

**Location**:
- macOS: `~/.config/slack-cli/config.toml`
- Linux: `~/.config/slack-cli/config.toml`
- Windows: `%APPDATA%\slack-cli\config.toml`

**Default config** (generated by `slack config init`):
```toml
bot_token = "xoxb-..."
user_token = "xoxp-..."

[cache]
ttl_users_hours = 24
ttl_channels_hours = 24
data_path = "~/.config/slack-cli/cache"  # Same for all platforms

[retry]
max_attempts = 3
initial_delay_ms = 1000
max_delay_ms = 60000

[connection]
timeout_seconds = 30
max_idle_per_host = 10
```

### Config Priority

```
CLI flags > Environment variables > Config file > Defaults
```

**Example**:
```bash
# Override config file token
slack users "john" --token xoxp-temporary-token
```

---

## 🏗️ Core Architecture

Fast local search with SQLite FTS5 (<10ms), 24-hour cache for users/channels, API rate limiting.
For detailed architecture, see [CLAUDE.md](CLAUDE.md).

---

## 🔧 Troubleshooting

### Cache Not Refreshing

```bash
# Delete and recreate cache
rm -rf ~/.config/slack-cli/cache

# Run again
slack cache refresh
```

### "Unauthorized" Error

**Checklist**:
- [ ] Check token format (`xoxp-` or `xoxb-`)
- [ ] Verify required scopes added
- [ ] Confirm workspace reinstall

**Test token**: Verify using Slack API `auth.test` endpoint

### Message Search Not Working

**Cause**: Missing user token or `search:read` scope

**Solution**:
1. Set `SLACK_USER_TOKEN` (`xoxp-`)
2. Add `search:read` scope
3. Reinstall to workspace

### Debug Logging

Use `RUST_LOG` environment variable for debug logging (e.g., `RUST_LOG=debug slack users "john"`)

### Inspect Cache Data

```bash
# Inspect cache directly with SQLite
sqlite3 ~/.config/slack-cli/cache/slack.db
```

---

## 📚 Command Reference

| Command | Description | Example |
|---------|-------------|---------|
| `users <query>` | Search users (name, email, display name) | `slack users "john" --limit 5` |
| `channels <query>` | Search channels (public/private/DM/group DM) | `slack channels "dev" --limit 10` |
| `send <channel> <text>` | Send message | `slack send "#general" "Hello!"` |
| `messages <channel>` | Get channel messages | `slack messages "#general" --limit 20` |
| `thread <channel> <ts>` | Get full thread | `slack thread "#dev" 1234567890.123456` |
| `members <channel>` | List channel members | `slack members "#dev-team"` |
| `search <query>` | Search messages (workspace-wide) | `slack search "deadline" --channel "#dev"` |
| `cache stats` | Show cache statistics (user/channel counts) | `slack cache stats` |
| `cache refresh` | Refresh cache (all/users/channels) | `slack cache refresh users` |
| `config init` | Initialize config | `slack config init --bot-token xoxb-...` |
| `config show` | Show config (masked tokens) | `slack config show` |

### Common Options

| Option | Description | Applies To |
|--------|-------------|------------|
| `--json` | JSON output format | All commands |
| `--token <TOKEN>` | Override token temporarily | All commands |
| `--limit <N>` | Limit results | users, channels, messages, thread, search |
| `--thread <TS>` | Thread timestamp (for replies) | send |
| `--channel <CH>` | Limit to specific channel | search |

**Notes**:
- `search` command requires User token (`xoxp-`) + `search:read` scope
- `cache refresh` supports `users` or `channels` argument for partial refresh (e.g., `slack cache refresh users`)
- Timestamp format: `1234567890.123456` (Slack message ts value)

---

## 🚀 Developer Guide

**Architecture, debugging, contribution guide**: See [CLAUDE.md](CLAUDE.md)

---

## 💬 Support

- **GitHub Issues**: [Report issues](https://github.com/junyeong-ai/slack-cli/issues)
- **Developer Docs**: [CLAUDE.md](CLAUDE.md)

---

<div align="center">

**🌐 [한국어](README.md)** | **English**

**Version 0.1.0** • Rust 2024 Edition

Made with ❤️ for productivity

</div>
