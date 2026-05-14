# slack-cli

Rust CLI for the Slack Web API. Single crate, SQLite + FTS5 local cache, async/await throughout.

## Layout

```
src/
├── main.rs       CLI entry, command dispatch
├── cli.rs        clap command definitions
├── config.rs     TOML config, env vars, token resolution
├── format.rs     Output formatting (table / JSON)
├── lib.rs        Library re-exports
├── slack/        See src/slack/CLAUDE.md
└── cache/        See src/cache/CLAUDE.md
```

## Build & test

```bash
cargo +1.95.0 nextest run --profile ci --all-features --workspace
cargo +1.95.0 clippy --all-targets --all-features -- -D warnings
cargo +1.95.0 fmt --all -- --check
```

Debug a single command:
```bash
RUST_LOG=debug cargo run -- users "john"
```

## Cross-cutting conventions

### Method naming on `Slack*Client`
Verb-only, no noun redundancy. Match the Slack API verb when one exists.
- `messages.send`, `messages.update`, `messages.delete`, `messages.history`, `messages.replies`
- `users.list`, `channels.list`, `channels.members`
- `reactions.add`, `pins.list`, `bookmarks.add`, `emoji.search`, `search.context`

No `send_message`, no `fetch_all_*`, no `get_*` prefixes.

### All HTTP goes through `SlackCore::api_call`
Per-method behaviour (encoding, token policy, rate limit) is declared once in `slack/api_config.rs::API_CONFIGS`. Never call `reqwest` directly from a domain client.

### Token policy
Declared per method in `api_config.rs`:
- `BotPreferred` — bot first, user fallback
- `UserPreferred` — user first, bot fallback
- `UserRequired` — user only (use when bot calls would need an `action_token` the CLI cannot supply)

### Output mode flows through the CLI bridge
`cli.json` is read in `main.rs` and used to derive request-shape options (e.g. `include_message_blocks`, `highlight` for search). Library types in `slack/` stay output-agnostic.

### Fields and constants live in code
CLI `--expand` field lists and defaults are defined in `config.rs` and `format.rs`. Do not duplicate them in docs. The README's "Available Fields" tables are the user-facing source of truth.
