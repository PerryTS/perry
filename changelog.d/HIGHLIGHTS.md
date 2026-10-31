## Highlights

This is the first release since v0.5.1520 (2026-09-11). The per-PR entries follow under "All changes". Numbers below come from the linked fragments or from measured results; PRs without a fragment are cited by number only.

### What is new in this release

**Faster, smaller programs.** On the js-compiled benchmark set, all 22 programs compile and print the same output as node, with this release and with v0.5.1520. Median run time, v0.5.1520 to this release: nbody 9368 to 364 ms, matmul 27916 to 182 ms, generators 20724 to 394 ms, array-sort 3300 to 102 ms, crc32 2804 to 524 ms. Compiled binaries shrank from about 17 MB to 6.5 MB, and hello world starts in 0.9 ms instead of 2.6 ms. No perry benchmark in that set is more than 10% slower than before (the worst is mandelbrot at +7%).

**Startup.** Small programs start faster: the runtime builds its object layouts when the first such object is born, maps its fixed caches on first use, and builds per-thread state in place. On macOS arm64, hello world takes 2.33 ms instead of 2.95 ms and uses 5.3 MB instead of 6.0 MB peak RSS (#13345); a further 57,000 instructions (8.5% of hello on x86-64) were removed (#13484), and Linux no longer parses `/proc/self/maps` for the stack bounds (#13456).

**Memory.** Large strings, arrays and objects are allocated in a young large-object space. Measured in the PRs: string building -22% RSS and JSON parsing -25% instructions (#13479), file reading -39% RSS and -74% instructions (#13463), and a second file-reading pass with -60% instructions and -27% RSS (#13392).

**Whole-program analysis that scales.** The compiler's analysis of what the program does (used to remove checks and resolve calls at compile time) now works per module, with contracts between modules. It is the default (#13132) and finishes on large programs where the whole-program version did not: fastify, the TypeScript compiler, and opencode (6,877 modules, 31 GiB for the whole compile, #13355). It proves more read and call sites than before (tsc +4.8%, TypeScript compiler sources +6.9%, Effect schema +13.5%, #13297).

**WebAssembly and WASI.** `perry compile --target wasip1` produces a core module that imports only `wasi_snapshot_preview1` and runs on plain preview 1 runtimes (#13343). It supports inbound TCP, signals and stdin (#13376), perry-stdlib and the Node module crates (#13356), and optional outbound networking (`PERRY_WASM_NET=1`, #13386) and page discard (`PERRY_WASM_DISCARD=1`, #13417). `--target wasi` produces a WASI 0.2 component; the native component host runs WASI 0.2 and 0.3 and no longer depends on tokio (#13189, #13364). Installed releases ship both the `wasm32-wasip1` and `wasm32-wasip2` runtime archives (#13394). Compiling for WASI needs LLVM 22.1.5 for native WebAssembly exception handling.

**Real programs.** The scoreboard of 43 real programs matched node on every row at the 2026-10-19 check. fastify, zod, the TypeScript compiler, Effect and Next.js 16 compile and run; fastify handles request bodies through `req.pipe` (#13275), and zod 4.4.3 no longer miscompiles `generateFastpass` (#13270). Programs no longer abort at startup on Linux kernels built without transparent huge pages (#13309). `node:trace_events` namespace imports work (#13607), and the Windows, `dns`, `net` and `zlib` modules received further fixes listed under "All changes".

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
- `RegExp` runs only on perry's own engine (Perex), with no `regex`/`fancy-regex`/`regress` fallback (#10142). The `v` flag is complete (#10372).

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
- Windows and macOS have their own effect tables, and runtime changes are checked on those targets before they land.

### Compile speed and memory

- Parallel code generation takes the Next.js compile from 19 to 5 minutes (#12711).
- A front-end prefetch (#12705) moved the tsc front end from 43.9 s to 35.9 s with byte-identical objects (#12728).
- The whole-program analysis is computed faster (#12686). The per-module object cache and the completed-build cache share one compile-environment identity (#12582), and cached units are checkpointed atomically (#9997).
- Compile memory is released across phase boundaries (#12813), class analysis is bounded by graph distance (#12814), and lowering inputs and function source text are released or shared instead of cloned (#12916).
- Compiling opencode now fits in about 30 GB, down from 140 GB: one walk per IIFE body and one shared type-alias map (#12890).
- Large links go through a response file on every host, so very big programs no longer hit command-line length limits (#12913).
- Compile wall time and peak RSS measured on 2026-10-17: tsc 286 s / 9.7 GB, effect 31 s / 3.8 GB, schema-record 30 s / 3.9 GB. `perry compile --report-size` writes a per-crate and per-symbol size report (#8576).

### Node and ecosystem compatibility

- opencode compiles and runs again: compile memory (#12890), Effect tagged errors and `extends Error` across modules (#12897, #12898, #12899), class-expression instances (#12919), and the macOS link (#12913).
- Subclasses of `EventEmitter`, streams, `Map`/`Set` and `Promise` run their overrides and methods correctly, through typed and untyped receivers and through `await` (#12815, #12888, #12891).
- `zod` and `marked` can be used in the same program (#12882). `{ ...a, ...b }` where `b` came from `{ ...undefined }` no longer crashes, which `marked.parse` hit (#12843).
- `commander` programs run 14.7% faster (#12894).
- `instanceof` is correct for subclasses of classes returned from IIFEs or factory calls, including namespace-imported ones (#12906). `import G = Ns.Member` now declares `G` (#12849).
- A macOS duplicate-symbol link error is fixed, as is a regex literal combined with `worker_threads` (#12887).
- `perry compile` no longer hangs in `Collecting modules...` on pnpm workspaces with symlink cycles (#12895).
- New parity fixes: `net.SocketAddress` and `BlockList` input validation, `domain`, `async_hooks` (`AsyncLocalStorage.disable`, `AsyncResource` receivers), `events.on` close handling, `SubtleCrypto` static calls, outgoing HTTP header values, and `worker_threads` error diagnostics. See the per-PR list.

- Next.js 16 App Route serving is byte-identical to node (#12673, #12678, #12692) and is tracked by a pinned 16.3.x production fixture (#10007).
- Namespaces, including `enum` + `namespace` merging, compile with direct calls (#12775, #12802).
- `events`, `stream`, `http` (`IncomingMessage`, `ServerResponse`), `worker_threads` (no multi-second stalls, #12448; `terminate()` stops any loop, #12762), `perf_hooks`, `dgram`, WebSocket upgrade (#12460) and `node:test` mocking (#12747) received parity fixes. See the per-PR list.
- Bun: `Bun.Transpiler`/`Bun.build` subsets (#9602), `bun:sqlite` (#8525), SQL (#9634), zero-copy `bun:ffi` views (#8329).
- On the 2026-10-19 check, all 43 scoreboard programs printed the same output as node. On the 2026-10-17 measurement, rows above 2× node by instructions were: fastify, schema-record, qs, zod, commander, effect, property-prototype. tsc is 1.72×; the rest are under 1.8×.

### Platforms

- Security: the TLS root store and webpki are updated to reject the compromised eMudhra certificate chains (#12930).
- The release pipeline builds and publishes on all 8 platforms, and every build and publish job checks that it is on the exact commit being released.

- Linux musl x86_64/aarch64 release packages build again (#9584, #9298, #12811). Android UI apps link `perry-stdlib` (#11290, #12768).
- macOS UI builds produce signed `.app` bundles (#10240). Windows gets `fork()` IPC over named pipes (#8318), libuv-shaped errnos and `node:net` `err.errno` (#10385, #10403), `file://` drive letters (#12545) and `fs.fchown` parity (#12525). visionOS heap guards were fixed (#12247).
- `perry compile --target wasi` builds a WASI 0.2 component and `--target wasip1` a preview 1 module (see above). Target detection covers OpenHarmony aliases (#12400).

### Known gaps

- Promise/timer ordering differs from node in one case that is not yet root-caused.
- Typed arrays larger than 1 GiB are not supported, and allocation-exhaustion behavior differs from node.
- On the competitive suite at 2026-10-19, 218 of 242 rows pass, with no regressions against the previous release.
- `console.log` of a string containing `'` keeps single quotes (node switches to double quotes), and a `RegExp.prototype.exec` result prints without its `index`, `input` and `groups` properties.
- Binary size: the zod build is about 1.1 MB (+11%) larger text after the `Symbol.hasInstance`/`super` fix (#12784).
