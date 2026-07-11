# slack/ — Slack Web API clients

Facade pattern: `SlackClient` owns one `Slack{Domain}Client` per domain. All clients share `Arc<SlackCore>`.

## Method naming on `Slack*Client`

Verb-only, no noun redundancy. Match the Slack API verb when one exists.

- `messages.send`, `messages.update`, `messages.delete`, `messages.history`, `messages.replies`, `messages.permalink`
- `users.list`, `channels.list`, `channels.members`
- `reactions.add`, `pins.list`, `bookmarks.add`, `emoji.search`, `search.context`
- `auth.test`, `auth.revoke`

Never `send_message`, `fetch_all_*`, `get_*`. When the Slack API verb is `getX` (e.g. `chat.getPermalink`), drop the `get` prefix so the method reads as a noun.

## Message payload model (`MessagePayload`)

`chat.postMessage` and `chat.update` share the same payload surface (text, markdown_text, blocks, attachments, metadata). `MessagePayload` captures this once:

```rust
messages.send(channel, payload, thread_ts)   // post
messages.update(channel, ts, payload)         // edit (no thread routing)
```

- `payload.validate()` enforces ≥1 content field before any HTTP call, and rejects markdown_text alongside text or blocks (Slack's `markdown_text_conflict`).
- `into_post_json` adds `thread_ts` only on send; `into_update_json` never carries thread routing.
- New send/update fields belong on `MessagePayload`, not on call sites.

Metadata is a first-class field on `MessagePayload` and on `SlackMessage`. `conversations.history` and `conversations.replies` always request `include_all_metadata=true` so round-tripping idempotency markers needs no extra flag.

## API call flow (core.rs)

```
SlackCore::api_call(method, params)
  → get_api_config(method)           encoding, token policy, rate policy
  → method-level rate limiter        governor + Jitter::up_to(100ms)
  → token via Authenticator::token_for
  → HTTP via reqwest                 Query or Json encoding
  → retry on HTTP 429                respect Retry-After header
  → parse JSON, check `"ok"` field   Err on ok=false with the API's error string
```

Effective rate = `min(config.connection.rate_limit_per_minute, per-method rate)`. The per-method rate is the ceiling; user config can only lower it.

### Token policy

Declared per method in `api_config.rs::API_CONFIGS`. The enum lives in `auth::policy`:

- `BotPreferred` — bot first, user fallback
- `UserPreferred` — user first, bot fallback
- `UserRequired` — user only (e.g. `assistant.search.context`, where bot calls would need an `action_token` a CLI never receives)

`Authenticator::token_for(policy)` is the single resolution point. Domain clients never touch tokens directly.

### Exception: `oauth.v2.access`

The only Slack endpoint not routed through `SlackCore::api_call`. It has no `Authorization` header and uses a different response envelope. Lives in `auth/oauth/exchange.rs` with a dedicated `reqwest::Client`.

For ad-hoc validation with an explicit token (login flows before persistence), use `SlackCore::api_call_with(method, params, token)` — same retry/rate-limit/JSON-envelope handling, but the token comes from the caller instead of the `Authenticator`.

## Adding a new API method

1. **`api_config.rs`**: insert into `API_CONFIGS` with `RequestEncoding`, `TokenPolicy`, requests/min, max page limit.
2. **`slack/{module}.rs`**: add the method to the matching `Slack*Client`. Verb-only name matching the Slack API verb.
3. **`cli.rs`**: add a `Command` variant. Mirror Slack API parameter names for fields; use clap `long = "..."` for terse user-facing flags.
4. **`main.rs`**: add the match arm. Resolve channel name → ID via `resolve_channel`; convert ISO dates via `parse_unix_seconds` / `parse_timestamp`.
5. **`format.rs`**: add a printer only if the response shape is genuinely new. Reuse existing printers where possible.
6. If the method needs scopes not already in `auth/oauth/scopes.rs::REQUIRED_USER_SCOPES`, extend that list. Existing PKCE profiles need a fresh `auth login` to pick up new scopes.

## Pagination shapes

Two patterns coexist by design:

| Shape | Used by | Caller responsibility |
|---|---|---|
| Returns `(Vec<T>, Option<cursor>)` | `messages.history` | Caller decides whether to follow cursor |
| Loops internally to a user `limit` | `search.context`, `messages.replies`, `users.list`, `channels.list` | Caller passes a total cap; method owns the loop |

Each internally-looping method defines its own `PAGE_SIZE` constant matching the Slack API's per-method max. Don't unify them — different endpoints cap differently (search=20, replies=1000, users/channels=200).

## Real-time Search (`search.rs`)

`assistant.search.context` is the only RTS method wired in. Invariants:

- `SearchOptions::MAX_LIMIT = 100` — user-facing total cap. Validate at the CLI layer (`parse_search_limit`) and clamp again at the library entry.
- `PAGE_SIZE = 20` — API hard limit per request. Never expose to callers.
- `TokenPolicy::UserRequired` — bot calls would need an `action_token` lifted from a message event payload that a CLI never receives.
- `SearchOptions` field names mirror the API parameters exactly. CLI flag short forms (`--include-context`, `--include-archived`, `--no-semantic`) are CLI affordances mapped via clap `long = "..."`.

## Response shapes

Domain types in `slack/types.rs` are shared across modules. Module-local result structs (e.g. `SearchMessageResult`) stay private to their module unless re-exported via `slack/mod.rs`.
