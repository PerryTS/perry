turnloop P8 group H — **deleted perry-stdlib's bundled `pg` / `mysql2` /
`ioredis` / `mongodb` copies**, taking the crate's `sqlx`, `redis` and
`mongodb` manifest edges with them. `scripts/tokio_inventory.py` goes from
**29 edges to 26**. The fourth group-H edge, `tokio-rustls`, is **NOT**
removed — see "What did not move" below.

**What these were.** The pre-#466 in-stdlib database implementations, behind
`bundled-pg` / `bundled-mysql2` / `bundled-ioredis` / `bundled-mongodb` (and
their `database-postgres` / `-mysql` / `-redis` / `-mongodb` back-compat
umbrellas). Since v0.5.565-568 the well-known flip has stripped each of those
features and routed `import 'pg'` / `'mysql2'` / `'mysql2/promise'` /
`'ioredis'` / `'redis'` / `'iovalkey'` / `'mongodb'` to perry-ext-pg /
-mysql2 / -ioredis / -mongodb. They survived only as the fallback the flip
declined to — reachable under `PERRY_DISABLE_WELL_KNOWN=1` or when a wrapper's
source crate was missing from disk.

**Why deleting is safe on the JS surface.** Every `extern "C"` symbol the
bundled copies defined is also defined by the matching wrapper, and each
wrapper defines strictly more: pg 10 → 25, mysql2 15 → 30, ioredis 18 → 33,
mongodb 26 → 41. The wrappers are supersets; there is no JS surface that only
the bundled copy served.

**What it costs.** The two fallback paths now fail loudly instead of silently
serving an older implementation, matching the precedent set when the in-stdlib
fastify adapter was removed:

* `PERRY_DISABLE_WELL_KNOWN=1` + a db import is a clear compile-time error
  naming the variable, instead of linking the bundled copy.
* A workspace missing `crates/perry-ext-<driver>/` is a clear compile-time
  error naming the crate and the path it looked in, instead of silently
  falling back.

Ordinary builds are unaffected: the flip already routed every one of these
imports to the wrapper, and `PERRY_NO_AUTO_OPTIMIZE` still links the prebuilt
`libperry_ext_*.a` ahead of the stdlib archive.

**Removed:** `crates/perry-stdlib/src/{pg/,mysql2/,ioredis.rs,mongodb.rs}` and
`src/common/dispatch_ioredis.rs` (4,710 lines), their `lib.rs` module
declarations, the `bundled-mysql2` handle method/property dispatch arms and the
`database-redis` ioredis arm in `src/common/dispatch/`, the eight Cargo
features above, and the `sqlx` / `redis` / `mongodb` / `bson` / `futures-util`
optional dependencies. `database` is now `["database-sqlite"]`: rusqlite is not
a tokio driver and `dispatch_sqlite_stmt` is deliberately retained through the
flip (Refs #643).

`crates/perry/src/commands/stdlib_features.rs` maps the seven import spellings
to no feature at all (the `undici` / `node:http` shape), and
`optimized_libs/driver.rs` re-asserts `async-runtime` for them by module name —
the wrappers still settle promises through perry-stdlib's `perry_ffi_*` shim,
and anything named in `module_to_features` gets stripped by the flip loop.

**Measured:** perry-stdlib's own normal dependency closure drops from 502 to
426 packages (−76, −15%). `Cargo.lock` does **not** shrink — perry-ext-pg /
-mysql2 / -ioredis / -mongodb keep the same drivers in the graph, so the P8
report's "cheapest lockfile reduction in the tree" claim does not hold while
the wrappers exist. The real win is that building `libperry_stdlib.a` no longer
compiles sqlx, the redis client or the mongodb driver at all — which also makes
the release job more robust, since that archive is required while the ext
archives are best-effort per host.

**What did not move: `perry-stdlib -> tokio-rustls`.** Group H's framing
("compiled out of every default build") never applied to this edge, and the
inventory's own `reached_when` for it said so. It is not a bundled fallback: it
is the live `node:tls` implementation (`src/tls.rs` + `src/tls/`, ~3.7k lines),
the bundled net client's TLS, and `bundled-ws`'s `wss://` connector. The flip
does not strip it — it turns it **on**, inserting `external-tls-server` for
every `node:http` / `node:https` / `node:http2` import (the ext-net preflight
hook `js_tls_client_preflight` is defined there) and `external-net-tls`
whenever net/http own the transport. No perry-ext-* wrapper owns a TLS
*server*. Deleting it would delete `tls.createServer()` and `wss://`, not
relocate them; it needs an accept-side turnloop-tls session first, which is a
transport job rather than a policy one. The inventory entry has been rewritten
to say that instead of inheriting the deleted copies' blurb.

Also fixed in passing: `intern_syscall_for_test` in
`src/turnloop_client/exchange.rs` was missing the `#[cfg(test)]` its sibling
test seams carry, which made `cargo check --all-targets` under
`RUSTFLAGS=-D warnings` fail on dead code — the `warnings` job's exact
configuration.
