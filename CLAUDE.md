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

The toolchain is pinned by `rust-toolchain.toml` (single source of truth —
rustup picks it up automatically; don't restate the version in commands).

```bash
cargo nextest run --profile ci --all-features --workspace
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Debug a single command:
```bash
RUST_LOG=debug cargo run -- users "john"
```

## Cross-cutting rules

- **User-facing reference is the README.** CLI flag enumerations, scope lists, and field tables live in `README.md`. Do not duplicate them in submodule `CLAUDE.md` files.
- **`--json` is the output-mode bridge.** When output mode changes what the library should request from Slack (e.g. `include_message_blocks=true` and `highlight=false` for `search.context`), `main.rs` derives the request-shape options from the parsed `--json` flag and passes them to library code. Library types in `slack/` stay output-agnostic.
- **Defaults live in code, not docs.** Per-command field defaults (`users_fields`, `channels_fields`, `messages_fields`) are declared in `config.rs` and applied in `format.rs`. `--expand` adds opt-in fields on top. Lean-by-default keeps AI agent context costs predictable; adding a new default field belongs there, not in any markdown file.
