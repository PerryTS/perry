## Highlights

This is the first release since v0.5.1520 (2026-09-11). The per-PR entries follow under "All changes". Numbers below come from the linked fragments or from the measured results in the project plan; PRs without a fragment are cited by number only.

### Upgrade notes and breaking changes

**Native npm bindings were removed; the real package is compiled from source.** If your project imports one of these, the package must now be installed in `node_modules` (no `perry.compilePackages` entry is needed). The old bindings were hand-written Rust copies that differed from upstream, for example `jsonwebtoken.verify` accepting forged tokens, `decimal.js` division returning `1`, and `commander` never running `.action()` callbacks.
- `axios` (#10679), `pg` (#10677), `mysql2` (#10680), `mongodb` (#11337), `ioredis`, `iovalkey` and `redis` (#11238)
- `jsonwebtoken` (#10687), `validator` (#10690), `dotenv` (#10691), `nanoid` (#10693), `uuid` (#10701)
- `decimal.js`, `big.js`, `bignumber.js` (#10704), `lru-cache` (#10708), `commander` (#10712)
- `qs`, `fastify`, `dayjs`, `date-fns`, `rate-limiter-flexible`, `node-cron` (#10765)
- the bare `fetch` alias for `node-fetch` (#10618). Benchmarks for the removed bindings are in #11466.

**Native GC roots (statepoints) are the default** (#7366). `PERRY_RS4GC=1` is no longer needed; `PERRY_RS4GC=0` goes back to the shadow stack on targets that support it. Targets whose frames the runtime cannot walk keep the shadow stack.

**The GC call-effect tables are no longer compiled into the compiler.** perry reads a `.gc-effects` sidecar that ships beside the prebuilt runtime archive and checks it against the archive's SHA-256 on every compile (#12735, #12660, #12674). A workspace build with a missing or stale sidecar stops with an error naming `scripts/gc_call_effects/sidecars.sh <dir>`. An installed perry without a sidecar compiles with every runtime call treated as a safepoint and prints one warning. After building the runtime, run that script on the output directory.

**No runtime rebuild at compile time.** perry links the prebuilt runtime and no longer runs cargo during a compile (#12684, #12694, #12701, #12704). An installed perry links the prebuilt full-feature stdlib and no longer pulls every optional subsystem into every program (#11598).

**Environment variables.**
- Removed: `PERRY_GC_ZEAL` and `PERRY_GC_ZEAL_ALLOC_KB`; use `PERRY_GC_SCHEDULE_SEED`, `PERRY_GC_SCHEDULE_RATE=1` and `PERRY_GC_SCHEDULE_ALLOC_KB` (#7741). Removed: `PERRY_LL_PREOPT_OPTNONE_INSTRS` (#8583).
- Changed defaults: the 16 MB nursery cap and the evacuating scavenge are on by default (`PERRY_GC_SCAVENGE=0` turns both off). The fragment measures peak RSS about 69% lower on the `gc_ratchet` probes (#7377). Old-page defragmentation is opt-in (`PERRY_GC_OLD_DEFRAG=1`, #7922).
- New: `PERRY_REQUIRE_FAITHFUL_BINDINGS=1` makes preferring a partial bundled binding over an installed `node_modules` copy a hard error; otherwise it prints a note per package. `perry.compilePackages` may be `"auto"`, `"all"` or `true`, and auto-compile is the default when it is omitted (#7137).

**Compiler behavior.**
- Native code is always built with `-O3`. Perry no longer drops a large module to `-Os`/`-O0` or marks a function `optnone` (#8421).
- Toolchain output (Cargo, clang, Swift, linker) is hidden unless it fails or you pass `--verbose` (#8421).
- The default number of concurrent codegen-unit workers is half the logical CPUs, clamped to 2–8, on non-Windows (#8593). Large bundles use more memory at once.
- `RegExp` runs only on Perex, with no `regex`/`fancy-regex`/`regress` fallback (#10142). The `v` flag is complete (#10372).

**JavaScript behavior that changed to match Node.**
- Error subclasses no longer have an own, enumerable `name`. It stays on the prototype until you assign it, which changes `JSON.stringify`, `Object.keys`, spread and `util.inspect` output (#9511).
- Native handles (Headers, Blob, Request/Response and similar) are objects in every coercion again (#12540).
- Fractional and very large TypeScript numeric enum values are kept as JavaScript numbers (#12788).
- `instanceof`, class method calls and `super` follow the live prototype chain, including after `setPrototypeOf` (#12409, #12466, #12784).

**Rust and FFI embedders.**
- `perry-ffi` is published to crates.io, and its ABI changed (#8122).
- perry-ffi 0.6 is breaking: one typed JS body ABI `body(callee, this, a0, …)`, `alloc_closure` takes a `js_function_info!`, `register_closure_arity` is removed (#11637).
- tokio is gone from the workspace. `spawn_async` and `spawn_blocking_with_reactor` are retired, and turnloop is the only event loop (#11402).
- The `ObjectHeader` shrank from 56 to 48 bytes for a two-field object (#8122), and the `perry-ext-streams` crate was removed; Web Streams stay in `perry-stdlib` (#12439).
- UI: `setPadding(widget, top, left, bottom, right)` is the canonical call; `widgetSetEdgeInsets` stays as a deprecated alias (#9962).
- Contributors: CI pins the Rust toolchain to `nightly-2026-08-20` (#8550).

### Object model

Objects, classes and prototypes use compiler-owned layouts. Property reads, method calls and stores are resolved from the layout, and the older inline caches, memo tables and certificates were deleted (#12703). Related PRs:
- builtin and node prototypes are sealed layouts (#12675, #12690, #12708, #12716) and stream/EventEmitter instances have static layouts (#12689, #12721)
- class layout families resolve across the import graph (#12782) and across modules for subclasses (#12800)
- namespaces, literal builders and CommonJS export calls use nominated layouts (#12802), and constructors with many stores compile smaller (#12804)
- inherited reads, `in` and stores work on prototype chains of any length (#12698)
- `super` calls follow live constructor prototypes (#12784) and native constructors keep own properties on sealed layouts (#12801)

Dynamic code (`eval`, `new Function`, dynamic `require`) is a single summary node over escaped values rather than disabling analysis for the whole program (#12707).

### Garbage collector

- Statepoints are the default root mechanism (above). The runtime call-effect tables moved out of the compiler into per-archive sidecars (#12660, #12674, #12735), and prod-profile archives prove 27 more call-effect rows (#12754).
- Dropping runtime frame pointers in favor of an unwind-table `Error.stack` is measured at 2.5–3.8% faster on every row (#12706, #12714).
- A moving-GC root-reload bug was fixed (#12712), and many rooting fixes are in the per-PR list. A static root-dominance checker gates the rooting changes.
- Windows and macOS have their own effect tables and a `cargo check` gate for runtime changes (#12706 broke Windows once; the gate is the fix).

### Compile speed and memory

- cpar parallel codegen takes the Next.js compile from 19 to 5 minutes (#12711).
- The cfront front-end prefetch (#12705) moved the tsc front end from 43.9 s to 35.9 s with byte-identical objects (#12728).
- Whole-program facts are computed faster (#12686). The per-module object cache and the completed-build cache share one compile-environment identity (#12582), and cached units are checkpointed atomically (#9997).
- Compile memory is released across phase boundaries (#12813), and class analysis is bounded by graph distance (#12814).
- Compile wall time and peak RSS on 2026-10-17 (plan, qb6): tsc 286 s / 9.7 GB, effect 31 s / 3.8 GB, schema-record 30 s / 3.9 GB. `perry compile --report-size` writes a per-crate and per-symbol size report (#8576).

### Node and ecosystem compatibility

- Next.js 16 App Route serving is byte-identical to node (#12673, #12678, #12692) and is tracked by a pinned 16.3.x production fixture (#10007).
- Namespaces, including `enum` + `namespace` merging, compile with direct calls (#12775, #12802).
- `events`, `stream`, `http` (`IncomingMessage`, `ServerResponse`), `worker_threads` (no multi-second stalls, #12448; `terminate()` stops any loop, #12762), `perf_hooks`, `dgram`, WebSocket upgrade (#12460) and `node:test` mocking (#12747) received parity fixes. See the per-PR list.
- Bun: `Bun.Transpiler`/`Bun.build` subsets (#9602), `bun:sqlite` (#8525), SQL (#9634), zero-copy `bun:ffi` views (#8329).
- On the 2026-10-17 scoreboard, all 23 program rows print the same output as node. Rows above 2× node by instructions: fastify, schema-record, qs, zod, commander, effect, property-prototype. tsc is 1.72×; the rest are under 1.8×.

### Platforms

- Linux musl x86_64/aarch64 release packages build again (#9584, #9298). Android UI apps link `perry-stdlib` (#11290, #12768).
- macOS UI builds produce signed `.app` bundles (#10240). Windows gets `fork()` IPC over named pipes (#8318), libuv-shaped errnos and `node:net` `err.errno` (#10385, #10403), `file://` drive letters (#12545) and `fs.fchown` parity (#12525). visionOS heap guards were fixed (#12247).
- `perry compile --target wasi` links a WASIp2 component behind the off-by-default `target-wasi` feature (#11413). Target detection covers OpenHarmony aliases (#12400).

### Known gaps

- `import G = Ns.Member` (TypeScript import-alias of a namespace member) is not supported.
- ES2025 `Set` methods (`union`, `intersection`, and the rest) are not supported.
- Promise/timer ordering differs from node in one case that is not yet root-caused.
- Typed arrays larger than 1 GiB are not supported, and allocation-exhaustion behavior differs from node.
- On the competitive suite at 2026-10-18, 218 of 246 rows pass; the remaining rows mostly fail from the long-standing 2026-10-03 list.
- Compiling opencode needs very large memory (an attempt with 16 module jobs reached about 84 GB); use fewer jobs.
- Binary size: the zod build is about 1.1 MB (+11%) larger text after the `Symbol.hasInstance`/`super` fix (#12784); a follow-up is planned.
