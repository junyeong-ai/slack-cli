# Slack CLI

[![CI](https://github.com/junyeong-ai/slack-cli/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/junyeong-ai/slack-cli/actions/workflows/ci.yml?query=branch%3Amain)
[![Rust](https://img.shields.io/badge/rust-1.97.0%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![DeepWiki](https://img.shields.io/badge/DeepWiki-junyeong--ai%2Fslack--cli-blue.svg?logo=data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACwAAAAyCAYAAAAnWDnqAAAAAXNSR0IArs4c6QAAA05JREFUaEPtmUtyEzEQhtWTQyQLHNak2AB7ZnyXZMEjXMGeK/AIi+QuHrMnbChYY7MIh8g01fJoopFb0uhhEqqcbWTp06/uv1saEDv4O3n3dV60RfP947Mm9/SQc0ICFQgzfc4CYZoTPAswgSJCCUJUnAAoRHOAUOcATwbmVLWdGoH//PB8mnKqScAhsD0kYP3j/Yt5LPQe2KvcXmGvRHcDnpxfL2zOYJ1mFwrryWTz0advv1Ut4CJgf5uhDuDj5eUcAUoahrdY/56ebRWeraTjMt/00Sh3UDtjgHtQNHwcRGOC98BJEAEymycmYcWwOprTgcB6VZ5JK5TAJ+fXGLBm3FDAmn6oPPjR4rKCAoJCal2eAiQp2x0vxTPB3ALO2CRkwmDy5WohzBDwSEFKRwPbknEggCPB/imwrycgxX2NzoMCHhPkDwqYMr9tRcP5qNrMZHkVnOjRMWwLCcr8ohBVb1OMjxLwGCvjTikrsBOiA6fNyCrm8V1rP93iVPpwaE+gO0SsWmPiXB+jikdf6SizrT5qKasx5j8ABbHpFTx+vFXp9EnYQmLx02h1QTTrl6eDqxLnGjporxl3NL3agEvXdT0WmEost648sQOYAeJS9Q7bfUVoMGnjo4AZdUMQku50McDcMWcBPvr0SzbTAFDfvJqwLzgxwATnCgnp4wDl6Aa+Ax283gghmj+vj7feE2KBBRMW3FzOpLOADl0Isb5587h/U4gGvkt5v60Z1VLG8BhYjbzRwyQZemwAd6cCR5/XFWLYZRIMpX39AR0tjaGGiGzLVyhse5C9RKC6ai42ppWPKiBagOvaYk8lO7DajerabOZP46Lby5wKjw1HCRx7p9sVMOWGzb/vA1hwiWc6jm3MvQDTogQkiqIhJV0nBQBTU+3okKCFDy9WwferkHjtxib7t3xIUQtHxnIwtx4mpg26/HfwVNVDb4oI9RHmx5WGelRVlrtiw43zboCLaxv46AZeB3IlTkwouebTr1y2NjSpHz68WNFjHvupy3q8TFn3Hos2IAk4Ju5dCo8B3wP7VPr/FGaKiG+T+v+TQqIrOqMTL1VdWV1DdmcbO8KXBz6esmYWYKPwDL5b5FA1a0hwapHiom0r/cKaoqr+27/XcrS5UwSMbQAAAABJRU5ErkJggg==)](https://deepwiki.com/junyeong-ai/slack-cli)

> **English** | **[한국어](README.md)**

**Run core Slack workflows from your terminal.** Send messages, search context, manage reactions, pins, bookmarks, users, and channels without opening a browser.

---

## Why Slack CLI?

- **Fast** — Millisecond searches powered by SQLite FTS5
- **Practical** — Messages, search, reactions, pins, bookmarks, users, and channels
- **Automatable** — Integrates with scripts, CI/CD, and AI agents

---

## Quick Start

```bash
# Install
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/install.sh | bash

# Log in (browser OAuth)
slack-cli auth login --client-id <your-client-id>

# Or paste an existing token
slack-cli auth login --user-token xoxp-your-token

# Use
slack-cli cache refresh
slack-cli users "john"
slack-cli send "#general" -t "Hello!"
```

---

## Key Features

### Channel identifier (`<channel>` argument)

`#name` · `name` (cache lookup) · `C…/G…` channel ID · `D…` DM ID · `U…/W…` user ID (auto-resolves to that user's DM channel — requires `im` in `channel_types` at the next `cache refresh`)

### Messages
```bash
slack-cli send "#general" -t "Announcement"             # Send (text)
slack-cli send "#general" --markdown-text "**bold**"    # Send (standard Markdown, rendered by Slack)
slack-cli send U123ABCDEF -t "DM by user-id"            # User ID → DM auto-resolution
slack-cli send "#general" -b @blocks.json -t "fallback" # Block Kit + fallback text
slack-cli send "#general" -m @meta.json -t "deploy done" # Attach idempotent metadata
echo '{"event_type":"x","event_payload":{}}' | slack-cli send "#general" -t "x" -m -
slack-cli update "#general" 1234.5678 -t "Edited"       # Update (text/markdown_text/blocks/attachments/metadata)
slack-cli delete "#general" 1234.5678                   # Delete
slack-cli permalink "#general" 1234.5678                # Fetch permalink URL
slack-cli messages "#general" --limit 15                # List (lean default fields)
slack-cli messages "#general" --expand blocks,reactions # Expand fields
slack-cli messages "#general" --oldest 2025-01-01 --latest 2025-01-31
slack-cli messages "#general" --exclude-bots            # Exclude bot messages
slack-cli messages "#general" --cursor <next_cursor>    # Next page (next_cursor from JSON output)
slack-cli thread "#general" 1234.5678                   # Thread
slack-cli search "keyword" --sort timestamp             # Real-time Search
```

**JSON input** — `--blocks` / `--attachments` / `--metadata` accept three source forms:

| Form | Meaning |
|---|---|
| `-` | Read from stdin (at most one flag per invocation) |
| `@path.json` | Read from a file |
| anything else | Inline JSON literal |

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

### Auth, Cache & Config
```bash
slack-cli auth login                              # Log into a workspace (default: PKCE)
slack-cli auth login --method static --user-token xoxp-...  # Paste an existing token
slack-cli auth profiles                           # List stored profiles
slack-cli auth status --verify                    # Inspect active profile + auth.test
slack-cli auth use work                           # Switch active profile
slack-cli auth logout                             # Remove the active profile

slack-cli --profile work users "john"             # Use a different profile for one call

slack-cli cache stats                             # Cache status
slack-cli cache refresh                           # Refresh cache
slack-cli config show                             # Show config
```

---

## Installation

### Automated Install (Recommended)
```bash
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/install.sh | bash
```

`install.sh` downloads the prebuilt GitHub Release binary, verifies its SHA-256 checksum (plus the sigstore signature when `cosign` is installed), and installs it to `~/.local/bin/slack-cli`. On Linux it auto-detects glibc vs musl. The same run can install the Claude Code skill into `~/.claude/skills/slack-workspace`, so no repository checkout is required.

```bash
# Install a specific release
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/install.sh | SLACK_CLI_VERSION=v0.5.0 bash

# Uninstall (noninteractive default removes only the binary and keeps skill/config)
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/uninstall.sh | bash

# Remove the skill and configuration too
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/slack-cli/main/scripts/uninstall.sh | bash -s -- --yes
```

### Cargo (Git)
```bash
cargo install --locked --git https://github.com/junyeong-ai/slack-cli
```

### Build from Source
```bash
git clone https://github.com/junyeong-ai/slack-cli && cd slack-cli
cargo build --release   # rust-toolchain.toml selects the 1.97.0 toolchain
```

**Requirements**: Rust 1.97.0+ (rustup)

---

## Authentication

`slack-cli` stores tokens in `~/.config/slack-cli/auth.json` with `0600` permissions, keyed by named workspace profiles. `config.toml` never contains tokens.

### Method 1 — PKCE OAuth (browser flow, recommended)

```bash
slack-cli auth login --client-id <client-id>
# Or via env
SLACK_CLI_CLIENT_ID=<client-id> slack-cli auth login
```

`auth login` briefly binds a callback server on `127.0.0.1:53682`, opens the Slack authorization page in your browser, and exchanges the code for a user token. One-time setup:

1. Create an app at [api.slack.com/apps](https://api.slack.com/apps)
2. **OAuth & Permissions** → add the User Token Scopes below
3. **Redirect URLs** → register `http://127.0.0.1:53682/callback`
4. **Manage Distribution** → enable PKCE and copy the client id

**User Token Scopes** (full feature set):
```
channels:read  channels:history  groups:read  groups:history
im:read  im:history  mpim:read  mpim:history
users:read  users:read.email  chat:write  metadata.message:read
reactions:read  reactions:write  pins:read  pins:write
bookmarks:read  bookmarks:write  emoji:read  search:read
```

### Method 2 — Paste an existing token (Static)

When you already have an `xoxp-` / `xoxb-` token:

```bash
slack-cli auth login --method static --user-token xoxp-your-token
# Register a bot token alongside it
slack-cli auth login --method static --user-token xoxp-... --bot-token xoxb-...
```

The token is validated via `auth.test` before the profile is persisted.

### Managing profiles

```bash
slack-cli auth profiles                  # List
slack-cli auth status --verify           # Active profile + auth.test
slack-cli auth use work                  # Switch active
slack-cli --profile work users "john"    # Use a different profile for one call
slack-cli auth logout                    # Remove active
slack-cli auth logout --all              # Remove every profile
```

`--profile NAME` is a global flag — position-independent.

---

## Config file

`~/.config/slack-cli/config.toml` (user preferences, no tokens):

```toml
[cache]
ttl_users_hours = 168          # 1 week
ttl_channels_hours = 168
refresh_threshold_percent = 10 # Warn as stale after 10% of TTL
channel_types = ["public_channel", "private_channel"]
                               # Conversation types to cache.
                               # Trim to match your token scopes (e.g. ["public_channel"] if no groups:read).
                               # Allowed: public_channel, private_channel, mpim, im

[output]
users_fields    = ["id", "name", "real_name", "email"]
channels_fields = ["id", "name", "type", "members"]
messages_fields = ["ts", "user", "bot_id", "username", "text", "thread_ts", "reply_count", "subtype", "metadata"]

# Unknown keys are rejected (not silently ignored). Stale keys from prior
# versions (`user_token`, `bot_token`, `max_idle_per_host`,
# `pool_idle_timeout_seconds`) surface as explicit errors — remove them.

[connection]
api_base_url = "https://slack.com/api"
rate_limit_per_minute = 20
app_distribution = "commercial_external"
timeout_seconds = 30
```

Set `app_distribution` according to Slack's `conversations.history` and `conversations.replies` rate-limit policy. Use `marketplace_or_internal` for Slack Marketplace-approved apps or internal customer-built apps.

### Environment variables

| Variable | Purpose |
|---|---|
| `SLACK_USER_TOKEN` | Bypass stored profiles and use this token directly (CI / headless) |
| `SLACK_BOT_TOKEN` | Same, bot token |
| `SLACK_PROFILE` | One-shot active profile override (same as global `--profile`) |
| `SLACK_CLI_CLIENT_ID` | PKCE login client id (same as `--client-id`) |

---

## Command Reference

| Command | Description |
|---------|-------------|
| `auth login` | Authenticate to a workspace (`--method pkce\|static`) |
| `auth logout [--all]` | Remove profile (`--keep-remote` skips `auth.revoke`) |
| `auth status [--verify]` | Profile status with optional token verification |
| `auth profiles` | List stored profiles |
| `auth use <name>` | Switch active profile |
| `users <query>` | Search users |
| `users --id <ids>` | Lookup by IDs (comma-separated) |
| `channels <query>` | Search channels |
| `channels --id <ids>` | Lookup by IDs (comma-separated) |
| `send <ch> [-t -b -a -m --markdown-text --thread]` | Send a message (≥1 content field required) |
| `update <ch> <ts> [-t -b -a -m --markdown-text]` | Update a message (≥1 content field required) |
| `delete <ch> <ts>` | Delete a message |
| `permalink <ch> <ts>` | Fetch the permalink URL for a message |
| `messages <ch>` | List messages |
| `thread <ch> <ts>` | List thread |
| `members <ch>` | List members |
| `search <query>` | Search with the Real-time Search API |
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
| `config show/path/edit` | Config management |

### Common Options
- `--json` — JSON output
- `--profile <name>` — Use a specific profile for this invocation (env: `SLACK_PROFILE`)
- `--config <path>` — Override the config.toml path
- `--verbose` — Enable debug logs

### users/channels Options
- `--limit <N>` — Limit results (default: `10`)
- `--id <ids>` — Lookup by IDs (comma-separated)
- `--expand <fields>` — Extra fields beyond the defaults
  - users: `display_name`, `status`, `status_emoji`, `avatar`, `title`, `timezone`, `is_admin`, `is_bot`, `deleted`
  - channels: `topic`, `purpose`, `created`, `creator`, `is_member`, `is_archived`, `is_private`, `user` (the DM peer's user id)

### send / update Options
- `-t, --text <TEXT>` — Message text (also used as the notification fallback when blocks are present)
- `--markdown-text <TEXT>` — Standard-markdown body, rendered by Slack (max 12,000 chars). Not combinable with `--text`/`--blocks`
- `-b, --blocks <SOURCE>` — Block Kit blocks (JSON array). `-` / `@file` / inline
- `-a, --attachments <SOURCE>` — Legacy attachments (JSON array). Same source vocabulary
- `-m, --metadata <SOURCE>` — Message metadata `{event_type, event_payload}` (JSON object). Same source vocabulary
- `--thread <ts>` — (send only) Post as a reply in the given thread

At least one of `text` / `markdown_text` / `blocks` / `attachments` must be provided. Only one flag per invocation may read from stdin (`-`).

### messages/thread Options
- `--limit <N>` — Limit results (default: `15`)
- `--cursor <cursor>` — (messages only) Fetch the next page using `next_cursor` from the previous response
- `--oldest <date>` — (messages only) Start time (Unix timestamp or YYYY-MM-DD)
- `--latest <date>` — (messages only) End time (Unix timestamp or YYYY-MM-DD)
- `--exclude-bots` — Exclude bot messages (messages and thread)
- `--expand <fields>` — Extra fields beyond the lean default
  - Computed: `date`, `user_name`
  - Response: `blocks`, `attachments`, `reactions`, `edited`, `parent_user_id`, `reply_users`, `reply_users_count`, `latest_reply`, `channel`, `permalink`

The lean `messages_fields` default is `ts`, `user`, `bot_id`, `username`, `text`, `thread_ts`, `reply_count`, `subtype`, `metadata`. The default output is intentionally compact so AI agents pay no extra context tax; rich fields are opt-in via `--expand`.

`messages --json` emits a `{messages: [...], next_cursor}` envelope. When `next_cursor` is not `null`, pass it back via `--cursor` for the next page. `thread --json` paginates internally up to `--limit`, so it stays a bare array.

### Exit Codes & Error Output

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | Generic error |
| `2` | Usage error (clap) |
| `3` | Auth error (re-login needed — `invalid_auth`, `missing_scope`, …) |
| `4` | Rate limited (retries exhausted) |

Runtime failures in `--json` mode print an `{"error": {"code", "message"}}` envelope to stderr. Usage errors (exit code `2`) happen at parse time, so they print clap's diagnostic text instead — branch on the exit code alone. `code` is Slack's own error string for API failures (`channel_not_found`, …) and otherwise one of `auth_error` / `rate_limited` / `http_error` / `network_error` / `error`. stdout always stays "parseable data or empty".

### search Options
- `--limit <N>` — Total results to return (1-100, default: `10`. Auto-paginates across 20-result pages.)
- `--channel <id|name>` — Restrict the search to one channel
- `--before <date>` — Only results before this time (Unix ts or YYYY-MM-DD)
- `--after <date>` — Only results after this time
- `--channel-types <types>` — Conversation types to search (default: `public_channel,private_channel,mpim,im`)
- `--content-types <types>` — Content types to search (default: `messages`)
- `--include-context` — Include surrounding context messages
- `--include-bots` — Include bot-authored messages
- `--include-archived` — Include archived channels
- `--no-semantic` — Force keyword-only matching (skip the API's automatic semantic mode)
- `--sort <score|timestamp>` — Sort field
- `--sort-dir <asc|desc>` — Sort direction

---

## Troubleshooting

### Reset Cache
```bash
rm -rf ~/.config/slack-cli/cache && slack-cli cache refresh
```

### Permission Errors
Check token scopes → Reinstall to Workspace → Re-run `slack-cli auth login` to pick up the new scopes

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
