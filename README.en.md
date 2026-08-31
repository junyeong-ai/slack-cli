# Slack CLI

[![CI](https://github.com/junyeong-ai/slack-cli/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/junyeong-ai/slack-cli/actions/workflows/ci.yml?query=branch%3Amain)
[![Rust](https://img.shields.io/badge/rust-1.98.0%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
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
slack-cli auth scopes                             # Scopes to register on your app
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
cargo build --release   # rust-toolchain.toml selects the 1.98.0 toolchain
```

**Requirements**: Rust 1.98.0+ (rustup)

---

## Authentication

`slack-cli` stores tokens in `auth.json` (mode `0600` inside a `0700` directory on Unix; on Windows it relies on the default `%APPDATA%` ACL), keyed by named workspace profiles. `config.toml` never contains user or bot tokens. The empty `auth.json.lock` beside it is the lock file that serialises concurrent invocations; it holds no content.

The scopes are derived from the Slack methods the CLI calls, so you can always read the current list off the binary itself:

```bash
slack-cli auth scopes          # Scopes to register on your Slack app
```

### Method 1 — PKCE OAuth (browser flow, recommended)

```bash
slack-cli auth login --client-id <client-id>
# Or via env
SLACK_CLI_CLIENT_ID=<client-id> slack-cli auth login
```

`auth login` briefly binds a callback server on `127.0.0.1:53682` (`--port` to change it), opens the Slack authorization page in your browser, and exchanges the code for a user token. The port has to match the Redirect URL registered on the app exactly. The browser opens only when the command is run from a terminal; anywhere else — a script, CI — the URL is printed instead, which `--no-browser` also forces. One-time setup:

1. Create an app at [api.slack.com/apps](https://api.slack.com/apps)
2. **OAuth & Permissions** → add the User Token Scopes below
3. **OAuth & Permissions** → Redirect URLs → register `http://127.0.0.1:53682/callback`
4. **OAuth & Permissions** → enable PKCE, then copy the client id from **Basic Information**

> Enabling PKCE marks the app a public client and cannot be undone without contacting Slack support. Slack routes a PKCE app's loopback redirect as a desktop redirect: it **cannot carry bot scopes**, and the tokens it issues **always rotate every 12 hours**. The CLI renews them from the refresh token, so this needs no re-login.

**User Token Scopes** (full feature set):

<!-- scopes:user -->
```
bookmarks:read  bookmarks:write  channels:history  channels:read
chat:write  emoji:read  groups:history  groups:read
im:history  im:read  mpim:history  mpim:read
pins:read  pins:write  reactions:read  reactions:write
search:read.files  search:read.im  search:read.mpim  search:read.private
search:read.public  search:read.users  users:read  users:read.email
```
<!-- /scopes:user -->

### Method 2 — Paste an existing token (static)

When you already have an `xoxp-` / `xoxb-` token:

```bash
slack-cli auth login --method static --user-token xoxp-your-token
# Register a bot token alongside it
slack-cli auth login --method static --user-token xoxp-... --bot-token xoxb-...
```

The token is validated via `auth.test` before the profile is persisted. Pasted tokens never expire and are never renewed.

**Bot Token Scopes** (when registering a bot token with `--bot-token`):

<!-- scopes:bot -->
```
bookmarks:read  bookmarks:write  channels:history  channels:read
chat:write  emoji:read  groups:history  groups:read
im:history  im:read  metadata.message:read  mpim:history
mpim:read  pins:read  pins:write  reactions:read
reactions:write  search:read.public  users:read  users:read.email
```
<!-- /scopes:bot -->

### Token rotation

Slack issues an expiry and a refresh token with every PKCE installation, and the CLI renews the pair through `oauth.v2.access` starting two hours before expiry. Renewal runs under a cross-process lock on `auth.json`, so concurrent invocations can never spend a refresh token Slack has already revoked.

```bash
slack-cli auth status          # Per-token expiry and whether it can be renewed
```

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

## Updating

```bash
slack-cli self update            # replace with the latest release (prompts first)
slack-cli self update --check    # report whether a newer version exists
slack-cli self update --yes      # skip the prompt
slack-cli self update --version 0.9.0   # install a specific release
```

It downloads the executable the release publishes beside the archive and
**always verifies its SHA-256**. When `cosign` is installed it also verifies the
sigstore signature, pinned to the release workflow **for that exact tag**, so a
validly-signed binary lifted from another release and re-uploaded under this
one's asset names is refused. With cosign installed, a release that publishes no
signature is refused outright: the checksum is produced by whoever published the
binary, so on its own it says nothing about origin. Without cosign the download
rests on its checksum and the command says so.

The replacement is staged in the destination directory and renamed into place,
so a failure part-way through never leaves a half-written binary.

This works where the binary lives somewhere you can write, such as
`~/.local/bin`. For a system-wide install, re-run `install.sh` instead.

Releases publish the bare executable from v0.9.0 onward, so naming an earlier
release with `--version` is refused with a message saying the asset is absent.
Use `install.sh` to move to one of those.

---

## Config file

`config.toml` (user preferences; it never holds user or bot tokens — `auth.json` owns those). The location follows the platform convention — `$XDG_CONFIG_HOME/slack-cli` or `~/.config/slack-cli` on Linux/macOS, `%APPDATA%\slack-cli` on Windows. Run `slack-cli config path` for the resolved path.

```toml
[auth]
client_id = "1234.5678"        # the app a browser login authorizes against (= --client-id)
exclude_scopes = []            # scopes the workspace will not grant (see below)

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

[retry]
max_attempts = 3               # 429 retries (Retry-After takes precedence)
initial_delay_ms = 1000        # first backoff when no Retry-After header
max_delay_ms = 60000
exponential_base = 2.0

[events]
mode = "spool"                 # stream | spool | archive — how long an event is kept
store_body = false             # false stores references only (no text, no raw payload)
store_raw = false              # true carries Slack's own payload on the event (for writing rules)
retention_days = 7             # the archive window, and the safety cap on a spool
buffer = 1024                  # in-flight queue depth
on_overflow = "drop_oldest"    # drop_oldest | drop_newest (a drop is always counted)
max_bytes = 268435456          # ceiling on the log's live data (256MiB, minimum 1MiB)
backfill = true                # recover what a disconnect missed
backfill_max_channels = 20     # how many channels one recovery may read
backfill_max_age_hours = 24    # how far back a read may reach (an older cursor reads from here, with a warning)
# data_path = "~/other/place"   # where the event store lives (default: events/ under the config dir)

[[events.sink]]
name = "agent"
type = "stdout"                # stdout | exec | http
                               # exec/http push; `events pull` pulls.
                               # Using both delivers every event twice.

[[events.rule]]
name = "mention"
on = ["message"]
mentions_me = true

[[events.rule]]
name = "watched-thread"
on = ["message", "reaction_added", "reaction_removed"]
subscribe_emoji = "eyes"       # follow replies in threads I reacted to with :eyes:
```

Set `app_distribution` according to Slack's `conversations.history` and `conversations.replies` rate-limit policy. Use `marketplace_or_internal` for Slack Marketplace-approved apps or internal customer-built apps.

`[auth]` resolves as **command-line flag > environment variable > `config.toml`**. Recording `client_id` here spares every `auth login` a flag or an environment variable.

`exclude_scopes` leaves scopes out of the authorization request for an app or workspace that will not grant them. Slack approves a scope set as a whole, so one scope the app cannot register fails the whole login. Every entry must be a scope the CLI would otherwise ask for; anything else is refused when the config loads. `auth scopes` prints the effective list, so what you register on the Slack app is what the login requests.

A command that needs an excluded scope is refused by Slack, and the CLI names the scope: `Slack API error: missing_scope. bookmarks.list needs bookmarks:read; the token in use was not granted it`.

### Environment variables

| Variable | Purpose |
|---|---|
| `SLACK_USER_TOKEN` | Bypass stored profiles and use this token directly (CI / headless) |
| `SLACK_BOT_TOKEN` | Same, bot token |
| `SLACK_APP_TOKEN` | App-level token (`xapp-`) for the Socket Mode connection. It never displaces a stored user or bot token. Running on `SLACK_USER_TOKEN` stores events under an `env` namespace rather than a profile's |
| `SLACK_PROFILE` | One-shot active profile override (same as global `--profile`) |
| `SLACK_CLI_CLIENT_ID` | client_id for the browser logins (same as `--client-id`; outranks `config.toml [auth]`) |
| `RUST_LOG` | Log filter (e.g. `debug`, `slack_cli::cache=debug`). Takes precedence over `--verbose` when set |

A `.env` file in the working directory supplies any of the above.

---

## Command Reference

| Command | Description |
|---------|-------------|
| `auth login` | Authenticate to a workspace (`--method pkce\|static`) |
| `auth logout [--all]` | Remove profile (`--keep-remote` skips `auth.revoke`) |
| `auth status [--verify]` | Profile status with optional token verification |
| `auth profiles` | List stored profiles |
| `auth use <name>` | Switch active profile |
| `auth scopes` | Print the OAuth scopes to register on your Slack app |
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
| `search --capabilities` | Report whether this workspace can search semantically |
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
| `watch` | Stream matching events to stdout (stores nothing, ignores configured sinks) |
| `daemon run` | Run the Socket Mode daemon in the foreground |
| `daemon status` | Whether a daemon is running (decided by its lock), its last heartbeat and counters |
| `daemon stop` | Signal the running daemon to stop (Unix only; use the service manager on Windows) |
| `events pull [--consumer --ack --follow]` | Read events a consumer has not acknowledged |
| `events ack --through <seq>` | Move a consumer's position forward |
| `events stats` | Event log size, age and consumer backlogs |
| `events prune` | Apply the retention policy now |
| `events path` | Where the daemon keeps its files |
| `cache stats/refresh` | Cache management |
| `config show/path/edit` | Config management |
| `self update` | Replace this binary with the latest release |
| `self update --check` | Report whether an update exists, changing nothing |

### Common Options
- `--json` — JSON output
- `--profile <name>` — Use a specific profile for this invocation (env: `SLACK_PROFILE`)
- `--config <path>` — Override the config.toml path. A named file that is absent is an error, not a fall back to defaults
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
- `--include-deleted-users` — Include deleted users in results
- `--modifiers <expr>` — Search modifiers, e.g. `"has:pin from:@alice"`
- `--no-semantic` — Force keyword-only matching (skip the API's automatic semantic mode)
- `--capabilities` — Report the workspace's semantic-search availability instead of running a query
- `--sort <score|timestamp>` — Sort field
- `--sort-dir <asc|desc>` — Sort direction

---

## Real-time events (Socket Mode)

Opens the WebSocket Slack pushes events down, and forwards only what a rule matches. `conversations.history` is capped at one request a minute and 15 messages for a non-Marketplace app, so polling cannot follow more than a channel or two. Event delivery is a separate axis from that limit.

### App setup

1. Turn on **Socket Mode**.
2. Under **Basic Information → App-Level Tokens**, create a token with the `connections:write` scope (it starts with `xapp-`).
3. Under **Event Subscriptions → Subscribe to events on behalf of users**, add `message.channels`, `message.groups`, `message.im`, `message.mpim`, `reaction_added` and `reaction_removed`. These are **user events**, not bot events — that is what delivers the conversations you can see without inviting a bot to every channel.
4. Register the token:

```bash
# Attaches an app-level token to an existing profile (it creates no new one)
slack-cli auth login --app-token xapp-1-A0000-000-abc

# Or through the environment
export SLACK_APP_TOKEN=xapp-1-A0000-000-abc
```

No OAuth grant issues an app-level token, so the browser flow cannot produce one. `connections:write` is never mixed into a user or bot scope request — Slack refuses the whole authorization when it is.

```bash
slack-cli watch                     # foreground. Stores nothing, and goes to stdout rather than the configured sinks
slack-cli watch --json | my-agent   # NDJSON stream
slack-cli daemon run                # long-lived, under launchd or systemd
slack-cli daemon status
```

### Retention modes

`events.mode` decides **how long** an event is kept and `events.store_body` decides **how much** of it. Cursors, thread subscriptions and deduplication keys live in a separate state database in every mode, and none of them holds anything anyone said.

| Mode | What reaches the disk | Delivery guarantee |
|---|---|---|
| `stream` | Nothing (beyond the state database) | Best-effort — an event that arrives while the consumer is down is gone |
| `spool` | Kept until acknowledged | At-least-once |
| `archive` | Kept for `retention_days` | At-least-once, with replay |

With `store_body = false` the log becomes an index of references — channel, ts, author, which rule matched. Fetch the body with `slack-cli thread` when it is actually needed, and Slack stays the only copy.

In `stream` mode `events pull` fails with the reason rather than returning an empty result that looks like a quiet workspace.

### Rules

Rules are configuration, not code, and a wrong one is refused when the config loads. So is a rule with no condition at all, which would forward the entire workspace.

- `mentions_me` — messages that name you
- `keywords` — case-insensitive substrings
- `from_users` — specific authors
- `channels` — an allowlist of conversation IDs, not names (`slack-cli channels <name>` prints them)
- `subscribe_emoji` — **your own** reaction subscribes that message's thread, and every later reply matches. Removing the reaction unsubscribes. The emoji is the subscribe button.

`include_own_messages` defaults to `false`: an assistant that answers its own messages is a loop.

### Wiring it to an agent

Events arrive as one JSON object per line under the `slack-cli.event/1` schema. There are two ways to receive them, and you want **one of them** — using both delivers every event twice.

- **push** — an `exec` or `http` sink calls the agent the moment a rule matches. Lowest latency, but **not the guaranteed path**: a failure is counted and never retried, because retrying inline would stall every event behind it. Whatever arrived while the agent was down is gone.
- **pull** — `events pull` reads from a cursor, so an agent that restarts resumes where it stopped. Needs `mode` set to `spool` or `archive`.

Replying needs no new protocol — it is the CLI you already have:

```bash
slack-cli events pull --consumer agent --follow --json |
  while read -r event; do
    ts=$(echo "$event" | jq -r '.thread_ts // .ts')
    ch=$(echo "$event" | jq -r '.channel')
    id=$(echo "$event" | jq -r '.id')
    reply=$(my-agent "$event")
    slack-cli send "$ch" --thread "$ts" -t "$reply" \
      -m "{\"event_type\":\"assistant_reply\",\"event_payload\":{\"source_event\":\"$id\"}}"
  done
```

Carrying the source event id in `--metadata` is what stops a restarted agent from answering the same message twice. Delivery is at-least-once, so a consumer has to be idempotent.

> **Warning**: a message sent with a user token goes out **under your own name**, with no bot badge. Send automatically with a bot token, or have the agent DM you a draft and send it yourself.

### Limits

- Socket Mode **does not replay** what a disconnect missed. Recovery reads `conversations.history` for channels and `conversations.replies` for subscribed threads, and only for channels a rule cares about, bounded by a count. The age bound clamps where a read *starts* rather than excluding a channel: an older cursor is read from the horizon, and the stretch before it is reported. Reaction events can be read back by neither, so subscription changes made during a disconnect are lost.
- An **edit or deletion made during a disconnect is not recovered**. `conversations.history` returns an edited message under its original ts, which collapses onto the original the daemon already saw. That is the floor of a ts-keyed recovery model, not a bug in it.
- A subscribed thread is re-read **from where it was last followed**. Without that cursor every reconnect would read the thread from its first message, and once the deduplication layers lapsed it would deliver the whole thread again.
- A gap deeper than `backfill_max_channels` and the five-page bound leaves its oldest part unread, with a warning. The cursor advances to the newest recovered message, so that stretch stays unrecovered afterwards.
- Events are **isolated per profile**, and running on `SLACK_USER_TOKEN` uses an `env` namespace rather than a profile's. The daemon and `daemon status` / `events pull` must run **under the same environment** to see the same store — `daemon status` and `events stats` print which one they are reading on their `profile :` line.
- Delivery is **serial**. One task owns the whole pipeline, which is what makes the deduplication gate race-free and what keeps a consumer seeing events in arrival order. A slow sink holds the pipeline, and the bounded queue in front of it absorbs — that is, discards — the pressure.
- The app-level token and the user token are registered separately, and Slack never checks they belong to the same workspace. A payload not authorized for this installation is discarded and reported. The judgement reads `authorizations`, not the top-level `team_id` — in a Slack Connect channel that field names the partner org, so judging on it would drop everything they say.
- Slack **splits** an app's events across its open connections. Registering one app token under two differently named profiles gives them separate locks, so two daemons start and each sees half. Two daemons see half a workspace each, which is why one is locked per profile. Use two Slack apps if you need two machines.
- An emoji subscription has to land on the **thread's first message** (or a top-level one). Reacting to a reply subscribes a thread rooted at that reply.
- A Socket Mode app cannot be listed in the public Slack Marketplace (org-wide distribution is fine).
- `watch` and `daemon run` cannot run at once for the same profile. They share cursors, thread subscriptions and deduplication keys, so only one holds the lock.

## Troubleshooting

### Reset Cache
```bash
rm -rf "$(dirname "$(slack-cli config path)")/cache" && slack-cli cache refresh
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
