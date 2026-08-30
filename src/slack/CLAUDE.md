# slack/ — Slack Web API clients

Facade pattern: `SlackClient` owns one `Slack{Domain}Client` per domain. All clients share `Arc<SlackCore>`.

## Method naming on `Slack*Client`

Verb-only, no noun redundancy. Match the Slack API verb when one exists.

- `messages.send`, `messages.update`, `messages.delete`, `messages.history`, `messages.replies`, `messages.permalink`
- `users.list`, `channels.list`, `channels.members`
- `reactions.add`, `pins.list`, `bookmarks.add`, `emoji.search`, `search.context`, `search.info`
- `auth.test`, `auth.revoke`, `apps.connection`

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
  → get_api_config(method)           encoding, token policy, rate policy, scopes
  → method-level rate limiter        governor + Jitter::up_to(100ms)
  → token via Authenticator::token_for
  → HTTP via reqwest                 Query or Json encoding
  → retry on HTTP 429                respect Retry-After header
  → parse JSON, check `"ok"` field   Err on ok=false with the API's error string
```

Effective rate = `min(config.connection.rate_limit_per_minute, per-method rate)`. The per-method rate is the ceiling; user config can only lower it.

### Token policy

Declared per method in `api_config.rs::API_METHODS`. The enum lives in `auth::policy`:

- `BotPreferred` — bot first, user fallback
- `UserPreferred` — user first, bot fallback
- `UserRequired` — user only (e.g. `assistant.search.context`, where bot calls would need an `action_token` a CLI never receives)
- `AppRequired` — the app-level token only (`apps.connections.open`). Disjoint from the installation axes in both directions

`Authenticator::token_for(policy)` is the single resolution point. Domain clients never touch tokens directly.

`TokenPolicy::accepts(kind)` answers the same question statically: whether a token of that kind can ever satisfy the method. That is what keeps unusable scopes out of an installation — a `UserRequired` method contributes nothing to the bot scope set.

### Scopes

`MethodScopes` on each entry of `API_METHODS` records what a token must carry for *this CLI's* use of the method. Where the CLI always sends an optional argument that widens the requirement, the scope behind it belongs there too: `include_all_metadata` on conversation reads pulls in `metadata.message:read` for bot tokens — Slack supports that scope on no other kind — and the default `email` output field pulls in `users:read.email`.

`MethodScopes` carries a third field, `app`, for the same reason `TokenPolicy` carries a fourth variant: an app-level scope belongs to a different namespace and is never part of an OAuth grant. Declaring `connections:write` under `user` or `bot` would union it into what `auth login` asks Slack for, and Slack refuses a grant that mixes the two — the failure `metadata.message:read` once caused. `MethodScopes::installation` and `::app_level` are the constructors that make the axis explicit.

`slack::scopes::required(kind)` is the union over every method the kind can reach, so a scope can never drift from the methods that need it. `tests/documented_scopes.rs` holds both READMEs to the same source.

`scopes::requested(kind, excluded)` is that union less `config.toml [auth].exclude_scopes`, and is what `auth login` asks Slack for and what `auth scopes` prints. Slack grants a scope set atomically, so an app that cannot register one scope cannot be installed at all; subtracting it costs only the methods that declare it. `scopes::is_known` validates each exclusion against the registry, so a name no method needs is refused rather than silently doing nothing.

When Slack answers `missing_scope`, `SlackCore` reports which scopes the method declares for the kind of token it sent — `Authenticator::token_for` returns that kind for this reason. There is no pre-flight check: a pasted token records no scopes, so the only sound signal is Slack's own refusal.

### Exception: `oauth.v2.access`

The only Slack endpoint not routed through `SlackCore::api_call`. It has no `Authorization` header and uses a different response envelope. Lives in `auth/oauth/exchange.rs` with a dedicated `reqwest::Client`.

For ad-hoc validation with an explicit token (login flows before persistence), use `SlackCore::api_call_with(method, params, token)` — same retry/rate-limit/JSON-envelope handling, but the token comes from the caller instead of the `Authenticator`.

## Adding a new API method

1. **`api_config.rs`**: insert into `API_METHODS` with `RequestEncoding`, `TokenPolicy`, `RatePolicy` and `MethodScopes`. `Query` is a GET, `Json` a POST with a JSON body — the convention Slack accepts almost everywhere — and `Form` a POST with `application/x-www-form-urlencoded`, for the methods where the documented content type is the only assurance there is. `apps.connections.open` is the one such method here, because the daemon cannot start without it and nothing else would reveal a mismatch. The scopes are declared here and nowhere else — the OAuth request set and the README both derive from them.
2. **`slack/{module}.rs`**: add the method to the matching `Slack*Client`. Verb-only name matching the Slack API verb.
3. **`cli.rs`**: add a `Command` variant. Mirror Slack API parameter names for fields; use clap `long = "..."` for terse user-facing flags.
4. **`main.rs`**: add the match arm. Resolve channel name → ID via `resolve_channel`; convert ISO dates via `parse_unix_seconds` / `parse_timestamp`.
5. **`format.rs`**: add a printer only if the response shape is genuinely new. Reuse existing printers where possible.

Adding scopes in step 1 fails `tests/documented_scopes.rs` until both READMEs are updated. Existing profiles need a fresh `auth login` to pick up new scopes.

## Pagination shapes

Two patterns coexist by design:

| Shape | Used by | Caller responsibility |
|---|---|---|
| Returns `(Vec<T>, Option<cursor>)` | `messages.history` | Caller decides whether to follow cursor |
| Loops internally to a user `limit` | `search.context`, `messages.replies`, `users.list`, `channels.list` | Caller passes a total cap; method owns the loop |

Each internally-looping method defines its own `PAGE_SIZE` constant matching the Slack API's per-method max. Don't unify them — different endpoints cap differently (search=20, replies=1000, users/channels=200).

## Real-time Search (`search.rs`)

Both RTS methods are wired in: `assistant.search.context` runs the query, `assistant.search.info` reports whether the workspace can rank semantically at all. Invariants:

- `SearchOptions::MAX_LIMIT = 100` — user-facing total cap. Validate at the CLI layer (`parse_search_limit`) and clamp again at the library entry.
- `PAGE_SIZE = 20` — API hard limit per request. Never expose to callers.
- `TokenPolicy::UserRequired` — bot calls would need an `action_token` lifted from a message event payload that a CLI never receives.
- `SearchOptions` field names mirror the API parameters exactly. CLI flag short forms (`--include-context`, `--include-archived`, `--no-semantic`) are CLI affordances mapped via clap `long = "..."`.
- `search.info` needs only `search:read.public` and takes `UserPreferred`, not `UserRequired`: unlike `context` it carries no `action_token` requirement, so a bot token answers it too.

## Response shapes

Domain types in `slack/types.rs` are shared across modules. Module-local result structs (e.g. `SearchMessageResult`) stay private to their module unless re-exported via `slack/mod.rs`.
