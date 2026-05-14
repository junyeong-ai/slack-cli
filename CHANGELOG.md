# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
