Windows: make the platform actually buildable and correct end-to-end, found by
running the #10385 validation work order against `turnloop/integration`.

Nothing in the turnloop migration had ever been compiled on Windows. It turned
out the platform had been unbuildable for longer than that, by three separate
pre-existing defects stacked on top of each other, each hidden by the one in
front of it. `main`'s `windows-build` job was red on a UI crate and died there,
so no Windows link was ever attempted in CI and nothing behind it was visible.

### The turnloop bug

`crates/perry-runtime/src/child_process/reactor.rs` — P2 changed
`cp_register_live_child_parts` to take `Option<CpPipe>` (the enum that carries a
child pipe's descriptor far enough for the loop to *adopt* it, instead of a
pre-boxed blocking reader). The cross-platform caller was migrated; the
`#[cfg(windows)]` one, `cp_register_windows_live_child` over
`WindowsForkChild`, was not, and no compiler on the project ever looked at it.
A pure cfg-divergence: invisible on Linux and macOS, unconditional on Windows.
Fixed with the migration's own `cp_pipe_from_file`, which also puts Windows'
fork children on the loop-adoption path P2 intended rather than the thread
fallback.

### Windows gets precise GC roots (#7354)

Every module containing a `try` refused to compile under `PERRY_RS4GC=1`, which
is the target-aware default — and since essentially all async code lowers to a
`try`, the whole turnloop surface was unreachable except with the collector in
its shadow-frame fallback.

The cause was a genuine conflict rather than an oversight. #7302 moved EH from
setjmp/longjmp to `invoke`/`landingpad` *because* longjmp could skip a
statepoint relocation write-back (#7174). On windows-msvc that lowering became
SEH funclets, and LLVM's `rewrite-statepoints-for-gc` does not support funclet
EH — it dies with an access violation (still reproducible on LLVM 22.1.8, not
just the 22.1.3 the refusal was measured on). So Windows could have precise
roots or `try`, never both.

Resolved by removing the second EH shape instead of the refusal. The
windows-msvc triple was never the problem: RS4GC behaves identically on Windows
and Linux triples for non-funclet IR. And LLVM emits
`.seh_handler perry_eh_personality, @unwind, @except` plus an Itanium-format
`GCC_except_table` for a COFF target as soon as the personality is not a
recognised MSVC one — our routine installed as the frame's x64 language
handler, with the same LSDA the ELF/Mach-O path already parses.

- `perry-codegen/src/stmt/try_stmt.rs` — one `invoke`/`landingpad` shape on
  every target; the `catchswitch`/`catchpad`/`__C_specific_handler` branch is
  gone. `runtime_decls` declared the SEH machinery *instead of* the personality
  the define lines now reference, and `needs_eh_funclets` would have forced
  every Windows `try` module off the in-process fast path; both removed.
- `perry-runtime/src/eh_lsda.rs` (new) — the GCC-LSDA decoder, split out of
  `eh.rs` (which is `cfg(not(windows))`) so both personalities share it.
- `perry-runtime/src/eh_windows.rs` — the x64 language handler. Declines phase
  2 and any code that is not `PERRY_SEH_CODE`, so access violations and foreign
  SEH keep unwinding past JS handlers (the contract `perry_seh_filter`
  carried); resolves `func_start` from `ImageBase + FunctionEntry->BeginAddress`
  and biases `ControlPc` by one the way the Itanium path does; transfers with
  `RtlUnwindEx`. No register setup — the pad reads the thrown value back from
  the rooted TLS slot via `js_get_exception`.

Verified live, not merely compiling: `PERRY_EH_TRACE=1` shows the personality
decoding a real landing pad on each catch, output is byte-identical to Node
26.5.1 across nested rethrow and `finally`, and the binary carries a populated
`.pgcmap` (1216 bytes) under `PERRY_RS4GC=1` that is **absent** with it off —
so the statepoints are real, not a silently-skipped pass.

### `class X extends LRUCache` could not link on Windows at all (#10293)

`perry-runtime::lru_subclass` was compiled unconditionally and binds the seven
`js_lru_cache_*` symbols through a plain `extern "C"` block, while its provider
(`perry-stdlib::lru_cache`) is `#[cfg(feature = "bundled-lru-cache")]`. Every
feature-subset build therefore carried unresolvable externs. That is invisible
on ELF/Mach-O, where the linker skips an archive member nothing references, and
fatal under MSVC `link.exe`, which pulls the whole object — so `perry.exe`
itself stopped linking, and so did every auto-optimized program whose feature
set omitted `bundled-lru-cache`.

Fixed with a `lru-subclass` feature on perry-runtime, off by default and
enabled only by a crate that links a provider; `bundled-lru-cache` now turns it
on. Feature selection already propagates correctly — `stdlib_features.rs` maps
`"lru-cache" → ["bundled-lru-cache"]` — so a program that imports lru-cache
gets the glue and one that does not pays nothing.

`perry.exe` additionally carries an unreachable, aborting shim for the same
symbols, because cargo feature unification defeats the gate there: the
documented build is one invocation over the coherent package set, and
perry-stdlib's feature turns the module on for perry.exe's copy of
perry-runtime too. Measured — `cargo build -p perry` alone links; adding
`-p perry-stdlib-static` reintroduces all seven. The durable fix is to stop
coupling these at link time at all (register the provider through a dispatch
table at init, as `js_stdlib_init_dispatch` already does), which is left as
follow-up.

### `err.errno` was wrong on Windows everywhere it was computed

libuv's errno is the negated OS errno on Linux and macOS, and **is not** on
Windows, where libuv uses its own `-4xxx` space: `UV_EADDRINUSE` is -4091, not
-10048. Three sites assumed the identity, so every Node program testing
`err.errno === -4091` silently matched nothing:

- `turnloop_net/errors.rs` `map_error` — `errno: -os`;
- `turnloop_net/abi.rs` `js_perry_net_errno_for_code` — same, in reverse;
- `perry-ext-http/src/transport_error.rs` — a macOS-vs-else table with no
  Windows arm, so Windows silently received the **Linux** numbers, plus two
  `raw_os_error()` sites negating the raw Winsock value.

All now route through a `libuv_errno` mapping whose values are read from the
pinned oracle itself (`util.getSystemErrorMap()` on Node 26.5.1) rather than
transcribed from libuv's headers.

The gap suite structurally cannot catch this class:
`test_gap_turnloop_listen_error.ts` deliberately asserts only that `errno` is
negative, because the value is platform data — and both the right and the wrong
answer are negative. A unit test in `errors.rs` guards it instead, asserting
libuv's number on Windows *and* asserting the negated-OS identity on unix, so
the reason the bug was invisible is itself pinned.

### Windows CI could not see any of this

`perry-ui-windows-winui` failed to build (`E0425`: `widget_layout_extras.rs`
calls `widgets::reorder_child`, which the Fluent backend never defined even
though the FFI file is shared by both backends). `windows-build` died there,
before any link, which is why the two link failures above were invisible to CI
while being unconditional on a developer machine. `reorder_child` added
following the crate's delegate-when-not-fluent idiom.

### The Node compat matrix could not run on Windows, then lied when it could

`scripts/node_compat_matrix.mjs` is Windows-aware for its oracle (`node.exe`,
`System32\tar.exe`) but not for Perry:

- `PERRY_BIN` was hardcoded to `target/release/perry` with no `.exe`, so the
  harness died in its own precondition check before probing anything. Now
  suffixed per platform, and overridable via `PERRY_BIN` — which matters
  independently, because Windows CI builds `--profile perry-dev`, so even a
  corrected suffix points at a path that job never produces.
- `perryFingerprint()` compiled with `-o outBin` and then tested
  `existsSync(outBin)`, but Perry writes `outBin.exe` on Windows. Every probe
  therefore returned `null` and the matrix reported working modules as
  **claimed-but-broken**. This is the worse of the two: the first failure is
  loud, this one produces confident wrong answers. Measured before/after on
  `os`, `path`, `url` — `UNRESOLVED/UNRESOLVED` becomes `match/match`.

### Two perry-runtime tests failed on Windows for reasons unrelated to it

Both were sitting behind the winui build break and surfaced the moment it was
fixed:

- `gc::tests::heap_generation::a_free_or_move_outside_every_scope_is_caught_in_debug_builds`
  asserted the debug-assertion outcome unconditionally, but the funnel check is
  `#[cfg(debug_assertions)]` and `perry-dev` inherits `release` — so it failed
  by construction under exactly the profile CI runs on Windows. Now asserts
  both arms, so it stays meaningful in either profile rather than going vacuous.
- `gc::tests::telemetry_verifier::emergency_full_trace_is_excluded_from_ordinary_pause_stats`
  keyed on `cfg!(any(target_env = "gnu", target_os = "macos"))`, a platform list
  predating the mimalloc purge path. `cycle_malloc_trim.rs` reports `executed`
  on any target where that purge ran, and `alloc-mimalloc` is in `default`, so
  the runtime was right and the expectation stale. Now asks the existing reach
  witness (`test_mimalloc_purge_count()`), so the branch follows the mechanism.

With those, `RUST_TEST_THREADS=1 cargo test --profile perry-dev --lib
-p perry-runtime` is **3942 passed, 0 failed** on Windows.

### Validation

`test_gap_turnloop_*` on Windows against Node 26.5.1: **15/16 pass**, 0 crashes
— from 16/16 refusing to compile before this change. The remaining failure is
`test_gap_turnloop_net_sockets`, and it is a real divergence rather than a
build artefact: a failed AF_UNIX `listen()` reports `EINVAL` where Node reports
`EACCES`, and `perry-ext-net`'s `socket_events.rs` prints
`[perry-ext-net] server N error: …` and continues where Node throws the
unhandled `'error'` event. The latter is a deliberate, documented cross-platform
deviation ("less hostile to test harnesses"), not a Windows bug, and is left
alone here rather than changed for every OS from a Windows ticket.

Also confirmed on Windows: `listen()` twice on one port gives `EADDRINUSE` and
nothing hijacks the port (turnloop's IOCP backend refuses `reuse_port` outright
and binds `SO_EXCLUSIVEADDRUSE`; both Perry call sites pass `reuse_port: false`
and `cluster.rs`'s `set_reuse_port` is `#[cfg(unix)]`), a failed bind delivers
an async `'error'` rather than hanging, and `child_process` spawn/stdio/exit
codes match.

### The connection ceiling does cost real memory on Windows (§3)

Measured, idle HTTP server, `perry-dev`:

| `max_operations` | idle working set | idle private |
| --- | --- | --- |
| 32_768 (current) | 69.0 MB | 84.7 MB |
| 2_048 | 32.9 MB | 48.0 MB |
| *(no net loop at all)* | 10.3 MB | 21.6 MB |

So ~36 MB of a Windows net loop's idle footprint is the operation ceiling
alone, and it scales linearly with it. That matches the structure: the IOCP
backend's `kernel` slab is one 1096-byte `OVERLAPPED`+addr+wire block per
operation, built and zeroed up front by `Iocp::new`, and deliberately *not*
paged — mapping a completion packet's pointer back to an op index is pointer
arithmetic over one allocation. `(32_768 - 2_048) * 1096 B` is 33.7 MB, and the
per-slot `bridges` account for the rest. On epoll/kqueue the equivalent table
is paged, so the same ceiling is free there. This is a genuine platform
asymmetry, not a tuning oversight.

`max_operations` is deliberately **not** lowered on Windows. A connection costs
two handles and an armed read, so a smaller operation ceiling is a connection
ceiling wearing a different name — it would reinstate exactly the refusal
#10351 removed. The fix belongs in turnloop: the slab must be *contiguous*, not
*committed*, so reserving the address range and committing only to the
high-water mark would keep the pointer arithmetic while dropping idle cost to
the paged backends' level. Recorded in `net_config()` with the numbers.

The ceiling itself is genuinely gone on Windows, which is the other half of the
question: **10,000 of 10,000 connections open, 0 failed** against a turnloop
HTTP server, so #10351's fix holds on IOCP and the old 2,048 refusal does not
reappear. Holding them costs 152 MB working set / 198 MB private, i.e. **8,700
bytes per connection** against Linux's measured 6,228 — ~40% more per
connection, on top of the eager slab above.

| | Windows (measured) | Linux (reported in #10385) |
| --- | --- | --- |
| connections accepted | 10,000 / 10,000 | 10,000 / 10,000 |
| idle RSS | 69.0 MB WS / 84.8 MB private | ~50 MB |
| RSS holding 10,000 | 152 MB WS / 198 MB private | — |
| bytes per connection | 8,700 | 6,228 |

Harness note: Windows' default dynamic port range is 49152-65535 (16,384
ports), so 10,000 localhost connections fit but with little headroom — an
`EADDRNOTAVAIL` in a rerun is the client running out of source ports, not the
server's ceiling. Connects are issued in batches of 200 so the measurement is
the ceiling rather than the listen backlog.
