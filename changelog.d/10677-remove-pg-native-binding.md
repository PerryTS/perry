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

Verified end to end without forcing `compilePackages`: a from-scratch
`node_modules` with a plain `"pg": "^8"` dependency and no
`perry.compilePackages` key compiles, links (24.7 MB binary), and reaches a
genuine `net.connect()` — `Connection refused` against a port with nothing
listening. No live Postgres was available to test a real query round-trip.
