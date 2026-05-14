# slack/ — Slack Web API clients

Facade pattern: `SlackClient` owns one `Slack{Domain}Client` per domain. All clients share `Arc<SlackCore>`.

## API call flow (core.rs)

```
SlackCore::api_call(method, params)
  → get_api_config(method)           encoding, token policy, rate policy
  → method-level rate limiter        governor + Jitter::up_to(100ms)
  → token selection                  per TokenPolicy
  → HTTP via reqwest                 Query or Json encoding
  → retry on HTTP 429                respect Retry-After header
  → parse JSON, check `"ok"` field   Err on ok=false with the API's error string
```

Effective rate = `min(config.connection.rate_limit_per_minute, per-method rate)`. The per-method rate is the ceiling; user config can only lower it.

## Adding a new API method

1. **`api_config.rs`**: insert into `API_CONFIGS` with `RequestEncoding`, `TokenPolicy`, requests/min, max page limit.
2. **`slack/{module}.rs`**: add the method to the matching `Slack*Client`. Verb-only name, matching the Slack API verb (`list`, `add`, `history`, `members`, …).
3. **`cli.rs`**: add a `Command` variant. Match Slack API parameter names for fields; use clap `long = "..."` to keep user-facing flags terse.
4. **`main.rs`**: add the match arm. Resolve channel name → ID via `resolve_channel`; convert ISO dates via `parse_unix_seconds` / `parse_timestamp`.
5. **`format.rs`**: add a print function only if the response shape is genuinely new. Reuse existing printers where possible.

## Pagination shapes

Two patterns coexist by design:

| Shape | Used by | Caller responsibility |
|---|---|---|
| Returns `(Vec<T>, Option<cursor>)` | `messages.history` | Caller decides whether to follow cursor |
| Loops internally to a user `limit` | `search.context`, `messages.replies`, `users.list`, `channels.list` | Caller passes a total cap; method owns the loop |

Each internally-looping method defines its own `PAGE_SIZE` constant matching the Slack API's per-method max. Don't unify them — different endpoints cap at different sizes (search=20, replies=1000, users/channels=200).

## Real-time Search (`search.rs`)

`assistant.search.context` is the only RTS method we wire. Key invariants:

- `SearchOptions::MAX_LIMIT = 100` — user-facing total cap. Validate at the CLI layer (`parse_search_limit`) and clamp again at the library entry.
- `PAGE_SIZE = 20` — API hard limit per request. Never expose to callers.
- `TokenPolicy::UserRequired` — bot tokens would require an `action_token` lifted from a message event payload, which a CLI never receives.
- `SearchOptions` fields mirror the API parameter names exactly. CLI flag short forms (`--include-context`, `--include-archived`, `--no-semantic`) are CLI affordances only; map via clap `long = "..."`.

## Response shapes

Domain types in `slack/types.rs` are shared across modules. Module-local result structs (e.g. `SearchMessageResult`) stay private to their module unless re-exported via `slack/mod.rs`.
