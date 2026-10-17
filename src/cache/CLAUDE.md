# cache/ — SQLite + FTS5 local cache

r2d2 connection pool, WAL mode. Used to make name-based lookups (users, channels) instant and offline-tolerant.

## Two-phase search

```
Phase 1: LIKE with priority         exact match ranks first, cheap
  SELECT data FROM users WHERE name LIKE ?
Phase 2: FTS5 phrase MATCH          only if Phase 1 returned nothing
  SELECT data FROM users JOIN users_fts WHERE users_fts MATCH ?
```

LIKE searches substrings and ranks exact names first. FTS5 searches a quoted phrase only when LIKE returns no rows; it does not perform typo correction or approximate matching.

## FTS5 query sanitization (mandatory)

Any user-supplied query MUST pass through `helpers::process_fts_query` before reaching `MATCH`. The helper:
- trims surrounding whitespace and removes `*` and `%`
- doubles embedded double quotes and wraps the remaining input as a literal phrase
- returns an empty string when nothing remains after trimming and wildcard removal

**An empty string skips Phase 2 entirely.** Submitting an empty MATCH expression to FTS5 errors out the statement.

## Schema versioning

`schema::SCHEMA_VERSION` gates auto-migration on cache open. **Bump it whenever the cached column set or FTS5 table definition changes** — adding a generated column, changing tokenizer, renaming a JSON path. Any stored version other than the current one (older *or* newer — cache contents are refetchable, so rebuild is always safe) drops and recreates every table; without a bump the cache deserializes against a stale schema and silently returns wrong data.

## Atomic refresh

A refresh fetches Slack data before opening a write transaction. `BEGIN IMMEDIATE` serializes the snapshot replacement and its sync timestamp across processes. Readers retain the previous committed snapshot until the replacement commits. An error or a process exit rolls the transaction back; there are no application-managed lock leases. Concurrent refreshes may fetch the same data. Empty successful snapshots replace old rows and update the sync timestamp. Initialization is determined by that timestamp, independently of row count; a completed empty snapshot must not trigger a fetch on every lookup.

## Adding a cached field

1. **`slack/types.rs`**: add to the struct (or under `SlackUser::profile`, etc.).
2. **`cache/schema.rs`**: add a generated column whose expression extracts the JSON path. Bump `SCHEMA_VERSION`.
3. **`cache/{users,channels}.rs`**: if the field affects search ranking, also add it to the FTS5 virtual table column list and re-index after migration.

Async callers use `execute` for synchronous reads. Cache creation and snapshot writes already run on bounded blocking workers. The worker owns its transaction until completion, even when its async caller is cancelled.

The CLI places each cache below an authorization-specific directory. Stored profile identity and authorization time remain stable through token rotation; a new login gets a new cache. Environment user/bot credentials and API endpoints are isolated. An unpartitioned legacy database is never reused. FTS external-content delete/update triggers must supply the old indexed values through the FTS5 `delete` command.
