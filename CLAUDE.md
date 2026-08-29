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
├── paths.rs      Platform config/cache directories (XDG on Unix, Known Folders on Windows)
├── auth/        See src/auth/CLAUDE.md
├── slack/       See src/slack/CLAUDE.md
├── cache/       See src/cache/CLAUDE.md
└── update/      See src/update/CLAUDE.md
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

- **User-facing reference is the README.** CLI flag enumerations, scope lists, and field tables live in `README.md`. Do not duplicate them in submodule `CLAUDE.md` files. `README.en.md` is a translation, not a subset: `tests/readme_parity.rs` fails the build when a table row naming a command, flag, environment variable or exit code exists in one language and not the other.
- **`--json` is the output-mode bridge.** When output mode changes what the library should request from Slack (e.g. `include_message_blocks=true` and `highlight=false` for `search.context`), `main.rs` derives the request-shape options from the parsed `--json` flag and passes them to library code. Library types in `slack/` stay output-agnostic.
- **Defaults live in code, not docs.** Per-command field defaults (`users_fields`, `channels_fields`, `messages_fields`) are declared in `config.rs` and applied in `format.rs`. `--expand` adds opt-in fields on top. Lean-by-default keeps AI agent context costs predictable; adding a new default field belongs there, not in any markdown file.
- **Scopes are derived, never listed by hand.** Each entry in `slack/api_config.rs::API_METHODS` declares the scopes its method needs; `slack::scopes::required(kind)` unions them. `auth login` requests that set, `auth scopes` prints it, and `tests/documented_scopes.rs` fails the build when the README publishes anything else.
- **The environment is loaded before anything reads it.** `main` calls `dotenvy::dotenv()` as its first statement, ahead of `Cli::parse()`. clap binds `--profile`, `--client-id` and `--client-secret` to env vars at parse time and the log filter reads `RUST_LOG`, so loading later would silently ignore a `.env` for all three.
- **Filesystem locations come from `paths.rs`.** `AppPaths` resolves the config root once per invocation and hands out `config.toml`, `auth.json` and the cache directory. No module reads `HOME` or `XDG_CONFIG_HOME` itself.
