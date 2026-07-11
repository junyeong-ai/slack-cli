# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.7.1] - 2026-07-11

### Fixed

- `messages` / `thread` no longer fail an entire page when Slack emits a message's `channel` field as a bare id string instead of an `{id, name}` object — both wire forms deserialize, output always serializes the object form. Busy channels hit this deterministically while paging with `--cursor`

## [0.7.0] - 2026-07-11

### Added

- `--markdown-text` on `send` and `update`: standard-Markdown message body rendered by Slack itself (`chat.postMessage`/`chat.update` `markdown_text`, max 12,000 chars) — no mrkdwn hand-translation needed. Mutually exclusive with `--text`/`--blocks`, enforced at both the clap and payload-validation boundaries
- Typed `SlackApiError` (`Api`, `RateLimitExhausted`, `Http`, `Transport`) replaces string-only Slack core errors; the CLI boundary classifies failures by downcast into differentiated exit codes — `0` ok, `1` generic, `2` usage (clap), `3` auth, `4` rate-limited — and `--json` mode prints an `{"error": {code, message}}` envelope to stderr for runtime failures, with Slack's own error string preserved verbatim as `code`
- `scripts/install.sh` verifies release signatures with cosign (sigstore bundle pinned to the tag-triggered release workflow identity) when cosign is installed, compares SHA-256 digests directly, and auto-detects glibc vs musl on Linux

### Changed

- **BREAKING**: `slack-cli messages --json` now emits a `{messages, next_cursor}` envelope instead of a bare array, exposing the `conversations.history` cursor so channel history is actually pageable via `--cursor`; `next_cursor` is `null` on the last page. `thread --json` keeps its bare-array shape (it paginates internally)
- Rust toolchain 1.95.0 → 1.97.0 (`rust-toolchain.toml` is the single pin; the duplicate `.tool-versions` that overrode it via `RUSTUP_TOOLCHAIN` is removed) and rusqlite 0.39 → 0.40 / r2d2_sqlite 0.34 → 0.35, picking up rusqlite's tainted-SAVEPOINT SQL-injection fix

### Fixed

- Cache schema migration actually runs: the stored `schema_version` is compared on open and any mismatch rebuilds every cache object inside a single `BEGIN IMMEDIATE` transaction, so concurrent processes serialize instead of interleaving DROP/CREATE; a non-integer stored version rebuilds instead of failing the open
- First-open `database is locked` races: the WAL switch happens once at pool creation (not per connection) with a bounded busy retry, since SQLite journal-mode transitions bypass the busy handler
- Release "latest" alias assets get regenerated `.sha256` files; the copied checksums previously referenced the versioned filenames and could never verify

### Documentation

- README (KO + EN), the `slack-workspace` skill, and module guides document the pagination envelope, `--markdown-text`, the exit-code / error-envelope contract (usage errors exit `2` with clap diagnostics by design), and the install verification chain; the skill's `allowed-tools` narrows from blanket `Bash` to `Bash(slack-cli *)` + `Bash(jq *)`

## [0.6.0] - 2026-05-19

### Added

- Introduce `MessagePayload`, the unified content surface for `chat.postMessage` and `chat.update` (text, blocks, attachments, metadata). The CLI exposes it via `-t/--text`, `-b/--blocks`, `-a/--attachments`, `-m/--metadata`; each JSON-source flag accepts `-` (stdin, max one per call), `@path.json`, or inline JSON, with array-vs-object shape validated before any HTTP call
- `slack-cli permalink <channel> <ts>` and `messages.permalink(channel, ts)` wrap `chat.getPermalink`
- `SlackMessage` exposes a typed `metadata` field; `conversations.history` and `conversations.replies` always request `include_all_metadata=true` so idempotency markers round-trip without an extra flag
- `SlackAuthIdentity` surfaces `url`, `bot_id`, `enterprise_id`, `enterprise_name`, and `is_enterprise_install` from `auth.test`; PKCE user-scope set gains `metadata.message:read`
- `[output] messages_fields` config key with a lean AI-first default (`ts`, `user`, `bot_id`, `username`, `text`, `thread_ts`, `reply_count`, `subtype`, `metadata`); rich fields are opt-in via `--expand` on `messages` and `thread`, both of which now also accept `--exclude-bots` for symmetry
- `<channel>` arguments accept `U…` / `W…` user IDs and auto-resolve to that user's cached DM channel (requires `im` in `cache.channel_types`)

### Changed

- **BREAKING**: `slack-cli send <channel> <text>` is now `slack-cli send <channel> -t <text>` (at least one of `text` / `blocks` / `attachments` required). `slack-cli update` mirrors the same shape minus `--thread`
- **BREAKING**: `slack-cli messages --json` projects through `messages_fields`; previously-implicit `blocks` / `attachments` / `reactions` / `permalink` fields require `--expand`
- **BREAKING**: `SlackMessageClient::{send, update}` library signatures take a `MessagePayload`
- **BREAKING**: `config.toml` rejects unknown keys (`deny_unknown_fields`); stale entries (`user_token`, `bot_token`, `connection.max_idle_per_host`, `connection.pool_idle_timeout_seconds`) now surface as explicit parse errors instead of being silently ignored
- **BREAKING**: HTTP connection-pool tuning (`max_idle_per_host`, `pool_idle_timeout_seconds`) is no longer a `[connection]` knob — the previous defaults are internal constants inside the Slack core

### Fixed

- `SlackChannel.name` and `MessageChannel.name` are now `Option<String>` — DM channels from `conversations.list?types=im` arrive without a `name` field, which previously crashed `cache refresh` with `missing field 'name'`. DMs round-trip through the cache cleanly, and `SlackChannel.user` exposes the DM peer

### Documentation

- Align README (KO + EN), the `slack-workspace` skill, and the per-module `CLAUDE.md` files with the new send / update / permalink surface, the channel-identifier table covering `U…` user IDs, the JSON source forms, and the lean `messages_fields` default

## [0.5.0] - 2026-05-16

### Added

- Introduce a multi-method authentication subsystem (`slack-cli auth login`) supporting `static` (paste an existing `xoxp-` / `xoxb-` token) and `pkce` (OAuth Authorization Code + PKCE with an embedded `client_id`); tokens persist to `${XDG_CONFIG_HOME:-~/.config}/slack-cli/auth.json` (mode `0600`, atomic write) keyed by named profiles
- `slack-cli auth {login, logout, status, profiles, use}` subcommand group; global `--profile` (env: `SLACK_PROFILE`) selects the active profile per invocation and is accepted at any position
- `SLACK_USER_TOKEN` / `SLACK_BOT_TOKEN` env vars bypass the store entirely for CI / headless use

### Changed

- **BREAKING**: Remove `bot_token` / `user_token` keys from `config.toml`; tokens now live in `auth.json` only
- **BREAKING**: Remove `--token` / `--user-token` global CLI flags
- **BREAKING**: Remove `slack-cli config init`; use `slack-cli auth login` instead

### Documentation

- Restructure root `CLAUDE.md` for progressive disclosure with a new `src/auth/CLAUDE.md` covering the auth subsystem
- Replace the `config init` flow with the `auth login` workflow in both `README.md` and `README.en.md`
- Align the `slack-workspace` Claude Code skill with the new auth flow and document the actual JSON response shape per command

### Fixed

- Correct skill JSON shape claims for `emoji`, `reactions`, `users`, and `channels` so generated `jq` queries match the real output envelope

## [0.4.0] - 2026-05-14

### Added

- Expand RTS option coverage with `--channel`, `--before`, `--after`, `--include-archived`, and `--no-semantic` flags; `highlight` and `include_message_blocks` auto-toggle by output mode

### Changed

- **BREAKING**: Align all client methods with verb-only naming (`messages.send`, `messages.history`, `messages.replies`, `users.list`, `channels.list`, `channels.members`, etc.); remove dead `pub` plumbing (`post_message`, `get_thread_replies`, `*_streaming` variants)
- **BREAKING**: Drop the `assistant.search.info` capabilities path and rename `SlackSearchClient::search` to `context`; remove `SearchCapabilities`
- Annotate `context()` failure with the `search:read.*` scope requirement so auth errors surface an actionable message

### Documentation

- Restructure `CLAUDE.md` with progressive disclosure: slim root file plus nested `src/slack/CLAUDE.md` and `src/cache/CLAUDE.md`; align `README` and skill manifest with the actual CLI surface

### Fixed

- Paginate `search.context` to the user-requested total instead of capping at a single 20-result page; raise `--limit` ceiling to 100
