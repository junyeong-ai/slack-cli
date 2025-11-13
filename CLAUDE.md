# Slack CLI - AI Agent Developer Guide

Essential knowledge for implementing features and debugging this Rust CLI tool.

---

## Core Patterns

### Atomic Swap Pattern

**Implementation**:
```rust
pub async fn save_users(&self, users: Vec<SlackUser>) -> CacheResult<()> {
    self.with_lock("users_update", || {
        let tx = conn.transaction()?;
        tx.execute("CREATE TEMP TABLE temp_users (...)", [])?;
        // Batch insert to temp
        tx.execute("DELETE FROM users", [])?;
        tx.execute("INSERT INTO users SELECT * FROM temp_users", [])?;
        tx.execute("UPDATE metadata SET value = ? WHERE key = 'last_user_sync'", [now])?;
        tx.commit()?;
        Ok(())
    }).await
}
```

**Why**: Zero downtime, no partial reads, safe for concurrent CLI invocations.

**Failure Handling**: Atomic per resource type. If refresh fails, old cache remains valid until TTL expires (24h). No partial updates.

---

### Distributed Locking

**Mechanism**:
- `locks` table: key, instance_id, acquired_at, expires_at
- 3 retries + exponential backoff (500ms, 1s, 1s)
- Stale lock cleanup (expires_at < now)
- Lock timeout: 60 seconds

**Why**: Multi-process safe for concurrent `slack cache refresh` invocations.

**Pattern**:
```rust
self.with_lock("key", || {
    // Critical section - auto-releases even on error
}).await
```

---

### 2-Phase Search Algorithm

**Phase 1**: LIKE with priority (exact=0, name LIKE=1, email LIKE=2)
**Phase 2**: FTS5 if Phase 1 empty (handles typos/fuzzy)

**Why**: Avoids FTS5 overhead for simple exact/substring matches.

---

### FTS5 Query Sanitization

```rust
pub(super) fn process_fts_query(&self, query: &str) -> String {
    let cleaned = query.trim()
        .replace("\"", "\"\"")  // Escape quotes
        .replace("*", "")        // Remove wildcards
        .replace("%", "")        // Remove SQL wildcards
        .trim()
        .to_string();

    if cleaned.is_empty() { return String::new(); }
    format!("\"{}\"", cleaned)  // Wrap in quotes for phrase search
}
```

**CRITICAL**: Empty FTS5 query → skip FTS5, use LIKE fallback. Otherwise syntax errors.

**Usage**:
```rust
let fts_query = self.process_fts_query(query);
if fts_query.is_empty() {
    // Fall back to LIKE
}
```

---

## Development Tasks

### Add Cache Field

1. **slack/types.rs**: Update struct
   ```rust
   pub struct SlackUser {
       pub id: String,
       pub name: String,
       pub new_field: Option<String>,  // ADD
   }
   ```

2. **cache/schema.rs**: Add generated column
   ```rust
   new_field TEXT GENERATED ALWAYS AS (json_extract(data, '$.new_field'))
   ```

3. **Bump SCHEMA_VERSION**: `const SCHEMA_VERSION: i32 = 3;`

4. **Update FTS5** (if searchable):
   ```rust
   CREATE VIRTUAL TABLE users_fts USING fts5(name, email, new_field, ...);
   ```

**Note**: Schema version bump triggers full cache rebuild.

---

### Schema Migrations

**Policy**: On SCHEMA_VERSION bump, `initialize_schema()` drops and recreates all tables.

**No migrations** - full rebuild from Slack API.

**Reason**: Cache is ephemeral (24h TTL), simpler than maintaining migration scripts.

---

### Add Slack API Method

1. Add method to appropriate client (`slack/users.rs`, `slack/channels.rs`, etc.)
2. Define rate limit in `slack/api_config.rs`
3. Add response type to `slack/types.rs`

---

## Common Issues

### Cache Staleness

**Symptom**: Outdated user/channel data in search results.

**Check**:
```bash
slack cache stats
```

**Fix**:
```bash
slack cache refresh
```

**Note**: Cache has 24h TTL. After expiration, stale data returned until refresh.

---

### Lock Contention

**Symptom**: `LockAcquisitionFailed` after 3 retries

**Cause**: Multiple `slack cache refresh` processes running concurrently

**Solutions**:
1. Wait for other process to finish
2. Increase retries (edit `cache/locks.rs`):
   ```rust
   const MAX_RETRIES: u32 = 5;  // Increase from 3
   ```

**Lock cleanup**: Stale locks (>60s) auto-cleaned on next acquisition attempt.

---

### FTS5 Syntax Errors

**Symptom**: `sqlite3_prepare_v2` errors during search

**Cause**: Special characters not sanitized before FTS5 MATCH query

**Fix**: Always call `process_fts_query()` before FTS5:
```rust
let fts_query = self.process_fts_query(user_input);
if fts_query.is_empty() {
    // Use LIKE fallback
} else {
    // Safe to use FTS5 MATCH
}
```

**Critical chars**: `"`, `*`, `%`

---

## Key Constants

**Locations**:
- `cache/locks.rs`: Lock timeout (60s), max retries (3), backoff timing
- `cache/schema.rs`: Schema version (2)
- `slack/users.rs`: API pagination limit (200)
- `config.rs`: Cache TTL (24h), retry config, connection timeouts

**To modify**: Edit constant in source, or add to `Config` struct + `config.toml` for user configuration.

---

This guide contains only implementation-critical knowledge. For user documentation, see [README.md](README.md).
