# slack-cli

Rust CLI for the Slack Web API. Single crate, SQLite + FTS5 local cache, async/await throughout.

## Layout

```
src/
├── main.rs       CLI entry, command dispatch
├── cli.rs        clap command definitions
├── config.rs     TOML config (user preferences only — no tokens)
├── format.rs     Output formatting (table / JSON)
├── lib.rs        Library re-exports
├── auth/        See src/auth/CLAUDE.md
├── slack/       See src/slack/CLAUDE.md
└── cache/       See src/cache/CLAUDE.md
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

## Cross-cutting rules

- **User-facing reference is the README.** CLI flag enumerations, scope lists, and field tables live in `README.md`. Do not duplicate them in submodule docs.
- **`cli.json` is the output-mode bridge.** `main.rs` reads `cli.json` and derives request-shape options (e.g. `include_message_blocks`, `highlight` for search) before calling library code. Library types in `slack/` stay output-agnostic.
- **Defaults live in code, not docs.** CLI `--expand` field defaults are declared in `config.rs` and applied in `format.rs`. Adding a new default field belongs there, not in any markdown file.
