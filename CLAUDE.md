# Slack CLI - AI Agent Developer Guide

Rust CLI for Slack API. SQLite FTS5 cache, async/await, facade pattern.

---

## Architecture

```
src/
├── main.rs              # CLI entry, command dispatch
├── cli.rs               # Clap command definitions
├── config.rs            # TOML config, env vars
├── format.rs            # Output formatting (table/JSON)
├── slack/
│   ├── core.rs          # HTTP client, rate limiting, retry
│   ├── client.rs        # Facade: 7 specialized clients
│   ├── api_config.rs    # API method configs (18 methods)
│   ├── messages.rs      # send, update, delete, search
│   ├── reactions.rs     # add, remove, get
│   ├── pins.rs          # add, remove, list
│   ├── bookmarks.rs     # add, remove, list
│   ├── emoji.rs         # list, search
│   ├── users.rs         # list (streaming pagination)
│   └── channels.rs      # list (streaming pagination)
└── cache/
    ├── sqlite_cache.rs  # r2d2 pool, WAL mode
    ├── schema.rs        # FTS5, generated columns
    ├── helpers.rs       # FTS5 query sanitization, cache status
    ├── locks.rs         # Distributed locking
    ├── background.rs    # Background auto-refresh
    ├── constants.rs     # Runtime constants
    ├── users.rs         # 2-phase search, ID lookup
    └── channels.rs      # 2-phase search, ID lookup
```

---

## Key Patterns

### API Call Flow
```rust
// slack/core.rs - All API calls go through this
SlackCore::api_call(method, params, files, prefer_user_token)
  → rate_limiter.until_ready()
  → get_api_config(method) // api_config.rs
  → HTTP request (GET/POST JSON/POST Form)
  → retry on 429 with Retry-After
  → parse JSON, check "ok" field
```

### Adding New API Method
1. `api_config.rs`: Add to `API_CONFIGS` map
2. `slack/{module}.rs`: Add method to appropriate client
3. `cli.rs`: Add Command variant
4. `main.rs`: Add match arm
5. `format.rs`: Add print function (if new type)

### Cache Search (2-Phase)
```rust
// Phase 1: LIKE with priority (exact match = 0)
SELECT data FROM users WHERE name LIKE ?
// Phase 2: FTS5 only if Phase 1 empty
SELECT data FROM users JOIN users_fts WHERE MATCH ?
```

### Distributed Locking
```rust
// cache/locks.rs - For concurrent cache refresh
self.with_lock("users_update", || {
    // Atomic swap via temp table
    // Auto-releases on success or error
}).await
```

---

## Critical Implementation Details

### FTS5 Query Sanitization
```rust
// cache/helpers.rs - MUST call before FTS5 MATCH
process_fts_query(query) → "\"escaped query\""
// Empty result → skip FTS5, use LIKE fallback
```

### Schema Version
```rust
// cache/schema.rs
const SCHEMA_VERSION: i32 = 2;
// Bump triggers full table recreation
```

### Rate Limiting
```rust
// slack/core.rs - Configurable rate limit with jitter
// Default: 20 req/min, configurable via config.connection.rate_limit_per_minute
governor::RateLimiter + Jitter::up_to(100ms)
```

### Token Priority
```rust
// api_config.rs
prefer_user_token: bool // true = user token for private channels
// core.rs: actual = param || config.prefer_user_token
```

---

## Common Tasks

### Add Cache Field
1. `slack/types.rs`: Add to struct
2. `cache/schema.rs`: Add generated column + bump SCHEMA_VERSION
3. Update FTS5 table if searchable

### Debug API Calls
```bash
RUST_LOG=debug cargo run -- users "john"
```

### Inspect Cache
```bash
sqlite3 ~/.config/slack-cli/cache/slack.db ".schema"
```

---

## Output Field Configuration

```rust
// config.rs - Configurable output fields
[output]
users_fields = ["id", "name", "real_name", "email"]     // default
channels_fields = ["id", "name", "type", "members"]     // default

// CLI: --expand adds fields to defaults
slack-cli users "john" --expand avatar,title

// format.rs - Dynamic field filtering for table/JSON output
filter_user_fields(user, fields) → serde_json::Value
get_user_field(user, field) → String
```

### Available Fields
- **users**: `id`, `name`, `real_name`, `display_name`, `email`, `status`, `status_emoji`, `avatar`, `title`, `timezone`, `is_admin`, `is_bot`, `deleted`
- **channels**: `id`, `name`, `type`, `members`, `topic`, `purpose`, `created`, `creator`, `is_member`, `is_archived`, `is_private`

---

## Constants

| Location | Constant | Value |
|----------|----------|-------|
| `cache/constants.rs` | MIN_REFRESH_INTERVAL | 3600s (1h cooldown) |
| `cache/constants.rs` | LOCK_TIMEOUT | 300s |
| `cache/constants.rs` | STALE_LOCK_THRESHOLD | 600s |
| `cache/schema.rs` | SCHEMA_VERSION | 2 |
| `config.rs` | rate_limit_per_minute | 20/min (configurable) |
| `config.rs` | ttl_users/channels_hours | 168h (1 week) |
| `config.rs` | refresh_threshold_percent | 10% |
| `config.rs` | users_fields | ["id", "name", "real_name", "email"] |
| `config.rs` | channels_fields | ["id", "name", "type", "members"] |

---

## Test Commands

```bash
cargo test                    # 93 tests
cargo clippy -- -D warnings   # Lint
cargo fmt --check             # Format check
```
