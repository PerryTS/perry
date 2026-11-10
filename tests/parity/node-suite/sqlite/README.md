# node:sqlite granular parity suite

Focused deterministic cases for Perry's `node:sqlite` compatibility layer. The
suite intentionally starts with isolated in-memory databases and compares
observable behavior rather than SQLite-version-specific error text.

## Coverage

- `database/`: construction and option validation, deferred/open/close/dispose
  state, batch execution, SQL errors, defensive/DQS configuration, in-memory
  location, runtime limits, and serialization/deserialization.
- `statements/`: prepare validation; `run()`, `get()`, `all()`, and `iterate()`;
  null-prototype rows; change metadata; and iterator invalidation.
- `binding/`: anonymous and numbered positional parameters, prefixed and bare
  named parameters, unknown-name policy, null/text/number/BigInt/blob binding,
  every standard typed-array view, `DataView`, and invalid values.
- `metadata/`: `sourceSQL`, `expandedSQL`, `columns()`, `readBigInts`, and
  `returnArrays`, including per-statement overrides.
- `transactions/`, `functions/`, and `authorizer/`: commit/rollback/savepoints,
  stable constraint diagnostics, scalar functions, aggregate/window functions,
  callback/options validation, and authorizer allow/deny/ignore/clear behavior.
- `tag-store/`: caching, capacity/eviction, `size`, `clear`, `db`, all four query
  methods, closed-database invalidation, and call-shape validation.
- `sessions/`: deterministic in-memory changeset/patchset creation, table
  filtering, and changeset application.
- `extension-loading/`: policy controls and missing-extension diagnostics only;
  it never loads a native binary.

Each fixture owns and explicitly closes every database it opens. No fixture
creates a persistent database or depends on execution order.

## Upstream selection

The correctness oracle is the pinned Node.js 26.5.0 tree at
[`bebd1b8`](https://github.com/nodejs/node/tree/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel).
The selection was made from all 18 `test-sqlite*.js`/`.mjs` files there, with
the core behavior primarily drawn from
[`test-sqlite-database-sync.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-sqlite-database-sync.js),
[`test-sqlite-statement-sync.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-sqlite-statement-sync.js),
[`test-sqlite-named-parameters.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-sqlite-named-parameters.js),
[`test-sqlite-data-types.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-sqlite-data-types.js),
and
[`test-sqlite-template-tag.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-sqlite-template-tag.js).

Deno's corresponding selection was reviewed at
[`tests/unit_node/sqlite_test.ts` (`f8a17c8`)](https://github.com/denoland/deno/blob/f8a17c8171569fa2870d740030aaa59c91fdf9ee/tests/unit_node/sqlite_test.ts).
It independently emphasizes batch execution, mixed/numbered binding, blobs,
iterator reuse, serialization, sessions, aggregates, and `SQLTagStore`; those
deterministic overlaps are represented here. Bun's tree at
[`c4fad46`](https://github.com/oven-sh/bun/tree/c4fad462e7dc20e5e9780f848db42e1e2f52186d/test)
contains `bun:sqlite` tests but no `node:sqlite` compatibility selection, so its
SQLite-specific cases were not treated as Node API coverage.

## Stopping judgment and exclusions

The current boundary is the deterministic in-memory API. Remaining Node cases
were not copied when they were redundant with a smaller fixture or belonged to
one of these separate-risk categories:

- filesystem paths (`string`, `Buffer`, and `URL`), read-only databases,
  file-backed `location()`, backup, permissions, and timeout or concurrent-lock
  behavior;
- real extension loading and native extension binaries;
- optional SQLite build features and compile options (math/percentile, dbstat,
  FTS3/4/5, RTree, RBU, and Geopoly);
- large strings/databases, garbage-collection retention, leak/stress checks,
  worker/process concurrency, callback re-entrancy crash guards, corruption,
  and recovery;
- the full changeset conflict matrix and platform-sensitive authorizer or
  extension edge cases.

Those exclusions require dedicated runtime/native or filesystem batches rather
than weakening this suite's deterministic signal. Error fixtures assert stable
`name`/`code` and behavior; they deliberately avoid bundled-SQLite message text.

## Measured Perry diagnostics

Repeated clean focused runs against Node 26.5.0 measure **24/40**. All 40
fixtures exit cleanly under the Node oracle, and the 16 mismatches reproduce in
isolation; none are harness, temporary-path, cleanup, ordering, or host-Node
failures. They identify these stable compatibility gaps:

- numbered positional parameters; non-`Uint8Array` typed-array views; empty
  blob and symbol binding; and the `Symbol.for("sqlite-type")` property;
- timeout and limit validation/enumerability, default DQS error classification,
  and the currently missing serialize/deserialize surface;
- function-valued aggregate starts, scalar varargs with SQL `NULL`, aggregate
  window result callback count, and numeric `expandedSQL` formatting;
- iterator post-completion/reset invalidation, NUL-containing prepare error
  classification, and absent SQLite `errcode` metadata on constraint errors.

The passing cases independently confirm the remaining covered behavior rather
than masking those gaps behind feature detection or conditional assertions.
