# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.12.0] - 2026-08-30

### Added

- **`config.toml [auth].exclude_scopes`.** Slack approves a scope set as a whole, so an app that may not register one of the scopes this CLI derives cannot be installed at all — which left such a workspace with no browser login. Exclusions subtract from the derived set rather than replacing it, so a method added to the registry still reaches every installation, and each entry is checked against that set: a name no method needs is refused when the config loads. `auth scopes` prints the effective set, so what you register on the Slack app is what the login requests
- A command refused for a scope now says which one: `missing_scope. bookmarks.list needs bookmarks:read`. Slack reports only that a scope is missing, while the registry already knows what the method declares

### Fixed

- **A browser opened for callers with no one to see it.** `auth login` opened one whenever `--no-browser` was absent, so a script, a CI job or a test could put an authorization window on a desktop. It opens only when the command is run from a terminal; anywhere else the URL is printed, which such a caller can act on
- **`--config` naming a file that is not there loaded defaults and exited zero,** discarding the argument. A config that could not be stat'd — behind a trailing slash, or an unsearchable parent — vanished the same way. Only a default location that has never been created falls back to defaults now
- **`config edit` was refused by the very config it repairs.** It and `config path` run before the config loads, so any rejected value can still be fixed. `config edit` refuses a path that is not a file, which the load used to catch
- `--port 0` asks for an ephemeral port, which no app can register as a Redirect URL; it produced `http://127.0.0.1:0/callback` and a refusal from Slack after the browser had opened. It is rejected as the usage error it is

## [0.11.0] - 2026-08-30

### Removed

- **`auth login --method client-secret`, `--client-secret`, `SLACK_CLI_CLIENT_SECRET` and `config.toml [auth].client_secret`.** Slack refuses a loopback redirect that omits PKCE — *Must use PKCE to redirect to a non-web URI* — and refuses bot scopes on one outright — *Bot scopes are not allowed when redirecting to a non-web URI*. Every address a CLI can receive a callback on is such a URI, so the confidential flow could never reach a consent screen and the bot token it advertised was never obtainable through a browser. Both messages come from Slack's own authorization endpoint. A bot token is registered by pasting it into `--method static`, which is where its scope list now lives. A `client_secret` left in `config.toml` is refused rather than ignored

### Fixed

- **Every browser login failed at the consent screen.** The requested user scopes included `metadata.message:read`, which Slack supports on bot and legacy tokens only. A scope set is granted as a whole, so the one unsupported entry failed the whole authorization with `Invalid permissions requested` — for every PKCE login since the flow was introduced in 0.8.0. The scope is now requested for bot tokens alone
- **A rejected `config.toml` printed its own contents.** `toml` reports a parse failure by quoting the offending line, and a config the CLI refuses is often one still holding a credential a newer schema no longer accepts — a `user_token` dropped before 0.5.0, a `client_secret` dropped in this release. The refusal repeats on every invocation, so the value reached the terminal and anything capturing it each time. The error now reports the file, line and column instead
- `config edit` no longer creates `config.toml` with mode `0600`; 0.10.0 added that for the client secret the file no longer holds

## [0.10.0] - 2026-08-30

### Added

- `config.toml` carries the OAuth app under `[auth]` — `client_id`, and optionally `client_secret` — so a browser login no longer needs a flag or an environment variable on every run. The environment was the only non-flag source before, and a value placed there is inherited by every process the CLI spawns, while a `.env` sits in whatever working directory the command ran from, usually a git repository. `config.toml` is a `0600` file inside a `0700` directory and belongs to no repository. Flags and the environment still outrank it, so CI is unaffected

### Fixed

- `client_secret` is read from the config but never written back, so `config show --json` — which serializes the whole config — cannot print it; the human view masks it. `config edit` now creates a missing `config.toml` already restricted to `0600` rather than tightening it afterwards, closing the window in which a fresh file is readable by anyone

## [0.9.0] - 2026-08-30

### Added

- `self update` replaces the running binary with a published release, with `--check` to report whether one exists, `--version` to name a release, and `--yes` to skip the prompt. Releases now publish the executable itself beside each archive, so the CLI needs no gzip, tar or zip reader on the one path that overwrites the binary a user runs
- The download is always checked against the published SHA-256, and against its sigstore signature when `cosign` is installed. The certificate is pinned to this repository's release workflow for the exact tag being installed, which refuses a validly-signed binary lifted from another release and re-uploaded under this one's asset names. With `cosign` installed a release that publishes no signature is refused outright — the checksum is produced by whoever published the binary, so on its own it says nothing about origin
- The replacement is staged in the destination directory and renamed into place, so an interrupted update leaves the existing binary untouched. Windows moves the running executable aside first and restores it if the swap fails

The executable asset exists from this release onward, so `--version` naming an earlier release is refused with a message saying so; use `scripts/install.sh` for those.

## [0.8.0] - 2026-08-30

### Added

- `auth login --method client-secret`: confidential-client OAuth using an app's client id and secret, authenticating with HTTP Basic. Slack routes the loopback redirect of a non-PKCE app as a server redirect, so this flow can request bot scopes and issue a `xoxb-` token in the same pass — something the PKCE flow structurally cannot do, because Slack treats a PKCE app's loopback as a desktop redirect and refuses bot scopes there
- `auth scopes` prints the OAuth scopes to register on the Slack app, derived from the API methods the CLI calls. `tests/documented_scopes.rs` holds both READMEs to that same set, and `tests/readme_parity.rs` fails the build when a command, flag, environment variable or exit code is documented in one language and not the other
- `assistant.search.info` via `search --capabilities`, reporting whether the workspace can rank semantically, plus the `--modifiers` (e.g. `has:pin from:@alice`) and `--include-deleted-users` parameters on `search`
- Windows support: config, auth store and cache now resolve through the platform base-directory convention (`%APPDATA%` on Windows, `$XDG_CONFIG_HOME` or `~/.config` elsewhere — byte-identical to the previous behaviour on Unix). CI now runs the test suite on `windows-latest`, which the release workflow has been shipping binaries for untested

### Fixed

- **Rotating tokens are renewed instead of expiring.** Slack issues a 12-hour access token and a refresh token for every PKCE installation, and for confidential apps with token rotation enabled. Neither field was parsed, so a PKCE profile stopped working 12 hours after login and needed a fresh `auth login`. Credentials now carry their expiry and refresh token, and are exchanged before use once they enter the renewal window
- **Concurrent invocations can no longer lose each other's writes.** Slack revokes a refresh token as soon as it is used, and the auth store was a read-modify-write with no locking. Every mutation now runs under an advisory lock on a sibling `auth.json.lock`, re-reading from disk inside the lock; `AuthStore::write` requires the lock guard, so an unlocked write does not compile
- **`search` requested the wrong scope.** `auth login` asked for the legacy `search:read`, which covers `search.messages` and not the Real-time Search API, so `slack-cli search` failed with `missing_scope` on every PKCE profile. Scopes are now declared per method in the API registry and unioned from there, which is what removes the hand-maintained list that had drifted
- `auth status --verify` and `auth logout` renewed nothing and used the stored token directly: verification reported a false failure on a profile every other command used successfully, and revocation silently failed against Slack while the local credential was deleted. Both now renew first
- A renewal that cannot complete — no client recorded, the exchange refused, the network down — no longer fails a command whose token is still valid; it warns, proceeds on the credential in hand, and retries next invocation. The failure is only fatal once the token is genuinely past its expiry
- `RUST_LOG` was documented but ignored: the log filter was built from a literal string. It is now read from the environment, and `.env` is loaded before arguments are parsed, so `SLACK_PROFILE`, `SLACK_CLI_CLIENT_ID` and `SLACK_CLI_CLIENT_SECRET` are honoured from a `.env` file too
- Token and client-secret prompts no longer echo to the terminal
- Bot and user scopes are recorded separately per credential rather than collapsed into one list
- `SECURITY.md` documented `cosign verify-blob` against `.sig` and `.pem` artifacts that releases do not publish; the command now matches the signature bundle the release workflow actually produces

### Changed

- **Breaking:** the auth store moves to schema 2. Existing stores are upgraded in place on first open. Profiles created by the previous PKCE flow hold a token that has already expired with no refresh token to renew it, so those need one `auth login`
- **Breaking:** `auth status --json` reports each token as `{token, expires_at, renewable, scopes}` rather than a masked string, and scopes move from the profile onto the credential
- **Breaking:** minimum supported Rust is 1.98.0
- Every pinned GitHub Action moved to its current release, and a scheduled workflow now opens a `cargo update` pull request for the transitive versions Dependabot does not track

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
