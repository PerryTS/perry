Removes the native mongodb binding (`crates/perry-ext-mongodb`). `import { MongoClient } from "mongodb"` (and `import * as`, `require("mongodb")`) now compiles npm mongodb from its own TypeScript source, like every other package. This is part of the native-binding removal campaign.

- Deleted:
  - the crate (with it the legacy mongodb 3.9 Rust driver, `turnloop-mongodb`, `bson` 3, `tokio-rustls` and `tokio-util` leave `Cargo.lock`) and its workspace member;
  - its `well_known_bindings.toml` row and the `module: "mongodb"` native-table rows;
  - the `js_mongodb_*` runtime declarations and the `new MongoClient(uri)` arm in `lower_builtin_new`;
  - the HIR `MongoClient` class routing, the API-manifest entries and the Android stub symbols;
  - the `mongodb` arms in the CLI's stdlib-feature, freshness and no-auto driver code, and the doc rows.

  The `upstream-pins.md` example now shows `bcrypt`, a binding that still exists.
- Re-derived by running each gate script on the resolved tree:
  - `native_result_ledger`: 294 → 282 rows, 260 → 248 providers.
  - `tokio_inventory --update`: 2 edges left, `perry-stdlib` and `perry-ui-android`'s tungstenite.
  - `workspace-architecture.json`: 70 → 69 members, externalize 12 → 11.
  - `unrooted_local_shape --update-baseline`: 382 → 380.
  - The two `perry-ext-mongodb` `gc_runtime_root_holders` entries are dropped.
  - The API reference and `perry.d.ts` are regenerated.
- The root devDependency moves from `^7.0.0` (which resolved to 7.0.0) to exactly `mongodb@7.5.0`. 7.0.0 compiled from source is still blocked by #11322 (`let u; u = new URL(...)` with a named `URL` import reads undefined properties, which breaks 7.0.0's `HostAddress`).
- New gap test `test_gap_mongodb_from_source`. It runs connect, `insertMany`, sorted `find`, `findOne`, `updateOne`, `countDocuments`, `deleteMany` and a final `find` against an in-process fake MongoDB wire-protocol server (`test-files/_helpers/fake_mongo_server.ts`), so CI needs no mongod. The fixture prints the resolved mongodb version.
