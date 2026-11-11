# Databases

Perry natively implements clients for MySQL, PostgreSQL, SQLite, MongoDB, and Redis.

## MySQL

```typescript
{{#include ../../examples/stdlib/database/snippets.ts:mysql}}
```

## PostgreSQL

```typescript
{{#include ../../examples/stdlib/database/snippets.ts:postgres}}
```

## SQLite

```typescript
{{#include ../../examples/stdlib/database/snippets.ts:sqlite}}
```

### better-sqlite3 statement reuse

The stdlib better-sqlite3 provider uses the connection-owned rusqlite statement
cache for SQL validation and `run`, `get`, and `all`. The default capacity is
16 compiled statements per connection. SQL whose Rust whitespace trimming
would remove more than SQLite's ASCII whitespace is prepared uncached with
its original bytes, preserving SQLite validation and Unicode identifiers
(including trailing Unicode inside aliases or comments). Such SQL does not
promise compilation reuse. SQL handles retain their own raw-mode
flag and bind parameters separately on every call. Returning a statement to
the cache clears its bindings. Closing the connection drops its cache.

Reuse is bounded, not a permanent compiled statement per JavaScript handle.
Eviction, schema changes, and overlapping statement leases can require a new
compilation. SQLite recompiles changed schemas when stepping; result column
names and counts are read after that step, including empty query completion.
Metadata-only `columns()` reads use a fresh preparation without executing SQL.
These guarantees describe compilation reuse, not a measured latency speedup.

`run()` accepts statement shapes whose results are not used: a row-returning
statement such as `INSERT ... RETURNING` is stepped to completion, its rows are
discarded, and the reported change count comes from the connection. A step
error makes the call fail instead of reporting a completed run, and `get()` /
`all()` fail rather than returning the rows read before the error.

## MongoDB

`mongodb` has no native binding: `import { MongoClient } from "mongodb"`
compiles the real npm `mongodb` driver (and `bson`) from source, so the whole
driver API — cursors, `bulkWrite`, sessions and transactions, change streams —
behaves as it does on Node.

```typescript
{{#include ../../examples/stdlib/database/snippets.ts:mongodb}}
```

## Redis

```typescript
{{#include ../../examples/stdlib/database/snippets.ts:redis}}
```

## Next Steps

- [Cryptography](crypto.md)
- [Overview](overview.md) — All stdlib modules
