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
