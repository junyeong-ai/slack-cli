# auth/ — Authentication subsystem

Single facade (`Authenticator`) resolves tokens for every Slack API call. Login strategies (Static, PKCE) write the same `Profile` shape into `auth.json`.

## Storage

- **`auth.json`** at `${XDG_CONFIG_HOME:-~/.config}/slack-cli/auth.json`, mode `0600`. Schema-versioned, atomic write via `tempfile::persist`. Machine-managed; do not hand-edit.
- **`config.toml`** never contains tokens.
- **Env vars** `SLACK_USER_TOKEN` / `SLACK_BOT_TOKEN` override the store entirely (CI / headless). `SLACK_PROFILE` (or global `--profile`) selects which stored profile is active for the invocation.

## Layout

```
auth/
├── authenticator.rs   Authenticator facade — token_for(policy), upsert/remove/set_active
├── cli_handler.rs     `slack-cli auth …` dispatch
├── env.rs             EnvOverrides — SLACK_USER_TOKEN / SLACK_BOT_TOKEN
├── errors.rs          AuthError + OAuthError (thiserror)
├── method.rs          AuthMethod enum (Static, Pkce)
├── policy.rs          TokenPolicy enum + pick(user, bot) — re-exported by slack/api_config.rs
├── profile.rs         Profile, TokenSet, WorkspaceInfo
├── secret.rs          SecretString wrapper + masking + serde adapter
├── state.rs           AuthState (versioned) — root of the JSON file
├── store.rs           AuthStore — JSON atomic write, 0600
├── login/
│   ├── static_login.rs  Validate paste tokens via auth.test → Profile
│   └── pkce_login.rs    Bind loopback → run OAuth → Profile
└── oauth/
    ├── flow.rs          run_pkce / run_pkce_authorized (test seam)
    ├── pkce.rs          RFC 7636 verifier + S256 challenge
    ├── callback.rs      LoopbackReceiver (127.0.0.1 only, single-shot accept)
    ├── browser.rs       `open` crate wrapper, honours --no-browser
    ├── exchange.rs      POST oauth.v2.access (raw reqwest, no SlackCore)
    └── scopes.rs        REQUIRED_USER_SCOPES (kept in sync with API_CONFIGS)
```

## Invariants

1. **Tokens never reach `config.toml` or logs.** `SecretString` auto-zeroizes on drop and masks `Debug`. Tracing macros only see metadata, never token values.
2. **`Authenticator::token_for` is the only token-resolution path.** Env tokens take precedence; otherwise the active profile is resolved via `explicit_profile` (covers global `--profile` and `SLACK_PROFILE`, since clap binds the env on `Cli::profile`) then `AuthState::active_profile`.
3. **Mutations are save-then-commit.** `upsert_profile`, `remove_profile`, `set_active`, `clear_all` build a new `AuthState`, persist it, and only then swap the in-memory copy. A failed write never leaves memory ahead of disk.
4. **OAuth state and PKCE verifier are inputs, not outputs, of `run_pkce_authorized`.** `run_pkce` is the convenience wrapper that generates them; tests pass fixed values.
5. **Callback server binds `127.0.0.1` only** on a fixed port (default `53682`, configurable via `--port`). Slack's redirect-URI matching is exact — no auto-fallback. Single accept, then drop.
6. **`oauth.v2.access` bypasses `SlackCore::api_call`.** It has no `Authorization` header and a different response envelope. See `src/slack/CLAUDE.md` for the documented exception.
7. **Removing the active profile clears active.** No auto-promotion. The user picks the next active via `slack-cli auth use NAME`.
8. **Login with an auto-derived profile name rejects collisions.** If the team-slug name already maps to a different `team_id`, the user must pass `--profile NAME` explicitly.

## Adding a new auth method

1. `auth/method.rs`: add a variant to `AuthMethod`.
2. `cli.rs`: add a variant to `AuthMethodArg` and the `From` impl.
3. `auth/login/<name>_login.rs`: implement `pub async fn run(...) -> anyhow::Result<Profile>` returning a fully-populated `Profile`.
4. `auth/cli_handler.rs::login`: add a `match` arm. If the strategy does not validate the token internally, call `slack.auth.test(token)` before `upsert_profile`.

`Profile`, `TokenSet`, `WorkspaceInfo` are uniform across methods — only the acquisition path differs.
