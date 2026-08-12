### Fixed

**Six gap tests aborted with "there is no reactor running" because two tokio compilations reached one binary (#7629).**

`test_gap_fetch_request_from_node_incoming_message`,
`test_gap_http_client_no_redirect_follow`, `test_gap_http_overloads_3226plus`,
`test_gap_http_req_async_iterator`,
`test_gap_http_res_socket_writable_onfinished` and
`test_gap_net_connect_bound_value` died with SIGABRT (exit 134) — classified
CRASH, not FAIL — after a Rust panic on a worker thread:

```
thread '<unnamed>' panicked at crates/perry-ext-http/src/server/server.rs:911:13:
there is no reactor running, must be called from the context of a Tokio 1.x runtime
```

Five panicked at perry-ext-http's `tokio::spawn`; `net_connect_bound_value`
panicked one frame lower, inside tokio's own `net/tcp/listener.rs` (from
perry-ext-net's `TcpListener::bind` → `PollEvented::new` → `Handle::current()`).
**Same cause, one fix** — the differing frame is only where each wrapper first
touched the reactor.

**Root cause.** `perry-ext-*` wrappers are `staticlib`s, so each bundles its own
copy of tokio; `libperry_stdlib.a` bundles one too, and perry-stdlib owns the
process's only runtime. tokio's `runtime::context::CONTEXT` is a
`thread_local!`, so its symbol carries the compiling crate instance's metadata
hash: two tokio compilations in one binary are two independent contexts.
perry-stdlib's runtime enters one, the wrapper reads the other, finds it empty,
and (under `panic = "abort"`) the process dies. `optimized_libs/driver.rs`
already documented this exact failure as #507 and prevents it on the
auto-optimize path by rebuilding every tokio-using wrapper **in the same cargo
invocation** as perry-stdlib-static — but nothing checked that the invariant
held, so every path that bypassed that rebuild produced a binary that linked
cleanly and aborted at its first socket.

Measured on `55fd197d5`, reading the compilation id straight out of the archive
member names (`ar t … | grep -o 'tokio-[0-9a-f]*'`):

| build | `libperry_stdlib.a` | `libperry_ext_http.a` | `libperry_ext_net.a` | result |
|---|---|---|---|---|
| auto-optimize | `tokio-692c8788…` | `tokio-692c8788…` | — | PASS 3/3 |
| `PERRY_NO_AUTO_OPTIMIZE=1` | `tokio-5aeb6213…` | `tokio-01c4c58f…` | `tokio-59c9ffcf…` | exit 134, 3/3 |
| one invocation, all packages | `tokio-5aeb6213…` | `tokio-5aeb6213…` | `tokio-5aeb6213…` | PASS 3/3 |

Three different tokios in the middle row, one per `cargo build -p <one crate>`.

**What changed.**

* New `crates/perry/src/commands/compile/shared_tokio.rs` — reads each
  archive's tokio compilation id and refuses a link whose wrapper archives
  disagree with the stdlib archive, naming both ids and the single `cargo
  build` that fixes it. The `ar` container is parsed in-process rather than
  through `llvm-ar`, so the check cannot silently stop gating when a tool is
  missing; only wrappers where `binding_needs_shared_tokio` holds are checked,
  which is the same predicate the #507 rebuild uses, so check and fix cannot
  drift. The report records what it compared, so "compared nothing" is
  distinguishable from "found no mismatch".
* `optimized_libs/no_auto.rs` no longer builds a tokio-using wrapper on its own
  under `PERRY_NO_AUTO_OPTIMIZE` (`cargo build -p perry-ext-http`, which is
  what manufactured the split). It cannot repair the situation either —
  building the wrapper *with* `perry-stdlib-static` would overwrite the
  prebuilt stdlib with this invocation's feature set and drop the
  `external-*-pump` features, trading an abort for a hang — so it says what to
  run instead.
* `run_parity_tests.sh`: `-p perry-ext-net` moves into `BUILD_PACKAGES`
  (it was a *second* `cargo build -p perry-ext-net -j1`, i.e. the same split);
  `PERRY_SKIP_BUILD=1` now verifies the required ext archives are present in
  `PERRY_RUNTIME_DIR` before running anything, with the exact command; and the
  no-auto gap path exports `PERRY_FORCE_WELL_KNOWN=events,http,net,ws,zlib`.

That last one is a second, independent defect the first fix uncovered: the
`external-*-pump` features are a property of the ONE prebuilt stdlib while
archive selection is per-import, so a stdlib built with `external-zlib-pump`
failed to link any test that did not import `node:zlib` (five undefined
`_js_ext_zlib_*`). The auto-optimize path never hits it because it enables a
pump only when it is also routing that module.

**Why CI stayed green.** The 8 gap shards run on `ubuntu-latest` and build
every archive in one `cargo build`, so the invariant held there by accident.
The failure was reachable only from a build configuration CI does not use —
which is how a red gap suite and a green required check coexisted for weeks.

### Changed

**The `[gc-pin-latch]` FATAL no longer asserts a cause its own tool refutes (#7990).**

#7645's abort text said "some site sets `GC_FLAG_PINNED` without going through
`gc::pin_object`" and told the reader to run `scripts/gc_pin_sites.py`. On the
tree where the abort was next observed that tool reports **OK**, and both of its
allowlisted exceptions are test-only, so the stated cause was refuted by the
stated remedy.

The message now decodes the flags byte by name, prints a **header-coherence
verdict** computed at the instant of the abort, explains that `GC_FLAG_TENURED`
on a nursery-resident object is ordinary (the non-moving generational path
tenures in place), and lists five candidates in the order the evidence
separates them — with the pin-site scan last and a note saying why it used to
lead.

The verdict is load-bearing rather than decorative. `GC_FLAG_INTERNED` is
written in exactly one file (`string/intern.rs`) and only on
`GC_TYPE_STRING`, so #7990's reported header (`obj_type=8` = Map, `flags=0x37`
including `INTERNED`) is **not a coherent Map**: it reads as memory that once
held an interned string, which points at the #7154 unrooted-slot class rather
than at pin bookkeeping, and explains the ~1-in-16 rate (an unrooted register
only goes bad when a collection lands in its window). The message now says so
and points at `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1
PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`.

Unit tests in `gc/pin.rs` cover the verdict in both directions, including a
case built from #7990's exact header bytes. The underlying fault is not fixed —
this makes the abort point at the right investigation. No CI gate was added: at
~6% of runs it would go red on a healthy tree often enough to be ignored, the
same reasoning that declined to gate #7803's 19%.
