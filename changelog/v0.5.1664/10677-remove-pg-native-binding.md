Removed the native `pg` binding: `crates/perry-ext-pg` (sqlx::postgres +
tokio bridge over `perry-ffi`) and the duplicate pre-#466 in-tree
implementation in `crates/perry-stdlib/src/pg/` (the `bundled-pg` feature),
kept alive since before the migration to a separate ext crate. Both defined
the same `extern "C"` symbols (`js_pg_client_new`, `js_pg_client_query`, …);
whichever won the link order silently shadowed the other. `import ... from
"pg"` no longer resolves as a native module at all — it compiles the real
npm `pg` package from source, same as any other TypeScript/JavaScript
dependency, with `pg` and its 13 transitive deps (`pg-connection-string`,
`pg-pool`, `pg-protocol`, `pg-types`, `pgpass`, `pg-int8`,
`postgres-{array,date,interval,bytea}`, `pg-cloudflare`, `split2`, `xtend`)
picked up automatically by Perry's compile-package wildcard when a project
has no `perry.compilePackages` entry, or explicit listing otherwise.

Removed the `[bindings.pg]` entry (`well_known_bindings.toml`), the `"pg"`
`NATIVE_MODULES` entry and manifest rows (`perry-api-manifest`), the pg
`NativeModSig` dispatch-table rows (`perry-codegen`'s
`lower_call/native_table/databases.rs`), the `stdlib_features.rs` /
`optimized_libs` feature-gate arms, the `bundled-pg`/`database-postgres`
Cargo features and the now-unreachable `sqlx` `"postgres"` feature on
`perry-stdlib`'s dependency (verified nothing else in the workspace
requests it), and the `perry-ext-pg` entry in `workspace-architecture.json`.
Regenerated `docs/api/perry.d.ts`, `docs/src/api/reference.md`, and
`docs/src/native-libraries/governance.md`'s generated table; updated
`docs/src/native-libraries/overview.md`'s well-known-binding description.

The removal also had to reach the call sites that still named the deleted
symbols, which the first pass missed: `lower_call/builtin.rs` lowered
`new Client(cfg)` / `new Pool(cfg)` from an `import ... from "pg"` straight to
`js_pg_client_new` / `js_pg_pool_new` (undefined at link time once the
providers are gone — and the real `pg` package constructs `new Client`), the
ten `js_pg_*` externs in `runtime_decls/stdlib_ffi/data_stores.rs`, the seven
`js_pg_*` stubs in `perry-ui-android/src/stdlib_stubs.rs`, `"pg"` in
perry-codegen-js's browser-throw list, and `-p perry-ext-pg` in
`scripts/run_doc_tests.sh` / `.ps1`. Dropping the `"Client" | "Pool" =>
Some(&["pg"])` import gate together with the two arms leaves a user-defined
`Pool`/`Client` on the generic path, which is what #536 wanted anyway.

Verified end to end against a live PostgreSQL 16.13 server, with the pinned
`pg@8.22.0` as the only dependency of a throwaway fixture and no
`perry.compilePackages` key: `CREATE TABLE` / parameterized `INSERT` /
`SELECT` (rows, `rowCount`, `command`, field names) / parameterized `DELETE` /
`DROP TABLE` on a `Client`, plus a `Pool` query — byte-for-byte identical
output to `node --experimental-strip-types` on the pinned Node 26.5.1.
