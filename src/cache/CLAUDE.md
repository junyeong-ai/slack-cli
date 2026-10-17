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
- returns a quoted string when the sanitized form is non-empty
- returns an empty string when the input contains no searchable text

**An empty string skips Phase 2 entirely.** Submitting an empty MATCH expression to FTS5 errors out the statement.

## Schema versioning

`schema::SCHEMA_VERSION` gates auto-migration on cache open. **Bump it whenever the cached column set or FTS5 table definition changes** — adding a generated column, changing tokenizer, renaming a JSON path. Any stored version other than the current one (older *or* newer — cache contents are refetchable, so rebuild is always safe) drops and recreates every table; without a bump the cache deserializes against a stale schema and silently returns wrong data.

## Atomic refresh

A refresh fetches Slack data before opening a write transaction. `BEGIN IMMEDIATE` serializes the snapshot replacement and its sync timestamp across processes. Readers retain the previous committed snapshot until the replacement commits. An error or a process exit rolls the transaction back; there are no application-managed lock leases. Concurrent refreshes may fetch the same data.

## Adding a cached field

1. **`slack/types.rs`**: add to the struct (or under `SlackUser::profile`, etc.).
2. **`cache/schema.rs`**: add a generated column whose expression extracts the JSON path. Bump `SCHEMA_VERSION`.
3. **`cache/{users,channels}.rs`**: if the field affects search ranking, also add it to the FTS5 virtual table column list and re-index after migration.

Async callers use `execute` for synchronous reads. Cache creation and snapshot writes already run on bounded blocking workers. The worker owns its transaction until completion, even when its async caller is cancelled.
