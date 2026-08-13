# #7629 / #7990 — working notes

Worktree `/Users/amlug/projects/perry/wt-reactor`, branched from `origin/main`
at `55fd197d5` (v0.5.1500). `CARGO_TARGET_DIR=$HOME/cargo-targets/reactor`.
Host: macOS arm64.

---

## 1. #7629 — root cause: TWO tokio compilations in one binary

The six aborting gap tests are **one defect**, and it is a *build-graph* defect,
not a runtime one.

`perry-ext-http` / `perry-ext-net` / `perry-ext-ws` / `perry-ext-fastify` are
`crate-type = ["staticlib"]`. A staticlib physically bundles every Rust crate it
depends on, tokio included. `libperry_stdlib.a` bundles tokio too, and
perry-stdlib is the crate that owns the process's one runtime
(`common::async_bridge`). Both archives land in the final link.

tokio's runtime context —
`tokio::runtime::context::CONTEXT` — is a `thread_local!`, so its symbol is
mangled with the **compiling crate instance's** metadata hash. Two tokio
compilations therefore mean **two independent CONTEXT variables**.
perry-stdlib's runtime enters one; the wrapper reads the other, finds it empty,
and panics. Shipping profiles are `panic = "abort"`, so that is a SIGABRT
(exit 134) → the harness classifies it CRASH, not FAIL.

This is not a new discovery, it is a **documented invariant that nothing
checked**. `optimized_libs/driver.rs` states it verbatim (#507):

> If they're built in a different target-dir than perry-stdlib … the mangled
> hash on `tokio::runtime::context::CONTEXT` differs between the two
> staticlibs — both end up in the final binary as distinct TLS variables.
> perry-stdlib's runtime sets one; `Handle::current()` from inside the wrapper
> reads the other (empty) one and panics with "there is no reactor running".

### Measured, on this tree

`ar t <archive> | grep -o 'tokio-[0-9a-f]*'` reads the tokio compilation id
straight out of the member names.

| build | `libperry_stdlib.a` | `libperry_ext_http.a` | `libperry_ext_net.a` | result |
|---|---|---|---|---|
| auto-optimize (`target/perry-auto-…`) | `tokio-692c87888a21349c` | `tokio-692c87888a21349c` | — | **PASS 3/3** |
| `PERRY_NO_AUTO_OPTIMIZE=1` | `tokio-5aeb62139069856e` | `tokio-01c4c58f10c605f6` | `tokio-59c9ffcfa9028790` | **exit 134, 3/3** |

Three different tokios in the second row, because each archive came from its own
`cargo build -p <one crate>` invocation. Cargo resolves feature unification per
invocation, so a one-crate build gets its own tokio compilation.

### Which paths violate the invariant

1. **`optimized_libs/no_auto.rs::build_missing_prebuilt_ext_lib`** — literally
   `cargo build --release -p perry-ext-http`. Reached whenever
   `PERRY_NO_AUTO_OPTIMIZE=1` and the archive is not on disk. This is the one
   that produced both witnesses here.
2. **`run_parity_tests.sh`'s node-suite net step** — a second
   `cargo build --release -p perry-ext-net -j1` *after* the main build.
3. **The driver's own fallback** when the #507 rebuild produced no archive; it
   already prints "CONTEXT panic risk on tokio I/O" and proceeds anyway.
4. **Any hand-run `cargo build -p perry-ext-http`** before a
   `PERRY_SKIP_BUILD=1` gap run — which is what the reporting agents did, since
   `PERRY_SKIP_BUILD=1` exports `PERRY_NO_AUTO_OPTIMIZE=1` and then builds
   nothing.

### Does `listener.rs:304` need a separate fix?

**No.** Same cause, same fix. The one-frame difference is only *where the
wrapper first touched the reactor*: perry-ext-http calls `tokio::spawn`
directly at `server.rs:911`, while perry-ext-net calls `TcpListener::bind`,
whose `PollEvented::new` reaches `Handle::current()` one frame deeper inside
tokio. Both were reproduced here and both are explained by the same three-way
tokio split above.

### Why CI never saw it

`conformance-smoke` (the 8 gap shards) runs on `ubuntu-latest` and builds every
archive in **one** `cargo build` invocation, so the invariant holds there by
accident. The last `test.yml` run on `main` has all 8 gap shards green while
these six abort on a macOS dev box. That is why "the gap suite is red on main"
and "CI is green" were both true.

---

## 2. The fix

Three parts, in order of how much they matter.

**(a) A link-time check that can fail.**
`crates/perry/src/commands/compile/shared_tokio.rs` parses the `ar` container
in-process (no `llvm-ar` dependency — a gate whose tool may be absent is a gate
that silently stops gating), reads each archive's `tokio-<hash>` compilation id
out of the member names, and compares `libperry_stdlib.a` against every
tokio-using wrapper on the link line. A mismatch is a hard error naming **both
ids** and the single `cargo build` that fixes it. The check reports what it
compared (`SharedTokioReport::compared_anything`), so a run that compared
nothing is distinguishable from a run that found no mismatch.

Only wrappers where `binding_needs_shared_tokio` is true are checked — the same
predicate the #507 rebuild uses to decide what to fold into its invocation, so
the check and the fix cannot drift apart. A CPU-only wrapper (bcrypt, argon2)
never enters a tokio context, and requiring a shared compilation there would
fail links that work.

**(b) Stop manufacturing the mismatch.**
`build_missing_prebuilt_ext_lib` now refuses to build a tokio-using wrapper on
its own under `PERRY_NO_AUTO_OPTIMIZE`, and says what to run instead. It cannot
repair the situation itself: building the wrapper *with* `perry-stdlib-static`
would fix tokio but silently overwrite the prebuilt stdlib with this
invocation's feature set, dropping the `external-*-pump` features the no-auto
flow needs — trading an abort for a hang.

**(c) Make the harness's own builds coherent.**
`run_parity_tests.sh`: fold `-p perry-ext-net` into `BUILD_PACKAGES` instead of
a second invocation, and under `PERRY_SKIP_BUILD=1` verify every required ext
archive is present in `PERRY_RUNTIME_DIR` before running anything — with the
exact command, instead of leaving the operator to decode six SIGABRTs.

**(d) A second, independent defect the first one was hiding.**
With coherent tokio the no-auto gap path still failed — now at *link*, with five
undefined `_js_ext_zlib_*`. The `external-*-pump` features are a property of the
ONE prebuilt stdlib, while ext-archive selection is per-import: a stdlib built
with `external-zlib-pump` references `js_ext_zlib_process_pending`
unconditionally, so it cannot link a test that does not import `node:zlib`; a
stdlib built *without* the pumps links, but the wrapper's queues are never
drained. **No subset of pump features serves a mixed corpus.** The
auto-optimize path never hits this because it enables a pump only when it is
also routing that module.

Worth stating plainly: the harness comment claiming its ext-package build
"compensates" for no-auto was **not true** — the recipe it describes fails to
link. That branch had never been run green.

The first attempt was `PERRY_FORCE_WELL_KNOWN=events,http,net,ws,zlib`, the
in-tree mechanism for unioning modules into `well_known_iteration_set`
regardless of imports. It works, and it is 17x too slow to keep — measured on
one trivial gap test, same host, back to back:

| | compile |
|---|---|
| no-auto, no force | **2.2 s** |
| no-auto + `PERRY_FORCE_WELL_KNOWN` | **37.7 s** |

(five extra archives, 193 MB, through strip-dedup on every link). At 554 tests
that turns a ~30 min gap run into ~5 h, which defeats the entire point of
`PERRY_SKIP_BUILD=1`. Measuring this before keeping it is the reason it is not
in the final change.

What landed instead: the **23 of 554** gap tests that import an ext-routed
module (`http|https|http2|net|ws|zlib|events`, matched in both spellings, both
quote styles, and through `require(...)` — `net_connect_bound_value` reaches
`net` only via `createRequire`) drop `PERRY_NO_AUTO_OPTIMIZE` for their own
compile. The other 531 keep the 2.2 s prebuilt path. Scoped to the `all` suite:
node-suite selects one module at a time, so its prebuilt stdlib and its ext
archives already agree.

## 2b. Validation

All on `b8a230366` (the fix), archives from ONE cargo invocation.

| step | result |
|---|---|
| `libperry_{stdlib,ext_http,ext_net,ext_ws}.a` tokio ids after one invocation | all `tokio-5aeb62139069856e` |
| `test_gap_fetch_request_from_node_incoming_message`, `PERRY_NO_AUTO_OPTIMIZE=1` | `len=55 match=true`, exit 0 (was exit 134, 3/3) |
| `test_gap_net_connect_bound_value`, `PERRY_NO_AUTO_OPTIMIZE=1` | full round trip, exit 0 (was exit 134, 3/3) |
| `perry -v` on both | prints `shared-tokio: … (matches stdlib)` for http/net/ws — the check is visibly live |
| **sabotage**: `cargo build --release -p perry-ext-http` alone → id diverges to `tokio-01c4c58f10c605f6` | compile **refused**, exit 1, both ids named, no binary produced |
| restore by **rebuilding** the full invocation (not `git checkout`) | ids coherent again, compiles pass again |
| `cargo test -p perry --bin perry shared_tokio` | 11 passed |
| `cargo test -p perry-runtime --lib gc::pin::` | 6 passed |

### The full gap suite

`PERRY_SKIP_BUILD=1 ./run_parity_tests.sh --filter test_gap_`, archives built with
the harness's own recipe (`-p perry -p perry-runtime -p perry-stdlib
-p perry-runtime-static -p perry-stdlib-static`), macOS arm64, 58 minutes:

```
Parity Pass:  538      Parity Fail: 15      Compile Fail: 1      Crashed: 0
```

**`Crashed: 0`.** All six #7629 witnesses PASS, plus `test_gap_events_import_4995`.

Against the committed Linux snapshot (`test-parity/gap_snapshot.json`, 15 entries):

* 14 of the 15 reproduce.
* `test_gap_iterator_helpers_2874` passes here (host difference or fixed since).
* **2 failures are not in the snapshot, and neither is caused by this change:**
  * `test_gap_specabi_reassign` — an output mismatch
    (`plain: 99 101 2` vs `plain: 0 0 2`, `captured: 77:2` vs `captured: 0:2`).
    A spec-ABI codegen defect; nothing in this change can alter program output.
    Filed as **#8006**.
  * `test_gap_zlib_4917_level` — `zlib.deflateRawSync` / `inflateRawSync`.
    `js_zlib_deflate_raw_sync` and `js_zlib_inflate_raw_sync` exist **only** in
    `perry-stdlib/src/zlib.rs`; `perry-ext-zlib` does not define them. The
    auto-optimize flip strips `compression-gzip` from the stdlib when it routes
    `node:zlib` to the wrapper ("the ext crate carries all codecs, so nothing is
    lost", `driver.rs`) — which is false for the *raw* sync entry points, so the
    link fails with two undefined symbols. Verified both ways on this tree: the
    no-auto path links it (full stdlib supplies them, exit 0) and the
    auto-optimize path does not. That makes it red under the DEFAULT
    `scripts/run_gap_tests.sh` on `main` today, and it is not in the snapshot
    either. It is deliberately **not** routed around here: excluding `zlib` from
    the ext-routed set would hide a real API gap to make a number green.
    Filed as **#8005**.

There is no `test-parity/gap_snapshot.macos.json` in the tree, so no macOS gap
baseline has ever been recorded; the comparison above is against the Linux one.

---

## 3. #7990 — the FATAL message was wrong, and the header says why

Not the same cause as #7629 (that one is a link-graph defect; this is inside the
collector). What is shared is the shape: **an error that names a cause its own
tool refutes.**

`gc_pin_sites.py` reports OK on this tree, and both of its allowlisted
exceptions are test-only (`gc/malloc.rs`'s `push_test_object`, and the latch
sabotage test), so neither can be reached from a user program. The only
production writers of `GC_FLAG_PINNED` are `pin_object` and
`pin_object_non_young`. The message's stated cause is therefore refuted, exactly
as the issue says.

### What the reported header actually says

```
obj_type=8 size=731 flags=0x37   (MARKED|ARENA|PINNED|INTERNED|TENURED)
```

Two of those decode differently than the issue assumed:

* **`TENURED` on a "young" object is not an anomaly.** `gc/types.rs` is explicit:
  *"Non-moving generational: tenured objects stay physically in nursery (no
  copying / forwarding-pointer machinery), but the trace pretends they're
  old-gen."* So TENURED + nursery-resident is the ordinary state.

* **`INTERNED` on a `GC_TYPE_MAP` is a contradiction.** `GC_FLAG_INTERNED` is
  written in exactly one file — `string/intern.rs`, two sites — and only on
  strings. Every other reference reads it, or *preserves* it across a move
  (`copying.rs:748`, `oldgen.rs:1932`). Nothing ever sets it on a Map.

So the header is **not a coherent live Map**. It reads like memory that once
held an interned string. That points at the #7154 rooting class — the same
class the rest of the sweep produces on other seeds — reaching the collector
rather than surfacing later in JS, *not* at pin bookkeeping. It also explains
the ~1-in-16 rate: an unrooted *register* only goes bad when a collection lands
in its window, so it is intermittent; a bad *cache* would be reproducible.

Note also that the latch check in `move_young` runs **before** the existing
size/plausibility guard a few lines below it, so a garbage header is
attributed to the pin latch before anything asks whether it is a plausible
object at all.

### What changed

`gc/pin.rs` grows `header_incoherence()` and `pinned_young_move_report()`, and
`copying.rs`'s reporter calls them. The message now:

* decodes the flags byte by name (no more hand-decoding `0x37`);
* prints a **coherence verdict** computed from the header at the instant of the
  abort — the only moment that evidence exists;
* explains TENURED-on-young instead of leaving it looking suspicious;
* lists five candidates in the order the evidence separates them, with the
  pin-site scan **last** and a note saying why it led before;
* points the incoherent case at `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1
  PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`, because the default depth of 4 misses
  a window hundreds of collections wide.

Unit tests in `pin.rs` pin all of that, including a case built from #7990's
exact header bytes and a case proving the verdict can come out "consistent"
(a verdict that can only say one thing is decoration).

### What is NOT closed on #7990

The underlying fault. This change makes the abort *point at the right
investigation*; it does not find the unrooted slot. Deliberately no CI gate:
at ~6% of runs, a gate would go red on a healthy tree often enough to teach
people to ignore it — the same reasoning that declined to gate #7803's 19%.
