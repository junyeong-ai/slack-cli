# auth/ — Authentication subsystem

Single facade (`Authenticator`) resolves tokens for every Slack API call and owns the lifecycle behind them. Login strategies (Static, browser OAuth) write the same `Profile` shape into `auth.json`.

## Storage

- **`auth.json`** at the platform config directory (`paths::AppPaths::auth_store`), mode `0600` inside a `0700` directory on Unix — `restrict_file`/`restrict_directory` are no-ops elsewhere, so on Windows the file inherits the `%APPDATA%` ACL. Schema-versioned, atomic write via `tempfile::persist`. Machine-managed; do not hand-edit.
- **`auth.json.lock`** beside it. Advisory lock only, never read. It is a sibling because writes replace `auth.json` by rename, which would strand a lock held on the unlinked inode.
- **`config.toml`** carries the Slack app's `client_id` under `[auth]` and never a token. The id is the app's public identifier — it travels in the authorize URL — so recording it stores nothing secret.
- **Env vars** `SLACK_USER_TOKEN` / `SLACK_BOT_TOKEN` override the store entirely (CI / headless). `SLACK_PROFILE` (or global `--profile`) selects which stored profile is active for the invocation. `SLACK_CLI_CLIENT_ID` names the Slack app, outranking `config.toml [auth]` and outranked by `--client-id`.

## Layout

```
auth/
├── authenticator.rs   Authenticator facade — token_for(policy), renew, transact
├── cli_handler.rs     `slack-cli auth …` dispatch
├── credential.rs      Credential, Readiness, TokenKind, TokenSet
├── env.rs             EnvOverrides — SLACK_USER_TOKEN / SLACK_BOT_TOKEN
├── errors.rs          AuthError + OAuthError (thiserror)
├── method.rs          AuthMethod enum (Static, Pkce)
├── migrate.rs         Schema 1 → 2 upgrade
├── policy.rs          TokenPolicy — select(kind) / accepts(kind)
├── profile.rs         Profile, WorkspaceInfo
├── secret.rs          SecretString wrapper + masking + serde adapters
├── state.rs           AuthState (versioned) — root of the JSON file
├── store.rs           AuthStore — locking, versioned read, atomic guarded write
├── login/
│   ├── static_login.rs   Validate pasted tokens via auth.test → Profile
│   └── browser_login.rs  Bind loopback → authorize → Profile
└── oauth/
    ├── flow.rs          Authorization — consent screen through token exchange
    ├── client.rs        OAuthClient — the Slack app's client id
    ├── pkce.rs          RFC 7636 verifier + S256 challenge
    ├── callback.rs      LoopbackReceiver (127.0.0.1 only, single-shot accept)
    ├── browser.rs       `open` crate wrapper, honours --no-browser
    └── exchange.rs      POST oauth.v2.access (raw reqwest, no SlackCore)
```

## The credential model

A `Credential` is a token plus everything needed to keep it alive: `refresh_token`, `expires_at`, and the `scopes` it was granted. `TokenSet` holds at most one per `TokenKind`. Scopes live on the credential, never on the profile — Slack's bot and user scope namespaces are distinct and must not be merged.

`Credential::readiness(now)` is the single decision point:

| | | |
|---|---|---|
| `Ready` | no expiry, or expiry still ahead with nothing to renew from | use the token |
| `NeedsRenewal` | holds a refresh token and expiry is within 2 hours (or past) | exchange it |
| `Expired` | expiry has passed with no refresh token | tell the user to log in again |

A renewal that cannot complete — no client recorded, the exchange refused, the network failed — is only fatal once the token is genuinely past its expiry. Inside the window the command runs on the credential it already holds and the next invocation retries, so a transient failure never turns a working token into an error.

Renewal is driven only by the recorded expiry — never by reacting to an API error. Slack's `expires_in` is authoritative, so nothing has to be inferred from a failure.

## Invariants

1. **Tokens never reach `config.toml` or logs.** `SecretString` auto-zeroizes on drop and masks `Debug`. Tracing macros only see metadata, never token values.
2. **`Authenticator::token_for` is the only token-resolution path.** Env tokens take precedence and are never renewed — their lifetime belongs to the caller. Otherwise the profile is resolved via `AuthState::resolve`, then the credential's `readiness` decides.
3. **Only a lock holder can write.** `AuthStore::write` takes `&StoreGuard`, so a state read before the lock was acquired cannot be written after another process has committed its own. `Authenticator::load` re-reads under the lock before persisting a schema upgrade for the same reason.
4. **Every mutation is one cross-process transaction.** `transact` and `renew` take the store lock, re-read from disk, apply, write, and only then swap the in-memory copy. Re-reading under the lock is what makes concurrent invocations safe: Slack revokes a refresh token once it is used, so a sibling process that renewed a moment ago has already written the successor, and this process adopts it instead of spending a token that is gone.
5. **The lock is acquired on the blocking pool.** Waiting on another process is unbounded; doing it on a runtime worker would strand the task that is meant to release the lock.
6. **PKCE verifier and OAuth state are inputs, not outputs, of `Authorization::run_with`.** `run` is the convenience wrapper that generates them; tests pass fixed values.
7. **Callback server binds `127.0.0.1` only** on a fixed port (default `53682`, configurable via `--port`). Slack's redirect-URI matching is exact — no auto-fallback. Single accept, then drop.
8. **`oauth.v2.access` bypasses `SlackCore::api_call`.** It has no `Authorization: Bearer` header and a different response envelope. See `src/slack/CLAUDE.md` for the documented exception.
9. **Removing the active profile clears active.** No auto-promotion. The user picks the next active via `slack-cli auth use NAME`.
10. **Login with an auto-derived profile name rejects collisions.** If the team-slug name already maps to a different `team_id`, the user must pass `--profile NAME` explicitly.

## The browser flow is always PKCE

Slack rejects a loopback redirect that omits PKCE — *Must use PKCE to redirect to a non-web URI* — and rejects bot scopes on one outright — *Bot scopes are not allowed when redirecting to a non-web URI*. Every address a CLI can receive a callback on is such a URI, so there is no confidential-client variant to pick: the authorize URL always carries `code_challenge`/`S256` and `user_scope` alone, and the exchange sends `client_id` with no secret. Tokens from this flow always rotate. A bot token reaches the CLI only by being pasted into `--method static`.

This is RFC 8252 (BCP 212) as Slack enforces it: a distributed CLI cannot keep a client secret, so it authorizes as a public client.

## Schema migration

`store::read` probes `version` before deserializing and dispatches. Schema 1 is upgraded through `migrate::from_v1`, which deserializes the old shape into named types rather than editing JSON in place. `Authenticator::load` persists the upgrade under the lock so the file converges on first open.

Add a schema by bumping `state::SCHEMA_VERSION`, adding the version arm in `store::read`, and adding a `migrate::from_vN` with its own definition of the old shape.

## Adding a new auth method

1. `auth/method.rs`: add a variant to `AuthMethod`.
2. `cli.rs`: add a variant to `AuthMethodArg` and the `From` impl.
3. `auth/login/<name>_login.rs`: implement `pub async fn run(...) -> anyhow::Result<Profile>` returning a fully-populated `Profile`.
4. `auth/cli_handler.rs`: add the `decide_method` rule and the `login` match arm. If the strategy does not validate the token internally, call `slack.auth.test(token)` before `upsert_profile`.

A browser-based variant has to clear the loopback constraints above before it is worth building.

`Profile`, `Credential`, `TokenSet` are uniform across methods — only the acquisition path differs.
