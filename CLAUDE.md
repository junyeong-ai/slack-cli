# slack-cli

Rust CLI for the Slack Web API. Single crate, async HTTP and Socket Mode, SQLite + FTS5 local cache. SQLite operations run on bounded blocking workers.

## Layout

```
src/
├── main.rs       CLI entry, command dispatch
├── commands/     Event command handlers
├── blocking.rs   Bounded execution of synchronous database operations
├── cli.rs        clap command definitions
├── config.rs     TOML config (user preferences + the OAuth app — no user or bot tokens)
├── format.rs     Output formatting (table / JSON)
├── lib.rs        Library re-exports
├── paths.rs      Platform config/cache directories (XDG on Unix, Known Folders on Windows)
├── tls.rs        TLS trust shared by every HTTPS and WSS connection
├── auth/        See src/auth/CLAUDE.md
├── slack/       See src/slack/CLAUDE.md
├── cache/       See src/cache/CLAUDE.md
├── events/      See src/events/CLAUDE.md
└── update/      See src/update/CLAUDE.md
```

## Build & test

The toolchain is pinned by `rust-toolchain.toml` (single source of truth —
rustup picks it up automatically; don't restate the version in commands). The
README badge and its build-from-source lines repeat it and the `rust-version`
for users, so a bump updates them too.

```bash
cargo nextest run --profile ci --all-features --workspace --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
```

Debug a single command:
```bash
RUST_LOG=debug cargo run -- users "john"
```

The remaining CI checks are `cargo check` at the `rust-version` declared in
`Cargo.toml`, `cargo deny --all-features --locked check`, `actionlint`,
`zizmor .github` (policy in `.github/zizmor.yml`),
`bash -n scripts/install.sh scripts/uninstall.sh`,
`shellcheck scripts/install.sh scripts/uninstall.sh`, and CodeQL.

The `main` ruleset, a repository setting rather than a file
(`gh api repos/junyeong-ai/slack-cli/rulesets`), requires these pull-request
jobs by exact name: `test (ubuntu-latest)`, `test (macos-latest)`,
`test (windows-latest)`, `Minimum supported Rust`, `rustfmt`, `clippy`,
`actionlint`, `zizmor` and `cargo-deny`. Renaming one of them, changing the
test matrix, or filtering when one runs leaves every pull request waiting on a
check that never reports, so update the ruleset when such a change merges.
Dependabot's non-major updates merge on their own once those checks pass
(`dependabot-auto-merge.yml`), after the three-day cooldown in
`.github/dependabot.yml`. Repository admins bypass the ruleset, which is how
release commits reach `main` directly.

## Cross-cutting rules

- **User-facing reference is the README.** CLI flag enumerations, scope lists, and field tables live in `README.md`. Do not duplicate them in submodule `CLAUDE.md` files. `README.en.md` is a translation, not a subset: `tests/readme_parity.rs` fails the build when a table row naming a command, flag, environment variable or exit code exists in one language and not the other.
- **`--json` is the output-mode bridge.** When output mode changes what the library should request from Slack (e.g. `include_message_blocks=true` and `highlight=false` for `search.context`), `main.rs` derives the request-shape options from the parsed `--json` flag and passes them to library code. Library types in `slack/` stay output-agnostic. Every `--json` document is written by `format::print_json`; event streams stay NDJSON. Diagnostics follow the same flag: under `--json` every stderr line is a JSON object (log records, the login prompt, the error envelope) except clap's usage errors (exit `2`), which are raised before the flag is read, and text logs are colored only on a terminal.
- **Defaults live in code, not docs.** `format::{UserField, ChannelField, MessageField}` is the field vocabulary shared by `--expand` and `config.toml [output]`, so an unknown name is refused rather than printed as nothing. Per-command defaults (`users_fields`, `channels_fields`, `messages_fields`) are declared in `config.rs` and applied in `format.rs`; `--expand` adds opt-in fields on top. Lean-by-default keeps AI agent context costs predictable, so a new field is opt-in unless every caller needs it. The READMEs and the skill list the defaults for their readers; a changed default updates them as well.
- **Scopes are derived, never listed by hand.** Each `slack/api_config.rs::API_METHODS` entry declares its scopes, `[auth].exclude_scopes` subtracts from their union, and `tests/documented_scopes.rs` holds the README to the result (`src/slack/CLAUDE.md`, "Scopes"). A command needs only its own methods' scopes: Slack's `missing_scope` refuses that command alone, and the channel directory several commands share reports the conversation types Slack refuses instead of failing. Config decides what is requested, never what may run, because a token's grant can differ from the config.
- **The environment is loaded before anything reads it.** `main` calls `dotenvy::dotenv()` as its first statement, ahead of `Cli::parse()`. clap binds `--profile` and `--client-id` to env vars at parse time and the log filter reads `RUST_LOG`, so loading later would silently ignore a `.env` for all three.
- **App-level credentials stay separate from installation credentials.** `TokenKind::App` only opens Socket Mode connections; `TokenPolicy::accepts` keeps it disjoint from `User`/`Bot`, so app-level scopes never enter the user/bot authorization request (`src/auth/CLAUDE.md`).
- **Targets require exact identity.** Name discovery may return partial matches; commands that act on a channel require one exact cached name or a channel ID. Never select a discovery result by position.
- **The event daemon reuses the library and owns nothing else.** `events/` builds on `slack/`, `auth/` and `config`; outside it, only the CLI (`main.rs`, `commands/events.rs`) and `format.rs`, which prints its types, use it. Its state lives outside `cache_dir` because the cache rebuilds itself on a schema change and Socket Mode never replays what it dropped.
- **Command output returns its errors.** Write stdout through `outln!` or `format::print_json`, which report a closed reader as `OutputError::Closed` and `main` ends quietly on it; `println!` panics instead, and `clippy::print_stdout` rejects it. Diagnostics go through `errln!`, which drops a failed stderr write so the command keeps its exit status; `clippy::print_stderr` rejects `eprintln!`. Prompts go to stderr too. The event daemon's stdout sink keeps its own writer (`events/sink.rs`).
- **Every connection shares one TLS trust.** HTTPS clients come from `tls::http_client_builder` and Socket Mode connects with `tls::client_config`, so a root the operating system trusts applies everywhere, and a system with none falls back to the bundled Mozilla roots in one place. `clippy.toml` disallows building a `reqwest` client or a WebSocket connection any other way.
- **Filesystem locations come from `paths.rs`.** `AppPaths` resolves the config root once per invocation and hands out `config.toml`, `auth.json` and the cache directory. No module reads `HOME` or `XDG_CONFIG_HOME` itself.
- **Database work leaves the async runtime.** Use `SqliteCache::execute`, `EventState::execute` or `EventRuntime::execute` for synchronous database operations from async code. Keep pool connections and transactions inside the blocking operation; never carry them across an await. Event preparation and commit remain separate from asynchronous sink delivery.
