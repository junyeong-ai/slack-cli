# cache/ — SQLite + FTS5 local cache

r2d2 connection pool, WAL mode. Used to make name-based lookups (users, channels) instant and offline-tolerant.

## Two-phase search

```
Phase 1: LIKE with priority         exact match ranks first, cheap
  SELECT data FROM users WHERE name LIKE ?
Phase 2: FTS5 MATCH                 only if Phase 1 returned nothing
  SELECT data FROM users JOIN users_fts WHERE users_fts MATCH ?
```

Phase 1 wins for the common case (typing the start of a name). Phase 2 is the fuzzy fallback. Always run them in this order — FTS5 over short inputs is noisy and slower than LIKE.

## FTS5 query sanitization (mandatory)

Any user-supplied query MUST pass through `helpers::process_fts_query` before reaching `MATCH`. The helper:
- escapes FTS5 operators (`"`, `*`, `(`, `)`, ...)
- returns `Some("\"escaped\"")` when the sanitized form is non-empty
- returns `None` when the input would produce an empty FTS5 expression

**`None` → skip Phase 2 entirely.** Submitting an empty MATCH expression to FTS5 errors out the statement.

## Schema versioning

`schema::SCHEMA_VERSION` gates auto-migration on cache open. **Bump it whenever the cached column set or FTS5 table definition changes** — adding a generated column, changing tokenizer, renaming a JSON path. Existing caches with a lower version are rebuilt; without a bump they deserialize against a stale schema and silently return wrong data.

## Distributed locking

```rust
self.with_lock("users_update", || {
    // Atomic swap via temp table.
    // Lock auto-releases on Ok and on error.
}).await
```

Two CLI invocations refreshing the same target concurrently will not double-fetch. Lock keys are domain-scoped strings (`users_update`, `channels_update`). Don't reuse a key across domains.

## Adding a cached field

1. **`slack/types.rs`**: add to the struct (or under `SlackUser::profile`, etc.).
2. **`cache/schema.rs`**: add a generated column whose expression extracts the JSON path. Bump `SCHEMA_VERSION`.
3. **`cache/{users,channels}.rs`**: if the field affects search ranking, also add it to the FTS5 virtual table column list and re-index after migration.
