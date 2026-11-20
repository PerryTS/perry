Removed the mysql2 native binding so `import mysql from "mysql2"` resolves to
the real npm package, per the owner's decision to stop shipping hand-written
Rust reimplementations of npm packages that drift from the real thing.

Two locations turned out to be separate hand-written mysql2
reimplementations, both removed: `crates/perry-ext-mysql2` (the
governance-tracked, well-known-table crate a plain `import mysql from
"mysql2"` actually linked against) and `crates/perry-stdlib/src/mysql2/`
(~1870 lines, gated behind the default-on `bundled-mysql2` perry-stdlib
feature). The two defined identical `js_mysql2_*` symbol names in **separate,
documented-as-disjoint handle registries** (perry-ffi's vs. perry-stdlib's
`common::handle`), so in the default (`full`-feature) build both crates'
archives carried the same symbols — a live footgun, not merely dead code.

Also removed the `bundled-mysql2` HIR heuristic in
`perry-hir/src/lower/expr_call/native_module.rs` that recognized a
bundler-inlined (webpack/turbopack) `createPool`/`createConnection` call by
its config-object shape and routed it to perry-ext-mysql2's FFI symbols. That
workaround existed only because an AOT binary couldn't run mysql2's
`generate-function`-built row parsers (`new Function` at runtime); #10675's
`dyn_eval` class-expression support fixes that generally, so the workaround
is no longer needed.

Removed the supporting registry wiring: `well_known_bindings.toml`,
`NATIVE_MODULES` + manifest rows in `perry-api-manifest`, the
`native_table/databases.rs` MySQL2 codegen section, `ext_registry.rs` FFI
routing, `stdlib_features.rs` / `optimized_libs` driver+freshness wiring,
`PERRY_NATIVE_EXTENSION_PACKAGES` in `resolve.rs`, `workspace-architecture.json`,
the Android `stdlib_stubs.rs` FFI stubs, and the `unrooted-local-shape` /
`string-payload-access` / `native-result-ledger` baselines for the deleted
files and symbols. Fixed the two explicit `-p perry-ext-mysql2` cargo build
args in `.github/workflows/test.yml` and `run_doc_tests.sh`/`.ps1`, which
would otherwise fail with "no such package". Regenerated
`docs/src/api/reference.md`, `docs/api/perry.d.ts`, and
`docs/src/native-libraries/governance.md`'s generated table, and added a
"Completed source migrations" entry for mysql2 matching the existing
`slugify` entry.

The `dyn_eval` class-expression support this depends on (#10661/#10675)
is already on `main`.

Validated with a real query round trip against a live MySQL 9.6.0 server, with the
pinned `mysql2@3.23.2` as the only dependency of a throwaway fixture and **no
`perry.compilePackages` entry at all** — Perry's default automatic
package-routing path compiles mysql2 and its full dependency tree from
source, with the `generate-function` row-parser factories handled at runtime
via `dyn_eval`. `CREATE TABLE` / prepared `INSERT` / `SELECT` (rows and field
names) / prepared `DELETE` (`affectedRows`) / `DROP TABLE` on a connection,
plus a pool query — byte-for-byte identical output to
`node --experimental-strip-types` on the pinned Node 26.5.1.
