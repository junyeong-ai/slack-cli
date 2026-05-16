# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
